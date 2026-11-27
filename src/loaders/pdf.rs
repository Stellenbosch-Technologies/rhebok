#[cfg(feature = "ocr")]
use crate::config::OcrMode;
use crate::config::PdfConfig;
use crate::loaders::ocr::EngineKind;
use crate::loaders::Loaded;
use crate::model::{Document, SourceType};
use anyhow::Result;
use pdf_oxide::converters::ConversionOptions;
use pdf_oxide::extractors::auto::PageKind;
use pdf_oxide::PdfDocument;
use std::path::Path;
#[cfg(feature = "ocr")]
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error(
        "`{path}` appears to be a scanned PDF with no reliable text layer; skipping ({})",
        scanned_reason(*.ocr_attempted)
    )]
    Scanned {
        path: String,
        /// Whether recognition was actually run against this file before it
        /// was given up on.
        ///
        /// The caller cannot infer it from `[pdf] ocr` alone: `OCR_DISABLED`
        /// is a PROCESS-WIDE latch, so the second scanned PDF of a run on a
        /// machine with no ONNX Runtime is never attempted at all even though
        /// the user asked for recognition. `src/index.rs` stamps an attempted
        /// failure (the anti-retry rule) and must NOT stamp this one — a
        /// stamp here makes a document that was never even looked at
        /// permanently invisible. See the `Err` arm of `consider_into`.
        ocr_attempted: bool,
    },
    #[error("`{path}` yielded no extractable text")]
    Empty { path: String },
}

fn scanned_reason(ocr_attempted: bool) -> &'static str {
    if ocr_attempted {
        "OCR ran and recovered no text"
    } else {
        "OCR has not run on it: it is off, waiting for the OCR pass, or was unavailable earlier in this run"
    }
}

/// One page of final Markdown, whatever produced it.
///
/// This exists so the assembly below never has to know whether a page came
/// from the text layer or from recognition — and so it can be tested without
/// an OCR engine or a filesystem.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedPage {
    /// 1-indexed, as stored and displayed.
    pub number: u32,
    pub markdown: String,
    /// Whether recognition contributed any of this page's text.
    pub ocr: bool,
}

/// Usable pages plus the 1-indexed pages that were dropped.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Extraction {
    pub pages: Vec<LoadedPage>,
    pub dropped: Vec<u32>,
}

/// OCR is off for the rest of this process.
///
/// This is not tidiness, it is the only thing preventing a crash. `ort`
/// returns a recoverable error the first time it cannot load ONNX Runtime and
/// then PANICS on the next attempt ("`OrtGetApiBase` must be present in ONNX
/// Runtime dylib"), which would abort an entire `rhebok index` run because one
/// scanned PDF happened to be in the vault. Measured in a standalone spike.
#[cfg(feature = "ocr")]
static OCR_DISABLED: AtomicBool = AtomicBool::new(false);

/// One recognition at a time, process-wide.
///
/// NOT a throughput decision. `OCR_DISABLED` above is a check-then-act latch —
/// read before the call, written after it fails — so N threads all read
/// `false`, all enter, and all reach `ort::init_from`. What that costs is
/// worse than the documented panic: `ort`'s own `OnceLock` initializes through
/// `call_once_force`, whose `Err` arm returns normally WITHOUT writing the
/// slot, so the `Once` is marked COMPLETED with its data still
/// `MaybeUninit::uninit()`. Every later `init_from` then returns `Ok` over
/// uninitialized memory, and the `OrtGetApiBase` panic CLAUDE.md records is a
/// symptom of that rather than a guarantee.
///
/// The cost of serializing is close to nothing: both engines recognize one
/// page at a time.
#[cfg(feature = "ocr")]
static OCR_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Announce the first recognition once per process, not once per document.
#[cfg(feature = "ocr")]
static ANNOUNCED: std::sync::Once = std::sync::Once::new();

/// Where a native library usually lives when a package manager put it there.
///
/// Probing is the NORMAL path on macOS, not a fallback: `ort` loads ONNX
/// Runtime with a bare `dlopen`, and `/opt/homebrew/lib` is not on its search
/// path. Without this, OCR would look enabled
/// and never once run.
fn probe(explicit: Option<&Path>, stem: &str) -> Option<std::path::PathBuf> {
    match explicit {
        Some(p) if p.exists() => Some(p.to_path_buf()),
        _ => find_library(
            std::env::consts::OS,
            &library_dirs(std::env::consts::OS),
            stem,
        ),
    }
}

pub fn library_dirs(os: &str) -> Vec<std::path::PathBuf> {
    let dirs: &[&str] = match os {
        "macos" => &["/opt/homebrew/lib", "/usr/local/lib", "/usr/lib"],
        _ => &[
            "/usr/local/lib",
            "/usr/lib",
            "/usr/lib/x86_64-linux-gnu",
            "/usr/lib/aarch64-linux-gnu",
            "/usr/lib64",
        ],
    };
    dirs.iter().map(std::path::PathBuf::from).collect()
}

fn shared_library_suffix(os: &str) -> &'static str {
    match os {
        "macos" => "dylib",
        _ => "so",
    }
}

pub fn find_library(
    os: &str,
    dirs: &[std::path::PathBuf],
    stem: &str,
) -> Option<std::path::PathBuf> {
    let unversioned = format!("lib{stem}.{}", shared_library_suffix(os));
    dirs.iter().find_map(|dir| {
        let plain = dir.join(&unversioned);
        if plain.exists() {
            return Some(plain);
        }
        if os == "macos" {
            return None;
        }
        highest_versioned_soname(dir, &unversioned)
    })
}

fn highest_versioned_soname(dir: &Path, unversioned: &str) -> Option<std::path::PathBuf> {
    let prefix = format!("{unversioned}.");
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let version: Vec<u64> = name
                .strip_prefix(&prefix)?
                .split('.')
                .map(|part| part.parse().ok())
                .collect::<Option<_>>()?;
            Some((version, entry.path()))
        })
        .filter(|(_, path)| path.exists())
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, path)| path)
}

/// The libraries OCR needs, as (env var, library stem, configured override).
fn libraries(cfg: &PdfConfig) -> Vec<(&'static str, &'static str, Option<&Path>)> {
    if EngineKind::for_choice(cfg.ocr_engine).needs_onnx_runtime() {
        vec![(
            "ORT_DYLIB_PATH",
            "onnxruntime",
            cfg.ort_dylib_path.as_deref(),
        )]
    } else {
        Vec::new()
    }
}

/// Where a library will be loaded from, if anywhere. An environment variable
/// always wins, exactly as `resolve_ocr_libraries` honours it.
fn locate(var: &str, explicit: Option<&Path>, stem: &str) -> Option<std::path::PathBuf> {
    if let Some(set) = std::env::var_os(var) {
        // Existence is checked even here. Reporting a variable's value back as
        // "found" would turn a typo'd path into a confident lie, which is the
        // failure this whole line of output exists to prevent.
        let set = std::path::PathBuf::from(set);
        return set.exists().then_some(set);
    }
    probe(explicit, stem)
}

/// Point the OCR loaders at their libraries. CALL THIS ONCE, FROM `main`,
/// BEFORE ANY THREAD IS SPAWNED.
///
/// This used to live in the loader, behind a `Once`, which put
/// `std::env::set_var` in a running multi-threaded program. POSIX `setenv` is
/// not thread-safe against a concurrent `getenv`, and the MCP server already
/// races it: `rhebok_update_index` runs on one `spawn_blocking` thread of a
/// `new_multi_thread` runtime (src/mcp.rs:156, :181) while `rhebok_search_knowledge` on
/// another reads `RHEBOK_DB` and builds a `reqwest` client that reads the proxy
/// variables (src/mcp.rs:90). Parallel document loading widens that to every
/// worker. The `Once` did not help: it serializes the WRITERS against each
/// other and against nothing else in the process.
///
/// `ORT_DYLIB_PATH` stays an environment variable because `ort` has no
/// path-taking API at all, and `OAR_HOME`, where oar-ocr caches its models,
/// because oar-ocr reads only that.
///
/// A variable the user already set always wins, exactly as before.
///
/// Library consumers that bypass `main` must call this themselves, while
/// single-threaded, or OCR sees only what the environment already carries.
pub fn resolve_ocr_libraries(cfg: &PdfConfig) {
    if std::env::var_os("OAR_HOME").is_none() {
        std::env::set_var("OAR_HOME", ocr_model_cache());
    }
    for (var, stem, explicit) in libraries(cfg) {
        if std::env::var_os(var).is_some() {
            continue;
        }
        if let Some(found) = probe(explicit, stem) {
            std::env::set_var(var, found);
        }
    }
}

pub fn ocr_model_cache() -> std::path::PathBuf {
    crate::config::Config::db_path().with_file_name("ocr-models")
}

/// Whether each OCR library can be found, for `rhebok status`.
///
/// Reports location only. A library that is present can still fail to load —
/// a version mismatch, say — so this must never be phrased as "OCR works".
/// Finding nothing, on the other hand, is conclusive.
pub fn ocr_library_paths(cfg: &PdfConfig) -> Vec<(&'static str, Option<std::path::PathBuf>)> {
    libraries(cfg)
        .into_iter()
        .map(|(var, stem, explicit)| (stem, locate(var, explicit, stem)))
        .collect()
}

pub fn describe_ocr(mode: &str, cfg: &PdfConfig, missing: &[&str]) -> String {
    describe_ocr_on(mode, cfg, missing, std::env::consts::OS)
}

fn describe_ocr_on(mode: &str, cfg: &PdfConfig, missing: &[&str], os: &str) -> String {
    let engine = EngineKind::for_choice_on(cfg.ocr_engine, os);
    let label = engine.label();
    match engine {
        EngineKind::Command => {
            return match cfg.ocr_command.as_deref().and_then(|argv| argv.first()) {
                Some(program) => {
                    format!("{mode} with {label} `{program}`, run on each scanned page")
                }
                None => format!(
                    "{mode} with {label}, but ocr_command is not set under [pdf]: text PDFs are \
                     indexed as usual, scanned pages are skipped"
                ),
            }
        }
        EngineKind::Http => {
            return match cfg.ocr_url.as_deref().and_then(endpoint_origin) {
                Some(origin) => format!(
                    "{mode} with {label} at {origin}; scanned pages are sent there as images"
                ),
                None => format!(
                    "{mode} with {label}, but ocr_url is not set under [pdf] to an http:// or \
                     https:// address: text PDFs are indexed as usual, scanned pages are skipped"
                ),
            }
        }
        EngineKind::Vision | EngineKind::Oar => {}
    }
    if engine == EngineKind::Vision && os != "macos" {
        return format!(
            "{mode} with {label}, which is only available on macOS: text PDFs are indexed as \
             usual, scanned pages are skipped; set ocr_engine = \"oar\" under [pdf] to read them"
        );
    }
    if !engine.needs_onnx_runtime() {
        return format!("{mode} with {label} (built into macOS)");
    }
    let engine = label;
    if missing.is_empty() {
        return format!("{mode} with {engine} (libraries found)");
    }
    format!(
        "{mode} with {engine}, optional and not installed ({} not found): text PDFs are indexed \
         as usual, scanned pages are skipped; install {} to read them (see \"Scanned PDFs\" in \
         docs/configuration.md), or set ocr = \"off\" under [pdf] to hide this line",
        missing.join(" and "),
        missing
            .iter()
            .map(|stem| library_name(stem))
            .collect::<Vec<_>>()
            .join(" and "),
    )
}

fn library_name(stem: &str) -> &str {
    match stem {
        "onnxruntime" => "ONNX Runtime",
        other => other,
    }
}

/// Turn OCR off for the rest of the run and say why, exactly once.
#[cfg(feature = "ocr")]
fn disable_ocr(path: &Path, engine: EngineKind, err: &dyn std::fmt::Display) {
    OCR_DISABLED.store(true, Ordering::Relaxed);
    eprintln!(
        "rhebok: {}: OCR unavailable, falling back to text-layer extraction for the \
         rest of this run: {err}\n{}",
        path.display(),
        library_hint(engine)
    );
}

#[cfg(feature = "ocr")]
fn library_hint(engine: EngineKind) -> &'static str {
    if matches!(engine, EngineKind::Command | EngineKind::Http) {
        "rhebok: check ocr_command or ocr_url under [pdf]; see \"Your own OCR engine\" in \
         docs/configuration.md"
    } else if engine.needs_onnx_runtime() {
        "rhebok: install ONNX Runtime, or set ORT_DYLIB_PATH (or `[pdf] ort_dylib_path` in \
         config); with `model_downloads = \"offline\"`, place the model files in `model_dir`"
    } else {
        "rhebok: Apple Vision needs macOS; set `ocr_engine = \"oar\"` under [pdf] elsewhere"
    }
}

fn endpoint_origin(url: &str) -> Option<String> {
    let url = url::Url::parse(url.trim()).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| {
        let host = url.host_str().unwrap_or_default();
        match url.port() {
            Some(port) => format!("{}://{host}:{port}", url.scheme()),
            None => format!("{}://{host}", url.scheme()),
        }
    })
}

#[cfg(feature = "ocr")]
fn ocr_token(engine: EngineKind) -> Option<String> {
    if engine != EngineKind::Http {
        return None;
    }
    crate::env_file::ocr_token(&crate::config::Config::config_path()).unwrap_or_else(|e| {
        eprintln!("rhebok: the OCR token could not be read: {e:#}");
        None
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Text,
    Ocr,
    TextWithOcr,
}

fn route_for(kind: Option<PageKind>, markdown: &str) -> Route {
    match kind {
        Some(PageKind::TextLayer | PageKind::Empty) => Route::Text,
        Some(PageKind::Scanned) => Route::Ocr,
        Some(PageKind::ImageText | PageKind::Mixed) => Route::TextWithOcr,
        Some(_) | None if markdown.trim().is_empty() => Route::Ocr,
        Some(_) | None => Route::Text,
    }
}

struct SourcePage {
    number: u32,
    markdown: String,
    route: Route,
}

fn annotation_targets(path: &Path, base_uri: &str) -> Vec<String> {
    use pdf_oxide::annotations::LinkAction;

    let Ok(doc) = open(path) else {
        return Vec::new();
    };
    let pages = doc.page_count().unwrap_or(0);
    (0..pages)
        .filter_map(|page| doc.get_annotations(page).ok())
        .flatten()
        .filter_map(|annotation| match annotation.action? {
            LinkAction::Uri(target) => super::links::resolve(&target, base_uri),
            LinkAction::GoToRemote { file, .. } => super::links::resolve(&file, base_uri),
            _ => None,
        })
        .collect()
}

fn open(path: &Path) -> Result<PdfDocument> {
    PdfDocument::open(path)
        .map_err(|e| anyhow::anyhow!("`{}` could not be read as a PDF: {e}", path.display()))
}

fn read_pages(doc: &PdfDocument) -> Result<Vec<SourcePage>> {
    let options = ConversionOptions::default();
    let count = doc.page_count()?;
    Ok((0..count)
        .map(|index| {
            let markdown = doc.to_markdown(index, &options).unwrap_or_default();
            let kind = doc.classify_page(index).ok().map(|c| c.kind);
            SourcePage {
                number: u32::try_from(index + 1).unwrap_or(u32::MAX),
                route: route_for(kind, &markdown),
                markdown,
            }
        })
        .collect())
}

fn text_layer(pages: Vec<SourcePage>) -> Extraction {
    let mut out = Extraction::default();
    for page in pages {
        if page.route == Route::Ocr || page.markdown.trim().is_empty() {
            out.dropped.push(page.number);
        } else {
            out.pages.push(LoadedPage {
                number: page.number,
                markdown: page.markdown,
                ocr: false,
            });
        }
    }
    out
}

#[cfg(feature = "ocr")]
fn render(
    doc: &PdfDocument,
    page: &SourcePage,
    dpi: f32,
) -> Option<crate::loaders::ocr::PageImage> {
    use pdf_oxide::rendering::{render_page, RenderOptions};
    let index = usize::try_from(page.number).ok()?.checked_sub(1)?;
    let dpi = dpi.round().clamp(1.0, 1200.0) as u32;
    let image = render_page(doc, index, &RenderOptions::with_dpi(dpi).as_raw()).ok()?;
    let expected = usize::try_from(image.width)
        .ok()?
        .checked_mul(usize::try_from(image.height).ok()?)?
        .checked_mul(4)?;
    (image.data.len() == expected).then_some(crate::loaders::ocr::PageImage {
        width: image.width,
        height: image.height,
        rgba: image.data,
    })
}

#[cfg(feature = "ocr")]
fn recognized(
    doc: &PdfDocument,
    pages: Vec<SourcePage>,
    force: bool,
    engine: &crate::loaders::ocr::Engine,
    dpi: f32,
) -> std::result::Result<Extraction, crate::loaders::ocr::EngineError> {
    let mut out = Extraction::default();
    for page in pages {
        let route = if force { Route::Ocr } else { page.route };
        let read = match route {
            Route::Text => None,
            Route::Ocr | Route::TextWithOcr => match render(doc, &page, dpi) {
                Some(image) => match engine.recognize(&image) {
                    Ok(lines) => Some(crate::loaders::ocr::page_text(&lines, image.width as f32)),
                    Err(e) if !e.spoils_the_run() => {
                        eprintln!("rhebok: page {} could not be recognized: {e}", page.number);
                        None
                    }
                    Err(e) => return Err(e),
                },
                None => {
                    eprintln!("rhebok: page {} could not be rendered for OCR", page.number);
                    None
                }
            },
        };
        let (markdown, ocr) = match (route, read) {
            (Route::Ocr, Some(text)) if !text.trim().is_empty() => (text, true),
            (Route::TextWithOcr, Some(text)) => {
                match crate::loaders::ocr::with_unseen_lines(&page.markdown, &text) {
                    Some(merged) => (merged, true),
                    None => (page.markdown, false),
                }
            }
            _ => (page.markdown, false),
        };
        if markdown.trim().is_empty() {
            out.dropped.push(page.number);
        } else {
            out.pages.push(LoadedPage {
                number: page.number,
                markdown,
                ocr,
            });
        }
    }
    Ok(out)
}

pub struct PdfLoader;

impl PdfLoader {
    /// Load with default settings, which means OCR in `auto` mode.
    pub fn load_file(path: &Path) -> Result<Document> {
        Self::load_file_with(path, &PdfConfig::default())
    }

    pub fn load_file_with(path: &Path, cfg: &PdfConfig) -> Result<Document> {
        Self::load_file_reporting(path, cfg).map(|loaded| loaded.doc)
    }

    /// `load_file_with`, keeping the record of what it could not read.
    ///
    /// `load_file_with` reports dropped pages on stderr and returns only the
    /// Document, which is enough for a single-pass index and not enough for a
    /// two-pass one. This is the same work, with `Extraction::dropped` carried
    /// out to the caller instead of discarded.
    pub fn load_file_reporting(path: &Path, cfg: &PdfConfig) -> Result<Loaded> {
        let (recognized, ocr_attempted) = Self::extract_with_ocr(path, cfg);
        let extracted = match recognized {
            Some(e) => e,
            None => Self::extract_native(path)?,
        };

        if extracted.pages.is_empty() {
            // Indexing an OCR-less scan embeds noise that is very hard to debug
            // later, so bail rather than store it.
            //
            // `ocr_attempted` rides out with the error because this is the one
            // failure the two-pass indexer treats specially, and it must not
            // treat "recognition tried and failed" the same as "recognition
            // never ran" — see `PdfError::Scanned`.
            anyhow::bail!(PdfError::Scanned {
                path: path.display().to_string(),
                ocr_attempted,
            });
        }

        // A mixed document must not silently drop content: the good pages
        // still index fine, so this isn't an error, but the user needs to know
        // pages are missing.
        if !extracted.dropped.is_empty() {
            let pages = extracted
                .dropped
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!(
                "rhebok: {}: skipping scanned/empty page(s) {pages} (no reliable text layer)",
                path.display()
            );
        }

        let (text, pages) = assemble(&extracted.pages);

        // Unreachable while `pages` is non-empty and every page carries
        // non-blank Markdown, which both extractors guarantee. Kept as a guard
        // so a future change to either filter cannot silently index a blank
        // document.
        if text.trim().is_empty() {
            anyhow::bail!(PdfError::Empty {
                path: path.display().to_string()
            });
        }

        // The crate exposes no document title; the filename is the honest source.
        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled")
            .to_string();

        let uri = format!("file://{}", path.canonicalize()?.display());
        let mut doc = Document::new(SourceType::Pdf, &uri, &title, &text);
        doc.meta = serde_json::json!({ "pages": pages });
        doc.link_uris = super::links::unique_in_order(
            annotation_targets(path, &uri)
                .into_iter()
                .chain(super::links::in_markdown(&text, &uri))
                .collect(),
        );
        Ok(Loaded {
            doc,
            pending_ocr: extracted.dropped,
            ocr_attempted,
        })
    }

    /// The text layer alone. Exactly the behaviour that shipped before OCR.
    fn extract_native(path: &Path) -> Result<Extraction> {
        Ok(text_layer(read_pages(&open(path)?)?))
    }

    /// Recognition, or `None` when OCR is off, latched off, or just failed —
    /// paired with whether the engine was actually CALLED.
    ///
    /// Returning `None` rather than an error is deliberate: every one of those
    /// cases must fall through to the text layer, which is what a machine
    /// without the native libraries has always done.
    ///
    /// The second half of the pair is the distinction that `None` alone
    /// destroys. `(None, false)` is "nothing looked at this file" — the mode
    /// is `Off`, or the process-wide latch already tripped on an earlier
    /// document. `(None, true)` is "recognition ran and failed". They lead to
    /// opposite decisions in the indexer, and conflating them deleted
    /// documents: see `PdfError::Scanned::ocr_attempted`.
    #[cfg(feature = "ocr")]
    fn extract_with_ocr(path: &Path, cfg: &PdfConfig) -> (Option<Extraction>, bool) {
        // Held across the latch read AND the call, so no thread can be inside
        // recognition when another flips the latch. A poisoned gate is
        // recovered rather than propagated: a panic in one document must not
        // disable OCR by making every later lock fail.
        let _gate = OCR_GATE.lock().unwrap_or_else(|e| e.into_inner());
        let mode = match ocr_mode_for(cfg, OCR_DISABLED.load(Ordering::Relaxed)) {
            Some(m) => m,
            // Not attempted: the user turned OCR off, or an earlier document
            // in this process already found the libraries unusable.
            None => return (None, false),
        };
        let force = mode == OcrMode::Force;
        let engine = EngineKind::for_choice(cfg.ocr_engine);

        let read = open(path).and_then(|doc| read_pages(&doc).map(|pages| (doc, pages)));
        let (doc, pages) = match read {
            Ok(read) => read,
            Err(e) => {
                eprintln!(
                    "rhebok: {}: OCR could not read this file: {e}",
                    path.display()
                );
                return (None, true);
            }
        };
        if !force && pages.iter().all(|page| page.route == Route::Text) {
            return (Some(text_layer(pages)), true);
        }

        // The first recognition on a cold cache spends ~20 seconds fetching a
        // model with nothing on stderr. Silence there is indistinguishable
        // from a hang.
        ANNOUNCED.call_once(|| eprintln!("{}", recognizing_notice(engine)));
        let settings = crate::loaders::ocr::EngineSettings {
            model_dir: cfg.model_dir.clone(),
            downloads: cfg.model_downloads,
            minimum_confidence: cfg.ocr_min_confidence,
            command: cfg.ocr_command.clone(),
            url: cfg.ocr_url.clone(),
            token: ocr_token(engine),
            timeout: std::time::Duration::from_secs(cfg.ocr_timeout_secs.max(1)),
        };
        let outcome = crate::loaders::ocr::engine_for(engine, &settings)
            .and_then(|ocr| recognized(&doc, pages, force, &ocr, cfg.dpi));
        match outcome {
            Ok(extraction) => (Some(extraction), true),
            Err(e) => {
                disable_ocr(path, engine, &e);
                // Attempted, and failed. THIS document really was looked at,
                // so the anti-retry stamp is honest for it; the ones that
                // follow it in this process are the ones that are not.
                (None, true)
            }
        }
    }

    #[cfg(not(feature = "ocr"))]
    fn extract_with_ocr(_path: &Path, _cfg: &PdfConfig) -> (Option<Extraction>, bool) {
        (None, false)
    }
}

#[cfg(feature = "ocr")]
fn recognizing_notice(engine: EngineKind) -> String {
    let download = if engine.needs_onnx_runtime() {
        "; the first run may download an OCR model"
    } else {
        ""
    };
    format!(
        "rhebok: recognizing scanned page(s) with {}{download}",
        engine.label()
    )
}

/// Whether to recognize at all, and in which mode.
///
/// Pure and separately testable ON PURPOSE. When this lived inline, a test of
/// the latch could not tell "OCR was skipped because it is latched off" from
/// "OCR ran and failed" — and it duly passed with the latch deleted. The
/// distinction matters because a second attempt after a failed ONNX Runtime
/// load does not return an error, it panics and takes the index run with it.
#[cfg(feature = "ocr")]
fn ocr_mode_for(cfg: &PdfConfig, latched_off: bool) -> Option<OcrMode> {
    if latched_off || cfg.ocr == OcrMode::Off {
        return None;
    }
    Some(cfg.ocr)
}

/// Concatenate pages and record where each one starts.
///
/// Pure: no filesystem, no native libraries. The page-number and offset
/// bookkeeping that citations depend on is tested through here.
fn assemble(pages: &[LoadedPage]) -> (String, Vec<serde_json::Value>) {
    let mut text = String::new();
    let mut meta = Vec::new();
    for page in pages {
        meta.push(serde_json::json!({
            "page": page.number,
            "offset": text.len(),
            "ocr": page.ocr,
        }));
        text.push_str(&page.markdown);
        text.push_str("\n\n");
    }
    (text, meta)
}

impl super::Loader for PdfLoader {
    fn load(&self, uri: &str) -> Result<Vec<Document>> {
        let path = uri.strip_prefix("file://").unwrap_or(uri);
        Ok(vec![Self::load_file(Path::new(path))?])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(number: u32, markdown: &str, ocr: bool) -> LoadedPage {
        LoadedPage {
            number,
            markdown: markdown.into(),
            ocr,
        }
    }

    #[cfg(feature = "ocr")]
    static LATCH_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(feature = "ocr")]
    fn latched_after(load: impl FnOnce()) -> bool {
        let _serial = LATCH_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        load();
        let _gate = OCR_GATE.lock().unwrap_or_else(|e| e.into_inner());
        let latched = OCR_DISABLED.load(Ordering::Relaxed);
        OCR_DISABLED.store(false, Ordering::Relaxed);
        latched
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn a_document_that_cannot_be_read_does_not_turn_ocr_off_for_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let not_a_pdf = dir.path().join("notes.pdf");
        std::fs::write(&not_a_pdf, "plain text with a .pdf name").unwrap();
        let latched = latched_after(|| {
            let loaded = PdfLoader::load_file_reporting(&not_a_pdf, &PdfConfig::default());
            assert!(loaded.is_err());
        });
        assert!(!latched, "one unreadable file must not turn OCR off");
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn an_engine_that_cannot_start_turns_ocr_off_for_the_run() {
        let broken = PdfConfig {
            ocr_min_confidence: 2.0,
            ..PdfConfig::default()
        };
        let latched = latched_after(|| {
            let _ = PdfLoader::load_file_reporting(
                Path::new("tests/fixtures/corpus/scanned-plain.pdf"),
                &broken,
            );
        });
        assert!(latched);
    }

    #[cfg(feature = "ocr")]
    fn command_engine(script: &str) -> PdfConfig {
        PdfConfig {
            ocr_engine: crate::config::OcrEngineChoice::Command,
            ocr_command: Some(vec![
                "sh".into(),
                "-c".into(),
                script.into(),
                "{image}".into(),
            ]),
            ..PdfConfig::default()
        }
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn a_command_engine_reads_a_scanned_pdf_end_to_end() {
        let cfg = command_engine("test -s \"$0\" && echo recognized by a command");
        let doc =
            PdfLoader::load_file_with(Path::new("tests/fixtures/corpus/scanned-plain.pdf"), &cfg)
                .unwrap();
        assert!(doc.text.contains("recognized by a command"), "{}", doc.text);
        assert_eq!(doc.meta["pages"][0]["ocr"], true);
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn a_command_that_fails_its_pages_leaves_ocr_on_for_the_run() {
        let cfg = command_engine("echo no luck >&2; exit 1");
        let latched = latched_after(|| {
            let loaded = PdfLoader::load_file_reporting(
                Path::new("tests/fixtures/corpus/scanned-six-pages.pdf"),
                &cfg,
            );
            assert!(matches!(
                loaded.as_ref().map_err(|e| e.downcast_ref::<PdfError>()),
                Err(Some(PdfError::Scanned {
                    ocr_attempted: true,
                    ..
                }))
            ));
        });
        assert!(
            !latched,
            "a page the command cannot read must not turn OCR off"
        );
    }

    #[test]
    fn each_page_kind_goes_to_the_reader_that_suits_it() {
        assert_eq!(route_for(Some(PageKind::TextLayer), "text"), Route::Text);
        assert_eq!(route_for(Some(PageKind::Empty), ""), Route::Text);
        assert_eq!(route_for(Some(PageKind::Scanned), "garbled"), Route::Ocr);
        assert_eq!(
            route_for(Some(PageKind::ImageText), "text"),
            Route::TextWithOcr
        );
        assert_eq!(route_for(Some(PageKind::Mixed), "text"), Route::TextWithOcr);
        assert_eq!(route_for(None, "  "), Route::Ocr);
        assert_eq!(route_for(None, "text"), Route::Text);
    }

    #[test]
    fn the_text_layer_drops_pages_that_need_ocr_and_keeps_true_numbers() {
        let source = |number, markdown: &str, route| SourcePage {
            number,
            markdown: markdown.into(),
            route,
        };
        let extraction = text_layer(vec![
            source(1, "first", Route::Text),
            source(2, "garbled", Route::Ocr),
            source(3, "figure caption", Route::TextWithOcr),
            source(4, " ", Route::Text),
        ]);
        assert_eq!(
            extraction.pages,
            vec![page(1, "first", false), page(3, "figure caption", false)]
        );
        assert_eq!(extraction.dropped, vec![2, 4]);
    }

    #[test]
    fn a_scanned_skip_says_whether_ocr_ran() {
        let tried = PdfError::Scanned {
            path: "a.pdf".into(),
            ocr_attempted: true,
        }
        .to_string();
        let skipped = PdfError::Scanned {
            path: "a.pdf".into(),
            ocr_attempted: false,
        }
        .to_string();

        assert!(!tried.contains("not enabled"), "{tried}");
        assert!(tried.contains("OCR ran"), "{tried}");
        assert!(skipped.contains("OCR has not run"), "{skipped}");
    }

    #[test]
    fn assemble_records_page_numbers_and_running_offsets() {
        let (text, meta) = assemble(&[page(1, "alpha", false), page(2, "beta", true)]);

        assert_eq!(text, "alpha\n\nbeta\n\n");
        assert_eq!(meta[0]["page"], 1);
        assert_eq!(meta[0]["offset"], 0);
        assert_eq!(meta[0]["ocr"], false);
        // "alpha" + "\n\n" = 7 bytes before the second page begins.
        assert_eq!(meta[1]["page"], 2);
        assert_eq!(meta[1]["offset"], 7);
        assert_eq!(meta[1]["ocr"], true);
    }

    #[test]
    fn assemble_preserves_true_page_numbers_across_a_gap() {
        // Page 2 was dropped. Pages 1 and 3 must keep their real numbers, or
        // every citation after the gap points at the wrong page.
        let (_, meta) = assemble(&[page(1, "first", false), page(3, "third", false)]);

        assert_eq!(meta[0]["page"], 1);
        assert_eq!(meta[1]["page"], 3);
    }

    #[test]
    #[cfg(feature = "ocr")]
    fn a_latched_failure_stops_every_later_ocr_attempt() {
        // The one that matters. `ort` errors on its first failed load of ONNX
        // Runtime and then PANICS on the next, so "never attempt again" is a
        // crash-safety property, not an optimization.
        let auto = PdfConfig::default();
        assert_eq!(auto.ocr, OcrMode::Auto, "default must be the OCR-on mode");

        assert_eq!(ocr_mode_for(&auto, false), Some(OcrMode::Auto));
        assert_eq!(
            ocr_mode_for(&auto, true),
            None,
            "latched off must win over an enabled config"
        );
    }

    #[test]
    #[cfg(feature = "ocr")]
    fn disable_ocr_sets_the_latch() {
        // Pairs with the test above: that one proves a latched run skips OCR,
        // this one proves a failure is what latches it.
        let _serial = LATCH_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        OCR_DISABLED.store(false, Ordering::Relaxed);
        disable_ocr(
            Path::new("x.pdf"),
            EngineKind::Oar,
            &"simulated load failure",
        );
        assert!(OCR_DISABLED.load(Ordering::Relaxed));
        OCR_DISABLED.store(false, Ordering::Relaxed);
    }

    #[test]
    #[cfg(feature = "ocr")]
    fn ocr_off_is_honoured_even_when_nothing_has_failed() {
        let off = PdfConfig {
            ocr: OcrMode::Off,
            ..PdfConfig::default()
        };
        assert_eq!(ocr_mode_for(&off, false), None);
    }

    #[test]
    #[cfg(feature = "ocr")]
    fn force_mode_reaches_the_engine_as_force() {
        let force = PdfConfig {
            ocr: OcrMode::Force,
            ..PdfConfig::default()
        };
        assert_eq!(ocr_mode_for(&force, false), Some(OcrMode::Force));
    }

    #[test]
    fn a_library_path_that_does_not_exist_is_reported_as_missing() {
        // Regression: `locate` used to hand back whatever the environment
        // variable said without checking it, so a typo'd path made
        // `rhebok status` report the library as found. Status output that
        // confidently lies is worse than no status output.
        let key = "ORT_DYLIB_PATH";
        let prior = std::env::var_os(key);
        std::env::set_var(key, "/definitely/not/here/libonnxruntime.dylib");

        let reported = ocr_library_paths(&PdfConfig {
            ocr_engine: crate::config::OcrEngineChoice::Oar,
            ..PdfConfig::default()
        });
        let runtime = reported
            .iter()
            .find(|(name, _)| *name == "onnxruntime")
            .expect("onnxruntime must be reported on");
        let seen = runtime.1.clone();

        match prior {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        assert_eq!(seen, None, "a non-existent path must not count as found");
    }

    #[test]
    fn linux_probes_the_multiarch_dirs_and_macos_probes_homebrew() {
        let linux = library_dirs("linux");
        for dir in [
            "/usr/lib/x86_64-linux-gnu",
            "/usr/lib/aarch64-linux-gnu",
            "/usr/local/lib",
            "/usr/lib64",
        ] {
            assert!(
                linux.contains(&std::path::PathBuf::from(dir)),
                "{dir}: {linux:?}"
            );
        }
        let mac = library_dirs("macos");
        assert!(
            mac.contains(&std::path::PathBuf::from("/opt/homebrew/lib")),
            "{mac:?}"
        );
    }

    #[test]
    fn a_linux_dir_holding_only_versioned_sonames_yields_the_highest_version() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "libonnxruntime.so.1.9.0",
            "libonnxruntime.so.1.16.3",
            "libonnxruntime.so.1.16.3.bak",
            "libonnxruntime_providers_shared.so",
        ] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        let dirs = vec![dir.path().to_path_buf()];
        assert_eq!(
            find_library("linux", &dirs, "onnxruntime"),
            Some(dir.path().join("libonnxruntime.so.1.16.3"))
        );
        assert_eq!(find_library("macos", &dirs, "onnxruntime"), None);
    }

    #[test]
    fn an_unversioned_library_wins_over_its_versioned_sonames() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["libonnxruntime.so", "libonnxruntime.so.7"] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        let dirs = vec![dir.path().to_path_buf()];
        assert_eq!(
            find_library("linux", &dirs, "onnxruntime"),
            Some(dir.path().join("libonnxruntime.so"))
        );
        std::fs::write(dir.path().join("libonnxruntime.dylib"), b"x").unwrap();
        assert_eq!(
            find_library("macos", &dirs, "onnxruntime"),
            Some(dir.path().join("libonnxruntime.dylib"))
        );
    }

    fn with_engine(ocr_engine: crate::config::OcrEngineChoice) -> PdfConfig {
        PdfConfig {
            ocr_engine,
            ..PdfConfig::default()
        }
    }

    #[test]
    fn missing_ocr_libraries_read_as_optional_not_as_a_failure() {
        use crate::config::OcrEngineChoice::Oar;
        let line = describe_ocr_on("auto", &with_engine(Oar), &["onnxruntime"], "linux");
        assert!(line.contains("optional"), "{line}");
        assert!(line.contains("onnxruntime not found"), "{line}");
        assert!(line.contains("install ONNX Runtime"), "{line}");
        assert!(line.contains("scanned pages are skipped"), "{line}");
        assert!(
            line.contains("\"Scanned PDFs\" in docs/configuration.md"),
            "{line}"
        );
        assert_eq!(
            describe_ocr_on("auto", &with_engine(Oar), &[], "linux"),
            "auto with oar-ocr (libraries found)"
        );
    }

    #[test]
    fn vision_needs_no_libraries_on_macos_and_is_unavailable_elsewhere() {
        use crate::config::OcrEngineChoice::Vision;
        assert!(ocr_library_paths(&with_engine(Vision)).is_empty());
        assert_eq!(
            describe_ocr_on("auto", &with_engine(Vision), &[], "macos"),
            "auto with Apple Vision (built into macOS)"
        );
        let linux = describe_ocr_on("force", &with_engine(Vision), &[], "linux");
        assert!(linux.contains("only available on macOS"), "{linux}");
        assert!(linux.contains("ocr_engine = \"oar\""), "{linux}");
    }

    #[test]
    fn the_status_line_names_your_command_or_endpoint_and_never_a_token() {
        use crate::config::OcrEngineChoice::{Command, Http};
        let command = PdfConfig {
            ocr_command: Some(vec!["tesseract".into(), "{image}".into(), "-".into()]),
            ..with_engine(Command)
        };
        assert_eq!(
            describe_ocr_on("auto", &command, &[], "linux"),
            "auto with your OCR command `tesseract`, run on each scanned page"
        );
        let unset = describe_ocr_on("auto", &with_engine(Command), &[], "linux");
        assert!(unset.contains("ocr_command is not set"), "{unset}");

        let endpoint = PdfConfig {
            ocr_url: Some("https://user:pw@ocr.example:8443/read?token=abc".into()),
            ..with_engine(Http)
        };
        let line = describe_ocr_on("auto", &endpoint, &[], "macos");
        assert_eq!(
            line,
            "auto with your OCR endpoint at https://ocr.example:8443; scanned pages are sent there as images"
        );
        let wrong = PdfConfig {
            ocr_url: Some("ftp://ocr.example/".into()),
            ..with_engine(Http)
        };
        assert!(describe_ocr_on("auto", &wrong, &[], "linux").contains("ocr_url is not set"));
    }

    #[test]
    fn probe_prefers_an_explicit_path_over_the_usual_prefixes() {
        let dir = tempfile::tempdir().unwrap();
        let explicit = dir.path().join("libonnxruntime.dylib");
        std::fs::write(&explicit, b"x").unwrap();

        assert_eq!(
            probe(Some(&explicit), "onnxruntime"),
            Some(explicit.clone())
        );
        // A name that exists in no prefix must not be invented.
        assert_eq!(probe(None, "not-a-real-library"), None);
    }

    #[test]
    #[cfg(feature = "ocr")]
    fn the_ocr_path_actually_takes_the_gate() {
        // This is the test that pins the WIRING, not just the primitive.
        // `extract_with_ocr` takes `OCR_GATE` at its very top, before the
        // `ocr_mode_for(...)?` early return — so a call into it must block for
        // as long as this thread holds the gate, regardless of whether OCR
        // ends up running, is latched off, or fails. `paper.pdf` is an
        // ordinary text PDF with no scanned pages, so this needs no OCR
        // engine and stays cheap; `OcrMode::Auto` just means
        // the production code path is actually entered instead of
        // short-circuited by config.
        //
        // Mutation-tested: deleting the production
        // `let _gate = OCR_GATE.lock().unwrap_or_else(|e| e.into_inner());`
        // line makes this test fail (the spawned thread finishes immediately,
        // even while this thread holds the gate).
        use std::sync::atomic::{AtomicBool, Ordering as O};
        use std::sync::mpsc;
        use std::sync::Arc;
        use std::time::Duration;

        let held = OCR_GATE.lock().unwrap_or_else(|e| e.into_inner());

        let done = Arc::new(AtomicBool::new(false));
        let done_writer = Arc::clone(&done);
        let (finished_tx, finished_rx) = mpsc::channel::<()>();

        let handle = std::thread::spawn(move || {
            let cfg = PdfConfig {
                ocr: OcrMode::Auto,
                ..PdfConfig::default()
            };
            let _ = PdfLoader::load_file_with(Path::new("tests/fixtures/corpus/paper.pdf"), &cfg);
            done_writer.store(true, O::SeqCst);
            let _ = finished_tx.send(());
        });

        // Bounded wait, never an unbounded one: if the gate is not being
        // taken, the spawned thread finishes almost instantly; if it is, it
        // cannot even start extraction until we release the lock below.
        let finished_while_gate_held = finished_rx.recv_timeout(Duration::from_millis(500)).is_ok();

        assert!(
            !finished_while_gate_held && !done.load(O::SeqCst),
            "PdfLoader::load_file_with finished while this thread held \
             OCR_GATE; extract_with_ocr must take the gate before doing \
             anything else, and the production line that does so is missing"
        );

        // Always release before joining, or a genuinely-blocked thread would
        // hang this test forever.
        drop(held);

        handle.join().expect("spawned loader thread must not panic");
        assert!(
            done.load(O::SeqCst),
            "load must complete once the gate is released"
        );
    }
}

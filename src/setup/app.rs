use super::Paths;
use anyhow::{anyhow, bail, Context, Result};
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const BUNDLE_NAME: &str = "Rhebok.app";
pub const BUNDLE_ID: &str = "com.stelltec.rhebok";
pub const APP_EXECUTABLE: &str = "Contents/MacOS/rhebok-app";

pub fn archive_name(target: &str) -> String {
    format!("rhebok-app-{target}.tar.gz")
}

pub fn checksum_name(target: &str) -> String {
    format!("rhebok-app-{target}.sha256")
}

pub fn bundle_identifier(info_plist: &str) -> Option<String> {
    const KEY: &str = "<key>CFBundleIdentifier</key>";
    let rest = info_plist[info_plist.find(KEY)? + KEY.len()..].trim_start();
    let rest = rest.strip_prefix("<string>")?;
    Some(rest[..rest.find("</string>")?].trim().to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Existing {
    Absent,
    Ours,
    Foreign(String),
    NotABundle,
}

pub fn existing(bundle: &Path) -> Existing {
    let meta = match std::fs::symlink_metadata(bundle) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Existing::Absent,
        Err(_) => return Existing::NotABundle,
    };
    if !meta.file_type().is_dir() {
        return Existing::NotABundle;
    }
    let id = std::fs::read_to_string(bundle.join("Contents/Info.plist"))
        .ok()
        .and_then(|plist| bundle_identifier(&plist));
    match id {
        Some(id) if id == BUNDLE_ID => Existing::Ours,
        Some(id) => Existing::Foreign(id),
        None => Existing::NotABundle,
    }
}

pub fn extract_bundle(archive: &Path, into: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(into).with_context(|| format!("create {}", into.display()))?;
    let f = std::fs::File::open(archive).with_context(|| format!("open {}", archive.display()))?;
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(f));
    for entry in ar.entries().context("read tar entries")? {
        let mut entry = entry.context("read tar entry")?;
        let path = entry.path().context("tar entry path")?.into_owned();
        let kind = entry.header().entry_type();
        if kind.is_pax_global_extensions() {
            continue;
        }
        let rel = bundle_relative(archive, &path)?;
        let dest = into.join(&rel);
        let mode = entry.header().mode().context("tar entry mode")? & 0o777;
        if kind.is_dir() {
            std::fs::create_dir_all(&dest).with_context(|| format!("create {}", dest.display()))?;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(mode))
                .with_context(|| format!("set permissions on {}", dest.display()))?;
        } else if kind.is_file() {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("create {}", parent.display()))?;
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            std::fs::write(&dest, bytes).with_context(|| format!("write {}", dest.display()))?;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(mode))
                .with_context(|| format!("set permissions on {}", dest.display()))?;
        } else {
            bail!(
                "{} holds `{}`, which is neither a directory nor a regular file",
                archive.display(),
                path.display()
            );
        }
    }
    let bundle = into.join(BUNDLE_NAME);
    if !bundle.join(APP_EXECUTABLE).is_file() {
        bail!(
            "{} contains no {BUNDLE_NAME}/{APP_EXECUTABLE}",
            archive.display()
        );
    }
    Ok(bundle)
}

fn bundle_relative(archive: &Path, path: &Path) -> Result<PathBuf> {
    let mut rel = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::Normal(part) => rel.push(part),
            _ => bail!(
                "{} holds `{}`, which is not a relative path inside {BUNDLE_NAME}",
                archive.display(),
                path.display()
            ),
        }
    }
    if rel.components().next() != Some(Component::Normal(BUNDLE_NAME.as_ref())) {
        bail!(
            "{} holds `{}`, which is outside {BUNDLE_NAME}",
            archive.display(),
            path.display()
        );
    }
    Ok(rel)
}

fn refuse_what_is_not_ours(bundle: &Path) -> Result<()> {
    match existing(bundle) {
        Existing::Absent | Existing::Ours => Ok(()),
        Existing::Foreign(id) => bail!(
            "{} belongs to `{id}`, not to rhebok; leaving it alone",
            bundle.display()
        ),
        Existing::NotABundle => bail!(
            "{} is not a {BUNDLE_NAME} bundle of ours; leaving it alone",
            bundle.display()
        ),
    }
}

pub fn place_bundle(staged: &Path, bundle: &Path) -> Result<()> {
    refuse_what_is_not_ours(bundle)?;
    let parent = bundle
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent folder", bundle.display()))?;
    std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    remove_leftovers(parent, Some(staged));
    if existing(bundle) == Existing::Absent {
        return std::fs::rename(staged, bundle)
            .with_context(|| format!("rename {} to {}", staged.display(), bundle.display()));
    }
    let old = parent.join(format!(".Rhebok.app.old-{}", std::process::id()));
    std::fs::rename(bundle, &old)
        .with_context(|| format!("move {} aside to {}", bundle.display(), old.display()))?;
    if let Err(e) = std::fs::rename(staged, bundle) {
        let context = format!("rename {} to {}", staged.display(), bundle.display());
        return match std::fs::rename(&old, bundle) {
            Ok(()) => Err(e).context(context),
            Err(restore) => Err(e).context(format!(
                "{context}; putting the old bundle back also failed ({restore}), so it is now at {}",
                old.display()
            )),
        };
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

fn remove_leftovers(parent: &Path, staged: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_leftover = entry
            .file_name()
            .to_str()
            .is_some_and(|n| n.starts_with(".Rhebok.app."));
        if !is_leftover || staged.is_some_and(|staged| staged.starts_with(&path)) {
            continue;
        }
        match entry.file_type() {
            Ok(t) if t.is_dir() => {
                let _ = std::fs::remove_dir_all(&path);
            }
            Ok(_) => {
                let _ = std::fs::remove_file(&path);
            }
            Err(_) => {}
        }
    }
}

const VERSION_LIMIT: Duration = Duration::from_secs(5);
const VERSION_POLL: Duration = Duration::from_millis(10);

pub fn bundle_version(bundle: &Path) -> Result<String> {
    let exe = bundle.join(APP_EXECUTABLE);
    let mut child = Command::new(&exe)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("run {} --version", exe.display()))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > VERSION_LIMIT => {
                let _ = child.kill();
                let _ = child.wait();
                bail!(
                    "{} --version did not answer within {} seconds",
                    exe.display(),
                    VERSION_LIMIT.as_secs()
                );
            }
            Ok(None) => std::thread::sleep(VERSION_POLL),
            Err(e) => return Err(e).with_context(|| format!("wait for {}", exe.display())),
        }
    }
    let out = child
        .wait_with_output()
        .with_context(|| format!("read {} --version", exe.display()))?;
    if !out.status.success() {
        bail!("{} --version exited with {}", exe.display(), out.status);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.trim()
        .strip_prefix("rhebok-app ")
        .map(|v| v.trim().to_string())
        .ok_or_else(|| anyhow!("{} --version answered `{}`", exe.display(), text.trim()))
}

pub const RELEASE_BUILD: bool = option_env!("RHEBOK_RELEASE_BUILD").is_some();

#[derive(Debug, Clone)]
pub struct Download {
    pub api: String,
    pub token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AppOpts {
    pub target: String,
    pub version: String,
    pub exe: PathBuf,
    pub download: Option<Download>,
}

#[derive(Debug, Default)]
pub struct AppReport {
    pub lines: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn install_app(paths: &Paths, opts: &AppOpts) -> AppReport {
    let mut report = AppReport::default();
    let pid = std::process::id();
    let unpack = paths.applications.join(format!(".Rhebok.app.{pid}-unpack"));
    let fetched = paths.tmp.join(format!("app-{pid}"));
    match place_from_release(paths, opts, &unpack, &fetched) {
        Ok(Outcome::Placed) => report.lines.push(format!(
            "app:      placed at {} ({})",
            paths.app_bundle().display(),
            opts.version
        )),
        Ok(Outcome::Kept) => report.lines.push(format!(
            "app:      kept {} ({})",
            paths.app_bundle().display(),
            opts.version
        )),
        Ok(Outcome::Skipped(warning)) => report.warnings.push(warning),
        Err(e) => report
            .warnings
            .push(format!("{BUNDLE_NAME} was not placed: {e:#}")),
    }
    let _ = std::fs::remove_dir_all(&unpack);
    let _ = std::fs::remove_dir_all(&fetched);
    report
}

enum Outcome {
    Placed,
    Kept,
    Skipped(String),
}

fn place_from_release(
    paths: &Paths,
    opts: &AppOpts,
    unpack: &Path,
    fetched: &Path,
) -> Result<Outcome> {
    let bundle = paths.app_bundle();
    let archive_name = archive_name(&opts.target);
    let checksum_name = checksum_name(&opts.target);
    let beside = opts
        .exe
        .parent()
        .map(|dir| (dir.join(&archive_name), dir.join(&checksum_name)));
    let (archive, checksum) = match beside {
        Some((a, c)) if a.is_file() && c.is_file() => (a, c),
        _ => match &opts.download {
            Some(_) if bundle_is_at(&bundle, &opts.version) => return Ok(Outcome::Kept),
            Some(download) => {
                refuse_what_is_not_ours(&bundle)?;
                fetch_assets(opts, download, &archive_name, &checksum_name, fetched)?
            }
            None => return keep_or_warn(&bundle, &opts.version),
        },
    };
    crate::update::download::verify_sha256(&archive, &checksum)?;
    let staged = extract_bundle(&archive, unpack)?;
    match existing(&staged) {
        Existing::Ours => {}
        Existing::Foreign(id) => {
            return Ok(Outcome::Skipped(format!(
                "{BUNDLE_NAME} was not placed: the archive's bundle has the identifier `{id}`, not `{BUNDLE_ID}`; the current bundle is left alone"
            )))
        }
        _ => bail!("the archive's bundle has no readable identifier; the current bundle is left alone"),
    }
    let staged_version = bundle_version(&staged).map_err(|e| {
        anyhow!("the archive's bundle did not report its version: {e:#}; the current bundle is left alone")
    })?;
    if staged_version != opts.version {
        bail!(
            "the archive's bundle reports version {staged_version}, not {}; the current bundle is left alone",
            opts.version
        );
    }
    place_bundle(&staged, &bundle)?;
    let found = match bundle_version(&bundle) {
        Ok(found) => found,
        Err(e) => {
            return Ok(Outcome::Skipped(format!(
                "{BUNDLE_NAME} was placed at {} but did not report its version: {e:#}",
                bundle.display()
            )))
        }
    };
    if found == opts.version {
        Ok(Outcome::Placed)
    } else {
        Ok(Outcome::Skipped(format!(
            "{BUNDLE_NAME} was placed but reports version {found}, not {}",
            opts.version
        )))
    }
}

fn bundle_is_at(bundle: &Path, version: &str) -> bool {
    existing(bundle) == Existing::Ours && bundle_version(bundle).is_ok_and(|v| v == version)
}

fn keep_or_warn(bundle: &Path, version: &str) -> Result<Outcome> {
    if bundle_is_at(bundle, version) {
        return Ok(Outcome::Kept);
    }
    Ok(Outcome::Skipped(format!(
        "{BUNDLE_NAME} was not placed: this is a development build; build and place it with make install"
    )))
}

fn fetch_assets(
    opts: &AppOpts,
    download: &Download,
    archive_name: &str,
    checksum_name: &str,
    into: &Path,
) -> Result<(PathBuf, PathBuf)> {
    use crate::update::{download as dl, release};
    let tag = format!("v{}", opts.version);
    let agent = format!("rhebok/{}", opts.version);
    let token = download.token.as_deref();
    let found = release::fetch_tag(&download.api, &tag, token, &agent)?
        .ok_or_else(|| anyhow!("the release {tag} was not found"))?;
    let archive_asset = release::asset(&found, archive_name)?;
    let checksum_asset = release::asset(&found, checksum_name)?;
    std::fs::create_dir_all(into).with_context(|| format!("create {}", into.display()))?;
    let archive = into.join(archive_name);
    let checksum = into.join(checksum_name);
    dl::download(&archive_asset.url, token, &agent, &archive)?;
    dl::download(&checksum_asset.url, token, &agent, &checksum)?;
    Ok((archive, checksum))
}

pub fn not_installed_message(macos: bool, release_build: bool) -> &'static str {
    match (macos, release_build) {
        (false, _) => "the native app is macOS only; run rhebok dashboard for the web one",
        (true, true) => "Rhebok.app is not installed; run rhebok install to place it",
        (true, false) => "Rhebok.app is not installed; build and place it with make install",
    }
}

pub fn uninstall_app(paths: &Paths) -> AppReport {
    let mut report = AppReport::default();
    let bundle = paths.app_bundle();
    remove_leftovers(&paths.applications, None);
    match existing(&bundle) {
        Existing::Absent => {}
        Existing::Ours => match std::fs::remove_dir_all(&bundle) {
            Ok(()) => {
                report.lines.push(format!("removed {}", bundle.display()));
                report.lines.push(
                    "a Rhebok window that is still open keeps running until you close it"
                        .to_string(),
                );
            }
            Err(e) => report
                .warnings
                .push(format!("could not remove {}: {e}", bundle.display())),
        },
        Existing::Foreign(id) => report.warnings.push(format!(
            "{} belongs to `{id}`, not to rhebok; left in place",
            bundle.display()
        )),
        Existing::NotABundle => report.warnings.push(format!(
            "{} is not a {BUNDLE_NAME} bundle of ours; left in place",
            bundle.display()
        )),
    }
    report
}

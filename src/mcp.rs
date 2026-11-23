use crate::config::{Config, Profile};
use crate::embed::Embedder;
use crate::pack::Pack;
use crate::store::Hit;
use anyhow::Result;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{
    handler::server::tool::ToolRouter, model::*, schemars, tool, tool_handler, tool_router,
    ServerHandler, ServiceExt,
};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    /// What to look for, in natural language: a question or a description of the note, not a single keyword. Exact identifiers (an ADR number, an error string) also match by keyword.
    pub query: String,
    /// Retrieval depth: 0 instant, 1 fast, 2 balanced, 3 thorough (the default), 4 exhaustive. Higher tiers consider more candidates, and tiers 2-4 also follow links between notes, so each step up is slower. Values above 4 are treated as 4.
    pub quality: Option<u8>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ContextArgs {
    /// The user's message, exactly as they wrote it. Do not summarise or rephrase it: the gate is calibrated against real prompts.
    pub message: String,
    /// Leave unset. Set only by the rhebok-prehook mod: answer without searching unless `[prehook] injector` is `mod`.
    pub prehook: Option<bool>,
    /// Leave unset. The Claude Code session id, sent by the rhebok-prehook mod so the command hook knows the mod is injecting there.
    pub session: Option<String>,
    /// The absolute path of the directory the user is working in. It selects the rhebok project the message belongs to. Omit when unknown.
    pub cwd: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RelatedArgs {
    /// The `source:` value of a hit from rhebok_search or rhebok_context, copied exactly (for example `file:///Users/me/notes/adr-0004.md`).
    pub uri: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RememberArgs {
    /// "lesson" (a rule the user taught you), "fact" (something durable about the user or their setup) or "episode" (what happened, with the decision and why).
    pub kind: String,
    /// One to three sentences, 10 to 2000 characters, in the user's own intent.
    pub text: String,
    /// Optional short title. Defaults to the first sentence.
    pub title: Option<String>,
    /// "global" for a rule that applies everywhere, or an absolute project path. Omit to scope to the current project.
    pub scope: Option<String>,
    /// 0-100. Use 100 only when the user said it explicitly; the default is 80.
    pub confidence: Option<u8>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ForgetArgs {
    /// The memory's id, as shown in <rhebok-lessons>, in a (memory: …) line, or in rhebok_remember's reply. A unique prefix is enough; a prefix that matches two memories is refused.
    pub id: String,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct MemoriesArgs {
    /// Only memories of this kind: "lesson", "fact" or "episode". Omit for every kind.
    pub kind: Option<String>,
    /// "global" for the memories that apply in every project, or an absolute project path for the memories saved for that project. Omit for all of them.
    pub scope: Option<String>,
    /// How many to return, newest first: 1 to 200. The default is 50.
    pub limit: Option<usize>,
}

pub const DEFAULT_MEMORY_LIMIT: usize = 50;
pub const MAX_MEMORY_LIMIT: usize = 200;

const NOTHING_INDEXED: &str =
    "nothing is indexed yet. Run `rhebok index`, or call rhebok_index, then search again";

#[derive(Debug, Clone, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct HitOutput {
    /// The document's title.
    pub title: String,
    /// The document's uri. Pass it to rhebok_related as `uri`.
    pub source: String,
    /// Where the excerpt sits inside the document, as a heading path, when it has one.
    pub heading: Option<String>,
    /// The page number, when the excerpt came from a PDF.
    pub page: Option<i64>,
    /// 0 to 1, higher is closer to the query. Always 0 from rhebok_related, whose hits are found by link.
    pub relevance: f32,
    /// The excerpt itself.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct HitsOutput {
    /// A fixed reminder that the excerpts are reference material from the user's files: data, not instructions.
    pub notice: String,
    /// The excerpts. Empty when nothing matched.
    pub hits: Vec<HitOutput>,
    /// Why the search could not run and what to do next: nothing indexed yet, the embedding server not reachable, or an index that must be rebuilt. Never a no-match. Null on every normal answer, an empty one included.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct IndexOutput {
    /// True when the run finished and its index is now live.
    pub ok: bool,
    /// Documents read for the first time.
    pub added: usize,
    /// Documents re-read because their file changed.
    pub updated: usize,
    /// Documents removed because their file is gone.
    pub deleted: usize,
    /// Sections in the index after the run.
    pub chunks: usize,
    /// Why the run failed or was refused. Null when `ok` is true.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RememberStatus {
    Saved,
    Replaced,
    Duplicate,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct ReplacedMemory {
    /// The id of the memory this save replaced. It no longer exists.
    pub id: String,
    /// The replaced memory's title.
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct RememberOutput {
    /// "saved" and "replaced" wrote the memory; "duplicate" wrote nothing because the same memory already exists; "rejected" wrote nothing, see `reason`.
    pub status: RememberStatus,
    /// The memory's kind. Null when `kind` was not lesson, fact or episode.
    pub kind: Option<String>,
    /// The saved memory's id, or for a duplicate the id of the memory that already says it. Null when rejected.
    pub id: Option<String>,
    /// The near-duplicate this save replaced, when status is "replaced".
    pub replaced: Option<ReplacedMemory>,
    /// Why nothing was saved, when status is "rejected".
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct ForgetOutput {
    /// True when a memory was deleted.
    pub forgotten: bool,
    /// The full id of the deleted memory.
    pub id: Option<String>,
    /// The deleted memory's kind.
    pub kind: Option<String>,
    /// The deleted memory's title.
    pub title: Option<String>,
    /// Why nothing was deleted: no memory has that id, the prefix matches more than one, or the memory store is disabled or busy.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct MemoryOutput {
    /// The id rhebok_forget takes.
    pub id: String,
    /// "lesson", "fact" or "episode".
    pub kind: String,
    /// The memory's title.
    pub title: String,
    /// The memory's full text.
    pub text: String,
    /// "global", or the absolute path of the project it was saved for.
    pub scope: String,
    /// Who saved it: "user", "claude" or "distill" (distilled from a finished session).
    pub origin: String,
    /// 0 to 100.
    pub confidence: u8,
    /// When it was saved, in Unix seconds.
    pub created: i64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct MemoriesOutput {
    /// The matching memories, newest first, at most `limit` of them.
    pub memories: Vec<MemoryOutput>,
    /// How many memories matched before `limit` was applied.
    pub total: usize,
    /// Why nothing could be listed: the memory store is disabled or unreadable, or an argument was invalid. Null on every normal answer, an empty one included.
    pub error: Option<String>,
}

impl HitsOutput {
    pub fn from_hits(hits: &[Hit]) -> Self {
        let safe = crate::budget::escape_reserved_tags;
        Self {
            notice: crate::budget::CONTEXT_PREAMBLE.trim_end().to_string(),
            hits: hits
                .iter()
                .map(|h| HitOutput {
                    title: safe(&h.title),
                    source: safe(&h.uri),
                    heading: (!h.heading_path.is_empty()).then(|| safe(&h.heading_path)),
                    page: h.page_no,
                    relevance: h.relevance,
                    text: safe(&h.text),
                })
                .collect(),
            error: None,
        }
    }

    pub fn refused(message: String) -> Self {
        Self {
            error: Some(message),
            ..Self::from_hits(&[])
        }
    }
}

impl IndexOutput {
    pub fn from_run(run: Result<crate::index::IndexStats>) -> Self {
        match run {
            Ok(s) => Self {
                ok: true,
                added: s.added,
                updated: s.updated,
                deleted: s.deleted,
                chunks: s.chunks,
                error: None,
            },
            Err(e) => Self::failed(format!("{e:#}")),
        }
    }

    pub fn failed(error: String) -> Self {
        Self {
            ok: false,
            added: 0,
            updated: 0,
            deleted: 0,
            chunks: 0,
            error: Some(error),
        }
    }

    pub fn describe(&self) -> String {
        match &self.error {
            None => format!(
                "indexed: {} added, {} updated, {} chunks",
                self.added, self.updated, self.chunks
            ),
            Some(e) => format!("index failed: {e}"),
        }
    }
}

impl RememberOutput {
    pub fn from_outcome(kind: crate::memory::MemoryKind, outcome: &crate::memory::Outcome) -> Self {
        use crate::memory::Outcome;
        let written = |status, id: &str| Self {
            status,
            kind: Some(kind.as_str().to_string()),
            id: Some(id.to_string()),
            replaced: None,
            reason: None,
        };
        match outcome {
            Outcome::Saved { id } => written(RememberStatus::Saved, id),
            Outcome::Replaced {
                id,
                previous,
                previous_title,
            } => Self {
                replaced: Some(ReplacedMemory {
                    id: previous.clone(),
                    title: previous_title.clone(),
                }),
                ..written(RememberStatus::Replaced, id)
            },
            Outcome::Duplicate { of } => written(RememberStatus::Duplicate, of),
            Outcome::Rejected(why) => Self::rejected(Some(kind), why.clone()),
        }
    }

    pub fn rejected(kind: Option<crate::memory::MemoryKind>, reason: String) -> Self {
        Self {
            status: RememberStatus::Rejected,
            kind: kind.map(|k| k.as_str().to_string()),
            id: None,
            replaced: None,
            reason: Some(reason),
        }
    }
}

impl ForgetOutput {
    pub fn from_result(result: Result<crate::memory::Memory>) -> Self {
        match result {
            Ok(m) => Self {
                forgotten: true,
                id: Some(m.id),
                kind: Some(m.facts.kind.as_str().to_string()),
                title: Some(m.title),
                reason: None,
            },
            Err(e) => Self::refused(format!("{e:#}")),
        }
    }

    pub fn refused(reason: String) -> Self {
        Self {
            forgotten: false,
            id: None,
            kind: None,
            title: None,
            reason: Some(reason),
        }
    }

    pub fn describe(&self) -> String {
        match (&self.id, &self.kind, &self.title, &self.reason) {
            (Some(id), Some(kind), Some(title), None) => format!("Forgot {kind} {id}: {title}"),
            (_, _, _, reason) => format!("Nothing forgotten: {}", reason.as_deref().unwrap_or("")),
        }
    }
}

impl MemoriesOutput {
    fn failed(error: String) -> Self {
        Self {
            memories: Vec::new(),
            total: 0,
            error: Some(error),
        }
    }
}

pub fn memories_at(cfg: &Config, root: &Path, args: &MemoriesArgs) -> MemoriesOutput {
    if !cfg.memory.enabled {
        return MemoriesOutput::failed(crate::memory::DISABLED.to_string());
    }
    let kind = match args
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
    {
        None => None,
        Some(k) => match crate::memory::MemoryKind::parse(k) {
            Some(kind) => Some(kind),
            None => {
                return MemoriesOutput::failed(format!(
                    "kind must be lesson, fact or episode, got `{k}`"
                ))
            }
        },
    };
    let scope = args
        .scope
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let global_only = scope == Some("global");
    let filter = crate::memory::Filter {
        kind,
        project: scope.filter(|_| !global_only).map(std::path::PathBuf::from),
    };
    let listed = match crate::memory::list_at(root, &filter) {
        Ok(listed) => listed,
        Err(e) => return MemoriesOutput::failed(format!("{e:#}")),
    };
    let matching: Vec<crate::memory::Memory> = listed
        .into_iter()
        .filter(|m| !global_only || m.facts.project.is_none())
        .collect();
    let limit = args
        .limit
        .unwrap_or(DEFAULT_MEMORY_LIMIT)
        .clamp(1, MAX_MEMORY_LIMIT);
    let safe = crate::budget::escape_reserved_tags;
    MemoriesOutput {
        total: matching.len(),
        memories: matching
            .into_iter()
            .take(limit)
            .map(|m| MemoryOutput {
                id: m.id,
                kind: m.facts.kind.as_str().to_string(),
                title: safe(&m.title),
                text: safe(&m.text),
                scope: m.facts.project.unwrap_or_else(|| "global".to_string()),
                origin: m.facts.origin.as_str().to_string(),
                confidence: m.facts.confidence,
                created: m.facts.created,
            })
            .collect(),
        error: None,
    }
}

fn reply<T: serde::Serialize>(text: String, structured: &T) -> CallToolResult {
    let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
    result.structured_content = serde_json::to_value(structured).ok();
    result
}

fn output_schema<T: schemars::JsonSchema + 'static>() -> Arc<JsonObject> {
    rmcp::handler::server::common::schema_for_output::<T>()
}

pub fn resolve_remember(
    args: RememberArgs,
    cwd: &std::path::Path,
) -> Result<crate::memory::Remember> {
    let kind = crate::memory::MemoryKind::parse(&args.kind).ok_or_else(|| {
        anyhow::anyhow!("kind must be lesson, fact or episode, got `{}`", args.kind)
    })?;
    let project = match args.scope.as_deref().map(str::trim) {
        Some("global") => None,
        Some(path) if !path.is_empty() => Some(std::path::PathBuf::from(path)),
        _ => Some(cwd.to_path_buf()),
    };
    Ok(crate::memory::Remember {
        kind,
        text: args.text,
        title: args.title,
        project,
        confidence: args.confidence.unwrap_or(80).min(100),
        origin: crate::memory::Origin::Claude,
        session: None,
        source_hash: None,
        source_stamp: None,
        created: None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ManifestStamp {
    dev: u64,
    ino: u64,
    mtime: i64,
    mtime_nsec: i64,
    len: u64,
}

impl ManifestStamp {
    fn of(dir: &Path) -> Option<Self> {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata(dir.join(crate::pack::manifest::MANIFEST_FILE)).ok()?;
        Some(Self {
            dev: m.dev(),
            ino: m.ino(),
            mtime: m.mtime(),
            mtime_nsec: m.mtime_nsec(),
            len: m.len(),
        })
    }
}

struct OpenedPack {
    stamp: Option<ManifestStamp>,
    pack: Option<Arc<Pack>>,
}

impl OpenedPack {
    fn reuse_or_open(
        slot: &mut Option<OpenedPack>,
        dir: &Path,
        open: impl FnOnce() -> Result<Option<Pack>>,
    ) -> Result<Option<Arc<Pack>>> {
        let stamp = ManifestStamp::of(dir);
        if let Some(held) = slot.as_ref().filter(|held| held.stamp == stamp) {
            return Ok(held.pack.clone());
        }
        *slot = None;
        let pack = open()?.map(Arc::new);
        *slot = Some(OpenedPack {
            stamp,
            pack: pack.clone(),
        });
        Ok(pack)
    }
}

#[derive(Default)]
struct SearchCache {
    embedder: Option<Arc<dyn Embedder>>,
    main: Option<OpenedPack>,
    memory: Option<OpenedPack>,
}

#[derive(Clone)]
pub struct RhebokTools {
    config: Config,
    cache: Arc<Mutex<SearchCache>>,
    pack_opens: Arc<AtomicUsize>,
    tool_router: ToolRouter<Self>,
}

impl RhebokTools {
    pub fn clamp_quality(q: Option<u8>) -> u8 {
        q.map(|v| v.min(4)).unwrap_or(3)
    }

    pub fn tool_description(name: &str) -> Option<String> {
        Self::tool_router()
            .list_all()
            .into_iter()
            .find(|t| t.name == name)
            .and_then(|t| t.description.map(|d| d.to_string()))
    }

    pub fn tools() -> Vec<String> {
        Self::tool_router()
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect()
    }

    pub fn search(&self, query: &str, quality: Option<u8>) -> String {
        self.search_reply(query, quality).0
    }

    pub fn search_reply(&self, query: &str, quality: Option<u8>) -> (String, HitsOutput) {
        let profile = Profile::tier(Self::clamp_quality(quality));
        // `mcp.threshold` existed in the config, was documented, and was
        // read by nothing — an explicit search returned every candidate
        // however weak. It is deliberately lower than the hook's (0.55 vs
        // 0.70): Claude asked for this list on purpose and can judge a
        // marginal result, where the hook fires unasked and must stay quiet.
        let min = self.config.surface(crate::config::Surface::Mcp).threshold;
        // The MCP surface's weights go with the MCP surface's threshold:
        // weighting happens before the gate, so `[mcp.weights]` is how a
        // strong transcript is allowed to clear 0.55 at all. Under the
        // global 0.45 it would need a 1.22 cosine, which cannot happen.
        // `profile` above is `quality`, which the caller picks per
        // call and can differ from `[mcp] quality`'s configured default.
        // `retrieve_from_opened` decides store-vs-pack against the tier
        // actually about to run, not the surface's default — see the same
        // reasoning on the dashboard's tier override in `dashboard.rs`.
        self.retriever_for(crate::config::Surface::Mcp, &profile)
            .and_then(|r| r.search_gated(query, &profile, min))
            .map(|hits| {
                if hits.is_empty() && self.nothing_indexed() {
                    return Self::cannot_search(NOTHING_INDEXED.to_string());
                }
                crate::usage::record_hits(&crate::config::Config::db_path(), &hits);
                (Self::format_hits(&hits), HitsOutput::from_hits(&hits))
            })
            .unwrap_or_else(Self::search_failure)
    }

    fn nothing_indexed(&self) -> bool {
        let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache
            .main
            .as_ref()
            .and_then(|held| held.pack.as_ref())
            .is_some_and(|pack| pack.rows() == 0)
    }

    pub fn context(&self, args: &ContextArgs) -> crate::context::Context {
        let profile = self.context_profile(args.prehook == Some(true));
        let retriever = || self.retriever_for(crate::config::Surface::Hook, &profile);
        let cwd = args.cwd.as_deref().map(std::path::Path::new);
        if args.prehook == Some(true) {
            crate::context::build_for_prehook(
                &args.message,
                cwd,
                args.session.as_deref(),
                retriever,
            )
        } else {
            crate::context::build_with(&self.config, &args.message, cwd, &profile, retriever)
        }
    }

    fn context_profile(&self, prehook: bool) -> Profile {
        let mut profile = self.config.profile_for(crate::config::Surface::Hook);
        if !prehook {
            profile.budget_ms = profile.budget_ms.max(
                self.config
                    .profile_for(crate::config::Surface::Mcp)
                    .budget_ms,
            );
        }
        profile
    }

    pub fn pack_opens(&self) -> usize {
        self.pack_opens.load(Ordering::SeqCst)
    }

    fn retriever_for(
        &self,
        surface: crate::config::Surface,
        profile: &Profile,
    ) -> Result<crate::retrieve::Retriever> {
        let db = Config::db_path();
        let dims = self.config.embed.dimensions;
        let (embedder, pack, memory) = {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            let embedder = match &cache.embedder {
                Some(held) => Arc::clone(held),
                None => {
                    let fresh: Arc<dyn Embedder> =
                        Arc::from(crate::embed::for_config(&self.config.embed)?);
                    cache.embedder = Some(Arc::clone(&fresh));
                    fresh
                }
            };
            let model_id = embedder.model_id();
            let pack = OpenedPack::reuse_or_open(&mut cache.main, &db, || {
                self.pack_opens.fetch_add(1, Ordering::SeqCst);
                crate::pack::open_pack_beside(&db, &model_id, dims)
            })?;
            let memory = OpenedPack::reuse_or_open(
                &mut cache.memory,
                &crate::memory::pack_dir(&crate::memory::default_root()),
                || crate::memory::open_pack(&self.config, &model_id),
            );
            (embedder, pack, memory)
        };
        crate::retrieve_from_opened(
            &self.config,
            surface,
            profile,
            Box::new(embedder),
            pack,
            memory,
        )
    }

    pub fn format_search_failure(e: anyhow::Error) -> String {
        Self::search_failure(e).0
    }

    pub fn search_failure(e: anyhow::Error) -> (String, HitsOutput) {
        eprintln!("rhebok: knowledge base unavailable — {e:#}");
        Self::cannot_search(Self::why_search_failed(&e))
    }

    fn why_search_failed(e: &anyhow::Error) -> String {
        if e.downcast_ref::<crate::store::NoIndex>().is_some() {
            return NOTHING_INDEXED.to_string();
        }
        if let Some(embed) = e.downcast_ref::<crate::retrieve::EmbedUnavailable>() {
            let what = if crate::embed::timed_out(&embed.0) {
                "the embedding server did not answer in time"
            } else if crate::embed::unreachable(&embed.0) {
                "the embedding server is not reachable"
            } else {
                "the embedding server could not embed the query"
            };
            return format!(
                "{what}, and the query must be embedded to search ({:#}). Try again once the \
                 embedding server answers",
                embed.0
            );
        }
        if let Some(refused) = e.downcast_ref::<crate::retrieve::PackRefused>() {
            return refused.to_string();
        }
        if let Some(missing) = e.downcast_ref::<crate::store::VectorExtensionMissing>() {
            return missing.to_string();
        }
        format!("{e:#}")
    }

    fn cannot_search(why: String) -> (String, HitsOutput) {
        let message = format!(
            "Could not search the knowledge base: {why}. This is not a no-match: it says \
             nothing about whether the user has notes on the subject."
        );
        (message.clone(), HitsOutput::refused(message))
    }

    pub fn memories(&self, args: &MemoriesArgs) -> MemoriesOutput {
        memories_at(&self.config, &crate::memory::default_root(), args)
    }

    pub fn format_hits(hits: &[Hit]) -> String {
        if hits.is_empty() {
            return "No matching notes found in the knowledge base.".to_string();
        }
        let excerpts = hits
            .iter()
            .map(|h| {
                let page = h.page_no.map(|p| format!(" (p. {p})")).unwrap_or_default();
                let heading = if h.heading_path.is_empty() {
                    String::new()
                } else {
                    format!("\nheading: {}", crate::budget::one_line(&h.heading_path))
                };
                // `relevance`, never `score`. `score` is an RRF rank value
                // topping out near 0.049, so an excellent match was reported to
                // the model as `score: 0.033` — reading as near-worthless and
                // exporting the ordinal/cardinal confusion the rest of the
                // system was built to avoid. `relevance` is a [0,1] cosine that
                // means the same thing here as it does at the gate.
                format!(
                    "## {}{}{}\nsource: {}\nrelevance: {:.3}\n\n{}",
                    crate::budget::one_line(&h.title),
                    page,
                    heading,
                    crate::budget::one_line(&h.uri),
                    h.relevance,
                    crate::budget::quote_lines(&h.text)
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        crate::budget::escape_reserved_tags(&format!(
            "{}\n{excerpts}",
            crate::budget::CONTEXT_PREAMBLE
        ))
    }
}

#[tool_router]
impl RhebokTools {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            cache: Arc::default(),
            pack_opens: Arc::default(),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        title = "Search the knowledge base",
        annotations(
            title = "Search the knowledge base",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = output_schema::<HitsOutput>(),
        description = "Search the user's personal knowledge base (notes, papers, saved \
                       articles, past agent sessions) for a query you write, by meaning and \
                       by keyword. Use it when you need something specific the user wrote, \
                       read or worked on before, instead of asking them to repeat it, and for \
                       follow-up lookups after rhebok_context. For the first lookup on a new \
                       message, use rhebok_context instead; to explore around a hit you \
                       already have, use rhebok_related; to see saved memories by id, use \
                       rhebok_memories. Reads a local index; only the query text leaves \
                       rhebok, to the configured embedding server (a local Ollama by \
                       default). Hits are ordered most relevant first, and hits below the \
                       MCP relevance gate are dropped, so an empty list means nothing \
                       matched well, not a failure. The text reply is the same hits as \
                       Markdown excerpts. When the search cannot run (nothing indexed yet, \
                       the embedding server not reachable, an index that must be rebuilt), \
                       `error` and the text reply both say why and what to do next; that is \
                       never a no-match."
    )]
    async fn rhebok_search(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        // The store and embedder both go through a blocking `reqwest` client;
        // calling that directly on a Tokio worker thread panics ("cannot drop
        // a runtime in a context where blocking is not allowed") because
        // `serve` already runs inside a multi-thread runtime. `spawn_blocking`
        // moves the work to Tokio's blocking pool, where nested blocking calls
        // are expected and safe. Confirmed by reproducing the panic against a
        // live empty index during self-review.
        let tools = self.clone();
        let (text, out) =
            tokio::task::spawn_blocking(move || tools.search_reply(&args.query, args.quality))
                .await
                .unwrap_or_else(|e| {
                    Self::search_failure(anyhow::anyhow!("the search stopped unexpectedly: {e}"))
                });
        Ok(reply(text, &out))
    }

    #[tool(
        title = "Find context for a message",
        annotations(
            title = "Find context for a message",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = output_schema::<crate::context::Context>(),
        description = "Find the excerpts and saved memories from the user's knowledge base \
                       that belong with the user's current message, gated and budgeted like \
                       the automatic prompt hook but with more time to search. Use it before \
                       answering whenever the message touches the user's own work: their \
                       projects, code, notes, decisions, past sessions, people or plans. Skip \
                       it when a <rhebok-context> block is already in the message, or when the \
                       message is about general knowledge rather than the user's own material. \
                       Prefer it over rhebok_search for the first lookup and over your own \
                       memory of the user; use rhebok_search for later, narrower queries you \
                       write yourself. Pass the user's message verbatim as `message`, plus \
                       `cwd` when you know the working directory; leave `prehook` and \
                       `session` unset. Read-only, local, and safe to repeat. A null `block` \
                       means nothing cleared the gate, which is a normal answer; `notes` says \
                       when retrieval was degraded, for example when the embedding server was \
                       unreachable."
    )]
    async fn rhebok_context(
        &self,
        Parameters(args): Parameters<ContextArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let tools = self.clone();
        let context = tokio::task::spawn_blocking(move || tools.context(&args))
            .await
            .unwrap_or_else(|e| {
                eprintln!("rhebok: rhebok_context worker panicked — {e:#}");
                crate::context::Context::default()
            });
        Ok(reply(context.to_json(), &context))
    }

    #[tool(
        title = "Find connected notes",
        annotations(
            title = "Find connected notes",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = output_schema::<HitsOutput>(),
        description = "List the notes linked to one document you already have, by \
                       structure rather than by meaning: the sections that follow its opening \
                       section, the notes it links to by [[wikilink]], and the note that \
                       replaced it, up to two links away. Use it after rhebok_search or \
                       rhebok_context returns a starting point, to follow a thread or read \
                       the surrounding sections. It does not search: to find a document in \
                       the first place, use rhebok_search. Read-only and local. Returns at \
                       most 10 hits, alternating adjacent sections and linked notes, in the \
                       same shape as rhebok_search; `relevance` is always 0 because these are \
                       found by link, not scored. An unknown uri or a document with no links \
                       returns an empty list; when the index cannot be read, `error` says why."
    )]
    async fn rhebok_related(
        &self,
        Parameters(args): Parameters<RelatedArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        // See rhebok_search: blocking store/HTTP calls must run off the async
        // worker threads.
        let config = self.config.clone();
        let (text, out) =
            tokio::task::spawn_blocking(move || match crate::related_for(&config, &args.uri) {
                Ok(hits) => (Self::format_hits(&hits), HitsOutput::from_hits(&hits)),
                Err(e) => Self::search_failure(e),
            })
            .await
            .unwrap_or_else(|e| {
                Self::search_failure(anyhow::anyhow!("the lookup stopped unexpectedly: {e}"))
            });
        Ok(reply(text, &out))
    }

    #[tool(
        title = "Re-index the knowledge base",
        annotations(
            title = "Re-index the knowledge base",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = output_schema::<IndexOutput>(),
        description = "Update the knowledge base index from the user's configured sources, \
                       picking up new, changed and deleted files. Use it when the user has just \
                       added or edited files and rhebok_search or rhebok_context does not find \
                       them yet. Do not call it routinely: rhebok already indexes at the start \
                       of every session. Takes no parameters; what is indexed comes from \
                       `sources` in the user's rhebok config. Incremental: only files whose \
                       size or modification time changed are re-read, and unchanged sections \
                       keep their stored embeddings, so a run with nothing new returns at once. \
                       New text is embedded through the configured embedding server, which can \
                       take minutes for many new files; search keeps answering from the \
                       previous index until the new one is swapped in. Removes documents whose \
                       source file is gone. Refuses, without changing anything, while another \
                       index run is in progress, and reports that in `error`."
    )]
    async fn rhebok_index(&self) -> Result<CallToolResult, ErrorData> {
        // See rhebok_search: blocking store/HTTP calls must run off the async
        // worker threads.
        let config = self.config.clone();
        let out =
            tokio::task::spawn_blocking(move || IndexOutput::from_run(crate::index_now(&config)))
                .await
                .unwrap_or_else(|e| IndexOutput::failed(format!("task panicked: {e}")));
        Ok(reply(out.describe(), &out))
    }

    #[tool(
        title = "Save a memory",
        annotations(
            title = "Save a memory",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        ),
        output_schema = output_schema::<RememberOutput>(),
        description = "Save a memory for future sessions: a lesson the user taught you (a \
                       correction or rule), a durable fact about the user or their setup, or \
                       an episode (what happened and what was decided). Use it when the user \
                       corrects you, states a preference, or asks you to remember something; \
                       do not use it for things already in their notes, which rhebok_search \
                       finds, or for details that matter only to this conversation. To change a \
                       memory, save the new wording: a near-duplicate of the same kind replaces \
                       the old one, and an exact duplicate is not saved again. To check what is \
                       already saved, use rhebok_memories; to remove a memory, use \
                       rhebok_forget. Lessons are shown to you at the start of every session in \
                       the matching project; every kind is also retrieved by relevance on later \
                       prompts. Saving sends the text to the configured embedding server and is \
                       rejected while it is unreachable; the memory itself is stored only on \
                       this machine."
    )]
    async fn rhebok_remember(
        &self,
        Parameters(args): Parameters<RememberArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let config = self.config.clone();
        let (text, out) = tokio::task::spawn_blocking(move || {
            let cwd = std::env::current_dir().unwrap_or_default();
            match resolve_remember(args, &cwd) {
                Ok(r) => {
                    let kind = r.kind;
                    match crate::memory::remember(&config, r) {
                        Ok(outcome) => (
                            outcome.describe(kind),
                            RememberOutput::from_outcome(kind, &outcome),
                        ),
                        Err(e) => {
                            eprintln!("rhebok: rhebok_remember failed — {e:#}");
                            (
                                format!("Not saved: {e}"),
                                RememberOutput::rejected(Some(kind), format!("{e:#}")),
                            )
                        }
                    }
                }
                Err(e) => (
                    format!("Not saved: {e}"),
                    RememberOutput::rejected(None, format!("{e:#}")),
                ),
            }
        })
        .await
        .unwrap_or_else(|e| {
            let why = format!("worker panicked: {e}");
            (
                format!("Not saved: {why}"),
                RememberOutput::rejected(None, why),
            )
        });
        Ok(reply(text, &out))
    }

    #[tool(
        title = "Forget a memory",
        annotations(
            title = "Forget a memory",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = output_schema::<ForgetOutput>(),
        description = "Permanently delete one saved memory (a lesson, fact or episode) by its \
                       id. Use it when the user says a memory is wrong or no longer applies. To \
                       correct a memory rather than drop it, call rhebok_remember with the new \
                       wording instead: it replaces the near-duplicate. To find an id, use \
                       rhebok_memories. This only removes rhebok memories; it never touches the \
                       user's notes or files, and it cannot remove a hit from rhebok_search. \
                       There is no undo: the memory is gone from the next prompt's retrieval \
                       and from session-start lessons, and restoring it means saving it again \
                       with rhebok_remember. Forgetting an id that is already gone deletes \
                       nothing and says so in `reason`."
    )]
    async fn rhebok_forget(
        &self,
        Parameters(args): Parameters<ForgetArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let config = self.config.clone();
        let out = tokio::task::spawn_blocking(move || {
            ForgetOutput::from_result(crate::memory::forget(&config, &args.id))
        })
        .await
        .unwrap_or_else(|e| ForgetOutput::refused(format!("worker panicked: {e}")));
        Ok(reply(out.describe(), &out))
    }

    #[tool(
        title = "List saved memories",
        annotations(
            title = "List saved memories",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = output_schema::<MemoriesOutput>(),
        description = "List the memories rhebok has saved (lessons, facts and episodes), \
                       newest first, with their ids, scope and full text. Use it when the user \
                       asks what you remember about them, before rhebok_forget when you need a \
                       memory's id, or before rhebok_remember to check whether a rule is \
                       already saved. It lists by filter, not by meaning: to find the memories \
                       relevant to a topic, use rhebok_search, which retrieves memories \
                       alongside notes. Read-only and local; it does not embed anything, so it \
                       works while the embedding server is down. `total` counts every match, so \
                       a `total` above the number returned means `limit` cut the list."
    )]
    async fn rhebok_memories(
        &self,
        Parameters(args): Parameters<MemoriesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let tools = self.clone();
        let out = tokio::task::spawn_blocking(move || tools.memories(&args))
            .await
            .unwrap_or_else(|e| MemoriesOutput::failed(format!("worker panicked: {e}")));
        let text = serde_json::to_string(&out).unwrap_or_else(|_| "{}".to_string());
        Ok(reply(text, &out))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for RhebokTools {
    fn get_info(&self) -> ServerConfig {
        // ServerConfig (= InitializeResult) is #[non_exhaustive]: a struct literal
        // will not compile. Build with `new`, then set the optional fields.
        let mut info = ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("rhebok", env!("CARGO_PKG_VERSION"))
                    .with_website_url("https://github.com/Stellenbosch-Technologies/rhebok"),
            );
        info.instructions = Some(
            "Semantic search over the user's personal knowledge base, plus durable memory: \
             rhebok_remember saves lessons, facts and episodes; rhebok_memories lists them; \
             rhebok_forget retracts one."
                .into(),
        );
        info
    }
}

pub fn serve(config: Config) -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let service = RhebokTools::new(config)
                .serve(rmcp::transport::stdio())
                .await?;
            service.waiting().await?;
            Ok::<_, anyhow::Error>(())
        })
}

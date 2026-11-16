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
    /// What to look for, in natural language.
    pub query: String,
    /// Retrieval depth 0-4. Higher is slower and more thorough. Omit for the default.
    pub quality: Option<u8>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ContextArgs {
    /// The user's message, exactly as they wrote it.
    pub message: String,
    /// Set only by the rhebok-prehook mod: answer without searching unless `[prehook] injector` is `mod`.
    pub prehook: Option<bool>,
    /// The Claude Code session id, recorded so the command hook knows the mod is injecting there.
    pub session: Option<String>,
    /// The directory the message was written in, which selects the rhebok project.
    pub cwd: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RelatedArgs {
    /// The `uri` field of a hit from a previous rhebok_search call.
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
    /// The id shown in <rhebok-lessons>, in a (memory: …) line, or by rhebok_remember.
    pub id: String,
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
                crate::usage::record_hits(&crate::config::Config::db_path(), &hits);
                Self::format_hits(&hits)
            })
            .unwrap_or_else(Self::format_search_failure)
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
        if let Some(refused) = e.downcast_ref::<crate::retrieve::PackRefused>() {
            return refused.to_string();
        }
        eprintln!("rhebok: rhebok_search retrieval unavailable — {e:#}");
        Self::format_hits(&[])
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
        description = "Search the user's personal knowledge base (notes, papers, saved \
                       articles, past Claude Code sessions) by meaning. Use this when the \
                       user refers to something they have written down, read, or worked on \
                       before, rather than asking them to repeat it."
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
        let text = tokio::task::spawn_blocking(move || tools.search(&args.query, args.quality))
            .await
            .unwrap_or_else(|e| {
                eprintln!("rhebok: rhebok_search worker panicked — {e:#}");
                Self::format_hits(&[])
            });
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
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
        description = "Call this FIRST, before answering, whenever the user's message touches \
                       their own work: their projects, code, notes, decisions, past sessions, \
                       people or plans. Pass the user's message verbatim as `message`. It returns \
                       the excerpts from the user's knowledge base that rhebok would add to the \
                       message, gated and budgeted like the automatic prompt hook but with more \
                       time to search. The answer is JSON: `block` is the context to use (null \
                       when nothing cleared the gate), `hits` lists the sources, and `notes` says \
                       when retrieval was degraded. Skip it only when a <rhebok-context> block is \
                       already in the message. Prefer it over rhebok_search for the first lookup, \
                       and over your own memory of the user."
    )]
    async fn rhebok_context(
        &self,
        Parameters(args): Parameters<ContextArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let tools = self.clone();
        let text = tokio::task::spawn_blocking(move || tools.context(&args).to_json())
            .await
            .unwrap_or_else(|e| {
                eprintln!("rhebok: rhebok_context worker panicked — {e:#}");
                crate::context::Context::default().to_json()
            });
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
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
        description = "Given the uri of a note already found, return notes connected to it \
                       in the knowledge graph — wikilinks and adjacent sections. Use this to \
                       follow a thread after rhebok_search finds a starting point."
    )]
    async fn rhebok_related(
        &self,
        Parameters(args): Parameters<RelatedArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        // See rhebok_search: blocking store/HTTP calls must run off the async
        // worker threads.
        let config = self.config.clone();
        let hits = tokio::task::spawn_blocking(move || {
            crate::related_for(&config, &args.uri).unwrap_or_default()
        })
        .await
        .unwrap_or_default();
        Ok(CallToolResult::success(vec![ContentBlock::text(
            Self::format_hits(&hits),
        )]))
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
        description = "Re-index the knowledge base, picking up new and changed files."
    )]
    async fn rhebok_index(&self) -> Result<CallToolResult, ErrorData> {
        // See rhebok_search: blocking store/HTTP calls must run off the async
        // worker threads.
        let config = self.config.clone();
        let msg = tokio::task::spawn_blocking(move || match crate::index_now(&config) {
            Ok(s) => format!(
                "indexed: {} added, {} updated, {} chunks",
                s.added, s.updated, s.chunks
            ),
            Err(e) => format!("index failed: {e}"),
        })
        .await
        .unwrap_or_else(|e| format!("index failed: task panicked: {e}"));
        Ok(CallToolResult::success(vec![ContentBlock::text(msg)]))
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
        description = "Save a memory for future sessions: a lesson the user taught you (a correction or rule), \
                       a durable fact about the user or their setup, or an episode (what happened and what was \
                       decided). Lessons are shown to you at the start of every session in the matching project; \
                       everything is also retrieved by relevance on later prompts. Returns the memory's id."
    )]
    async fn rhebok_remember(
        &self,
        Parameters(args): Parameters<RememberArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let config = self.config.clone();
        let msg = tokio::task::spawn_blocking(move || {
            let cwd = std::env::current_dir().unwrap_or_default();
            match resolve_remember(args, &cwd) {
                Ok(r) => {
                    let kind = r.kind;
                    match crate::memory::remember(&config, r) {
                        Ok(outcome) => outcome.describe(kind),
                        Err(e) => {
                            eprintln!("rhebok: rhebok_remember failed — {e:#}");
                            format!("Not saved: {e}")
                        }
                    }
                }
                Err(e) => format!("Not saved: {e}"),
            }
        })
        .await
        .unwrap_or_else(|e| format!("Not saved: worker panicked: {e}"));
        Ok(CallToolResult::success(vec![ContentBlock::text(msg)]))
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
        description = "Delete a memory by id, when the user says a lesson or fact no longer applies."
    )]
    async fn rhebok_forget(
        &self,
        Parameters(args): Parameters<ForgetArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let config = self.config.clone();
        let msg =
            tokio::task::spawn_blocking(move || match crate::memory::forget(&config, &args.id) {
                Ok(m) => format!("Forgot {} {}: {}", m.facts.kind.as_str(), m.id, m.title),
                Err(e) => format!("Nothing forgotten: {e}"),
            })
            .await
            .unwrap_or_else(|e| format!("Nothing forgotten: worker panicked: {e}"));
        Ok(CallToolResult::success(vec![ContentBlock::text(msg)]))
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
             rhebok_remember saves lessons, facts and episodes; rhebok_forget retracts one."
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

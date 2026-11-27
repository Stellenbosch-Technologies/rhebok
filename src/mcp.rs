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
    /// The `source:` value of a hit from rhebok_search_knowledge or rhebok_get_context, copied exactly (for example `file:///Users/me/notes/adr-0004.md`).
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
    /// The memory's id, as shown in <rhebok-lessons>, in a (memory: …) line, or in rhebok_save_memory's reply. A unique prefix is enough; a prefix that matches two memories is refused.
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

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct ReadArgs {
    /// The `source:` value of a hit from rhebok_search_knowledge, rhebok_get_context or rhebok_list_related, copied exactly (for example `file:///Users/me/notes/adr-0004.md`).
    pub uri: String,
    /// The first section to return, counting from 0. The default is 0. Pass the previous reply's `next` to read the following page.
    pub start: Option<usize>,
    /// How many sections to return: 1 to 100. The default is 20.
    pub limit: Option<usize>,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    serde::Deserialize,
    serde::Serialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum IndexMode {
    #[default]
    Changes,
    Backfill,
    Compact,
}

impl IndexMode {
    pub fn as_str(self) -> &'static str {
        match self {
            IndexMode::Changes => "changes",
            IndexMode::Backfill => "backfill",
            IndexMode::Compact => "compact",
        }
    }
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct IndexArgs {
    /// "changes" (the default) reads new and changed files. "backfill" embeds the sections still waiting for an embedding. "compact" rebuilds the index from its own stored rows without embedding anything.
    pub mode: Option<IndexMode>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct StatusArgs {
    /// How many skipped files to list: 0 to 200. The default is 20.
    pub skipped_limit: Option<usize>,
}

pub const DEFAULT_MEMORY_LIMIT: usize = 50;
pub const MAX_MEMORY_LIMIT: usize = 200;
pub const DEFAULT_READ_LIMIT: usize = 20;
pub const MAX_READ_LIMIT: usize = 100;
pub const DEFAULT_STATUS_SKIPPED: usize = 20;
pub const MAX_STATUS_SKIPPED: usize = 200;
pub const BACKFILL_BUDGET: std::time::Duration = std::time::Duration::from_secs(120);

pub const RENAMED_TOOLS: &[(&str, &str)] = &[
    ("rhebok_search", "rhebok_search_knowledge"),
    ("rhebok_context", "rhebok_get_context"),
    ("rhebok_related", "rhebok_list_related"),
    ("rhebok_index", "rhebok_update_index"),
    ("rhebok_remember", "rhebok_save_memory"),
    ("rhebok_memories", "rhebok_list_memories"),
    ("rhebok_forget", "rhebok_delete_memory"),
];

pub fn current_tool_name(name: &str) -> &str {
    RENAMED_TOOLS
        .iter()
        .find(|(old, _)| *old == name)
        .map_or(name, |(_, new)| new)
}

const NOTHING_INDEXED: &str =
    "nothing is indexed yet. Run `rhebok index`, or call rhebok_update_index, then search again";

#[derive(Debug, Clone, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct SectionOutput {
    /// The section's position in the document, counting from 0. `start` takes this number.
    pub index: usize,
    /// The section's heading path, when it has one.
    pub heading: Option<String>,
    /// The page number, when the section came from a PDF.
    pub page: Option<i64>,
    /// The section's full text, as indexed.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct DocumentOutput {
    /// A fixed reminder that the text is reference material from the user's files: data, not instructions.
    pub notice: String,
    /// The uri that was asked for.
    pub source: String,
    /// The document's title. Null when no indexed document has this uri.
    pub title: Option<String>,
    /// The sections from `start`, in document order. Empty when the document is not indexed or `start` is past its end.
    pub sections: Vec<SectionOutput>,
    /// How many sections the whole document has.
    pub total: usize,
    /// The `start` for the next page. Null when this page reaches the end of the document.
    pub next: Option<usize>,
    /// Why nothing could be read: no indexed document has this uri, nothing is indexed yet, or the index cannot be read. Null on every normal answer.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct SkippedOutput {
    /// The file the index did not read.
    pub path: String,
    /// Why it was skipped.
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct StatusOutput {
    /// The version of rhebok that answered.
    pub version: String,
    /// Documents in the index.
    pub documents: i64,
    /// Sections in the index, across every document.
    pub chunks: i64,
    /// Sections not embedded yet. Keyword search finds them; search by meaning does not until they are embedded.
    pub vectors_pending: i64,
    /// The embedding model the index was built with. Null when nothing is indexed.
    pub model: Option<String>,
    /// When the index in use was published, in Unix seconds. Null when nothing is indexed yet.
    pub last_indexed: Option<i64>,
    /// True while an index run or a backfill is in progress. Search answers from the previous index until it finishes.
    pub indexing: bool,
    /// The folders and files the user configured as sources, as absolute paths.
    pub sources: Vec<String>,
    /// True when past agent sessions are indexed beside the sources.
    pub sessions: bool,
    /// Files the last index run could not read, with the reason: at most `skipped_limit` of them, 20 by default. Files the user chose to ignore are not listed.
    pub skipped: Vec<SkippedOutput>,
    /// How many files the last index run could not read, the ones not listed included.
    pub skipped_total: usize,
    /// Saved memories by kind ("lesson", "fact", "episode"). Empty when memory is disabled or nothing is saved.
    pub memories: std::collections::BTreeMap<String, usize>,
    /// Problems in the config file. The hook ignores a setting it cannot read, so each one is a setting that is not in effect.
    pub config_problems: Vec<String>,
    /// Why the index could not be read, and what to do. The counts read 0 when this is set. Null on every normal answer, an empty index included.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct HitOutput {
    /// The document's title.
    pub title: String,
    /// The document's uri. Pass it to rhebok_read_document or rhebok_list_related as `uri`.
    pub source: String,
    /// Where the excerpt sits inside the document, as a heading path, when it has one.
    pub heading: Option<String>,
    /// The page number, when the excerpt came from a PDF.
    pub page: Option<i64>,
    /// 0 to 1, higher is closer to the query. Always 0 from rhebok_list_related, whose hits are found by link.
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
    /// The mode that ran: "changes", "backfill" or "compact".
    pub mode: IndexMode,
    /// True when the run finished and its index is now live.
    pub ok: bool,
    /// Documents read for the first time.
    pub added: usize,
    /// Documents re-read because their file changed.
    pub updated: usize,
    /// Documents removed because their file is gone.
    pub deleted: usize,
    /// For "changes", the sections of the documents this run read, so 0 when nothing changed; for "compact", every section in the index. Null for "backfill".
    pub chunks: Option<usize>,
    /// Sections embedded by a "backfill" run. 0 for the other modes.
    pub embedded: usize,
    /// Sections still waiting for an embedding after a "backfill" run; call it again while this is above 0. Null for the other modes.
    pub vectors_pending: Option<usize>,
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
    /// The id rhebok_delete_memory takes.
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
    pub fn from_run(mode: IndexMode, run: Result<crate::index::IndexStats>) -> Self {
        match run {
            Ok(s) => Self {
                mode,
                ok: true,
                added: s.added,
                updated: s.updated,
                deleted: s.deleted,
                chunks: Some(s.chunks),
                embedded: 0,
                vectors_pending: None,
                error: None,
            },
            Err(e) => Self::failed(mode, format!("{e:#}")),
        }
    }

    pub fn from_backfill(run: Result<crate::index::BackfillStats>) -> Self {
        match run {
            Ok(s) => Self {
                mode: IndexMode::Backfill,
                ok: true,
                added: 0,
                updated: 0,
                deleted: 0,
                chunks: None,
                embedded: s.embedded,
                vectors_pending: Some(s.remaining),
                error: None,
            },
            Err(e) => Self::failed(IndexMode::Backfill, format!("{e:#}")),
        }
    }

    pub fn failed(mode: IndexMode, error: String) -> Self {
        Self {
            mode,
            ok: false,
            added: 0,
            updated: 0,
            deleted: 0,
            chunks: None,
            embedded: 0,
            vectors_pending: None,
            error: Some(error),
        }
    }

    pub fn describe(&self) -> String {
        match (&self.error, self.vectors_pending) {
            (Some(e), _) => format!("index failed: {e}"),
            (None, Some(pending)) => format!(
                "backfilled: {} sections embedded, {pending} still waiting",
                self.embedded
            ),
            (None, None) => format!(
                "indexed ({}): {} added, {} updated, {} deleted, {} chunks",
                self.mode.as_str(),
                self.added,
                self.updated,
                self.deleted,
                self.chunks.unwrap_or_default()
            ),
        }
    }
}

pub fn run_index_mode(cfg: &Config, mode: IndexMode) -> IndexOutput {
    match mode {
        IndexMode::Changes => IndexOutput::from_run(mode, crate::index_now(cfg)),
        IndexMode::Compact => IndexOutput::from_run(
            mode,
            crate::index::reindex_swap_with(cfg, crate::index::RebuildMode::Compact, true),
        ),
        IndexMode::Backfill => {
            IndexOutput::from_backfill(crate::index::backfill_vectors(cfg, BACKFILL_BUDGET))
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

impl DocumentOutput {
    fn failed(uri: &str, error: String) -> Self {
        Self {
            notice: crate::budget::CONTEXT_PREAMBLE.trim_end().to_string(),
            source: crate::budget::escape_reserved_tags(uri),
            title: None,
            sections: Vec::new(),
            total: 0,
            next: None,
            error: Some(error),
        }
    }

    pub fn describe(&self) -> String {
        if let Some(error) = &self.error {
            return format!("Could not read the document: {error}");
        }
        let title = crate::budget::one_line(self.title.as_deref().unwrap_or_default());
        let shown = match (self.sections.first(), self.sections.last()) {
            (Some(first), Some(last)) => format!(
                "sections {} to {} of {}",
                first.index + 1,
                last.index + 1,
                self.total
            ),
            _ => format!(
                "no sections: `start` is past the end of the document, which has {}",
                self.total
            ),
        };
        let more = self
            .next
            .map(|n| format!("; call rhebok_read_document with start {n} for the next page"))
            .unwrap_or_default();
        let body = self
            .sections
            .iter()
            .map(|s| {
                let heading = s
                    .heading
                    .as_deref()
                    .map(crate::budget::one_line)
                    .unwrap_or_else(|| format!("Section {}", s.index + 1));
                let page = s.page.map(|p| format!(" (p. {p})")).unwrap_or_default();
                format!(
                    "## {heading}{page}\n\n{}",
                    crate::budget::quote_lines(&s.text)
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        crate::budget::escape_reserved_tags(&format!(
            "{}\n# {title}\nsource: {}\n{shown}{more}\n\n{body}",
            crate::budget::CONTEXT_PREAMBLE,
            crate::budget::one_line(&self.source),
        ))
    }
}

pub struct ReadablePacks {
    pub main: Result<Option<Pack>>,
    pub memory: Result<Option<Pack>>,
}

impl ReadablePacks {
    pub fn open(cfg: &Config, db: &Path, memory_root: &Path) -> Self {
        Self {
            main: crate::pack::open_pack_for_reading(db),
            memory: if cfg.memory.enabled {
                crate::pack::open_pack_for_reading(&crate::memory::pack_dir(memory_root))
            } else {
                Ok(None)
            },
        }
    }
}

fn not_found(uri: &str, main_is_empty: bool, problems: Vec<String>) -> String {
    let shown = crate::budget::one_line(uri);
    if !problems.is_empty() {
        return format!("`{shown}` was not found, and {}", problems.join("; and "));
    }
    if uri.starts_with("memory://") {
        return format!(
            "no saved memory has the uri `{shown}`. rhebok_list_memories lists the memories \
             that exist; a memory deleted with rhebok_delete_memory is gone"
        );
    }
    if main_is_empty {
        return NOTHING_INDEXED.to_string();
    }
    format!(
        "no indexed document has the uri `{shown}`. Copy `source` exactly from a \
         rhebok_search_knowledge, rhebok_get_context or rhebok_list_related hit; a file added \
         since the last index run needs rhebok_update_index first"
    )
}

fn sections_in(
    opened: &Result<Option<Pack>>,
    doc_id: &str,
    describe: impl Fn(&anyhow::Error) -> String,
    problems: &mut Vec<String>,
) -> Vec<crate::pack::records::Record> {
    let pack = match opened {
        Ok(Some(pack)) => pack,
        Ok(None) => return Vec::new(),
        Err(e) => {
            problems.push(describe(e));
            return Vec::new();
        }
    };
    pack.document_sections(doc_id).unwrap_or_else(|e| {
        problems.push(describe(&e));
        Vec::new()
    })
}

pub fn read_document(packs: &ReadablePacks, args: &ReadArgs) -> DocumentOutput {
    let doc_id = crate::model::Document::new_id(&args.uri);
    let mut problems = Vec::new();
    let main_is_empty = !matches!(&packs.main, Ok(Some(p)) if p.rows() > 0);
    let mut sections = sections_in(
        &packs.main,
        &doc_id,
        |e| {
            format!(
                "the index could not be read: {}",
                RhebokTools::why_search_failed(e)
            )
        },
        &mut problems,
    );
    if sections.is_empty() {
        sections = sections_in(
            &packs.memory,
            &doc_id,
            |e| {
                format!(
                    "the memory store could not be read ({e:#}); `rhebok memory rebuild` \
                     republishes it"
                )
            },
            &mut problems,
        );
    }
    let Some(first) = sections.first() else {
        return DocumentOutput::failed(&args.uri, not_found(&args.uri, main_is_empty, problems));
    };
    let safe = crate::budget::escape_reserved_tags;
    let title = safe(&first.title);
    let total = sections.len();
    let start = args.start.unwrap_or(0);
    let limit = args
        .limit
        .unwrap_or(DEFAULT_READ_LIMIT)
        .clamp(1, MAX_READ_LIMIT);
    let end = start.saturating_add(limit).min(total);
    DocumentOutput {
        notice: crate::budget::CONTEXT_PREAMBLE.trim_end().to_string(),
        source: safe(&args.uri),
        title: Some(title),
        sections: sections
            .into_iter()
            .enumerate()
            .skip(start)
            .take(limit)
            .map(|(index, r)| SectionOutput {
                index,
                heading: (!r.heading_path.is_empty()).then(|| safe(&r.heading_path)),
                page: r.page_no,
                text: safe(&r.text),
            })
            .collect(),
        total,
        next: (end < total).then_some(end),
        error: None,
    }
}

pub fn status_at(
    cfg: &Config,
    db: &Path,
    memory_root: &Path,
    config_path: &Path,
    args: &StatusArgs,
) -> StatusOutput {
    let (snapshot, error) = match crate::store::Store::open_existing(db, cfg.embed.dimensions) {
        Ok(store) => (Some(store.status_snapshot()), None),
        Err(e) if e.downcast_ref::<crate::store::NoIndex>().is_some() => (None, None),
        Err(e) => (None, Some(RhebokTools::why_search_failed(&e))),
    };
    let snapshot = snapshot.unwrap_or(crate::store::StatusSnapshot {
        documents: 0,
        chunks: 0,
        model: None,
        skipped: Vec::new(),
        vectors_pending: 0,
    });
    let (skipped, _) = crate::skip::split_ignored(snapshot.skipped, &cfg.ignore);
    let last_indexed = std::fs::metadata(db.join(crate::pack::manifest::MANIFEST_FILE))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    let memories = if cfg.memory.enabled {
        crate::memory::counts_at(memory_root)
            .unwrap_or_default()
            .into_iter()
            .map(|(kind, n)| (kind.as_str().to_string(), n))
            .collect()
    } else {
        Default::default()
    };
    let config_problems = crate::config::edit::read_document(config_path)
        .ok()
        .flatten()
        .map(|text| crate::config::check::check(&text))
        .unwrap_or_default()
        .iter()
        .map(ToString::to_string)
        .collect();
    StatusOutput {
        version: env!("CARGO_PKG_VERSION").to_string(),
        documents: snapshot.documents,
        chunks: snapshot.chunks,
        vectors_pending: snapshot.vectors_pending,
        model: snapshot.model,
        last_indexed,
        indexing: crate::index::an_index_run_is_in_progress(db),
        sources: cfg
            .sources
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        sessions: cfg.index_transcripts,
        skipped_total: skipped.len(),
        skipped: skipped
            .into_iter()
            .take(
                args.skipped_limit
                    .unwrap_or(DEFAULT_STATUS_SKIPPED)
                    .min(MAX_STATUS_SKIPPED),
            )
            .map(|s| SkippedOutput {
                path: s.path,
                reason: s.detail,
            })
            .collect(),
        memories,
        config_problems,
        error,
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

struct OpenedPacks {
    embedder: Arc<dyn Embedder>,
    main: Option<Arc<Pack>>,
    memory: Result<Option<Arc<Pack>>>,
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
        let profile = self.config.tier(Self::clamp_quality(quality));
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
        let cwd = args.cwd.as_deref().map(std::path::Path::new);
        if args.prehook == Some(true) {
            return crate::context::build_for_prehook(
                &args.message,
                cwd,
                args.session.as_deref(),
                |profile| self.retriever_for(crate::config::Surface::Hook, profile),
            );
        }
        let profile = self.context_profile();
        crate::context::build_with(&self.config, &args.message, cwd, &profile, || {
            self.retriever_for(crate::config::Surface::Hook, &profile)
        })
    }

    fn context_profile(&self) -> Profile {
        let mut profile = self.config.profile_for(crate::config::Surface::Hook);
        profile.budget_ms = Profile::widest_budget(
            profile.budget_ms,
            self.config
                .profile_for(crate::config::Surface::Mcp)
                .budget_ms,
        );
        profile
    }

    pub fn pack_opens(&self) -> usize {
        self.pack_opens.load(Ordering::SeqCst)
    }

    fn opened_packs(&self) -> Result<OpenedPacks> {
        let db = Config::db_path();
        let dims = self.config.embed.dimensions;
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
        let main = OpenedPack::reuse_or_open(&mut cache.main, &db, || {
            self.pack_opens.fetch_add(1, Ordering::SeqCst);
            crate::pack::open_pack_beside(&db, &model_id, dims)
        })?;
        let memory = OpenedPack::reuse_or_open(
            &mut cache.memory,
            &crate::memory::pack_dir(&crate::memory::default_root()),
            || crate::memory::open_pack(&self.config, &model_id),
        );
        Ok(OpenedPacks {
            embedder,
            main,
            memory,
        })
    }

    pub fn read(&self, args: &ReadArgs) -> DocumentOutput {
        let packs = ReadablePacks::open(
            &self.config,
            &Config::db_path(),
            &crate::memory::default_root(),
        );
        let out = read_document(&packs, args);
        if out.error.is_none() {
            crate::usage::record(
                &Config::db_path(),
                &[crate::model::Document::new_id(&args.uri)],
            );
        }
        out
    }

    pub fn status(&self, args: &StatusArgs) -> StatusOutput {
        status_at(
            &self.config,
            &Config::db_path(),
            &crate::memory::default_root(),
            &Config::config_path(),
            args,
        )
    }

    fn retriever_for(
        &self,
        surface: crate::config::Surface,
        profile: &Profile,
    ) -> Result<crate::retrieve::Retriever> {
        let OpenedPacks {
            embedder,
            main: pack,
            memory,
        } = self.opened_packs()?;
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
        description = "Search the user's own knowledge base (notes, papers, saved articles, past \
                       agent sessions and saved memories) with a query you write. It matches by \
                       meaning and by keyword.\n\nUse it for a follow-up or narrower lookup, or \
                       whenever you need something the user wrote, read or decided before instead \
                       of asking them to repeat it. For the first lookup on a new message, use \
                       rhebok_get_context, which takes the user's message itself. To read a hit's \
                       whole document, use rhebok_read_document; to follow its links, use \
                       rhebok_list_related. When nothing you expected comes back, check \
                       rhebok_get_status.\n\nParameters: write `query` as a question or a \
                       description of the note, such as \"why did we stop using PgBouncer session \
                       mode\". A single keyword ranks worse, but an exact identifier such as \
                       ADR-0004 or an error string still matches. `quality` trades time for \
                       recall: 0 is the fastest, the default 3 usually answers within a second, \
                       and 4 can take about two seconds. Search again at 4 only when the first \
                       pass came back thin.\n\nBehavior: only the query text leaves rhebok, to \
                       the configured embedding server (a local Ollama by default). Hits under \
                       the relevance gate are dropped, so an empty list is an honest no-match. \
                       When the search cannot run, `error` says why and what to do, and that is \
                       never a no-match.\n\nReturns: at most 10 hits, most relevant first, each \
                       with its `source` uri, heading, page, `relevance` from 0 to 1 and excerpt \
                       text."
    )]
    async fn rhebok_search_knowledge(
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
        title = "Get context for a message",
        annotations(
            title = "Get context for a message",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = output_schema::<crate::context::Context>(),
        description = "Get the excerpts and saved memories that belong with the user's current \
                       message, chosen and size-limited like the automatic prompt hook, with more \
                       time to search. It takes the user's message; rhebok_search_knowledge takes \
                       a query you write.\n\nUse it before answering whenever the message touches \
                       the user's own work: their projects, code, notes, decisions, past \
                       sessions, people or plans. Skip it when the message already contains a \
                       <rhebok-context> block, or when it asks about general knowledge. Use \
                       rhebok_search_knowledge for later, narrower lookups.\n\nParameters: pass \
                       `message` exactly as the user wrote it, typos included. The relevance gate \
                       is calibrated on real prompts, so a summary or a rewrite is judged against \
                       the wrong bar. `cwd` decides which project's memories count, so pass the \
                       working directory whenever you know it. `prehook` and `session` belong to \
                       rhebok's own Claude Code mod: never set them.\n\nBehavior: it embeds the \
                       message through the configured embedding server and usually answers within \
                       a second. It changes nothing in the index or in the saved \
                       memories.\n\nReturns: `block`, ready to read as context, or null when \
                       nothing cleared the gate, which is a normal answer; `hits`, the title and \
                       uri of each excerpt in the block; and `notes`, which says when retrieval \
                       was degraded, for example because the embedding server was unreachable."
    )]
    async fn rhebok_get_context(
        &self,
        Parameters(args): Parameters<ContextArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let tools = self.clone();
        let context = tokio::task::spawn_blocking(move || tools.context(&args))
            .await
            .unwrap_or_else(|e| {
                eprintln!("rhebok: rhebok_get_context worker panicked — {e:#}");
                crate::context::Context::default()
            });
        Ok(reply(context.to_json(), &context))
    }

    #[tool(
        title = "List connected notes",
        annotations(
            title = "List connected notes",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = output_schema::<HitsOutput>(),
        description = "List the documents and sections connected to one document by structure, \
                       not by meaning: the sections after its opening section, the notes it links \
                       to with [[wikilinks]], and the note that replaced it, up to two links \
                       away.\n\nUse it after rhebok_search_knowledge or rhebok_get_context gives \
                       you a starting point, to follow a thread of linked notes. It does not \
                       search: to find a document, use rhebok_search_knowledge; to read one from \
                       start to end, use rhebok_read_document.\n\nParameters: `uri` is the \
                       `source` of a hit, unchanged. Notes have file:// uris and past sessions \
                       claude-session:// or codex-session:// uris. A path without its scheme, or \
                       a uri with spaces encoded as %20, finds nothing.\n\nBehavior: it walks the \
                       link graph stored by the last index run and calls no embedding server. A \
                       link added to a note since then appears only after \
                       rhebok_update_index.\n\nReturns: at most 10 hits, alternating adjacent \
                       sections and linked notes, in the shape rhebok_search_knowledge uses, with \
                       `relevance` always 0. An unknown uri or a document with no links returns \
                       an empty list; `error` is set only when the index cannot be read."
    )]
    async fn rhebok_list_related(
        &self,
        Parameters(args): Parameters<RelatedArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        // See rhebok_search_knowledge: blocking store/HTTP calls must run off the async
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
        title = "Read a document",
        annotations(
            title = "Read a document",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = output_schema::<DocumentOutput>(),
        description = "Read one indexed document in full, section by section in document order, \
                       by its uri: a note, a PDF, a past agent session or a saved memory.\n\nUse \
                       it when an excerpt from rhebok_search_knowledge, rhebok_get_context or \
                       rhebok_list_related is not enough and you need the whole document around \
                       it. It does not search: to find a document, use \
                       rhebok_search_knowledge.\n\nParameters: `uri` is a hit's `source`, \
                       unchanged, for example file:///Users/me/notes/adr-0004.md or \
                       memory://lesson/c6f771fc1a36. A bare path, or a uri with spaces encoded as \
                       %20, finds nothing. Page through a long document by passing each reply's \
                       `next` back as `start` until `next` is null. A section is usually 1,000 to \
                       2,000 characters, so raise `limit` toward 100 only when you need many \
                       sections at once.\n\nBehavior: it returns the text as last indexed, which \
                       can be older than the file on disk until rhebok_update_index runs. It \
                       calls no embedding server, and nothing leaves this machine.\n\nReturns: \
                       the title, the requested sections with their headings and page numbers, \
                       `total` and `next`. A uri that is not indexed returns no sections, and \
                       `error` says why and what to do."
    )]
    async fn rhebok_read_document(
        &self,
        Parameters(args): Parameters<ReadArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let tools = self.clone();
        let uri = args.uri.clone();
        let out = tokio::task::spawn_blocking(move || tools.read(&args))
            .await
            .unwrap_or_else(|e| DocumentOutput::failed(&uri, format!("worker panicked: {e}")));
        Ok(reply(out.describe(), &out))
    }

    #[tool(
        title = "Get the knowledge base status",
        annotations(
            title = "Get the knowledge base status",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = output_schema::<StatusOutput>(),
        description = "Get the state of the user's rhebok knowledge base: document and section \
                       counts, sections waiting for an embedding, the embedding model, when the \
                       index was last published, whether an index run is in progress, the \
                       sources, the files the index could not read and why, saved memories by \
                       kind, and config problems.\n\nUse it when a search finds nothing you \
                       expected, to tell an empty, stale or still-building index from a real \
                       no-match; before rhebok_update_index, to see whether a run is already \
                       going; or when the user asks what rhebok holds. It lists no documents or \
                       memories: use rhebok_search_knowledge and rhebok_list_memories for \
                       those.\n\nParameters: `skipped_limit` caps the list of unreadable files at \
                       20 by default. Raise it, up to 200, when `skipped_total` is larger and you \
                       need every file, for example to explain why a folder is missing; set it to \
                       0 when only the counts matter.\n\nBehavior: it reads only local files and \
                       makes no network call, so it cannot tell whether the embedding server is \
                       up; a failing search reports that. During an index run it answers from the \
                       index that is live.\n\nReturns: `documents` 0 with `last_indexed` null \
                       means nothing is indexed yet. `vectors_pending` above 0 means search by \
                       meaning misses those sections until a backfill embeds them. When the index \
                       cannot be read, `error` says why and the counts read 0."
    )]
    async fn rhebok_get_status(
        &self,
        Parameters(args): Parameters<StatusArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let tools = self.clone();
        let out = tokio::task::spawn_blocking(move || tools.status(&args))
            .await
            .unwrap_or_else(|e| StatusOutput {
                version: env!("CARGO_PKG_VERSION").to_string(),
                error: Some(format!("worker panicked: {e}")),
                ..StatusOutput::default()
            });
        let text = serde_json::to_string(&out).unwrap_or_else(|_| "{}".to_string());
        Ok(reply(text, &out))
    }

    #[tool(
        title = "Update the knowledge base index",
        annotations(
            title = "Update the knowledge base index",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = output_schema::<IndexOutput>(),
        description = "Update the user's knowledge base index: read new and changed files, embed \
                       sections that are still waiting, or compact the index.\n\nUse it when the \
                       user has just added or edited files that a search does not find, when \
                       rhebok_get_status shows `vectors_pending` above 0, or when a search error \
                       asks for a compaction. Do not call it routinely: rhebok already indexes at \
                       the start of every session. Check rhebok_get_status first to see whether a \
                       run is already going.\n\nParameters: `mode` \"changes\" (the default) \
                       stats every configured source, reads only files whose size or modification \
                       time changed, and removes documents whose file is gone. \"backfill\" \
                       embeds waiting sections and starts no new batch after two minutes, though \
                       the batch under way and the final rebuild of the index can run past that; \
                       call it again while `vectors_pending` stays above 0. \"compact\" rebuilds \
                       the index from its own stored rows, which repairs a search that refuses \
                       its index and reclaims disk space, without reading any file or embedding \
                       anything.\n\nBehavior: \"changes\" and \"backfill\" send new text to the \
                       configured embedding server, which can take minutes for many files. Search \
                       answers from the previous index until the new one is swapped in. While \
                       another index run holds the lock, it refuses and changes \
                       nothing.\n\nReturns: the documents added, updated and deleted and the \
                       sections read, or for a backfill the sections embedded and still waiting; \
                       `error` says why a run failed or was refused."
    )]
    async fn rhebok_update_index(
        &self,
        Parameters(args): Parameters<IndexArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        // See rhebok_search_knowledge: blocking store/HTTP calls must run off the async
        // worker threads.
        let config = self.config.clone();
        let mode = args.mode.unwrap_or_default();
        let out = tokio::task::spawn_blocking(move || run_index_mode(&config, mode))
            .await
            .unwrap_or_else(|e| IndexOutput::failed(mode, format!("task panicked: {e}")));
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
        description = "Save a memory that later sessions see: a lesson (a correction or a rule \
                       the user gave you), a fact (something durable about the user or their \
                       setup) or an episode (what happened and what was decided).\n\nUse it when \
                       the user corrects you, states a preference, or asks you to remember \
                       something. Do not use it for what their notes already say, which \
                       rhebok_search_knowledge finds, or for details that matter only in this \
                       conversation. To see what is saved, use rhebok_list_memories; to remove a \
                       memory, use rhebok_delete_memory.\n\nParameters: write `text` as the rule \
                       or fact itself, in one to three sentences that make sense with no \
                       conversation around them, for example \"Never commit with the personal \
                       email; use the noreply address.\" `scope` defaults to the current project; \
                       pass \"global\" only for a rule the user means everywhere. Leave \
                       `confidence` at 80, and use 100 only when the user stated the rule \
                       outright; a memory you save below the configured floor (80 by default) is \
                       refused.\n\nBehavior: the text is sent to the configured embedding server, \
                       so a save fails while that server is unreachable; the memory itself stays \
                       on this machine. A memory almost the same as one of the same kind (cosine \
                       similarity 0.94 or more by default) replaces it, and the replaced memory \
                       is deleted with no undo. Lessons appear at the start of every session in \
                       their project; every kind can be retrieved on later prompts.\n\nReturns: \
                       `status` saved, replaced, duplicate or rejected; the memory's `id`, which \
                       rhebok_delete_memory takes; the id and title of a memory it replaced; and \
                       `reason` when nothing was saved."
    )]
    async fn rhebok_save_memory(
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
                            eprintln!("rhebok: rhebok_save_memory failed — {e:#}");
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
        title = "Delete a memory",
        annotations(
            title = "Delete a memory",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        ),
        output_schema = output_schema::<ForgetOutput>(),
        description = "Delete one saved memory (a lesson, fact or episode) by its id.\n\nUse it \
                       when the user says a memory is wrong or no longer applies. To correct a \
                       memory instead, save the new wording with rhebok_save_memory, which \
                       replaces the near-duplicate. To find an id, use \
                       rhebok_list_memories.\n\nParameters: `id` is usually 12 hex characters, \
                       such as c6f771fc1a36; an episode distilled from a session has that \
                       session's id instead. A shorter prefix works when exactly one memory \
                       starts with it, so c6f7 can be enough; a prefix that matches two memories \
                       deletes nothing.\n\nBehavior: there is no undo. The memory leaves \
                       session-start lessons and prompt retrieval at once, and restoring it means \
                       saving it again. It touches only rhebok's memory store, never the user's \
                       notes or files, so it cannot remove a search hit.\n\nReturns: `forgotten` \
                       with the deleted memory's id, kind and title. An id that matches nothing \
                       deletes nothing and says so in `reason`."
    )]
    async fn rhebok_delete_memory(
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
        description = "List the memories rhebok has saved for the user (lessons, facts and \
                       episodes), newest first, with each one's id, scope, origin, confidence and \
                       full text.\n\nUse it when the user asks what you remember about them, to \
                       find the id rhebok_delete_memory takes, or to check whether a rule is \
                       already saved before rhebok_save_memory. It filters and does not rank: to \
                       find the memories about a topic, use rhebok_search_knowledge, which \
                       retrieves memories beside notes.\n\nParameters: combine `kind` and `scope` \
                       to narrow the list, for example kind \"lesson\" with scope \"global\" for \
                       the rules that apply everywhere. A project path as `scope` lists only that \
                       project's own memories, not the global ones. Raise `limit` up to 200 when \
                       `total` shows the list was cut.\n\nBehavior: it embeds nothing, so it \
                       works while the embedding server is down. It also lists the memories that \
                       the session-start lessons hold back for low confidence.\n\nReturns: \
                       `memories`, and `total`, which counts every match before `limit` is \
                       applied. `error` explains an empty answer that could not be given, for \
                       example when memory is disabled."
    )]
    async fn rhebok_list_memories(
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
    async fn call_tool(
        &self,
        mut request: CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = current_tool_name(&request.name).to_string();
        request.name = name.into();
        let call = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        self.tool_router.call(call).await
    }

    fn get_info(&self) -> ServerConfig {
        // ServerConfig (= InitializeResult) is #[non_exhaustive]: a struct literal
        // will not compile. Build with `new`, then set the optional fields.
        let mut info = ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("rhebok", env!("CARGO_PKG_VERSION"))
                    .with_website_url("https://github.com/Stellenbosch-Technologies/rhebok"),
            );
        info.instructions = Some(
            "Search and read the user's personal knowledge base, plus durable memory. \
             rhebok_get_context gets what belongs with the user's message; \
             rhebok_search_knowledge runs a query you write; rhebok_read_document reads a \
             hit's whole document; rhebok_list_related follows its links; rhebok_get_status \
             says what is indexed and rhebok_update_index updates it. rhebok_save_memory saves \
             lessons, facts and episodes; rhebok_list_memories lists them; \
             rhebok_delete_memory deletes one."
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

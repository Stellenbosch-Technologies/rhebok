use crate::config::{Config, Profile, Surface};
use crate::hook::{spawn_remote_load, REMOTE_LOAD_WINDOW};
use crate::retrieve::Retriever;
use crate::store::Hit;
use anyhow::Result;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, serde::Serialize, PartialEq, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ContextHit {
    /// The source document's uri. Pass it to rhebok_list_related as `uri`.
    pub uri: String,
    /// The source document's title.
    pub title: String,
    /// 0 to 1, higher is closer to the message.
    pub relevance: f32,
    /// The id of the excerpt within the index.
    pub chunk_id: String,
    /// The mounted knowledge base the hit came from, when it is not the user's own index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mount: Option<String>,
}

impl From<&Hit> for ContextHit {
    fn from(h: &Hit) -> Self {
        let shown =
            |field: &str| crate::budget::escape_reserved_tags(&crate::budget::one_line(field));
        ContextHit {
            uri: shown(&h.uri),
            title: shown(&h.title),
            relevance: h.relevance,
            chunk_id: h.chunk_id.clone(),
            mount: h.mount.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, PartialEq, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct Context {
    /// The context to use, as a <rhebok-context> block of excerpts and memories. Null when nothing cleared the relevance gate, which is a normal answer.
    pub block: Option<String>,
    /// The sources quoted in `block`, most relevant first.
    pub hits: Vec<ContextHit>,
    /// Why retrieval was degraded or partial, for example an unreachable embedding server or a skipped mount. Empty on a normal answer.
    pub notes: Vec<String>,
    /// Why no search ran at all, for example a message too short to search on.
    pub skipped: Option<String>,
    /// How long the lookup took, in milliseconds.
    pub elapsed_ms: u64,
    /// The time budget the lookup ran under, in milliseconds.
    pub budget_ms: u32,
    /// Set only for the rhebok-prehook mod: which injector is configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub injector: Option<&'static str>,
    /// Set only for the rhebok-prehook mod: how it should show its status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_style: Option<&'static str>,
    /// The rhebok project `cwd` belongs to, when it belongs to one.
    pub project: Option<String>,
    /// A project file found under `cwd` that is ignored because the user has not trusted it yet.
    pub untrusted_project_file: Option<String>,
}

impl Context {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

pub fn build(cfg: &Config, prompt: &str, cwd: Option<&Path>) -> Context {
    let profile = cfg.profile_for(Surface::Hook);
    build_with(cfg, prompt, cwd, &profile, || {
        crate::retrieve_for(cfg, Surface::Hook)
    })
}

pub fn build_for_prehook(
    prompt: &str,
    cwd: Option<&Path>,
    session: Option<&str>,
    retriever: impl FnOnce(&Profile) -> Result<Retriever>,
) -> Context {
    let cfg = Config::load();
    let injector = cfg.prehook.injector;
    let status_style = Some(cfg.prehook.status.as_str());
    if injector != crate::prehook::Injector::Mod {
        return Context {
            skipped: Some(format!("`[prehook] injector` is {}", injector.as_str())),
            injector: Some(injector.as_str()),
            status_style,
            ..Context::default()
        };
    }
    if let Some(s) = session {
        let _ = crate::prehook::mark_mod_running(&Config::db_path(), s);
    }
    let profile = cfg.profile_for(Surface::Hook);
    Context {
        injector: Some(injector.as_str()),
        status_style,
        ..build_with(&cfg, prompt, cwd, &profile, || retriever(&profile))
    }
}

pub fn build_with(
    cfg: &Config,
    prompt: &str,
    cwd: Option<&Path>,
    profile: &Profile,
    retriever: impl FnOnce() -> Result<Retriever>,
) -> Context {
    let started = Instant::now();
    let resolution = cwd
        .map(|c| crate::project::resolve(cfg, c))
        .unwrap_or_default();
    let project = resolution.name().map(str::to_string);
    let untrusted_project_file = resolution
        .untrusted
        .as_ref()
        .map(|p| p.display().to_string());
    if !crate::hook::should_retrieve(prompt) {
        return Context {
            skipped: Some("the prompt is too short to search on".to_string()),
            project,
            untrusted_project_file,
            ..Context::default()
        };
    }
    let wanted = scopes_for(cfg, resolution.project.as_ref());
    let mut mount_notes = Vec::new();
    let mounts = mounts_for(cfg, resolution.project.as_ref(), &mut mount_notes);
    let memory_filter = crate::project::MemoryFilter {
        cwd: cwd.map(Path::to_path_buf),
        project: resolution.project.clone(),
    };
    let scoped = || {
        let wanted = wanted?;
        retriever().and_then(|r| {
            r.with_scope(&Config::db_path(), &wanted)
                .map(|r| r.with_memory_filter(memory_filter).with_mounts(mounts))
                .map_err(anyhow::Error::from)
        })
    };
    let mut context = gather(cfg, prompt, profile, scoped);
    context.notes.splice(0..0, mount_notes);
    context.elapsed_ms = started.elapsed().as_millis() as u64;
    context.project = project;
    context.untrusted_project_file = untrusted_project_file;
    context
}

pub fn scopes_for(
    cfg: &Config,
    project: Option<&crate::project::Project>,
) -> Result<Vec<(String, String)>> {
    let Some(project) = project else {
        return Ok(Vec::new());
    };
    let defs = crate::project::scope_definitions(cfg);
    std::iter::once(project.name.clone())
        .chain(
            project
                .rules
                .imports
                .iter()
                .filter(|name| !cfg.mounts.contains_key(*name))
                .cloned(),
        )
        .map(|name| {
            defs.iter()
                .find(|d| d.name == name)
                .map(|d| (name.clone(), d.hash.clone()))
                .ok_or_else(|| anyhow::anyhow!("project scope `{name}` has no definition"))
        })
        .collect()
}

pub fn mounts_for(
    cfg: &Config,
    project: Option<&crate::project::Project>,
    notes: &mut Vec<String>,
) -> Vec<(String, std::sync::Arc<crate::pack::Pack>)> {
    let Some(project) = project else {
        return Vec::new();
    };
    project
        .rules
        .imports
        .iter()
        .filter_map(|name| cfg.mounts.get(name).map(|m| (name, m)))
        .filter_map(|(name, mount)| {
            match crate::project::open_mount(cfg, &Config::expand_tilde_path(&mount.path)) {
                Ok(pack) => Some((name.clone(), std::sync::Arc::new(pack))),
                Err(e) => {
                    notes.push(format!("rhebok: mount `{name}` skipped — {e:#}"));
                    None
                }
            }
        })
        .collect()
}

fn gather(
    cfg: &Config,
    prompt: &str,
    profile: &Profile,
    retriever: impl FnOnce() -> Result<Retriever>,
) -> Context {
    let surface = cfg.surface(Surface::Hook);
    let mut notes = Vec::new();

    // The relevance gate. Below threshold, injecting costs tokens and buys noise.
    // Gate on `relevance`, never on `score`. `score` is an RRF rank value whose
    // arithmetic maximum is about 0.049, so comparing it against a human
    // threshold like 0.55 rejects every hit and the hook silently injects
    // nothing forever. `relevance` is a [0,1] similarity with an absolute
    // meaning, carried by every hit regardless of which retriever found it.
    //
    // `search_gated` applies it inside the pipeline, before rerank and before
    // diversity selection — filtering out here instead let MMR spend its slots
    // on hits that were about to be discarded.
    //
    // Weighted with the HOOK's weights, not the global table. The weights
    // multiply `relevance`, which is what `threshold` is compared against, so
    // the two have to come from the same surface or the gate is calibrated
    // against a scale nobody configured.
    // Take priority over any background index for the duration of this
    // retrieval — without it, the query embed queued behind bulk batches and
    // the hook was mute for every prompt of a multi-minute reindex.
    let _priority = crate::index::QueryPriority::announce(&Config::db_path());
    let relevant: Vec<Hit> = match retriever()
        .and_then(|r| r.search_gated_with_report(prompt, profile, surface.threshold))
    {
        Ok((hits, report)) => {
            // The hook exits 0 no matter what, so a pipeline that silently
            // stopped running a stage looks exactly like an honest no-match.
            // stdout carries the hook's JSON contract, so stderr — wherever
            // the invoking process (Claude Code, or a shell during manual
            // testing) sends it — is the only place a human can see this
            // happened. Unlike `run_session_start`'s indexer subprocess, this
            // process's own stderr is not redirected to `db.log`.
            if report.degraded {
                notes.push(format!(
                    "rhebok: retrieval degraded — budget {}ms exceeded, ran only [{}]",
                    profile.budget_ms,
                    report.stages_run.join("+")
                ));
            }
            // Distinct from the budget line above: this profile had time to
            // run BM25, asked for it, and there was no pack to serve it — the
            // store-fallback path, where `Store::fts_search` always errors
            // now that lbug's FTS index is gone. The query still answers
            // vector-only rather than failing outright, so stderr is the only
            // place this loss is visible.
            if let Some(reason) = &report.bm25_unavailable {
                notes.push(format!(
                    "rhebok: retrieval degraded — bm25 unavailable, ran vector-only ({reason})"
                ));
            }
            // The mirror image: a pack published before any embedding has
            // postings but no vectors (`Manifest::rows_with_vectors == 0`,
            // `Pack::has_vectors() == false`). Vector search answers empty
            // rather than erroring, so this is the only way the loss is
            // visible — see `StageReport::vectors_unavailable`.
            if let Some(reason) = &report.vectors_unavailable {
                notes.push(format!(
                    "rhebok: retrieval degraded — vectors unavailable, ran bm25-only ({reason})"
                ));
            }
            if let Some(reason) = &report.memory_unavailable {
                notes.push(format!(
                    "rhebok: memory unavailable — {reason}; run `rhebok memory rebuild`"
                ));
            }
            hits
        }
        // A hard error here is almost always `open_existing` refusing to load
        // the `vector` extension — a build/link misconfiguration, not a
        // transient condition. Silently returning made that failure
        // indistinguishable from an honest no-match, forever. The hook still
        // exits 0 and never blocks the prompt; stderr is the only channel
        // left to say this happened, since stdout carries the hook's JSON
        // contract.
        Err(e) if cfg.embed.remote.is_some() && crate::embed::timed_out(&e) => {
            let log = Config::db_path().with_extension("log");
            notes.push(match spawn_remote_load(&log) {
                Ok(Some(pid)) => format!(
                    "rhebok: retrieval unavailable — the embedding endpoint did not answer in \
                     time, probably while loading its model; loading it in the background \
                     (pid {pid}) for the next prompt ({e:#})"
                ),
                Ok(None) => format!(
                    "rhebok: retrieval unavailable — the embedding endpoint did not answer in \
                     time; a background load started in the last {}s is still running ({e:#})",
                    REMOTE_LOAD_WINDOW.as_secs()
                ),
                Err(spawn) => format!(
                    "rhebok: retrieval unavailable — {e:#}; could not start a background load: \
                     {spawn}"
                ),
            });
            return Context {
                notes,
                ..Context::default()
            };
        }
        Err(e) => {
            notes.push(format!("rhebok: retrieval unavailable — {e:#}"));
            return Context {
                notes,
                ..Context::default()
            };
        }
    };

    let injected = crate::budget::admitted_by_budget(&relevant, surface.max_tokens);
    let own: Vec<Hit> = relevant[..injected]
        .iter()
        .filter(|h| h.mount.is_none())
        .cloned()
        .collect();
    crate::usage::record_hits(&Config::db_path(), &own);

    Context {
        block: crate::hook::build_context(&relevant, surface.max_tokens),
        hits: relevant[..injected].iter().map(ContextHit::from).collect(),
        notes,
        skipped: None,
        elapsed_ms: 0,
        budget_ms: profile.budget_ms,
        injector: None,
        status_style: None,
        project: None,
        untrusted_project_file: None,
    }
}

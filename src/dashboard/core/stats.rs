use super::DashboardError;
use crate::config::Config;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Tiers {
    pub hook: u8,
    pub mcp: u8,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Stats {
    pub documents: i64,
    pub chunks: i64,
    pub by_source: HashMap<String, u32>,
    pub sessions_by_agent: BTreeMap<String, u32>,
    pub model: Option<String>,
    pub skipped: Vec<crate::skip::Skip>,
    pub skipped_ignored: usize,
    pub bench: Option<Vec<crate::bench::BenchTier>>,
    pub bench_provenance: Option<crate::bench::Provenance>,
    pub bench_at: Option<u64>,
    pub tiers: Tiers,
    pub hook_threshold: f32,
    pub memory: BTreeMap<String, usize>,
}

pub fn stats(cfg: &Config) -> Result<Stats, DashboardError> {
    let db = Config::db_path();
    let (snap, by_source, sessions_by_agent) = if no_index_yet(&db) {
        (
            crate::store::StatusSnapshot {
                documents: 0,
                chunks: 0,
                model: None,
                skipped: Vec::new(),
                vectors_pending: 0,
            },
            Default::default(),
            Default::default(),
        )
    } else {
        let store = crate::store::Store::open_existing(&db, cfg.embed.dimensions)?;
        (
            store.status_snapshot(),
            store.counts_by_source().unwrap_or_default(),
            store.counts_sessions_by_agent().unwrap_or_default(),
        )
    };
    let (skipped, skipped_ignored) = crate::skip::split_ignored(snap.skipped, &cfg.ignore);
    let report_path = crate::bench::report_path();
    let report = crate::bench::read_report_at(&report_path);
    let bench_at = std::fs::metadata(&report_path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let (bench, bench_provenance) = match report {
        Some(r) => (Some(r.tiers), r.provenance),
        None => (None, None),
    };
    Ok(Stats {
        documents: snap.documents,
        chunks: snap.chunks,
        by_source,
        sessions_by_agent,
        model: snap.model,
        skipped,
        skipped_ignored,
        bench,
        bench_provenance,
        bench_at,
        tiers: Tiers {
            hook: cfg.quality_for(crate::config::Surface::Hook),
            mcp: cfg.quality_for(crate::config::Surface::Mcp),
        },
        hook_threshold: cfg.hook.threshold,
        memory: crate::memory::counts_at(&crate::memory::default_root())
            .unwrap_or_default()
            .into_iter()
            .map(|(k, n)| (k.as_str().to_string(), n))
            .collect(),
    })
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunningProgress {
    pub pct: f64,
    pub docs_done: u64,
    pub docs_total: u64,
    pub chunks: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub elapsed_s: u64,
    pub eta_s: u64,
    pub pid: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(untagged)]
pub enum Progress {
    Running(RunningProgress),
    Idle { idle: bool },
}

impl Progress {
    pub fn is_running(&self) -> bool {
        matches!(self, Progress::Running(_))
    }
}

pub fn progress() -> Result<Progress, DashboardError> {
    Ok(progress_at(&Config::db_path().with_extension("progress")))
}

pub(crate) fn progress_at(path: &std::path::Path) -> Progress {
    let idle = Progress::Idle { idle: true };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return idle;
    };
    let Ok(running) = serde_json::from_str::<RunningProgress>(&raw) else {
        return idle;
    };
    if crate::index::pid_is_alive(running.pid) {
        Progress::Running(running)
    } else {
        idle
    }
}

pub(in crate::dashboard) fn no_index_yet(db: &std::path::Path) -> bool {
    !db.exists() && !db.with_extension("old").exists()
}

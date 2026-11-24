use super::DashboardError;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

static REAPING: Mutex<BTreeSet<u32>> = Mutex::new(BTreeSet::new());

fn reaping() -> MutexGuard<'static, BTreeSet<u32>> {
    REAPING.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct IndexStarted {
    pub started: bool,
    pub pid: u32,
    pub reindex: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct IndexRun {
    pub pid: u32,
    pub reindex: bool,
    pub finished: bool,
    pub ok: Option<bool>,
    pub code: Option<i32>,
    pub log: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct IndexRunView {
    pub run: Option<IndexRun>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Record {
    pid: u32,
    reindex: bool,
    #[serde(default)]
    started_at: u64,
    finished: bool,
    ok: Option<bool>,
    code: Option<i32>,
    log: String,
}

fn record_path(db: &Path) -> PathBuf {
    db.with_extension("indexrun")
}

fn read_record(db: &Path) -> Option<Record> {
    let text = std::fs::read_to_string(record_path(db)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_record(db: &Path, record: &Record) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(record)?;
    crate::config::edit::replace_file(&record_path(db), &bytes, None)
}

pub fn run_status(db: &Path) -> IndexRunView {
    let reaping = reaping();
    let run = read_record(db).map(|mut r| {
        if !r.finished && !reaping.contains(&r.pid) && !crate::index::pid_is_alive(r.pid) {
            r.finished = true;
            r.ok = None;
            r.code = None;
        }
        IndexRun {
            pid: r.pid,
            reindex: r.reindex,
            finished: r.finished,
            ok: r.ok,
            code: r.code,
            log: r.log,
        }
    });
    IndexRunView { run }
}

fn an_index_is_running(db: &Path) -> bool {
    crate::index::IndexLock::is_held(db) || super::stats::progress().is_ok_and(|p| p.is_running())
}

fn write_outcome(db: &Path, pid: u32, ok: bool, code: Option<i32>) {
    if let Some(mut record) = read_record(db).filter(|r| r.pid == pid && !r.finished) {
        record.finished = true;
        record.ok = Some(ok);
        record.code = code;
        let _ = write_record(db, &record);
    }
}

fn record_exit(db: &Path, pid: u32, status: std::io::Result<std::process::ExitStatus>) {
    let mut reaping = reaping();
    write_outcome(
        db,
        pid,
        status.as_ref().is_ok_and(|s| s.success()),
        status.ok().and_then(|s| s.code()),
    );
    reaping.remove(&pid);
}

pub fn record_own_outcome(db: &Path, code: i32) {
    let _reaping = reaping();
    write_outcome(db, std::process::id(), code == 0, Some(code));
}

pub fn start(db: &Path, cli: &Path, reindex: bool) -> Result<IndexStarted, DashboardError> {
    if an_index_is_running(db) {
        return Err(DashboardError::Conflict(
            "an index is already running".to_string(),
        ));
    }
    super::install::require_cli(cli)?;
    let log = db.with_extension("log");
    let sink = || {
        crate::hook::open_log_for_append(&log)
            .map(std::process::Stdio::from)
            .unwrap_or_else(|_| std::process::Stdio::null())
    };
    let mut command = std::process::Command::new(cli);
    command.arg("index");
    if reindex {
        command.arg("--reindex");
    }
    let mut child = command
        .stdout(sink())
        .stderr(sink())
        .spawn()
        .map_err(|e| DashboardError::Internal(e.to_string()))?;
    let pid = child.id();
    let started_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let record = Record {
        pid,
        reindex,
        started_at,
        finished: false,
        ok: None,
        code: None,
        log: log.display().to_string(),
    };
    let mut reaping = reaping();
    if let Err(e) = write_record(db, &record) {
        drop(reaping);
        let _ = child.kill();
        let _ = child.wait();
        return Err(DashboardError::Internal(format!("{e:#}")));
    }
    reaping.insert(pid);
    drop(reaping);
    let db = db.to_path_buf();
    std::thread::spawn(move || {
        let status = child.wait();
        record_exit(&db, pid, status);
    });
    Ok(IndexStarted {
        started: true,
        pid,
        reindex,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unfinished(pid: u32) -> Record {
        Record {
            pid,
            reindex: false,
            started_at: 1,
            finished: false,
            ok: None,
            code: None,
            log: "x.log".to_string(),
        }
    }

    fn dead_pid() -> u32 {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    #[test]
    fn a_run_reaped_but_not_yet_recorded_never_reads_as_failed() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db");
        let pid = dead_pid();
        write_record(&db, &unfinished(pid)).unwrap();
        reaping().insert(pid);

        let run = run_status(&db).run.unwrap();
        assert!(!run.finished, "{run:?}");
        assert_eq!(run.ok, None);

        record_exit(&db, pid, Ok(std::process::ExitStatus::default()));
        assert!(!reaping().contains(&pid));
        let run = run_status(&db).run.unwrap();
        assert!(run.finished, "{run:?}");
        assert_eq!(run.ok, Some(true));
        assert_eq!(run.code, Some(0));
    }

    #[cfg(unix)]
    #[test]
    fn a_reaper_leaves_a_run_that_recorded_its_own_outcome_alone() {
        use std::os::unix::process::ExitStatusExt;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db");
        let pid = dead_pid();
        let mut own = unfinished(pid);
        own.finished = true;
        own.ok = Some(true);
        own.code = Some(0);
        write_record(&db, &own).unwrap();

        record_exit(&db, pid, Ok(std::process::ExitStatus::from_raw(1 << 8)));
        let run = run_status(&db).run.unwrap();
        assert_eq!(
            (run.finished, run.ok, run.code),
            (true, Some(true), Some(0))
        );
    }

    #[test]
    fn a_process_records_its_own_outcome_only_on_a_record_that_names_it() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db");
        write_record(&db, &unfinished(std::process::id())).unwrap();
        record_own_outcome(&db, 1);
        let run = run_status(&db).run.unwrap();
        assert_eq!(
            (run.finished, run.ok, run.code),
            (true, Some(false), Some(1))
        );

        let other = dead_pid();
        write_record(&db, &unfinished(other)).unwrap();
        record_own_outcome(&db, 0);
        let raw: Record = read_record(&db).unwrap();
        assert!(!raw.finished);
        assert_eq!(raw.pid, other);
    }
}

use super::DashboardError;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateStarted {
    pub started: bool,
    pub pid: u32,
}

pub fn start(paths: &crate::setup::Paths, cli: &Path) -> Result<UpdateStarted, DashboardError> {
    if let Some(why) = refusal(paths) {
        return Err(DashboardError::Conflict(why));
    }
    super::install::require_cli(cli)?;
    let log = paths.db.with_extension("log");
    let sink = || {
        crate::hook::open_log_for_append(&log)
            .map(std::process::Stdio::from)
            .unwrap_or_else(|_| std::process::Stdio::null())
    };
    let child = std::process::Command::new(cli)
        .args(["update", "--yes", "--quiet"])
        .stdout(sink())
        .stderr(sink())
        .spawn()
        .map_err(|e| DashboardError::Internal(e.to_string()))?;
    Ok(UpdateStarted {
        started: true,
        pid: child.id(),
    })
}

fn refusal(paths: &crate::setup::Paths) -> Option<String> {
    if crate::index::IndexLock::is_held(&paths.db) {
        return Some("an index is running; wait for it to finish".to_string());
    }
    if let Some(s) = crate::update::Status::running(&paths.update_status) {
        return Some(format!("an update is already running ({})", s.phase));
    }
    None
}

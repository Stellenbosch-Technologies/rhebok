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

#[derive(Debug, Clone, serde::Serialize)]
pub struct RestartStarted {
    pub started: bool,
    pub pid: u32,
    pub dashboard_pid: u32,
}

pub fn restart(paths: &crate::setup::Paths) -> Result<RestartStarted, DashboardError> {
    if let Some(s) = crate::update::Status::running(&paths.update_status) {
        return Err(DashboardError::Conflict(format!(
            "an update is running ({}); restart once it finishes",
            s.phase
        )));
    }
    let cli =
        crate::restart::binary(paths).map_err(|e| DashboardError::Internal(format!("{e:#}")))?;
    super::install::require_cli(&cli)?;
    let log = paths.db.with_extension("log");
    let sink = || {
        crate::hook::open_log_for_append(&log)
            .map(std::process::Stdio::from)
            .unwrap_or_else(|_| std::process::Stdio::null())
    };
    let mut command = std::process::Command::new(&cli);
    command
        .arg("restart")
        .stdin(std::process::Stdio::null())
        .stdout(sink())
        .stderr(sink());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let child = command
        .spawn()
        .map_err(|e| DashboardError::Internal(e.to_string()))?;
    Ok(RestartStarted {
        started: true,
        pid: child.id(),
        dashboard_pid: std::process::id(),
    })
}

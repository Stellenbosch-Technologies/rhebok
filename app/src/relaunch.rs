use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const SETTLE: Duration = Duration::from_millis(750);

pub fn installed_app(paths: &rhebok::setup::Paths, running: Option<PathBuf>) -> PathBuf {
    let installed = paths.root.join("bin/rhebok-app");
    let beside_cli = paths.bin.with_file_name("rhebok-app");
    [installed.clone(), beside_cli]
        .into_iter()
        .find(|candidate| candidate.is_file())
        .or(running)
        .unwrap_or(installed)
}

pub fn relaunch(app: &Path, args: &[OsString], log: &Path) -> Result<u32, String> {
    let sink = || {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .map(std::process::Stdio::from)
            .unwrap_or_else(|_| std::process::Stdio::null())
    };
    let mut command = std::process::Command::new(app);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(sink())
        .stderr(sink());
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", app.display()))?;
    let deadline = Instant::now() + SETTLE;
    while Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!(
                "{} exited with {status} as it started; see {}",
                app.display(),
                log.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Ok(child.id())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    fn kill(pid: u32) {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status();
    }

    #[test]
    fn the_installed_app_wins_then_the_one_beside_the_cli_then_the_running_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = rhebok::setup::Paths::at(dir.path(), Vec::new(), dir.path().join("cache"));
        paths.bin = dir.path().join("brew/bin/rhebok");
        let running = dir.path().join("dev/rhebok-app");
        assert_eq!(installed_app(&paths, Some(running.clone())), running);

        let beside = dir.path().join("brew/bin/rhebok-app");
        place(&beside);
        assert_eq!(installed_app(&paths, Some(running.clone())), beside);

        let installed = dir.path().join("bin/rhebok-app");
        place(&installed);
        assert_eq!(installed_app(&paths, Some(running)), installed);
    }

    #[test]
    fn a_relaunched_app_that_keeps_running_is_reported_by_its_pid() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("db.log");
        let pid = relaunch(Path::new("/bin/sleep"), &["30".into()], &log).unwrap();
        let alive = rhebok::index::pid_is_alive(pid);
        kill(pid);
        assert!(alive);
    }

    #[test]
    fn a_relaunched_app_that_exits_at_once_is_an_error_naming_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("db.log");
        let err = relaunch(Path::new("/usr/bin/false"), &[], &log).unwrap_err();
        assert!(err.contains("exited"), "{err}");
        assert!(err.contains(&log.display().to_string()), "{err}");

        let missing = dir.path().join("no-such-app");
        let err = relaunch(&missing, &[], &log).unwrap_err();
        assert!(err.contains("could not start"), "{err}");
    }
}

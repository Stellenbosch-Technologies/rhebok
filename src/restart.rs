use crate::index::{pid_is_alive, IndexLock};
use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const STOP_GRACE: Duration = Duration::from_secs(10);
const KILL_GRACE: Duration = Duration::from_secs(5);
const START_GRACE: Duration = Duration::from_secs(15);

#[derive(serde::Serialize, serde::Deserialize)]
struct DashboardRecord {
    port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningDashboard {
    pub pid: u32,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockHolder {
    Index { pid: u32, args: Vec<String> },
    Other { pid: u32, args: Vec<String> },
}

fn record_prefix(db: &Path) -> Option<String> {
    Some(format!("{}.dashboard.", db.file_name()?.to_str()?))
}

fn record_path(db: &Path, pid: u32) -> PathBuf {
    db.with_extension(format!("dashboard.{pid}"))
}

pub fn announce_dashboard(db: &Path, port: u16) -> Result<()> {
    running_dashboards(db);
    let path = record_path(db, std::process::id());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(&DashboardRecord { port })?;
    crate::config::edit::replace_file(&path, &bytes, None)
}

pub fn running_dashboards(db: &Path) -> Vec<RunningDashboard> {
    let (Some(dir), Some(prefix)) = (db.parent(), record_prefix(db)) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|n| n.strip_prefix(&prefix))
            .filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|digits| digits.parse::<u32>().ok())
        else {
            continue;
        };
        let record = std::fs::read(entry.path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<DashboardRecord>(&bytes).ok());
        let argv = pid_is_alive(pid).then(|| argv_of(pid));
        match (record, argv) {
            (Some(r), Some(Some(a))) if runs(&a, "dashboard") => {
                found.push(RunningDashboard { pid, port: r.port })
            }
            (_, Some(None)) => {}
            _ => {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    found.sort_by_key(|d| d.pid);
    found
}

pub fn lock_holder(db: &Path) -> Option<LockHolder> {
    let pid = IndexLock::holder(db)?;
    let argv = argv_of(pid).unwrap_or_default();
    let is_index = runs(&argv, "index");
    let args = argv.into_iter().skip(1).collect();
    Some(if is_index {
        LockHolder::Index { pid, args }
    } else {
        LockHolder::Other { pid, args }
    })
}

fn runs(argv: &[String], subcommand: &str) -> bool {
    let named_rhebok = argv
        .first()
        .and_then(|exe| Path::new(exe).file_name())
        .is_some_and(|name| name == "rhebok");
    named_rhebok && argv.get(1).map(String::as_str) == Some(subcommand)
}

pub fn binary(paths: &crate::setup::Paths) -> Result<PathBuf> {
    if paths.bin.is_file() {
        return Ok(paths.bin.clone());
    }
    std::env::current_exe().context("could not find the rhebok binary to start")
}

pub fn restart(paths: &crate::setup::Paths, out: &mut impl Write) -> Result<()> {
    if let Some(update) = crate::update::Status::running(&paths.update_status) {
        bail!(
            "an update is running ({}); restart once it finishes",
            update.phase
        );
    }
    let db = &paths.db;
    let dashboards = running_dashboards(db);
    let holder = lock_holder(db);
    if dashboards.is_empty() && holder.is_none() {
        writeln!(out, "nothing to restart: no dashboard or index is running")?;
        return Ok(());
    }
    let bin = binary(paths)?;
    let log = db.with_extension("log");

    for d in &dashboards {
        stop(d.pid).with_context(|| format!("could not stop the dashboard (pid {})", d.pid))?;
        let _ = std::fs::remove_file(record_path(db, d.pid));
        writeln!(
            out,
            "stopped the dashboard on http://127.0.0.1:{} (pid {})",
            d.port, d.pid
        )?;
    }

    match &holder {
        Some(LockHolder::Index { pid, args }) => {
            stop(*pid).with_context(|| format!("could not stop the index (pid {pid})"))?;
            recover_interrupted_swap(db)?;
            writeln!(
                out,
                "stopped `rhebok {}` (pid {pid}); the live index is unchanged, and what this run had not yet published is discarded",
                args.join(" ")
            )?;
            let child = spawn_detached(&bin, args, &log)?;
            crate::dashboard::core::index::hand_over(db, *pid, child.id());
            writeln!(
                out,
                "started `rhebok {}` again (pid {}), logging to {}",
                args.join(" "),
                child.id(),
                log.display()
            )?;
        }
        Some(LockHolder::Other { pid, args }) => {
            let what = if args.is_empty() {
                "the process holding the index lock".to_string()
            } else {
                format!("`rhebok {}`", args.join(" "))
            };
            writeln!(
                out,
                "left {what} (pid {pid}) running: restart stops only an index and the dashboard"
            )?;
        }
        None => {}
    }

    let mut failed = Vec::new();
    for d in &dashboards {
        let port = d.port.to_string();
        let args = ["dashboard", "--port", port.as_str(), "--no-open"].map(String::from);
        let child = spawn_detached(&bin, &args, &log)?;
        match wait_until_serving(child, db) {
            Ok(pid) => writeln!(
                out,
                "started the dashboard again on http://127.0.0.1:{} (pid {pid}), logging to {}",
                d.port,
                log.display()
            )?,
            Err(why) => failed.push(format!("port {}: {why}", d.port)),
        }
    }
    if !failed.is_empty() {
        bail!(
            "the dashboard did not start again ({}); see {}",
            failed.join("; "),
            log.display()
        );
    }
    Ok(())
}

fn recover_interrupted_swap(db: &Path) -> Result<()> {
    let old = db.with_extension("old");
    if !db.exists() && old.is_dir() {
        std::fs::rename(&old, db).with_context(|| {
            format!(
                "the index was stopped mid-swap; move {} back to {} by hand",
                old.display(),
                db.display()
            )
        })?;
    }
    Ok(())
}

fn spawn_detached(bin: &Path, args: &[String], log: &Path) -> Result<std::process::Child> {
    let sink = || {
        crate::hook::open_log_for_append(log)
            .map(std::process::Stdio::from)
            .unwrap_or_else(|_| std::process::Stdio::null())
    };
    let mut command = std::process::Command::new(bin);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(sink())
        .stderr(sink());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    command
        .spawn()
        .with_context(|| format!("could not start {}", bin.display()))
}

fn wait_until_serving(mut child: std::process::Child, db: &Path) -> Result<u32, String> {
    let announced = record_path(db, child.id());
    let deadline = Instant::now() + START_GRACE;
    while Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("it exited with {status}"));
        }
        if announced.is_file() {
            return Ok(child.id());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(format!(
        "it was not serving after {}s",
        START_GRACE.as_secs()
    ))
}

fn stop(pid: u32) -> Result<()> {
    signal(pid, Signal::Terminate);
    if wait_for_exit(pid, STOP_GRACE) {
        return Ok(());
    }
    signal(pid, Signal::Kill);
    if wait_for_exit(pid, KILL_GRACE) {
        return Ok(());
    }
    bail!(
        "it was still running {}s after SIGKILL",
        KILL_GRACE.as_secs()
    )
}

fn wait_for_exit(pid: u32, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if !pid_is_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    !pid_is_alive(pid)
}

enum Signal {
    Terminate,
    Kill,
}

#[cfg(unix)]
fn signal(pid: u32, which: Signal) {
    let sig = match which {
        Signal::Terminate => libc::SIGTERM,
        Signal::Kill => libc::SIGKILL,
    };
    unsafe {
        libc::kill(pid as libc::pid_t, sig);
    }
}

#[cfg(not(unix))]
fn signal(_pid: u32, _which: Signal) {}

#[cfg(target_os = "linux")]
pub fn argv_of(pid: u32) -> Option<Vec<String>> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let args: Vec<String> = raw
        .split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    (!args.is_empty()).then_some(args)
}

#[cfg(target_os = "macos")]
pub fn argv_of(pid: u32) -> Option<Vec<String>> {
    let mut argmax: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>();
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    let read_max = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            (&mut argmax as *mut libc::c_int).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if read_max != 0 || argmax <= 0 {
        return None;
    }
    let mut buf = vec![0u8; argmax as usize];
    let mut size = buf.len();
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
    let read_args = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if read_args != 0 {
        return None;
    }
    buf.truncate(size);
    parse_procargs2(&buf)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn argv_of(_pid: u32) -> Option<Vec<String>> {
    None
}

pub fn parse_procargs2(buf: &[u8]) -> Option<Vec<String>> {
    let argc = i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?);
    let argc = usize::try_from(argc).ok().filter(|n| *n > 0)?;
    let rest = &buf[4..];
    let exec_path_end = rest.iter().position(|b| *b == 0)?;
    let mut rest = &rest[exec_path_end..];
    while rest.first() == Some(&0) {
        rest = &rest[1..];
    }
    let args: Vec<String> = rest
        .split(|b| *b == 0)
        .take(argc)
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    (args.len() == argc).then_some(args)
}

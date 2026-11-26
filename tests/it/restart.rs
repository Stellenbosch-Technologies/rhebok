use crate::common;

use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    fn new(ollama: &str) -> Sandbox {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::create_dir_all(dir.path().join("home")).unwrap();
        for i in 0..3 {
            std::fs::write(
                notes.join(format!("n{i}.md")),
                format!("# Note {i}\n\nrestart {i}\n"),
            )
            .unwrap();
        }
        std::fs::write(
            dir.path().join("config.toml"),
            format!(
                "index_transcripts = false\nsources = [\"{}\"]\n\n[embed]\nollama_url = \"{ollama}\"\n",
                notes.display()
            ),
        )
        .unwrap();
        Sandbox { dir }
    }

    fn db(&self) -> PathBuf {
        self.dir.path().join("data/db")
    }

    fn rhebok(&self) -> Command {
        let mut c = Command::new(assert_cmd::cargo::cargo_bin("rhebok"));
        c.env("RHEBOK_DB", self.db())
            .env("RHEBOK_CONFIG", self.dir.path().join("config.toml"))
            .env("HOME", self.dir.path().join("home"))
            .env("CODEX_HOME", self.dir.path().join("home/.codex"))
            .stdin(Stdio::null());
        c
    }

    fn restart(&self) -> (bool, String) {
        let out = self.rhebok().arg("restart").output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.success(), text)
    }
}

fn reaped(mut child: std::process::Child) -> u32 {
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    pid
}

fn alive(pid: u32) -> bool {
    rhebok::index::pid_is_alive(pid)
}

fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if ready() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

struct KillOnDrop(u32);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        kill(self.0);
    }
}

fn kill(pid: u32) {
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
}

fn version_pid(port: u16) -> Option<u32> {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
    let request =
        format!("GET /api/version HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut text = String::new();
    stream.read_to_string(&mut text).ok()?;
    let body = &text[text.find("\r\n\r\n")? + 4..];
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v["pid"].as_u64().map(|p| p as u32)
}

fn start_dashboard(sandbox: &Sandbox) -> (u32, u16) {
    let mut child = sandbox
        .rhebok()
        .args(["dashboard", "--port", "0", "--no-open"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    std::io::BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let port: u16 = line
        .trim()
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("no port in {line:?}"));
    (reaped(child), port)
}

#[test]
fn with_nothing_running_restart_says_so_and_succeeds() {
    let sandbox = Sandbox::new(&common::fake_ollama());
    let (ok, out) = sandbox.restart();
    assert!(ok, "{out}");
    assert!(out.contains("nothing to restart"), "{out}");
}

#[test]
fn restart_brings_the_dashboard_back_on_the_same_port_as_a_new_process() {
    let sandbox = Sandbox::new(&common::fake_ollama());
    let (old, port) = start_dashboard(&sandbox);
    let _old = KillOnDrop(old);
    wait_for("the dashboard to answer", || version_pid(port) == Some(old));

    let (ok, out) = sandbox.restart();
    assert!(ok, "{out}");
    assert!(
        out.contains(&format!(
            "stopped the dashboard on http://127.0.0.1:{port} (pid {old})"
        )),
        "{out}"
    );

    let new = version_pid(port);
    if let Some(pid) = new {
        kill(pid);
    }
    assert!(
        !alive(old),
        "the old dashboard (pid {old}) is still running:\n{out}"
    );
    let new = new.unwrap_or_else(|| panic!("nothing answers on port {port} after restart:\n{out}"));
    assert_ne!(new, old, "{out}");
    assert!(
        out.contains(&format!(
            "started the dashboard again on http://127.0.0.1:{port} (pid {new})"
        )),
        "{out}"
    );
}

#[test]
fn restart_stops_a_running_index_and_starts_it_again_with_the_same_arguments() {
    let sandbox = Sandbox::new(&common::fake_ollama_delayed(Duration::from_secs(60)));
    let db = sandbox.db();
    let old = reaped(
        sandbox
            .rhebok()
            .args(["index", "--reindex"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let _old = KillOnDrop(old);
    wait_for("the index to take the lock", || {
        rhebok::index::IndexLock::holder(&db) == Some(old)
    });

    let (ok, out) = sandbox.restart();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut new = None;
    while new.is_none() && Instant::now() < deadline {
        new = rhebok::index::IndexLock::holder(&db).filter(|pid| *pid != old);
        std::thread::sleep(Duration::from_millis(50));
    }
    let argv = new.and_then(rhebok::restart::argv_of);
    if let Some(pid) = new {
        kill(pid);
    }
    assert!(ok, "{out}");
    assert!(
        !alive(old),
        "the old index (pid {old}) is still running:\n{out}"
    );
    let new = new.unwrap_or_else(|| panic!("no index holds the lock after restart:\n{out}"));
    assert_ne!(new, old, "{out}");
    let argv = argv.unwrap_or_else(|| panic!("could not read the arguments of pid {new}:\n{out}"));
    assert_eq!(&argv[1..], ["index", "--reindex"], "{out}");
    assert!(
        out.contains(&format!(
            "started `rhebok index --reindex` again (pid {new})"
        )),
        "{out}"
    );
}

#[test]
fn restart_leaves_a_lock_holder_that_is_not_an_index_running() {
    let sandbox = Sandbox::new(&common::fake_ollama());
    let db = sandbox.db();
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let other = reaped(Command::new("sleep").arg("30").spawn().unwrap());
    let _other = KillOnDrop(other);
    std::fs::write(db.with_extension("lock"), other.to_string()).unwrap();

    let (ok, out) = sandbox.restart();
    let still = alive(other);
    assert!(ok, "{out}");
    assert!(
        still,
        "restart stopped a process that was not an index:\n{out}"
    );
    assert!(out.contains(&format!("(pid {other}) running")), "{out}");
}

#[test]
fn a_dashboard_record_whose_process_is_gone_is_swept_and_not_restarted() {
    let sandbox = Sandbox::new(&common::fake_ollama());
    let db = sandbox.db();
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let mut gone = Command::new("true").spawn().unwrap();
    let pid = gone.id();
    gone.wait().unwrap();
    let record = db.with_extension(format!("dashboard.{pid}"));
    std::fs::write(&record, r#"{"port":1}"#).unwrap();

    assert!(rhebok::restart::running_dashboards(&db).is_empty());
    assert!(!record.exists(), "the stale record was left behind");
}

#[test]
fn a_dashboard_record_naming_a_process_that_is_not_a_dashboard_is_ignored() {
    let sandbox = Sandbox::new(&common::fake_ollama());
    let db = sandbox.db();
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let other = reaped(Command::new("sleep").arg("30").spawn().unwrap());
    let _other = KillOnDrop(other);
    std::fs::write(
        db.with_extension(format!("dashboard.{other}")),
        r#"{"port":1}"#,
    )
    .unwrap();

    let (ok, out) = sandbox.restart();
    let still = alive(other);
    assert!(ok, "{out}");
    assert!(
        still,
        "restart stopped a process that was not a dashboard:\n{out}"
    );
    assert!(out.contains("nothing to restart"), "{out}");
}

#[test]
fn argv_of_reads_a_live_process_arguments_including_spaces() {
    let child = Command::new("/bin/sh")
        .args(["-c", "sleep 30; true", "with space"])
        .spawn()
        .unwrap();
    let pid = reaped(child);
    let _cleanup = KillOnDrop(pid);
    let mut argv = None;
    wait_for("the shell's arguments to be readable", || {
        argv = rhebok::restart::argv_of(pid);
        argv.is_some()
    });
    assert_eq!(
        argv.unwrap(),
        ["/bin/sh", "-c", "sleep 30; true", "with space"]
    );
}

#[test]
fn procargs2_parsing_skips_the_exec_path_and_its_padding() {
    let mut buf = 2i32.to_ne_bytes().to_vec();
    buf.extend_from_slice(b"/opt/bin/rhebok\0\0\0\0rhebok\0dashboard\0HOME=/x\0");
    assert_eq!(
        rhebok::restart::parse_procargs2(&buf).unwrap(),
        ["rhebok", "dashboard"]
    );
    assert_eq!(rhebok::restart::parse_procargs2(&buf[..10]), None);
}

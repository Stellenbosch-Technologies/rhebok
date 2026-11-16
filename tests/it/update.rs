use rhebok::setup::Paths;
use rhebok::update::download::{extract_single, verify_sha256};
use rhebok::update::release::parse_release;
use rhebok::update::{
    check, check_decision, run, CheckDecision, Outcome, Status, UpdateCheck, UpdateOpts,
};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const TARGET: &str = "test-target";
const H: u64 = 3600;

fn mirror(latest_version: &str, files: Vec<(String, Vec<u8>)>) -> String {
    let files = Arc::new(Mutex::new(files));
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let latest = latest_version.to_string();
    let base2 = base.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let mut buf = vec![0u8; 8192];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
            let (status, ctype, body): (&str, &str, Vec<u8>) = if path == "/releases/latest" {
                let assets: Vec<serde_json::Value> = files
                    .lock()
                    .unwrap()
                    .iter()
                    .map(
                        |(n, _)| serde_json::json!({ "name": n, "url": format!("{base2}/dl/{n}") }),
                    )
                    .collect();
                let v = serde_json::json!({ "tag_name": format!("v{latest}"), "html_url": format!("{base2}/tag/v{latest}"), "assets": assets });
                ("200 OK", "application/json", v.to_string().into_bytes())
            } else if let Some(name) = path.strip_prefix("/dl/") {
                match files.lock().unwrap().iter().find(|(n, _)| n == name) {
                    Some((_, b)) => ("200 OK", "application/octet-stream", b.clone()),
                    None => ("404 Not Found", "text/plain", b"no".to_vec()),
                }
            } else {
                ("404 Not Found", "text/plain", b"no".to_vec())
            };
            let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = s.write_all(&body);
        }
    });
    base
}

fn tarball(version: &str, record: &Path) -> Vec<u8> {
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'rhebok {version}'; exit 0; fi\necho \"$*\" >> \"{}\"\nexit 0\n",
        record.display()
    );
    let mut header = tar::Header::new_gnu();
    header.set_size(script.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    let mut out = Vec::new();
    {
        let enc = flate2::write::GzEncoder::new(&mut out, flate2::Compression::default());
        let mut ar = tar::Builder::new(enc);
        ar.append_data(&mut header, "rhebok", script.as_bytes())
            .unwrap();
        ar.into_inner().unwrap().finish().unwrap();
    }
    out
}

fn sha_line(bytes: &[u8], name: &str) -> Vec<u8> {
    use sha2::Digest;
    format!("{}  {name}\n", hex::encode(sha2::Sha256::digest(bytes))).into_bytes()
}

fn assets(version: &str, record: &Path) -> Vec<(String, Vec<u8>)> {
    let tgz = tarball(version, record);
    let name = format!("rhebok-{TARGET}.tar.gz");
    let sha = sha_line(&tgz, &name);
    vec![(name, tgz), (format!("rhebok-{TARGET}.sha256"), sha)]
}

struct Fx {
    _t: tempfile::TempDir,
    paths: Paths,
    record: PathBuf,
}

fn fixture(installed_bin_version: &str) -> Fx {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("root");
    let paths = Paths::at(&root, vec![t.path().join("link")], t.path().join("cache"));
    std::fs::create_dir_all(paths.bin.parent().unwrap()).unwrap();
    std::fs::write(
        &paths.bin,
        format!("#!/bin/sh\necho 'rhebok {installed_bin_version}'\n"),
    )
    .unwrap();
    std::fs::set_permissions(&paths.bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    Fx {
        record: t.path().join("record"),
        _t: t,
        paths,
    }
}

fn opts(fx: &Fx, api: &str, check_only: bool) -> UpdateOpts {
    UpdateOpts {
        paths: fx.paths.clone(),
        installed: "1.0.0".to_string(),
        api: api.to_string(),
        token: None,
        target: TARGET.to_string(),
        check_only,
    }
}

#[test]
fn a_release_tag_parses_and_a_non_semver_tag_is_an_error() {
    let v = serde_json::json!({ "tag_name": "v1.2.3", "html_url": "u", "assets": [{ "name": "a", "url": "b" }] });
    let r = parse_release(&v).unwrap();
    assert_eq!(r.version.to_string(), "1.2.3");
    assert_eq!(r.assets[0].url, "b");
    let bad = serde_json::json!({ "tag_name": "nightly", "assets": [] });
    assert!(parse_release(&bad)
        .unwrap_err()
        .to_string()
        .contains("nightly"));
}

fn chk(installed: &str, latest: Option<&str>, checked_at: u64, error: Option<&str>) -> UpdateCheck {
    UpdateCheck {
        installed: installed.into(),
        latest: latest.map(str::to_string),
        url: None,
        checked_at,
        error: error.map(str::to_string),
    }
}

#[test]
fn the_check_decision_is_24h_after_success_1h_after_failure_and_never_when_disabled() {
    let now = 100 * H;
    assert!(matches!(
        check_decision(None, now, true),
        CheckDecision::Check
    ));
    assert!(matches!(
        check_decision(Some(&chk("1", Some("1"), now - 23 * H, None)), now, true),
        CheckDecision::Skip(_)
    ));
    assert!(matches!(
        check_decision(Some(&chk("1", Some("1"), now - 25 * H, None)), now, true),
        CheckDecision::Check
    ));
    assert!(matches!(
        check_decision(Some(&chk("1", None, now - H / 2, Some("boom"))), now, true),
        CheckDecision::Skip(_)
    ));
    assert!(matches!(
        check_decision(Some(&chk("1", None, now - 2 * H, Some("boom"))), now, true),
        CheckDecision::Check
    ));
    assert!(matches!(
        check_decision(None, now, false),
        CheckDecision::Skip(_)
    ));
}

#[test]
fn available_compares_as_semver_not_as_text() {
    assert_eq!(
        chk("0.9.1", Some("0.10.0"), 0, None).available().as_deref(),
        Some("0.10.0")
    );
    assert_eq!(chk("1.0.0", Some("1.0.0"), 0, None).available(), None);
    assert_eq!(chk("1.0.0", Some("0.9.9"), 0, None).available(), None);
}

#[test]
fn a_flipped_byte_fails_the_checksum_with_both_digests() {
    let t = tempfile::tempdir().unwrap();
    let mut bytes = b"hello release".to_vec();
    let sha = sha_line(&bytes, "x.tar.gz");
    std::fs::write(t.path().join("x.sha256"), &sha).unwrap();
    std::fs::write(t.path().join("x.tar.gz"), &bytes).unwrap();
    verify_sha256(&t.path().join("x.tar.gz"), &t.path().join("x.sha256")).unwrap();
    bytes[0] ^= 1;
    std::fs::write(t.path().join("x.tar.gz"), &bytes).unwrap();
    let err = verify_sha256(&t.path().join("x.tar.gz"), &t.path().join("x.sha256"))
        .unwrap_err()
        .to_string();
    let expected = String::from_utf8(sha).unwrap();
    assert!(
        err.contains(expected.split_whitespace().next().unwrap()),
        "{err}"
    );
    assert_eq!(
        err.split_whitespace().filter(|w| w.len() == 64).count(),
        2,
        "{err}"
    );
}

struct Member {
    name: &'static str,
    kind: tar::EntryType,
    body: &'static [u8],
    link: Option<&'static str>,
}

fn file(name: &'static str, body: &'static [u8]) -> Member {
    Member {
        name,
        kind: tar::EntryType::Regular,
        body,
        link: None,
    }
}

fn archive_of(dir: &Path, label: &str, members: &[Member]) -> PathBuf {
    let mut bytes = Vec::new();
    {
        let enc = flate2::write::GzEncoder::new(&mut bytes, flate2::Compression::default());
        let mut ar = tar::Builder::new(enc);
        for m in members {
            let mut h = tar::Header::new_gnu();
            let raw = &mut h.as_gnu_mut().unwrap().name;
            raw[..m.name.len()].copy_from_slice(m.name.as_bytes());
            h.set_entry_type(m.kind);
            h.set_size(m.body.len() as u64);
            h.set_mode(0o644);
            if let Some(link) = m.link {
                h.as_gnu_mut().unwrap().linkname[..link.len()].copy_from_slice(link.as_bytes());
            }
            h.set_cksum();
            ar.append(&h, m.body).unwrap();
        }
        ar.into_inner().unwrap().finish().unwrap();
    }
    let at = dir.join(format!("{label}.tgz"));
    std::fs::write(&at, bytes).unwrap();
    at
}

fn unpacked(dir: &Path) -> Vec<std::ffi::OsString> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect()
}

#[test]
fn extraction_accepts_exactly_one_file_named_rhebok() {
    let t = tempfile::tempdir().unwrap();
    let good = tarball("2.0.0", &t.path().join("r"));
    std::fs::write(t.path().join("good.tgz"), &good).unwrap();
    let bin = extract_single(&t.path().join("good.tgz"), &t.path().join("out")).unwrap();
    assert!(bin.ends_with("rhebok"));
    assert!(bin.metadata().unwrap().permissions().mode() & 0o111 != 0);

    let two = archive_of(
        t.path(),
        "two",
        &[file("rhebok", b"one"), file("rhebok", b"two")],
    );
    let err = extract_single(&two, &t.path().join("out2"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("more than one file named rhebok"), "{err}");

    let none = archive_of(t.path(), "none", &[file("README", b"readme")]);
    let err = extract_single(&none, &t.path().join("out3"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("no file named rhebok"), "{err}");
}

#[test]
fn extraction_skips_the_licence_and_notices_packed_beside_the_binary() {
    let t = tempfile::tempdir().unwrap();
    let packed = archive_of(
        t.path(),
        "packed",
        &[
            file("LICENSE", b"licence"),
            file("rhebok", b"binary"),
            file("THIRD-PARTY-NOTICES.md", b"notices"),
        ],
    );
    let bin = extract_single(&packed, &t.path().join("out")).unwrap();
    assert_eq!(std::fs::read(&bin).unwrap(), b"binary");
    assert_eq!(unpacked(&t.path().join("out")), ["rhebok"]);
}

#[test]
fn extraction_ignores_a_root_file_a_later_release_adds_beside_the_binary() {
    let t = tempfile::tempdir().unwrap();
    let later = archive_of(
        t.path(),
        "later",
        &[
            Member {
                name: "pax_global_header",
                kind: tar::EntryType::XGlobalHeader,
                body: b"15 comment=abc\n",
                link: None,
            },
            file("README.md", b"readme"),
            file("./rhebok", b"binary"),
            file("LICENSE", b"licence"),
            file("_rhebok.zsh", b"completion"),
        ],
    );
    let bin = extract_single(&later, &t.path().join("out")).unwrap();
    assert_eq!(std::fs::read(&bin).unwrap(), b"binary");
    assert_eq!(unpacked(&t.path().join("out")), ["rhebok"]);
}

#[test]
fn extraction_refuses_any_entry_that_is_not_a_regular_file_at_the_archive_root() {
    let t = tempfile::tempdir().unwrap();
    let refused = [
        file("bin/rhebok", b"binary"),
        file("docs/README.md", b"readme"),
        file("/rhebok", b"binary"),
        file("../rhebok", b"binary"),
        file("../README.md", b"readme"),
        Member {
            name: "rhebok-link",
            kind: tar::EntryType::Symlink,
            body: b"",
            link: Some("/bin/sh"),
        },
        Member {
            name: "notes",
            kind: tar::EntryType::Link,
            body: b"",
            link: Some("rhebok"),
        },
        Member {
            name: "share/",
            kind: tar::EntryType::Directory,
            body: b"",
            link: None,
        },
        Member {
            name: "fifo",
            kind: tar::EntryType::Fifo,
            body: b"",
            link: None,
        },
    ];
    for (i, bad) in refused.into_iter().enumerate() {
        let name = bad.name;
        let at = archive_of(
            t.path(),
            &format!("bad{i}"),
            &[file("rhebok", b"binary"), bad],
        );
        let out = t.path().join(format!("out{i}"));
        let err = extract_single(&at, &out)
            .err()
            .unwrap_or_else(|| panic!("`{name}` was accepted"))
            .to_string();
        assert!(err.contains(name), "`{name}`: {err}");
    }
}

#[test]
fn up_to_date_writes_the_check_and_finishes_the_status() {
    let fx = fixture("1.0.0");
    let api = mirror("1.0.0", assets("1.0.0", &fx.record));
    match run(&opts(&fx, &api, false)).unwrap() {
        Outcome::UpToDate(v) => assert_eq!(v, "1.0.0"),
        other => panic!("{other:?}"),
    }
    let c = UpdateCheck::read(&fx.paths.update_json).unwrap();
    assert_eq!(c.latest.as_deref(), Some("1.0.0"));
    assert!(c.error.is_none());
    let s = Status::read(&fx.paths.update_status).unwrap();
    assert!(s.done && s.ok == Some(true));
    assert!(Status::running(&fx.paths.update_status).is_none());
}

#[test]
fn check_only_reports_and_downloads_nothing() {
    let fx = fixture("1.0.0");
    let api = mirror("2.0.0", assets("2.0.0", &fx.record));
    let outcome = run(&opts(&fx, &api, true)).unwrap();
    assert!(!fx.record.exists(), "the hand-over must not have run");
    match outcome {
        Outcome::CheckOnly(c) => assert_eq!(c.available().as_deref(), Some("2.0.0")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_check_writes_no_update_status_because_a_check_is_not_an_update() {
    let fx = fixture("1.0.0");
    let api = mirror("2.0.0", assets("2.0.0", &fx.record));
    std::fs::create_dir_all(&fx.paths.root).unwrap();
    let earlier = serde_json::json!({
        "pid": 1, "started_at": 7, "from": "0.9.0", "to": "1.0.0",
        "phase": "done", "message": "updated 0.9.0 -> 1.0.0", "done": true, "ok": true
    });
    std::fs::write(&fx.paths.update_status, earlier.to_string()).unwrap();

    run(&opts(&fx, &api, true)).unwrap();

    let after =
        Status::read(&fx.paths.update_status).expect("the status file must survive a check");
    assert_eq!(
        after.from, "0.9.0",
        "a check must not rewrite an update's status: {after:?}"
    );
    assert_eq!(after.to.as_deref(), Some("1.0.0"));
    assert_eq!(after.message, "updated 0.9.0 -> 1.0.0");
}

#[test]
fn a_check_on_a_machine_that_never_updated_leaves_no_status_at_all() {
    let fx = fixture("1.0.0");
    let api = mirror("2.0.0", assets("2.0.0", &fx.record));
    run(&opts(&fx, &api, true)).unwrap();
    assert!(
        !fx.paths.update_status.exists(),
        "a check must not invent an update status"
    );
}

#[test]
fn a_mislabelled_asset_is_refused_before_the_installed_binary_is_touched() {
    let fx = fixture("1.0.0");
    let api = mirror("2.0.0", assets("1.0.0", &fx.record));
    let before = std::fs::read(&fx.paths.bin).unwrap();
    let err = run(&opts(&fx, &api, false)).unwrap_err().to_string();
    assert!(err.contains("2.0.0") && err.contains("1.0.0"), "{err}");
    assert_eq!(std::fs::read(&fx.paths.bin).unwrap(), before);
    assert!(!fx.record.exists(), "the hand-over must not have run");
    let s = Status::read(&fx.paths.update_status).unwrap();
    assert!(s.done && s.ok == Some(false) && s.phase == "failed");
    assert!(
        std::fs::read_dir(&fx.paths.tmp)
            .map(|mut d| d.next().is_none())
            .unwrap_or(true),
        "staging cleaned"
    );
}

#[test]
fn a_genuine_update_hands_over_to_the_new_binary_s_install() {
    let fx = fixture("1.0.0");
    let api = mirror("2.0.0", assets("2.0.0", &fx.record));
    match run(&opts(&fx, &api, false)).unwrap() {
        Outcome::Updated { from, to } => {
            assert_eq!((from.as_str(), to.as_str()), ("1.0.0", "2.0.0"))
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(&fx.record).unwrap().trim(),
        "install --yes --quiet"
    );
    let s = Status::read(&fx.paths.update_status).unwrap();
    assert!(s.done && s.ok == Some(true) && s.to.as_deref() == Some("2.0.0"));
}

#[test]
fn a_checksum_mismatch_is_a_hard_failure() {
    let fx = fixture("1.0.0");
    let mut a = assets("2.0.0", &fx.record);
    a[1].1 = sha_line(b"other bytes", &a[0].0);
    let api = mirror("2.0.0", a);
    let err = run(&opts(&fx, &api, false)).unwrap_err().to_string();
    assert!(err.to_lowercase().contains("checksum"), "{err}");
}

#[test]
fn update_refuses_under_an_index_lock_or_a_live_update_but_not_a_dead_one() {
    let fx = fixture("1.0.0");
    let api = mirror("2.0.0", assets("2.0.0", &fx.record));
    std::fs::write(
        fx.paths.db.with_extension("lock"),
        std::process::id().to_string(),
    )
    .unwrap();
    assert!(run(&opts(&fx, &api, false))
        .unwrap_err()
        .to_string()
        .contains("index"));
    match run(&opts(&fx, &api, true)) {
        Ok(Outcome::CheckOnly(c)) => assert_eq!(c.available().as_deref(), Some("2.0.0")),
        other => panic!("--check must not be blocked by an index: {other:?}"),
    }
    std::fs::remove_file(fx.paths.db.with_extension("lock")).unwrap();

    let live = serde_json::json!({ "pid": std::process::id(), "started_at": 0, "from": "1.0.0", "to": null, "phase": "downloading", "message": "", "done": false, "ok": null });
    std::fs::write(&fx.paths.update_status, live.to_string()).unwrap();
    assert!(run(&opts(&fx, &api, true))
        .unwrap_err()
        .to_string()
        .contains("already running"));

    let dead = serde_json::json!({ "pid": 9_999_999, "started_at": 0, "from": "1.0.0", "to": null, "phase": "downloading", "message": "", "done": false, "ok": null });
    std::fs::write(&fx.paths.update_status, dead.to_string()).unwrap();
    assert!(run(&opts(&fx, &api, true)).is_ok());
}

#[test]
fn a_failed_check_records_the_error_and_keeps_the_previous_latest() {
    let fx = fixture("1.0.0");
    std::fs::create_dir_all(&fx.paths.root).unwrap();
    chk("1.0.0", Some("1.5.0"), 7, None)
        .write(&fx.paths.update_json)
        .unwrap();
    assert!(check(&opts(&fx, "http://127.0.0.1:1", true)).is_err());
    let c = UpdateCheck::read(&fx.paths.update_json).unwrap();
    assert_eq!(c.latest.as_deref(), Some("1.5.0"));
    assert!(c.error.is_some());
    assert!(c.checked_at > 7);
}

fn homebrew(fx: &Fx, upgrade_to: Option<&str>) -> Paths {
    let dir = fx.record.parent().unwrap();
    let prefix = dir.join("brew");
    let keg = |v: &str| prefix.join(format!("Cellar/rhebok/{v}/bin"));
    let stub = |v: &str| {
        format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'rhebok {v}'; exit 0; fi\necho \"$*\" >> \"{}\"\n",
            fx.record.display()
        )
    };
    std::fs::create_dir_all(keg("1.0.0")).unwrap();
    std::fs::write(keg("1.0.0").join("rhebok"), stub("1.0.0")).unwrap();
    std::fs::set_permissions(
        keg("1.0.0").join("rhebok"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::fs::create_dir_all(prefix.join("opt")).unwrap();
    std::os::unix::fs::symlink("../Cellar/rhebok/1.0.0", prefix.join("opt/rhebok")).unwrap();
    let upgrade = match upgrade_to {
        Some(v) => format!(
            "mkdir -p \"{keg}\"\ncat > \"{keg}/rhebok\" <<'EOS'\n{stub}EOS\nchmod 755 \"{keg}/rhebok\"\nln -sfn ../Cellar/rhebok/{v} \"{opt}\"\n",
            keg = keg(v).display(),
            stub = stub(v),
            opt = prefix.join("opt/rhebok").display()
        ),
        None => ":\n".to_string(),
    };
    let brew = prefix.join("bin/brew");
    std::fs::create_dir_all(brew.parent().unwrap()).unwrap();
    std::fs::write(
        &brew,
        format!(
            "#!/bin/sh\necho \"$*\" >> \"{calls}\"\nif [ \"$*\" = 'upgrade rhebok' ]; then\n{upgrade}fi\n",
            calls = dir.join("brew-calls").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&brew, std::fs::Permissions::from_mode(0o755)).unwrap();
    fx.paths.clone().installed_by_homebrew(&prefix)
}

#[test]
fn a_homebrew_install_updates_through_brew_then_runs_the_new_install() {
    let fx = fixture("1.0.0");
    let api = mirror("2.0.0", assets("2.0.0", &fx.record));
    let opts = UpdateOpts {
        paths: homebrew(&fx, Some("2.0.0")),
        ..opts(&fx, &api, false)
    };
    match run(&opts).unwrap() {
        Outcome::Updated { from, to } => {
            assert_eq!((from.as_str(), to.as_str()), ("1.0.0", "2.0.0"))
        }
        other => panic!("{other:?}"),
    }
    let calls = std::fs::read_to_string(fx.record.parent().unwrap().join("brew-calls")).unwrap();
    assert_eq!(calls, "update\nupgrade rhebok\n");
    assert_eq!(
        std::fs::read_to_string(&fx.record).unwrap().trim(),
        "install --yes --quiet",
        "the new version refreshes the plugin"
    );
    assert!(
        std::fs::read_dir(&fx.paths.tmp)
            .map(|mut d| d.next().is_none())
            .unwrap_or(true),
        "Homebrew downloads the release, not rhebok"
    );
    let s = Status::read(&fx.paths.update_status).unwrap();
    assert!(s.done && s.ok == Some(true) && s.to.as_deref() == Some("2.0.0"));
}

#[test]
fn when_homebrew_has_not_caught_up_with_the_release_the_update_says_so() {
    let fx = fixture("1.0.0");
    let api = mirror("2.0.0", assets("2.0.0", &fx.record));
    let opts = UpdateOpts {
        paths: homebrew(&fx, None),
        ..opts(&fx, &api, false)
    };
    let err = run(&opts).unwrap_err().to_string();
    assert!(
        err.contains("rhebok 1.0.0")
            && err.contains("rhebok 2.0.0")
            && err.contains("try again later"),
        "{err}"
    );
    assert!(!fx.record.exists(), "no install ran");
    let s = Status::read(&fx.paths.update_status).unwrap();
    assert!(s.done && s.ok == Some(false));
}

fn answering(status_line: &'static str) -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let mut buf = vec![0u8; 8192];
            let _ = s.read(&mut buf);
            let body = br#"{"message":"refused"}"#;
            let _ = write!(
                s,
                "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(body);
        }
    });
    base
}

fn check_error(status_line: &'static str, token: Option<&str>) -> String {
    let fx = fixture("1.0.0");
    std::fs::create_dir_all(&fx.paths.root).unwrap();
    let mut o = opts(&fx, &answering(status_line), true);
    o.token = token.map(str::to_string);
    let err = format!("{:#}", check(&o).unwrap_err());
    let recorded = UpdateCheck::read(&fx.paths.update_json).unwrap().error;
    assert!(
        recorded.as_deref().is_some_and(|r| err.contains(r)),
        "the error the dashboard reads must be the one the CLI printed: {recorded:?} vs {err}"
    );
    assert!(
        !err.to_lowercase().contains("private"),
        "no refusal may tell a user the repository is private: {err}"
    );
    err
}

#[test]
fn a_missing_release_is_a_successful_check_that_waits_the_normal_interval() {
    for token in [None, Some("ghp_test")] {
        let fx = fixture("1.0.0");
        std::fs::create_dir_all(&fx.paths.root).unwrap();
        chk("1.0.0", Some("1.5.0"), 7, None)
            .write(&fx.paths.update_json)
            .unwrap();
        let mut o = opts(&fx, &answering("404 Not Found"), true);
        o.token = token.map(str::to_string);

        let record = check(&o).expect("a 404 means nothing is published, which is not a failure");
        assert!(record.no_release_published(), "{record:?}");
        let saved = UpdateCheck::read(&fx.paths.update_json).unwrap();
        assert!(
            saved.error.is_none(),
            "a missing release must not be saved as a failed check: {saved:?}"
        );
        assert!(
            saved.latest.is_none(),
            "a release that is gone must not still be offered: {saved:?}"
        );
        assert_eq!(saved.available(), None);

        let two_hours_later = saved.checked_at + 2 * H;
        assert!(
            matches!(
                check_decision(Some(&saved), two_hours_later, true),
                CheckDecision::Skip(_)
            ),
            "a missing release must wait CHECK_EVERY, not the shorter retry after a failure"
        );
        assert!(matches!(
            check_decision(Some(&saved), saved.checked_at + 25 * H, true),
            CheckDecision::Check
        ));
    }
}

#[test]
fn an_update_with_no_release_published_says_so_and_installs_nothing() {
    let fx = fixture("1.0.0");
    std::fs::create_dir_all(&fx.paths.root).unwrap();
    let api = answering("404 Not Found");
    match run(&opts(&fx, &api, false)).unwrap() {
        Outcome::NoReleasePublished(v) => assert_eq!(v, "1.0.0"),
        other => panic!("{other:?}"),
    }
    assert!(!fx.record.exists(), "the hand-over must not have run");
    let s = Status::read(&fx.paths.update_status).unwrap();
    assert!(s.done && s.ok == Some(true), "{s:?}");
    assert!(
        s.message
            .contains("no rhebok release has been published yet"),
        "{s:?}"
    );

    match run(&opts(&fx, &api, true)).unwrap() {
        Outcome::CheckOnly(c) => assert!(c.no_release_published(), "{c:?}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_refusal_from_github_blames_the_rate_limit_or_the_token_and_calls_the_token_optional() {
    for status_line in ["403 Forbidden", "429 Too Many Requests", "401 Unauthorized"] {
        let err = check_error(status_line, None);
        assert!(err.contains("GitHub refused the request"), "{err}");
        assert!(err.contains("rate limit"), "{err}");
        assert!(
            err.contains("RHEBOK_GITHUB_TOKEN is optional"),
            "the token must be offered as optional, never as required: {err}"
        );
    }

    let err = check_error("403 Forbidden", Some("ghp_test"));
    assert!(err.contains("GitHub refused the request"), "{err}");
    assert!(
        err.contains("rate limit for the token in RHEBOK_GITHUB_TOKEN"),
        "{err}"
    );

    let err = check_error("401 Unauthorized", Some("ghp_test"));
    assert!(
        err.contains("did not accept the token in RHEBOK_GITHUB_TOKEN"),
        "{err}"
    );

    let err = check_error("500 Internal Server Error", None);
    assert!(err.contains("HTTP 500"), "{err}");
    assert!(!err.contains("GitHub refused"), "{err}");
}

#[test]
fn no_refusal_calls_the_release_repository_private_and_none_requires_a_token() {
    use reqwest::StatusCode;
    for status in [
        StatusCode::UNAUTHORIZED,
        StatusCode::FORBIDDEN,
        StatusCode::NOT_FOUND,
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::BAD_GATEWAY,
    ] {
        for token in [None, Some("ghp_test")] {
            let msg = rhebok::update::release::refusal("https://api/x", status, token).to_string();
            assert!(!msg.to_lowercase().contains("private"), "{msg}");
            assert!(!msg.contains("create a"), "{msg}");
        }
    }
    let msg =
        rhebok::update::release::refusal("https://api/x", StatusCode::FORBIDDEN, None).to_string();
    assert!(msg.contains("RHEBOK_GITHUB_TOKEN is optional"), "{msg}");
}

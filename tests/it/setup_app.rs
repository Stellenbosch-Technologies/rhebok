use rhebok::setup::app::{
    archive_name, bundle_identifier, bundle_version, checksum_name, existing, extract_bundle,
    install_app, not_installed_message, place_bundle, uninstall_app, AppOpts, Download, Existing,
    APP_EXECUTABLE, BUNDLE_ID, BUNDLE_NAME,
};
use rhebok::setup::Paths;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tar::{EntryType, Header};

fn info_plist(id: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n\t<key>CFBundleName</key>\n\t<string>Rhebok</string>\n\t<key>CFBundleIdentifier</key>\n\t<string>{id}</string>\n</dict>\n</plist>\n"
    )
}

fn executable(version: &str) -> String {
    format!("#!/bin/sh\necho \"rhebok-app {version}\"\n")
}

struct Entry<'a> {
    path: &'a str,
    kind: EntryType,
    mode: u32,
    data: &'a [u8],
    link: Option<&'a str>,
}

fn file<'a>(path: &'a str, mode: u32, data: &'a [u8]) -> Entry<'a> {
    Entry {
        path,
        kind: EntryType::Regular,
        mode,
        data,
        link: None,
    }
}

fn folder(path: &str) -> Entry<'_> {
    Entry {
        path,
        kind: EntryType::Directory,
        mode: 0o755,
        data: b"",
        link: None,
    }
}

fn raw_archive(dir: &Path, name: &str, entries: &[Entry]) -> PathBuf {
    let out = dir.join(name);
    let gz = flate2::write::GzEncoder::new(
        std::fs::File::create(&out).unwrap(),
        flate2::Compression::fast(),
    );
    let mut b = tar::Builder::new(gz);
    for e in entries {
        let mut h = Header::new_gnu();
        let bytes = e.path.as_bytes();
        h.as_old_mut().name[..bytes.len()].copy_from_slice(bytes);
        h.set_entry_type(e.kind);
        h.set_mode(e.mode);
        h.set_size(e.data.len() as u64);
        if let Some(l) = e.link {
            let lb = l.as_bytes();
            h.as_old_mut().linkname[..lb.len()].copy_from_slice(lb);
        }
        h.set_cksum();
        b.append(&h, e.data).unwrap();
    }
    b.into_inner().unwrap().finish().unwrap();
    out
}

pub fn bundle_archive(dir: &Path, version: &str, id: &str) -> PathBuf {
    let plist = info_plist(id);
    let exe = executable(version);
    raw_archive(
        dir,
        &format!("bundle-{version}.tar.gz"),
        &[
            folder("Rhebok.app/"),
            folder("Rhebok.app/Contents/"),
            folder("Rhebok.app/Contents/MacOS/"),
            file("Rhebok.app/Contents/Info.plist", 0o644, plist.as_bytes()),
            file(
                "Rhebok.app/Contents/MacOS/rhebok-app",
                0o755,
                exe.as_bytes(),
            ),
        ],
    )
}

fn unpacked(dir: &Path, version: &str, id: &str) -> PathBuf {
    let archive = bundle_archive(dir, version, id);
    extract_bundle(&archive, &dir.join(format!("unpack-{version}"))).unwrap()
}

#[test]
fn names_follow_the_release_asset_scheme() {
    assert_eq!(BUNDLE_NAME, "Rhebok.app");
    assert_eq!(BUNDLE_ID, "com.stelltec.rhebok");
    assert_eq!(APP_EXECUTABLE, "Contents/MacOS/rhebok-app");
    assert_eq!(
        archive_name("aarch64-apple-darwin"),
        "rhebok-app-aarch64-apple-darwin.tar.gz"
    );
    assert_eq!(
        checksum_name("aarch64-apple-darwin"),
        "rhebok-app-aarch64-apple-darwin.sha256"
    );
}

#[test]
fn bundle_identifier_reads_the_id_across_whitespace() {
    let plist =
        "<dict>\n<key>CFBundleIdentifier</key>\n\t\t<string>com.stelltec.rhebok</string>\n</dict>";
    assert_eq!(
        bundle_identifier(plist).as_deref(),
        Some("com.stelltec.rhebok")
    );
    assert_eq!(
        bundle_identifier("<key>CFBundleIdentifier</key><string>a.b</string>").as_deref(),
        Some("a.b")
    );
}

#[test]
fn bundle_identifier_without_the_key_is_none() {
    assert_eq!(
        bundle_identifier("<key>CFBundleName</key><string>x</string>"),
        None
    );
    assert_eq!(bundle_identifier("<key>CFBundleIdentifier</key>"), None);
}

#[test]
fn extract_bundle_unpacks_a_bundle_with_its_modes() {
    let t = tempfile::tempdir().unwrap();
    let archive = bundle_archive(t.path(), "1.0.0", BUNDLE_ID);
    let into = t.path().join("out");
    let bundle = extract_bundle(&archive, &into).unwrap();
    assert_eq!(bundle, into.join("Rhebok.app"));
    let exe = bundle.join(APP_EXECUTABLE);
    let mode = std::fs::metadata(&exe).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755);
    assert!(std::fs::read_to_string(bundle.join("Contents/Info.plist"))
        .unwrap()
        .contains(BUNDLE_ID));
    assert_eq!(existing(&bundle), Existing::Ours);

    let plist = info_plist(BUNDLE_ID);
    let setuid = raw_archive(
        t.path(),
        "setuid.tar.gz",
        &[
            folder("Rhebok.app/"),
            folder("Rhebok.app/Contents/"),
            folder("Rhebok.app/Contents/MacOS/"),
            file("Rhebok.app/Contents/Info.plist", 0o644, plist.as_bytes()),
            file(
                "Rhebok.app/Contents/MacOS/rhebok-app",
                0o4755,
                b"#!/bin/sh\n",
            ),
        ],
    );
    let bundle = extract_bundle(&setuid, &t.path().join("setuid-out")).unwrap();
    let mode = std::fs::metadata(bundle.join(APP_EXECUTABLE))
        .unwrap()
        .permissions()
        .mode()
        & 0o7777;
    assert_eq!(mode, 0o755);
}

#[test]
fn extract_bundle_refuses_a_link() {
    let t = tempfile::tempdir().unwrap();
    let archive = raw_archive(
        t.path(),
        "a.tar.gz",
        &[Entry {
            path: "Rhebok.app/Contents/evil",
            kind: EntryType::Symlink,
            mode: 0o777,
            data: b"",
            link: Some("/etc"),
        }],
    );
    let err = extract_bundle(&archive, &t.path().join("out")).unwrap_err();
    assert!(
        format!("{err:#}").contains("Rhebok.app/Contents/evil"),
        "{err:#}"
    );
}

#[test]
fn extract_bundle_refuses_an_absolute_path() {
    let t = tempfile::tempdir().unwrap();
    let archive = raw_archive(t.path(), "a.tar.gz", &[file("/tmp/escape", 0o644, b"x")]);
    let err = extract_bundle(&archive, &t.path().join("out")).unwrap_err();
    assert!(format!("{err:#}").contains("/tmp/escape"), "{err:#}");
}

#[test]
fn extract_bundle_refuses_a_parent_component() {
    let t = tempfile::tempdir().unwrap();
    let archive = raw_archive(
        t.path(),
        "a.tar.gz",
        &[file("Rhebok.app/../escape", 0o644, b"x")],
    );
    let err = extract_bundle(&archive, &t.path().join("out")).unwrap_err();
    assert!(
        format!("{err:#}").contains("Rhebok.app/../escape"),
        "{err:#}"
    );
    assert!(!t.path().join("out/escape").exists());
}

#[test]
fn extract_bundle_refuses_an_entry_outside_the_bundle() {
    let t = tempfile::tempdir().unwrap();
    let archive = raw_archive(t.path(), "a.tar.gz", &[file("Other.app/x", 0o644, b"x")]);
    let err = extract_bundle(&archive, &t.path().join("out")).unwrap_err();
    assert!(format!("{err:#}").contains("Other.app/x"), "{err:#}");
}

#[test]
fn extract_bundle_refuses_a_bundle_without_its_executable() {
    let t = tempfile::tempdir().unwrap();
    let archive = raw_archive(
        t.path(),
        "a.tar.gz",
        &[file("Rhebok.app/Contents/Info.plist", 0o644, b"x")],
    );
    let err = extract_bundle(&archive, &t.path().join("out")).unwrap_err();
    assert!(
        format!("{err:#}").contains("Rhebok.app/Contents/MacOS/rhebok-app"),
        "{err:#}"
    );
}

#[test]
fn place_bundle_puts_a_bundle_where_none_was() {
    let t = tempfile::tempdir().unwrap();
    let staged = unpacked(t.path(), "1.0.0", BUNDLE_ID);
    let bundle = t.path().join("Applications/Rhebok.app");
    place_bundle(&staged, &bundle).unwrap();
    assert_eq!(bundle_version(&bundle).unwrap(), "1.0.0");
    assert!(!staged.exists());
}

#[test]
fn place_bundle_replaces_our_bundle_while_its_executable_is_open() {
    let t = tempfile::tempdir().unwrap();
    let bundle = t.path().join("Applications/Rhebok.app");
    place_bundle(&unpacked(t.path(), "1.0.0", BUNDLE_ID), &bundle).unwrap();
    let held = std::fs::File::open(bundle.join(APP_EXECUTABLE)).unwrap();
    place_bundle(&unpacked(t.path(), "2.0.0", BUNDLE_ID), &bundle).unwrap();
    drop(held);
    assert_eq!(bundle_version(&bundle).unwrap(), "2.0.0");
    let beside: Vec<_> = std::fs::read_dir(bundle.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(beside, vec![std::ffi::OsString::from("Rhebok.app")]);
}

#[test]
fn place_bundle_refuses_a_foreign_bundle_and_leaves_it() {
    let t = tempfile::tempdir().unwrap();
    let bundle = t.path().join("Applications/Rhebok.app");
    place_bundle(&unpacked(t.path(), "1.0.0", "com.example.other"), &bundle).unwrap();
    assert_eq!(
        existing(&bundle),
        Existing::Foreign("com.example.other".to_string())
    );
    let staged = unpacked(t.path(), "2.0.0", BUNDLE_ID);
    let err = place_bundle(&staged, &bundle).unwrap_err();
    assert!(format!("{err:#}").contains("com.example.other"), "{err:#}");
    assert_eq!(bundle_version(&bundle).unwrap(), "1.0.0");
    assert!(staged.exists());
}

#[test]
fn place_bundle_refuses_a_symlink_and_leaves_it() {
    let t = tempfile::tempdir().unwrap();
    let apps = t.path().join("Applications");
    std::fs::create_dir_all(&apps).unwrap();
    let real = unpacked(t.path(), "1.0.0", BUNDLE_ID);
    let bundle = apps.join("Rhebok.app");
    std::os::unix::fs::symlink(&real, &bundle).unwrap();
    assert_eq!(existing(&bundle), Existing::NotABundle);
    let staged = unpacked(t.path(), "2.0.0", BUNDLE_ID);
    assert!(place_bundle(&staged, &bundle).is_err());
    assert_eq!(std::fs::read_link(&bundle).unwrap(), real);
    assert!(staged.exists());
}

#[test]
fn a_missing_applications_folder_is_created() {
    let t = tempfile::tempdir().unwrap();
    let paths = Paths::at(t.path(), vec![], t.path().join("cache"));
    assert_eq!(paths.app_bundle(), t.path().join("Applications/Rhebok.app"));
    assert!(!paths.applications.exists());
    place_bundle(&unpacked(t.path(), "1.0.0", BUNDLE_ID), &paths.app_bundle()).unwrap();
    assert_eq!(bundle_version(&paths.app_bundle()).unwrap(), "1.0.0");
}

#[test]
fn leftover_staging_folders_are_removed() {
    let t = tempfile::tempdir().unwrap();
    let apps = t.path().join("Applications");
    std::fs::create_dir_all(apps.join(".Rhebok.app.123/Rhebok.app")).unwrap();
    std::fs::create_dir_all(apps.join(".Rhebok.app.old-456/Contents")).unwrap();
    std::fs::create_dir_all(apps.join("Other.app")).unwrap();
    let unpack = apps.join(".Rhebok.app.789-unpack");
    let archive = bundle_archive(t.path(), "1.0.0", BUNDLE_ID);
    let staged = extract_bundle(&archive, &unpack).unwrap();
    place_bundle(&staged, &apps.join("Rhebok.app")).unwrap();
    assert!(!apps.join(".Rhebok.app.123").exists());
    assert!(!apps.join(".Rhebok.app.old-456").exists());
    assert!(apps.join("Other.app").exists());
    assert!(unpack.exists());
}

#[test]
fn bundle_version_reads_the_executables_answer() {
    let t = tempfile::tempdir().unwrap();
    let bundle = unpacked(t.path(), "3.4.5", BUNDLE_ID);
    assert_eq!(bundle_version(&bundle).unwrap(), "3.4.5");
}

#[test]
fn bundle_version_gives_up_on_an_executable_that_never_answers() {
    let t = tempfile::tempdir().unwrap();
    let bundle = unpacked(t.path(), "1.0.0", BUNDLE_ID);
    let exe = bundle.join(APP_EXECUTABLE);
    std::fs::write(&exe, "#!/bin/sh\nsleep 30\n").unwrap();
    let started = std::time::Instant::now();
    let err = bundle_version(&bundle).unwrap_err();
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert!(format!("{err:#}").contains("did not answer within 5 seconds"));
}

const TARGET: &str = "test-target";

fn sha_line(bytes: &[u8], name: &str) -> Vec<u8> {
    use sha2::Digest;
    format!("{}  {name}\n", hex::encode(sha2::Sha256::digest(bytes))).into_bytes()
}

fn release_files(archive: &Path) -> Vec<(String, Vec<u8>)> {
    let bytes = std::fs::read(archive).unwrap();
    let sha = sha_line(&bytes, &archive_name(TARGET));
    vec![(archive_name(TARGET), bytes), (checksum_name(TARGET), sha)]
}

fn beside(dir: &Path, archive: &Path) {
    for (name, bytes) in release_files(archive) {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
}

fn tag_server(version: &str, files: Vec<(String, Vec<u8>)>) -> String {
    counted_tag_server(version, files).0
}

fn counted_tag_server(version: &str, files: Vec<(String, Vec<u8>)>) -> (String, Arc<AtomicUsize>) {
    let requests = Arc::new(AtomicUsize::new(0));
    let counted = requests.clone();
    let files = Arc::new(Mutex::new(files));
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let tag_path = format!("/releases/tags/v{version}");
    let version = version.to_string();
    let base2 = base.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            counted.fetch_add(1, Ordering::SeqCst);
            let mut buf = vec![0u8; 8192];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
            let (status, ctype, body): (&str, &str, Vec<u8>) = if path == tag_path {
                let assets: Vec<serde_json::Value> = files
                    .lock()
                    .unwrap()
                    .iter()
                    .map(
                        |(n, _)| serde_json::json!({ "name": n, "url": format!("{base2}/dl/{n}") }),
                    )
                    .collect();
                let v = serde_json::json!({ "tag_name": format!("v{version}"), "html_url": format!("{base2}/tag/v{version}"), "assets": assets });
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
    (base, requests)
}

struct Home {
    _t: tempfile::TempDir,
    root: PathBuf,
    exe_dir: PathBuf,
    paths: Paths,
}

fn home_named(name: &str) -> Home {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join(name);
    let exe_dir = t.path().join("exe");
    std::fs::create_dir_all(&exe_dir).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(exe_dir.join("rhebok"), "").unwrap();
    let paths = Paths::at(&root, vec![], t.path().join("cache"));
    Home {
        _t: t,
        root,
        exe_dir,
        paths,
    }
}

fn home() -> Home {
    home_named("root")
}

fn app_opts(h: &Home, version: &str, download: Option<Download>) -> AppOpts {
    AppOpts {
        target: TARGET.to_string(),
        version: version.to_string(),
        exe: h.exe_dir.join("rhebok"),
        download,
    }
}

fn stub(api: &str) -> Option<Download> {
    Some(Download {
        api: api.to_string(),
        token: None,
    })
}

fn old_bundle(h: &Home, version: &str) {
    let staging = h.root.join("old-staging");
    std::fs::create_dir_all(&staging).unwrap();
    let staged = unpacked(&staging, version, BUNDLE_ID);
    place_bundle(&staged, &h.paths.app_bundle()).unwrap();
    std::fs::remove_dir_all(&staging).unwrap();
}

fn no_temp_folders(h: &Home) {
    let hidden: Vec<_> = std::fs::read_dir(&h.paths.applications)
        .map(|d| {
            d.flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with(".Rhebok.app."))
                .collect()
        })
        .unwrap_or_default();
    assert!(hidden.is_empty(), "left behind: {hidden:?}");
    let fetched = std::fs::read_dir(&h.paths.tmp)
        .map(|d| d.flatten().count())
        .unwrap_or(0);
    assert_eq!(fetched, 0, "download folder left behind");
}

#[test]
fn install_places_the_bundle_from_the_archive_beside_the_binary() {
    let h = home();
    let archive = bundle_archive(&h.root, "1.2.3", BUNDLE_ID);
    beside(&h.exe_dir, &archive);
    let r = install_app(&h.paths, &app_opts(&h, "1.2.3", None));
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(
        r.lines,
        vec![format!(
            "app:      placed at {} (1.2.3)",
            h.paths.app_bundle().display()
        )]
    );
    assert_eq!(bundle_version(&h.paths.app_bundle()).unwrap(), "1.2.3");
    no_temp_folders(&h);
}

#[test]
fn an_update_from_an_old_updater_downloads_the_bundle_for_its_own_version() {
    let h = home();
    let archive = bundle_archive(&h.root, "1.2.3", BUNDLE_ID);
    let api = tag_server("1.2.3", release_files(&archive));
    let r = install_app(&h.paths, &app_opts(&h, "1.2.3", stub(&api)));
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(r.lines.len(), 1);
    assert!(
        r.lines[0].starts_with("app:      placed at "),
        "{:?}",
        r.lines
    );
    assert!(r.lines[0].ends_with("(1.2.3)"));
    assert_eq!(bundle_version(&h.paths.app_bundle()).unwrap(), "1.2.3");
    no_temp_folders(&h);
}

#[test]
fn a_dev_build_with_no_archive_keeps_a_matching_bundle() {
    let h = home();
    old_bundle(&h, "2.0.0");
    let r = install_app(&h.paths, &app_opts(&h, "2.0.0", None));
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(
        r.lines,
        vec![format!(
            "app:      kept {} (2.0.0)",
            h.paths.app_bundle().display()
        )]
    );
}

#[test]
fn a_dev_build_with_no_archive_and_no_bundle_warns_with_make_install() {
    let h = home();
    let r = install_app(&h.paths, &app_opts(&h, "2.0.0", None));
    assert!(r.lines.is_empty());
    assert_eq!(
        r.warnings,
        vec![
            "Rhebok.app was not placed: this is a development build; build and place it with make install"
                .to_string()
        ]
    );
}

#[test]
fn a_bad_checksum_leaves_the_old_bundle_and_warns() {
    let h = home();
    old_bundle(&h, "1.0.0");
    let archive = bundle_archive(&h.root, "1.2.3", BUNDLE_ID);
    beside(&h.exe_dir, &archive);
    std::fs::write(
        h.exe_dir.join(checksum_name(TARGET)),
        format!("{}  x\n", "0".repeat(64)),
    )
    .unwrap();
    let r = install_app(&h.paths, &app_opts(&h, "1.2.3", None));
    assert!(r.lines.is_empty());
    assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
    assert!(r.warnings[0].contains("checksum mismatch"));
    assert_eq!(bundle_version(&h.paths.app_bundle()).unwrap(), "1.0.0");
    no_temp_folders(&h);
}

#[test]
fn a_release_without_the_app_asset_keeps_the_bundle_and_warns() {
    let h = home();
    old_bundle(&h, "1.0.0");
    let api = tag_server("1.2.3", vec![("rhebok-other.tar.gz".to_string(), vec![1])]);
    let r = install_app(&h.paths, &app_opts(&h, "1.2.3", stub(&api)));
    assert!(r.lines.is_empty());
    assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
    assert!(r.warnings[0].contains("v1.2.3"), "{}", r.warnings[0]);
    assert_eq!(bundle_version(&h.paths.app_bundle()).unwrap(), "1.0.0");
    no_temp_folders(&h);
}

#[test]
fn an_unknown_release_tag_is_a_warning() {
    let h = home();
    let api = tag_server("9.9.9", vec![]);
    let r = install_app(&h.paths, &app_opts(&h, "1.2.3", stub(&api)));
    assert!(r.lines.is_empty());
    assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
    assert!(r.warnings[0].contains("v1.2.3"));
    no_temp_folders(&h);
}

#[test]
fn an_archive_whose_app_will_not_report_its_version_is_not_placed() {
    let h = home();
    old_bundle(&h, "1.0.0");
    let plist = info_plist(BUNDLE_ID);
    let archive = raw_archive(
        &h.root,
        "mute.tar.gz",
        &[
            folder("Rhebok.app/"),
            folder("Rhebok.app/Contents/"),
            folder("Rhebok.app/Contents/MacOS/"),
            file("Rhebok.app/Contents/Info.plist", 0o644, plist.as_bytes()),
            file(
                "Rhebok.app/Contents/MacOS/rhebok-app",
                0o755,
                b"#!/bin/sh\nexit 3\n",
            ),
        ],
    );
    beside(&h.exe_dir, &archive);
    let r = install_app(&h.paths, &app_opts(&h, "1.2.3", None));
    assert!(r.lines.is_empty());
    assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
    assert!(
        r.warnings[0].starts_with(
            "Rhebok.app was not placed: the archive's bundle did not report its version: "
        ),
        "{}",
        r.warnings[0]
    );
    assert!(
        r.warnings[0].ends_with("; the current bundle is left alone"),
        "{}",
        r.warnings[0]
    );
    assert_eq!(bundle_version(&h.paths.app_bundle()).unwrap(), "1.0.0");
    no_temp_folders(&h);
}

#[test]
fn an_archive_whose_app_reports_another_version_is_not_placed() {
    let h = home();
    old_bundle(&h, "1.0.0");
    let archive = bundle_archive(&h.root, "9.9.9", BUNDLE_ID);
    beside(&h.exe_dir, &archive);
    let r = install_app(&h.paths, &app_opts(&h, "1.2.3", None));
    assert!(r.lines.is_empty(), "{:?}", r.lines);
    assert_eq!(
        r.warnings,
        vec![
            "Rhebok.app was not placed: the archive's bundle reports version 9.9.9, not 1.2.3; the current bundle is left alone"
                .to_string()
        ]
    );
    assert_eq!(bundle_version(&h.paths.app_bundle()).unwrap(), "1.0.0");
    no_temp_folders(&h);
}

#[test]
fn a_download_keeps_a_bundle_already_at_its_version_without_a_request() {
    let h = home();
    old_bundle(&h, "1.2.3");
    let archive = bundle_archive(&h.root, "1.2.3", BUNDLE_ID);
    let (api, requests) = counted_tag_server("1.2.3", release_files(&archive));
    let r = install_app(&h.paths, &app_opts(&h, "1.2.3", stub(&api)));
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(
        r.lines,
        vec![format!(
            "app:      kept {} (1.2.3)",
            h.paths.app_bundle().display()
        )]
    );
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    no_temp_folders(&h);
}

#[test]
fn a_download_leaves_a_foreign_bundle_without_a_request() {
    let h = home();
    foreign_bundle(&h);
    let archive = bundle_archive(&h.root, "1.2.3", BUNDLE_ID);
    let (api, requests) = counted_tag_server("1.2.3", release_files(&archive));
    let r = install_app(&h.paths, &app_opts(&h, "1.2.3", stub(&api)));
    assert!(r.lines.is_empty(), "{:?}", r.lines);
    assert_eq!(
        r.warnings,
        vec![format!(
            "Rhebok.app was not placed: {} belongs to `com.example.other`, not to rhebok; leaving it alone",
            h.paths.app_bundle().display()
        )]
    );
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    assert_eq!(
        existing(&h.paths.app_bundle()),
        Existing::Foreign("com.example.other".to_string())
    );
    no_temp_folders(&h);
}

#[test]
fn a_download_leaves_something_that_is_not_a_bundle_without_a_request() {
    let h = home();
    std::fs::create_dir_all(&h.paths.applications).unwrap();
    std::fs::write(h.paths.app_bundle(), "not a bundle").unwrap();
    let archive = bundle_archive(&h.root, "1.2.3", BUNDLE_ID);
    let (api, requests) = counted_tag_server("1.2.3", release_files(&archive));
    let r = install_app(&h.paths, &app_opts(&h, "1.2.3", stub(&api)));
    assert!(r.lines.is_empty(), "{:?}", r.lines);
    assert_eq!(
        r.warnings,
        vec![format!(
            "Rhebok.app was not placed: {} is not a Rhebok.app bundle of ours; leaving it alone",
            h.paths.app_bundle().display()
        )]
    );
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    assert_eq!(
        std::fs::read_to_string(h.paths.app_bundle()).unwrap(),
        "not a bundle"
    );
    no_temp_folders(&h);
}

fn foreign_bundle(h: &Home) {
    let staging = h.root.join("foreign-staging");
    std::fs::create_dir_all(&staging).unwrap();
    let staged = unpacked(&staging, "1.0.0", "com.example.other");
    std::fs::create_dir_all(&h.paths.applications).unwrap();
    std::fs::rename(staged, h.paths.app_bundle()).unwrap();
}

#[test]
fn uninstall_removes_our_bundle() {
    let h = home();
    old_bundle(&h, "1.0.0");
    let r = uninstall_app(&h.paths);
    assert!(!h.paths.app_bundle().exists());
    assert_eq!(
        r.lines,
        vec![
            format!("removed {}", h.paths.app_bundle().display()),
            "a Rhebok window that is still open keeps running until you close it".to_string(),
        ]
    );
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
}

#[test]
fn uninstall_removes_leftover_staging_folders_beside_the_bundle() {
    let h = home();
    old_bundle(&h, "1.0.0");
    let unpack = h.paths.applications.join(".Rhebok.app.123-unpack");
    let old = h.paths.applications.join(".Rhebok.app.old-456");
    std::fs::create_dir_all(unpack.join("Rhebok.app/Contents")).unwrap();
    std::fs::create_dir_all(&old).unwrap();
    let other = h.paths.applications.join("Other.app");
    std::fs::create_dir_all(&other).unwrap();
    let r = uninstall_app(&h.paths);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert!(!h.paths.app_bundle().exists());
    assert!(!unpack.exists());
    assert!(!old.exists());
    assert!(other.is_dir());
}

#[test]
fn uninstall_does_nothing_when_there_is_no_bundle() {
    let h = home();
    let r = uninstall_app(&h.paths);
    assert!(r.lines.is_empty() && r.warnings.is_empty());
}

#[test]
fn uninstall_leaves_a_foreign_bundle_and_warns() {
    let h = home();
    foreign_bundle(&h);
    let r = uninstall_app(&h.paths);
    assert!(r.lines.is_empty());
    assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
    assert!(
        r.warnings[0].contains("com.example.other"),
        "{}",
        r.warnings[0]
    );
    assert!(h.paths.app_bundle().join(APP_EXECUTABLE).is_file());
}

#[test]
fn uninstall_leaves_a_symlinked_bundle_and_warns() {
    let h = home();
    let real = unpacked(&h.root, "1.0.0", BUNDLE_ID);
    std::fs::create_dir_all(&h.paths.applications).unwrap();
    std::os::unix::fs::symlink(&real, h.paths.app_bundle()).unwrap();
    let r = uninstall_app(&h.paths);
    assert!(r.lines.is_empty());
    assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
    assert_eq!(std::fs::read_link(h.paths.app_bundle()).unwrap(), real);
    assert!(real.join(APP_EXECUTABLE).is_file());
}

#[test]
fn the_not_installed_message_fits_each_build() {
    assert_eq!(
        not_installed_message(true, true),
        "Rhebok.app is not installed; run rhebok install to place it"
    );
    assert_eq!(
        not_installed_message(true, false),
        "Rhebok.app is not installed; build and place it with make install"
    );
    assert_eq!(
        not_installed_message(false, true),
        "the native app is macOS only; run rhebok dashboard for the web one"
    );
    assert_eq!(
        not_installed_message(false, false),
        "the native app is macOS only; run rhebok dashboard for the web one"
    );
}

#[test]
fn an_unreachable_release_server_is_a_warning_not_an_error() {
    let h = home();
    let r = install_app(&h.paths, &app_opts(&h, "1.2.3", stub("http://127.0.0.1:9")));
    assert!(r.lines.is_empty());
    assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
    assert!(r.warnings[0].starts_with("Rhebok.app was not placed"));
    no_temp_folders(&h);
}

#[test]
fn a_home_with_a_space_places_and_starts_the_bundle() {
    let h = home_named("a b");
    let archive = bundle_archive(&h.root, "1.2.3", BUNDLE_ID);
    beside(&h.exe_dir, &archive);
    let r = install_app(&h.paths, &app_opts(&h, "1.2.3", None));
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(r.lines.len(), 1);
    assert_eq!(bundle_version(&h.paths.app_bundle()).unwrap(), "1.2.3");
    no_temp_folders(&h);
}

#[test]
fn an_archive_with_a_foreign_id_is_not_placed() {
    let h = home();
    old_bundle(&h, "1.0.0");
    let archive = bundle_archive(&h.root, "1.2.3", "com.example.other");
    beside(&h.exe_dir, &archive);
    let r = install_app(&h.paths, &app_opts(&h, "1.2.3", None));
    assert!(r.lines.is_empty());
    assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
    assert!(r.warnings[0].contains("com.example.other"));
    assert_eq!(bundle_version(&h.paths.app_bundle()).unwrap(), "1.0.0");
    no_temp_folders(&h);
}

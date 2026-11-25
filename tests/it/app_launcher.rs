use std::os::unix::fs::PermissionsExt;
use std::process::Command;

fn rhebok(data: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rhebok"));
    cmd.arg("app")
        .env("RHEBOK_DB", data.join("db"))
        .env("HOME", data)
        .env_remove("BRAIN_DB");
    cmd
}

#[test]
fn rhebok_app_without_the_binary_says_how_to_get_it() {
    let tmp = tempfile::tempdir().unwrap();
    let out = rhebok(tmp.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let expected = rhebok::setup::app::not_installed_message(cfg!(target_os = "macos"), false);
    assert_eq!(String::from_utf8_lossy(&out.stderr).trim(), expected);
}

#[test]
fn rhebok_app_runs_the_installed_binary_with_the_same_arguments() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let app = bin.join("rhebok-app");
    std::fs::write(&app, "#!/bin/sh\necho \"ran $*\"\nexit 7\n").unwrap();
    std::fs::set_permissions(&app, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = rhebok(tmp.path())
        .args(["--flag", "value"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(7));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "ran --flag value"
    );
}

fn stub(path: &std::path::Path, name: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\necho {name}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn copy_of_the_cli(dir: &std::path::Path) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let copy = dir.join("rhebok");
    let original = env!("CARGO_BIN_EXE_rhebok");
    if std::fs::hard_link(original, &copy).is_err() {
        std::fs::copy(original, &copy).unwrap();
    }
    copy
}

fn run_copy(cli: &std::path::Path, data: &std::path::Path) -> std::process::Output {
    Command::new(cli)
        .arg("app")
        .env("RHEBOK_DB", data.join("db"))
        .env("HOME", data)
        .output()
        .unwrap()
}

#[test]
fn the_app_in_the_data_root_wins_over_the_one_beside_the_cli() {
    let tmp = tempfile::tempdir().unwrap();
    let cli = copy_of_the_cli(&tmp.path().join("keg"));
    stub(&tmp.path().join("keg/rhebok-app"), "beside");
    stub(&tmp.path().join("bin/rhebok-app"), "root");
    let out = run_copy(&cli, tmp.path());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "root");
}

#[test]
fn the_app_beside_the_cli_is_used_when_the_data_root_has_none() {
    let tmp = tempfile::tempdir().unwrap();
    let cli = copy_of_the_cli(&tmp.path().join("keg"));
    stub(&tmp.path().join("keg/rhebok-app"), "beside");
    let out = run_copy(&cli, tmp.path());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "beside");
}

fn info_plist(bundle: &std::path::Path, id: &str) {
    std::fs::create_dir_all(bundle.join("Contents")).unwrap();
    std::fs::write(
        bundle.join("Contents/Info.plist"),
        format!("<plist><dict><key>CFBundleIdentifier</key><string>{id}</string></dict></plist>"),
    )
    .unwrap();
}

fn bundle_with_id(home: &std::path::Path, id: &str, name: &str) {
    let bundle = home.join("Applications/Rhebok.app");
    info_plist(&bundle, id);
    stub(&bundle.join("Contents/MacOS/rhebok-app"), name);
}

fn bundle_stub(home: &std::path::Path, name: &str) {
    bundle_with_id(home, rhebok::setup::app::BUNDLE_ID, name);
}

#[test]
fn rhebok_app_starts_the_bundle_in_applications() {
    let tmp = tempfile::tempdir().unwrap();
    let bundle = tmp.path().join("Applications/Rhebok.app");
    info_plist(&bundle, rhebok::setup::app::BUNDLE_ID);
    let app = bundle.join("Contents/MacOS/rhebok-app");
    std::fs::create_dir_all(app.parent().unwrap()).unwrap();
    std::fs::write(&app, "#!/bin/sh\necho \"bundle $*\"\n").unwrap();
    std::fs::set_permissions(&app, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = rhebok(tmp.path())
        .args(["--flag", "value"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "bundle --flag value"
    );
}

#[test]
fn the_bundle_wins_over_a_root_copy() {
    let tmp = tempfile::tempdir().unwrap();
    bundle_stub(tmp.path(), "bundle");
    stub(&tmp.path().join("bin/rhebok-app"), "root");
    let out = rhebok(tmp.path()).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "bundle");
}

#[test]
fn a_foreign_bundle_in_applications_is_not_started() {
    let tmp = tempfile::tempdir().unwrap();
    bundle_with_id(tmp.path(), "com.example.other", "foreign");
    stub(&tmp.path().join("bin/rhebok-app"), "root");
    let out = rhebok(tmp.path()).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "root");
}

#[test]
fn a_bundle_without_an_info_plist_is_not_started() {
    let tmp = tempfile::tempdir().unwrap();
    stub(
        &tmp.path()
            .join("Applications/Rhebok.app/Contents/MacOS/rhebok-app"),
        "unknown",
    );
    stub(&tmp.path().join("bin/rhebok-app"), "root");
    let out = rhebok(tmp.path()).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "root");
}

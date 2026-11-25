use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn target() -> Option<&'static str> {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => Some("aarch64-apple-darwin"),
        ("x86_64", "linux") => Some("x86_64-unknown-linux-gnu"),
        ("aarch64", "linux") => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}

fn release_dir(dir: &Path, record: &Path, corrupt: bool) -> String {
    use sha2::Digest;
    let script = format!(
        "#!/bin/sh\necho \"$*\" >> \"{}\"\nexit 0\n",
        record.display()
    );
    let mut header = tar::Header::new_gnu();
    header.set_size(script.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    let mut tgz = Vec::new();
    {
        let enc = flate2::write::GzEncoder::new(&mut tgz, flate2::Compression::default());
        let mut ar = tar::Builder::new(enc);
        ar.append_data(&mut header, "rhebok", script.as_bytes())
            .unwrap();
        ar.into_inner().unwrap().finish().unwrap();
    }
    let t = target().unwrap();
    std::fs::write(dir.join(format!("rhebok-{t}.tar.gz")), &tgz).unwrap();
    let digest = if corrupt {
        "0".repeat(64)
    } else {
        hex::encode(sha2::Sha256::digest(&tgz))
    };
    std::fs::write(
        dir.join(format!("rhebok-{t}.sha256")),
        format!("{digest}  rhebok-{t}.tar.gz\n"),
    )
    .unwrap();
    format!("file://{}", dir.display())
}

#[test]
fn install_sh_is_valid_shell() {
    let out = Command::new("sh")
        .args(["-n", "scripts/install.sh"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn install_sh_downloads_verifies_and_runs_rhebok_install_with_the_flags() {
    let Some(_) = target() else { return };
    let t = tempfile::tempdir().unwrap();
    let record = t.path().join("record");
    let base = release_dir(t.path(), &record, false);
    let out = Command::new("sh")
        .args(["scripts/install.sh", "--yes", "--quiet"])
        .env("RHEBOK_RELEASE_BASE", &base)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&record).unwrap().trim(),
        "install --yes --quiet"
    );
    let mode = std::fs::metadata("scripts/install.sh")
        .unwrap()
        .permissions()
        .mode();
    assert!(mode & 0o111 != 0, "install.sh must be executable in git");
}

#[test]
fn install_sh_refuses_a_bad_checksum_and_runs_nothing() {
    let Some(_) = target() else { return };
    let t = tempfile::tempdir().unwrap();
    let record = t.path().join("record");
    let base = release_dir(t.path(), &record, true);
    let out = Command::new("sh")
        .args(["scripts/install.sh"])
        .env("RHEBOK_RELEASE_BASE", &base)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("checksum"));
    assert!(!record.exists());
}

#[cfg(target_os = "macos")]
fn app_release_dir(dir: &Path, record: &Path, with_app: bool) -> String {
    let base = release_dir(dir, record, false);
    let script = format!(
        "#!/bin/sh\nls \"$(dirname \"$0\")\" >> \"{}\"\nexit 0\n",
        record.display()
    );
    let mut header = tar::Header::new_gnu();
    header.set_size(script.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    let mut tgz = Vec::new();
    {
        let enc = flate2::write::GzEncoder::new(&mut tgz, flate2::Compression::default());
        let mut ar = tar::Builder::new(enc);
        ar.append_data(&mut header, "rhebok", script.as_bytes())
            .unwrap();
        ar.into_inner().unwrap().finish().unwrap();
    }
    use sha2::Digest;
    let t = target().unwrap();
    std::fs::write(dir.join(format!("rhebok-{t}.tar.gz")), &tgz).unwrap();
    std::fs::write(
        dir.join(format!("rhebok-{t}.sha256")),
        format!(
            "{}  rhebok-{t}.tar.gz\n",
            hex::encode(sha2::Sha256::digest(&tgz))
        ),
    )
    .unwrap();
    if with_app {
        let app = b"app archive bytes";
        std::fs::write(dir.join(rhebok::setup::app::archive_name(t)), app).unwrap();
        std::fs::write(
            dir.join(rhebok::setup::app::checksum_name(t)),
            format!(
                "{}  {}\n",
                hex::encode(sha2::Sha256::digest(app)),
                rhebok::setup::app::archive_name(t)
            ),
        )
        .unwrap();
    }
    base
}

#[cfg(target_os = "macos")]
#[test]
fn install_sh_fetches_the_app_archive_beside_the_cli() {
    let t = tempfile::tempdir().unwrap();
    let record = t.path().join("record");
    let base = app_release_dir(t.path(), &record, true);
    let out = Command::new("sh")
        .args(["scripts/install.sh"])
        .env("RHEBOK_RELEASE_BASE", &base)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let listing = std::fs::read_to_string(&record).unwrap();
    let name = rhebok::setup::app::archive_name(target().unwrap());
    let sha = rhebok::setup::app::checksum_name(target().unwrap());
    assert!(listing.lines().any(|l| l == name), "{listing}");
    assert!(listing.lines().any(|l| l == sha), "{listing}");
}

#[cfg(target_os = "macos")]
#[test]
fn install_sh_goes_on_when_the_app_download_fails() {
    let t = tempfile::tempdir().unwrap();
    let record = t.path().join("record");
    let base = app_release_dir(t.path(), &record, false);
    let out = Command::new("sh")
        .args(["scripts/install.sh"])
        .env("RHEBOK_RELEASE_BASE", &base)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("could not download the app") && stderr.contains("will try again"),
        "{stderr}"
    );
    let listing = std::fs::read_to_string(&record).unwrap();
    assert!(!listing.contains("rhebok-app"), "{listing}");
}

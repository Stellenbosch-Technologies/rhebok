use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const TARGETS: [&str; 3] = [
    "aarch64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
];

fn host_target() -> Option<&'static str> {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => Some("aarch64-apple-darwin"),
        ("x86_64", "linux") => Some("x86_64-unknown-linux-gnu"),
        ("aarch64", "linux") => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}

fn append(ar: &mut tar::Builder<impl std::io::Write>, name: &str, body: &str, mode: u32) {
    let mut header = tar::Header::new_gnu();
    header.set_size(body.len() as u64);
    header.set_mode(mode);
    header.set_cksum();
    ar.append_data(&mut header, name, body.as_bytes()).unwrap();
}

fn fake_release(dist: &Path) {
    for t in TARGETS {
        let mut tgz = Vec::new();
        {
            let enc = flate2::write::GzEncoder::new(&mut tgz, flate2::Compression::default());
            let mut ar = tar::Builder::new(enc);
            append(
                &mut ar,
                "rhebok",
                &format!("#!/bin/sh\necho \"bundled {t} $*\"\n"),
                0o755,
            );
            append(&mut ar, "LICENSE", "license text\n", 0o644);
            append(
                &mut ar,
                "THIRD-PARTY-NOTICES.md",
                "# Third-party notices\n",
                0o644,
            );
            ar.into_inner().unwrap().finish().unwrap();
        }
        std::fs::write(dist.join(format!("rhebok-{t}.tar.gz")), &tgz).unwrap();
    }
}

fn bundle(dist: &Path, out: &Path) -> Output {
    Command::new("sh")
        .args(["scripts/mcp-bundle.sh", "9.8.7"])
        .arg(dist)
        .arg(out)
        .output()
        .unwrap()
}

fn built() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let dist = t.path().join("dist");
    let out = t.path().join("out");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    fake_release(&dist);
    let o = bundle(&dist, &out);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    (t, out)
}

fn extracted(out: &Path, into: &Path) {
    let o = Command::new("unzip")
        .arg("-q")
        .arg(out.join("rhebok-mcp.mcpb"))
        .arg("-d")
        .arg(into)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

fn json(p: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

#[test]
fn the_bundle_holds_its_manifest_the_launcher_and_every_release_target() {
    let (t, out) = built();
    let root = t.path().join("x");
    extracted(&out, &root);
    for f in [
        "manifest.json",
        "icon.png",
        "LICENSE",
        "THIRD-PARTY-NOTICES.md",
        "server/rhebok-mcp",
    ] {
        assert!(root.join(f).is_file(), "missing {f}");
    }
    for target in TARGETS {
        assert!(
            root.join("server").join(target).join("rhebok").is_file(),
            "missing the {target} build"
        );
    }
    assert_eq!(
        std::fs::read(root.join("icon.png")).unwrap(),
        std::fs::read("plugin/icon.png").unwrap()
    );
    let m = json(&root.join("manifest.json"));
    assert_eq!(m["name"], "rhebok");
    assert_eq!(m["version"], "9.8.7");
    assert_eq!(m["icon"], "icon.png");
    assert_eq!(m["license"], "AGPL-3.0-only");
    assert_eq!(m["server"]["type"], "binary");
    assert_eq!(m["server"]["entry_point"], "server/rhebok-mcp");
    assert_eq!(
        m["server"]["mcp_config"]["command"],
        "${__dirname}/server/rhebok-mcp"
    );
    assert_eq!(m["server"]["mcp_config"]["args"][0], "mcp");
    assert_eq!(
        m["compatibility"]["platforms"],
        serde_json::json!(["darwin", "linux"])
    );
}

#[test]
fn server_json_names_the_release_asset_and_its_digest() {
    use sha2::Digest;
    let (_t, out) = built();
    let s = json(&out.join("server.json"));
    assert_eq!(s["name"], "io.github.Stellenbosch-Technologies/rhebok");
    assert_eq!(s["version"], "9.8.7");
    let description = s["description"].as_str().unwrap();
    assert!(
        !description.is_empty() && description.chars().count() <= 100,
        "the registry allows 1-100 characters: {description}"
    );
    let p = &s["packages"][0];
    assert_eq!(p["registryType"], "mcpb");
    assert_eq!(
        p["identifier"],
        "https://github.com/Stellenbosch-Technologies/rhebok/releases/download/v9.8.7/rhebok-mcp.mcpb"
    );
    assert_eq!(p["transport"]["type"], "stdio");
    let digest = hex::encode(sha2::Sha256::digest(
        std::fs::read(out.join("rhebok-mcp.mcpb")).unwrap(),
    ));
    assert_eq!(p["fileSha256"], digest.as_str());
}

fn png_size(path: &str) -> String {
    let bytes = std::fs::read(path).unwrap();
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    format!("{width}x{height}")
}

#[test]
fn server_json_declares_the_size_the_icon_really_has_and_it_is_512_square() {
    let (_t, out) = built();
    let s = json(&out.join("server.json"));
    let size = png_size("plugin/icon.png");
    assert_eq!(size, "512x512");
    assert_eq!(s["icons"][0]["sizes"][0], size.as_str());
}

fn launch(root: &Path, home: &Path) -> String {
    let o = Command::new(root.join("server/rhebok-mcp"))
        .arg("mcp")
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

#[test]
fn the_launcher_runs_the_bundled_build_for_this_machine() {
    let Some(host) = host_target() else { return };
    let (t, out) = built();
    let root = t.path().join("x");
    extracted(&out, &root);
    let home = t.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    assert_eq!(launch(&root, &home), format!("bundled {host} mcp"));
}

#[test]
fn the_launcher_prefers_the_rhebok_that_install_placed() {
    use std::os::unix::fs::PermissionsExt;
    if host_target().is_none() {
        return;
    }
    let (t, out) = built();
    let root = t.path().join("x");
    extracted(&out, &root);
    let home = t.path().join("home");
    let data = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/rhebok/bin")
    } else {
        home.join(".local/share/rhebok/bin")
    };
    std::fs::create_dir_all(&data).unwrap();
    let installed = data.join("rhebok");
    std::fs::write(&installed, "#!/bin/sh\necho \"installed $*\"\n").unwrap();
    std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(launch(&root, &home), "installed mcp");
}

#[test]
fn a_release_missing_a_target_builds_no_bundle() {
    let t = tempfile::tempdir().unwrap();
    let dist = t.path().join("dist");
    let out = t.path().join("out");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    fake_release(&dist);
    std::fs::remove_file(dist.join("rhebok-aarch64-unknown-linux-gnu.tar.gz")).unwrap();
    let o = bundle(&dist, &out);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("aarch64-unknown-linux-gnu"));
    assert!(!out.join("rhebok-mcp.mcpb").exists());
    assert!(!out.join("server.json").exists());
}

#[test]
fn the_launcher_prefers_a_rhebok_on_path() {
    use std::os::unix::fs::PermissionsExt;
    if host_target().is_none() {
        return;
    }
    let (t, out) = built();
    let root = t.path().join("x");
    extracted(&out, &root);
    let home = t.path().join("home");
    let bin = t.path().join("bin");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    let on_path = bin.join("rhebok");
    std::fs::write(&on_path, "#!/bin/sh\necho \"on path $*\"\n").unwrap();
    std::fs::set_permissions(&on_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let o = Command::new(root.join("server/rhebok-mcp"))
        .arg("mcp")
        .env_clear()
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(String::from_utf8(o.stdout).unwrap().trim(), "on path mcp");
}

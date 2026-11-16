use anyhow::{anyhow, bail, Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};

pub fn download(url: &str, token: Option<&str>, agent: &str, to: &Path) -> Result<()> {
    let mut req = super::release::client(agent, std::time::Duration::from_secs(300))?
        .get(url)
        .header("Accept", "application/octet-stream");
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let mut resp = req.send().with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        bail!("GET {url}: HTTP {}", resp.status());
    }
    let mut file = std::fs::File::create(to).with_context(|| format!("create {}", to.display()))?;
    std::io::copy(&mut resp, &mut file).with_context(|| format!("write {}", to.display()))?;
    Ok(())
}

pub fn verify_sha256(archive: &Path, sha_file: &Path) -> Result<()> {
    let expected = std::fs::read_to_string(sha_file)
        .with_context(|| format!("read {}", sha_file.display()))?
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_lowercase();
    if expected.len() != 64 {
        bail!("{} does not start with a sha256 digest", sha_file.display());
    }
    let mut f =
        std::fs::File::open(archive).with_context(|| format!("open {}", archive.display()))?;
    let actual = crate::sha256::hex_of_reader(&mut f)?;
    if actual != expected {
        bail!(
            "checksum mismatch for {}: expected {expected} vs actual {actual}",
            archive.display()
        );
    }
    Ok(())
}

pub fn extract_single(archive: &Path, into: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(into).with_context(|| format!("create {}", into.display()))?;
    let f = std::fs::File::open(archive).with_context(|| format!("open {}", archive.display()))?;
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(f));
    let mut out: Option<PathBuf> = None;
    for entry in ar.entries().context("read tar entries")? {
        let mut entry = entry.context("read tar entry")?;
        let path = entry.path().context("tar entry path")?.into_owned();
        let kind = entry.header().entry_type();
        if kind.is_pax_global_extensions() {
            continue;
        }
        if !kind.is_file() {
            bail!(
                "{} holds `{}`, which is not a regular file; an update archive holds only files",
                archive.display(),
                path.display()
            );
        }
        let Some(name) = root_file_name(&path) else {
            bail!(
                "{} holds `{}`, which is not a file at the archive root",
                archive.display(),
                path.display()
            );
        };
        if name != "rhebok" {
            continue;
        }
        if out.is_some() {
            bail!(
                "{} holds more than one file named rhebok",
                archive.display()
            );
        }
        let dest = into.join("rhebok");
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        std::fs::write(&dest, bytes)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
        out = Some(dest);
    }
    out.ok_or_else(|| anyhow!("{} contains no file named rhebok", archive.display()))
}

fn root_file_name(path: &Path) -> Option<&std::ffi::OsStr> {
    use std::path::Component;
    let mut normal = path.components().filter(|c| *c != Component::CurDir);
    match (normal.next(), normal.next()) {
        (Some(Component::Normal(name)), None) => Some(name),
        _ => None,
    }
}

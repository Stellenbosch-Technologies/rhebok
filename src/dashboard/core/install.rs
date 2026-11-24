use crate::config::Config;
use crate::setup::agents::AgentEnv;
use crate::setup::install::{fix_for, install_checks, Fix, InstallCheck};
use std::path::{Path, PathBuf};

use super::DashboardError;

#[derive(Debug, Clone, serde::Serialize)]
pub struct InstallCheckView {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
    pub fix: Option<Fix>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InstallConfig {
    pub path: PathBuf,
    pub errors: Vec<crate::config::check::FieldError>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EmbedView {
    pub backend: &'static str,
    pub url: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InstallView {
    pub root: PathBuf,
    pub binary: PathBuf,
    pub exe: PathBuf,
    pub checks: Vec<InstallCheckView>,
    pub config: InstallConfig,
    pub embed: EmbedView,
}

pub fn view(config_path: &Path, env: &AgentEnv, cli: &Path) -> InstallView {
    let claude = env.claude.available();
    let checks = install_checks(&env.paths, &env.claude, &env.version, cli)
        .into_iter()
        .map(|c| {
            let fix = fix_for(&c, cli, claude);
            InstallCheckView {
                name: c.name,
                ok: c.ok,
                detail: c.detail,
                fix,
            }
        })
        .collect();
    let errors = crate::config::edit::read_document(config_path)
        .ok()
        .flatten()
        .map(|text| crate::config::check::check(&text))
        .unwrap_or_default();
    let cfg = Config::load_from(config_path);
    let (remote, url, model) = super::embed::embed_target(&cfg);
    InstallView {
        root: env.paths.root.clone(),
        binary: env.paths.bin.clone(),
        exe: cli.to_path_buf(),
        checks,
        config: InstallConfig {
            path: config_path.to_path_buf(),
            errors,
        },
        embed: EmbedView {
            backend: if remote { "remote" } else { "ollama" },
            url,
            model,
        },
    }
}

pub fn require_cli(cli: &Path) -> Result<(), DashboardError> {
    if is_runnable(cli) {
        return Ok(());
    }
    let missing = InstallCheck {
        name: "binary",
        ok: false,
        detail: format!("{} is not an executable file", cli.display()),
    };
    let fix = match fix_for(&missing, cli, false) {
        Some(Fix::Command { command }) => command,
        Some(Fix::Manual { text }) => text,
        Some(Fix::Connect { agent }) => agent.to_string(),
        None => String::new(),
    };
    Err(DashboardError::CliMissing { fix })
}

#[cfg(unix)]
fn is_runnable(cli: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(cli).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_runnable(cli: &Path) -> bool {
    cli.is_file()
}

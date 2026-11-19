use super::install::InstallReport;
use crate::config::edit::{Outcome, Patch};
use crate::embed::choice::QWEN3;
use crate::env_file::{self, MODEL_VAR, TOKEN_VAR, URL_VAR};
use anyhow::Result;
use std::path::Path;
use toml_edit::value;

pub fn uncalibrated_gates() -> String {
    format!(
        "gates:    hook {} and MCP {}, measured for {} only. Every model scores on its own \
         scale, so run `rhebok bench` with a golden set to see where this one should sit",
        QWEN3.hook_threshold, QWEN3.mcp_threshold, QWEN3.label
    )
}

pub fn custom_patch(model: &str, dimensions: usize) -> Patch {
    Patch {
        set: vec![
            ("embed.model".into(), value(model)),
            ("embed.dimensions".into(), value(dimensions as i64)),
        ],
        unset: vec!["embed.prefix_scheme".into()],
    }
}

pub fn apply_custom(
    config_path: &Path,
    model: &str,
    dimensions: usize,
    first_install: bool,
    r: &mut InstallReport,
) {
    let current = crate::config::Config::load_from(config_path).embed;
    let chosen = format!("embed:    {model}, {dimensions} dimensions");
    if current.model == model && current.dimensions == dimensions {
        r.lines.push(chosen);
        r.lines.push(uncalibrated_gates());
        return;
    }
    match crate::config::edit::update(config_path, None, &custom_patch(model, dimensions)) {
        Ok(Outcome::Written { .. }) => {
            r.lines.push(chosen);
            r.lines.push(uncalibrated_gates());
            if !first_install {
                r.warnings.push(format!(
                    "the embedding model changed from {} to {model}; run `rhebok index --reindex` \
                     to embed everything again, because the existing index cannot be searched with it",
                    current.model
                ));
            }
        }
        Ok(Outcome::Invalid(errors)) => r.warnings.push(format!(
            "could not set the embedding model in {}: {}",
            config_path.display(),
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        )),
        Ok(Outcome::Conflict { .. }) => {}
        Err(e) => r.warnings.push(format!(
            "could not set the embedding model in {}: {e:#}",
            config_path.display()
        )),
    }
}

pub fn write_endpoint(
    config_path: &Path,
    url: &str,
    model: &str,
    token: Option<&str>,
) -> Result<()> {
    if let Some(dir) = config_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    env_file::write_entries(
        &env_file::env_path(config_path),
        &[
            (URL_VAR, Some(url.to_string())),
            (MODEL_VAR, Some(model.to_string())),
            (TOKEN_VAR, token.map(str::to_string)),
        ],
    )
}

pub fn pick_model(listed: &[String], answer: &str) -> Option<String> {
    let answer = answer.trim();
    if answer.is_empty() {
        return (listed.len() == 1).then(|| listed[0].clone());
    }
    match answer.parse::<usize>() {
        Ok(n) => listed.get(n.wrapping_sub(1)).cloned(),
        Err(_) => Some(answer.to_string()),
    }
}

use super::install::InstallReport;
use crate::config::edit::{Outcome, Patch};
use crate::config::EmbedConfig;
use crate::embed::calibrate::{self, GateEstimate};
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

pub fn estimated_gates(model: &str, e: &GateEstimate) -> String {
    let source = format!("{} built-in sentence pairs", e.pairs);
    let kept: Vec<String> = [
        (e.hook_kept_qwen3s(), "hook", e.measured_hook, e.hook),
        (e.mcp_kept_qwen3s(), "MCP", e.measured_mcp, e.mcp),
    ]
    .iter()
    .filter(|(kept, ..)| *kept)
    .map(|(_, gate, measured, floor)| format!("{gate} {measured}, so it kept {floor}"))
    .collect();
    let how = if kept.is_empty() {
        format!(
            "estimated for {model} from {source} (unrelated text scores about {} with it)",
            e.unrelated_mean
        )
    } else {
        format!(
            "for {model}. The estimate from {source} came out looser than {}'s gates ({}), \
             and rhebok never sets a gate below {}'s",
            QWEN3.label,
            kept.join("; "),
            QWEN3.label
        )
    };
    format!(
        "gates:    hook {} and MCP {}, {how}. This is a coarse estimate; `rhebok bench` with a \
         golden set is the real calibration",
        e.hook, e.mcp
    )
}

pub fn measure_gates(embed: &EmbedConfig) -> Result<GateEstimate> {
    calibrate::measure(&*crate::embed::for_config(embed)?)
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

pub fn gate_patch(e: &GateEstimate) -> Patch {
    Patch {
        set: vec![
            ("hook.threshold".into(), value(e.hook)),
            ("mcp.threshold".into(), value(e.mcp)),
        ],
        unset: vec![],
    }
}

fn gates_are_qwen3s(cfg: &crate::config::Config) -> bool {
    cfg.hook.threshold == QWEN3.hook_threshold as f32
        && cfg.mcp.threshold == QWEN3.mcp_threshold as f32
}

fn invalid(config_path: &Path, what: &str, errors: &[impl std::fmt::Display]) -> String {
    format!(
        "could not set {what} in {}: {}",
        config_path.display(),
        errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; ")
    )
}

pub fn apply_custom(
    config_path: &Path,
    model: &str,
    dimensions: usize,
    first_install: bool,
    measure: impl FnOnce(&EmbedConfig) -> Result<GateEstimate>,
    r: &mut InstallReport,
) {
    let current = crate::config::Config::load_from(config_path);
    let chosen = format!("embed:    {model}, {dimensions} dimensions");
    if current.embed.model == model && current.embed.dimensions == dimensions {
        r.lines.push(chosen);
        if gates_are_qwen3s(&current) {
            set_gates(config_path, model, measure, r);
        } else {
            r.lines.push(format!(
                "gates:    hook {} and MCP {}, kept from {}",
                current.hook.threshold,
                current.mcp.threshold,
                config_path.display()
            ));
        }
        return;
    }
    match crate::config::edit::update(config_path, None, &custom_patch(model, dimensions)) {
        Ok(Outcome::Written { .. }) => {
            r.lines.push(chosen);
            set_gates(config_path, model, measure, r);
            if !first_install {
                r.warnings.push(format!(
                    "the embedding model changed from {} to {model}; run `rhebok index --reindex` \
                     to embed everything again, because the existing index cannot be searched with it",
                    current.embed.model
                ));
            }
        }
        Ok(Outcome::Invalid(errors)) => {
            r.warnings
                .push(invalid(config_path, "the embedding model", &errors))
        }
        Ok(Outcome::Conflict { .. }) => {}
        Err(e) => r.warnings.push(format!(
            "could not set the embedding model in {}: {e:#}",
            config_path.display()
        )),
    }
}

fn set_gates(
    config_path: &Path,
    model: &str,
    measure: impl FnOnce(&EmbedConfig) -> Result<GateEstimate>,
    r: &mut InstallReport,
) {
    let estimate = match measure(&crate::config::Config::load_from(config_path).embed) {
        Ok(e) => e,
        Err(e) => {
            r.lines.push(uncalibrated_gates());
            r.warnings.push(format!(
                "could not estimate the relevance gates for {model}: {e:#}. They stay at {}'s; \
                 run `rhebok install --embed-model {model}` again once it embeds",
                QWEN3.label
            ));
            return;
        }
    };
    match crate::config::edit::update(config_path, None, &gate_patch(&estimate)) {
        Ok(Outcome::Written { .. }) => {
            r.lines.push(estimated_gates(model, &estimate));
            if !estimate.separates() {
                r.warnings.push(format!(
                    "{model} scores {} of the {} built-in matched pairs below its own estimated hook \
                     gate, so it barely tells related text from unrelated text. Check the gates \
                     with `rhebok bench` and a golden set before relying on the hook",
                    estimate.matched_below_hook, estimate.pairs
                ));
            }
        }
        Ok(Outcome::Invalid(errors)) => {
            r.warnings
                .push(invalid(config_path, "the relevance gates", &errors))
        }
        Ok(Outcome::Conflict { .. }) => {}
        Err(e) => r.warnings.push(format!(
            "could not set the relevance gates in {}: {e:#}",
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

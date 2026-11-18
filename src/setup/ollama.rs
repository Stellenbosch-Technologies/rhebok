use crate::config::Config;
use anyhow::{bail, Context, Result};

#[derive(Debug)]
pub enum OllamaState {
    Unreachable(String),
    Reachable { missing: Vec<String> },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OllamaModels {
    pub required: Vec<String>,
    pub optional: Vec<String>,
}

impl OllamaModels {
    pub fn is_empty(&self) -> bool {
        self.required.is_empty() && self.optional.is_empty()
    }

    pub fn all(&self) -> Vec<String> {
        self.required
            .iter()
            .chain(&self.optional)
            .cloned()
            .collect()
    }

    pub fn only_optional(&self) -> bool {
        self.required.is_empty() && !self.optional.is_empty()
    }
}

pub fn ollama_models(cfg: &Config) -> OllamaModels {
    let mut required = Vec::new();
    if cfg.embed.remote.is_none() && cfg.embed.remote_error.is_none() {
        required.push(cfg.embed.model.clone());
    }
    if cfg.embed.contextual {
        required.push(cfg.embed.enrich_model.clone());
    }
    let mut optional = Vec::new();
    if crate::memory::distill::runs_automatically(cfg)
        && !required.contains(&cfg.memory.distill_model)
    {
        optional.push(cfg.memory.distill_model.clone());
    }
    OllamaModels { required, optional }
}

pub fn optional_model_advice(model: &str) -> String {
    format!(
        "{model} ({}) is optional: it distills finished sessions into episode memories. \
         Run `ollama pull {model}` to turn that on, or set distill_episodes = false under \
         [memory] to stop asking",
        approximate_size(model).unwrap_or("size unknown")
    )
}

pub fn unreachable_for_distillation_only(url: &str, why: &str, optional: &[String]) -> String {
    format!(
        "Ollama is not reachable at {url} ({why}). Embedding does not need it here; only \
         optional session distillation does. {}",
        optional
            .iter()
            .map(|m| optional_model_advice(m))
            .collect::<Vec<_>>()
            .join(". ")
    )
}

pub fn approximate_size(model: &str) -> Option<&'static str> {
    if let Some(choice) = crate::embed::choice::for_model(model) {
        return Some(choice.size);
    }
    match model {
        "qwen3:4b-instruct" | "qwen3:4b" => Some("about 2.5 GB"),
        _ => None,
    }
}

pub fn version(url: &str) -> Option<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;
    let v: serde_json::Value = client
        .get(format!("{}/api/version", url.trim_end_matches('/')))
        .send()
        .and_then(|r| r.error_for_status())
        .ok()?
        .json()
        .ok()?;
    v["version"].as_str().map(str::to_string)
}

fn too_new_for(running: Option<&str>, model: &str) -> Option<String> {
    let choice = crate::embed::choice::for_model(model)?;
    let min = choice.min_ollama?;
    let running = running?;
    choice.needs_newer_ollama_than(running).then(|| {
        format!(
            "{model} needs Ollama {min} or newer and this one is {running}; upgrade it \
             (https://ollama.com/download, or `brew upgrade ollama`), restart `ollama serve`, \
             then run `ollama pull {model}`"
        )
    })
}

fn split_by_ollama_version(url: &str, models: Vec<String>) -> (Vec<String>, Vec<String>) {
    if !models
        .iter()
        .any(|m| crate::embed::choice::for_model(m).is_some_and(|c| c.min_ollama.is_some()))
    {
        return (models, Vec::new());
    }
    let running = version(url);
    let mut pullable = Vec::new();
    let mut refusals = Vec::new();
    for m in models {
        match too_new_for(running.as_deref(), &m) {
            Some(why) => refusals.push(why),
            None => pullable.push(m),
        }
    }
    (pullable, refusals)
}

fn with_sizes(models: &[String]) -> String {
    models
        .iter()
        .map(|m| match approximate_size(m) {
            Some(size) => format!("{m} ({size})"),
            None => format!("{m} (size unknown)"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn probe(url: &str, models: &[String]) -> OllamaState {
    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
    {
        Ok(c) => c,
        Err(e) => return OllamaState::Unreachable(e.to_string()),
    };
    let tags = format!("{}/api/tags", url.trim_end_matches('/'));
    let v: serde_json::Value = match client.get(&tags).send().and_then(|r| r.error_for_status()) {
        Ok(r) => match r.json() {
            Ok(v) => v,
            Err(e) => return OllamaState::Unreachable(format!("{tags}: {e}")),
        },
        Err(e) => return OllamaState::Unreachable(e.to_string()),
    };
    let present: Vec<String> = v["models"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["name"].as_str().or_else(|| m["model"].as_str()))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let missing = models
        .iter()
        .filter(|m| {
            !present
                .iter()
                .any(|p| p == *m || p.trim_end_matches(":latest") == m.trim_end_matches(":latest"))
        })
        .cloned()
        .collect();
    OllamaState::Reachable { missing }
}

pub fn pull(model: &str) -> Result<()> {
    let status = std::process::Command::new("ollama")
        .arg("pull")
        .arg(model)
        .status()
        .context("cannot run `ollama pull`; is the ollama CLI installed?")?;
    if !status.success() {
        bail!("`ollama pull {model}` exited {status}");
    }
    Ok(())
}

pub fn status_lines(url: &str, models: &OllamaModels) -> Vec<String> {
    if models.is_empty() {
        return Vec::new();
    }
    let missing = match probe(url, &models.all()) {
        OllamaState::Unreachable(why) if models.only_optional() => {
            return vec![unreachable_for_distillation_only(
                url,
                &why,
                &models.optional,
            )];
        }
        OllamaState::Unreachable(why) => {
            return vec![format!(
                "unreachable at {url} — start it with `ollama serve` ({why})"
            )];
        }
        OllamaState::Reachable { missing } => missing,
    };
    if missing.is_empty() {
        return vec![format!("reachable at {url}, model(s) present")];
    }
    let (missing_required, missing_optional): (Vec<String>, Vec<String>) = missing
        .into_iter()
        .partition(|m| models.required.contains(m));
    let mut lines = Vec::new();
    let (missing_required, refusals) = split_by_ollama_version(url, missing_required);
    if missing_required.is_empty() && refusals.is_empty() {
        lines.push(reachable_with(url, &models.required));
    }
    lines.extend(refusals);
    if !missing_required.is_empty() {
        lines.push(format!(
            "reachable, missing model(s): {} — run: {}",
            missing_required.join(", "),
            missing_required
                .iter()
                .map(|m| format!("ollama pull {m}"))
                .collect::<Vec<_>>()
                .join(" && ")
        ));
    }
    lines.extend(missing_optional.iter().map(|m| optional_model_advice(m)));
    lines
}

fn reachable_with(url: &str, required: &[String]) -> String {
    if required.is_empty() {
        format!("reachable at {url}")
    } else {
        format!("reachable at {url}, {} present", required.join(", "))
    }
}

pub struct Consent {
    pub yes: bool,
    pub quiet: bool,
    pub confirm: fn(&str, bool) -> bool,
}

impl Consent {
    fn to_pull_required(&self, question: &str) -> bool {
        self.yes || (self.confirm)(question, true)
    }

    fn to_pull_optional(&self, question: &str) -> bool {
        !self.yes && !self.quiet && (self.confirm)(question, false)
    }
}

pub fn preflight(
    url: &str,
    models: &OllamaModels,
    consent: &Consent,
    pull: fn(&str) -> Result<()>,
) -> (Vec<String>, Vec<String>) {
    let mut lines = Vec::new();
    let mut warnings = Vec::new();
    let all = models.all();
    let missing = match probe(url, &all) {
        OllamaState::Unreachable(why) if models.only_optional() => {
            warnings.push(unreachable_for_distillation_only(
                url,
                &why,
                &models.optional,
            ));
            return (lines, warnings);
        }
        OllamaState::Unreachable(why) => {
            warnings.push(format!(
                "Ollama is not reachable at {url} ({why}). Install it from https://ollama.com/download, \
                 start it with `ollama serve`, then run `rhebok install` again to pull the model."
            ));
            return (lines, warnings);
        }
        OllamaState::Reachable { missing } => missing,
    };
    if missing.is_empty() {
        lines.push(format!(
            "ollama:   reachable at {url}, model(s) present: {}",
            all.join(", ")
        ));
        return (lines, warnings);
    }
    let (missing_required, missing_optional): (Vec<String>, Vec<String>) = missing
        .into_iter()
        .partition(|m| models.required.contains(m));
    let (missing_required, refusals) = split_by_ollama_version(url, missing_required);
    warnings.extend(refusals);
    if !missing_required.is_empty() {
        if consent.to_pull_required(&format!("pull {}?", with_sizes(&missing_required))) {
            pull_each(&missing_required, pull, &mut lines, &mut warnings);
        } else {
            warnings.push(format!(
                "missing model(s): {} — run: {}",
                missing_required.join(", "),
                missing_required
                    .iter()
                    .map(|m| format!("ollama pull {m}"))
                    .collect::<Vec<_>>()
                    .join(" && ")
            ));
        }
    }
    for m in &missing_optional {
        let question = format!(
            "pull {}? It is optional and only distills finished sessions into episode memories",
            with_sizes(std::slice::from_ref(m))
        );
        if consent.to_pull_optional(&question) {
            pull_each(std::slice::from_ref(m), pull, &mut lines, &mut warnings);
        } else {
            lines.push(format!("ollama:   {}", optional_model_advice(m)));
        }
    }
    (lines, warnings)
}

fn pull_each(
    models: &[String],
    pull: fn(&str) -> Result<()>,
    lines: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    for m in models {
        match pull(m) {
            Ok(()) => lines.push(format!("ollama:   pulled {m}")),
            Err(e) => warnings.push(format!("{e:#}; run: ollama pull {m}")),
        }
    }
}

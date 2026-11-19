use super::choice::{self, EmbedChoice, CHOICES};
use anyhow::{bail, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum ModelSource {
    Preset(&'static EmbedChoice),
    Ollama(String),
    Endpoint(String),
}

pub fn parse(input: &str) -> Result<ModelSource> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(ModelSource::Preset(&CHOICES[0]));
    }
    if let Some(preset) = input
        .parse::<usize>()
        .ok()
        .and_then(|n| CHOICES.get(n.wrapping_sub(1)))
    {
        return Ok(ModelSource::Preset(preset));
    }
    if let Some(preset) = choice::find(input) {
        return Ok(ModelSource::Preset(preset));
    }
    let Some((scheme, rest)) = input.split_once("://") else {
        return ollama_model(input);
    };
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        bail!("`{input}` is not an http or https link");
    }
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match host {
        "ollama.com" => from_ollama_page(input, &segments),
        "huggingface.co" | "hf.co" => from_hugging_face_page(input, &segments),
        _ => Ok(ModelSource::Endpoint(
            input
                .split(['?', '#'])
                .next()
                .unwrap_or(input)
                .trim_end_matches('/')
                .to_string(),
        )),
    }
}

fn from_ollama_page(input: &str, segments: &[&str]) -> Result<ModelSource> {
    match segments {
        ["library", name, ..] => ollama_model(name),
        [namespace, name, ..] if !matches!(*namespace, "search" | "blog" | "download") => {
            ollama_model(&format!("{namespace}/{name}"))
        }
        _ => bail!(
            "`{input}` is not an Ollama model page; paste a link such as \
             https://ollama.com/library/nomic-embed-text"
        ),
    }
}

fn from_hugging_face_page(input: &str, segments: &[&str]) -> Result<ModelSource> {
    match segments {
        [org, repo, ..] => ollama_model(&format!("hf.co/{org}/{repo}")),
        _ => bail!(
            "`{input}` is not a Hugging Face model page; paste a link such as \
             https://huggingface.co/<organisation>/<model>"
        ),
    }
}

fn ollama_model(name: &str) -> Result<ModelSource> {
    let name = name.trim();
    if name.is_empty() || name.contains(char::is_whitespace) {
        bail!("`{name}` is not a model name");
    }
    Ok(match choice::for_model(name) {
        Some(preset) => ModelSource::Preset(preset),
        None => ModelSource::Ollama(name.to_string()),
    })
}

impl ModelSource {
    pub fn describe(&self) -> String {
        match self {
            ModelSource::Preset(c) => format!("{} ({})", c.label, c.model),
            ModelSource::Ollama(m) => format!("Ollama model {m}"),
            ModelSource::Endpoint(url) => format!("the embedding endpoint at {url}"),
        }
    }
}

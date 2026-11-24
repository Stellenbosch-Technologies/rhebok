use crate::config::Config;
use std::path::Path;

const EMBED_TEST_TEXT: &str = "rhebok connection test";

#[derive(Debug, Clone, serde::Serialize)]
pub struct EmbedTest {
    pub ok: bool,
    pub latency_ms: u64,
    pub dimensions: Option<usize>,
    pub backend: &'static str,
    pub url: Option<String>,
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub(in crate::dashboard) fn embed_target(cfg: &Config) -> (bool, Option<String>, Option<String>) {
    let remote = cfg.embed.remote.is_some() || cfg.embed.remote_error.is_some();
    let (url, model) = match &cfg.embed.remote {
        Some(r) => (Some(r.url.clone()), Some(r.model.clone())),
        None if remote => (None, None),
        None => (
            Some(cfg.embed.ollama_url.clone()),
            Some(cfg.embed.model.clone()),
        ),
    };
    (remote, url, model)
}

pub fn test(config_path: &Path) -> EmbedTest {
    let cfg = Config::load_from(config_path);
    let (remote, url, model) = embed_target(&cfg);
    let started = std::time::Instant::now();
    let result = crate::embed::for_config(&cfg.embed).and_then(|embedder| {
        let _priority = crate::index::QueryPriority::announce(&Config::db_path());
        embedder.embed_query(EMBED_TEST_TEXT)
    });
    let latency_ms = started.elapsed().as_millis() as u64;
    let backend = if remote { "remote" } else { "ollama" };
    match result {
        Ok(vector) => EmbedTest {
            ok: true,
            latency_ms,
            dimensions: Some(vector.len()),
            backend,
            url,
            model,
            error: None,
        },
        Err(e) => EmbedTest {
            ok: false,
            latency_ms,
            dimensions: None,
            backend,
            url,
            model,
            error: Some(format!("{e:#}")),
        },
    }
}

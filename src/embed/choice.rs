use crate::config::edit::Patch;
use serde::Serialize;
use toml_edit::value;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct EmbedChoice {
    pub id: &'static str,
    pub label: &'static str,
    pub model: &'static str,
    pub dimensions: usize,
    pub hook_threshold: f64,
    pub mcp_threshold: f64,
    pub size: &'static str,
    pub min_ollama: Option<&'static str>,
    pub summary: &'static str,
}

pub const QWEN3: EmbedChoice = EmbedChoice {
    id: "qwen3",
    label: "Qwen3-Embedding 0.6B",
    model: "qwen3-embedding:0.6b",
    dimensions: 512,
    hook_threshold: 0.66,
    mcp_threshold: 0.55,
    size: "about 640 MB",
    min_ollama: None,
    summary: "the default; runs on any recent Ollama",
};

pub const GEMMA: EmbedChoice = EmbedChoice {
    id: "gemma",
    label: "EmbeddingGemma 2 (270M, bf16)",
    model: "embeddinggemma-2:270m-bf16-text",
    dimensions: 512,
    hook_threshold: 0.76,
    mcp_threshold: 0.68,
    size: "about 575 MB",
    min_ollama: Some("0.40.0"),
    summary: "about 10% faster per query, same recall; needs Ollama 0.40 or newer",
};

pub const CHOICES: [EmbedChoice; 2] = [QWEN3, GEMMA];

pub fn find(id: &str) -> Option<&'static EmbedChoice> {
    let wanted = id.trim().to_ascii_lowercase();
    CHOICES.iter().find(|c| c.id == wanted || c.model == wanted)
}

pub fn for_model(model: &str) -> Option<&'static EmbedChoice> {
    CHOICES.iter().find(|c| c.model == model.trim())
}

pub fn ids() -> String {
    CHOICES.iter().map(|c| c.id).collect::<Vec<_>>().join(", ")
}

impl EmbedChoice {
    pub fn patch(&self) -> Patch {
        Patch {
            set: vec![
                ("embed.model".into(), value(self.model)),
                ("embed.dimensions".into(), value(self.dimensions as i64)),
                ("hook.threshold".into(), value(self.hook_threshold)),
                ("mcp.threshold".into(), value(self.mcp_threshold)),
            ],
            unset: vec!["embed.prefix_scheme".into()],
        }
    }

    pub fn starter_block(&self) -> String {
        format!(
            "\n# The embedding model chosen at install, and the relevance gates measured for it.\n\
             # Changing the model means `rhebok index --reindex`.\n\
             [embed]\nmodel = \"{}\"\ndimensions = {}\n\n[hook]\nthreshold = {}\n\n[mcp]\nthreshold = {}\n",
            self.model, self.dimensions, self.hook_threshold, self.mcp_threshold
        )
    }

    pub fn needs_newer_ollama_than(&self, running: &str) -> bool {
        match self.min_ollama {
            Some(min) => version_tuple(running) < version_tuple(min),
            None => false,
        }
    }
}

fn version_tuple(v: &str) -> (u64, u64, u64) {
    let mut parts = v
        .trim()
        .trim_start_matches('v')
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| p.parse::<u64>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::ollama::PrefixScheme;

    #[test]
    fn the_default_choice_is_the_default_config() {
        let cfg = crate::config::Config::default();
        assert_eq!(QWEN3.model, cfg.embed.model);
        assert_eq!(QWEN3.dimensions, cfg.embed.dimensions);
        assert_eq!(QWEN3.hook_threshold as f32, cfg.hook.threshold);
        assert_eq!(QWEN3.mcp_threshold as f32, cfg.mcp.threshold);
    }

    #[test]
    fn every_choice_detects_its_own_prefix_scheme() {
        assert_eq!(
            PrefixScheme::for_model(QWEN3.model),
            PrefixScheme::Qwen3Instruct
        );
        assert_eq!(PrefixScheme::for_model(GEMMA.model), PrefixScheme::Gemma);
    }

    #[test]
    fn every_choice_gates_mcp_below_the_hook() {
        for c in CHOICES {
            assert!(c.mcp_threshold < c.hook_threshold, "{}", c.id);
        }
    }

    #[test]
    fn a_choice_is_found_by_id_or_model() {
        assert_eq!(find("gemma"), Some(&GEMMA));
        assert_eq!(find(" Qwen3 "), Some(&QWEN3));
        assert_eq!(find("embeddinggemma-2:270m-bf16-text"), Some(&GEMMA));
        assert_eq!(find("nomic"), None);
    }

    #[test]
    fn gemma_refuses_an_ollama_older_than_0_40() {
        assert!(GEMMA.needs_newer_ollama_than("0.32.15"));
        assert!(GEMMA.needs_newer_ollama_than("0.39.9"));
        assert!(!GEMMA.needs_newer_ollama_than("0.40.0"));
        assert!(!GEMMA.needs_newer_ollama_than("0.40.16"));
        assert!(!GEMMA.needs_newer_ollama_than("1.0.0-rc1"));
        assert!(!QWEN3.needs_newer_ollama_than("0.1.0"));
    }

    #[test]
    fn the_catalog_reaches_the_dashboard_with_its_gates_as_written() {
        let json = serde_json::to_string(&GEMMA).unwrap();
        assert!(json.contains(r#""hook_threshold":0.76"#), "{json}");
        assert!(json.contains(r#""mcp_threshold":0.68"#), "{json}");
    }

    #[test]
    fn the_patch_sets_the_model_its_width_and_both_gates_and_clears_the_scheme() {
        let p = GEMMA.patch();
        let keys: Vec<&str> = p.set.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            [
                "embed.model",
                "embed.dimensions",
                "hook.threshold",
                "mcp.threshold"
            ]
        );
        assert_eq!(p.set[2].1.as_float(), Some(0.76));
        assert_eq!(p.unset, ["embed.prefix_scheme"]);
    }
}

use rhebok::config::{Config, EmbedConfig};
use rhebok::embed::calibrate::GateEstimate;
use rhebok::setup::install::{InstallReport, STARTER_CONFIG};
use rhebok::setup::model::{apply_custom, measure_gates, pick_model, write_endpoint};
use std::os::unix::fs::PermissionsExt;

fn nomic_like() -> anyhow::Result<GateEstimate> {
    Ok(GateEstimate {
        hook: 0.77,
        mcp: 0.70,
        measured_hook: 0.77,
        measured_mcp: 0.70,
        unrelated_mean: 0.73,
        pairs: 24,
        matched_below_hook: 0,
    })
}

fn unreachable(_: &EmbedConfig) -> anyhow::Result<GateEstimate> {
    anyhow::bail!("Ollama is unreachable")
}

fn bag_of_words(text: &str) -> Vec<f32> {
    with_shared_part(text, 0.9)
}

fn bag_of_words_alone(text: &str) -> Vec<f32> {
    with_shared_part(text, 0.0)
}

fn with_shared_part(text: &str, shared: f32) -> Vec<f32> {
    let mut v = vec![0.0f32; 257];
    for word in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3 && !matches!(*w, "search" | "query" | "document"))
    {
        let h = word
            .to_lowercase()
            .bytes()
            .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
        v[(h % 256) as usize] += 1.0;
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
    let mut v: Vec<f32> = v.iter().map(|x| x / norm).collect();
    v[256] = shared;
    v
}

fn starter_on(dir: &std::path::Path, ollama: &str) -> std::path::PathBuf {
    let p = dir.join("config.toml");
    std::fs::write(
        &p,
        format!("{STARTER_CONFIG}\n[embed]\nollama_url = \"{ollama}\"\n"),
    )
    .unwrap();
    p
}

fn starter(dir: &std::path::Path) -> std::path::PathBuf {
    let p = dir.join("config.toml");
    std::fs::write(&p, STARTER_CONFIG).unwrap();
    p
}

#[test]
fn a_custom_model_sets_its_name_its_native_width_and_gates_estimated_for_it() {
    let t = tempfile::tempdir().unwrap();
    let config = starter(t.path());
    let mut r = InstallReport::default();
    apply_custom(
        &config,
        "nomic-embed-text",
        768,
        true,
        |_| nomic_like(),
        &mut r,
    );
    let cfg = Config::load_from(&config);
    assert_eq!(cfg.embed.model, "nomic-embed-text");
    assert_eq!(cfg.embed.dimensions, 768);
    assert_eq!(cfg.embed.prefix_scheme, None);
    assert!(
        r.lines
            .iter()
            .any(|l| l.contains("nomic-embed-text, 768 dimensions")),
        "{r:?}"
    );
    assert!(
        r.lines.iter().any(|l| l.starts_with("gates:")
            && l.contains("hook 0.77 and MCP 0.7,")
            && l.contains("coarse estimate")
            && l.contains("rhebok bench")),
        "{r:?}"
    );
    assert_eq!(cfg.hook.threshold, 0.77);
    assert_eq!(cfg.mcp.threshold, 0.70);
    assert!(
        r.warnings.is_empty(),
        "a first install has no index to redo: {r:?}"
    );
    let text = std::fs::read_to_string(&config).unwrap();
    assert!(
        text.contains("# Directories and files to index"),
        "comments survive: {text}"
    );
}

#[test]
fn changing_the_model_of_an_existing_install_says_to_reindex() {
    let t = tempfile::tempdir().unwrap();
    let config = starter(t.path());
    let mut r = InstallReport::default();
    apply_custom(&config, "bge-m3", 1024, false, |_| nomic_like(), &mut r);
    assert!(
        r.warnings
            .iter()
            .any(|w| w.contains("rhebok index --reindex") && w.contains("bge-m3")),
        "{r:?}"
    );
}

#[test]
fn choosing_the_model_already_configured_changes_nothing_and_warns_about_nothing() {
    let t = tempfile::tempdir().unwrap();
    let config = starter(t.path());
    apply_custom(
        &config,
        "bge-m3",
        1024,
        true,
        |_| nomic_like(),
        &mut InstallReport::default(),
    );
    let before = std::fs::read_to_string(&config).unwrap();
    let mut r = InstallReport::default();
    apply_custom(&config, "bge-m3", 1024, false, |_| nomic_like(), &mut r);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), before);
    assert!(r.warnings.is_empty(), "{r:?}");
}

#[test]
fn an_endpoint_is_written_to_a_private_env_file_that_resolves_back() {
    let t = tempfile::tempdir().unwrap();
    let config = t.path().join("rhebok").join("config.toml");
    write_endpoint(
        &config,
        "http://gpu-box:1234",
        "text-embedding-bge-m3",
        None,
    )
    .unwrap();
    let env = rhebok::env_file::env_path(&config);
    assert_eq!(
        std::fs::metadata(&env).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let remote = rhebok::env_file::resolve(&config).unwrap().unwrap();
    assert_eq!(remote.url, "http://gpu-box:1234");
    assert_eq!(remote.model, "text-embedding-bge-m3");
    assert_eq!(remote.token, None);

    write_endpoint(&config, "http://gpu-box:1234", "m", Some("secret")).unwrap();
    assert_eq!(
        rhebok::env_file::resolve(&config).unwrap().unwrap().token,
        Some("secret".to_string())
    );
}

#[test]
fn a_model_is_picked_by_number_by_name_or_by_enter_when_there_is_only_one() {
    let two = vec!["a-embed".to_string(), "b-embed".to_string()];
    assert_eq!(pick_model(&two, "2\n"), Some("b-embed".into()));
    assert_eq!(pick_model(&two, " c-embed \n"), Some("c-embed".into()));
    assert_eq!(pick_model(&two, "\n"), None);
    assert_eq!(pick_model(&two, "7\n"), None);
    let one = vec!["only".to_string()];
    assert_eq!(pick_model(&one, "\n"), Some("only".into()));
    assert_eq!(pick_model(&[], "typed-name\n"), Some("typed-name".into()));
}

#[test]
fn the_gates_come_from_the_models_own_scores_on_the_built_in_pairs() {
    let ollama = crate::common::fake_ollama_embedding(bag_of_words);
    let t = tempfile::tempdir().unwrap();
    let config = starter_on(t.path(), &ollama);
    let mut r = InstallReport::default();
    apply_custom(
        &config,
        "nomic-embed-text",
        257,
        true,
        measure_gates,
        &mut r,
    );
    let cfg = Config::load_from(&config);
    assert!(
        cfg.hook.threshold > 0.70,
        "a model whose unrelated text scores about 0.75 keeps Qwen3's 0.66: {r:?}"
    );
    assert!(cfg.mcp.threshold < cfg.hook.threshold, "{r:?}");
    assert!(cfg.mcp.threshold > 0.55, "{r:?}");
    assert!(
        r.lines
            .iter()
            .any(|l| l.contains("estimated for nomic-embed-text from 24")),
        "{r:?}"
    );
    assert!(r.warnings.is_empty(), "{r:?}");
}

#[test]
fn a_model_that_cannot_be_probed_keeps_qwen3s_gates_and_says_how_to_retry() {
    let t = tempfile::tempdir().unwrap();
    let config = starter(t.path());
    let mut r = InstallReport::default();
    apply_custom(&config, "nomic-embed-text", 768, true, unreachable, &mut r);
    let cfg = Config::load_from(&config);
    assert_eq!(cfg.embed.model, "nomic-embed-text");
    assert_eq!(cfg.hook.threshold, 0.66);
    assert_eq!(cfg.mcp.threshold, 0.55);
    assert!(
        r.lines.iter().any(|l| l.contains("measured for Qwen3")),
        "{r:?}"
    );
    assert!(
        r.warnings
            .iter()
            .any(|w| w.contains("Ollama is unreachable")
                && w.contains("rhebok install --embed-model nomic-embed-text")),
        "{r:?}"
    );
}

#[test]
fn reinstalling_the_same_model_estimates_only_gates_still_left_at_qwen3s() {
    let t = tempfile::tempdir().unwrap();
    let config = starter(t.path());
    apply_custom(
        &config,
        "nomic-embed-text",
        768,
        true,
        unreachable,
        &mut InstallReport::default(),
    );
    let mut r = InstallReport::default();
    apply_custom(
        &config,
        "nomic-embed-text",
        768,
        false,
        |_| nomic_like(),
        &mut r,
    );
    assert_eq!(Config::load_from(&config).hook.threshold, 0.77);
    assert!(r.warnings.is_empty(), "{r:?}");

    let tuned = std::fs::read_to_string(&config)
        .unwrap()
        .replace("threshold = 0.77", "threshold = 0.8");
    std::fs::write(&config, &tuned).unwrap();
    let mut r = InstallReport::default();
    apply_custom(
        &config,
        "nomic-embed-text",
        768,
        false,
        |_| -> anyhow::Result<GateEstimate> { panic!("tuned gates are never re-estimated") },
        &mut r,
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), tuned);
    assert!(
        r.lines
            .iter()
            .any(|l| l.contains("hook 0.8 and MCP 0.7, kept from")),
        "{r:?}"
    );
}

#[test]
fn a_model_that_does_not_separate_matched_from_unrelated_pairs_is_warned_about() {
    let t = tempfile::tempdir().unwrap();
    let config = starter(t.path());
    let mut r = InstallReport::default();
    apply_custom(
        &config,
        "weak-embed",
        384,
        true,
        |_| {
            Ok(GateEstimate {
                matched_below_hook: 12,
                ..nomic_like()?
            })
        },
        &mut r,
    );
    assert!(
        r.warnings
            .iter()
            .any(|w| w.contains("12 of the 24") && w.contains("rhebok bench")),
        "{r:?}"
    );
}

#[test]
fn a_model_that_scores_unrelated_text_low_keeps_qwen3s_gates_and_says_why() {
    let ollama = crate::common::fake_ollama_embedding(bag_of_words_alone);
    let t = tempfile::tempdir().unwrap();
    let config = starter_on(t.path(), &ollama);
    let mut r = InstallReport::default();
    apply_custom(&config, "all-minilm", 257, true, measure_gates, &mut r);
    let cfg = Config::load_from(&config);
    assert_eq!(cfg.hook.threshold, 0.66, "{r:?}");
    assert_eq!(cfg.mcp.threshold, 0.55, "{r:?}");
    assert!(
        r.lines.iter().any(
            |l| l.starts_with("gates:    hook 0.66 and MCP 0.55, for all-minilm")
                && l.contains("came out looser than Qwen3")
                && l.contains("so it kept 0.66")
                && l.contains("so it kept 0.55")
                && l.contains("rhebok bench")
        ),
        "{r:?}"
    );
}

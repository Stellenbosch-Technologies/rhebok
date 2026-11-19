use rhebok::config::Config;
use rhebok::setup::install::{InstallReport, STARTER_CONFIG};
use rhebok::setup::model::{apply_custom, pick_model, write_endpoint};
use std::os::unix::fs::PermissionsExt;

fn starter(dir: &std::path::Path) -> std::path::PathBuf {
    let p = dir.join("config.toml");
    std::fs::write(&p, STARTER_CONFIG).unwrap();
    p
}

#[test]
fn a_custom_model_sets_its_name_and_native_width_and_says_the_gates_are_uncalibrated() {
    let t = tempfile::tempdir().unwrap();
    let config = starter(t.path());
    let mut r = InstallReport::default();
    apply_custom(&config, "nomic-embed-text", 768, true, &mut r);
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
        r.lines
            .iter()
            .any(|l| l.starts_with("gates:") && l.contains("rhebok bench")),
        "{r:?}"
    );
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
    apply_custom(&config, "bge-m3", 1024, false, &mut r);
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
    apply_custom(&config, "bge-m3", 1024, true, &mut InstallReport::default());
    let before = std::fs::read_to_string(&config).unwrap();
    let mut r = InstallReport::default();
    apply_custom(&config, "bge-m3", 1024, false, &mut r);
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

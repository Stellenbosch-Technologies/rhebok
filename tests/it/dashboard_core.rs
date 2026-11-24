use rhebok::config::Config;
use rhebok::dashboard::core::config::{check, save, ConfigCheck, ConfigSave};
use rhebok::dashboard::core::{ignore, DashboardError};

fn config_in(dir: &tempfile::TempDir, text: &str) -> std::path::PathBuf {
    let path = dir.path().join("config.toml");
    std::fs::write(&path, text).unwrap();
    path
}

fn etag(path: &std::path::Path) -> String {
    rhebok::config::edit::etag_of(std::fs::read_to_string(path).ok().as_deref())
}

fn patch(etag: String, set: serde_json::Value) -> ConfigSave {
    serde_json::from_value(serde_json::json!({ "etag": etag, "set": set })).unwrap()
}

#[test]
fn a_stale_etag_is_a_config_changed_error_that_carries_the_current_etag() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_in(&dir, "[hook]\nquality = 1\n");
    let stale = etag(&path);
    std::fs::write(&path, "[hook]\nquality = 2\n").unwrap();
    let err = save(
        &path,
        patch(stale, serde_json::json!({ "hook.threshold": 0.7 })),
    )
    .unwrap_err();
    match err {
        DashboardError::ConfigChanged { etag: current, .. } => assert_eq!(current, etag(&path)),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "[hook]\nquality = 2\n"
    );
}

#[test]
fn a_value_out_of_range_is_unprocessable_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_in(&dir, "[hook]\nquality = 1\n");
    let err = save(
        &path,
        patch(etag(&path), serde_json::json!({ "hook.quality": 9 })),
    )
    .unwrap_err();
    assert!(matches!(err, DashboardError::Unprocessable(e) if !e.is_empty()));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "[hook]\nquality = 1\n"
    );
}

#[test]
fn a_good_save_answers_with_the_new_view() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_in(&dir, "[hook]\nquality = 1\n");
    let saved = save(
        &path,
        patch(etag(&path), serde_json::json!({ "hook.quality": 2 })),
    )
    .unwrap();
    assert_eq!(saved.0["etag"], etag(&path));
    assert_eq!(saved.0["file"]["hook"]["quality"], 2);
}

#[test]
fn check_reports_what_a_patch_would_introduce_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_in(&dir, "[hook]\nquality = 1\n");
    let req: ConfigCheck =
        serde_json::from_value(serde_json::json!({ "set": { "hook.quality": 9 } })).unwrap();
    let checked = check(&path, req).unwrap();
    assert!(!checked.blocking.is_empty());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "[hook]\nquality = 1\n"
    );
}

#[test]
fn an_ignore_pattern_that_matches_nothing_is_explained_not_failed() {
    let preview = ignore::preview(&Config::default(), "   ").unwrap();
    assert!(preview.error.is_some());
    assert_eq!((preview.matches, preview.of), (0, 0));
    assert!(preview.sample.is_empty());
}

mod agents {
    use rhebok::dashboard::core::agents::{connect, disconnect, list, AgentRequest};
    use rhebok::dashboard::core::{AgentRefusal, DashboardError};
    use rhebok::setup::agents::cursor::Cursor;
    use rhebok::setup::agents::AgentEnv;
    use rhebok::setup::Paths;

    fn env() -> (tempfile::TempDir, AgentEnv) {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let paths = Paths::at(
            &home.join(".local/share/rhebok"),
            vec![],
            home.join(".claude/plugins/cache/rhebok"),
        );
        std::fs::create_dir_all(paths.bin.parent().unwrap()).unwrap();
        std::fs::write(&paths.bin, "#!/bin/sh\n").unwrap();
        let env = AgentEnv::at(&home, paths, vec![]);
        (t, env)
    }

    fn request(id: &str) -> AgentRequest {
        serde_json::from_value(serde_json::json!({ "id": id })).unwrap()
    }

    fn refusal(e: DashboardError) -> (AgentRefusal, String) {
        match e {
            DashboardError::AgentRefused { kind, message } => (kind, message),
            other => panic!("expected AgentRefused, got {other:?}"),
        }
    }

    #[test]
    fn the_list_names_every_agent_and_the_snippets() {
        let (_t, env) = env();
        let v = serde_json::to_value(list(&env)).unwrap();
        assert!(v["agents"].as_array().unwrap().len() >= 5);
        assert!(v["snippets"]["mcp_json"].is_string());
    }

    #[test]
    fn connecting_then_disconnecting_changes_the_status() {
        let (_t, env) = env();
        let done = connect(&env, request("cursor")).unwrap();
        assert_eq!(done.agent["status"]["state"], "connected");
        let done = disconnect(&env, request("cursor")).unwrap();
        assert_ne!(done.agent["status"]["state"], "connected");
    }

    #[test]
    fn a_file_that_does_not_parse_is_an_unparseable_refusal_and_stays_byte_identical() {
        let (_t, env) = env();
        let file = Cursor::config_file(&env);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let garbage = "{ \"mcpServers\": { // a comment\n";
        std::fs::write(&file, garbage).unwrap();
        let (kind, message) = refusal(connect(&env, request("cursor")).unwrap_err());
        assert_eq!(kind, AgentRefusal::Unparseable);
        assert!(message.contains(&file.display().to_string()));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), garbage);
        let (kind, _) = refusal(disconnect(&env, request("cursor")).unwrap_err());
        assert_eq!(kind, AgentRefusal::Unparseable);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), garbage);
    }

    #[test]
    fn an_unknown_agent_an_unwanted_option_and_a_blank_id_are_each_refused_by_kind() {
        let (_t, env) = env();
        let (kind, _) = refusal(connect(&env, request("vim")).unwrap_err());
        assert_eq!(kind, AgentRefusal::UnknownAgent);
        let with_instructions: AgentRequest =
            serde_json::from_value(serde_json::json!({ "id": "cursor", "instructions": true }))
                .unwrap();
        let (kind, _) = refusal(connect(&env, with_instructions).unwrap_err());
        assert_eq!(kind, AgentRefusal::BadOption);
        match connect(&env, request("  ")).unwrap_err() {
            DashboardError::BadRequest(m) => assert_eq!(m, "no agent id given"),
            other => panic!("{other:?}"),
        }
    }
}

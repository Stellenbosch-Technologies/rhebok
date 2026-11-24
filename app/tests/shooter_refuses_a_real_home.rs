#![cfg(feature = "shoot")]

use std::process::Command;

#[test]
fn an_agent_writing_shot_refuses_a_home_that_is_not_a_sandbox() {
    let home = tempfile::tempdir().expect("temporary home");
    let out = home.path().join("x.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rhebok-app"))
        .args(["--screenshot", "integrations"])
        .arg(&out)
        .env("HOME", home.path())
        .env("RHEBOK_PARITY_INTEGRATIONS", "disconnect")
        .output()
        .expect("run rhebok-app");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("is not a throwaway rhebok-e2e sandbox"),
        "{stderr}"
    );
    assert!(!out.exists());
}

#[test]
fn a_settings_writing_shot_refuses_a_home_that_is_not_a_sandbox() {
    for state in ["conflict", "reindex"] {
        let home = tempfile::tempdir().expect("temporary home");
        let out = home.path().join("x.png");
        let output = Command::new(env!("CARGO_BIN_EXE_rhebok-app"))
            .args(["--screenshot", "settings"])
            .arg(&out)
            .env("HOME", home.path())
            .env("RHEBOK_PARITY_SETTINGS", state)
            .output()
            .expect("run rhebok-app");
        assert_eq!(output.status.code(), Some(1), "{state}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("saves config.toml or starts an index")
                && stderr.contains("is not a throwaway rhebok-e2e sandbox"),
            "{state}: {stderr}"
        );
        assert!(!out.exists(), "{state}");
    }
}

#[test]
fn a_writing_shot_refuses_a_config_or_database_outside_the_sandbox() {
    let sandbox = tempfile::Builder::new()
        .prefix("rhebok-e2e-")
        .tempdir()
        .expect("sandbox root");
    let home = sandbox.path().join("home");
    std::fs::create_dir_all(&home).expect("sandbox home");
    let outside = tempfile::tempdir().expect("a directory outside the sandbox");
    let inside_config = home.join(".config/rhebok/config.toml");
    let inside_db = sandbox.path().join("data/db");
    let cases = [
        (
            "settings",
            "RHEBOK_PARITY_SETTINGS",
            "conflict",
            outside.path().join("config.toml"),
            inside_db.clone(),
            "RHEBOK_CONFIG",
        ),
        (
            "settings",
            "RHEBOK_PARITY_SETTINGS",
            "reindex",
            inside_config.clone(),
            outside.path().join("db"),
            "RHEBOK_DB",
        ),
        (
            "integrations",
            "RHEBOK_PARITY_INTEGRATIONS",
            "change",
            inside_config.clone(),
            outside.path().join("db"),
            "RHEBOK_DB",
        ),
    ];
    for (tab, variable, state, config, db, named) in cases {
        let out = sandbox.path().join("x.png");
        let output = Command::new(env!("CARGO_BIN_EXE_rhebok-app"))
            .args(["--screenshot", tab])
            .arg(&out)
            .env("HOME", &home)
            .env("CODEX_HOME", home.join(".codex"))
            .env("RHEBOK_CONFIG", &config)
            .env("RHEBOK_DB", &db)
            .env(variable, state)
            .output()
            .expect("run rhebok-app");
        assert_eq!(output.status.code(), Some(1), "{tab} {state}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(named) && stderr.contains("outside the rhebok-e2e sandbox"),
            "{tab} {state}: {stderr}"
        );
        assert!(!out.exists(), "{tab} {state}");
    }
    assert!(!outside.path().join("config.toml").exists());
    assert!(!outside.path().join("db").exists());
}

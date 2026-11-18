use crate::common;
use assert_cmd::Command;
use rhebok::prehook::{command_hook_injects, mark_mod_running, marker, prompt_injectors, Injector};
use std::path::{Path, PathBuf};

struct Sandbox {
    _t: tempfile::TempDir,
    db: PathBuf,
    cfg: PathBuf,
}

const PROMPT: &str = "why did the connection pooler drop sessions";

fn indexed(injector: Option<&str>) -> Sandbox {
    let t = tempfile::tempdir().unwrap();
    let notes = t.path().join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(
        notes.join("pooling.md"),
        "# Pooling\n\nPgBouncer runs in transaction mode and drops session state.",
    )
    .unwrap();
    let ollama = common::fake_ollama();
    let prehook = injector
        .map(|i| format!("[prehook]\ninjector = \"{i}\"\n\n"))
        .unwrap_or_default();
    let cfg = t.path().join("config.toml");
    std::fs::write(
        &cfg,
        format!(
            "index_transcripts = false\nsources = [\"{}\"]\n\n{prehook}[embed]\nollama_url = \"{ollama}\"\n",
            notes.display()
        ),
    )
    .unwrap();
    let db = t.path().join("db");
    rhebok(&db, &cfg).arg("index").assert().success();
    Sandbox { _t: t, db, cfg }
}

fn rhebok(db: &Path, cfg: &Path) -> Command {
    let mut c = Command::cargo_bin("rhebok").unwrap();
    c.env("RHEBOK_DB", db).env("RHEBOK_CONFIG", cfg);
    c
}

fn command_hook_output(s: &Sandbox, session: &str) -> String {
    let out = rhebok(&s.db, &s.cfg)
        .args(["hook", "prompt"])
        .write_stdin(serde_json::json!({ "prompt": PROMPT, "session_id": session }).to_string())
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn the_command_hook_stands_aside_only_in_sessions_the_mod_is_running_in() {
    let s = indexed(None);
    mark_mod_running(&s.db, "with-mod").unwrap();
    assert!(
        command_hook_output(&s, "with-mod").is_empty(),
        "the mod injects in a session it marked, so the command hook must not"
    );
    assert!(
        command_hook_output(&s, "without-mod").contains("pooling.md"),
        "a session the mod never marked must still get context from the command hook"
    );
}

#[test]
fn injector_command_injects_even_where_the_mod_is_running() {
    let s = indexed(Some("command"));
    mark_mod_running(&s.db, "with-mod").unwrap();
    assert!(command_hook_output(&s, "with-mod").contains("pooling.md"));
}

#[test]
fn injector_off_injects_nothing_from_the_command_hook() {
    let s = indexed(Some("off"));
    assert!(command_hook_output(&s, "any").is_empty());
}

#[test]
fn mod_start_marks_the_session_only_when_the_mod_is_the_injector() {
    let s = indexed(None);
    let out = rhebok(&s.db, &s.cfg)
        .args(["hook", "mod-start"])
        .write_stdin(r#"{"session_id":"sess-1"}"#)
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["injector"], "mod");
    assert!(marker(&s.db, "sess-1").unwrap().exists());

    let off = indexed(Some("command"));
    rhebok(&off.db, &off.cfg)
        .args(["hook", "mod-start"])
        .write_stdin(r#"{"session_id":"sess-1"}"#)
        .assert()
        .success();
    assert!(!marker(&off.db, "sess-1").unwrap().exists());
}

#[test]
fn a_session_id_that_is_not_a_plain_token_never_names_a_file() {
    let db = Path::new("/tmp/x/db");
    assert!(marker(db, "../../etc/passwd").is_none());
    assert!(marker(db, "").is_none());
    assert!(marker(db, "a/b").is_none());
    assert_eq!(
        marker(db, "3f2a-9_c").unwrap(),
        PathBuf::from("/tmp/x/db.mod.3f2a-9_c")
    );
}

#[test]
fn the_decision_matrix() {
    let t = tempfile::tempdir().unwrap();
    let db = t.path().join("db");
    mark_mod_running(&db, "marked").unwrap();
    assert!(!command_hook_injects(Injector::Mod, &db, Some("marked")));
    assert!(command_hook_injects(Injector::Mod, &db, Some("unmarked")));
    assert!(command_hook_injects(Injector::Mod, &db, None));
    assert!(command_hook_injects(Injector::Command, &db, Some("marked")));
    assert!(!command_hook_injects(Injector::Off, &db, Some("unmarked")));
}

fn prehook_json(s: &Sandbox, session: &str) -> serde_json::Value {
    let out = rhebok(&s.db, &s.cfg)
        .args(["context", "--json", "--prehook", "--session", session])
        .write_stdin(PROMPT)
        .output()
        .unwrap();
    assert!(out.status.success());
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn the_prehook_path_searches_and_marks_only_while_the_mod_is_the_injector() {
    let on = indexed(None);
    let v = prehook_json(&on, "sess-2");
    assert_eq!(v["injector"], "mod");
    assert!(v["block"].as_str().unwrap().contains("pooling.md"), "{v}");
    assert!(marker(&on.db, "sess-2").unwrap().exists());

    let off = indexed(Some("command"));
    let v = prehook_json(&off, "sess-2");
    assert_eq!(v["injector"], "command");
    assert!(v["block"].is_null(), "{v}");
    assert!(v["hits"].as_array().unwrap().is_empty(), "{v}");
    assert!(!marker(&off.db, "sess-2").unwrap().exists());
}

#[test]
fn the_prehook_reads_the_injector_fresh_on_every_call() {
    let s = indexed(None);
    assert_eq!(prehook_json(&s, "sess-3")["injector"], "mod");
    let text = std::fs::read_to_string(&s.cfg).unwrap();
    std::fs::write(&s.cfg, format!("[prehook]\ninjector = \"off\"\n\n{text}")).unwrap();
    assert_eq!(prehook_json(&s, "sess-3")["injector"], "off");
}

#[test]
fn the_prehook_carries_the_status_style_it_is_set_to_whichever_injector_runs() {
    let s = indexed(None);
    assert_eq!(prehook_json(&s, "sess-4")["status_style"], "counts");
    let text = std::fs::read_to_string(&s.cfg).unwrap();
    for (injector, style) in [("mod", "words"), ("off", "glyph")] {
        std::fs::write(
            &s.cfg,
            format!("[prehook]\ninjector = \"{injector}\"\nstatus = \"{style}\"\n\n{text}"),
        )
        .unwrap();
        let v = prehook_json(&s, "sess-4");
        assert_eq!(v["injector"], injector, "{v}");
        assert_eq!(v["status_style"], style, "{v}");
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn prompt_hook(command: &str) -> String {
    serde_json::json!({ "hooks": { "UserPromptSubmit": [ { "hooks": [
        { "type": "command", "command": command }
    ] } ] } })
    .to_string()
}

#[test]
fn every_rhebok_or_brain_prompt_hook_is_found_so_doubles_can_be_named() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let rhebok_dir = home.join("cache/rhebok");
    let brain_dir = home.join("cache/brain");
    write(
        &home.join("plugins/installed_plugins.json"),
        &serde_json::json!({ "version": 2, "plugins": {
            "rhebok@rhebok": [ { "installPath": rhebok_dir } ],
            "brain@brain": [ { "installPath": brain_dir } ],
            "other@x": [ { "installPath": home.join("cache/other") } ]
        } })
        .to_string(),
    );
    write(
        &rhebok_dir.join("hooks/hooks.json"),
        &prompt_hook("\"/b/rhebok\" hook prompt"),
    );
    write(
        &brain_dir.join("hooks/hooks.json"),
        &prompt_hook("\"/b/brain\" hook prompt"),
    );
    write(
        &home.join("cache/other/hooks/hooks.json"),
        &prompt_hook("peon-ping"),
    );
    write(
        &home.join("settings.json"),
        &prompt_hook("/usr/local/bin/rhebok hook prompt"),
    );

    let found = prompt_injectors(home);
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found.iter().any(|f| f.starts_with("plugin rhebok@rhebok")));
    assert!(found.iter().any(|f| f.starts_with("plugin brain@brain")));
    assert!(found.iter().any(|f| f.starts_with("settings.json")));
}

#[test]
fn the_installed_plugin_carries_the_mod_with_the_binary_path_filled_in() {
    let files = rhebok::setup::plugin::render("9.9.9", Path::new("/opt/rhebok/bin/rhebok"));
    let module = files
        .iter()
        .find(|(name, _)| name == "hooks/rhebok-prehook.ts")
        .expect("the mod ships in the plugin");
    let text = String::from_utf8(module.1.clone()).unwrap();
    assert!(
        text.contains("const BIN = '/opt/rhebok/bin/rhebok'"),
        "{text}"
    );
    let hooks = files
        .iter()
        .find(|(name, _)| name == "hooks/hooks.json")
        .unwrap();
    let hooks: serde_json::Value = serde_json::from_slice(&hooks.1).unwrap();
    assert_eq!(hooks["modules"][0], "./rhebok-prehook.ts");
    assert!(hooks["hooks"]["UserPromptSubmit"].is_array());
}

#[test]
fn the_mod_a_marketplace_installs_runs_the_launcher_inside_its_own_plugin() {
    let shipped = std::fs::read_to_string("plugin/hooks/rhebok-prehook.ts").unwrap();
    assert!(
        shipped.contains("const BIN = '${CLAUDE_PLUGIN_ROOT}/scripts/rhebok.sh'"),
        "{shipped}"
    );
    assert!(
        shipped.contains("binaryFor(BIN, () => $.plugin.root)"),
        "{shipped}"
    );
    let hooks: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string("plugin/hooks/hooks.json").unwrap()).unwrap();
    assert_eq!(hooks["modules"][0], "./rhebok-prehook.ts");
}

use rhebok::dashboard::core::agents::{self as core_agents, AgentChanged, AgentRequest};
use rhebok::dashboard::core::{AgentRefusal, DashboardError};
use rhebok::hook::PromptAgent;
use rhebok::setup::agents::files::{backup_path, Unparseable};
use rhebok::setup::agents::{
    self, api, claude_desktop, claude_desktop::ClaudeDesktop, codex, codex::Codex, cursor::Cursor,
    gemini::Gemini, Agent, AgentEnv, AgentError, ConnectOptions, Status,
};
use rhebok::setup::Paths;
use std::path::{Path, PathBuf};

fn request(id: &str) -> AgentRequest {
    AgentRequest {
        id: id.to_string(),
        instructions: false,
    }
}

fn refused(result: Result<AgentChanged, DashboardError>) -> (AgentRefusal, String) {
    match result {
        Err(DashboardError::AgentRefused { kind, message }) => (kind, message),
        other => panic!("expected an agent refusal, got {other:?}"),
    }
}

struct Fx {
    _t: tempfile::TempDir,
    home: PathBuf,
    env: AgentEnv,
}

fn fixture() -> Fx {
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
    Fx { _t: t, home, env }
}

fn bin(fx: &Fx) -> String {
    fx.env.bin_str()
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

fn json(p: &Path) -> serde_json::Value {
    serde_json::from_str(&read(p)).unwrap()
}

fn write(p: &Path, text: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn none() -> ConnectOptions {
    ConnectOptions::default()
}

#[test]
fn connecting_cursor_creates_the_entry_and_connecting_again_changes_nothing() {
    let fx = fixture();
    let file = Cursor::config_file(&fx.env);
    assert_eq!(Cursor.status(&fx.env), Status::NotConnected);

    let change = Cursor.connect(&fx.env, &none()).unwrap();
    assert_eq!(change.files, vec![file.clone()]);
    assert!(change.backups.is_empty(), "nothing existed to back up");
    let v = json(&file);
    assert_eq!(v["mcpServers"]["rhebok"]["command"], bin(&fx));
    assert_eq!(
        v["mcpServers"]["rhebok"]["args"],
        serde_json::json!(["mcp"])
    );
    assert_eq!(v["mcpServers"]["rhebok"]["type"], "stdio");
    assert_eq!(Cursor.status(&fx.env), Status::Connected);

    let before = read(&file);
    let again = Cursor.connect(&fx.env, &none()).unwrap();
    assert!(again.is_empty(), "{again:?}");
    assert_eq!(read(&file), before);
    assert!(!backup_path(&file).exists());
}

const DESKTOP: &str = r#"{
    "globalShortcut": "Ctrl+Space",
    "mcpServers": {
        "other": {
            "command": "/usr/bin/other",
            "args": ["serve"],
            "env": { "Z": "1", "A": "2" }
        }
    },
    "zeta": 1
}
"#;

#[test]
fn a_json_connect_keeps_every_other_key_in_order_and_backs_up_the_original_once() {
    let fx = fixture();
    let file = ClaudeDesktop::config_file(&fx.env);
    write(&file, DESKTOP);

    let change = ClaudeDesktop.connect(&fx.env, &none()).unwrap();
    let backup = backup_path(&file);
    assert_eq!(change.backups, vec![backup.clone()]);
    assert_eq!(read(&backup), DESKTOP);

    let text = read(&file);
    let order = [
        "globalShortcut",
        "mcpServers",
        "other",
        "\"Z\"",
        "\"A\"",
        "rhebok",
        "zeta",
    ];
    let at: Vec<usize> = order.iter().map(|k| text.find(k).unwrap()).collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "order lost:\n{text}");
    assert!(
        text.contains("\n    \"globalShortcut\""),
        "indent lost:\n{text}"
    );

    let gone = ClaudeDesktop.disconnect(&fx.env).unwrap();
    assert_eq!(gone.files, vec![file.clone()]);
    assert!(
        gone.backups.is_empty(),
        "the first backup is never replaced"
    );
    let v = json(&file);
    assert!(v["mcpServers"].get("rhebok").is_none());
    assert_eq!(v["mcpServers"]["other"]["command"], "/usr/bin/other");
    assert_eq!(v["globalShortcut"], "Ctrl+Space");
    assert_eq!(v["zeta"], 1);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(DESKTOP).unwrap(),
        v,
        "disconnect restores the original content"
    );

    ClaudeDesktop.connect(&fx.env, &none()).unwrap();
    assert_eq!(
        read(&backup),
        DESKTOP,
        "a later write never clobbers the backup"
    );
}

#[test]
fn a_file_that_does_not_parse_is_refused_and_left_byte_identical() {
    let fx = fixture();
    let file = Cursor::config_file(&fx.env);
    let garbage = "{ \"mcpServers\": { // a comment\n";
    write(&file, garbage);

    let err = Cursor.connect(&fx.env, &none()).unwrap_err();
    assert!(err.downcast_ref::<Unparseable>().is_some(), "{err:#}");
    assert!(format!("{err:#}").contains(&file.display().to_string()));
    assert_eq!(read(&file), garbage);
    assert!(!backup_path(&file).exists());
    assert!(matches!(Cursor.status(&fx.env), Status::Broken(_)));

    Cursor.disconnect(&fx.env).unwrap_err();
    assert_eq!(read(&file), garbage);

    let (kind, message) = refused(core_agents::connect(&fx.env, request("cursor")));
    assert_eq!(kind, AgentRefusal::Unparseable, "{message}");
    assert_eq!(read(&file), garbage);
}

#[test]
fn a_non_object_top_level_is_refused() {
    let fx = fixture();
    let file = Gemini::config_file(&fx.env);
    write(&file, "[1, 2]\n");
    assert!(Gemini.connect(&fx.env, &none()).is_err());
    assert_eq!(read(&file), "[1, 2]\n");
}

#[test]
fn an_entry_pointing_at_another_binary_or_other_args_is_stale_and_connect_repairs_it() {
    let fx = fixture();
    let file = Cursor::config_file(&fx.env);
    write(
        &file,
        r#"{"mcpServers":{"rhebok":{"command":"/old/rhebok","args":["mcp"],"env":{"K":"v"}}}}"#,
    );
    match Cursor.status(&fx.env) {
        Status::Stale(r) => assert!(r.contains("/old/rhebok"), "{r}"),
        s => panic!("expected stale, got {s:?}"),
    }

    write(
        &file,
        &format!(
            r#"{{"mcpServers":{{"rhebok":{{"command":"{}","args":["serve"],"env":{{"K":"v"}}}}}}}}"#,
            bin(&fx)
        ),
    );
    match Cursor.status(&fx.env) {
        Status::Stale(r) => assert!(r.contains("args"), "{r}"),
        s => panic!("expected stale, got {s:?}"),
    }

    let change = Cursor.connect(&fx.env, &none()).unwrap();
    assert_eq!(change.files, vec![file.clone()]);
    assert_eq!(Cursor.status(&fx.env), Status::Connected);
    assert_eq!(json(&file)["mcpServers"]["rhebok"]["env"]["K"], "v");
}

#[test]
fn a_matching_entry_whose_binary_is_missing_is_broken() {
    let fx = fixture();
    Cursor.connect(&fx.env, &none()).unwrap();
    std::fs::remove_file(fx.env.bin()).unwrap();
    match Cursor.status(&fx.env) {
        Status::Broken(r) => assert!(r.contains("rhebok install"), "{r}"),
        s => panic!("expected broken, got {s:?}"),
    }
}

const CODEX_TOML: &str = r#"# my codex settings
model = "o3" # the good one

[mcp_servers.other]
# keep this server
command = "other"
args = ["x"]

[profiles.fast]
model = "mini"
"#;

#[test]
fn codex_keeps_comments_adds_the_server_and_hook_and_disconnect_restores_the_file() {
    let fx = fixture();
    let config = Codex::config_file(&fx.env);
    write(&config, CODEX_TOML);

    let change = Codex.connect(&fx.env, &none()).unwrap();
    assert!(change.files.contains(&config));
    assert!(change.files.contains(&Codex::hooks_file(&fx.env)));
    assert_eq!(change.backups, vec![backup_path(&config)]);
    let text = read(&config);
    for line in CODEX_TOML.lines() {
        assert!(text.contains(line), "lost `{line}`:\n{text}");
    }
    assert!(text.contains("[mcp_servers.rhebok]"), "{text}");
    let parsed: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(
        parsed["mcp_servers"]["rhebok"]["command"].as_str(),
        Some(bin(&fx).as_str())
    );
    assert_eq!(
        parsed["mcp_servers"]["rhebok"]["args"][0].as_str(),
        Some("mcp")
    );
    assert_eq!(
        parsed["mcp_servers"]["other"]["command"].as_str(),
        Some("other")
    );

    let hooks = json(&Codex::hooks_file(&fx.env));
    let cmd = hooks["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(cmd, format!("\"{}\" hook prompt --agent codex", bin(&fx)));
    assert_eq!(
        hooks["hooks"]["UserPromptSubmit"][0]["hooks"][0]["timeout"],
        5
    );
    assert_eq!(Codex.status(&fx.env), Status::Connected);
    assert!(
        !Codex::instructions_file(&fx.env).exists(),
        "instructions are opt-in"
    );

    assert!(Codex.connect(&fx.env, &none()).unwrap().is_empty());

    Codex.disconnect(&fx.env).unwrap();
    assert_eq!(read(&config), CODEX_TOML);
    assert_eq!(Codex.status(&fx.env), Status::NotConnected);
}

#[test]
fn codex_reports_a_missing_hook_or_another_binary_as_stale() {
    let fx = fixture();
    write(
        &Codex::config_file(&fx.env),
        &format!(
            "[mcp_servers.rhebok]\ncommand = \"{}\"\nargs = [\"mcp\"]\n",
            bin(&fx)
        ),
    );
    match Codex.status(&fx.env) {
        Status::Stale(r) => assert!(r.contains("prompt hook is missing"), "{r}"),
        s => panic!("expected stale, got {s:?}"),
    }
    Codex.connect(&fx.env, &none()).unwrap();
    write(
        &Codex::config_file(&fx.env),
        "[mcp_servers.rhebok]\ncommand = \"/elsewhere/rhebok\"\nargs = [\"mcp\"]\n",
    );
    match Codex.status(&fx.env) {
        Status::Stale(r) => assert!(r.contains("/elsewhere/rhebok"), "{r}"),
        s => panic!("expected stale, got {s:?}"),
    }

    Codex.connect(&fx.env, &none()).unwrap();
    assert_eq!(Codex.status(&fx.env), Status::Connected);
    write(
        &Codex::hooks_file(&fx.env),
        r#"{"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"\"/elsewhere/rhebok\" hook prompt --agent codex"}]}]}}"#,
    );
    match Codex.status(&fx.env) {
        Status::Stale(r) => assert!(r.contains("UserPromptSubmit hook runs"), "{r}"),
        s => panic!("expected stale, got {s:?}"),
    }
    Codex.connect(&fx.env, &none()).unwrap();
    let hooks = json(&Codex::hooks_file(&fx.env));
    let groups = hooks["hooks"]["UserPromptSubmit"].as_array().unwrap();
    assert_eq!(
        groups.len(),
        1,
        "the stale hook is repointed, not duplicated"
    );
    assert_eq!(
        groups[0]["hooks"][0]["command"],
        format!("\"{}\" hook prompt --agent codex", bin(&fx))
    );
}

#[test]
fn codex_refuses_a_config_that_is_not_toml() {
    let fx = fixture();
    let config = Codex::config_file(&fx.env);
    write(&config, "model = \n[[[");
    let err = Codex.connect(&fx.env, &none()).unwrap_err();
    assert!(err.downcast_ref::<Unparseable>().is_some(), "{err:#}");
    assert_eq!(read(&config), "model = \n[[[");
    assert!(
        !Codex::hooks_file(&fx.env).exists(),
        "nothing is written when one file is refused"
    );
}

#[test]
fn the_agents_md_block_is_added_and_removed_leaving_the_text_around_it() {
    let fx = fixture();
    let md = Codex::instructions_file(&fx.env);
    let mine = "# My rules\n\nAlways run the tests.\n";
    write(&md, mine);

    let opts = ConnectOptions { instructions: true };
    Codex.connect(&fx.env, &opts).unwrap();
    let text = read(&md);
    assert!(text.starts_with(mine), "{text}");
    assert!(text.contains(codex::BLOCK_START) && text.contains(codex::BLOCK_END));
    assert!(text.contains("rhebok_search") && text.contains("rhebok_remember"));
    assert_eq!(Codex.instructions(&fx.env), Some(true));

    assert!(Codex.connect(&fx.env, &opts).unwrap().is_empty());

    Codex.disconnect(&fx.env).unwrap();
    assert_eq!(read(&md), mine);
    assert_eq!(Codex.instructions(&fx.env), Some(false));

    let middle = format!("top\n{}bottom\n", codex::block_text());
    assert_eq!(codex::without_block(&middle).unwrap(), "top\nbottom\n");
    let edited = format!(
        "top\n{}\nold words\n{}\nbottom\n",
        codex::BLOCK_START,
        codex::BLOCK_END
    );
    assert_eq!(
        codex::with_block(&edited),
        format!("top\n{}bottom\n", codex::block_text())
    );
}

const GEMINI: &str = r#"{
  "theme": "dark",
  "hooks": {
    "BeforeAgent": [
      { "hooks": [ { "type": "command", "command": "mine.sh", "timeout": 100 } ] }
    ],
    "SessionStart": [
      { "hooks": [ { "type": "command", "command": "start.sh" } ] }
    ]
  }
}
"#;

#[test]
fn gemini_gets_the_server_and_a_before_agent_hook_beside_the_users_own() {
    let fx = fixture();
    let file = Gemini::config_file(&fx.env);
    write(&file, GEMINI);

    Gemini.connect(&fx.env, &none()).unwrap();
    let v = json(&file);
    assert_eq!(v["mcpServers"]["rhebok"]["command"], bin(&fx));
    let groups = v["hooks"]["BeforeAgent"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0]["hooks"][0]["command"], "mine.sh");
    assert_eq!(
        groups[1]["hooks"][0]["command"],
        format!("\"{}\" hook prompt --agent gemini", bin(&fx))
    );
    assert_eq!(groups[1]["hooks"][0]["timeout"], 5000);
    assert_eq!(Gemini.status(&fx.env), Status::Connected);

    Gemini.disconnect(&fx.env).unwrap();
    assert_eq!(
        json(&file),
        serde_json::from_str::<serde_json::Value>(GEMINI).unwrap()
    );
}

#[test]
fn instructions_are_refused_for_an_agent_that_has_no_instructions_file() {
    let fx = fixture();
    let err =
        agents::connect(&Cursor, &fx.env, &ConnectOptions { instructions: true }).unwrap_err();
    assert!(matches!(
        err.downcast_ref::<AgentError>(),
        Some(AgentError::BadOption(_))
    ));
    assert!(!Cursor::config_file(&fx.env).exists());
}

#[test]
fn the_api_lists_every_agent_and_round_trips_a_connect() {
    let fx = fixture();
    let v = api::list(&fx.env);
    let ids: Vec<&str> = v["agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["claude-code", "codex", "claude-desktop", "cursor", "gemini"]
    );
    for a in v["agents"].as_array().unwrap() {
        assert!(a["name"].is_string());
        assert!(a["detected"]["installed"].is_boolean());
        assert!(a["status"]["state"].is_string());
        assert!(a["capabilities"]["mcp"].as_bool().unwrap());
    }
    let codex = &v["agents"][1];
    assert_eq!(codex["instructions"], false);
    assert_eq!(codex["capabilities"]["prompt_hook"], true);
    assert_eq!(codex["capabilities"]["transcripts"], true);
    assert!(v["snippets"]["mcp_json"]
        .as_str()
        .unwrap()
        .contains(&bin(&fx)));
    assert!(v["snippets"]["codex_toml"]
        .as_str()
        .unwrap()
        .contains("[mcp_servers.rhebok]"));

    let v =
        serde_json::to_value(core_agents::connect(&fx.env, request("cursor")).unwrap()).unwrap();
    assert_eq!(v["agent"]["status"]["state"], "connected");
    assert_eq!(
        v["change"]["files"][0],
        Cursor::config_file(&fx.env).display().to_string()
    );
    assert_eq!(v["change"]["backups"], serde_json::json!([]));

    let v =
        serde_json::to_value(core_agents::disconnect(&fx.env, request("cursor")).unwrap()).unwrap();
    assert_eq!(v["agent"]["status"]["state"], "not_connected");

    assert_eq!(
        refused(core_agents::connect(&fx.env, request("vim"))).0,
        AgentRefusal::UnknownAgent
    );
    match core_agents::connect(&fx.env, request("")) {
        Err(DashboardError::BadRequest(m)) => assert_eq!(m, "no agent id given"),
        other => panic!("a blank id is a bad request: {other:?}"),
    }
    let with_instructions = AgentRequest {
        id: "cursor".to_string(),
        instructions: true,
    };
    assert_eq!(
        refused(core_agents::connect(&fx.env, with_instructions)).0,
        AgentRefusal::BadOption
    );
    assert_eq!(
        refused(core_agents::connect(&fx.env, request("claude-code"))).0,
        AgentRefusal::Unavailable
    );
}

#[test]
fn install_offers_detected_agents_and_quiet_only_repairs_stale_ones() {
    let fx = fixture();
    std::fs::create_dir_all(fx.home.join(".cursor")).unwrap();
    write(
        &Gemini::config_file(&fx.env),
        r#"{"mcpServers":{"rhebok":{"command":"/old/rhebok","args":["mcp"]}}}"#,
    );

    let quiet = agents::offer_after_install(&fx.env, true, true, |_, _| panic!("never asks"));
    assert!(quiet.warnings.is_empty(), "{:?}", quiet.warnings);
    assert_eq!(Cursor.status(&fx.env), Status::NotConnected);
    assert_eq!(Gemini.status(&fx.env), Status::Connected);

    let declined = agents::offer_after_install(&fx.env, false, false, |q, default_yes| {
        assert!(
            !default_yes,
            "connecting another agent must default to no: {q}"
        );
        false
    });
    assert_eq!(Cursor.status(&fx.env), Status::NotConnected);
    assert!(declined
        .lines
        .iter()
        .any(|l| l.contains("rhebok connect cursor")));

    agents::offer_after_install(&fx.env, true, false, |_, _| panic!("--yes never asks"));
    assert_eq!(Cursor.status(&fx.env), Status::Connected);
    assert_eq!(
        ClaudeDesktop.status(&fx.env),
        Status::NotConnected,
        "not detected"
    );

    let r = agents::disconnect_all_but_claude_code(&fx.env);
    assert_eq!(r.lines.len(), 2, "{:?}", r.lines);
    assert_eq!(Cursor.status(&fx.env), Status::NotConnected);
    assert_eq!(Gemini.status(&fx.env), Status::NotConnected);
}

#[test]
fn a_config_reached_through_a_symlink_is_edited_in_place() {
    let fx = fixture();
    let real = fx.home.join("dotfiles/mcp.json");
    write(&real, "{}\n");
    let link = Cursor::config_file(&fx.env);
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    Cursor.connect(&fx.env, &none()).unwrap();
    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(json(&real)["mcpServers"]["rhebok"]["command"], bin(&fx));
}

#[test]
fn each_agent_gets_its_own_hook_envelope_and_claude_codes_is_unchanged() {
    assert_eq!(
        PromptAgent::ClaudeCode.envelope(Some("ctx")).unwrap(),
        r#"{"hookSpecificOutput":{"additionalContext":"ctx","hookEventName":"UserPromptSubmit"}}"#
    );
    assert_eq!(PromptAgent::ClaudeCode.envelope(None), None);
    assert_eq!(
        PromptAgent::Codex.envelope(Some("ctx")),
        PromptAgent::ClaudeCode.envelope(Some("ctx"))
    );
    assert_eq!(
        PromptAgent::Gemini.envelope(Some("ctx")).unwrap(),
        r#"{"hookSpecificOutput":{"additionalContext":"ctx"}}"#
    );
    assert_eq!(PromptAgent::Gemini.envelope(None).unwrap(), "{}");
    let codex_stdin = r#"{"session_id":"s","cwd":"/","hook_event_name":"UserPromptSubmit","turn_id":"t","prompt":"how did I fix it"}"#;
    assert_eq!(
        PromptAgent::Codex.prompt_of(codex_stdin).as_deref(),
        Some("how did I fix it")
    );
    let gemini_stdin = r#"{"session_id":"s","hook_event_name":"BeforeAgent","timestamp":"x","prompt":"my notes on raft"}"#;
    assert_eq!(
        PromptAgent::Gemini.prompt_of(gemini_stdin).as_deref(),
        Some("my notes on raft")
    );
}

#[test]
fn claude_desktop_is_detected_from_the_environments_applications_folder() {
    let fx = fixture();
    assert!(!ClaudeDesktop.detect(&fx.env).installed);
    std::fs::create_dir_all(fx.env.applications.join("Claude.app")).unwrap();
    assert_eq!(
        ClaudeDesktop.detect(&fx.env).installed,
        cfg!(target_os = "macos")
    );
}

const DESKTOP_WITH_BR8N: &str = r#"{
  "mcpServers": {
    "br8n": { "command": "/old/br8n", "args": ["mcp"] },
    "other": { "command": "/usr/bin/other" }
  },
  "preferences": { "sidebarMode": "epitaxy" }
}
"#;

fn desktop_running(fx: &mut Fx) {
    fx.env.desktop_rewrites = |_| true;
}

#[test]
fn the_running_desktop_app_is_found_in_the_process_list_and_its_helpers_are_not() {
    let helpers = "/Applications/Claude.app/Contents/Frameworks/Claude Helper.app/Contents/MacOS/Claude Helper\n\
                   /Applications/Claude.app/Contents/Helpers/disclaimer\n\
                   /Users/u/Library/Application Support/Claude/claude-code/2.1.0/x/claude.app/Contents/MacOS/claude\n";
    assert!(!claude_desktop::lists_the_app(helpers));
    let with_app = format!("{helpers}/Applications/Claude.app/Contents/MacOS/Claude\n");
    assert!(claude_desktop::lists_the_app(&with_app));
    assert!(claude_desktop::lists_the_app(
        "/Users/u/Applications/Claude.app/Contents/MacOS/Claude   \n"
    ));
}

#[test]
fn a_desktop_config_outside_the_account_home_is_never_reported_as_rewritten() {
    let fx = fixture();
    let file = ClaudeDesktop::config_file(&fx.env);
    write(&file, DESKTOP);
    assert!(!claude_desktop::rewrites_while_running(&file));
}

#[test]
fn connecting_claude_desktop_while_it_runs_writes_nothing_and_says_how_to_finish() {
    let mut fx = fixture();
    desktop_running(&mut fx);
    let file = ClaudeDesktop::config_file(&fx.env);
    write(&file, DESKTOP_WITH_BR8N);

    let err = ClaudeDesktop.connect(&fx.env, &none()).unwrap_err();
    assert!(
        matches!(
            err.downcast_ref::<AgentError>(),
            Some(AgentError::Unavailable(_))
        ),
        "{err:#}"
    );
    let message = format!("{err:#}");
    assert!(message.contains("Claude Desktop is running"), "{message}");
    assert!(
        message.contains("rhebok connect claude-desktop"),
        "{message}"
    );
    assert_eq!(read(&file), DESKTOP_WITH_BR8N);
    assert!(!backup_path(&file).exists());
}

#[test]
fn an_already_connected_desktop_needs_no_write_so_a_running_app_is_no_obstacle() {
    let mut fx = fixture();
    let file = ClaudeDesktop::config_file(&fx.env);
    write(&file, DESKTOP);
    ClaudeDesktop.connect(&fx.env, &none()).unwrap();
    let before = read(&file);

    desktop_running(&mut fx);
    let again = ClaudeDesktop.connect(&fx.env, &none()).unwrap();
    assert!(again.is_empty(), "{again:?}");
    assert_eq!(read(&file), before);
}

#[test]
fn disconnecting_claude_desktop_while_it_runs_is_refused_only_when_rhebok_is_in_the_file() {
    let mut fx = fixture();
    let file = ClaudeDesktop::config_file(&fx.env);
    write(&file, DESKTOP);
    ClaudeDesktop.connect(&fx.env, &none()).unwrap();
    let connected = read(&file);

    desktop_running(&mut fx);
    let err = ClaudeDesktop.disconnect(&fx.env).unwrap_err();
    assert!(
        format!("{err:#}").contains("rhebok disconnect claude-desktop"),
        "{err:#}"
    );
    assert_eq!(read(&file), connected);

    write(&file, DESKTOP_WITH_BR8N);
    let nothing = ClaudeDesktop.disconnect(&fx.env).unwrap();
    assert!(nothing.is_empty(), "{nothing:?}");
    assert_eq!(read(&file), DESKTOP_WITH_BR8N);
}

#[test]
fn a_leftover_br8n_entry_reads_as_stale_and_connecting_replaces_it() {
    let fx = fixture();
    let file = ClaudeDesktop::config_file(&fx.env);
    write(&file, DESKTOP_WITH_BR8N);
    match ClaudeDesktop.status(&fx.env) {
        Status::Stale(reason) => assert!(reason.contains("br8n"), "{reason}"),
        other => panic!("expected stale, got {other:?}"),
    }

    let change = ClaudeDesktop.connect(&fx.env, &none()).unwrap();
    assert_eq!(change.files, vec![file.clone()]);
    let v = json(&file);
    assert!(v["mcpServers"].get("br8n").is_none(), "{v}");
    assert_eq!(v["mcpServers"]["rhebok"]["command"], bin(&fx));
    assert_eq!(v["mcpServers"]["other"]["command"], "/usr/bin/other");
    assert_eq!(v["preferences"]["sidebarMode"], "epitaxy");
    assert_eq!(ClaudeDesktop.status(&fx.env), Status::Connected);
}

#[test]
fn a_br8n_entry_beside_a_current_rhebok_one_still_reads_as_stale() {
    let fx = fixture();
    let file = ClaudeDesktop::config_file(&fx.env);
    write(&file, DESKTOP);
    ClaudeDesktop.connect(&fx.env, &none()).unwrap();
    let mut v = json(&file);
    v["mcpServers"]["br8n"] = serde_json::json!({ "command": "/old/br8n", "args": ["mcp"] });
    write(&file, &serde_json::to_string_pretty(&v).unwrap());

    assert!(matches!(ClaudeDesktop.status(&fx.env), Status::Stale(_)));
    ClaudeDesktop.connect(&fx.env, &none()).unwrap();
    assert!(json(&file)["mcpServers"].get("br8n").is_none());
    assert_eq!(ClaudeDesktop.status(&fx.env), Status::Connected);
}

#[test]
fn a_quiet_install_repairs_a_leftover_br8n_entry_once_claude_desktop_is_closed() {
    let mut fx = fixture();
    let file = ClaudeDesktop::config_file(&fx.env);
    write(&file, DESKTOP_WITH_BR8N);

    desktop_running(&mut fx);
    let open = agents::offer_after_install(&fx.env, false, true, |_, _| panic!("never asks"));
    assert!(
        open.warnings
            .iter()
            .any(|w| w.contains("Claude Desktop is running")),
        "{open:?}"
    );
    assert_eq!(read(&file), DESKTOP_WITH_BR8N);

    fx.env.desktop_rewrites = |_| false;
    agents::offer_after_install(&fx.env, false, true, |_, _| panic!("never asks"));
    assert_eq!(ClaudeDesktop.status(&fx.env), Status::Connected);
}

#[test]
fn the_agents_api_answers_409_while_claude_desktop_runs() {
    let mut fx = fixture();
    desktop_running(&mut fx);
    write(&ClaudeDesktop::config_file(&fx.env), DESKTOP);
    let (kind, message) = refused(core_agents::connect(&fx.env, request("claude-desktop")));
    assert_eq!(kind, AgentRefusal::Unavailable, "{message}");
    assert!(message.contains("Claude Desktop is running"), "{message}");
}

#[test]
fn the_account_config_is_matched_through_a_symlinked_home_and_nothing_else_is() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let config = home.join("Library/Application Support/Claude/claude_desktop_config.json");
    write(&config, "{}");
    assert!(claude_desktop::is_account_config(&config, &home));

    let linked = t.path().join("linked-home");
    std::os::unix::fs::symlink(&home, &linked).unwrap();
    assert!(claude_desktop::is_account_config(
        &linked.join("Library/Application Support/Claude/claude_desktop_config.json"),
        &home
    ));

    assert!(!claude_desktop::is_account_config(
        &config.with_file_name("other.json"),
        &home
    ));
    let elsewhere = t.path().join("sandbox");
    write(
        &elsewhere.join("Library/Application Support/Claude/claude_desktop_config.json"),
        "{}",
    );
    assert!(!claude_desktop::is_account_config(&config, &elsewhere));
}

#[test]
fn a_broken_connection_with_a_br8n_entry_is_still_not_written_while_claude_desktop_runs() {
    let mut fx = fixture();
    let file = ClaudeDesktop::config_file(&fx.env);
    write(&file, DESKTOP);
    ClaudeDesktop.connect(&fx.env, &none()).unwrap();
    let mut v = json(&file);
    v["mcpServers"]["br8n"] = serde_json::json!({ "command": "/old/br8n", "args": ["mcp"] });
    write(&file, &serde_json::to_string_pretty(&v).unwrap());
    std::fs::remove_file(fx.env.bin()).unwrap();
    assert!(matches!(ClaudeDesktop.status(&fx.env), Status::Broken(_)));
    let before = read(&file);

    desktop_running(&mut fx);
    assert!(ClaudeDesktop.connect(&fx.env, &none()).is_err());
    assert_eq!(read(&file), before);
}

#[test]
fn an_interactive_install_does_not_offer_claude_desktop_while_it_runs() {
    let mut fx = fixture();
    std::fs::create_dir_all(ClaudeDesktop::config_dir(&fx.env)).unwrap();
    assert!(ClaudeDesktop.detect(&fx.env).installed);
    desktop_running(&mut fx);
    let r = agents::offer_after_install(&fx.env, false, false, |q, _| {
        assert!(!q.contains("Claude Desktop"), "asked: {q}");
        false
    });
    assert!(
        r.lines
            .iter()
            .any(|l| l.starts_with("claude-desktop:") && l.contains("it is running")),
        "{r:?}"
    );
    assert!(!ClaudeDesktop::config_file(&fx.env).exists());
}

#[test]
fn a_leftover_br8n_entry_is_reported_once_while_claude_desktop_runs() {
    let mut fx = fixture();
    write(&ClaudeDesktop::config_file(&fx.env), DESKTOP_WITH_BR8N);
    desktop_running(&mut fx);
    let r = agents::offer_after_install(&fx.env, false, true, |_, _| panic!("never asks"));
    let about: Vec<_> = r
        .warnings
        .iter()
        .chain(&r.lines)
        .filter(|l| l.contains("claude-desktop"))
        .collect();
    assert_eq!(about.len(), 1, "{r:?}");
    assert!(
        about[0].contains("rhebok connect claude-desktop"),
        "{about:?}"
    );
}

#[test]
fn uninstall_is_refused_while_claude_desktop_runs_with_rhebok_in_its_config() {
    let mut fx = fixture();
    assert!(agents::refuse_uninstall_while_blocked(&fx.env).is_ok());
    let file = ClaudeDesktop::config_file(&fx.env);
    write(&file, DESKTOP);
    ClaudeDesktop.connect(&fx.env, &none()).unwrap();
    assert!(agents::refuse_uninstall_while_blocked(&fx.env).is_ok());

    desktop_running(&mut fx);
    let message = format!(
        "{:#}",
        agents::refuse_uninstall_while_blocked(&fx.env).unwrap_err()
    );
    assert!(message.contains("rhebok uninstall"), "{message}");
    assert!(message.contains("Nothing was removed"), "{message}");

    write(&file, DESKTOP_WITH_BR8N);
    assert!(agents::refuse_uninstall_while_blocked(&fx.env).is_ok());
}

#[test]
fn replacing_a_br8n_entry_adopts_its_backup_and_names_the_chat_instructions() {
    let fx = fixture();
    let file = ClaudeDesktop::config_file(&fx.env);
    write(&file, DESKTOP_WITH_BR8N);
    let mut legacy_backup = file.clone().into_os_string();
    legacy_backup.push(".br8n-bak");
    let legacy_backup = PathBuf::from(legacy_backup);
    write(&legacy_backup, DESKTOP);

    let change = ClaudeDesktop.connect(&fx.env, &none()).unwrap();
    assert_eq!(read(&backup_path(&file)), DESKTOP);
    assert!(!legacy_backup.exists());
    assert!(
        change.notes.iter().any(|n| n.contains("br8n_context")),
        "{change:?}"
    );
}

#[test]
fn a_br8n_entry_beside_a_working_rhebok_one_still_counts_as_connected_for_chat() {
    let fx = fixture();
    let file = ClaudeDesktop::config_file(&fx.env);
    write(&file, DESKTOP);
    ClaudeDesktop.connect(&fx.env, &none()).unwrap();
    let mut v = json(&file);
    v["mcpServers"]["br8n"] = serde_json::json!({ "command": "/old/br8n", "args": ["mcp"] });
    write(&file, &serde_json::to_string_pretty(&v).unwrap());

    assert!(matches!(ClaudeDesktop.status(&fx.env), Status::Stale(_)));
    assert_eq!(ClaudeDesktop::rhebok_status(&fx.env), Status::Connected);
}

#[test]
fn only_an_unparseable_file_is_flagged_as_one_to_fix_by_hand() {
    let mut fx = fixture();
    let file = ClaudeDesktop::config_file(&fx.env);
    write(&file, "{ // not json\n}\n");
    let (kind, message) = refused(core_agents::connect(&fx.env, request("claude-desktop")));
    assert_eq!(kind, AgentRefusal::Unparseable, "{message}");

    write(&file, DESKTOP);
    desktop_running(&mut fx);
    let (kind, message) = refused(core_agents::connect(&fx.env, request("claude-desktop")));
    assert_eq!(kind, AgentRefusal::Unavailable, "{message}");
}

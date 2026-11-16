use rhebok::setup::agents::{codex, cursor::Cursor, AgentEnv};
use rhebok::setup::claude::ClaudeCli;
use rhebok::setup::migrate::{migrate, renamed_env_text, Crontab};
#[cfg(feature = "backup")]
use rhebok::setup::migrate::{migrated_cron_line, quoted_cron_line};
use rhebok::setup::Paths;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

struct Fx {
    t: tempfile::TempDir,
    home: PathBuf,
    data: PathBuf,
    linkdir: PathBuf,
    paths: Paths,
    config: PathBuf,
}

fn fixture() -> Fx {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let data = t.path().join("data");
    let linkdir = t.path().join("linkdir");
    for d in [&home, &data, &linkdir] {
        std::fs::create_dir_all(d).unwrap();
    }
    let paths = Paths::at(
        &data.join("rhebok"),
        vec![linkdir.clone()],
        home.join(".claude/plugins/cache/rhebok"),
    );
    let config = data.join("rhebok/config.toml");
    Fx {
        t,
        home,
        data,
        linkdir,
        paths,
        config,
    }
}

fn write(p: &Path, text: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

fn executable(p: &Path, body: &str) {
    write(p, body);
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn no_claude() -> ClaudeCli {
    ClaudeCli::at(Path::new("/nonexistent/claude"))
}

fn no_crontab() -> Crontab {
    Crontab::at(Path::new("/nonexistent/crontab"))
}

fn env(fx: &Fx) -> AgentEnv {
    AgentEnv::at(&fx.home, fx.paths.clone(), vec![])
}

fn legacy_install(fx: &Fx) -> PathBuf {
    let old = fx.data.join("br8n");
    write(&old.join("config.toml"), "sources = [\"~/notes\"]\n");
    write(&old.join("db/graph.kz"), "index bytes");
    write(&old.join("memory/pack/pack.rec"), "memory bytes");
    executable(&old.join("bin/br8n"), "#!/bin/sh\necho br8n\n");
    old
}

#[test]
fn a_br8n_install_moves_to_rhebok_with_its_index_config_and_memories() {
    let fx = fixture();
    let old = legacy_install(&fx);

    let r = migrate(
        &fx.paths,
        &fx.config,
        &no_claude(),
        &env(&fx),
        &no_crontab(),
    )
    .unwrap();

    assert!(r.migrated());
    assert!(!old.is_dir(), "the br8n directory is moved, not copied");
    assert_eq!(read(&fx.config), "sources = [\"~/notes\"]\n");
    assert_eq!(read(&fx.paths.root.join("db/graph.kz")), "index bytes");
    assert_eq!(
        read(&fx.paths.root.join("memory/pack/pack.rec")),
        "memory bytes"
    );
    assert!(
        !fx.paths.root.join("bin/br8n").exists(),
        "the old binary does not survive under the new name's directory"
    );
}

#[test]
fn a_second_migration_finds_nothing_to_do() {
    let fx = fixture();
    legacy_install(&fx);
    migrate(
        &fx.paths,
        &fx.config,
        &no_claude(),
        &env(&fx),
        &no_crontab(),
    )
    .unwrap();

    let again = migrate(
        &fx.paths,
        &fx.config,
        &no_claude(),
        &env(&fx),
        &no_crontab(),
    )
    .unwrap();

    assert!(!again.migrated());
    assert!(again.lines.is_empty(), "{:?}", again.lines);
    assert!(again.warnings.is_empty(), "{:?}", again.warnings);
}

#[test]
fn an_existing_rhebok_directory_is_never_overwritten_by_the_br8n_one() {
    let fx = fixture();
    let old = legacy_install(&fx);
    write(&fx.config, "sources = []\n");

    let r = migrate(
        &fx.paths,
        &fx.config,
        &no_claude(),
        &env(&fx),
        &no_crontab(),
    )
    .unwrap();

    assert!(!r.migrated());
    assert!(old.join("db/graph.kz").exists());
    assert_eq!(read(&fx.config), "sources = []\n");
    assert!(
        r.warnings.iter().any(|w| w.contains("both")),
        "{:?}",
        r.warnings
    );
}

#[test]
fn a_running_br8n_index_stops_the_migration_before_anything_moves() {
    let fx = fixture();
    let old = legacy_install(&fx);
    write(&old.join("db.lock"), &std::process::id().to_string());

    let err = migrate(
        &fx.paths,
        &fx.config,
        &no_claude(),
        &env(&fx),
        &no_crontab(),
    )
    .unwrap_err();

    assert!(format!("{err:#}").contains("running"), "{err:#}");
    assert!(old.join("db/graph.kz").exists());
    assert!(!fx.paths.root.exists());
}

#[test]
fn the_env_file_keys_are_renamed_and_everything_else_is_kept() {
    let text = "# remote embedder\nBR8N_EMBED_URL=http://h:1234\nexport BR8N_EMBED_TOKEN=secret\nOTHER=1\n  BR8N_EMBED_MODEL=qwen\n";
    let renamed = renamed_env_text(text).unwrap();
    assert_eq!(
        renamed,
        "# remote embedder\nRHEBOK_EMBED_URL=http://h:1234\nexport RHEBOK_EMBED_TOKEN=secret\nOTHER=1\n  RHEBOK_EMBED_MODEL=qwen\n"
    );
    assert_eq!(renamed_env_text("RHEBOK_EMBED_URL=x\n"), None);
}

#[test]
fn the_migrated_env_file_keeps_its_private_mode() {
    let fx = fixture();
    let old = legacy_install(&fx);
    write(&old.join("env"), "BR8N_EMBED_TOKEN=secret\n");
    std::fs::set_permissions(old.join("env"), std::fs::Permissions::from_mode(0o600)).unwrap();

    migrate(
        &fx.paths,
        &fx.config,
        &no_claude(),
        &env(&fx),
        &no_crontab(),
    )
    .unwrap();

    let env_file = fx.paths.root.join("env");
    assert_eq!(read(&env_file), "RHEBOK_EMBED_TOKEN=secret\n");
    let mode = std::fs::metadata(&env_file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn only_a_path_link_to_the_old_binary_is_removed() {
    let fx = fixture();
    let old = legacy_install(&fx);
    let ours = fx.linkdir.join("br8n");
    std::os::unix::fs::symlink(old.join("bin/br8n"), &ours).unwrap();
    let elsewhere = fx.t.path().join("other-linkdir");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let foreign = elsewhere.join("br8n");
    std::os::unix::fs::symlink("/opt/somebody-else/br8n", &foreign).unwrap();
    let mut paths = fx.paths.clone();
    paths.link_dirs.push(elsewhere);

    migrate(&paths, &fx.config, &no_claude(), &env(&fx), &no_crontab()).unwrap();

    assert!(ours.symlink_metadata().is_err());
    assert!(foreign.symlink_metadata().is_ok());
}

#[test]
fn a_cursor_connection_moves_from_the_br8n_server_to_the_rhebok_one() {
    let fx = fixture();
    legacy_install(&fx);
    let file = Cursor::config_file(&env(&fx));
    write(
        &file,
        r#"{
  "mcpServers": {
    "br8n": { "command": "/old/br8n", "args": ["mcp"] },
    "keep": { "command": "/usr/bin/keep" }
  }
}
"#,
    );

    let r = migrate(
        &fx.paths,
        &fx.config,
        &no_claude(),
        &env(&fx),
        &no_crontab(),
    )
    .unwrap();

    let doc: serde_json::Value = serde_json::from_str(&read(&file)).unwrap();
    let servers = doc["mcpServers"].as_object().unwrap();
    assert!(!servers.contains_key("br8n"));
    assert_eq!(servers["keep"]["command"], "/usr/bin/keep");
    assert_eq!(
        servers["rhebok"]["command"],
        fx.paths.bin.display().to_string()
    );
    assert!(
        r.lines.iter().any(|l| l.contains("cursor")),
        "{:?}",
        r.lines
    );
}

#[test]
fn a_codex_connection_and_its_guidance_block_move_to_rhebok() {
    let fx = fixture();
    legacy_install(&fx);
    let e = env(&fx);
    let config = codex::Codex::config_file(&e);
    write(
        &config,
        "model = \"o4\"\n\n[mcp_servers.br8n]\ncommand = \"/old/br8n\"\nargs = [\"mcp\"]\n",
    );
    let agents_md = codex::Codex::instructions_file(&e);
    write(
        &agents_md,
        "# Mine\n\n<!-- br8n:start -->\nold guidance\n<!-- br8n:end -->\n",
    );

    migrate(&fx.paths, &fx.config, &no_claude(), &e, &no_crontab()).unwrap();

    let toml = read(&config);
    assert!(!toml.contains("br8n"), "{toml}");
    assert!(toml.contains("[mcp_servers.rhebok]"), "{toml}");
    assert!(toml.contains("model = \"o4\""), "{toml}");
    let md = read(&agents_md);
    assert!(!md.contains("br8n"), "{md}");
    assert!(md.starts_with("# Mine\n"), "{md}");
    assert!(md.contains(codex::BLOCK_START), "{md}");
}

#[test]
fn the_old_plugin_and_marketplace_are_removed_from_claude_code() {
    let fx = fixture();
    let old = legacy_install(&fx);
    let stub = fx.t.path().join("stub");
    std::fs::create_dir_all(&stub).unwrap();
    write(
        &stub.join("marketplaces.json"),
        &format!(
            r#"[{{"name":"br8n","source":"directory","path":"{p}","installLocation":"{p}"}}]"#,
            p = old.join("plugin").display()
        ),
    );
    write(
        &stub.join("plugins.json"),
        r#"[{"id":"br8n@br8n","version":"0.1.0","scope":"user","enabled":true}]"#,
    );
    executable(
        &stub.join("claude"),
        &format!(
            "#!/bin/sh\necho \"$*\" >> \"{d}/calls\"\ncase \"$*\" in\n\
             '--version') exit 0 ;;\n\
             'plugin marketplace list --json') cat \"{d}/marketplaces.json\" ;;\n\
             'plugin list --json') cat \"{d}/plugins.json\" ;;\n\
             *) exit 0 ;;\nesac\n",
            d = stub.display()
        ),
    );
    write(
        &fx.home.join(".claude/plugins/cache/br8n/br8n/0.1.0/x"),
        "cached",
    );

    migrate(
        &fx.paths,
        &fx.config,
        &ClaudeCli::at(&stub.join("claude")),
        &env(&fx),
        &no_crontab(),
    )
    .unwrap();

    let calls = read(&stub.join("calls"));
    assert!(calls.contains("plugin uninstall br8n@br8n"), "{calls}");
    assert!(calls.contains("plugin marketplace remove br8n"), "{calls}");
    assert!(!fx.home.join(".claude/plugins/cache/br8n").exists());
}

#[test]
fn a_br8n_marketplace_this_install_did_not_create_is_left_alone() {
    let fx = fixture();
    legacy_install(&fx);
    let stub = fx.t.path().join("stub");
    std::fs::create_dir_all(&stub).unwrap();
    write(
        &stub.join("marketplaces.json"),
        r#"[{"name":"br8n","source":"directory","path":"/somebody/else","installLocation":"/somebody/else"}]"#,
    );
    write(&stub.join("plugins.json"), "[]");
    executable(
        &stub.join("claude"),
        &format!(
            "#!/bin/sh\necho \"$*\" >> \"{d}/calls\"\ncase \"$*\" in\n\
             '--version') exit 0 ;;\n\
             'plugin marketplace list --json') cat \"{d}/marketplaces.json\" ;;\n\
             'plugin list --json') cat \"{d}/plugins.json\" ;;\n\
             *) exit 0 ;;\nesac\n",
            d = stub.display()
        ),
    );

    let r = migrate(
        &fx.paths,
        &fx.config,
        &ClaudeCli::at(&stub.join("claude")),
        &env(&fx),
        &no_crontab(),
    )
    .unwrap();

    assert!(!read(&stub.join("calls")).contains("marketplace remove"));
    assert!(
        r.warnings.iter().any(|w| w.contains("left alone")),
        "{:?}",
        r.warnings
    );
}

#[cfg(feature = "backup")]
#[test]
fn the_br8n_backup_schedule_is_rewritten_for_the_rhebok_binary() {
    let fx = fixture();
    let old = legacy_install(&fx);
    let stub = fx.t.path().join("cron");
    std::fs::create_dir_all(&stub).unwrap();
    write(
        &stub.join("current"),
        &format!(
            "MAILTO=\"\"\n15 3 * * * BR8N_CONFIG={} {} backup >/dev/null 2>&1 # br8n-backup\n",
            old.join("config.toml").display(),
            old.join("bin/br8n").display()
        ),
    );
    executable(
        &stub.join("crontab"),
        &format!(
            "#!/bin/sh\ncase \"$1\" in\n  -l) cat \"{d}/current\" ;;\n  -) cat > \"{d}/written\" ;;\nesac\n",
            d = stub.display()
        ),
    );

    let r = migrate(
        &fx.paths,
        &fx.config,
        &no_claude(),
        &env(&fx),
        &Crontab::at(&stub.join("crontab")),
    )
    .unwrap();

    let written = read(&stub.join("written"));
    assert!(written.starts_with("MAILTO=\"\"\n"), "{written}");
    assert!(!written.contains("br8n"), "{written}");
    assert!(
        written.contains(&format!(
            "15 3 * * * RHEBOK_CONFIG='{}' '{}' backup",
            fx.config.display(),
            fx.paths.bin.display()
        )),
        "{written}"
    );
    assert!(written.trim_end().ends_with("# rhebok-backup"), "{written}");
    assert!(
        r.lines.iter().any(|l| l.contains("schedule")),
        "{:?}",
        r.lines
    );
}

#[cfg(feature = "backup")]
#[test]
fn a_cron_line_without_a_config_override_keeps_its_schedule() {
    let fx = fixture();
    let line = migrated_cron_line(
        "0 */6 * * * /x/br8n/bin/br8n backup >/dev/null 2>&1 # br8n-backup",
        &fx.paths,
        &[],
    )
    .unwrap();
    assert_eq!(
        line,
        format!(
            "0 */6 * * * '{}' backup >/dev/null 2>&1 # rhebok-backup",
            fx.paths.bin.display()
        )
    );
}

#[cfg(feature = "backup")]
fn stub_crontab(fx: &Fx, current: &str) -> (Crontab, PathBuf) {
    let stub = fx.t.path().join("cron");
    std::fs::create_dir_all(&stub).unwrap();
    write(&stub.join("current"), current);
    executable(
        &stub.join("crontab"),
        &format!(
            "#!/bin/sh\ncase \"$1\" in\n  -l) cat \"{d}/current\" ;;\n  -) cat > \"{d}/written\" ;;\nesac\n",
            d = stub.display()
        ),
    );
    (Crontab::at(&stub.join("crontab")), stub.join("written"))
}

#[cfg(feature = "backup")]
const SPACED_BIN: &str = "/Users/x/Library/Application Support/rhebok/bin/rhebok";
#[cfg(feature = "backup")]
const SPACED_CONFIG: &str = "/Users/x/Library/Application Support/rhebok/config.toml";

#[cfg(feature = "backup")]
#[test]
fn an_unquoted_rhebok_backup_line_is_quoted_on_install_and_the_rest_is_kept() {
    let fx = fixture();
    let (crontab, written) = stub_crontab(
        &fx,
        &format!(
            "MAILTO=\"\"\n0 3 * * * {SPACED_BIN} backup >/dev/null 2>&1 # rhebok-backup\n@hourly /usr/bin/true\n"
        ),
    );

    let r = migrate(&fx.paths, &fx.config, &no_claude(), &env(&fx), &crontab).unwrap();

    assert_eq!(
        read(&written),
        format!(
            "MAILTO=\"\"\n0 3 * * * '{SPACED_BIN}' backup >/dev/null 2>&1 # rhebok-backup\n@hourly /usr/bin/true\n"
        )
    );
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert!(
        r.lines.iter().any(|l| l.contains("schedule quoted")),
        "{:?}",
        r.lines
    );
}

#[cfg(feature = "backup")]
#[test]
fn an_unquoted_line_with_a_config_override_splits_at_the_program() {
    assert_eq!(
        quoted_cron_line(&format!(
            "@daily RHEBOK_CONFIG={SPACED_CONFIG} {SPACED_BIN} backup >/dev/null 2>&1 # rhebok-backup"
        ))
        .unwrap(),
        Some(format!(
            "@daily RHEBOK_CONFIG='{SPACED_CONFIG}' '{SPACED_BIN}' backup >/dev/null 2>&1 # rhebok-backup"
        ))
    );
}

#[cfg(feature = "backup")]
#[test]
fn a_quoted_rhebok_backup_line_is_left_byte_identical() {
    let fx = fixture();
    let quoted = rhebok::backup::cron_line(
        "0 3 * * *",
        Path::new(SPACED_BIN),
        Some(Path::new(SPACED_CONFIG)),
    )
    .unwrap();
    assert_eq!(quoted_cron_line(&quoted).unwrap(), None);
    let (crontab, written) = stub_crontab(&fx, &format!("{quoted}\n"));

    let r = migrate(&fx.paths, &fx.config, &no_claude(), &env(&fx), &crontab).unwrap();

    assert!(!written.exists(), "{}", read(&written));
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert!(r.lines.is_empty(), "{:?}", r.lines);
}

#[cfg(feature = "backup")]
#[test]
fn a_commented_out_rhebok_backup_line_is_left_byte_identical_with_no_warning() {
    let fx = fixture();
    let (crontab, written) = stub_crontab(
        &fx,
        &format!(
            "#0 3 * * * '{SPACED_BIN}' backup >/dev/null 2>&1 # rhebok-backup\n  # 0 3 * * * {SPACED_BIN} backup >/dev/null 2>&1 # rhebok-backup\n"
        ),
    );

    let r = migrate(&fx.paths, &fx.config, &no_claude(), &env(&fx), &crontab).unwrap();

    assert!(!written.exists(), "{}", read(&written));
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert!(r.lines.is_empty(), "{:?}", r.lines);
}

#[cfg(feature = "backup")]
#[test]
fn a_rhebok_backup_line_that_cannot_be_read_is_warned_about_and_left_alone() {
    let fx = fixture();
    let (crontab, written) = stub_crontab(
        &fx,
        "0 3 * * * /opt/tools/backup-all >/dev/null # rhebok-backup\n",
    );

    let r = migrate(&fx.paths, &fx.config, &no_claude(), &env(&fx), &crontab).unwrap();

    assert!(!written.exists(), "{}", read(&written));
    assert!(
        r.warnings
            .iter()
            .any(|w| w.contains("could not read the backup schedule")
                && w.contains("rhebok backup schedule")),
        "{:?}",
        r.warnings
    );
}

#[test]
fn nothing_is_touched_when_there_was_never_a_br8n_install() {
    let fx = fixture();
    let r = migrate(
        &fx.paths,
        &fx.config,
        &no_claude(),
        &env(&fx),
        &no_crontab(),
    )
    .unwrap();
    assert!(!r.migrated());
    assert!(r.lines.is_empty(), "{:?}", r.lines);
    assert!(!fx.paths.root.exists());
}

#[test]
fn a_repo_that_still_has_a_br8n_project_file_is_found() {
    let t = tempfile::tempdir().unwrap();
    let repo = t.path().join("repo");
    write(&repo.join(".br8n/project.toml"), "name = \"old\"\n");
    let nested = repo.join("src/deep");
    std::fs::create_dir_all(&nested).unwrap();

    assert_eq!(
        rhebok::project::find_repo_file(&nested),
        Some(repo.join(".br8n/project.toml"))
    );

    write(&repo.join(".rhebok/project.toml"), "name = \"new\"\n");
    assert_eq!(
        rhebok::project::find_repo_file(&nested),
        Some(repo.join(".rhebok/project.toml"))
    );
}

#[test]
fn the_audit_counts_injections_made_under_the_br8n_tag() {
    let t = tempfile::tempdir().unwrap();
    let block = |tag: &str| {
        serde_json::json!({
            "type": "attachment",
            "attachment": {
                "type": "hook_additional_context",
                "content": [format!("<{tag}>\n[A note](file:///n.md)\n</{tag}>")]
            }
        })
        .to_string()
    };
    write(
        &t.path().join("p/s.jsonl"),
        &format!("{}\n{}\n", block("br8n-context"), block("rhebok-context")),
    );

    let a = rhebok::audit::audit(t.path()).unwrap();

    assert_eq!(a.injections, 2);
}

#[cfg(feature = "backup")]
#[test]
fn a_backup_manifest_written_by_br8n_still_reads() {
    let json =
        r#"{"version":1,"created_at":"2026-11-01T00:00:00Z","host":"h","br8n_version":"0.1.0"}"#;
    let m: rhebok::backup::manifest::Manifest = serde_json::from_str(json).unwrap();
    assert_eq!(m.rhebok_version, "0.1.0");
}

#[test]
fn the_old_directory_is_replaced_by_a_marker_file_a_running_br8n_cannot_write_into() {
    let fx = fixture();
    let old = legacy_install(&fx);

    migrate(
        &fx.paths,
        &fx.config,
        &no_claude(),
        &env(&fx),
        &no_crontab(),
    )
    .unwrap();

    assert!(
        old.is_file(),
        "a regular file stands where the directory was"
    );
    assert!(read(&old).contains("rhebok"));
    assert!(std::fs::create_dir_all(old.join("memory")).is_err());
}

#[test]
fn the_env_file_is_rewritten_without_leaving_a_copy_of_the_token() {
    let fx = fixture();
    let old = legacy_install(&fx);
    write(&old.join("env"), "BR8N_EMBED_TOKEN=secret\n");
    std::fs::set_permissions(old.join("env"), std::fs::Permissions::from_mode(0o600)).unwrap();

    migrate(
        &fx.paths,
        &fx.config,
        &no_claude(),
        &env(&fx),
        &no_crontab(),
    )
    .unwrap();

    let leftovers: Vec<String> = std::fs::read_dir(&fx.paths.root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("env") && n != "env")
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn config_paths_into_the_moved_directory_follow_it_and_keep_their_comments() {
    let home = Path::new("/home/u");
    let moves = vec![(
        PathBuf::from("/home/u/.config/br8n"),
        PathBuf::from("/home/u/.config/rhebok"),
    )];
    let text = "sources = [\"~/notes\", \"/home/u/.config/br8n/extra\"]\n\n[backup]\n# the key\nkey_file = \"/home/u/.config/br8n/backup.key\"\n\n[backup.drive]\nclient_secret_file = \"~/.config/br8n/drive-client.json\" # from Google\n";

    let (updated, changes) =
        rhebok::setup::migrate::moved_config_text(text, &moves, Some(home)).unwrap();

    assert_eq!(
        updated,
        "sources = [\"~/notes\", \"/home/u/.config/rhebok/extra\"]\n\n[backup]\n# the key\nkey_file = \"/home/u/.config/rhebok/backup.key\"\n\n[backup.drive]\nclient_secret_file = \"~/.config/rhebok/drive-client.json\" # from Google\n"
    );
    assert_eq!(changes.len(), 3, "{changes:?}");
}

#[test]
fn a_migrated_s3_backup_keeps_reading_the_br8n_prefix() {
    let (updated, _) =
        rhebok::setup::migrate::moved_config_text("[backup.s3]\nbucket = \"b\"\n", &[], None)
            .unwrap();
    assert_eq!(updated, "[backup.s3]\nbucket = \"b\"\nprefix = \"br8n/\"\n");

    assert!(rhebok::setup::migrate::moved_config_text(
        "[backup.s3]\nbucket = \"b\"\nprefix = \"mine/\"\n",
        &[],
        None,
    )
    .is_none());
}

#[test]
fn a_migration_pins_the_s3_prefix_in_the_moved_config() {
    let fx = fixture();
    let old = legacy_install(&fx);
    write(&old.join("config.toml"), "[backup.s3]\nbucket = \"b\"\n");

    migrate(
        &fx.paths,
        &fx.config,
        &no_claude(),
        &env(&fx),
        &no_crontab(),
    )
    .unwrap();

    assert_eq!(
        read(&fx.config),
        "[backup.s3]\nbucket = \"b\"\nprefix = \"br8n/\"\n"
    );
}

#[test]
fn an_existing_br8n_backup_of_an_agent_config_becomes_the_rhebok_one() {
    let fx = fixture();
    legacy_install(&fx);
    let file = Cursor::config_file(&env(&fx));
    write(
        &file,
        r#"{"mcpServers":{"br8n":{"command":"/old/br8n","args":["mcp"]}}}"#,
    );
    let mut original = file.file_name().unwrap().to_os_string();
    original.push(".br8n-bak");
    write(
        &file.with_file_name(&original),
        "{\"before\": \"rhebok\"}\n",
    );

    migrate(
        &fx.paths,
        &fx.config,
        &no_claude(),
        &env(&fx),
        &no_crontab(),
    )
    .unwrap();

    let backup = rhebok::setup::agents::files::backup_path(&file);
    assert_eq!(read(&backup), "{\"before\": \"rhebok\"}\n");
    assert!(!file.with_file_name(original).exists());
}

#[test]
fn a_gemini_hook_left_named_br8n_is_renamed() {
    let fx = fixture();
    legacy_install(&fx);
    let e = env(&fx);
    let file = rhebok::setup::agents::gemini::Gemini::config_file(&e);
    write(
        &file,
        r#"{"mcpServers":{"br8n":{"command":"/old/br8n","args":["mcp"]}},"hooks":{"BeforeAgent":[{"hooks":[{"name":"br8n","type":"command","command":"\"/old/br8n\" hook prompt --agent gemini","timeout":5000}]}]}}"#,
    );

    migrate(&fx.paths, &fx.config, &no_claude(), &e, &no_crontab()).unwrap();

    let doc: serde_json::Value = serde_json::from_str(&read(&file)).unwrap();
    let hook = &doc["hooks"]["BeforeAgent"][0]["hooks"][0];
    assert_eq!(hook["name"], "rhebok");
    assert!(hook["command"]
        .as_str()
        .unwrap()
        .contains(&fx.paths.bin.display().to_string()));
    assert_eq!(
        doc["hooks"]["BeforeAgent"][0]["hooks"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_br8n_embed_endpoint_left_in_the_env_file_is_refused_not_ignored() {
    let t = tempfile::tempdir().unwrap();
    let config = t.path().join("config.toml");
    write(&t.path().join("env"), "BR8N_EMBED_URL=http://h:1234\n");
    std::fs::set_permissions(t.path().join("env"), std::fs::Permissions::from_mode(0o600)).unwrap();

    let err = rhebok::env_file::resolve(&config).unwrap_err();

    assert!(format!("{err:#}").contains("RHEBOK_EMBED_URL"), "{err:#}");
}

#[test]
fn leftover_br8n_variables_are_named_with_their_replacement() {
    let warnings = rhebok::setup::migrate::legacy_env_warnings(
        ["PATH", "BR8N_DB", "RHEBOK_CONFIG"]
            .into_iter()
            .map(String::from),
    );
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("BR8N_DB") && warnings[0].contains("RHEBOK_DB"));
}

#[cfg(feature = "backup")]
#[test]
fn an_at_macro_schedule_survives_and_a_foreign_line_is_not_rewritten() {
    let fx = fixture();
    assert_eq!(
        migrated_cron_line(
            "@daily /x/br8n/bin/br8n backup >/dev/null 2>&1 # br8n-backup",
            &fx.paths,
            &[]
        ),
        Some(format!(
            "@daily '{}' backup >/dev/null 2>&1 # rhebok-backup",
            fx.paths.bin.display()
        ))
    );
    assert_eq!(
        migrated_cron_line(
            "0 3 * * * /usr/bin/other thing # br8n-backup",
            &fx.paths,
            &[]
        ),
        None
    );
}

#[cfg(feature = "backup")]
#[test]
fn the_schedule_is_rewritten_when_the_binary_it_runs_is_removed_even_without_a_move() {
    let fx = fixture();
    executable(&fx.paths.root.join("bin/br8n"), "#!/bin/sh\n");
    let stub = fx.t.path().join("cron");
    write(
        &stub.join("current"),
        &format!(
            "0 3 * * * {} backup >/dev/null 2>&1 # br8n-backup\n",
            fx.paths.root.join("bin/br8n").display()
        ),
    );
    executable(
        &stub.join("crontab"),
        &format!(
            "#!/bin/sh\ncase \"$1\" in\n  -l) cat \"{d}/current\" ;;\n  -) cat > \"{d}/written\" ;;\nesac\n",
            d = stub.display()
        ),
    );

    migrate(
        &fx.paths,
        &fx.config,
        &no_claude(),
        &env(&fx),
        &Crontab::at(&stub.join("crontab")),
    )
    .unwrap();

    assert!(read(&stub.join("written")).contains(&fx.paths.bin.display().to_string()));
}

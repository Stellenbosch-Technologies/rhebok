use rhebok::setup::claude::ClaudeCli;
use rhebok::setup::install::{install, uninstall, InstallOpts, UninstallOpts};
use rhebok::setup::ollama::OllamaModels;
use rhebok::setup::Paths;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

fn executable(p: &Path, body: &str) {
    std::fs::write(p, body).unwrap();
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn stub_claude(dir: &Path) -> ClaudeCli {
    std::fs::write(dir.join("marketplaces.json"), "[]").unwrap();
    std::fs::write(dir.join("plugins.json"), "[]").unwrap();
    let p = dir.join("claude");
    executable(
        &p,
        &format!(
            "#!/bin/sh\necho \"$*\" >> \"{d}/calls\"\ncase \"$*\" in\n\
             '--version') exit 0 ;;\n\
             'plugin marketplace list --json') cat \"{d}/marketplaces.json\" ;;\n\
             'plugin list --json') cat \"{d}/plugins.json\" ;;\n\
             *) exit 0 ;;\nesac\n",
            d = dir.display()
        ),
    );
    ClaudeCli::at(&p)
}

fn calls(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("calls"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

struct Fx {
    _t: tempfile::TempDir,
    root: PathBuf,
    stubdir: PathBuf,
    paths: Paths,
    opts: InstallOpts,
}

fn fixture(version_printed: &str) -> Fx {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("root");
    let stubdir = t.path().join("stub");
    let linkdir = t.path().join("linkdir");
    std::fs::create_dir_all(&stubdir).unwrap();
    std::fs::create_dir_all(&linkdir).unwrap();
    let exe = stubdir.join("rhebok-src");
    executable(
        &exe,
        &format!("#!/bin/sh\necho 'rhebok {version_printed}'\n"),
    );
    let paths = Paths::at(&root, vec![linkdir], t.path().join("cache/rhebok"));
    let opts = InstallOpts {
        exe,
        version: "1.2.3".to_string(),
        config_path: t.path().join("cfg/config.toml"),
        claude: stub_claude(&stubdir),
        yes: true,
        quiet: true,
        ollama_url: "http://127.0.0.1:1".to_string(),
        models: OllamaModels {
            required: vec!["qwen3-embedding:0.6b".to_string()],
            optional: vec![],
        },
        embed: None,
        confirm: |_, _| false,
        pull: |_| Ok(()),
    };
    Fx {
        _t: t,
        root,
        stubdir,
        paths,
        opts,
    }
}

#[test]
fn a_first_install_lays_everything_out_and_registers() {
    let fx = fixture("1.2.3");
    let report = install(&fx.paths, &fx.opts).unwrap();
    assert!(fx.paths.bin.is_file(), "binary placed");
    assert_eq!(
        std::fs::read(&fx.paths.bin).unwrap(),
        std::fs::read(&fx.opts.exe).unwrap()
    );
    let plugin =
        std::fs::read_to_string(fx.root.join("plugin/.claude-plugin/plugin.json")).unwrap();
    assert!(
        plugin.contains("\"1.2.3\"")
            && plugin.contains(&fx.paths.bin.to_string_lossy().to_string())
    );
    let link = fx.paths.link_dirs[0].join("rhebok");
    assert_eq!(std::fs::read_link(&link).unwrap(), fx.paths.bin);
    assert!(fx.opts.config_path.is_file(), "starter config written");
    assert!(std::fs::read_to_string(&fx.opts.config_path)
        .unwrap()
        .contains("sources"));
    assert_eq!(
        calls(&fx.stubdir),
        vec![
            "--version".to_string(),
            "plugin marketplace list --json".to_string(),
            format!("plugin marketplace add {}", fx.paths.plugin.display()),
            "plugin list --json".to_string(),
            "plugin install rhebok@rhebok".to_string(),
        ]
    );
    assert_eq!(report.warnings.len(), 2, "{:?}", report.warnings);
    assert!(
        report.warnings.iter().any(|w| w.contains("is not on PATH")),
        "{:?}",
        report.warnings
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("Ollama is not reachable")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn a_second_install_copies_no_binary_keeps_the_config_and_updates_the_plugin() {
    let fx = fixture("1.2.3");
    install(&fx.paths, &fx.opts).unwrap();
    std::fs::write(&fx.opts.config_path, "sources = [\"/mine\"]\n").unwrap();
    let before_ino = std::fs::metadata(&fx.paths.bin).unwrap().ino();

    let stray = fx.paths.plugin.join("commands/old.md");
    std::fs::create_dir_all(stray.parent().unwrap()).unwrap();
    std::fs::write(&stray, "stale").unwrap();

    std::fs::write(
        fx.stubdir.join("marketplaces.json"),
        format!(
            r#"[{{"name":"rhebok","source":"directory","path":"{}"}}]"#,
            fx.paths.plugin.display()
        ),
    )
    .unwrap();
    std::fs::write(
        fx.stubdir.join("plugins.json"),
        r#"[{"id":"rhebok@rhebok","version":"1.2.3"}]"#,
    )
    .unwrap();
    std::fs::remove_file(fx.stubdir.join("calls")).unwrap();

    let opts = InstallOpts {
        exe: fx.paths.bin.clone(),
        ..clone_opts(&fx.opts)
    };
    install(&fx.paths, &opts).unwrap();
    assert_eq!(
        std::fs::metadata(&fx.paths.bin).unwrap().ino(),
        before_ino,
        "binary must not be re-copied when it is already in place"
    );
    assert_eq!(
        std::fs::read_to_string(&fx.opts.config_path).unwrap(),
        "sources = [\"/mine\"]\n"
    );
    assert!(
        !stray.exists(),
        "stale plugin file must be removed by re-install"
    );
    assert_eq!(
        calls(&fx.stubdir),
        vec![
            "--version",
            "plugin marketplace list --json",
            "plugin list --json",
            "plugin update rhebok@rhebok"
        ]
    );
}

fn clone_opts(o: &InstallOpts) -> InstallOpts {
    InstallOpts {
        exe: o.exe.clone(),
        version: o.version.clone(),
        config_path: o.config_path.clone(),
        claude: o.claude.clone(),
        yes: o.yes,
        quiet: o.quiet,
        ollama_url: o.ollama_url.clone(),
        models: o.models.clone(),
        embed: o.embed,
        confirm: o.confirm,
        pull: o.pull,
    }
}

#[test]
fn a_marketplace_named_rhebok_pointing_elsewhere_is_repointed() {
    let fx = fixture("1.2.3");
    std::fs::write(
        fx.stubdir.join("marketplaces.json"),
        r#"[{"name":"rhebok","source":"directory","path":"/old/clone"}]"#,
    )
    .unwrap();
    let report = install(&fx.paths, &fx.opts).unwrap();
    let c = calls(&fx.stubdir);
    let remove = c
        .iter()
        .position(|l| l == "plugin marketplace remove rhebok")
        .expect("remove");
    let add = c
        .iter()
        .position(|l| l == &format!("plugin marketplace add {}", fx.paths.plugin.display()))
        .expect("add");
    assert!(remove < add, "remove must precede add: {c:?}");
    assert!(
        report.lines.iter().any(|l| l.contains("/old/clone")),
        "{:?}",
        report.lines
    );
}

#[test]
fn a_rhebok_plugin_from_another_marketplace_gives_way_to_the_installed_one() {
    let fx = fixture("1.2.3");
    std::fs::write(
        fx.stubdir.join("plugins.json"),
        r#"[{"id":"rhebok@claude-community","version":"1.2.0"},{"id":"rhebok-extras@elsewhere","version":"1.0.0"}]"#,
    )
    .unwrap();
    let report = install(&fx.paths, &fx.opts).unwrap();
    let c = calls(&fx.stubdir);
    let ours = c
        .iter()
        .position(|l| l == "plugin install rhebok@rhebok")
        .expect("install");
    let theirs = c
        .iter()
        .position(|l| l == "plugin uninstall rhebok@claude-community")
        .expect("uninstall");
    assert!(
        ours < theirs,
        "never leave Claude Code without the plugin: {c:?}"
    );
    assert!(
        !c.iter().any(|l| l.contains("rhebok-extras")),
        "a plugin that only shares the prefix is not rhebok's: {c:?}"
    );
    assert!(
        report
            .lines
            .iter()
            .any(|l| l.contains("removed rhebok@claude-community")),
        "{:?}",
        report.lines
    );
}

#[test]
fn when_the_plugin_cannot_be_installed_the_other_rhebok_plugin_stays() {
    let fx = fixture("1.2.3");
    std::fs::write(
        fx.stubdir.join("plugins.json"),
        r#"[{"id":"rhebok@claude-community","version":"1.2.0"}]"#,
    )
    .unwrap();
    let claude = fx.stubdir.join("claude");
    let body = std::fs::read_to_string(&claude).unwrap().replace(
        "*) exit 0 ;;",
        "'plugin install rhebok@rhebok') exit 1 ;;\n*) exit 0 ;;",
    );
    executable(&claude, &body);
    let report = install(&fx.paths, &fx.opts).unwrap();
    assert!(!calls(&fx.stubdir)
        .iter()
        .any(|l| l.starts_with("plugin uninstall")));
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("claude plugin install rhebok@rhebok")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn a_placed_binary_that_reports_the_wrong_version_is_a_hard_failure() {
    let fx = fixture("0.0.9");
    let err = install(&fx.paths, &fx.opts).unwrap_err().to_string();
    assert!(err.contains("0.0.9") && err.contains("1.2.3"), "{err}");
}

#[test]
fn a_missing_claude_cli_is_a_warning_and_everything_else_still_happens() {
    let mut fx = fixture("1.2.3");
    fx.opts.claude = ClaudeCli::at(Path::new("/nonexistent/claude"));
    let report = install(&fx.paths, &fx.opts).unwrap();
    assert!(fx.paths.bin.is_file() && fx.paths.plugin.join("hooks/hooks.json").is_file());
    assert!(
        report.warnings.iter().any(|w| w.contains("Claude Code")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn the_link_dir_is_created_when_missing_and_an_existing_wrong_link_is_replaced() {
    let fx = fixture("1.2.3");
    let dir = fx.paths.link_dirs[0].clone();
    std::fs::remove_dir(&dir).unwrap();
    install(&fx.paths, &fx.opts).unwrap();
    assert_eq!(
        std::fs::read_link(dir.join("rhebok")).unwrap(),
        fx.paths.bin
    );
    std::os::unix::fs::symlink("/elsewhere", dir.join("rhebok")).ok();
    std::fs::remove_file(dir.join("rhebok")).unwrap();
    std::os::unix::fs::symlink("/elsewhere", dir.join("rhebok")).unwrap();
    install(&fx.paths, &fx.opts).unwrap();
    assert_eq!(
        std::fs::read_link(dir.join("rhebok")).unwrap(),
        fx.paths.bin
    );
}

fn installed_fixture() -> Fx {
    let fx = fixture("1.2.3");
    install(&fx.paths, &fx.opts).unwrap();
    std::fs::create_dir_all(&fx.paths.db).unwrap();
    std::fs::write(fx.paths.db.join("graph.kz"), b"x").unwrap();
    std::fs::create_dir_all(&fx.paths.claude_cache).unwrap();
    std::fs::write(fx.paths.claude_cache.join("stale"), b"x").unwrap();
    std::fs::write(
        fx.stubdir.join("marketplaces.json"),
        format!(
            r#"[{{"name":"rhebok","source":"directory","path":"{}"}}]"#,
            fx.paths.plugin.display()
        ),
    )
    .unwrap();
    std::fs::write(
        fx.stubdir.join("plugins.json"),
        r#"[{"id":"rhebok@rhebok","version":"1.2.3"}]"#,
    )
    .unwrap();
    std::fs::remove_file(fx.stubdir.join("calls")).unwrap();
    fx
}

fn unopts(fx: &Fx, purge: bool) -> UninstallOpts {
    UninstallOpts {
        purge,
        yes: true,
        claude: fx.opts.claude.clone(),
        config_path: fx.opts.config_path.clone(),
        confirm: |_, _| false,
    }
}

#[test]
fn uninstall_removes_what_install_made_and_keeps_the_data() {
    let fx = installed_fixture();
    let report = uninstall(&fx.paths, &unopts(&fx, false)).unwrap();
    assert!(!fx.paths.bin.exists() && !fx.paths.plugin.exists() && !fx.paths.tmp.exists());
    assert!(
        !fx.paths.link_dirs[0]
            .join("rhebok")
            .symlink_metadata()
            .is_ok(),
        "link removed"
    );
    assert!(!fx.paths.claude_cache.exists(), "claude cache removed");
    assert!(fx.paths.db.join("graph.kz").is_file(), "index kept");
    assert!(fx.opts.config_path.is_file(), "config kept");
    assert!(report.kept.contains(&fx.opts.config_path) && report.kept.contains(&fx.paths.db));
    assert_eq!(
        calls(&fx.stubdir),
        vec![
            "--version",
            "plugin list --json",
            "plugin uninstall rhebok@rhebok",
            "plugin marketplace list --json",
            "plugin marketplace remove rhebok"
        ]
    );
}

#[test]
fn uninstall_removes_the_rhebok_plugin_whichever_marketplace_it_came_from() {
    let fx = installed_fixture();
    std::fs::write(
        fx.stubdir.join("plugins.json"),
        r#"[{"id":"rhebok@rhebok","version":"1.2.3"},{"id":"rhebok@claude-community","version":"1.2.0"}]"#,
    )
    .unwrap();
    let report = uninstall(&fx.paths, &unopts(&fx, false)).unwrap();
    let c = calls(&fx.stubdir);
    for id in ["rhebok@rhebok", "rhebok@claude-community"] {
        assert!(
            c.contains(&format!("plugin uninstall {id}")),
            "{id} left behind: {c:?}"
        );
        assert!(
            report.lines.contains(&format!("unregistered {id}")),
            "{:?}",
            report.lines
        );
    }
}

#[test]
fn purge_removes_the_root_and_the_config() {
    let fx = installed_fixture();
    uninstall(&fx.paths, &unopts(&fx, true)).unwrap();
    assert!(!fx.root.exists());
    assert!(!fx.opts.config_path.exists());
}

#[test]
fn purge_without_consent_is_refused_and_removes_nothing() {
    let fx = installed_fixture();
    let opts = UninstallOpts {
        yes: false,
        ..unopts(&fx, true)
    };
    assert!(uninstall(&fx.paths, &opts).is_err());
    assert!(fx.paths.bin.is_file() && fx.opts.config_path.is_file());
}

#[test]
fn uninstall_refuses_while_an_index_holds_the_lock() {
    let fx = installed_fixture();
    std::fs::write(
        fx.paths.db.with_extension("lock"),
        std::process::id().to_string(),
    )
    .unwrap();
    let err = uninstall(&fx.paths, &unopts(&fx, false))
        .unwrap_err()
        .to_string();
    assert!(err.contains("index"), "{err}");
    assert!(fx.paths.bin.is_file());
}

#[test]
fn a_foreign_link_and_a_foreign_marketplace_are_left_alone() {
    let fx = installed_fixture();
    let link = fx.paths.link_dirs[0].join("rhebok");
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("/elsewhere/rhebok", &link).unwrap();
    std::fs::write(
        fx.stubdir.join("marketplaces.json"),
        r#"[{"name":"rhebok","source":"directory","path":"/somebody/elses"}]"#,
    )
    .unwrap();
    let report = uninstall(&fx.paths, &unopts(&fx, false)).unwrap();
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        std::path::PathBuf::from("/elsewhere/rhebok")
    );
    assert!(!calls(&fx.stubdir)
        .iter()
        .any(|c| c.starts_with("plugin marketplace remove")));
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("/somebody/elses")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn with_no_model_left_on_ollama_install_does_not_ask_for_ollama() {
    let fx = fixture("1.2.3");
    let opts = InstallOpts {
        models: OllamaModels::default(),
        ..clone_opts(&fx.opts)
    };
    let report = install(&fx.paths, &opts).unwrap();
    let said = [report.lines.clone(), report.warnings.clone()].concat();
    assert!(
        !said.iter().any(|l| l.to_lowercase().contains("ollama")),
        "{said:?}"
    );
}

#[test]
fn each_failed_install_check_names_the_fix_the_dashboard_offers() {
    use rhebok::setup::install::{fix_for, Fix, InstallCheck};
    let exe = Path::new("/opt/build/rhebok");
    let failed = |name: &'static str| InstallCheck {
        name,
        ok: false,
        detail: String::new(),
    };
    let install = Some(Fix::Command {
        command: "\"/opt/build/rhebok\" install".to_string(),
    });
    let connect = Some(Fix::Connect {
        agent: "claude-code",
    });
    for name in ["plugin", "marketplace", "registration"] {
        assert_eq!(fix_for(&failed(name), exe, true), connect, "{name}");
    }
    assert_eq!(
        fix_for(&failed("plugin"), exe, false),
        install,
        "without `claude` the plugin is only written by an install"
    );
    for name in ["binary", "path"] {
        assert_eq!(fix_for(&failed(name), exe, true), install, "{name}");
    }
    assert!(matches!(
        fix_for(&failed("claude"), exe, false),
        Some(Fix::Manual { .. })
    ));
    let passing = InstallCheck {
        ok: true,
        ..failed("plugin")
    };
    assert_eq!(fix_for(&passing, exe, true), None);
}

static PULLED_BY_UPDATE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn ollama_with_only_the_embedder() -> String {
    use std::io::{Read, Write};
    let body = r#"{"models":[{"name":"qwen3-embedding:0.6b"}]}"#;
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
        }
    });
    format!("http://{addr}")
}

#[test]
fn install_yes_quiet_as_update_runs_it_does_not_pull_the_distill_model() {
    let fx = fixture("1.2.3");
    PULLED_BY_UPDATE.lock().unwrap().clear();
    let opts = InstallOpts {
        yes: true,
        quiet: true,
        ollama_url: ollama_with_only_the_embedder(),
        models: OllamaModels {
            required: vec!["qwen3-embedding:0.6b".to_string()],
            optional: vec!["qwen3:4b-instruct".to_string()],
        },
        confirm: |q, _| panic!("install --yes --quiet asked: {q}"),
        pull: |m| {
            PULLED_BY_UPDATE.lock().unwrap().push(m.to_string());
            Ok(())
        },
        ..clone_opts(&fx.opts)
    };
    let report = install(&fx.paths, &opts).unwrap();
    assert!(
        PULLED_BY_UPDATE.lock().unwrap().is_empty(),
        "{:?}",
        PULLED_BY_UPDATE.lock().unwrap()
    );
    assert!(
        report
            .lines
            .iter()
            .any(|l| l.contains("ollama pull qwen3:4b-instruct")
                && l.contains("distill_episodes = false")),
        "{:?}",
        report.lines
    );
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.contains("qwen3:4b-instruct")),
        "an optional model left unpulled is not a warning: {:?}",
        report.warnings
    );
}

#[test]
fn a_first_install_that_chooses_gemma_writes_its_model_and_both_gates() {
    let fx = fixture("1.2.3");
    let opts = InstallOpts {
        embed: Some(&rhebok::embed::choice::GEMMA),
        ..clone_opts(&fx.opts)
    };
    let report = install(&fx.paths, &opts).unwrap();
    let cfg = rhebok::config::Config::load_from(&fx.opts.config_path);
    assert_eq!(cfg.embed.model, "embeddinggemma-2:270m-bf16-text");
    assert_eq!(cfg.embed.dimensions, 512);
    assert_eq!(cfg.hook.threshold, 0.82);
    assert_eq!(cfg.mcp.threshold, 0.68);
    let text = std::fs::read_to_string(&fx.opts.config_path).unwrap();
    assert!(
        text.starts_with(rhebok::setup::install::STARTER_CONFIG),
        "the model goes after the starter, so its commented top-level switches stay top-level: {text}"
    );
    let uncommented = text.replace("# index_transcripts = false", "index_transcripts = false");
    let uncommented: rhebok::config::Config = toml::from_str(&uncommented).unwrap();
    assert!(!uncommented.index_transcripts, "{text}");
    assert!(
        rhebok::config::check::check(&text).is_empty(),
        "{:?}",
        rhebok::config::check::check(&text)
    );
    assert!(
        report
            .lines
            .iter()
            .any(|l| l.starts_with("embed:") && l.contains("hook gate 0.82")),
        "{:?}",
        report.lines
    );
    assert!(
        !report.warnings.iter().any(|w| w.contains("--reindex")),
        "a fresh install has no index to rebuild: {:?}",
        report.warnings
    );
}

#[test]
fn a_first_install_that_keeps_the_default_model_leaves_the_starter_untouched() {
    let fx = fixture("1.2.3");
    let opts = InstallOpts {
        embed: Some(&rhebok::embed::choice::QWEN3),
        ..clone_opts(&fx.opts)
    };
    install(&fx.paths, &opts).unwrap();
    assert_eq!(
        std::fs::read_to_string(&fx.opts.config_path).unwrap(),
        rhebok::setup::install::STARTER_CONFIG
    );
}

#[test]
fn choosing_a_different_model_on_reinstall_switches_it_and_says_to_reindex() {
    let fx = fixture("1.2.3");
    install(&fx.paths, &fx.opts).unwrap();
    std::fs::write(
        &fx.opts.config_path,
        "sources = [\"/mine\"]\n\n[hook]\nthreshold = 0.68\n",
    )
    .unwrap();
    let opts = InstallOpts {
        exe: fx.paths.bin.clone(),
        embed: Some(&rhebok::embed::choice::GEMMA),
        ..clone_opts(&fx.opts)
    };
    let report = install(&fx.paths, &opts).unwrap();
    let cfg = rhebok::config::Config::load_from(&fx.opts.config_path);
    assert_eq!(cfg.embed.model, "embeddinggemma-2:270m-bf16-text");
    assert_eq!(cfg.hook.threshold, 0.82, "the old model's gate is replaced");
    assert_eq!(cfg.sources, vec![std::path::PathBuf::from("/mine")]);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("rhebok index --reindex")
                && w.contains("from qwen3-embedding:0.6b to embeddinggemma-2:270m-bf16-text")),
        "{:?}",
        report.warnings
    );

    let again = install(&fx.paths, &opts).unwrap();
    assert!(
        !again.warnings.iter().any(|w| w.contains("--reindex")),
        "choosing the model already configured changes nothing: {:?}",
        again.warnings
    );
}

#[test]
fn reinstalling_the_configured_model_reports_the_gates_config_toml_holds() {
    let fx = fixture("1.2.3");
    install(&fx.paths, &fx.opts).unwrap();
    let written =
        "sources = [\"/mine\"]\n\n[hook]\nthreshold = 0.76\n\n[mcp]\nthreshold = 0.68\n\n\
                   [embed]\nmodel = \"embeddinggemma-2:270m-bf16-text\"\ndimensions = 512\n";
    std::fs::write(&fx.opts.config_path, written).unwrap();
    let opts = InstallOpts {
        exe: fx.paths.bin.clone(),
        embed: Some(&rhebok::embed::choice::GEMMA),
        ..clone_opts(&fx.opts)
    };
    let report = install(&fx.paths, &opts).unwrap();
    let line = report
        .lines
        .iter()
        .find(|l| l.starts_with("embed:"))
        .unwrap_or_else(|| panic!("{:?}", report.lines));
    assert!(line.contains("hook gate 0.76"), "{line}");
    assert!(line.contains("MCP gate 0.68"), "{line}");
    assert!(line.contains("the preset is 0.82 and 0.68"), "{line}");
    assert_eq!(
        std::fs::read_to_string(&fx.opts.config_path).unwrap(),
        written,
        "gates the config already holds are left alone"
    );

    std::fs::write(
        &fx.opts.config_path,
        written.replace("threshold = 0.76", "threshold = 0.82"),
    )
    .unwrap();
    let report = install(&fx.paths, &opts).unwrap();
    let line = report
        .lines
        .iter()
        .find(|l| l.starts_with("embed:"))
        .unwrap_or_else(|| panic!("{:?}", report.lines));
    assert!(line.contains("hook gate 0.82, MCP gate 0.68"), "{line}");
    assert!(!line.contains("preset"), "{line}");
}

#[test]
fn the_closing_summary_points_at_the_warnings_when_there_are_any() {
    let config = Path::new("/home/someone/.config/rhebok/config.toml");
    let clean = rhebok::setup::install::next_steps(config, 0);
    assert!(clean.contains("Installed. Two things left"), "{clean}");
    assert!(!clean.contains("marked !"), "{clean}");

    let one = rhebok::setup::install::next_steps(config, 1);
    assert!(one.contains("read the line marked ! above"), "{one}");

    let three = rhebok::setup::install::next_steps(config, 3);
    assert!(three.contains("read the 3 lines marked ! above"), "{three}");

    for summary in [&clean, &one, &three] {
        assert!(summary.contains(&config.display().to_string()), "{summary}");
        assert!(summary.contains("rhebok index"), "{summary}");
    }
}

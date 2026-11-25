use rhebok::setup::agents::AgentEnv;
use rhebok::setup::claude::ClaudeCli;
use rhebok::setup::install::{install, uninstall, InstallOpts, UninstallOpts};
use rhebok::setup::migrate::{migrate, Crontab};
use rhebok::setup::{homebrew_prefix, Paths};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn executable(p: &Path, body: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
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
            "#!/bin/sh\ncase \"$*\" in\n\
             'plugin marketplace list --json') cat \"{d}/marketplaces.json\" ;;\n\
             'plugin list --json') cat \"{d}/plugins.json\" ;;\n\
             *) exit 0 ;;\nesac\n",
            d = dir.display()
        ),
    );
    ClaudeCli::at(&p)
}

struct Fx {
    _t: tempfile::TempDir,
    prefix: PathBuf,
    keg_bin: PathBuf,
    paths: Paths,
    opts: InstallOpts,
}

fn homebrew_fixture() -> Fx {
    let t = tempfile::tempdir().unwrap();
    let prefix = t.path().join("brew");
    let keg_bin = prefix.join("Cellar/rhebok/1.2.3/bin/rhebok");
    executable(&keg_bin, "#!/bin/sh\necho 'rhebok 1.2.3'\n");
    std::fs::create_dir_all(prefix.join("opt")).unwrap();
    std::os::unix::fs::symlink("../Cellar/rhebok/1.2.3", prefix.join("opt/rhebok")).unwrap();
    let stub = t.path().join("stub");
    std::fs::create_dir_all(&stub).unwrap();
    let linkdir = t.path().join("linkdir");
    std::fs::create_dir_all(&linkdir).unwrap();
    let paths = Paths::at(
        &t.path().join("root"),
        vec![linkdir],
        t.path().join("cache"),
    )
    .installed_by_homebrew(&prefix);
    let opts = InstallOpts {
        exe: keg_bin.clone(),
        version: "1.2.3".to_string(),
        config_path: t.path().join("cfg/config.toml"),
        claude: stub_claude(&stub),
        yes: true,
        quiet: true,
        ollama_url: "http://127.0.0.1:1".to_string(),
        models: rhebok::setup::ollama::OllamaModels::default(),
        embed: None,
        confirm: |_, _| false,
        pull: |_| Ok(()),
        app: None,
    };
    Fx {
        _t: t,
        prefix,
        keg_bin,
        paths,
        opts,
    }
}

#[test]
fn a_binary_inside_a_homebrew_keg_names_the_prefix() {
    for (exe, prefix) in [
        (
            "/opt/homebrew/Cellar/rhebok/0.1.1/bin/rhebok",
            Some("/opt/homebrew"),
        ),
        (
            "/home/linuxbrew/.linuxbrew/Cellar/rhebok/0.1.1_1/bin/rhebok",
            Some("/home/linuxbrew/.linuxbrew"),
        ),
        (
            "/Users/x/Library/Application Support/rhebok/bin/rhebok",
            None,
        ),
        ("/opt/homebrew/Cellar/other/1.0/bin/rhebok", None),
        ("/opt/homebrew/Cellar/rhebok/0.1.1/libexec/rhebok", None),
    ] {
        assert_eq!(
            homebrew_prefix(Path::new(exe)),
            prefix.map(PathBuf::from),
            "{exe}"
        );
    }
}

#[test]
fn installed_by_homebrew_runs_the_opt_link_and_keeps_its_data_in_the_root() {
    let fx = homebrew_fixture();
    assert_eq!(fx.paths.bin, fx.prefix.join("opt/rhebok/bin/rhebok"));
    assert_eq!(fx.paths.homebrew.as_deref(), Some(fx.prefix.as_path()));
    assert!(fx.paths.db.starts_with(&fx.paths.root));
    assert!(fx.paths.plugin.starts_with(&fx.paths.root));
}

#[test]
fn a_homebrew_install_neither_copies_nor_links_the_binary() {
    let fx = homebrew_fixture();
    let report = install(&fx.paths, &fx.opts).unwrap();
    assert!(!fx.paths.root.join("bin").exists(), "nothing copied");
    assert!(
        std::fs::read_dir(&fx.paths.link_dirs[0])
            .unwrap()
            .next()
            .is_none(),
        "nothing linked"
    );
    let hooks = std::fs::read_to_string(fx.paths.plugin.join("hooks/hooks.json")).unwrap();
    assert!(
        hooks.contains(
            &fx.prefix
                .join("opt/rhebok/bin/rhebok")
                .display()
                .to_string()
        ),
        "the plugin must run the opt link, which survives brew upgrade: {hooks}"
    );
    assert!(
        report
            .lines
            .iter()
            .any(|l| l.contains("Homebrew puts rhebok on PATH")),
        "{:?}",
        report.lines
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(fx.keg_bin.is_file());
}

#[test]
fn a_keg_that_opt_does_not_link_refuses_to_install_over_the_one_it_does() {
    let fx = homebrew_fixture();
    let newer = fx.prefix.join("Cellar/rhebok/2.0.0/bin/rhebok");
    let newer_body = "#!/bin/sh\necho 'rhebok 2.0.0'\n";
    executable(&newer, newer_body);
    std::fs::remove_file(fx.prefix.join("opt/rhebok")).unwrap();
    std::os::unix::fs::symlink("../Cellar/rhebok/2.0.0", fx.prefix.join("opt/rhebok")).unwrap();

    let err = install(&fx.paths, &fx.opts).unwrap_err();
    let message = format!("{err:#}");
    assert!(
        message.contains(&fx.paths.bin.display().to_string()) && message.contains("install"),
        "{message}"
    );
    assert_eq!(std::fs::read_to_string(&newer).unwrap(), newer_body);
    assert!(fx.keg_bin.is_file());
    assert!(
        !fx.paths.plugin.exists(),
        "a refused install writes nothing"
    );
}

#[test]
fn a_homebrew_install_retires_the_copy_an_earlier_install_placed() {
    let fx = homebrew_fixture();
    let own = fx.paths.root.join("bin/rhebok");
    executable(&own, "#!/bin/sh\necho 'rhebok 1.0.0'\n");
    let linkdir = &fx.paths.link_dirs[0];
    std::os::unix::fs::symlink(&own, linkdir.join("rhebok")).unwrap();
    std::os::unix::fs::symlink("/elsewhere/tool", linkdir.join("tool")).unwrap();

    let report = install(&fx.paths, &fx.opts).unwrap();
    assert!(!own.exists() && !fx.paths.root.join("bin").exists());
    assert!(linkdir.join("rhebok").symlink_metadata().is_err());
    assert!(
        linkdir.join("tool").symlink_metadata().is_ok(),
        "a link that is not rhebok's stays"
    );
    assert!(
        report
            .lines
            .iter()
            .any(|l| l.contains("the copy from before Homebrew installed rhebok")),
        "{:?}",
        report.lines
    );
}

#[test]
fn uninstall_leaves_homebrews_binary_and_says_how_to_remove_it() {
    let fx = homebrew_fixture();
    install(&fx.paths, &fx.opts).unwrap();
    let report = uninstall(
        &fx.paths,
        &UninstallOpts {
            purge: false,
            yes: true,
            claude: fx.opts.claude.clone(),
            config_path: fx.opts.config_path.clone(),
            confirm: |_, _| false,
        },
    )
    .unwrap();
    assert!(
        fx.keg_bin.is_file(),
        "Homebrew's keg is Homebrew's to remove"
    );
    assert!(!fx.paths.plugin.exists());
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("brew uninstall rhebok")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn a_br8n_install_migrated_into_a_homebrew_rhebok_keeps_no_binary_of_its_own() {
    let fx = homebrew_fixture();
    let data = fx._t.path().join("data");
    let old = data.join("br8n");
    executable(&old.join("bin/br8n"), "#!/bin/sh\necho 'br8n 0.4.0'\n");
    std::fs::create_dir_all(old.join("db")).unwrap();
    std::fs::write(old.join("db/graph.kz"), "index bytes").unwrap();
    let linkdir = fx.paths.link_dirs[0].clone();
    std::os::unix::fs::symlink(old.join("bin/br8n"), linkdir.join("br8n")).unwrap();
    let paths = Paths::at(
        &data.join("rhebok"),
        vec![linkdir.clone()],
        fx.paths.claude_cache.clone(),
    )
    .installed_by_homebrew(&fx.prefix);
    let opts = InstallOpts {
        config_path: paths.root.join("config.toml"),
        ..fx.opts
    };

    migrate(
        &paths,
        &opts.config_path,
        &opts.claude,
        &AgentEnv::at(&fx._t.path().join("home"), paths.clone(), vec![]),
        &Crontab::at(Path::new("/nonexistent/crontab")),
    )
    .unwrap();
    let report = install(&paths, &opts).unwrap();

    assert_eq!(
        std::fs::read_to_string(paths.db.join("graph.kz")).unwrap(),
        "index bytes"
    );
    assert!(
        !paths.root.join("bin").exists(),
        "the br8n binary must not survive in the data folder Homebrew's rhebok uses"
    );
    assert!(
        std::fs::read_dir(&linkdir).unwrap().next().is_none(),
        "nothing linked"
    );
    let hooks = std::fs::read_to_string(paths.plugin.join("hooks/hooks.json")).unwrap();
    assert!(
        hooks.contains(
            &fx.prefix
                .join("opt/rhebok/bin/rhebok")
                .display()
                .to_string()
        ),
        "{hooks}"
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(fx.keg_bin.is_file());
}

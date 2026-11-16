use rhebok::config::Config;
use rhebok::project::{is_trusted_in, resolve_with, trust_in, Origin, Rule};
use std::path::Path;

fn config(text: &str) -> Config {
    toml::from_str(text).unwrap()
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn the_deepest_matching_root_selects_the_project() {
    let t = tempfile::tempdir().unwrap();
    let work = t.path().join("work");
    let cfg = config(&format!(
        "[projects.work]\nroots = [\"{w}\"]\n\n[projects.api]\nroots = [\"{w}/api\"]\n",
        w = work.display()
    ));
    let trust = t.path().join("trust.json");
    assert_eq!(
        resolve_with(&cfg, &work.join("api/src"), &trust).name(),
        Some("api")
    );
    assert_eq!(
        resolve_with(&cfg, &work.join("web"), &trust).name(),
        Some("work")
    );
    assert_eq!(resolve_with(&cfg, t.path(), &trust).name(), None);
}

#[test]
fn a_root_that_only_shares_a_name_prefix_does_not_match() {
    let t = tempfile::tempdir().unwrap();
    let cfg = config(&format!(
        "[projects.api]\nroots = [\"{}/api\"]\n",
        t.path().display()
    ));
    let trust = t.path().join("trust.json");
    assert_eq!(
        resolve_with(&cfg, &t.path().join("api-old"), &trust).name(),
        None
    );
}

#[test]
fn a_repo_project_file_is_ignored_until_trusted_and_again_after_any_edit() {
    let t = tempfile::tempdir().unwrap();
    let repo = t.path().join("repo");
    let file = repo.join(".rhebok/project.toml");
    write(&file, "name = \"side\"\ninclude = [\"tag:side\"]\n");
    let trust = t.path().join("trust.json");
    let cfg = Config::default();

    let before = resolve_with(&cfg, &repo.join("src"), &trust);
    assert_eq!(before.name(), None);
    assert_eq!(before.untrusted.as_deref(), Some(file.as_path()));

    trust_in(&trust, &repo).unwrap();
    let trusted = resolve_with(&cfg, &repo.join("src"), &trust);
    assert_eq!(trusted.name(), Some("side"));
    assert!(matches!(trusted.project.unwrap().origin, Origin::Repo(_)));
    assert!(trusted.untrusted.is_none());

    write(&file, "name = \"side\"\ninclude = [\"tag:everything\"]\n");
    assert!(!is_trusted_in(&trust, &file));
    assert_eq!(resolve_with(&cfg, &repo.join("src"), &trust).name(), None);
}

#[test]
fn trusting_a_file_with_a_broken_rule_is_refused() {
    let t = tempfile::tempdir().unwrap();
    let repo = t.path().join("repo");
    write(
        &repo.join(".rhebok/project.toml"),
        "name = \"x\"\ninclude = [\"notes/**\"]\n",
    );
    let err = trust_in(&t.path().join("trust.json"), &repo).unwrap_err();
    assert!(format!("{err:#}").contains("is not a rule"), "{err:#}");
}

#[test]
fn a_config_project_wins_over_a_repo_file_at_the_same_depth() {
    let t = tempfile::tempdir().unwrap();
    let repo = t.path().join("repo");
    write(&repo.join(".rhebok/project.toml"), "name = \"from-repo\"\n");
    let trust = t.path().join("trust.json");
    trust_in(&trust, &repo).unwrap();
    let cfg = config(&format!(
        "[projects.mine]\nroots = [\"{}\"]\n",
        repo.display()
    ));
    assert_eq!(resolve_with(&cfg, &repo, &trust).name(), Some("mine"));
}

#[test]
fn rules_parse_by_their_prefix() {
    assert!(matches!(Rule::parse("tag:adr").unwrap(), Rule::Tag(t) if t == "adr"));
    assert!(matches!(Rule::parse("type:pdf").unwrap(), Rule::Type(t) if t == "pdf"));
    assert!(matches!(Rule::parse("agent:codex").unwrap(), Rule::Agent(a) if a == "codex"));
    assert!(matches!(
        Rule::parse("file:///notes/**").unwrap(),
        Rule::Uri(_)
    ));
    assert!(Rule::parse("agent:gpt").is_err());
    assert!(Rule::parse("tag:").is_err());
    assert!(Rule::parse("notes/**").is_err());
}

#[test]
fn config_check_names_every_project_problem() {
    let errors = rhebok::config::check::check(
        "[projects.brain]\nroots = []\ninclude = [\"notes/**\"]\nimports = [\"vault:adr\"]\ncolour = \"red\"\n\n[projects.brain.slices.adr]\ninclude = [\"tag:adr\"]\nbogus = 1\n",
    );
    let text: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
    let has = |needle: &str| text.iter().any(|t| t.contains(needle));
    assert!(
        has("projects.brain.colour: unknown key `colour`; a project takes"),
        "{text:?}"
    );
    assert!(
        has("projects.brain.slices.adr.bogus: unknown key `bogus`; a slice takes"),
        "{text:?}"
    );
    assert!(
        has("projects.brain.include: `notes/**` is not a rule"),
        "{text:?}"
    );
    assert!(
        has("projects.brain.imports: `vault:adr` names neither"),
        "{text:?}"
    );
    assert!(
        has("projects.brain.roots: a project needs at least one root"),
        "{text:?}"
    );
}

#[test]
fn a_valid_project_passes_config_check() {
    let errors = rhebok::config::check::check(
        "[projects.brain]\nroots = [\"/tmp\"]\ninclude = [\"tag:rust\", \"file:///notes/**\"]\nimports = [\"brain:adr\"]\n\n[projects.brain.slices.adr]\ninclude = [\"tag:adr\"]\n",
    );
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn chat_instructions_name_the_tool_and_the_root_that_selects_the_project() {
    let cfg = config("[projects.api]\nroots = [\"/work/api\"]\n");
    let text = rhebok::project::chat_instructions(&cfg, "api").unwrap();
    assert!(text.contains("`rhebok_context`"), "{text}");
    assert!(text.contains("`cwd` set to \"/work/api\""), "{text}");
    assert!(text.contains("project `api`"), "{text}");
    assert!(rhebok::project::chat_instructions(&cfg, "nope").is_err());
}

#[test]
fn the_instructions_command_warns_when_claude_desktop_cannot_see_rhebok() {
    let t = tempfile::tempdir().unwrap();
    let cfg = t.path().join("config.toml");
    std::fs::write(&cfg, "[projects.api]\nroots = [\"/work/api\"]\n").unwrap();
    let out = assert_cmd::Command::cargo_bin("rhebok")
        .unwrap()
        .env("RHEBOK_CONFIG", &cfg)
        .env("RHEBOK_DB", t.path().join("db"))
        .env("HOME", t.path())
        .args(["project", "instructions", "api"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with("Before you answer"), "{stdout}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("rhebok connect claude-desktop"), "{stderr}");
}

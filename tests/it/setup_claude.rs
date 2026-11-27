use rhebok::setup::claude::ClaudeCli;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn stub(dir: &Path, marketplaces: &str, plugins: &str, exit: i32) -> PathBuf {
    std::fs::write(dir.join("marketplaces.json"), marketplaces).unwrap();
    std::fs::write(dir.join("plugins.json"), plugins).unwrap();
    let script = format!(
        "#!/bin/sh\n\
         echo \"$*\" >> \"{d}/calls\"\n\
         case \"$*\" in\n\
           '--version') echo 'stub'; exit 0 ;;\n\
           'plugin marketplace list --json') cat \"{d}/marketplaces.json\"; exit 0 ;;\n\
           'plugin list --json') cat \"{d}/plugins.json\"; exit 0 ;;\n\
           *) echo 'stub refused' >&2; exit {exit} ;;\n\
         esac\n",
        d = dir.display()
    );
    let p = dir.join("claude");
    std::fs::write(&p, script).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn calls(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("calls"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

const MARKETS: &str = r#"[{"name":"rhebok","source":"directory","path":"/old/clone","installLocation":"/old/clone"},{"name":"official","source":"github","repo":"anthropics/x","installLocation":"/x"}]"#;
const PLUGINS: &str = r#"[{"id":"rhebok@rhebok","version":"0.2.0","scope":"user","enabled":true,"installPath":"/c/rhebok/rhebok/0.2.0"}]"#;

#[test]
fn lists_parse_the_json_shapes_the_real_cli_prints() {
    let t = tempfile::tempdir().unwrap();
    let cli = ClaudeCli::at(&stub(t.path(), MARKETS, PLUGINS, 0));
    assert!(cli.available());
    let m = cli.marketplaces().unwrap();
    assert_eq!(m.len(), 2);
    assert_eq!(m[0].name, "rhebok");
    assert_eq!(m[0].source, "directory");
    assert_eq!(m[0].path.as_deref(), Some(Path::new("/old/clone")));
    assert_eq!(m[1].path, None);
    let p = cli.plugins().unwrap();
    assert_eq!(p[0].id, "rhebok@rhebok");
    assert_eq!(p[0].version, "0.2.0");
    assert_eq!(
        p[0].install_path.as_deref(),
        Some(Path::new("/c/rhebok/rhebok/0.2.0"))
    );
}

#[test]
fn mutating_calls_pass_the_exact_argv() {
    let t = tempfile::tempdir().unwrap();
    let cli = ClaudeCli::at(&stub(t.path(), "[]", "[]", 0));
    cli.marketplace_add(Path::new("/r/plugin")).unwrap();
    cli.marketplace_remove("rhebok").unwrap();
    cli.plugin_install("rhebok@rhebok").unwrap();
    cli.plugin_update("rhebok@rhebok").unwrap();
    cli.plugin_uninstall("rhebok@rhebok").unwrap();
    assert_eq!(
        calls(t.path()),
        vec![
            "plugin marketplace add /r/plugin",
            "plugin marketplace remove rhebok",
            "plugin install rhebok@rhebok",
            "plugin update rhebok@rhebok",
            "plugin uninstall rhebok@rhebok",
        ]
    );
}

#[test]
fn a_failing_call_reports_the_command_and_the_stderr() {
    let t = tempfile::tempdir().unwrap();
    let cli = ClaudeCli::at(&stub(t.path(), "[]", "[]", 3));
    let err = cli.plugin_install("rhebok@rhebok").unwrap_err().to_string();
    assert!(err.contains("claude plugin install rhebok@rhebok"), "{err}");
    assert!(err.contains("stub refused"), "{err}");
}

#[test]
fn a_missing_program_is_not_available_and_lists_error() {
    let cli = ClaudeCli::at(Path::new("/nonexistent/claude"));
    assert!(!cli.available());
    assert!(cli.marketplaces().is_err());
}

#[test]
fn claude_is_looked_for_on_path_first_then_where_its_installers_and_homebrew_put_it() {
    let home = Path::new("/home/someone");
    let homebrew = [
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
        PathBuf::from("/home/linuxbrew/.linuxbrew/bin/claude"),
    ];
    let mut expected = vec![
        PathBuf::from("claude"),
        home.join(".local/bin/claude"),
        home.join(".claude/local/claude"),
    ];
    expected.extend(homebrew.clone());
    assert_eq!(ClaudeCli::candidates(Some(home)), expected);
    let mut without_home = vec![PathBuf::from("claude")];
    without_home.extend(homebrew);
    assert_eq!(ClaudeCli::candidates(None), without_home);
}

#[test]
fn an_app_started_without_homebrew_on_its_path_still_searches_homebrew() {
    let launched_from_finder =
        std::env::join_paths(["/usr/bin", "/bin", "/usr/sbin", "/sbin"]).ok();
    assert_eq!(
        rhebok::setup::agents::search_path_from(launched_from_finder),
        [
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin",
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/home/linuxbrew/.linuxbrew/bin",
        ]
        .map(PathBuf::from)
    );
}

#[test]
fn a_shell_path_keeps_its_order_and_gains_no_duplicate() {
    let shell = std::env::join_paths(["/usr/local/bin", "/Users/x/bin", "/opt/homebrew/bin"]).ok();
    assert_eq!(
        rhebok::setup::agents::search_path_from(shell),
        [
            "/usr/local/bin",
            "/Users/x/bin",
            "/opt/homebrew/bin",
            "/home/linuxbrew/.linuxbrew/bin",
        ]
        .map(PathBuf::from)
    );
    assert_eq!(
        rhebok::setup::agents::search_path_from(None),
        [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/home/linuxbrew/.linuxbrew/bin",
        ]
        .map(PathBuf::from)
    );
}

#[test]
fn the_first_candidate_that_runs_is_the_one_used() {
    let t = tempfile::tempdir().unwrap();
    let installed = stub(t.path(), "[]", "[]", 0);
    let cli =
        ClaudeCli::first_available(&[PathBuf::from("/nonexistent/claude"), installed.clone()]);
    assert_eq!(cli.program(), installed.as_path());
    assert!(cli.available());
}

#[test]
fn with_no_candidate_that_runs_the_bare_command_is_kept() {
    let cli = ClaudeCli::first_available(&[
        PathBuf::from("/nonexistent/claude"),
        PathBuf::from("/also/nonexistent/claude"),
    ]);
    assert_eq!(cli.program(), Path::new("claude"));
}

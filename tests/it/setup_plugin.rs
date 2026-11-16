use rhebok::setup::plugin::{render, write};
use std::path::Path;

fn rendered(rel: &str) -> String {
    let files = render(
        "9.8.7",
        Path::new("/Users/x/Library/Application Support/rhebok/bin/rhebok"),
    );
    let (_, bytes) = files.iter().find(|(p, _)| p == rel).unwrap_or_else(|| {
        panic!(
            "{rel} is not in the rendered plugin: {:?}",
            files.iter().map(|(p, _)| p).collect::<Vec<_>>()
        )
    });
    String::from_utf8(bytes.clone()).unwrap()
}

#[test]
fn every_embedded_file_is_rendered_and_no_placeholder_survives() {
    let files = render("9.8.7", Path::new("/opt/rhebok/bin/rhebok"));
    let names: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
    for want in [
        ".claude-plugin/plugin.json",
        ".claude-plugin/marketplace.json",
        "hooks/hooks.json",
        "commands/rhebok-index.md",
        "commands/rhebok-search.md",
        "commands/rhebok-status.md",
        "commands/rhebok-bench.md",
        "commands/rhebok-golden.md",
        "commands/rhebok-add.md",
        "commands/rhebok-remember.md",
        "commands/rhebok-memories.md",
        "skills/rhebok-retrieval/SKILL.md",
        "skills/rhebok-memory/SKILL.md",
        "scripts/rhebok.sh",
    ] {
        assert!(names.contains(&want), "missing {want} in {names:?}");
    }
    for (p, bytes) in &files {
        let s = String::from_utf8_lossy(bytes);
        assert!(
            !s.contains("scripts/rhebok.sh"),
            "{p} still runs the launcher instead of the installed binary"
        );
    }
}

#[test]
fn the_version_and_the_binary_path_are_substituted() {
    let plugin: serde_json::Value =
        serde_json::from_str(&rendered(".claude-plugin/plugin.json")).unwrap();
    assert_eq!(plugin["version"], "9.8.7");
    assert_eq!(
        plugin["mcpServers"]["rhebok"]["command"],
        "/Users/x/Library/Application Support/rhebok/bin/rhebok"
    );
}

#[test]
fn hook_commands_quote_the_binary_path_because_it_has_a_space() {
    let hooks: serde_json::Value = serde_json::from_str(&rendered("hooks/hooks.json")).unwrap();
    let prompt = hooks["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    let start = hooks["hooks"]["SessionStart"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert_eq!(
        prompt,
        "\"/Users/x/Library/Application Support/rhebok/bin/rhebok\" hook prompt"
    );
    assert_eq!(
        start,
        "\"/Users/x/Library/Application Support/rhebok/bin/rhebok\" hook session-start"
    );
    assert_eq!(hooks["hooks"]["SessionStart"][0]["hooks"][0]["timeout"], 10);
}

#[test]
fn write_lays_the_tree_out_and_overwrites_on_a_second_call() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "1.0.0", Path::new("/a/rhebok")).unwrap();
    let first = std::fs::read_to_string(t.path().join(".claude-plugin/plugin.json")).unwrap();
    assert!(first.contains("\"1.0.0\""));
    assert!(t.path().join("commands/rhebok-status.md").is_file());
    assert!(t.path().join("skills/rhebok-retrieval/SKILL.md").is_file());
    write(t.path(), "2.0.0", Path::new("/b/rhebok")).unwrap();
    let second = std::fs::read_to_string(t.path().join(".claude-plugin/plugin.json")).unwrap();
    assert!(second.contains("\"2.0.0\"") && second.contains("/b/rhebok"));
}

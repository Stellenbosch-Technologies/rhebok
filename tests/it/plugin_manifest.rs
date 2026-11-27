fn read(p: &str) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(p).expect(p)).expect(p)
}

#[test]
fn plugin_manifest_declares_name_and_mcp_server_and_carries_the_cargo_version() {
    let m = read("plugin/.claude-plugin/plugin.json");
    assert_eq!(m["name"], "rhebok");
    assert_eq!(
        m["version"],
        env!("CARGO_PKG_VERSION"),
        "bump plugin.json with Cargo.toml: a marketplace install reads this version"
    );
    assert_eq!(
        m["mcpServers"]["rhebok"]["command"],
        "${CLAUDE_PLUGIN_ROOT}/scripts/rhebok.sh"
    );
    assert_eq!(m["mcpServers"]["rhebok"]["args"][0], "mcp");
    let market = read("plugin/.claude-plugin/marketplace.json");
    assert_eq!(market["plugins"][0]["source"], "./");
    assert!(
        market["plugins"][0].get("version").is_none(),
        "plugin.json is the one place the plugin's version lives"
    );
}

#[test]
fn hooks_run_the_launcher_from_the_plugin_root() {
    let h = read("plugin/hooks/hooks.json");
    for (event, which) in [
        ("UserPromptSubmit", "prompt"),
        ("SessionStart", "session-start"),
    ] {
        let cmd = h["hooks"][event][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert_eq!(
            cmd,
            format!("\"${{CLAUDE_PLUGIN_ROOT}}/scripts/rhebok.sh\" hook {which}"),
            "{event}"
        );
    }
}

#[test]
fn the_launcher_is_an_executable_posix_shell_script() {
    use std::os::unix::fs::PermissionsExt;
    let p = "plugin/scripts/rhebok.sh";
    let mode = std::fs::metadata(p).unwrap().permissions().mode();
    assert_eq!(mode & 0o111, 0o111, "{p} must be executable: {mode:o}");
    assert!(std::fs::read_to_string(p)
        .unwrap()
        .starts_with("#!/bin/sh\n"));
}

#[test]
fn the_repository_root_is_a_marketplace_that_lists_the_plugin() {
    let market = read(".claude-plugin/marketplace.json");
    assert_eq!(market["name"], "rhebok");
    assert_eq!(market["plugins"][0]["name"], "rhebok");
    assert_eq!(market["plugins"][0]["source"], "./plugin");
}

#[test]
fn every_command_file_has_frontmatter_with_a_description() {
    for name in [
        "rhebok-index",
        "rhebok-search",
        "rhebok-status",
        "rhebok-bench",
        "rhebok-golden",
        "rhebok-add",
        "rhebok-remember",
        "rhebok-memories",
    ] {
        let p = format!("plugin/commands/{name}.md");
        let s = std::fs::read_to_string(&p).expect(&p);
        assert!(s.starts_with("---"), "{p} needs frontmatter");
        assert!(s.contains("description:"), "{p} needs a description");
    }
}

#[test]
fn skill_has_a_trigger_oriented_description() {
    let s = std::fs::read_to_string("plugin/skills/rhebok-retrieval/SKILL.md").unwrap();
    assert!(s.starts_with("---"));
    assert!(s.contains("name: rhebok-retrieval"));
    assert!(s.to_lowercase().contains("use when"));
}

#[test]
fn the_memory_skill_names_both_tools_and_when_to_use_them() {
    let s = std::fs::read_to_string("plugin/skills/rhebok-memory/SKILL.md").unwrap();
    assert!(s.starts_with("---"));
    assert!(s.contains("name: rhebok-memory"));
    assert!(s.to_lowercase().contains("use when"));
    assert!(s.contains("rhebok_save_memory") && s.contains("rhebok_delete_memory"));
    assert!(s.contains("<rhebok-lessons>"));
}

#[test]
fn the_memory_skill_names_every_origin_as_rhebok_renders_it() {
    use rhebok::memory::{saved_by, Origin};
    let s = std::fs::read_to_string("plugin/skills/rhebok-memory/SKILL.md")
        .unwrap()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for origin in [Origin::User, Origin::Claude, Origin::Distill] {
        let rendered = format!("`{}`", saved_by(origin));
        assert!(s.contains(&rendered), "the skill must name {rendered}");
    }
}

#[test]
fn the_directory_listing_fields_point_at_a_png_in_the_plugin_and_https_pages() {
    let m = read("plugin/.claude-plugin/plugin.json");
    let icon = m["icon"].as_str().expect("plugin.json names an icon");
    let path = std::path::Path::new("plugin").join(icon.trim_start_matches("./"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(
        bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "{} is not a PNG",
        path.display()
    );
    for field in ["documentationUrl", "supportUrl"] {
        let url = m[field]
            .as_str()
            .unwrap_or_else(|| panic!("{field} is set"));
        assert!(url.starts_with("https://"), "{field} is {url}");
    }
}

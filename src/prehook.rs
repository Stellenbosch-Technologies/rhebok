use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const STALE_MARKER_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Injector {
    #[default]
    Mod,
    Command,
    Off,
}

impl Injector {
    pub fn as_str(self) -> &'static str {
        match self {
            Injector::Mod => "mod",
            Injector::Command => "command",
            Injector::Off => "off",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Injector::Mod => {
                "mod (the rhebok-prehook mod injects; the command hook takes over in any \
                 session the mod is not running in)"
            }
            Injector::Command => "command (the classic UserPromptSubmit hook injects)",
            Injector::Off => "off (nothing is injected into prompts)",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrehookConfig {
    #[serde(default)]
    pub injector: Injector,
}

fn safe_session(session: &str) -> Option<&str> {
    let ok = !session.is_empty()
        && session.len() <= 128
        && session
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then_some(session)
}

pub fn marker(db: &Path, session: &str) -> Option<PathBuf> {
    safe_session(session).map(|s| db.with_extension(format!("mod.{s}")))
}

pub fn mark_mod_running(db: &Path, session: &str) -> std::io::Result<bool> {
    let Some(path) = marker(db, session) else {
        return Ok(false);
    };
    std::fs::write(path, b"")?;
    Ok(true)
}

pub fn mod_is_running(db: &Path, session: &str) -> bool {
    marker(db, session).is_some_and(|p| p.exists())
}

pub fn command_hook_injects(injector: Injector, db: &Path, session: Option<&str>) -> bool {
    match injector {
        Injector::Off => false,
        Injector::Command => true,
        Injector::Mod => !session.is_some_and(|s| mod_is_running(db, s)),
    }
}

pub fn sweep_stale_markers(db: &Path, max_age: Duration) {
    let (Some(dir), Some(stem)) = (db.parent(), db.file_stem().and_then(|s| s.to_str())) else {
        return;
    };
    let prefix = format!("{stem}.mod.");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let ours = entry
            .file_name()
            .to_str()
            .is_some_and(|n| n.starts_with(&prefix));
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > max_age);
        if ours && stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn prompt_commands(hooks_json: &serde_json::Value) -> Vec<String> {
    hooks_json["hooks"]["UserPromptSubmit"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
        .filter_map(|h| h["command"].as_str())
        .filter(|c| {
            c.contains("hook prompt") && ["rhebok", "br8n", "brain"].iter().any(|n| c.contains(n))
        })
        .map(str::to_string)
        .collect()
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

pub fn prompt_injectors(claude_home: &Path) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    if let Some(settings) = read_json(&claude_home.join("settings.json")) {
        found.extend(
            prompt_commands(&settings)
                .into_iter()
                .map(|c| format!("settings.json: {c}")),
        );
    }
    let installed = read_json(&claude_home.join("plugins/installed_plugins.json"));
    let plugins = installed
        .as_ref()
        .and_then(|v| v["plugins"].as_object())
        .into_iter()
        .flatten();
    for (name, installs) in plugins {
        for install in installs.as_array().into_iter().flatten() {
            let Some(dir) = install["installPath"].as_str() else {
                continue;
            };
            if let Some(hooks) = read_json(&Path::new(dir).join("hooks/hooks.json")) {
                found.extend(
                    prompt_commands(&hooks)
                        .into_iter()
                        .map(|c| format!("plugin {name}: {c}")),
                );
            }
        }
    }
    found
}

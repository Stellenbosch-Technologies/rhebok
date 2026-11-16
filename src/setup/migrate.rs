use super::agents::{
    self, codex, files, json_config, AgentEnv, ConnectOptions, LEGACY_SERVER_NAME,
};
use super::claude::ClaudeCli;
use super::Paths;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

pub const LEGACY_NAME: &str = "br8n";
pub const LEGACY_PLUGIN_ID: &str = "br8n@br8n";
pub const LEGACY_ENV_PREFIX: &str = "BR8N_";
pub const ENV_PREFIX: &str = "RHEBOK_";

pub struct Crontab {
    #[cfg_attr(not(feature = "backup"), allow(dead_code))]
    program: PathBuf,
}

impl Crontab {
    pub fn system() -> Crontab {
        Crontab::at(Path::new("crontab"))
    }

    pub fn at(program: &Path) -> Crontab {
        Crontab {
            program: program.to_path_buf(),
        }
    }

    #[cfg(feature = "backup")]
    fn read(&self) -> Option<String> {
        let out = std::process::Command::new(&self.program)
            .arg("-l")
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    #[cfg(feature = "backup")]
    fn write(&self, body: &str) -> std::io::Result<std::process::ExitStatus> {
        use std::io::Write;
        let mut child = std::process::Command::new(&self.program)
            .arg("-")
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .expect("stdin is piped")
            .write_all(body.as_bytes())?;
        child.wait()
    }
}

#[derive(Debug, Default)]
pub struct MigrationReport {
    pub lines: Vec<String>,
    pub warnings: Vec<String>,
    pub moved: Vec<(PathBuf, PathBuf)>,
}

impl MigrationReport {
    pub fn migrated(&self) -> bool {
        !self.moved.is_empty()
    }
}

pub fn legacy_sibling(dir: &Path) -> Option<PathBuf> {
    (dir.file_name()? == "rhebok").then(|| dir.with_file_name(LEGACY_NAME))
}

pub fn migrate(
    paths: &Paths,
    config_path: &Path,
    cli: &ClaudeCli,
    env: &AgentEnv,
    crontab: &Crontab,
) -> Result<MigrationReport> {
    let mut r = MigrationReport::default();
    move_legacy_dirs(paths, config_path, &mut r)?;
    let removed_old_binary = remove_stale_binary(paths, &mut r);
    rename_env_keys(config_path, &mut r);
    if r.migrated() {
        rewrite_moved_config(config_path, &mut r);
    }
    unregister_legacy_plugin(paths, cli, &mut r);
    remove_legacy_links(paths, &mut r);
    reconnect_legacy_agents(env, &mut r);
    #[cfg(feature = "backup")]
    {
        if r.migrated() || removed_old_binary {
            rewrite_legacy_cron(paths, crontab, &mut r);
        }
        quote_unquoted_cron(crontab, &mut r);
    }
    #[cfg(not(feature = "backup"))]
    let _ = (crontab, removed_old_binary);
    if r.migrated() {
        r.warnings.push(
            "br8n moved to rhebok: restart Claude Code and Claude Desktop, and stop any `br8n dashboard` still running, so nothing keeps using the old binary"
                .to_string(),
        );
    }
    Ok(r)
}

fn legacy_moves(paths: &Paths, config_path: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut moves: Vec<(PathBuf, PathBuf)> = Vec::new();
    if let Some(old) = legacy_sibling(&paths.root) {
        moves.push((old, paths.root.clone()));
    }
    if let Some(dir) = config_path.parent() {
        if let Some(old) = legacy_sibling(dir) {
            if !moves.iter().any(|(o, _)| *o == old) {
                moves.push((old, dir.to_path_buf()));
            }
        }
    }
    moves
}

fn move_legacy_dirs(paths: &Paths, config_path: &Path, r: &mut MigrationReport) -> Result<()> {
    let moves: Vec<_> = legacy_moves(paths, config_path)
        .into_iter()
        .filter(|(old, _)| old.is_dir())
        .collect();
    for (old, new) in &moves {
        if new.exists() {
            r.warnings.push(format!(
                "both {} and {} exist, so {} was left where it is; move what you need by hand",
                old.display(),
                new.display(),
                old.display()
            ));
            continue;
        }
        for db in [old.join("db"), old.join("memory/db")] {
            if crate::index::IndexLock::is_held(&db) {
                bail!(
                    "a br8n index or memory write is running in {}; wait for it to finish, then run `rhebok install` again",
                    old.display()
                );
            }
        }
    }
    for (old, new) in moves {
        if new.exists() {
            continue;
        }
        if let Some(parent) = new.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        std::fs::rename(&old, &new)
            .with_context(|| format!("move {} to {}", old.display(), new.display()))?;
        r.lines
            .push(format!("moved:    {} -> {}", old.display(), new.display()));
        leave_tombstone(&old, &new, r);
        r.moved.push((old, new));
    }
    Ok(())
}

pub const TOMBSTONE: &str = "br8n is now rhebok. This file stands where the br8n directory was, so that a br8n process still running fails loudly instead of writing to a directory nothing reads.\n";

fn leave_tombstone(old: &Path, new: &Path, r: &mut MigrationReport) {
    let text = format!("{TOMBSTONE}Everything moved to {}\n", new.display());
    if let Err(e) = std::fs::write(old, text) {
        r.warnings.push(format!(
            "could not leave a marker at {}: {e}; a br8n process still running may recreate it",
            old.display()
        ));
    }
}

fn remove_stale_binary(paths: &Paths, r: &mut MigrationReport) -> bool {
    let stale = paths.root.join("bin").join(LEGACY_NAME);
    if stale.symlink_metadata().is_err() {
        return false;
    }
    match std::fs::remove_file(&stale) {
        Ok(()) => {
            r.lines.push(format!("removed:  {}", stale.display()));
            true
        }
        Err(e) => {
            r.warnings
                .push(format!("could not remove {}: {e}", stale.display()));
            false
        }
    }
}

pub fn legacy_env_warnings(vars: impl Iterator<Item = String>) -> Vec<String> {
    let mut legacy: Vec<String> = vars.filter(|k| k.starts_with(LEGACY_ENV_PREFIX)).collect();
    legacy.sort();
    legacy
        .into_iter()
        .map(|k| {
            format!(
                "{k} is set in your environment, and rhebok no longer reads it; rename it to {ENV_PREFIX}{}",
                &k[LEGACY_ENV_PREFIX.len()..]
            )
        })
        .collect()
}

fn rewrite_moved_config(config_path: &Path, r: &mut MigrationReport) {
    let Ok(text) = std::fs::read_to_string(config_path) else {
        return;
    };
    let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf());
    let Some((updated, changes)) = moved_config_text(&text, &r.moved, home.as_deref()) else {
        return;
    };
    let mode = std::fs::metadata(config_path)
        .map(|m| std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o777)
        .ok();
    match crate::config::edit::replace_file(config_path, updated.as_bytes(), mode) {
        Ok(()) => r.lines.extend(
            changes
                .into_iter()
                .map(|c| format!("config:   {c} in {}", config_path.display())),
        ),
        Err(e) => r.warnings.push(format!(
            "could not update {}: {e:#}; {}",
            config_path.display(),
            changes.join("; ")
        )),
    }
}

pub fn moved_config_text(
    text: &str,
    moves: &[(PathBuf, PathBuf)],
    home: Option<&Path>,
) -> Option<(String, Vec<String>)> {
    let mut doc: toml_edit::DocumentMut = text.parse().ok()?;
    let mut changes = Vec::new();
    rewrite_paths_in_table(doc.as_table_mut(), "", moves, home, &mut changes);
    pin_legacy_s3_prefix(&mut doc, &mut changes);
    (!changes.is_empty()).then(|| (doc.to_string(), changes))
}

fn pin_legacy_s3_prefix(doc: &mut toml_edit::DocumentMut, changes: &mut Vec<String>) {
    let Some(s3) = doc
        .get_mut("backup")
        .and_then(|b| b.as_table_like_mut())
        .and_then(|b| b.get_mut("s3"))
        .and_then(|s| s.as_table_like_mut())
    else {
        return;
    };
    if s3.contains_key("prefix") {
        return;
    }
    s3.insert("prefix", toml_edit::value("br8n/"));
    changes.push("set [backup.s3] prefix = \"br8n/\", where br8n kept your backups".to_string());
}

fn rewrite_paths_in_table(
    table: &mut dyn toml_edit::TableLike,
    at: &str,
    moves: &[(PathBuf, PathBuf)],
    home: Option<&Path>,
    changes: &mut Vec<String>,
) {
    for (key, item) in table.iter_mut() {
        let here = if at.is_empty() {
            key.to_string()
        } else {
            format!("{at}.{key}")
        };
        rewrite_paths_in_item(item, &here, moves, home, changes);
    }
}

fn rewrite_paths_in_item(
    item: &mut toml_edit::Item,
    at: &str,
    moves: &[(PathBuf, PathBuf)],
    home: Option<&Path>,
    changes: &mut Vec<String>,
) {
    match item {
        toml_edit::Item::Table(t) => rewrite_paths_in_table(t, at, moves, home, changes),
        toml_edit::Item::ArrayOfTables(a) => {
            for t in a.iter_mut() {
                rewrite_paths_in_table(t, at, moves, home, changes);
            }
        }
        toml_edit::Item::Value(v) => rewrite_paths_in_value(v, at, moves, home, changes),
        toml_edit::Item::None => {}
    }
}

fn rewrite_paths_in_value(
    v: &mut toml_edit::Value,
    at: &str,
    moves: &[(PathBuf, PathBuf)],
    home: Option<&Path>,
    changes: &mut Vec<String>,
) {
    match v {
        toml_edit::Value::String(s) => {
            if let Some(moved) = moved_path_text(s.value(), moves, home) {
                changes.push(format!("{at} now points at {moved}"));
                let decor = s.decor().clone();
                *s = toml_edit::Formatted::new(moved);
                *s.decor_mut() = decor;
            }
        }
        toml_edit::Value::Array(a) => {
            for x in a.iter_mut() {
                rewrite_paths_in_value(x, at, moves, home, changes);
            }
        }
        toml_edit::Value::InlineTable(t) => rewrite_paths_in_table(t, at, moves, home, changes),
        _ => {}
    }
}

pub fn moved_path_text(
    raw: &str,
    moves: &[(PathBuf, PathBuf)],
    home: Option<&Path>,
) -> Option<String> {
    let (tilde, expanded) = match (raw.strip_prefix("~/"), home) {
        (Some(rest), Some(h)) => (true, h.join(rest)),
        _ => (false, PathBuf::from(raw)),
    };
    let (old, new) = moves.iter().find(|(old, _)| expanded.starts_with(old))?;
    let moved = new.join(expanded.strip_prefix(old).ok()?);
    match (tilde, home.and_then(|h| moved.strip_prefix(h).ok())) {
        (true, Some(rest)) => Some(format!("~/{}", rest.display())),
        _ => Some(moved.display().to_string()),
    }
}

pub fn renamed_env_text(text: &str) -> Option<String> {
    let mut changed = false;
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let indent = line.len() - line.trim_start().len();
        let (lead, rest) = line.split_at(indent);
        let (export, rest) = match rest.strip_prefix("export ") {
            Some(after) => ("export ", after),
            None => ("", rest),
        };
        match rest.strip_prefix(LEGACY_ENV_PREFIX) {
            Some(after) => {
                changed = true;
                out.push_str(lead);
                out.push_str(export);
                out.push_str(ENV_PREFIX);
                out.push_str(after);
            }
            None => out.push_str(line),
        }
    }
    changed.then_some(out)
}

fn rename_env_keys(config_path: &Path, r: &mut MigrationReport) {
    let path = crate::env_file::env_path(config_path);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let Some(renamed) = renamed_env_text(&text) else {
        return;
    };
    match crate::config::edit::replace_file(&path, renamed.as_bytes(), Some(0o600)) {
        Ok(()) => r.lines.push(format!(
            "env:      renamed {LEGACY_ENV_PREFIX}* keys to {ENV_PREFIX}* in {}",
            path.display()
        )),
        Err(e) => r.warnings.push(format!(
            "could not rename the {LEGACY_ENV_PREFIX}* keys in {}: {e:#}; rename them to {ENV_PREFIX}* by hand",
            path.display()
        )),
    }
}

fn unregister_legacy_plugin(paths: &Paths, cli: &ClaudeCli, r: &mut MigrationReport) {
    if let Some(cache) = legacy_sibling(&paths.claude_cache) {
        if cache.is_dir() {
            match std::fs::remove_dir_all(&cache) {
                Ok(()) => r.lines.push(format!("removed:  {}", cache.display())),
                Err(e) => r
                    .warnings
                    .push(format!("could not remove {}: {e}", cache.display())),
            }
        }
    }
    if !cli.available() {
        return;
    }
    if let Ok(plugins) = cli.plugins() {
        if plugins.iter().any(|p| p.id == LEGACY_PLUGIN_ID) {
            match cli.plugin_uninstall(LEGACY_PLUGIN_ID) {
                Ok(()) => r
                    .lines
                    .push(format!("plugin:   uninstalled {LEGACY_PLUGIN_ID}")),
                Err(e) => r.warnings.push(format!(
                    "{e:#}; run by hand: claude plugin uninstall {LEGACY_PLUGIN_ID}"
                )),
            }
        }
    }
    let Ok(markets) = cli.marketplaces() else {
        return;
    };
    let Some(m) = markets.iter().find(|m| m.name == LEGACY_NAME) else {
        return;
    };
    let ours = m
        .path
        .as_deref()
        .is_some_and(|p| was_legacy_plugin_dir(p, paths));
    if !ours {
        r.warnings.push(format!(
            "a marketplace named {LEGACY_NAME} points at {}, which br8n did not create; left alone",
            m.path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| m.source.clone())
        ));
        return;
    }
    match cli.marketplace_remove(LEGACY_NAME) {
        Ok(()) => r.lines.push(format!("market:   removed {LEGACY_NAME}")),
        Err(e) => r.warnings.push(format!(
            "{e:#}; run by hand: claude plugin marketplace remove {LEGACY_NAME}"
        )),
    }
}

fn was_legacy_plugin_dir(dir: &Path, paths: &Paths) -> bool {
    let legacy_plugin = legacy_sibling(&paths.root).map(|old| old.join("plugin"));
    legacy_plugin.as_deref() == Some(dir) || dir == paths.plugin
}

fn remove_legacy_links(paths: &Paths, r: &mut MigrationReport) {
    let mut targets = vec![paths.root.join("bin").join(LEGACY_NAME)];
    if let Some(old) = legacy_sibling(&paths.root) {
        targets.push(old.join("bin").join(LEGACY_NAME));
    }
    for dir in &paths.link_dirs {
        let link = dir.join(LEGACY_NAME);
        let Ok(target) = std::fs::read_link(&link) else {
            continue;
        };
        if !targets.contains(&target) {
            continue;
        }
        match std::fs::remove_file(&link) {
            Ok(()) => r.lines.push(format!("removed:  {}", link.display())),
            Err(e) => r
                .warnings
                .push(format!("could not remove {}: {e}", link.display())),
        }
    }
}

struct LegacyAgent {
    instructions: bool,
    files: Vec<PathBuf>,
}

fn reconnect_legacy_agents(env: &AgentEnv, r: &mut MigrationReport) {
    for agent in agents::all() {
        let found = match remove_legacy_entries(agent.id(), env) {
            Ok(found) => found,
            Err(e) => {
                r.warnings.push(format!(
                    "{}: could not remove the old br8n entries: {e:#}",
                    agent.id()
                ));
                continue;
            }
        };
        let Some(found) = found else {
            continue;
        };
        let opts = ConnectOptions {
            instructions: found.instructions,
        };
        if agent.id() == "claude-desktop" {
            r.warnings.push(
                "claude-desktop: Chat Project instructions copied from `br8n project instructions` name the old br8n_context tool; paste the text `rhebok project instructions <name>` prints instead".to_string(),
            );
        }
        match agents::connect(agent.as_ref(), env, &opts) {
            Ok(change) => r.lines.push(format!(
                "agent:    {} moved from br8n to rhebok ({})",
                agent.id(),
                found
                    .files
                    .iter()
                    .chain(&change.files)
                    .map(|f| f.display().to_string())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            Err(e) => r.warnings.push(format!(
                "{}: the old br8n entry was removed, but connecting rhebok failed: {e:#}; run `rhebok connect {}`",
                agent.id(),
                agent.id()
            )),
        }
    }
}

fn remove_legacy_entries(id: &str, env: &AgentEnv) -> Result<Option<LegacyAgent>> {
    match id {
        "codex" => remove_legacy_codex(env),
        "cursor" => remove_legacy_json(&agents::cursor::Cursor::config_file(env)),
        "gemini" => remove_legacy_json(&agents::gemini::Gemini::config_file(env)),
        "claude-desktop" => {
            remove_legacy_json(&agents::claude_desktop::ClaudeDesktop::config_file(env))
        }
        _ => Ok(None),
    }
}

pub fn adopt_legacy_backup(file: &Path) {
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(".br8n-bak");
    let legacy = file.with_file_name(name);
    let current = files::backup_path(file);
    if legacy.is_file() && current.symlink_metadata().is_err() {
        let _ = std::fs::rename(&legacy, &current);
    }
}

fn remove_legacy_json(file: &Path) -> Result<Option<LegacyAgent>> {
    if !file.is_file() {
        return Ok(None);
    }
    let mut loaded = json_config::load(file)?;
    if !json_config::remove_server_named(&mut loaded.doc, LEGACY_SERVER_NAME) {
        return Ok(None);
    }
    adopt_legacy_backup(file);
    let mut change = agents::Change::default();
    json_config::save(file, &loaded, &mut change)?;
    Ok(Some(LegacyAgent {
        instructions: false,
        files: change.files,
    }))
}

fn remove_legacy_codex(env: &AgentEnv) -> Result<Option<LegacyAgent>> {
    for file in [
        codex::Codex::config_file(env),
        codex::Codex::hooks_file(env),
        codex::Codex::instructions_file(env),
    ] {
        adopt_legacy_backup(&file);
    }
    let mut written = Vec::new();
    let config = codex::Codex::config_file(env);
    if config.is_file() {
        let mut toml = codex::load_toml(&config)?;
        if codex::remove_server_named(&mut toml, LEGACY_SERVER_NAME) {
            files::write_atomic(&config, &toml.to_string())?;
            written.push(config);
        }
    }
    let agents_md = codex::Codex::instructions_file(env);
    let mut instructions = false;
    if let Some(text) = files::read_optional(&agents_md)? {
        if let Some(updated) = codex::without_legacy_block(&text) {
            files::write_atomic(&agents_md, &updated)?;
            written.push(agents_md);
            instructions = true;
        }
    }
    Ok((!written.is_empty()).then_some(LegacyAgent {
        instructions,
        files: written,
    }))
}

#[cfg(feature = "backup")]
pub fn migrated_cron_line(
    line: &str,
    paths: &Paths,
    moves: &[(PathBuf, PathBuf)],
) -> Option<String> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let width = if fields.first()?.starts_with('@') {
        1
    } else {
        5
    };
    if fields.len() <= width {
        return None;
    }
    let schedule = fields[..width].join(" ");
    let command: Vec<&str> = fields[width..]
        .iter()
        .copied()
        .filter(|f| !f.starts_with("BR8N_CONFIG="))
        .collect();
    let runs_br8n_backup = command
        .first()
        .is_some_and(|exe| Path::new(exe).file_name().is_some_and(|n| n == LEGACY_NAME))
        && command.get(1) == Some(&"backup");
    if !runs_br8n_backup {
        return None;
    }
    let config = fields
        .iter()
        .find_map(|f| f.strip_prefix("BR8N_CONFIG="))
        .map(|p| moved_path(Path::new(p), moves));
    crate::backup::cron_line(&schedule, &paths.bin, config.as_deref()).ok()
}

#[cfg(feature = "backup")]
fn moved_path(p: &Path, moves: &[(PathBuf, PathBuf)]) -> PathBuf {
    moves
        .iter()
        .find_map(|(old, new)| p.strip_prefix(old).ok().map(|rest| new.join(rest)))
        .unwrap_or_else(|| p.to_path_buf())
}

#[cfg(feature = "backup")]
fn rewrite_legacy_cron(paths: &Paths, crontab: &Crontab, r: &mut MigrationReport) {
    let Some(existing) = crontab.read() else {
        return;
    };
    let Some(legacy) = existing
        .lines()
        .find(|l| l.trim_end().ends_with(crate::backup::LEGACY_CRON_SENTINEL))
    else {
        return;
    };
    let Some(line) = migrated_cron_line(legacy, paths, &r.moved) else {
        r.warnings.push(format!(
            "could not read the br8n backup schedule `{legacy}`; run `rhebok backup schedule` to set it again"
        ));
        return;
    };
    let body = crate::backup::crontab_with(&existing, Some(&line));
    match crontab.write(&body) {
        Ok(status) if status.success() => {
            r.lines.push("backup:   schedule moved to rhebok".to_string())
        }
        Ok(status) => r.warnings.push(format!(
            "`crontab -` exited with {status}; run `rhebok backup schedule` to move the br8n backup schedule"
        )),
        Err(e) => r.warnings.push(format!(
            "could not rewrite the crontab ({e}); run `rhebok backup schedule` to move the br8n backup schedule"
        )),
    }
}

#[cfg(feature = "backup")]
pub fn quoted_cron_line(line: &str) -> Result<Option<String>> {
    let first = line
        .split_whitespace()
        .next()
        .context("the line is empty")?;
    let width = if first.starts_with('@') { 1 } else { 5 };
    let schedule = line
        .split_whitespace()
        .take(width)
        .collect::<Vec<_>>()
        .join(" ");
    let command = after_fields(line, width)
        .context("the line has no command after its schedule")?
        .trim_end();
    if command.starts_with('\'') || command.starts_with("RHEBOK_CONFIG='") {
        return Ok(None);
    }
    let suffix = format!(" backup >/dev/null 2>&1 {}", crate::backup::CRON_SENTINEL);
    let invocation = command
        .strip_suffix(&suffix)
        .context("the command is not `<rhebok> backup` as rhebok writes it")?;
    let (config, exe) = match invocation.strip_prefix("RHEBOK_CONFIG=") {
        Some(both) => {
            let (config, exe) = split_config_and_program(both)?;
            (Some(config), exe)
        }
        None => (None, invocation),
    };
    if !names_rhebok(exe) {
        bail!("`{exe}` is not a path to a rhebok program");
    }
    crate::backup::cron_line(&schedule, Path::new(exe), config.map(Path::new)).map(Some)
}

#[cfg(feature = "backup")]
fn after_fields(line: &str, n: usize) -> Option<&str> {
    let mut rest = line.trim_start();
    for _ in 0..n {
        let end = rest.find(char::is_whitespace)?;
        rest = rest[end..].trim_start();
    }
    (!rest.is_empty()).then_some(rest)
}

#[cfg(feature = "backup")]
fn names_rhebok(exe: &str) -> bool {
    let path = Path::new(exe);
    path.is_absolute() && path.file_name().is_some_and(|n| n == "rhebok")
}

#[cfg(feature = "backup")]
fn split_config_and_program(both: &str) -> Result<(&str, &str)> {
    let splits: Vec<(&str, &str)> = both
        .match_indices(" /")
        .map(|(at, _)| (&both[..at], &both[at + 1..]))
        .filter(|(config, exe)| !config.is_empty() && names_rhebok(exe))
        .collect();
    match splits.as_slice() {
        [one] => Ok(*one),
        [] => bail!("no rhebok program follows `RHEBOK_CONFIG=`"),
        _ => bail!("`{both}` splits into a config path and a program in more than one way"),
    }
}

#[cfg(feature = "backup")]
fn quote_unquoted_cron(crontab: &Crontab, r: &mut MigrationReport) {
    let Some(existing) = crontab.read() else {
        return;
    };
    let mut repaired = 0usize;
    let mut lines: Vec<String> = Vec::new();
    for line in existing.lines() {
        if line.trim_start().starts_with('#')
            || !line.trim_end().ends_with(crate::backup::CRON_SENTINEL)
        {
            lines.push(line.to_string());
            continue;
        }
        match quoted_cron_line(line) {
            Ok(Some(quoted)) => {
                repaired += 1;
                lines.push(quoted);
            }
            Ok(None) => lines.push(line.to_string()),
            Err(e) => {
                r.warnings.push(format!(
                    "could not read the backup schedule `{line}` ({e:#}); run `rhebok backup schedule` to set it again"
                ));
                lines.push(line.to_string());
            }
        }
    }
    if repaired == 0 {
        return;
    }
    let mut body = lines.join("\n");
    body.push('\n');
    match crontab.write(&body) {
        Ok(status) if status.success() => r
            .lines
            .push("backup:   schedule quoted, so a path with a space runs".to_string()),
        Ok(status) => r.warnings.push(format!(
            "`crontab -` exited with {status}; run `rhebok backup schedule` to quote the backup schedule"
        )),
        Err(e) => r.warnings.push(format!(
            "could not rewrite the crontab ({e}); run `rhebok backup schedule` to quote the backup schedule"
        )),
    }
}

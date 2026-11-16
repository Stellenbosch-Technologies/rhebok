use super::json_config::{self, McpJsonAgent};
use super::{
    Agent, AgentEnv, AgentError, Capabilities, Change, ConnectOptions, Detected, Status,
    LEGACY_SERVER_NAME, SERVER_NAME,
};
use anyhow::Result;
use std::path::{Path, PathBuf};

pub struct ClaudeDesktop;

const APP_EXECUTABLE: &str = "/Claude.app/Contents/MacOS/Claude";
const ACCOUNT_CONFIG: &str = "Library/Application Support/Claude/claude_desktop_config.json";

pub const CHAT_INSTRUCTIONS_NOTE: &str = "Chat Project instructions copied from `br8n project instructions` name the old br8n_context tool; paste the text `rhebok project instructions <name>` prints instead";

pub fn rewrites_while_running(file: &Path) -> bool {
    cfg!(target_os = "macos")
        && account_home().is_some_and(|home| is_account_config(file, &home))
        && app_is_running()
}

pub fn is_account_config(file: &Path, account_home: &Path) -> bool {
    same_file(file, &account_home.join(ACCOUNT_CONFIG))
}

pub fn lists_the_app(process_list: &str) -> bool {
    process_list
        .lines()
        .any(|line| line.trim_end().ends_with(APP_EXECUTABLE))
}

fn app_is_running() -> bool {
    let uid = unsafe { libc::getuid() }.to_string();
    std::process::Command::new("/bin/ps")
        .args(["-U", &uid, "-o", "comm="])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .is_ok_and(|out| lists_the_app(&String::from_utf8_lossy(&out.stdout)))
}

fn account_home() -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut buf = vec![0 as libc::c_char; 16 * 1024];
    let mut pwd = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    let rc = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            pwd.as_mut_ptr(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut found,
        )
    };
    if rc != 0 || found.is_null() || unsafe { (*found).pw_dir }.is_null() {
        return None;
    }
    let dir = unsafe { std::ffi::CStr::from_ptr((*found).pw_dir) };
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(dir.to_bytes())))
}

fn same_file(a: &Path, b: &Path) -> bool {
    let resolved = |p: &Path| {
        let dir = p.parent()?.canonicalize().ok()?;
        Some(dir.join(p.file_name()?))
    };
    a == b || matches!((resolved(a), resolved(b)), (Some(x), Some(y)) if x == y)
}

impl ClaudeDesktop {
    pub fn config_dir(env: &AgentEnv) -> PathBuf {
        env.app_config.join("Claude")
    }

    pub fn config_file(env: &AgentEnv) -> PathBuf {
        Self::config_dir(env).join("claude_desktop_config.json")
    }

    pub fn rhebok_status(env: &AgentEnv) -> Status {
        Self::file_agent(&Self::config_file(env)).status_ignoring_br8n(env)
    }

    fn has_br8n_entry(file: &Path) -> bool {
        Self::has_entry(file, LEGACY_SERVER_NAME)
    }

    fn has_entry(file: &Path, name: &str) -> bool {
        json_config::load(file).is_ok_and(|l| json_config::has_server_named(&l.doc, name))
    }

    fn file_agent(file: &Path) -> McpJsonAgent<'_> {
        McpJsonAgent {
            file,
            extra: &[],
            hook: None,
        }
    }
}

impl Agent for ClaudeDesktop {
    fn id(&self) -> &'static str {
        "claude-desktop"
    }

    fn display_name(&self) -> &'static str {
        "Claude Desktop"
    }

    fn detect(&self, env: &AgentEnv) -> Detected {
        let mut dirs = vec![Self::config_dir(env)];
        if cfg!(target_os = "macos") {
            dirs.push(env.applications.join("Claude.app"));
        }
        super::detect_by(env, &[], &dirs, Self::config_file(env))
    }

    fn status(&self, env: &AgentEnv) -> Status {
        Self::file_agent(&Self::config_file(env)).status(env)
    }

    fn connect(&self, env: &AgentEnv, _opts: &ConnectOptions) -> Result<Change> {
        if let Some(reason) = self.blocked_connect(env) {
            return Err(AgentError::Unavailable(format!(
                "{reason}. Quit Claude Desktop (⌘Q), run `rhebok connect claude-desktop` from a terminal, then open Claude Desktop again"
            ))
            .into());
        }
        let file = Self::config_file(env);
        let had_br8n = Self::has_br8n_entry(&file);
        let mut change = Self::file_agent(&file).connect(env)?;
        if had_br8n {
            change.notes.push(CHAT_INSTRUCTIONS_NOTE.to_string());
        }
        if !change.is_empty() {
            change
                .notes
                .push("open Claude Desktop, or restart it, to load it".to_string());
        }
        Ok(change)
    }

    fn disconnect(&self, env: &AgentEnv) -> Result<Change> {
        if let Some(reason) = self.blocked_disconnect(env) {
            return Err(AgentError::Unavailable(format!(
                "{reason}. Quit Claude Desktop (⌘Q), then run `rhebok disconnect claude-desktop` from a terminal, or delete `mcpServers.rhebok` from that file by hand"
            ))
            .into());
        }
        Self::file_agent(&Self::config_file(env)).disconnect()
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            mcp: true,
            ..Capabilities::default()
        }
    }

    fn snippet(&self, env: &AgentEnv) -> Option<String> {
        Some(super::mcp_json_snippet(env))
    }

    fn blocked_connect(&self, env: &AgentEnv) -> Option<String> {
        let reason = self.blocked(env)?;
        Self::file_agent(&Self::config_file(env))
            .connect_would_write(env)
            .unwrap_or(false)
            .then_some(reason)
    }

    fn blocked_disconnect(&self, env: &AgentEnv) -> Option<String> {
        let reason = self.blocked(env)?;
        Self::has_entry(&Self::config_file(env), SERVER_NAME).then_some(reason)
    }

    fn blocked(&self, env: &AgentEnv) -> Option<String> {
        let file = Self::config_file(env);
        (env.desktop_rewrites)(&file).then(|| {
            format!(
                "Claude Desktop is running, and it saves {} from memory, which would undo the change",
                file.display()
            )
        })
    }
}

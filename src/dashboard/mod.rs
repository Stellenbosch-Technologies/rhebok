//! `rhebok dashboard` — a local web view of the index, and the only place
//! that writes a memory or the config over HTTP.
//!
//! Hand-rolled thread-per-connection HTTP, GETs plus a handful of POSTs,
//! bound to 127.0.0.1. No framework: the surface is a few JSON endpoints and
//! a static bundle, and the test stubs already established this exact pattern.
//!
//! Every POST passes `origin_allowed` before it is routed, config writes are
//! serialised on `CONFIG_WRITE`, and the two that
//! write a memory are serialised on `MEMORY_WRITE` — `IndexLock` is a
//! read-then-write pidfile, so two threads entering it together would both
//! pass, and lbug's own file lock does not conflict with a lock the same
//! process already holds.
//!
//! THE rule of this module: the store is opened PER REQUEST and dropped
//! before the response is written. LadybugDB holds an exclusive OS file
//! lock — a held-open store would silence the prompt hook for as long as
//! the dashboard runs.

pub mod core;
mod http;

use crate::config::Config;
use crate::setup::agents::AgentEnv;
use anyhow::Result;
use std::net::TcpListener;

#[derive(Clone)]
pub enum ConfigSource {
    Fixed(Box<Config>),
    File(std::path::PathBuf),
}

impl ConfigSource {
    fn path(&self) -> std::path::PathBuf {
        match self {
            ConfigSource::Fixed(_) => Config::config_path(),
            ConfigSource::File(path) => path.clone(),
        }
    }

    fn current(&self) -> Config {
        match self {
            ConfigSource::Fixed(cfg) => cfg.as_ref().clone(),
            ConfigSource::File(path) => Config::load_from(path),
        }
    }
}

impl From<Config> for ConfigSource {
    fn from(cfg: Config) -> ConfigSource {
        ConfigSource::Fixed(Box::new(cfg))
    }
}

pub fn serve(port: Option<u16>, open_browser: bool) -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port.unwrap_or(0)))?;
    let addr = listener.local_addr()?;
    println!("rhebok dashboard: http://{addr}");
    if let Err(e) = crate::restart::announce_dashboard(&Config::db_path(), addr.port()) {
        eprintln!("rhebok dashboard: rhebok restart will not find this dashboard: {e:#}");
    }
    if open_browser {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let _ = std::process::Command::new(opener)
            .arg(format!("http://{addr}"))
            .spawn();
    }
    serve_on(ConfigSource::File(Config::config_path()), listener)
}

/// Test seam: serve on an already-bound listener, forever.
pub fn serve_on(source: impl Into<ConfigSource>, listener: TcpListener) -> Result<()> {
    serve_on_with_agents(source, listener, None)
}

pub fn serve_on_with_agents(
    source: impl Into<ConfigSource>,
    listener: TcpListener,
    agents: Option<AgentEnv>,
) -> Result<()> {
    let source = source.into();
    let agents = agents.map(std::sync::Arc::new);
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let source = source.clone();
        let agents = agents.clone();
        std::thread::spawn(move || http::handle(stream, &source, agents.as_deref()));
    }
    Ok(())
}

use super::files::Unparseable;
use super::{Agent, AgentEnv, AgentError, Change, ConnectOptions};
use serde_json::{json, Value};

pub fn agent_json(agent: &dyn Agent, env: &AgentEnv) -> Value {
    let status = agent.status(env);
    let mut v = json!({
        "id": agent.id(),
        "name": agent.display_name(),
        "detected": agent.detect(env),
        "status": { "state": status.state() },
        "capabilities": agent.capabilities(),
        "snippet": agent.snippet(env),
    });
    if let Some(reason) = status.reason() {
        v["status"]["reason"] = json!(reason);
    }
    if let Some(on) = agent.instructions(env) {
        v["instructions"] = json!(on);
    }
    v
}

pub fn snippets(env: &AgentEnv) -> Value {
    json!({
        "mcp_json": super::mcp_json_snippet(env),
        "codex_toml": super::codex_toml_snippet(env),
    })
}

pub fn list(env: &AgentEnv) -> Value {
    let agents = super::all();
    let rows: Vec<Value> = std::thread::scope(|s| {
        let handles: Vec<_> = agents
            .iter()
            .map(|a| s.spawn(move || agent_json(a.as_ref(), env)))
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| json!({ "error": "agent probe panicked" }))
            })
            .collect()
    });
    json!({ "agents": rows, "snippets": snippets(env) })
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AgentRequest {
    pub id: String,
    #[serde(default)]
    pub instructions: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRefusal {
    UnknownAgent,
    BadOption,
    Unavailable,
    Unparseable,
    Failed,
}

impl AgentRefusal {
    fn of(e: &anyhow::Error) -> Self {
        if e.downcast_ref::<Unparseable>().is_some() {
            return AgentRefusal::Unparseable;
        }
        match e.downcast_ref::<AgentError>() {
            Some(AgentError::Unknown(_)) => AgentRefusal::UnknownAgent,
            Some(AgentError::BadOption(_)) => AgentRefusal::BadOption,
            Some(AgentError::Unavailable(_)) => AgentRefusal::Unavailable,
            None => AgentRefusal::Failed,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentFailure {
    pub kind: AgentRefusal,
    pub message: String,
}

impl From<anyhow::Error> for AgentFailure {
    fn from(e: anyhow::Error) -> Self {
        AgentFailure {
            kind: AgentRefusal::of(&e),
            message: format!("{e:#}"),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentChanged {
    pub agent: Value,
    pub change: Change,
}

fn outcome(
    env: &AgentEnv,
    id: &str,
    act: impl FnOnce(&dyn Agent) -> anyhow::Result<Change>,
) -> Result<AgentChanged, AgentFailure> {
    let agent = super::find(id)?;
    let change = act(agent.as_ref())?;
    Ok(AgentChanged {
        agent: agent_json(agent.as_ref(), env),
        change,
    })
}

pub(crate) fn connect_agent(
    env: &AgentEnv,
    req: &AgentRequest,
) -> Result<AgentChanged, AgentFailure> {
    let opts = ConnectOptions {
        instructions: req.instructions,
    };
    outcome(env, &req.id, |a| super::connect(a, env, &opts))
}

pub(crate) fn disconnect_agent(
    env: &AgentEnv,
    req: &AgentRequest,
) -> Result<AgentChanged, AgentFailure> {
    outcome(env, &req.id, |a| a.disconnect(env))
}

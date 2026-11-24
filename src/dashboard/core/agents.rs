use super::DashboardError;
use crate::setup::agents::api::{self, AgentFailure};
use crate::setup::agents::AgentEnv;

pub use crate::setup::agents::api::{AgentChanged, AgentRequest};

static AGENT_WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, serde::Serialize)]
#[serde(transparent)]
pub struct AgentList(pub serde_json::Value);

pub fn list(env: &AgentEnv) -> AgentList {
    AgentList(api::list(env))
}

pub fn connect(env: &AgentEnv, req: AgentRequest) -> Result<AgentChanged, DashboardError> {
    write(&req, |req| api::connect_agent(env, req))
}

pub fn disconnect(env: &AgentEnv, req: AgentRequest) -> Result<AgentChanged, DashboardError> {
    write(&req, |req| api::disconnect_agent(env, req))
}

fn write(
    req: &AgentRequest,
    act: impl FnOnce(&AgentRequest) -> Result<AgentChanged, AgentFailure>,
) -> Result<AgentChanged, DashboardError> {
    if req.id.trim().is_empty() {
        return Err(DashboardError::BadRequest("no agent id given".to_string()));
    }
    let _writing = AGENT_WRITE.lock().unwrap_or_else(|e| e.into_inner());
    act(req).map_err(|f| DashboardError::AgentRefused {
        kind: f.kind,
        message: f.message,
    })
}

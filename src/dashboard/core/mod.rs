pub mod agents;
pub mod config;
pub mod document;
pub mod embed;
pub mod graph;
pub mod ignore;
pub mod index;
pub mod install;
pub mod memories;
pub mod memory_write;
pub mod search;
pub mod stats;
pub mod update;
pub mod version;

pub use crate::setup::agents::api::AgentRefusal;

#[derive(Debug)]
pub enum DashboardError {
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    ConfigChanged { message: String, etag: String },
    Unprocessable(Vec<crate::config::check::FieldError>),
    AgentRefused { kind: AgentRefusal, message: String },
    Unavailable(String),
    VectorExtensionMissing(String),
    CliMissing { fix: String },
    Internal(String),
}

impl std::fmt::Display for DashboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DashboardError::BadRequest(m)
            | DashboardError::NotFound(m)
            | DashboardError::Conflict(m)
            | DashboardError::ConfigChanged { message: m, .. }
            | DashboardError::AgentRefused { message: m, .. }
            | DashboardError::Unavailable(m)
            | DashboardError::VectorExtensionMissing(m)
            | DashboardError::Internal(m) => f.write_str(m),
            DashboardError::Unprocessable(errors) => {
                let summary = errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                write!(f, "not saved: {summary}")
            }
            DashboardError::CliMissing { .. } => f.write_str("the rhebok CLI was not found"),
        }
    }
}

impl std::error::Error for DashboardError {}

impl From<anyhow::Error> for DashboardError {
    fn from(e: anyhow::Error) -> Self {
        match e.downcast_ref::<crate::store::VectorExtensionMissing>() {
            Some(missing) => DashboardError::VectorExtensionMissing(missing.to_string()),
            None => DashboardError::Unavailable(e.to_string()),
        }
    }
}

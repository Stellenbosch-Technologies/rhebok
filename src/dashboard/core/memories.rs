use super::DashboardError;
use crate::config::Config;

#[derive(Debug, Clone, serde::Serialize)]
pub struct MemoryView {
    #[serde(flatten)]
    pub memory: crate::memory::Memory,
    pub project_name: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Memories {
    pub memories: Vec<MemoryView>,
    pub unavailable: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReachedDocument {
    pub doc_id: String,
    pub title: String,
    pub uri: String,
    pub relevance: f32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MemoryReach {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reach: Option<Vec<ReachedDocument>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl MemoryReach {
    fn failed(message: impl Into<String>) -> Self {
        Self {
            reach: None,
            error: Some(message.into()),
        }
    }
}

fn readable_memories(cfg: &Config) -> Result<Vec<crate::memory::Memory>, String> {
    if !cfg.memory.enabled {
        return Err(crate::memory::DISABLED.to_string());
    }
    crate::memory::list_at(
        &crate::memory::default_root(),
        &crate::memory::Filter::default(),
    )
    .map_err(|e| format!("{e:#}"))
}

fn with_project_names(cfg: &Config, list: Vec<crate::memory::Memory>) -> Vec<MemoryView> {
    list.into_iter()
        .map(|memory| {
            let project_name = memory.facts.project.as_deref().and_then(|p| {
                crate::project::resolve(cfg, std::path::Path::new(p))
                    .project
                    .map(|project| project.name)
            });
            MemoryView {
                memory,
                project_name,
            }
        })
        .collect()
}

pub fn list(cfg: &Config) -> Memories {
    match readable_memories(cfg) {
        Ok(list) => Memories {
            memories: with_project_names(cfg, list),
            unavailable: None,
        },
        Err(reason) => Memories {
            memories: Vec::new(),
            unavailable: Some(reason),
        },
    }
}

pub fn reach(cfg: &Config, id: &str) -> Result<MemoryReach, DashboardError> {
    if id.trim().is_empty() {
        return Ok(MemoryReach::failed("no id given"));
    }
    let all = match readable_memories(cfg) {
        Ok(all) => all,
        Err(reason) => return Ok(MemoryReach::failed(reason)),
    };
    let Some(m) = all.iter().find(|m| m.id == id) else {
        return Ok(MemoryReach::failed(format!("no memory with id `{id}`")));
    };
    let surface = crate::config::Surface::Hook;
    let profile = cfg.profile_for(surface);
    let threshold = cfg.surface(surface).threshold;
    let _priority = crate::index::QueryPriority::announce(&Config::db_path());
    let retriever = crate::retrieve_for_profile(cfg, surface, &profile)?;
    match retriever.search_gated(&m.text, &profile, threshold) {
        Ok(hits) => Ok(MemoryReach {
            reach: Some(
                hits.iter()
                    .filter(|h| h.source_type != "memory")
                    .map(|h| ReachedDocument {
                        doc_id: h.doc_id.clone(),
                        title: h.title.clone(),
                        uri: h.uri.clone(),
                        relevance: h.relevance,
                    })
                    .collect(),
            ),
            error: None,
        }),
        Err(e) => super::search::failure_message(e)
            .map(MemoryReach::failed)
            .map_err(DashboardError::from),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disabled_memory_store_is_named_rather_than_read_back_as_empty() {
        let mut cfg = crate::config::Config::default();
        cfg.memory.enabled = false;
        let v = list(&cfg);
        assert!(
            v.memories.is_empty(),
            "a disabled store lists nothing: {v:?}"
        );
        assert_eq!(
            v.unavailable.as_deref(),
            Some(crate::memory::DISABLED),
            "disabled must not look like an empty store, which invites a create the server refuses: {v:?}"
        );
    }

    #[test]
    fn a_disabled_memory_store_refuses_a_reach_by_name() {
        let mut cfg = crate::config::Config::default();
        cfg.memory.enabled = false;
        let v = reach(&cfg, "abc").unwrap();
        assert_eq!(
            v.error.as_deref(),
            Some(crate::memory::DISABLED),
            "a disabled store must refuse a reach the way it refuses a listing: {v:?}"
        );
    }
}

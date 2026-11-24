use super::DashboardError;
use crate::config::Config;
use crate::store::query::{GraphEdge, GraphEntity, GraphNode};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Node {
    pub id: String,
    pub title: String,
    pub source_type: String,
    pub chunks: u32,
    pub inbound: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<String>,
}

impl From<GraphNode> for Node {
    fn from(n: GraphNode) -> Node {
        Node {
            id: n.id,
            title: n.title,
            source_type: n.source_type,
            chunks: n.chunks,
            inbound: n.inbound,
            agent: n.agent,
            memory_kind: None,
            memory_id: None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub entities: Vec<GraphEntity>,
    pub tags: Vec<String>,
    pub edges: Vec<GraphEdge>,
}

pub fn graph(cfg: &Config) -> Result<Graph, DashboardError> {
    let db = Config::db_path();
    let mut graph = if super::stats::no_index_yet(&db) {
        Graph {
            nodes: Vec::new(),
            entities: Vec::new(),
            tags: Vec::new(),
            edges: Vec::new(),
        }
    } else {
        let snap = {
            let store = crate::store::Store::open_existing(&db, cfg.embed.dimensions)?;
            store.graph_snapshot()?
        };
        Graph {
            nodes: snap.nodes.into_iter().map(Node::from).collect(),
            entities: snap.entities,
            tags: snap.tags,
            edges: snap.edges,
        }
    };
    if cfg.memory.enabled {
        fold_in_memories(&mut graph);
    }
    Ok(graph)
}

fn fold_in_memories(graph: &mut Graph) {
    let root = crate::memory::default_root();
    let Ok(mems) = crate::memory::list_at(&root, &crate::memory::Filter::default()) else {
        return;
    };
    let mut projects: std::collections::BTreeSet<String> = Default::default();
    for m in &mems {
        graph.nodes.push(Node {
            id: m.doc_id.clone(),
            title: m.title.clone(),
            source_type: "memory".to_string(),
            chunks: 1,
            inbound: 0,
            agent: None,
            memory_kind: Some(m.facts.kind.as_str()),
            memory_id: Some(m.id.clone()),
        });
        if let Some(s) = &m.facts.session {
            graph.edges.push(GraphEdge {
                from: m.doc_id.clone(),
                to: crate::model::Document::new_id(s),
                kind: "distilled-from".to_string(),
            });
        }
        if let Some(p) = &m.facts.project {
            projects.insert(p.clone());
            graph.edges.push(GraphEdge {
                from: m.doc_id.clone(),
                to: format!("project:{p}"),
                kind: "scoped-to".to_string(),
            });
        }
    }
    for p in projects {
        graph.nodes.push(Node {
            id: format!("project:{p}"),
            title: p,
            source_type: "project".to_string(),
            chunks: 0,
            inbound: 0,
            agent: None,
            memory_kind: None,
            memory_id: None,
        });
    }
}

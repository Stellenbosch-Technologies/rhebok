use super::DashboardError;
use crate::config::Config;
use crate::store::query::DocumentDetail;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(untagged)]
pub enum Document {
    Found(DocumentDetail),
    Missing { error: String },
}

pub fn document(cfg: &Config, id: &str) -> Result<Document, DashboardError> {
    if id.trim().is_empty() {
        return Ok(Document::Missing {
            error: "missing id".to_string(),
        });
    }
    let detail = {
        let store = crate::store::Store::open_existing(&Config::db_path(), cfg.embed.dimensions)?;
        store.document_detail(id)?
    };
    Ok(match detail {
        Some(d) => Document::Found(d),
        None => Document::Missing {
            error: "unknown document".to_string(),
        },
    })
}

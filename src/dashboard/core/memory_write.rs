use super::DashboardError;
use crate::config::Config;

static MEMORY_WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SaveRequest {
    pub id: Option<String>,
    pub kind: String,
    pub text: String,
    pub scope: String,
    pub confidence: Option<u8>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct DeleteRequest {
    pub id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "outcome", rename_all = "lowercase")]
pub enum SaveOutcome {
    Saved { id: String },
    Replaced { id: String, previous_title: String },
    Duplicate { id: String },
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeleteOutcome {
    pub id: String,
    pub title: String,
}

fn memory_busy() -> Option<DashboardError> {
    let db = crate::memory::store_dir(&crate::memory::default_root());
    crate::index::IndexLock::is_held(&db)
        .then(|| DashboardError::Conflict(crate::memory::MemoryBusy.to_string()))
}

fn memory_failure(e: anyhow::Error) -> DashboardError {
    if e.downcast_ref::<crate::memory::MemoryNotFound>().is_some()
        || e.downcast_ref::<crate::memory::MemoryAmbiguous>().is_some()
    {
        return DashboardError::NotFound(format!("{e}"));
    }
    if e.downcast_ref::<crate::memory::MemoryBusy>().is_some() {
        return DashboardError::Conflict(format!("{e}"));
    }
    DashboardError::Unavailable(format!("{e:#}"))
}

pub fn save(cfg: &Config, req: SaveRequest) -> Result<SaveOutcome, DashboardError> {
    let _writing = MEMORY_WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let _priority = crate::index::QueryPriority::announce(&Config::db_path());
    let Some(kind) = crate::memory::MemoryKind::parse(&req.kind) else {
        return Err(DashboardError::BadRequest(format!(
            "kind `{}` is not lesson, fact or episode",
            req.kind
        )));
    };
    if let Some(busy) = memory_busy() {
        return Err(busy);
    }
    if req.confidence.is_some_and(|c| c > 100) {
        return Err(DashboardError::BadRequest(
            "confidence is a percent, so it must be 0 to 100".to_string(),
        ));
    }
    let project = match req.scope.trim() {
        "global" | "" => None,
        p => Some(std::path::PathBuf::from(p)),
    };
    let remember = crate::memory::Remember {
        kind,
        text: req.text,
        title: None,
        project,
        confidence: req.confidence.unwrap_or(100),
        origin: crate::memory::Origin::User,
        session: None,
        source_hash: None,
        source_stamp: None,
        created: None,
    };
    let done = match &req.id {
        Some(id) if id.trim().is_empty() => {
            return Err(DashboardError::BadRequest(
                "an edit must name the id it is editing".to_string(),
            ));
        }
        Some(id) => crate::memory::edit(cfg, id, remember),
        None => crate::memory::remember(cfg, remember),
    };
    match done.map_err(memory_failure)? {
        crate::memory::Outcome::Rejected(why) => Err(DashboardError::BadRequest(why)),
        crate::memory::Outcome::Saved { id } => Ok(SaveOutcome::Saved { id }),
        crate::memory::Outcome::Replaced {
            id, previous_title, ..
        } => Ok(SaveOutcome::Replaced { id, previous_title }),
        crate::memory::Outcome::Duplicate { of } => Ok(SaveOutcome::Duplicate { id: of }),
    }
}

pub fn delete(cfg: &Config, req: DeleteRequest) -> Result<DeleteOutcome, DashboardError> {
    let _writing = MEMORY_WRITE.lock().unwrap_or_else(|e| e.into_inner());
    if req.id.trim().is_empty() {
        return Err(DashboardError::BadRequest(
            "a delete must name the id it is deleting".to_string(),
        ));
    }
    if let Some(busy) = memory_busy() {
        return Err(busy);
    }
    let forgotten = crate::memory::forget(cfg, &req.id).map_err(memory_failure)?;
    Ok(DeleteOutcome {
        id: forgotten.id,
        title: forgotten.title,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_failure_is_classified_by_its_type_not_by_its_words() {
        use crate::memory::{MemoryAmbiguous, MemoryBusy, MemoryNotFound};
        let conflict = |e: anyhow::Error| matches!(memory_failure(e), DashboardError::Conflict(_));
        let not_found = |e: anyhow::Error| matches!(memory_failure(e), DashboardError::NotFound(_));
        let unavailable =
            |e: anyhow::Error| matches!(memory_failure(e), DashboardError::Unavailable(_));
        assert!(conflict(MemoryBusy.into()));
        assert!(
            conflict(anyhow::Error::from(MemoryBusy).context("saving a memory")),
            "a busy store is still a conflict once a caller has added context"
        );
        assert!(not_found(MemoryNotFound("abc".into()).into()));
        assert!(not_found(
            MemoryAmbiguous {
                id: "a".into(),
                matches: 2
            }
            .into()
        ));
        assert!(
            unavailable(anyhow::anyhow!("{MemoryBusy}")),
            "an error that only says it is busy is not a busy store"
        );
        assert!(unavailable(anyhow::anyhow!("the id matches 2 memories")));
    }
}

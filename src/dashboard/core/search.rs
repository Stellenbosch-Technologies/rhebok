use super::DashboardError;
use crate::config::Config;

#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    pub q: String,
    pub tier: Option<u8>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchOutcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(flatten)]
    pub explain: Option<crate::retrieve::Explain>,
}

impl SearchOutcome {
    fn failed(message: impl Into<String>) -> Self {
        Self {
            error: Some(message.into()),
            explain: None,
        }
    }
}

pub fn search(cfg: &Config, q: &SearchQuery) -> Result<SearchOutcome, DashboardError> {
    if q.q.trim().is_empty() {
        return Ok(SearchOutcome::failed("empty query"));
    }
    let surface = crate::config::Surface::Hook;
    let profile = q
        .tier
        .map(|t| cfg.tier(t))
        .unwrap_or_else(|| cfg.profile_for(surface));
    let threshold = cfg.surface(surface).threshold;
    let max_tokens = cfg.surface(surface).max_tokens;
    let _priority = crate::index::QueryPriority::announce(&Config::db_path());
    let retriever = match crate::retrieve_for_profile(cfg, surface, &profile) {
        Ok(r) => r,
        Err(e) => return refusal_or_outage(e),
    };
    match retriever.search_explained(&q.q, &profile, threshold, max_tokens) {
        Ok(explain) => Ok(SearchOutcome {
            error: None,
            explain: Some(explain),
        }),
        Err(e) => refusal_or_outage(e),
    }
}

fn refusal_or_outage(e: anyhow::Error) -> Result<SearchOutcome, DashboardError> {
    failure_message(e)
        .map(SearchOutcome::failed)
        .map_err(DashboardError::from)
}

pub(in crate::dashboard) fn failure_message(e: anyhow::Error) -> Result<String, anyhow::Error> {
    if e.downcast_ref::<crate::retrieve::EmbedUnavailable>()
        .is_some()
    {
        return Ok("embedding unavailable".to_string());
    }
    if let Some(refused) = e.downcast_ref::<crate::retrieve::PackRefused>() {
        return Ok(refused.to_string());
    }
    if let Some(missing) = e.downcast_ref::<crate::store::VectorExtensionMissing>() {
        return Ok(missing.to_string());
    }
    Err(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_embedding_failure_gets_the_banner_a_store_failure_gets_a_503() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path(), 4).unwrap();
        let store_err = store
            .exec("MATCH (x:NoSuchLabel) RETURN x.nope", vec![])
            .expect_err("querying a label that does not exist must fail");

        let out = refusal_or_outage(store_err);
        let err = out.expect_err("a store failure must propagate to the 503 path");
        assert!(matches!(err, DashboardError::Unavailable(_)), "{err:?}");
        assert!(
            !format!("{err:?}").contains("embedding"),
            "a store failure must not be described as an embedding one: {err:?}"
        );

        let embed_err = crate::retrieve::EmbedUnavailable(anyhow::anyhow!(
            "error sending request for url (http://127.0.0.1:11434/api/embed)"
        ));
        let out = refusal_or_outage(anyhow::Error::new(embed_err))
            .expect("an embedding outage is a 200 with a payload, not an HTTP error");
        assert_eq!(out.error.as_deref(), Some("embedding unavailable"));

        let refused = crate::retrieve::PackRefused(anyhow::anyhow!(
            "this index predates the retrieval pack and can no longer be searched \
             directly — run `rhebok index --compact` to rebuild the pack from the rows \
             you already have (no re-embedding), or `rhebok index --reindex` to rebuild \
             from source"
        ));
        let message = refused.to_string();
        let out = refusal_or_outage(anyhow::Error::new(refused)).expect(
            "a pack refusal is a stable state, not a hiccup — it must be a 200 with a payload, \
             not the 503 a client retries and reports as busy",
        );
        assert_eq!(
            out.error.as_deref(),
            Some(message.as_str()),
            "the dashboard must show the refusal's OWN message, not a generic banner"
        );
    }
}

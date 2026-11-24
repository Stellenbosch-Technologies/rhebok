use super::DashboardError;
use crate::config::Config;

#[derive(Debug, Clone, serde::Serialize)]
pub struct IgnorePreview {
    pub error: Option<String>,
    pub matches: usize,
    pub of: usize,
    pub sample: Vec<String>,
}

pub fn preview(cfg: &Config, pattern: &str) -> Result<IgnorePreview, DashboardError> {
    let entry = match crate::ignore_list::Entry::parse(pattern) {
        Ok(entry) => entry,
        Err(why) => {
            return Ok(IgnorePreview {
                error: Some(why),
                matches: 0,
                of: 0,
                sample: Vec::new(),
            })
        }
    };
    let db = Config::db_path();
    let skipped = if super::stats::no_index_yet(&db) {
        Vec::new()
    } else {
        crate::store::Store::open_existing(&db, cfg.embed.dimensions)?
            .status_snapshot()
            .skipped
    };
    let (shown, _) = crate::skip::split_ignored(skipped, &cfg.ignore);
    let matched: Vec<&str> = shown
        .iter()
        .filter(|s| entry.matches(std::path::Path::new(&s.path)))
        .map(|s| s.path.as_str())
        .collect();
    Ok(IgnorePreview {
        error: None,
        matches: matched.len(),
        of: shown.len(),
        sample: matched.iter().take(5).map(|s| s.to_string()).collect(),
    })
}

use super::DashboardError;
use crate::config::check::{check as check_text, FieldError};
use crate::config::edit::{new_errors, patched_text, read_document, update, Outcome, Patch};
use crate::env_file::{self, MODEL_VAR, TOKEN_VAR, URL_VAR};
use std::path::Path;

static CONFIG_WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, serde::Serialize)]
#[serde(transparent)]
pub struct ConfigView(pub serde_json::Value);

pub type ConfigSaved = ConfigView;
pub type EmbedEndpointSaved = ConfigView;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ConfigSave {
    pub etag: String,
    #[serde(default)]
    pub set: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub unset: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ConfigCheck {
    #[serde(default)]
    pub set: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    pub unset: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConfigChecked {
    pub errors: Vec<FieldError>,
    pub blocking: Vec<FieldError>,
}

fn present<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<Option<String>>, D::Error> {
    <Option<String> as serde::Deserialize>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct EmbedEndpoint {
    #[serde(default, deserialize_with = "present")]
    pub url: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    pub model: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    pub token: Option<Option<String>>,
}

fn internal(e: anyhow::Error) -> DashboardError {
    DashboardError::Internal(format!("{e:#}"))
}

pub fn view(path: &Path) -> Result<ConfigView, DashboardError> {
    Ok(ConfigView(crate::config::view::view(path)?))
}

fn view_after_write(path: &Path) -> Result<ConfigView, DashboardError> {
    crate::config::view::view(path)
        .map(ConfigView)
        .map_err(internal)
}

const FILE_ONLY: &str = "pdf.ocr_command";

fn ocr_command_in(text: Option<&str>) -> Option<toml::Value> {
    text?
        .parse::<toml::Table>()
        .ok()?
        .get("pdf")?
        .get("ocr_command")
        .cloned()
}

fn file_only_change(current: Option<&str>, patch: &Patch) -> Option<FieldError> {
    let next = patched_text(current, patch).ok()?;
    (ocr_command_in(current) != ocr_command_in(Some(&next))).then(|| {
        FieldError::new(
            FILE_ONLY,
            "runs a program on each scanned page, so it can be changed only in config.toml \
             or with `rhebok config set`, not from the dashboard",
        )
    })
}

pub fn save(path: &Path, req: ConfigSave) -> Result<ConfigSaved, DashboardError> {
    let _writing = CONFIG_WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let patch = Patch::from_json(&req.set, &req.unset).map_err(DashboardError::Unprocessable)?;
    let current = read_document(path).map_err(internal)?;
    if let Some(refused) = file_only_change(current.as_deref(), &patch) {
        return Err(DashboardError::Unprocessable(vec![refused]));
    }
    match update(path, Some(&req.etag), &patch).map_err(internal)? {
        Outcome::Written { .. } => view_after_write(path),
        Outcome::Conflict { etag } => Err(DashboardError::ConfigChanged {
            message:
                "the config file changed since it was read; reload it and make the change again"
                    .to_string(),
            etag,
        }),
        Outcome::Invalid(errors) => Err(DashboardError::Unprocessable(errors)),
    }
}

pub fn check(path: &Path, req: ConfigCheck) -> Result<ConfigChecked, DashboardError> {
    let patch = match Patch::from_json(&req.set, &req.unset) {
        Ok(patch) => patch,
        Err(errors) => {
            return Ok(ConfigChecked {
                blocking: errors.clone(),
                errors,
            })
        }
    };
    let current = read_document(path).map_err(internal)?;
    if let Some(refused) = file_only_change(current.as_deref(), &patch) {
        return Ok(ConfigChecked {
            blocking: vec![refused.clone()],
            errors: vec![refused],
        });
    }
    let (errors, blocking) = match patched_text(current.as_deref(), &patch) {
        Ok(next) => {
            let errors = check_text(&next);
            let before = current.as_deref().map(check_text).unwrap_or_default();
            let blocking = new_errors(&before, errors.clone());
            (errors, blocking)
        }
        Err(errors) => (errors.clone(), errors),
    };
    Ok(ConfigChecked { errors, blocking })
}

fn endpoint_setting(var: &str) -> &'static str {
    match var {
        URL_VAR => "endpoint.url",
        MODEL_VAR => "endpoint.model",
        _ => "embed.token",
    }
}

pub fn set_embed_endpoint(
    path: &Path,
    req: EmbedEndpoint,
) -> Result<EmbedEndpointSaved, DashboardError> {
    let _writing = CONFIG_WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let changes: Vec<(&str, Option<String>)> = [
        (URL_VAR, req.url),
        (MODEL_VAR, req.model),
        (TOKEN_VAR, req.token),
    ]
    .into_iter()
    .filter_map(|(var, field)| {
        field.map(|value| {
            let value = value
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
            (var, value)
        })
    })
    .collect();
    if changes.is_empty() {
        return view_after_write(path);
    }
    let env_path = env_file::env_path(path);
    let mut merged = env_file::read_lenient(&env_path);
    for (var, value) in &changes {
        match value {
            Some(v) => merged.insert(var.to_string(), v.clone()),
            None => merged.remove(*var),
        };
    }
    let effective = |var: &str| {
        std::env::var(var)
            .ok()
            .filter(|v| !v.is_empty())
            .or_else(|| merged.get(var).cloned())
    };
    let mut errors = Vec::new();
    for (var, value) in &changes {
        if value.as_deref().is_some_and(|v| v.contains(['\n', '\r'])) {
            errors.push(FieldError::new(
                endpoint_setting(var),
                "must be a single line",
            ));
        }
    }
    if let Some(url) = effective(URL_VAR) {
        if let Some(why) = crate::config::check::endpoint_url_problem(&url) {
            errors.push(FieldError::new("endpoint.url", why));
        }
        for var in [MODEL_VAR, TOKEN_VAR] {
            if effective(var).is_none() {
                errors.push(FieldError::new(
                    endpoint_setting(var),
                    "is required while an endpoint URL is set",
                ));
            }
        }
    }
    if !errors.is_empty() {
        return Err(DashboardError::Unprocessable(errors));
    }
    env_file::write_entries(&env_path, &changes).map_err(internal)?;
    view_after_write(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::edit::etag_of;

    fn write(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, text).unwrap();
        (dir, path)
    }

    fn request(path: &Path, set: serde_json::Value, unset: &[&str]) -> ConfigSave {
        let text = std::fs::read_to_string(path).ok();
        ConfigSave {
            etag: etag_of(text.as_deref()),
            set: set.as_object().cloned().unwrap_or_default(),
            unset: unset.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn refused(result: Result<ConfigSaved, DashboardError>) -> bool {
        matches!(result, Err(DashboardError::Unprocessable(ref errors)) if errors.iter().any(|e| e.path == FILE_ONLY))
    }

    #[test]
    fn the_dashboard_cannot_set_change_or_remove_the_ocr_command() {
        let original = "[pdf]\nocr_command = [\"tesseract\", \"{image}\", \"-\"]\n";
        let (_dir, path) = write(original);
        for (set, unset) in [
            (
                serde_json::json!({ "pdf.ocr_command": ["sh", "-c", "curl evil"] }),
                vec![],
            ),
            (
                serde_json::json!({ "pdf": { "ocr_command": ["sh"] } }),
                vec![],
            ),
            (serde_json::json!({}), vec!["pdf.ocr_command"]),
            (serde_json::json!({}), vec!["pdf"]),
        ] {
            let req = request(&path, set.clone(), &unset);
            assert!(refused(save(&path, req)), "{set} {unset:?}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        }
        let (_empty_dir, empty) = write("");
        let req = request(
            &empty,
            serde_json::json!({ "pdf.ocr_command": ["sh"] }),
            &[],
        );
        assert!(refused(save(&empty, req)));
    }

    #[test]
    fn other_settings_still_save_beside_an_ocr_command() {
        let (_dir, path) = write("[pdf]\nocr_command = [\"tesseract\", \"{image}\", \"-\"]\n");
        let req = request(
            &path,
            serde_json::json!({ "pdf.ocr_engine": "http", "pdf.ocr_url": "http://ocr.local/read" }),
            &[],
        );
        assert!(save(&path, req).is_ok());
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(
            saved.contains("ocr_command = [\"tesseract\", \"{image}\", \"-\"]"),
            "{saved}"
        );
        assert!(
            saved.contains("ocr_url = \"http://ocr.local/read\""),
            "{saved}"
        );
    }

    #[test]
    fn the_settings_check_reports_the_refusal_as_blocking() {
        let (_dir, path) = write("");
        let checked = check(
            &path,
            ConfigCheck {
                set: serde_json::json!({ "pdf.ocr_command": ["sh"] })
                    .as_object()
                    .cloned()
                    .unwrap(),
                unset: Vec::new(),
            },
        )
        .unwrap();
        assert!(checked.blocking.iter().any(|e| e.path == FILE_ONLY));
    }
}

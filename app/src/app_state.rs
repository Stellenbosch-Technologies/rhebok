use std::io::Write;
use std::path::{Path, PathBuf};

use gpui_kit::{App, Global};

pub const FILE_NAME: &str = "app-state.json";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AppState {
    pub setup_dismissed: bool,
}

pub struct AppStateFile(pub PathBuf);

impl Global for AppStateFile {}

pub fn path_beside(db: &Path) -> PathBuf {
    db.with_file_name(FILE_NAME)
}

pub fn default_path() -> PathBuf {
    path_beside(&rhebok::config::Config::db_path())
}

pub fn install(cx: &mut App, path: PathBuf) {
    cx.set_global(AppStateFile(path));
}

pub fn installed_path(cx: &App) -> Option<PathBuf> {
    cx.try_global::<AppStateFile>().map(|f| f.0.clone())
}

pub fn load(path: &Path) -> AppState {
    let read = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    AppState {
        setup_dismissed: read
            .as_ref()
            .and_then(|v| v.get("setup_dismissed"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    }
}

pub fn save(path: &Path, state: AppState) -> std::io::Result<()> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let body = serde_json::json!({ "setup_dismissed": state.setup_dismissed }).to_string();
    let staged = dir.join(format!(".{FILE_NAME}.{}.tmp", std::process::id()));
    let written =
        write_synced(&staged, body.as_bytes()).and_then(|()| std::fs::rename(&staged, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    written?;
    std::fs::File::open(dir)?.sync_all()
}

fn write_synced(path: &Path, body: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    file.write_all(body)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::{load, path_beside, save, AppState, FILE_NAME};
    use std::path::Path;

    #[test]
    fn a_missing_or_unparseable_state_file_means_not_dismissed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        assert_eq!(load(&path), AppState::default());
        for broken in [
            "",
            "{",
            "not json",
            "[true]",
            "{\"setup_dismissed\": \"yes\"}",
        ] {
            std::fs::write(&path, broken).unwrap();
            assert!(!load(&path).setup_dismissed, "{broken:?}");
        }
        std::fs::write(&path, "{}").unwrap();
        assert!(!load(&path).setup_dismissed);
        std::fs::create_dir_all(dir.path().join("as-a-dir").join(FILE_NAME)).unwrap();
        assert!(!load(&dir.path().join("as-a-dir").join(FILE_NAME)).setup_dismissed);
    }

    #[test]
    fn saving_replaces_the_file_whole_and_leaves_nothing_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rhebok").join(FILE_NAME);
        save(
            &path,
            AppState {
                setup_dismissed: true,
            },
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\"setup_dismissed\":true}"
        );
        assert!(load(&path).setup_dismissed);
        std::fs::write(&path, "garbage that is longer than the new state").unwrap();
        save(
            &path,
            AppState {
                setup_dismissed: false,
            },
        )
        .unwrap();
        assert!(!load(&path).setup_dismissed);
        let names: Vec<String> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [FILE_NAME]);
    }

    #[test]
    fn the_state_file_sits_beside_the_database() {
        assert_eq!(
            path_beside(Path::new("/data/rhebok/db")),
            Path::new("/data/rhebok/app-state.json")
        );
    }
}

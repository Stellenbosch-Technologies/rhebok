pub mod agents;
pub mod app;
pub mod claude;
pub mod install;
pub mod migrate;
pub mod model;
pub mod ollama;
pub mod plugin;

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub db: PathBuf,
    pub bin: PathBuf,
    pub plugin: PathBuf,
    pub tmp: PathBuf,
    pub update_json: PathBuf,
    pub update_status: PathBuf,
    pub link_dirs: Vec<PathBuf>,
    pub claude_cache: PathBuf,
    pub applications: PathBuf,
    pub homebrew: Option<PathBuf>,
}

impl Paths {
    pub fn from_env() -> Paths {
        let db = crate::config::Config::db_path();
        let root = db
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let home = directories::BaseDirs::new()
            .map(|b| b.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let link_dirs = match std::env::var_os("RHEBOK_LINK_DIR") {
            Some(d) => vec![PathBuf::from(d)],
            None => vec![PathBuf::from("/usr/local/bin"), home.join(".local/bin")],
        };
        let mut paths = Paths::at(&root, link_dirs, home.join(".claude/plugins/cache/rhebok"));
        paths.db = db;
        paths.applications = home.join("Applications");
        let exe = std::env::current_exe().and_then(|e| e.canonicalize()).ok();
        match exe.as_deref().and_then(homebrew_prefix) {
            Some(prefix) => paths.installed_by_homebrew(&prefix),
            None => paths,
        }
    }

    pub fn at(root: &Path, link_dirs: Vec<PathBuf>, claude_cache: PathBuf) -> Paths {
        Paths {
            root: root.to_path_buf(),
            db: root.join("db"),
            bin: root.join("bin/rhebok"),
            plugin: root.join("plugin"),
            tmp: root.join("tmp"),
            update_json: root.join("update.json"),
            update_status: root.join("update.status"),
            link_dirs,
            claude_cache,
            applications: root.join("Applications"),
            homebrew: None,
        }
    }

    pub fn app_bundle(&self) -> PathBuf {
        self.applications.join(app::BUNDLE_NAME)
    }

    pub fn installed_by_homebrew(self, prefix: &Path) -> Paths {
        Paths {
            bin: prefix.join("opt/rhebok/bin/rhebok"),
            homebrew: Some(prefix.to_path_buf()),
            ..self
        }
    }
}

pub fn answer_is_yes(line: &str, default_yes: bool) -> bool {
    match line.trim() {
        "" => default_yes,
        "y" | "Y" | "yes" | "YES" => true,
        _ => false,
    }
}

pub fn homebrew_prefix(exe: &Path) -> Option<PathBuf> {
    let bin = exe.parent()?;
    let formula = bin.parent()?.parent()?;
    let cellar = formula.parent()?;
    let named = |p: &Path, name: &str| p.file_name().is_some_and(|n| n == name);
    let in_a_keg = named(exe, "rhebok")
        && named(bin, "bin")
        && named(formula, "rhebok")
        && named(cellar, "Cellar");
    in_a_keg
        .then(|| cellar.parent())
        .flatten()
        .map(Path::to_path_buf)
}

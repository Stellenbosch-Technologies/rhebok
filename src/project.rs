use anyhow::{bail, Context as _, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const REPO_FILE: &str = ".rhebok/project.toml";
pub const LEGACY_REPO_FILE: &str = ".br8n/project.toml";
pub const PROJECT_KEYS: &[&str] = &[
    "roots", "include", "exclude", "memories", "imports", "slices",
];
pub const REPO_KEYS: &[&str] = &[
    "name", "include", "exclude", "memories", "imports", "slices",
];
pub const SLICE_KEYS: &[&str] = &["include", "exclude"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryScope {
    #[default]
    Both,
    Project,
    Global,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliceConfig {
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectConfig {
    #[serde(default)]
    pub roots: Vec<PathBuf>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub memories: MemoryScope,
    #[serde(default)]
    pub imports: Vec<String>,
    #[serde(default)]
    pub slices: BTreeMap<String, SliceConfig>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountConfig {
    pub path: PathBuf,
}

pub const SLICE_FILE: &str = "slice.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliceManifest {
    pub name: String,
    pub exported_at: i64,
    pub documents: usize,
    pub chunks: usize,
    pub transcripts_left_out: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoProjectFile {
    pub name: String,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub memories: MemoryScope,
    #[serde(default)]
    pub imports: Vec<String>,
    #[serde(default)]
    pub slices: BTreeMap<String, SliceConfig>,
}

#[derive(Debug, Clone)]
pub enum Rule {
    Uri(globset::GlobMatcher),
    Tag(String),
    Type(String),
    Agent(String),
}

pub const AGENTS: &[&str] = &["claude", "codex"];

impl Rule {
    pub fn parse(text: &str) -> Result<Rule> {
        let text = text.trim();
        if let Some(tag) = text.strip_prefix("tag:") {
            return non_empty(tag, text).map(|t| Rule::Tag(t.to_string()));
        }
        if let Some(kind) = text.strip_prefix("type:") {
            return non_empty(kind, text).map(|t| Rule::Type(t.to_string()));
        }
        if let Some(agent) = text.strip_prefix("agent:") {
            if !AGENTS.contains(&agent) {
                bail!("`{text}`: agent must be one of {}", AGENTS.join(", "));
            }
            return Ok(Rule::Agent(agent.to_string()));
        }
        if !text.contains("://") {
            bail!(
                "`{text}` is not a rule: write a URI glob (`file:///notes/**`), \
                 `tag:<tag>`, `type:<source type>` or `agent:claude|codex`"
            );
        }
        let glob = globset::GlobBuilder::new(&expand_uri_tilde(text))
            .literal_separator(true)
            .build()
            .with_context(|| format!("`{text}` is not a valid glob"))?;
        Ok(Rule::Uri(glob.compile_matcher()))
    }
}

fn non_empty<'a>(value: &'a str, whole: &str) -> Result<&'a str> {
    if value.trim().is_empty() {
        bail!("`{whole}` names nothing after the colon");
    }
    Ok(value.trim())
}

fn expand_uri_tilde(uri: &str) -> String {
    let Some(rest) = uri.strip_prefix("file://~") else {
        return uri.to_string();
    };
    let home = directories::BaseDirs::new()
        .map(|b| b.home_dir().to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("file://{home}{rest}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Config,
    Repo(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Project {
    pub name: String,
    pub root: PathBuf,
    pub origin: Origin,
    pub rules: ProjectConfig,
}

#[derive(Debug, Clone, Default)]
pub struct Resolution {
    pub project: Option<Project>,
    pub untrusted: Option<PathBuf>,
}

impl Resolution {
    pub fn name(&self) -> Option<&str> {
        self.project.as_ref().map(|p| p.name.as_str())
    }
}

pub fn find_repo_file(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors().find_map(repo_file_in)
}

fn repo_file_in(dir: &Path) -> Option<PathBuf> {
    [REPO_FILE, LEGACY_REPO_FILE]
        .iter()
        .map(|name| dir.join(name))
        .find(|file| file.is_file())
}

pub fn read_repo_file(file: &Path) -> Result<RepoProjectFile> {
    let text = std::fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    toml::from_str(&text).with_context(|| format!("parse {}", file.display()))
}

fn repo_root(file: &Path) -> PathBuf {
    file.parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

pub fn resolve(cfg: &crate::config::Config, cwd: &Path) -> Resolution {
    resolve_with(cfg, cwd, &trust_store_path())
}

pub fn resolve_with(cfg: &crate::config::Config, cwd: &Path, trust: &Path) -> Resolution {
    let mut best: Option<(usize, Project)> = None;
    for (name, rules) in &cfg.projects {
        for root in &rules.roots {
            let root = crate::config::Config::expand_tilde_path(root);
            if cwd.starts_with(&root) {
                let depth = root.components().count();
                if best.as_ref().is_none_or(|(d, _)| depth > *d) {
                    best = Some((
                        depth,
                        Project {
                            name: name.clone(),
                            root,
                            origin: Origin::Config,
                            rules: rules.clone(),
                        },
                    ));
                }
            }
        }
    }
    let mut untrusted = None;
    if let Some(file) = find_repo_file(cwd) {
        if !is_trusted_in(trust, &file) {
            untrusted = Some(file);
        } else if let Some(repo) = read_repo_file(&file)
            .ok()
            .filter(|r| !cfg.projects.contains_key(&r.name))
        {
            let root = repo_root(&file);
            let depth = root.components().count();
            if best.as_ref().is_none_or(|(d, _)| depth > *d) {
                best = Some((
                    depth,
                    Project {
                        name: repo.name.clone(),
                        root,
                        origin: Origin::Repo(file),
                        rules: ProjectConfig {
                            roots: Vec::new(),
                            include: repo.include,
                            exclude: repo.exclude,
                            memories: repo.memories,
                            imports: repo.imports,
                            slices: repo.slices,
                        },
                    },
                ));
            }
        }
    }
    Resolution {
        project: best.map(|(_, p)| p),
        untrusted,
    }
}

pub fn all_projects(cfg: &crate::config::Config) -> BTreeMap<String, ProjectConfig> {
    all_projects_with(cfg, &trust_store_path())
}

pub fn all_projects_with(
    cfg: &crate::config::Config,
    trust: &Path,
) -> BTreeMap<String, ProjectConfig> {
    let mut all = cfg.projects.clone();
    for file in read_trust(trust).keys().map(PathBuf::from) {
        if !is_trusted_in(trust, &file) {
            continue;
        }
        let Ok(repo) = read_repo_file(&file) else {
            continue;
        };
        all.entry(repo.name.clone()).or_insert(ProjectConfig {
            roots: vec![repo_root(&file)],
            include: repo.include,
            exclude: repo.exclude,
            memories: repo.memories,
            imports: repo.imports,
            slices: repo.slices,
        });
    }
    all
}

pub fn scope_definitions(cfg: &crate::config::Config) -> Vec<crate::pack::scope::Definition> {
    crate::pack::scope::definitions(&all_projects(cfg))
}

pub fn trust_store_path() -> PathBuf {
    crate::config::Config::db_path().with_file_name("trust.json")
}

fn file_hash(file: &Path) -> Result<String> {
    let bytes = std::fs::read(file).with_context(|| format!("read {}", file.display()))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn read_trust(store: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(store)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn trust_key(file: &Path) -> String {
    std::fs::canonicalize(file)
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

pub fn is_trusted_in(store: &Path, file: &Path) -> bool {
    let Ok(hash) = file_hash(file) else {
        return false;
    };
    read_trust(store).get(&trust_key(file)) == Some(&hash)
}

pub fn trust_in(store: &Path, target: &Path) -> Result<PathBuf> {
    let file = if target.is_dir() {
        repo_file_in(target).unwrap_or_else(|| target.join(REPO_FILE))
    } else {
        target.to_path_buf()
    };
    let repo = read_repo_file(&file)?;
    validate_repo(&repo).map_err(|errors| anyhow::anyhow!(errors.join("; ")))?;
    let mut all = read_trust(store);
    all.insert(trust_key(&file), file_hash(&file)?);
    if let Some(parent) = store.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = store.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&all)?)?;
    std::fs::rename(&tmp, store)?;
    Ok(file)
}

fn rule_errors(prefix: &str, rules: &[String], out: &mut Vec<String>) {
    for rule in rules {
        if let Err(e) = Rule::parse(rule) {
            out.push(format!("{prefix}: {e:#}"));
        }
    }
}

pub fn validate_rules(prefix: &str, include: &[String], exclude: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    rule_errors(&format!("{prefix}.include"), include, &mut out);
    rule_errors(&format!("{prefix}.exclude"), exclude, &mut out);
    out
}

pub fn validate_repo(repo: &RepoProjectFile) -> std::result::Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if repo.name.trim().is_empty() {
        errors.push("name: a project file must name its project".to_string());
    }
    errors.extend(validate_rules("", &repo.include, &repo.exclude));
    for (slice, rules) in &repo.slices {
        errors.extend(validate_rules(
            &format!("slices.{slice}"),
            &rules.include,
            &rules.exclude,
        ));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub const IMPORT_WARNING: usize = 5;
pub const MOUNT_WARNING: usize = 2;

pub fn import_lines(cfg: &crate::config::Config, project: &Project) -> Vec<String> {
    let defs = scope_definitions(cfg);
    project
        .rules
        .imports
        .iter()
        .map(|name| {
            if let Some(mount) = cfg.mounts.get(name) {
                let dir = crate::config::Config::expand_tilde_path(&mount.path);
                match read_slice(&dir) {
                    Ok(slice) => format!(
                        "imports {name}: mounted from {} ({} documents)",
                        dir.display(),
                        slice.documents
                    ),
                    Err(e) => format!("imports {name}: mount unusable ({e:#})"),
                }
            } else if defs.iter().any(|d| &d.name == name) {
                format!("imports {name}: a local slice")
            } else {
                format!("imports {name}: names nothing, so it is ignored")
            }
        })
        .collect()
}

pub fn import_warnings(cfg: &crate::config::Config) -> Vec<String> {
    let mut out = Vec::new();
    for (name, project) in all_projects(cfg) {
        let mounts = project
            .imports
            .iter()
            .filter(|i| cfg.mounts.contains_key(*i))
            .count();
        let local = project.imports.len() - mounts;
        if local > IMPORT_WARNING {
            out.push(format!(
                "project {name} imports {local} local slices; each is one more list merged on \
                 every prompt in it (more than {IMPORT_WARNING} is worth a second look)"
            ));
        }
        if mounts > MOUNT_WARNING {
            out.push(format!(
                "project {name} imports {mounts} mounts; each is another pack opened and \
                 searched on every prompt in it (more than {MOUNT_WARNING} slows the hook)"
            ));
        }
    }
    out
}

pub fn describe(resolution: &Resolution, cwd: &Path) -> String {
    let mut out = match &resolution.project {
        None => format!("none for {}\n", cwd.display()),
        Some(p) => match &p.origin {
            Origin::Config => format!(
                "{} for {} (config.toml, root {})\n",
                p.name,
                cwd.display(),
                p.root.display()
            ),
            Origin::Repo(file) => format!(
                "{} for {} (trusted {})\n",
                p.name,
                cwd.display(),
                file.display()
            ),
        },
    };
    if let Some(file) = &resolution.untrusted {
        out.push_str(&format!(
            "{} is not trusted, so it is ignored; review it, then run `rhebok project trust {}`\n",
            file.display(),
            repo_root(file).display()
        ));
    }
    out
}

pub fn uri_for(target: &str) -> String {
    if target.contains("://") {
        return target.to_string();
    }
    let path = std::fs::canonicalize(target).unwrap_or_else(|_| PathBuf::from(target));
    format!("file://{}", path.display())
}

pub fn explain(cfg: &crate::config::Config, target: &str) -> Result<String> {
    use crate::pack::scope::{verdict, Facts, Verdict};
    let uri = uri_for(target);
    let db = crate::config::Config::db_path();
    let store = crate::store::Store::open_existing(&db, cfg.embed.dimensions)?;
    let doc = store
        .all_document_rows()?
        .into_iter()
        .find(|d| d.uri == uri)
        .ok_or_else(|| anyhow::anyhow!("{uri} is not in the index"))?;
    drop(store);
    let defs = scope_definitions(cfg);
    let facts = Facts {
        uri: &doc.uri,
        source_type: &doc.source_type,
        tags: &doc.tags,
    };
    let mut out = format!(
        "{}\n  type: {}, tags: {}\n",
        doc.uri,
        doc.source_type,
        if doc.tags.is_empty() {
            "none".to_string()
        } else {
            doc.tags.join(", ")
        }
    );
    if defs.is_empty() {
        out.push_str("  no projects are defined, so every prompt searches the whole index\n");
    }
    for (i, def) in defs.iter().enumerate() {
        let line = match verdict(&defs, i, &facts) {
            Verdict::In { by: Some(rule) } => format!("in   {} (included by `{rule}`)", def.name),
            Verdict::In { by: None } => format!("in   {} (it has no include rules)", def.name),
            Verdict::Out { reason } => format!("out  {} ({reason})", def.name),
        };
        out.push_str(&format!("  {line}\n"));
    }
    if !crate::pack::scope::current(&db, &defs) {
        out.push_str(
            "  the index's scopes are older than these rules; run `rhebok project rescope`\n",
        );
    }
    Ok(out)
}

fn project_roots(project: &Project) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = project
        .rules
        .roots
        .iter()
        .map(|r| crate::config::Config::expand_tilde_path(r))
        .collect();
    if !project.root.as_os_str().is_empty() {
        roots.push(project.root.clone());
    }
    roots
}

pub fn memory_applies(
    memory_project: Option<&str>,
    cwd: Option<&Path>,
    active: Option<&Project>,
) -> bool {
    let scope = active.map_or(MemoryScope::Both, |p| p.rules.memories);
    let global = memory_project.is_none();
    let path = memory_project.map(Path::new);
    let local = matches!((path, cwd), (Some(p), Some(c)) if c.starts_with(p));
    let in_project = match (memory_project, active) {
        (Some(saved), Some(project)) => {
            saved == project.name
                || project_roots(project)
                    .iter()
                    .any(|root| Path::new(saved).starts_with(root))
        }
        _ => false,
    };
    match scope {
        MemoryScope::Both => global || in_project || local,
        MemoryScope::Project => in_project || local,
        MemoryScope::Global => global,
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryFilter {
    pub cwd: Option<PathBuf>,
    pub project: Option<Project>,
}

impl MemoryFilter {
    pub fn admits(&self, memory_project: Option<&str>) -> bool {
        memory_applies(memory_project, self.cwd.as_deref(), self.project.as_ref())
    }
}

fn is_transcript(uri: &str) -> bool {
    uri.starts_with("claude-session://") || uri.starts_with("codex-session://")
}

#[derive(Debug, Clone, Default)]
pub struct ExportPlan {
    pub name: String,
    pub rows: Vec<usize>,
    pub documents: std::collections::BTreeSet<String>,
    pub transcripts_left_out: usize,
}

pub fn plan_export(
    cfg: &crate::config::Config,
    name: &str,
    include_transcripts: bool,
) -> Result<(crate::pack::Pack, ExportPlan)> {
    let db = crate::config::Config::db_path();
    let manifest = crate::pack::manifest::Manifest::read(&db)?;
    let pack = crate::pack::Pack::open(&db, &manifest.model_id, manifest.dims)?;
    let defs = scope_definitions(cfg);
    let def = defs
        .iter()
        .find(|d| d.name == name)
        .ok_or_else(|| anyhow::anyhow!("`{name}` is not a project or `<project>:<slice>`"))?;
    let set = crate::pack::scope::row_set(
        crate::pack::scope::Reader::open(&db)?.as_ref(),
        pack.rows(),
        &[(def.name.clone(), def.hash.clone())],
    )?;
    let mut plan = ExportPlan {
        name: name.to_string(),
        ..ExportPlan::default()
    };
    let mut left_out = std::collections::BTreeSet::new();
    for row in set.iter() {
        let record = pack.record(row)?;
        if !include_transcripts && is_transcript(&record.uri) {
            left_out.insert(record.uri);
            continue;
        }
        plan.documents.insert(record.uri);
        plan.rows.push(row);
    }
    plan.transcripts_left_out = left_out.len();
    Ok((pack, plan))
}

pub fn export(
    cfg: &crate::config::Config,
    name: &str,
    out: &Path,
    include_transcripts: bool,
) -> Result<ExportPlan> {
    let (pack, plan) = plan_export(cfg, name, include_transcripts)?;
    if plan.rows.is_empty() {
        bail!("`{name}` has nothing to export");
    }
    if out.exists() && std::fs::read_dir(out)?.next().is_some() && !out.join(SLICE_FILE).exists() {
        bail!(
            "{} is not empty and is not an earlier export; choose an empty directory",
            out.display()
        );
    }
    std::fs::create_dir_all(out)?;
    let manifest = crate::pack::manifest::Manifest::read(&crate::config::Config::db_path())?;
    let mut rows = Vec::with_capacity(plan.rows.len());
    let mut lifecycles = std::collections::HashMap::new();
    for &row in &plan.rows {
        let record = pack.record(row)?;
        if record.lifecycle != Default::default() {
            lifecycles.insert(record.doc_id.clone(), record.lifecycle);
        }
        let vector = pack.vector(row)?.unwrap_or_default();
        rows.push((record, vector));
    }
    crate::pack::Pack::build(
        out,
        &manifest.model_id,
        manifest.dims,
        rows,
        &Default::default(),
        &lifecycles,
    )?;
    let slice = SliceManifest {
        name: name.to_string(),
        exported_at: crate::memory::now_secs(),
        documents: plan.documents.len(),
        chunks: plan.rows.len(),
        transcripts_left_out: plan.transcripts_left_out,
    };
    std::fs::write(out.join(SLICE_FILE), serde_json::to_string_pretty(&slice)?)?;
    Ok(plan)
}

pub fn read_slice(dir: &Path) -> Result<SliceManifest> {
    let text = std::fs::read_to_string(dir.join(SLICE_FILE))
        .with_context(|| format!("{} is not an exported rhebok slice", dir.display()))?;
    serde_json::from_str(&text).with_context(|| format!("read {}", dir.join(SLICE_FILE).display()))
}

pub fn open_mount(cfg: &crate::config::Config, dir: &Path) -> Result<crate::pack::Pack> {
    read_slice(dir)?;
    let model_id = crate::embed::for_config(&cfg.embed)?.model_id();
    crate::pack::Pack::open(dir, &model_id, cfg.embed.dimensions).with_context(|| {
        format!(
            "{} cannot be searched here: it must be exported by a rhebok using the same \
             embedding model ({model_id}), analyzer and pack format",
            dir.display()
        )
    })
}

pub fn chat_instructions(cfg: &crate::config::Config, name: &str) -> Result<String> {
    let projects = all_projects(cfg);
    let project = projects.get(name).ok_or_else(|| {
        anyhow::anyhow!("`{name}` is not a project; `rhebok project list` shows them")
    })?;
    let root = project
        .roots
        .first()
        .map(|r| crate::config::Config::expand_tilde_path(r))
        .ok_or_else(|| anyhow::anyhow!("project `{name}` has no roots to name it by"))?;
    Ok(format!(
        "Before you answer each of my messages, call the rhebok tool `rhebok_context` with \
`message` set to my message exactly as I wrote it and `cwd` set to \"{root}\". \
That selects my rhebok project `{name}`, so the answer only draws on that project's notes \
and memories.\n\
\n\
If the result's `block` is not null, treat it as context from my own notes: use it where \
it is relevant and name the notes you used. If `block` is null, answer normally and do \
not mention rhebok. If `notes` is not empty, tell me its first line in one sentence, \
because it means rhebok could not search properly.\n\
\n\
Skip the call for messages that are only a greeting or a thank-you.\n",
        root = root.display()
    ))
}

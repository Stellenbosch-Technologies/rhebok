use crate::pack::records::Record;
use crate::project::{ProjectConfig, Rule};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

pub const SCOPE_FILE: &str = "pack.scope";
const MAGIC: &[u8; 8] = b"BR8NSCP1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Project,
    Slice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub kind: Kind,
    pub hash: String,
    pub start: usize,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Header {
    rows: usize,
    scopes: Vec<Entry>,
}

#[derive(Debug, Clone)]
pub struct Definition {
    pub name: String,
    pub kind: Kind,
    pub hash: String,
    include: Vec<(String, Rule)>,
    exclude: Vec<(String, Rule)>,
    parent: Option<usize>,
}

pub struct Facts<'a> {
    pub uri: &'a str,
    pub source_type: &'a str,
    pub tags: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    In { by: Option<String> },
    Out { reason: String },
}

fn agent_of(uri: &str) -> Option<&'static str> {
    if uri.starts_with("claude-session://") {
        Some("claude")
    } else if uri.starts_with("codex-session://") {
        Some("codex")
    } else {
        None
    }
}

fn matches(rule: &Rule, facts: &Facts<'_>) -> bool {
    match rule {
        Rule::Uri(glob) => glob.is_match(facts.uri),
        Rule::Tag(tag) => facts.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)),
        Rule::Type(kind) => facts.source_type.eq_ignore_ascii_case(kind),
        Rule::Agent(agent) => agent_of(facts.uri) == Some(agent.as_str()),
    }
}

fn parse_all(rules: &[String]) -> Vec<(String, Rule)> {
    rules
        .iter()
        .filter_map(|r| Rule::parse(r).ok().map(|parsed| (r.clone(), parsed)))
        .collect()
}

fn rules_hash(include: &[String], exclude: &[String], parent: Option<&str>) -> String {
    let canonical = serde_json::json!({ "include": include, "exclude": exclude, "parent": parent });
    hex::encode(Sha256::digest(canonical.to_string().as_bytes()))
}

pub fn slice_name(project: &str, slice: &str) -> String {
    format!("{project}:{slice}")
}

pub fn definitions(projects: &BTreeMap<String, ProjectConfig>) -> Vec<Definition> {
    let mut out = Vec::new();
    for (name, project) in projects {
        let parent_hash = rules_hash(&project.include, &project.exclude, None);
        out.push(Definition {
            name: name.clone(),
            kind: Kind::Project,
            hash: parent_hash.clone(),
            include: parse_all(&project.include),
            exclude: parse_all(&project.exclude),
            parent: None,
        });
        let parent = out.len() - 1;
        for (slice, rules) in &project.slices {
            out.push(Definition {
                name: slice_name(name, slice),
                kind: Kind::Slice,
                hash: rules_hash(&rules.include, &rules.exclude, Some(&parent_hash)),
                include: parse_all(&rules.include),
                exclude: parse_all(&rules.exclude),
                parent: Some(parent),
            });
        }
    }
    out
}

pub fn verdict(defs: &[Definition], index: usize, facts: &Facts<'_>) -> Verdict {
    let def = &defs[index];
    if let Some(parent) = def.parent {
        if let Verdict::Out { reason } = verdict(defs, parent, facts) {
            return Verdict::Out {
                reason: format!("not in project {} ({reason})", defs[parent].name),
            };
        }
    }
    let by = if def.include.is_empty() {
        None
    } else {
        match def.include.iter().find(|(_, r)| matches(r, facts)) {
            Some((text, _)) => Some(text.clone()),
            None => {
                return Verdict::Out {
                    reason: "no include rule matches".to_string(),
                }
            }
        }
    };
    if let Some((text, _)) = def.exclude.iter().find(|(_, r)| matches(r, facts)) {
        return Verdict::Out {
            reason: format!("excluded by `{text}`"),
        };
    }
    Verdict::In { by }
}

pub fn write(
    dir: &Path,
    rows: usize,
    records: impl Iterator<Item = Result<Record>>,
    tags: &HashMap<String, Vec<String>>,
    defs: &[Definition],
) -> Result<()> {
    let empty: Vec<String> = Vec::new();
    let mut members: Vec<Vec<u32>> = vec![Vec::new(); defs.len()];
    for (row, r) in records.enumerate() {
        let r = r?;
        let facts = Facts {
            uri: &r.uri,
            source_type: &r.source_type,
            tags: tags.get(&r.doc_id).unwrap_or(&empty),
        };
        for (i, list) in members.iter_mut().enumerate() {
            if matches!(verdict(defs, i, &facts), Verdict::In { .. }) {
                list.push(row as u32);
            }
        }
    }
    let mut scopes = Vec::new();
    let mut start = 0;
    for (def, list) in defs.iter().zip(&members) {
        scopes.push(Entry {
            name: def.name.clone(),
            kind: def.kind,
            hash: def.hash.clone(),
            start,
            count: list.len(),
        });
        start += list.len();
    }
    let header = serde_json::to_vec(&Header { rows, scopes })?;
    let tmp = dir.join(format!("{SCOPE_FILE}.tmp"));
    super::write_buffered(&tmp, |out| {
        use std::io::Write;
        out.write_all(MAGIC)?;
        out.write_all(&(header.len() as u32).to_le_bytes())?;
        out.write_all(&header)?;
        for list in &members {
            for row in list {
                out.write_all(&row.to_le_bytes())?;
            }
        }
        Ok(())
    })?;
    std::fs::rename(&tmp, dir.join(SCOPE_FILE))
        .with_context(|| format!("publish {}", dir.join(SCOPE_FILE).display()))
}

pub fn write_beside(
    dir: &Path,
    tags: &HashMap<String, Vec<String>>,
    defs: &[Definition],
) -> Result<()> {
    let recs = super::records::Reader::open(dir)?;
    let rows = recs.len();
    write(dir, rows, (0..rows).map(|row| recs.get(row)), tags, defs)
}

pub fn current(dir: &Path, defs: &[Definition]) -> bool {
    let Ok(Some(reader)) = Reader::open(dir) else {
        return defs.is_empty();
    };
    reader.entries().len() == defs.len()
        && defs
            .iter()
            .all(|d| reader.entry(&d.name).is_some_and(|e| e.hash == d.hash))
}

pub struct Reader {
    map: memmap2::Mmap,
    body: usize,
    header: Header,
}

impl Reader {
    pub fn open(dir: &Path) -> Result<Option<Reader>> {
        let path = dir.join(SCOPE_FILE);
        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("open {}", path.display())),
        };
        let map = unsafe { memmap2::Mmap::map(&file) }
            .with_context(|| format!("mmap {}", path.display()))?;
        if map.len() < 12 || &map[..8] != MAGIC {
            bail!("{} is not a rhebok scope file", path.display());
        }
        let len = u32::from_le_bytes(map[8..12].try_into().unwrap()) as usize;
        let body = 12 + len;
        anyhow::ensure!(map.len() >= body, "{} is truncated", path.display());
        let header: Header = serde_json::from_slice(&map[12..body])
            .with_context(|| format!("read {}'s header", path.display()))?;
        let total: usize = header.scopes.iter().map(|s| s.count).sum();
        anyhow::ensure!(
            map.len() == body + total * 4,
            "{} is truncated",
            path.display()
        );
        Ok(Some(Reader { map, body, header }))
    }

    pub fn rows(&self) -> usize {
        self.header.rows
    }

    pub fn entries(&self) -> &[Entry] {
        &self.header.scopes
    }

    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.header.scopes.iter().find(|s| s.name == name)
    }

    pub fn members(&self, entry: &Entry) -> impl Iterator<Item = usize> + '_ {
        let from = self.body + entry.start * 4;
        (0..entry.count).map(move |i| {
            let o = from + i * 4;
            u32::from_le_bytes(self.map[o..o + 4].try_into().unwrap()) as usize
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowSet {
    bits: Vec<u64>,
    len: usize,
    pub names: Vec<String>,
}

impl RowSet {
    pub fn empty(rows: usize) -> RowSet {
        RowSet {
            bits: vec![0; rows.div_ceil(64)],
            len: 0,
            names: Vec::new(),
        }
    }

    pub fn insert(&mut self, row: usize) {
        let (w, b) = (row / 64, row % 64);
        if w < self.bits.len() && self.bits[w] & (1 << b) == 0 {
            self.bits[w] |= 1 << b;
            self.len += 1;
        }
    }

    pub fn contains(&self, row: usize) -> bool {
        let (w, b) = (row / 64, row % 64);
        w < self.bits.len() && self.bits[w] & (1 << b) != 0
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.bits.iter().enumerate().flat_map(|(w, word)| {
            (0..64).filter_map(move |b| (word & (1 << b) != 0).then_some(w * 64 + b))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeError {
    Missing { name: String },
    Stale { name: String },
    NoFile,
    Generation { scope_rows: usize, pack_rows: usize },
}

impl std::fmt::Display for ScopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScopeError::NoFile => write!(
                f,
                "this index has no project scopes yet; run `rhebok project rescope`"
            ),
            ScopeError::Missing { name } => write!(
                f,
                "project scope `{name}` is not in the index yet; run `rhebok project rescope`"
            ),
            ScopeError::Stale { name } => write!(
                f,
                "project scope `{name}` was built from rules that have since changed; \
                 run `rhebok project rescope`"
            ),
            ScopeError::Generation {
                scope_rows,
                pack_rows,
            } => write!(
                f,
                "pack.scope covers {scope_rows} rows but the pack has {pack_rows}; \
                 run `rhebok project rescope`"
            ),
        }
    }
}

impl std::error::Error for ScopeError {}

pub fn row_set(
    reader: Option<&Reader>,
    pack_rows: usize,
    wanted: &[(String, String)],
) -> std::result::Result<RowSet, ScopeError> {
    let reader = reader.ok_or(ScopeError::NoFile)?;
    if reader.rows() != pack_rows {
        return Err(ScopeError::Generation {
            scope_rows: reader.rows(),
            pack_rows,
        });
    }
    let mut set = RowSet::empty(pack_rows);
    for (name, hash) in wanted {
        let entry = reader
            .entry(name)
            .ok_or_else(|| ScopeError::Missing { name: name.clone() })?;
        if &entry.hash != hash {
            return Err(ScopeError::Stale { name: name.clone() });
        }
        for row in reader.members(entry) {
            set.insert(row);
        }
        set.names.push(name.clone());
    }
    Ok(set)
}

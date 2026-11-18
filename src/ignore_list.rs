use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum Entry {
    Name(String),
    Path(PathBuf),
    Glob(globset::GlobMatcher, bool),
}

impl Entry {
    pub fn parse(text: &str) -> Result<Entry, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("an empty entry matches nothing".to_string());
        }
        let rooted = text.starts_with('/') || text.starts_with("~/") || text == "~";
        if text.contains(['*', '?', '[', '{']) {
            let expanded = if rooted {
                crate::config::Config::expand_tilde_path(Path::new(text))
                    .display()
                    .to_string()
            } else {
                text.to_string()
            };
            let per_component = !expanded.contains('/');
            let pattern = if per_component || rooted || expanded.starts_with("**/") {
                expanded
            } else {
                format!("**/{expanded}")
            };
            let glob = globset::GlobBuilder::new(&pattern)
                .literal_separator(true)
                .build()
                .map_err(|e| format!("`{text}` is not a valid glob: {e}"))?;
            return Ok(Entry::Glob(glob.compile_matcher(), per_component));
        }
        if rooted {
            return Ok(Entry::Path(crate::config::Config::expand_tilde_path(
                Path::new(text),
            )));
        }
        if text.contains('/') {
            return Err(format!(
                "`{text}` is a relative path: write a name (`drafts`), an absolute path \
                 (`~/notes/drafts`) or a glob (`**/drafts/*.md`)"
            ));
        }
        Ok(Entry::Name(text.to_string()))
    }

    pub fn matches(&self, path: &Path) -> bool {
        match self {
            Entry::Name(name) => path
                .components()
                .any(|c| c.as_os_str().to_str() == Some(name.as_str())),
            Entry::Path(root) => path.starts_with(root),
            Entry::Glob(glob, true) => path.components().any(|c| glob.is_match(c.as_os_str())),
            Entry::Glob(glob, false) => glob.is_match(path),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct IgnoreList {
    entries: Vec<Entry>,
}

impl IgnoreList {
    pub fn new(entries: &[String]) -> IgnoreList {
        IgnoreList {
            entries: entries
                .iter()
                .filter_map(|e| Entry::parse(e).ok())
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn matches(&self, path: &Path) -> bool {
        self.entries.iter().any(|e| e.matches(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(entries: &[&str]) -> IgnoreList {
        IgnoreList::new(&entries.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn a_name_matches_any_component_and_nothing_else() {
        let l = list(&["_templates"]);
        assert!(l.matches(Path::new("/v/_templates/adr.md")));
        assert!(!l.matches(Path::new("/v/my_templates/adr.md")));
    }

    #[test]
    fn an_absolute_path_matches_itself_and_what_is_under_it() {
        let l = list(&["/v/drafts"]);
        assert!(l.matches(Path::new("/v/drafts")));
        assert!(l.matches(Path::new("/v/drafts/a.md")));
        assert!(!l.matches(Path::new("/v/drafts-old/a.md")));
    }

    #[test]
    fn a_glob_without_a_slash_matches_a_component() {
        let l = list(&["*.jsonl"]);
        assert!(l.matches(Path::new("/h/.claude/projects/p/s.jsonl")));
        assert!(!l.matches(Path::new("/h/notes/a.md")));
    }

    #[test]
    fn a_relative_glob_with_a_slash_matches_at_any_depth() {
        let l = list(&["workflows/*/journal.jsonl"]);
        assert!(l.matches(Path::new(
            "/h/.claude/projects/p/s/subagents/workflows/wf_1/journal.jsonl"
        )));
        assert!(!l.matches(Path::new("/h/.claude/projects/p/s/journal.jsonl")));
    }

    #[test]
    fn a_single_star_does_not_cross_a_folder() {
        let l = list(&["/h/*/a.md"]);
        assert!(l.matches(Path::new("/h/x/a.md")));
        assert!(!l.matches(Path::new("/h/x/y/a.md")));
    }

    #[test]
    fn bad_entries_are_refused_by_parse_and_dropped_by_the_list() {
        assert!(Entry::parse("notes/drafts").is_err());
        assert!(Entry::parse("[").is_err());
        assert!(Entry::parse("  ").is_err());
        assert!(list(&["[", "notes/drafts"]).is_empty());
    }
}

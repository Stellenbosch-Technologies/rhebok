use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipKind {
    Settling,
    Empty,
    Unreadable,
    Scanned,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct IoCode {
    pub kind: String,
    pub os_error: Option<i32>,
}

impl IoCode {
    fn of(e: &std::io::Error) -> IoCode {
        IoCode {
            kind: format!("{:?}", e.kind()),
            os_error: e.raw_os_error(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Skip {
    pub path: String,
    pub kind: SkipKind,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub io: Option<IoCode>,
}

#[derive(Debug)]
pub struct EmptyTranscript;

impl std::fmt::Display for EmptyTranscript {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("empty transcript")
    }
}

impl std::error::Error for EmptyTranscript {}

impl Skip {
    pub fn settling(path: &Path, detail: String) -> Skip {
        Skip {
            path: path.display().to_string(),
            kind: SkipKind::Settling,
            detail,
            io: None,
        }
    }

    pub fn unreadable(path: &Path, e: &std::io::Error) -> Skip {
        Skip {
            path: path.display().to_string(),
            kind: SkipKind::Unreadable,
            detail: e.to_string(),
            io: Some(IoCode::of(e)),
        }
    }

    pub fn from_error(path: &Path, e: &anyhow::Error) -> Skip {
        let io = e
            .chain()
            .find_map(|c| c.downcast_ref::<std::io::Error>())
            .map(IoCode::of);
        let scanned = e.chain().any(|c| {
            matches!(
                c.downcast_ref::<crate::loaders::pdf::PdfError>(),
                Some(crate::loaders::pdf::PdfError::Scanned { .. })
            )
        });
        let kind = if e.chain().any(|c| c.is::<EmptyTranscript>()) {
            SkipKind::Empty
        } else if scanned {
            SkipKind::Scanned
        } else if io.is_some() {
            SkipKind::Unreadable
        } else {
            SkipKind::Failed
        };
        Skip {
            path: path.display().to_string(),
            kind,
            detail: format!("{e:#}"),
            io,
        }
    }
}

impl std::fmt::Display for Skip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.detail)
    }
}

pub fn decode(json: &str) -> Vec<Skip> {
    if let Ok(skips) = serde_json::from_str::<Vec<Skip>>(json) {
        return skips;
    }
    serde_json::from_str::<Vec<String>>(json)
        .unwrap_or_default()
        .into_iter()
        .map(|line| {
            let (path, detail) = match line.split_once(": ") {
                Some((path, detail)) => (path.to_string(), detail.to_string()),
                None => (line.clone(), String::new()),
            };
            Skip {
                path,
                kind: SkipKind::Failed,
                detail,
                io: None,
            }
        })
        .collect()
}

pub fn split_ignored(skips: Vec<Skip>, ignore: &[String]) -> (Vec<Skip>, usize) {
    let list = crate::ignore_list::IgnoreList::new(ignore);
    if list.is_empty() {
        return (skips, 0);
    }
    let before = skips.len();
    let shown: Vec<Skip> = skips
        .into_iter()
        .filter(|s| !list.matches(Path::new(&s.path)))
        .collect();
    let hidden = before - shown.len();
    (shown, hidden)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_transcript_is_classified_by_type_not_by_its_words() {
        let typed = anyhow::Error::new(EmptyTranscript).context("loading s.jsonl");
        assert_eq!(
            Skip::from_error(Path::new("/s.jsonl"), &typed).kind,
            SkipKind::Empty
        );
        let worded = anyhow::anyhow!("empty transcript");
        assert_eq!(
            Skip::from_error(Path::new("/s.jsonl"), &worded).kind,
            SkipKind::Failed
        );
    }

    #[test]
    fn an_io_failure_keeps_its_code_and_the_whole_chain() {
        let io = std::io::Error::from_raw_os_error(13);
        let e = anyhow::Error::new(io).context("reading /n/a.md");
        let s = Skip::from_error(Path::new("/n/a.md"), &e);
        assert_eq!(s.kind, SkipKind::Unreadable);
        assert_eq!(s.io.as_ref().and_then(|c| c.os_error), Some(13));
        assert_eq!(
            s.io.as_ref().map(|c| c.kind.as_str()),
            Some("PermissionDenied")
        );
        assert!(s.detail.starts_with("reading /n/a.md: "), "{}", s.detail);
    }

    #[test]
    fn a_list_written_before_kinds_existed_still_reads() {
        let old = r#"["/h/p/s.jsonl: empty transcript", "/h/n/a.md: reading /h/n/a.md: denied"]"#;
        let skips = decode(old);
        assert_eq!(skips[0].path, "/h/p/s.jsonl");
        assert_eq!(skips[0].detail, "empty transcript");
        assert_eq!(skips[1].path, "/h/n/a.md");
        assert_eq!(skips[1].detail, "reading /h/n/a.md: denied");
    }

    #[test]
    fn ignored_skips_are_hidden_and_counted() {
        let skips = decode(r#"["/h/a/journal.jsonl: x", "/h/b/s.jsonl: y"]"#);
        let (shown, hidden) = split_ignored(skips, &["journal.jsonl".to_string()]);
        assert_eq!(hidden, 1);
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].path, "/h/b/s.jsonl");
    }
}

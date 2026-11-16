use crate::pack::Pack;
use aho_corasick::AhoCorasick;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};

pub const MIN_QUERY_WORDS: usize = 3;
pub const WHOLE_SESSION_QUOTES: usize = 10;
const TRANSCRIPT: &str = "transcript";

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Footprint {
    pub sessions: BTreeSet<String>,
    pub rows: BTreeSet<usize>,
}

pub fn normalize(text: &str) -> String {
    text.split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

pub struct Scanner {
    matcher: Option<AhoCorasick>,
    owners: Vec<String>,
    quoting_rows: BTreeSet<usize>,
    quoted: BTreeMap<String, BTreeSet<usize>>,
}

impl Scanner {
    pub fn new<S: AsRef<str>>(queries: &[S]) -> Result<Scanner> {
        let patterns: BTreeSet<String> = queries
            .iter()
            .map(|q| normalize(q.as_ref()))
            .filter(|q| q.split(' ').count() >= MIN_QUERY_WORDS)
            .collect();
        let matcher = if patterns.is_empty() {
            None
        } else {
            Some(AhoCorasick::new(patterns).context("could not build the golden-query matcher")?)
        };
        Ok(Scanner {
            matcher,
            owners: Vec::new(),
            quoting_rows: BTreeSet::new(),
            quoted: BTreeMap::new(),
        })
    }

    pub fn push(&mut self, doc_id: &str, source_type: &str, text: &str) {
        let row = self.owners.len();
        self.owners.push(doc_id.to_string());
        let Some(matcher) = self.matcher.as_ref().filter(|_| source_type == TRANSCRIPT) else {
            return;
        };
        let found: BTreeSet<usize> = matcher
            .find_overlapping_iter(&normalize(text))
            .map(|m| m.pattern().as_usize())
            .collect();
        if !found.is_empty() {
            self.quoting_rows.insert(row);
            self.quoted
                .entry(doc_id.to_string())
                .or_default()
                .extend(found);
        }
    }

    pub fn finish(self) -> Footprint {
        let sessions: BTreeSet<String> = self
            .quoted
            .into_iter()
            .filter(|(_, queries)| queries.len() >= WHOLE_SESSION_QUOTES)
            .map(|(doc, _)| doc)
            .collect();
        let rows = self
            .owners
            .iter()
            .enumerate()
            .filter(|(row, doc)| self.quoting_rows.contains(row) || sessions.contains(*doc))
            .map(|(row, _)| row)
            .collect();
        Footprint { sessions, rows }
    }
}

pub fn in_pack<S: AsRef<str>>(pack: &Pack, queries: &[S]) -> Result<Footprint> {
    let mut scanner = Scanner::new(queries)?;
    for row in 0..pack.rows() {
        let record = pack.record(row)?;
        scanner.push(&record.doc_id, &record.source_type, &record.text);
    }
    Ok(scanner.finish())
}

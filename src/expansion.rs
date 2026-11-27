use std::collections::{BTreeSet, HashSet};
use std::hash::Hash;

use crate::model::Chunk;

pub const FOLLOWED_LINK_KINDS: [&str; 2] = ["wikilink", "superseded-by"];

pub fn adjacent_order(
    found: impl IntoIterator<Item = (String, String, i64)>,
    seeds: &[String],
) -> Vec<String> {
    let seeds: HashSet<&str> = seeds.iter().map(String::as_str).collect();
    let ordered: BTreeSet<(String, i64, String)> = found
        .into_iter()
        .filter(|(id, _, _)| !seeds.contains(id.as_str()))
        .map(|(id, doc, ord)| (doc, ord, id))
        .collect();
    ordered.into_iter().map(|(_, _, id)| id).collect()
}

pub fn linked_order(
    target_docs: impl IntoIterator<Item = String>,
    seeds: &[String],
) -> Vec<String> {
    let seeds: HashSet<&str> = seeds.iter().map(String::as_str).collect();
    let docs: BTreeSet<String> = target_docs.into_iter().collect();
    docs.into_iter()
        .map(|doc| Chunk::id(&doc, 0))
        .filter(|id| !seeds.contains(id.as_str()))
        .collect()
}

pub fn interleave<T: Clone + Eq + Hash>(
    adjacent: &[T],
    linked: &[T],
    max_neighbors: usize,
) -> Vec<T> {
    let mut seen = HashSet::new();
    let mut chosen = Vec::with_capacity(max_neighbors);
    let mut a = adjacent.iter().take(max_neighbors);
    let mut l = linked.iter().take(max_neighbors);
    loop {
        let (na, nl) = (a.next(), l.next());
        if na.is_none() && nl.is_none() {
            break;
        }
        for id in [na, nl].into_iter().flatten() {
            if chosen.len() < max_neighbors && seen.insert(id) {
                chosen.push(id.clone());
            }
        }
        if chosen.len() >= max_neighbors {
            break;
        }
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn adjacent_chunks_order_by_document_then_numeric_ordinal_without_seeds_or_repeats() {
        let found = vec![
            ("b:10".into(), "b".into(), 10),
            ("b:8".into(), "b".into(), 8),
            ("a:3".into(), "a".into(), 3),
            ("b:9".into(), "b".into(), 9),
            ("b:8".into(), "b".into(), 8),
        ];
        assert_eq!(
            adjacent_order(found, &ids(&["b:9"])),
            ids(&["a:3", "b:8", "b:10"])
        );
    }

    #[test]
    fn linked_documents_become_their_first_chunk_in_document_order_without_seeds() {
        let got = linked_order(ids(&["z", "c", "z", "m"]), &ids(&["m:0"]));
        assert_eq!(got, ids(&["c:0", "z:0"]));
    }

    #[test]
    fn interleave_alternates_and_never_reads_past_each_lists_own_limit() {
        let adjacent = ids(&["x", "a1", "a2", "a3"]);
        let linked = ids(&["x", "l1"]);
        assert_eq!(interleave(&adjacent, &linked, 3), ids(&["x", "a1", "l1"]));
        assert_eq!(interleave(&adjacent, &linked, 2), ids(&["x", "a1"]));
        assert_eq!(
            interleave(&ids(&["x", "y", "z"]), &ids(&["x", "y", "w"]), 2),
            ids(&["x", "y"])
        );
    }
}

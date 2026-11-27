use crate::common;
use rhebok::config::{Config, Profile, Surface};
use rhebok::model::{Chunk, Document, SourceType};
use rhebok::pack::{graph, Pack};
use rhebok::store::Store;
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

const SETTINGS: [(u8, usize); 5] = [(1, 5), (1, 8), (2, 12), (2, 2), (1, 1)];

struct Corpus {
    _dir: tempfile::TempDir,
    idx: rhebok::index::Indexer,
    chunks: Vec<String>,
    long: Document,
    linked: Document,
}

fn document(uri: &str, title: &str, body: &str) -> Document {
    Document::new(SourceType::Markdown, uri, title, body)
}

fn corpus() -> Corpus {
    let (dir, idx, _calls) = common::setup();
    let mut body = String::from("# Long\n\n");
    for i in 0..14 {
        body.push_str(&format!(
            "## S{i}\n\n{}\n\n",
            "filler words here. ".repeat(90)
        ));
    }
    let long = document("file:///long.md", "Long", &body);
    let linked = document("file:///linked.md", "Linked", "# Linked\n\nOne hop away.");
    let far = document("file:///far.md", "Far", "# Far\n\nTwo hops away.");
    let retired = document(
        "file:///retired.md",
        "Retired",
        "# Retired\n\nThe old decision.",
    );
    let current = document(
        "file:///current.md",
        "Current",
        "# Current\n\nThe new decision.",
    );
    let empty = document("file:///empty.md", "Empty", "");
    let docs = vec![
        long.clone(),
        linked.clone(),
        far.clone(),
        retired.clone(),
        current.clone(),
        empty.clone(),
    ];
    idx.index_documents(&docs).unwrap();
    for (from, to, kind) in [
        (&long, &linked, "wikilink"),
        (&linked, &far, "wikilink"),
        (&far, &long, "wikilink"),
        (&long, &long, "wikilink"),
        (&linked, &empty, "wikilink"),
        (&retired, &current, "superseded-by"),
        (&current, &retired, "supersedes"),
        (&far, &retired, "wikilink"),
    ] {
        idx.store().link_documents(&from.id, &to.id, kind).unwrap();
    }
    let chunks = idx
        .store()
        .all_rows_for_pack()
        .unwrap()
        .into_iter()
        .map(|(r, _)| r.chunk_id)
        .collect();
    Corpus {
        _dir: dir,
        idx,
        chunks,
        long,
        linked,
    }
}

fn publish(store: &Store, dir: &Path) -> Vec<String> {
    let rows = store.all_rows_for_pack().unwrap();
    let ids: Vec<String> = rows.iter().map(|(r, _)| r.chunk_id.clone()).collect();
    std::fs::create_dir_all(dir).unwrap();
    Pack::build(
        dir,
        "fake@4",
        4,
        rows,
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    graph::write(dir, &ids, &store.graph_for_pack().unwrap()).unwrap();
    ids
}

fn ids_from_pack(pack: &Pack, seeds: &[String], hops: u8, max: usize) -> Vec<String> {
    pack.expand(seeds, hops, max)
        .unwrap()
        .into_iter()
        .map(|r| r.chunk_id)
        .collect()
}

fn ids_from_store(store: &Store, seeds: &[String], hops: u8, max: usize) -> Vec<String> {
    store
        .expand(seeds, hops, max)
        .unwrap()
        .into_iter()
        .map(|h| h.chunk_id)
        .collect()
}

#[test]
fn the_pack_expands_to_exactly_what_the_store_expands_to() {
    let c = corpus();
    let dir = tempfile::tempdir().unwrap();
    publish(c.idx.store(), dir.path());
    let pack = Pack::open(dir.path(), "fake@4", 4).unwrap();
    assert!(pack.has_graph());

    let mut seed_sets: Vec<Vec<String>> = c.chunks.iter().map(|id| vec![id.clone()]).collect();
    for (i, a) in c.chunks.iter().enumerate() {
        let b = &c.chunks[(i * 7 + 3) % c.chunks.len()];
        let d = &c.chunks[(i * 11 + 5) % c.chunks.len()];
        seed_sets.push(vec![a.clone(), b.clone(), d.clone()]);
    }
    seed_sets.push(vec![Chunk::id(&c.long.id, 9999)]);
    seed_sets.push(vec![]);

    let mut nonempty = 0;
    for seeds in &seed_sets {
        for (hops, max) in SETTINGS {
            let from_store = ids_from_store(c.idx.store(), seeds, hops, max);
            let from_pack = ids_from_pack(&pack, seeds, hops, max);
            assert_eq!(
                from_pack, from_store,
                "seeds {seeds:?}, hops {hops}, up to {max}"
            );
            nonempty += usize::from(!from_store.is_empty());
        }
    }
    assert!(
        nonempty > seed_sets.len(),
        "the fixture must exercise real expansion"
    );
}

#[test]
fn the_pack_reaches_two_hops_out_and_back_round_a_cycle() {
    let c = corpus();
    let dir = tempfile::tempdir().unwrap();
    publish(c.idx.store(), dir.path());
    let pack = Pack::open(dir.path(), "fake@4", 4).unwrap();
    let seed = Chunk::id(&c.linked.id, 0);

    let one = ids_from_pack(&pack, std::slice::from_ref(&seed), 1, 12);
    let two = ids_from_pack(&pack, std::slice::from_ref(&seed), 2, 12);
    let long_first = Chunk::id(&c.long.id, 0);
    assert!(
        !one.contains(&long_first),
        "one hop must not reach long.md: {one:?}"
    );
    assert!(
        two.contains(&long_first),
        "two hops must reach long.md: {two:?}"
    );
}

#[test]
fn a_pack_with_a_graph_serves_tier_two_with_no_store() {
    let c = corpus();
    let dir = tempfile::tempdir().unwrap();
    publish(c.idx.store(), dir.path());
    let pack = Arc::new(Pack::open(dir.path(), "fake@4", 4).unwrap());

    let retriever = rhebok::retrieve_from_opened(
        &Config::default(),
        Surface::Hook,
        &Profile::tier(2),
        Box::new(common::FakeEmbedder {
            calls: Arc::new(AtomicUsize::new(0)),
            queries: Arc::new(AtomicUsize::new(0)),
        }),
        Some(pack),
        Ok(None),
    )
    .unwrap();
    assert!(
        retriever.store().is_none(),
        "tier 2 must not open the database"
    );

    let explain = retriever
        .search_explained("filler words here", &Profile::tier(2), 0.0, 1_000_000)
        .unwrap();
    let graph = explain
        .stages
        .iter()
        .find(|s| s.name == "graph")
        .expect("the graph stage must run from the pack");
    assert!(
        !graph.hits.is_empty(),
        "the pack graph must contribute neighbours"
    );
}

#[test]
fn a_pack_without_a_graph_refuses_to_expand_rather_than_returning_nothing() {
    let c = corpus();
    let dir = tempfile::tempdir().unwrap();
    publish(c.idx.store(), dir.path());
    std::fs::remove_file(dir.path().join(graph::GRAPH_FILE)).unwrap();
    let pack = Arc::new(Pack::open(dir.path(), "fake@4", 4).unwrap());
    assert!(!pack.has_graph());
    assert!(pack.expand(&[c.chunks[0].clone()], 1, 5).is_err());

    let retriever = rhebok::retrieve::Retriever::packed(
        pack,
        Box::new(common::FakeEmbedder {
            calls: Arc::new(AtomicUsize::new(0)),
            queries: Arc::new(AtomicUsize::new(0)),
        }),
        "http://localhost:11434".into(),
    );
    let err = retriever
        .search_explained("filler words here", &Profile::tier(2), 0.0, 1_000_000)
        .unwrap_err();
    assert!(
        format!("{err:#}").contains("pack.graph"),
        "the refusal must name the missing file: {err:#}"
    );
}

#[test]
fn a_graph_from_another_generation_is_ignored_not_trusted() {
    let c = corpus();
    let dir = tempfile::tempdir().unwrap();
    let ids = publish(c.idx.store(), dir.path());

    let other = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(other.path()).unwrap();
    let mut shorter = c.idx.store().graph_for_pack().unwrap();
    let dropped = ids[0].clone();
    shorter.chunks.retain(|(id, _, _)| *id != dropped);
    graph::write(other.path(), &ids[1..], &shorter).unwrap();
    std::fs::copy(
        other.path().join(graph::GRAPH_FILE),
        dir.path().join(graph::GRAPH_FILE),
    )
    .unwrap();

    let pack = Pack::open(dir.path(), "fake@4", 4).unwrap();
    assert!(
        !pack.has_graph(),
        "a graph over a different row count must not be joined to these rows"
    );
}

#[test]
fn a_corrupt_edge_fails_the_expansion_loudly() {
    let c = corpus();
    let dir = tempfile::tempdir().unwrap();
    publish(c.idx.store(), dir.path());
    let path = dir.path().join(graph::GRAPH_FILE);
    let mut bytes = std::fs::read(&path).unwrap();
    let header =
        |i: usize| u32::from_le_bytes(bytes[8 + 4 * i..12 + 4 * i].try_into().unwrap()) as usize;
    let (rows, docs, adjacent) = (header(0), header(1), header(2));
    assert!(adjacent > 0, "the fixture must have adjacency to corrupt");
    let first_adjacent = 24 + 4 * (3 * rows + docs + rows + 1);
    bytes[first_adjacent..first_adjacent + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    std::fs::write(&path, &bytes).unwrap();

    let pack = Pack::open(dir.path(), "fake@4", 4).unwrap();
    let every_chunk: Vec<String> = c.chunks.clone();
    assert!(
        pack.expand(&every_chunk, 2, 1000).is_err(),
        "an edge pointing past the end must be an error, never a wrong neighbour"
    );
}

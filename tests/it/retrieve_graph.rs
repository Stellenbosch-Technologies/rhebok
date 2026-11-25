use crate::common;
use rhebok::model::{Chunk, Document, SourceType};

#[test]
fn expansion_pulls_adjacent_chunks_replacing_text_overlap() {
    let (_d, idx, _calls) = common::setup();
    let body = format!(
        "# T\n\n{}\n\n## Second\n\n{}",
        "alpha ".repeat(400),
        "beta ".repeat(400)
    );
    let doc = Document::new(SourceType::Markdown, "file:///a.md", "T", &body);
    idx.index_documents(std::slice::from_ref(&doc)).unwrap();

    let seed = rhebok::model::Chunk::id(&doc.id, 0);
    let expanded = idx
        .store()
        .expand(std::slice::from_ref(&seed), 1, 5)
        .unwrap();

    assert!(expanded
        .iter()
        .any(|h| h.chunk_id == rhebok::model::Chunk::id(&doc.id, 1)));
    assert!(
        !expanded.iter().any(|h| h.chunk_id == seed),
        "seeds are not re-returned"
    );
}

#[test]
fn expansion_follows_wikilinks_to_other_documents() {
    let (_d, idx, _calls) = common::setup();
    let mut a = Document::new(SourceType::Markdown, "file:///a.md", "A", "See [[b]].");
    a.links = vec!["b".into()];
    let b = Document::new(
        SourceType::Markdown,
        "file:///b.md",
        "b",
        "The answer is 42.",
    );
    idx.index_documents(&[a.clone(), b.clone()]).unwrap();
    idx.resolve_links(&[a.clone(), b.clone()]).unwrap();

    let expanded = idx
        .store()
        .expand(&[rhebok::model::Chunk::id(&a.id, 0)], 1, 5)
        .unwrap();
    assert!(
        expanded.iter().any(|h| h.doc_id == b.id),
        "linked note must be reachable"
    );
}

#[test]
fn adjacent_neighbours_are_ordered_numerically_not_lexicographically() {
    // chunk_id is "{doc_id}:{ord}", so ORDER BY c.id string-sorts the ordinal:
    // "…:10" lands before "…:8" because '1' < '8'. RRF derives rank from list
    // position, so that silently promotes a distant chunk over an adjacent one.
    let (_d, idx, _c) = common::setup();
    let mut body = String::from("# Doc\n\n");
    for i in 0..12 {
        body.push_str(&format!(
            "## Section {i}\n\n{}\n\n",
            "filler words here. ".repeat(90)
        ));
    }
    let doc = Document::new(SourceType::Markdown, "file:///big.md", "Big", &body);
    idx.index_documents(std::slice::from_ref(&doc)).unwrap();

    let hits = idx.store().expand(&[Chunk::id(&doc.id, 9)], 1, 8).unwrap();
    let ords: Vec<i64> = hits
        .iter()
        .filter_map(|h| h.chunk_id.rsplit(':').next()?.parse().ok())
        .collect();
    let mut sorted = ords.clone();
    sorted.sort_unstable();
    assert_eq!(
        ords, sorted,
        "neighbour ordinals must be numerically ordered, got {ords:?}"
    );
}

#[test]
fn linked_neighbours_are_not_crowded_out_by_adjacent_ones() {
    // Each traversal has its own LIMIT. Concatenating then truncating meant a
    // long document's adjacent chunks could consume the whole budget and drop
    // every wikilinked note.
    let (_d, idx, _c) = common::setup();
    let mut body = String::from("# Long\n\n");
    for i in 0..10 {
        body.push_str(&format!(
            "## S{i}\n\n{}\n\n",
            "filler words here. ".repeat(90)
        ));
    }
    body.push_str("See [[target]].");
    let mut long = Document::new(SourceType::Markdown, "file:///long.md", "Long", &body);
    long.links = vec!["target".into()];
    let target = Document::new(
        SourceType::Markdown,
        "file:///target.md",
        "target",
        "# Target\n\nThe answer.",
    );

    let docs = vec![long.clone(), target.clone()];
    idx.index_documents(&docs).unwrap();
    idx.resolve_links(&docs).unwrap();

    let hits = idx.store().expand(&[Chunk::id(&long.id, 4)], 1, 2).unwrap();
    assert!(
        hits.iter().any(|h| h.doc_id == target.id),
        "a linked document must survive a budget that adjacency alone could fill"
    );
}

#[test]
fn expansion_respects_max_neighbors() {
    let (_d, idx, _calls) = common::setup();
    let body = format!("# T\n\n{}", "word ".repeat(4000));
    let doc = Document::new(SourceType::Markdown, "file:///a.md", "T", &body);
    idx.index_documents(std::slice::from_ref(&doc)).unwrap();
    let seed = rhebok::model::Chunk::id(&doc.id, 0);
    assert!(idx.store().expand(&[seed], 2, 2).unwrap().len() <= 2);
}

#[test]
fn expansion_of_nothing_is_empty_not_an_error() {
    let (_d, idx, _calls) = common::setup();
    assert!(idx.store().expand(&[], 1, 5).unwrap().is_empty());
}

/// Expansion follows the links a human wrote, not the lifecycle pointers the
/// indexer materialises.
///
/// `expand` has a `LIMIT max_neighbors`. A `superseded-by` edge that consumes
/// one of those slots displaces a neighbour worth reading — and it points at a
/// record the vault has explicitly retired.
///
/// Distinct from the demotion in `LifecycleSource`, which stops such a hit
/// being injected undemoted once it is in the pool. This stops it entering the
/// pool at all. Both are wanted: a superseded document reached by an ordinary
/// wikilink should still be expanded to, and still demoted.
#[test]
fn expansion_follows_wikilinks_and_not_lifecycle_edges() {
    let (_d, idx, _calls) = common::setup();

    let live = Document::new(
        SourceType::Markdown,
        "file:///new.md",
        "Maven",
        "The current decision.",
    );
    let dead = Document::new(
        SourceType::Markdown,
        "file:///old.md",
        "Ant",
        "The retired decision.",
    );
    let cited = Document::new(
        SourceType::Markdown,
        "file:///tooling.md",
        "Tooling",
        "A note the live record links to.",
    );
    idx.index_documents(&[live.clone(), dead.clone(), cited.clone()])
        .unwrap();

    // The live record links to `tooling` in prose AND declares that it
    // supersedes `old`. Expansion must reach the first and not the second.
    idx.store()
        .link_documents(&live.id, &cited.id, "wikilink")
        .unwrap();
    idx.store()
        .link_documents(&live.id, &dead.id, "supersedes")
        .unwrap();

    let seed = rhebok::model::Chunk::id(&live.id, 0);
    let expanded = idx
        .store()
        .expand(std::slice::from_ref(&seed), 1, 10)
        .unwrap();
    let reached: std::collections::HashSet<String> =
        expanded.iter().map(|h| h.doc_id.clone()).collect();

    assert!(
        reached.contains(&cited.id),
        "the prose wikilink must still be followed; reached {reached:?}"
    );
    assert!(
        !reached.contains(&dead.id),
        "a `supersedes` edge is a tombstone pointer, not a link a reader would \
         click — following it spends a neighbour slot on a retired record; \
         reached {reached:?}"
    );

    // And the OTHER direction, which is not symmetric with it. Seeding from the
    // RETIRED record must still reach its replacement: `superseded-by` sits on
    // the dead document and points at the live one, and landing on a dead
    // decision and being shown the current one is the whole point.
    //
    // The first cut of this filter admitted `wikilink` alone, which passed the
    // two assertions above while quietly removing this traversal. An ADR may
    // declare `superseded-by:` in frontmatter without also writing the prose
    // link, and for those the replacement becomes unreachable — `LifecycleSource`
    // cannot compensate, because it demotes what is in the pool and an
    // unreachable document is absent rather than demoted.
    idx.store()
        .link_documents(&dead.id, &live.id, "superseded-by")
        .unwrap();
    let from_dead = rhebok::model::Chunk::id(&dead.id, 0);
    let reached_from_dead: std::collections::HashSet<String> = idx
        .store()
        .expand(std::slice::from_ref(&from_dead), 1, 10)
        .unwrap()
        .iter()
        .map(|h| h.doc_id.clone())
        .collect();
    assert!(
        reached_from_dead.contains(&live.id),
        "`superseded-by` points from the retired record AT its replacement — \
         dropping it makes the live record unreachable from the dead one; \
         reached {reached_from_dead:?}"
    );
}

const EXPAND_PROBE_STORE: &str = "RHEBOK_EXPAND_PROBE_STORE";
const BULKY_DOCUMENTS: usize = 64;
const BULKY_CHUNKS_PER_DOCUMENT: usize = 8;
const BULKY_CHUNK_BYTES: usize = 128 * 1024;
const BULKY_TEXT_BYTES: u64 =
    (BULKY_DOCUMENTS * BULKY_CHUNKS_PER_DOCUMENT * BULKY_CHUNK_BYTES) as u64;

fn expand_probe_seed() -> String {
    Chunk::id(&Document::new_id("file:///seed.md"), 0)
}

fn write_a_seed_beside_unrelated_documents(idx: &rhebok::index::Indexer, chunk_bytes: usize) {
    let mut seed = Document::new(
        SourceType::Markdown,
        "file:///seed.md",
        "Seed",
        &format!(
            "# Seed\n\n{}\n\n## Second\n\n{}\n\nSee [[target]].",
            "alpha ".repeat(400),
            "beta ".repeat(400)
        ),
    );
    seed.links = vec!["target".into()];
    let target = Document::new(
        SourceType::Markdown,
        "file:///target.md",
        "target",
        "The answer is 42.",
    );
    idx.index_documents(&[seed.clone(), target.clone()])
        .unwrap();
    idx.resolve_links(&[seed, target]).unwrap();

    idx.store()
        .in_transaction(|store| {
            for d in 0..BULKY_DOCUMENTS {
                let doc = Document::new(
                    SourceType::Markdown,
                    &format!("file:///bulky-{d}.md"),
                    &format!("Bulky {d}"),
                    "",
                );
                store.upsert_document(&doc)?;
                let chunks: Vec<Chunk> = (0..BULKY_CHUNKS_PER_DOCUMENT as i64)
                    .map(|ord| Chunk {
                        id: Chunk::id(&doc.id, ord),
                        doc_id: doc.id.clone(),
                        ord,
                        text: "x".repeat(chunk_bytes),
                        embed_text: format!("bulky {d} {ord}"),
                        heading_path: String::new(),
                        page_no: None,
                    })
                    .collect();
                let vecs = vec![vec![0.5_f32; 4]; chunks.len()];
                store.replace_chunks(&doc.id, &chunks, &vecs)?;
            }
            Ok(())
        })
        .unwrap();
}

fn peak_growth_of_one_expansion(chunk_bytes: usize) -> u64 {
    let (dir, idx, _calls) = common::setup();
    write_a_seed_beside_unrelated_documents(&idx, chunk_bytes);
    drop(idx);

    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "retrieve_graph::expand_footprint_probe",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(EXPAND_PROBE_STORE, dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "the probe failed: {stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
        .lines()
        .find_map(|l| l.split_once("expand peak growth: ").map(|(_, v)| v))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or_else(|| panic!("the probe printed no measurement: {stdout}"))
}

#[test]
fn graph_expansion_does_not_read_the_text_of_chunks_it_does_not_return() {
    let small = peak_growth_of_one_expansion(16);
    let bulky = peak_growth_of_one_expansion(BULKY_CHUNK_BYTES);
    let grown_by_text = bulky.saturating_sub(small);
    assert!(
        grown_by_text < BULKY_TEXT_BYTES / 4,
        "one expansion peaked {grown_by_text} bytes higher when documents the seed never reaches \
         held {BULKY_TEXT_BYTES} bytes of chunk text ({small} bytes with tiny chunks, {bulky} with \
         bulky ones)"
    );
}

#[test]
#[ignore = "child process of graph_expansion_does_not_read_the_text_of_chunks_it_does_not_return"]
fn expand_footprint_probe() {
    let Some(dir) = std::env::var_os(EXPAND_PROBE_STORE) else {
        return;
    };
    let store = rhebok::store::Store::open_existing(std::path::Path::new(&dir), 4).unwrap();
    let before = rhebok::bench::synthetic::peak_rss_bytes();
    let hits = store.expand(&[expand_probe_seed()], 2, 12).unwrap();
    let grown = rhebok::bench::synthetic::peak_rss_bytes().saturating_sub(before);
    let reached: std::collections::HashSet<String> = hits.iter().map(|h| h.uri.clone()).collect();
    assert!(
        reached.contains("file:///target.md") && !reached.iter().any(|u| u.contains("bulky")),
        "the probe must reach the seed's own neighbours and nothing else; reached {reached:?}"
    );
    println!("expand peak growth: {grown}");
}

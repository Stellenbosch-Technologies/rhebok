use crate::common;
use rhebok::bench::footprint::{in_pack, Scanner, WHOLE_SESSION_QUOTES};
use rhebok::config::Profile;
use rhebok::pack::records::Record;
use rhebok::pack::Pack;
use rhebok::retrieve::Retriever;
use std::collections::BTreeSet;

const QUERY: &str = "how do I tie a bowline knot";

fn record(chunk_id: &str, doc_id: &str, source_type: &str, text: &str) -> Record {
    Record {
        chunk_id: chunk_id.into(),
        doc_id: doc_id.into(),
        text: text.into(),
        heading_path: String::new(),
        uri: format!("file:///n/{doc_id}"),
        title: doc_id.into(),
        page_no: None,
        source_type: source_type.into(),
        inbound: 0,
        lifecycle: Default::default(),
        last_used: None,
        memory: None,
    }
}

fn queries() -> Vec<String> {
    let mut q: Vec<String> = (0..WHOLE_SESSION_QUOTES - 1)
        .map(|i| format!("golden question number {i}"))
        .collect();
    q.push(QUERY.into());
    q
}

fn rows() -> Vec<Record> {
    let all_but_one = queries()[..WHOLE_SESSION_QUOTES - 1].join("\n");
    vec![
        record("author:0", "author", "transcript", &all_but_one),
        record(
            "author:1",
            "author",
            "transcript",
            "Golden question\nnumber 0 and How do I tie a bowline knot",
        ),
        record("author:2", "author", "transcript", "we spoke about rope"),
        record(
            "note:0",
            "note",
            "markdown",
            "How do I tie a bowline knot? Like this.",
        ),
        record(
            "other:0",
            "other",
            "transcript",
            "a bowline is a knot you tie",
        ),
        record("session:0", "session", "transcript", "we spoke about rope"),
        record(
            "session:1",
            "session",
            "transcript",
            "probe.sh 1 \"How do I   tie a\nBowline knot\"",
        ),
    ]
}

fn scan(queries: &[String]) -> rhebok::bench::footprint::Footprint {
    let mut scanner = Scanner::new(queries).unwrap();
    for r in rows() {
        scanner.push(&r.doc_id, &r.source_type, &r.text);
    }
    scanner.finish()
}

fn build(dir: &std::path::Path) -> Pack {
    let data = rows()
        .into_iter()
        .map(|r| (r, common::hash_vec(QUERY)))
        .collect();
    Pack::build(
        dir,
        "fake@4",
        4,
        data,
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    Pack::open(dir, "fake@4", 4).unwrap()
}

#[test]
fn a_session_quoting_enough_golden_queries_is_left_out_whole() {
    let found = scan(&queries());
    assert_eq!(found.sessions, BTreeSet::from(["author".to_string()]));
    assert!(
        found.rows.is_superset(&BTreeSet::from([0, 1, 2])),
        "{:?}",
        found.rows
    );
}

#[test]
fn a_session_quoting_one_golden_query_loses_only_that_chunk() {
    let found = scan(&queries());
    assert!(found.rows.contains(&6), "{:?}", found.rows);
    assert!(!found.rows.contains(&5), "{:?}", found.rows);
    assert!(!found.sessions.contains("session"));
}

#[test]
fn a_note_quoting_a_golden_query_is_never_left_out() {
    assert!(!scan(&queries()).rows.contains(&3));
}

#[test]
fn a_query_shorter_than_three_words_marks_nothing() {
    assert!(scan(&["bowline knot".to_string()]).rows.is_empty());
}

#[test]
fn the_pack_scan_finds_the_same_rows_as_the_scanner() {
    let dir = tempfile::tempdir().unwrap();
    let pack = build(dir.path());
    let found = in_pack(&pack, &queries()).unwrap();
    let rows: BTreeSet<usize> = ["author:0", "author:1", "author:2", "session:1"]
        .iter()
        .map(|c| pack.row_of(c).unwrap())
        .collect();
    assert_eq!(found.rows, rows);
}

#[test]
fn a_retriever_excluding_the_footprint_never_returns_it() {
    let dir = tempfile::tempdir().unwrap();
    let pack = build(dir.path());
    let found = in_pack(&pack, &queries()).unwrap();
    let chunks = |r: &Retriever| -> BTreeSet<String> {
        r.search(QUERY, &Profile::tier(1))
            .unwrap()
            .into_iter()
            .map(|h| h.chunk_id)
            .collect()
    };
    let open_dir = tempfile::tempdir().unwrap();
    let open = Retriever::packed(
        build(open_dir.path()),
        Box::new(common::FakeEmbedder::default()),
        "http://127.0.0.1:1".into(),
    );
    let all = chunks(&open);
    assert!(
        all.contains("session:1") && all.contains("author:2"),
        "{all:?}"
    );

    let left_out = Retriever::packed(
        pack,
        Box::new(common::FakeEmbedder::default()),
        "http://127.0.0.1:1".into(),
    )
    .excluding_rows(&found.rows);
    let got = chunks(&left_out);
    for gone in ["author:0", "author:1", "author:2", "session:1"] {
        assert!(!got.contains(gone), "{gone} came back: {got:?}");
    }
    assert!(
        got.contains("note:0") && got.contains("session:0"),
        "{got:?}"
    );
}

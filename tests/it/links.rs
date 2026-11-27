use crate::common::setup;
use rhebok::loaders::links;
use rhebok::loaders::markdown::MarkdownLoader;
use rhebok::loaders::web::WebLoader;
use rhebok::model::{Document, SourceType};

fn file_uri(p: &std::path::Path) -> String {
    format!("file://{}", p.canonicalize().unwrap().display())
}

fn write(p: &std::path::Path, body: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

#[test]
fn a_markdown_note_records_the_files_its_ordinary_links_point_at() {
    let vault = tempfile::tempdir().unwrap();
    let root = vault.path();
    write(&root.join("plans/beta.md"), "# Beta\n");
    write(&root.join("specs/Design Doc.md"), "# Design\n");
    write(&root.join("papers/report.pdf"), "%PDF-1.4\n");
    write(&root.join("plans/gamma.md"), "# Gamma\n");
    write(
        &root.join("plans/alpha.md"),
        "# Alpha\n\n\
         See [beta](beta.md), [the design](../specs/Design%20Doc.md#goals),\n\
         [the paper](../papers/report.pdf) and [gamma](./gamma).\n\n\
         [missing](nowhere.md) [mail](mailto:a@b.c) [top](#alpha)\n\n\
         ```\n[in code](beta.md)\n```\n",
    );

    let doc = MarkdownLoader::load_file(&root.join("plans/alpha.md")).unwrap();

    assert_eq!(
        doc.link_uris,
        vec![
            file_uri(&root.join("plans/beta.md")),
            file_uri(&root.join("specs/Design Doc.md")),
            file_uri(&root.join("papers/report.pdf")),
            file_uri(&root.join("plans/gamma.md")),
        ]
    );
}

#[test]
fn web_addresses_in_prose_are_recorded_with_and_without_link_syntax() {
    let targets = links::in_markdown(
        "Read [the guide](https://Example.com/guide/#install) or \
         https://example.com/faq/, and <https://example.com/about>.",
        "file:///notes/a.md",
    );

    assert_eq!(
        targets,
        vec![
            "https://example.com/guide".to_string(),
            "https://example.com/faq".to_string(),
            "https://example.com/about".to_string(),
        ]
    );
}

#[test]
fn a_web_page_resolves_its_relative_links_against_its_own_address() {
    let html = r#"<html><head><title>Docs</title></head><body><article>
        <h1>Docs</h1>
        <p>This page explains the install. Continue to the
        <a href="../setup/next.html">next step</a> or the
        <a href="https://other.example.org/page">reference</a>. There is enough
        prose here for readability to keep the article body intact.</p>
        </article></body></html>"#;

    let doc = WebLoader::from_html("https://example.com/docs/intro.html", html).unwrap();

    assert!(
        doc.link_uris
            .contains(&"https://example.com/setup/next.html".to_string()),
        "{:?}",
        doc.link_uris
    );
    assert!(
        doc.link_uris
            .contains(&"https://other.example.org/page".to_string()),
        "{:?}",
        doc.link_uris
    );
}

#[test]
fn a_link_by_path_becomes_a_link_edge_to_the_indexed_document() {
    let (_d, idx, _c) = setup();
    let mut note = Document::new(
        SourceType::Markdown,
        "file:///vault/alpha.md",
        "Alpha",
        "See the paper.",
    );
    note.link_uris = vec![
        "file:///vault/papers/report.pdf".into(),
        "https://example.com/guide".into(),
        "file:///vault/not-indexed.md".into(),
        "file:///vault/alpha.md".into(),
    ];
    let pdf = Document::new(
        SourceType::Pdf,
        "file:///vault/papers/report.pdf",
        "Report",
        "Findings.",
    );
    let page = Document::new(
        SourceType::Web,
        "https://example.com/guide/",
        "Guide",
        "Install it.",
    );

    let docs = vec![note.clone(), pdf.clone(), page.clone()];
    idx.index_documents(&docs).unwrap();
    let edges = idx.resolve_links(&docs).unwrap();

    assert_eq!(edges, 2);
    let mut linked = idx.store().linked_docs(&note.id, "link").unwrap();
    linked.sort();
    let mut expected = vec![pdf.id.clone(), page.id.clone()];
    expected.sort();
    assert_eq!(linked, expected);
}

#[test]
fn a_pdf_that_links_to_a_note_counts_as_an_inbound_link_for_that_note() {
    let (_d, idx, _c) = setup();
    let mut pdf = Document::new(
        SourceType::Pdf,
        "file:///vault/report.pdf",
        "Report",
        "See the plan.",
    );
    pdf.link_uris = vec!["file:///vault/plan.md".into()];
    let plan = Document::new(
        SourceType::Markdown,
        "file:///vault/plan.md",
        "Plan",
        "Do it.",
    );

    let docs = vec![pdf, plan.clone()];
    idx.index_documents(&docs).unwrap();
    idx.resolve_links(&docs).unwrap();

    let inbound = idx.store().inbound_link_counts().unwrap();
    assert_eq!(inbound.get(&plan.id), Some(&1));
}

#[test]
fn a_wikilink_can_name_a_pdf_by_its_file_name_or_a_note_by_its_path() {
    let (_d, idx, _c) = setup();
    let mut note = Document::new(
        SourceType::Markdown,
        "file:///vault/alpha.md",
        "Alpha",
        "![[report.pdf]] and [[projects/beta]].",
    );
    note.links = vec!["report.pdf".into(), "projects/beta".into()];
    let pdf = Document::new(
        SourceType::Pdf,
        "file:///vault/papers/report.pdf",
        "Quarterly findings",
        "Findings.",
    );
    let beta = Document::new(
        SourceType::Markdown,
        "file:///vault/projects/beta.md",
        "Beta plan",
        "Plan.",
    );

    let docs = vec![note.clone(), pdf.clone(), beta.clone()];
    idx.index_documents(&docs).unwrap();
    idx.resolve_links(&docs).unwrap();

    let mut linked = idx.store().linked_docs(&note.id, "wikilink").unwrap();
    linked.sort();
    let mut expected = vec![pdf.id, beta.id];
    expected.sort();
    assert_eq!(linked, expected);
}

#[test]
fn graph_expansion_and_the_document_panel_follow_a_link_by_path() {
    let (_d, idx, _c) = setup();
    let mut note = Document::new(
        SourceType::Markdown,
        "file:///vault/alpha.md",
        "Alpha",
        "See the paper.",
    );
    note.link_uris = vec!["file:///vault/report.pdf".into()];
    let pdf = Document::new(
        SourceType::Pdf,
        "file:///vault/report.pdf",
        "Report",
        "The answer is 42.",
    );
    let docs = vec![note.clone(), pdf.clone()];
    idx.index_documents(&docs).unwrap();
    idx.resolve_links(&docs).unwrap();

    let expanded = idx
        .store()
        .expand(&[rhebok::model::Chunk::id(&note.id, 0)], 1, 5)
        .unwrap();
    assert!(expanded.iter().any(|h| h.doc_id == pdf.id), "{expanded:?}");

    let detail = idx.store().document_detail(&pdf.id).unwrap().unwrap();
    assert_eq!(detail.inbound, 1);
}

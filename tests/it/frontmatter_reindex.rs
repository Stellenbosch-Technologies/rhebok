use rhebok::config::Config;
use rhebok::embed::Embedder;
use rhebok::index::Indexer;
use rhebok::loaders::markdown::MarkdownLoader;
use rhebok::pack::status::Lifecycle;
use rhebok::store::Store;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct TextCounter(Arc<AtomicUsize>);

impl Embedder for TextCounter {
    fn embed_documents(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        self.0.fetch_add(texts.len(), Ordering::SeqCst);
        Ok(texts.iter().map(|t| crate::common::hash_vec(t)).collect())
    }
    fn embed_query(&self, t: &str) -> anyhow::Result<Vec<f32>> {
        Ok(crate::common::hash_vec(t))
    }
    fn warm(&self) -> anyhow::Result<()> {
        Ok(())
    }
    fn model_id(&self) -> String {
        "text-counter@4".into()
    }
    fn dimensions(&self) -> usize {
        4
    }
}

fn note(tags: &str, status: &str) -> String {
    format!(
        "---\ntitle: Pooling decision\ntags: [{tags}]\nid: ADR-0007\nstatus: {status}\nsuperseded-by: ADR-0009\n---\n\n# Pooling decision\n\nUse PgBouncer in transaction mode for every service.\n"
    )
}

#[test]
fn an_edit_to_frontmatter_alone_reaches_the_store_without_re_embedding() {
    let dir = tempfile::tempdir().unwrap();
    let embedded = Arc::new(AtomicUsize::new(0));
    let idx = Indexer::new(
        Store::open(&dir.path().join("db"), 4).unwrap(),
        Box::new(TextCounter(embedded.clone())),
        Config::default(),
    )
    .silent();
    let path = dir.path().join("adr.md");

    std::fs::write(&path, note("alpha", "accepted")).unwrap();
    let first = MarkdownLoader::load_file(&path).unwrap();
    idx.index_documents(std::slice::from_ref(&first)).unwrap();
    let embedded_first = embedded.load(Ordering::SeqCst);
    assert!(embedded_first > 0);

    std::fs::write(&path, note("beta, gamma", "superseded")).unwrap();
    let second = MarkdownLoader::load_file(&path).unwrap();
    assert_eq!(first.text, second.text, "the body must be untouched");
    let stats = idx.index_documents(std::slice::from_ref(&second)).unwrap();

    assert_eq!((stats.updated, stats.skipped), (1, 0));
    assert_eq!(
        embedded.load(Ordering::SeqCst),
        embedded_first,
        "no chunk's embed_text changed, so nothing is re-embedded"
    );

    let mut tags = idx
        .store()
        .document_tags()
        .unwrap()
        .remove(&second.id)
        .unwrap_or_default();
    tags.sort();
    assert_eq!(tags, ["beta", "gamma"]);

    let lifecycle = idx
        .store()
        .all_lifecycles(&Default::default())
        .unwrap()
        .remove(&second.id);
    assert_eq!(lifecycle, Some(Lifecycle::Superseded));
}

#[test]
fn each_frontmatter_field_alone_is_an_edit() {
    for (before, after) in [
        ("tags: [a]", "tags: [b]"),
        ("status: accepted", "status: superseded"),
        ("title: Old name", "title: New name"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let idx = Indexer::new(
            Store::open(&dir.path().join("db"), 4).unwrap(),
            Box::new(TextCounter(Arc::new(AtomicUsize::new(0)))),
            Config::default(),
        )
        .silent();
        let path = dir.path().join("note.md");
        let body = "\n\nConnection pooling notes.\n";

        std::fs::write(&path, format!("---\n{before}\n---{body}")).unwrap();
        idx.index_documents(&[MarkdownLoader::load_file(&path).unwrap()])
            .unwrap();
        std::fs::write(&path, format!("---\n{after}\n---{body}")).unwrap();
        let stats = idx
            .index_documents(&[MarkdownLoader::load_file(&path).unwrap()])
            .unwrap();

        assert_eq!(
            (stats.updated, stats.skipped),
            (1, 0),
            "{before} -> {after}"
        );
    }
}

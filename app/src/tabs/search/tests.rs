use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use gpui_kit::base::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    px, size, AppContext, Bounds, Entity, Focusable, Point, SharedString, TestAppContext,
    WindowBounds, WindowHandle, WindowOptions,
};
use rhebok::dashboard::core::document::Document;
use rhebok::dashboard::core::search::SearchOutcome;
use rhebok::dashboard::core::stats::{Stats, Tiers};
use rhebok::dashboard::core::DashboardError;
use rhebok::pack::status::Lifecycle;
use rhebok::retrieve::{Explain, HitSummary, StageTrace};
use rhebok::store::query::DocumentDetail;

use super::{card_id, subtitle_of, toggle_id, Search, OUTAGE, SUGGESTED};
use crate::backend::{self, FixtureBackend};
use crate::shell::{Shell, TabId};
use crate::theme;
use crate::version_gate::Gate;
use crate::widgets::document_facts::{indexed_at, status_note};

fn stats() -> Stats {
    Stats {
        documents: 4,
        chunks: 5,
        by_source: HashMap::new(),
        sessions_by_agent: BTreeMap::new(),
        model: None,
        skipped: Vec::new(),
        skipped_ignored: 0,
        bench: None,
        bench_provenance: None,
        bench_at: None,
        tiers: Tiers { hook: 1, mcp: 2 },
        hook_threshold: 0.68,
        memory: BTreeMap::new(),
    }
}

fn hit(chunk: &str, relevance: f32) -> HitSummary {
    HitSummary {
        chunk_id: chunk.to_string(),
        doc_id: format!("doc-{chunk}"),
        title: format!("Title {chunk}"),
        heading: String::new(),
        source_type: "markdown".to_string(),
        relevance,
        score: 0.0,
        excerpt: format!("excerpt of {chunk}"),
        memory_kind: None,
    }
}

fn hits(prefix: &str, n: usize, relevance: f32) -> Vec<HitSummary> {
    (0..n)
        .map(|i| hit(&format!("{prefix}{i}"), relevance))
        .collect()
}

fn explain(
    vector: Vec<HitSummary>,
    bm25: Vec<HitSummary>,
    graph: Vec<HitSummary>,
    pool: Vec<HitSummary>,
    fused: Vec<HitSummary>,
    injected: &[&str],
) -> Explain {
    Explain {
        stages: vec![
            StageTrace {
                name: "vector",
                hits: vector,
            },
            StageTrace {
                name: "bm25",
                hits: bm25,
            },
            StageTrace {
                name: "graph",
                hits: graph,
            },
            StageTrace {
                name: "fused",
                hits: pool,
            },
        ],
        fused,
        threshold: 0.68,
        injected: injected.iter().map(|s| s.to_string()).collect(),
        degraded: false,
        bm25_unavailable: None,
        vectors_unavailable: None,
        memory_unavailable: None,
        elapsed_ms: 87,
    }
}

fn answer(ex: Explain) -> SearchOutcome {
    SearchOutcome {
        error: None,
        explain: Some(ex),
    }
}

fn nothing() -> SearchOutcome {
    answer(explain(
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &[],
    ))
}

fn detail() -> DocumentDetail {
    DocumentDetail {
        id: "doc-v0".to_string(),
        uri: "file:///vault/notes/sourdough.md".to_string(),
        title: "Sourdough starter".to_string(),
        source_type: "markdown".to_string(),
        indexed_at: 1_763_830_000,
        chunks: 3,
        inbound: 2,
        status: Some("proposed".to_string()),
        lifecycle: Lifecycle::Investigating,
    }
}

fn fixture() -> Arc<FixtureBackend> {
    let fixture = FixtureBackend::default();
    fixture.stats.set(Ok(stats()));
    Arc::new(fixture)
}

fn open(
    cx: &mut TestAppContext,
    fixture: Arc<FixtureBackend>,
) -> (WindowHandle<Root>, Entity<Shell>) {
    let (window, shell) = cx.update(|cx| {
        if !cx.has_global::<theme::Tokens>() {
            theme::install(cx);
        }
        backend::install(cx, fixture);
        let (window, shell) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(1400.), px(900.)),
                })),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|cx| Shell::new(Gate::Ok, cx)),
        )
        .expect("open test window");
        (window.downcast::<Root>().expect("Base Root"), shell)
    });
    cx.run_until_parked();
    click(cx, window, TabId::Search.button_id());
    (window, shell)
}

fn search(cx: &mut TestAppContext, shell: &Entity<Shell>) -> Entity<Search> {
    shell
        .read_with(cx, |s, _| s.search.clone())
        .expect("search opened")
}

fn click(cx: &mut TestAppContext, window: WindowHandle<Root>, id: impl Into<SharedString>) {
    let id = id.into();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn label(cx: &mut TestAppContext, window: WindowHandle<Root>, id: &str) -> Option<String> {
    let id = SharedString::from(id.to_string());
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window
            .try_find(id)
            .map(|e| e.label().unwrap_or_default().to_string())
    })
    .unwrap()
}

fn description(cx: &mut TestAppContext, window: WindowHandle<Root>, id: &str) -> Option<String> {
    let id = SharedString::from(id.to_string());
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.find(id).description().map(str::to_string)
    })
    .unwrap()
}

fn top(cx: &mut TestAppContext, window: WindowHandle<Root>, id: &str) -> f32 {
    let id = SharedString::from(id.to_string());
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        f32::from(window.find(id).bounds().origin.y)
    })
    .unwrap()
}

fn expanded(cx: &mut TestAppContext, window: WindowHandle<Root>, id: &str) -> Option<bool> {
    let id = SharedString::from(id.to_string());
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.find(id).expanded()
    })
    .unwrap()
}

fn type_and_enter(cx: &mut TestAppContext, window: WindowHandle<Root>, text: &str) {
    click(cx, window, "search-input");
    cx.update_window(window.into(), |_, window, cx| {
        window.input(text, cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn card(column: &str, section: u8, chunk: &str) -> String {
    card_id(column, section, chunk).to_string()
}

#[gpui_kit::test]
fn the_five_columns_cap_at_six_cards_and_count_the_rest(cx: &mut TestAppContext) {
    let fixture = fixture();
    let fused = hits("f", 8, 0.9);
    let injected: Vec<String> = fused.iter().map(|h| h.chunk_id.clone()).collect();
    let injected: Vec<&str> = injected.iter().map(String::as_str).collect();
    fixture.search.set(Ok(answer(explain(
        hits("v", 8, 0.8),
        hits("b", 7, 0.0),
        hits("g", 6, 0.0),
        hits("f", 9, 0.9),
        fused,
        &injected,
    ))));
    let (window, _) = open(cx, fixture);
    type_and_enter(cx, window, "sourdough");

    for (column, count, prefix, more) in [
        ("vector", "8", "v", Some("+ 2 more")),
        ("bm25", "7", "b", Some("+ 1 more")),
        ("graph", "6", "g", None),
        ("fused", "9", "f", Some("+ 3 more")),
        ("injected", "8", "f", Some("+ 2 more")),
    ] {
        assert_eq!(
            label(cx, window, &format!("column-{column}-count")).as_deref(),
            Some(count),
            "{column} count"
        );
        let total: usize = count.parse().unwrap();
        for i in 0..total {
            let shown = label(cx, window, &card(column, 0, &format!("{prefix}{i}"))).is_some();
            assert_eq!(shown, i < 6, "{column} card {i}");
        }
        assert_eq!(
            label(cx, window, &format!("more-{column}-0")).as_deref(),
            more,
            "{column} overflow line"
        );
    }
    assert_eq!(
        label(cx, window, "column-fused").as_deref(),
        Some("fused · weighted")
    );
    assert_eq!(
        label(cx, window, "search-elapsed").as_deref(),
        Some("87 ms")
    );
}

#[gpui_kit::test]
fn the_fused_column_splits_at_the_gate(cx: &mut TestAppContext) {
    let fixture = fixture();
    let pool = vec![
        hit("f0", 0.9),
        hit("f1", 0.7),
        hit("f2", 0.68),
        hit("f3", 0.5),
        hit("f4", 0.3),
    ];
    fixture.search.set(Ok(answer(explain(
        Vec::new(),
        Vec::new(),
        Vec::new(),
        pool,
        Vec::new(),
        &[],
    ))));
    let (window, _) = open(cx, fixture);
    type_and_enter(cx, window, "sourdough");

    assert_eq!(
        label(cx, window, "divider-fused").as_deref(),
        Some("gate 0.68")
    );
    for chunk in ["f0", "f1", "f2"] {
        assert_eq!(
            description(cx, window, &card("fused", 0, chunk)),
            None,
            "{chunk}"
        );
    }
    for chunk in ["f3", "f4"] {
        assert_eq!(
            description(cx, window, &card("fused", 1, chunk)).as_deref(),
            Some("dimmed"),
            "{chunk}"
        );
    }
    let rule = top(cx, window, "divider-fused");
    assert!(top(cx, window, &card("fused", 0, "f2")) < rule);
    assert!(rule < top(cx, window, &card("fused", 1, "f3")));
    assert_eq!(label(cx, window, "divider-injected"), None);
}

#[gpui_kit::test]
fn injected_cards_are_accent_bordered_in_both_columns(cx: &mut TestAppContext) {
    let fixture = fixture();
    let above = vec![hit("f0", 0.9), hit("f1", 0.8), hit("f2", 0.75)];
    fixture.search.set(Ok(answer(explain(
        Vec::new(),
        Vec::new(),
        Vec::new(),
        above.clone(),
        above,
        &["f0", "f1"],
    ))));
    let (window, _) = open(cx, fixture);
    type_and_enter(cx, window, "sourdough");

    for chunk in ["f0", "f1"] {
        assert_eq!(
            description(cx, window, &card("fused", 0, chunk)).as_deref(),
            Some("injected")
        );
        assert_eq!(
            description(cx, window, &card("injected", 0, chunk)).as_deref(),
            Some("injected")
        );
    }
    assert_eq!(description(cx, window, &card("fused", 0, "f2")), None);
    assert_eq!(
        label(cx, window, "column-injected-count").as_deref(),
        Some("2")
    );
    assert_eq!(
        label(cx, window, "divider-injected").as_deref(),
        Some("token budget")
    );
    assert_eq!(
        description(cx, window, &card("injected", 1, "f2")).as_deref(),
        Some("dimmed")
    );
}

#[gpui_kit::test]
fn an_embedding_outage_shows_the_ollama_text(cx: &mut TestAppContext) {
    let fixture = fixture();
    fixture.search.set(Ok(SearchOutcome {
        error: Some("embedding unavailable".to_string()),
        explain: None,
    }));
    let (window, _) = open(cx, fixture.clone());
    type_and_enter(cx, window, "sourdough");
    assert_eq!(label(cx, window, "search-error").as_deref(), Some(OUTAGE));
    assert_eq!(
        OUTAGE,
        "Ollama is not reachable — search needs the embedding model. Graph and Health still work."
    );
    assert_eq!(label(cx, window, "search-results"), None);
    assert_eq!(label(cx, window, "search-elapsed"), None);
    assert_eq!(label(cx, window, "search-empty"), None);

    fixture.search.set(Ok(SearchOutcome {
        error: Some("this index predates the retrieval pack".to_string()),
        explain: None,
    }));
    click(cx, window, "search-button");
    assert_eq!(
        label(cx, window, "search-error").as_deref(),
        Some("this index predates the retrieval pack")
    );

    fixture
        .search
        .set(Err(DashboardError::Unavailable("store busy".to_string())));
    click(cx, window, "search-button");
    assert_eq!(
        label(cx, window, "search-error").as_deref(),
        Some("The index is busy — try again in a moment.")
    );
}

#[gpui_kit::test]
fn expanding_a_card_fetches_its_document_once(cx: &mut TestAppContext) {
    let fixture = fixture();
    fixture.search.set(Ok(answer(explain(
        vec![hit("v0", 0.71)],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &[],
    ))));
    fixture.document.set(Ok(Document::Found(detail())));
    let (window, _) = open(cx, fixture.clone());
    type_and_enter(cx, window, "sourdough");
    let card = card("vector", 0, "v0");
    let toggle = toggle_id(&card).to_string();
    assert_eq!(expanded(cx, window, &toggle), Some(false));
    assert_eq!(fixture.document.calls(), 0);

    click(cx, window, toggle.clone());
    assert_eq!(expanded(cx, window, &toggle), Some(true));
    for _ in 0..3 {
        assert_eq!(
            label(cx, window, "document-facts").as_deref(),
            Some("file:///vault/notes/sourdough.md")
        );
    }
    assert_eq!(fixture.document.calls(), 1);
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|r| r.starts_with("document"))
            .collect::<Vec<_>>(),
        ["document doc-v0"]
    );
    assert_eq!(
        label(cx, window, &format!("{card}-chunk")).as_deref(),
        Some("v0 · above the 0.68 gate (pre-weight)")
    );
    assert_eq!(
        label(cx, window, &format!("{card}-excerpt")).as_deref(),
        Some("excerpt of v0")
    );

    click(cx, window, toggle.clone());
    assert_eq!(expanded(cx, window, &toggle), Some(false));
    assert_eq!(label(cx, window, "document-facts"), None);
    assert_eq!(fixture.document.calls(), 1);
}

#[gpui_kit::test]
fn a_bm25_card_shows_its_raw_score_not_a_bar(cx: &mut TestAppContext) {
    let fixture = fixture();
    let mut keyword = hit("b0", 0.0);
    keyword.score = 6.866;
    fixture.search.set(Ok(answer(explain(
        vec![hit("v0", 0.6875)],
        vec![keyword],
        vec![hit("g0", 0.0)],
        Vec::new(),
        Vec::new(),
        &[],
    ))));
    let (window, _) = open(cx, fixture);
    type_and_enter(cx, window, "sourdough");
    let bm25 = card("bm25", 0, "b0");
    assert_eq!(
        label(cx, window, &format!("{bm25}-score")).as_deref(),
        Some("bm25 6.87 raw score")
    );
    assert_eq!(label(cx, window, &format!("{bm25}-bar")), None);
    let graph = card("graph", 0, "g0");
    assert_eq!(
        label(cx, window, &format!("{graph}-score")).as_deref(),
        Some("graph neighbour no score of its own")
    );
    assert_eq!(label(cx, window, &format!("{graph}-bar")), None);
    let vector = card("vector", 0, "v0");
    assert_eq!(
        label(cx, window, &format!("{vector}-bar")).as_deref(),
        Some("0.688")
    );
    assert_eq!(label(cx, window, &format!("{vector}-score")), None);
}

#[gpui_kit::test]
fn a_new_search_drops_the_previous_one(cx: &mut TestAppContext) {
    let fixture = fixture();
    fixture.search.set(Ok(answer(explain(
        vec![hit("v0", 0.7)],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &[],
    ))));
    let (window, shell) = open(cx, fixture.clone());
    let search = search(cx, &shell);
    search.update(cx, |s, cx| {
        s.submit("first".to_string(), cx);
        s.submit("second".to_string(), cx);
    });
    cx.run_until_parked();
    assert_eq!(fixture.search.calls(), 1, "the dropped search still ran");
    assert_eq!(fixture.requests(), ["search \"second\" tier=Some(1)"]);
    assert!(!search.read_with(cx, |s, _| s.busy()));
    assert!(label(cx, window, &card("vector", 0, "v0")).is_some());
}

#[gpui_kit::test]
fn enter_and_the_button_both_search_at_the_chosen_tier(cx: &mut TestAppContext) {
    let fixture = fixture();
    fixture.search.set(Ok(nothing()));
    let (window, shell) = open(cx, fixture.clone());
    assert_eq!(search(cx, &shell).read_with(cx, |s, _| s.tier()), 1);
    assert!(label(cx, window, "search-empty").is_some());
    click(cx, window, "search-button");
    assert_eq!(fixture.search.calls(), 0, "searched an empty query");

    type_and_enter(cx, window, "sourdough");
    click(cx, window, "search-tier-3");
    click(cx, window, "search-button");
    assert_eq!(
        fixture.requests(),
        [
            "search \"sourdough\" tier=Some(1)",
            "search \"sourdough\" tier=Some(3)"
        ]
    );
    assert_eq!(label(cx, window, "search-empty"), None);
}

#[gpui_kit::test]
fn a_search_highlights_the_fused_documents_on_the_shell(cx: &mut TestAppContext) {
    let fixture = fixture();
    let fused = vec![hit("f0", 0.9), hit("f1", 0.8)];
    fixture.search.set(Ok(answer(explain(
        Vec::new(),
        Vec::new(),
        Vec::new(),
        fused.clone(),
        fused,
        &["f0"],
    ))));
    let (window, shell) = open(cx, fixture);
    type_and_enter(cx, window, "sourdough");
    let highlight = shell.read_with(cx, |s, _| s.highlight().clone());
    assert_eq!(
        highlight,
        HashSet::from(["doc-f0".to_string(), "doc-f1".to_string()])
    );
}

#[gpui_kit::test]
fn a_seeded_query_shows_the_suggestion_until_it_is_edited_or_dismissed(cx: &mut TestAppContext) {
    let fixture = fixture();
    fixture.search.set(Ok(nothing()));
    let (window, shell) = open(cx, fixture.clone());
    let search = search(cx, &shell);
    search.update(cx, |s, cx| s.seed("feed the starter".to_string(), cx));
    cx.run_until_parked();
    assert_eq!(fixture.search.calls(), 1);
    assert!(label(cx, window, "suggested-query").is_some());
    assert_eq!(
        search.read_with(cx, |s, _| s.query().to_string()),
        "feed the starter"
    );
    click(cx, window, "search-input");
    cx.update_window(window.into(), |_, window, cx| window.input("s", cx))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(label(cx, window, "suggested-query"), None);
    cx.update_window(window.into(), |_, window, cx| window.press("backspace", cx))
        .unwrap();
    cx.run_until_parked();
    assert!(label(cx, window, "suggested-query").is_some());
    click(cx, window, "suggested-dismiss");
    assert_eq!(label(cx, window, "suggested-query"), None);
    assert!(SUGGESTED.starts_with("Setup is done."));
}

#[test]
fn a_subtitle_is_the_heading_or_the_first_line_that_is_not_the_title() {
    let mut h = hit("c", 0.5);
    h.heading = "Feeding".to_string();
    assert_eq!(subtitle_of(&h), "Feeding");
    h.heading.clear();
    h.title = "Sourdough starter".to_string();
    h.excerpt = "## Sourdough Starter\n\n---\n  ##   Feed   it\ttwice a day  \nmore".to_string();
    assert_eq!(subtitle_of(&h), "Feed it twice a day");
    h.excerpt = "x".repeat(130);
    let cut = subtitle_of(&h);
    assert_eq!(cut.chars().count(), 120);
    assert!(cut.ends_with('…'));
    h.excerpt = format!("{} {}", "a".repeat(118), "b".repeat(10));
    assert_eq!(subtitle_of(&h), format!("{}…", "a".repeat(118)));
    h.excerpt = "---\n***".to_string();
    assert_eq!(subtitle_of(&h), "");
}

#[test]
fn document_facts_name_the_vault_status_only_when_it_differs() {
    let mut d = detail();
    assert_eq!(status_note(&d).as_deref(), Some(" (vault says “proposed”)"));
    d.status = Some("INVESTIGATING".to_string());
    assert_eq!(status_note(&d), None);
    d.status = Some(String::new());
    assert_eq!(status_note(&d), None);
    d.status = None;
    assert_eq!(status_note(&d), None);
    let at = indexed_at(1_763_830_000);
    assert!(
        at.contains("/2025, ") && (at.ends_with(" AM") || at.ends_with(" PM")),
        "{at}"
    );
}

#[gpui_kit::test]
fn clicking_a_card_takes_focus_from_the_input(cx: &mut TestAppContext) {
    let fixture = fixture();
    fixture.search.set(Ok(answer(explain(
        vec![hit("v0", 0.71)],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &[],
    ))));
    fixture.document.set(Ok(Document::Found(detail())));
    let (window, shell) = open(cx, fixture);
    type_and_enter(cx, window, "sourdough");
    let input = search(cx, &shell).read_with(cx, |s, _| s.input.clone().expect("input"));
    let focused = |cx: &mut TestAppContext| {
        cx.update_window(window.into(), |_, window, cx| {
            input.read(cx).focus_handle(cx).is_focused(window)
        })
        .unwrap()
    };
    assert!(focused(cx));
    click(cx, window, toggle_id(&card("vector", 0, "v0")).to_string());
    assert!(
        !focused(cx),
        "the input kept focus after a card was clicked"
    );
}

#[gpui_kit::test]
fn cards_paint_their_frames_on_chromes_fractional_columns(cx: &mut TestAppContext) {
    let fixture = fixture();
    let above = vec![hit("f0", 0.9)];
    fixture.search.set(Ok(answer(explain(
        Vec::new(),
        Vec::new(),
        Vec::new(),
        above.clone(),
        above,
        &["f0"],
    ))));
    let (window, _) = open(cx, fixture);
    type_and_enter(cx, window, "sourdough");
    let frames = [card("fused", 0, "f0"), card("injected", 0, "f0")].map(|id| {
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let accent = theme::tokens(cx).accent;
            let card = window.find(SharedString::from(id)).bounds();
            let (top, left) = (f32::from(card.top()) * 2., f32::from(card.left()) * 2.);
            window
                .painted_quads()
                .into_iter()
                .filter(|q| q.border_color == accent && q.border_widths.left.0 > 0.)
                .filter(|q| (q.bounds.origin.y.0 - top).abs() <= 2.)
                .filter(|q| (q.bounds.origin.x.0 - left).abs() <= 4.)
                .map(|q| {
                    (
                        q.bounds.origin.x.0,
                        q.bounds.origin.x.0 + q.bounds.size.width.0,
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap()
    });
    assert_eq!(
        frames,
        [vec![(1686., 2216.)], vec![(2236., 2768.)]],
        "Chrome lays the last two of five flex-1 columns out at x=842.8..1108.4 and 1118.4..1384"
    );
}

fn chosen_tier(cx: &mut TestAppContext, window: WindowHandle<Root>) -> Vec<(f32, f32)> {
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let surface = gpui_kit::Background::from(theme::tokens(cx).surface);
        let control = window.find("search-tier").bounds();
        let (top, bottom) = (
            f32::from(control.top()) * 2.,
            f32::from(control.bottom()) * 2.,
        );
        let (left, right) = (
            f32::from(control.left()) * 2. - 4.,
            f32::from(control.right()) * 2. + 4.,
        );
        window
            .painted_quads()
            .into_iter()
            .filter(|q| q.background == surface)
            .filter(|q| q.bounds.origin.y.0 > top && q.bounds.origin.y.0 < bottom)
            .filter(|q| q.bounds.origin.x.0 > left && q.bounds.origin.x.0 < right)
            .map(|q| {
                (
                    q.bounds.origin.x.0,
                    q.bounds.origin.x.0 + q.bounds.size.width.0,
                )
            })
            .collect()
    })
    .unwrap()
}

#[gpui_kit::test]
fn the_tier_pill_paints_where_chrome_does_before_and_after_a_search(_cx: &mut TestAppContext) {
    let mut real_fonts = TestAppContext::build_with_text_system(
        gpui_kit::TestDispatcher::new(0),
        None,
        Arc::new(gpui_apple::AppleTextSystem::new()),
    );
    let cx = &mut real_fonts;
    let fixture = fixture();
    fixture.search.set(Ok(answer(explain(
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![hit("f0", 0.9)],
        vec![hit("f0", 0.9)],
        &["f0"],
    ))));
    let (window, _) = open(cx, fixture);
    assert_eq!(
        chosen_tier(cx, window),
        [(2090., 2168.)],
        "Chrome lays the tier control out at x=986.609 before a search"
    );
    type_and_enter(cx, window, "sourdough");
    assert_eq!(
        label(cx, window, "search-elapsed").as_deref(),
        Some("87 ms")
    );
    assert_eq!(
        chosen_tier(cx, window),
        [(1994., 2072.)],
        "after a search Chrome lays it out 48px further left, at x=938.609, before the 36px of \"87 ms\" and its gap"
    );
}

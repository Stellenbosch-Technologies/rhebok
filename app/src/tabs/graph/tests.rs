use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use gpui_kit::base::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::InputEvent;
use gpui_kit::{
    point, px, size, AppContext, Background, Bounds, Entity, Point, ScrollDelta, SharedString,
    TestAppContext, WindowBounds, WindowHandle, WindowOptions,
};
use rhebok::dashboard::core::document::Document;
use rhebok::dashboard::core::graph::{Graph, Node};
use rhebok::dashboard::core::memories::{Memories as MemoryList, MemoryReach, MemoryView};
use rhebok::dashboard::core::stats::{Stats, Tiers};
use rhebok::dashboard::core::DashboardError;
use rhebok::memory::{Memory, MemoryFacts, MemoryKind, Origin};
use rhebok::store::query::{GraphEdge, GraphEntity};

use super::GraphTab;
use crate::backend::{self, FixtureBackend};
use crate::graph::paint::LABEL_SIZE;
use crate::shell::Shell;
use crate::theme;
use crate::version_gate::Gate;

fn stats() -> Stats {
    Stats {
        documents: 3,
        chunks: 3,
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

fn node(id: &str, title: &str, source_type: &str) -> Node {
    Node {
        id: id.to_string(),
        title: title.to_string(),
        source_type: source_type.to_string(),
        chunks: 1,
        inbound: 0,
        agent: None,
        memory_kind: None,
        memory_id: None,
    }
}

fn edge(from: &str, to: &str, kind: &str) -> GraphEdge {
    GraphEdge {
        from: from.to_string(),
        to: to.to_string(),
        kind: kind.to_string(),
    }
}

fn small_graph() -> Graph {
    let mut session = node("s1", "A session", "transcript");
    session.agent = Some("claude-code");
    let mut memory = node("m1", "Feed the starter", "memory");
    memory.memory_kind = Some("lesson");
    memory.memory_id = Some("abc123".to_string());
    Graph {
        nodes: vec![
            node("a", "Sourdough starter", "markdown"),
            node("b", "Garden notes", "markdown"),
            node("c", "Deploy checklist", "markdown"),
            session,
            memory,
            node("project:/x", "/x", "project"),
        ],
        entities: vec![GraphEntity {
            id: "e1".to_string(),
            name: "flour.md".to_string(),
            kind: "file".to_string(),
        }],
        tags: vec!["bread".to_string()],
        edges: vec![
            edge("a", "b", "wikilink"),
            edge("b", "a", "wikilink"),
            edge("a", "bread", "tagged"),
            edge("a", "e1", "mentions"),
            edge("s1", "e1", "mentions"),
            edge("m1", "project:/x", "scoped-to"),
        ],
    }
}

fn wide_graph(n: usize) -> Graph {
    Graph {
        nodes: (0..n)
            .map(|i| node(&format!("n{i}"), &format!("Note number {i}"), "markdown"))
            .collect(),
        entities: Vec::new(),
        tags: Vec::new(),
        edges: (1..n)
            .map(|i| edge(&format!("n{}", i / 2), &format!("n{i}"), "wikilink"))
            .collect(),
    }
}

fn memories() -> MemoryList {
    MemoryList {
        memories: vec![MemoryView {
            memory: Memory {
                id: "abc123".to_string(),
                uri: "memory://abc123".to_string(),
                doc_id: "m1".to_string(),
                title: "Feed the starter".to_string(),
                text: "Feed the starter\ntwice a day.".to_string(),
                facts: MemoryFacts {
                    kind: MemoryKind::Lesson,
                    created: 100,
                    project: Some("/x".to_string()),
                    origin: Origin::User,
                    confidence: 80,
                    session: None,
                    source_hash: None,
                    source_stamp: None,
                },
            },
            project_name: None,
        }],
        unavailable: None,
    }
}

fn fixture(graph: Graph) -> Arc<FixtureBackend> {
    let fixture = FixtureBackend::default();
    fixture.stats.set(Ok(stats()));
    fixture.graph.set(Ok(graph));
    fixture.memories.set(memories());
    fixture.document.set(Ok(Document::Missing {
        error: "document not found".to_string(),
    }));
    Arc::new(fixture)
}

fn open(
    cx: &mut TestAppContext,
    fixture: Arc<FixtureBackend>,
) -> (WindowHandle<Root>, Entity<Shell>, Entity<GraphTab>) {
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
    let graph = shell
        .read_with(cx, |s, _| s.graph.clone())
        .expect("the graph is the default tab");
    graph.update(cx, |g, _| g.settle());
    render(cx, window);
    (window, shell, graph)
}

fn render(cx: &mut TestAppContext, window: WindowHandle<Root>) {
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
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

fn click_node(
    cx: &mut TestAppContext,
    window: WindowHandle<Root>,
    graph: &Entity<GraphTab>,
    id: &str,
) {
    let at = graph.read_with(cx, |g, _| {
        let ix = g.index_of(id).expect("node is shown");
        g.screen_of(ix).expect("node has a position")
    });
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click_at("graph-canvas", point(px(at.x as f32), px(at.y as f32)), cx);
    })
    .unwrap();
    cx.run_until_parked();
    render(cx, window);
}

fn rings(cx: &mut TestAppContext, window: WindowHandle<Root>) -> Vec<(f64, f64, f64)> {
    let sun = theme::Tokens::light().sun;
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let scale = f64::from(window.scale_factor());
        window
            .painted_quads()
            .into_iter()
            .filter(|q| {
                q.border_color == sun
                    && q.background == Background::from(gpui_kit::transparent_black())
            })
            .map(|q| {
                (
                    f64::from(q.bounds.origin.x.0) / scale,
                    f64::from(q.bounds.origin.y.0) / scale,
                    f64::from(q.bounds.size.width.0) / scale,
                )
            })
            .collect()
    })
    .unwrap()
}

#[gpui_kit::test]
fn clicking_a_node_opens_its_panel(cx: &mut TestAppContext) {
    let backend = fixture(small_graph());
    let (window, _, graph) = open(cx, backend.clone());
    assert_eq!(label(cx, window, "graph-panel"), None);

    click_node(cx, window, &graph, "a");
    assert_eq!(
        label(cx, window, "graph-panel").as_deref(),
        Some("Sourdough starter")
    );
    assert!(backend.requests().contains(&"document a".to_string()));
    assert_eq!(
        label(cx, window, "graph-linked-0").as_deref(),
        Some("→ Garden notes")
    );
    assert_eq!(
        label(cx, window, "graph-linked-1").as_deref(),
        Some("← Garden notes")
    );
    assert!(label(cx, window, "graph-tag-chips").is_some());
    assert!(label(cx, window, "graph-mention-chips").is_some());

    click_node(cx, window, &graph, "project:/x");
    assert_eq!(label(cx, window, "graph-panel").as_deref(), Some("/x"));
    assert_eq!(
        label(cx, window, "project-details-memories").as_deref(),
        Some("1")
    );

    click_node(cx, window, &graph, "m1");
    assert_eq!(
        label(cx, window, "memory-details-kind").as_deref(),
        Some("lesson")
    );
    assert_eq!(
        label(cx, window, "memory-details-confidence").as_deref(),
        Some("80")
    );
    backend.memory_reach.set(Ok(MemoryReach {
        reach: Some(Vec::new()),
        error: None,
    }));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("graph-reach", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        label(cx, window, "graph-reach-nothing").as_deref(),
        Some("nothing")
    );
    assert!(backend
        .requests()
        .iter()
        .all(|r| !r.starts_with("memory_save") && !r.starts_with("memory_delete")));

    click_node(cx, window, &graph, "tag:bread");
    assert_eq!(label(cx, window, "graph-panel"), None);
}

#[gpui_kit::test]
fn sessions_are_hidden_until_toggled(cx: &mut TestAppContext) {
    let (window, _, graph) = open(cx, fixture(small_graph()));
    let ids = |graph: &Entity<GraphTab>, cx: &mut TestAppContext| {
        graph.read_with(cx, |g, _| {
            g.shown()
                .nodes
                .iter()
                .map(|n| n.id.clone())
                .collect::<Vec<_>>()
        })
    };
    assert!(!ids(&graph, cx).contains(&"s1".to_string()));
    assert_eq!(
        ids(&graph, cx),
        ["a", "b", "c", "m1", "project:/x", "e1", "tag:bread"]
    );
    assert_eq!(
        label(cx, window, "graph-sessions").as_deref(),
        Some("Sessions (1 hidden)")
    );
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("graph-sessions", cx);
    })
    .unwrap();
    render(cx, window);
    assert!(ids(&graph, cx).contains(&"s1".to_string()));
    assert_eq!(
        label(cx, window, "graph-sessions").as_deref(),
        Some("Sessions shown")
    );
    assert_eq!(
        graph.read_with(cx, |g, _| g.shown().edges.len()),
        6,
        "the session's mention reaches the entity once sessions show"
    );
}

#[gpui_kit::test]
fn labels_appear_only_at_zoom_one_point_five(cx: &mut TestAppContext) {
    let (window, _, graph) = open(cx, fixture(wide_graph(30)));
    let k = graph.read_with(cx, |g, _| g.view().k);
    assert!(
        (k - 4. / 30f64.cbrt()).abs() < 1e-12,
        "fitted to 30 nodes: {k}"
    );
    render(cx, window);
    assert_eq!(graph.read_with(cx, |g, _| g.labels_painted()), 0);

    let scroll = |cx: &mut TestAppContext, dy: f32| {
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.scroll(
                "graph-canvas",
                ScrollDelta::Pixels(point(px(0.), px(dy))),
                cx,
            );
        })
        .unwrap();
        render(cx, window);
    };
    scroll(cx, 50.);
    let k = graph.read_with(cx, |g, _| g.view().k);
    assert!(k < 1.5);
    assert_eq!(graph.read_with(cx, |g, _| g.labels_painted()), 0);
    scroll(cx, 100.);
    let k = graph.read_with(cx, |g, _| g.view().k);
    assert!(k >= 1.5, "two wheel steps zoom past 1.5: {k}");
    assert!(graph.read_with(cx, |g, _| g.labels_painted()) > 0);
    assert_eq!(LABEL_SIZE, 11.);
}

#[gpui_kit::test]
fn search_hits_ring_their_nodes(cx: &mut TestAppContext) {
    let (window, shell, graph) = open(cx, fixture(small_graph()));
    assert!(rings(cx, window).is_empty());
    shell.update(cx, |s, cx| s.set_highlight(vec!["b".to_string()], cx));
    render(cx, window);
    let rings = rings(cx, window);
    assert_eq!(rings.len(), 1);
    let centre = graph.read_with(cx, |g, _| {
        let at = g.screen_of(g.index_of("b").unwrap()).unwrap();
        let k = g.view().k;
        (at, k)
    });
    let ((at, k), (left, top, width)) = (centre, rings[0]);
    let outer = (crate::graph::node_radius(0) + 2. + 0.75) * k;
    assert!(
        (width - 2. * outer).abs() <= 1.,
        "ring {width} vs {}",
        2. * outer
    );
    let canvas = cx
        .update_window(window.into(), |_, window, _| {
            window.find("graph-canvas").bounds().origin
        })
        .unwrap();
    let cx_ = left - f64::from(canvas.x) + outer;
    let cy_ = top - f64::from(canvas.y) + outer;
    assert!(
        (cx_ - at.x).abs() <= 1. && (cy_ - at.y).abs() <= 1.,
        "ring centred at ({cx_}, {cy_}), node at {at:?}"
    );
}

#[gpui_kit::test]
fn a_failed_load_offers_try_again(cx: &mut TestAppContext) {
    let backend = fixture(small_graph());
    backend
        .graph
        .set(Err(DashboardError::Unavailable("store busy".into())));
    let (window, _, graph) = open(cx, backend.clone());
    assert_eq!(
        label(cx, window, "graph-error").as_deref(),
        Some("The index is busy — try again in a moment.")
    );
    backend.graph.set(Ok(small_graph()));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("graph-retry", cx);
    })
    .unwrap();
    render(cx, window);
    assert_eq!(label(cx, window, "graph-error"), None);
    assert!(graph.read_with(cx, |g, _| g.graph().is_some()));
    assert_eq!(backend.graph.calls(), 2);
}

#[gpui_kit::test]
fn a_memory_write_refetches_the_graph_but_a_visit_does_not(cx: &mut TestAppContext) {
    let backend = fixture(small_graph());
    let (window, shell, _) = open(cx, backend.clone());
    assert_eq!(backend.graph.calls(), 1);
    shell.update(cx, |s, cx| s.open_tab(crate::shell::TabId::Search, cx));
    shell.update(cx, |s, cx| s.open_tab(crate::shell::TabId::Graph, cx));
    render(cx, window);
    assert_eq!(backend.graph.calls(), 1);
    shell.update(cx, |s, cx| s.memory_written(cx));
    render(cx, window);
    assert_eq!(backend.graph.calls(), 2);
    assert_eq!(backend.memories.calls(), 2);
}

#[gpui_kit::test]
fn dragging_empty_space_pans_and_dragging_a_node_moves_it(cx: &mut TestAppContext) {
    let (window, _, graph) = open(cx, fixture(small_graph()));
    let before = graph.read_with(cx, |g, _| g.view().pan);
    graph.update(cx, |g, cx| {
        g.press(point(5., 5.), cx);
        g.moved(point(45., 25.), true, cx);
        g.release(point(45., 25.), cx);
    });
    let after = graph.read_with(cx, |g, _| g.view().pan);
    assert_eq!((after.x - before.x, after.y - before.y), (40., 20.));
    assert!(graph.read_with(cx, |g, _| g.selected().is_none()));

    let (ix, at, k) = graph.read_with(cx, |g, _| {
        let ix = g.index_of("c").unwrap();
        (ix, g.screen_of(ix).unwrap(), g.view().k)
    });
    let start = graph.read_with(cx, |g, _| g.layout().unwrap().precise(ix));
    graph.update(cx, |g, cx| {
        g.press(at, cx);
        g.moved(point(at.x + 30., at.y), true, cx);
    });
    let moved = graph.read_with(cx, |g, _| g.layout().unwrap().precise(ix));
    assert!((moved.x - (start.x + 30. / k)).abs() < 1e-9);
    assert!(graph.read_with(cx, |g, _| g.layout().unwrap().warm()));
    graph.update(cx, |g, cx| g.release(point(at.x + 30., at.y), cx));
    assert!(
        graph.read_with(cx, |g, _| g.selected().is_none()),
        "a drag is not a click"
    );
    let at = graph.read_with(cx, |g, _| g.screen_of(ix).unwrap());
    graph.update(cx, |g, cx| {
        g.press(at, cx);
        g.moved(point(at.x + 3., at.y + 3.), true, cx);
        g.release(point(at.x + 3., at.y + 3.), cx);
    });
    assert_eq!(
        graph.read_with(cx, |g, _| g.selected().map(|n| n.id.clone())),
        Some("c".to_string()),
        "a press that moves five pixels or less is still a click"
    );
    render(cx, window);
}

#[gpui_kit::test]
fn hovering_a_node_shows_its_title(cx: &mut TestAppContext) {
    let (window, _, graph) = open(cx, fixture(small_graph()));
    let at = graph.read_with(cx, |g, _| g.screen_of(g.index_of("b").unwrap()).unwrap());
    graph.update(cx, |g, cx| g.moved(at, true, cx));
    render(cx, window);
    assert_eq!(
        label(cx, window, "graph-tooltip").as_deref(),
        Some("Garden notes")
    );
    graph.update(cx, |g, cx| g.moved(point(3., 3.), true, cx));
    render(cx, window);
    assert_eq!(label(cx, window, "graph-tooltip"), None);
}

fn frames(cx: &mut TestAppContext, window: WindowHandle<Root>, n: usize) {
    for _ in 0..n {
        cx.update_window(window.into(), |_, window, cx| {
            window.simulate_next_frame(cx);
        })
        .unwrap();
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn a_reload_while_the_tab_is_hidden_keeps_simulating(cx: &mut TestAppContext) {
    let (window, shell, graph) = open(cx, fixture(small_graph()));
    frames(cx, window, 1);
    assert!(!graph.read_with(cx, |g, _| g.layout().unwrap().warm()));
    shell.update(cx, |s, cx| s.open_tab(crate::shell::TabId::Search, cx));
    render(cx, window);
    shell.update(cx, |s, cx| s.memory_written(cx));
    cx.run_until_parked();
    let before = graph.read_with(cx, |g, _| g.ticks());
    assert!(graph.read_with(cx, |g, _| g.layout().unwrap().warm()));
    frames(cx, window, 5);
    assert_eq!(
        graph.read_with(cx, |g, _| g.ticks()),
        before + 5,
        "a hidden, still-mounted graph ticks once per frame like force-graph"
    );
}

#[gpui_kit::test]
fn a_trackpad_pinch_zooms_like_chromes_ctrl_wheel(cx: &mut TestAppContext) {
    let (window, _, graph) = open(cx, fixture(small_graph()));
    let k = graph.read_with(cx, |g, _| g.view().k);
    let centre = cx
        .update_window(window.into(), |_, window, _| {
            window.find("graph-canvas").bounds().center()
        })
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.dispatch_event(
            gpui_kit::PinchEvent {
                position: centre,
                delta: 0.1,
                modifiers: gpui_kit::Modifiers::default(),
                phase: gpui_kit::TouchPhase::Moved,
            }
            .to_platform_input(),
            cx,
        );
    })
    .unwrap();
    let zoomed = graph.read_with(cx, |g, _| g.view().k);
    let chrome_delta_y = -100. * (1. + f64::from(0.1f32)).ln();
    let expected = k * 2f64.powf(-chrome_delta_y * 0.002 * 10.);
    assert!(
        (zoomed - expected).abs() < 1e-9,
        "pinch 0.1 zoomed {k} to {zoomed}, Chrome's ctrl-wheel gives {expected}"
    );
}

#[test]
fn the_tooltip_sits_on_chromes_helvetica_line() {
    use crate::theme::{css_line_box, normal_line_height, Face, HELVETICA};
    let mut cx = TestAppContext::build_with_text_system(
        gpui_kit::TestDispatcher::new(0),
        None,
        Arc::new(gpui_apple::AppleTextSystem::new()),
    );
    let cx = &mut cx;
    let (line, height) = cx.update(|cx| {
        let face = Face::new(HELVETICA, gpui_kit::FontWeight::NORMAL, 12., 0.);
        (
            css_line_box(cx, face.with_normal_line_height(cx), &[]),
            normal_line_height(cx, face),
        )
    });
    assert_eq!(
        height, 14.,
        "Chrome's line-height: normal for 12px Helvetica"
    );
    assert_eq!((line.height, line.baseline), (14., 11.));
    let (window, _, graph) = open(cx, fixture(small_graph()));
    let at = graph.read_with(cx, |g, _| g.screen_of(g.index_of("b").unwrap()).unwrap());
    graph.update(cx, |g, cx| g.moved(at, true, cx));
    render(cx, window);
    let tip = cx
        .update_window(window.into(), |_, window, _| {
            window.find("graph-tooltip").bounds()
        })
        .unwrap();
    assert_eq!(f32::from(tip.size.height), 20.);
}

#[gpui_kit::test]
fn a_long_mention_chip_wraps_inside_the_panel(cx: &mut TestAppContext) {
    let mut graph = small_graph();
    graph.entities.push(GraphEntity {
        id: "e2".to_string(),
        name: "a mention whose name runs on well past the width of the node panel".to_string(),
        kind: "concept".to_string(),
    });
    graph.edges.push(edge("a", "e2", "mentions"));
    let (window, _, view) = open(cx, fixture(graph));
    click_node(cx, window, &view, "a");
    let (panel, chips) = cx
        .update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            (
                window.find("graph-panel").bounds(),
                window
                    .find_all("graph-mention-chips")
                    .into_iter()
                    .chain((0..2).map(|ix| window.find(format!("graph-mention-{ix}"))))
                    .map(|chip| chip.bounds())
                    .fold(
                        Bounds::default(),
                        |a: Bounds<gpui_kit::Pixels>, b| {
                            if b.right() > a.right() {
                                b
                            } else {
                                a
                            }
                        },
                    ),
            )
        })
        .unwrap();
    assert!(
        chips.right() <= panel.right() - px(20.),
        "chips end at {:?}, the panel's content box at {:?}",
        chips.right(),
        panel.right() - px(20.)
    );
}

#[gpui_kit::test]
fn a_press_keeps_the_tooltip_until_the_drag_starts(cx: &mut TestAppContext) {
    let (window, _, graph) = open(cx, fixture(small_graph()));
    let at = graph.read_with(cx, |g, _| g.screen_of(g.index_of("b").unwrap()).unwrap());
    graph.update(cx, |g, cx| {
        g.moved(at, true, cx);
        g.press(at, cx);
        g.moved(point(at.x + 3., at.y), true, cx);
    });
    render(cx, window);
    assert_eq!(
        label(cx, window, "graph-tooltip").as_deref(),
        Some("Garden notes")
    );
    graph.update(cx, |g, cx| g.moved(point(at.x + 12., at.y), true, cx));
    render(cx, window);
    assert_eq!(label(cx, window, "graph-tooltip"), None);
    graph.update(cx, |g, cx| g.release(point(at.x + 12., at.y), cx));
}

#[gpui_kit::test]
fn the_viewport_corners_are_masked_in_the_page_colour(cx: &mut TestAppContext) {
    let (window, _, _) = open(cx, fixture(small_graph()));
    let ground = theme::Tokens::light().ground;
    let (viewport, masks, scale) = cx
        .update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let masks: Vec<_> = window
                .painted_quads()
                .into_iter()
                .filter(|q| q.border_color == ground && q.background.is_transparent())
                .collect();
            (
                window.find("graph-viewport").bounds(),
                masks,
                window.scale_factor(),
            )
        })
        .unwrap();
    assert!(!masks.is_empty(), "no corner mask was painted");
    let outer = masks
        .iter()
        .map(|q| {
            (
                q.bounds.origin.x.0 / scale,
                q.bounds.origin.y.0 / scale,
                q.corner_radii.top_left.0 / scale,
            )
        })
        .fold((f32::MAX, f32::MAX, 0f32), |a, b| {
            (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2))
        });
    assert_eq!(
        (outer.0, outer.1),
        (
            f32::from(viewport.origin.x) - 8.,
            f32::from(viewport.origin.y) - 8.
        )
    );
    assert_eq!(outer.2, 16., "the mask's inner edge follows the 8px corner");
}

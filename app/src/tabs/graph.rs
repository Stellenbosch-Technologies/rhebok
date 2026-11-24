use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rhebok::dashboard::core::graph::{Graph as GraphData, Node};
use rhebok::dashboard::core::memories::{Memories as MemoryList, ReachedDocument};

use crate::backend::{describe, run_backend};
use crate::graph::layout::{Layout, Lcg, NodeSeed};
use crate::graph::paint::{self, Frame};
use crate::graph::{
    agent_name, document_count, hidden_sessions, legend_colour, legend_label, mentions_of,
    neighbours, node_at, project_memories, selectable, session_agents, sessions_label, shown,
    tags_of, wheel_delta, Shown, View, DRAG_CLICK_TOLERANCE, LEGEND,
};
use crate::shell::Shell;
use crate::tabs::memories::small_caps;
use crate::theme::{
    normal_line_height, tokens, Face, Tokens, Type, HELVETICA, MONO, SANS, XS_FACE, XS_MONO_FACE,
};
use crate::widgets::css_box::css_rule_top;
use crate::widgets::document_facts::{indexed_at, DocumentFacts};
use crate::widgets::exact::unrounded_width;
use crate::widgets::inline_flow::{inline_flow, Segment};
use crate::widgets::js::{to_fixed, wire};
use crate::widgets::tail::ellipsis_text;
use crate::widgets::wrap::wrapped_text;
use crate::widgets::{banner, baseline, button, document_facts, exact_text, text_button, tracked};

pub const TRY_AGAIN: &str = "Try again";
pub const LOADING: &str = "Loading…";
pub const NOT_FOUND: &str = "This memory could not be found.";
pub const ASK_REACH: &str = "What would this pull in?";
pub const REACHING: &str = "Reaching…";
pub const NOTHING: &str = "nothing";
pub const NONE_LINKED: &str = "none";
pub const NO_DOCUMENTS: &str = "No documents indexed yet";
pub const NO_DOCUMENTS_HELP: &str =
    "The graph is drawn from the folders rhebok indexes. Add one and index it to see your notes here.";
pub const OPEN_SETUP: &str = "Open guided setup";
pub const ADD_FOLDER: &str = "Add a folder";
pub const SETUP_HELP: &str =
    "Guided setup picks folders, connects your coding agents and runs the first index.";
pub const DRAG_ALPHA_TARGET: f64 = 0.3;
pub const TOOLTIP_SIZE: f32 = 12.;
pub const TOOLTIP_FACE: Face = Face::new(HELVETICA, FontWeight::NORMAL, TOOLTIP_SIZE, 0.);

const TRACKING_WIDER: f32 = 0.05;
const TRY_FACE: Face = Face::new(SANS, FontWeight::SEMIBOLD, 12., 16.);
const PROJECT_LABELS: [&str; 2] = ["Path", "Memories"];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gesture {
    Pan {
        last: Point<f64>,
    },
    Node {
        index: usize,
        start: Point<f64>,
        from: Point<f64>,
        dragged: bool,
    },
}

#[derive(Default, Clone, Copy)]
struct Canvas {
    origin: Point<Pixels>,
    size: Size<Pixels>,
}

pub struct GraphTab {
    shell: WeakEntity<Shell>,
    graph: Option<GraphData>,
    error: Option<String>,
    memories: Option<MemoryList>,
    sessions: bool,
    shown: Shown,
    layout: Option<Layout>,
    random: Lcg,
    view: View,
    canvas: Rc<Cell<Canvas>>,
    selected: Option<Node>,
    facts: Option<Entity<DocumentFacts>>,
    reach: Option<Vec<ReachedDocument>>,
    reach_error: Option<String>,
    reaching: bool,
    hover: Option<usize>,
    pointer: Option<Point<f64>>,
    gesture: Option<Gesture>,
    reloads: u64,
    tick_pending: bool,
    window: Option<AnyWindowHandle>,
    ticks: u64,
    tick_times: Vec<Duration>,
    labels_painted: Rc<Cell<usize>>,
    graph_task: Option<Task<()>>,
    memories_task: Option<Task<()>>,
    reach_task: Option<Task<()>>,
    _subscriptions: [Subscription; 1],
}

fn reloads_of(shell: &Shell) -> u64 {
    shell.memory_writes() + shell.index_runs()
}

impl GraphTab {
    pub fn new(shell: Entity<Shell>, cx: &mut Context<GraphTab>) -> GraphTab {
        let subscriptions = [cx.observe(&shell, |graph, shell, cx| {
            let reloads = reloads_of(shell.read(cx));
            if reloads != graph.reloads {
                graph.reloads = reloads;
                graph.load(cx);
            }
            cx.notify();
        })];
        let mut graph = GraphTab {
            shell: shell.downgrade(),
            graph: None,
            error: None,
            memories: None,
            sessions: false,
            shown: Shown::default(),
            layout: None,
            random: Lcg::default(),
            view: View::default(),
            canvas: Rc::new(Cell::new(Canvas::default())),
            selected: None,
            facts: None,
            reach: None,
            reach_error: None,
            reaching: false,
            hover: None,
            pointer: None,
            gesture: None,
            reloads: 0,
            tick_pending: false,
            window: None,
            ticks: 0,
            tick_times: Vec::new(),
            labels_painted: Rc::new(Cell::new(0)),
            graph_task: None,
            memories_task: None,
            reach_task: None,
            _subscriptions: subscriptions,
        };
        let this = cx.entity().downgrade();
        cx.defer(move |cx| {
            let reloads = reloads_of(shell.read(cx));
            let _ = this.update(cx, |graph, _| graph.reloads = reloads);
        });
        graph.load(cx);
        graph
    }

    pub fn graph(&self) -> Option<&GraphData> {
        self.graph.as_ref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn shown(&self) -> &Shown {
        &self.shown
    }

    pub fn sessions_shown(&self) -> bool {
        self.sessions
    }

    pub fn layout(&self) -> Option<&Layout> {
        self.layout.as_ref()
    }

    pub fn view(&self) -> View {
        self.view
    }

    pub fn selected(&self) -> Option<&Node> {
        self.selected.as_ref()
    }

    pub fn hovered(&self) -> Option<usize> {
        self.hover
    }

    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    pub fn tick_times(&self) -> &[Duration] {
        &self.tick_times
    }

    pub fn labels_painted(&self) -> usize {
        self.labels_painted.get()
    }

    pub fn canvas_size(&self) -> Size<Pixels> {
        self.canvas.get().size
    }

    pub fn reach(&self) -> Option<&[ReachedDocument]> {
        self.reach.as_deref()
    }

    pub fn reach_error(&self) -> Option<&str> {
        self.reach_error.as_deref()
    }

    pub fn load(&mut self, cx: &mut Context<GraphTab>) {
        self.error = None;
        self.graph_task = Some(run_backend(
            cx,
            |backend| backend.graph(),
            |graph: &mut GraphTab, result, cx| match result {
                Ok(data) => {
                    graph.graph = Some(data);
                    graph.rebuild();
                    graph.start_ticking(cx);
                }
                Err(e) => graph.error = Some(describe(&e)),
            },
        ));
        self.memories_task = Some(run_backend(
            cx,
            |backend| backend.memories(),
            |graph: &mut GraphTab, memories, _| graph.memories = Some(memories),
        ));
        cx.notify();
    }

    fn rebuild(&mut self) {
        let Some(data) = &self.graph else {
            return;
        };
        if let Some(layout) = &self.layout {
            self.random = layout.random();
        }
        self.shown = shown(data, self.sessions);
        let seeds = vec![NodeSeed::default(); self.shown.nodes.len()];
        self.layout = Some(Layout::with_random(&seeds, &self.shown.edges, self.random));
        self.hover = None;
        self.gesture = None;
        self.view.fit(self.shown.nodes.len());
    }

    pub fn toggle_sessions(&mut self, cx: &mut Context<GraphTab>) {
        self.sessions = !self.sessions;
        self.rebuild();
        self.start_ticking(cx);
        cx.notify();
    }

    pub fn settle(&mut self) -> u64 {
        let mut steps = 0;
        if let Some(layout) = &mut self.layout {
            while layout.step() {
                steps += 1;
            }
        }
        self.ticks += steps;
        steps
    }

    pub fn tick(&mut self) -> bool {
        let Some(layout) = &mut self.layout else {
            return false;
        };
        let started = Instant::now();
        let warm = layout.step();
        self.tick_times.push(started.elapsed());
        self.ticks += 1;
        warm
    }

    fn wants_tick(&self) -> bool {
        !self.tick_pending && self.layout.as_ref().is_some_and(Layout::warm)
    }

    fn on_frame(this: WeakEntity<GraphTab>) -> impl FnOnce(&mut Window, &mut App) + 'static {
        move |window, cx| {
            let _ = this.update(cx, |graph, cx| {
                graph.tick_pending = false;
                graph.tick();
                graph.schedule_tick(window, cx);
                cx.notify();
            });
        }
    }

    fn schedule_tick(&mut self, window: &mut Window, cx: &mut Context<GraphTab>) {
        self.window = Some(window.window_handle());
        if !self.wants_tick() {
            return;
        }
        self.tick_pending = true;
        window.on_next_frame(Self::on_frame(cx.entity().downgrade()));
    }

    fn start_ticking(&mut self, cx: &mut Context<GraphTab>) {
        let Some(handle) = self.window else {
            return;
        };
        if !self.wants_tick() {
            return;
        }
        let frame = Self::on_frame(cx.entity().downgrade());
        self.tick_pending = cx
            .update_window(handle, |_, window, _| window.on_next_frame(frame))
            .is_ok();
    }

    pub fn select(&mut self, id: &str, cx: &mut Context<GraphTab>) {
        let picked = self.graph.as_ref().and_then(|g| selectable(g, id));
        let changed = picked.as_ref().map(|n| &n.id) != self.selected.as_ref().map(|n| &n.id);
        self.selected = picked;
        if changed {
            self.reach = None;
            self.reach_error = None;
            self.reaching = false;
            self.reach_task = None;
        }
        if let Some(node) = &self.selected {
            if node.source_type != "memory" && node.source_type != "project" {
                let id = node.id.clone();
                match &self.facts {
                    Some(facts) => facts.update(cx, |f, cx| f.set_id(id, cx)),
                    None => self.facts = Some(document_facts(id, cx)),
                }
            }
        }
        cx.notify();
    }

    pub fn load_reach(&mut self, cx: &mut Context<GraphTab>) {
        let Some(id) = self.selected.as_ref().and_then(|n| n.memory_id.clone()) else {
            return;
        };
        self.reaching = true;
        self.reach = None;
        self.reach_error = None;
        let asked = id.clone();
        self.reach_task = Some(run_backend(
            cx,
            move |backend| backend.memory_reach(asked),
            move |graph: &mut GraphTab, result, _| {
                if graph.selected.as_ref().and_then(|n| n.memory_id.as_deref()) != Some(&id) {
                    return;
                }
                match result {
                    Ok(r) => match r.error {
                        Some(error) => graph.reach_error = Some(error),
                        None => graph.reach = Some(r.reach.unwrap_or_default()),
                    },
                    Err(e) => graph.reach_error = Some(describe(&e)),
                }
                graph.reaching = false;
            },
        ));
        cx.notify();
    }

    pub fn screen_of(&self, index: usize) -> Option<Point<f64>> {
        let p = *self.layout.as_ref()?.positions().get(index)?;
        Some(self.view.to_screen(
            point(f64::from(p.x), f64::from(p.y)),
            self.canvas.get().size,
        ))
    }

    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.shown.nodes.iter().position(|n| n.id == id)
    }

    fn node_under(&self, at: Point<f64>) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        node_at(
            &self.view,
            &self.shown.nodes,
            layout.positions(),
            at,
            self.canvas.get().size,
        )
    }

    pub fn press(&mut self, at: Point<f64>, cx: &mut Context<GraphTab>) {
        self.pointer = Some(at);
        let under = self.node_under(at);
        self.gesture = Some(match (under, self.layout.as_mut()) {
            (Some(index), Some(layout)) => {
                let from = layout.precise(index);
                layout.pin(index, from);
                Gesture::Node {
                    index,
                    start: at,
                    from,
                    dragged: false,
                }
            }
            _ => Gesture::Pan { last: at },
        });
        cx.notify();
    }

    pub fn moved(&mut self, at: Point<f64>, inside: bool, cx: &mut Context<GraphTab>) {
        self.pointer = inside.then_some(at);
        match self.gesture {
            Some(Gesture::Pan { last }) => {
                self.view.pan_by(point(at.x - last.x, at.y - last.y));
                self.gesture = Some(Gesture::Pan { last: at });
                self.hover = None;
            }
            Some(Gesture::Node {
                index,
                start,
                from,
                dragged,
            }) => {
                let k = self.view.k;
                let to = point(from.x + (at.x - start.x) / k, from.y + (at.y - start.y) / k);
                let far = ((at.x - start.x).powi(2) + (at.y - start.y).powi(2)).sqrt()
                    > DRAG_CLICK_TOLERANCE;
                if let Some(layout) = &mut self.layout {
                    layout.pin(index, to);
                    if dragged || far {
                        layout.set_alpha_target(DRAG_ALPHA_TARGET);
                    }
                }
                self.gesture = Some(Gesture::Node {
                    index,
                    start,
                    from,
                    dragged: dragged || far,
                });
                if dragged || far {
                    self.hover = None;
                }
            }
            None => {
                let under = if inside { self.node_under(at) } else { None };
                if under == self.hover {
                    if inside {
                        cx.notify();
                    }
                    return;
                }
                self.hover = under;
            }
        }
        cx.notify();
    }

    pub fn release(&mut self, at: Point<f64>, cx: &mut Context<GraphTab>) {
        let Some(gesture) = self.gesture.take() else {
            return;
        };
        if let Gesture::Node { index, dragged, .. } = gesture {
            if let Some(layout) = &mut self.layout {
                layout.unpin(index);
                if layout.alpha_target() != 0. {
                    layout.set_alpha_target(0.);
                }
            }
            if !dragged {
                if let Some(id) = self
                    .node_under(at)
                    .and_then(|ix| self.shown.nodes.get(ix))
                    .map(|n| n.id.clone())
                {
                    self.select(&id, cx);
                }
            }
        }
        self.hover = self.node_under(at);
        cx.notify();
    }

    pub fn wheel(&mut self, at: Point<f64>, delta: f64, ctrl: bool, cx: &mut Context<GraphTab>) {
        self.view.zoom_at(at, delta, ctrl, self.canvas.get().size);
        cx.notify();
    }

    fn ringed(&self, cx: &App) -> Vec<bool> {
        let shell = self.shell.upgrade();
        let highlight = shell.as_ref().map(|s| s.read(cx).highlight());
        let selected = self.selected.as_ref().map(|n| n.id.as_str());
        self.shown
            .nodes
            .iter()
            .map(|n| {
                highlight.is_some_and(|h| h.contains(&n.id)) || selected == Some(n.id.as_str())
            })
            .collect()
    }
}

pub fn pinch_delta(magnification: f32) -> f64 {
    -100. * (1. + f64::from(magnification)).ln()
}

fn local(position: Point<Pixels>, canvas: Canvas) -> Point<f64> {
    point(
        f64::from(position.x - canvas.origin.x),
        f64::from(position.y - canvas.origin.y),
    )
}

fn badge(text: &str, ink: Hsla, background: Hsla) -> impl IntoElement {
    div().child(
        tracked(
            text.to_uppercase(),
            SANS,
            FontWeight::SEMIBOLD,
            px(11.),
            TRACKING_WIDER,
        )
        .line_height(px(16.5))
        .text_color(ink)
        .framed(background, 4., 8., 2.),
    )
}

pub const ARROW_FAMILY: &str = ".SystemUIFont";

fn arrow(glyph: &'static str, colour: Hsla) -> impl IntoElement {
    baseline::web_baseline(StyledText::new(glyph).with_runs(vec![TextRun {
        len: glyph.len(),
        font: font(ARROW_FAMILY),
        color: colour,
        background_color: None,
        underline: None,
        strikethrough: None,
    }]))
}

pub const PANEL_WIDTH: f32 = 330.;
pub const PANEL_CONTENT: f32 = PANEL_WIDTH - 2. * 20. - 2.;
const CHIP_PAD_X: f32 = 6.;

fn chip_fits(window: &Window, text: &str, family: &'static str) -> bool {
    let mut style = window.text_style();
    style.font_family = family.into();
    style.font_weight = FontWeight::NORMAL;
    style.font_size = px(11.).into();
    unrounded_width(window, text, &style) <= px(PANEL_CONTENT - 2. * CHIP_PAD_X)
}

fn chip(
    id: String,
    text: String,
    family: &'static str,
    fits: bool,
    t: &Tokens,
) -> impl IntoElement {
    div()
        .id(SharedString::from(id))
        .test_support()
        .aria_label(SharedString::from(text.clone()))
        .min_w(px(0.))
        .bg(t.sunken)
        .rounded(px(4.))
        .px(px(CHIP_PAD_X))
        .py(px(2.))
        .font_family(family)
        .font_weight(FontWeight::NORMAL)
        .type_px(11.)
        .text_color(t.ink)
        .map(|d| {
            if fits {
                d.child(exact_text(text))
            } else {
                d.child(wrapped_text(text))
            }
        })
}

fn section(title: &str, t: &Tokens) -> Div {
    div().child(div().pb(px(6.)).child(small_caps(title, t)))
}

fn label_width(window: &Window, labels: &[&str]) -> Pixels {
    let mut style = window.text_style();
    style.font_family = SANS.into();
    style.font_weight = FontWeight::NORMAL;
    style.font_size = px(11.).into();
    labels
        .iter()
        .map(|label| unrounded_width(window, label, &style))
        .fold(px(0.), |a, b| a.max(b))
}

fn facts_grid(
    id: &'static str,
    rows: Vec<(&'static str, String, bool)>,
    width: Pixels,
    t: &Tokens,
) -> impl IntoElement {
    div()
        .id(id)
        .test_support()
        .flex()
        .flex_col()
        .gap(px(4.))
        .regular()
        .type_px(11.)
        .children(rows.into_iter().map(move |(label, value, mono)| {
            let tip = SharedString::from(value.clone());
            div()
                .id(SharedString::from(format!("{id}-{}", label.to_lowercase())))
                .test_support()
                .aria_label(SharedString::from(value.clone()))
                .flex()
                .gap(px(8.))
                .child(
                    div()
                        .flex_none()
                        .w(width)
                        .whitespace_nowrap()
                        .text_color(t.muted)
                        .child(baseline::text(label)),
                )
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "{id}-{}-value",
                            label.to_lowercase()
                        )))
                        .flex_grow(1.)
                        .min_w(px(0.))
                        .map(|d| {
                            if mono {
                                d.mono()
                                    .tooltip(move |window, cx| {
                                        Tooltip::new(tip.clone()).build(window, cx)
                                    })
                                    .child(baseline::web_baseline(ellipsis_text(value)))
                                    .into_any_element()
                            } else {
                                d.child(baseline::text(value)).into_any_element()
                            }
                        }),
                )
        }))
}

fn collapsed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl GraphTab {
    fn memory_details(
        &self,
        node: &Node,
        window: &Window,
        t: &Tokens,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let note = |text: String| {
            div()
                .id("memory-details")
                .test_support()
                .aria_label(SharedString::from(text.clone()))
                .type_px(11.)
                .text_color(t.muted)
                .child(baseline::text(text))
                .into_any_element()
        };
        let Some(memories) = &self.memories else {
            return vec![note(LOADING.to_string())];
        };
        if let Some(reason) = &memories.unavailable {
            return vec![note(reason.clone())];
        }
        let Some(m) = memories
            .memories
            .iter()
            .find(|m| Some(&m.memory.id) == node.memory_id.as_ref())
        else {
            return vec![note(NOT_FOUND.to_string())];
        };
        let facts = &m.memory.facts;
        let mut rows = vec![
            ("Kind", facts.kind.as_str().to_string(), false),
            ("Created", indexed_at(facts.created), false),
        ];
        if let Some(project) = facts.project.as_ref().filter(|p| !p.is_empty()) {
            rows.push(("Project", project.clone(), true));
        }
        rows.push(("Origin", facts.origin.as_str().to_string(), false));
        rows.push(("Confidence", facts.confidence.to_string(), false));
        if let Some(session) = facts.session.as_ref().filter(|s| !s.is_empty()) {
            rows.push(("Session", session.clone(), true));
        }
        let labels: Vec<&str> = rows.iter().map(|r| r.0).collect();
        let width = label_width(window, &labels);
        let mut out = vec![
            facts_grid("memory-details", rows, width, t).into_any_element(),
            div()
                .id("memory-text")
                .test_support()
                .aria_label(SharedString::from(m.memory.text.clone()))
                .size_sm()
                .child(wrapped_text(collapsed(&m.memory.text)))
                .into_any_element(),
            div()
                .flex()
                .child(
                    button::secondary(
                        "graph-reach",
                        if self.reaching { REACHING } else { ASK_REACH },
                    )
                    .compact()
                    .on_click(cx.listener(|graph, _, _, cx| graph.load_reach(cx))),
                )
                .into_any_element(),
        ];
        if let Some(error) = &self.reach_error {
            out.push(
                section("Reach", t)
                    .child(
                        div()
                            .id("graph-reach-error")
                            .test_support()
                            .aria_label(SharedString::from(error.clone()))
                            .size_sm()
                            .text_color(t.warn)
                            .child(wrapped_text(error.clone())),
                    )
                    .into_any_element(),
            );
        }
        if let Some(reach) = &self.reach {
            out.push(
                section("Reach", t)
                    .when(reach.is_empty(), |d| {
                        d.child(
                            div()
                                .id("graph-reach-nothing")
                                .test_support()
                                .aria_label(NOTHING)
                                .size_sm()
                                .text_color(t.muted)
                                .child(baseline::text(NOTHING)),
                        )
                    })
                    .children(reach.iter().enumerate().map(|(ix, r)| {
                        let relevance = to_fixed(wire(r.relevance), 2);
                        div()
                            .id(SharedString::from(format!("graph-reach-{ix}")))
                            .test_support()
                            .aria_label(SharedString::from(format!("{} {relevance}", r.title)))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .py(px(6.))
                            .relative()
                            .border_t_1()
                            .border_color(transparent_black())
                            .child(css_rule_top(t.sunken))
                            .size_sm()
                            .child(
                                div()
                                    .flex()
                                    .min_w(px(0.))
                                    .overflow_hidden()
                                    .child(baseline::web_baseline(ellipsis_text(r.title.clone()))),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(t.muted)
                                    .child(exact_text(relevance)),
                            )
                    }))
                    .into_any_element(),
            );
        }
        out
    }

    fn project_details(&self, node: &Node, window: &Window, t: &Tokens) -> AnyElement {
        let count = self
            .graph
            .as_ref()
            .map_or(0, |g| project_memories(g, &node.id));
        let width = label_width(window, &PROJECT_LABELS);
        facts_grid(
            "project-details",
            vec![
                ("Path", node.title.clone(), true),
                ("Memories", count.to_string(), false),
            ],
            width,
            t,
        )
        .into_any_element()
    }

    fn panel(
        &self,
        node: &Node,
        window: &Window,
        t: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let graph = self.graph.as_ref();
        let linked = graph.map(|g| neighbours(g, &node.id)).unwrap_or_default();
        let tags = graph.map(|g| tags_of(g, &node.id)).unwrap_or_default();
        let mentions = graph.map(|g| mentions_of(g, &node.id)).unwrap_or_default();
        let body: Vec<AnyElement> = match node.source_type.as_str() {
            "memory" => self.memory_details(node, window, t, cx),
            "project" => vec![self.project_details(node, window, t)],
            _ => self
                .facts
                .clone()
                .map(|f| vec![f.into_any_element()])
                .unwrap_or_default(),
        };
        div()
            .id("graph-panel")
            .test_support()
            .aria_label(SharedString::from(node.title.clone()))
            .occlude()
            .w(px(PANEL_WIDTH))
            .flex_none()
            .bg(t.surface)
            .border_1()
            .border_color(t.line)
            .rounded(px(8.))
            .p(px(20.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .overflow_y_scroll()
            .child(
                div()
                    .id("graph-badges")
                    .test_support()
                    .flex()
                    .gap(px(8.))
                    .items_center()
                    .child(badge(&node.source_type, t.accent, t.accent_soft))
                    .when_some(node.agent, |d, agent| {
                        d.child(badge(&agent_name(agent), t.muted, t.sunken))
                    }),
            )
            .child(
                div()
                    .id("graph-title")
                    .test_support()
                    .aria_label(SharedString::from(node.title.clone()))
                    .semibold()
                    .text_size(px(18.))
                    .line_height(px(24.75))
                    .child(wrapped_text(node.title.clone())),
            )
            .children(body)
            .child(
                section("Linked notes", t)
                    .when(linked.is_empty(), |d| {
                        d.child(
                            div()
                                .id("graph-linked-none")
                                .test_support()
                                .aria_label(NONE_LINKED)
                                .size_sm()
                                .text_color(t.muted)
                                .child(baseline::text(NONE_LINKED)),
                        )
                    })
                    .children(linked.into_iter().enumerate().map(|(ix, n)| {
                        div()
                            .id(SharedString::from(format!("graph-linked-{ix}")))
                            .test_support()
                            .aria_label(SharedString::from(format!("{} {}", n.arrow(), n.title)))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .py(px(6.))
                            .relative()
                            .border_t_1()
                            .border_color(transparent_black())
                            .child(css_rule_top(t.sunken))
                            .size_sm()
                            .child(div().text_color(t.muted).child(arrow(n.arrow(), t.muted)))
                            .child(div().min_w(px(0.)).child(wrapped_text(n.title)))
                    })),
            )
            .when(!tags.is_empty(), |d| {
                d.child(
                    section("Tags", t).child(
                        div()
                            .id("graph-tag-chips")
                            .test_support()
                            .flex()
                            .flex_wrap()
                            .gap(px(6.))
                            .children(tags.into_iter().enumerate().map(|(ix, tag)| {
                                let text = format!("#{tag}");
                                let fits = chip_fits(window, &text, MONO);
                                chip(format!("graph-tag-{ix}"), text, MONO, fits, t)
                            })),
                    ),
                )
            })
            .when(!mentions.is_empty(), |d| {
                d.child(
                    section("Mentions", t).child(
                        div()
                            .id("graph-mention-chips")
                            .test_support()
                            .flex()
                            .flex_wrap()
                            .gap(px(6.))
                            .children(mentions.into_iter().enumerate().map(|(ix, m)| {
                                let fits = chip_fits(window, &m, SANS);
                                chip(format!("graph-mention-{ix}"), m, SANS, fits, t)
                            })),
                    ),
                )
            })
    }

    fn error_view(&self, error: String, t: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("graph")
            .test_support()
            .aria_label("Graph")
            .w_full()
            .flex()
            .flex_col()
            .gap(px(12.))
            .items_center()
            .justify_center()
            .bg(t.viewport)
            .rounded(px(8.))
            .child(banner::error("graph-error", error))
            .child(
                text_button("graph-retry", TRY_AGAIN, TRY_FACE, t.viewport_text)
                    .underline()
                    .on_click(cx.listener(|graph, _, _, cx| graph.load(cx))),
            )
            .into_any_element()
    }

    fn no_documents(&self, t: &Tokens, cx: &mut Context<Self>) -> impl IntoElement {
        let code = |text: &str| {
            Segment::chip(text.to_string(), XS_MONO_FACE, None).coloured(t.viewport_text)
        };
        let plain =
            |text: &str| Segment::text(text.to_string(), XS_FACE).coloured(t.viewport_muted);
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("graph-empty")
                    .test_support()
                    .aria_label(NO_DOCUMENTS)
                    .occlude()
                    .max_w(px(448.))
                    .bg(t.viewport_raised.alpha(0.9))
                    .border_1()
                    .border_color(t.viewport_line)
                    .rounded(px(8.))
                    .px(px(20.))
                    .py(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .size_sm()
                    .text_color(t.viewport_muted)
                    .child(
                        div()
                            .semibold()
                            .text_color(t.viewport_text)
                            .child(baseline::text(NO_DOCUMENTS)),
                    )
                    .child(wrapped_text(NO_DOCUMENTS_HELP))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(button::primary("graph-setup", OPEN_SETUP).on_click(
                                cx.listener(|graph, _, _, cx| {
                                    if let Some(shell) = graph.shell.upgrade() {
                                        shell.update(cx, |shell, cx| shell.request_setup(cx));
                                    }
                                }),
                            ))
                            .child(
                                div()
                                    .id("graph-add-folder")
                                    .test_support()
                                    .aria_label(ADD_FOLDER)
                                    .cursor_pointer()
                                    .px(px(16.))
                                    .py(px(8.))
                                    .border_1()
                                    .border_color(t.viewport_line)
                                    .rounded(px(6.))
                                    .semibold()
                                    .size_sm()
                                    .text_color(t.viewport_text)
                                    .child(exact_text(ADD_FOLDER))
                                    .on_click(cx.listener(|graph, _, _, cx| {
                                        if let Some(shell) = graph.shell.upgrade() {
                                            shell.update(cx, |shell, cx| shell.open_sources(cx));
                                        }
                                    })),
                            ),
                    )
                    .child(div().size_xs().child(wrapped_text(SETUP_HELP)))
                    .child(div().size_xs().child(inline_flow(
                        XS_FACE,
                        vec![
                            plain("Opens Settings › Sources. From a terminal: add it to "),
                            code("sources"),
                            plain(" in config.toml, then run "),
                            code("rhebok index"),
                            plain("."),
                        ],
                    ))),
            )
    }

    fn legend(&self, t: &Tokens) -> impl IntoElement {
        let agents = self.graph.as_ref().map(session_agents).unwrap_or_default();
        div()
            .id("graph-legend")
            .test_support()
            .occlude()
            .absolute()
            .bottom(px(12.))
            .left(px(12.))
            .flex()
            .gap(px(14.))
            .size_xs()
            .text_color(t.viewport_text)
            .bg(t.viewport_raised.alpha(0.9))
            .border_1()
            .border_color(t.viewport_line)
            .rounded(px(6.))
            .px(px(12.))
            .py(px(8.))
            .children(LEGEND.map(|key| {
                let label = legend_label(&agents, key);
                div()
                    .id(SharedString::from(format!("legend-{key}")))
                    .test_support()
                    .aria_label(SharedString::from(label.clone()))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .size(px(8.))
                            .rounded_full()
                            .bg(legend_colour(&t.graph, key)),
                    )
                    .child(exact_text(label))
            }))
    }

    fn tooltip(&self, window: &Window, cx: &App) -> Option<impl IntoElement> {
        let at = self.pointer.map(|p| point(p.x.floor(), p.y.floor()))?;
        let node = self.shown.nodes.get(self.hover?)?;
        let size = self.canvas.get().size;
        let (w, h) = (f64::from(size.width), f64::from(size.height));
        let mut style = window.text_style();
        style.font_family = HELVETICA.into();
        style.font_weight = FontWeight::NORMAL;
        style.font_size = px(TOOLTIP_SIZE).into();
        let room = (w * 0.5).max(150.);
        let line = normal_line_height(cx, TOOLTIP_FACE);
        let width = (f64::from(unrounded_width(window, &node.title, &style)) + 10.).min(room);
        let left = at.x - at.x / w * width;
        let below = !(h > 130. && h - at.y < 100.);
        Some(
            div()
                .id("graph-tooltip")
                .test_support()
                .aria_label(SharedString::from(node.title.clone()))
                .absolute()
                .left(px(left as f32))
                .map(|d| {
                    if below {
                        d.top(px((at.y + 21.) as f32))
                    } else {
                        d.bottom(px((h - at.y + 6.) as f32))
                    }
                })
                .max_w(px(room as f32))
                .px(px(5.))
                .py(px(3.))
                .rounded(px(3.))
                .bg(hsla(0., 0., 0., 0.6))
                .text_color(rgb(0xeeeeee))
                .font_family(HELVETICA)
                .font_weight(FontWeight::NORMAL)
                .text_size(px(TOOLTIP_SIZE))
                .line_height(px(line))
                .child(baseline::text(node.title.clone())),
        )
    }

    fn canvas_element(&self, t: &Tokens, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let shared = self.canvas.clone();
        let painted = self.labels_painted.clone();
        let view = self.view;
        let nodes = self.shown.nodes.clone();
        let edges = self.shown.edges.clone();
        let positions = self
            .layout
            .as_ref()
            .map(|l| l.positions().to_vec())
            .unwrap_or_default();
        let ringed = self.ringed(cx);
        let palette = t.graph;
        let ground = t.ground;
        let ink = t.viewport_text;
        let cursor = match self.gesture {
            Some(Gesture::Node { .. }) => Some(CursorStyle::ClosedHand),
            Some(Gesture::Pan { .. }) => None,
            None => self.hover.map(|_| CursorStyle::PointingHand),
        };
        let element = canvas(
            move |bounds, window, _| {
                shared.set(Canvas {
                    origin: bounds.origin,
                    size: bounds.size,
                });
                window.insert_hitbox(bounds, HitboxBehavior::Normal)
            },
            move |bounds, hitbox, window, cx| {
                paint::paint_grid(bounds, ink, window);
                let frame = Frame {
                    bounds,
                    view,
                    nodes: &nodes[..],
                    edges: &edges[..],
                    positions: &positions,
                    ringed: &ringed,
                    palette,
                };
                painted.set(paint::paint(&frame, window, cx));
                window.paint_quad(paint::corner_mask(bounds, ground));
                if let Some(style) = cursor {
                    window.set_cursor_style(style, &hitbox);
                }
                let canvas = Canvas {
                    origin: bounds.origin,
                    size: bounds.size,
                };
                let down = this.clone();
                let target = hitbox.clone();
                window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble
                        && e.button == MouseButton::Left
                        && target.is_hovered(window)
                    {
                        down.update(cx, |g, cx| g.press(local(e.position, canvas), cx));
                    }
                });
                let moving = this.clone();
                let target = hitbox.clone();
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble {
                        let inside = target.is_hovered(window);
                        moving.update(cx, |g, cx| g.moved(local(e.position, canvas), inside, cx));
                    }
                });
                let up = this.clone();
                window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble && e.button == MouseButton::Left {
                        up.update(cx, |g, cx| g.release(local(e.position, canvas), cx));
                    }
                });
                let pinch = this.clone();
                let target = hitbox.clone();
                window.on_mouse_event(move |e: &PinchEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble && target.is_hovered(window) {
                        let delta = pinch_delta(e.delta);
                        pinch.update(cx, |g, cx| {
                            g.wheel(local(e.position, canvas), delta, true, cx)
                        });
                    }
                });
                let wheel = this;
                let target = hitbox;
                window.on_mouse_event(move |e: &ScrollEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble && target.is_hovered(window) {
                        let (delta, lines) = match e.delta {
                            ScrollDelta::Pixels(p) => {
                                (point(f64::from(p.x), f64::from(p.y)), false)
                            }
                            ScrollDelta::Lines(l) => (point(f64::from(l.x), f64::from(l.y)), true),
                        };
                        let ctrl = e.modifiers.control;
                        wheel.update(cx, |g, cx| {
                            g.wheel(
                                local(e.position, canvas),
                                wheel_delta(delta, lines),
                                ctrl,
                                cx,
                            )
                        });
                    }
                });
            },
        )
        .size_full();
        div()
            .id("graph-canvas")
            .test_support()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(element)
    }
}

impl Render for GraphTab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *tokens(cx);
        if let Some(error) = self.error.clone() {
            return self.error_view(error, &t, cx);
        }
        self.schedule_tick(window, cx);
        if self.gesture.is_none() {
            self.hover = self.pointer.and_then(|at| self.node_under(at));
        }
        let graph = self.graph.clone();
        let selected = self.selected.clone();
        let empty = graph.as_ref().is_some_and(|g| document_count(g) == 0);
        let tooltip = self.tooltip(window, cx);
        let panel = selected.map(|node| self.panel(&node, window, &t, cx));
        let legend = self.legend(&t);
        let toggle_label = sessions_label(graph.as_ref().map_or(0, hidden_sessions), self.sessions);
        div()
            .id("graph")
            .test_support()
            .aria_label("Graph")
            .size_full()
            .flex()
            .gap(px(16.))
            .child(
                div()
                    .id("graph-viewport")
                    .test_support()
                    .relative()
                    .flex_grow(1.)
                    .min_w(px(0.))
                    .h_full()
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(t.viewport)
                    .child(self.canvas_element(&t, cx))
                    .child(
                        div()
                            .id("graph-sessions")
                            .test_support()
                            .role(Role::Button)
                            .aria_label(SharedString::from(toggle_label.clone()))
                            .occlude()
                            .absolute()
                            .top(px(12.))
                            .left(px(12.))
                            .cursor_pointer()
                            .size_xs()
                            .text_color(t.viewport_muted)
                            .bg(t.viewport_raised.alpha(0.9))
                            .border_1()
                            .border_color(t.viewport_line)
                            .rounded(px(6.))
                            .px(px(10.))
                            .py(px(6.))
                            .child(exact_text(toggle_label))
                            .on_click(cx.listener(|graph, _, _, cx| graph.toggle_sessions(cx))),
                    )
                    .when(empty, |d| d.child(self.no_documents(&t, cx)))
                    .child(legend)
                    .when_some(tooltip, |d, tip| d.child(tip)),
            )
            .when_some(panel, |d, panel| d.child(panel))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;

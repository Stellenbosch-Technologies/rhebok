use std::collections::{HashMap, HashSet};

use gpui_kit::base::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rhebok::dashboard::core::search::{SearchOutcome, SearchQuery};
use rhebok::retrieve::{Explain, HitSummary};

use crate::backend::{describe, run_backend};
use crate::shell::Shell;
use crate::theme::{tokens, Face, Tokens, Type, MONO, SANS};
use crate::widgets::banner::{self, Tone};
use crate::widgets::button::Variant;
use crate::widgets::css_box::{css_box_at, css_rule_top, ExactSpan};
use crate::widgets::exact::{row_ending_before, text_style_of, unrounded_width, Tracks};
use crate::widgets::js::{to_fixed, wire};
use crate::widgets::tail::ellipsis_text;
use crate::widgets::text_input::input_state;
use crate::widgets::wrap::wrapped_text;
use crate::widgets::{
    baseline, button, divider, document_facts, exact_text, segmented, text_input, tracked,
    DocumentFacts, ExactLink,
};

pub const TIERS: [&str; 5] = ["instant", "fast", "balanced", "thorough", "exhaustive"];
pub const DEFAULT_TIER: usize = 1;
const GAP: f32 = 12.;
const COLUMN_GAP: f32 = 10.;
pub const CAP: usize = 6;
pub const SUBTITLE_CHARS: usize = 120;
pub const PLACEHOLDER: &str = "Search your knowledge base…";
pub const EMPTY_HINT: &str = "Type a query to watch the pipeline: what each retriever found, how fusion ordered it, and what cleared the gate.";
pub const SUGGESTED: &str = "Setup is done. This query was picked from your own notes to show what rhebok finds; type your own question above.";
pub const EMBEDDING_UNAVAILABLE: &str = "embedding unavailable";
pub const OUTAGE: &str =
    "Ollama is not reachable — search needs the embedding model. Graph and Health still work.";

const PLAIN_STAGES: [&str; 3] = ["vector", "bm25", "graph"];
const NAME_FACE: Face = Face::new(SANS, FontWeight::SEMIBOLD, 11., 16.5);
const COUNT_FACE: Face = Face::new(MONO, FontWeight::NORMAL, 11., 16.5);
const TRACKING_WIDER: f32 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Real,
    PreWeight,
    Unmeasured,
}

#[derive(Debug, Clone)]
pub struct CardSpec {
    pub id: SharedString,
    pub hit: HitSummary,
    pub column: &'static str,
    pub dim: bool,
    pub injected: bool,
    pub gate: Gate,
}

#[derive(Debug, Clone)]
pub enum Entry {
    Card(Box<CardSpec>),
    More { id: SharedString, hidden: usize },
    Rule { id: SharedString, label: String },
}

#[derive(Debug, Clone)]
pub struct ColumnSpec {
    pub key: &'static str,
    pub name: &'static str,
    pub count: usize,
    pub entries: Vec<Entry>,
}

pub fn card_id(column: &str, section: u8, chunk_id: &str) -> SharedString {
    format!("card-{column}-{section}-{chunk_id}").into()
}

pub fn toggle_id(card: &str) -> SharedString {
    format!("{card}-toggle").into()
}

pub fn threshold_text(threshold: f32) -> String {
    to_fixed(wire(threshold), 2)
}

pub fn subtitle_of(hit: &HitSummary) -> String {
    if !hit.heading.is_empty() {
        return hit.heading.clone();
    }
    let title = hit.title.trim().to_lowercase();
    let line = hit
        .excerpt
        .split('\n')
        .map(|l| {
            let l = l.trim_start();
            l.trim_start_matches('#')
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .find(|l| l.chars().any(char::is_alphanumeric) && l.to_lowercase() != title)
        .unwrap_or_default();
    let units: Vec<u16> = line.encode_utf16().collect();
    if units.len() <= SUBTITLE_CHARS {
        return line;
    }
    let cut = String::from_utf16_lossy(&units[..SUBTITLE_CHARS - 1]);
    format!("{}…", cut.trim_end())
}

fn push_capped(
    entries: &mut Vec<Entry>,
    hits: &[&HitSummary],
    column: &'static str,
    section: u8,
    dim: bool,
    injected: impl Fn(&HitSummary) -> bool,
    gate: Gate,
) {
    for hit in hits.iter().take(CAP) {
        entries.push(Entry::Card(Box::new(CardSpec {
            id: card_id(column, section, &hit.chunk_id),
            hit: (*hit).clone(),
            column,
            dim,
            injected: injected(hit),
            gate,
        })));
    }
    if hits.len() > CAP {
        entries.push(Entry::More {
            id: format!("more-{column}-{section}").into(),
            hidden: hits.len() - CAP,
        });
    }
}

pub fn stage<'a>(ex: &'a Explain, name: &str) -> &'a [HitSummary] {
    ex.stages
        .iter()
        .find(|s| s.name == name)
        .map(|s| s.hits.as_slice())
        .unwrap_or(&[])
}

pub fn columns(ex: &Explain) -> Vec<ColumnSpec> {
    let injected: HashSet<&str> = ex.injected.iter().map(String::as_str).collect();
    let mut out = Vec::with_capacity(5);
    for name in PLAIN_STAGES {
        let hits: Vec<&HitSummary> = stage(ex, name).iter().collect();
        let gate = if name == "vector" {
            Gate::PreWeight
        } else {
            Gate::Unmeasured
        };
        let mut entries = Vec::new();
        push_capped(&mut entries, &hits, name, 0, false, |_| false, gate);
        out.push(ColumnSpec {
            key: name,
            name,
            count: hits.len(),
            entries,
        });
    }
    let threshold = ex.threshold;
    let pool = stage(ex, "fused");
    let above: Vec<&HitSummary> = pool.iter().filter(|h| h.relevance >= threshold).collect();
    let below: Vec<&HitSummary> = pool.iter().filter(|h| h.relevance < threshold).collect();
    let mut entries = Vec::new();
    push_capped(
        &mut entries,
        &above,
        "fused",
        0,
        false,
        |h| injected.contains(h.chunk_id.as_str()),
        Gate::Real,
    );
    entries.push(Entry::Rule {
        id: "divider-fused".into(),
        label: format!("gate {}", threshold_text(threshold)),
    });
    push_capped(
        &mut entries,
        &below,
        "fused",
        1,
        true,
        |_| false,
        Gate::Real,
    );
    out.push(ColumnSpec {
        key: "fused",
        name: "fused · weighted",
        count: pool.len(),
        entries,
    });
    let admitted: Vec<&HitSummary> = ex
        .fused
        .iter()
        .filter(|h| injected.contains(h.chunk_id.as_str()))
        .collect();
    let over: Vec<&HitSummary> = ex
        .fused
        .iter()
        .filter(|h| !injected.contains(h.chunk_id.as_str()))
        .collect();
    let mut entries = Vec::new();
    push_capped(
        &mut entries,
        &admitted,
        "injected",
        0,
        false,
        |_| true,
        Gate::Real,
    );
    if !over.is_empty() {
        entries.push(Entry::Rule {
            id: "divider-injected".into(),
            label: "token budget".to_string(),
        });
        push_capped(
            &mut entries,
            &over,
            "injected",
            1,
            true,
            |_| false,
            Gate::Real,
        );
    }
    out.push(ColumnSpec {
        key: "injected",
        name: "injected",
        count: admitted.len(),
        entries,
    });
    out
}

pub fn chunk_line(spec: &CardSpec, threshold: f32) -> String {
    let hit = &spec.hit;
    match spec.gate {
        Gate::Unmeasured => format!("{} · not measured at this stage", hit.chunk_id),
        gate => format!(
            "{} · {} the {} gate{}",
            hit.chunk_id,
            if hit.relevance >= threshold {
                "above"
            } else {
                "below"
            },
            threshold_text(threshold),
            if gate == Gate::PreWeight {
                " (pre-weight)"
            } else {
                ""
            }
        ),
    }
}

pub fn native_score(spec: &CardSpec) -> (String, &'static str) {
    if spec.column == "bm25" {
        (
            format!("bm25 {}", to_fixed(wire(spec.hit.score), 2)),
            "raw score",
        )
    } else {
        ("graph neighbour".to_string(), "no score of its own")
    }
}

pub fn elapsed_line(ex: &Explain) -> String {
    format!(
        "{} ms{}",
        ex.elapsed_ms,
        if ex.degraded { " · degraded" } else { "" }
    )
}

pub fn tier_row(window: &Window, elapsed: Option<&str>) -> (Div, ExactLink) {
    let elapsed_width = elapsed.map_or(px(0.), |line| {
        px(GAP)
            + unrounded_width(
                window,
                line,
                &text_style_of(window, MONO, FontWeight::NORMAL, 12.),
            )
    });
    let search_button = button::frame_width(window, "Search", Variant::Primary, false);
    row_ending_before(px(GAP) + search_button + elapsed_width)
}

pub enum Ran {
    Failed(String),
    Done(Explain),
}

pub struct Search {
    shell: WeakEntity<Shell>,
    input: Option<Entity<InputState>>,
    pending: Option<String>,
    q: String,
    tier: usize,
    ran: Option<Ran>,
    busy: bool,
    err: Option<String>,
    suggested: Option<String>,
    task: Option<Task<()>>,
    open: HashMap<SharedString, Entity<DocumentFacts>>,
    _input_events: Option<Subscription>,
    pointer: Option<FocusHandle>,
}

impl Search {
    pub fn new(shell: WeakEntity<Shell>) -> Search {
        Search {
            shell,
            input: None,
            pending: None,
            q: String::new(),
            tier: DEFAULT_TIER,
            ran: None,
            busy: false,
            err: None,
            suggested: None,
            task: None,
            open: HashMap::new(),
            _input_events: None,
            pointer: None,
        }
    }

    fn take_focus(&mut self, window: &mut Window, cx: &mut Context<Search>) {
        let pointer = self
            .pointer
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        window.focus(&pointer, cx);
    }

    pub fn query(&self) -> &str {
        &self.q
    }

    pub fn tier(&self) -> usize {
        self.tier
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    pub fn ran(&self) -> Option<&Ran> {
        self.ran.as_ref()
    }

    pub fn error(&self) -> Option<&str> {
        self.err.as_deref()
    }

    pub fn open_cards(&self) -> Vec<SharedString> {
        let mut ids: Vec<SharedString> = self.open.keys().cloned().collect();
        ids.sort();
        ids
    }

    pub fn seed(&mut self, query: String, cx: &mut Context<Search>) {
        self.suggested = Some(query.clone());
        self.submit(query, cx);
    }

    pub fn submit(&mut self, query: String, cx: &mut Context<Search>) {
        self.q = query.clone();
        self.pending = Some(query.clone());
        self.run(query, cx);
    }

    pub fn set_tier(&mut self, tier: usize, cx: &mut Context<Search>) {
        self.tier = tier.min(TIERS.len() - 1);
        cx.notify();
    }

    pub fn go(&mut self, cx: &mut Context<Search>) {
        self.run(self.q.clone(), cx);
    }

    fn run(&mut self, q: String, cx: &mut Context<Search>) {
        if q.trim().is_empty() {
            return;
        }
        self.busy = true;
        self.err = None;
        let tier = self.tier as u8;
        self.task = Some(run_backend(
            cx,
            move |backend| {
                backend.search(SearchQuery {
                    q,
                    tier: Some(tier),
                })
            },
            |search: &mut Search, result, cx| {
                search.busy = false;
                match result {
                    Ok(outcome) => search.land(outcome, cx),
                    Err(e) => search.err = Some(describe(&e)),
                }
            },
        ));
        cx.notify();
    }

    fn land(&mut self, outcome: SearchOutcome, cx: &mut Context<Search>) {
        match (outcome.error, outcome.explain) {
            (Some(error), _) => self.ran = Some(Ran::Failed(error)),
            (None, Some(explain)) => {
                let ids: Vec<String> = explain.fused.iter().map(|h| h.doc_id.clone()).collect();
                self.ran = Some(Ran::Done(explain));
                if let Some(shell) = self.shell.upgrade() {
                    shell.update(cx, |shell, cx| shell.set_highlight(ids, cx));
                }
            }
            (None, None) => {}
        }
    }

    fn toggle(&mut self, card: SharedString, doc_id: String, cx: &mut Context<Search>) {
        if self.open.remove(&card).is_none() {
            let facts = document_facts(doc_id, cx);
            self.open.insert(card, facts);
        }
        cx.notify();
    }

    fn dismiss(&mut self, cx: &mut Context<Search>) {
        self.suggested = None;
        cx.notify();
    }

    fn ensure_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Search>,
    ) -> Entity<InputState> {
        if let Some(input) = &self.input {
            if let Some(value) = self.pending.take() {
                input.update(cx, |state, cx| state.set_value(value, window, cx));
            }
            return input.clone();
        }
        self.pending = None;
        let t = *tokens(cx);
        let input = input_state(PLACEHOLDER, &self.q, &t, window, cx);
        self._input_events =
            Some(
                cx.subscribe_in(&input, window, |search, state, event, _, cx| match event {
                    InputEvent::Change => {
                        search.q = state.read(cx).value().to_string();
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => search.go(cx),
                    _ => {}
                }),
            );
        self.input = Some(input.clone());
        input
    }

    fn top_bar(&mut self, window: &mut Window, cx: &mut Context<Search>) -> Div {
        let t = *tokens(cx);
        let input = self.ensure_input(window, cx);
        let this = cx.entity().downgrade();
        let elapsed = match &self.ran {
            Some(Ran::Done(ex)) => Some(elapsed_line(ex)),
            _ => None,
        };
        let (row, tier_end) = tier_row(window, elapsed.as_deref());
        row.flex()
            .items_center()
            .gap(px(GAP))
            .child(
                text_input("search-input", PLACEHOLDER, &input)
                    .flex_grow(1.)
                    .px(px(12.))
                    .py(px(8.))
                    .regular()
                    .size_sm(),
            )
            .child(
                segmented("search-tier", &TIERS, self.tier, move |ix, window, cx| {
                    let _ = this.update(cx, |search, cx| {
                        search.take_focus(window, cx);
                        search.set_tier(ix, cx)
                    });
                })
                .tight()
                .ends_at(&tier_end),
            )
            .child(
                button::primary("search-button", "Search")
                    .disabled(self.busy)
                    .on_click(cx.listener(|search, _, window, cx| {
                        search.take_focus(window, cx);
                        search.go(cx)
                    })),
            )
            .when_some(elapsed, |d, elapsed| {
                d.child(
                    div()
                        .id("search-elapsed")
                        .test_support()
                        .aria_label(SharedString::from(elapsed.clone()))
                        .mono()
                        .size_xs()
                        .text_color(t.muted)
                        .child(baseline::text(elapsed)),
                )
            })
    }

    fn suggestion(&self, t: &Tokens, cx: &mut Context<Search>) -> Option<impl IntoElement> {
        let shown = self.suggested.as_deref().is_some_and(|s| s == self.q);
        shown.then(|| {
            banner::notice("suggested-query", "", Tone::Accent)
                .flex()
                .items_center()
                .gap(px(12.))
                .child(div().flex_grow(1.).child(baseline::text(SUGGESTED)))
                .child(
                    div()
                        .id("suggested-dismiss")
                        .test_support()
                        .role(Role::Button)
                        .aria_label("dismiss")
                        .flex_none()
                        .size_xs()
                        .text_color(t.accent)
                        .cursor_pointer()
                        .on_click(cx.listener(|search, _, window, cx| {
                            search.take_focus(window, cx);
                            search.dismiss(cx)
                        }))
                        .child(baseline::text("dismiss")),
                )
        })
    }

    fn column(
        &self,
        spec: &ColumnSpec,
        threshold: f32,
        span: ExactSpan,
        t: &Tokens,
        cx: &mut Context<Search>,
    ) -> impl IntoElement {
        let count = spec.count.to_string();
        let line = [NAME_FACE, COUNT_FACE];
        div()
            .id(SharedString::from(format!("column-{}", spec.key)))
            .test_support()
            .aria_label(spec.name)
            .flex()
            .flex_col()
            .gap(px(8.))
            .flex_basis(px(0.))
            .flex_grow(1.)
            .min_w(px(0.))
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .px(px(2.))
                    .child(
                        tracked(
                            spec.name.to_uppercase(),
                            SANS,
                            FontWeight::SEMIBOLD,
                            px(11.),
                            TRACKING_WIDER,
                        )
                        .on_line(NAME_FACE, &line)
                        .text_color(t.muted),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("column-{}-count", spec.key)))
                            .test_support()
                            .aria_label(SharedString::from(count.clone()))
                            .child(
                                exact_text(count)
                                    .mono()
                                    .text_size(px(11.))
                                    .text_color(t.muted.opacity(0.7))
                                    .on_line(NAME_FACE, &line),
                            ),
                    ),
            )
            .children(spec.entries.iter().map(|entry| {
                match entry {
                    Entry::Card(card) => self
                        .card(card, threshold, span.clone(), t, cx)
                        .into_any_element(),
                    Entry::More { id, hidden } => {
                        let label = format!("+ {hidden} more");
                        div()
                            .id(id.clone())
                            .test_support()
                            .aria_label(SharedString::from(label.clone()))
                            .text_center()
                            .type_px(11.)
                            .text_color(t.muted.opacity(0.8))
                            .child(baseline::text(label))
                            .into_any_element()
                    }
                    Entry::Rule { id, label } => {
                        divider(id.clone(), label.clone()).into_any_element()
                    }
                }
            }))
    }

    fn score(&self, spec: &CardSpec, t: &Tokens) -> AnyElement {
        if spec.gate == Gate::Unmeasured {
            let (score, note) = native_score(spec);
            let value = if spec.column == "bm25" {
                exact_text(score.clone()).mono().type_px(11.)
            } else {
                exact_text(score.clone()).regular().type_px(11.)
            };
            return div()
                .id(SharedString::from(format!("{}-score", spec.id)))
                .test_support()
                .aria_label(SharedString::from(format!("{score} {note}")))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_color(t.muted)
                .child(value)
                .child(div().flex_grow(1.))
                .child(
                    tracked(
                        note.to_uppercase(),
                        SANS,
                        FontWeight::NORMAL,
                        px(9.),
                        TRACKING_WIDER,
                    )
                    .line_height(px(13.5)),
                )
                .into_any_element();
        }
        let relevance = wire(spec.hit.relevance);
        let value = to_fixed(relevance, 3);
        let fraction = relevance.clamp(0., 1.) as f32;
        div()
            .id(SharedString::from(format!("{}-bar", spec.id)))
            .test_support()
            .aria_label(SharedString::from(value.clone()))
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .flex_grow(1.)
                    .h(px(5.))
                    .bg(t.sunken)
                    .rounded(px(4.))
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .w(relative(fraction))
                            .bg(t.accent)
                            .rounded(px(4.)),
                    ),
            )
            .child(exact_text(value).mono().type_px(11.))
            .into_any_element()
    }

    fn card(
        &self,
        spec: &CardSpec,
        threshold: f32,
        span: ExactSpan,
        t: &Tokens,
        cx: &mut Context<Search>,
    ) -> impl IntoElement {
        let hit = &spec.hit;
        let facts = self.open.get(&spec.id).cloned();
        let open = facts.is_some();
        let dot = if hit.source_type == "transcript" {
            t.graph.session
        } else {
            t.accent
        };
        let subtitle = subtitle_of(hit);
        let description = match (spec.injected, spec.dim) {
            (true, _) => Some("injected"),
            (false, true) => Some("dimmed"),
            _ => None,
        };
        let id = spec.id.clone();
        let doc_id = hit.doc_id.clone();
        let header = div()
            .id(toggle_id(&spec.id))
            .test_support()
            .role(Role::Button)
            .aria_label(SharedString::from(hit.title.clone()))
            .aria_expanded(open)
            .flex()
            .items_center()
            .gap(px(6.))
            .min_w(px(0.))
            .cursor_pointer()
            .on_click(
                cx.listener(move |search, _, _, cx| search.toggle(id.clone(), doc_id.clone(), cx)),
            )
            .child(div().flex_none().size(px(7.)).rounded_full().bg(dot))
            .child(
                div()
                    .min_w(px(0.))
                    .size_xs()
                    .medium()
                    .child(baseline::web_baseline(ellipsis_text(hit.title.clone()))),
            )
            .when_some(hit.memory_kind.clone(), |d, kind| {
                d.child(
                    div().flex_none().child(
                        tracked(
                            kind.to_uppercase(),
                            SANS,
                            FontWeight::SEMIBOLD,
                            px(9.),
                            TRACKING_WIDER,
                        )
                        .line_height(px(13.5))
                        .text_color(t.accent)
                        .framed(t.sunken, 4., 4., 1.),
                    ),
                )
            })
            .child(
                div()
                    .flex_none()
                    .type_px(10.)
                    .text_color(t.muted)
                    .child(baseline::text(if open { "▾" } else { "▸" })),
            );
        div()
            .id(spec.id.clone())
            .test_support()
            .aria_label(SharedString::from(hit.title.clone()))
            .when_some(description, |d, text| d.aria_description(text))
            .flex()
            .flex_col()
            .gap(px(6.))
            .relative()
            .px(px(10.))
            .py(px(8.))
            .border_1()
            .border_color(transparent_black())
            .rounded(px(6.))
            .when(spec.dim, |d| d.opacity(0.45))
            .child(css_box_at(
                t.surface,
                Some(if spec.injected { t.accent } else { t.line }),
                6.,
                Some(span),
            ))
            .child(header)
            .when(!subtitle.is_empty(), |d| {
                d.child(
                    div()
                        .type_px(11.)
                        .text_color(t.muted)
                        .mt(px(-4.))
                        .pl(px(13.))
                        .child(baseline::web_baseline(ellipsis_text(subtitle))),
                )
            })
            .child(self.score(spec, t))
            .when_some(facts, |d, facts| {
                let excerpt = if hit.excerpt.is_empty() {
                    "no excerpt".to_string()
                } else {
                    hit.excerpt.clone()
                };
                let line = chunk_line(spec, threshold);
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .pt(px(4.))
                        .relative()
                        .border_t_1()
                        .border_color(transparent_black())
                        .child(css_rule_top(t.sunken))
                        .child(
                            div()
                                .id(SharedString::from(format!("{}-excerpt", spec.id)))
                                .test_support()
                                .aria_label(SharedString::from(excerpt.clone()))
                                .text_size(px(11.))
                                .line_height(px(17.875))
                                .text_color(t.muted)
                                .max_h(px(224.))
                                .overflow_y_scroll()
                                .child(wrapped_text(excerpt)),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("{}-chunk", spec.id)))
                                .test_support()
                                .aria_label(SharedString::from(line.clone()))
                                .mono()
                                .type_px(10.)
                                .text_color(t.muted)
                                .child(baseline::text(line)),
                        )
                        .child(facts),
                )
            })
    }
}

impl Render for Search {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *tokens(cx);
        let top = self.top_bar(window, cx);
        let suggestion = self.suggestion(&t, cx);
        let failure = match (&self.err, &self.ran) {
            (Some(err), _) => Some(err.clone()),
            (None, Some(Ran::Failed(error))) => Some(if error == EMBEDDING_UNAVAILABLE {
                OUTAGE.to_string()
            } else {
                error.clone()
            }),
            _ => None,
        };
        let results = match (&self.err, &self.ran) {
            (None, Some(Ran::Done(ex))) => Some((columns(ex), ex.threshold)),
            _ => None,
        };
        let shown: HashSet<SharedString> = results
            .iter()
            .flat_map(|(columns, _)| columns.iter().flat_map(|c| c.entries.iter()))
            .filter_map(|entry| match entry {
                Entry::Card(card) => Some(card.id.clone()),
                _ => None,
            })
            .collect();
        self.open.retain(|id, _| shown.contains(id));
        let empty = self.err.is_none() && self.ran.is_none();
        let pointer = self
            .pointer
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        div()
            .id("search")
            .track_focus(&pointer)
            .test_support()
            .aria_label("Search")
            .size_full()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(top)
            .when_some(suggestion, |d, banner| d.child(banner))
            .when_some(failure, |d, failure| {
                d.child(banner::error("search-error", failure))
            })
            .when_some(results, |d, (columns, threshold)| {
                let tracks = Tracks::new(0., COLUMN_GAP, columns.len());
                d.child(
                    div()
                        .id("search-results")
                        .test_support()
                        .relative()
                        .flex()
                        .gap(px(COLUMN_GAP))
                        .flex_grow(1.)
                        .min_h(px(0.))
                        .overflow_scroll()
                        .children(tracks.edges())
                        .children(columns.iter().enumerate().map(|(ix, column)| {
                            self.column(column, threshold, tracks.span(ix, None), &t, cx)
                        })),
                )
            })
            .when(empty, |d| {
                d.child(
                    div()
                        .id("search-empty")
                        .test_support()
                        .aria_label(EMPTY_HINT)
                        .p(px(8.))
                        .size_sm()
                        .text_color(t.muted)
                        .child(baseline::text(EMPTY_HINT)),
                )
            })
    }
}

#[cfg(test)]
mod tests;

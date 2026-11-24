use std::time::Duration;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::widgets::baseline;
use rhebok::dashboard::core::index::IndexRun;
use rhebok::dashboard::core::stats::{Progress, Stats};
use rhebok::dashboard::core::DashboardError;

use crate::backend::run_backend;
use crate::shell::grouped;
use crate::theme::{tokens, Tokens, Type, MONO, SANS};
use crate::widgets::banner::{self, Tone};
use crate::widgets::js::to_fixed;
use crate::widgets::{button, exact_text, panel, tracked};

pub const RUN_POLL: Duration = Duration::from_millis(700);

#[derive(Debug, Clone, PartialEq)]
pub struct Slice {
    pub key: String,
    pub label: String,
    pub count: u32,
    pub colour: Hsla,
}

pub fn agent_name(agent: &str) -> &str {
    match agent {
        "claude-code" => "Claude Code",
        "codex" => "Codex",
        other => other,
    }
}

const SECOND_AGENT: u32 = 0xC4D3DE;

pub fn slices(stats: &Stats, t: &Tokens) -> Vec<Slice> {
    let agents: Vec<(&String, &u32)> = stats
        .sessions_by_agent
        .iter()
        .filter(|(_, n)| **n > 0)
        .collect();
    let mut sources: Vec<(&String, &u32)> = stats.by_source.iter().collect();
    sources.sort_by(|a, b| a.0.cmp(b.0));
    let mut out = Vec::new();
    for (source, count) in sources {
        match source.as_str() {
            "transcript" if agents.len() > 1 => {
                for (ix, (agent, n)) in agents.iter().enumerate() {
                    out.push(Slice {
                        key: format!("transcript:{agent}"),
                        label: format!("{} sessions", agent_name(agent)),
                        count: **n,
                        colour: if ix == 0 {
                            t.graph.session
                        } else {
                            rgb(SECOND_AGENT).into()
                        },
                    });
                }
            }
            "transcript" => out.push(Slice {
                key: source.clone(),
                label: match agents.first() {
                    Some((agent, _)) => format!("{} sessions", agent_name(agent)),
                    None => "sessions".to_string(),
                },
                count: *count,
                colour: t.graph.session,
            }),
            "markdown" => out.push(Slice {
                key: source.clone(),
                label: "notes".to_string(),
                count: *count,
                colour: t.graph.note,
            }),
            _ => out.push(Slice {
                key: source.clone(),
                label: source.clone(),
                count: *count,
                colour: t.graph.project,
            }),
        }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.count));
    out
}

pub fn progress_line(p: &rhebok::dashboard::core::stats::RunningProgress) -> String {
    format!(
        "{}%, {}/{} documents, about {} min left",
        to_fixed(p.pct, 1),
        p.docs_done,
        p.docs_total,
        ((p.eta_s as f64) / 60.).round().max(1.)
    )
}

pub fn running_line(p: &rhebok::dashboard::core::stats::RunningProgress) -> String {
    format!(
        "{}% · {}/{} docs",
        to_fixed(p.pct, 1),
        p.docs_done,
        p.docs_total
    )
}

#[derive(Default)]
pub struct IndexPanel {
    pub starting: bool,
    pub note: Option<String>,
    pub task: Option<Task<()>>,
}

pub fn index_now_label(indexing: bool, starting: bool) -> &'static str {
    if indexing {
        "Indexing…"
    } else if starting {
        "Starting…"
    } else {
        "Index now"
    }
}

fn big(id: &'static str, text: String) -> impl IntoElement {
    div()
        .id(id)
        .test_support()
        .aria_label(text.clone())
        .text_size(px(30.))
        .line_height(px(30.))
        .semibold()
        .tabular()
        .child(exact_text(text))
}

fn caption(text: String, t: &Tokens) -> impl IntoElement {
    div()
        .pt(px(4.))
        .size_sm()
        .text_color(t.muted)
        .child(exact_text(text))
}

pub fn index_panel(
    stats: &Stats,
    progress: &Progress,
    state: &IndexPanel,
    t: &Tokens,
    on_index: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> crate::widgets::panel::Panel {
    let indexing = progress.is_running();
    let slices = slices(stats, t);
    let total: u32 = slices.iter().map(|s| s.count).sum();
    let per_doc = if stats.documents > 0 {
        (stats.chunks as f64 / stats.documents as f64).round() as i64
    } else {
        0
    };
    let chunks_caption = if per_doc > 0 {
        format!("chunks, about {per_doc} per document")
    } else {
        "chunks".to_string()
    };
    let model = stats
        .model
        .as_deref()
        .map(|m| m.split('@').next().unwrap_or(m).to_string())
        .unwrap_or_else(|| "no model yet".to_string());
    let aside = div()
        .flex()
        .flex_grow(1.)
        .justify_end()
        .child(
            button::secondary("index-now", index_now_label(indexing, state.starting))
                .disabled(state.starting || indexing)
                .on_click(on_index),
        )
        .into_any_element();
    panel("Index", Some(aside), "index-panel")
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_x(px(40.))
                .gap_y(px(16.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(big("index-documents", grouped(stats.documents)))
                        .child(caption("documents".to_string(), t)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(big("index-chunks", grouped(stats.chunks)))
                        .child(caption(chunks_caption, t)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w(px(0.))
                        .child(
                            div()
                                .id("index-model")
                                .test_support()
                                .aria_label(model.clone())
                                .pt(px(6.))
                                .text_size(px(18.))
                                .line_height(px(22.5))
                                .semibold()
                                .truncate()
                                .child(baseline::text(model)),
                        )
                        .child(caption("embedding model, 512-token chunks".to_string(), t)),
                ),
        )
        .when(total > 0, |p| {
            p.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .h(px(12.))
                            .rounded(px(4.))
                            .overflow_hidden()
                            .bg(t.sunken)
                            .gap(px(1.))
                            .children(slices.iter().enumerate().map(|(ix, s)| {
                                div()
                                    .h_full()
                                    .w(relative(s.count as f32 / total as f32))
                                    .min_w(px(4.))
                                    .bg(s.colour)
                                    .when(ix == 0, |d| d.rounded_l(px(4.)))
                                    .when(ix + 1 == slices.len(), |d| d.rounded_r(px(4.)))
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_x(px(20.))
                            .gap_y(px(4.))
                            .size_sm()
                            .children(slices.iter().map(|s| {
                                div()
                                    .id(SharedString::from(format!("slice-{}", s.key)))
                                    .test_support()
                                    .aria_label(format!(
                                        "{} {}",
                                        grouped(i64::from(s.count)),
                                        s.label
                                    ))
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(div().size(px(10.)).rounded(px(4.)).bg(s.colour))
                                    .child(
                                        exact_text(grouped(i64::from(s.count))).tabular().medium(),
                                    )
                                    .child(exact_text(s.label.clone()).text_color(t.muted))
                            })),
                    ),
            )
        })
        .when_some(
            match progress {
                Progress::Running(p) => Some(p.clone()),
                Progress::Idle { .. } => None,
            },
            |panel, p| {
                let line = progress_line(&p);
                panel.child(
                    div()
                        .id("index-progress")
                        .test_support()
                        .aria_label(line.clone())
                        .flex()
                        .items_center()
                        .gap(px(14.))
                        .pt(px(4.))
                        .child(
                            div()
                                .flex_grow(1.)
                                .h(px(8.))
                                .bg(t.sunken)
                                .rounded(px(4.))
                                .overflow_hidden()
                                .child(
                                    div()
                                        .h_full()
                                        .bg(t.accent)
                                        .rounded(px(4.))
                                        .w(relative((p.pct / 100.).clamp(0., 1.) as f32)),
                                ),
                        )
                        .child(
                            div()
                                .flex_none()
                                .size_sm()
                                .tabular()
                                .child(baseline::text(line)),
                        ),
                )
            },
        )
        .when_some(state.note.clone(), |panel, note| {
            panel.child(
                div()
                    .id("index-note")
                    .test_support()
                    .aria_label(note.clone())
                    .size_sm()
                    .text_color(t.warn)
                    .child(baseline::text(note)),
            )
        })
}

#[derive(Debug, Clone)]
pub enum IndexState {
    Idle,
    Starting {
        reindex: bool,
    },
    Running {
        reindex: bool,
        pid: u32,
        progress: Progress,
    },
    Done {
        reindex: bool,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexFinished;

pub struct IndexStatus {
    id: SharedString,
    state: IndexState,
    start: Option<Task<()>>,
    poll: Option<Task<()>>,
    tick: Option<Task<()>>,
    tick_in_flight: bool,
}

impl EventEmitter<IndexFinished> for IndexStatus {}

pub fn failure_reason(run: &IndexRun) -> String {
    let exit = match run.code {
        None => " on a signal".to_string(),
        Some(code) => format!(" with code {code}"),
    };
    format!(
        "The index failed: rhebok index exited{exit}; its output is in {}",
        run.log
    )
}

pub fn not_started(e: &DashboardError) -> String {
    match e {
        DashboardError::Conflict(m) => {
            format!("Not started: {m}. Wait for it to finish, then try again.")
        }
        other => format!("Not started: {other}"),
    }
}

impl IndexStatus {
    pub fn new(id: impl Into<SharedString>) -> IndexStatus {
        IndexStatus {
            id: id.into(),
            state: IndexState::Idle,
            start: None,
            poll: None,
            tick: None,
            tick_in_flight: false,
        }
    }

    pub fn state(&self) -> &IndexState {
        &self.state
    }

    pub fn busy(&self) -> bool {
        matches!(
            self.state,
            IndexState::Starting { .. } | IndexState::Running { .. }
        )
    }

    pub fn start(&mut self, reindex: bool, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        self.state = IndexState::Starting { reindex };
        self.start = Some(run_backend(
            cx,
            move |backend| backend.index_start(reindex),
            |status: &mut IndexStatus, result, cx| match result {
                Ok(started) => status.running(started.reindex, started.pid, cx),
                Err(e) => {
                    status.state = IndexState::Failed {
                        reason: not_started(&e),
                    }
                }
            },
        ));
        cx.notify();
    }

    fn running(&mut self, reindex: bool, pid: u32, cx: &mut Context<Self>) {
        self.state = IndexState::Running {
            reindex,
            pid,
            progress: Progress::Idle { idle: true },
        };
        self.poll = Some(cx.spawn(async move |this, cx| loop {
            if this.update(cx, |status, cx| status.tick(pid, cx)).is_err() {
                break;
            }
            cx.background_executor().timer(RUN_POLL).await;
        }));
    }

    pub(super) fn tick(&mut self, pid: u32, cx: &mut Context<Self>) {
        if self.tick_in_flight {
            return;
        }
        self.tick_in_flight = true;
        self.tick = Some(run_backend(
            cx,
            |backend| (backend.progress(), backend.index_status()),
            move |status: &mut IndexStatus, (progress, run), cx| {
                status.tick_in_flight = false;
                let IndexState::Running {
                    reindex,
                    pid: running,
                    ..
                } = status.state
                else {
                    return;
                };
                if running != pid {
                    return;
                }
                if let Some(run) = run.run.filter(|r| r.pid == pid && r.finished) {
                    status.poll = None;
                    status.state = if run.ok == Some(true) {
                        IndexState::Done {
                            reindex: run.reindex,
                        }
                    } else {
                        IndexState::Failed {
                            reason: failure_reason(&run),
                        }
                    };
                    cx.emit(IndexFinished);
                    return;
                }
                if let Ok(progress) = progress {
                    status.state = IndexState::Running {
                        reindex,
                        pid,
                        progress,
                    };
                }
            },
        ));
    }
}

impl Render for IndexStatus {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *tokens(cx);
        self.element(self.id.clone(), &t)
    }
}

impl IndexStatus {
    pub fn element(&self, id: SharedString, t: &Tokens) -> AnyElement {
        match &self.state {
            IndexState::Idle => div().id(id).into_any_element(),
            IndexState::Starting { reindex } => {
                let text = format!(
                    "Starting {}…",
                    if *reindex {
                        "a full re-index"
                    } else {
                        "an index"
                    }
                );
                div()
                    .id(id)
                    .test_support()
                    .aria_label(text.clone())
                    .size_sm()
                    .text_color(t.muted)
                    .child(baseline::text(text))
                    .into_any_element()
            }
            IndexState::Running {
                reindex, progress, ..
            } => {
                let (pct, text) = match progress {
                    Progress::Running(p) => (p.pct, running_line(p)),
                    Progress::Idle { .. } => (0., "preparing…".to_string()),
                };
                div()
                    .id(id)
                    .test_support()
                    .aria_label(text.clone())
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        tracked(
                            if *reindex { "RE-INDEXING" } else { "INDEXING" },
                            SANS,
                            FontWeight::SEMIBOLD,
                            px(11.),
                            0.05,
                        )
                        .text_color(t.muted),
                    )
                    .child(
                        div()
                            .flex_grow(1.)
                            .h(px(8.))
                            .bg(t.sunken)
                            .rounded(px(4.))
                            .overflow_hidden()
                            .child(
                                div()
                                    .h_full()
                                    .bg(t.accent)
                                    .rounded(px(4.))
                                    .w(relative((pct / 100.).clamp(0., 1.) as f32)),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .font_family(MONO)
                            .size_xs()
                            .child(baseline::text(text)),
                    )
                    .into_any_element()
            }
            IndexState::Done { reindex } => {
                let text = format!(
                    "{} The header counts and the graph are refreshed.",
                    if *reindex {
                        "Full re-index finished."
                    } else {
                        "Index finished."
                    }
                );
                banner::notice(id, text, Tone::Accent)
                    .compact()
                    .into_any_element()
            }
            IndexState::Failed { reason } => banner::error(id, reason.clone())
                .compact()
                .into_any_element(),
        }
    }
}

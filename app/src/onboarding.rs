use std::cmp::Ordering;
use std::time::Duration;

use gpui_kit::base::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rhebok::dashboard::core::config::{ConfigCheck, ConfigSave};
use rhebok::dashboard::core::graph::Graph;
use rhebok::dashboard::core::DashboardError;
use serde_json::Value as Json;
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

use crate::backend::{describe, run_backend};
use crate::shell::Shell;
use crate::tabs::health::index_panel::{IndexFinished, IndexState, IndexStatus};
use crate::tabs::integrations::UNREADABLE;
use crate::tabs::settings::list_editor::{
    remove_button, string_list, ListState, CHECKING, DUPLICATE,
};
use crate::tabs::settings::pending::ConfigState;
use crate::tabs::settings::sources::{self, SOURCES_PLACEHOLDER};
use crate::theme::{tokens, Face, Tokens, Type, SANS, SM_FACE, XS_MONO_FACE};
use crate::widgets::agent_card::{self, AgentActions, AgentHost, AgentRow, Agents, CardEvent};
use crate::widgets::baseline::web_baseline;
use crate::widgets::css_box::{css_box, css_rule_top};
use crate::widgets::exact::unrounded_width;
use crate::widgets::inline_flow::{inline_flow, Segment};
use crate::widgets::tail::ellipsis_text;
use crate::widgets::text_input::input_state;
use crate::widgets::wrap::wrapped_text;
use crate::widgets::{banner, baseline, button, exact_text, text_button, text_input, AgentCard};

mod collation_table;
#[cfg(test)]
mod tests;

pub const STEPS: [&str; 3] = ["Pick folders", "Connect agents", "Index"];
pub const GENERIC_QUERY: &str = "what did I work on recently";
pub const NOT_NOTES: [&str; 3] = ["transcript", "memory", "project"];
pub const TITLE: &str = "Set up rhebok";
pub const SUBTITLE: &str = "Nothing is indexed yet. Three steps, and every one can be skipped.";
pub const SKIP: &str = "Skip setup";
pub const FOLDERS_INTRO: &str = "rhebok indexes the folders you name here: notes, docs, PDFs. Folders are walked recursively and a single file works too. They are saved to ";
pub const NO_FOLDERS: &str = "no folders yet";
pub const FOLDER_LABEL: &str = "folder to index";
pub const ADD_FOLDER: &str = "Add folder";
pub const SAVE: &str = "Save and continue";
pub const SAVING: &str = "Saving…";
pub const WITHOUT_SAVING: &str = "continue without saving";
pub const CONFLICT: &str = "config.toml changed on disk while this page was open, so nothing was saved. It has been read again; check the list and save once more.";
pub const READING: &str = "Reading config.toml…";
pub const CONNECT_INTRO: &str = "Connected agents can search rhebok through MCP tools, and some get relevant notes added to every prompt. Each connection writes only rhebok's own entry and keeps the original file as .rhebok-bak. The Integrations tab can change this later.";
pub const NO_AGENT: &str = "No coding agent was found on this machine. The Integrations tab has a snippet for any MCP client.";
pub const LOOKING: &str = "Looking for coding agents on this machine…";
pub const CONTINUE: &str = "Continue";
pub const RECOMMENDED: &str = "recommended";
pub const RECOMMENDED_AGENT: &str = "claude-code";
pub const INDEX_INTRO: &str = "rhebok reads every file, splits it into chunks and embeds each one with the local model. Only new and changed files are read on later runs.";
pub const PICKING: &str = "Picking a first search from your notes…";
pub const TRY_AGAIN: &str = "Try again";
pub const PROBE_DELAY: Duration = Duration::from_millis(250);

pub const ROOT_ID: &str = "onboarding";
pub const STEPS_ID: &str = "setup-steps";
pub const FOLDERS_ID: &str = "setup-folders";
pub const SKIP_ID: &str = "setup-skip";
pub const INPUT_ID: &str = "setup-folder-input";
pub const ADD_ID: &str = "setup-add-folder";
pub const SAVE_ID: &str = "setup-save";
pub const WITHOUT_SAVING_ID: &str = "setup-without-saving";
pub const CONTINUE_ID: &str = "setup-continue";
pub const TRY_AGAIN_ID: &str = "setup-try-again";
pub const INDEX_STATUS_ID: &str = "setup-index-status";

const LINK_FACE: Face = Face::new(SANS, FontWeight::SEMIBOLD, 12., 16.);
const MAX_WIDTH: f32 = 900.;
const CIRCLE: f32 = 24.;
const CONNECTOR: f32 = 40.;
const STEP_GAP: f32 = 8.;
const STEPS_GAP: f32 = 12.;

pub fn step_id(ix: usize) -> SharedString {
    format!("setup-step-{ix}").into()
}

pub fn folder_id(ix: usize) -> SharedString {
    format!("setup-folder-{ix}").into()
}

pub fn remove_id(ix: usize) -> SharedString {
    format!("setup-folder-remove-{ix}").into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Weight {
    primary: u32,
    secondary: u32,
    tertiary: u32,
}

const UNLISTED_SYMBOL: u32 = 1 << 8;
const DIGIT: u32 = 1 << 22;
const LETTER: u32 = 1 << 23;

fn weight_of(ch: char) -> Weight {
    if let Some((_, primary, tertiary)) = collation_table::VARIABLE.iter().find(|(c, ..)| *c == ch)
    {
        return Weight {
            primary: *primary,
            secondary: 0,
            tertiary: *tertiary,
        };
    }
    let primary = if let Some(digit) = ch.to_digit(10) {
        DIGIT + digit
    } else if ch.is_alphabetic() {
        LETTER + ch.to_lowercase().next().map_or(u32::from(ch), u32::from)
    } else {
        UNLISTED_SYMBOL + u32::from(ch)
    };
    Weight {
        primary,
        secondary: 0,
        tertiary: u32::from(ch.is_uppercase()),
    }
}

fn mark_weight(mark: char) -> u32 {
    collation_table::MARKS
        .iter()
        .find(|(m, _)| *m == mark)
        .map_or(u32::from(mark), |(_, w)| *w)
}

fn weights(text: &str) -> Vec<Weight> {
    let mut out: Vec<Weight> = Vec::new();
    for ch in text.nfd() {
        if is_combining_mark(ch) {
            if let Some(last) = out.last_mut() {
                last.secondary = last.secondary * 64 + mark_weight(ch);
            }
            continue;
        }
        out.push(weight_of(ch));
    }
    out
}

pub fn locale_compare(a: &str, b: &str) -> Ordering {
    let (a, b) = (weights(a), weights(b));
    let level = |pick: fn(&Weight) -> u32| {
        let left: Vec<u32> = a.iter().map(pick).collect();
        let right: Vec<u32> = b.iter().map(pick).collect();
        left.cmp(&right)
    };
    level(|w| w.primary)
        .then_with(|| level(|w| w.secondary))
        .then_with(|| level(|w| w.tertiary))
}

pub fn suggest_query(graph: Option<&Graph>) -> String {
    let Some(graph) = graph else {
        return GENERIC_QUERY.to_string();
    };
    let mut notes: Vec<_> = graph
        .nodes
        .iter()
        .filter(|n| !NOT_NOTES.contains(&n.source_type.as_str()) && !n.title.trim().is_empty())
        .collect();
    notes.sort_by(|a, b| {
        b.chunks
            .cmp(&a.chunks)
            .then_with(|| locale_compare(&a.title, &b.title))
    });
    notes
        .first()
        .map(|n| n.title.trim().to_string())
        .unwrap_or_else(|| GENERIC_QUERY.to_string())
}

pub fn effective_sources(view: &Json) -> Vec<String> {
    string_list(view["effective"].get("sources"))
}

#[derive(Default)]
struct Folders {
    state: Option<ConfigState>,
    load_err: Option<String>,
    list: Vec<String>,
    entry: ListState,
    saving: bool,
    notice: Option<String>,
    input: Option<Entity<InputState>>,
    load_task: Option<Task<()>>,
    save_task: Option<Task<()>>,
    _input_events: Option<Subscription>,
}

#[derive(Default)]
struct Connect {
    agents: Option<Agents>,
    err: Option<String>,
    actions: AgentActions,
    load_task: Option<Task<()>>,
}

struct IndexStep {
    status: Entity<IndexStatus>,
    finishing: bool,
    finished: bool,
    suggest_task: Option<Task<()>>,
    _subscriptions: [Subscription; 2],
}

pub struct Onboarding {
    shell: WeakEntity<Shell>,
    step: usize,
    folders: Folders,
    connect: Connect,
    index: Option<IndexStep>,
    scroll: ScrollHandle,
}

impl AgentHost for Onboarding {
    fn agent_actions(&mut self) -> &mut AgentActions {
        &mut self.connect.actions
    }

    fn agents_changed(&mut self, cx: &mut Context<Self>) {
        self.load_agents(cx);
    }
}

impl Onboarding {
    pub fn new(shell: WeakEntity<Shell>, cx: &mut Context<Onboarding>) -> Onboarding {
        let mut onboarding = Onboarding {
            shell,
            step: 0,
            folders: Folders::default(),
            connect: Connect::default(),
            index: None,
            scroll: ScrollHandle::new(),
        };
        onboarding.load_config(cx);
        onboarding
    }

    pub fn step(&self) -> usize {
        self.step
    }

    pub fn folders(&self) -> &[String] {
        &self.folders.list
    }

    pub fn config_read(&self) -> bool {
        self.folders.state.is_some() || self.folders.load_err.is_some()
    }

    pub fn candidate_checked(&self) -> bool {
        !self.folders.entry.trimmed().is_empty() && !self.folders.entry.checking(true)
    }

    pub fn saving(&self) -> bool {
        self.folders.saving
    }

    pub fn notice(&self) -> Option<&str> {
        self.folders.notice.as_deref()
    }

    pub fn agents(&self) -> Option<&Agents> {
        self.connect.agents.as_ref()
    }

    pub fn index_status(&self) -> Option<&Entity<IndexStatus>> {
        self.index.as_ref().map(|i| &i.status)
    }

    pub fn finishing(&self) -> bool {
        self.index.as_ref().is_some_and(|i| i.finishing)
    }

    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll
    }

    fn agents_busy(&self) -> bool {
        self.connect.agents.as_ref().is_some_and(|agents| {
            agents
                .agents
                .iter()
                .any(|a| self.connect.actions.running(&a.id).is_some())
        })
    }

    fn load_config(&mut self, cx: &mut Context<Onboarding>) {
        self.folders.load_task = Some(run_backend(
            cx,
            |backend| backend.config_view(),
            |view: &mut Onboarding, result, _| match result {
                Ok(config) => {
                    view.folders.list = effective_sources(&config.0);
                    view.folders.state = Some(ConfigState::new(config.0));
                    view.folders.load_err = None;
                    view.folders.entry.scheduled = None;
                }
                Err(e) => view.folders.load_err = Some(describe(&e)),
            },
        ));
    }

    pub fn skip(&mut self, cx: &mut Context<Onboarding>) {
        if let Some(shell) = self.shell.upgrade() {
            shell.update(cx, |shell, cx| shell.skip_setup(cx));
        }
    }

    pub fn go_to(&mut self, step: usize, cx: &mut Context<Onboarding>) {
        self.step = step;
        if step == 1 {
            self.connect = Connect::default();
            self.load_agents(cx);
        }
        if step == 2 {
            self.start_index_step(cx);
        }
        cx.notify();
    }

    pub fn set_candidate(&mut self, text: String, cx: &mut Context<Onboarding>) {
        self.folders.entry.candidate = text;
        self.revalidate(cx);
        cx.notify();
    }

    fn revalidate(&mut self, cx: &mut Context<Onboarding>) {
        if self.folders.state.is_none() {
            return;
        }
        let list = self.folders.list.clone();
        let entry = &mut self.folders.entry;
        let trimmed = entry.trimmed().to_string();
        let key = (trimmed.clone(), list.clone());
        if entry.scheduled.as_ref() == Some(&key) {
            return;
        }
        entry.scheduled = Some(key);
        if trimmed.is_empty() {
            entry.task = None;
            return;
        }
        let mut probe: Vec<Json> = list.into_iter().map(Json::String).collect();
        probe.push(Json::String(trimmed.clone()));
        let mut set = serde_json::Map::new();
        set.insert("sources".to_string(), Json::Array(probe));
        let request = ConfigCheck {
            set,
            unset: Vec::new(),
        };
        entry.task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PROBE_DELAY).await;
            if !sources::is_absolute(&trimmed) {
                let _ = this.update(cx, |view, cx| {
                    view.folders.entry.verdict = Some((trimmed, Some(sources::RELATIVE.into())));
                    cx.notify();
                });
                return;
            }
            let call = this.update(cx, move |_, cx| {
                run_backend(
                    cx,
                    move |backend| backend.config_check(request),
                    move |view: &mut Onboarding, result, _| {
                        let problem = result
                            .ok()
                            .and_then(|c| sources::source_problem(&trimmed, &c.errors));
                        view.folders.entry.verdict = Some((trimmed, problem));
                    },
                )
            });
            if let Ok(call) = call {
                call.await;
            }
        }));
    }

    pub fn add_folder(&mut self, cx: &mut Context<Onboarding>) {
        let folders = &mut self.folders;
        if folders.saving || folders.entry.blocked(&folders.list, true) {
            return;
        }
        folders.list.push(folders.entry.trimmed().to_string());
        folders.entry.candidate.clear();
        folders.entry.clear = true;
        self.revalidate(cx);
        cx.notify();
    }

    pub fn remove_folder(&mut self, item: &str, cx: &mut Context<Onboarding>) {
        if self.folders.saving {
            return;
        }
        self.folders.list.retain(|x| x != item);
        self.revalidate(cx);
        cx.notify();
    }

    pub fn save(&mut self, cx: &mut Context<Onboarding>) {
        let folders = &mut self.folders;
        let Some(state) = &folders.state else {
            return;
        };
        if folders.saving || folders.list.is_empty() {
            return;
        }
        let mut set = serde_json::Map::new();
        set.insert(
            "sources".to_string(),
            Json::Array(folders.list.iter().cloned().map(Json::String).collect()),
        );
        let request = ConfigSave {
            etag: state.etag().to_string(),
            set,
            unset: Vec::new(),
        };
        folders.saving = true;
        folders.notice = None;
        folders.save_task = Some(run_backend(
            cx,
            move |backend| backend.config_save(request),
            |view: &mut Onboarding, result, cx| match result {
                Ok(saved) => {
                    view.folders.saving = false;
                    view.folders.state = Some(ConfigState::new(saved.0));
                    view.go_to(1, cx);
                }
                Err(DashboardError::ConfigChanged { .. }) => {
                    view.folders.notice = Some(CONFLICT.to_string());
                    view.reread_after_conflict(cx);
                }
                Err(DashboardError::Unprocessable(errors)) => {
                    view.folders.saving = false;
                    view.folders.notice = Some(format!(
                        "Nothing was saved. {}",
                        DashboardError::Unprocessable(errors)
                    ));
                }
                Err(e) => {
                    view.folders.saving = false;
                    view.folders.notice = Some(format!("Nothing was saved: {}", describe(&e)));
                }
            },
        ));
        cx.notify();
    }

    fn reread_after_conflict(&mut self, cx: &mut Context<Onboarding>) {
        self.folders.save_task = Some(run_backend(
            cx,
            |backend| backend.config_view(),
            |view: &mut Onboarding, result, cx| {
                view.folders.saving = false;
                match result {
                    Ok(config) => {
                        view.folders.state = Some(ConfigState::new(config.0));
                        view.folders.entry.scheduled = None;
                        view.revalidate(cx);
                    }
                    Err(e) => {
                        view.folders.notice = Some(format!("Nothing was saved: {}", describe(&e)))
                    }
                }
            },
        ));
    }

    fn load_agents(&mut self, cx: &mut Context<Onboarding>) {
        self.connect.load_task = Some(run_backend(
            cx,
            |backend| backend.agents(),
            |view: &mut Onboarding, list, _| match agent_card::parse_agents(&list) {
                Ok(agents) => {
                    view.connect.agents = Some(agents);
                    view.connect.err = None;
                }
                Err(e) => {
                    eprintln!("rhebok-app: {e}");
                    view.connect.err = Some(format!("{UNREADABLE} {e}"));
                }
            },
        ));
    }

    pub fn card_event(&mut self, id: &str, event: CardEvent, cx: &mut Context<Onboarding>) {
        let Some(agent) = self
            .connect
            .agents
            .as_ref()
            .and_then(|a| a.agents.iter().find(|row| row.id == id))
            .cloned()
        else {
            return;
        };
        agent_card::handle(self, &agent, event, cx);
    }

    pub fn continue_from_agents(&mut self, cx: &mut Context<Onboarding>) {
        if self.agents_busy() {
            return;
        }
        self.go_to(2, cx);
    }

    fn start_index_step(&mut self, cx: &mut Context<Onboarding>) {
        let status = cx.new(|_| IndexStatus::new(INDEX_STATUS_ID));
        let subscriptions = [
            cx.subscribe(&status, |view, status, _: &IndexFinished, cx| {
                if matches!(status.read(cx).state(), IndexState::Done { .. }) {
                    view.finish(cx);
                }
            }),
            cx.observe(&status, |_, _, cx| cx.notify()),
        ];
        status.update(cx, |status, cx| status.start(false, cx));
        self.index = Some(IndexStep {
            status,
            finishing: false,
            finished: false,
            suggest_task: None,
            _subscriptions: subscriptions,
        });
    }

    pub fn retry_index(&mut self, cx: &mut Context<Onboarding>) {
        if let Some(index) = &self.index {
            index
                .status
                .update(cx, |status, cx| status.start(false, cx));
        }
    }

    fn finish(&mut self, cx: &mut Context<Onboarding>) {
        let Some(index) = &mut self.index else {
            return;
        };
        if index.finished {
            return;
        }
        index.finished = true;
        index.finishing = true;
        index.suggest_task = Some(run_backend(
            cx,
            |backend| backend.graph(),
            |view: &mut Onboarding, graph, cx| {
                let query = suggest_query(graph.as_ref().ok());
                if let Some(shell) = view.shell.upgrade() {
                    cx.defer(move |cx| shell.update(cx, |shell, cx| shell.finish_setup(query, cx)));
                }
            },
        ));
        cx.notify();
    }

    fn ensure_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Onboarding>,
    ) -> Entity<InputState> {
        if let Some(input) = &self.folders.input {
            return input.clone();
        }
        let t = *tokens(cx);
        let input = input_state(SOURCES_PLACEHOLDER, "", &t, window, cx);
        self.folders._input_events =
            Some(
                cx.subscribe_in(&input, window, |view, state, event, _, cx| match event {
                    InputEvent::Change => {
                        let text = state.read(cx).value().to_string();
                        view.set_candidate(text, cx);
                    }
                    InputEvent::PressEnter { .. } => view.add_folder(cx),
                    _ => {}
                }),
            );
        self.folders.input = Some(input.clone());
        input
    }

    fn header(&self, t: &Tokens, cx: &mut Context<Onboarding>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_grow(1.)
                    .min_w(px(0.))
                    .child(div().size_2xl().semibold().child(baseline::text(TITLE)))
                    .child(
                        div()
                            .size_sm()
                            .text_color(t.muted)
                            .child(wrapped_text(SUBTITLE)),
                    ),
            )
            .child(
                text_button(SKIP_ID, SKIP, LINK_FACE, t.muted)
                    .underline()
                    .on_click(cx.listener(|view, _, _, cx| view.skip(cx))),
            )
    }

    fn steps(&self, t: &Tokens, window: &Window) -> impl IntoElement {
        let step = self.step;
        let css_px = |x: f32| (x + 0.5).floor();
        let mut x = 0.;
        let mut parts: Vec<AnyElement> = Vec::new();
        for (ix, label) in STEPS.iter().enumerate() {
            let (fill, ink, border) = match ix.cmp(&step) {
                Ordering::Less => (t.accent, white(), None),
                Ordering::Equal => (t.accent_soft, t.accent, Some(t.accent)),
                Ordering::Greater => (t.sunken, t.muted, None),
            };
            let weight = if ix == step {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::NORMAL
            };
            let mut style = window.text_style();
            style.font_family = SANS.into();
            style.font_weight = weight;
            style.font_size = px(SM_FACE.size).into();
            let width = f32::from(unrounded_width(window, label, &style));
            parts.push(
                div()
                    .id(step_id(ix))
                    .test_support()
                    .aria_label(*label)
                    .aria_selected(ix == step)
                    .absolute()
                    .left(px(css_px(x)))
                    .top_0()
                    .size(px(CIRCLE))
                    .when(border.is_some(), |d| {
                        d.border_1().border_color(transparent_black())
                    })
                    .child(css_box(fill, border, CIRCLE / 2.))
                    .into_any_element(),
            );
            parts.push(
                div()
                    .absolute()
                    .left(px(x))
                    .top_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(CIRCLE))
                    .child(
                        exact_text((ix + 1).to_string())
                            .size_xs()
                            .semibold()
                            .text_color(ink),
                    )
                    .into_any_element(),
            );
            let label_x = x + CIRCLE + STEP_GAP;
            parts.push(
                div()
                    .absolute()
                    .left(px(label_x))
                    .top(px((CIRCLE - SM_FACE.line_height) / 2.))
                    .child(
                        exact_text(*label)
                            .size_sm()
                            .font_weight(weight)
                            .when(ix != step, |d| d.text_color(t.muted)),
                    )
                    .into_any_element(),
            );
            x = label_x + width + STEP_GAP;
            if ix + 1 < STEPS.len() {
                parts.push(
                    div()
                        .absolute()
                        .left(px(css_px(x)))
                        .top(px(css_px((CIRCLE - 1.) / 2.)))
                        .w(px(CONNECTOR))
                        .h(px(1.))
                        .bg(t.line)
                        .into_any_element(),
                );
                x += CONNECTOR;
            }
            x += STEPS_GAP;
        }
        div()
            .id(STEPS_ID)
            .test_support()
            .relative()
            .h(px(CIRCLE))
            .children(parts)
    }

    fn folders_body(&mut self, window: &mut Window, cx: &mut Context<Onboarding>) -> AnyElement {
        let t = *tokens(cx);
        if let Some(err) = &self.folders.load_err {
            return banner::error("setup-load-error", err.clone()).into_any_element();
        }
        let Some(state) = &self.folders.state else {
            return muted_line("setup-reading", READING, &t);
        };
        let path = state.path().to_string();
        let input = self.ensure_input(window, cx);
        let saving = self.folders.saving;
        if std::mem::take(&mut self.folders.entry.clear) {
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        input.update(cx, |state, cx| state.set_disabled(saving, cx));
        let folders = &self.folders;
        let entry = &folders.entry;
        let list = folders.list.clone();
        let trimmed = entry.trimmed().to_string();
        let checking = entry.checking(true);
        let problem = entry.problem(true).map(str::to_string);
        let duplicate = list.contains(&trimmed);
        let add_blocked = saving || entry.blocked(&list, true);
        let intro = div().size_sm().text_color(t.muted).child(inline_flow(
            SM_FACE,
            vec![
                Segment::text(FOLDERS_INTRO, SM_FACE),
                Segment::chip(path, XS_MONO_FACE, None),
                Segment::text(" as ", SM_FACE),
                Segment::chip("sources", XS_MONO_FACE, None),
                Segment::text(".", SM_FACE),
            ],
        ));
        let rows = div()
            .id(FOLDERS_ID)
            .test_support()
            .relative()
            .flex()
            .flex_col()
            .border_1()
            .border_color(transparent_black())
            .child(css_box(transparent_black(), Some(t.line), 6.))
            .when(list.is_empty(), |d| {
                d.child(
                    div()
                        .id("setup-no-folders")
                        .test_support()
                        .aria_label(NO_FOLDERS)
                        .px(px(12.))
                        .py(px(8.))
                        .size_sm()
                        .text_color(t.muted)
                        .child(baseline::text(NO_FOLDERS)),
                )
            })
            .children(list.iter().enumerate().map(|(ix, item)| {
                let removed = item.clone();
                div()
                    .id(folder_id(ix))
                    .test_support()
                    .aria_label(SharedString::from(item.clone()))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(6.))
                    .when(ix > 0, |d| {
                        d.relative()
                            .border_t_1()
                            .border_color(transparent_black())
                            .child(css_rule_top(t.sunken))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_grow(1.)
                            .min_w(px(0.))
                            .mono()
                            .size_xs()
                            .child(web_baseline(ellipsis_text(item.clone()))),
                    )
                    .child(
                        remove_button(remove_id(ix), &t)
                            .disabled(saving)
                            .disabled_opacity(1.)
                            .on_click(
                                cx.listener(move |view, _, _, cx| view.remove_folder(&removed, cx)),
                            ),
                    )
            }));
        let add = div()
            .flex()
            .gap(px(8.))
            .child(
                text_input(INPUT_ID, FOLDER_LABEL, &input)
                    .flex_grow(1.)
                    .min_w(px(0.))
                    .px(px(10.))
                    .py(px(6.))
                    .mono()
                    .size_sm(),
            )
            .child(
                button::secondary(ADD_ID, ADD_FOLDER)
                    .disabled(add_blocked)
                    .on_click(cx.listener(|view, _, _, cx| view.add_folder(cx))),
            );
        let message = |id: &'static str, text: String, colour: Hsla| {
            div()
                .id(id)
                .test_support()
                .aria_label(SharedString::from(text.clone()))
                .size_xs()
                .text_color(colour)
                .child(wrapped_text(text))
        };
        let actions = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                button::primary(SAVE_ID, if saving { SAVING } else { SAVE })
                    .small()
                    .disabled(saving || list.is_empty())
                    .on_click(cx.listener(|view, _, _, cx| view.save(cx))),
            )
            .child(
                text_button(WITHOUT_SAVING_ID, WITHOUT_SAVING, LINK_FACE, t.muted)
                    .underline()
                    .disabled(saving)
                    .disabled_opacity(1.)
                    .on_click(cx.listener(|view, _, _, cx| view.go_to(1, cx))),
            );
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(intro)
            .child(rows)
            .child(add)
            .when(checking, |d| {
                d.child(message("setup-checking", CHECKING.into(), t.muted))
            })
            .when(duplicate, |d| {
                d.child(message("setup-duplicate", DUPLICATE.into(), t.warn))
            })
            .when_some(problem, |d, problem| {
                d.child(message("setup-problem", problem, t.warn).role(Role::Alert))
            })
            .when_some(folders.notice.clone(), |d, notice| {
                d.child(banner::error("setup-notice", notice))
            })
            .child(actions)
            .into_any_element()
    }

    fn card(&self, agent: &AgentRow, cx: &mut Context<Onboarding>) -> AnyElement {
        let id = agent.id.clone();
        AgentCard::new(agent, &self.connect.actions)
            .highlight((agent.id == RECOMMENDED_AGENT).then_some(RECOMMENDED))
            .on_event(
                cx.listener(move |view, event: &CardEvent, _, cx| view.card_event(&id, *event, cx)),
            )
            .into_any_element()
    }

    fn connect_body(&self, cx: &mut Context<Onboarding>) -> AnyElement {
        let t = *tokens(cx);
        let Some(agents) = self.connect.agents.clone() else {
            return match &self.connect.err {
                Some(err) => banner::error("setup-agents-error", err.clone()).into_any_element(),
                None => muted_line("setup-looking", LOOKING, &t),
            };
        };
        let detected: Vec<&AgentRow> = agents.agents.iter().filter(|a| a.shown_as_card()).collect();
        let cards: Vec<AnyElement> = detected.iter().map(|a| self.card(a, cx)).collect();
        let busy = self.agents_busy();
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .size_sm()
                    .text_color(t.muted)
                    .child(wrapped_text(CONNECT_INTRO)),
            )
            .when(detected.is_empty(), |d| {
                d.child(
                    div()
                        .id("setup-no-agents")
                        .test_support()
                        .aria_label(NO_AGENT)
                        .size_sm()
                        .child(wrapped_text(NO_AGENT)),
                )
            })
            .child(agent_card::card_grid(cards))
            .child(
                div().flex().child(
                    button::primary(CONTINUE_ID, CONTINUE)
                        .small()
                        .disabled(busy)
                        .on_click(cx.listener(|view, _, _, cx| view.continue_from_agents(cx))),
                ),
            )
            .into_any_element()
    }

    fn index_body(&self, cx: &mut Context<Onboarding>) -> AnyElement {
        let t = *tokens(cx);
        let Some(index) = &self.index else {
            return div().into_any_element();
        };
        let status = index.status.read(cx);
        let state = status.state().clone();
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .size_sm()
                    .text_color(t.muted)
                    .child(wrapped_text(INDEX_INTRO)),
            )
            .when(!matches!(state, IndexState::Idle), |d| {
                d.child(status.element(INDEX_STATUS_ID.into(), &t))
            })
            .when(index.finishing, |d| {
                d.child(
                    div()
                        .id("setup-finishing")
                        .test_support()
                        .aria_label(PICKING)
                        .size_sm()
                        .text_color(t.muted)
                        .child(wrapped_text(PICKING)),
                )
            })
            .when(matches!(state, IndexState::Failed { .. }), |d| {
                d.child(
                    div().flex().child(
                        button::primary(TRY_AGAIN_ID, TRY_AGAIN)
                            .small()
                            .on_click(cx.listener(|view, _, _, cx| view.retry_index(cx))),
                    ),
                )
            })
            .into_any_element()
    }
}

fn muted_line(id: &'static str, text: &'static str, t: &Tokens) -> AnyElement {
    div()
        .id(id)
        .test_support()
        .aria_label(text)
        .size_sm()
        .text_color(t.muted)
        .child(wrapped_text(text))
        .into_any_element()
}

impl Render for Onboarding {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *tokens(cx);
        let body = match self.step {
            0 => self.folders_body(window, cx),
            1 => self.connect_body(cx),
            _ => self.index_body(cx),
        };
        div()
            .id(ROOT_ID)
            .test_support()
            .aria_label(TITLE)
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(MAX_WIDTH))
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .py(px(16.))
                    .child(self.header(&t, cx))
                    .child(self.steps(&t, window))
                    .child(
                        div()
                            .id("setup-card")
                            .test_support()
                            .relative()
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .p(px(24.))
                            .border_1()
                            .border_color(transparent_black())
                            .child(css_box(t.surface, Some(t.line), 8.))
                            .child(
                                div()
                                    .size_xl()
                                    .semibold()
                                    .child(baseline::text(STEPS[self.step.min(2)])),
                            )
                            .child(body),
                    ),
            )
    }
}

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rhebok::dashboard::core::agents::{AgentList, AgentRequest};
use serde_json::Value;

use crate::backend::run_backend;
use crate::theme::{tokens, Face, Tokens, Type, SANS, XS_FACE, XS_MONO_FACE};
use crate::widgets::css_box::{css_box, css_ring};
use crate::widgets::exact::unrounded_width;
use crate::widgets::tail::ellipsis_text;
use crate::widgets::wrap::wrapped_text;
use crate::widgets::{
    button, change_report, checkbox, exact_text, text_button, tracked, AgentAction, Outcome,
};

pub const CAPABILITIES: [(&str, &str); 5] = [
    ("mcp", "MCP tools"),
    ("prompt_hook", "context on every prompt"),
    ("session_hook", "session start"),
    ("transcripts", "sessions indexed"),
    ("instructions", "instructions"),
];

pub const AGENT_HINTS: [(&str, &str, &str); 5] = [
    (
        "claude-code",
        "the rhebok plugin: MCP tools, context on every prompt and at session start, and its sessions indexed",
        "https://claude.com/claude-code",
    ),
    (
        "codex",
        "MCP tools, context on every prompt, and its sessions indexed",
        "https://github.com/openai/codex",
    ),
    (
        "claude-desktop",
        "rhebok's MCP tools in chat",
        "https://claude.ai/download",
    ),
    (
        "cursor",
        "rhebok's MCP tools in the editor's agent",
        "https://cursor.com",
    ),
    (
        "gemini",
        "MCP tools and context on every prompt",
        "https://github.com/google-gemini/gemini-cli",
    ),
];

pub const INSTRUCTIONS_LABEL: &str = "Also add guidance to AGENTS.md";
pub const INSTRUCTIONS_ON: &str = "AGENTS.md carries rhebok's guidance block.";

const PILL_LINE: f32 = 16.5;
const CHIP_HEIGHT: f32 = 22.5;

pub fn hint(id: &str) -> Option<(&'static str, &'static str)> {
    AGENT_HINTS
        .iter()
        .find(|(agent, _, _)| *agent == id)
        .map(|(_, gives, url)| (*gives, *url))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentDetected {
    pub installed: bool,
    pub version: Option<String>,
    pub config_path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentStatus {
    pub state: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentCapabilities {
    pub mcp: bool,
    pub prompt_hook: bool,
    pub session_hook: bool,
    pub transcripts: bool,
    pub instructions: bool,
}

impl AgentCapabilities {
    pub fn has(&self, key: &str) -> bool {
        match key {
            "mcp" => self.mcp,
            "prompt_hook" => self.prompt_hook,
            "session_hook" => self.session_hook,
            "transcripts" => self.transcripts,
            "instructions" => self.instructions,
            _ => false,
        }
    }

    pub fn labels(&self) -> Vec<(&'static str, &'static str)> {
        CAPABILITIES
            .into_iter()
            .filter(|(key, _)| self.has(key))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRow {
    pub id: String,
    pub name: String,
    pub detected: AgentDetected,
    pub status: AgentStatus,
    pub capabilities: AgentCapabilities,
    pub instructions: Option<bool>,
}

impl AgentRow {
    pub fn shown_as_card(&self) -> bool {
        self.detected.installed || self.status.state != "not_connected"
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentSnippets {
    pub mcp_json: String,
    pub codex_toml: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Agents {
    pub agents: Vec<AgentRow>,
    pub snippets: AgentSnippets,
}

fn field<'a>(v: &'a Value, path: &[&str]) -> Result<&'a Value, String> {
    path.iter().try_fold(v, |at, key| {
        at.get(key)
            .ok_or_else(|| format!("the agent list has no `{}`", path.join(".")))
    })
}

fn need_text(v: &Value, path: &[&str]) -> Result<String, String> {
    field(v, path)?
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("`{}` in the agent list is not text", path.join(".")))
}

fn need_flag(v: &Value, path: &[&str]) -> Result<bool, String> {
    field(v, path)?.as_bool().ok_or_else(|| {
        format!(
            "`{}` in the agent list is not true or false",
            path.join(".")
        )
    })
}

fn text_or_null(v: &Value, path: &[&str]) -> Result<Option<String>, String> {
    match field(v, path)? {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.clone())),
        _ => Err(format!(
            "`{}` in the agent list is neither text nor null",
            path.join(".")
        )),
    }
}

fn optional<T>(
    v: &Value,
    path: &[&str],
    read: impl Fn(&Value, &[&str]) -> Result<T, String>,
) -> Result<Option<T>, String> {
    match field(v, path) {
        Ok(_) => read(v, path).map(Some),
        Err(_) => Ok(None),
    }
}

impl AgentRow {
    pub fn from_json(v: &Value) -> Result<AgentRow, String> {
        let id = need_text(v, &["id"])?;
        let named = |e: String| format!("{id}: {e}");
        let row = || -> Result<AgentRow, String> {
            Ok(AgentRow {
                id: id.clone(),
                name: need_text(v, &["name"])?,
                detected: AgentDetected {
                    installed: need_flag(v, &["detected", "installed"])?,
                    version: text_or_null(v, &["detected", "version"])?,
                    config_path: text_or_null(v, &["detected", "config_path"])?,
                },
                status: AgentStatus {
                    state: need_text(v, &["status", "state"])?,
                    reason: optional(v, &["status", "reason"], need_text)?,
                },
                capabilities: AgentCapabilities {
                    mcp: need_flag(v, &["capabilities", "mcp"])?,
                    prompt_hook: need_flag(v, &["capabilities", "prompt_hook"])?,
                    session_hook: need_flag(v, &["capabilities", "session_hook"])?,
                    transcripts: need_flag(v, &["capabilities", "transcripts"])?,
                    instructions: need_flag(v, &["capabilities", "instructions"])?,
                },
                instructions: optional(v, &["instructions"], need_flag)?,
            })
        };
        row().map_err(named)
    }
}

pub fn parse_agents(list: &AgentList) -> Result<Agents, String> {
    let rows = field(&list.0, &["agents"])?
        .as_array()
        .ok_or("`agents` in the agent list is not a list")?;
    let agents = rows
        .iter()
        .map(AgentRow::from_json)
        .collect::<Result<Vec<_>, _>>()?;
    let snippets = AgentSnippets {
        mcp_json: need_text(&list.0, &["snippets", "mcp_json"])?,
        codex_toml: need_text(&list.0, &["snippets", "codex_toml"])?,
    };
    Ok(Agents { agents, snippets })
}

pub fn status_label(state: &str) -> &'static str {
    match state {
        "connected" => "connected",
        "not_connected" => "not connected",
        "stale" => "needs repair",
        _ => "broken",
    }
}

pub fn confirm_text(name: &str) -> String {
    format!("Remove rhebok from {name}? Only rhebok's own entries are removed; everything else in the file stays.")
}

pub fn locked_text(name: &str) -> String {
    format!("Writing {name}'s config. The card is locked until the server answers.")
}

pub fn instructions_hint(name: &str) -> String {
    format!("Adds a marked block telling {name} when to use rhebok's tools. The global AGENTS.md is read in every repository, so this is off unless you want it.")
}

pub fn card_id(id: &str) -> SharedString {
    format!("agent-{id}").into()
}

pub fn part_id(id: &str, part: &str) -> SharedString {
    format!("agent-{id}-{part}").into()
}

#[derive(Default)]
pub struct AgentActions {
    busy: HashMap<String, AgentAction>,
    last: HashMap<String, Outcome>,
    confirming: HashSet<String>,
    instructions: HashSet<String>,
    tasks: HashMap<String, Task<()>>,
}

impl AgentActions {
    pub fn running(&self, id: &str) -> Option<AgentAction> {
        self.busy.get(id).copied()
    }

    pub fn outcome(&self, id: &str) -> Option<&Outcome> {
        self.last.get(id)
    }

    pub fn confirming(&self, id: &str) -> bool {
        self.confirming.contains(id)
    }

    pub fn instructions(&self, id: &str) -> bool {
        self.instructions.contains(id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardEvent {
    Connect,
    AskDisconnect,
    Disconnect,
    Cancel,
    Instructions(bool),
}

pub trait AgentHost: Sized + 'static {
    fn agent_actions(&mut self) -> &mut AgentActions;
    fn agents_changed(&mut self, cx: &mut Context<Self>);
}

pub fn handle<V: AgentHost>(view: &mut V, agent: &AgentRow, event: CardEvent, cx: &mut Context<V>) {
    let id = agent.id.clone();
    let actions = view.agent_actions();
    match event {
        CardEvent::Connect => {
            actions.confirming.remove(&id);
            let instructions =
                agent.capabilities.instructions && actions.instructions.contains(&id);
            act(view, id, AgentAction::Connect, instructions, cx);
        }
        CardEvent::AskDisconnect => {
            actions.confirming.insert(id);
        }
        CardEvent::Cancel => {
            actions.confirming.remove(&id);
        }
        CardEvent::Disconnect => {
            actions.confirming.remove(&id);
            act(view, id, AgentAction::Disconnect, false, cx);
        }
        CardEvent::Instructions(on) => {
            if on {
                actions.instructions.insert(id);
            } else {
                actions.instructions.remove(&id);
            }
        }
    }
    cx.notify();
}

fn act<V: AgentHost>(
    view: &mut V,
    id: String,
    action: AgentAction,
    instructions: bool,
    cx: &mut Context<V>,
) {
    if view.agent_actions().busy.contains_key(&id) {
        return;
    }
    let request = AgentRequest {
        id: id.clone(),
        instructions,
    };
    let done_id = id.clone();
    let task = run_backend(
        cx,
        move |backend| match action {
            AgentAction::Connect => backend.agent_connect(request),
            AgentAction::Disconnect => backend.agent_disconnect(request),
        },
        move |view: &mut V, result, cx| {
            let actions = view.agent_actions();
            actions
                .last
                .insert(done_id.clone(), Outcome::of(action, result));
            actions.busy.remove(&done_id);
            view.agents_changed(cx);
        },
    );
    let actions = view.agent_actions();
    actions.busy.insert(id.clone(), action);
    actions.last.remove(&id);
    actions.tasks.insert(id, task);
}

pub fn card_grid(cards: Vec<AnyElement>) -> Div {
    let mut cards = cards.into_iter().peekable();
    let mut rows = Vec::new();
    while let Some(first) = cards.next() {
        let cell = |card: AnyElement| {
            div()
                .flex_grow(1.)
                .flex_basis(px(0.))
                .min_w(px(0.))
                .child(card)
        };
        let second = cards.next();
        let lone = second.is_none();
        rows.push(
            div()
                .flex()
                .items_start()
                .gap(px(16.))
                .child(cell(first))
                .when_some(second, |d, card| d.child(cell(card)))
                .when(lone, |d| {
                    d.child(div().flex_grow(1.).flex_basis(px(0.)).min_w(px(0.)))
                }),
        );
    }
    div().flex().flex_col().gap(px(16.)).children(rows)
}

type OnEvent = Rc<dyn Fn(&CardEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct AgentCard {
    agent: AgentRow,
    running: Option<AgentAction>,
    outcome: Option<Outcome>,
    confirming: bool,
    instructions: bool,
    highlight: Option<SharedString>,
    children: Vec<AnyElement>,
    on_event: Option<OnEvent>,
}

impl AgentCard {
    pub fn new(agent: &AgentRow, actions: &AgentActions) -> AgentCard {
        AgentCard {
            agent: agent.clone(),
            running: actions.running(&agent.id),
            outcome: actions.outcome(&agent.id).cloned(),
            confirming: actions.confirming(&agent.id),
            instructions: actions.instructions(&agent.id),
            highlight: None,
            children: Vec::new(),
            on_event: None,
        }
    }

    pub fn highlight(mut self, highlight: Option<&str>) -> Self {
        self.highlight = highlight.map(|h| SharedString::from(h.to_string()));
        self
    }

    pub fn on_event(
        mut self,
        handler: impl Fn(&CardEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_event = Some(Rc::new(handler));
        self
    }

    fn send(&self, event: CardEvent) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let handler = self.on_event.clone();
        move |_, window, cx| {
            if let Some(handler) = &handler {
                handler(&event, window, cx);
            }
        }
    }

    fn header(&self, t: &Tokens) -> impl IntoElement {
        let agent = &self.agent;
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(exact_text(agent.name.clone()).semibold().size_lg())
            .when_some(agent.detected.version.clone(), |d, version| {
                d.child(
                    div()
                        .id(part_id(&agent.id, "version"))
                        .test_support()
                        .aria_label(version.clone())
                        .min_w(px(0.))
                        .mono()
                        .size_xs()
                        .text_color(t.muted)
                        .child(ellipsis_text(version)),
                )
            })
            .when_some(self.highlight.clone(), |d, highlight| {
                d.child(
                    div()
                        .id(part_id(&agent.id, "highlight"))
                        .test_support()
                        .aria_label(highlight.clone())
                        .flex_none()
                        .child(
                            exact_text(highlight)
                                .semibold()
                                .text_size(px(11.))
                                .line_height(px(PILL_LINE))
                                .text_color(t.accent),
                        ),
                )
            })
            .child(div().flex_grow(1.))
            .child(status_pill(&agent.id, &agent.status.state, t))
    }

    fn capabilities(&self, t: &Tokens) -> impl IntoElement {
        div().flex().flex_wrap().gap(px(6.)).children(
            self.agent
                .capabilities
                .labels()
                .into_iter()
                .map(|(key, label)| {
                    div()
                        .id(part_id(&self.agent.id, &format!("capability-{key}")))
                        .test_support()
                        .aria_label(label)
                        .relative()
                        .flex_none()
                        .border_1()
                        .border_color(transparent_black())
                        .px(px(8.))
                        .py(px(2.))
                        .child(css_box(transparent_black(), Some(t.line), CHIP_HEIGHT / 2.))
                        .child(
                            exact_text(label)
                                .text_size(px(11.))
                                .line_height(px(PILL_LINE))
                                .text_color(t.muted),
                        )
                }),
        )
    }

    fn config_line(path: &str, t: &Tokens, window: &Window) -> impl IntoElement {
        let mut style = window.text_style();
        style.font_family = SANS.into();
        style.font_weight = FontWeight::NORMAL;
        style.font_size = px(12.).into();
        let indent = unrounded_width(window, "Config ", &style);
        div()
            .relative()
            .size_xs()
            .text_color(t.muted)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .child(exact_text("Config").on_line(XS_FACE, &[XS_FACE, XS_MONO_FACE])),
            )
            .child(
                div().mono().child(
                    wrapped_text(path.to_string())
                        .break_all()
                        .indent(indent)
                        .on_line(XS_FACE, &[XS_FACE, XS_MONO_FACE]),
                ),
            )
    }

    fn instructions_box(&self, locked: bool, t: &Tokens) -> impl IntoElement {
        let agent = &self.agent;
        let checked = self.instructions;
        let handler = self.on_event.clone();
        let label_handler = self.on_event.clone();
        div()
            .flex()
            .items_start()
            .gap(px(8.))
            .size_sm()
            .child(
                div().mt(px(4.)).child(
                    checkbox(
                        part_id(&agent.id, "instructions"),
                        INSTRUCTIONS_LABEL,
                        checked,
                    )
                    .accent(t.accent)
                    .disabled(locked)
                    .on_toggle(move |on, window, cx| {
                        if let Some(handler) = &handler {
                            handler(&CardEvent::Instructions(on), window, cx);
                        }
                    }),
                ),
            )
            .child(
                div()
                    .id(part_id(&agent.id, "instructions-label"))
                    .test_support()
                    .aria_label(INSTRUCTIONS_LABEL)
                    .flex()
                    .flex_col()
                    .min_w(px(0.))
                    .when(!locked, |d| {
                        d.on_click(move |_, window, cx| {
                            if let Some(handler) = &label_handler {
                                handler(&CardEvent::Instructions(!checked), window, cx);
                            }
                        })
                    })
                    .child(wrapped_text(INSTRUCTIONS_LABEL))
                    .child(
                        div()
                            .size_xs()
                            .text_color(t.muted)
                            .child(wrapped_text(instructions_hint(&agent.name))),
                    ),
            )
    }

    fn confirm_row(&self, locked: bool, t: &Tokens) -> impl IntoElement {
        let agent = &self.agent;
        div()
            .id(part_id(&agent.id, "confirm"))
            .test_support()
            .aria_label(SharedString::from(confirm_text(&agent.name)))
            .relative()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(8.))
            .child(css_box(t.warn_soft, None, 6.))
            .child(
                div()
                    .flex_grow(1.)
                    .min_w(px(0.))
                    .size_sm()
                    .text_color(t.warn)
                    .child(wrapped_text(confirm_text(&agent.name))),
            )
            .child(
                button::danger(part_id(&agent.id, "confirm-disconnect"), "Disconnect")
                    .disabled(locked)
                    .on_click(self.send(CardEvent::Disconnect)),
            )
            .child(
                text_button(
                    part_id(&agent.id, "cancel"),
                    "Cancel",
                    Face::new(SANS, FontWeight::SEMIBOLD, 12., 16.),
                    t.muted,
                )
                .px(px(8.))
                .disabled(locked)
                .disabled_opacity(1.)
                .on_click(self.send(CardEvent::Cancel)),
            )
    }

    fn action_row(&self, locked: bool, t: &Tokens) -> impl IntoElement {
        let agent = &self.agent;
        let state = agent.status.state.as_str();
        let repair = state == "stale" || state == "broken";
        let running = self.running;
        let busy = |action: AgentAction, label: &'static str| {
            (running == Some(action)).then(|| SharedString::from(label))
        };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .when(state == "not_connected", |d| {
                d.child(
                    button::primary(part_id(&agent.id, "connect"), "Connect")
                        .small()
                        .disabled(locked)
                        .busy_label(busy(AgentAction::Connect, "Connecting…"))
                        .on_click(self.send(CardEvent::Connect)),
                )
            })
            .when(repair, |d| {
                d.child(
                    button::primary(part_id(&agent.id, "repair"), "Repair")
                        .small()
                        .disabled(locked)
                        .busy_label(busy(AgentAction::Connect, "Repairing…"))
                        .on_click(self.send(CardEvent::Connect)),
                )
            })
            .when(state != "not_connected", |d| {
                d.child(
                    button::secondary(part_id(&agent.id, "disconnect"), "Disconnect")
                        .disabled(locked)
                        .busy_label(busy(AgentAction::Disconnect, "Disconnecting…"))
                        .on_click(self.send(CardEvent::AskDisconnect)),
                )
            })
            .when(locked, |d| {
                d.child(
                    div()
                        .id(part_id(&agent.id, "locked"))
                        .test_support()
                        .aria_label(SharedString::from(locked_text(&agent.name)))
                        .min_w(px(0.))
                        .size_xs()
                        .text_color(t.muted)
                        .child(wrapped_text(locked_text(&agent.name))),
                )
            })
    }
}

pub fn status_pill(id: &str, state: &str, t: &Tokens) -> impl IntoElement {
    let (background, colour) = match state {
        "connected" => (t.accent_soft, t.accent),
        "not_connected" => (t.sunken, t.muted),
        _ => (t.warn_soft, t.warn),
    };
    let label = status_label(state);
    div()
        .id(part_id(id, "status"))
        .test_support()
        .aria_label(label)
        .flex_none()
        .child(
            tracked(
                label.to_uppercase(),
                SANS,
                FontWeight::SEMIBOLD,
                px(11.),
                0.05,
            )
            .line_height(px(PILL_LINE))
            .text_color(colour)
            .framed(background, 4., 8., 2.),
        )
}

impl ParentElement for AgentCard {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for AgentCard {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *tokens(cx);
        let agent = self.agent.clone();
        let locked = self.running.is_some();
        let state = agent.status.state.clone();
        let border = if self.highlight.is_some() {
            t.accent
        } else {
            t.line
        };
        let children = std::mem::take(&mut self.children);
        div()
            .id(card_id(&agent.id))
            .test_support()
            .aria_label(agent.name.clone())
            .relative()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(20.))
            .border_1()
            .border_color(transparent_black())
            .rounded(px(8.))
            .when(self.highlight.is_some(), |d| {
                d.child(css_ring(t.accent_soft, 2., 10.))
            })
            .child(css_box(t.surface, Some(border), 8.))
            .child(self.header(&t))
            .when_some(agent.status.reason.clone(), |d, reason| {
                d.child(
                    div()
                        .id(part_id(&agent.id, "reason"))
                        .test_support()
                        .aria_label(SharedString::from(reason.clone()))
                        .size_xs()
                        .text_color(t.warn)
                        .child(wrapped_text(reason)),
                )
            })
            .child(self.capabilities(&t))
            .when_some(agent.detected.config_path.clone(), |d, path| {
                d.child(Self::config_line(&path, &t, window))
            })
            .children(children)
            .when(
                agent.capabilities.instructions && state != "connected",
                |d| d.child(self.instructions_box(locked, &t)),
            )
            .when(agent.instructions == Some(true), |d| {
                d.child(
                    div()
                        .size_xs()
                        .text_color(t.muted)
                        .child(wrapped_text(INSTRUCTIONS_ON)),
                )
            })
            .map(|d| {
                if self.confirming {
                    d.child(self.confirm_row(locked, &t))
                } else {
                    d.child(self.action_row(locked, &t))
                }
            })
            .when_some(self.outcome.clone(), |d, outcome| {
                d.child(change_report(
                    part_id(&agent.id, "report"),
                    &outcome,
                    &agent.name,
                    cx,
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{
        div, px, Context, IntoElement, ParentElement, Render, Styled, TestAppContext, Window,
    };

    use super::status_pill;
    use crate::theme::tokens;
    use crate::widgets::button::tests::painted_span;

    struct Header;

    impl Render for Header {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let t = *tokens(cx);
            div()
                .flex()
                .w(px(671.))
                .child(div().flex_grow(1.))
                .child(status_pill("cursor", "connected", &t))
        }
    }

    #[gpui_kit::test]
    fn the_connected_pill_paints_where_chrome_does(_cx: &mut TestAppContext) {
        assert_eq!(
            painted_span(800., || Header, |t| t.accent_soft),
            (1182., 1342.),
            "Chrome lays CONNECTED out at x=591.313 w=79.688 and paints device px 1182..1342"
        );
    }
}

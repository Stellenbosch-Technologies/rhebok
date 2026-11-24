use gpui_kit::base::input::{Textarea, TextareaState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rhebok::dashboard::core::stats::Stats;
use serde_json::Value;

use crate::backend::run_backend;
use crate::shell::{Shell, TabId, TabShown};
use crate::theme::{css_line_box, tokens, Face, Tokens, Type, MONO, SANS, XS_FACE};
use crate::widgets::agent_card::{self, part_id, AgentActions, AgentHost, AgentRow, Agents};
use crate::widgets::css_box::{css_box, css_rule_bottom};
use crate::widgets::text_input::editor_style;
use crate::widgets::wrap::wrapped_text;
use crate::widgets::{
    banner, baseline, button, disclosure, exact_text, text_button, tracked, AgentCard,
};

pub const BLURB: &str = "Connecting writes only rhebok's own entry into each agent's config, keeps the original beside it as .rhebok-bak, and never rewrites a file it cannot parse.";
pub const NO_AGENTS: &str = "No coding agent was found on this machine. Any MCP client can still use rhebok with the snippet below.";
pub const OTHER_MCP_HINT: &str = "Paste one of these into the client's MCP config. Both run the installed rhebok binary by its absolute path.";
pub const LOOKING: &str = "Looking for agents…";
pub const UNREADABLE: &str = "The agent list could not be read:";
pub const SNIPPET_LABELS: [&str; 2] = ["JSON (mcpServers)", "Codex TOML"];
pub const SNIPPET_IDS: [&str; 2] = ["snippet-json", "snippet-toml"];

const H2_FACE: Face = Face::new(SANS, FontWeight::SEMIBOLD, 20., 28.);
const BUTTON_FACE: Face = Face::new(SANS, FontWeight::SEMIBOLD, 12., 16.);
const BUTTON_TEXT_TOP: f32 = 7.;
const SUMMARY_FACE: Face = Face::new(SANS, FontWeight::SEMIBOLD, 14., 20.);
const SUMMARY_COUNT_FACE: Face = Face::new(MONO, FontWeight::MEDIUM, 12., 12. * 20. / 14.);
const SESSIONS_KEY_LINE: f32 = 11. * 20. / 14.;
const SNIPPET_LINE: f32 = 16.;
const SNIPPET_PADDING: f32 = 12.;

pub fn sessions_line(
    agent: &AgentRow,
    config: &Value,
    stats: Option<&Stats>,
) -> Option<(String, String, bool)> {
    if !agent.capabilities.transcripts {
        return None;
    }
    let effective = &config["effective"];
    let all = effective["index_transcripts"].as_bool().unwrap_or(false);
    let codex = agent.id == "codex";
    let on = if codex {
        all && effective["index_codex_sessions"].as_bool().unwrap_or(false)
    } else {
        all
    };
    let key = if codex && all {
        "index_codex_sessions"
    } else {
        "index_transcripts"
    };
    let count = stats.and_then(|s| s.sessions_by_agent.get(&agent.id));
    let mut text = if on {
        "Sessions are indexed".to_string()
    } else {
        "Sessions are not indexed".to_string()
    };
    if let (true, Some(count)) = (on, count) {
        text.push_str(&format!(" ({count} in the index)"));
    }
    let value = js_string(&effective[key]);
    Some((text, format!("{key} = {value}"), on))
}

fn js_string(v: &Value) -> String {
    match v {
        Value::Null => "undefined".to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub fn pre_rows(text: &str) -> usize {
    text.strip_suffix('\n').unwrap_or(text).split('\n').count()
}

pub fn missing_id(id: &str) -> SharedString {
    format!("missing-{id}").into()
}

pub struct Integrations {
    shell: WeakEntity<Shell>,
    agents: Option<Agents>,
    err: Option<String>,
    config: Option<Value>,
    stats: Option<Stats>,
    actions: AgentActions,
    missing_open: bool,
    copied: [bool; 2],
    snippets: Option<[Entity<TextareaState>; 2]>,
    snippet_text: [String; 2],
    scroll: ScrollHandle,
    load_task: Option<Task<()>>,
    config_task: Option<Task<()>>,
    _subscriptions: [Subscription; 2],
}

impl AgentHost for Integrations {
    fn agent_actions(&mut self) -> &mut AgentActions {
        &mut self.actions
    }

    fn agents_changed(&mut self, cx: &mut Context<Self>) {
        self.reload(cx);
    }
}

impl Integrations {
    pub fn new(shell: Entity<Shell>, cx: &mut Context<Integrations>) -> Integrations {
        let subscriptions = [
            cx.subscribe(&shell, |view, _, shown: &TabShown, cx| {
                if shown.0 == TabId::Integrations {
                    view.load_config(cx);
                }
            }),
            cx.observe(&shell, |view, shell, cx| {
                view.stats = shell.read(cx).stats().cloned();
                cx.notify();
            }),
        ];
        let this = cx.entity().downgrade();
        let watched = shell.clone();
        cx.defer(move |cx| {
            let shown = watched.read(cx).tab() == TabId::Integrations;
            let stats = watched.read(cx).stats().cloned();
            let _ = this.update(cx, |view, cx| {
                view.stats = stats;
                if shown {
                    view.load_config(cx);
                }
            });
        });
        let mut view = Integrations {
            shell: shell.downgrade(),
            agents: None,
            err: None,
            config: None,
            stats: None,
            actions: AgentActions::default(),
            missing_open: false,
            copied: [false; 2],
            snippets: None,
            snippet_text: [String::new(), String::new()],
            scroll: ScrollHandle::new(),
            load_task: None,
            config_task: None,
            _subscriptions: subscriptions,
        };
        view.reload(cx);
        view
    }

    pub fn error(&self) -> Option<&str> {
        self.err.as_deref()
    }

    pub fn agents(&self) -> Option<&Agents> {
        self.agents.as_ref()
    }

    pub fn actions(&self) -> &AgentActions {
        &self.actions
    }

    pub fn missing_open(&self) -> bool {
        self.missing_open
    }

    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll
    }

    pub fn reload(&mut self, cx: &mut Context<Integrations>) {
        self.load_task = Some(run_backend(
            cx,
            |backend| backend.agents(),
            |view: &mut Integrations, list, _cx| match agent_card::parse_agents(&list) {
                Ok(agents) => {
                    view.agents = Some(agents);
                    view.err = None;
                }
                Err(e) => {
                    eprintln!("rhebok-app: {e}");
                    view.err = Some(format!("{UNREADABLE} {e}"));
                }
            },
        ));
    }

    fn load_config(&mut self, cx: &mut Context<Integrations>) {
        self.config_task = Some(run_backend(
            cx,
            |backend| backend.config_view(),
            |view: &mut Integrations, result, _cx| {
                view.config = result.ok().map(|v| v.0);
            },
        ));
    }

    pub fn toggle_missing(&mut self, cx: &mut Context<Integrations>) {
        self.missing_open = !self.missing_open;
        cx.notify();
    }

    pub fn card_event(
        &mut self,
        id: &str,
        event: agent_card::CardEvent,
        cx: &mut Context<Integrations>,
    ) {
        let Some(agent) = self
            .agents
            .as_ref()
            .and_then(|a| a.agents.iter().find(|row| row.id == id))
            .cloned()
        else {
            return;
        };
        agent_card::handle(self, &agent, event, cx);
    }

    fn copy(&mut self, ix: usize, cx: &mut Context<Integrations>) {
        let text = self.snippet_text[ix].clone();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied[ix] = true;
        cx.notify();
    }

    fn open_sources(&mut self, cx: &mut Context<Integrations>) {
        if let Some(shell) = self.shell.upgrade() {
            shell.update(cx, |shell, cx| shell.open_sources(cx));
        }
    }

    fn sync_snippets(&mut self, window: &mut Window, cx: &mut Context<Integrations>) {
        let Some(agents) = &self.agents else {
            return;
        };
        let wanted = [
            agents.snippets.mcp_json.clone(),
            agents.snippets.codex_toml.clone(),
        ];
        let t = *tokens(cx);
        if self.snippets.is_none() {
            let mut style = editor_style(&t);
            style.background = t.sunken;
            let make = |text: &str, window: &mut Window, cx: &mut Context<Integrations>| {
                let style = style.clone();
                let rows = pre_rows(text);
                let value = SharedString::from(text.to_string());
                cx.new(|cx| {
                    let mut state = TextareaState::new(window, cx)
                        .rows(rows)
                        .soft_wrap(false)
                        .default_value(value);
                    state.set_editor_style(style);
                    state.set_readonly(true, cx);
                    state
                })
            };
            let json = make(&wanted[0], window, cx);
            let toml = make(&wanted[1], window, cx);
            self.snippets = Some([json, toml]);
            self.snippet_text = wanted;
            return;
        }
        if let Some(states) = self.snippets.clone() {
            for (ix, state) in states.iter().enumerate() {
                if self.snippet_text[ix] != wanted[ix] {
                    let text = wanted[ix].clone();
                    let rows = pre_rows(&text);
                    state.update(cx, |s, cx| {
                        s.set_rows(rows, cx);
                        s.set_value(text, window, cx);
                    });
                    self.snippet_text[ix] = wanted[ix].clone();
                }
            }
        }
    }

    fn header(&self, t: &Tokens, cx: &mut Context<Integrations>) -> impl IntoElement {
        let h2 = css_line_box(cx, H2_FACE, &[H2_FACE]).baseline;
        let blurb = css_line_box(cx, XS_FACE, &[XS_FACE]).baseline;
        let refresh = BUTTON_TEXT_TOP + css_line_box(cx, BUTTON_FACE, &[BUTTON_FACE]).baseline;
        let line = h2.max(blurb).max(refresh);
        div()
            .flex()
            .items_start()
            .gap(px(12.))
            .child(
                exact_text("Integrations")
                    .mt(px(line - h2))
                    .semibold()
                    .size_xl(),
            )
            .child(
                div()
                    .id("integrations-blurb")
                    .test_support()
                    .aria_label(BLURB)
                    .mt(px(line - blurb))
                    .flex_grow(1.)
                    .min_w(px(0.))
                    .size_xs()
                    .text_color(t.muted)
                    .child(wrapped_text(BLURB)),
            )
            .child(
                div().mt(px(line - refresh)).child(
                    button::secondary("integrations-refresh", "Refresh")
                        .on_click(cx.listener(|view, _, _, cx| view.reload(cx))),
                ),
            )
    }

    fn card(&self, agent: &AgentRow, t: &Tokens, cx: &mut Context<Integrations>) -> AnyElement {
        let id = agent.id.clone();
        let sessions = self
            .config
            .as_ref()
            .and_then(|config| sessions_line(agent, config, self.stats.as_ref()));
        AgentCard::new(agent, &self.actions)
            .on_event(
                cx.listener(move |view, event: &agent_card::CardEvent, _, cx| {
                    view.card_event(&id, *event, cx)
                }),
            )
            .when_some(sessions, |card, (text, key, on)| {
                card.child(
                    div()
                        .id(part_id(&agent.id, "sessions"))
                        .test_support()
                        .aria_label(SharedString::from(format!("{text} {key}")))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .size_sm()
                        .child(
                            div()
                                .text_color(if on { t.accent } else { t.muted })
                                .child(wrapped_text(text)),
                        )
                        .child(
                            div()
                                .mono()
                                .text_size(px(11.))
                                .line_height(px(SESSIONS_KEY_LINE))
                                .text_color(t.muted)
                                .child(wrapped_text(key)),
                        )
                        .child(
                            text_button(
                                part_id(&agent.id, "sources"),
                                "Settings › Sources",
                                BUTTON_FACE,
                                t.accent,
                            )
                            .underline()
                            .on_click(cx.listener(|view, _, _, cx| view.open_sources(cx))),
                        ),
                )
            })
            .into_any_element()
    }

    fn grid(
        &self,
        detected: &[&AgentRow],
        t: &Tokens,
        cx: &mut Context<Integrations>,
    ) -> impl IntoElement {
        agent_card::card_grid(
            detected
                .iter()
                .map(|agent| self.card(agent, t, cx))
                .collect(),
        )
    }

    fn not_installed(
        &self,
        missing: &[&AgentRow],
        t: &Tokens,
        cx: &mut Context<Integrations>,
    ) -> impl IntoElement {
        let open = self.missing_open;
        let count = missing.len().to_string();
        div()
            .id("not-installed")
            .test_support()
            .aria_label("Not installed")
            .aria_expanded(open)
            .relative()
            .flex()
            .flex_col()
            .border_1()
            .border_color(transparent_black())
            .px(px(20.))
            .py(px(12.))
            .child(css_box(t.surface, Some(t.line), 8.))
            .child(
                div()
                    .id("not-installed-toggle")
                    .test_support()
                    .role(Role::Button)
                    .aria_label(SharedString::from(format!("Not installed {count}")))
                    .aria_expanded(open)
                    .flex()
                    .cursor_pointer()
                    .on_click(cx.listener(|view, _, _, cx| view.toggle_missing(cx)))
                    .child(disclosure(open, t.ink, SUMMARY_FACE.line_height))
                    .child(
                        exact_text("Not installed ")
                            .semibold()
                            .size_sm()
                            .on_line(SUMMARY_FACE, &[SUMMARY_FACE, SUMMARY_COUNT_FACE]),
                    )
                    .child(
                        exact_text(count)
                            .mono_medium()
                            .text_size(px(12.))
                            .text_color(t.muted)
                            .on_line(SUMMARY_FACE, &[SUMMARY_FACE, SUMMARY_COUNT_FACE]),
                    ),
            )
            .when(open, |d| {
                let last = missing.len().saturating_sub(1);
                d.child(div().flex().flex_col().pt(px(8.)).children(
                    missing.iter().enumerate().map(|(ix, agent)| {
                        let (gives, url) = agent_card::hint(&agent.id).unwrap_or(("", ""));
                        let gives = format!("rhebok would give it {gives}.");
                        div()
                            .id(missing_id(&agent.id))
                            .test_support()
                            .aria_label(SharedString::from(agent.name.clone()))
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .py(px(8.))
                            .when(ix < last, |d| {
                                d.relative()
                                    .border_b_1()
                                    .border_color(transparent_black())
                                    .child(css_rule_bottom(t.sunken))
                            })
                            .child(
                                div()
                                    .w(px(130.))
                                    .flex_none()
                                    .size_sm()
                                    .semibold()
                                    .child(wrapped_text(agent.name.clone())),
                            )
                            .child(
                                div()
                                    .flex_grow(1.)
                                    .min_w(px(0.))
                                    .size_sm()
                                    .text_color(t.muted)
                                    .child(wrapped_text(gives)),
                            )
                            .child(
                                text_button(
                                    SharedString::from(format!("missing-{}-get", agent.id)),
                                    format!("Get {}", agent.name),
                                    BUTTON_FACE,
                                    t.accent,
                                )
                                .underline()
                                .flex_none()
                                .on_click(move |_, _, cx| cx.open_url(url)),
                            )
                    }),
                ))
            })
    }

    fn snippet(&self, ix: usize, t: &Tokens, cx: &mut Context<Integrations>) -> impl IntoElement {
        let id = SNIPPET_IDS[ix];
        let state = self.snippets.as_ref().map(|s| s[ix].clone());
        let rows = pre_rows(&self.snippet_text[ix]);
        div()
            .id(id)
            .test_support()
            .aria_label(SNIPPET_LABELS[ix])
            .flex()
            .flex_col()
            .gap(px(6.))
            .min_w(px(0.))
            .flex_grow(1.)
            .flex_basis(px(0.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div().flex_grow(1.).min_w(px(0.)).child(
                            tracked(
                                SNIPPET_LABELS[ix].to_uppercase(),
                                SANS,
                                FontWeight::SEMIBOLD,
                                px(11.),
                                0.05,
                            )
                            .text_color(t.muted),
                        ),
                    )
                    .when(self.copied[ix], |d| {
                        d.child(
                            div()
                                .id(SharedString::from(format!("{id}-copied")))
                                .test_support()
                                .aria_label("copied")
                                .size_xs()
                                .text_color(t.accent)
                                .child(wrapped_text("copied")),
                        )
                    })
                    .child(
                        button::secondary(SharedString::from(format!("{id}-copy")), "Copy")
                            .on_click(cx.listener(move |view, _, _, cx| view.copy(ix, cx))),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!("{id}-text")))
                    .relative()
                    .overflow_hidden()
                    .h(px(rows as f32 * SNIPPET_LINE + 2. * SNIPPET_PADDING))
                    .pl(px(SNIPPET_PADDING))
                    .py(px(SNIPPET_PADDING))
                    .mono()
                    .size_xs()
                    .text_color(t.ink)
                    .child(css_box(t.sunken, None, 6.))
                    .when_some(state, |d, state| {
                        d.child(baseline::on_whole_pixel_line(Textarea::new(&state)))
                    }),
            )
    }

    fn other_mcp(&self, t: &Tokens, cx: &mut Context<Integrations>) -> impl IntoElement {
        div()
            .id("other-mcp")
            .test_support()
            .aria_label("Any other MCP client")
            .relative()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(20.))
            .border_1()
            .border_color(transparent_black())
            .child(css_box(t.surface, Some(t.line), 8.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .semibold()
                            .size_lg()
                            .child(wrapped_text("Any other MCP client")),
                    )
                    .child(
                        div()
                            .size_sm()
                            .text_color(t.muted)
                            .child(wrapped_text(OTHER_MCP_HINT)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(16.))
                    .child(self.snippet(0, t, cx))
                    .child(self.snippet(1, t, cx)),
            )
    }
}

impl Render for Integrations {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *tokens(cx);
        self.sync_snippets(window, cx);
        if let (Some(err), None) = (&self.err, &self.agents) {
            return div()
                .id("integrations")
                .test_support()
                .aria_label("Integrations")
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(12.))
                .pt(px(32.))
                .child(banner::error("integrations-error", err.clone()))
                .child(
                    text_button("integrations-retry", "Try again", BUTTON_FACE, t.accent)
                        .underline()
                        .on_click(cx.listener(|view, _, _, cx| view.reload(cx))),
                )
                .into_any_element();
        }
        let Some(agents) = self.agents.clone() else {
            return div()
                .id("integrations")
                .test_support()
                .aria_label("Integrations")
                .size_full()
                .p(px(16.))
                .size_sm()
                .text_color(t.muted)
                .child(wrapped_text(LOOKING))
                .into_any_element();
        };
        let detected: Vec<&AgentRow> = agents.agents.iter().filter(|a| a.shown_as_card()).collect();
        let missing: Vec<&AgentRow> = agents
            .agents
            .iter()
            .filter(|a| !a.shown_as_card())
            .collect();
        div()
            .id("integrations")
            .test_support()
            .aria_label("Integrations")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(1100.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .pb(px(24.))
                    .child(self.header(&t, cx))
                    .when_some(self.err.clone(), |d, err| {
                        d.child(banner::error("integrations-error", err))
                    })
                    .when(detected.is_empty(), |d| {
                        d.child(
                            div()
                                .id("no-agents")
                                .test_support()
                                .aria_label(NO_AGENTS)
                                .relative()
                                .border_1()
                                .border_color(transparent_black())
                                .p(px(20.))
                                .size_sm()
                                .text_color(t.muted)
                                .child(css_box(t.surface, Some(t.line), 8.))
                                .child(wrapped_text(NO_AGENTS)),
                        )
                    })
                    .child(self.grid(&detected, &t, cx))
                    .when(!missing.is_empty(), |d| {
                        d.child(self.not_installed(&missing, &t, cx))
                    })
                    .child(self.other_mcp(&t, cx)),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::path::PathBuf;
    use std::sync::Arc;

    use gpui_kit::base::Root;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        px, size, AppContext, Bounds, Entity, Point, SharedString, TestAppContext, WindowBounds,
        WindowHandle, WindowOptions,
    };
    use rhebok::dashboard::core::agents::{AgentChanged, AgentList};
    use rhebok::dashboard::core::config::ConfigView;
    use rhebok::dashboard::core::stats::{Stats, Tiers};
    use rhebok::dashboard::core::{AgentRefusal, DashboardError};
    use rhebok::setup::agents::Change;
    use serde_json::{json, Value};

    use super::{missing_id, Integrations, SNIPPET_IDS};
    use crate::backend::{self, FixtureBackend};
    use crate::shell::{Shell, TabId};
    use crate::theme;
    use crate::version_gate::Gate;
    use crate::widgets::agent_card::{confirm_text, part_id, status_label};
    use crate::widgets::change_report::UNPARSEABLE;

    const MCP_JSON: &str = "{\n  \"mcpServers\": {\n    \"rhebok\": {\n      \"command\": \"/home/u/rhebok\",\n      \"args\": [\"mcp\"]\n    }\n  }\n}";
    const CODEX_TOML: &str = "[mcp_servers.rhebok]\ncommand = \"/home/u/rhebok\"\nargs = [\"mcp\"]";

    fn agent(id: &str, name: &str, installed: bool, state: &str, reason: Option<&str>) -> Value {
        let mut v = json!({
            "id": id,
            "name": name,
            "detected": {
                "installed": installed,
                "version": if installed { json!("1.2.3") } else { Value::Null },
                "config_path": format!("/home/u/.{id}/config.json"),
            },
            "status": { "state": state },
            "capabilities": {
                "mcp": true,
                "prompt_hook": id == "codex" || id == "gemini",
                "session_hook": false,
                "transcripts": id == "codex",
                "instructions": id == "codex",
            },
            "snippet": "",
        });
        if let Some(reason) = reason {
            v["status"]["reason"] = json!(reason);
        }
        v
    }

    fn list(cursor_state: &str) -> AgentList {
        AgentList(json!({
            "agents": [
                agent("claude-code", "Claude Code", false, "not_connected", None),
                agent("codex", "Codex", false, "not_connected", None),
                agent("cursor", "Cursor", true, cursor_state, None),
                agent("gemini", "Gemini CLI", true, "broken", Some("~/.gemini/settings.json cannot be read as JSON")),
            ],
            "snippets": { "mcp_json": MCP_JSON, "codex_toml": CODEX_TOML },
        }))
    }

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

    fn fixture(cursor_state: &str) -> Arc<FixtureBackend> {
        let fixture = FixtureBackend::default();
        fixture.stats.set(Ok(stats()));
        fixture.agents.set(list(cursor_state));
        fixture.config_view.set(Ok(ConfigView(json!({
            "effective": { "index_transcripts": true, "index_codex_sessions": false }
        }))));
        Arc::new(fixture)
    }

    fn changed(files: &[&str]) -> AgentChanged {
        AgentChanged {
            agent: Value::Null,
            change: Change {
                files: files.iter().map(PathBuf::from).collect(),
                backups: Vec::new(),
                notes: Vec::new(),
            },
        }
    }

    fn open(
        cx: &mut TestAppContext,
        fixture: Arc<FixtureBackend>,
    ) -> (WindowHandle<Root>, Entity<Integrations>) {
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
        click(cx, window, TabId::Integrations.button_id());
        let view = shell
            .read_with(cx, |s: &Shell, _| s.integrations.clone())
            .expect("integrations opened");
        render(cx, window);
        (window, view)
    }

    fn render(cx: &mut TestAppContext, window: WindowHandle<Root>) {
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.run_until_parked();
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

    fn label(
        cx: &mut TestAppContext,
        window: WindowHandle<Root>,
        id: SharedString,
    ) -> Option<String> {
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window
                .try_find(id)
                .map(|e| e.label().unwrap_or_default().to_string())
        })
        .unwrap()
    }

    fn requests_named(fixture: &FixtureBackend, prefix: &str) -> Vec<String> {
        fixture
            .requests()
            .into_iter()
            .filter(|r| r.starts_with(prefix))
            .collect()
    }

    #[gpui_kit::test]
    fn connect_shows_the_files_written_and_reloads_the_list(cx: &mut TestAppContext) {
        let fixture = fixture("not_connected");
        fixture
            .agent_connect
            .set(Ok(changed(&["/home/u/.cursor/mcp.json"])));
        let (window, view) = open(cx, fixture.clone());
        assert_eq!(fixture.agents.calls(), 1);
        assert_eq!(
            label(cx, window, part_id("cursor", "status")).as_deref(),
            Some(status_label("not_connected"))
        );
        fixture.agents.set(list("connected"));
        click(cx, window, part_id("cursor", "connect"));
        assert_eq!(
            requests_named(&fixture, "agent_connect"),
            [r#"agent_connect AgentRequest { id: "cursor", instructions: false }"#]
        );
        assert_eq!(fixture.agents.calls(), 2, "the list was not reloaded");
        assert_eq!(
            label(cx, window, part_id("cursor", "report")).as_deref(),
            Some("Connected Cursor.")
        );
        let files = view.read_with(cx, |v, _| {
            v.actions()
                .outcome("cursor")
                .and_then(|o| o.change.clone())
                .map(|c| c.files)
        });
        assert_eq!(files, Some(vec![PathBuf::from("/home/u/.cursor/mcp.json")]));
        assert_eq!(
            label(cx, window, "agent-cursor-report-files".into()).as_deref(),
            Some("Files written /home/u/.cursor/mcp.json")
        );
        assert_eq!(
            label(cx, window, part_id("cursor", "status")).as_deref(),
            Some(status_label("connected"))
        );
        assert!(label(cx, window, part_id("cursor", "disconnect")).is_some());
    }

    #[gpui_kit::test]
    fn disconnect_asks_first(cx: &mut TestAppContext) {
        let fixture = fixture("connected");
        fixture
            .agent_disconnect
            .set(Ok(changed(&["/home/u/.cursor/mcp.json"])));
        let (window, _) = open(cx, fixture.clone());
        click(cx, window, part_id("cursor", "disconnect"));
        assert!(requests_named(&fixture, "agent_disconnect").is_empty());
        assert_eq!(
            label(cx, window, part_id("cursor", "confirm")),
            Some(confirm_text("Cursor"))
        );
        click(cx, window, part_id("cursor", "cancel"));
        assert!(label(cx, window, part_id("cursor", "confirm")).is_none());
        assert!(requests_named(&fixture, "agent_disconnect").is_empty());

        fixture.agents.set(list("not_connected"));
        click(cx, window, part_id("cursor", "disconnect"));
        click(cx, window, part_id("cursor", "confirm-disconnect"));
        assert_eq!(
            requests_named(&fixture, "agent_disconnect"),
            [r#"agent_disconnect AgentRequest { id: "cursor", instructions: false }"#]
        );
        assert!(label(cx, window, part_id("cursor", "confirm")).is_none());
        assert_eq!(
            label(cx, window, part_id("cursor", "report")).as_deref(),
            Some("Disconnected Cursor.")
        );
        assert_eq!(
            label(cx, window, part_id("cursor", "status")).as_deref(),
            Some(status_label("not_connected"))
        );
    }

    #[gpui_kit::test]
    fn an_unparseable_config_shows_the_fix_it_by_hand_text(cx: &mut TestAppContext) {
        let fixture = fixture("not_connected");
        let message = "~/.gemini/settings.json is not valid JSON; rhebok left it as it was";
        fixture.agent_connect.set(Err(DashboardError::AgentRefused {
            kind: AgentRefusal::Unparseable,
            message: message.to_string(),
        }));
        let (window, _) = open(cx, fixture.clone());
        assert_eq!(
            label(cx, window, part_id("gemini", "reason")).as_deref(),
            Some("~/.gemini/settings.json cannot be read as JSON")
        );
        click(cx, window, part_id("gemini", "repair"));
        assert_eq!(
            label(cx, window, part_id("gemini", "report")).as_deref(),
            Some(message)
        );
        assert_eq!(
            label(cx, window, "agent-gemini-report-unparseable".into()).as_deref(),
            Some(UNPARSEABLE)
        );

        fixture.agent_connect.set(Err(DashboardError::AgentRefused {
            kind: AgentRefusal::Failed,
            message: "permission denied".to_string(),
        }));
        click(cx, window, part_id("gemini", "repair"));
        assert_eq!(
            label(cx, window, part_id("gemini", "report")).as_deref(),
            Some("permission denied")
        );
        assert!(label(cx, window, "agent-gemini-report-unparseable".into()).is_none());
    }

    #[gpui_kit::test]
    fn the_not_installed_list_is_collapsed_by_default(cx: &mut TestAppContext) {
        let (window, view) = open(cx, fixture("not_connected"));
        assert_eq!(
            label(cx, window, "not-installed-toggle".into()).as_deref(),
            Some("Not installed 2")
        );
        assert!(!view.read_with(cx, |v, _| v.missing_open()));
        assert!(label(cx, window, missing_id("codex")).is_none());
        assert!(label(cx, window, part_id("codex", "status")).is_none());
        click(cx, window, "not-installed-toggle");
        assert_eq!(
            label(cx, window, missing_id("codex")).as_deref(),
            Some("Codex")
        );
        assert_eq!(
            label(cx, window, missing_id("claude-code")).as_deref(),
            Some("Claude Code")
        );
        assert!(label(cx, window, missing_id("cursor")).is_none());
        click(cx, window, "not-installed-toggle");
        assert!(label(cx, window, missing_id("codex")).is_none());
    }

    #[gpui_kit::test]
    fn copy_writes_the_snippet_to_the_clipboard(cx: &mut TestAppContext) {
        let (window, _) = open(cx, fixture("not_connected"));
        assert!(label(cx, window, format!("{}-copied", SNIPPET_IDS[1]).into()).is_none());
        click(cx, window, format!("{}-copy", SNIPPET_IDS[1]));
        assert_eq!(
            cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
            Some(CODEX_TOML)
        );
        assert_eq!(
            label(cx, window, format!("{}-copied", SNIPPET_IDS[1]).into()).as_deref(),
            Some("copied")
        );
        assert!(label(cx, window, format!("{}-copied", SNIPPET_IDS[0]).into()).is_none());
        click(cx, window, format!("{}-copy", SNIPPET_IDS[0]));
        assert_eq!(
            cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
            Some(MCP_JSON)
        );
    }

    #[gpui_kit::test]
    fn a_stale_entry_offers_repair_and_says_why(cx: &mut TestAppContext) {
        let fixture = fixture("not_connected");
        let mut stale = list("connected");
        stale.0["agents"][2]["status"] =
            json!({ "state": "stale", "reason": "names another binary" });
        fixture.agents.set(stale);
        fixture
            .agent_connect
            .set(Ok(changed(&["/home/u/.cursor/mcp.json"])));
        let (window, _) = open(cx, fixture.clone());
        assert_eq!(
            label(cx, window, part_id("cursor", "status")).as_deref(),
            Some("needs repair")
        );
        assert_eq!(
            label(cx, window, part_id("cursor", "reason")).as_deref(),
            Some("names another binary")
        );
        assert!(label(cx, window, part_id("cursor", "connect")).is_none());
        assert!(label(cx, window, part_id("cursor", "disconnect")).is_some());
        assert_eq!(
            label(cx, window, part_id("gemini", "status")).as_deref(),
            Some("broken")
        );
        click(cx, window, part_id("cursor", "repair"));
        assert_eq!(requests_named(&fixture, "agent_connect").len(), 1);
    }

    #[gpui_kit::test]
    fn an_agent_that_is_connected_but_not_installed_keeps_its_card(cx: &mut TestAppContext) {
        let fixture = fixture("not_connected");
        let mut orphan = list("not_connected");
        orphan.0["agents"][1]["status"] = json!({ "state": "connected" });
        fixture.agents.set(orphan);
        let (window, _) = open(cx, fixture);
        assert!(label(cx, window, part_id("codex", "status")).is_some());
        assert_eq!(
            label(cx, window, "not-installed-toggle".into()).as_deref(),
            Some("Not installed 1")
        );
    }

    #[gpui_kit::test]
    fn codex_guidance_is_off_by_default_and_sent_when_ticked(cx: &mut TestAppContext) {
        let fixture = fixture("not_connected");
        let mut codex = list("not_connected");
        codex.0["agents"][1]["detected"]["installed"] = json!(true);
        fixture.agents.set(codex);
        fixture
            .agent_connect
            .set(Ok(changed(&["/home/u/.codex/config.toml"])));
        let (window, _) = open(cx, fixture.clone());
        assert_eq!(
            label(cx, window, part_id("codex", "sessions")).as_deref(),
            Some("Sessions are not indexed index_codex_sessions = false")
        );
        click(cx, window, part_id("codex", "instructions"));
        click(cx, window, part_id("codex", "connect"));
        assert_eq!(
            requests_named(&fixture, "agent_connect"),
            [r#"agent_connect AgentRequest { id: "codex", instructions: true }"#]
        );
    }

    #[test]
    fn the_real_agent_list_parses_with_every_row_and_field() {
        use rhebok::dashboard::core::agents::{connect, list, AgentRequest};
        use rhebok::setup::agents::AgentEnv;
        use rhebok::setup::Paths;

        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        for dir in [".cursor", ".codex", ".gemini"] {
            std::fs::create_dir_all(home.join(dir)).unwrap();
        }
        let paths = Paths::at(&home.join("rhebok"), vec![], home.join("cache"));
        std::fs::create_dir_all(paths.bin.parent().unwrap()).unwrap();
        std::fs::write(&paths.bin, "#!/bin/sh\n").unwrap();
        let env = AgentEnv::at(&home, paths, vec![]);
        connect(
            &env,
            AgentRequest {
                id: "cursor".into(),
                instructions: false,
            },
        )
        .unwrap();
        let raw = list(&env);
        let parsed =
            crate::widgets::agent_card::parse_agents(&raw).expect("the core's own list parses");
        let rows = raw.0["agents"].as_array().unwrap();
        assert_eq!(parsed.agents.len(), rows.len());
        assert_eq!(rows.len(), 5);
        let text = |v: &Value| v.as_str().map(str::to_string);
        for (row, agent) in rows.iter().zip(&parsed.agents) {
            assert_eq!(Some(agent.id.clone()), text(&row["id"]));
            assert_eq!(Some(agent.name.clone()), text(&row["name"]));
            assert_eq!(
                Some(agent.detected.installed),
                row["detected"]["installed"].as_bool()
            );
            assert_eq!(agent.detected.version, text(&row["detected"]["version"]));
            assert_eq!(
                agent.detected.config_path,
                text(&row["detected"]["config_path"])
            );
            assert_eq!(
                Some(agent.status.state.clone()),
                text(&row["status"]["state"])
            );
            assert_eq!(agent.status.reason, text(&row["status"]["reason"]));
            for key in [
                "mcp",
                "prompt_hook",
                "session_hook",
                "transcripts",
                "instructions",
            ] {
                assert_eq!(
                    Some(agent.capabilities.has(key)),
                    row["capabilities"][key].as_bool(),
                    "{} {key}",
                    agent.id
                );
            }
            assert_eq!(agent.instructions, row["instructions"].as_bool());
        }
        let cursor = parsed.agents.iter().find(|a| a.id == "cursor").unwrap();
        assert_eq!(cursor.status.state, "connected");
        assert!(cursor.detected.installed && cursor.detected.config_path.is_some());
        assert!(parsed.agents.iter().any(|a| !a.detected.installed));
        assert!(parsed.agents.iter().any(|a| a.instructions.is_some()));
        assert_eq!(
            Some(parsed.snippets.mcp_json),
            text(&raw.0["snippets"]["mcp_json"])
        );
        assert_eq!(
            Some(parsed.snippets.codex_toml),
            text(&raw.0["snippets"]["codex_toml"])
        );
    }

    #[gpui_kit::test]
    fn an_agent_list_missing_a_field_is_an_error_not_a_guess(cx: &mut TestAppContext) {
        let fixture = fixture("not_connected");
        let mut broken = list("not_connected");
        broken.0["agents"][2]["detected"]
            .as_object_mut()
            .unwrap()
            .remove("installed");
        fixture.agents.set(broken);
        let (window, view) = open(cx, fixture);
        assert_eq!(
            label(cx, window, "integrations-error".into()).as_deref(),
            Some("The agent list could not be read: cursor: the agent list has no `detected.installed`")
        );
        assert!(view.read_with(cx, |v, _| v.agents().is_none()));
        assert!(label(cx, window, part_id("cursor", "status")).is_none());
        assert!(label(cx, window, "integrations-retry".into()).is_some());
    }

    #[gpui_kit::test]
    fn clicking_the_guidance_label_ticks_its_checkbox(cx: &mut TestAppContext) {
        let fixture = fixture("not_connected");
        let mut codex = list("not_connected");
        codex.0["agents"][1]["detected"]["installed"] = json!(true);
        fixture.agents.set(codex);
        fixture
            .agent_connect
            .set(Ok(changed(&["/home/u/.codex/config.toml"])));
        let (window, view) = open(cx, fixture.clone());
        click(cx, window, part_id("codex", "instructions-label"));
        assert!(view.read_with(cx, |v, _| v.actions().instructions("codex")));
        click(cx, window, part_id("codex", "instructions-label"));
        assert!(!view.read_with(cx, |v, _| v.actions().instructions("codex")));
        click(cx, window, part_id("codex", "instructions-label"));
        click(cx, window, part_id("codex", "connect"));
        assert_eq!(
            requests_named(&fixture, "agent_connect"),
            [r#"agent_connect AgentRequest { id: "codex", instructions: true }"#]
        );
    }
}

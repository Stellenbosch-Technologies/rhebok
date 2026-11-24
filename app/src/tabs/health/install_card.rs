use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::widgets::baseline;
use crate::widgets::css_box::{css_box, css_rule_top};
use rhebok::dashboard::core::agents::AgentRequest;
use rhebok::dashboard::core::embed::EmbedTest;
use rhebok::dashboard::core::install::{InstallCheckView, InstallView};
use rhebok::setup::install::Fix;

use crate::backend::run_backend;
use crate::shell::Shell;
use crate::theme::{tokens, Tokens, Type, BODY_FACE, MONO, SANS, SM_FACE, XS_FACE, XS_MONO_FACE};
use crate::widgets::{
    button, change_report, copy_command, inline_line, min_content_width, tracked, AgentAction,
    Outcome, Rich,
};

pub fn check_label(name: &str) -> &str {
    match name {
        "binary" => "Installed binary",
        "plugin" => "Plugin files",
        "marketplace" => "Claude Code marketplace",
        "registration" => "Claude Code plugin",
        "claude" => "Claude Code",
        "path" => "rhebok on PATH",
        other => other,
    }
}

pub fn summary(install: &InstallView) -> String {
    let problems = install.checks.iter().filter(|c| !c.ok).count() + install.config.errors.len();
    match problems {
        0 => format!("all {} checks pass", install.checks.len()),
        1 => "1 problem".to_string(),
        n => format!("{n} problems"),
    }
}

pub fn embed_target(install: &InstallView) -> String {
    format!(
        "{} {} · {}",
        if install.embed.backend == "remote" {
            "remote endpoint"
        } else {
            "Ollama"
        },
        install.embed.url.as_deref().unwrap_or("(not set)"),
        install.embed.model.as_deref().unwrap_or("no model")
    )
}

pub fn embed_outcome(test: Option<&EmbedTest>, testing: bool) -> Option<String> {
    if testing {
        return Some(" · testing…".to_string());
    }
    let test = test?;
    Some(if test.ok {
        format!(
            " · reachable, {} dimensions in {} ms",
            test.dimensions.map(|d| d.to_string()).unwrap_or_default(),
            test.latency_ms
        )
    } else {
        format!(" · {}", test.error.as_deref().unwrap_or("not reachable"))
    })
}

pub struct InstallCard {
    shell: WeakEntity<Shell>,
    install: Option<InstallView>,
    active: bool,
    connecting: bool,
    outcome: Option<Outcome>,
    embed: Option<EmbedTest>,
    testing: bool,
    tested: bool,
    load: Option<Task<()>>,
    connect: Option<Task<()>>,
    embed_task: Option<Task<()>>,
}

impl InstallCard {
    pub fn new(shell: WeakEntity<Shell>) -> InstallCard {
        InstallCard {
            shell,
            install: None,
            active: false,
            connecting: false,
            outcome: None,
            embed: None,
            testing: false,
            tested: false,
            load: None,
            connect: None,
            embed_task: None,
        }
    }

    pub fn has_install(&self) -> bool {
        self.install.is_some()
    }

    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if active == self.active {
            return;
        }
        self.active = active;
        if active {
            self.load(cx);
            self.test_embed_once(cx);
        }
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.load = Some(run_backend(
            cx,
            |backend| backend.install(),
            |card: &mut InstallCard, install, cx| {
                card.install = Some(install);
                card.test_embed_once(cx);
            },
        ));
    }

    fn test_embed_once(&mut self, cx: &mut Context<Self>) {
        if self.active && self.install.is_some() && !self.tested {
            self.tested = true;
            self.test_embed(cx);
        }
    }

    fn test_embed(&mut self, cx: &mut Context<Self>) {
        self.testing = true;
        self.embed_task = Some(run_backend(
            cx,
            |backend| backend.embed_test(),
            |card: &mut InstallCard, test, _| {
                card.embed = Some(test);
                card.testing = false;
            },
        ));
        cx.notify();
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        if self.connecting {
            return;
        }
        self.connecting = true;
        self.outcome = None;
        self.connect = Some(run_backend(
            cx,
            |backend| {
                backend.agent_connect(AgentRequest {
                    id: "claude-code".to_string(),
                    instructions: false,
                })
            },
            |card: &mut InstallCard, result, cx| {
                card.outcome = Some(Outcome::of(AgentAction::Connect, result));
                card.connecting = false;
                card.load(cx);
            },
        ));
        cx.notify();
    }

    fn open_setting(&mut self, key: String, cx: &mut Context<Self>) {
        if let Some(shell) = self.shell.upgrade() {
            shell.update(cx, |shell, cx| shell.open_setting(&key, cx));
        }
    }

    fn fix(
        &self,
        check: &InstallCheckView,
        first_connect: bool,
        t: &Tokens,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let id = format!("check-{}-fix", check.name);
        let muted = |text: String| {
            div()
                .id(SharedString::from(id.clone()))
                .test_support()
                .aria_label(text.clone())
                .size_xs()
                .text_color(t.muted)
                .child(inline_line(cx, BODY_FACE, &[XS_FACE], text))
                .into_any_element()
        };
        Some(match check.fix.as_ref()? {
            Fix::Connect { .. } if !first_connect => {
                muted("the same Connect fixes this".to_string())
            }
            Fix::Connect { .. } => button::primary("connect-claude-code", "Connect Claude Code")
                .small()
                .busy_label(self.connecting.then(|| "Connecting…".into()))
                .on_click(cx.listener(|card, _, _, cx| card.connect(cx)))
                .into_any_element(),
            Fix::Manual { text } => muted(text.clone()),
            Fix::Command { command } => copy_command(SharedString::from(id), command.clone())
                .install_fix()
                .into_any_element(),
        })
    }

    fn row(id: SharedString, t: &Tokens) -> Stateful<Div> {
        div()
            .id(id)
            .flex()
            .items_center()
            .relative()
            .gap(px(12.))
            .py(px(8.))
            .border_t_1()
            .border_color(transparent_black())
            .child(css_rule_top(t.sunken))
            .child(dot(t.warn))
    }

    fn embed_line(
        &self,
        install: &InstallView,
        t: &Tokens,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colour = match &self.embed {
            Some(test) if test.ok => t.accent,
            Some(_) => t.warn,
            None => t.line,
        };
        let target = embed_target(install);
        let mut rich = Rich::new().push(&target, MONO, FontWeight::NORMAL, t.muted);
        if let Some(outcome) = embed_outcome(self.embed.as_ref(), self.testing) {
            let colour = match &self.embed {
                _ if self.testing => t.muted,
                Some(test) if test.ok => t.accent,
                _ => t.warn,
            };
            rich = rich.push(outcome, SANS, FontWeight::NORMAL, colour);
        }
        let label = rich.text();
        let faces = rich.faces(12., 16.);
        let basis = rich.max_content_width(window, 12.);
        div()
            .id("embed-line")
            .test_support()
            .flex()
            .items_center()
            .relative()
            .gap(px(12.))
            .py(px(8.))
            .border_t_1()
            .border_color(transparent_black())
            .child(css_rule_top(t.sunken))
            .child(dot(colour))
            .child(
                div()
                    .w(px(190.))
                    .flex_none()
                    .size_sm()
                    .semibold()
                    .child(baseline::text("Embeddings")),
            )
            .child(
                div()
                    .id("embed-status")
                    .test_support()
                    .aria_label(label)
                    .flex_basis(basis)
                    .flex_grow(1.)
                    .min_w(px(0.))
                    .text_size(px(12.))
                    .child(inline_line(cx, SM_FACE, &faces, rich.styled())),
            )
            .child(
                button::secondary("embed-test", "Test again")
                    .disabled(self.testing)
                    .on_click(cx.listener(|card, _, _, cx| card.test_embed(cx))),
            )
            .into_any_element()
    }
}

fn dot(colour: Hsla) -> Div {
    div().size(px(8.)).flex_none().rounded_full().bg(colour)
}

impl Render for InstallCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *tokens(cx);
        let Some(install) = self.install.clone() else {
            return div().into_any_element();
        };
        let failed: Vec<&InstallCheckView> = install.checks.iter().filter(|c| !c.ok).collect();
        let first_connect = failed
            .iter()
            .find(|c| matches!(c.fix, Some(Fix::Connect { .. })))
            .map(|c| c.name);
        let problems = failed.len() + install.config.errors.len();
        let summary = summary(&install);
        let mut card = div()
            .id("install-card")
            .test_support()
            .flex()
            .flex_col()
            .relative()
            .px(px(16.))
            .py(px(12.))
            .border_1()
            .border_color(transparent_black())
            .rounded(px(8.))
            .child(css_box(t.surface, Some(t.line), 8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .pb(px(8.))
                    .child(
                        tracked("INSTALL", SANS, FontWeight::SEMIBOLD, px(11.), 0.05)
                            .text_color(t.muted),
                    )
                    .child(
                        div()
                            .id("install-root")
                            .test_support()
                            .aria_label(install.root.display().to_string())
                            .flex_grow(1.)
                            .min_w(px(0.))
                            .mono()
                            .size_xs()
                            .text_color(t.muted)
                            .truncate()
                            .child(baseline::text(install.root.display().to_string())),
                    )
                    .child(
                        div()
                            .id("install-summary")
                            .test_support()
                            .aria_label(summary.clone())
                            .min_w(min_content_width(
                                window,
                                cx,
                                &summary,
                                SANS,
                                FontWeight::SEMIBOLD,
                                12.,
                            ))
                            .size_xs()
                            .semibold()
                            .text_color(if problems > 0 { t.warn } else { t.accent })
                            .child(baseline::text(summary)),
                    )
                    .child(
                        div()
                            .id("install-recheck")
                            .test_support()
                            .role(Role::Button)
                            .aria_label("recheck")
                            .flex_none()
                            .size_xs()
                            .semibold()
                            .text_color(t.accent)
                            .cursor_pointer()
                            .on_click(cx.listener(|card, _, _, cx| card.load(cx)))
                            .child(baseline::text("recheck")),
                    ),
            );
        for check in &failed {
            let fix = self.fix(check, Some(check.name) == first_connect, &t, cx);
            card = card.child(
                Self::row(format!("check-{}", check.name).into(), &t)
                    .test_support()
                    .aria_label(check_label(check.name).to_string())
                    .child(
                        div()
                            .w(px(190.))
                            .flex_none()
                            .size_sm()
                            .semibold()
                            .child(baseline::text(check_label(check.name).to_string())),
                    )
                    .child(
                        div()
                            .flex_grow(1.)
                            .min_w(px(0.))
                            .size_sm()
                            .text_color(t.muted)
                            .child(baseline::text(check.detail.clone())),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .min_h(px(24.))
                            .max_w(relative(0.45))
                            .when_some(fix, |d, fix| d.child(fix)),
                    ),
            );
        }
        if !failed.is_empty() {
            let caption = Rich::new()
                        .push(
                            "The binary, plugin and PATH checks describe what ",
                            SANS,
                            FontWeight::NORMAL,
                            t.muted,
                        )
                        .push("rhebok install", MONO, FontWeight::NORMAL, t.muted)
                        .push(
                            " sets up; a dashboard run from a build that is not installed fails them by design.",
                            SANS,
                            FontWeight::NORMAL,
                            t.muted,
                        )
                        .styled();
            card = card.child(div().pb(px(8.)).size_xs().child(inline_line(
                cx,
                XS_FACE,
                &[XS_FACE, XS_MONO_FACE],
                caption,
            )));
        }
        if let Some(outcome) = &self.outcome {
            card = card.child(div().pb(px(8.)).child(change_report(
                "install-change",
                outcome,
                "Claude Code",
                cx,
            )));
        }
        for error in &install.config.errors {
            let key = error.path.clone();
            let line = error
                .line
                .map_or_else(String::new, |line| format!(" line {line}"));
            card = card.child(
                Self::row(format!("config-error-{}", error.path).into(), &t)
                    .child(
                        div()
                            .w(px(190.))
                            .flex_none()
                            .size_sm()
                            .semibold()
                            .child(baseline::text(format!("config.toml{line}"))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_baseline()
                            .gap(px(4.))
                            .flex_grow(1.)
                            .min_w(px(0.))
                            .size_sm()
                            .child(div().mono().size_xs().child(baseline::text(
                                if error.path.is_empty() {
                                    "syntax".to_string()
                                } else {
                                    error.path.clone()
                                },
                            )))
                            .child(
                                div()
                                    .text_color(t.warn)
                                    .child(baseline::text(error.message.clone())),
                            ),
                    )
                    .child(
                        button::secondary(
                            SharedString::from(format!("config-error-{}-open", error.path)),
                            "Open in Settings",
                        )
                        .on_click(
                            cx.listener(move |card, _, _, cx| card.open_setting(key.clone(), cx)),
                        ),
                    ),
            );
        }
        card.child(self.embed_line(&install, &t, window, cx))
            .into_any_element()
    }
}

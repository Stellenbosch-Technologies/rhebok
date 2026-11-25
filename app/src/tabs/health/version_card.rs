use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::widgets::baseline;
use crate::widgets::css_box::css_box;
use rhebok::dashboard::core::version::Version;

use crate::backend::{describe, run_backend};
use crate::tabs::health::POLL;
use crate::theme::{tokens, Tokens, Type, BODY_FACE, MONO, SANS};
use crate::widgets::{ago, exact_text, inline_line, tracked, Rich};

pub const RESTART_WARNING: &str = "An index is running. Restart stops it and starts it again: the work it has not yet published is lost, and the live index is unchanged.";

pub struct VersionCard {
    version: Option<Version>,
    busy: bool,
    error: Option<String>,
    active: bool,
    indexing: bool,
    poll: Option<Task<()>>,
    tick: Option<Task<()>>,
    tick_in_flight: bool,
    action: Option<Task<()>>,
    confirming: bool,
    restarting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    Updating,
    Updated,
    Failed,
    Available,
    CheckFailed,
    NoRelease,
    UpToDate,
}

pub fn shown(v: &Version) -> Shown {
    match &v.update {
        Some(u) if !u.done => Shown::Updating,
        Some(u) if u.ok == Some(true) && u.to.is_some() => Shown::Updated,
        Some(u) if u.ok == Some(false) => Shown::Failed,
        _ if v.update_available.is_some() => Shown::Available,
        _ if v.error.is_some() => Shown::CheckFailed,
        _ if v.no_release => Shown::NoRelease,
        _ => Shown::UpToDate,
    }
}

pub fn status(v: &Version, t: &Tokens) -> Rich {
    let checked = ago(v.checked_at);
    let rich = Rich::new();
    let sans = |rich: Rich, text: &str, color| rich.push(text, SANS, FontWeight::NORMAL, color);
    let mono = |rich: Rich, text: &str| rich.push(text, MONO, FontWeight::NORMAL, t.ink);
    match (shown(v), &v.update) {
        (Shown::Updating, Some(u)) => {
            let rich = sans(
                rich,
                &format!("Updating to {}: ", u.to.as_deref().unwrap_or("…")),
                t.ink,
            );
            let rich = sans(mono(rich, &u.phase), " ", t.ink);
            if u.message.is_empty() {
                rich
            } else {
                sans(rich, &format!("— {}", u.message), t.muted)
            }
        }
        (Shown::Updated, Some(u)) => {
            let rich = mono(
                sans(rich, "Updated to ", t.ink),
                u.to.as_deref().unwrap_or_default(),
            );
            sans(
                rich,
                ". Restart this app so it runs the new version, and restart Claude Code.",
                t.ink,
            )
        }
        (Shown::Failed, Some(u)) => sans(
            rich,
            &format!("Update failed: {}. See db.log beside the index.", u.message),
            t.warn,
        ),
        (Shown::Available, _) => {
            let rich = mono(rich, v.update_available.as_deref().unwrap_or_default());
            let rich = mono(sans(rich, " available, you have ", t.ink), &v.installed);
            let rich = if v.url.is_some() {
                sans(rich, " · ", t.ink).link("release notes", SANS, t.accent)
            } else {
                rich
            };
            match &v.error {
                Some(error) => sans(
                    rich,
                    &format!(" · last check failed {checked}: {error}"),
                    t.warn,
                ),
                None => rich,
            }
        }
        (Shown::CheckFailed, _) => sans(
            rich,
            &format!(
                "update check failed {checked}: {}",
                v.error.as_deref().unwrap_or_default()
            ),
            t.warn,
        ),
        (Shown::NoRelease, _) => sans(
            rich,
            &format!("no release published yet · checked {checked}"),
            t.muted,
        ),
        _ => sans(rich, &format!("up to date · checked {checked}"), t.muted),
    }
}

fn card_button(
    id: &'static str,
    label: &'static str,
    accent: bool,
    disabled: bool,
    t: &Tokens,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .test_support()
        .role(Role::Button)
        .aria_label(label)
        .flex_none()
        .px(px(12.))
        .py(px(4.))
        .rounded(px(6.))
        .size_sm()
        .medium()
        .border_1()
        .map(|d| {
            if accent {
                d.bg(t.accent).text_color(white()).border_color(t.accent)
            } else {
                d.border_color(t.line)
            }
        })
        .when(disabled, |d| d.opacity(0.4))
        .when(!disabled, |d| d.cursor_pointer().on_click(on_click))
        .child(baseline::text(label))
}

impl Default for VersionCard {
    fn default() -> Self {
        VersionCard::new()
    }
}

impl VersionCard {
    pub fn new() -> VersionCard {
        VersionCard {
            version: None,
            busy: false,
            error: None,
            active: false,
            indexing: false,
            poll: None,
            tick: None,
            tick_in_flight: false,
            action: None,
            confirming: false,
            restarting: false,
        }
    }

    pub fn has_version(&self) -> bool {
        self.version.is_some()
    }

    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if active == self.active {
            return;
        }
        self.active = active;
        self.poll = active.then(|| {
            cx.spawn(async move |this, cx| loop {
                if this.update(cx, |card, cx| card.tick(cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(POLL).await;
            })
        });
    }

    pub fn set_indexing(&mut self, indexing: bool, cx: &mut Context<Self>) {
        if indexing != self.indexing {
            self.indexing = indexing;
            cx.notify();
        }
    }

    pub(super) fn tick(&mut self, cx: &mut Context<Self>) {
        if self.tick_in_flight {
            return;
        }
        self.tick_in_flight = true;
        self.tick = Some(run_backend(
            cx,
            |backend| backend.version(false),
            |card: &mut VersionCard, result, _| {
                card.tick_in_flight = false;
                if let Ok(v) = result {
                    card.version = Some(v);
                }
            },
        ));
    }

    fn check(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        self.action = Some(run_backend(
            cx,
            |backend| backend.version(true),
            |card: &mut VersionCard, result, _| {
                match result {
                    Ok(v) => card.version = Some(v),
                    Err(e) => card.error = Some(describe(&e)),
                }
                card.busy = false;
            },
        ));
        cx.notify();
    }

    fn ask_to_restart(&mut self, cx: &mut Context<Self>) {
        if self.indexing {
            self.confirming = true;
            cx.notify();
        } else {
            self.restart(cx);
        }
    }

    fn cancel_restart(&mut self, cx: &mut Context<Self>) {
        self.confirming = false;
        cx.notify();
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        self.confirming = false;
        self.restarting = true;
        self.busy = true;
        self.error = None;
        self.action = Some(run_backend(
            cx,
            |backend| backend.restart(),
            |card: &mut VersionCard, result, cx| match result {
                Ok(_) => cx.quit(),
                Err(e) => {
                    card.error = Some(describe(&e));
                    card.restarting = false;
                    card.busy = false;
                }
            },
        ));
        cx.notify();
    }

    fn update(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        self.action = Some(run_backend(
            cx,
            |backend| backend.update_start().and_then(|_| backend.version(false)),
            |card: &mut VersionCard, result, _| {
                match result {
                    Ok(v) => card.version = Some(v),
                    Err(e) => card.error = Some(describe(&e)),
                }
                card.busy = false;
            },
        ));
        cx.notify();
    }
}

impl Render for VersionCard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *tokens(cx);
        let Some(v) = self.version.clone() else {
            return div().into_any_element();
        };
        let state = shown(&v);
        let running = state == Shown::Updating;
        let succeeded = state == Shown::Updated;
        let show_update =
            !succeeded && !running && (v.update_available.is_some() || state == Shown::Failed);
        let show_check = !succeeded && !running;
        let rich = if self.restarting {
            let words = if self.indexing {
                "Restarting the app and the index…"
            } else {
                "Restarting the app…"
            };
            Rich::new().push(words, SANS, FontWeight::NORMAL, t.ink)
        } else if self.confirming {
            Rich::new().push(RESTART_WARNING, SANS, FontWeight::NORMAL, t.warn)
        } else {
            status(&v, &t)
        };
        let confirming = self.confirming;
        let show_restart = !running && !self.restarting && !confirming;
        let show_update = show_update && !self.restarting && !confirming;
        let show_check = show_check && !self.restarting && !confirming;
        let label = rich.text();
        let links = rich.links();
        let faces = rich.faces(14., 20.);
        let url = v.url.clone();
        let busy = self.busy;
        let update_disabled = busy || self.indexing;
        div()
            .id("version-card")
            .test_support()
            .flex()
            .items_center()
            .gap(px(14.))
            .relative()
            .px(px(16.))
            .py(px(12.))
            .border_1()
            .border_color(transparent_black())
            .rounded(px(8.))
            .child(css_box(t.surface, Some(t.line), 8.))
            .child(tracked("RHEBOK", SANS, FontWeight::SEMIBOLD, px(11.), 0.05).text_color(t.muted))
            .child(
                div()
                    .id("version-installed")
                    .test_support()
                    .aria_label(v.installed.clone())
                    .flex_none()
                    .mono()
                    .size_sm()
                    .child(exact_text(v.installed.clone())),
            )
            .child(
                div()
                    .id("version-status")
                    .test_support()
                    .aria_label(label)
                    .flex_grow(1.)
                    .min_w(px(0.))
                    .size_sm()
                    .child(inline_line(
                        cx,
                        BODY_FACE,
                        &faces,
                        InteractiveText::new("version-notes", rich.styled()).on_click(
                            links,
                            move |_, _, cx| {
                                if let Some(url) = &url {
                                    cx.open_url(url);
                                }
                            },
                        ),
                    )),
            )
            .when_some(self.error.clone(), |d, error| {
                d.child(
                    div()
                        .id("version-error")
                        .test_support()
                        .aria_label(error.clone())
                        .flex_none()
                        .size_xs()
                        .text_color(t.warn)
                        .child(baseline::text(error)),
                )
            })
            .when(
                self.indexing && !running && !confirming && !self.restarting,
                |d| {
                    d.child(
                        div()
                            .flex_none()
                            .size_xs()
                            .text_color(t.muted)
                            .child(baseline::text("an index is running")),
                    )
                },
            )
            .when(confirming, |d| {
                d.child(card_button(
                    "version-restart-confirm",
                    "Stop the index and restart",
                    false,
                    busy,
                    &t,
                    cx.listener(|card, _, _, cx| card.restart(cx)),
                ))
                .child(card_button(
                    "version-restart-cancel",
                    "Cancel",
                    false,
                    busy,
                    &t,
                    cx.listener(|card, _, _, cx| card.cancel_restart(cx)),
                ))
            })
            .when(show_restart, |d| {
                d.child(card_button(
                    "version-restart",
                    "Restart",
                    succeeded,
                    busy,
                    &t,
                    cx.listener(|card, _, _, cx| card.ask_to_restart(cx)),
                ))
            })
            .when(show_update, |d| {
                d.child(card_button(
                    "version-update",
                    "Update",
                    true,
                    update_disabled,
                    &t,
                    cx.listener(|card, _, _, cx| card.update(cx)),
                ))
            })
            .when(show_check, |d| {
                d.child(card_button(
                    "version-check",
                    "Check now",
                    false,
                    busy,
                    &t,
                    cx.listener(|card, _, _, cx| card.check(cx)),
                ))
            })
            .into_any_element()
    }
}

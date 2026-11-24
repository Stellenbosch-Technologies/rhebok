use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::{
    clip, js_length, kind_chip, moment, small_caps, Busy, Leave, Memories, KINDS, PATH_PLACEHOLDER,
};
use crate::theme::{Face, Tokens, Type, MONO, SANS};
use crate::widgets::css_box::css_rule_top;
use crate::widgets::js::{to_fixed, wire};
use crate::widgets::select::SelectSize;
use crate::widgets::tail::ellipsis_text;
use crate::widgets::text_input::{text_area, text_input};
use crate::widgets::wrap::wrapped_text;
use crate::widgets::{banner, baseline, button, checkbox, exact_text, select, text_button};

pub const NOTHING: &str = "nothing";
pub const ASK_REACH: &str = "What would this pull in?";
pub const REACHING: &str = "Reaching…";

const CLOSE_FACE: Face = Face::new(SANS, FontWeight::NORMAL, 12., 16.);
const TITLE_CHARS: usize = 48;

impl Memories {
    pub fn discard_text(&self) -> Option<String> {
        let leaving = self.leaving.as_ref()?;
        let destination = match leaving {
            Leave::Pick(id) => {
                let title = self
                    .find(id)
                    .map(|m| m.memory.title.clone())
                    .unwrap_or_else(|| id.clone());
                format!("open {id}, “{}”", clip(&title, TITLE_CHARS))
            }
            Leave::New => "start a new memory".to_string(),
            Leave::Close => "close the panel".to_string(),
        };
        let lost = match &self.draft {
            None => String::new(),
            Some(d) => match &d.id {
                Some(id) => {
                    let title = self
                        .selected()
                        .map(|m| m.memory.title.clone())
                        .unwrap_or_else(|| id.clone());
                    format!("your unsaved changes to “{}”", clip(&title, TITLE_CHARS))
                }
                None if !d.text.trim().is_empty() => format!(
                    "the new {} you were writing, “{}”,",
                    d.kind,
                    clip(&d.text, TITLE_CHARS)
                ),
                None => format!("the new {} you started", d.kind),
            },
        };
        Some(format!(
            "Discard {lost} and {destination}? Nothing is written until you press Save."
        ))
    }

    pub fn confirm_text(&self) -> Option<String> {
        self.confirming.as_ref().map(|c| {
            format!(
                "Delete “{}” ({})? A memory exists nowhere else, so this cannot be undone.",
                c.title, c.id
            )
        })
    }

    fn header(&self, t: &Tokens, cx: &mut Context<Memories>) -> Div {
        let draft = self.draft.as_ref().expect("an open panel");
        let locked = self.locked();
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(kind_chip(t, &draft.kind))
            .child(
                div()
                    .id("editor-heading")
                    .test_support()
                    .aria_label(if draft.id.is_some() {
                        "Editing"
                    } else {
                        "New memory"
                    })
                    .child(small_caps(
                        if draft.id.is_some() {
                            "Editing"
                        } else {
                            "New memory"
                        },
                        t,
                    )),
            )
            .when(self.dirty(), |d| {
                d.child(
                    div()
                        .id("editor-unsaved")
                        .test_support()
                        .aria_label("unsaved")
                        .type_px(11.)
                        .text_color(t.warn)
                        .child(exact_text("unsaved")),
                )
            })
            .child(div().flex_grow(1.))
            .child(
                text_button("editor-close", "close", CLOSE_FACE, t.muted)
                    .px(px(6.))
                    .disabled(locked)
                    .disabled_opacity(0.4)
                    .on_click(cx.listener(|m, _, window, cx| {
                        m.take_focus(window, cx);
                        m.request(Leave::Close, cx)
                    })),
            )
    }

    fn warn_box(id: &'static str, text: String, buttons: Div, t: &Tokens) -> AnyElement {
        div()
            .id(id)
            .test_support()
            .aria_label(SharedString::from(text.clone()))
            .bg(t.warn_soft)
            .text_color(t.warn)
            .rounded(px(6.))
            .px(px(12.))
            .py(px(10.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().size_sm().child(wrapped_text(text)))
            .child(buttons)
            .into_any_element()
    }

    fn identity(&self, t: &Tokens) -> Option<(Div, Div)> {
        let m = self.selected()?;
        let title = div()
            .id("editor-title")
            .test_support()
            .aria_label(SharedString::from(m.memory.title.clone()))
            .semibold()
            .text_size(px(18.))
            .line_height(px(24.75))
            .child(wrapped_text(m.memory.title.clone()));
        let mut facts: Vec<(&str, String, bool)> = vec![
            ("Id", m.memory.id.clone(), true),
            ("Created", moment(m.memory.facts.created), false),
            ("Origin", m.memory.facts.origin.as_str().to_string(), false),
        ];
        if let Some(session) = &m.memory.facts.session {
            facts.push(("Session", session.clone(), true));
        }
        let cell = || div().h(px(16.5)).type_px(11.);
        let dl = div()
            .flex()
            .gap(px(8.))
            .type_px(11.)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .text_color(t.muted)
                    .children(
                        facts
                            .iter()
                            .map(|(name, _, _)| cell().child(exact_text(name.to_string()))),
                    ),
            )
            .child(
                div()
                    .flex_grow(1.)
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .children(facts.into_iter().map(|(name, value, mono)| {
                        cell()
                            .id(SharedString::from(format!(
                                "editor-fact-{}",
                                name.to_lowercase()
                            )))
                            .test_support()
                            .aria_label(SharedString::from(value.clone()))
                            .flex()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .when(mono, |d| d.font_family(MONO))
                            .child(baseline::web_baseline(ellipsis_text(value)))
                    })),
            );
        Some((div().child(title), dl))
    }

    fn reach_view(&self, t: &Tokens) -> Option<AnyElement> {
        let label = || div().pb(px(6.)).child(small_caps("Reach", t));
        if let Some(error) = &self.reach_error {
            return Some(
                div()
                    .child(label())
                    .child(
                        div()
                            .id("reach-error")
                            .test_support()
                            .aria_label(SharedString::from(error.clone()))
                            .size_sm()
                            .text_color(t.warn)
                            .child(wrapped_text(error.clone())),
                    )
                    .into_any_element(),
            );
        }
        let reach = self.reach.as_ref()?;
        Some(
            div()
                .id("reach")
                .child(label())
                .when(reach.is_empty(), |d| {
                    d.child(
                        div()
                            .id("reach-nothing")
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
                        .id(SharedString::from(format!("reach-{ix}")))
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
                                .font_family(MONO)
                                .size_xs()
                                .child(exact_text(relevance)),
                        )
                }))
                .into_any_element(),
        )
    }

    pub(super) fn editor(
        &self,
        t: &Tokens,
        _window: &mut Window,
        cx: &mut Context<Memories>,
    ) -> AnyElement {
        let draft = self.draft.clone().expect("an open panel");
        let locked = self.locked();
        let selected = self.selected().is_some();
        let text = self.inputs.text.clone().expect("inputs exist");
        let confidence = self.inputs.confidence.clone().expect("inputs exist");
        let path = self.inputs.path.clone().expect("inputs exist");
        let characters = format!("{} characters", js_length(draft.text.trim()));
        let kind_ix = KINDS.iter().position(|k| *k == draft.kind).unwrap_or(0);
        let this = cx.entity().downgrade();
        let saving = matches!(self.busy, Some(Busy::Save(_)));
        let deleting = matches!(self.busy, Some(Busy::Delete(_)));
        let global = draft.global;
        let discard = self.discard_text().map(|text| {
            Self::warn_box(
                "discard-prompt",
                text,
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        button::warn("discard", "Discard")
                            .compact()
                            .disabled(locked)
                            .on_click(cx.listener(|m, _, window, cx| {
                                m.take_focus(window, cx);
                                m.discard(cx)
                            })),
                    )
                    .child(
                        button::quiet("keep-editing", "Keep editing")
                            .compact()
                            .on_click(cx.listener(|m, _, _, cx| m.keep_editing(cx))),
                    ),
                t,
            )
        });
        let confirm = self.confirm_text().map(|text| {
            Self::warn_box(
                "delete-confirm",
                text,
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        button::warn(
                            "delete-it",
                            if deleting { "Deleting…" } else { "Delete it" },
                        )
                        .compact()
                        .disabled(locked)
                        .on_click(cx.listener(|m, _, window, cx| {
                            m.take_focus(window, cx);
                            m.confirm_delete(cx)
                        })),
                    )
                    .child(
                        button::quiet("keep-it", "Keep it")
                            .compact()
                            .disabled(locked)
                            .on_click(cx.listener(|m, _, _, cx| m.keep_it(cx))),
                    ),
                t,
            )
        });
        let identity = self.identity(t);
        let reach = self.reach_view(t);
        let kinds: Vec<SharedString> = KINDS.iter().map(|k| SharedString::from(*k)).collect();
        div()
            .id("memory-editor")
            .test_support()
            .w(px(380.))
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
            .child(self.header(t, cx))
            .when_some(discard, |d, b| d.child(b))
            .when_some(identity, |d, (title, dl)| d.child(title).child(dl))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(small_caps("Text", t))
                    .child(
                        text_area("editor-text", "Text", &text)
                            .h(px(200.))
                            .px(px(10.))
                            .py(px(8.))
                            .regular()
                            .text_size(px(14.))
                            .line_height(px(22.75))
                            .when(locked, |d| d.opacity(0.6)),
                    )
                    .child(
                        div()
                            .id("editor-chars")
                            .test_support()
                            .aria_label(SharedString::from(characters.clone()))
                            .font_family(MONO)
                            .type_px(10.)
                            .text_color(t.muted)
                            .child(baseline::text(characters)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .flex_grow(1.)
                            .min_w(px(0.))
                            .child(small_caps("Kind", t))
                            .child(
                                select("editor-kind", kinds, kind_ix, SelectSize::Sm, {
                                    let this = this.clone();
                                    move |ix, _, cx| {
                                        let kind = KINDS[ix.min(KINDS.len() - 1)].to_string();
                                        let _ = this.update(cx, |m, cx| m.set_kind(kind, cx));
                                    }
                                })
                                .disabled(locked)
                                .w_full(),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .w(px(110.))
                            .child(small_caps("Confidence", t))
                            .child(
                                text_input("editor-confidence", "Confidence", &confidence)
                                    .px(px(8.))
                                    .py(px(6.))
                                    .mono()
                                    .size_sm()
                                    .when(locked, |d| d.opacity(0.6)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(small_caps("Scope", t))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .id("editor-global-label")
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .gap(px(6.))
                                    .size_xs()
                                    .when(!locked, |d| {
                                        d.on_click(
                                            cx.listener(move |m, _, _, cx| {
                                                m.set_global(!global, cx)
                                            }),
                                        )
                                    })
                                    .child(
                                        checkbox("editor-global", "global", global)
                                            .disabled(locked),
                                    )
                                    .child(exact_text("global")),
                            )
                            .child(
                                text_input("editor-path", PATH_PLACEHOLDER, &path)
                                    .flex_grow(1.)
                                    .px(px(8.))
                                    .py(px(6.))
                                    .mono()
                                    .size_xs()
                                    .when(global || locked, |d| d.opacity(0.45)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        button::primary("editor-save", if saving { "Saving…" } else { "Save" })
                            .disabled(locked)
                            .on_click(cx.listener(|m, _, window, cx| {
                                m.take_focus(window, cx);
                                m.save(cx)
                            })),
                    )
                    .when(selected, |d| {
                        d.child(
                            button::secondary(
                                "editor-reach",
                                if self.reaching { REACHING } else { ASK_REACH },
                            )
                            .compact()
                            .disabled(self.reaching || locked)
                            .on_click(cx.listener(
                                |m, _, window, cx| {
                                    m.take_focus(window, cx);
                                    m.load_reach(cx)
                                },
                            )),
                        )
                    })
                    .child(div().flex_grow(1.))
                    .when(selected && self.confirming.is_none(), |d| {
                        d.child(
                            button::danger("editor-delete", "Delete")
                                .compact()
                                .disabled(locked)
                                .on_click(cx.listener(|m, _, window, cx| {
                                    m.take_focus(window, cx);
                                    m.press_delete(cx)
                                })),
                        )
                    }),
            )
            .when_some(confirm, |d, b| d.child(b))
            .when_some(self.panel_err.clone(), |d, err| {
                d.child(banner::error("editor-error", err))
            })
            .when_some(reach, |d, r| d.child(r))
            .into_any_element()
    }
}

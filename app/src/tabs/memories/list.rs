use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rhebok::dashboard::core::memories::MemoryView;

use super::{
    day, first_line, kind_chip, scope_of, Leave, Memories, EVERY_SCOPE, FILTER_KINDS,
    FILTER_PLACEHOLDER,
};
use crate::theme::{css_line_box, Face, Tokens, Type, MONO, SANS};
use crate::widgets::css_box::css_rule_top;
use crate::widgets::tail::ellipsis_text;
use crate::widgets::text_input::text_input;
use crate::widgets::wrap::wrapped_text;
use crate::widgets::{
    baseline, button, exact_text, segmented, select, text_button, words, SelectSize,
};

pub const EMPTY_BEFORE: &str = "No memories yet — write one with ";
pub const EMPTY_COMMAND: &str = "rhebok memory add";
pub const EMPTY_AFTER: &str = " or the New button.";
pub const NO_MATCH: &str = "No memory matches these filters.";

const NOTICE_BUTTON: Face = Face::new(SANS, FontWeight::SEMIBOLD, 12., 16.);
const DISMISS: Face = Face::new(SANS, FontWeight::NORMAL, 12., 16.);
const EMPTY_FACE: Face = Face::new(SANS, FontWeight::NORMAL, 14., 20.);
const EMPTY_CODE: Face = Face::new(MONO, FontWeight::NORMAL, 14., 20.);
const MONO_ASCENT_EM: f32 = 1.025;
const MONO_DESCENT_EM: f32 = 0.275;

pub fn row_id(id: &str) -> SharedString {
    format!("memory-row-{id}").into()
}

pub fn count_text(shown: usize, all: usize) -> String {
    if shown == all {
        all.to_string()
    } else {
        format!("{shown} of {all}")
    }
}

impl Memories {
    pub(super) fn toolbar(&mut self, t: &Tokens, cx: &mut Context<Memories>) -> Div {
        let this = cx.entity().downgrade();
        let scopes = self.scopes();
        let scope = self.scope();
        let mut options: Vec<SharedString> = vec![EVERY_SCOPE.into()];
        options.extend(scopes.iter().map(|s| SharedString::from(s.clone())));
        let chosen = scopes
            .iter()
            .position(|s| *s == scope)
            .map_or(0, |ix| ix + 1);
        let values: Vec<String> = std::iter::once("all".to_string())
            .chain(scopes.iter().cloned())
            .collect();
        let pick_scope = this.clone();
        let filter = self.inputs.filter.clone().expect("inputs exist");
        let count = count_text(self.shown().len(), self.all().len());
        let locked = self.locked();
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                segmented(
                    "memories-kind",
                    &FILTER_KINDS,
                    self.kind,
                    move |ix, window, cx| {
                        let _ = this.update(cx, |m, cx| {
                            m.take_focus(window, cx);
                            m.set_kind_filter(ix, cx)
                        });
                    },
                )
                .tight(),
            )
            .child(
                select(
                    "memories-scope",
                    options,
                    chosen,
                    SelectSize::Xs,
                    move |ix, _, cx| {
                        let value = values.get(ix).cloned().unwrap_or_else(|| "all".into());
                        let _ = pick_scope.update(cx, |m, cx| m.set_scope_filter(value, cx));
                    },
                )
                .max_w(px(220.)),
            )
            .child(
                text_input("memories-filter", FILTER_PLACEHOLDER, &filter)
                    .flex_grow(1.)
                    .px(px(12.))
                    .py(px(8.))
                    .regular()
                    .size_sm(),
            )
            .child(
                div()
                    .id("memories-count")
                    .test_support()
                    .aria_label(SharedString::from(count.clone()))
                    .child(
                        exact_text(count)
                            .font_family(MONO)
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(t.muted),
                    ),
            )
            .child(
                button::primary("memories-new", "New")
                    .disabled(locked)
                    .on_click(cx.listener(|m, _, window, cx| {
                        m.take_focus(window, cx);
                        m.request(Leave::New, cx)
                    })),
            )
    }

    pub(super) fn notice_view(&self, t: &Tokens, cx: &mut Context<Memories>) -> Option<AnyElement> {
        let notice = self.notice.as_ref()?;
        let warn = notice.warn || notice.stale.is_some();
        let (bg, fg) = if warn {
            (t.warn_soft, t.warn)
        } else {
            (t.accent_soft, t.accent)
        };
        let text = notice.full_text();
        let open = notice.open.clone();
        let locked = self.locked();
        Some(
            div()
                .id("memories-notice")
                .test_support()
                .aria_label(SharedString::from(text.clone()))
                .when(warn, |d| d.aria_description("warn"))
                .rounded(px(6.))
                .px(px(16.))
                .py(px(8.))
                .size_sm()
                .bg(bg)
                .text_color(fg)
                .flex()
                .items_start()
                .gap(px(12.))
                .child(div().flex_grow(1.).min_w(px(0.)).child(wrapped_text(text)))
                .when(notice.stale.is_some(), |d| {
                    d.child(
                        text_button("notice-reload", "Reload the list", NOTICE_BUTTON, fg)
                            .flex_none()
                            .underline()
                            .on_click(cx.listener(|m, _, _, cx| m.reload_list(cx))),
                    )
                })
                .when_some(open, |d, id| {
                    d.child(
                        text_button("notice-open", format!("Open {id}"), NOTICE_BUTTON, fg)
                            .flex_none()
                            .underline()
                            .disabled(locked)
                            .on_click(cx.listener(move |m, _, window, cx| {
                                m.take_focus(window, cx);
                                m.request(Leave::Pick(id.clone()), cx)
                            })),
                    )
                })
                .child(
                    text_button("notice-dismiss", "dismiss", DISMISS, fg)
                        .flex_none()
                        .on_click(cx.listener(|m, _, _, cx| m.dismiss_notice(cx))),
                )
                .into_any_element(),
        )
    }

    fn row(&self, m: &MemoryView, t: &Tokens, cx: &mut Context<Memories>) -> AnyElement {
        let id = m.memory.id.clone();
        let selected = self.selected_id.as_deref() == Some(id.as_str());
        let locked = self.locked();
        let scope = scope_of(m);
        let pick = id.clone();
        div()
            .id(row_id(&id))
            .test_support()
            .role(Role::Button)
            .aria_label(SharedString::from(first_line(&m.memory.text)))
            .aria_selected(selected)
            .w_full()
            .px(px(12.))
            .py(px(10.))
            .relative()
            .border_t_1()
            .border_color(transparent_black())
            .child(css_rule_top(t.sunken))
            .flex()
            .items_center()
            .gap(px(10.))
            .min_w(px(0.))
            .when(selected, |d| d.bg(t.accent_soft))
            .when(locked && !selected, |d| d.opacity(0.5))
            .when(locked, |d| d.cursor_not_allowed())
            .when(!locked, |d| d.cursor_pointer())
            .on_click(cx.listener(move |m, _, window, cx| {
                m.take_focus(window, cx);
                m.request(Leave::Pick(pick.clone()), cx)
            }))
            .child(kind_chip(t, m.memory.facts.kind.as_str()))
            .child(
                div()
                    .flex_none()
                    .w(px(82.))
                    .type_px(11.)
                    .text_color(t.muted)
                    .child(baseline::text(day(m.memory.facts.created))),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(120.))
                    .flex()
                    .overflow_hidden()
                    .font_family(MONO)
                    .type_px(11.)
                    .text_color(t.muted)
                    .child(baseline::web_baseline(ellipsis_text(scope))),
            )
            .child(
                div()
                    .flex_grow(1.)
                    .flex()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .size_xs()
                    .child(baseline::web_baseline(ellipsis_text(first_line(
                        &m.memory.text,
                    )))),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(92.))
                    .flex()
                    .justify_end()
                    .overflow_hidden()
                    .font_family(MONO)
                    .type_px(11.)
                    .text_color(t.muted)
                    .child(baseline::web_baseline(ellipsis_text(id))),
            )
            .into_any_element()
    }

    pub(super) fn list(&self, t: &Tokens, cx: &mut Context<Memories>) -> AnyElement {
        let all = self.all().len();
        let shown = self.shown();
        let empty = (all == 0).then(|| {
            let runs = [EMPTY_FACE, EMPTY_CODE];
            let baseline = css_line_box(cx, EMPTY_FACE, &runs).baseline;
            let code = words(EMPTY_COMMAND);
            let code_words = code.len();
            div()
                .id("memories-empty")
                .test_support()
                .aria_label(SharedString::from(format!(
                    "{EMPTY_BEFORE}{EMPTY_COMMAND}{EMPTY_AFTER}"
                )))
                .p(px(16.))
                .size_sm()
                .text_color(t.muted)
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .children(
                            words(EMPTY_BEFORE)
                                .into_iter()
                                .map(|w| w.on_line(EMPTY_FACE, &runs)),
                        )
                        .children(code.into_iter().enumerate().map(|(ix, w)| {
                            let (first, last) = (ix == 0, ix + 1 == code_words);
                            div()
                                .relative()
                                .font_family(MONO)
                                .when(first, |d| d.pl(px(4.)))
                                .when(last, |d| d.pr(px(4.)))
                                .child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .top(px(baseline - EMPTY_CODE.size * MONO_ASCENT_EM))
                                        .h(px(EMPTY_CODE.size * (MONO_ASCENT_EM + MONO_DESCENT_EM)))
                                        .when(first, |d| d.rounded_l(px(4.)))
                                        .when(last, |d| d.rounded_r(px(4.)))
                                        .bg(t.sunken),
                                )
                                .child(w.on_line(EMPTY_FACE, &runs))
                        }))
                        .children(
                            words(EMPTY_AFTER)
                                .into_iter()
                                .map(|w| w.on_line(EMPTY_FACE, &runs)),
                        ),
                )
        });
        let no_match = (all > 0 && shown.is_empty()).then(|| {
            div()
                .id("memories-no-match")
                .test_support()
                .aria_label(NO_MATCH)
                .p(px(16.))
                .size_sm()
                .text_color(t.muted)
                .child(baseline::text(NO_MATCH))
        });
        let rows: Vec<AnyElement> = shown.into_iter().map(|m| self.row(m, t, cx)).collect();
        div()
            .id("memories-list")
            .test_support()
            .flex_grow(1.)
            .min_w(px(0.))
            .min_h(px(0.))
            .flex()
            .flex_col()
            .bg(t.surface)
            .border_1()
            .border_color(t.line)
            .rounded(px(8.))
            .overflow_y_scroll()
            .when_some(empty, |d, e| d.child(e))
            .when_some(no_match, |d, e| d.child(e))
            .children(rows)
            .into_any_element()
    }
}

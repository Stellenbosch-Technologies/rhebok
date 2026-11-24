use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde_json::Value as Json;

use super::fields::{candidate_key, input_id, Ctx};
use crate::theme::{tokens, Face, Tokens, Type, SANS};
use crate::widgets::baseline::web_baseline;
use crate::widgets::css_box::{css_box, css_rule_top};
use crate::widgets::tail::ellipsis_text;
use crate::widgets::text_button::TextButton;
use crate::widgets::{baseline, button, exact_text, text_button, text_input};

pub const CHECKING: &str = "checking…";
pub const DUPLICATE: &str = "already in the list";
pub const NONE: &str = "none";

const REMOVE_FACE: Face = Face::new(SANS, FontWeight::NORMAL, 12., 16.);

#[derive(Default)]
pub struct ListState {
    pub candidate: String,
    pub verdict: Option<(String, Option<String>)>,
    pub scheduled: Option<(String, Vec<String>)>,
    pub clear: bool,
    pub task: Option<Task<()>>,
}

impl ListState {
    pub fn trimmed(&self) -> &str {
        self.candidate.trim()
    }

    pub fn checking(&self, validating: bool) -> bool {
        let trimmed = self.trimmed();
        validating
            && !trimmed.is_empty()
            && self.verdict.as_ref().map(|(c, _)| c.as_str()) != Some(trimmed)
    }

    pub fn problem(&self, validating: bool) -> Option<&str> {
        if self.checking(validating) {
            return None;
        }
        match &self.verdict {
            Some((candidate, problem)) if candidate == self.trimmed() => problem.as_deref(),
            _ => None,
        }
    }

    pub fn blocked(&self, list: &[String], validating: bool) -> bool {
        let trimmed = self.trimmed();
        trimmed.is_empty()
            || list.iter().any(|item| item == trimmed)
            || self.problem(validating).is_some()
            || self.checking(validating)
    }
}

pub fn remove_button(id: impl Into<ElementId>, t: &Tokens) -> TextButton {
    text_button(id, "remove", REMOVE_FACE, t.muted).hover_colour(t.warn)
}

pub fn string_list(value: Option<&Json>) -> Vec<String> {
    match value {
        Some(Json::Array(items)) => items
            .iter()
            .map(|v| match v {
                Json::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub fn list_id(path: &str, part: &str) -> SharedString {
    format!("{path}-{part}").into()
}

pub struct ListEditor<'a> {
    pub path: &'a str,
    pub add_label: &'a str,
    pub validating: bool,
}

pub fn list_editor(
    ctx: &Ctx,
    editor: ListEditor,
    state: Option<&ListState>,
    item_errors: impl Fn(&str) -> Vec<String>,
    cx: &App,
) -> AnyElement {
    let t = *tokens(cx);
    let path = editor.path;
    let validating = editor.validating;
    let list = string_list(ctx.value(path).as_ref());
    let empty = ListState::default();
    let state = state.unwrap_or(&empty);
    let trimmed = state.trimmed().to_string();
    let checking = state.checking(validating);
    let problem = state.problem(validating).map(str::to_string);
    let duplicate = list.contains(&trimmed);
    let blocked = ctx.locked || state.blocked(&list, validating);
    let key = candidate_key(path);
    let rows = div()
        .id(list_id(path, "list"))
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
                    .px(px(12.))
                    .py(px(8.))
                    .size_sm()
                    .text_color(t.muted)
                    .child(baseline::text(NONE)),
            )
        })
        .children(list.iter().enumerate().map(|(ix, item)| {
            let errors = item_errors(item);
            let missing = !errors.is_empty();
            let remaining: Vec<toml::Value> = list
                .iter()
                .filter(|x| *x != item)
                .map(|x| toml::Value::String(x.clone()))
                .collect();
            let remove = ctx.setter(path, Some(toml::Value::Array(remaining)));
            div()
                .id(list_id(path, &format!("item-{ix}")))
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
                        .text_color(if missing { t.warn } else { t.ink })
                        .child(web_baseline(ellipsis_text(item.clone()))),
                )
                .when(missing, |d| {
                    d.child(
                        exact_text("missing")
                            .regular()
                            .text_size(px(11.))
                            .line_height(px(16.5))
                            .text_color(t.warn),
                    )
                })
                .child(
                    remove_button(list_id(path, &format!("remove-{ix}")), &t)
                        .disabled(ctx.locked)
                        .on_click(move |_, window, cx| remove(window, cx)),
                )
        }));
    let view = ctx.view.clone();
    let add_path = path.to_string();
    let add = div()
        .flex()
        .gap(px(8.))
        .child(match ctx.input(&key) {
            Some(input) => text_input(input_id(&key), format!("add to {path}"), input)
                .flex_grow(1.)
                .min_w(px(0.))
                .px(px(10.))
                .py(px(6.))
                .mono()
                .size_sm()
                .when(ctx.locked, |d| d.opacity(0.5))
                .into_any_element(),
            None => div().flex_grow(1.).h(px(34.)).into_any_element(),
        })
        .child(
            button::secondary(list_id(path, "add"), editor.add_label.to_string())
                .disabled(blocked)
                .on_click(move |_, _, cx| {
                    let _ = view.update(cx, |s, cx| s.add_candidate(&add_path, cx));
                }),
        );
    let message = |id: SharedString, text: String, colour: Hsla| {
        div()
            .id(id)
            .test_support()
            .aria_label(SharedString::from(text.clone()))
            .size_xs()
            .text_color(colour)
            .child(crate::widgets::wrap::wrapped_text(text))
    };
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(rows)
        .child(add)
        .when(!trimmed.is_empty() && checking, |d| {
            d.child(message(list_id(path, "checking"), CHECKING.into(), t.muted))
        })
        .when(duplicate && !trimmed.is_empty(), |d| {
            d.child(message(
                list_id(path, "duplicate"),
                DUPLICATE.into(),
                t.warn,
            ))
        })
        .when_some(problem, |d, problem| {
            d.child(message(list_id(path, "candidate-error"), problem, t.warn).role(Role::Alert))
        })
        .into_any_element()
}

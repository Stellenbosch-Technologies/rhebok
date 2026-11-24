use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::widgets::baseline;
use rhebok::dashboard::core::agents::AgentChanged;
use rhebok::dashboard::core::{AgentRefusal, DashboardError};
use rhebok::setup::agents::Change;

use crate::theme::{tokens, Type, SANS};
use crate::widgets::css_box::css_box;
use crate::widgets::tracked;
use crate::widgets::wrap::wrapped_text;

pub const UNPARSEABLE: &str =
    "Nothing was written: the file is exactly as it was. Fix it by hand, then try again.";

const LABEL_LINE: f32 = 11. * 20. / 14.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentAction {
    Connect,
    Disconnect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub action: AgentAction,
    pub change: Option<Change>,
    pub error: Option<String>,
    pub unparseable: bool,
}

impl Outcome {
    pub fn of(action: AgentAction, result: Result<AgentChanged, DashboardError>) -> Outcome {
        match result {
            Ok(changed) => Outcome {
                action,
                change: Some(changed.change),
                error: None,
                unparseable: false,
            },
            Err(e) => Outcome {
                action,
                change: None,
                unparseable: matches!(
                    e,
                    DashboardError::AgentRefused {
                        kind: AgentRefusal::Unparseable,
                        ..
                    }
                ),
                error: Some(e.to_string()),
            },
        }
    }
}

pub fn headline(outcome: &Outcome, name: &str) -> String {
    if let Some(error) = &outcome.error {
        return error.clone();
    }
    let change = outcome.change.clone().unwrap_or_default();
    let nothing = change.files.is_empty() && change.notes.is_empty();
    match (nothing, outcome.action) {
        (true, AgentAction::Connect) => format!("{name} was already connected; nothing changed."),
        (true, AgentAction::Disconnect) => format!("{name} had nothing of rhebok's to remove."),
        (false, AgentAction::Connect) => format!("Connected {name}."),
        (false, AgentAction::Disconnect) => format!("Disconnected {name}."),
    }
}

fn paths(id: ElementId, label: &str, paths: &[std::path::PathBuf], cx: &App) -> Option<AnyElement> {
    if paths.is_empty() {
        return None;
    }
    let t = tokens(cx);
    Some(
        div()
            .id(id)
            .test_support()
            .aria_label(SharedString::from(format!(
                "{label} {}",
                paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            )))
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div().opacity(0.8).child(
                    tracked(
                        label.to_uppercase(),
                        SANS,
                        FontWeight::SEMIBOLD,
                        px(11.),
                        0.05,
                    )
                    .line_height(px(LABEL_LINE))
                    .text_color(t.accent),
                ),
            )
            .children(paths.iter().map(|p| {
                div()
                    .mono()
                    .size_xs()
                    .child(wrapped_text(p.display().to_string()).break_all())
            }))
            .into_any_element(),
    )
}

pub fn change_report(
    id: impl Into<ElementId>,
    outcome: &Outcome,
    name: &str,
    cx: &App,
) -> AnyElement {
    let t = *tokens(cx);
    let id: ElementId = id.into();
    let hand_fix = ElementId::Name(format!("{id}-unparseable").into());
    let files_id = ElementId::Name(format!("{id}-files").into());
    let backups_id = ElementId::Name(format!("{id}-backups").into());
    let text = headline(outcome, name);
    if outcome.error.is_some() {
        return div()
            .id(id)
            .test_support()
            .role(Role::Alert)
            .aria_label(text.clone())
            .flex()
            .flex_col()
            .gap(px(4.))
            .rounded(px(6.))
            .px(px(12.))
            .py(px(8.))
            .size_sm()
            .relative()
            .child(css_box(t.warn_soft, None, 6.))
            .text_color(t.warn)
            .child(wrapped_text(text))
            .when(outcome.unparseable, |d| {
                d.child(
                    div()
                        .id(hand_fix)
                        .test_support()
                        .aria_label(UNPARSEABLE)
                        .size_xs()
                        .child(wrapped_text(UNPARSEABLE)),
                )
            })
            .into_any_element();
    }
    let change = outcome.change.clone().unwrap_or_default();
    div()
        .id(id)
        .test_support()
        .role(Role::Status)
        .aria_label(text.clone())
        .flex()
        .flex_col()
        .gap(px(8.))
        .rounded(px(6.))
        .px(px(12.))
        .py(px(8.))
        .size_sm()
        .relative()
        .child(css_box(t.accent_soft, None, 6.))
        .text_color(t.accent)
        .child(div().semibold().child(wrapped_text(text)))
        .children(paths(files_id, "Files written", &change.files, cx))
        .children(paths(
            backups_id,
            "Originals kept as .rhebok-bak",
            &change.backups,
            cx,
        ))
        .children((!change.notes.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .pl(px(16.))
                .children(change.notes.iter().map(|note| {
                    div()
                        .relative()
                        .child(div().absolute().left(px(-12.)).child(baseline::text("•")))
                        .child(wrapped_text(note.clone()))
                }))
        }))
        .into_any_element()
}

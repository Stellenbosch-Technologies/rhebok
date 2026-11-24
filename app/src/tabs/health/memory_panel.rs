use gpui_kit::*;

use crate::widgets::baseline;
use rhebok::dashboard::core::stats::Stats;

use crate::theme::{Tokens, Type};
use crate::widgets::panel::Panel;
use crate::widgets::{button, panel};

pub fn kind_colour(kind: &str, t: &Tokens) -> Hsla {
    match kind {
        "lesson" => t.graph.lesson,
        "fact" => t.graph.fact,
        "episode" => t.graph.episode,
        _ => t.graph.memory,
    }
}

pub fn saved_caption(total: usize) -> &'static str {
    if total == 1 {
        "memory saved"
    } else {
        "memories saved"
    }
}

pub fn memory_panel(
    stats: &Stats,
    t: &Tokens,
    on_open: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Panel {
    let kinds: Vec<(&String, usize)> = stats
        .memory
        .iter()
        .filter(|(_, n)| **n > 0)
        .map(|(k, n)| (k, *n))
        .collect();
    let total: usize = kinds.iter().map(|(_, n)| n).sum();
    let aside = div()
        .flex()
        .flex_grow(1.)
        .justify_end()
        .child(button::secondary("memory-open", "Open Memories").on_click(on_open))
        .into_any_element();
    let body = if kinds.is_empty() {
        div()
            .size_sm()
            .text_color(t.muted)
            .child(baseline::text("Nothing saved yet. Lessons and facts you save, and summaries of past sessions, show up here."))
            .into_any_element()
    } else {
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .size_sm()
            .children(kinds.iter().map(|(kind, n)| {
                let name = if *n == 1 {
                    kind.to_string()
                } else {
                    format!("{kind}s")
                };
                div()
                    .id(SharedString::from(format!("memory-kind-{kind}")))
                    .test_support()
                    .aria_label(format!("{n} {name}"))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().size(px(10.)).rounded_full().bg(kind_colour(kind, t)))
                    .child(div().flex_grow(1.).child(baseline::text(name)))
                    .child(
                        div()
                            .tabular()
                            .medium()
                            .child(baseline::text(n.to_string())),
                    )
            }))
            .into_any_element()
    };
    panel("Memory", Some(aside), "memory-panel")
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("memory-total")
                        .test_support()
                        .aria_label(total.to_string())
                        .text_size(px(30.))
                        .line_height(px(30.))
                        .semibold()
                        .tabular()
                        .child(baseline::text(total.to_string())),
                )
                .child(
                    div()
                        .pt(px(4.))
                        .size_sm()
                        .text_color(t.muted)
                        .child(baseline::text(saved_caption(total))),
                ),
        )
        .child(body)
}

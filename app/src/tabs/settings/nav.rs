use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::theme::{tokens, Face, Type, MONO, SANS};
use crate::widgets::css_box::css_box;
use crate::widgets::wrap::wrapped_text;
use crate::widgets::{baseline, exact_text, tracked};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Section {
    #[default]
    Sources,
    Retrieval,
    Embedding,
    Memory,
    Documents,
    Updates,
    Advanced,
}

pub const SECTIONS: [Section; 7] = [
    Section::Sources,
    Section::Retrieval,
    Section::Embedding,
    Section::Memory,
    Section::Documents,
    Section::Updates,
    Section::Advanced,
];

pub const NOT_CREATED: &str = "not created yet; the first save creates it";

const FILE_FACE: Face = Face::new(SANS, FontWeight::NORMAL, 11., 16.5);
const FILE_MONO_FACE: Face = Face::new(MONO, FontWeight::NORMAL, 11., 16.5);

impl Section {
    pub fn label(self) -> &'static str {
        match self {
            Section::Sources => "Sources",
            Section::Retrieval => "Retrieval",
            Section::Embedding => "Embedding",
            Section::Memory => "Memory",
            Section::Documents => "Documents",
            Section::Updates => "Updates & backup",
            Section::Advanced => "Advanced",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Section::Sources => "sources",
            Section::Retrieval => "retrieval",
            Section::Embedding => "embedding",
            Section::Memory => "memory",
            Section::Documents => "documents",
            Section::Updates => "updates",
            Section::Advanced => "advanced",
        }
    }

    pub fn nav_id(self) -> SharedString {
        format!("settings-nav-{}", self.slug()).into()
    }
}

pub fn section_of(path: &str) -> Section {
    let head = path.split('.').next().unwrap_or_default();
    if path.starts_with("hook.weights") || path.starts_with("mcp.weights") || head == "weights" {
        return Section::Advanced;
    }
    if head == "sources"
        || head == "ignore"
        || head.starts_with("index_transcripts")
        || head == "index_codex_sessions"
    {
        return Section::Sources;
    }
    match head {
        "hook" | "mcp" => Section::Retrieval,
        "embed" | "endpoint" => Section::Embedding,
        "memory" => Section::Memory,
        "pdf" => Section::Documents,
        "update" | "backup" => Section::Updates,
        _ => Section::Advanced,
    }
}

pub fn field_id(path: &str) -> SharedString {
    let mut out = String::from("field-");
    let mut in_run = false;
    for ch in path.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
            in_run = false;
        } else if !in_run {
            out.push('-');
            in_run = true;
        }
    }
    out.into()
}

pub struct NavRow {
    pub section: Section,
    pub active: bool,
    pub problems: usize,
    pub edited: usize,
}

pub fn nav(
    rows: Vec<NavRow>,
    file: &str,
    exists: bool,
    on_pick: impl Fn(Section, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> impl IntoElement {
    let t = *tokens(cx);
    div()
        .id("settings-nav")
        .test_support()
        .relative()
        .flex_none()
        .w(px(210.))
        .flex()
        .flex_col()
        .gap(px(2.))
        .p(px(8.))
        .border_1()
        .border_color(transparent_black())
        .child(css_box(t.surface, Some(t.line), 8.))
        .children(rows.into_iter().map(|row| {
            let pick = on_pick.clone();
            let section = row.section;
            div()
                .id(section.nav_id())
                .test_support()
                .role(Role::Button)
                .aria_label(section.label())
                .aria_selected(row.active)
                .relative()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .py(px(8.))
                .rounded(px(6.))
                .size_sm()
                .cursor_pointer()
                .map(|d| {
                    if row.active {
                        d.child(css_box(t.accent_soft, None, 6.))
                            .semibold()
                            .text_color(t.accent)
                    } else {
                        d.regular().text_color(t.ink).hover(|s| s.bg(t.sunken))
                    }
                })
                .on_click(move |_, window, cx| pick(section, window, cx))
                .child(
                    div()
                        .flex_grow(1.)
                        .min_w(px(0.))
                        .child(baseline::text(section.label())),
                )
                .when(row.problems > 0, |d| {
                    d.child(
                        div()
                            .id(SharedString::from(format!(
                                "settings-nav-{}-problems",
                                section.slug()
                            )))
                            .test_support()
                            .size(px(6.))
                            .rounded_full()
                            .bg(t.warn),
                    )
                })
                .when(row.edited > 0, |d| {
                    let count = row.edited.to_string();
                    d.child(
                        div()
                            .id(SharedString::from(format!(
                                "settings-nav-{}-edited",
                                section.slug()
                            )))
                            .test_support()
                            .aria_label(count.clone())
                            .child(
                                exact_text(count)
                                    .mono()
                                    .text_size(px(10.))
                                    .line_height(px(20.))
                                    .text_color(t.accent),
                            ),
                    )
                })
        }))
        .child(div().flex_grow(1.))
        .child(
            div()
                .id("settings-file")
                .test_support()
                .aria_label(SharedString::from(file.to_string()))
                .flex()
                .flex_col()
                .px(px(12.))
                .py(px(8.))
                .text_size(px(11.))
                .line_height(px(16.5))
                .text_color(t.muted)
                .child(
                    div().pb(px(4.)).child(
                        tracked("FILE", SANS, FontWeight::SEMIBOLD, px(11.), 0.05)
                            .line_height(px(16.5)),
                    ),
                )
                .child(
                    div().mono().child(
                        wrapped_text(file.to_string())
                            .break_all()
                            .on_line(FILE_FACE, &[FILE_FACE, FILE_MONO_FACE]),
                    ),
                )
                .when(!exists, |d| {
                    d.child(
                        div()
                            .pt(px(4.))
                            .regular()
                            .child(wrapped_text(NOT_CREATED).break_all()),
                    )
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::{field_id, section_of, Section};

    #[test]
    fn a_key_lands_in_the_section_the_dashboard_puts_it_in() {
        assert_eq!(section_of("sources"), Section::Sources);
        assert_eq!(section_of("sources.0"), Section::Sources);
        assert_eq!(
            section_of("index_transcripts_max_age_days"),
            Section::Sources
        );
        assert_eq!(section_of("index_codex_sessions"), Section::Sources);
        assert_eq!(section_of("hook.threshold"), Section::Retrieval);
        assert_eq!(section_of("hook.weights.decay"), Section::Advanced);
        assert_eq!(section_of("weights.authority"), Section::Advanced);
        assert_eq!(section_of("endpoint.url"), Section::Embedding);
        assert_eq!(section_of("pdf.ocr"), Section::Documents);
        assert_eq!(section_of("backup.target"), Section::Updates);
        assert_eq!(section_of("ollama_url"), Section::Advanced);
        assert_eq!(field_id("hook.weights.decay"), "field-hook-weights-decay");
        assert_eq!(field_id("a..b"), "field-a-b");
    }
}

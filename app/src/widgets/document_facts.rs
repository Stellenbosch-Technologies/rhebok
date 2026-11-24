use chrono::{Local, TimeZone};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::*;
use rhebok::dashboard::core::document::Document;
use rhebok::store::query::DocumentDetail;

use crate::backend::{describe, run_backend};
use crate::theme::{tokens, Type, SANS};
use crate::widgets::baseline;
use crate::widgets::exact::unrounded_width;
use crate::widgets::tail::ellipsis_text;

pub const LABELS: [&str; 4] = ["Path", "Indexed", "Status", "Size"];

#[derive(Debug, Clone)]
pub enum Facts {
    Loading,
    Failed(String),
    Missing(String),
    Found(DocumentDetail),
}

pub struct DocumentFacts {
    id: String,
    facts: Facts,
    task: Option<Task<()>>,
}

pub fn document_facts(id: String, cx: &mut App) -> Entity<DocumentFacts> {
    cx.new(|cx| DocumentFacts::new(id, cx))
}

pub fn indexed_at(unix: i64) -> String {
    match Local.timestamp_opt(unix, 0).single() {
        Some(at) => at.format("%-m/%-d/%Y, %-I:%M:%S %p").to_string(),
        None => "Invalid Date".to_string(),
    }
}

pub fn status_note(detail: &DocumentDetail) -> Option<String> {
    let lifecycle = lifecycle(detail);
    detail
        .status
        .as_deref()
        .filter(|s| !s.is_empty() && s.to_lowercase() != lifecycle.to_lowercase())
        .map(|s| format!(" (vault says “{s}”)"))
}

pub fn lifecycle(detail: &DocumentDetail) -> String {
    format!("{:?}", detail.lifecycle)
}

pub fn size_line(detail: &DocumentDetail) -> String {
    format!("{} chunks · {} inbound", detail.chunks, detail.inbound)
}

impl DocumentFacts {
    pub fn new(id: String, cx: &mut Context<DocumentFacts>) -> DocumentFacts {
        let mut facts = DocumentFacts {
            id,
            facts: Facts::Loading,
            task: None,
        };
        facts.fetch(cx);
        facts
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn facts(&self) -> &Facts {
        &self.facts
    }

    pub fn set_id(&mut self, id: String, cx: &mut Context<DocumentFacts>) {
        if id != self.id {
            self.id = id;
            self.fetch(cx);
        }
    }

    fn fetch(&mut self, cx: &mut Context<DocumentFacts>) {
        self.facts = Facts::Loading;
        let id = self.id.clone();
        self.task = Some(run_backend(
            cx,
            move |backend| backend.document(id),
            |facts: &mut DocumentFacts, result, _| {
                facts.facts = match result {
                    Ok(Document::Found(detail)) => Facts::Found(detail),
                    Ok(Document::Missing { error }) => Facts::Missing(error),
                    Err(e) => Facts::Failed(describe(&e)),
                };
            },
        ));
        cx.notify();
    }
}

fn label_width(window: &Window) -> Pixels {
    let mut style = window.text_style();
    style.font_family = SANS.into();
    style.font_weight = FontWeight::NORMAL;
    style.font_size = px(11.).into();
    LABELS
        .iter()
        .map(|label| unrounded_width(window, label, &style))
        .fold(px(0.), |a, b| a.max(b))
}

impl Render for DocumentFacts {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *tokens(cx);
        let root = || {
            div()
                .id("document-facts")
                .test_support()
                .regular()
                .type_px(11.)
        };
        let note = |message: &str| {
            root()
                .aria_label(SharedString::from(message.to_string()))
                .text_color(t.muted)
                .child(baseline::text(message.to_string()))
        };
        let detail = match &self.facts {
            Facts::Loading => return note("Loading…").into_any_element(),
            Facts::Failed(message) | Facts::Missing(message) => {
                return note(message).into_any_element()
            }
            Facts::Found(detail) => detail.clone(),
        };
        let width = label_width(window);
        let row = |label: &'static str, value: AnyElement| {
            div()
                .id(SharedString::from(format!("fact-{}", label.to_lowercase())))
                .test_support()
                .flex()
                .gap(px(8.))
                .child(
                    div()
                        .flex_none()
                        .w(width)
                        .whitespace_nowrap()
                        .text_color(t.muted)
                        .child(baseline::text(label)),
                )
                .child(div().flex_grow(1.).min_w(px(0.)).child(value))
        };
        let lifecycle = lifecycle(&detail);
        let status = match status_note(&detail) {
            Some(note) => {
                let text = format!("{lifecycle}{note}");
                let muted = lifecycle.len()..text.len();
                baseline::web_baseline(StyledText::new(text).with_highlights([(
                    muted,
                    HighlightStyle {
                        color: Some(t.muted),
                        ..HighlightStyle::default()
                    },
                )]))
                .into_any_element()
            }
            None => baseline::text(lifecycle.clone()).into_any_element(),
        };
        let uri = SharedString::from(detail.uri.clone());
        root()
            .aria_label(SharedString::from(detail.uri.clone()))
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(row(
                "Path",
                div()
                    .id("fact-path-value")
                    .mono()
                    .tooltip(move |window, cx| Tooltip::new(uri.clone()).build(window, cx))
                    .child(baseline::web_baseline(ellipsis_text(detail.uri.clone())))
                    .into_any_element(),
            ))
            .child(row(
                "Indexed",
                baseline::text(indexed_at(detail.indexed_at)).into_any_element(),
            ))
            .child(row("Status", status))
            .child(row(
                "Size",
                baseline::text(size_line(&detail)).into_any_element(),
            ))
            .into_any_element()
    }
}

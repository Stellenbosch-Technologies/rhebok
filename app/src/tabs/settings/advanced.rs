use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde_json::Value as Json;

use super::fields::{
    bounds_recorder, card, error_list, label, number_field, number_input, toggle_field, Ctx,
    InputSpec, RING_RADIUS, RING_WIDTH,
};
use super::nav::field_id;
use super::pending::{js_number, js_stringify, same, ConfigState};
use super::raw_toml::{raw_section, RawView};
use crate::theme::{tokens, Face, Type, MONO, SM_FACE};
use crate::widgets::css_box::css_ring_at;
use crate::widgets::exact::Tracks;
use crate::widgets::inline_flow::{inline_flow, Segment};
use crate::widgets::wrap::wrapped_text;

pub const WEIGHT_KEYS: [&str; 10] = [
    "markdown",
    "pdf",
    "web",
    "transcript",
    "memory",
    "authority",
    "current",
    "investigating",
    "proposed",
    "superseded",
];

pub const SURFACES: [&str; 2] = ["hook", "mcp"];
pub const COLUMNS: [&str; 3] = ["Global", "Prompt hook", "MCP"];
pub const NAME_COLUMN: f32 = 140.;
pub const CELL_INPUT: f32 = 90.;
pub const COLUMN_GAP: f32 = 16.;
pub const ROW_GAP: f32 = 8.;
pub const EDITED: &str = "edited";
pub const INHERITS: &str = "inherits";
pub const SHIPPED: &str = "shipped default";

pub const DECAY_NUMBERS: [(&str, &str); 3] = [
    ("weights.decay.grace_days", "Grace (days)"),
    ("weights.decay.half_life_days", "Half-life (days)"),
    ("weights.decay.floor", "Floor (0-1)"),
];

const NAME_DEFAULT_FACE: Face = Face::new(MONO, FontWeight::NORMAL, 11., 20.);

pub fn weights_intro(state: &ConfigState) -> String {
    let transcript = state
        .default_of("weights.transcript")
        .map(|v| match v {
            Json::Number(n) => js_number(n),
            other => js_stringify(other),
        })
        .unwrap_or_else(|| "undefined".into());
    format!("Multipliers on a hit's relevance. A surface override replaces the global value for that surface only. An empty surface cell shows, greyed, the value that surface uses with the saved config. A cell marked \"shipped default\" uses its own default, which applies only while the Global value is the shipped one: MCP's transcript weight, while Global transcript is {transcript}. Any other Global value reaches both surfaces.")
}

fn shown_number(value: &Json) -> String {
    match value {
        Json::Number(n) => js_number(n),
        Json::String(s) => s.clone(),
        other => js_stringify(other),
    }
}

pub fn inputs(state: &ConfigState) -> Vec<InputSpec> {
    let mut out = Vec::new();
    for name in WEIGHT_KEYS {
        out.push(InputSpec::number(&format!("weights.{name}"), false, None));
        for surface in SURFACES {
            let placeholder = state.surface_weight(surface, name).map(shown_number);
            out.push(InputSpec::number(
                &format!("{surface}.weights.{name}"),
                placeholder.is_some(),
                placeholder.as_deref(),
            ));
        }
    }
    out.extend(
        DECAY_NUMBERS
            .iter()
            .map(|(path, _)| InputSpec::number(path, false, None)),
    );
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Edited,
    Inherits,
    Shipped,
}

impl Tag {
    pub fn text(self) -> &'static str {
        match self {
            Tag::Edited => EDITED,
            Tag::Inherits => INHERITS,
            Tag::Shipped => SHIPPED,
        }
    }
}

pub fn cell_tag(
    ctx: &Ctx,
    path: &str,
    in_use: Option<&Json>,
    global: Option<&Json>,
) -> Option<Tag> {
    if ctx.pending.contains(path) {
        return Some(Tag::Edited);
    }
    let inherited = in_use.is_some() && ctx.value(path).is_none_or(|v| v.is_null());
    if !inherited {
        return None;
    }
    Some(if same_number(in_use, global) {
        Tag::Inherits
    } else {
        Tag::Shipped
    })
}

fn weight_cell(
    ctx: &Ctx,
    path: &str,
    column: (&Tracks, usize),
    in_use: Option<&Json>,
    global: Option<&Json>,
    cx: &App,
) -> AnyElement {
    let t = *tokens(cx);
    let id = field_id(path);
    let errors: Vec<_> = ctx.errors.iter().filter(|e| e.path == path).collect();
    let ringed = ctx.ring == Some(path);
    let tag = cell_tag(ctx, path, in_use, global).map(|tag| {
        let text = div()
            .text_size(px(10.))
            .line_height(px(15.))
            .child(wrapped_text(tag.text()));
        let text = match tag {
            Tag::Edited => text.semibold().text_color(t.accent),
            _ => text.text_color(t.muted),
        };
        let suffix = if tag == Tag::Edited {
            "edited"
        } else {
            "inherited"
        };
        div()
            .id(SharedString::from(format!("{id}-{suffix}")))
            .test_support()
            .aria_label(tag.text())
            .child(text)
    });
    div()
        .id(id.clone())
        .test_support()
        .aria_label(SharedString::from(path.to_string()))
        .relative()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(bounds_recorder(ctx, path))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(number_input(
                    ctx,
                    path,
                    CELL_INPUT,
                    Some(column.0.span(column.1, Some(px(CELL_INPUT)))),
                ))
                .children(tag),
        )
        .when_some(error_list(path, &errors, ctx.blocking, &t, cx), |d, e| {
            d.child(e)
        })
        .when(ringed, |d| {
            d.child(
                div()
                    .absolute()
                    .top(px(1.))
                    .left(px(1.))
                    .right(px(1.))
                    .bottom(px(1.))
                    .child(css_ring_at(
                        t.warn,
                        RING_WIDTH,
                        RING_RADIUS + RING_WIDTH,
                        Some(column.0.span(column.1, None)),
                    )),
            )
        })
        .into_any_element()
}

fn table_row(first: AnyElement, cells: Vec<AnyElement>) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(COLUMN_GAP))
        .child(div().flex_none().w(px(NAME_COLUMN)).child(first))
        .children(
            cells
                .into_iter()
                .map(|cell| div().flex_1().min_w(px(0.)).child(cell)),
        )
}

fn weight_row(ctx: &Ctx, columns: &Tracks, name: &str, cx: &App) -> Div {
    let t = *tokens(cx);
    let fallback = ctx
        .state
        .default_of(&format!("weights.{name}"))
        .map(shown_number)
        .unwrap_or_else(|| "undefined".into());
    let global_path = format!("weights.{name}");
    let global = super::pending::at(ctx.state.effective(), &global_path).cloned();
    let label = div()
        .id(SharedString::from(format!("weights-row-{name}")))
        .test_support()
        .aria_label(SharedString::from(format!("{name} ({fallback})")))
        .pt(px(6.))
        .size_sm()
        .child(inline_flow(
            SM_FACE,
            vec![
                Segment::text(format!("{name} "), SM_FACE),
                Segment::text(format!("({fallback})"), NAME_DEFAULT_FACE).coloured(t.muted),
            ],
        ))
        .into_any_element();
    let mut cells = vec![weight_cell(ctx, &global_path, (columns, 0), None, None, cx)];
    for (ix, surface) in SURFACES.iter().enumerate() {
        cells.push(weight_cell(
            ctx,
            &format!("{surface}.weights.{name}"),
            (columns, ix + 1),
            ctx.state.surface_weight(surface, name),
            global.as_ref(),
            cx,
        ));
    }
    table_row(label, cells)
}

fn weights_card(ctx: &Ctx, cx: &App) -> AnyElement {
    let t = *tokens(cx);
    let columns = Tracks::new(NAME_COLUMN + COLUMN_GAP, COLUMN_GAP, COLUMNS.len());
    let header = table_row(
        div().into_any_element(),
        COLUMNS
            .iter()
            .map(|c| label(c).text_color(t.muted).into_any_element())
            .collect(),
    );
    let table = div()
        .id("weights-table")
        .test_support()
        .flex()
        .relative()
        .flex_col()
        .gap(px(ROW_GAP))
        .children(columns.edges())
        .child(header)
        .children(
            WEIGHT_KEYS
                .iter()
                .map(|name| weight_row(ctx, &columns, name, cx)),
        );
    card("card-weights", "Ranking weights", None, cx)
        .child(
            div()
                .size_sm()
                .text_color(t.muted)
                .child(wrapped_text(weights_intro(ctx.state))),
        )
        .child(table)
        .into_any_element()
}

fn decay_card(ctx: &Ctx, cx: &App) -> AnyElement {
    card("card-decay", "Usage decay (transcripts)", None, cx)
        .child(toggle_field(
            ctx,
            "weights.decay.enabled",
            "Enabled",
            None,
            cx,
        ))
        .child(
            div()
                .flex()
                .gap(px(24.))
                .children(DECAY_NUMBERS.iter().map(|(path, label)| {
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(number_field(ctx, path, label, None, cx))
                })),
        )
        .into_any_element()
}

pub fn advanced_section(ctx: &Ctx, raw: RawView, cx: &App) -> Vec<AnyElement> {
    vec![
        weights_card(ctx, cx),
        decay_card(ctx, cx),
        raw_section(ctx, raw, cx),
    ]
}

pub fn same_number(a: Option<&Json>, b: Option<&Json>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => same(a, b),
        (None, None) => true,
        _ => false,
    }
}

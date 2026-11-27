use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rhebok::bench::BenchTier;
use serde_json::Value as Json;

use super::fields::{
    card, code_line, number_field, number_input, parse_number, range_slider, select_field, shell,
    slider_fraction, Ctx, Field, Hint, InputSpec, Part,
};
use crate::theme::{tokens, Type, XS_FACE, XS_MONO_FACE};
use crate::widgets::js::{to_fixed, wire};
use crate::widgets::segmented::segmented;

pub const TIERS: [&str; 5] = ["instant", "fast", "balanced", "thorough", "exhaustive"];
pub const THRESHOLD_HINT: &str =
    "Hits below this relevance are never injected. Lower finds more, and lets more noise in.";
pub const MAX_TOKENS_HINT: &str = "The token budget for what one search hands back.";
pub const INJECTOR_HINT: &str =
    "mod is the rhebok-prehook mod; command is the classic UserPromptSubmit hook.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Hook,
    Mcp,
}

impl Surface {
    pub fn key(self) -> &'static str {
        match self {
            Surface::Hook => "hook",
            Surface::Mcp => "mcp",
        }
    }

    pub fn default_tier(self) -> usize {
        match self {
            Surface::Hook => 1,
            Surface::Mcp => 3,
        }
    }
}

pub fn inputs() -> Vec<InputSpec> {
    [Surface::Hook, Surface::Mcp]
        .into_iter()
        .flat_map(|s| {
            [
                InputSpec::number(&format!("{}.threshold", s.key()), false, None),
                InputSpec::number(&format!("{}.max_tokens", s.key()), false, None),
            ]
        })
        .collect()
}

pub fn bench_line(bench: &[BenchTier], tier: &str) -> Option<SharedString> {
    bench
        .iter()
        .find(|b| b.tier == tier)
        .map(|b| format!("R@5 {} · {}ms", to_fixed(wire(b.recall_at_5), 2), b.p50_ms).into())
}

fn surface_card(
    ctx: &Ctx,
    surface: Surface,
    title: &str,
    bench: Option<&[BenchTier]>,
    cx: &App,
) -> AnyElement {
    let t = *tokens(cx);
    let quality = format!("{}.quality", surface.key());
    let threshold = format!("{}.threshold", surface.key());
    let tier = match ctx.value(&quality) {
        Some(Json::Number(n)) => n
            .as_f64()
            .filter(|v| v.fract() == 0. && (0.0..TIERS.len() as f64).contains(v))
            .map_or(usize::MAX, |v| v as usize),
        _ => surface.default_tier(),
    };
    let view = ctx.view.clone();
    let key = quality.clone();
    let tiers = segmented(
        format!("segmented-{quality}"),
        &TIERS,
        tier,
        move |ix, _, cx| {
            let _ = view.update(cx, |s, cx| {
                s.set(&key, Some(toml::Value::Integer(ix as i64)), cx)
            });
        },
    )
    .disabled(ctx.locked)
    .stacked(
        TIERS
            .iter()
            .map(|tier| bench.and_then(|b| bench_line(b, tier)))
            .collect(),
    );
    let quality_control = div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(div().flex().child(tiers))
        .when(bench.is_none(), |d| {
            d.child(
                code_line(
                    &[
                        Part::Text("Run "),
                        Part::Code("rhebok bench"),
                        Part::Text(" to see each tier's measured recall and latency here."),
                    ],
                    XS_FACE,
                    XS_MONO_FACE,
                    Some(t.sunken),
                    cx,
                )
                .id(SharedString::from(format!("{quality}-bench-hint")))
                .test_support()
                .aria_label("Run rhebok bench to see each tier's measured recall and latency here.")
                .size_xs()
                .text_color(t.muted),
            )
        })
        .into_any_element();
    let quality_field = shell(
        ctx,
        Field::new(&quality, "Quality tier")
            .without_default()
            .hint(Hint::Text(
                format!(
                    "Default for this surface: {}. Higher tiers find more and take longer.",
                    TIERS[surface.default_tier()]
                )
                .into(),
            )),
        quality_control,
        cx,
    );
    let fraction = slider_fraction(ctx.value(&threshold).as_ref());
    let view = ctx.view.clone();
    let key = threshold.clone();
    let threshold_control = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(range_slider(
            format!("slider-{threshold}"),
            fraction,
            ctx.locked,
            move |text, _, cx| {
                let _ = view.update(cx, |s, cx| s.set(&key, parse_number(&text, false), cx));
            },
        ))
        .child(number_input(ctx, &threshold, 90., None))
        .into_any_element();
    let threshold_field = shell(
        ctx,
        Field::new(&threshold, "Relevance threshold").hint(THRESHOLD_HINT),
        threshold_control,
        cx,
    );
    card(format!("card-{}", surface.key()), title, None, cx)
        .child(quality_field)
        .child(threshold_field)
        .child(number_field(
            ctx,
            &format!("{}.max_tokens", surface.key()),
            "Max tokens",
            Some(MAX_TOKENS_HINT.into()),
            cx,
        ))
        .into_any_element()
}

pub fn retrieval_section(ctx: &Ctx, bench: Option<&[BenchTier]>, cx: &App) -> Vec<AnyElement> {
    let claude = card("card-claude-code", "Claude Code", None, cx)
        .child(select_field(
            ctx,
            Field::new("prehook.injector", "Who adds context").hint(INJECTOR_HINT),
            &["mod", "command", "off"],
            None,
            cx,
        ))
        .child(select_field(
            ctx,
            Field::new("prehook.status", "Status line").hint(Hint::Parts(vec![
                Part::Text("counts shows "),
                Part::Code("4 hits · 181 ms"),
                Part::Text(", glyph shows "),
                Part::Code("● 4 · 181 ms"),
                Part::Text(", words shows "),
                Part::Code("added 4 notes"),
                Part::Text(". Takes effect on the next prompt."),
            ])),
            &["counts", "glyph", "words"],
            None,
            cx,
        ));
    vec![
        surface_card(ctx, Surface::Hook, "Prompt hook", bench, cx),
        surface_card(
            ctx,
            Surface::Mcp,
            "MCP tool (rhebok_search_knowledge)",
            bench,
            cx,
        ),
        claude.into_any_element(),
    ]
}

#[cfg(test)]
mod tests {
    use rhebok::bench::BenchTier;

    use super::bench_line;

    #[test]
    fn a_bench_line_reads_recall_to_two_places_and_whole_milliseconds() {
        let bench = [BenchTier {
            tier: "fast".into(),
            recall_at_5: 0.905,
            p50_ms: 101,
            p95_ms: 200,
            absent: Vec::new(),
        }];
        assert_eq!(
            bench_line(&bench, "fast").as_deref(),
            Some("R@5 0.91 · 101ms")
        );
        assert_eq!(bench_line(&bench, "instant"), None);
    }
}

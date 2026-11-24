use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use rhebok::bench::BenchTier;
use rhebok::dashboard::core::stats::Stats;

use super::Health;
use crate::shell::grouped;
use crate::theme::{Face, Tokens, Type, SANS, SM_FACE};
use crate::widgets::css_box::css_rounded_rule_top;
use crate::widgets::panel::Panel;
use crate::widgets::{
    ago, baseline, button, copy_command, dotted_link, exact_text, inline_line, panel, words, Rich,
};

pub const TIER_NAMES: [&str; 5] = ["instant", "fast", "balanced", "thorough", "exhaustive"];
pub const STACK_BELOW: f32 = 768.;
pub const TIER_TRACK_MIN: f32 = 150.;
pub const COLUMN_GAP: f32 = 24.;
pub const ROW_INSET: f32 = 8.;
pub const ROW_GAP: f32 = 6.;
pub const OVERHANG: f32 = (COLUMN_GAP - ROW_GAP) / 2.;
const BADGE_GAP: f32 = 6.;
const BADGE_SIZE: f32 = 11.;
const BADGE_LINE: f32 = 11. * 20. / 14.;
const BADGE_CHROME: f32 = 4. * 2. + 2.;
const DETAIL_MEASURE: usize = 78;

const SM_SEMIBOLD: Face = Face::new(SANS, FontWeight::SEMIBOLD, 14., 20.);

#[derive(Default)]
pub struct RetrievalPanel {
    pub picked: Option<String>,
}

pub fn js_number(value: f32) -> f64 {
    f64::from(value)
}

pub fn js_round(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

pub fn pct(recall: f32) -> String {
    format!("{}%", js_round(js_number(recall) * 100.))
}

fn signed(n: i64) -> String {
    if n >= 0 {
        format!("+{n}")
    } else {
        n.to_string()
    }
}

pub fn tier_name(index: u8) -> Option<&'static str> {
    TIER_NAMES.get(usize::from(index)).copied()
}

pub fn comparison(tier: &BenchTier, all: &[BenchTier], hook: Option<&str>) -> Option<String> {
    let base = all.iter().find(|t| Some(t.tier.as_str()) == hook)?;
    if base.tier == tier.tier {
        return None;
    }
    let points = js_round((js_number(tier.recall_at_5) - js_number(base.recall_at_5)) * 100.);
    let ms = tier.p50_ms as i64 - base.p50_ms as i64;
    Some(format!(
        " Against {}, the hook's tier, that is {} points of recall for {} ms.",
        base.tier,
        signed(points),
        signed(ms)
    ))
}

pub fn detail_rich(tier: &BenchTier, all: &[BenchTier], hook: Option<&str>, t: &Tokens) -> Rich {
    let rest = format!(
        ", {} of the test questions that have an answer found it in the top five results. Half the searches finished within {} ms and 95% within {} ms.{}",
        pct(tier.recall_at_5),
        tier.p50_ms,
        tier.p95_ms,
        comparison(tier, all, hook).unwrap_or_default()
    );
    Rich::new()
        .push("At ", SANS, FontWeight::NORMAL, t.ink)
        .push(&tier.tier, SANS, FontWeight::SEMIBOLD, t.ink)
        .push(rest, SANS, FontWeight::NORMAL, t.ink)
}

fn text_width(window: &Window, weight: FontWeight, size: f32, text: &str) -> Pixels {
    let run = TextRun {
        len: text.len(),
        font: Font {
            weight,
            ..font(SharedString::from(SANS))
        },
        color: black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .layout_line(text, px(size), &[run], None)
        .width
}

fn badges<'a>(
    name: &str,
    hook: Option<&'a str>,
    mcp: Option<&'a str>,
) -> Vec<(&'static str, bool)> {
    let mut out = Vec::new();
    if Some(name) == hook {
        out.push(("hook", true));
    }
    if Some(name) == mcp {
        out.push(("search tool", false));
    }
    out
}

pub fn tier_track(
    window: &Window,
    bench: &[BenchTier],
    selected: Option<&str>,
    hook: Option<&str>,
    mcp: Option<&str>,
) -> Pixels {
    let header = text_width(window, FontWeight::NORMAL, 12., "Tier") + px(2. * ROW_INSET);
    bench
        .iter()
        .map(|tier| {
            let weight = if Some(tier.tier.as_str()) == selected {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::NORMAL
            };
            let name = text_width(window, weight, 14., &tier.tier);
            let badges = badges(&tier.tier, hook, mcp)
                .into_iter()
                .map(|(label, _)| {
                    px(BADGE_GAP + BADGE_CHROME)
                        + text_width(window, FontWeight::SEMIBOLD, BADGE_SIZE, label)
                })
                .fold(px(0.), |a, b| a + b);
            name + badges + px(ROW_INSET - OVERHANG)
        })
        .fold(header.max(px(TIER_TRACK_MIN)), |a, b| a.max(b))
}

fn badge(label: &'static str, hook: bool, t: &Tokens) -> Div {
    div()
        .flex_none()
        .px(px(4.))
        .rounded(px(4.))
        .border_1()
        .map(|d| {
            if hook {
                d.border_color(t.accent.opacity(0.4)).text_color(t.accent)
            } else {
                d.border_color(t.line).text_color(t.muted)
            }
        })
        .semibold()
        .text_size(px(BADGE_SIZE))
        .line_height(px(BADGE_LINE))
        .child(exact_text(label))
}

fn bar_track() -> Div {
    div()
        .flex_grow(1.)
        .flex_basis(px(0.))
        .min_w(px(0.))
        .h(px(10.))
}

fn stacked_label(text: &'static str, t: &Tokens) -> Div {
    div()
        .w(px(48.))
        .flex_none()
        .size_xs()
        .text_color(t.muted)
        .child(baseline::text(text))
}

fn cells(tier: &BenchTier, slowest: f64, stacked: bool, t: &Tokens) -> (Div, Div) {
    let recall = js_round(js_number(tier.recall_at_5) * 100.) as f32 / 100.;
    let found = div()
        .flex()
        .items_center()
        .gap(px(10.))
        .when(stacked, |d| d.child(stacked_label("found", t)))
        .child(
            bar_track()
                .bg(t.sunken)
                .rounded(px(4.))
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(relative(recall))
                        .bg(t.accent)
                        .rounded(px(4.)),
                ),
        )
        .child(
            div()
                .w(px(48.))
                .flex_none()
                .flex()
                .justify_end()
                .tabular()
                .medium()
                .child(exact_text(pct(tier.recall_at_5)).tabular().medium()),
        );
    let p95 = (tier.p95_ms as f64 / slowest) as f32;
    let p50 = ((tier.p50_ms as f64 / slowest) * 100.).max(1.) as f32 / 100.;
    let time = div()
        .flex()
        .items_center()
        .gap(px(10.))
        .when(stacked, |d| d.child(stacked_label("time", t)))
        .child(
            bar_track()
                .relative()
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .w(relative(p95))
                        .bg(t.accent_soft)
                        .rounded(px(4.)),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .w(relative(p50))
                        .bg(t.sun)
                        .rounded(px(4.)),
                ),
        )
        .child(
            div()
                .w(px(96.))
                .flex_none()
                .flex()
                .justify_end()
                .tabular()
                .child(exact_text(tier.p50_ms.to_string()).medium().tabular())
                .child(
                    exact_text(format!(" / {} ms", tier.p95_ms))
                        .tabular()
                        .text_color(t.muted),
                ),
        );
    (found, time)
}

pub struct Selection<'a> {
    pub bench: &'a [BenchTier],
    pub selected: Option<&'a BenchTier>,
    pub hook: Option<&'static str>,
    pub mcp: Option<&'static str>,
}

pub fn select<'a>(stats: &'a Stats, picked: Option<&str>) -> Selection<'a> {
    let bench = stats.bench.as_deref().unwrap_or(&[]);
    let hook = tier_name(stats.tiers.hook);
    let mcp = tier_name(stats.tiers.mcp);
    let wanted = picked.or(hook);
    let selected = bench
        .iter()
        .find(|t| Some(t.tier.as_str()) == wanted)
        .or(bench.first());
    Selection {
        bench,
        selected,
        hook,
        mcp,
    }
}

fn empty_panel(t: &Tokens) -> Panel {
    panel("Retrieval quality", None, "retrieval-panel").child(
        div()
            .flex()
            .flex_wrap()
            .items_start()
            .size_sm()
            .text_color(t.muted)
            .children(words("Nothing measured yet. Run "))
            .child(copy_command("retrieval-empty", "rhebok bench"))
            .children(words(
                " to measure, for each quality tier, how often the right document is found and how long a search takes.",
            )),
    )
}

pub fn retrieval_panel(
    stats: &Stats,
    state: &RetrievalPanel,
    t: &Tokens,
    window: &mut Window,
    cx: &mut Context<Health>,
) -> Panel {
    let s = select(stats, state.picked.as_deref());
    let Some(selected) = s.selected else {
        return empty_panel(t);
    };
    let stacked = window.viewport_size().width < px(STACK_BELOW);
    let slowest = s.bench.iter().map(|t| t.p95_ms).max().unwrap_or(1).max(1) as f64;
    let aside_text = match &stats.bench_provenance {
        Some(prov) => format!(
            "measured {} with {} test questions",
            ago(stats.bench_at),
            prov.cases
        ),
        None => format!("measured {}", ago(stats.bench_at)),
    };
    let aside = div()
        .id("retrieval-measured")
        .test_support()
        .aria_label(aside_text.clone())
        .size_sm()
        .text_color(t.muted)
        .child(exact_text(aside_text))
        .into_any_element();
    let track = tier_track(window, s.bench, Some(selected.tier.as_str()), s.hook, s.mcp);
    let mut table = div().flex().flex_col().size_sm();
    if !stacked {
        table = table.child(
            div()
                .flex()
                .gap(px(COLUMN_GAP))
                .pl(px(ROW_INSET))
                .pb(px(6.))
                .size_xs()
                .text_color(t.muted)
                .child(
                    div()
                        .w(track - px(ROW_INSET))
                        .flex_none()
                        .child(baseline::text("Tier")),
                )
                .child(
                    div()
                        .flex_basis(px(ROW_INSET))
                        .flex_grow(1.)
                        .min_w(px(0.))
                        .child(baseline::text("Right document in the top five")),
                )
                .child(
                    div()
                        .flex_basis(px(0.))
                        .flex_grow(1.)
                        .min_w(px(0.))
                        .pr(px(ROW_INSET))
                        .child(baseline::text("Search time: median, then 95th percentile")),
                ),
        );
    }
    for (ix, tier) in s.bench.iter().enumerate() {
        let on = tier.tier == selected.tier;
        let name = tier.tier.clone();
        let (found, time) = cells(tier, slowest, stacked, t);
        let first = div()
            .flex()
            .items_center()
            .gap(px(BADGE_GAP))
            .child(
                exact_text(tier.tier.clone()).map(|d| if on { d.semibold() } else { d.regular() }),
            )
            .children(
                badges(&tier.tier, s.hook, s.mcp)
                    .into_iter()
                    .map(|(label, hook)| badge(label, hook, t)),
            );
        let row = div()
            .id(SharedString::from(format!("tier-{}", tier.tier)))
            .test_support()
            .role(Role::Button)
            .aria_label(tier.tier.clone())
            .cursor_pointer()
            .rounded(px(6.))
            .py(px(8.))
            .when(ix > 0, |d| {
                d.relative()
                    .border_t_1()
                    .border_color(transparent_black())
                    .child(css_rounded_rule_top(t.sunken, 6.))
            })
            .map(|d| {
                if on {
                    d.bg(t.sunken)
                } else {
                    d.hover(|s| s.bg(t.sunken.opacity(0.5)))
                }
            })
            .on_click(cx.listener(move |health, _, _, cx| {
                health.retrieval.picked = Some(name.clone());
                cx.notify();
            }));
        table = table.child(if stacked {
            row.px(px(ROW_INSET))
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(first)
                .child(found)
                .child(time)
        } else {
            row.pl(px(ROW_INSET))
                .flex()
                .items_center()
                .gap(px(ROW_GAP))
                .child(first.w(track - px(ROW_INSET - OVERHANG)).flex_none())
                .child(
                    found
                        .flex_basis(px(OVERHANG + ROW_INSET))
                        .flex_grow(1.)
                        .min_w(px(0.)),
                )
                .child(
                    time.flex_basis(px(0.))
                        .flex_grow(1.)
                        .min_w(px(0.))
                        .pr(px(ROW_INSET)),
                )
        });
    }
    let mut body = panel("Retrieval quality", Some(aside), "retrieval-panel");
    if let Some(then) = stats.bench_provenance.as_ref().and_then(|p| p.documents) {
        if then != stats.documents {
            body = body.child(
                div()
                    .id("bench-stale")
                    .test_support()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x(px(12.))
                    .gap_y(px(4.))
                    .bg(t.accent_soft)
                    .rounded(px(6.))
                    .px(px(14.))
                    .py(px(8.))
                    .size_sm()
                    .child(div().flex_grow(1.).min_w(px(0.)).max_w_full().child(baseline::text(format!(
                        "The index has changed since this was measured ({} documents then, {} now), so these numbers may have moved.",
                        grouped(then),
                        grouped(stats.documents)
                    ))))
                    .child(copy_command("bench-stale-command", "rhebok bench")),
            );
        }
    }
    body = body
        .child(table)
        .child(tier_detail(selected, s.bench, s.hook, s.mcp, t, window, cx));
    if let Some(hook) = s.hook {
        body = body.child(threshold_line(hook, stats.hook_threshold, t, cx));
    }
    body
}

fn tier_detail(
    tier: &BenchTier,
    all: &[BenchTier],
    hook: Option<&str>,
    mcp: Option<&str>,
    t: &Tokens,
    window: &mut Window,
    cx: &mut Context<Health>,
) -> impl IntoElement {
    let rich = detail_rich(tier, all, hook, t);
    let sentence = rich.text();
    let measure = text_width(window, FontWeight::NORMAL, 14., "0") * DETAIL_MEASURE as f32;
    let change_hook = Some(tier.tier.as_str()) != hook;
    let change_mcp = Some(tier.tier.as_str()) != mcp;
    div()
        .id("tier-detail")
        .test_support()
        .aria_label(sentence)
        .flex()
        .flex_col()
        .gap(px(10.))
        .bg(t.sunken.opacity(0.6))
        .rounded(px(6.))
        .px(px(14.))
        .py(px(12.))
        .size_sm()
        .child(div().max_w(measure).child(inline_line(
            cx,
            SM_FACE,
            &[SM_FACE, SM_SEMIBOLD],
            rich.styled(),
        )))
        .when(change_hook || change_mcp, |d| {
            d.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .when(change_hook, |d| {
                        d.child(
                            button::secondary("tier-change-hook", "Change the hook's tier")
                                .on_click(cx.listener(|health, _, _, cx| {
                                    health.open_setting("hook.quality", cx)
                                })),
                        )
                    })
                    .when(change_mcp, |d| {
                        d.child(
                            button::secondary("tier-change-mcp", "Change the search tool's tier")
                                .on_click(cx.listener(|health, _, _, cx| {
                                    health.open_setting("mcp.quality", cx)
                                })),
                        )
                    }),
            )
        })
}

pub fn to_fixed_2(value: f64) -> String {
    crate::widgets::js::to_fixed(value, 2)
}

pub fn threshold_label(threshold: f32) -> String {
    to_fixed_2(js_number(threshold))
}

fn threshold_line(
    hook: &'static str,
    threshold: f32,
    t: &Tokens,
    cx: &mut Context<Health>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_wrap()
        .size_sm()
        .text_color(t.muted)
        .children(words("Before each prompt the hook searches at "))
        .child(exact_text(hook).semibold().text_color(t.ink))
        .children(words(" and passes on only results scoring at least "))
        .child(
            dotted_link("hook-threshold", threshold_label(threshold), SM_SEMIBOLD).on_click(
                cx.listener(|health, _, _, cx| health.open_setting("hook.threshold", cx)),
            ),
        )
        .child(exact_text("."))
}

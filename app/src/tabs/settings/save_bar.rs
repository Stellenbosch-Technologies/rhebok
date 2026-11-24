use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::pending::DiffRow;
use super::Settings;
use crate::theme::{tokens, Face, Type, SANS};
use crate::widgets::baseline::web_baseline;
use crate::widgets::css_box::css_box;
use crate::widgets::tail::ellipsis_text;
use crate::widgets::{button, exact_text, text_button, tracked};

pub const DIFF_HEADING: &str = "THIS WILL BE WRITTEN TO CONFIG.TOML";
pub const REINDEX_WARNING: &str =
    "The embedding model or dimensions change: the index will need a full re-index.";
pub const SHADOW: u32 = 0x2e363c24;

const LINK_FACE: Face = Face::new(SANS, FontWeight::NORMAL, 12., 16.);
const BACK_FACE: Face = Face::new(SANS, FontWeight::SEMIBOLD, 12., 16.);

pub struct SaveBar {
    pub dirty: usize,
    pub checking: bool,
    pub blocking: usize,
    pub first_problem: Option<String>,
    pub preview: bool,
    pub saving: bool,
    pub locked: bool,
    pub can_save: bool,
    pub rows: Vec<DiffRow>,
    pub needs_reindex: bool,
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

pub fn status_text(dirty: usize) -> String {
    format!("{dirty} unsaved change{}", plural(dirty))
}

pub fn fix_text(blocking: usize) -> String {
    format!("fix {blocking} problem{} to save", plural(blocking))
}

fn act(
    view: &WeakEntity<Settings>,
    f: impl Fn(&mut Settings, &mut Context<Settings>) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let view = view.clone();
    move |_, _, cx| {
        let _ = view.update(cx, |s, cx| f(s, cx));
    }
}

fn preview(bar: &SaveBar, view: &WeakEntity<Settings>, cx: &App) -> impl IntoElement {
    let t = *tokens(cx);
    div()
        .id("diff-preview")
        .test_support()
        .flex()
        .flex_col()
        .gap(px(6.))
        .max_h(px(240.))
        .overflow_y_scroll()
        .child(
            tracked(DIFF_HEADING, SANS, FontWeight::SEMIBOLD, px(11.), 0.05)
                .line_height(px(16.5))
                .text_color(t.muted),
        )
        .children(bar.rows.iter().map(|row| {
            let path = row.path.clone();
            div()
                .id(SharedString::from(format!("diff-row-{}", row.path)))
                .test_support()
                .role(Role::Button)
                .aria_label(SharedString::from(format!(
                    "{} {} → {}",
                    row.path, row.old, row.new
                )))
                .flex()
                .gap(px(8.))
                .px(px(4.))
                .rounded(px(4.))
                .mono()
                .size_xs()
                .cursor_pointer()
                .hover(|s| s.bg(t.sunken))
                .on_click(act(view, move |s, cx| s.jump(&path, cx)))
                .child(exact_text(row.path.clone()).mono_medium())
                .child(
                    div()
                        .flex()
                        .min_w(px(0.))
                        .text_color(t.muted)
                        .child(web_baseline(ellipsis_text(row.old.clone()))),
                )
                .child(exact_text("→").text_color(t.muted))
                .child(
                    div()
                        .flex()
                        .min_w(px(0.))
                        .text_color(t.accent)
                        .child(web_baseline(ellipsis_text(row.new.clone()))),
                )
        }))
        .when(bar.needs_reindex, |d| {
            d.child(
                div()
                    .id("reindex-warning")
                    .test_support()
                    .aria_label(REINDEX_WARNING)
                    .size_xs()
                    .text_color(t.warn)
                    .child(crate::widgets::wrap::wrapped_text(REINDEX_WARNING)),
            )
        })
}

pub const SHADOW_OFFSET: f32 = -4.;
pub const SHADOW_BLUR: f32 = 16.;

pub fn raised_shadow() -> impl IntoElement {
    canvas(
        |_, _, _| (),
        |bounds, _, window, _| {
            let shadow = BoxShadow {
                color: rgba(SHADOW).into(),
                offset: point(px(0.), px(SHADOW_OFFSET)),
                blur_radius: px(SHADOW_BLUR / 2.),
                spread_radius: px(0.),
                inset: false,
            };
            let reach = bounds.dilate(px(SHADOW_BLUR - SHADOW_OFFSET));
            window.paint_layer(reach, |window| {
                window.paint_drop_shadows(bounds, Corners::all(px(8.)), &[shadow])
            });
        },
    )
    .absolute()
    .inset(px(-1.))
}

pub fn save_bar(bar: SaveBar, view: WeakEntity<Settings>, cx: &App) -> AnyElement {
    let t = *tokens(cx);
    let status = status_text(bar.dirty);
    let first = bar.first_problem.clone();
    div()
        .id("save-bar")
        .test_support()
        .relative()
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(12.))
        .px(px(16.))
        .py(px(12.))
        .border_1()
        .border_color(transparent_black())
        .rounded(px(8.))
        .child(raised_shadow())
        .child(css_box(t.surface, Some(t.line), 8.))
        .when(bar.preview, |d| d.child(preview(&bar, &view, cx)))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .id("save-bar-status")
                        .test_support()
                        .aria_label(SharedString::from(status.clone()))
                        .child(exact_text(status).semibold().size_sm()),
                )
                .when(bar.checking, |d| {
                    d.child(
                        div()
                            .id("save-bar-checking")
                            .test_support()
                            .aria_label("checking…")
                            .child(exact_text("checking…").size_xs().text_color(t.muted)),
                    )
                })
                .when(!bar.checking && bar.blocking > 0, |d| {
                    d.child(
                        text_button("save-bar-fix", fix_text(bar.blocking), LINK_FACE, t.warn)
                            .underline()
                            .on_click(act(&view, move |s, cx| {
                                if let Some(path) = first.clone() {
                                    s.jump(&path, cx);
                                }
                            })),
                    )
                })
                .child(div().flex_grow(1.))
                .child(
                    button::quiet("save-bar-discard", "Discard")
                        .py(px(8.))
                        .disabled(bar.locked)
                        .on_click(act(&view, |s, cx| s.discard(cx))),
                )
                .map(|d| {
                    if bar.preview {
                        d.child(
                            text_button("save-bar-back", "Back", BACK_FACE, t.muted)
                                .px(px(8.))
                                .py(px(8.))
                                .disabled(bar.locked)
                                .disabled_opacity(1.)
                                .on_click(act(&view, |s, cx| s.close_preview(cx))),
                        )
                        .child(
                            button::primary(
                                "save-bar-write",
                                if bar.saving {
                                    "Saving…"
                                } else {
                                    "Write config.toml"
                                },
                            )
                            .disabled(!bar.can_save)
                            .on_click(act(&view, |s, cx| s.save(cx))),
                        )
                    } else {
                        d.child(
                            button::primary("save-bar-save", "Save")
                                .disabled(!bar.can_save)
                                .on_click(act(&view, |s, cx| s.open_preview(cx))),
                        )
                    }
                }),
        )
        .into_any_element()
}

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::theme::{tokens, Type, MONO, SANS};
use crate::widgets::css_box::paint_css_box;
use crate::widgets::exact::{
    ending_at, exact_left, text_style_of as face, unrounded_width, ExactLink,
};
use crate::widgets::{baseline, exact_text};

type OnSelect = Rc<dyn Fn(usize, &mut Window, &mut App)>;

pub const INSET: f32 = 2.;
pub const GAP: f32 = 2.;
pub const TIGHT_LEADING: f32 = 1.25;
pub const SUBLINE_SIZE: f32 = 10.;

#[derive(IntoElement)]
pub struct Segmented {
    id: SharedString,
    options: Vec<SharedString>,
    selected: usize,
    disabled: bool,
    tight: bool,
    ends_at: Option<ExactLink>,
    sublines: Option<Vec<Option<SharedString>>>,
    on_select: OnSelect,
}

pub fn segmented(
    id: impl Into<SharedString>,
    options: &[&str],
    selected: usize,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Segmented {
    Segmented {
        id: id.into(),
        options: options
            .iter()
            .map(|o| SharedString::from(o.to_string()))
            .collect(),
        selected,
        disabled: false,
        tight: false,
        ends_at: None,
        sublines: None,
        on_select: Rc::new(on_select),
    }
}

impl Segmented {
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn tight(mut self) -> Self {
        self.tight = true;
        self
    }

    pub fn ends_at(mut self, link: &ExactLink) -> Self {
        self.ends_at = Some(link.clone());
        self
    }

    pub fn stacked(mut self, sublines: Vec<Option<SharedString>>) -> Self {
        self.sublines = Some(sublines);
        self
    }
}

pub const CHIP_BLURS: [f32; 2] = [3., 2.];

pub fn chip_shadow() -> Vec<BoxShadow> {
    let shade = hsla(0., 0., 0., 0.1);
    vec![
        BoxShadow::new(px(0.), px(1.), shade).blur_radius(px(CHIP_BLURS[0] / 2.)),
        BoxShadow::new(px(0.), px(1.), shade)
            .blur_radius(px(CHIP_BLURS[1] / 2.))
            .spread_radius(px(-1.)),
    ]
}

pub fn stacked_height(has_sublines: bool) -> f32 {
    let label = 12. * TIGHT_LEADING;
    let sub = if has_sublines {
        SUBLINE_SIZE * TIGHT_LEADING
    } else {
        0.
    };
    label + sub
}

pub fn chip_lefts(widths: &[f32]) -> (Vec<f32>, f32) {
    let mut lefts = Vec::with_capacity(widths.len());
    let mut x = INSET;
    for (ix, width) in widths.iter().enumerate() {
        if ix > 0 {
            x += GAP;
        }
        lefts.push(x);
        x += width;
    }
    (lefts, x + INSET)
}

impl RenderOnce for Segmented {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *tokens(cx);
        let disabled = self.disabled;
        let tight = self.tight;
        let (pad_x, pad_y) = if tight { (10., 4.) } else { (12., 6.) };
        let sublines = self.sublines.clone();
        let stacked = sublines.is_some();
        let content_height = match &sublines {
            Some(lines) => stacked_height(lines.iter().any(Option::is_some)),
            None => 16.,
        };
        let chip_height = content_height + 2. * pad_y;
        let sub_face = face(window, MONO, FontWeight::NORMAL, SUBLINE_SIZE);
        let widths: Vec<f32> = self
            .options
            .iter()
            .enumerate()
            .map(|(ix, label)| {
                let weight = if ix == self.selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                };
                let label = f32::from(unrounded_width(
                    window,
                    label,
                    &face(window, SANS, weight, 12.),
                ));
                let sub = sublines
                    .as_ref()
                    .and_then(|lines| lines.get(ix).cloned().flatten())
                    .map_or(0., |sub| {
                        f32::from(unrounded_width(window, &sub, &sub_face))
                    });
                label.max(sub) + 2. * pad_x
            })
            .collect();
        let (lefts, total) = chip_lefts(&widths);
        let ends_at = self.ends_at.clone();
        let exact_end = ends_at.clone();
        let chosen = lefts
            .get(self.selected)
            .copied()
            .zip(widths.get(self.selected).copied());
        let control = div()
            .id(ElementId::Name(self.id.clone()))
            .test_support()
            .role(Role::RadioGroup)
            .relative()
            .flex_none()
            .w(px(total.round()))
            .h(px(chip_height + 2. * INSET))
            .rounded(px(6.))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let left = exact_end
                            .as_ref()
                            .and_then(|end| exact_left(end, px(total), bounds.left()))
                            .map_or(f32::from(bounds.origin.x).round(), f32::from);
                        let origin = point(px(left), px(f32::from(bounds.origin.y).round()));
                        let frame = Bounds {
                            origin,
                            size: size(px(total), px(chip_height + 2. * INSET)),
                        };
                        paint_css_box(window, frame, 6., t.sunken, None);
                        if let Some((left, width)) = chosen {
                            let chip = Bounds {
                                origin: point(origin.x + px(left), origin.y + px(INSET)),
                                size: size(px(width), px(chip_height)),
                            };
                            if !tight {
                                window.paint_drop_shadows(
                                    chip,
                                    Corners::all(px(4.)),
                                    &chip_shadow(),
                                );
                            }
                            paint_css_box(window, chip, 4., t.surface, None);
                        }
                    },
                )
                .absolute()
                .inset_0(),
            )
            .children(self.options.into_iter().enumerate().map(|(ix, label)| {
                let chosen = ix == self.selected;
                let on_select = self.on_select.clone();
                let subline = sublines
                    .as_ref()
                    .and_then(|lines| lines.get(ix).cloned().flatten());
                let id = self.id.clone();
                div()
                    .id(ElementId::Name(format!("{}-{ix}", self.id).into()))
                    .test_support()
                    .role(Role::RadioButton)
                    .aria_label(label.clone())
                    .aria_toggled(if chosen {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .absolute()
                    .top(px(INSET))
                    .left(px(lefts[ix]))
                    .w(px(widths[ix]))
                    .h(px(chip_height))
                    .when(!stacked, |d| d.pl(px(pad_x)))
                    .pt(px(pad_y))
                    .size_xs()
                    .rounded(px(4.))
                    .map(|d| {
                        if chosen {
                            d.semibold().text_color(t.accent)
                        } else {
                            d.regular().text_color(t.muted)
                        }
                    })
                    .when(disabled, |d| d.cursor_not_allowed())
                    .when(!disabled, |d| {
                        d.cursor_pointer()
                            .on_click(move |_, window, cx| on_select(ix, window, cx))
                    })
                    .map(|d| {
                        if stacked {
                            d.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .child(exact_text(label).line_height(px(12. * TIGHT_LEADING)))
                                    .when_some(subline, |d, sub| {
                                        d.child(
                                            div()
                                                .id(ElementId::Name(
                                                    format!("{id}-{ix}-bench").into(),
                                                ))
                                                .test_support()
                                                .aria_label(sub.clone())
                                                .child(
                                                    exact_text(sub)
                                                        .mono()
                                                        .text_size(px(SUBLINE_SIZE))
                                                        .line_height(px(
                                                            SUBLINE_SIZE * TIGHT_LEADING
                                                        ))
                                                        .text_color(t.muted),
                                                ),
                                        )
                                    }),
                            )
                        } else if tight {
                            d.child(exact_text(label))
                        } else {
                            d.child(baseline::text(label))
                        }
                    })
            }));
        match ends_at {
            Some(link) => ending_at(&link, px(total), control).into_any_element(),
            None => control.into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        div, px, size, AppContext, Background, Bounds, Context, IntoElement, ParentElement, Pixels,
        Point, Render, Styled, TestAppContext, TestDispatcher, Window, WindowBounds, WindowOptions,
    };

    use super::{chip_lefts, segmented};
    use crate::tabs::search::tier_row;
    use crate::theme::{self, tokens};
    use crate::widgets::button;
    use crate::widgets::exact::row_ending_before;

    const TIERS: [&str; 5] = ["instant", "fast", "balanced", "thorough", "exhaustive"];

    struct SearchRow {
        trailing: Option<Pixels>,
    }

    impl Render for SearchRow {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let (row, end) = match self.trailing {
                Some(trailing) => row_ending_before(trailing),
                None => tier_row(window, None),
            };
            row.flex()
                .items_center()
                .gap(px(12.))
                .w(px(1384.))
                .child(div().flex_grow(1.).h(px(36.)))
                .child(
                    segmented("search-tier", &TIERS, 1, |_, _, _| ())
                        .tight()
                        .ends_at(&end),
                )
                .child(button::primary("search-button", "Search"))
        }
    }

    type Spans = (Vec<(f32, f32)>, Vec<(f32, f32)>, f32);

    fn painted_spans(view: impl FnOnce() -> SearchRow + 'static) -> Spans {
        let mut cx = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            None,
            Arc::new(gpui_apple::AppleTextSystem::new()),
        );
        let (window, _) = cx.update(|cx| {
            theme::install(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1400.), px(100.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| view()),
            )
            .unwrap()
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let t = *tokens(cx);
            let spans = |colour| {
                let wanted = Background::from(colour);
                window
                    .painted_quads()
                    .into_iter()
                    .filter(|q| q.background == wanted)
                    .map(|q| {
                        (
                            q.bounds.origin.x.0,
                            q.bounds.origin.x.0 + q.bounds.size.width.0,
                        )
                    })
                    .collect::<Vec<_>>()
            };
            let label = f32::from(window.find("search-tier-1").bounds().origin.x);
            (spans(t.sunken), spans(t.surface), label)
        })
        .unwrap()
    }

    #[gpui_kit::test]
    fn a_right_anchored_control_paints_its_pills_where_chrome_does(_cx: &mut TestAppContext) {
        let (frames, chips, label) = painted_spans(|| SearchRow { trailing: None });
        assert_eq!(frames.first().map(|f| f.0), Some(1974.), "{frames:?}");
        assert_eq!(
            chips,
            [(2090., 2168.)],
            "Chrome lays the tier control out at x=986.609 and paints the chosen pill over device px 2090..2168"
        );
        assert!(
            (label - 1045.39).abs() <= 0.25,
            "Chrome places the fast label at x=1045.39, within the half device pixel GPUI snaps it to: {label}"
        );
    }

    #[gpui_kit::test]
    fn an_end_more_than_a_pixel_from_the_layout_is_ignored(_cx: &mut TestAppContext) {
        let (_, chips, label) = painted_spans(|| SearchRow {
            trailing: Some(px(12.)),
        });
        assert_eq!(
            chips,
            [(2092., 2170.)],
            "an end 73px from where layout put the control is not Chrome's origin; the pill keeps the laid-out one"
        );
        assert_eq!(label, 1046., "and so do the labels and their hit boxes");
    }

    #[test]
    fn chips_sit_at_chromes_fractional_offsets() {
        let (lefts, total) = chip_lefts(&[32.7, 52.14, 38.73, 59.72]);
        let rounded: Vec<f32> = lefts.iter().map(|l| (l * 100.).round() / 100.).collect();
        assert_eq!(rounded, [2., 36.7, 90.84, 131.57]);
        assert!((total - 193.29).abs() < 0.001, "{total}");
    }
}

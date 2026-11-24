use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::theme::{tokens, Type, SANS};
use crate::widgets::css_box::css_box;
use crate::widgets::exact::unrounded_width;
use crate::widgets::tail::ellipsis_text;
use crate::widgets::{baseline, exact_text};

pub const CHROME_EXTRA: f32 = 38.;
pub const TEXT_INSET: f32 = 13.;
pub const PAD_X: f32 = 8.;
pub const MENULIST_INSET: f32 = 4.;
pub const ARROW_RIGHT: f32 = 4.3;
pub const ARROW_WIDTH: f32 = 9.;
pub const ARROW_HEIGHT: f32 = 5.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectSize {
    Xs,
    Sm,
}

impl SelectSize {
    pub fn font_size(self) -> f32 {
        match self {
            SelectSize::Xs => 12.,
            SelectSize::Sm => 14.,
        }
    }

    pub fn line_height(self) -> f32 {
        match self {
            SelectSize::Xs => 19.,
            SelectSize::Sm => 22.,
        }
    }

    pub fn height(self) -> f32 {
        self.line_height() + 14.
    }
}

type OnSelect = Rc<dyn Fn(usize, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Select {
    id: SharedString,
    options: Vec<SharedString>,
    selected: usize,
    size: SelectSize,
    pad_x: f32,
    disabled: bool,
    style: StyleRefinement,
    on_select: OnSelect,
}

pub fn select(
    id: impl Into<SharedString>,
    options: Vec<SharedString>,
    selected: usize,
    size: SelectSize,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Select {
    Select {
        id: id.into(),
        options,
        selected,
        size,
        pad_x: PAD_X,
        disabled: false,
        style: StyleRefinement::default(),
        on_select: Rc::new(on_select),
    }
}

pub fn option_id(select: &str, ix: usize) -> SharedString {
    format!("{select}-option-{ix}").into()
}

impl Select {
    pub fn pad_x(mut self, pad_x: f32) -> Self {
        self.pad_x = pad_x;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Styled for Select {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

pub fn intrinsic_width(window: &Window, options: &[SharedString], size: SelectSize) -> Pixels {
    let mut face = window.text_style();
    face.font_family = SANS.into();
    face.font_weight = FontWeight::NORMAL;
    face.font_size = px(size.font_size()).into();
    let widest = options
        .iter()
        .map(|o| unrounded_width(window, o, &face))
        .fold(px(0.), |a, b| a.max(b));
    px(f32::from(widest).ceil() + CHROME_EXTRA)
}

fn arrow(colour: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let at = |x: f32, y: f32| point(bounds.origin.x + px(x), bounds.origin.y + px(y));
            let mut path = PathBuilder::stroke(px(2.));
            path.move_to(at(0.9, 0.9));
            path.line_to(at(ARROW_WIDTH / 2., ARROW_HEIGHT - 0.6));
            path.line_to(at(ARROW_WIDTH - 0.9, 0.9));
            if let Ok(path) = path.build() {
                window.paint_path(path, colour);
            }
        },
    )
    .w(px(ARROW_WIDTH))
    .h(px(ARROW_HEIGHT))
}

impl RenderOnce for Select {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *tokens(cx);
        let open = window.use_keyed_state(
            ElementId::Name(format!("{}-open", self.id).into()),
            cx,
            |_, _| false,
        );
        let is_open = *open.read(cx) && !self.disabled;
        let natural =
            intrinsic_width(window, &self.options, self.size) + px(2. * (self.pad_x - PAD_X));
        let label = self.options.get(self.selected).cloned().unwrap_or_default();
        let size = self.size;
        let toggle = open.clone();
        let mut frame = div()
            .id(ElementId::Name(self.id.clone()))
            .relative()
            .w(natural)
            .h(px(size.height()))
            .border_1()
            .border_color(transparent_black())
            .rounded(px(6.))
            .text_color(t.ink)
            .regular()
            .text_size(px(size.font_size()))
            .line_height(px(size.line_height()));
        frame.style().refine(&self.style);
        let menu_id = self.id.clone();
        let options = self.options.clone();
        let on_select = self.on_select.clone();
        frame
            .test_support()
            .role(Role::ComboBox)
            .aria_label(label.clone())
            .aria_expanded(is_open)
            .child(css_box(t.surface, Some(t.line), 6.))
            .when(self.disabled, |d| d.cursor_default())
            .when(!self.disabled, |d| {
                d.cursor_default().on_click(move |_, _, cx| {
                    toggle.update(cx, |open, cx| {
                        *open = !*open;
                        cx.notify();
                    })
                })
            })
            .child(
                div()
                    .id(ElementId::Name(format!("{}-text", self.id).into()))
                    .test_support()
                    .absolute()
                    .top(px(6.))
                    .left(px(self.pad_x + MENULIST_INSET))
                    .right(px(ARROW_RIGHT + ARROW_WIDTH + 2.))
                    .h(px(size.line_height()))
                    .flex()
                    .overflow_hidden()
                    .child(baseline::web_baseline(ellipsis_text(label))),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(px(ARROW_RIGHT - 1.))
                    .flex()
                    .items_center()
                    .child(arrow(t.ink)),
            )
            .when(is_open, |d| {
                let close = open.clone();
                d.child(
                    deferred(
                        anchored().snap_to_window().child(
                            div()
                                .id(ElementId::Name(format!("{menu_id}-menu").into()))
                                .occlude()
                                .mt(px(size.height()))
                                .min_w(natural)
                                .py(px(4.))
                                .bg(t.surface)
                                .border_1()
                                .border_color(t.line)
                                .rounded(px(6.))
                                .shadow_md()
                                .on_mouse_down_out(move |_, _, cx| {
                                    close.update(cx, |open, cx| {
                                        *open = false;
                                        cx.notify();
                                    })
                                })
                                .children(options.into_iter().enumerate().map(|(ix, option)| {
                                    let on_select = on_select.clone();
                                    let open = open.clone();
                                    div()
                                        .id(ElementId::Name(option_id(&menu_id, ix)))
                                        .test_support()
                                        .role(Role::MenuItem)
                                        .aria_label(option.clone())
                                        .px(px(TEXT_INSET - 1.))
                                        .py(px(2.))
                                        .whitespace_nowrap()
                                        .hover(|s| s.bg(t.accent_soft))
                                        .when(ix == self.selected, |d| d.semibold())
                                        .on_click(move |_, window, cx| {
                                            open.update(cx, |open, cx| {
                                                *open = false;
                                                cx.notify();
                                            });
                                            on_select(ix, window, cx);
                                        })
                                        .child(exact_text(option))
                                })),
                        ),
                    )
                    .with_priority(1),
                )
            })
            .when(self.disabled, |d| d.opacity(0.6))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui_kit::base::input::TextareaState;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        div, px, size, AppContext, Bounds, Context, Entity, IntoElement, ParentElement, Pixels,
        Point, Render, SharedString, Styled, TestAppContext, TestDispatcher, Window, WindowBounds,
        WindowOptions,
    };

    use super::{select, SelectSize};
    use crate::theme::{self, tokens};
    use crate::widgets::segmented;
    use crate::widgets::text_input::{text_area, textarea_state};

    struct Probe {
        state: Entity<TextareaState>,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .pt(px(74.5))
                .pl(px(16.35))
                .flex()
                .items_start()
                .gap(px(12.))
                .child(
                    segmented(
                        "seg",
                        &["all", "lesson", "fact", "episode"],
                        0,
                        |_, _, _| {},
                    )
                    .tight(),
                )
                .child(select(
                    "sel",
                    vec![SharedString::from("every scope")],
                    0,
                    SelectSize::Xs,
                    |_, _, _| {},
                ))
                .child(
                    text_area("area", "area", &self.state)
                        .w(px(200.3))
                        .h(px(200.))
                        .px(px(10.))
                        .py(px(8.)),
                )
        }
    }

    fn whole_css_px(v: f32) -> bool {
        (v / 2.).fract().abs() < 0.01
    }

    #[gpui_kit::test]
    fn frames_are_painted_on_whole_css_pixels_like_chrome(_cx: &mut TestAppContext) {
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
                        size: size(px(900.), px(400.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        let t = *tokens(cx);
                        Probe {
                            state: textarea_state("", 8, &t, window, cx),
                        }
                    })
                },
            )
            .unwrap()
        });
        let off: Vec<String> = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                let frames: Vec<Bounds<Pixels>> = ["seg", "seg-0", "sel", "area"]
                    .map(|id| window.find(id).bounds())
                    .to_vec();
                window
                    .painted_quads()
                    .into_iter()
                    .filter(|q| {
                        let o = q.bounds.origin;
                        frames.iter().any(|f| {
                            let (x, y) = (f32::from(f.origin.x) * 2., f32::from(f.origin.y) * 2.);
                            (o.x.0 - x).abs() <= 3. && (o.y.0 - y).abs() <= 3.
                        })
                    })
                    .filter(|q| {
                        let b = q.bounds;
                        ![b.origin.x.0, b.origin.y.0, b.size.width.0, b.size.height.0]
                            .into_iter()
                            .all(whole_css_px)
                    })
                    .map(|q| format!("{:?}", q.bounds))
                    .collect()
            })
            .unwrap();
        assert!(
            off.is_empty(),
            "frames painted off whole CSS pixels: {off:?}"
        );
    }

    struct Toolbar;

    impl Render for Toolbar {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .pt(px(72.))
                .pl(px(16.))
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    segmented(
                        "kind",
                        &["all", "lesson", "fact", "episode"],
                        0,
                        |_, _, _| {},
                    )
                    .tight(),
                )
                .child(
                    select(
                        "scope",
                        vec![
                            SharedString::from("every scope"),
                            SharedString::from(
                                "/var/folders/y4/kmqkwsz575zfpdd44snr727m0000gn/T/corpus",
                            ),
                        ],
                        0,
                        SelectSize::Xs,
                        |_, _, _| {},
                    )
                    .max_w(px(220.)),
                )
        }
    }

    #[gpui_kit::test]
    fn the_scope_select_is_painted_where_chrome_paints_it(_cx: &mut TestAppContext) {
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
                        size: size(px(900.), px(200.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| Toolbar),
            )
            .unwrap()
        });
        let painted = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                let frame = window.find("scope").bounds();
                window
                    .painted_quads()
                    .into_iter()
                    .find(|q| {
                        q.border_widths.left.0 > 0.
                            && (q.bounds.origin.x.0 / 2. - f32::from(frame.origin.x)).abs() <= 1.
                    })
                    .map(|q| {
                        (
                            q.bounds.origin.x.0,
                            (q.bounds.origin.x + q.bounds.size.width).0,
                        )
                    })
            })
            .unwrap();
        assert_eq!(
            painted,
            Some((CHROME_LEFT * 2., CHROME_RIGHT * 2.)),
            "Chrome lays the select out at 221.297 and paints its border from 221 to 441"
        );
    }

    const CHROME_LEFT: f32 = 221.;
    const CHROME_RIGHT: f32 = 441.;
}

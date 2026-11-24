use gpui_kit::*;

pub fn css_px_box(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    let snap = |p: Point<Pixels>| point(px(f32::from(p.x).round()), px(f32::from(p.y).round()));
    Bounds::from_corners(snap(bounds.origin), snap(bounds.bottom_right()))
}

pub fn paint_css_box(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    radius: f32,
    background: Hsla,
    border: Option<Hsla>,
) {
    window.paint_quad(quad(
        css_px_box(bounds),
        px(radius),
        background,
        px(if border.is_some() { 1. } else { 0. }),
        border.unwrap_or(transparent_black()),
        BorderStyle::Solid,
    ));
}

pub type ExactSpan = std::rc::Rc<dyn Fn() -> Option<(Pixels, Pixels)>>;

fn on_exact_span(bounds: Bounds<Pixels>, span: Option<&ExactSpan>, outset: f32) -> Bounds<Pixels> {
    match span.and_then(|span| span()) {
        Some((left, width)) => Bounds {
            origin: point(left - px(outset), bounds.origin.y),
            size: size(width + px(2. * outset), bounds.size.height),
        },
        None => bounds,
    }
}

pub fn css_box(background: Hsla, border: Option<Hsla>, radius: f32) -> impl IntoElement {
    css_box_at(background, border, radius, None)
}

pub fn css_box_at(
    background: Hsla,
    border: Option<Hsla>,
    radius: f32,
    span: Option<ExactSpan>,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let bounds = on_exact_span(bounds, span.as_ref(), 0.);
            paint_css_box(window, bounds, radius, background, border)
        },
    )
    .absolute()
    .inset(px(if border.is_some() { -1. } else { 0. }))
}

pub fn css_rule_top(colour: Hsla) -> impl IntoElement {
    css_rule(colour).top(px(-1.))
}

pub fn css_rule_bottom(colour: Hsla) -> impl IntoElement {
    css_rule(colour).bottom(px(-1.))
}

pub fn css_rounded_rule_top(colour: Hsla, radius: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            window.paint_quad(quad(
                css_px_box(bounds),
                px(radius),
                transparent_black(),
                Edges {
                    top: px(1.),
                    ..Edges::default()
                },
                colour,
                BorderStyle::Solid,
            ))
        },
    )
    .absolute()
    .top(px(-1.))
    .left_0()
    .right_0()
    .bottom_0()
}

fn css_rule(colour: Hsla) -> Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| window.paint_quad(fill(css_px_box(bounds), colour)),
    )
    .absolute()
    .left_0()
    .right_0()
    .h(px(1.))
}

pub fn css_ring(colour: Hsla, width: f32, radius: f32) -> impl IntoElement {
    css_ring_at(colour, width, radius, None)
}

pub fn css_ring_at(
    colour: Hsla,
    width: f32,
    radius: f32,
    span: Option<ExactSpan>,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            window.paint_quad(quad(
                css_px_box(on_exact_span(bounds, span.as_ref(), width)),
                px(radius),
                transparent_black(),
                px(width),
                colour,
                BorderStyle::Solid,
            ));
        },
    )
    .absolute()
    .inset(px(-1. - width))
}

pub fn css_px_box_exact(
    bounds: Bounds<Pixels>,
    width: Option<Pixels>,
    scale: f32,
) -> Bounds<Pixels> {
    let Some(width) = width.filter(|w| (bounds.size.width - *w).abs() <= px(0.5)) else {
        return css_px_box(bounds);
    };
    let reach = 0.5 / scale;
    let (left, right, width) = (
        f32::from(bounds.left()),
        f32::from(bounds.right()),
        f32::from(width),
    );
    let lowest = (left - reach).max(right - width - reach);
    let highest = (left + reach).min(right - width + reach);
    let exact_left = if lowest <= highest {
        (lowest + highest) / 2.
    } else {
        left
    };
    css_px_box(Bounds::from_corners(
        point(px(exact_left), bounds.top()),
        point(px(exact_left + width), bounds.bottom()),
    ))
}

pub fn css_box_exact(
    background: Hsla,
    border: Option<Hsla>,
    radius: f32,
    width: Option<Pixels>,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let scale = window.scale_factor();
            window.paint_quad(quad(
                css_px_box_exact(bounds, width, scale),
                px(radius),
                background,
                px(if border.is_some() { 1. } else { 0. }),
                border.unwrap_or(transparent_black()),
                BorderStyle::Solid,
            ))
        },
    )
    .absolute()
    .inset(px(if border.is_some() { -1. } else { 0. }))
}

pub fn css_box_after(
    background: Hsla,
    radius: f32,
    width: Pixels,
    link: crate::widgets::exact::ExactLink,
    gap: Pixels,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let scale = window.scale_factor();
            let painted = match link.end() {
                Some(end) => css_px_box(Bounds::from_corners(
                    point(end + gap, bounds.top()),
                    point(end + gap + width, bounds.bottom()),
                )),
                None => css_px_box_exact(bounds, Some(width), scale),
            };
            window.paint_quad(quad(
                painted,
                px(radius),
                background,
                px(0.),
                transparent_black(),
                BorderStyle::Solid,
            ))
        },
    )
    .absolute()
    .inset_0()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        div, px, size, transparent_black, AppContext, Bounds, Context, IntoElement, ParentElement,
        Point, Render, Styled, TestAppContext, TestDispatcher, Window, WindowBounds, WindowOptions,
    };

    use super::{css_rule_bottom, css_rule_top};
    use crate::theme::{self, tokens};

    struct Rows;

    impl Render for Rows {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let t = *tokens(cx);
            div().pt(px(179.75)).child(
                div()
                    .relative()
                    .h(px(30.))
                    .border_t_1()
                    .border_color(transparent_black())
                    .child(css_rule_top(t.sunken)),
            )
        }
    }

    struct Footer;

    impl Render for Footer {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let t = *tokens(cx);
            div().pt(px(149.75)).child(
                div()
                    .relative()
                    .h(px(30.))
                    .border_b_1()
                    .border_color(transparent_black())
                    .child(css_rule_bottom(t.sunken)),
            )
        }
    }

    #[gpui_kit::test]
    fn a_bottom_rule_paints_on_the_whole_css_pixel_chrome_snaps_it_to(_cx: &mut TestAppContext) {
        assert_eq!(
            painted_rules(|| Footer),
            [(358., 2.)],
            "Chrome paints a border-bottom laid out at y=178.75 on CSS pixel 179"
        );
    }

    fn painted_rules<V: Render>(view: impl FnOnce() -> V + 'static) -> Vec<(f32, f32)> {
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
                        size: size(px(400.), px(300.)),
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
            let sunken = gpui_kit::Background::from(tokens(cx).sunken);
            window
                .painted_quads()
                .into_iter()
                .filter(|q| q.background == sunken)
                .map(|q| (q.bounds.origin.y.0, q.bounds.size.height.0))
                .collect::<Vec<_>>()
        })
        .unwrap()
    }

    #[gpui_kit::test]
    fn a_rule_paints_on_the_whole_css_pixel_chrome_snaps_it_to(_cx: &mut TestAppContext) {
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
                        size: size(px(400.), px(300.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| Rows),
            )
            .unwrap()
        });
        let rules = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                let sunken = gpui_kit::Background::from(tokens(cx).sunken);
                window
                    .painted_quads()
                    .into_iter()
                    .filter(|q| q.background == sunken)
                    .map(|q| (q.bounds.origin.y.0, q.bounds.size.height.0))
                    .collect::<Vec<_>>()
            })
            .unwrap();
        assert_eq!(
            rules,
            [(360., 2.)],
            "Chrome paints a border-top laid out at y=179.75 on CSS pixel 180"
        );
    }
}

use gpui_kit::base::input::{Input, InputEditorStyle, InputState, Textarea, TextareaState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::theme::{tokens, Tokens};
use crate::widgets::baseline;
use crate::widgets::css_box::{css_box, css_box_at, css_ring, css_ring_at, ExactSpan};

pub fn editor_style(t: &Tokens) -> InputEditorStyle {
    InputEditorStyle {
        foreground: t.ink,
        muted_foreground: t.ink.opacity(0.5),
        background: t.surface,
        border: t.line,
        selection: t.accent_soft,
        caret: t.ink,
        ..InputEditorStyle::default()
    }
}

pub fn input_state<V: 'static>(
    placeholder: &str,
    value: &str,
    t: &Tokens,
    window: &mut Window,
    cx: &mut Context<V>,
) -> Entity<InputState> {
    let style = editor_style(t);
    let placeholder = SharedString::from(placeholder.to_string());
    let value = SharedString::from(value.to_string());
    cx.new(|cx| {
        let mut state = InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(value);
        state.set_editor_style(style);
        state
    })
}

pub fn textarea_state<V: 'static>(
    value: &str,
    rows: usize,
    t: &Tokens,
    window: &mut Window,
    cx: &mut Context<V>,
) -> Entity<TextareaState> {
    let style = editor_style(t);
    let value = SharedString::from(value.to_string());
    cx.new(|cx| {
        let mut state = TextareaState::new(window, cx)
            .rows(rows)
            .default_value(value);
        state.set_editor_style(style);
        state
    })
}

#[derive(IntoElement)]
pub struct TextInput {
    id: ElementId,
    label: SharedString,
    state: Entity<InputState>,
    background: Option<Hsla>,
    frame_span: Option<ExactSpan>,
    style: StyleRefinement,
}

pub fn text_input(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    state: &Entity<InputState>,
) -> TextInput {
    TextInput {
        id: id.into(),
        label: label.into(),
        state: state.clone(),
        background: None,
        frame_span: None,
        style: StyleRefinement::default(),
    }
}

impl TextInput {
    pub fn background(mut self, colour: Hsla) -> Self {
        self.background = Some(colour);
        self
    }

    pub fn frame_span(mut self, span: Option<ExactSpan>) -> Self {
        self.frame_span = span;
        self
    }
}

impl Styled for TextInput {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TextInput {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *tokens(cx);
        let focused = self.state.read(cx).focus_handle(cx).is_focused(window);
        let mut frame = div()
            .id(self.id)
            .relative()
            .border_1()
            .border_color(transparent_black())
            .rounded(px(RADIUS))
            .text_color(t.ink);
        frame.style().refine(&self.style);
        frame
            .test_support()
            .aria_label(self.label)
            .child(css_box_at(
                self.background.unwrap_or(t.surface),
                Some(t.line),
                RADIUS,
                self.frame_span.clone(),
            ))
            .child(baseline::web_baseline(Input::new(&self.state)))
            .when(focused, |d| {
                d.child(css_ring_at(
                    t.accent,
                    RING_WIDTH,
                    RADIUS + RING_WIDTH,
                    self.frame_span,
                ))
            })
    }
}

pub const GRIP_COLOUR: u32 = 0x5C5C5C;
pub const GRIP_SIZE: f32 = 12.;
pub const RADIUS: f32 = 6.;
pub const RING_WIDTH: f32 = 2.;

fn resize_grip() -> impl IntoElement {
    canvas(
        |_, _, _| (),
        |bounds, _, window, _| {
            let colour: Hsla = rgb(GRIP_COLOUR).into();
            let corner = bounds.bottom_right();
            let at = |x: f32, y: f32| point(corner.x - px(x), corner.y - px(y));
            for (from, to) in [((10., 3.), (3., 10.)), ((6., 3.), (3., 6.))] {
                let mut path = PathBuilder::stroke(px(1.));
                path.move_to(at(from.0, from.1));
                path.line_to(at(to.0, to.1));
                if let Ok(path) = path.build() {
                    window.paint_path(path, colour);
                }
            }
        },
    )
    .absolute()
    .right_0()
    .bottom_0()
    .size(px(GRIP_SIZE))
}

#[derive(IntoElement)]
pub struct TextArea {
    id: ElementId,
    label: SharedString,
    state: Entity<TextareaState>,
    style: StyleRefinement,
}

pub fn text_area(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    state: &Entity<TextareaState>,
) -> TextArea {
    TextArea {
        id: id.into(),
        label: label.into(),
        state: state.clone(),
        style: StyleRefinement::default(),
    }
}

impl Styled for TextArea {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Resize {
    height: Option<Pixels>,
    drag: Option<(Pixels, Pixels)>,
}

fn style_height(style: &StyleRefinement) -> Option<Pixels> {
    match style.size.height {
        Some(Length::Definite(DefiniteLength::Absolute(AbsoluteLength::Pixels(height)))) => {
            Some(height)
        }
        _ => None,
    }
}

fn drag_listener(resize: Entity<Resize>, minimum: Pixels) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            if resize.read(cx).drag.is_none() {
                return;
            }
            let moving = resize.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                moving.update(cx, |r, cx| {
                    if let Some((start_y, start_height)) = r.drag {
                        r.height = Some((start_height + event.position.y - start_y).max(minimum));
                        cx.notify();
                    }
                });
            });
            let releasing = resize.clone();
            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                releasing.update(cx, |r, cx| {
                    r.drag = None;
                    cx.notify();
                });
            });
        },
    )
    .absolute()
    .size_0()
}

impl RenderOnce for TextArea {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *tokens(cx);
        let focused = self.state.read(cx).focus_handle(cx).is_focused(window);
        let minimum = style_height(&self.style);
        let resize = window.use_keyed_state(
            ElementId::Name(format!("{}-resize", self.id).into()),
            cx,
            |_, _| Resize::default(),
        );
        let height = resize.read(cx).height.or(minimum);
        let mut frame = div()
            .id(self.id)
            .relative()
            .border_1()
            .border_color(transparent_black())
            .rounded(px(RADIUS))
            .text_color(t.ink);
        frame.style().refine(&self.style);
        let pressed = resize.clone();
        frame
            .when_some(height, |d, height| d.h(height))
            .test_support()
            .aria_label(self.label)
            .child(css_box(t.surface, Some(t.line), RADIUS))
            .child(baseline::on_whole_pixel_line(Textarea::new(&self.state)))
            .child(resize_grip())
            .when(focused, |d| {
                d.child(css_ring(t.accent, RING_WIDTH, RADIUS + RING_WIDTH))
            })
            .when_some(minimum, |d, minimum| {
                d.child(
                    div()
                        .id("resize-grip")
                        .absolute()
                        .right_0()
                        .bottom_0()
                        .size(px(GRIP_SIZE))
                        .cursor(CursorStyle::ResizeUpDown)
                        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                            cx.stop_propagation();
                            pressed.update(cx, |r, cx| {
                                r.drag = Some((event.position.y, height.unwrap_or(minimum)));
                                cx.notify();
                            });
                        }),
                )
                .child(drag_listener(resize.clone(), minimum))
            })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui_kit::base::input::InputState;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        div, px, size, AppContext, Bounds, Context, Entity, FontWeight, IntoElement, ParentElement,
        Point, Render, Styled, TestAppContext, TestDispatcher, Window, WindowBounds, WindowOptions,
    };

    use super::{input_state, text_input};
    use crate::theme::{self, css_line_box, gpui_baseline, tokens, Face, Type, MONO, SANS};

    const SEARCH: Face = Face::new(SANS, FontWeight::NORMAL, 14., 20.);
    const FILTER: Face = Face::new(SANS, FontWeight::NORMAL, 14., 20.);
    const IGNORE: Face = Face::new(MONO, FontWeight::NORMAL, 12., 16.);

    struct Probe {
        inputs: Vec<Entity<InputState>>,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let t = *tokens(cx);
            div()
                .child(
                    text_input("search", "search", &self.inputs[0])
                        .mt(px(72.))
                        .px(px(12.))
                        .py(px(8.))
                        .regular()
                        .size_sm(),
                )
                .child(
                    text_input("filter", "filter", &self.inputs[1])
                        .mt(px(31.25))
                        .px(px(10.))
                        .py(px(4.))
                        .background(t.ground)
                        .regular()
                        .size_sm(),
                )
                .child(
                    text_input("ignore", "ignore", &self.inputs[2])
                        .mt(px(17.5))
                        .px(px(10.))
                        .py(px(6.))
                        .mono()
                        .size_xs(),
                )
        }
    }

    #[gpui_kit::test]
    fn input_text_sits_on_the_web_baseline(_cx: &mut TestAppContext) {
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
                        size: size(px(600.), px(400.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        let t = *tokens(cx);
                        Probe {
                            inputs: ["a", "b", "c"]
                                .map(|v| input_state("", v, &t, window, cx))
                                .to_vec(),
                        }
                    })
                },
            )
            .unwrap()
        });
        for (frame, face, padding) in [
            ("search", SEARCH, 8.),
            ("filter", FILTER, 4.),
            ("ignore", IGNORE, 6.),
        ] {
            let (content_top, editor_top, web, gpui) = cx
                .update_window(window, |_, window, cx| {
                    window.render_frame(cx);
                    let frame_top = f32::from(window.find(frame).bounds().origin.y);
                    let editor = window.within(frame).find("input-state").bounds();
                    (
                        frame_top + 1. + padding,
                        f32::from(editor.origin.y),
                        css_line_box(cx, face, &[face]).baseline,
                        gpui_baseline(cx, &[face], face.line_height),
                    )
                })
                .unwrap();
            let painted = ((editor_top + gpui) * 2.).round();
            let chrome = (content_top.round() + web) * 2.;
            assert_eq!(
                painted, chrome,
                "{frame}: glyph baseline at device {painted}, Chrome paints it at {chrome} (content top {content_top})"
            );
        }
    }
}

#[cfg(test)]
mod frame_tests {
    use std::sync::Arc;

    use gpui_kit::base::input::InputState;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        div, px, size, AppContext, Background, Bounds, Context, Entity, Focusable, IntoElement,
        ParentElement, Point, Render, Styled, TestAppContext, TestDispatcher, Window, WindowBounds,
        WindowOptions,
    };

    use super::{input_state, text_input};
    use crate::theme::{self, tokens, Type};

    struct Probe {
        input: Entity<InputState>,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let t = *tokens(cx);
            div().flex().pl(px(240.516)).pt(px(20.25)).child(
                text_input("filter", "filter", &self.input)
                    .w(px(255.3))
                    .px(px(10.))
                    .py(px(4.))
                    .background(t.ground)
                    .regular()
                    .size_sm(),
            )
        }
    }

    type Edges = (f32, f32, f32, f32);

    fn edges(b: Bounds<gpui_kit::ScaledPixels>) -> Edges {
        (
            b.origin.x.0,
            b.origin.y.0,
            b.origin.x.0 + b.size.width.0,
            b.origin.y.0 + b.size.height.0,
        )
    }

    fn painted(focused: bool) -> (Vec<Edges>, Vec<(Edges, f32, f32)>) {
        let mut cx = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            None,
            Arc::new(gpui_apple::AppleTextSystem::new()),
        );
        let (window, probe) = cx.update(|cx| {
            theme::install(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(800.), px(100.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        let t = *tokens(cx);
                        Probe {
                            input: input_state("", "", &t, window, cx),
                        }
                    })
                },
            )
            .unwrap()
        });
        cx.update_window(window, |_, window, cx| {
            if focused {
                let handle = probe.read(cx).input.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
            }
            window.render_frame(cx);
            let t = *tokens(cx);
            let quads = window.painted_quads();
            let frames = quads
                .iter()
                .filter(|q| q.background == Background::from(t.ground) && q.border_color == t.line)
                .map(|q| edges(q.bounds))
                .collect();
            let mut rings: Vec<_> = quads
                .iter()
                .filter(|q| q.border_color == t.accent && q.border_widths.top.0 > 0.)
                .map(|q| {
                    (
                        edges(q.bounds),
                        q.border_widths.top.0,
                        q.corner_radii.top_left.0,
                    )
                })
                .collect();
            rings.dedup();
            (frames, rings)
        })
        .unwrap()
    }

    #[gpui_kit::test]
    fn an_input_paints_its_frame_on_whole_css_pixels_like_chrome(_cx: &mut TestAppContext) {
        let (frames, rings) = painted(false);
        assert_eq!(
            frames,
            [(482., 40., 992., 100.)],
            "Chrome paints a box at x=240.516 y=20.25 w=255.3 over whole CSS pixels"
        );
        assert!(rings.is_empty(), "an unfocused input has no ring");
    }

    #[gpui_kit::test]
    fn a_focused_input_rings_its_rounded_corners_like_the_textarea(_cx: &mut TestAppContext) {
        let (_, rings) = painted(true);
        assert_eq!(
            rings,
            [((478., 36., 996., 104.), 4., 16.)],
            "the ring is 2px of accent outside the frame's whole CSS pixels, around rounded-md corners"
        );
    }
}

#[cfg(test)]
mod textarea_tests {
    use std::sync::Arc;

    use gpui_kit::base::input::TextareaState;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        div, px, size, AppContext, Bounds, Context, Entity, FontWeight, IntoElement, ParentElement,
        Point, Render, Styled, TestAppContext, TestDispatcher, Window, WindowBounds, WindowOptions,
    };

    use super::{text_area, textarea_state};
    use crate::theme::{self, css_line_box, gpui_baseline, tokens, Face, Type, SANS};

    const MEMORY_TEXT: Face = Face::new(SANS, FontWeight::NORMAL, 14., 22.75);
    const GPUI_LINE: f32 = 23.;
    const CHROME_LINE_EIGHT_DRIFT: f32 = 3.;

    struct Probe {
        state: Entity<TextareaState>,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(338.)).child(
                text_area("memory", "memory", &self.state)
                    .mt(px(31.5))
                    .h(px(200.))
                    .px(px(10.))
                    .py(px(8.))
                    .regular()
                    .text_size(px(14.))
                    .line_height(px(22.75)),
            )
        }
    }

    #[gpui_kit::test]
    fn textarea_lines_sit_on_the_web_baselines(_cx: &mut TestAppContext) {
        let mut cx = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            None,
            Arc::new(gpui_apple::AppleTextSystem::new()),
        );
        let (window, probe) = cx.update(|cx| {
            theme::install(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(600.), px(400.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        let t = *tokens(cx);
                        Probe {
                            state: textarea_state("first\nsecond", 8, &t, window, cx),
                        }
                    })
                },
            )
            .unwrap()
        });
        let (content_top, editor_top, web, gpui, scale) = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                window.render_frame(cx);
                let frame_top = f32::from(window.find("memory").bounds().origin.y);
                let editor = window.within("memory").find("input-state").bounds();
                (
                    frame_top + 1. + 8.,
                    f32::from(editor.origin.y),
                    css_line_box(cx, MEMORY_TEXT, &[MEMORY_TEXT]).baseline,
                    gpui_baseline(cx, &[MEMORY_TEXT], GPUI_LINE),
                    window.scale_factor(),
                )
            })
            .unwrap();
        let line_height = cx
            .update(|cx| probe.read(cx).state.read(cx).line_height())
            .map(f32::from);
        assert_eq!(
            line_height,
            Some(GPUI_LINE),
            "GPUI rounds a text style's line height to a whole pixel"
        );
        let line = |n: f32| {
            let painted = ((editor_top + gpui + (n - 1.) * GPUI_LINE) * scale).round();
            let chrome =
                ((content_top.round() + web + (n - 1.) * MEMORY_TEXT.line_height) * scale).round();
            (painted, chrome)
        };
        let (painted, chrome) = line(1.);
        assert_eq!(painted, chrome, "line 1: device {painted}, Chrome {chrome}");
        let (painted, chrome) = line(2.);
        assert_eq!(painted, chrome, "line 2: device {painted}, Chrome {chrome}");
        let (painted, chrome) = line(8.);
        assert_eq!(
            painted - chrome,
            CHROME_LINE_EIGHT_DRIFT,
            "line 8: device {painted}, Chrome {chrome}"
        );
    }

    fn frame_height(cx: &mut TestAppContext, window: gpui_kit::AnyWindowHandle) -> f32 {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            f32::from(window.find("memory").bounds().size.height)
        })
        .unwrap()
    }

    fn drag_grip(cx: &mut TestAppContext, window: gpui_kit::AnyWindowHandle, dy: f32) {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let frame = window.find("memory").bounds();
            let from = gpui_kit::point(frame.right() - px(4.), frame.bottom() - px(4.));
            window.drag(from, gpui_kit::point(from.x, from.y + px(dy)), cx);
        })
        .unwrap();
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn the_grip_resizes_the_textarea_vertically_down_to_its_rows(_cx: &mut TestAppContext) {
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
                        size: size(px(600.), px(600.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        let t = *tokens(cx);
                        Probe {
                            state: textarea_state("first", 8, &t, window, cx),
                        }
                    })
                },
            )
            .unwrap()
        });
        assert_eq!(frame_height(&mut cx, window), 200.);
        drag_grip(&mut cx, window, 60.);
        assert_eq!(frame_height(&mut cx, window), 260.);
        drag_grip(&mut cx, window, -20.);
        assert_eq!(frame_height(&mut cx, window), 240.);
        drag_grip(&mut cx, window, -150.);
        assert_eq!(
            frame_height(&mut cx, window),
            200.,
            "the textarea never shrinks below its rows"
        );
    }

    #[gpui_kit::test]
    fn a_focused_textarea_rings_its_rounded_corners(_cx: &mut TestAppContext) {
        let mut cx = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            None,
            Arc::new(gpui_apple::AppleTextSystem::new()),
        );
        let (window, probe) = cx.update(|cx| {
            theme::install(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(600.), px(400.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        let t = *tokens(cx);
                        Probe {
                            state: textarea_state("first", 8, &t, window, cx),
                        }
                    })
                },
            )
            .unwrap()
        });
        let ring = cx
            .update_window(window, |_, window, cx| {
                let handle = gpui_kit::Focusable::focus_handle(probe.read(cx).state.read(cx), cx);
                window.focus(&handle, cx);
                window.render_frame(cx);
                let accent = tokens(cx).accent;
                window
                    .painted_quads()
                    .into_iter()
                    .find(|q| q.border_color == accent && q.border_widths.top.0 > 0.)
                    .map(|q| (q.border_widths.top.0, q.corner_radii.top_left.0))
            })
            .unwrap();
        assert_eq!(
            ring,
            Some((4., 16.)),
            "the ring is 2px of accent around rounded-md corners, as Chrome draws the outline"
        );
    }
}

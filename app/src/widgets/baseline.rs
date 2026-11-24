use gpui_kit::*;

use crate::theme::{css_line_box, gpui_baseline, Face};

pub struct WebBaseline {
    child: AnyElement,
    faces: Option<(Face, Vec<Face>)>,
    line: Option<(Face, Vec<Face>)>,
    whole_pixel_line: bool,
}

pub const WORD_JOINER: char = '\u{2060}';

pub const BREAK_OPPORTUNITY: char = '\u{200B}';

pub fn with_chrome_breaks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        out.push(ch);
        if ch == '/' {
            out.push(WORD_JOINER);
        } else if ch == '-' && chars.peek().is_some_and(char::is_ascii_digit)
            || ch == ' ' && chars.peek() == Some(&'/')
        {
            out.push(BREAK_OPPORTUNITY);
        }
    }
    out
}

pub fn without_joiners(text: &str) -> String {
    text.chars()
        .filter(|ch| *ch != WORD_JOINER && *ch != BREAK_OPPORTUNITY)
        .collect()
}

pub fn text(text: impl Into<SharedString>) -> WebBaseline {
    let text: SharedString = text.into();
    if text.contains(['/', '-']) {
        web_baseline(SharedString::from(with_chrome_breaks(&text)))
    } else {
        web_baseline(text)
    }
}

pub fn web_baseline(child: impl IntoElement) -> WebBaseline {
    WebBaseline {
        child: child.into_any_element(),
        faces: None,
        line: None,
        whole_pixel_line: false,
    }
}

pub fn on_whole_pixel_line(child: impl IntoElement) -> WebBaseline {
    WebBaseline {
        whole_pixel_line: true,
        ..web_baseline(child)
    }
}

pub fn on_web_baseline(child: impl IntoElement, strut: Face, runs: &[Face]) -> WebBaseline {
    WebBaseline {
        child: child.into_any_element(),
        faces: Some((strut, runs.to_vec())),
        line: None,
        whole_pixel_line: false,
    }
}

pub fn on_shared_line(child: impl IntoElement, strut: Face, line: &[Face]) -> WebBaseline {
    WebBaseline {
        child: child.into_any_element(),
        faces: None,
        line: Some((strut, line.to_vec())),
        whole_pixel_line: false,
    }
}

pub fn face_of(style: &TextStyle, rem: Pixels) -> Option<Face> {
    Some(Face {
        family: known_family(&style.font_family)?,
        weight: style.font_weight,
        size: f32::from(style.font_size.to_pixels(rem)),
        line_height: f32::from(style.line_height.to_pixels(style.font_size, rem)),
    })
}

fn known_family(family: &SharedString) -> Option<&'static str> {
    match family.as_ref() {
        crate::theme::SANS => Some(crate::theme::SANS),
        crate::theme::MONO => Some(crate::theme::MONO),
        crate::theme::HELVETICA => Some(crate::theme::HELVETICA),
        unknown => unknown_family(unknown),
    }
}

#[cfg(test)]
fn unknown_family(family: &str) -> Option<&'static str> {
    panic!("no web baseline is known for the font family {family}")
}

#[cfg(not(test))]
fn unknown_family(_family: &str) -> Option<&'static str> {
    None
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Baselines {
    pub gpui: f32,
    pub web: f32,
}

pub fn baselines(cx: &App, strut: Face, runs: &[Face], line_height: f32) -> Baselines {
    Baselines {
        gpui: gpui_baseline(cx, runs, line_height),
        web: css_line_box(cx, strut, runs).baseline,
    }
}

pub fn web_offset(content_top: f32, baselines: Baselines) -> f32 {
    content_top.round() + baselines.web - (content_top + baselines.gpui)
}

impl IntoElement for WebBaseline {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for WebBaseline {
    type RequestLayoutState = Option<Baselines>;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Option<Baselines>) {
        let layout = self.child.request_layout(window, cx);
        let style = window.text_style();
        let rem = window.rem_size();
        let painted = if self.whole_pixel_line {
            f32::from(style.line_height_in_pixels(rem))
        } else {
            f32::from(window.pixel_snap(style.line_height.to_pixels(style.font_size, rem)))
        };
        let faces = self
            .faces
            .clone()
            .or_else(|| face_of(&style, rem).map(|own| (own, vec![own])));
        let baselines = faces.map(|(strut, runs)| match &self.line {
            Some((line_strut, line)) => Baselines {
                gpui: gpui_baseline(cx, &runs, painted),
                web: css_line_box(cx, *line_strut, line).baseline,
            },
            None => baselines(cx, strut, &runs, painted),
        });
        (layout, baselines)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        baselines: &mut Option<Baselines>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let content_top = bounds.origin.y - window.pixel_snap_point(window.element_offset()).y;
        let offset = baselines.map_or(px(0.), |b| {
            window.pixel_snap(px(web_offset(f32::from(content_top), b)))
        });
        window.with_element_offset(point(px(0.), offset), |window| {
            self.child.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _baselines: &mut Option<Baselines>,
        _prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

pub fn shared_line_height(cx: &App, strut: Face, line: &[Face]) -> Pixels {
    px(css_line_box(cx, strut, line).height)
}

pub fn inline_line(cx: &App, strut: Face, runs: &[Face], child: impl IntoElement) -> Div {
    div()
        .line_height(px(css_line_box(cx, strut, runs).height))
        .child(on_web_baseline(child, strut, runs))
}

#[cfg(test)]
mod tests {
    use super::{web_baseline, web_offset, with_chrome_breaks, without_joiners, Baselines};
    use crate::theme::{self, Type};
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        div, point, px, size, AppContext, Bounds, Context, InteractiveElement, IntoElement,
        ParentElement, Pixels, Point, Render, ScrollHandle, StatefulInteractiveElement, Styled,
        TestAppContext, TestDispatcher, TestSupportExt, Window, WindowBounds, WindowOptions,
    };
    use std::sync::Arc;

    struct Scroller {
        handle: ScrollHandle,
    }

    impl Render for Scroller {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("scroller")
                .h(px(200.))
                .overflow_y_scroll()
                .track_scroll(&self.handle)
                .child(
                    div()
                        .id("box")
                        .test_support()
                        .mt(px(31.25))
                        .p(px(16.))
                        .border_1()
                        .type_px(15.)
                        .child(web_baseline(
                            div().id("glyphs").test_support().child("Index"),
                        )),
                )
                .child(div().h(px(2000.)))
        }
    }

    #[gpui_kit::test]
    fn scrolling_never_moves_text_relative_to_its_box(_cx: &mut TestAppContext) {
        let mut cx = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            None,
            Arc::new(gpui_apple::AppleTextSystem::new()),
        );
        let handle = ScrollHandle::new();
        let scroll = handle.clone();
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
                move |_, cx| cx.new(|_| Scroller { handle: scroll }),
            )
            .unwrap()
        });
        let gap = |cx: &mut TestAppContext, by: f32| -> Pixels {
            handle.set_offset(point(px(0.), px(-by)));
            cx.update_window(window, |_, window, cx| {
                window.render_frame(cx);
                window.find("glyphs").bounds().origin.y - window.find("box").bounds().origin.y
            })
            .unwrap()
        };
        let resting = gap(&mut cx, 0.);
        for by in [0.25, 0.5, 0.75, 1.3, 2.6] {
            let moved = gap(&mut cx, by);
            assert!(
                (moved - resting).abs() < px(0.01),
                "scrolled {by}px: text sits {moved:?} below its box, {resting:?} at rest"
            );
        }
    }

    struct Blank;

    impl Render for Blank {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn run(text: &str, family: &'static str) -> gpui_kit::TextRun {
        gpui_kit::TextRun {
            len: text.len(),
            font: gpui_kit::font(family),
            color: gpui_kit::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    fn with_window<R>(cx: &mut TestAppContext, f: impl FnOnce(&mut Window) -> R) -> R {
        let (window, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Blank)).unwrap()
        });
        cx.update_window(window, |_, window, _| f(window)).unwrap()
    }

    fn break_points(cx: &mut TestAppContext, text: &str, width: f32) -> Vec<usize> {
        let text = text.to_string();
        with_window(cx, move |window| {
            let lines = window
                .text_system()
                .shape_text(
                    text.clone().into(),
                    px(12.),
                    &[run(&text, theme::MONO)],
                    Some(px(width)),
                    None,
                )
                .unwrap();
            lines[0]
                .wrap_boundaries
                .iter()
                .map(|b| lines[0].unwrapped_layout.runs[b.run_ix].glyphs[b.glyph_ix].index)
                .collect()
        })
    }

    fn apple() -> TestAppContext {
        let cx = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            None,
            Arc::new(gpui_apple::AppleTextSystem::new()),
        );
        cx.update(theme::install);
        cx
    }

    #[gpui_kit::test]
    fn a_long_path_wraps_only_where_chrome_would(_cx: &mut TestAppContext) {
        let mut cx = apple();
        let path = "rhebok on PATH is /Users/me/.local/bin/rhebok, not /var/folders/y4/kmq/T/rhebok-e2e-0l6Lea/home/Library/rhebok/bin/rhebok";
        let joined = with_chrome_breaks(path);
        let breaks = break_points(&mut cx, &joined, 300.);
        assert!(!breaks.is_empty(), "nothing wrapped");
        for at in &breaks {
            let before = without_joiners(&joined[..*at]).chars().last().unwrap();
            assert!(
                before == ' ' || before == '-',
                "broke after {before:?} at {at} in {joined:?}"
            );
        }
        assert!(
            breaks
                .iter()
                .any(|at| without_joiners(&joined[..*at]).ends_with("e2e-")),
            "never broke between a hyphen and a digit, which Chrome does: {breaks:?}"
        );
        let unjoined = break_points(&mut cx, path, 300.);
        assert!(
            unjoined.iter().any(|at| path[..*at].ends_with('/')),
            "the fixture never breaks after a slash without joiners, so it proves nothing"
        );
        let spaced = with_chrome_breaks("rhebok, not /var/folders/y4/kmqkwsz575");
        let before_slash = spaced.find('/').unwrap();
        assert_eq!(
            break_points(&mut cx, &spaced, 120.).first(),
            Some(&before_slash),
            "did not break between a space and a slash, which Chrome does"
        );
        let solid = with_chrome_breaks("/var/folders/y4/kmqkwsz575zfpdd44snr727m0000gn/T");
        assert!(
            !break_points(&mut cx, &solid, 120.).is_empty(),
            "a path with no break opportunity overflowed instead of breaking mid-word"
        );
    }

    #[gpui_kit::test]
    fn the_word_joiner_has_no_width_and_never_reaches_a_label(_cx: &mut TestAppContext) {
        let mut cx = apple();
        let path = "/Users/me/.local/bin/rhebok-e2e-0l6Lea";
        let joined = with_chrome_breaks(path);
        assert_ne!(joined, path);
        assert_eq!(without_joiners(&joined), path);
        let (a, b) = (path.to_string(), joined.clone());
        let (plain, with) = with_window(&mut cx, move |window| {
            let width = |text: &str| {
                window
                    .text_system()
                    .layout_line(text, px(14.), &[run(text, theme::SANS)], None)
                    .width
            };
            (width(&a), width(&b))
        });
        assert!((plain - with).abs() < px(0.01), "{plain:?} vs {with:?}");
        let rich = crate::widgets::Rich::new().push(
            path,
            theme::SANS,
            gpui_kit::FontWeight::NORMAL,
            gpui_kit::black(),
        );
        assert_eq!(rich.text().as_ref(), path);
    }

    #[test]
    fn the_offset_lands_on_chromes_snapped_baseline() {
        let title = Baselines {
            gpui: 15.93,
            web: 15.,
        };
        let offset = web_offset(15.75, title);
        assert!(
            (offset - (16. + 15. - (15.75 + 15.93))).abs() < 1e-4,
            "{offset}"
        );
        assert!((15.75 + 15.93 + offset - 31.).abs() < 1e-4);
        let fix = Baselines {
            gpui: 15.744,
            web: 17.,
        };
        assert!((233.5 + 15.744 + web_offset(233.5, fix) - 251.).abs() < 1e-4);
        assert!((502.25 + 15.93 + web_offset(502.25, title) - 517.).abs() < 1e-4);
    }
}

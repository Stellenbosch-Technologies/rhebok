use gpui_kit::*;

use crate::theme::Face;
use crate::widgets::css_box::ExactSpan;
use crate::widgets::wrap::break_pieces;
use crate::widgets::{advances, baseline};

#[derive(IntoElement)]
pub struct ExactText {
    text: SharedString,
    style: StyleRefinement,
    line: Option<(Face, Vec<Face>)>,
}

pub fn exact_text(text: impl Into<SharedString>) -> ExactText {
    ExactText {
        text: text.into(),
        style: StyleRefinement::default(),
        line: None,
    }
}

pub fn unbreakable(text: &str) -> impl Iterator<Item = &str> {
    break_pieces(text)
        .into_iter()
        .map(move |piece| text[piece.range].trim_end_matches(' '))
}

pub fn min_content_width(
    window: &Window,
    cx: &App,
    text: &str,
    family: &'static str,
    weight: FontWeight,
    size: f32,
) -> Pixels {
    let mut style = window.text_style();
    style.font_family = family.into();
    style.font_weight = weight;
    style.font_size = px(size).into();
    let widest = unbreakable(text)
        .map(|segment| {
            let wrapped = advances(cx, family, weight, px(size), segment)
                .into_iter()
                .fold(px(0.), |a, b| a + b);
            unrounded_width(window, segment, &style).max(wrapped)
        })
        .fold(px(0.), |a, b| a.max(b));
    let scale = window.scale_factor();
    px((f32::from(widest) * scale).ceil() / scale)
}

pub fn word_pieces(text: &str) -> Vec<String> {
    break_pieces(text)
        .into_iter()
        .map(|piece| {
            let word = &text[piece.range];
            if piece.forced {
                format!("{word} ")
            } else {
                word.to_string()
            }
        })
        .filter(|word| !word.is_empty())
        .collect()
}

pub fn words(text: &str) -> Vec<ExactText> {
    word_pieces(text).into_iter().map(exact_text).collect()
}

impl ExactText {
    pub fn on_line(mut self, strut: Face, line: &[Face]) -> Self {
        self.line = Some((strut, line.to_vec()));
        self
    }
}

impl Styled for ExactText {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

pub const LAYOUT_UNIT: f32 = 64.;

#[derive(Clone, Default)]
pub struct ExactLink(std::rc::Rc<std::cell::Cell<Option<Pixels>>>);

impl ExactLink {
    pub fn new() -> ExactLink {
        ExactLink::default()
    }

    pub fn end(&self) -> Option<Pixels> {
        self.0.get()
    }

    fn left_edge(&self) -> impl IntoElement {
        let link = self.0.clone();
        link.set(None);
        canvas(
            move |bounds, _, _| link.set(Some(bounds.left())),
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_0()
    }

    fn right_edge_less(&self, width: Pixels) -> impl IntoElement {
        let link = self.0.clone();
        link.set(None);
        canvas(
            move |bounds, _, _| link.set(Some(bounds.left() - width)),
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .right_0()
        .size_0()
    }

    pub fn ends_at(&self, width: Pixels) -> impl IntoElement {
        let link = self.0.clone();
        link.set(None);
        canvas(
            move |bounds, _, _| link.set(Some(bounds.left() + width)),
            |_, _, _, _| {},
        )
        .absolute()
        .size_0()
    }
}

#[derive(Clone)]
pub struct Tracks {
    left: ExactLink,
    right: ExactLink,
    lead: f32,
    gap: f32,
    count: usize,
}

impl Tracks {
    pub fn new(lead: f32, gap: f32, count: usize) -> Tracks {
        Tracks {
            left: ExactLink::new(),
            right: ExactLink::new(),
            lead,
            gap,
            count,
        }
    }

    pub fn edges(&self) -> [AnyElement; 2] {
        [
            self.left.left_edge().into_any_element(),
            self.right.right_edge_less(px(0.)).into_any_element(),
        ]
    }

    pub fn span(&self, ix: usize, width: Option<Pixels>) -> ExactSpan {
        let tracks = self.clone();
        std::rc::Rc::new(move || {
            let (left, right) = (tracks.left.end()?, tracks.right.end()?);
            let count = tracks.count as f32;
            let gaps = tracks.gap * (count - 1.);
            let track = (right - left - px(tracks.lead + gaps)) / count;
            let start = left + px(tracks.lead) + (track + px(tracks.gap)) * ix as f32;
            Some((start, width.unwrap_or(track)))
        })
    }
}

pub fn row_ending_before(trailing: Pixels) -> (Div, ExactLink) {
    let end = ExactLink::new();
    let row = div().relative().child(end.right_edge_less(trailing));
    (row, end)
}

pub fn exact_left(link: &ExactLink, width: Pixels, laid_out_left: Pixels) -> Option<Pixels> {
    link.end()
        .map(|end| end - width)
        .filter(|left| (*left - laid_out_left).abs() < px(1.))
}

pub struct EndingAt {
    link: ExactLink,
    width: Pixels,
    child: AnyElement,
}

pub fn ending_at(link: &ExactLink, width: Pixels, child: impl IntoElement) -> EndingAt {
    EndingAt {
        link: link.clone(),
        width,
        child: child.into_any_element(),
    }
}

impl IntoElement for EndingAt {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for EndingAt {
    type RequestLayoutState = ();
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
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let offset = exact_left(&self.link, self.width, bounds.left())
            .map_or(px(0.), |left| left - bounds.left());
        window.with_element_offset(point(offset, px(0.)), |window| {
            self.child.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

pub fn text_style_of(
    window: &Window,
    family: &'static str,
    weight: FontWeight,
    size: f32,
) -> TextStyle {
    let mut face = window.text_style();
    face.font_family = family.into();
    face.font_weight = weight;
    face.font_size = px(size).into();
    face
}

pub fn layout_unit(width: Pixels) -> Pixels {
    px((f32::from(width) * LAYOUT_UNIT - 1e-3).ceil().max(0.) / LAYOUT_UNIT)
}

pub fn unrounded_width(window: &Window, text: &str, style: &TextStyle) -> Pixels {
    let font_size = style.font_size.to_pixels(window.rem_size());
    layout_unit(
        window
            .text_system()
            .layout_line(text, font_size, &[style.to_run(text.len())], None)
            .width,
    )
}

impl RenderOnce for ExactText {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut text_style = window.text_style();
        text_style.refine(&self.style.text);
        let width = unrounded_width(window, &self.text, &text_style);
        let mut element = div().flex_none().whitespace_nowrap();
        element.style().refine(&self.style);
        match self.line {
            Some((strut, line)) => element
                .w(width)
                .line_height(baseline::shared_line_height(cx, strut, &line))
                .child(baseline::on_shared_line(self.text, strut, &line)),
            None => element.w(width).child(baseline::text(self.text)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        div, font, px, size, AppContext, Bounds, Context, Font, FontWeight, IntoElement,
        ParentElement, Point, Render, SharedString, Styled, TestAppContext, TestDispatcher,
        TextRun, Window, WindowBounds, WindowOptions,
    };
    use gpui_kit::{InteractiveElement, TestSupportExt};

    use super::{exact_text, unbreakable, word_pieces};
    use crate::theme::{self, SANS};

    #[test]
    fn words_break_where_the_shared_wrap_rules_break() {
        assert_eq!(word_pieces("a--b c"), ["a--", "b ", "c"]);
        assert_eq!(
            word_pieces("well-known *.tmp"),
            ["well-", "known ", "*.tmp"]
        );
        assert_eq!(word_pieces("a\nb"), ["a ", "b"]);
    }

    #[test]
    fn min_content_segments_follow_the_shared_wrap_rules() {
        let segments: Vec<&str> = unbreakable("x--y z\nwell-known").collect();
        assert_eq!(segments, ["x--", "y", "z", "well-", "known"]);
    }

    const TEXT: &str = "chunks, about 70 per document";

    struct Probe;

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().flex().child(
                div()
                    .id("probe")
                    .test_support()
                    .flex_none()
                    .font_family(SANS)
                    .text_size(px(14.))
                    .child(exact_text(TEXT)),
            )
        }
    }

    #[gpui_kit::test]
    fn exact_text_is_as_wide_as_the_unrounded_line(_cx: &mut TestAppContext) {
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
                        size: size(px(800.), px(100.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| Probe),
            )
            .unwrap()
        });
        let (width, line, snapped) = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                let run = TextRun {
                    len: TEXT.len(),
                    font: Font {
                        weight: FontWeight::NORMAL,
                        ..font(SharedString::from(SANS))
                    },
                    color: gpui_kit::black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let line = window
                    .text_system()
                    .layout_line(TEXT, px(14.), &[run], None)
                    .width;
                (
                    window.find("probe").bounds().size.width,
                    line,
                    window.pixel_snap(line),
                )
            })
            .unwrap();
        let line = f32::from(line);
        assert!(
            line.fract() > 0.01 && line.fract() < 0.99,
            "{line} has no fraction"
        );
        assert!(
            snapped < px(line.ceil()),
            "{line} snaps to its ceiling, which cannot tell exact from rounded up"
        );
        assert!(
            (width - snapped).abs() < px(0.01),
            "laid out {width:?}, line {line} snapped to {snapped:?}"
        );
    }
}

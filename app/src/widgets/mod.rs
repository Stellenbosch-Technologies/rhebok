pub mod agent_card;
pub mod ago;
pub mod banner;
pub mod baseline;
pub mod button;
pub mod change_report;
pub mod checkbox;
pub mod chevron;
pub mod copy_command;
pub mod css_box;
pub mod divider;
pub mod document_facts;
pub mod dotted_link;
pub mod exact;
pub mod inline_flow;
pub mod js;
pub mod panel;
pub mod rich;
pub mod segmented;
pub mod select;
pub mod tail;
pub mod text_button;
pub mod text_input;
pub mod wrap;

pub use agent_card::AgentCard;
pub use ago::ago;
pub use baseline::{inline_line, on_web_baseline, text};
pub use change_report::{change_report, AgentAction, Outcome};
pub use checkbox::checkbox;
pub use chevron::{chevron, disclosure};
pub use copy_command::copy_command;
pub use divider::divider;
pub use document_facts::{document_facts, DocumentFacts};
pub use dotted_link::dotted_link;
pub use exact::{exact_text, min_content_width, words};
pub use panel::panel;
pub use rich::Rich;
pub use segmented::segmented;
pub use select::{select, SelectSize};
pub use text_button::text_button;
pub use text_input::{text_area, text_input};

use gpui_kit::*;

use crate::theme::Face;
use gpui_kit::prelude::FluentBuilder;

use crate::widgets::css_box::{css_box_after, css_box_exact};
pub use crate::widgets::exact::ExactLink;

#[derive(IntoElement)]
pub struct Tracked {
    text: SharedString,
    family: SharedString,
    weight: FontWeight,
    size: Pixels,
    tracking: Pixels,
    style: StyleRefinement,
    line: Option<(Face, Vec<Face>)>,
    frame: Option<Frame>,
    ends: Option<ExactLink>,
}

#[derive(Clone)]
struct Frame {
    background: Hsla,
    radius: f32,
    pad_x: f32,
    pad_y: f32,
    after: Option<(ExactLink, Pixels)>,
}

pub fn tracked(
    text: impl Into<SharedString>,
    family: impl Into<SharedString>,
    weight: FontWeight,
    size: Pixels,
    tracking_em: f32,
) -> Tracked {
    Tracked {
        text: text.into(),
        family: family.into(),
        weight,
        size,
        tracking: size * tracking_em,
        style: StyleRefinement::default(),
        line: None,
        frame: None,
        ends: None,
    }
}

fn shape_tracked(
    window: &Window,
    text: &SharedString,
    family: &SharedString,
    weight: FontWeight,
    size: Pixels,
) -> ShapedLine {
    window.text_system().shape_line(
        text.clone(),
        size,
        &[TextRun {
            len: text.len(),
            font: Font {
                weight,
                ..font(family.clone())
            },
            color: black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    )
}

pub fn advances(
    cx: &App,
    family: &str,
    weight: FontWeight,
    size: Pixels,
    text: &str,
) -> Vec<Pixels> {
    let text_system = cx.text_system();
    let id = text_system.resolve_font(&Font {
        weight,
        ..font(SharedString::from(family.to_string()))
    });
    text.chars()
        .map(|ch| {
            text_system
                .advance(id, size, ch)
                .map(|a| a.width)
                .unwrap_or_default()
        })
        .collect()
}

impl Styled for Tracked {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Tracked {
    pub fn framed(mut self, background: Hsla, radius: f32, pad_x: f32, pad_y: f32) -> Self {
        self.frame = Some(Frame {
            background,
            radius,
            pad_x,
            pad_y,
            after: None,
        });
        self
    }

    pub fn ends(mut self, link: &ExactLink) -> Self {
        self.ends = Some(link.clone());
        self
    }

    pub fn after(mut self, link: &ExactLink, gap: Pixels) -> Self {
        if let Some(frame) = &mut self.frame {
            frame.after = Some((link.clone(), gap));
        }
        self
    }

    pub fn on_line(mut self, strut: Face, line: &[Face]) -> Self {
        self.line = Some((strut, line.to_vec()));
        self
    }

    fn glyph_height(&self) -> Pixels {
        match self.style.text.line_height {
            Some(DefiniteLength::Absolute(AbsoluteLength::Pixels(height))) => height,
            _ => self.size * 1.5,
        }
    }
}

impl RenderOnce for Tracked {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let shaped = shape_tracked(window, &self.text, &self.family, self.weight, self.size);
        let mut lefts = Vec::new();
        for (ix, (at, _)) in self.text.char_indices().enumerate() {
            lefts.push(shaped.x_for_index(at) + self.tracking * ix as f32);
        }
        let x = exact::layout_unit(shaped.width + self.tracking * lefts.len() as f32);
        let line = self.line.clone();
        let height = match &line {
            Some((strut, faces)) => baseline::shared_line_height(cx, *strut, faces),
            None => self.glyph_height(),
        };
        let mut row = div()
            .relative()
            .flex_none()
            .w(x)
            .h(height)
            .font_family(self.family)
            .font_weight(self.weight)
            .text_size(self.size);
        row.style().refine(&self.style);
        if line.is_some() {
            row = row.h(height).line_height(height);
        }
        let row = row
            .when_some(self.ends.clone(), |d, link| d.child(link.ends_at(x)))
            .children(self.text.chars().zip(lefts).map(|(ch, left)| {
                let glyph = SharedString::from(ch.to_string());
                div()
                    .absolute()
                    .top_0()
                    .left(left)
                    .whitespace_nowrap()
                    .child(match &line {
                        Some((strut, faces)) => baseline::on_shared_line(glyph, *strut, faces),
                        None => baseline::text(glyph),
                    })
            }));
        match self.frame {
            None => row.into_any_element(),
            Some(frame) => {
                let width = x + px(2. * frame.pad_x);
                div()
                    .relative()
                    .flex_none()
                    .px(px(frame.pad_x))
                    .py(px(frame.pad_y))
                    .map(|d| match frame.after.clone() {
                        Some((link, gap)) => d.child(css_box_after(
                            frame.background,
                            frame.radius,
                            width,
                            link,
                            gap,
                        )),
                        None => d.child(css_box_exact(
                            frame.background,
                            None,
                            frame.radius,
                            Some(width),
                        )),
                    })
                    .child(row)
                    .into_any_element()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        div, px, size, AppContext, Bounds, Context, FontWeight, IntoElement, ParentElement, Pixels,
        Point, Render, Styled, TestAppContext, Window, WindowBounds, WindowOptions,
    };
    use gpui_kit::{InteractiveElement, TestSupportExt};

    use super::{advances, tracked};
    use crate::theme::{self, MONO, SANS};

    const COUNTS: &str = "3649 docs · 254,816 chunks";

    struct Probe;

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .child(div().id("probe").test_support().flex_none().child(tracked(
                    COUNTS,
                    MONO,
                    FontWeight::NORMAL,
                    px(12.),
                    0.06,
                )))
        }
    }

    #[gpui_kit::test]
    fn tracked_text_is_as_wide_as_its_advances_plus_the_tracking(_cx: &mut TestAppContext) {
        let mut cx = TestAppContext::build_with_text_system(
            gpui_kit::TestDispatcher::new(0),
            None,
            Arc::new(gpui_apple::AppleTextSystem::new()),
        );
        let (window, expected) = cx.update(|cx| {
            theme::install(cx);
            let sum = advances(cx, MONO, FontWeight::NORMAL, px(12.), COUNTS)
                .into_iter()
                .fold(px(0.), |a, b| a + b);
            let expected = sum + px(12. * 0.06) * COUNTS.chars().count() as f32;
            let (window, _) = gpui_kit::open_window(
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
            .unwrap();
            (window, expected)
        });
        let width: Pixels = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                window.find("probe").bounds().size.width
            })
            .unwrap();
        assert!(expected > px(150.), "advances look unshaped: {expected:?}");
        assert!(
            (width - expected).abs() < px(0.5),
            "tracked width {width:?}, expected {expected:?}"
        );
    }

    struct Chip;

    impl Render for Chip {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .child(div().id("chip").test_support().flex_none().child(tracked(
                    "FACT",
                    SANS,
                    FontWeight::SEMIBOLD,
                    px(10.),
                    0.05,
                )))
        }
    }

    #[gpui_kit::test]
    fn tracked_text_keeps_kerning_like_chrome(_cx: &mut TestAppContext) {
        let mut cx = TestAppContext::build_with_text_system(
            gpui_kit::TestDispatcher::new(0),
            None,
            Arc::new(gpui_apple::AppleTextSystem::new()),
        );
        let (window, _) = cx.update(|cx| {
            theme::install(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(400.), px(100.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| Chip),
            )
            .unwrap()
        });
        let width: Pixels = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                window.find("chip").bounds().size.width
            })
            .unwrap();
        let chrome = px(23.28);
        assert!(
            (width - chrome).abs() <= px(0.25),
            "the FACT chip's text is {width:?} wide, Chrome lays it out at {chrome:?}"
        );
    }

    struct PillThenSibling;

    impl Render for PillThenSibling {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .child(tracked(
                    "CONNECTED",
                    SANS,
                    FontWeight::SEMIBOLD,
                    px(11.),
                    0.05,
                ))
                .child(div().id("after").test_support().w(px(10.)).h(px(10.)))
        }
    }

    #[gpui_kit::test]
    fn tracked_text_lays_out_at_its_exact_width(_cx: &mut TestAppContext) {
        let mut cx = TestAppContext::build_with_text_system(
            gpui_kit::TestDispatcher::new(0),
            None,
            Arc::new(gpui_apple::AppleTextSystem::new()),
        );
        let (window, _) = cx.update(|cx| {
            theme::install(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(400.), px(100.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| PillThenSibling),
            )
            .unwrap()
        });
        let after = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                window.find("after").bounds().origin.x
            })
            .unwrap();
        let chrome = px(63.688);
        assert!(
            (after - chrome).abs() <= px(0.25),
            "the next element starts at {after:?}; Chrome puts it at {chrome:?}, so the tracked label is not laid out at its exact width"
        );
    }
}

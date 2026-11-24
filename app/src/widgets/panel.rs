use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::widgets::baseline;
use crate::widgets::css_box::css_box;

use crate::theme::{tokens, Type};

#[derive(IntoElement)]
pub struct Panel {
    title: SharedString,
    aside: Option<AnyElement>,
    testid: SharedString,
    style: StyleRefinement,
    children: Vec<AnyElement>,
}

pub fn panel(
    title: impl Into<SharedString>,
    aside: Option<AnyElement>,
    testid: impl Into<SharedString>,
) -> Panel {
    Panel {
        title: title.into(),
        aside,
        testid: testid.into(),
        style: StyleRefinement::default(),
        children: Vec::new(),
    }
}

impl Styled for Panel {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Panel {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Panel {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = tokens(cx);
        let mut section = div()
            .id(ElementId::Name(self.testid.clone()))
            .flex()
            .flex_col()
            .gap(px(12.))
            .min_w(px(0.))
            .relative()
            .p(px(16.))
            .border_1()
            .border_color(transparent_black())
            .rounded(px(8.));
        section.style().refine(&self.style);
        section
            .test_support()
            .aria_label(self.title.clone())
            .child(css_box(t.surface, Some(t.line), 8.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(12.))
                    .min_h(px(28.))
                    .child(
                        div()
                            .semibold()
                            .type_px(15.)
                            .child(baseline::text(self.title)),
                    )
                    .when_some(self.aside, |d, aside| d.child(aside)),
            )
            .children(self.children)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        div, px, size, AppContext, Bounds, Context, IntoElement, ParentElement, Point, Render,
        Styled, TestAppContext, TestDispatcher, Window, WindowBounds, WindowOptions,
    };

    use super::panel;
    use crate::theme::{self, tokens};

    struct Card;

    impl Render for Card {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .pt(px(179.75))
                .child(panel("Index", None, "index-panel").w(px(300.)).h(px(100.)))
        }
    }

    #[gpui_kit::test]
    fn a_panel_paints_its_frame_on_the_whole_css_pixels_chrome_snaps_it_to(
        _cx: &mut TestAppContext,
    ) {
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
                        size: size(px(400.), px(400.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| Card),
            )
            .unwrap()
        });
        let frames = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                let line = tokens(cx).line;
                window
                    .painted_quads()
                    .into_iter()
                    .filter(|q| q.border_color == line && q.border_widths.top.0 > 0.)
                    .map(|q| (q.bounds.origin.y.0, q.bounds.size.height.0))
                    .collect::<Vec<_>>()
            })
            .unwrap();
        assert_eq!(
            frames,
            [(360., 200.)],
            "Chrome paints a card laid out at y=179.75 over CSS pixels 180..280"
        );
    }
}

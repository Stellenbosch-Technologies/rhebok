use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::theme::{tokens, underline_thickness, underline_top, Face};
use crate::widgets::exact_text;

type OnClick = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

const UNDERLINE_OFFSET: f32 = 2.;
const DOT_WIDTH: f32 = 1.;

#[derive(IntoElement)]
pub struct DottedLink {
    id: ElementId,
    label: SharedString,
    face: Face,
    on_click: Option<OnClick>,
}

pub fn dotted_link(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    face: Face,
) -> DottedLink {
    DottedLink {
        id: id.into(),
        label: label.into(),
        face,
        on_click: None,
    }
}

impl DottedLink {
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

fn dots(colour: Hsla, thickness: f32) -> Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let mut x = bounds.origin.x;
            while x + px(DOT_WIDTH) <= bounds.right() {
                window.paint_quad(fill(
                    Bounds {
                        origin: point(x, bounds.origin.y),
                        size: size(px(DOT_WIDTH), px(thickness)),
                    },
                    colour,
                ));
                x += px(2. * DOT_WIDTH);
            }
        },
    )
}

impl RenderOnce for DottedLink {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let accent = tokens(cx).accent;
        let underline = underline_top(cx, self.face, UNDERLINE_OFFSET);
        let thickness = underline_thickness(self.face);
        div()
            .id(self.id)
            .test_support()
            .role(Role::Button)
            .aria_label(self.label.clone())
            .relative()
            .flex_none()
            .cursor_pointer()
            .font_family(self.face.family)
            .font_weight(self.face.weight)
            .text_size(px(self.face.size))
            .line_height(px(self.face.line_height))
            .text_color(accent)
            .when_some(self.on_click, |d, handler| {
                d.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .child(exact_text(self.label))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(underline))
                    .h(px(thickness))
                    .child(dots(accent, thickness).size_full()),
            )
    }
}

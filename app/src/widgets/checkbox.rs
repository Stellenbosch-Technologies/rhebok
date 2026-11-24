use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::widgets::css_box::css_px_box;

pub const BOX: f32 = 13.;
pub const CHROME_BLUE: u32 = 0x0075FF;
pub const CHROME_BORDER: u32 = 0x767676;
pub const CHROME_DISABLED_FILL: u32 = 0x767676;

type OnToggle = Rc<dyn Fn(bool, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    label: SharedString,
    checked: bool,
    disabled: bool,
    accent: Option<Hsla>,
    on_toggle: Option<OnToggle>,
}

pub fn checkbox(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
) -> Checkbox {
    Checkbox {
        id: id.into(),
        label: label.into(),
        checked,
        disabled: false,
        accent: None,
        on_toggle: None,
    }
}

impl Checkbox {
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn accent(mut self, colour: Hsla) -> Self {
        self.accent = Some(colour);
        self
    }

    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }
}

fn paint_box(checked: bool, disabled: bool, accent: Option<Hsla>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let bounds = css_px_box(bounds);
            let colour: Hsla = if disabled {
                rgb(CHROME_DISABLED_FILL).into()
            } else {
                accent.unwrap_or_else(|| rgb(CHROME_BLUE).into())
            };
            let colour = if disabled && checked {
                colour.opacity(0.45)
            } else {
                colour
            };
            if checked {
                window.paint_quad(quad(
                    bounds,
                    px(2.),
                    colour,
                    px(0.),
                    transparent_black(),
                    BorderStyle::Solid,
                ));
                let at = |x: f32, y: f32| point(bounds.origin.x + px(x), bounds.origin.y + px(y));
                let mut path = PathBuilder::stroke(px(2.));
                path.move_to(at(3.3, 6.6));
                path.line_to(at(5.6, 8.9));
                path.line_to(at(10., 3.9));
                if let Ok(path) = path.build() {
                    window.paint_path(path, white());
                }
            } else {
                let border: Hsla = rgb(CHROME_BORDER).into();
                window.paint_quad(quad(
                    bounds,
                    px(2.),
                    white(),
                    px(1.),
                    if disabled {
                        border.opacity(0.5)
                    } else {
                        border
                    },
                    BorderStyle::Solid,
                ));
            }
        },
    )
    .size(px(BOX))
}

impl RenderOnce for Checkbox {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let checked = self.checked;
        div()
            .id(self.id)
            .test_support()
            .role(Role::CheckBox)
            .aria_label(self.label)
            .aria_toggled(if checked {
                Toggled::True
            } else {
                Toggled::False
            })
            .size(px(BOX))
            .when_some(self.on_toggle.filter(|_| !self.disabled), |d, handler| {
                d.on_click(move |_, window, cx| handler(!checked, window, cx))
            })
            .child(paint_box(checked, self.disabled, self.accent))
    }
}

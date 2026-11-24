use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::theme::{gpui_baseline, Face};
use crate::widgets::{baseline, exact_text};

pub const UNDERLINE_THICKNESS_EM: f32 = 0.05;

type OnClick = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct TextButton {
    id: ElementId,
    label: SharedString,
    face: Face,
    colour: Hsla,
    underline: bool,
    hover_colour: Option<Hsla>,
    disabled: bool,
    disabled_opacity: f32,
    style: StyleRefinement,
    on_click: Option<OnClick>,
}

pub fn text_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    face: Face,
    colour: Hsla,
) -> TextButton {
    TextButton {
        id: id.into(),
        label: label.into(),
        face,
        colour,
        underline: false,
        hover_colour: None,
        disabled: false,
        disabled_opacity: 0.5,
        style: StyleRefinement::default(),
        on_click: None,
    }
}

impl TextButton {
    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    pub fn hover_colour(mut self, colour: Hsla) -> Self {
        self.hover_colour = Some(colour);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn disabled_opacity(mut self, opacity: f32) -> Self {
        self.disabled_opacity = opacity;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl Styled for TextButton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TextButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let face = self.face;
        let thickness = (face.size * UNDERLINE_THICKNESS_EM).max(1.);
        let top = underline_below_glyph(cx, face, thickness);
        let colour = self.colour;
        let mut button = div()
            .id(self.id)
            .relative()
            .font_family(face.family)
            .font_weight(face.weight)
            .text_size(px(face.size))
            .line_height(px(face.line_height))
            .text_color(colour);
        button.style().refine(&self.style);
        let disabled = self.disabled;
        button
            .test_support()
            .role(Role::Button)
            .aria_label(self.label.clone())
            .when(disabled, |d| {
                d.opacity(self.disabled_opacity).cursor_not_allowed()
            })
            .when(!disabled, |d| d.cursor_pointer())
            .when_some(self.hover_colour.filter(|_| !disabled), |d, hover| {
                d.hover(move |s| s.text_color(hover))
            })
            .when_some(self.on_click.filter(|_| !disabled), |d, handler| {
                d.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .child(
                div()
                    .relative()
                    .child(exact_text(self.label))
                    .when(self.underline, |d| {
                        d.child(
                            div().absolute().inset_0().child(baseline::web_baseline(
                                div().relative().h(px(face.line_height)).child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .top(px(top))
                                        .h(px(thickness))
                                        .bg(colour),
                                ),
                            )),
                        )
                    }),
            )
    }
}

pub fn underline_gap(thickness: f32) -> f32 {
    (thickness / 2.).ceil().max(1.)
}

pub fn underline_below_glyph(cx: &App, face: Face, thickness: f32) -> f32 {
    gpui_baseline(cx, &[face], face.line_height) + underline_gap(thickness)
}

#[cfg(test)]
mod tests {
    use super::underline_gap;

    #[test]
    fn an_underline_leaves_blinks_gap_below_the_baseline() {
        assert_eq!(underline_gap(0.55), 1.);
        assert_eq!(underline_gap(1.), 1.);
        assert_eq!(underline_gap(3.), 2.);
    }
}

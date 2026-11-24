use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::theme::{tokens, Type, SANS};
use crate::widgets::css_box::css_box_exact;
use crate::widgets::exact::{unbreakable, unrounded_width};
use crate::widgets::wrap::wrapped_text;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Primary,
    Secondary,
    Danger,
    Warn,
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonSize {
    Regular,
    Small,
    Compact,
}

pub const INERT_OPACITY: f32 = 0.5;

pub fn faded_over(ink: Hsla, under: Hsla) -> Hsla {
    let (top, base) = (Rgba::from(ink), Rgba::from(under));
    let mix = |a: f32, b: f32| a * INERT_OPACITY + b * (1. - INERT_OPACITY);
    Hsla::from(Rgba {
        r: mix(top.r, base.r),
        g: mix(top.g, base.g),
        b: mix(top.b, base.b),
        a: 1.,
    })
}

type OnClick = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    variant: Variant,
    size: ButtonSize,
    disabled: bool,
    style: StyleRefinement,
    busy_label: Option<SharedString>,
    on_click: Option<OnClick>,
}

fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, variant: Variant) -> Button {
    Button {
        id: id.into(),
        label: label.into(),
        variant,
        size: ButtonSize::Regular,
        disabled: false,
        style: StyleRefinement::default(),
        busy_label: None,
        on_click: None,
    }
}

pub fn primary(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    button(id, label, Variant::Primary)
}

pub fn secondary(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    button(id, label, Variant::Secondary)
}

pub fn danger(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    button(id, label, Variant::Danger)
}

pub fn warn(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    button(id, label, Variant::Warn)
}

pub fn quiet(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    button(id, label, Variant::Quiet)
}

impl Button {
    pub fn small(mut self) -> Self {
        self.size = ButtonSize::Small;
        self
    }

    pub fn compact(mut self) -> Self {
        self.size = ButtonSize::Compact;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn busy_label(mut self, label: Option<SharedString>) -> Self {
        self.busy_label = label;
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

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

fn metrics(size: ButtonSize, variant: Variant) -> (f32, f32, f32) {
    let (inset, font) = match (size, variant) {
        (ButtonSize::Regular, Variant::Primary) => (16., 14.),
        (ButtonSize::Small, Variant::Primary) => (14., 12.),
        (ButtonSize::Compact, _) => (10., 12.),
        _ => (12., 12.),
    };
    let border = if matches!(variant, Variant::Secondary | Variant::Quiet) {
        1.
    } else {
        0.
    };
    (inset, font, border)
}

fn label_face(window: &Window, size: f32) -> TextStyle {
    let mut face = window.text_style();
    face.font_family = SANS.into();
    face.font_weight = FontWeight::SEMIBOLD;
    face.font_size = px(size).into();
    face
}

pub fn frame_width(window: &Window, label: &str, variant: Variant, small: bool) -> Pixels {
    let size = if small {
        ButtonSize::Small
    } else {
        ButtonSize::Regular
    };
    let (inset, font, border) = metrics(size, variant);
    unrounded_width(window, label, &label_face(window, font)) + px(2. * (inset + border))
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = tokens(cx);
        let inert = self.disabled || self.busy_label.is_some();
        let label = self.busy_label.unwrap_or(self.label);
        let (inset, size, border) = metrics(self.size, self.variant);
        let face = label_face(window, size);
        let scale = window.scale_factor();
        let unrounded = unrounded_width(window, &label, &face);
        let natural = px((f32::from(unrounded) * scale).ceil() / scale);
        let frame = unrounded + px(2. * (inset + border));
        let widest = unbreakable(&label)
            .map(|segment| unrounded_width(window, segment, &face))
            .fold(px(0.), |a, b| a.max(b));
        let floor = widest + px(2. * (inset + border));
        let (background, border_colour) = match self.variant {
            Variant::Primary => (t.accent, None),
            Variant::Secondary => (t.accent_soft, Some(t.line)),
            Variant::Danger => (t.warn_soft, None),
            Variant::Warn => (t.warn, None),
            Variant::Quiet => (t.surface, Some(t.line)),
        };
        let mut button = div()
            .id(self.id)
            .flex()
            .min_w(floor)
            .items_center()
            .justify_center()
            .text_center()
            .rounded(px(6.))
            .semibold()
            .map(|d| match (self.size, self.variant) {
                (ButtonSize::Regular, Variant::Primary) => d.px(px(16.)).py(px(8.)).size_sm(),
                (ButtonSize::Small, Variant::Primary) => d.px(px(14.)).py(px(6.)).size_xs(),
                (ButtonSize::Compact, _) => d.px(px(10.)).py(px(6.)).size_xs(),
                _ => d.px(px(12.)).py(px(6.)).size_xs(),
            })
            .relative()
            .map(|d| {
                let (ink, bordered) = match self.variant {
                    Variant::Primary => (white(), false),
                    Variant::Secondary => (t.accent, true),
                    Variant::Danger => (t.warn, false),
                    Variant::Warn => (white(), false),
                    Variant::Quiet => (t.warn, true),
                };
                let ink = if inert {
                    faded_over(ink, t.surface)
                } else {
                    ink
                };
                d.text_color(ink).when(bordered, |d| d.border_1())
            })
            .when(inert, |d| d.cursor_not_allowed());
        button.style().refine(&self.style);
        button
            .test_support()
            .role(Role::Button)
            .aria_label(label.clone())
            .when(!inert, |d| d.cursor_pointer())
            .when_some(self.on_click.filter(|_| !inert), |d, handler| {
                d.on_click(move |event, window, cx| handler(event, window, cx))
            })
            .child(css_box_exact(
                if inert {
                    background.opacity(INERT_OPACITY)
                } else {
                    background
                },
                border_colour.map(|c| if inert { c.opacity(INERT_OPACITY) } else { c }),
                6.,
                Some(frame),
            ))
            .child(
                div()
                    .min_w(px(0.))
                    .w(natural)
                    .mr(unrounded - natural)
                    .child(wrapped_text(label)),
            )
    }
}

#[cfg(test)]
pub mod tests {
    use std::sync::Arc;

    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        div, point, px, size, AppContext, Bounds, Context, IntoElement, ParentElement, Point,
        Render, Styled, TestAppContext, TestDispatcher, Window, WindowBounds, WindowOptions,
    };

    use super::{primary, secondary};
    use crate::theme;
    use crate::widgets::css_box::css_px_box;

    struct Row;

    impl Render for Row {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .gap(px(8.))
                .child(primary("search", "Search"))
                .child(secondary("index", "Index now"))
                .child(secondary("memories", "Open Memories"))
                .child(secondary("again", "Test again"))
        }
    }

    #[gpui_kit::test]
    fn a_button_is_as_wide_as_chrome_lays_it_out(_cx: &mut TestAppContext) {
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
                |_, cx| cx.new(|_| Row),
            )
            .unwrap()
        });
        let widths = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                ["search", "index", "memories", "again"]
                    .map(|id| f32::from(window.find(id).bounds().size.width))
            })
            .unwrap();
        let chrome = [73.156, 79.203, 106.672, 77.734];
        for (width, chrome) in widths.into_iter().zip(chrome) {
            assert!(
                (width - chrome).abs() <= 0.25,
                "laid out {width}, Chrome {chrome}: more than a device pixel's snap apart"
            );
        }
    }

    #[test]
    fn a_disabled_button_fades_as_one_group_like_css_opacity() {
        let theme = theme::Tokens::light();
        let label = gpui_kit::Rgba::from(super::faded_over(gpui_kit::white(), theme.surface));
        let chrome = [0xfe, 0xfc, 0xf9];
        for (channel, chrome) in [label.r, label.g, label.b].into_iter().zip(chrome) {
            assert!(
                (channel * 255. - chrome as f32).abs() <= 1.,
                "white at 50% over surface reads {label:?}, Chrome paints #fefcf9"
            );
        }
    }

    #[test]
    fn button_decoration_is_painted_on_whole_css_pixels() {
        let snapped = css_px_box(Bounds {
            origin: point(px(822.), px(95.5)),
            size: size(px(79.5), px(28.)),
        });
        assert_eq!(snapped.origin, point(px(822.), px(96.)));
        assert_eq!(snapped.size, size(px(80.), px(28.)));
        let snapped = css_px_box(Bounds {
            origin: point(px(1310.5), px(72.)),
            size: size(px(73.5), px(36.)),
        });
        assert_eq!(snapped.origin.x, px(1311.));
        assert_eq!(snapped.size.width, px(73.));
    }

    pub fn painted_span<V: Render>(
        width: f32,
        view: impl FnOnce() -> V + 'static,
        background: impl FnOnce(&theme::Tokens) -> gpui_kit::Hsla,
    ) -> (f32, f32) {
        let mut cx = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            None,
            Arc::new(gpui_apple::AppleTextSystem::new()),
        );
        let (window, colour) = cx.update(|cx| {
            theme::install(cx);
            let colour = background(theme::tokens(cx));
            let (window, _) = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(width), px(100.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| view()),
            )
            .unwrap();
            (window, colour)
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let wanted = gpui_kit::Background::from(colour);
            let quads: Vec<_> = window
                .painted_quads()
                .into_iter()
                .filter(|q| q.background == wanted)
                .collect();
            assert_eq!(quads.len(), 1, "expected one painted frame in that colour");
            let b = quads[0].bounds;
            (b.origin.x.0, b.origin.x.0 + b.size.width.0)
        })
        .unwrap()
    }

    struct HalfPixel;

    impl Render for HalfPixel {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .pl(px(240.516))
                .child(secondary("disconnect", "Disconnect"))
        }
    }

    #[gpui_kit::test]
    fn a_button_on_a_half_pixel_paints_its_frame_where_chrome_does(_cx: &mut TestAppContext) {
        assert_eq!(
            painted_span(800., || HalfPixel, |t| t.accent_soft),
            (482., 646.),
            "Chrome paints this Disconnect at x=240.516 w=82.953 over device px 482..646"
        );
    }

    struct RepairRow;

    impl Render for RepairRow {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .gap(px(8.))
                .pl(px(171.))
                .child(primary("repair", "Repair").small())
                .child(secondary("disconnect", "Disconnect"))
        }
    }

    #[gpui_kit::test]
    fn a_button_after_another_paints_where_chrome_does(_cx: &mut TestAppContext) {
        assert_eq!(
            painted_span(800., || RepairRow, |t| t.accent_soft),
            (482., 646.),
            "Chrome lays Disconnect out at x=240.516 after Repair and paints device px 482..646"
        );
    }

    struct PanelHeader;

    impl Render for PanelHeader {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .justify_end()
                .w(px(901.656))
                .child(secondary("index", "Index now"))
        }
    }

    #[gpui_kit::test]
    fn a_right_aligned_button_on_fractional_edges_paints_where_chrome_does(
        _cx: &mut TestAppContext,
    ) {
        assert_eq!(
            painted_span(1000., || PanelHeader, |t| t.accent_soft),
            (1644., 1804.),
            "Chrome lays Index now out at x=822.453 r=901.656 and paints device px 1644..1804"
        );
    }
}

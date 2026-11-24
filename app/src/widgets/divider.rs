use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::widgets::baseline;

use crate::theme::{tokens, Type};

pub fn dash_spans(length: f32, thickness: f32) -> Vec<(f32, f32)> {
    let (dash, gap) = if thickness >= 3. {
        (thickness * 2., thickness)
    } else {
        (thickness * 3., thickness * 2.)
    };
    let fewest = ((length + gap) / (dash + gap)).floor();
    let most = fewest + 1.;
    let gap_for = |dashes: f32| (length - dashes * dash) / (dashes - 1.);
    let (wide, narrow) = (gap_for(fewest), gap_for(most));
    let (count, spacing) = if narrow <= 0. || (wide - gap).abs() < (narrow - gap).abs() {
        (fewest, wide)
    } else {
        (most, narrow)
    };
    if count < 2. || !spacing.is_finite() {
        return vec![(0., length.max(0.))];
    }
    (0..count as usize)
        .map(|ix| (ix as f32 * (dash + spacing), dash))
        .collect()
}

pub fn dashed_rule(thickness: Pixels, colour: Hsla) -> Div {
    div().flex_grow(1.).h(thickness).child(
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let spans = dash_spans(f32::from(bounds.size.width), f32::from(thickness));
                for (start, length) in spans {
                    window.paint_quad(fill(
                        Bounds {
                            origin: point(bounds.origin.x + px(start), bounds.origin.y),
                            size: size(px(length), thickness),
                        },
                        colour,
                    ));
                }
            },
        )
        .size_full(),
    )
}

#[derive(IntoElement)]
pub struct Divider {
    id: ElementId,
    label: SharedString,
    style: StyleRefinement,
}

pub fn divider(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Divider {
    Divider {
        id: id.into(),
        label: label.into(),
        style: StyleRefinement::default(),
    }
}

impl Styled for Divider {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Divider {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let warn = tokens(cx).warn;
        let rule = || dashed_rule(px(2.), warn);
        let mut divider = div()
            .id(self.id)
            .flex()
            .items_center()
            .gap(px(8.))
            .py(px(2.));
        divider.style().refine(&self.style);
        divider
            .test_support()
            .aria_label(self.label.clone())
            .child(rule())
            .child(
                div()
                    .mono_medium()
                    .type_px(11.)
                    .text_color(warn)
                    .child(baseline::text(self.label)),
            )
            .child(rule())
    }
}

#[cfg(test)]
mod tests {
    use super::dash_spans;

    #[test]
    fn a_dashed_rule_spaces_its_dashes_the_way_chrome_does() {
        let spans = dash_spans(95.1, 2.);
        assert_eq!(spans.len(), 10);
        assert!(spans.iter().all(|(_, len)| *len == 6.));
        let gap = spans[1].0 - spans[0].0 - 6.;
        assert!((gap - 3.9).abs() < 0.01, "{gap}");
        let (last, len) = spans[9];
        assert!((last + len - 95.1).abs() < 0.001);
        assert_eq!(dash_spans(10., 2.), vec![(0., 10.)]);
    }
}

use gpui_kit::*;

pub const ELLIPSIS: &str = "…";

pub fn tail_that_fits(text: &str, room: Pixels, width: impl Fn(&str) -> Pixels) -> Option<String> {
    if width(text) <= room {
        return None;
    }
    let room = room - width(ELLIPSIS);
    let tail = text
        .char_indices()
        .map(|(at, _)| &text[at..])
        .find(|tail| width(tail) <= room)
        .unwrap_or("");
    Some(tail.to_string())
}

pub fn head_that_fits(text: &str, room: Pixels, width: impl Fn(&str) -> Pixels) -> Option<String> {
    if width(text) <= room {
        return None;
    }
    let room = room - width(ELLIPSIS);
    let ends: Vec<usize> = text
        .char_indices()
        .map(|(at, _)| at)
        .chain(std::iter::once(text.len()))
        .collect();
    let (mut fits, mut over) = (0, ends.len());
    while over - fits > 1 {
        let mid = (fits + over) / 2;
        if width(&text[..ends[mid]]) <= room {
            fits = mid;
        } else {
            over = mid;
        }
    }
    Some(text[..ends[fits]].to_string())
}

pub fn tail_lefts(right: Pixels, tail: Pixels, ellipsis: Pixels) -> (Pixels, Pixels) {
    let tail_left = right - tail;
    (tail_left - ellipsis, tail_left)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cut {
    Start,
    End,
}

pub struct TailText {
    text: SharedString,
    cut: Cut,
}

pub fn tail_text(text: impl Into<SharedString>) -> TailText {
    TailText {
        text: text.into(),
        cut: Cut::Start,
    }
}

pub fn ellipsis_text(text: impl Into<SharedString>) -> TailText {
    TailText {
        text: text.into(),
        cut: Cut::End,
    }
}

pub struct Measured {
    style: TextStyle,
    font_size: Pixels,
    line_height: Pixels,
}

pub struct Shaped {
    ellipsis: Option<ShapedLine>,
    tail: ShapedLine,
}

impl IntoElement for TailText {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

fn shape(window: &Window, measured: &Measured, text: &str) -> ShapedLine {
    window.text_system().shape_line(
        SharedString::from(text.to_string()),
        measured.font_size,
        &[measured.style.to_run(text.len())],
        None,
    )
}

impl Element for TailText {
    type RequestLayoutState = Measured;
    type PrepaintState = Shaped;

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
        _cx: &mut App,
    ) -> (LayoutId, Measured) {
        let style = window.text_style();
        let rem = window.rem_size();
        let font_size = style.font_size.to_pixels(rem);
        let line_height = style.line_height.to_pixels(style.font_size, rem);
        let measured = Measured {
            style,
            font_size,
            line_height,
        };
        let full = shape(window, &measured, &self.text).width;
        let mut layout = Style {
            flex_grow: if self.cut == Cut::Start { 1. } else { 0. },
            flex_shrink: 1.,
            ..Style::default()
        };
        layout.min_size.width = px(0.).into();
        layout.size.height = line_height.into();
        let id = window.request_measured_layout(layout, move |known, _, _, _| {
            size(
                known.width.unwrap_or(full),
                known.height.unwrap_or(line_height),
            )
        });
        (id, measured)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        measured: &mut Measured,
        window: &mut Window,
        _cx: &mut App,
    ) -> Shaped {
        let measure = |text: &str| shape(window, measured, text).width;
        let fitted = match self.cut {
            Cut::Start => tail_that_fits(&self.text, bounds.size.width, measure),
            Cut::End => head_that_fits(&self.text, bounds.size.width, measure),
        };
        match fitted {
            None => Shaped {
                ellipsis: None,
                tail: shape(window, measured, &self.text),
            },
            Some(tail) => Shaped {
                ellipsis: Some(shape(window, measured, ELLIPSIS)),
                tail: shape(window, measured, &tail),
            },
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        measured: &mut Measured,
        shaped: &mut Shaped,
        window: &mut Window,
        cx: &mut App,
    ) {
        let height = measured.line_height;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            let Some(ellipsis) = &shaped.ellipsis else {
                let _ = shaped
                    .tail
                    .paint(bounds.origin, height, TextAlign::Left, None, window, cx);
                return;
            };
            let (ellipsis_left, tail_left) = match self.cut {
                Cut::Start => tail_lefts(bounds.right(), shaped.tail.width, ellipsis.width),
                Cut::End => (bounds.left() + shaped.tail.width, bounds.left()),
            };
            let top = bounds.origin.y;
            let _ = ellipsis.paint(
                point(ellipsis_left, top),
                height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
            let _ = shaped.tail.paint(
                point(tail_left, top),
                height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{px, Pixels};

    use super::{head_that_fits, ELLIPSIS};

    fn monospace(s: &str) -> Pixels {
        px(s.chars().count() as f32)
    }

    #[test]
    fn an_end_ellipsis_keeps_the_space_chrome_keeps() {
        assert_eq!(head_that_fits("equal weights", px(20.), monospace), None);
        let head = head_that_fits("equal weights of flour", px(9.), monospace).unwrap();
        assert_eq!(head, "equal we");
        let head = head_that_fits("equal weights of flour", px(7.), monospace).unwrap();
        assert_eq!(format!("{head}{ELLIPSIS}"), "equal …");
        assert_eq!(head_that_fits("abc", px(0.5), monospace).unwrap(), "");
    }
}

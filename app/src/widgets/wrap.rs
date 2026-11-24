use std::ops::Range;

use gpui_kit::*;

use crate::theme::{css_line_box, gpui_baseline, Face};
use crate::widgets::baseline::{self, Baselines};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakPiece {
    pub range: Range<usize>,
    pub forced: bool,
}

pub fn break_pieces(text: &str) -> Vec<BreakPiece> {
    let mut out = Vec::new();
    let mut start = 0;
    let paragraphs: Vec<&str> = text.split('\n').collect();
    let last = paragraphs.len() - 1;
    for (ix, paragraph) in paragraphs.into_iter().enumerate() {
        let mut at = start;
        let before = out.len();
        let mut chars = paragraph.char_indices().peekable();
        while let Some((ix, ch)) = chars.next() {
            let next = chars.peek().map(|(_, c)| *c);
            let breaks = ch == ' ' || (ch == '-' && next != Some('-'));
            if breaks || next.is_none() {
                let end = start + ix + ch.len_utf8();
                out.push(BreakPiece {
                    range: at..end,
                    forced: false,
                });
                at = end;
            }
        }
        if out.len() == before {
            out.push(BreakPiece {
                range: at..at,
                forced: false,
            });
        }
        if ix < last {
            if let Some(piece) = out.last_mut() {
                piece.forced = true;
            }
        }
        start += paragraph.len() + 1;
    }
    out
}

pub fn chrome_lines(text: &str, room: Pixels, width: impl Fn(&str) -> Pixels) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for piece in break_pieces(text) {
        let part = &text[piece.range];
        let candidate = format!("{line}{part}");
        if line.trim().is_empty() || width(candidate.trim_end_matches(' ')) <= room {
            line = candidate;
        } else {
            lines.push(std::mem::take(&mut line));
            line = part.to_string();
        }
        if piece.forced {
            lines.push(std::mem::take(&mut line));
        }
    }
    lines.push(line);
    lines
}

const NO_BREAK_BEFORE: &str = ")]}.,:;!?/-\"'%";
const NO_BREAK_AFTER: &str = "([{\"'$";

fn breakable(before: char, after: char) -> bool {
    !NO_BREAK_BEFORE.contains(after) && !NO_BREAK_AFTER.contains(before)
}

pub fn break_all_lines(
    text: &str,
    first_room: Pixels,
    room: Pixels,
    width: impl Fn(&str) -> Pixels,
) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line: Vec<char> = Vec::new();
        for ch in paragraph.chars() {
            let fits = if lines.is_empty() { first_room } else { room };
            let candidate: String = line.iter().chain(std::iter::once(&ch)).collect();
            if ch == ' ' || line.is_empty() || width(candidate.trim_end_matches(' ')) <= fits {
                line.push(ch);
                continue;
            }
            let at = (1..=line.len())
                .rev()
                .find(|&ix| {
                    let after = line.get(ix).copied().unwrap_or(ch);
                    breakable(line[ix - 1], after)
                })
                .unwrap_or(line.len());
            let tail = line.split_off(at);
            lines.push(line.into_iter().collect());
            line = tail;
            line.push(ch);
        }
        lines.push(line.into_iter().collect());
    }
    lines
}

#[derive(Clone)]
pub struct WrappedText {
    text: SharedString,
    break_all: bool,
    indent: Pixels,
    line: Option<(Face, Vec<Face>)>,
}

pub fn wrapped_text(text: impl Into<SharedString>) -> WrappedText {
    WrappedText {
        text: text.into(),
        break_all: false,
        indent: px(0.),
        line: None,
    }
}

impl WrappedText {
    pub fn break_all(mut self) -> Self {
        self.break_all = true;
        self
    }

    pub fn indent(mut self, indent: Pixels) -> Self {
        self.indent = indent;
        self
    }

    pub fn on_line(mut self, strut: Face, line: &[Face]) -> Self {
        self.line = Some((strut, line.to_vec()));
        self
    }

    fn lines(&self, room: Pixels, width: impl Fn(&str) -> Pixels) -> Vec<String> {
        if self.break_all {
            break_all_lines(&self.text, room - self.indent, room, width)
        } else {
            chrome_lines(&self.text, room, width)
        }
    }
}

pub struct Measured {
    style: TextStyle,
    font_size: Pixels,
    line_height: Pixels,
    baselines: Option<Baselines>,
}

impl IntoElement for WrappedText {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

fn shape(window: &Window, style: &TextStyle, font_size: Pixels, text: &str) -> ShapedLine {
    window.text_system().shape_line(
        SharedString::from(text.to_string()),
        font_size,
        &[style.to_run(text.len())],
        None,
    )
}

impl Element for WrappedText {
    type RequestLayoutState = Measured;
    type PrepaintState = Vec<(ShapedLine, Pixels)>;

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
    ) -> (LayoutId, Measured) {
        let style = window.text_style();
        let rem = window.rem_size();
        let font_size = style.font_size.to_pixels(rem);
        let own_face = baseline::face_of(&style, rem);
        let (line_height, baselines) = match (&self.line, own_face) {
            (Some((strut, faces)), Some(face)) => {
                let shared = css_line_box(cx, *strut, faces);
                (
                    px(shared.height),
                    Some(Baselines {
                        gpui: gpui_baseline(cx, &[face], shared.height),
                        web: shared.baseline,
                    }),
                )
            }
            _ => {
                let line_height = style.line_height.to_pixels(style.font_size, rem);
                (
                    line_height,
                    own_face
                        .map(|face| baseline::baselines(cx, face, &[face], f32::from(line_height))),
                )
            }
        };
        let wrapper = self.clone();
        let indent = self.indent;
        let measure_style = style.clone();
        let mut layout = Style::default();
        layout.min_size.width = px(0.).into();
        let id = window.request_measured_layout(layout, move |known, available, window, _| {
            let width = |s: &str| shape(window, &measure_style, font_size, s).width;
            let room = known.width.or(match available.width {
                AvailableSpace::Definite(room) => Some(room),
                _ => None,
            });
            let snap = px(0.5 / window.scale_factor());
            let lines = match room {
                Some(room) => wrapper.lines(room + snap, width),
                None => wrapper.text.split('\n').map(str::to_string).collect(),
            };
            let widest = lines
                .iter()
                .enumerate()
                .map(|(ix, l)| {
                    width(l.trim_end_matches(' ')) + if ix == 0 { indent } else { px(0.) }
                })
                .fold(px(0.), |a, b| a.max(b));
            let scale = window.scale_factor();
            let height = f32::from(line_height) * lines.len().max(1) as f32;
            size(
                known.width.unwrap_or(widest),
                known.height.unwrap_or(px((height * scale).round() / scale)),
            )
        });
        (
            id,
            Measured {
                style,
                font_size,
                line_height,
                baselines,
            },
        )
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        measured: &mut Measured,
        window: &mut Window,
        _cx: &mut App,
    ) -> Vec<(ShapedLine, Pixels)> {
        let width = |s: &str| shape(window, &measured.style, measured.font_size, s).width;
        let content_top = bounds.origin.y - window.pixel_snap_point(window.element_offset()).y;
        let height = measured.line_height;
        let snap = px(0.5 / window.scale_factor());
        self.lines(bounds.size.width + snap, width)
            .iter()
            .enumerate()
            .map(|(ix, line)| {
                let top = height * ix as f32;
                let shift = measured.baselines.map_or(px(0.), |b| {
                    window.pixel_snap(px(baseline::web_offset(f32::from(content_top + top), b)))
                });
                (
                    shape(
                        window,
                        &measured.style,
                        measured.font_size,
                        line.trim_end_matches(' '),
                    ),
                    top + shift,
                )
            })
            .collect()
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        measured: &mut Measured,
        lines: &mut Vec<(ShapedLine, Pixels)>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let height = measured.line_height;
        for (ix, (line, top)) in lines.iter().enumerate() {
            let indent = if ix == 0 { self.indent } else { px(0.) };
            let origin = point(bounds.origin.x + indent, bounds.origin.y + *top);
            let _ = line.paint(
                origin,
                height,
                measured.style.text_align,
                Some(bounds.size.width),
                window,
                cx,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{px, Pixels};

    use super::{break_all_lines, chrome_lines};

    fn monospace(s: &str) -> Pixels {
        px(s.chars().count() as f32)
    }

    #[test]
    fn lines_break_at_spaces_and_hang_trailing_spaces() {
        assert_eq!(
            chrome_lines("feed it twice a day", px(10.), monospace),
            ["feed it ", "twice a ", "day"]
        );
        assert_eq!(
            chrome_lines("feed it twice", px(7.), monospace),
            ["feed it ", "twice"]
        );
        assert_eq!(
            chrome_lines("# Title\n\nbody", px(40.), monospace),
            ["# Title", "", "body"]
        );
        assert_eq!(
            chrome_lines("an overlongword fits", px(5.), monospace),
            ["an ", "overlongword ", "fits"]
        );
        assert_eq!(
            chrome_lines("well-fed starter", px(6.), monospace),
            ["well-", "fed ", "starter"]
        );
    }

    #[test]
    fn a_line_never_breaks_between_two_hyphens() {
        assert_eq!(
            chrome_lines("index --reindex", px(1.), monospace),
            ["index ", "--", "reindex"]
        );
        assert_eq!(chrome_lines("a-b", px(1.), monospace), ["a-", "b"]);
    }

    #[test]
    fn break_all_breaks_between_any_two_characters_after_a_shorter_first_line() {
        assert_eq!(
            break_all_lines("/var/folders/x", px(6.), px(8.), monospace),
            ["/var/f", "olders/x"]
        );
        assert_eq!(
            break_all_lines("ab cd", px(2.), px(2.), monospace),
            ["ab ", "cd"]
        );
        assert_eq!(
            break_all_lines("/tmp/buk3K1/home", px(11.), px(11.), monospace),
            ["/tmp/buk3K", "1/home"]
        );
    }
}

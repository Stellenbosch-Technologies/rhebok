use gpui_kit::*;

use crate::theme::{css_line_box, gpui_baseline, Face};
use crate::widgets::baseline::{web_offset, Baselines};
use crate::widgets::wrap::break_pieces;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chip {
    pub background: Hsla,
    pub pad: f32,
    pub radius: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub text: SharedString,
    pub face: Face,
    pub colour: Option<Hsla>,
    pub chip: Option<Chip>,
}

impl Segment {
    pub fn text(text: impl Into<SharedString>, face: Face) -> Segment {
        Segment {
            text: text.into(),
            face,
            colour: None,
            chip: None,
        }
    }

    pub fn chip(text: impl Into<SharedString>, face: Face, chip: Option<Chip>) -> Segment {
        Segment {
            text: text.into(),
            face,
            colour: None,
            chip: chip.or(Some(Chip {
                background: transparent_black(),
                pad: 0.,
                radius: 0.,
            })),
        }
    }

    pub fn coloured(mut self, colour: Hsla) -> Segment {
        self.colour = Some(colour);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fragment {
    pub segment: usize,
    pub text: String,
    pub opens: bool,
    pub closes: bool,
}

fn fragments(segments: &[Segment]) -> Vec<(Vec<Fragment>, bool)> {
    let text: String = segments.iter().map(|s| s.text.as_ref()).collect();
    let mut bounds = Vec::new();
    let mut at = 0;
    for segment in segments {
        bounds.push(at..at + segment.text.len());
        at += segment.text.len();
    }
    break_pieces(&text)
        .into_iter()
        .map(|piece| {
            let parts = bounds
                .iter()
                .enumerate()
                .filter_map(|(ix, seg)| {
                    let from = piece.range.start.max(seg.start);
                    let to = piece.range.end.min(seg.end);
                    (from < to).then(|| Fragment {
                        segment: ix,
                        text: text[from..to].to_string(),
                        opens: from == seg.start,
                        closes: to == seg.end,
                    })
                })
                .collect();
            (parts, piece.forced)
        })
        .collect()
}

fn merged(line: &[Fragment]) -> Vec<Fragment> {
    let mut out: Vec<Fragment> = Vec::new();
    for part in line {
        match out.last_mut() {
            Some(last) if last.segment == part.segment => {
                last.text.push_str(&part.text);
                last.closes = part.closes;
            }
            _ => out.push(part.clone()),
        }
    }
    out
}

fn pads(segment: &Segment, part: &Fragment) -> (f32, f32) {
    let pad = segment.chip.map_or(0., |c| c.pad);
    (
        if part.opens { pad } else { 0. },
        if part.closes { pad } else { 0. },
    )
}

fn shown(line: &[Fragment], ix: usize) -> &str {
    if ix + 1 == line.len() {
        line[ix].text.trim_end_matches(' ')
    } else {
        &line[ix].text
    }
}

pub fn line_width(
    segments: &[Segment],
    line: &[Fragment],
    width: &impl Fn(&Segment, &str) -> Pixels,
) -> Pixels {
    let line = merged(line);
    (0..line.len())
        .map(|ix| {
            let segment = &segments[line[ix].segment];
            let (left, right) = pads(segment, &line[ix]);
            width(segment, shown(&line, ix)) + px(left + right)
        })
        .fold(px(0.), |a, b| a + b)
}

pub fn flow_lines(
    segments: &[Segment],
    room: Pixels,
    width: impl Fn(&Segment, &str) -> Pixels,
) -> Vec<Vec<Fragment>> {
    let mut lines: Vec<Vec<Fragment>> = Vec::new();
    let mut line: Vec<Fragment> = Vec::new();
    for (piece, forced) in fragments(segments) {
        let blank = line.iter().all(|p| p.text.trim().is_empty());
        let mut candidate = line.clone();
        candidate.extend(piece.iter().cloned());
        if blank || line_width(segments, &candidate, &width) <= room {
            line = candidate;
        } else {
            lines.push(merged(&std::mem::take(&mut line)));
            line = piece;
        }
        if forced {
            lines.push(merged(&std::mem::take(&mut line)));
        }
    }
    lines.push(merged(&line));
    lines
}

pub struct InlineFlow {
    strut: Face,
    segments: Vec<Segment>,
}

pub fn inline_flow(strut: Face, segments: Vec<Segment>) -> InlineFlow {
    InlineFlow { strut, segments }
}

impl InlineFlow {
    fn faces(&self) -> Vec<Face> {
        let mut faces = vec![self.strut];
        for segment in &self.segments {
            if !faces.contains(&segment.face) {
                faces.push(segment.face);
            }
        }
        faces
    }
}

fn shape(window: &Window, segment: &Segment, colour: Hsla, text: &str) -> ShapedLine {
    let face = segment.face;
    window.text_system().shape_line(
        SharedString::from(text.to_string()),
        px(face.size),
        &[TextRun {
            len: text.len(),
            font: Font {
                weight: face.weight,
                ..font(SharedString::from(face.family))
            },
            color: segment.colour.unwrap_or(colour),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    )
}

fn rounded_metrics(cx: &App, face: Face) -> (f32, f32) {
    let id = cx.text_system().resolve_font(&Font {
        weight: face.weight,
        ..font(SharedString::from(face.family))
    });
    let ascent = f32::from(cx.text_system().ascent(id, px(face.size))).round();
    let descent = (-f32::from(cx.text_system().descent(id, px(face.size)))).round();
    (ascent, descent)
}

impl IntoElement for InlineFlow {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

pub struct Laid {
    line_height: Pixels,
    web_baseline: f32,
    colour: Hsla,
}

pub struct Placed {
    text: ShapedLine,
    origin: Point<Pixels>,
    chip: Option<(Hsla, Bounds<Pixels>, Corners<Pixels>)>,
}

impl Element for InlineFlow {
    type RequestLayoutState = Laid;
    type PrepaintState = Vec<Placed>;

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
    ) -> (LayoutId, Laid) {
        let faces = self.faces();
        let line = css_line_box(cx, self.strut, &faces);
        let line_height = px(line.height);
        let colour = window.text_style().color;
        let segments = self.segments.clone();
        let mut layout = Style::default();
        layout.min_size.width = px(0.).into();
        let id = window.request_measured_layout(layout, move |known, available, window, _| {
            let width = |segment: &Segment, text: &str| shape(window, segment, colour, text).width;
            let room = known.width.or(match available.width {
                AvailableSpace::Definite(room) => Some(room),
                _ => None,
            });
            let snap = px(0.5 / window.scale_factor());
            let lines = match room {
                Some(room) => flow_lines(&segments, room + snap, width),
                None => flow_lines(&segments, px(f32::MAX), width),
            };
            let widest = lines
                .iter()
                .map(|l| line_width(&segments, l, &width))
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
            Laid {
                line_height,
                web_baseline: line.baseline,
                colour,
            },
        )
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        laid: &mut Laid,
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<Placed> {
        let colour = laid.colour;
        let width = |segment: &Segment, text: &str| shape(window, segment, colour, text).width;
        let snap = px(0.5 / window.scale_factor());
        let lines = flow_lines(&self.segments, bounds.size.width + snap, width);
        let content_top = bounds.origin.y - window.pixel_snap_point(window.element_offset()).y;
        let mut placed = Vec::new();
        for (ix, line) in lines.iter().enumerate() {
            let top = laid.line_height * ix as f32;
            let mut x = bounds.origin.x;
            for part_ix in 0..line.len() {
                let part = &line[part_ix];
                let segment = &self.segments[part.segment];
                let (left, right) = pads(segment, part);
                let shaped = shape(window, segment, colour, shown(line, part_ix));
                let baselines = Baselines {
                    gpui: gpui_baseline(cx, &[segment.face], f32::from(laid.line_height)),
                    web: laid.web_baseline,
                };
                let shift =
                    window.pixel_snap(px(web_offset(f32::from(content_top + top), baselines)));
                let origin = point(x + px(left), bounds.origin.y + top + shift);
                let far = x + px(left) + shaped.width + px(right);
                let chip = segment.chip.filter(|c| c.background.a > 0.).map(|chip| {
                    let (ascent, descent) = rounded_metrics(cx, segment.face);
                    let base = origin.y + px(baselines.gpui);
                    let round = |on: bool| px(if on { chip.radius } else { 0. });
                    (
                        chip.background,
                        Bounds::from_corners(
                            point(x, base - px(ascent)),
                            point(far, base + px(descent)),
                        ),
                        Corners {
                            top_left: round(part.opens),
                            bottom_left: round(part.opens),
                            top_right: round(part.closes),
                            bottom_right: round(part.closes),
                        },
                    )
                });
                x = far;
                placed.push(Placed {
                    text: shaped,
                    origin,
                    chip,
                });
            }
        }
        placed
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        laid: &mut Laid,
        placed: &mut Vec<Placed>,
        window: &mut Window,
        cx: &mut App,
    ) {
        for item in placed.iter() {
            if let Some((background, bounds, corners)) = item.chip {
                window.paint_quad(fill(bounds, background).corner_radii(corners));
            }
        }
        for item in placed.iter() {
            let _ = item.text.paint(
                item.origin,
                laid.line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui_kit::{
        div, px, size, AppContext, Bounds, Context, FontWeight, IntoElement, ParentElement, Pixels,
        Point, Render, Styled, TestAppContext, TestDispatcher, Window, WindowBounds, WindowOptions,
    };

    use gpui_kit::test::TestWindowExt;

    use super::{flow_lines, inline_flow, Chip, Segment};
    use crate::theme::{self, Face, MONO, SANS};
    use crate::widgets::wrap::chrome_lines;

    const SANS_FACE: Face = Face::new(SANS, FontWeight::NORMAL, 14., 20.);
    const CODE_FACE: Face = Face::new(MONO, FontWeight::NORMAL, 12., 20.);

    fn monospace(_: &Segment, s: &str) -> Pixels {
        px(s.chars().count() as f32)
    }

    fn chip(pad: f32) -> Option<Chip> {
        Some(Chip {
            background: gpui_kit::black(),
            pad,
            radius: 4.,
        })
    }

    fn lines(segments: &[Segment], room: f32) -> Vec<String> {
        flow_lines(segments, px(room), monospace)
            .into_iter()
            .map(|l| l.into_iter().map(|p| p.text).collect())
            .collect()
    }

    #[test]
    fn no_line_breaks_between_a_bracket_and_the_chip_beside_it() {
        let segments = vec![
            Segment::text("sessions from (", SANS_FACE),
            Segment::chip("~/.codex/sessions", CODE_FACE, chip(1.)),
            Segment::text("). Only counts", SANS_FACE),
        ];
        let got = lines(&segments, 20.);
        assert_eq!(
            got,
            ["sessions from ", "(~/.codex/sessions). ", "Only counts"]
        );
        for line in &got {
            assert!(!line.trim_end().ends_with('('), "{got:?}");
            assert!(!line.starts_with(')'), "{got:?}");
        }
    }

    #[test]
    fn a_chip_longer_than_the_line_breaks_at_its_own_spaces_and_hyphens() {
        let segments = vec![
            Segment::text("(", SANS_FACE),
            Segment::chip("rhebok index --reindex", CODE_FACE, chip(1.)),
            Segment::text(").", SANS_FACE),
        ];
        assert_eq!(lines(&segments, 10.), ["(rhebok ", "index --", "reindex)."]);
    }

    #[test]
    fn plain_text_breaks_exactly_where_chrome_lines_breaks_it() {
        for text in [
            "feed it twice a day",
            "well-fed starter",
            "an overlongword fits",
            "# Title\n\nbody",
        ] {
            for room in [5., 7., 10., 40.] {
                let segments = vec![Segment::text(text, SANS_FACE)];
                assert_eq!(
                    lines(&segments, room),
                    chrome_lines(text, px(room), |s| px(s.chars().count() as f32)),
                    "{text:?} in {room}"
                );
            }
        }
    }

    struct Broken;

    impl Render for Broken {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let t = *theme::tokens(cx);
            div().w(px(70.)).child(inline_flow(
                SANS_FACE,
                vec![
                    Segment::text("(", SANS_FACE),
                    Segment::chip(
                        "rhebok index --reindex",
                        CODE_FACE,
                        Some(Chip {
                            background: t.sunken,
                            pad: 4.,
                            radius: 4.,
                        }),
                    ),
                    Segment::text(").", SANS_FACE),
                ],
            ))
        }
    }

    #[gpui_kit::test]
    fn a_broken_chip_paints_each_fragment_with_only_its_outer_ends_rounded(
        _cx: &mut TestAppContext,
    ) {
        let mut cx = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            None,
            Arc::new(gpui_apple::AppleTextSystem::new()),
        );
        let (window, sunken) = cx.update(|cx| {
            theme::install(cx);
            let sunken = theme::tokens(cx).sunken;
            let (window, _) = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(300.), px(200.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| Broken),
            )
            .unwrap();
            (window, sunken)
        });
        let corners = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                let scale = window.scale_factor();
                window
                    .painted_quads()
                    .into_iter()
                    .filter(|q| q.background == gpui_kit::Background::from(sunken))
                    .map(|q| {
                        (
                            q.corner_radii.top_left.0 / scale,
                            q.corner_radii.top_right.0 / scale,
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap();
        assert_eq!(
            corners,
            [(4., 0.), (0., 0.), (0., 4.)],
            "Chrome slices a broken inline box: rounded and padded only where the box starts and ends"
        );
    }
}

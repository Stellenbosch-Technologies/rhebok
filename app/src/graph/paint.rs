use gpui_kit::*;

use super::{node_radius, short_title, View, LABEL_CHARS, LABEL_ZOOM};
use crate::graph::ShownNode;
use crate::theme::{GraphPalette, SANS};

pub const GRID: f32 = 48.;
pub const GRID_ALPHA: f32 = 0.06;
pub const RING_WIDTH: f64 = 1.5;
pub const RING_GAP: f64 = 2.;
pub const LABEL_SIZE: f32 = 11.;
pub const LABEL_GAP: f64 = 3.;
pub const LABEL_TOP_ASCENT: f32 = 506. / 64.;
pub const LABEL_REACH: f64 = LABEL_CHARS as f64 * LABEL_SIZE as f64;
pub const CORNER_RADIUS: f32 = 8.;

pub fn corner_mask(bounds: Bounds<Pixels>, page: Hsla) -> PaintQuad {
    let r = px(CORNER_RADIUS);
    quad(
        Bounds::new(
            bounds.origin - point(r, r),
            size(bounds.size.width + r * 2., bounds.size.height + r * 2.),
        ),
        r * 2.,
        transparent_black(),
        r,
        page,
        BorderStyle::default(),
    )
}

pub fn label_visible(bounds: Bounds<Pixels>, centre: Point<f64>, radius: f64, k: f64) -> bool {
    let top = centre.y + radius * k + LABEL_GAP;
    let (x0, y0) = (f64::from(bounds.origin.x), f64::from(bounds.origin.y));
    let (x1, y1) = (
        x0 + f64::from(bounds.size.width),
        y0 + f64::from(bounds.size.height),
    );
    centre.x + LABEL_REACH / 2. >= x0
        && centre.x - LABEL_REACH / 2. <= x1
        && top <= y1
        && top + f64::from(LABEL_SIZE) * 2. >= y0
}

pub struct Frame<'a> {
    pub bounds: Bounds<Pixels>,
    pub view: View,
    pub nodes: &'a [ShownNode],
    pub edges: &'a [(usize, usize)],
    pub positions: &'a [Point<f32>],
    pub ringed: &'a [bool],
    pub palette: GraphPalette,
}

pub fn grid_lines(bounds: Bounds<Pixels>) -> Vec<Bounds<Pixels>> {
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let mut lines = Vec::new();
    let mut x = 0.;
    while x < w {
        lines.push(Bounds::new(
            bounds.origin + point(px(x), px(0.)),
            size(px(1f32.min(w - x)), bounds.size.height),
        ));
        x += GRID;
    }
    let mut y = 0.;
    while y < h {
        lines.push(Bounds::new(
            bounds.origin + point(px(0.), px(y)),
            size(bounds.size.width, px(1f32.min(h - y))),
        ));
        y += GRID;
    }
    lines
}

pub fn paint_grid(bounds: Bounds<Pixels>, ink: Hsla, window: &mut Window) {
    let colour = ink.alpha(GRID_ALPHA);
    for line in grid_lines(bounds) {
        window.paint_quad(fill(line, colour));
    }
}

fn screen(frame: &Frame, ix: usize) -> Point<f64> {
    let p = frame.positions[ix];
    let s = frame
        .view
        .to_screen(point(f64::from(p.x), f64::from(p.y)), frame.bounds.size);
    point(
        s.x + f64::from(frame.bounds.origin.x),
        s.y + f64::from(frame.bounds.origin.y),
    )
}

fn at(p: Point<f64>) -> Point<Pixels> {
    point(px(p.x as f32), px(p.y as f32))
}

pub fn edge_path(frame: &Frame) -> Option<Path<Pixels>> {
    if frame.edges.is_empty() {
        return None;
    }
    let mut path = PathBuilder::stroke(px(1.));
    for &(a, b) in frame.edges {
        if a >= frame.positions.len() || b >= frame.positions.len() {
            continue;
        }
        path.move_to(at(screen(frame, a)));
        path.line_to(at(screen(frame, b)));
    }
    path.build().ok()
}

pub fn circle(centre: Point<f64>, radius: f64, colour: Hsla) -> PaintQuad {
    let r = px(radius as f32);
    quad(
        Bounds::new(at(centre) - point(r, r), size(r * 2., r * 2.)),
        r,
        colour,
        px(0.),
        transparent_black(),
        BorderStyle::default(),
    )
}

pub fn ring(centre: Point<f64>, radius: f64, k: f64, colour: Hsla) -> PaintQuad {
    let outer = px(((radius + RING_GAP + RING_WIDTH / 2.) * k) as f32);
    quad(
        Bounds::new(
            at(centre) - point(outer, outer),
            size(outer * 2., outer * 2.),
        ),
        outer,
        transparent_black(),
        px((RING_WIDTH * k) as f32),
        colour,
        BorderStyle::default(),
    )
}

pub fn labels_shown(k: f64) -> bool {
    k >= LABEL_ZOOM
}

fn visible(bounds: Bounds<Pixels>, centre: Point<f64>, reach: f64) -> bool {
    let (x0, y0) = (f64::from(bounds.origin.x), f64::from(bounds.origin.y));
    let (x1, y1) = (
        x0 + f64::from(bounds.size.width),
        y0 + f64::from(bounds.size.height),
    );
    centre.x + reach >= x0
        && centre.x - reach <= x1
        && centre.y + reach >= y0
        && centre.y - reach <= y1
}

pub fn label_line(window: &Window, title: &str, colour: Hsla) -> ShapedLine {
    let text = SharedString::from(short_title(title));
    window.text_system().shape_line(
        text.clone(),
        px(LABEL_SIZE),
        &[TextRun {
            len: text.len(),
            font: font(SANS),
            color: colour,
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    )
}

pub fn label_origin(line: &ShapedLine, centre: Point<f64>, radius: f64, k: f64) -> Point<Pixels> {
    let top = centre.y + radius * k + LABEL_GAP;
    point(
        px(centre.x as f32) - line.width / 2.,
        px(top as f32 + LABEL_TOP_ASCENT) - line.ascent,
    )
}

pub fn paint(frame: &Frame, window: &mut Window, cx: &mut App) -> usize {
    let mut painted = 0;
    let p = frame.palette;
    if let Some(path) = edge_path(frame) {
        window.paint_path(path, p.viewport_line);
    }
    let k = frame.view.k;
    let labels = labels_shown(k);
    for (ix, node) in frame.nodes.iter().enumerate().take(frame.positions.len()) {
        let centre = screen(frame, ix);
        let radius = node_radius(node.inbound);
        let reach = (radius + RING_GAP + RING_WIDTH) * k;
        if !labels && !visible(frame.bounds, centre, reach) {
            continue;
        }
        if visible(frame.bounds, centre, reach) {
            let colour = super::kind_colour(&p, &node.kind, node.memory_kind.as_deref());
            window.paint_quad(circle(centre, radius * k, colour));
            if frame.ringed.get(ix).copied().unwrap_or(false) {
                window.paint_quad(ring(centre, radius, k, p.sun));
            }
        }
        if labels && label_visible(frame.bounds, centre, radius, k) {
            let line = label_line(window, &node.title, p.viewport_text);
            let origin = label_origin(&line, centre, radius, k);
            let height = line.ascent + line.descent;
            let _ = line.paint(origin, height, TextAlign::Left, None, window, cx);
            painted += 1;
        }
    }
    painted
}

pub mod layout;
pub mod paint;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui_kit::{point, Hsla, Pixels, Point, Size};
use rhebok::dashboard::core::graph::{Graph, Node};

use crate::tabs::memories::sort_js;
use crate::theme::GraphPalette;

pub const NODE_REL_SIZE: f64 = 4.;
pub const LABEL_ZOOM: f64 = 1.5;
pub const LABEL_CHARS: usize = 32;
pub const ZOOM_TO_NODES: f64 = 4.;
pub const MIN_ZOOM: f64 = 0.01;
pub const MAX_ZOOM: f64 = 1000.;
pub const WHEEL_PER_PIXEL: f64 = 0.002;
pub const WHEEL_PER_LINE: f64 = 40.;
pub const DRAG_CLICK_TOLERANCE: f64 = 5.;
pub const LEGEND: [&str; 6] = ["note", "session", "tag", "entity", "memory", "project"];

#[derive(Debug, Clone, PartialEq)]
pub struct ShownNode {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub memory_kind: Option<String>,
    pub inbound: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shown {
    pub nodes: Rc<[ShownNode]>,
    pub edges: Rc<[(usize, usize)]>,
}

pub fn node_value(inbound: u32) -> f64 {
    4. + 2. * f64::from(inbound).sqrt()
}

pub fn node_radius(inbound: u32) -> f64 {
    node_value(inbound).sqrt() * NODE_REL_SIZE
}

pub fn agent_name(agent: &str) -> String {
    match agent {
        "claude-code" => "Claude Code".to_string(),
        "codex" => "Codex".to_string(),
        other => other.to_string(),
    }
}

pub fn kind_colour(p: &GraphPalette, kind: &str, memory_kind: Option<&str>) -> Hsla {
    if let Some(shade) = memory_kind {
        return match shade {
            "lesson" => p.lesson,
            "fact" => p.fact,
            "episode" => p.episode,
            _ => p.memory,
        };
    }
    match kind {
        "transcript" => p.session,
        "entity" => p.entity,
        "tag" => p.tag,
        "memory" => p.memory,
        "project" => p.project,
        _ => p.note,
    }
}

pub fn legend_colour(p: &GraphPalette, key: &str) -> Hsla {
    match key {
        "session" => p.session,
        "tag" => p.tag,
        "entity" => p.entity,
        "memory" => p.memory,
        "project" => p.project,
        _ => p.note,
    }
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

pub fn short_title(title: &str) -> String {
    if utf16_len(title) <= LABEL_CHARS {
        return title.to_string();
    }
    let mut units = 0;
    let head: String = title
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units < LABEL_CHARS
        })
        .collect();
    format!("{}…", head.trim_end())
}

pub fn shown(graph: &Graph, sessions: bool) -> Shown {
    let mut nodes: Vec<ShownNode> = graph
        .nodes
        .iter()
        .filter(|n| sessions || n.source_type != "transcript")
        .map(|n| ShownNode {
            id: n.id.clone(),
            title: n.title.clone(),
            kind: n.source_type.clone(),
            memory_kind: n.memory_kind.map(str::to_string),
            inbound: n.inbound,
        })
        .collect();
    nodes.extend(graph.entities.iter().map(|e| ShownNode {
        id: e.id.clone(),
        title: e.name.clone(),
        kind: "entity".to_string(),
        memory_kind: None,
        inbound: 0,
    }));
    nodes.extend(graph.tags.iter().map(|t| ShownNode {
        id: format!("tag:{t}"),
        title: format!("#{t}"),
        kind: "tag".to_string(),
        memory_kind: None,
        inbound: 0,
    }));
    let mut index: HashMap<&str, usize> = HashMap::new();
    for (ix, n) in nodes.iter().enumerate() {
        index.insert(n.id.as_str(), ix);
    }
    let edges: Vec<(usize, usize)> = graph
        .edges
        .iter()
        .filter_map(|e| {
            let target = if e.kind == "tagged" {
                format!("tag:{}", e.to)
            } else {
                e.to.clone()
            };
            Some((*index.get(e.from.as_str())?, *index.get(target.as_str())?))
        })
        .collect();
    Shown {
        nodes: nodes.into(),
        edges: edges.into(),
    }
}

pub fn document_count(graph: &Graph) -> usize {
    graph
        .nodes
        .iter()
        .filter(|n| n.source_type != "memory" && n.source_type != "project")
        .count()
}

pub fn hidden_sessions(graph: &Graph) -> usize {
    graph
        .nodes
        .iter()
        .filter(|n| n.source_type == "transcript")
        .count()
}

pub fn session_agents(graph: &Graph) -> Vec<String> {
    let mut names: Vec<String> = graph
        .nodes
        .iter()
        .filter_map(|n| n.agent.map(agent_name))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    sort_js(&mut names);
    names
}

pub fn sessions_label(hidden: usize, shown: bool) -> String {
    if shown {
        "Sessions shown".to_string()
    } else {
        format!("Sessions ({hidden} hidden)")
    }
}

pub fn legend_label(agents: &[String], key: &str) -> String {
    if key == "session" && agents.len() > 1 {
        format!("session ({})", agents.join(", "))
    } else {
        key.to_string()
    }
}

fn title_of(graph: &Graph, id: &str) -> String {
    graph
        .nodes
        .iter()
        .find(|n| n.id == id)
        .map_or_else(|| id.to_string(), |n| n.title.clone())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbour {
    pub outgoing: bool,
    pub title: String,
}

impl Neighbour {
    pub fn arrow(&self) -> &'static str {
        if self.outgoing {
            "→"
        } else {
            "←"
        }
    }
}

pub fn neighbours(graph: &Graph, selected: &str) -> Vec<Neighbour> {
    let mut seen = HashSet::new();
    graph
        .edges
        .iter()
        .filter(|e| e.kind != "mentions" && e.kind != "tagged")
        .filter(|e| e.from == selected || e.to == selected)
        .map(|e| {
            if e.from == selected {
                Neighbour {
                    outgoing: true,
                    title: title_of(graph, &e.to),
                }
            } else {
                Neighbour {
                    outgoing: false,
                    title: title_of(graph, &e.from),
                }
            }
        })
        .filter(|n| seen.insert((n.outgoing, n.title.clone())))
        .collect()
}

pub fn tags_of(graph: &Graph, selected: &str) -> Vec<String> {
    let mut tags: Vec<String> = graph
        .edges
        .iter()
        .filter(|e| e.kind == "tagged" && e.from == selected)
        .map(|e| e.to.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    sort_js(&mut tags);
    tags
}

pub fn mentions_of(graph: &Graph, selected: &str) -> Vec<String> {
    let name = |id: &str| {
        graph
            .entities
            .iter()
            .find(|e| e.id == id)
            .map_or_else(|| id.to_string(), |e| e.name.clone())
    };
    let mut names: Vec<String> = graph
        .edges
        .iter()
        .filter(|e| e.kind == "mentions" && e.from == selected)
        .map(|e| name(&e.to))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    sort_js(&mut names);
    names
}

pub fn project_memories(graph: &Graph, project: &str) -> usize {
    graph
        .edges
        .iter()
        .filter(|e| e.kind == "scoped-to" && e.to == project)
        .count()
}

pub fn selectable(graph: &Graph, id: &str) -> Option<Node> {
    graph.nodes.iter().find(|n| n.id == id).cloned()
}

pub fn wheel_delta(delta: Point<f64>, lines: bool) -> f64 {
    if lines {
        -delta.y * WHEEL_PER_LINE
    } else {
        -delta.y
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub k: f64,
    pub pan: Point<f64>,
    pub last_set_zoom: f64,
}

impl Default for View {
    fn default() -> View {
        View {
            k: 1.,
            pan: point(0., 0.),
            last_set_zoom: 1.,
        }
    }
}

fn half(size: Size<Pixels>) -> Point<f64> {
    point(f64::from(size.width) / 2., f64::from(size.height) / 2.)
}

impl View {
    pub fn translate(&self, size: Size<Pixels>) -> Point<f64> {
        let c = half(size);
        point(c.x + self.pan.x, c.y + self.pan.y)
    }

    pub fn to_screen(&self, p: Point<f64>, size: Size<Pixels>) -> Point<f64> {
        let t = self.translate(size);
        point(t.x + self.k * p.x, t.y + self.k * p.y)
    }

    pub fn to_graph(&self, s: Point<f64>, size: Size<Pixels>) -> Point<f64> {
        let t = self.translate(size);
        point((s.x - t.x) / self.k, (s.y - t.y) / self.k)
    }

    pub fn fit(&mut self, nodes: usize) {
        if self.k == self.last_set_zoom && nodes > 0 {
            let k = ZOOM_TO_NODES / (nodes as f64).cbrt();
            self.scale_about_centre(k);
            self.last_set_zoom = k;
        }
    }

    fn scale_about_centre(&mut self, k: f64) {
        let k = k.clamp(MIN_ZOOM, MAX_ZOOM);
        let centre = point(-self.pan.x / self.k, -self.pan.y / self.k);
        self.k = k;
        self.pan = point(-centre.x * k, -centre.y * k);
    }

    pub fn zoom_at(&mut self, at: Point<f64>, delta: f64, ctrl: bool, size: Size<Pixels>) {
        let scale = if ctrl { 10. } else { 1. };
        let k = (self.k * 2f64.powf(-delta * WHEEL_PER_PIXEL * scale)).clamp(MIN_ZOOM, MAX_ZOOM);
        if k == self.k {
            return;
        }
        let under = self.to_graph(at, size);
        self.k = k;
        let c = half(size);
        self.pan = point(at.x - under.x * k - c.x, at.y - under.y * k - c.y);
    }

    pub fn pan_by(&mut self, d: Point<f64>) {
        self.pan = point(self.pan.x + d.x, self.pan.y + d.y);
    }
}

pub fn node_at(
    view: &View,
    nodes: &[ShownNode],
    positions: &[Point<f32>],
    at: Point<f64>,
    size: Size<Pixels>,
) -> Option<usize> {
    (0..nodes.len().min(positions.len())).rev().find(|&ix| {
        let p = positions[ix];
        let s = view.to_screen(point(f64::from(p.x), f64::from(p.y)), size);
        let r = node_radius(nodes[ix].inbound) * view.k + 1.;
        (s.x - at.x).powi(2) + (s.y - at.y).powi(2) <= r * r
    })
}

#[cfg(test)]
mod tests {
    use gpui_kit::{point, px, size, Point};

    use super::{short_title, View, MAX_ZOOM, MIN_ZOOM};

    #[test]
    fn the_wheel_zooms_toward_the_pointer_by_d3_zooms_factor() {
        let canvas = size(px(1000.), px(800.));
        let mut view = View::default();
        view.fit(27);
        assert_eq!(view.k, 4. / 3.);
        let pointer = point(700., 150.);
        let under = view.to_graph(pointer, canvas);
        view.zoom_at(pointer, -100., false, canvas);
        assert!((view.k - 4. / 3. * 2f64.powf(0.2)).abs() < 1e-12);
        let still = view.to_graph(pointer, canvas);
        assert!((still.x - under.x).abs() < 1e-9 && (still.y - under.y).abs() < 1e-9);
        view.zoom_at(pointer, 100., true, canvas);
        assert!((view.k - 4. / 3. * 2f64.powf(0.2) * 2f64.powf(-2.)).abs() < 1e-12);
        view.zoom_at(pointer, -1e6, false, canvas);
        assert_eq!(view.k, MAX_ZOOM);
        view.zoom_at(pointer, 1e6, false, canvas);
        assert_eq!(view.k, MIN_ZOOM);
    }

    #[test]
    fn the_fit_follows_the_node_count_until_the_user_zooms() {
        let canvas = size(px(1000.), px(800.));
        let mut view = View::default();
        view.fit(8);
        assert_eq!(view.k, 2.);
        view.pan_by(point(10., -4.));
        view.fit(64);
        assert_eq!(view.k, 1.);
        assert_eq!(view.pan, point(5., -2.));
        view.zoom_at(point(0., 0.), -10., false, canvas);
        let zoomed = view.k;
        view.fit(8);
        assert_eq!(view.k, zoomed);
        assert_eq!(
            View::default().to_screen(Point::default(), canvas),
            point(500., 400.)
        );
    }

    #[test]
    fn long_titles_are_cut_at_thirty_two_characters() {
        assert_eq!(short_title("Short"), "Short");
        let exact = "a".repeat(32);
        assert_eq!(short_title(&exact), exact);
        assert_eq!(
            short_title("The ingest service runbook for the night shift"),
            "The ingest service runbook for…"
        );
    }
}

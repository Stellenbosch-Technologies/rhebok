use gpui_kit::{point, Point};

pub const ALPHA_MIN: f64 = 0.001;
pub const ALPHA_DECAY: f64 = 0.0228;
pub const VELOCITY_DECAY: f64 = 0.4;
pub const LINK_DISTANCE: f64 = 30.;
pub const CHARGE: f64 = -30.;
pub const THETA: f64 = 0.9;

const INITIAL_RADIUS: f64 = 10.;
const DISTANCE_MIN2: f64 = 1.;
const NONE: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct NodeSeed {
    pub at: Option<Point<f64>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lcg(u64);

impl Default for Lcg {
    fn default() -> Lcg {
        Lcg(1)
    }
}

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = (1_664_525 * self.0 + 1_013_904_223) % 4_294_967_296;
        self.0 as f64 / 4_294_967_296.
    }

    fn jiggle(&mut self) -> f64 {
        (self.next() - 0.5) * 1e-6
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Body {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    fixed: Option<(f64, f64)>,
}

struct Link {
    source: usize,
    target: usize,
    bias: f64,
    strength: f64,
}

#[derive(Clone, Copy)]
enum Cell {
    Branch([u32; 4]),
    Leaf { body: u32, next: u32 },
}

#[derive(Clone, Copy)]
struct Quad {
    cell: Cell,
    x: f64,
    y: f64,
    value: f64,
}

#[derive(Default)]
struct Quadtree {
    quads: Vec<Quad>,
    root: u32,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Quadtree {
    fn leaf(&mut self, body: usize) -> u32 {
        self.quads.push(Quad {
            cell: Cell::Leaf {
                body: body as u32,
                next: NONE,
            },
            x: 0.,
            y: 0.,
            value: 0.,
        });
        (self.quads.len() - 1) as u32
    }

    fn branch(&mut self) -> u32 {
        self.quads.push(Quad {
            cell: Cell::Branch([NONE; 4]),
            x: 0.,
            y: 0.,
            value: 0.,
        });
        (self.quads.len() - 1) as u32
    }

    fn set_child(&mut self, parent: u32, slot: usize, child: u32) {
        if let Cell::Branch(children) = &mut self.quads[parent as usize].cell {
            children[slot] = child;
        }
    }

    fn build(&mut self, bodies: &[Body]) {
        self.quads.clear();
        self.root = NONE;
        let (mut x0, mut y0, mut x1, mut y1) =
            (f64::INFINITY, f64::INFINITY, -f64::INFINITY, -f64::INFINITY);
        for b in bodies {
            x0 = x0.min(b.x);
            x1 = x1.max(b.x);
            y0 = y0.min(b.y);
            y1 = y1.max(b.y);
        }
        if x0 > x1 || y0 > y1 {
            return;
        }
        self.x0 = x0.floor();
        self.y0 = y0.floor();
        self.x1 = self.x0 + 1.;
        self.y1 = self.y0 + 1.;
        self.cover(x1, y1);
        for i in 0..bodies.len() {
            self.add(bodies, i);
        }
    }

    fn cover(&mut self, x: f64, y: f64) {
        let (mut x0, mut y0, mut x1, mut y1) = (self.x0, self.y0, self.x1, self.y1);
        let mut z = if x1 - x0 != 0. { x1 - x0 } else { 1. };
        while x0 > x || x >= x1 || y0 > y || y >= y1 {
            let i = (usize::from(y < y0) << 1) | usize::from(x < x0);
            z *= 2.;
            match i {
                0 => {
                    x1 = x0 + z;
                    y1 = y0 + z;
                }
                1 => {
                    x0 = x1 - z;
                    y1 = y0 + z;
                }
                2 => {
                    x1 = x0 + z;
                    y0 = y1 - z;
                }
                _ => {
                    x0 = x1 - z;
                    y0 = y1 - z;
                }
            }
        }
        (self.x0, self.y0, self.x1, self.y1) = (x0, y0, x1, y1);
    }

    fn add(&mut self, bodies: &[Body], body: usize) {
        let (x, y) = (bodies[body].x, bodies[body].y);
        let leaf = self.leaf(body);
        if self.root == NONE {
            self.root = leaf;
            return;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (self.x0, self.y0, self.x1, self.y1);
        let mut parent = NONE;
        let mut node = self.root;
        let mut i = 0;
        while let Cell::Branch(children) = self.quads[node as usize].cell {
            let xm = (x0 + x1) / 2.;
            let right = x >= xm;
            if right {
                x0 = xm;
            } else {
                x1 = xm;
            }
            let ym = (y0 + y1) / 2.;
            let bottom = y >= ym;
            if bottom {
                y0 = ym;
            } else {
                y1 = ym;
            }
            parent = node;
            i = (usize::from(bottom) << 1) | usize::from(right);
            node = children[i];
            if node == NONE {
                self.set_child(parent, i, leaf);
                return;
            }
        }
        let Cell::Leaf { body: other, .. } = self.quads[node as usize].cell else {
            return;
        };
        let (xp, yp) = (bodies[other as usize].x, bodies[other as usize].y);
        if x == xp && y == yp {
            self.quads[leaf as usize].cell = Cell::Leaf {
                body: body as u32,
                next: node,
            };
            if parent == NONE {
                self.root = leaf;
            } else {
                self.set_child(parent, i, leaf);
            }
            return;
        }
        let j = loop {
            let fresh = self.branch();
            if parent == NONE {
                self.root = fresh;
            } else {
                self.set_child(parent, i, fresh);
            }
            parent = fresh;
            let xm = (x0 + x1) / 2.;
            let right = x >= xm;
            if right {
                x0 = xm;
            } else {
                x1 = xm;
            }
            let ym = (y0 + y1) / 2.;
            let bottom = y >= ym;
            if bottom {
                y0 = ym;
            } else {
                y1 = ym;
            }
            i = (usize::from(bottom) << 1) | usize::from(right);
            let j = (usize::from(yp >= ym) << 1) | usize::from(xp >= xm);
            if i != j {
                break j;
            }
        };
        self.set_child(parent, j, node);
        self.set_child(parent, i, leaf);
    }

    fn accumulate(&mut self, bodies: &[Body], strengths: &[f64]) {
        if self.root == NONE {
            return;
        }
        let mut stack = vec![self.root];
        let mut order = Vec::with_capacity(self.quads.len());
        while let Some(q) = stack.pop() {
            if let Cell::Branch(children) = self.quads[q as usize].cell {
                stack.extend(children.into_iter().filter(|&c| c != NONE));
            }
            order.push(q);
        }
        for &q in order.iter().rev() {
            let quad = self.quads[q as usize];
            match quad.cell {
                Cell::Branch(children) => {
                    let (mut strength, mut weight, mut x, mut y) = (0., 0., 0., 0.);
                    for c in children.into_iter().filter(|&c| c != NONE) {
                        let child = self.quads[c as usize];
                        let w = child.value.abs();
                        if w != 0. {
                            strength += child.value;
                            weight += w;
                            x += w * child.x;
                            y += w * child.y;
                        }
                    }
                    let quad = &mut self.quads[q as usize];
                    quad.x = x / weight;
                    quad.y = y / weight;
                    quad.value = strength;
                }
                Cell::Leaf { body, .. } => {
                    let mut strength = 0.;
                    let mut link = q;
                    while link != NONE {
                        let Cell::Leaf { body, next } = self.quads[link as usize].cell else {
                            break;
                        };
                        strength += strengths[body as usize];
                        link = next;
                    }
                    let quad = &mut self.quads[q as usize];
                    quad.x = bodies[body as usize].x;
                    quad.y = bodies[body as usize].y;
                    quad.value = strength;
                }
            }
        }
    }
}

pub struct Layout {
    bodies: Vec<Body>,
    links: Vec<Link>,
    strengths: Vec<f64>,
    positions: Vec<Point<f32>>,
    alpha: f64,
    alpha_target: f64,
    running: bool,
    random: Lcg,
    tree: Quadtree,
}

impl Layout {
    pub fn new(nodes: &[NodeSeed], edges: &[(usize, usize)]) -> Layout {
        Layout::with_random(nodes, edges, Lcg::default())
    }

    pub fn with_random(nodes: &[NodeSeed], edges: &[(usize, usize)], random: Lcg) -> Layout {
        let roll = std::f64::consts::PI * (3. - 5f64.sqrt());
        let bodies: Vec<Body> = nodes
            .iter()
            .enumerate()
            .map(|(i, seed)| {
                let (x, y) = match seed.at {
                    Some(p) => (p.x, p.y),
                    None => {
                        let radius = INITIAL_RADIUS * (0.5 + i as f64).sqrt();
                        let angle = i as f64 * roll;
                        (radius * angle.cos(), radius * angle.sin())
                    }
                };
                Body {
                    x,
                    y,
                    ..Body::default()
                }
            })
            .collect();
        let edges: Vec<(usize, usize)> = edges
            .iter()
            .copied()
            .filter(|&(a, b)| a < bodies.len() && b < bodies.len())
            .collect();
        let mut count = vec![0usize; bodies.len()];
        for &(a, b) in &edges {
            count[a] += 1;
            count[b] += 1;
        }
        let links = edges
            .iter()
            .map(|&(source, target)| Link {
                source,
                target,
                bias: count[source] as f64 / (count[source] + count[target]) as f64,
                strength: 1. / count[source].min(count[target]) as f64,
            })
            .collect();
        let mut layout = Layout {
            strengths: vec![CHARGE; bodies.len()],
            positions: Vec::with_capacity(bodies.len()),
            bodies,
            links,
            alpha: 1.,
            alpha_target: 0.,
            running: true,
            random,
            tree: Quadtree::default(),
        };
        layout.publish();
        layout
    }

    pub fn step(&mut self) -> bool {
        if !self.running {
            return false;
        }
        self.tick();
        if self.alpha < ALPHA_MIN {
            self.running = false;
        }
        self.running
    }

    pub fn positions(&self) -> &[Point<f32>] {
        &self.positions
    }

    pub fn precise(&self, i: usize) -> Point<f64> {
        point(self.bodies[i].x, self.bodies[i].y)
    }

    pub fn alpha(&self) -> f64 {
        self.alpha
    }

    pub fn random(&self) -> Lcg {
        self.random
    }

    pub fn warm(&self) -> bool {
        self.running
    }

    pub fn alpha_target(&self) -> f64 {
        self.alpha_target
    }

    pub fn set_alpha_target(&mut self, target: f64) {
        self.alpha_target = target;
        self.running = true;
    }

    pub fn pin(&mut self, i: usize, p: Point<f64>) {
        let body = &mut self.bodies[i];
        body.fixed = Some((p.x, p.y));
        body.x = p.x;
        body.y = p.y;
        self.positions[i] = point(p.x as f32, p.y as f32);
    }

    pub fn unpin(&mut self, i: usize) {
        self.bodies[i].fixed = None;
    }

    fn publish(&mut self) {
        self.positions.clear();
        self.positions
            .extend(self.bodies.iter().map(|b| point(b.x as f32, b.y as f32)));
    }

    fn tick(&mut self) {
        self.alpha += (self.alpha_target - self.alpha) * ALPHA_DECAY;
        self.link_force();
        self.charge_force();
        self.center_force();
        let decay = 1. - VELOCITY_DECAY;
        for body in &mut self.bodies {
            match body.fixed {
                None => {
                    body.vx *= decay;
                    body.x += body.vx;
                    body.vy *= decay;
                    body.y += body.vy;
                }
                Some((fx, fy)) => {
                    body.x = fx;
                    body.vx = 0.;
                    body.y = fy;
                    body.vy = 0.;
                }
            }
        }
        self.publish();
    }

    fn link_force(&mut self) {
        let alpha = self.alpha;
        for link in &self.links {
            let (s, t) = (self.bodies[link.source], self.bodies[link.target]);
            let mut x = t.x + t.vx - s.x - s.vx;
            if x == 0. || x.is_nan() {
                x = self.random.jiggle();
            }
            let mut y = t.y + t.vy - s.y - s.vy;
            if y == 0. || y.is_nan() {
                y = self.random.jiggle();
            }
            let mut l = (x * x + y * y).sqrt();
            l = (l - LINK_DISTANCE) / l * alpha * link.strength;
            x *= l;
            y *= l;
            let b = link.bias;
            let target = &mut self.bodies[link.target];
            target.vx -= x * b;
            target.vy -= y * b;
            let b = 1. - b;
            let source = &mut self.bodies[link.source];
            source.vx += x * b;
            source.vy += y * b;
        }
    }

    fn charge_force(&mut self) {
        let mut tree = std::mem::take(&mut self.tree);
        tree.build(&self.bodies);
        tree.accumulate(&self.bodies, &self.strengths);
        if tree.root != NONE {
            let theta2 = THETA * THETA;
            let mut stack: Vec<(u32, f64, f64, f64, f64)> = Vec::new();
            for i in 0..self.bodies.len() {
                stack.clear();
                stack.push((tree.root, tree.x0, tree.y0, tree.x1, tree.y1));
                while let Some((q, x0, y0, x1, y1)) = stack.pop() {
                    if !self.apply(&tree, i, q, x1 - x0, theta2) {
                        if let Cell::Branch(children) = tree.quads[q as usize].cell {
                            let xm = (x0 + x1) / 2.;
                            let ym = (y0 + y1) / 2.;
                            let quads = [
                                (children[3], xm, ym, x1, y1),
                                (children[2], x0, ym, xm, y1),
                                (children[1], xm, y0, x1, ym),
                                (children[0], x0, y0, xm, ym),
                            ];
                            stack.extend(quads.into_iter().filter(|q| q.0 != NONE));
                        }
                    }
                }
            }
        }
        self.tree = tree;
    }

    fn apply(&mut self, tree: &Quadtree, i: usize, q: u32, w: f64, theta2: f64) -> bool {
        let quad = tree.quads[q as usize];
        if quad.value == 0. {
            return true;
        }
        let alpha = self.alpha;
        let node = self.bodies[i];
        let mut x = quad.x - node.x;
        let mut y = quad.y - node.y;
        let mut l = x * x + y * y;
        if w * w / theta2 < l {
            if x == 0. {
                x = self.random.jiggle();
                l += x * x;
            }
            if y == 0. {
                y = self.random.jiggle();
                l += y * y;
            }
            if l < DISTANCE_MIN2 {
                l = (DISTANCE_MIN2 * l).sqrt();
            }
            let body = &mut self.bodies[i];
            body.vx += x * quad.value * alpha / l;
            body.vy += y * quad.value * alpha / l;
            return true;
        }
        let Cell::Leaf { body: head, next } = quad.cell else {
            return false;
        };
        if head as usize != i || next != NONE {
            if x == 0. {
                x = self.random.jiggle();
                l += x * x;
            }
            if y == 0. {
                y = self.random.jiggle();
                l += y * y;
            }
            if l < DISTANCE_MIN2 {
                l = (DISTANCE_MIN2 * l).sqrt();
            }
        }
        let mut link = q;
        while link != NONE {
            let Cell::Leaf { body, next } = tree.quads[link as usize].cell else {
                break;
            };
            if body as usize != i {
                let w = self.strengths[body as usize] * alpha / l;
                let node = &mut self.bodies[i];
                node.vx += x * w;
                node.vy += y * w;
            }
            link = next;
        }
        true
    }

    fn center_force(&mut self) {
        let n = self.bodies.len();
        if n == 0 {
            return;
        }
        let (mut sx, mut sy) = (0., 0.);
        for body in &self.bodies {
            sx += body.x;
            sy += body.y;
        }
        let sx = sx / n as f64;
        let sy = sy / n as f64;
        for body in &mut self.bodies {
            if sx != 0. {
                body.x -= sx;
            }
            if sy != 0. {
                body.y -= sy;
            }
        }
    }
}

#[cfg(test)]
mod golden;

#[cfg(test)]
mod tests {
    use gpui_kit::{point, Point};

    use super::golden::D3_SMALL_300;
    use super::{Layout, NodeSeed};

    fn lcg_graph(n: usize, m: usize, seed: u32) -> Vec<(usize, usize)> {
        let mut s = seed;
        let mut next = || {
            s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            f64::from(s) / 4_294_967_296.
        };
        (0..m)
            .map(|_| {
                let a = (next() * n as f64).floor() as usize;
                let b = (next() * n as f64).floor() as usize;
                (a, b)
            })
            .collect()
    }

    fn seeds(n: usize) -> Vec<NodeSeed> {
        vec![NodeSeed::default(); n]
    }

    fn distance(a: Point<f32>, b: Point<f32>) -> f32 {
        ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
    }

    #[test]
    fn the_layout_cools_and_stops() {
        let edges = lcg_graph(50, 80, 3);
        let mut layout = Layout::new(&seeds(50), &edges);
        let mut steps = 0;
        while layout.step() {
            steps += 1;
            assert!(steps < 400, "still warm after 400 steps");
        }
        assert!(!layout.warm());
        assert!(!layout.step(), "a cooled layout does not tick again");
        assert!(layout
            .positions()
            .iter()
            .all(|p| p.x.is_finite() && p.y.is_finite()));
        assert!(layout.positions().iter().any(|p| p.x != 0. || p.y != 0.));
    }

    #[test]
    fn linked_nodes_end_closer_than_unlinked_ones() {
        let n = 24;
        let edges: Vec<(usize, usize)> = (0..n / 2).map(|i| (2 * i, 2 * i + 1)).collect();
        let mut layout = Layout::new(&seeds(n), &edges);
        while layout.step() {}
        let p = layout.positions();
        let linked: f32 = edges
            .iter()
            .map(|&(a, b)| distance(p[a], p[b]))
            .sum::<f32>()
            / edges.len() as f32;
        let mut unlinked = Vec::new();
        for a in 0..n {
            for b in a + 1..n {
                if !edges.contains(&(a, b)) {
                    unlinked.push(distance(p[a], p[b]));
                }
            }
        }
        let unlinked = unlinked.iter().sum::<f32>() / unlinked.len() as f32;
        assert!(
            linked * 2. < unlinked,
            "linked pairs average {linked}, unlinked {unlinked}"
        );
    }

    #[test]
    fn the_same_seed_gives_the_same_layout() {
        let edges = lcg_graph(40, 60, 11);
        let run = || {
            let mut layout = Layout::new(&seeds(40), &edges);
            while layout.step() {}
            layout.positions().to_vec()
        };
        let first = run();
        assert_eq!(first, run());
        let mut moved = seeds(40);
        moved[0].at = Some(point(500., 500.));
        let mut other = Layout::new(&moved, &edges);
        while other.step() {}
        assert_ne!(first, other.positions().to_vec());
    }

    #[test]
    fn the_layout_matches_d3_force_3d_tick_for_tick() {
        let edges = lcg_graph(50, 80, 3);
        let mut layout = Layout::new(&seeds(50), &edges);
        for _ in 0..300 {
            layout.step();
        }
        for (i, &(x, y)) in D3_SMALL_300.iter().enumerate() {
            let p = layout.precise(i);
            assert!(
                (p.x - x).abs() < 1e-9 && (p.y - y).abs() < 1e-9,
                "node {i} is at ({}, {}), d3-force-3d put it at ({x}, {y}); the golden comes from `node app/tests/golden/d3_force.mjs > app/src/graph/layout/golden.rs`",
                p.x,
                p.y
            );
        }
        assert!(layout.alpha() < 0.001 && layout.alpha() > 0.000_98);
    }

    #[test]
    fn a_pinned_node_holds_its_place_until_released() {
        let edges = lcg_graph(30, 40, 5);
        let mut layout = Layout::new(&seeds(30), &edges);
        layout.set_alpha_target(0.3);
        layout.pin(4, point(120., -80.));
        for _ in 0..50 {
            assert!(layout.step(), "a dragged layout stays warm");
            assert_eq!(layout.positions()[4], point(120., -80.));
        }
        layout.set_alpha_target(0.);
        layout.unpin(4);
        layout.step();
        assert_ne!(layout.positions()[4], point(120., -80.));
        let mut steps = 0;
        while layout.step() {
            steps += 1;
            assert!(steps < 1000);
        }
    }
}

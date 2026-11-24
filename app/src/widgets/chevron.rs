use gpui_kit::*;

pub const BOX_WIDTH: f32 = 8.141;
pub const BOX_HEIGHT: f32 = 15.;
const POINTING_RIGHT: [(f32, f32); 3] = [(1.3, 3.3), (1.3, 9.5), (7.75, 6.4)];

pub fn chevron_points(expanded: bool) -> [(f32, f32); 3] {
    let (cx, cy) = (BOX_WIDTH / 2., BOX_HEIGHT / 2.);
    POINTING_RIGHT.map(|(x, y)| {
        if expanded {
            (cx - (y - cy), cy + (x - cx))
        } else {
            (x, y)
        }
    })
}

pub fn chevron(expanded: bool, colour: Hsla) -> Div {
    let points = chevron_points(expanded);
    div().flex_none().w(px(BOX_WIDTH)).h(px(BOX_HEIGHT)).child(
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let at =
                    |(x, y): (f32, f32)| point(bounds.origin.x + px(x), bounds.origin.y + px(y));
                let mut path = PathBuilder::fill();
                path.move_to(at(points[0]));
                path.line_to(at(points[1]));
                path.line_to(at(points[2]));
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, colour);
                }
            },
        )
        .size_full(),
    )
}

pub const MARKER_WIDTH: f32 = 14.83;
const DISCLOSURE_CLOSED: [(f32, f32); 3] = [(0., 4.9), (0., 13.75), (8.1, 9.3)];
const DISCLOSURE_OPEN: [(f32, f32); 3] = [(0., 5.2), (9.35, 5.2), (4.7, 13.4)];

pub fn disclosure_points(open: bool) -> [(f32, f32); 3] {
    if open {
        DISCLOSURE_OPEN
    } else {
        DISCLOSURE_CLOSED
    }
}

pub fn disclosure(open: bool, colour: Hsla, line_height: f32) -> Div {
    let points = disclosure_points(open);
    div()
        .flex_none()
        .w(px(MARKER_WIDTH))
        .h(px(line_height))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let at = |(x, y): (f32, f32)| {
                        point(bounds.origin.x + px(x), bounds.origin.y + px(y))
                    };
                    let mut path = PathBuilder::fill();
                    path.move_to(at(points[0]));
                    path.line_to(at(points[1]));
                    path.line_to(at(points[2]));
                    path.close();
                    if let Ok(path) = path.build() {
                        window.paint_path(path, colour);
                    }
                },
            )
            .size_full(),
        )
}

use gpui_kit::*;

pub const WINDOW_TITLE: &str = "Rhebok";
pub const HEADER_HEIGHT: f32 = 56.;
pub const HEADER_INSET: f32 = 24.;
pub const TRAFFIC_LIGHT_LEFT: f32 = 20.;
pub const TRAFFIC_LIGHT_SIZE: f32 = 14.;
pub const TRAFFIC_LIGHTS_WIDTH: f32 = 60.;
pub const HEADER_INSET_BESIDE_TRAFFIC_LIGHTS: f32 =
    TRAFFIC_LIGHT_LEFT + TRAFFIC_LIGHTS_WIDTH + HEADER_INSET;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Chrome {
    #[default]
    Plain,
    TrafficLights,
}

impl Chrome {
    pub fn header_inset(self) -> f32 {
        match self {
            Chrome::Plain => HEADER_INSET,
            Chrome::TrafficLights => HEADER_INSET_BESIDE_TRAFFIC_LIGHTS,
        }
    }
}

pub fn titlebar() -> TitlebarOptions {
    TitlebarOptions {
        title: Some(WINDOW_TITLE.into()),
        appears_transparent: true,
        traffic_light_position: Some(point(
            px(TRAFFIC_LIGHT_LEFT),
            px((HEADER_HEIGHT - TRAFFIC_LIGHT_SIZE) / 2.),
        )),
    }
}

pub fn window_options(bounds: Bounds<Pixels>) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(1024.), px(700.))),
        titlebar: Some(titlebar()),
        app_owns_titlebar_drag: true,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{px, size, Bounds, WindowBounds};

    use super::{
        titlebar, window_options, Chrome, HEADER_HEIGHT, HEADER_INSET, TRAFFIC_LIGHTS_WIDTH,
        TRAFFIC_LIGHT_LEFT, TRAFFIC_LIGHT_SIZE,
    };

    #[test]
    fn the_title_bar_is_transparent_and_keeps_the_window_title() {
        let options = titlebar();
        assert!(options.appears_transparent);
        assert_eq!(options.title.as_deref(), Some("Rhebok"));
    }

    #[test]
    fn the_app_alone_handles_drags_and_double_clicks_in_the_title_bar() {
        let bounds = Bounds::new(Default::default(), size(px(1400.), px(900.)));
        let options = window_options(bounds);
        assert!(options.app_owns_titlebar_drag);
        let titlebar = options.titlebar.expect("titlebar");
        assert!(titlebar.appears_transparent);
        assert_eq!(titlebar.title.as_deref(), Some("Rhebok"));
        assert_eq!(options.window_bounds, Some(WindowBounds::Windowed(bounds)));
    }

    #[test]
    fn the_traffic_lights_sit_centred_on_the_header_row() {
        let position = titlebar().traffic_light_position.expect("position");
        let top = f32::from(position.y);
        assert_eq!(top + TRAFFIC_LIGHT_SIZE + top, HEADER_HEIGHT);
        assert_eq!(f32::from(position.x), TRAFFIC_LIGHT_LEFT);
    }

    #[test]
    fn the_header_clears_the_traffic_lights_only_in_a_window_that_has_them() {
        let lights_end = TRAFFIC_LIGHT_LEFT + TRAFFIC_LIGHTS_WIDTH;
        assert_eq!(
            Chrome::TrafficLights.header_inset(),
            lights_end + HEADER_INSET
        );
        assert_eq!(Chrome::Plain.header_inset(), HEADER_INSET);
        assert_eq!(Chrome::default(), Chrome::Plain);
    }
}

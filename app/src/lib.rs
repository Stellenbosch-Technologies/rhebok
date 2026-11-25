pub mod app_state;
pub mod backend;
pub mod chrome;
pub mod dock_icon;
pub mod graph;
pub mod onboarding;
pub mod relaunch;
pub mod shell;
#[cfg(feature = "shoot")]
pub mod shoot;
pub mod tabs;
pub mod theme;
pub mod version_gate;
pub mod widgets;

pub fn version_line() -> String {
    format!("rhebok-app {}", rhebok::VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_names_the_app_and_the_shared_version() {
        assert_eq!(version_line(), format!("rhebok-app {}", rhebok::VERSION));
    }
}

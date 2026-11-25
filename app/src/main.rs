use std::sync::Arc;

use gpui_kit::*;
use rhebok_app::backend::{self, CoreBackend, RetryingReads};
use rhebok_app::chrome::{self, Chrome};
use rhebok_app::shell::Shell;
use rhebok_app::{app_state, dock_icon, theme, version_gate};

fn main() {
    if std::env::args().nth(1).is_some_and(|a| a == "--version") {
        println!("{}", rhebok_app::version_line());
        return;
    }
    #[cfg(feature = "shoot")]
    {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args
            .first()
            .is_some_and(|a| a == "--screenshot" || a == "--side-by-side" || a == "--measure-graph")
        {
            if let Err(e) = rhebok_app::shoot::run(&args) {
                eprintln!("rhebok-app: {e}");
                std::process::exit(1);
            }
            return;
        }
    }
    #[cfg(not(feature = "shoot"))]
    if std::env::args()
        .nth(1)
        .is_some_and(|a| a == "--screenshot" || a == "--side-by-side" || a == "--measure-graph")
    {
        eprintln!("rhebok-app was built without screenshot support; rebuild with --features shoot");
        std::process::exit(2);
    }
    let gate = version_gate::check(&version_gate::installed_cli(), rhebok::VERSION);
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx: &mut App| {
            let icon = cx.background_spawn(async { dock_icon::png() });
            theme::install(cx);
            if gate == version_gate::Gate::Ok {
                backend::install(cx, Arc::new(RetryingReads::new(CoreBackend::from_env())));
                app_state::install(cx, app_state::default_path());
            }
            cx.set_quit_mode(QuitMode::LastWindowClosed);
            let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);
            gpui_kit::open_window(chrome::window_options(bounds), cx, move |window, cx| {
                window.on_next_frame(move |_, cx| {
                    cx.spawn(async move |_| dock_icon::install(&icon.await))
                        .detach();
                });
                cx.new(|cx| Shell::new(gate, cx).with_chrome(Chrome::TrafficLights))
            })
            .expect("open window");
            cx.activate(true);
        });
}

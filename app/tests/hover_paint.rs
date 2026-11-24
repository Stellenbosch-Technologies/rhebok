use std::sync::Arc;

use gpui_kit::assets::Assets;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    div, px, size, AnyWindowHandle, AppContext, Bounds, Context, HeadlessAppContext, IntoElement,
    ParentElement, Render, Rgba, Styled, Window, WindowBounds, WindowOptions,
};
use rhebok_app::tabs::settings::list_editor::remove_button;
use rhebok_app::theme::{self, tokens};

struct Remove;

impl Render for Remove {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *tokens(cx);
        div()
            .size_full()
            .bg(t.surface)
            .p(px(20.))
            .child(remove_button("remove", &t))
    }
}

fn reddest(cx: &mut HeadlessAppContext, window: AnyWindowHandle) -> f32 {
    cx.capture_screenshot(window)
        .expect("capture")
        .pixels()
        .map(|p| f32::from(p.0[0]) - f32::from(p.0[1]))
        .fold(0., f32::max)
}

fn the_remove_button_turns_warn_while_the_pointer_is_over_it() {
    let mut cx = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        Arc::new(Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(theme::install);
    let window: AnyWindowHandle = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(120.), px(60.)),
                    })),
                    focus: false,
                    show: false,
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| Remove),
            )
        })
        .expect("open window")
        .0;
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .expect("render");
    let resting = reddest(&mut cx, window);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("remove", cx);
        window.render_frame(cx);
    })
    .expect("hover");
    let hovered = reddest(&mut cx, window);
    let warn = Rgba::from(theme::Tokens::light().warn);
    let warn_red = (warn.r - warn.g) * 255.;
    assert!(resting < 20., "the resting label is already red: {resting}");
    assert!(
        hovered > warn_red * 0.6,
        "the hovered label is not warn: {hovered} against {warn_red}"
    );
}

fn main() {
    the_remove_button_turns_warn_while_the_pointer_is_over_it();
    println!("test the_remove_button_turns_warn_while_the_pointer_is_over_it ... ok");
}

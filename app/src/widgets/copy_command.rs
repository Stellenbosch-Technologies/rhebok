use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::widgets::baseline;
use crate::widgets::css_box::css_box;
use crate::widgets::exact_text;

use crate::theme::{tokens, Type};

#[derive(IntoElement)]
pub struct CopyCommand {
    id: ElementId,
    command: SharedString,
    install_fix: bool,
}

pub fn copy_command(id: impl Into<ElementId>, command: impl Into<SharedString>) -> CopyCommand {
    CopyCommand {
        id: id.into(),
        command: command.into(),
        install_fix: false,
    }
}

pub fn copy_label(copied: bool) -> &'static str {
    if copied {
        "copied"
    } else {
        "copy"
    }
}

impl CopyCommand {
    pub fn install_fix(mut self) -> Self {
        self.install_fix = true;
        self
    }
}

impl RenderOnce for CopyCommand {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *tokens(cx);
        let copied = window.use_keyed_state(self.id.clone(), cx, |_, _| false);
        let label = copy_label(*copied.read(cx));
        let command = self.command.clone();
        let button_id = ElementId::Name(format!("{}-copy", self.id).into());
        let code = div()
            .relative()
            .mono()
            .size_xs()
            .child(css_box(t.sunken, None, 4.))
            .map(|d| {
                if self.install_fix {
                    d.px(px(8.)).py(px(4.)).min_w(px(0.)).child(
                        div()
                            .min_w(px(0.))
                            .truncate()
                            .child(baseline::text(self.command.clone())),
                    )
                } else {
                    d.px(px(6.))
                        .py(px(2.))
                        .child(baseline::text(self.command.clone()))
                }
            });
        div()
            .id(self.id.clone())
            .test_support()
            .aria_label(self.command.clone())
            .flex()
            .items_center()
            .gap(px(8.))
            .when(self.install_fix, |d| d.min_w(px(0.)))
            .when(!self.install_fix, |d| d.flex_none())
            .child(code)
            .child(
                div()
                    .id(button_id)
                    .test_support()
                    .role(Role::Button)
                    .aria_label(label)
                    .flex_none()
                    .size_xs()
                    .semibold()
                    .text_color(t.accent)
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(command.to_string()));
                        copied.update(cx, |copied, cx| {
                            *copied = true;
                            cx.notify();
                        });
                    })
                    .child(exact_text(label)),
            )
    }
}

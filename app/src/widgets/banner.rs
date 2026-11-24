use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::widgets::baseline;

use crate::theme::{tokens, Type};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Accent,
    Warn,
}

#[derive(IntoElement)]
pub struct Banner {
    id: ElementId,
    message: SharedString,
    tone: Tone,
    compact: bool,
    style: StyleRefinement,
    children: Vec<AnyElement>,
}

pub fn error(id: impl Into<ElementId>, message: impl Into<SharedString>) -> Banner {
    notice(id, message, Tone::Warn)
}

pub fn notice(id: impl Into<ElementId>, message: impl Into<SharedString>, tone: Tone) -> Banner {
    Banner {
        id: id.into(),
        message: message.into(),
        tone,
        compact: false,
        style: StyleRefinement::default(),
        children: Vec::new(),
    }
}

impl Banner {
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }
}

impl Styled for Banner {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Banner {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Banner {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = tokens(cx);
        let (bg, fg) = match self.tone {
            Tone::Accent => (t.accent_soft, t.accent),
            Tone::Warn => (t.warn_soft, t.warn),
        };
        let role = match self.tone {
            Tone::Accent => Role::Status,
            Tone::Warn => Role::Alert,
        };
        let mut banner = div()
            .id(self.id)
            .bg(bg)
            .text_color(fg)
            .rounded(px(6.))
            .size_sm()
            .map(|d| {
                if self.compact {
                    d.px(px(12.)).py(px(8.))
                } else {
                    d.px(px(16.)).py(px(10.))
                }
            });
        banner.style().refine(&self.style);
        banner
            .test_support()
            .role(role)
            .aria_label(self.message.clone())
            .when(!self.message.is_empty(), |d| {
                d.child(baseline::text(self.message))
            })
            .children(self.children)
    }
}

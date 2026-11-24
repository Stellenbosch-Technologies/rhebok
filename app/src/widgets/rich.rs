use std::ops::Range;

use gpui_kit::*;

use crate::theme::{Face, MONO, SANS};
use crate::widgets::baseline::{with_chrome_breaks, without_joiners};

#[derive(Default)]
pub struct Rich {
    text: String,
    runs: Vec<TextRun>,
    links: Vec<Range<usize>>,
}

fn face(family: &str, weight: FontWeight) -> Font {
    Font {
        weight,
        ..font(SharedString::from(family.to_string()))
    }
}

impl Rich {
    pub fn new() -> Rich {
        Rich::default()
    }

    fn run(mut self, text: &str, font: Font, color: Hsla, underline: bool) -> Self {
        if text.is_empty() {
            return self;
        }
        let text = with_chrome_breaks(text);
        self.text.push_str(&text);
        self.runs.push(TextRun {
            len: text.len(),
            font,
            color,
            background_color: None,
            underline: underline.then(|| UnderlineStyle {
                thickness: px(1.),
                color: Some(color),
                wavy: false,
            }),
            strikethrough: None,
        });
        self
    }

    pub fn push(
        self,
        text: impl AsRef<str>,
        family: &str,
        weight: FontWeight,
        color: Hsla,
    ) -> Self {
        self.run(text.as_ref(), face(family, weight), color, false)
    }

    pub fn link(mut self, text: impl AsRef<str>, family: &str, color: Hsla) -> Self {
        let start = self.text.len();
        self = self.run(text.as_ref(), face(family, FontWeight::NORMAL), color, true);
        self.links.push(start..self.text.len());
        self
    }

    pub fn text(&self) -> SharedString {
        without_joiners(&self.text).into()
    }

    pub fn links(&self) -> Vec<Range<usize>> {
        self.links.clone()
    }

    pub fn faces(&self, size: f32, line_height: f32) -> Vec<Face> {
        let mut faces: Vec<Face> = Vec::new();
        for run in &self.runs {
            let family = if run.font.family.as_ref() == MONO {
                MONO
            } else {
                SANS
            };
            let face = Face::new(family, run.font.weight, size, line_height);
            if !faces.contains(&face) {
                faces.push(face);
            }
        }
        faces
    }

    pub fn max_content_width(&self, window: &Window, size: f32) -> Pixels {
        window
            .text_system()
            .layout_line(&self.text, px(size), &self.runs, None)
            .width
    }

    pub fn styled(self) -> StyledText {
        StyledText::new(self.text).with_runs(self.runs)
    }
}

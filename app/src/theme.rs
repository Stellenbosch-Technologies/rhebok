use std::borrow::Cow;

use gpui_kit::component::Theme;
use gpui_kit::*;

pub const SANS: &str = "Source Sans 3";
pub const MONO: &str = "IBM Plex Mono";
pub const HELVETICA: &str = "Helvetica";

const SANS_FONTS: [&[u8]; 3] = [
    include_bytes!("../assets/fonts/SourceSans3-Regular.ttf"),
    include_bytes!("../assets/fonts/SourceSans3-Medium.ttf"),
    include_bytes!("../assets/fonts/SourceSans3-Semibold.ttf"),
];

const MONO_FONTS: [&[u8]; 2] = [
    include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerticalMetrics {
    pub ascender: i16,
    pub descender: i16,
    pub line_gap: i16,
    pub win_ascent: u16,
    pub win_descent: u16,
}

pub const WEB_SANS_METRICS: VerticalMetrics = VerticalMetrics {
    ascender: 1024,
    descender: -400,
    line_gap: 0,
    win_ascent: 934,
    win_descent: 288,
};

const USE_TYPO_METRICS: u16 = 1 << 7;

pub(crate) const HHEA_METRICS_END: usize = 10;
pub(crate) const OS2_METRICS_END: usize = 78;

fn table(font: &[u8], tag: &[u8; 4], needed: usize) -> Option<usize> {
    let count = u16::from_be_bytes(font.get(4..6)?.try_into().ok()?) as usize;
    (0..count).find_map(|ix| {
        let record = font.get(12 + 16 * ix..28 + 16 * ix)?;
        if &record[0..4] != tag {
            return None;
        }
        let offset = u32::from_be_bytes(record[8..12].try_into().ok()?) as usize;
        let length = u32::from_be_bytes(record[12..16].try_into().ok()?) as usize;
        (length >= needed && offset + needed <= font.len()).then_some(offset)
    })
}

fn put(font: &mut [u8], at: usize, value: [u8; 2]) {
    font[at..at + 2].copy_from_slice(&value);
}

pub fn with_vertical_metrics(font: &[u8], metrics: VerticalMetrics) -> Vec<u8> {
    let (Some(hhea), Some(os2)) = (
        table(font, b"hhea", HHEA_METRICS_END),
        table(font, b"OS/2", OS2_METRICS_END),
    ) else {
        return font.to_vec();
    };
    let mut bytes = font.to_vec();
    put(&mut bytes, hhea + 4, metrics.ascender.to_be_bytes());
    put(&mut bytes, hhea + 6, metrics.descender.to_be_bytes());
    put(&mut bytes, hhea + 8, metrics.line_gap.to_be_bytes());
    let selection = u16::from_be_bytes([bytes[os2 + 62], bytes[os2 + 63]]);
    put(
        &mut bytes,
        os2 + 62,
        (selection | USE_TYPO_METRICS).to_be_bytes(),
    );
    put(&mut bytes, os2 + 68, metrics.ascender.to_be_bytes());
    put(&mut bytes, os2 + 70, metrics.descender.to_be_bytes());
    put(&mut bytes, os2 + 72, metrics.line_gap.to_be_bytes());
    put(&mut bytes, os2 + 74, metrics.win_ascent.to_be_bytes());
    put(&mut bytes, os2 + 76, metrics.win_descent.to_be_bytes());
    bytes
}

#[derive(Debug, Clone, Copy)]
pub struct GraphPalette {
    pub note: Hsla,
    pub session: Hsla,
    pub tag: Hsla,
    pub entity: Hsla,
    pub memory: Hsla,
    pub project: Hsla,
    pub accent: Hsla,
    pub sun: Hsla,
    pub ink: Hsla,
    pub muted: Hsla,
    pub line: Hsla,
    pub viewport: Hsla,
    pub viewport_line: Hsla,
    pub viewport_text: Hsla,
    pub lesson: Hsla,
    pub fact: Hsla,
    pub episode: Hsla,
}

#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub ground: Hsla,
    pub surface: Hsla,
    pub sunken: Hsla,
    pub ink: Hsla,
    pub muted: Hsla,
    pub line: Hsla,
    pub accent: Hsla,
    pub accent_soft: Hsla,
    pub sun: Hsla,
    pub stone: Hsla,
    pub warn: Hsla,
    pub warn_soft: Hsla,
    pub warn_line: Hsla,
    pub viewport: Hsla,
    pub viewport_raised: Hsla,
    pub viewport_line: Hsla,
    pub viewport_text: Hsla,
    pub viewport_muted: Hsla,
    pub graph: GraphPalette,
}

impl Global for Tokens {}

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

impl Tokens {
    pub fn light() -> Tokens {
        Tokens {
            ground: hex(0xF7F1E7),
            surface: hex(0xFDFAF4),
            sunken: hex(0xEFE6D6),
            ink: hex(0x2E363C),
            muted: hex(0x5E5A52),
            line: hex(0xE2D6C2),
            accent: hex(0x983B15),
            accent_soft: hex(0xF6E0CE),
            sun: hex(0xE56936),
            stone: hex(0x817D73),
            warn: hex(0x9A2440),
            warn_soft: hex(0xF3CDC4),
            warn_line: hex(0xE3A99E),
            viewport: hex(0x2E363C),
            viewport_raised: hex(0x383F44),
            viewport_line: hex(0x525C62),
            viewport_text: hex(0xF2D6AC),
            viewport_muted: hex(0xBCAD95),
            graph: GraphPalette {
                note: hex(0xF2D6AC),
                session: hex(0x9FB8CC),
                tag: hex(0xE56936),
                entity: hex(0xD4B85A),
                memory: hex(0xD47A92),
                project: hex(0xA39C8C),
                accent: hex(0x983B15),
                sun: hex(0xE56936),
                ink: hex(0x2E363C),
                muted: hex(0x57534A),
                line: hex(0xD6B88A),
                viewport: hex(0x2E363C),
                viewport_line: hex(0x525C62),
                viewport_text: hex(0xF5DAB0),
                lesson: hex(0xE7A1B0),
                fact: hex(0xD47A92),
                episode: hex(0xC46E88),
            },
        }
    }
}

pub fn add_fonts(cx: &App) {
    cx.text_system()
        .add_fonts(
            SANS_FONTS
                .iter()
                .map(|bytes| Cow::Owned(with_vertical_metrics(bytes, WEB_SANS_METRICS)))
                .chain(MONO_FONTS.iter().map(|bytes| Cow::Borrowed(*bytes)))
                .collect(),
        )
        .expect("the embedded fonts load");
}

pub fn install(cx: &mut App) {
    add_fonts(cx);
    gpui_kit::init(cx);
    let t = Tokens::light();
    Theme::update(cx, |theme| {
        theme.font_family = SANS.into();
        theme.mono_font_family = MONO.into();
        theme.font_size = px(16.);
        theme.mono_font_size = px(12.);
        theme.radius = px(6.);
        theme.radius_lg = px(8.);
        theme.shadow = false;
        let c = &mut theme.colors;
        c.background = t.ground;
        c.foreground = t.ink;
        c.border = t.line;
        c.input = t.line;
        c.ring = t.accent;
        c.caret = t.ink;
        c.selection = t.accent_soft;
        c.muted = t.sunken;
        c.muted_foreground = t.muted;
        c.popover = t.surface;
        c.popover_foreground = t.ink;
        c.primary = t.accent;
        c.primary_hover = t.accent;
        c.primary_active = t.accent;
        c.primary_foreground = gpui_kit::white();
        c.secondary = t.accent_soft;
        c.secondary_hover = t.accent_soft;
        c.secondary_active = t.accent_soft;
        c.secondary_foreground = t.accent;
        c.danger = t.warn;
        c.danger_hover = t.warn;
        c.danger_active = t.warn;
        c.danger_foreground = gpui_kit::white();
        c.accent = t.sunken;
        c.accent_foreground = t.ink;
        c.link = t.accent;
        c.link_hover = t.accent;
        c.link_active = t.accent;
        c.list = t.surface;
        c.list_hover = t.sunken;
        c.list_active = t.accent_soft;
        c.list_active_border = t.line;
    });
    cx.set_global(t);
}

pub fn tokens(cx: &App) -> &Tokens {
    cx.global::<Tokens>()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Face {
    pub family: &'static str,
    pub weight: FontWeight,
    pub size: f32,
    pub line_height: f32,
}

impl Face {
    pub const fn new(
        family: &'static str,
        weight: FontWeight,
        size: f32,
        line_height: f32,
    ) -> Face {
        Face {
            family,
            weight,
            size,
            line_height,
        }
    }
}

pub const CLASSIC_MAC_FAMILIES: [&str; 3] = ["Times", HELVETICA, "Courier"];

pub fn css_ascent_descent(cx: &App, face: Face) -> (f32, f32) {
    let (ascent, descent) = ascent_descent(cx, face);
    let (mut ascent, descent) = (ascent.round(), descent.round());
    if CLASSIC_MAC_FAMILIES.contains(&face.family) {
        ascent += ((ascent + descent) * 0.15 + 0.5).floor();
    }
    (ascent, descent)
}

pub fn normal_line_height(cx: &App, face: Face) -> f32 {
    let (ascent, descent) = css_ascent_descent(cx, face);
    ascent + descent
}

impl Face {
    pub fn with_normal_line_height(self, cx: &App) -> Face {
        Face {
            line_height: normal_line_height(cx, self),
            ..self
        }
    }
}

pub const BODY_FACE: Face = Face::new(SANS, FontWeight::NORMAL, 16., 24.);
pub const SM_FACE: Face = Face::new(SANS, FontWeight::NORMAL, 14., 20.);
pub const XS_FACE: Face = Face::new(SANS, FontWeight::NORMAL, 12., 16.);
pub const XS_MONO_FACE: Face = Face::new(MONO, FontWeight::NORMAL, 12., 16.);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineBox {
    pub height: f32,
    pub baseline: f32,
}

fn ascent_descent(cx: &App, face: Face) -> (f32, f32) {
    let id = cx.text_system().resolve_font(&Font {
        weight: face.weight,
        ..font(SharedString::from(face.family))
    });
    let ascent = f32::from(cx.text_system().ascent(id, px(face.size)));
    let descent = -f32::from(cx.text_system().descent(id, px(face.size)));
    (ascent, descent)
}

pub fn underline_top(cx: &App, face: Face, offset: f32) -> f32 {
    let (ascent, _) = ascent_descent(cx, face);
    css_line_box(cx, face, &[face]).baseline + (ascent - ascent.round()) + offset
}

pub fn underline_thickness(face: Face) -> f32 {
    face.size / 10.
}

pub fn css_line_box(cx: &App, strut: Face, inline: &[Face]) -> LineBox {
    let (above, below) = std::iter::once(strut)
        .chain(inline.iter().copied())
        .map(|face| {
            let (ascent, descent) = css_ascent_descent(cx, face);
            let half_leading = ((face.line_height - ascent - descent) / 2.).floor();
            let above = ascent + half_leading;
            (above, face.line_height - above)
        })
        .fold((f32::MIN, f32::MIN), |(a, b), (above, below)| {
            (a.max(above), b.max(below))
        });
    LineBox {
        height: above + below,
        baseline: above,
    }
}

pub fn gpui_baseline(cx: &App, runs: &[Face], line_height: f32) -> f32 {
    let (ascent, descent) = runs
        .iter()
        .map(|face| ascent_descent(cx, *face))
        .fold((0f32, 0f32), |(a, d), (ascent, descent)| {
            (a.max(ascent), d.max(descent))
        });
    (line_height - ascent - descent) / 2. + ascent
}

pub trait Type: Styled + Sized {
    fn regular(self) -> Self {
        self.font_family(SANS).font_weight(FontWeight::NORMAL)
    }

    fn medium(self) -> Self {
        self.font_family(SANS).font_weight(FontWeight::MEDIUM)
    }

    fn semibold(self) -> Self {
        self.font_family(SANS).font_weight(FontWeight::SEMIBOLD)
    }

    fn mono(self) -> Self {
        self.font_family(MONO).font_weight(FontWeight::NORMAL)
    }

    fn mono_medium(self) -> Self {
        self.font_family(MONO).font_weight(FontWeight::MEDIUM)
    }

    fn size_xs(self) -> Self {
        self.text_size(px(12.)).line_height(px(16.))
    }

    fn size_sm(self) -> Self {
        self.text_size(px(14.)).line_height(px(20.))
    }

    fn size_base(self) -> Self {
        self.text_size(px(16.)).line_height(px(24.))
    }

    fn size_lg(self) -> Self {
        self.text_size(px(18.)).line_height(px(28.))
    }

    fn size_xl(self) -> Self {
        self.text_size(px(20.)).line_height(px(28.))
    }

    fn size_2xl(self) -> Self {
        self.text_size(px(24.)).line_height(px(32.))
    }

    fn size_3xl(self) -> Self {
        self.text_size(px(30.)).line_height(px(36.))
    }

    fn tabular(self) -> Self {
        self.font_features(FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)])))
    }

    fn type_px(self, size: f32) -> Self {
        self.text_size(px(size)).line_height(px(size * 1.5))
    }
}

impl<E: Styled> Type for E {}

#[cfg(test)]
mod tests {
    use super::{
        css_line_box, gpui_baseline, install, table, with_vertical_metrics, Face, LineBox,
        BODY_FACE, HHEA_METRICS_END, MONO, OS2_METRICS_END, SANS, SANS_FONTS, WEB_SANS_METRICS,
        XS_FACE, XS_MONO_FACE,
    };
    use gpui_kit::TestDispatcher;
    use gpui_kit::{font, px, App, Font, FontId, FontWeight, Pixels, SharedString, TestAppContext};
    use std::sync::Arc;

    fn face(cx: &App, family: &str, weight: FontWeight) -> FontId {
        cx.text_system().resolve_font(&Font {
            weight,
            ..font(SharedString::from(family.to_string()))
        })
    }

    fn width(cx: &App, family: &str, weight: FontWeight, text: &str) -> Pixels {
        let id = face(cx, family, weight);
        text.chars()
            .map(|ch| cx.text_system().advance(id, px(22.), ch).unwrap().width)
            .fold(px(0.), |a, b| a + b)
    }

    #[gpui_kit::test]
    fn the_embedded_fonts_register_under_the_families_the_theme_names(_cx: &mut TestAppContext) {
        let cx = TestAppContext::build_with_text_system(
            gpui_kit::TestDispatcher::new(0),
            None,
            Arc::new(gpui_apple::AppleTextSystem::new()),
        );
        cx.update(|cx| {
            install(cx);
            let names = cx.text_system().all_font_names();
            assert!(names.iter().any(|n| n == SANS), "{names:?}");
            assert!(names.iter().any(|n| n == MONO), "{names:?}");
            for weighted in [
                "Source Sans 3 Medium",
                "Source Sans 3 Semibold",
                "IBM Plex Mono Medium",
            ] {
                assert!(
                    !names.iter().any(|n| n == weighted),
                    "{weighted} registered as its own family"
                );
            }
            let regular = width(cx, SANS, FontWeight::NORMAL, "Rhebok");
            let medium = width(cx, SANS, FontWeight::MEDIUM, "Rhebok");
            let semibold = width(cx, SANS, FontWeight::SEMIBOLD, "Rhebok");
            assert_ne!(regular, medium);
            assert_ne!(medium, semibold);
            assert_ne!(
                face(cx, MONO, FontWeight::NORMAL),
                face(cx, MONO, FontWeight::MEDIUM)
            );
            let fallback = face(cx, "No Such Family", FontWeight::NORMAL);
            for (family, weight) in [
                (SANS, FontWeight::NORMAL),
                (SANS, FontWeight::MEDIUM),
                (SANS, FontWeight::SEMIBOLD),
                (MONO, FontWeight::NORMAL),
                (MONO, FontWeight::MEDIUM),
            ] {
                assert_ne!(
                    face(cx, family, weight),
                    fallback,
                    "{family} {weight:?} fell back"
                );
            }
        });
    }

    fn ascent_and_descent(cx: &App, family: &str, weight: FontWeight, size: f32) -> (f32, f32) {
        let id = face(cx, family, weight);
        let ascent = cx.text_system().ascent(id, px(size));
        let descent = cx.text_system().descent(id, px(size));
        (f32::from(ascent), -f32::from(descent))
    }

    #[gpui_kit::test]
    fn the_sans_font_sits_on_the_web_fonts_baseline(_cx: &mut TestAppContext) {
        let cx = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            None,
            Arc::new(gpui_apple::AppleTextSystem::new()),
        );
        cx.update(|cx| {
            install(cx);
            for weight in [FontWeight::NORMAL, FontWeight::MEDIUM, FontWeight::SEMIBOLD] {
                let (ascent, descent) = ascent_and_descent(cx, SANS, weight, 22.);
                assert!(
                    (ascent - 22.528).abs() < 0.01,
                    "sans {weight:?} ascent {ascent}"
                );
                assert!(
                    (descent - 8.8).abs() < 0.01,
                    "sans {weight:?} descent {descent}"
                );
            }
            let (ascent, descent) = ascent_and_descent(cx, MONO, FontWeight::NORMAL, 12.);
            assert!((ascent - 12.3).abs() < 0.01, "mono ascent {ascent}");
            assert!((descent - 3.3).abs() < 0.01, "mono descent {descent}");
        });
    }

    #[test]
    fn a_font_with_a_short_os2_table_is_left_unchanged() {
        let font = SANS_FONTS[0];
        let os2 = table(font, b"OS/2", OS2_METRICS_END).expect("the font has an OS/2 table");
        let count = u16::from_be_bytes([font[4], font[5]]) as usize;
        let record = (0..count)
            .map(|ix| 12 + 16 * ix)
            .find(|at| &font[*at..*at + 4] == b"OS/2")
            .unwrap();
        let mut short = font.to_vec();
        short[record + 12..record + 16]
            .copy_from_slice(&(OS2_METRICS_END as u32 - 1).to_be_bytes());
        assert_eq!(with_vertical_metrics(&short, WEB_SANS_METRICS), short);
        assert_ne!(with_vertical_metrics(font, WEB_SANS_METRICS), font.to_vec());
        assert!(os2 > 0);
    }

    fn shorten(font: &[u8], tag: &[u8; 4], length: usize) -> Vec<u8> {
        let count = u16::from_be_bytes([font[4], font[5]]) as usize;
        let record = (0..count)
            .map(|ix| 12 + 16 * ix)
            .find(|at| &font[*at..*at + 4] == tag)
            .expect("the table exists");
        let mut short = font.to_vec();
        short[record + 12..record + 16].copy_from_slice(&(length as u32).to_be_bytes());
        short
    }

    #[test]
    fn a_font_with_a_short_hhea_table_is_left_unchanged() {
        let font = SANS_FONTS[0];
        let short = shorten(font, b"hhea", HHEA_METRICS_END - 1);
        assert_eq!(with_vertical_metrics(&short, WEB_SANS_METRICS), short);
    }

    const TITLE: Face = Face::new(SANS, FontWeight::SEMIBOLD, 15., 22.5);

    fn apple() -> TestAppContext {
        let cx = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            None,
            Arc::new(gpui_apple::AppleTextSystem::new()),
        );
        cx.update(install);
        cx
    }

    #[gpui_kit::test]
    fn the_css_line_box_matches_chromes_for_the_health_cases(_cx: &mut TestAppContext) {
        apple().update(|cx| {
            assert_eq!(
                css_line_box(cx, XS_FACE, &[XS_FACE, XS_MONO_FACE]),
                LineBox {
                    height: 17.,
                    baseline: 12.
                },
                "caption: xs sans with inline Plex mono"
            );
            assert_eq!(
                css_line_box(cx, BODY_FACE, &[XS_FACE]),
                LineBox {
                    height: 24.,
                    baseline: 17.
                },
                "Connect fix: xs text on a 16/24 strut"
            );
            assert_eq!(
                css_line_box(cx, TITLE, &[TITLE]),
                LineBox {
                    height: 22.5,
                    baseline: 15.
                },
                "panel title: 15px in a 22.5px line"
            );
        });
    }

    #[gpui_kit::test]
    fn gpui_centres_the_baseline_on_unrounded_metrics(_cx: &mut TestAppContext) {
        apple().update(|cx| {
            let close = |got: f32, want: f32, case: &str| {
                assert!((got - want).abs() < 0.005, "{case}: {got} != {want}");
            };
            close(
                gpui_baseline(cx, &[XS_FACE, XS_MONO_FACE], 17.),
                (17. - 12.3 - 4.8) / 2. + 12.3,
                "caption",
            );
            close(
                gpui_baseline(cx, &[XS_FACE], 24.),
                (24. - 12.288 - 4.8) / 2. + 12.288,
                "Connect fix",
            );
            close(
                gpui_baseline(cx, &[TITLE], 22.5),
                (22.5 - 15.36 - 6.) / 2. + 15.36,
                "panel title",
            );
        });
    }
}

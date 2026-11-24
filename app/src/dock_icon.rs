use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{imageops, ImageEncoder, ImageFormat, Rgba, RgbaImage};

pub const ART: &[u8] = include_bytes!("../../plugin/icon.png");
pub const CANVAS: u32 = 832;
pub const BODY: f32 = 670.;
pub const CORNER_RADIUS: f32 = 150.;
const SHADOW_DROP: f32 = 8.;
const SHADOW_BLUR: f32 = 22.;
const SHADOW_ALPHA: f32 = 0.30;
const FILL_TOP: [f32; 3] = [252., 249., 243.];
const FILL_BOTTOM: [f32; 3] = [240., 231., 216.];

pub fn art() -> RgbaImage {
    image::load_from_memory_with_format(ART, ImageFormat::Png)
        .expect("the icon art is a PNG compiled into the binary")
        .to_rgba8()
}

pub fn compose() -> RgbaImage {
    let centre = CANVAS as f32 / 2.;
    let half = BODY / 2.;
    let top = centre - half;
    let mut canvas = RgbaImage::from_fn(CANVAS, CANVAS, |x, y| {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let shadow_distance = rounded_square_distance(px, py - SHADOW_DROP, centre, half);
        let falloff = 1. - (shadow_distance / SHADOW_BLUR).clamp(0., 1.);
        let shadow = Rgba([0., 0., 0., SHADOW_ALPHA * falloff * falloff]);
        let coverage = (0.5 - rounded_square_distance(px, py, centre, half)).clamp(0., 1.);
        let t = ((py - top) / BODY).clamp(0., 1.);
        let mut fill = [0.; 3];
        for (channel, value) in fill.iter_mut().enumerate() {
            *value = FILL_TOP[channel] + (FILL_BOTTOM[channel] - FILL_TOP[channel]) * t;
        }
        over(Rgba([fill[0], fill[1], fill[2], coverage]), shadow)
    });
    let art = art();
    let left = i64::from((CANVAS - art.width()) / 2);
    let upper = i64::from((CANVAS - art.height()) / 2);
    imageops::overlay(&mut canvas, &art, left, upper);
    canvas
}

pub fn png() -> Vec<u8> {
    let icon = compose();
    let mut bytes = Vec::new();
    PngEncoder::new_with_quality(&mut bytes, CompressionType::Fast, FilterType::Adaptive)
        .write_image(
            icon.as_raw(),
            icon.width(),
            icon.height(),
            image::ExtendedColorType::Rgba8,
        )
        .expect("an in-memory PNG encode cannot fail");
    bytes
}

fn rounded_square_distance(x: f32, y: f32, centre: f32, half: f32) -> f32 {
    let inner = half - CORNER_RADIUS;
    let qx = (x - centre).abs() - inner;
    let qy = (y - centre).abs() - inner;
    let outside = qx.max(0.).hypot(qy.max(0.));
    outside + qx.max(qy).min(0.) - CORNER_RADIUS
}

fn over(top: Rgba<f32>, bottom: Rgba<f32>) -> Rgba<u8> {
    let alpha = top[3] + bottom[3] * (1. - top[3]);
    if alpha <= 0. {
        return Rgba([0, 0, 0, 0]);
    }
    let channel = |i: usize| {
        let value = (top[i] * top[3] + bottom[i] * bottom[3] * (1. - top[3])) / alpha;
        value.round().clamp(0., 255.) as u8
    };
    Rgba([
        channel(0),
        channel(1),
        channel(2),
        (alpha * 255.).round() as u8,
    ])
}

#[cfg(target_os = "macos")]
pub fn install(png: &[u8]) {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{NSData, NSSize};

    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(png);
    let Some(icon) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let points = f64::from(CANVAS) / 2.;
    icon.setSize(NSSize::new(points, points));
    let app = NSApplication::sharedApplication(main_thread);
    unsafe { app.setApplicationIconImage(Some(&icon)) };
}

#[cfg(not(target_os = "macos"))]
pub fn install(_png: &[u8]) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_art_is_the_512_pixel_plugin_icon() {
        let art = art();
        assert_eq!((art.width(), art.height()), (512, 512));
        assert_eq!(art.get_pixel(0, 0)[3], 0);
    }

    #[test]
    fn the_icon_is_a_padded_rounded_square_with_the_art_inside() {
        let icon = image::load_from_memory_with_format(&png(), ImageFormat::Png)
            .expect("decodes")
            .to_rgba8();
        assert_eq!((icon.width(), icon.height()), (CANVAS, CANVAS));
        assert_eq!(icon.get_pixel(0, 0)[3], 0);
        assert_eq!(icon.get_pixel(CANVAS - 1, CANVAS - 1)[3], 0);
        let edge = ((CANVAS as f32 - BODY) / 2.) as u32;
        assert_eq!(icon.get_pixel(edge + 4, edge + 4)[3], 0);
        assert_eq!(icon.get_pixel(edge + 4, CANVAS / 2)[3], 255);
        assert_eq!(icon.get_pixel(CANVAS / 2, edge + 4)[3], 255);
        let art = art();
        let offset = (CANVAS - art.width()) / 2;
        let drawn = icon.get_pixel(offset + 256, offset + 256);
        let source = art.get_pixel(256, 256);
        for channel in 0..3 {
            assert!(drawn[channel].abs_diff(source[channel]) <= 3);
        }
    }
}

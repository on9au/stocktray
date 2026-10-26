//! Draws the tray icon: rows of white text on a rounded square coloured by the day's move.
//!
//! Each row is rasterised from a system font sized to fill it, then squeezed horizontally when
//! too wide. A squeezed digit stays more legible at 16 px than a uniformly shrunken one.

use std::path::PathBuf;

use ab_glyph::{Font as _, FontVec, PxScale, ScaleFont, point};

/// ab_glyph reads outlines from the font file on demand. (fontdue, tried first, parses every
/// glyph up front: 20 MB for Segoe UI Bold.)
pub type Font = FontVec;

/// Tried in order from `%WINDIR%\Fonts`. Segoe UI Bold ships with every Windows 11 install.
const FONTS: [&str; 2] = ["segoeuib.ttf", "arialbd.ttf"];
/// Most of the icon's height a single row of digits fills.
const TEXT_HEIGHT: f32 = 0.72;
/// Squeeze no narrower than this; past it, shrink the text instead.
const MIN_SQUEEZE: f32 = 0.6;
/// Letter spacing, as a share of the horizontal scale. Negative tightens the gaps.
const TRACKING: f32 = -0.03;

/// The day's move, which picks the icon's colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trend {
    Up,
    Down,
    Flat,
    /// No fresh quote: still loading, or the last fetch failed.
    Unknown,
}

impl Trend {
    /// (background, text)
    fn colours(self) -> ([u8; 3], [u8; 3]) {
        const WHITE: [u8; 3] = [0xFF, 0xFF, 0xFF];
        match self {
            Self::Up => ([0x1A, 0x7F, 0x37], WHITE),
            Self::Down => ([0xC4, 0x24, 0x2B], WHITE),
            Self::Flat => ([0x4B, 0x55, 0x63], WHITE),
            Self::Unknown => ([0x6E, 0x77, 0x81], [0xD8, 0xDC, 0xE0]),
        }
    }
}

pub fn load_font() -> Option<Font> {
    let windir = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    FONTS.iter().find_map(|name| Font::try_from_vec(std::fs::read(windir.join("Fonts").join(name)).ok()?).ok())
}

/// Renders a `size`×`size` icon as straight (not premultiplied) RGBA, with `lines` stacked top
/// to bottom. Without a font, only the coloured square is drawn.
pub fn render(font: Option<&Font>, lines: &[String], trend: Trend, size: u32) -> Vec<u8> {
    let n = size as usize;
    let (bg, fg) = trend.colours();
    let radius = (size as f32 / 6.0).max(2.0);
    let text = font.map_or_else(|| vec![0.0; n * n], |font| text_mask(font, lines, n));
    let lerp = |c: usize, t: f32| (bg[c] as f32 + (fg[c] as f32 - bg[c] as f32) * t).round() as u8;
    (0..n * n)
        .flat_map(|i| {
            let alpha = (rounded_square_coverage(i % n, i / n, n, radius) * 255.0).round() as u8;
            [lerp(0, text[i]), lerp(1, text[i]), lerp(2, text[i]), alpha]
        })
        .collect()
}

/// How much of pixel (x, y) the rounded square covers, from its signed distance field.
fn rounded_square_coverage(x: usize, y: usize, n: usize, radius: f32) -> f32 {
    let half = n as f32 / 2.0;
    let qx = (x as f32 + 0.5 - half).abs() - (half - radius);
    let qy = (y as f32 + 0.5 - half).abs() - (half - radius);
    let distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
    (0.5 - distance).clamp(0.0, 1.0)
}

/// Text coverage (0..=1) for each pixel of an n×n icon: the lines stacked and centred, with a
/// padding-wide gap between them, cropped to the padding if they still don't fit.
fn text_mask(font: &Font, lines: &[String], n: usize) -> Vec<f32> {
    let pad = (n / 16).max(1);
    let inner = pad..n - pad;
    let rows = lines.len().max(1);
    let row_h = (n as f32 * TEXT_HEIGHT).min((inner.len() - pad * (rows - 1)) as f32 / rows as f32);
    // Whole-pixel bounds add up to a pixel to the ink, hence the - 1.
    let lines: Vec<Line> = lines.iter().map(|text| fit(font, text, row_h - 1.0, inner.len())).collect();
    let height = lines.iter().map(|line| line.height + pad).sum::<usize>().saturating_sub(pad);

    let mut mask = vec![0.0; n * n];
    let mut top = (n as isize - height as isize) / 2;
    for line in &lines {
        let w = line.width.min(inner.len());
        let (src_x, left) = ((line.width - w) / 2, (n - w) / 2);
        for y in 0..line.height {
            let row = top + y as isize;
            if inner.contains(&(row as usize)) && row >= 0 {
                mask[row as usize * n + left..][..w].copy_from_slice(&line.coverage[y * line.width + src_x..][..w]);
            }
        }
        top += (line.height + pad) as isize;
    }
    mask
}

/// A rasterised line of text, cropped to its ink.
struct Line {
    width: usize,
    height: usize,
    coverage: Vec<f32>,
}

/// Rasterises `text` with digits `height` px tall, squeezed or shrunk to at most `max_w` wide.
fn fit(font: &Font, text: &str, height: f32, max_w: usize) -> Line {
    // PxScale is the font's full ascent-to-descent height, so convert via the unscaled outline of
    // '0'. Unscaled bounds are y-up (min.y is the top), hence abs.
    let digit_h = font.outline(font.glyph_id('0')).map_or(0.7 * font.height_unscaled(), |o| o.bounds.height().abs());
    let y = height * font.height_unscaled() / digit_h;
    let mut scale = PxScale { x: y, y };
    let mut line = rasterise(font, text, scale);
    // Pen positions are rounded, so the width doesn't scale exactly; a few passes converge.
    for _ in 0..4 {
        if line.width <= max_w {
            break;
        }
        // Squeeze; once at MIN_SQUEEZE, shrink in both directions.
        let x = scale.x * max_w as f32 / line.width as f32;
        scale = PxScale { x, y: scale.y.min(x / MIN_SQUEEZE) };
        line = rasterise(font, text, scale);
    }
    line
}

fn rasterise(font: &Font, text: &str, scale: PxScale) -> Line {
    let scaled = font.as_scaled(scale);
    // Lay out along a baseline at y = 0 (y grows downwards).
    let mut pen = 0.0f32;
    let mut previous = None;
    let mut outlines = Vec::new();
    for c in text.chars() {
        let id = font.glyph_id(c);
        pen += previous.map_or(0.0, |previous| scaled.kern(previous, id));
        // Whole-pixel positions keep vertical stems sharp.
        outlines.extend(font.outline_glyph(id.with_scale_and_position(scale, point(pen.round(), 0.0))));
        pen += scaled.h_advance(id) + scale.x * TRACKING;
        previous = Some(id);
    }
    if outlines.is_empty() {
        return Line { width: 1, height: 1, coverage: vec![0.0] };
    }

    // px_bounds are whole pixels, so the ink box is too.
    let bounds: Vec<_> = outlines.iter().map(|o| o.px_bounds()).collect();
    let left = bounds.iter().map(|b| b.min.x).fold(f32::MAX, f32::min);
    let top = bounds.iter().map(|b| b.min.y).fold(f32::MAX, f32::min);
    let right = bounds.iter().map(|b| b.max.x).fold(f32::MIN, f32::max);
    let bottom = bounds.iter().map(|b| b.max.y).fold(f32::MIN, f32::max);
    let (width, height) = ((right - left) as usize, (bottom - top) as usize);

    let mut coverage = vec![0.0f32; width * height];
    for (outline, b) in outlines.iter().zip(&bounds) {
        let (ox, oy) = ((b.min.x - left) as usize, (b.min.y - top) as usize);
        outline.draw(|x, y, c| {
            let cell = &mut coverage[(oy + y as usize) * width + ox + x as usize];
            // Glyphs can overlap once tracking is tightened; keep the darker pixel.
            *cell = cell.max(c.min(1.0));
        });
    }
    Line { width, height, coverage }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|&t| t.to_owned()).collect()
    }

    #[test]
    fn renders_every_size_without_font() {
        for size in [16, 20, 24, 32] {
            let rgba = render(None, &lines(&["40", "57"]), Trend::Up, size);
            assert_eq!(rgba.len(), (size * size * 4) as usize);
            // Corners are rounded off (only anti-aliasing left), the centre opaque.
            assert!(rgba[3] < 64, "corner alpha {} at {size}px", rgba[3]);
            assert_eq!(rgba[((size / 2 * size + size / 2) * 4 + 3) as usize], 255);
        }
    }

    #[test]
    fn text_stays_inside_icon() {
        let Some(font) = load_font() else { return };
        for texts in [&["40", "57"][..], &["1234", "50"], &["12k"], &["…"]] {
            for n in [16, 24, 32] {
                let mask = text_mask(&font, &lines(texts), n);
                assert!(mask.iter().any(|&c| c > 0.5), "{texts:?} at {n}px drew nothing");
                // The outermost rows and columns are padding.
                let edge = |i: usize| mask[i] == 0.0 && mask[i * n] == 0.0 && mask[n * n - 1 - i] == 0.0;
                assert!((0..n).all(edge), "{texts:?} at {n}px touches the edge");
            }
        }
    }
}

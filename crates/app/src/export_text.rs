//! Lettering for the export images: SN Pro, smoothed, at any pixel size.
//!
//! The two weights are static cuts (wght 400 and 700, fontTools' instancer) of the variable SN Pro
//! the webview loads, as TTF because the font libraries cannot read woff2. They are embedded for
//! the same reason the power icons are: the renderer draws synchronously on both targets. Sizes
//! are CSS pixels (the em), so a layout copied from the beta's card keeps its type scale.

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont};
use image::{Rgba, RgbaImage};
use std::sync::LazyLock;

static REGULAR: LazyLock<FontRef<'static>> = LazyLock::new(|| {
    FontRef::try_from_slice(include_bytes!("../assets/fonts/SNPro-Regular.ttf"))
        .expect("the bundled SN Pro Regular is a valid TTF")
});
static BOLD: LazyLock<FontRef<'static>> = LazyLock::new(|| {
    FontRef::try_from_slice(include_bytes!("../assets/fonts/SNPro-Bold.ttf"))
        .expect("the bundled SN Pro Bold is a valid TTF")
});

const ELLIPSIS: char = '…';

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weight {
    Regular,
    Bold,
}

impl Weight {
    fn font(self) -> &'static FontRef<'static> {
        match self {
            Weight::Regular => &REGULAR,
            Weight::Bold => &BOLD,
        }
    }
}

/// How one run of text looks: its size in CSS pixels, its weight and its colour.
#[derive(Clone, Copy, Debug)]
pub struct Lettering {
    pub size: f32,
    pub weight: Weight,
    pub color: Rgba<u8>,
}

impl Lettering {
    pub fn new(size: f32, weight: Weight, color: Rgba<u8>) -> Self {
        Self {
            size,
            weight,
            color,
        }
    }

    fn scale(&self) -> PxScale {
        self.weight
            .font()
            .pt_to_px_scale(self.size)
            .expect("the bundled SN Pro states its units per em")
    }

    /// Height of one line, ascender top to descender bottom.
    pub fn height(&self) -> usize {
        let scaled = self.weight.font().as_scaled(self.scale());
        (scaled.ascent() - scaled.descent()).ceil() as usize
    }

    /// Width `text` takes when drawn.
    pub fn width(&self, text: &str) -> usize {
        self.layout(text).last().map_or(0.0, |&(_, end)| end).ceil() as usize
    }

    /// Each glyph with the x it starts at and the x the next one starts at, from 0.
    fn layout(&self, text: &str) -> Vec<(GlyphId, f32)> {
        let scaled = self.weight.font().as_scaled(self.scale());
        let mut caret = 0.0;
        let mut previous: Option<GlyphId> = None;
        let mut glyphs = Vec::with_capacity(text.len());
        for ch in text.chars() {
            let id = scaled.glyph_id(ch);
            if let Some(previous) = previous {
                caret += scaled.kern(previous, id);
            }
            caret += scaled.h_advance(id);
            glyphs.push((id, caret));
            previous = Some(id);
        }
        glyphs
    }

    /// Draw `text` with the top of its line at `y`. Returns the width drawn.
    pub fn draw(&self, img: &mut RgbaImage, x: usize, y: usize, text: &str) -> usize {
        let font = self.weight.font();
        let scale = self.scale();
        let scaled = font.as_scaled(scale);
        let baseline = y as f32 + scaled.ascent();
        let glyphs = self.layout(text);
        for &(id, end) in &glyphs {
            let origin = ab_glyph::point(x as f32 + end - scaled.h_advance(id), baseline);
            let Some(outline) = font.outline_glyph(id.with_scale_and_position(scale, origin))
            else {
                continue;
            };
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                let px = bounds.min.x as i64 + i64::from(gx);
                let py = bounds.min.y as i64 + i64::from(gy);
                blend(img, px, py, self.color, coverage);
            });
        }
        glyphs.last().map_or(0.0, |&(_, end)| end).ceil() as usize
    }

    /// `text` cut to fit `max_width`, ending in `…` when it had to cut. Empty when not even
    /// the ellipsis fits.
    pub fn fit(&self, text: &str, max_width: usize) -> String {
        if self.width(text) <= max_width {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        (0..chars.len())
            .rev()
            .map(|keep| {
                let kept: String = chars[..keep].iter().collect();
                format!("{}{ELLIPSIS}", kept.trim_end())
            })
            .find(|cut| self.width(cut) <= max_width)
            .unwrap_or_default()
    }
}

/// Lay `color` over the pixel at `coverage` (0 to 1), so text drawn on a transparent canvas
/// keeps its soft edges rather than gaining a dark fringe.
pub(crate) fn blend(img: &mut RgbaImage, x: i64, y: i64, color: Rgba<u8>, coverage: f32) {
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return;
    };
    if x >= img.width() || y >= img.height() {
        return;
    }
    let alpha = coverage.clamp(0.0, 1.0) * f32::from(color[3]) / 255.0;
    if alpha <= 0.0 {
        return;
    }
    let under = img.get_pixel_mut(x, y);
    let under_alpha = f32::from(under[3]) / 255.0;
    let out_alpha = alpha + under_alpha * (1.0 - alpha);
    for channel in 0..3 {
        let over = f32::from(color[channel]) * alpha;
        let below = f32::from(under[channel]) * under_alpha * (1.0 - alpha);
        under[channel] = ((over + below) / out_alpha).round() as u8;
    }
    under[3] = (out_alpha * 255.0).round() as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);

    #[test]
    fn fit_never_runs_past_its_width() {
        let names = [
            "Sonic Diffusion",
            "Prestige Power Quick",
            "Level 50 Tanker — Sonic Aura / Sonic Melee",
            "W",
            "",
        ];
        for weight in [Weight::Regular, Weight::Bold] {
            for size in [10.0, 22.0, 38.0] {
                let lettering = Lettering::new(size, weight, WHITE);
                for name in names {
                    for room in (0..400).step_by(7) {
                        let cut = lettering.fit(name, room);
                        assert!(
                            lettering.width(&cut) <= room,
                            "{name:?} at {size}px cut to {cut:?}, wider than {room}"
                        );
                    }
                }
            }
        }
    }

    /// Writes `target/lettering-sample.png` to look at: both weights at the card's sizes.
    #[test]
    #[ignore = "writes a picture to look at; run with --ignored"]
    fn lettering_sample() {
        let mut img = RgbaImage::from_pixel(900, 330, Rgba([20, 20, 30, 255]));
        let mut y = 10;
        for size in [10.0, 16.0, 24.0, 40.0] {
            for weight in [Weight::Regular, Weight::Bold] {
                let lettering = Lettering::new(size, weight, WHITE);
                lettering.draw(
                    &mut img,
                    10,
                    y,
                    "Sonic Diffusion — L50 Tanker 156.25% “AV” …",
                );
                y += lettering.height() + 4;
            }
        }
        let out = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/lettering-sample.png"
        );
        img.save(out).expect("the sample writes");
    }
}

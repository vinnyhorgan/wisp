//! software renderer: fills rects and blits glyph masks into a cpu
//! framebuffer. pixel format is 0x00rrggbb, softbuffer's native layout.
//!
//! all coordinate math is done in i64 so arbitrary rects from lua -- even
//! ones sitting at the edges of the i32 range -- clip safely instead of
//! overflowing.

use crate::font::Font;
use crate::image::Image;

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// touching edges count as overlapping; the rencache relies on this to
    /// merge adjacent dirty regions
    pub fn overlaps(self, b: Rect) -> bool {
        let (ax, ay, ax2, ay2) = self.bounds();
        let (bx, by, bx2, by2) = b.bounds();
        bx2 >= ax && bx <= ax2 && by2 >= ay && by <= ay2
    }

    pub fn intersect(self, b: Rect) -> Rect {
        let (ax, ay, ax2, ay2) = self.bounds();
        let (bx, by, bx2, by2) = b.bounds();
        let x = ax.max(bx);
        let y = ay.max(by);
        let w = (ax2.min(bx2) - x).clamp(0, i32::MAX as i64);
        let h = (ay2.min(by2) - y).clamp(0, i32::MAX as i64);
        Rect::new(x as i32, y as i32, w as i32, h as i32)
    }

    pub fn merge(self, b: Rect) -> Rect {
        let (ax, ay, ax2, ay2) = self.bounds();
        let (bx, by, bx2, by2) = b.bounds();
        let x = ax.min(bx);
        let y = ay.min(by);
        let w = (ax2.max(bx2) - x).min(i32::MAX as i64);
        let h = (ay2.max(by2) - y).min(i32::MAX as i64);
        Rect::new(x as i32, y as i32, w as i32, h as i32)
    }

    pub fn is_empty(self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    /// corners as (x1, y1, x2, y2) in i64, safe against i32 overflow
    fn bounds(self) -> (i64, i64, i64, i64) {
        let (x, y) = (self.x as i64, self.y as i64);
        (x, y, x + self.width as i64, y + self.height as i64)
    }
}

/// exact (a * b) / 255 with rounding; lite used `>> 8`, which slightly
/// darkened every blend
#[inline]
fn mul255(a: u8, b: u8) -> u8 {
    (((a as u32 * b as u32 + 128) * 257) >> 16) as u8
}

#[inline]
fn pack(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

#[inline]
fn blend(dst: u32, r: u8, g: u8, b: u8, a: u8) -> u32 {
    let ia = 255 - a;
    let dr = mul255((dst >> 16) as u8, ia);
    let dg = mul255((dst >> 8) as u8, ia);
    let db = mul255(dst as u8, ia);
    // the u8 sums cannot overflow: the exact values obey
    // (a*s + (255-a)*d) / 255 <= 255, and rounding each half adds under
    // one, so the pair can carry past 255 only if the exact sum already
    // reached it -- in which case neither half rounded up
    pack(mul255(r, a) + dr, mul255(g, a) + dg, mul255(b, a) + db)
}

pub struct Framebuffer {
    pub pixels: Vec<u32>,
    pub width: i32,
    pub height: i32,
    clip: Rect,
}

impl Framebuffer {
    pub fn new(width: i32, height: i32) -> Framebuffer {
        let (width, height) = (width.max(1), height.max(1));
        Framebuffer {
            pixels: vec![0; width as usize * height as usize],
            width,
            height,
            clip: Rect::new(0, 0, width, height),
        }
    }

    pub fn resize(&mut self, width: i32, height: i32) {
        let (width, height) = (width.max(1), height.max(1));
        if (width, height) == (self.width, self.height) {
            return;
        }
        self.width = width;
        self.height = height;
        self.pixels.clear();
        self.pixels.resize(width as usize * height as usize, 0);
        self.clip = Rect::new(0, 0, width, height);
    }

    pub fn set_clip(&mut self, rect: Rect) {
        self.clip = rect.intersect(Rect::new(0, 0, self.width, self.height));
    }

    pub fn draw_rect(&mut self, rect: Rect, color: Color) {
        if color.a == 0 {
            return;
        }
        let r = rect.intersect(self.clip);
        if r.is_empty() {
            return;
        }
        for j in r.y..r.y + r.height {
            // index math in usize: the clip guarantees the values are
            // non-negative, and usize can never wrap on a real buffer
            let row = j as usize * self.width as usize + r.x as usize;
            let span = &mut self.pixels[row..row + r.width as usize];
            if color.a == 255 {
                span.fill(pack(color.r, color.g, color.b));
            } else {
                for px in span {
                    *px = blend(*px, color.r, color.g, color.b, color.a);
                }
            }
        }
    }

    /// blits an alpha mask (a rasterized glyph) tinted with `color`
    fn draw_mask(&mut self, mask: &[u8], mw: i32, x: i32, y: i32, color: Color) {
        if color.a == 0 || mask.is_empty() || mw <= 0 {
            return;
        }
        let mh = mask.len() as i32 / mw;
        let sub = Rect::new(x, y, mw, mh).intersect(self.clip);
        if sub.is_empty() {
            return;
        }
        for j in 0..sub.height {
            let src_row = (sub.y - y + j) as usize * mw as usize + (sub.x - x) as usize;
            let dst_row = (sub.y + j) as usize * self.width as usize + sub.x as usize;
            for i in 0..sub.width as usize {
                let a = mul255(mask[src_row + i], color.a);
                if a != 0 {
                    let px = &mut self.pixels[dst_row + i];
                    *px = blend(*px, color.r, color.g, color.b, a);
                }
            }
        }
    }

    /// draws `image` scaled to fill `dest` (nearest neighbor), tinted by
    /// `color` -- white is identity, like draw_text. every output pixel
    /// maps to its source pixel from its absolute offset in `dest` alone,
    /// so clipping skips pixels but can never shift the mapping: a scaled
    /// image redrawn in parts is pixel-identical to one drawn whole. the
    /// rounding artifacts that sank lite-xl's first canvas (#1438) came
    /// from scaling the clipped subrect instead
    pub fn draw_image(&mut self, image: &Image, dest: Rect, color: Color) {
        if color.a == 0 || dest.is_empty() {
            return;
        }
        let sub = dest.intersect(self.clip);
        if sub.is_empty() {
            return;
        }
        for j in 0..sub.height {
            let oy = sub.y as i64 - dest.y as i64 + j as i64;
            let sy = (oy * image.height() as i64 / dest.height as i64) as i32;
            let dst_row = (sub.y + j) as usize * self.width as usize + sub.x as usize;
            for i in 0..sub.width {
                let ox = sub.x as i64 - dest.x as i64 + i as i64;
                let sx = (ox * image.width() as i64 / dest.width as i64) as i32;
                let c = image.pixel(sx, sy);
                let a = mul255(c.a, color.a);
                if a == 0 {
                    continue;
                }
                let r = mul255(c.r, color.r);
                let g = mul255(c.g, color.g);
                let b = mul255(c.b, color.b);
                let px = &mut self.pixels[dst_row + i as usize];
                *px = if a == 255 {
                    pack(r, g, b)
                } else {
                    blend(*px, r, g, b, a)
                };
            }
        }
    }

    /// draws `text` with its top-left corner at (x, y); returns the pen x
    /// position after the last glyph
    pub fn draw_text(&mut self, font: &Font, text: &str, x: i32, y: i32, color: Color) -> i32 {
        let mut pen = x;
        let baseline = y.saturating_add(font.ascent());
        for ch in text.chars() {
            if ch == '\t' {
                pen = pen.saturating_add(font.tab_advance());
                continue;
            }
            let glyph = font.glyph(ch);
            self.draw_mask(
                &glyph.mask,
                glyph.width,
                pen.saturating_add(glyph.left),
                baseline.saturating_sub(glyph.top),
                color,
            );
            pen = pen.saturating_add(glyph.advance);
        }
        pen
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn color(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }

    #[test]
    fn opaque_rect_fills_exactly() {
        let mut fb = Framebuffer::new(10, 10);
        fb.draw_rect(Rect::new(2, 3, 4, 2), color(255, 0, 0, 255));
        let red = pack(255, 0, 0);
        let filled = fb.pixels.iter().filter(|&&p| p == red).count();
        assert_eq!(filled, 8);
        assert_eq!(fb.pixels[(3 * 10 + 2) as usize], red);
        assert_eq!(fb.pixels[(4 * 10 + 5) as usize], red);
        assert_eq!(fb.pixels[(3 * 10 + 6) as usize], 0);
    }

    #[test]
    fn rect_is_clipped() {
        let mut fb = Framebuffer::new(10, 10);
        fb.set_clip(Rect::new(4, 4, 2, 2));
        fb.draw_rect(Rect::new(0, 0, 10, 10), color(0, 255, 0, 255));
        let green = pack(0, 255, 0);
        assert_eq!(fb.pixels.iter().filter(|&&p| p == green).count(), 4);
    }

    #[test]
    fn rect_outside_framebuffer_is_safe() {
        let mut fb = Framebuffer::new(10, 10);
        fb.draw_rect(Rect::new(-100, -100, 50, 50), color(255, 255, 255, 255));
        fb.draw_rect(Rect::new(100, 100, 50, 50), color(255, 255, 255, 255));
        fb.draw_rect(Rect::new(-5, -5, 10, 10), color(255, 255, 255, 255));
        assert_eq!(fb.pixels[0], pack(255, 255, 255));
    }

    #[test]
    fn zero_alpha_draws_nothing() {
        let mut fb = Framebuffer::new(4, 4);
        fb.draw_rect(Rect::new(0, 0, 4, 4), color(255, 255, 255, 0));
        assert!(fb.pixels.iter().all(|&p| p == 0));
    }

    #[test]
    fn alpha_blend_is_exact() {
        // 50% white over black must give exactly 128 (rounded), not 127
        let mut fb = Framebuffer::new(1, 1);
        fb.draw_rect(Rect::new(0, 0, 1, 1), color(255, 255, 255, 128));
        assert_eq!(fb.pixels[0], pack(128, 128, 128));
    }

    #[test]
    fn full_alpha_blend_is_identity() {
        let mut fb = Framebuffer::new(1, 1);
        fb.draw_rect(Rect::new(0, 0, 1, 1), color(10, 20, 30, 255));
        fb.draw_rect(Rect::new(0, 0, 1, 1), color(200, 100, 50, 255));
        assert_eq!(fb.pixels[0], pack(200, 100, 50));
    }

    #[test]
    fn rect_ops() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert_eq!(a.intersect(b), Rect::new(5, 5, 5, 5));
        assert_eq!(a.merge(b), Rect::new(0, 0, 15, 15));
        assert!(a.overlaps(b));
        assert!(!Rect::new(0, 0, 2, 2).overlaps(Rect::new(5, 5, 2, 2)));
        assert!(a.intersect(Rect::new(20, 20, 5, 5)).is_empty());
    }

    /// a 2x2 test card: red, green / blue, white
    fn card() -> Image {
        Image::from_rgba(
            2,
            2,
            &[
                255, 0, 0, 255, 0, 255, 0, 255, //
                0, 0, 255, 255, 255, 255, 255, 255,
            ],
        )
        .unwrap()
    }

    #[test]
    fn image_draws_at_natural_size() {
        let mut fb = Framebuffer::new(6, 6);
        fb.draw_image(&card(), Rect::new(1, 2, 2, 2), color(255, 255, 255, 255));
        assert_eq!(fb.pixels[(2 * 6 + 1) as usize], pack(255, 0, 0));
        assert_eq!(fb.pixels[(2 * 6 + 2) as usize], pack(0, 255, 0));
        assert_eq!(fb.pixels[(3 * 6 + 1) as usize], pack(0, 0, 255));
        assert_eq!(fb.pixels[(3 * 6 + 2) as usize], pack(255, 255, 255));
        // and not a pixel more
        let inked = fb.pixels.iter().filter(|&&p| p != 0).count();
        assert_eq!(inked, 4);
    }

    #[test]
    fn nearest_upscale_maps_pixels_exactly() {
        let mut fb = Framebuffer::new(4, 4);
        fb.draw_image(&card(), Rect::new(0, 0, 4, 4), color(255, 255, 255, 255));
        // each source pixel becomes a clean 2x2 block
        for (x, y, want) in [
            (0, 0, pack(255, 0, 0)),
            (1, 1, pack(255, 0, 0)),
            (3, 0, pack(0, 255, 0)),
            (0, 3, pack(0, 0, 255)),
            (3, 3, pack(255, 255, 255)),
        ] {
            assert_eq!(fb.pixels[(y * 4 + x) as usize], want, "at ({x}, {y})");
        }
    }

    #[test]
    fn image_alpha_and_tint_blend_exactly() {
        // a half-transparent white source pixel over black must give 128,
        // and a tint must multiply both color and alpha
        let img = Image::from_rgba(1, 1, &[255, 255, 255, 128]).unwrap();
        let mut fb = Framebuffer::new(1, 1);
        fb.draw_image(&img, Rect::new(0, 0, 1, 1), color(255, 255, 255, 255));
        assert_eq!(fb.pixels[0], pack(128, 128, 128));

        let mut fb = Framebuffer::new(1, 1);
        let opaque = Image::from_rgba(1, 1, &[255, 255, 255, 255]).unwrap();
        fb.draw_image(&opaque, Rect::new(0, 0, 1, 1), color(255, 0, 0, 128));
        assert_eq!(fb.pixels[0], pack(128, 0, 0));
    }

    // the regression lite-xl's first canvas shipped (#1438): repainting
    // part of a scaled image produced different pixels than painting it
    // whole, because the clipped subrect was scaled instead of the whole
    // image. wisp's mapping is absolute, so any partition of the screen
    // into clips must reproduce the unclipped frame bit for bit
    #[test]
    fn a_scaled_image_redrawn_in_parts_matches_the_whole() {
        let img =
            Image::from_rgba(3, 3, &(0..36).map(|i| (i * 7) as u8).collect::<Vec<_>>()).unwrap();
        // awkward scale and offset on purpose: 3x3 into 17x13 at (-2, 3)
        let dest = Rect::new(-2, 3, 17, 13);
        let mut whole = Framebuffer::new(16, 16);
        whole.draw_image(&img, dest, color(255, 255, 255, 255));

        let mut parts = Framebuffer::new(16, 16);
        for (cx, cy, cw, ch) in [(0, 0, 7, 16), (7, 0, 9, 5), (7, 5, 9, 11)] {
            parts.set_clip(Rect::new(cx, cy, cw, ch));
            parts.draw_image(&img, dest, color(255, 255, 255, 255));
        }
        assert_eq!(whole.pixels, parts.pixels);
    }

    #[test]
    fn extreme_rects_do_not_overflow() {
        let huge = Rect::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX);
        let tiny = Rect::new(0, 0, 4, 4);
        let _ = huge.overlaps(tiny);
        let _ = huge.intersect(tiny);
        let _ = huge.merge(tiny);
        let mut fb = Framebuffer::new(8, 8);
        fb.draw_rect(
            Rect::new(i32::MAX - 1, i32::MAX - 1, i32::MAX, i32::MAX),
            color(1, 2, 3, 255),
        );
        fb.draw_rect(
            Rect::new(i32::MIN, 0, i32::MAX, i32::MAX),
            color(1, 2, 3, 128),
        );
    }

    proptest! {
        // seeds of past failures are kept in tests/regressions and replayed
        // before any new random cases
        #![proptest_config(ProptestConfig {
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::Direct("tests/regressions/renderer.txt"),
            )),
            ..ProptestConfig::default()
        })]

        // any destination with any clip must never panic, and must never
        // write a single pixel outside the clip region
        #[test]
        fn draw_image_respects_clip_for_any_input(
            dx in any::<i32>(), dy in any::<i32>(),
            dw in any::<i32>(), dh in any::<i32>(),
            cx in -20..20i32, cy in -20..20i32,
            cw in 0..30i32, ch in 0..30i32,
        ) {
            let mut fb = Framebuffer::new(16, 16);
            fb.set_clip(Rect::new(cx, cy, cw, ch));
            let img = Image::from_rgba(2, 2, &[255; 16]).unwrap();
            fb.draw_image(&img, Rect::new(dx, dy, dw, dh), color(255, 255, 255, 255));
            let clip = Rect::new(cx, cy, cw, ch).intersect(Rect::new(0, 0, 16, 16));
            for y in 0..16 {
                for x in 0..16 {
                    let inside = x >= clip.x && x < clip.x + clip.width
                        && y >= clip.y && y < clip.y + clip.height;
                    if !inside {
                        prop_assert_eq!(fb.pixels[(y * 16 + x) as usize], 0);
                    }
                }
            }
        }

        // any rect with any clip must never panic, and must never write a
        // single pixel outside the clip region
        #[test]
        fn draw_rect_respects_clip_for_any_input(
            rx in any::<i32>(), ry in any::<i32>(),
            rw in any::<i32>(), rh in any::<i32>(),
            cx in -20..20i32, cy in -20..20i32,
            cw in 0..30i32, ch in 0..30i32,
            (r, g, b, a) in (any::<u8>(), any::<u8>(), any::<u8>(), any::<u8>()),
        ) {
            let mut fb = Framebuffer::new(16, 16);
            fb.set_clip(Rect::new(cx, cy, cw, ch));
            fb.draw_rect(Rect::new(rx, ry, rw, rh), color(r, g, b, a));
            let clip = Rect::new(cx, cy, cw, ch).intersect(Rect::new(0, 0, 16, 16));
            for y in 0..16 {
                for x in 0..16 {
                    let inside = x >= clip.x && x < clip.x + clip.width
                        && y >= clip.y && y < clip.y + clip.height;
                    if !inside {
                        prop_assert_eq!(fb.pixels[(y * 16 + x) as usize], 0);
                    }
                }
            }
        }
    }
}

//! Software renderer: fills rects and blits glyph masks into a CPU
//! framebuffer. Pixel format is 0x00RRGGBB, softbuffer's native layout.

use crate::font::Font;

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

    /// Touching edges count as overlapping; the rencache relies on this to
    /// merge adjacent dirty regions.
    pub fn overlaps(self, b: Rect) -> bool {
        b.x + b.width >= self.x
            && b.x <= self.x + self.width
            && b.y + b.height >= self.y
            && b.y <= self.y + self.height
    }

    pub fn intersect(self, b: Rect) -> Rect {
        let x1 = self.x.max(b.x);
        let y1 = self.y.max(b.y);
        let x2 = (self.x + self.width).min(b.x + b.width);
        let y2 = (self.y + self.height).min(b.y + b.height);
        Rect::new(x1, y1, (x2 - x1).max(0), (y2 - y1).max(0))
    }

    pub fn merge(self, b: Rect) -> Rect {
        let x1 = self.x.min(b.x);
        let y1 = self.y.min(b.y);
        let x2 = (self.x + self.width).max(b.x + b.width);
        let y2 = (self.y + self.height).max(b.y + b.height);
        Rect::new(x1, y1, x2 - x1, y2 - y1)
    }

    pub fn is_empty(self) -> bool {
        self.width <= 0 || self.height <= 0
    }
}

/// Exact (a * b) / 255 with rounding.
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
            pixels: vec![0; (width * height) as usize],
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
        self.pixels.resize((width * height) as usize, 0);
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
            let row = (j * self.width + r.x) as usize;
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

    /// Blits an alpha mask (a rasterized glyph) tinted with `color`.
    fn draw_mask(&mut self, mask: &[u8], mw: i32, x: i32, y: i32, color: Color) {
        if color.a == 0 || mask.is_empty() {
            return;
        }
        let mh = mask.len() as i32 / mw.max(1);
        let sub = Rect::new(x, y, mw, mh).intersect(self.clip);
        if sub.is_empty() {
            return;
        }
        for j in 0..sub.height {
            let src_row = ((sub.y - y + j) * mw + (sub.x - x)) as usize;
            let dst_row = ((sub.y + j) * self.width + sub.x) as usize;
            for i in 0..sub.width as usize {
                let a = mul255(mask[src_row + i], color.a);
                if a != 0 {
                    let px = &mut self.pixels[dst_row + i];
                    *px = blend(*px, color.r, color.g, color.b, a);
                }
            }
        }
    }

    /// Draws `text` with its top-left corner at (x, y); returns the pen x
    /// position after the last glyph.
    pub fn draw_text(&mut self, font: &Font, text: &str, x: i32, y: i32, color: Color) -> i32 {
        let mut pen = x;
        let baseline = y + font.ascent();
        for ch in text.chars() {
            if ch == '\t' {
                pen += font.tab_advance();
                continue;
            }
            let glyph = font.glyph(ch);
            self.draw_mask(
                &glyph.mask,
                glyph.width,
                pen + glyph.left,
                baseline - glyph.top,
                color,
            );
            pen += glyph.advance;
        }
        pen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // 50% white over black must give exactly 128 (rounded), not 127:
        // the >>8 approximation in lite darkened every blend.
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
}

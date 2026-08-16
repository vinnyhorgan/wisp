//! font loading and glyph rasterization on top of swash, which gives us
//! hinted, freetype-class glyphs in pure rust (lite used stb_truetype,
//! which does not hint).
//!
//! layout mirrors lite: sizes are pixels per em, advances are floored to
//! whole pixels, line height is ascent + descent + leading, and text is
//! drawn from the top of the line (the renderer adds the ascent to find
//! the baseline). tabs and newlines produce no pixels; the tab advance is
//! mutable at runtime via lua's `Font:set_tab_width()`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::rc::Rc;

use swash::scale::{Render, ScaleContext, Source};
use swash::{CacheKey, FontRef};

pub struct Glyph {
    pub advance: i32,
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub mask: Box<[u8]>,
}

pub struct Font {
    data: Vec<u8>,
    offset: u32,
    key: CacheKey,
    size: Cell<f32>,
    height: Cell<i32>,
    ascent: Cell<i32>,
    tab_advance: Cell<i32>,
    glyphs: RefCell<HashMap<char, Rc<Glyph>>>,
    context: RefCell<ScaleContext>,
}

impl Font {
    pub fn load(path: &Path, size: f32) -> io::Result<Font> {
        let data = std::fs::read(path)?;
        let font_ref = FontRef::from_index(&data, 0)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not a valid font"))?;
        let (offset, key) = (font_ref.offset, font_ref.key);
        let font = Font {
            data,
            offset,
            key,
            size: Cell::new(0.0),
            height: Cell::new(0),
            ascent: Cell::new(0),
            tab_advance: Cell::new(0),
            glyphs: RefCell::new(HashMap::new()),
            context: RefCell::new(ScaleContext::new()),
        };
        font.set_size(size);
        // a sane default until lua calls set_tab_width; lite left this as
        // whatever the font baked for the tab glyph
        font.tab_advance.set(font.glyph(' ').advance * 2);
        Ok(font)
    }

    /// re-scales the font in place: every holder of this font sees the
    /// new metrics, which is the whole point -- runtime zoom cannot
    /// chase down every captured reference (lite-xl's set_size, same
    /// shape). the glyph cache is dropped; the tab advance is left
    /// alone because the docview recomputes it on every draw. sizes
    /// clamp to [1, 2048]: a zero line height on the draw path is
    /// fatal, not cosmetic, and a glyph mask grows with the square of
    /// the size -- unclamped, a huge size is an abortable multi-gb
    /// allocation the moment anything rasterizes
    pub fn set_size(&self, size: f32) {
        let size = if size.is_finite() {
            size.clamp(1.0, 2048.0)
        } else {
            1.0
        };
        let m = self.font_ref().metrics(&[]).scale(size);
        self.size.set(size);
        self.height
            .set((m.ascent + m.descent + m.leading + 0.5) as i32);
        self.ascent.set((m.ascent + 0.5) as i32);
        self.glyphs.borrow_mut().clear();
    }

    pub fn size(&self) -> f32 {
        self.size.get()
    }

    /// swash's borrowed view into the font data; cheap to construct
    fn font_ref(&self) -> FontRef<'_> {
        FontRef {
            data: &self.data,
            offset: self.offset,
            key: self.key,
        }
    }

    pub fn height(&self) -> i32 {
        self.height.get()
    }

    pub fn ascent(&self) -> i32 {
        self.ascent.get()
    }

    pub fn tab_advance(&self) -> i32 {
        self.tab_advance.get()
    }

    pub fn set_tab_advance(&self, n: i32) {
        self.tab_advance.set(n);
    }

    /// rasterizes (or fetches from cache) the glyph for a codepoint
    pub fn glyph(&self, ch: char) -> Rc<Glyph> {
        if let Some(glyph) = self.glyphs.borrow().get(&ch) {
            return Rc::clone(glyph);
        }
        let font_ref = self.font_ref();
        let id = font_ref.charmap().map(ch);
        let advance = font_ref
            .glyph_metrics(&[])
            .scale(self.size.get())
            .advance_width(id)
            .floor() as i32;

        // newlines keep their advance but draw nothing, like in lite
        let glyph = if ch == '\n' {
            Rc::new(Glyph {
                advance,
                left: 0,
                top: 0,
                width: 0,
                mask: Box::new([]),
            })
        } else {
            let mut context = self.context.borrow_mut();
            let mut scaler = context
                .builder(font_ref)
                .size(self.size.get())
                .hint(true)
                .build();
            let image = Render::new(&[Source::Outline]).render(&mut scaler, id);
            let (left, top, width, mask) = match image {
                Some(img) => (
                    img.placement.left,
                    img.placement.top,
                    img.placement.width as i32,
                    img.data.into_boxed_slice(),
                ),
                None => (0, 0, 0, Box::new([]) as Box<[u8]>),
            };
            Rc::new(Glyph {
                advance,
                left,
                top,
                width,
                mask,
            })
        };
        self.glyphs.borrow_mut().insert(ch, Rc::clone(&glyph));
        glyph
    }

    /// width of `text` in pixels, honoring the current tab advance
    pub fn width_of(&self, text: &str) -> i32 {
        let mut x: i64 = 0;
        for ch in text.chars() {
            if ch == '\t' {
                x += self.tab_advance.get() as i64;
            } else {
                x += self.glyph(ch).advance as i64;
            }
        }
        x.clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }

    /// ink bounding box of `text` as (x, y, width, height) relative to the
    /// draw origin (top-left of the line box), or none for invisible text.
    /// ink can escape the metric box: nerd font icons fill the em square
    /// and overhang their advance, accents can rise above the ascent
    pub fn ink_box_of(&self, text: &str) -> Option<(i32, i32, i32, i32)> {
        let (mut pen, mut b): (i64, Option<[i64; 4]>) = (0, None);
        for ch in text.chars() {
            if ch == '\t' {
                pen += self.tab_advance.get() as i64;
                continue;
            }
            let g = self.glyph(ch);
            if g.width > 0 && !g.mask.is_empty() {
                let h = g.mask.len() as i64 / g.width as i64;
                let x0 = pen + g.left as i64;
                let y0 = (self.ascent.get() - g.top) as i64;
                let (x1, y1) = (x0 + g.width as i64, y0 + h);
                b = Some(match b {
                    None => [x0, y0, x1, y1],
                    Some(p) => [p[0].min(x0), p[1].min(y0), p[2].max(x1), p[3].max(y1)],
                });
            }
            pen += g.advance as i64;
        }
        b.map(|[x0, y0, x1, y1]| {
            let c = |v: i64| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            (c(x0), c(y0), c(x1 - x0), c(y1 - y0))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn font_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/jetbrainsmono.ttf")
    }

    #[test]
    fn loads_and_has_sane_metrics() {
        let font = Font::load(&font_path(), 14.0).unwrap();
        assert!(
            font.height() >= 14 && font.height() <= 24,
            "height {}",
            font.height()
        );
        assert!(font.ascent() > 0 && font.ascent() < font.height());
    }

    #[test]
    fn monospace_advances_are_uniform() {
        let font = Font::load(&font_path(), 14.0).unwrap();
        let w = font.glyph('m').advance;
        assert!(w > 0);
        for ch in "iMW0 .".chars() {
            assert_eq!(font.glyph(ch).advance, w, "advance of {ch:?}");
        }
        assert_eq!(font.width_of("hello"), 5 * w);
    }

    #[test]
    fn glyphs_have_pixels() {
        let font = Font::load(&font_path(), 14.0).unwrap();
        let g = font.glyph('A');
        assert!(g.width > 0 && !g.mask.is_empty());
        assert!(
            g.mask.iter().any(|&a| a > 128),
            "glyph should have opaque pixels"
        );
    }

    #[test]
    fn tab_and_newline_are_invisible() {
        let font = Font::load(&font_path(), 14.0).unwrap();
        assert!(font.glyph('\n').mask.is_empty());
        font.set_tab_advance(42);
        assert_eq!(font.width_of("\t"), 42);
        assert_eq!(font.width_of("a\tb"), 42 + 2 * font.glyph('a').advance);
    }

    #[test]
    fn set_size_rescales_in_place_and_drops_stale_glyphs() {
        let font = Font::load(&font_path(), 14.0).unwrap();
        let h14 = font.height();
        let w14 = font.glyph('m').advance;
        font.set_size(28.0);
        assert_eq!(font.size(), 28.0);
        assert!(font.height() > h14);
        assert!(
            font.glyph('m').advance > w14,
            "the glyph cache served a stale size"
        );
        // hostile sizes clamp instead of producing a zero line height,
        // which would be fatal on the draw path
        font.set_size(0.0);
        assert!(font.height() >= 1);
        font.set_size(f32::NAN);
        assert!(font.height() >= 1);
        // and the top end clamps too: unclamped, rasterizing a glyph at
        // a huge size is a multi-gb allocation that aborts the process
        font.set_size(1e9);
        assert_eq!(font.size(), 2048.0);
        assert!(!font.glyph('m').mask.is_empty());
    }

    #[test]
    fn missing_font_file_is_an_error() {
        assert!(Font::load(Path::new("/nonexistent.ttf"), 14.0).is_err());
    }

    #[test]
    fn garbage_font_data_is_an_error_not_a_panic() {
        let dir = std::env::temp_dir().join("wisp-font-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("garbage.ttf");
        std::fs::write(&path, b"this is not a font at all").unwrap();
        assert!(Font::load(&path, 14.0).is_err());
    }

    #[test]
    fn exotic_codepoints_do_not_collide() {
        // lite aliased glyph pages: `codepoint >> 8 % 256` mapped u+10400
        // onto the same page as u+0400; ours must keep them distinct
        let font = Font::load(&font_path(), 14.0).unwrap();
        let a = font.glyph('\u{10400}');
        let b = font.glyph('\u{0400}');
        // under lite's aliasing these were literally the same glyph slot;
        // ours must be independent allocations, each cached under its own
        // codepoint
        assert!(!Rc::ptr_eq(&a, &b));
        assert!(font.glyphs.borrow().contains_key(&'\u{10400}'));
        assert!(font.glyphs.borrow().contains_key(&'\u{0400}'));
    }

    #[test]
    fn ink_box_reports_glyphs_escaping_their_metrics() {
        let font = Font::load(&font_path(), 14.0).unwrap();
        assert!(font.ink_box_of("").is_none());
        assert!(font.ink_box_of("\t").is_none());
        let (_, _, w, h) = font.ink_box_of("A").unwrap();
        assert!(w > 0 && h > 0);
        // at least one of the icons the editor uses inks outside its
        // metric box (advance x height); the render cache depends on
        // ink_box_of reporting that overhang
        let icons = "\u{F016}\u{F07B}\u{F07C}\u{F054}\u{F078}\u{F013}\u{F05A}\u{F071}";
        let escapes = icons.chars().any(|ch| {
            let s = ch.to_string();
            let (ix, iy, iw, ih) = font.ink_box_of(&s).unwrap();
            ix < 0 || iy < 0 || ix + iw > font.width_of(&s) || iy + ih > font.height()
        });
        assert!(escapes, "expected some icon ink outside the metric box");
    }

    #[test]
    fn absurdly_long_text_does_not_overflow_width() {
        let font = Font::load(&font_path(), 14.0).unwrap();
        let text = "m".repeat(1 << 20);
        let w = font.width_of(&text);
        assert!(w > 0);
        // and a width that would exceed i32 saturates instead of wrapping
        font.set_tab_advance(i32::MAX);
        assert_eq!(font.width_of("\t\t\t"), i32::MAX);
    }
}

//! A cache over the software renderer -- all drawing operations are stored
//! as commands when issued. At the end of the frame the commands are hashed
//! into a grid of cells; cells that changed since the previous frame are
//! merged into dirty rectangles and only those regions are redrawn.
//!
//! This is a faithful port of lite's rencache.c with the fixed-size buffers
//! replaced by growable ones (no more dropped commands on huge frames, no
//! out-of-bounds cells on displays wider than 7680px).

use std::rc::Rc;

use crate::font::Font;
use crate::renderer::{Color, Framebuffer, Rect};

pub const CELL_SIZE: i32 = 96;

const HASH_INITIAL: u32 = 2166136261;

#[derive(Default)]
struct Fnv(u32);

impl Fnv {
    fn new() -> Fnv {
        Fnv(HASH_INITIAL)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ b as u32).wrapping_mul(16777619);
        }
    }

    fn write_i32(&mut self, v: i32) {
        self.write(&v.to_le_bytes());
    }
}

enum Command {
    SetClip {
        rect: Rect,
    },
    DrawRect {
        rect: Rect,
        color: Color,
    },
    DrawText {
        font: Rc<Font>,
        text: String,
        rect: Rect,
        color: Color,
        tab_advance: i32,
    },
}

impl Command {
    fn rect(&self) -> Rect {
        match self {
            Command::SetClip { rect }
            | Command::DrawRect { rect, .. }
            | Command::DrawText { rect, .. } => *rect,
        }
    }

    fn hash(&self) -> u32 {
        let mut h = Fnv::new();
        let rect = self.rect();
        h.write_i32(rect.x);
        h.write_i32(rect.y);
        h.write_i32(rect.width);
        h.write_i32(rect.height);
        match self {
            Command::SetClip { .. } => h.write(&[0]),
            Command::DrawRect { color, .. } => {
                h.write(&[1, color.r, color.g, color.b, color.a]);
            }
            Command::DrawText {
                font,
                text,
                color,
                tab_advance,
                ..
            } => {
                h.write(&[2, color.r, color.g, color.b, color.a]);
                h.write(&(Rc::as_ptr(font) as usize).to_le_bytes());
                h.write_i32(*tab_advance);
                h.write(text.as_bytes());
            }
        }
        h.0
    }
}

/// Tiny deterministic xorshift; only used to color the debug overlay.
struct XorShift(u32);

impl XorShift {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
}

pub struct RenCache {
    commands: Vec<Command>,
    cells: Vec<u32>,
    cells_prev: Vec<u32>,
    grid_w: i32,
    grid_h: i32,
    screen: Rect,
    show_debug: bool,
    rng: XorShift,
}

impl Default for RenCache {
    fn default() -> Self {
        Self::new()
    }
}

impl RenCache {
    pub fn new() -> RenCache {
        RenCache {
            commands: Vec::new(),
            cells: Vec::new(),
            cells_prev: Vec::new(),
            grid_w: 0,
            grid_h: 0,
            screen: Rect::default(),
            show_debug: false,
            rng: XorShift(0x12345678),
        }
    }

    pub fn show_debug(&mut self, enable: bool) {
        self.show_debug = enable;
    }

    pub fn invalidate(&mut self) {
        self.cells_prev.fill(u32::MAX);
    }

    pub fn begin_frame(&mut self, width: i32, height: i32) {
        if self.screen.width != width || self.screen.height != height {
            self.screen = Rect::new(0, 0, width, height);
            self.grid_w = width / CELL_SIZE + 1;
            self.grid_h = height / CELL_SIZE + 1;
            let n = (self.grid_w * self.grid_h) as usize;
            self.cells.clear();
            self.cells.resize(n, HASH_INITIAL);
            self.cells_prev.clear();
            self.cells_prev.resize(n, u32::MAX);
        }
    }

    pub fn set_clip_rect(&mut self, rect: Rect) {
        self.commands.push(Command::SetClip {
            rect: rect.intersect(self.screen),
        });
    }

    pub fn draw_rect(&mut self, rect: Rect, color: Color) {
        if !self.screen.overlaps(rect) {
            return;
        }
        self.commands.push(Command::DrawRect { rect, color });
    }

    pub fn draw_text(&mut self, font: &Rc<Font>, text: &str, x: i32, y: i32, color: Color) -> i32 {
        let rect = Rect::new(x, y, font.width_of(text), font.height());
        if self.screen.overlaps(rect) {
            self.commands.push(Command::DrawText {
                font: Rc::clone(font),
                text: text.to_owned(),
                rect,
                color,
                tab_advance: font.tab_advance(),
            });
        }
        x + rect.width
    }

    fn update_overlapping_cells(&mut self, r: Rect, h: u32) {
        let x1 = (r.x / CELL_SIZE).max(0);
        let y1 = (r.y / CELL_SIZE).max(0);
        let x2 = ((r.x + r.width) / CELL_SIZE).min(self.grid_w - 1);
        let y2 = ((r.y + r.height) / CELL_SIZE).min(self.grid_h - 1);
        for y in y1..=y2 {
            for x in x1..=x2 {
                let idx = (x + y * self.grid_w) as usize;
                let mut f = Fnv(self.cells[idx]);
                f.write(&h.to_le_bytes());
                self.cells[idx] = f.0;
            }
        }
    }

    /// Replays the frame's commands into `fb` over each dirty region and
    /// returns the dirty rectangles that need presenting.
    pub fn end_frame(&mut self, fb: &mut Framebuffer) -> Vec<Rect> {
        // update cells from commands
        let mut cr = self.screen;
        let updates: Vec<(Rect, u32)> = self
            .commands
            .iter()
            .filter_map(|cmd| {
                if let Command::SetClip { rect } = cmd {
                    cr = *rect;
                }
                let r = cmd.rect().intersect(cr);
                if r.is_empty() {
                    None
                } else {
                    Some((r, cmd.hash()))
                }
            })
            .collect();
        for (r, h) in updates {
            self.update_overlapping_cells(r, h);
        }

        // push a rect for each cell changed since the last frame, merging
        // overlapping rects as we go
        let mut rects: Vec<Rect> = Vec::new();
        for y in 0..self.grid_h {
            for x in 0..self.grid_w {
                let idx = (x + y * self.grid_w) as usize;
                if self.cells[idx] != self.cells_prev[idx] {
                    push_rect(&mut rects, Rect::new(x, y, 1, 1));
                }
                self.cells_prev[idx] = HASH_INITIAL;
            }
        }

        // expand rects from cells to pixels
        for r in &mut rects {
            r.x *= CELL_SIZE;
            r.y *= CELL_SIZE;
            r.width *= CELL_SIZE;
            r.height *= CELL_SIZE;
            *r = r.intersect(self.screen);
        }
        rects.retain(|r| !r.is_empty());

        // redraw updated regions
        for &r in &rects {
            fb.set_clip(r);
            for cmd in &self.commands {
                match cmd {
                    Command::SetClip { rect } => fb.set_clip(rect.intersect(r)),
                    Command::DrawRect { rect, color } => fb.draw_rect(*rect, *color),
                    Command::DrawText {
                        font,
                        text,
                        rect,
                        color,
                        tab_advance,
                    } => {
                        font.set_tab_advance(*tab_advance);
                        fb.draw_text(font, text, rect.x, rect.y, *color);
                    }
                }
            }
            if self.show_debug {
                let n = self.rng.next();
                let color = Color {
                    r: n as u8,
                    g: (n >> 8) as u8,
                    b: (n >> 16) as u8,
                    a: 50,
                };
                fb.set_clip(r);
                fb.draw_rect(r, color);
            }
        }

        // swap cell buffers, drop this frame's commands (any font kept alive
        // only by a command is released here -- lite needed an explicit
        // FREE_FONT command queue for this; Rc does it for us)
        std::mem::swap(&mut self.cells, &mut self.cells_prev);
        self.commands.clear();
        rects
    }
}

fn push_rect(rects: &mut Vec<Rect>, r: Rect) {
    for rp in rects.iter_mut().rev() {
        if rp.overlaps(r) {
            *rp = rp.merge(r);
            return;
        }
    }
    rects.push(r);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(cache: &mut RenCache, fb: &mut Framebuffer, draws: &[(Rect, Color)]) -> Vec<Rect> {
        cache.begin_frame(fb.width, fb.height);
        cache.set_clip_rect(Rect::new(0, 0, fb.width, fb.height));
        for &(r, c) in draws {
            cache.draw_rect(r, c);
        }
        cache.end_frame(fb)
    }

    const WHITE: Color = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    const RED: Color = Color {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };

    #[test]
    fn identical_frames_produce_no_dirty_rects() {
        let mut cache = RenCache::new();
        let mut fb = Framebuffer::new(400, 300);
        let draws = [(Rect::new(10, 10, 50, 50), WHITE)];
        let first = frame(&mut cache, &mut fb, &draws);
        assert!(!first.is_empty(), "first frame must redraw");
        for _ in 0..10 {
            let rects = frame(&mut cache, &mut fb, &draws);
            assert!(rects.is_empty(), "unchanged frame redrew {rects:?}");
        }
    }

    #[test]
    fn changed_region_is_redrawn_and_covers_the_change() {
        let mut cache = RenCache::new();
        let mut fb = Framebuffer::new(400, 300);
        frame(&mut cache, &mut fb, &[(Rect::new(10, 10, 50, 50), WHITE)]);
        let rects = frame(&mut cache, &mut fb, &[(Rect::new(10, 10, 50, 50), RED)]);
        assert!(!rects.is_empty());
        let change = Rect::new(10, 10, 50, 50);
        assert!(
            rects.iter().any(|r| r.intersect(change) == change),
            "dirty rects {rects:?} must cover the changed rect"
        );
        // and the framebuffer actually shows the new color
        assert_eq!(fb.pixels[(20 * 400 + 20) as usize], 0xff0000);
    }

    #[test]
    fn resize_invalidates_everything() {
        let mut cache = RenCache::new();
        let mut fb = Framebuffer::new(400, 300);
        frame(&mut cache, &mut fb, &[(Rect::new(0, 0, 400, 300), WHITE)]);
        fb.resize(500, 400);
        let rects = frame(&mut cache, &mut fb, &[(Rect::new(0, 0, 500, 400), WHITE)]);
        let total: i64 = rects.iter().map(|r| r.width as i64 * r.height as i64).sum();
        assert!(total >= 500 * 400, "resize must redraw the whole screen");
    }

    #[test]
    fn huge_screen_does_not_panic() {
        // lite wrote out of bounds past 80x50 cells (7680x4800 px)
        let mut cache = RenCache::new();
        let mut fb = Framebuffer::new(10000, 6000);
        let rects = frame(
            &mut cache,
            &mut fb,
            &[(Rect::new(9000, 5000, 500, 500), WHITE)],
        );
        assert!(!rects.is_empty());
    }

    #[test]
    fn invalidate_forces_full_redraw() {
        let mut cache = RenCache::new();
        let mut fb = Framebuffer::new(400, 300);
        let draws = [(Rect::new(10, 10, 50, 50), WHITE)];
        frame(&mut cache, &mut fb, &draws);
        cache.invalidate();
        let rects = frame(&mut cache, &mut fb, &draws);
        let total: i64 = rects.iter().map(|r| r.width as i64 * r.height as i64).sum();
        assert!(total >= 400 * 300);
    }

    #[test]
    fn clip_rect_limits_dirty_region() {
        let mut cache = RenCache::new();
        let mut fb = Framebuffer::new(400, 300);
        // frame 1: just a small clip
        cache.begin_frame(400, 300);
        cache.set_clip_rect(Rect::new(0, 0, 50, 50));
        cache.end_frame(&mut fb);
        // frame 2: same clip plus a draw entirely outside it -- the clipped
        // draw must not dirty anything
        cache.begin_frame(400, 300);
        cache.set_clip_rect(Rect::new(0, 0, 50, 50));
        cache.draw_rect(Rect::new(200, 200, 50, 50), WHITE);
        let rects = cache.end_frame(&mut fb);
        assert!(rects.is_empty(), "fully clipped draw dirtied {rects:?}");
    }
}

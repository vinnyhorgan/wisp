//! The headless platform: a scripted event queue, a virtual clock, and a
//! captured framebuffer. It runs the real, unmodified editor Lua with zero
//! windowing -- this is how wisp tests the whole editor in CI.

use std::any::Any;
use std::collections::VecDeque;

use crate::api::{Engine, Shared};
use crate::boot::{self, Resume, Yield};
use crate::platform::{Event, Platform};
use crate::renderer::{Framebuffer, Rect};

pub struct HeadlessPlatform {
    pub queue: VecDeque<Event>,
    pub clock: f64,
    pub size: (i32, i32),
    pub focus: bool,
    pub clipboard: String,
    pub title: String,
    pub frame_count: usize,
    pub last_frame: Vec<u32>,
}

impl HeadlessPlatform {
    pub fn new(width: i32, height: i32) -> HeadlessPlatform {
        HeadlessPlatform {
            queue: VecDeque::new(),
            clock: 0.0,
            size: (width, height),
            focus: true,
            clipboard: String::new(),
            title: String::new(),
            frame_count: 0,
            last_frame: Vec::new(),
        }
    }
}

impl Platform for HeadlessPlatform {
    fn poll_event(&mut self) -> Option<Event> {
        self.queue.pop_front()
    }

    fn now(&self) -> f64 {
        self.clock
    }

    fn window_size(&self) -> (i32, i32) {
        self.size
    }

    fn window_has_focus(&self) -> bool {
        self.focus
    }

    fn set_window_title(&mut self, title: &str) {
        self.title = title.to_owned();
    }

    fn set_window_mode(&mut self, _mode: &str) {}

    fn set_cursor(&mut self, _cursor: &str) {}

    fn get_clipboard(&mut self) -> Option<String> {
        Some(self.clipboard.clone())
    }

    fn set_clipboard(&mut self, text: &str) {
        self.clipboard = text.to_owned();
    }

    fn present(&mut self, fb: &Framebuffer, _rects: &[Rect]) {
        self.frame_count += 1;
        self.last_frame.clone_from(&fb.pixels);
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Boots the real editor on a headless platform and drives its coroutine,
/// advancing the virtual clock through every wait/sleep yield.
pub struct Headless {
    pub engine: Shared,
    _lua: mlua::Lua,
    thread: mlua::Thread,
    pending: Resume,
    pub exited: Option<i32>,
}

impl Headless {
    /// `project_dir` becomes the editor's project directory (keep it stable
    /// and minimal: its listing is rendered by the treeview). The editor
    /// finds `data/` via this crate's manifest dir.
    pub fn boot(project_dir: &str, width: i32, height: i32) -> Headless {
        let exedir = env!("CARGO_MANIFEST_DIR").to_owned();
        let engine = Engine::shared(Box::new(HeadlessPlatform::new(width, height)));
        let args = vec!["wisp".to_owned(), project_dir.to_owned()];
        let (lua, thread) = boot::init_lua(
            &engine,
            &exedir,
            &format!("{exedir}/wisp"),
            &args,
            1.0,
            true,
        )
        .expect("lua boot failed");
        Headless {
            engine,
            _lua: lua,
            thread,
            pending: Resume::Start,
            exited: None,
        }
    }

    fn with_platform<R>(&self, f: impl FnOnce(&mut HeadlessPlatform) -> R) -> R {
        let mut engine = self.engine.borrow_mut();
        let platform = engine
            .platform
            .as_any_mut()
            .downcast_mut::<HeadlessPlatform>()
            .expect("headless platform");
        f(platform)
    }

    pub fn push_event(&self, event: Event) {
        self.with_platform(|p| p.queue.push_back(event));
    }

    pub fn frame_count(&self) -> usize {
        self.with_platform(|p| p.frame_count)
    }

    pub fn last_frame(&self) -> (Vec<u32>, i32, i32) {
        let (w, h) = self.with_platform(|p| p.size);
        (self.with_platform(|p| p.last_frame.clone()), w, h)
    }

    pub fn window_title(&self) -> String {
        self.with_platform(|p| p.title.clone())
    }

    /// Resumes the editor coroutine once and advances the virtual clock as
    /// dictated by the yield. Returns false once the editor has exited.
    pub fn step(&mut self) -> bool {
        if self.exited.is_some() {
            return false;
        }
        let pending = std::mem::replace(&mut self.pending, Resume::Start);
        match boot::resume(&self.thread, pending) {
            Yield::Wait(timeout) => {
                let has_event = self.with_platform(|p| !p.queue.is_empty());
                if !has_event {
                    self.with_platform(|p| p.clock += timeout.max(0.0));
                }
                self.pending = Resume::EventAvailable(has_event);
            }
            Yield::Sleep(secs) => {
                self.with_platform(|p| p.clock += secs.max(0.0));
                self.pending = Resume::Start;
            }
            Yield::Exit(code) => {
                self.exited = Some(code);
                return false;
            }
        }
        true
    }

    /// Steps until the editor has presented at least `frames` frames.
    /// Panics after `max_steps` so a hung editor fails loudly.
    pub fn run_until_frames(&mut self, frames: usize, max_steps: usize) {
        for _ in 0..max_steps {
            if self.frame_count() >= frames {
                return;
            }
            if !self.step() {
                panic!(
                    "editor exited (code {:?}) before presenting {frames} frames",
                    self.exited
                );
            }
        }
        panic!(
            "editor did not present {frames} frames within {max_steps} steps (got {})",
            self.frame_count()
        );
    }
}

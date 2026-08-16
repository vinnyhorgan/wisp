//! the headless platform: a scripted event queue, a virtual clock, and a
//! captured framebuffer. it runs the real, unmodified editor lua with zero
//! windowing -- this is how wisp tests the whole editor.
//!
//! time only passes when the editor asks it to (wait_event and sleep
//! advance the virtual clock), so every frame is bit-for-bit reproducible:
//! caret blinks, double clicks and background threads all behave the same
//! on every run.

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
    last_frame_size: (i32, i32),
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
            last_frame_size: (0, 0),
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
        self.last_frame_size = (fb.width, fb.height);
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// boots the real editor on a headless platform and drives its coroutine,
/// advancing the virtual clock through every wait and sleep yield
pub struct Headless {
    pub engine: Shared,
    pub exited: Option<i32>,
    // the state must outlive the coroutine that lives in it
    _lua: mlua::Lua,
    thread: mlua::Thread,
    pending: Resume,
}

impl Headless {
    /// `project_dir` becomes the editor's project directory; keep it stable
    /// and minimal, its listing is rendered by the treeview. the editor
    /// finds data/ via this crate's manifest dir
    pub fn boot(project_dir: &str, width: i32, height: i32, scale: f64) -> Headless {
        Self::boot_with_exedir(
            env!("CARGO_MANIFEST_DIR"),
            project_dir,
            width,
            height,
            scale,
        )
    }

    /// boot with no directory argument, as `wisp` run bare from a shell:
    /// the editor opens the process cwd as its project
    pub fn boot_bare(width: i32, height: i32, scale: f64) -> Headless {
        Self::boot_args(env!("CARGO_MANIFEST_DIR"), &[], width, height, scale)
    }

    /// boot against a different editor root (a directory holding a `data/`
    /// tree): tests use this to inject a user module or a modified plugin
    /// without touching the repo's own data
    pub fn boot_with_exedir(
        exedir: &str,
        project_dir: &str,
        width: i32,
        height: i32,
        scale: f64,
    ) -> Headless {
        Self::boot_args(exedir, &[project_dir], width, height, scale)
    }

    fn boot_args(exedir: &str, args: &[&str], width: i32, height: i32, scale: f64) -> Headless {
        let exedir = exedir.to_owned();
        let engine = Engine::shared(Box::new(HeadlessPlatform::new(width, height)));
        let mut args: Vec<std::ffi::OsString> = args.iter().map(|a| a.into()).collect();
        args.insert(0, "wisp".into());
        let (lua, thread) = boot::init_lua(
            &engine,
            &exedir,
            &format!("{exedir}/wisp"),
            &args,
            scale,
            true,
        )
        .expect("lua boot failed");
        Headless {
            engine,
            exited: None,
            _lua: lua,
            thread,
            pending: Resume::Start,
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
        // dimensions are captured with the pixels at present time, so they
        // agree even if the window was resized since
        self.with_platform(|p| {
            let (w, h) = p.last_frame_size;
            (p.last_frame.clone(), w, h)
        })
    }

    /// resizes the virtual window: updates the reported size and delivers
    /// the resized event, like a real windowing system would
    pub fn resize(&self, width: i32, height: i32) {
        self.with_platform(|p| {
            p.size = (width, height);
            p.queue.push_back(Event::Resized(width, height));
        });
    }

    pub fn window_title(&self) -> String {
        self.with_platform(|p| p.title.clone())
    }

    /// sets the virtual clock. it boots at 0.0, which is integral -- a
    /// value with a fractional part exercises require-time code the way
    /// a real desktop clock does (lua's %d and %x refuse non-integral
    /// floats since 5.3, and get_time() is never integral on desktop)
    pub fn set_clock(&self, t: f64) {
        self.with_platform(|p| p.clock = t);
    }

    /// focuses or unfocuses the virtual window, like alt-tabbing would;
    /// an unfocused editor draws no caret, which lets tests compare
    /// frames without accounting for the blink phase
    pub fn set_focus(&self, focus: bool) {
        self.with_platform(|p| p.focus = focus);
    }

    /// resumes the editor coroutine once and advances the virtual clock as
    /// dictated by the yield; returns false once the editor has exited
    pub fn step(&mut self) -> bool {
        if self.exited.is_some() {
            return false;
        }
        let pending = self.pending;
        self.pending = Resume::Start;
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
            }
            Yield::Exit(code) => {
                self.exited = Some(code);
                return false;
            }
        }
        true
    }

    /// steps until the editor has presented at least `frames` frames;
    /// panics after `max_steps` so a hung editor fails loudly
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

    /// steps `n` times, stopping early if the editor exits; useful for
    /// letting the editor settle or process queued events
    pub fn run_steps(&mut self, n: usize) {
        for _ in 0..n {
            if !self.step() {
                return;
            }
        }
    }
}

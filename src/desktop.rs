//! the desktop platform: winit windowing, softbuffer presentation,
//! arboard clipboard -- and the driver that runs the editor coroutine
//! inside the winit event loop.
//!
//! winit owns the os event loop, as it is designed to; the editor lua
//! believes it owns its own loop, as it is designed to. the coroutine
//! protocol in boot.rs lets both be true: when lua calls wait_event or
//! sleep, the coroutine parks, control returns here, and winit blocks in
//! the os with waituntil. idle costs nothing.

use std::any::Any;
use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{CursorIcon, Fullscreen, Window, WindowId};

use crate::api::{Engine, Shared};
use crate::boot::{self, Resume, Yield};
use crate::keys;
use crate::platform::{Event, Platform};
use crate::renderer::{Framebuffer, Rect};

pub struct DesktopPlatform {
    pub queue: VecDeque<Event>,
    pub focus: bool,
    start: Instant,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    surface_size: (u32, u32),
    shown: bool,
    clipboard: Option<arboard::Clipboard>,
    // echoes back what the editor last copied, so copy/paste inside the
    // editor keeps working when the system clipboard is broken or absent
    clipboard_fallback: String,
}

impl DesktopPlatform {
    fn new(start: Instant) -> DesktopPlatform {
        DesktopPlatform {
            queue: VecDeque::new(),
            focus: false,
            start,
            window: None,
            surface: None,
            surface_size: (0, 0),
            shown: false,
            clipboard: None,
            clipboard_fallback: String::new(),
        }
    }
}

impl Platform for DesktopPlatform {
    fn poll_event(&mut self) -> Option<Event> {
        self.queue.pop_front()
    }

    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn window_size(&self) -> (i32, i32) {
        match &self.window {
            Some(window) => {
                let size = window.inner_size();
                (size.width as i32, size.height as i32)
            }
            None => (0, 0),
        }
    }

    fn window_has_focus(&self) -> bool {
        self.focus
    }

    fn set_window_title(&mut self, title: &str) {
        if let Some(window) = &self.window {
            window.set_title(title);
        }
    }

    fn set_window_mode(&mut self, mode: &str) {
        let Some(window) = &self.window else { return };
        match mode {
            "fullscreen" => window.set_fullscreen(Some(Fullscreen::Borderless(None))),
            "maximized" => {
                window.set_fullscreen(None);
                window.set_maximized(true);
            }
            _ => {
                window.set_fullscreen(None);
                window.set_maximized(false);
            }
        }
    }

    fn set_cursor(&mut self, cursor: &str) {
        let Some(window) = &self.window else { return };
        let icon = match cursor {
            "ibeam" => CursorIcon::Text,
            "sizeh" => CursorIcon::EwResize,
            "sizev" => CursorIcon::NsResize,
            "hand" => CursorIcon::Pointer,
            _ => CursorIcon::Default,
        };
        window.set_cursor(icon);
    }

    fn get_clipboard(&mut self) -> Option<String> {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        if let Some(clipboard) = &mut self.clipboard {
            match clipboard.get_text() {
                Ok(text) => return Some(text),
                // a working clipboard holding no text (an image, or
                // nothing at all) is an empty paste -- like sdl's "" in
                // lite -- not a cue to resurrect an old copy
                Err(arboard::Error::ContentNotAvailable) => return Some(String::new()),
                // a broken or absent clipboard: fall through to the echo
                Err(_) => {}
            }
        }
        Some(self.clipboard_fallback.clone())
    }

    fn set_clipboard(&mut self, text: &str) {
        self.clipboard_fallback = text.to_owned();
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        if let Some(clipboard) = &mut self.clipboard {
            let _ = clipboard.set_text(text);
        }
    }

    fn present(&mut self, fb: &Framebuffer, rects: &[Rect]) {
        let (Some(surface), Some(window)) = (&mut self.surface, &self.window) else {
            return;
        };
        let (Some(w), Some(h)) = (
            NonZeroU32::new(fb.width as u32),
            NonZeroU32::new(fb.height as u32),
        ) else {
            return;
        };
        if self.surface_size != (w.get(), h.get()) {
            if surface.resize(w, h).is_err() {
                return;
            }
            self.surface_size = (w.get(), h.get());
        }
        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };
        if buffer.len() != fb.pixels.len() {
            return;
        }
        buffer.copy_from_slice(&fb.pixels);
        let damage: Vec<softbuffer::Rect> = rects
            .iter()
            .filter_map(|r| {
                Some(softbuffer::Rect {
                    x: r.x as u32,
                    y: r.y as u32,
                    width: NonZeroU32::new(r.width as u32)?,
                    height: NonZeroU32::new(r.height as u32)?,
                })
            })
            .collect();
        let _ = buffer.present_with_damage(&damage);
        if !self.shown {
            window.set_visible(true);
            self.shown = true;
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

enum Parked {
    Start,
    Wait { deadline: f64 },
    Sleep { deadline: f64 },
    Done(i32),
}

/// turns raw button presses into lite's click counts. repeat presses of
/// the same button within half a second and `radius` pixels count up,
/// and the count cycles after a triple click so mashing goes caret,
/// word, line, caret, ... instead of counting up forever like sdl did
struct ClickCounter {
    time: f64,
    button: &'static str,
    pos: (f64, f64),
    clicks: i32,
}

impl ClickCounter {
    fn new() -> ClickCounter {
        ClickCounter {
            time: -1.0,
            button: "?",
            pos: (0.0, 0.0),
            clicks: 0,
        }
    }

    fn press(&mut self, button: &'static str, now: f64, pos: (f64, f64), radius: f64) -> i32 {
        let near = (pos.0 - self.pos.0).abs() < radius && (pos.1 - self.pos.1).abs() < radius;
        if button == self.button && now - self.time < 0.5 && near {
            self.clicks = self.clicks % 3 + 1;
        } else {
            self.clicks = 1;
        }
        self.time = now;
        self.button = button;
        self.pos = pos;
        self.clicks
    }
}

struct App {
    engine: Shared,
    exedir: String,
    start: Instant,
    lua: Option<(mlua::Lua, mlua::Thread)>,
    parked: Parked,
    mods: ModifiersState,
    /// last known cursor position; none until the first real motion, so
    /// the first move cannot report a phantom delta from the origin
    cursor: Option<(f64, f64)>,
    clicks: ClickCounter,
    /// set from a signal handler, cleared when the editor is told
    terminated: Arc<std::sync::atomic::AtomicBool>,
}

impl App {
    fn with_platform<R>(&self, f: impl FnOnce(&mut DesktopPlatform) -> R) -> R {
        let mut engine = self.engine.borrow_mut();
        let platform = engine
            .platform
            .as_any_mut()
            .downcast_mut::<DesktopPlatform>()
            .expect("desktop platform");
        f(platform)
    }

    fn push(&self, event: Event) {
        self.with_platform(|p| p.queue.push_back(event));
    }

    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn deadline_instant(&self, t: f64) -> Instant {
        self.start + Duration::from_secs_f64(clamp_deadline(self.now(), t))
    }
}

/// a plugin can ask to wait forever (`wait_event(math.huge)`), and
/// `Duration::from_secs_f64` panics on non-finite or oversized values:
/// clamp the deadline to a day out. waking idle once a day costs
/// nothing, and the wait loop re-parks on the still-infinite deadline
fn clamp_deadline(now: f64, t: f64) -> f64 {
    let far = now + 86_400.0;
    if t.is_finite() {
        t.clamp(0.0, far)
    } else {
        far
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.lua.is_some() {
            return;
        }

        let (w, h) = event_loop
            .primary_monitor()
            .map(|m| {
                let size = m.size();
                (size.width * 4 / 5, size.height * 4 / 5)
            })
            .filter(|&(w, h)| w > 0 && h > 0)
            .unwrap_or((1024, 768));
        let attrs = Window::default_attributes()
            .with_title("")
            .with_visible(false)
            .with_inner_size(PhysicalSize::new(w, h));
        // wayland matches a window to its .desktop file by app id, x11 by
        // WM_CLASS; a window with neither gets a placeholder icon and no
        // grouping in whatever the desktop uses for a task list. winit
        // keeps both in one field, so naming it once covers both
        #[cfg(all(unix, not(target_os = "macos")))]
        let attrs = {
            use winit::platform::wayland::WindowAttributesExtWayland;
            attrs.with_name("wisp", "wisp")
        };
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );
        let context =
            softbuffer::Context::new(window.clone()).expect("failed to create softbuffer context");
        let surface = softbuffer::Surface::new(&context, window.clone())
            .expect("failed to create softbuffer surface");
        let scale = window.scale_factor();
        self.with_platform(|p| {
            p.window = Some(window);
            p.surface = Some(surface);
        });

        let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
        let exefile = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| format!("{}/wisp", self.exedir));
        let (lua, thread) =
            boot::init_lua(&self.engine, &self.exedir, &exefile, &args, scale, false)
                .expect("failed to initialize lua");
        self.lua = Some((lua, thread));
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => self.push(Event::Quit),
            WindowEvent::Resized(size) => {
                // minimized windows report 0x0; lite never saw those from
                // sdl, so the lua layer is not built to lay out at zero
                if size.width > 0 && size.height > 0 {
                    self.push(Event::Resized(size.width as i32, size.height as i32));
                }
            }
            WindowEvent::RedrawRequested => self.push(Event::Exposed),
            WindowEvent::Focused(focus) => self.with_platform(|p| p.focus = focus),
            WindowEvent::ModifiersChanged(mods) => self.mods = mods.state(),
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                // x11 synthesizes presses for every held key when focus
                // returns; forwarding them would type a tab on alt-tab
                // (the exact sdl bug lite worked around). releases still
                // pass so held modifiers unstick
                if is_synthetic && event.state == ElementState::Pressed {
                    return;
                }
                // name keys by their unshifted character, like sdl did:
                // the stock keymap binds "ctrl+shift+[", not "ctrl+shift+{"
                let key = event.key_without_modifiers();
                let name = keys::key_name(&key, event.location);
                match event.state {
                    ElementState::Pressed => {
                        if let Some(name) = name {
                            self.push(Event::KeyPressed(name));
                        }
                        // super rides along with ctrl for the day this
                        // runs on a mac, where cmd chords carry text
                        if let Some(text) = &event.text
                            && !self.mods.control_key()
                            && !self.mods.super_key()
                            && !text.is_empty()
                            && !text.chars().any(|c| c.is_control())
                        {
                            self.push(Event::TextInput(text.to_string()));
                        }
                    }
                    ElementState::Released => {
                        if let Some(name) = name {
                            self.push(Event::KeyReleased(name));
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                // deltas are differences of truncated positions, so a
                // stream of sub-pixel moves still sums to the true travel
                // (truncating each fractional delta would drift to zero);
                // the very first move has no history and delta zero
                let prev = self.cursor.unwrap_or((position.x, position.y));
                let (x, y) = (position.x as i32, position.y as i32);
                let (dx, dy) = (x - prev.0 as i32, y - prev.1 as i32);
                self.cursor = Some((position.x, position.y));
                self.push(Event::MouseMoved(x, y, dx, dy));
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let name = match button {
                    MouseButton::Left => "left",
                    MouseButton::Middle => "middle",
                    MouseButton::Right => "right",
                    _ => "?",
                };
                // winit's button events carry no coordinates (sdl's did);
                // a press before any motion falls back to the origin
                let pos = self.cursor.unwrap_or((0.0, 0.0));
                let (x, y) = (pos.0 as i32, pos.1 as i32);
                match state {
                    ElementState::Pressed => {
                        // the double-click slop scales with the display,
                        // so 2x screens keep the same physical feel
                        let radius = 8.0
                            * self
                                .with_platform(|p| p.window.as_ref().map(|w| w.scale_factor()))
                                .unwrap_or(1.0);
                        let clicks = self.clicks.press(name, self.now(), pos, radius);
                        self.push(Event::MousePressed(name, x, y, clicks));
                    }
                    ElementState::Released => {
                        self.push(Event::MouseReleased(name, x, y));
                    }
                }
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                let (x, y, phase) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x as f64, y as f64, None),
                    // a trackpad glide: pixel deltas arrive phased, and
                    // the lua layer rails each gesture to one axis
                    MouseScrollDelta::PixelDelta(pos) => (
                        pos.x / 40.0,
                        pos.y / 40.0,
                        Some(match phase {
                            TouchPhase::Started => "started",
                            TouchPhase::Moved => "moved",
                            TouchPhase::Ended | TouchPhase::Cancelled => "ended",
                        }),
                    ),
                };
                // zero deltas carry nothing -- except a gesture's end,
                // which the lua layer needs to unlock its rails
                if x != 0.0 || y != 0.0 || phase == Some("ended") {
                    self.push(Event::MouseWheel(x, y, phase));
                }
            }
            WindowEvent::DroppedFile(path) => {
                // winit reports no cursor motion during a drag, so these
                // coordinates are the last known position, not the drop
                // point; the file still opens, it may just land in another
                // split. sdl could do better, winit has no drop position
                let (x, y) = self.cursor.unwrap_or((0.0, 0.0));
                self.push(Event::FileDropped(path, x as i32, y as i32));
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some((_, thread)) = &self.lua else { return };
        let thread = thread.clone();
        // swap, not load: the editor must be told once, and telling it
        // twice would interrupt the shutdown it already started
        if self
            .terminated
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            self.push(Event::Terminate);
        }
        // resume the editor at most a handful of times before returning to
        // winit: consecutive slow frames would otherwise never let the os
        // deliver events (sdl pumped its own queue; winit only pumps when
        // we return)
        for _ in 0..4 {
            let now = self.now();
            let resume_arg = match self.parked {
                Parked::Done(code) => {
                    if code != 0 {
                        // process::exit skips destructors: close the lua
                        // state first so spawned processes are killed and
                        // reaped instead of orphaned
                        self.lua = None;
                        std::process::exit(code);
                    }
                    event_loop.exit();
                    return;
                }
                Parked::Start => Resume::Start,
                Parked::Wait { deadline } => {
                    let has_event = self.with_platform(|p| !p.queue.is_empty());
                    if has_event {
                        Resume::EventAvailable(true)
                    } else if now >= deadline {
                        Resume::EventAvailable(false)
                    } else {
                        event_loop.set_control_flow(ControlFlow::WaitUntil(
                            self.deadline_instant(deadline),
                        ));
                        return;
                    }
                }
                Parked::Sleep { deadline } => {
                    if now >= deadline {
                        Resume::Start
                    } else {
                        event_loop.set_control_flow(ControlFlow::WaitUntil(
                            self.deadline_instant(deadline),
                        ));
                        return;
                    }
                }
            };
            self.parked = match boot::resume(&thread, resume_arg) {
                Yield::Wait(t) => Parked::Wait {
                    deadline: self.now() + t.max(0.0),
                },
                Yield::Sleep(t) => Parked::Sleep {
                    deadline: self.now() + t.max(0.0),
                },
                Yield::Exit(code) => Parked::Done(code),
            };
        }
        event_loop.set_control_flow(ControlFlow::Poll);
    }
}

/// directory that contains data/: next to the executable for checkouts
/// and unpacked releases, the crate root for cargo run, and for a lone
/// installed binary the embedded copy unpacked into the user data dir
fn find_exedir() -> String {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
        && dir.join("data").is_dir()
    {
        return dir.display().to_string();
    }
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    if manifest.join("data").is_dir() {
        return manifest.display().to_string();
    }
    let root = crate::embed::unpack().expect("failed to unpack the editor data");
    root.display().to_string()
}

const USAGE: &str = "\
usage: wisp [file or directory ...]

  -h, --help     show this message and exit
  -v, --version  show the version and exit

with no arguments wisp opens the directory it was launched from.
";

/// sigterm (a logout, `kill`, a service manager), sigint (ctrl+c in the
/// terminal that launched us) and sighup (that terminal going away) all
/// mean the same thing: shut down now. unhandled, they kill the process
/// outright -- unsaved work gone, the terminal's children orphaned.
///
/// the flag they set is read in `about_to_wait`, so it is acted on within
/// one turn of the editor's loop; that loop never waits longer than a
/// quarter of a second, so the delay is not one anybody can feel
fn catch_terminating_signals() -> Arc<std::sync::atomic::AtomicBool> {
    let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(unix)]
    for signal in [
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGHUP,
    ] {
        // failing to register costs only the graceful shutdown, which is
        // not worth refusing to start over
        let _ = signal_hook::flag::register(signal, Arc::clone(&flag));
    }
    flag
}

pub fn run() {
    // asked what it is, a program answers on stdout and exits. opening a
    // window instead is the thing every linux user complains about
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--" => break,
            "-h" | "--help" => return print!("{USAGE}"),
            "-v" | "--version" => return println!("wisp {}", env!("CARGO_PKG_VERSION")),
            _ => {}
        }
    }
    let terminated = catch_terminating_signals();

    let event_loop = EventLoop::new().expect("failed to create event loop");
    let start = Instant::now();
    let engine = Engine::shared(Box::new(DesktopPlatform::new(start)));
    let mut app = App {
        engine,
        exedir: find_exedir(),
        start,
        lua: None,
        parked: Parked::Start,
        mods: ModifiersState::empty(),
        cursor: None,
        clicks: ClickCounter::new(),
        terminated,
    };
    event_loop.run_app(&mut app).expect("event loop error");
}

#[cfg(test)]
mod tests {
    use super::{ClickCounter, catch_terminating_signals, clamp_deadline};

    /// the handler is installed for the whole process, which only makes
    /// sigterm non-fatal here; raising it delivers to this thread
    #[cfg(unix)]
    #[test]
    fn a_terminating_signal_reaches_the_flag() {
        let flag = catch_terminating_signals();
        assert!(!flag.load(std::sync::atomic::Ordering::Relaxed));
        nix::sys::signal::raise(nix::sys::signal::Signal::SIGTERM).unwrap();
        assert!(
            flag.load(std::sync::atomic::Ordering::Relaxed),
            "sigterm never reached the flag: the editor would die on a logout"
        );
    }

    #[test]
    fn rapid_clicks_cycle_through_caret_word_line() {
        let mut c = ClickCounter::new();
        let counts: Vec<i32> = (0..7)
            .map(|i| c.press("left", i as f64 * 0.1, (5.0, 5.0), 8.0))
            .collect();
        assert_eq!(counts, [1, 2, 3, 1, 2, 3, 1]);
    }

    #[test]
    fn slow_clicks_do_not_count_up() {
        let mut c = ClickCounter::new();
        assert_eq!(c.press("left", 0.0, (5.0, 5.0), 8.0), 1);
        assert_eq!(c.press("left", 0.6, (5.0, 5.0), 8.0), 1);
    }

    #[test]
    fn moving_the_mouse_resets_the_count() {
        let mut c = ClickCounter::new();
        assert_eq!(c.press("left", 0.0, (5.0, 5.0), 8.0), 1);
        assert_eq!(c.press("left", 0.1, (20.0, 5.0), 8.0), 1);
        assert_eq!(c.press("left", 0.2, (20.0, 7.0), 8.0), 2);
    }

    #[test]
    fn changing_button_resets_the_count() {
        let mut c = ClickCounter::new();
        assert_eq!(c.press("left", 0.0, (5.0, 5.0), 8.0), 1);
        assert_eq!(c.press("left", 0.1, (5.0, 5.0), 8.0), 2);
        assert_eq!(c.press("right", 0.2, (5.0, 5.0), 8.0), 1);
    }

    #[test]
    fn a_scaled_radius_widens_the_double_click_slop() {
        let mut c = ClickCounter::new();
        assert_eq!(c.press("left", 0.0, (5.0, 5.0), 16.0), 1);
        // 10px apart: outside an 8px radius, inside a 2x-scaled 16px one
        assert_eq!(c.press("left", 0.1, (15.0, 5.0), 16.0), 2);
    }

    #[test]
    fn hostile_wait_deadlines_never_panic_the_timer() {
        // `wait_event(math.huge)` from a plugin reaches this math; the
        // old code fed it straight to Duration::from_secs_f64 and died
        for t in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN, 1e300, -5.0] {
            let clamped = clamp_deadline(100.0, t);
            assert!(clamped.is_finite() && clamped >= 0.0, "t = {t}");
            // the panic the clamp exists to prevent
            let _ = std::time::Duration::from_secs_f64(clamped);
        }
        // ordinary deadlines pass through untouched
        assert_eq!(clamp_deadline(100.0, 100.016), 100.016);
    }
}

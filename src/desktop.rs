//! The desktop platform: winit windowing, softbuffer presentation, arboard
//! clipboard -- and the driver that runs the editor coroutine inside the
//! winit event loop.
//!
//! winit owns the OS event loop, as it is designed to; the editor Lua
//! believes it owns its own loop, as it is designed to. The coroutine
//! protocol in boot.rs lets both be true: when Lua calls wait_event or
//! sleep, the coroutine parks, control returns here, and winit blocks in
//! the OS with ControlFlow::WaitUntil. Idle costs nothing.

use std::any::Any;
use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
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
        if let Some(clipboard) = &mut self.clipboard
            && let Ok(text) = clipboard.get_text()
        {
            return Some(text);
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
        if buffer.len() == fb.pixels.len() {
            buffer.copy_from_slice(&fb.pixels);
        }
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
    Done,
}

struct App {
    engine: Shared,
    exedir: String,
    start: Instant,
    lua: Option<(mlua::Lua, mlua::Thread)>,
    parked: Parked,
    mods: ModifiersState,
    cursor: (f64, f64),
    click_time: f64,
    click_button: &'static str,
    click_pos: (f64, f64),
    clicks: i32,
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
        self.start + Duration::from_secs_f64(t.max(0.0))
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

        let args: Vec<String> = std::env::args().collect();
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
                self.push(Event::Resized(size.width as i32, size.height as i32));
            }
            WindowEvent::RedrawRequested => self.push(Event::Exposed),
            WindowEvent::Focused(focus) => self.with_platform(|p| p.focus = focus),
            WindowEvent::ModifiersChanged(mods) => self.mods = mods.state(),
            WindowEvent::KeyboardInput { event, .. } => {
                let name = keys::key_name(&event.logical_key, event.location);
                match event.state {
                    ElementState::Pressed => {
                        if let Some(name) = name {
                            self.push(Event::KeyPressed(name));
                        }
                        if let Some(text) = &event.text
                            && !self.mods.control_key()
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
                let (dx, dy) = (position.x - self.cursor.0, position.y - self.cursor.1);
                self.cursor = (position.x, position.y);
                self.push(Event::MouseMoved(
                    position.x as i32,
                    position.y as i32,
                    dx as i32,
                    dy as i32,
                ));
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let name = match button {
                    MouseButton::Left => "left",
                    MouseButton::Middle => "middle",
                    MouseButton::Right => "right",
                    _ => "?",
                };
                let (x, y) = (self.cursor.0 as i32, self.cursor.1 as i32);
                match state {
                    ElementState::Pressed => {
                        let now = self.now();
                        let near = (self.cursor.0 - self.click_pos.0).abs() < 8.0
                            && (self.cursor.1 - self.click_pos.1).abs() < 8.0;
                        if name == self.click_button && now - self.click_time < 0.5 && near {
                            self.clicks += 1;
                        } else {
                            self.clicks = 1;
                        }
                        self.click_time = now;
                        self.click_button = name;
                        self.click_pos = self.cursor;
                        self.push(Event::MousePressed(name, x, y, self.clicks));
                    }
                    ElementState::Released => {
                        self.push(Event::MouseReleased(name, x, y));
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64,
                    MouseScrollDelta::PixelDelta(pos) => pos.y / 40.0,
                };
                if y != 0.0 {
                    self.push(Event::MouseWheel(y));
                }
            }
            WindowEvent::DroppedFile(path) => {
                self.push(Event::FileDropped(
                    path.display().to_string(),
                    self.cursor.0 as i32,
                    self.cursor.1 as i32,
                ));
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some((_, thread)) = &self.lua else { return };
        let thread = thread.clone();
        loop {
            let now = self.now();
            let resume_arg = match self.parked {
                Parked::Done => {
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
                Yield::Exit(_) => Parked::Done,
            };
        }
    }
}

/// Directory that contains `data/`: next to the executable for installs,
/// the crate root for `cargo run`.
fn find_exedir() -> String {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
        && dir.join("data").is_dir()
    {
        return dir.display().to_string();
    }
    env!("CARGO_MANIFEST_DIR").to_owned()
}

pub fn run() {
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
        cursor: (0.0, 0.0),
        click_time: -1.0,
        click_button: "?",
        click_pos: (0.0, 0.0),
        clicks: 0,
    };
    event_loop.run_app(&mut app).expect("event loop error");
}

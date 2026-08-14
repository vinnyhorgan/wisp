//! the platform boundary. everything above this trait (renderer, rencache,
//! fonts, the lua api) is pure and platform-agnostic; everything below it
//! is either the winit/softbuffer desktop or the scripted headless
//! implementation used by the tests.
//!
//! note there is no `wait_event` or `sleep` here: those are the two
//! blocking calls in lite's api, and in wisp they are coroutine yields
//! handled by whoever drives the lua thread (the winit loop or the
//! headless driver).

use std::any::Any;

use crate::renderer::{Framebuffer, Rect};

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Quit,
    Resized(i32, i32),
    Exposed,
    FileDropped(std::path::PathBuf, i32, i32),
    KeyPressed(String),
    KeyReleased(String),
    TextInput(String),
    MousePressed(&'static str, i32, i32, i32),
    MouseReleased(&'static str, i32, i32),
    MouseMoved(i32, i32, i32, i32),
    /// horizontal then vertical scroll, in lines
    MouseWheel(f64, f64),
}

pub trait Platform {
    fn poll_event(&mut self) -> Option<Event>;
    /// monotonic time in seconds; headless implementations return a virtual
    /// clock so rendering is deterministic
    fn now(&self) -> f64;
    fn window_size(&self) -> (i32, i32);
    fn window_has_focus(&self) -> bool;
    fn set_window_title(&mut self, title: &str);
    fn set_window_mode(&mut self, mode: &str);
    fn set_cursor(&mut self, cursor: &str);
    fn get_clipboard(&mut self) -> Option<String>;
    fn set_clipboard(&mut self, text: &str);
    fn present(&mut self, fb: &Framebuffer, rects: &[Rect]);
    /// escape hatch for the drivers, which know their concrete platform
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

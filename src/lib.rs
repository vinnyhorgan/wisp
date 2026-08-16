//! wisp -- what lite could really be
//!
//! a tiny editor core in rust, running rxi's lite editor lua on top.
//! the core exposes just enough api (`system.*`, `renderer.*`) for the lua
//! layer to build the entire editor; everything here exists to make that
//! surface small, solid and pleasant.
//!
//! the layering is strict: renderer, rencache and font are pure and know
//! nothing about windows or lua; the platform trait hides winit/softbuffer
//! (desktop) behind the same interface as the scripted headless platform
//! used by the end-to-end tests.

pub mod api;
pub mod boot;
pub mod desktop;
mod embed;
pub mod font;
pub mod headless;
pub mod image;
mod keys;
pub mod platform;
pub mod process;
pub mod rencache;
pub mod renderer;
pub mod terminal;
pub mod watch;

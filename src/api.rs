//! The Lua-facing API: the `system` and `renderer` tables, mirroring
//! lite's C API surface exactly -- with one deliberate omission:
//! `system.show_confirm_dialog` does not exist. wisp never draws OS UI on
//! top of the editor (see DEVIATIONS.md).

use std::cell::RefCell;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mlua::{AnyUserData, IntoLuaMulti, Lua, MultiValue, Table, UserData, UserDataMethods, Value};

use crate::font::Font;
use crate::platform::{Event, Platform};
use crate::rencache::RenCache;
use crate::renderer::{Color, Framebuffer, Rect};

pub struct Engine {
    pub platform: Box<dyn Platform>,
    pub fb: Framebuffer,
    pub cache: RenCache,
}

pub type Shared = Rc<RefCell<Engine>>;

impl Engine {
    pub fn shared(platform: Box<dyn Platform>) -> Shared {
        Rc::new(RefCell::new(Engine {
            platform,
            fb: Framebuffer::new(1, 1),
            cache: RenCache::new(),
        }))
    }
}

struct LuaFont(Rc<Font>);

impl UserData for LuaFont {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("set_tab_width", |_, this, n: f64| {
            this.0.set_tab_advance(n as i32);
            Ok(())
        });
        methods.add_method("get_width", |_, this, text: mlua::LuaString| {
            Ok(this.0.width_of(&String::from_utf8_lossy(&text.as_bytes())))
        });
        methods.add_method("get_height", |_, this, ()| Ok(this.0.height()));
    }
}

fn lua_path(s: &mlua::LuaString) -> PathBuf {
    PathBuf::from(std::ffi::OsStr::from_bytes(&s.as_bytes()))
}

fn check_color(t: Option<Table>) -> mlua::Result<Color> {
    let Some(t) = t else {
        return Ok(Color {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        });
    };
    let r: f64 = t.get(1)?;
    let g: f64 = t.get(2)?;
    let b: f64 = t.get(3)?;
    let a: Option<f64> = t.get(4)?;
    let c = |v: f64| v.clamp(0.0, 255.0) as u8;
    Ok(Color {
        r: c(r),
        g: c(g),
        b: c(b),
        a: c(a.unwrap_or(255.0)),
    })
}

fn event_to_multi(lua: &Lua, event: Event) -> mlua::Result<MultiValue> {
    match event {
        Event::Quit => ("quit",).into_lua_multi(lua),
        Event::Resized(w, h) => ("resized", w, h).into_lua_multi(lua),
        Event::Exposed => ("exposed",).into_lua_multi(lua),
        Event::FileDropped(file, x, y) => ("filedropped", file, x, y).into_lua_multi(lua),
        Event::KeyPressed(key) => ("keypressed", key).into_lua_multi(lua),
        Event::KeyReleased(key) => ("keyreleased", key).into_lua_multi(lua),
        Event::TextInput(text) => ("textinput", text).into_lua_multi(lua),
        Event::MousePressed(b, x, y, clicks) => {
            ("mousepressed", b, x, y, clicks).into_lua_multi(lua)
        }
        Event::MouseReleased(b, x, y) => ("mousereleased", b, x, y).into_lua_multi(lua),
        Event::MouseMoved(x, y, dx, dy) => ("mousemoved", x, y, dx, dy).into_lua_multi(lua),
        Event::MouseWheel(y) => ("mousewheel", y).into_lua_multi(lua),
    }
}

/// The exact scoring function from lite's system.fuzzy_match, ported from
/// C (with the out-of-bounds walk on trailing spaces fixed).
pub fn fuzzy_match(mut s: &[u8], mut p: &[u8]) -> Option<i32> {
    let mut score: i32 = 0;
    let mut run: i32 = 0;
    while !s.is_empty() && !p.is_empty() {
        while s.first() == Some(&b' ') {
            s = &s[1..];
        }
        while p.first() == Some(&b' ') {
            p = &p[1..];
        }
        let (Some(&sc), Some(&pc)) = (s.first(), p.first()) else {
            break;
        };
        if sc.eq_ignore_ascii_case(&pc) {
            score += run * 10 - (sc != pc) as i32;
            run += 1;
            p = &p[1..];
        } else {
            score -= 10;
            run = 0;
        }
        s = &s[1..];
    }
    if !p.is_empty() {
        return None;
    }
    Some(score - s.len() as i32)
}

pub fn register(lua: &Lua, engine: &Shared) -> mlua::Result<()> {
    let system = lua.create_table()?;
    let renderer = lua.create_table()?;

    // --- system ------------------------------------------------------------

    {
        let eng = engine.clone();
        system.set(
            "poll_event",
            lua.create_function(move |lua, _: MultiValue| {
                let event = {
                    let mut e = eng.borrow_mut();
                    let event = e.platform.poll_event();
                    if matches!(event, Some(Event::Exposed)) {
                        e.cache.invalidate();
                    }
                    event
                };
                match event {
                    Some(event) => event_to_multi(lua, event),
                    None => Ok(MultiValue::new()),
                }
            })?,
        )?;
    }
    // system.wait_event and system.sleep are defined in the bootstrap
    // prelude as coroutine yields; see boot.rs
    {
        let eng = engine.clone();
        system.set(
            "get_time",
            lua.create_function(move |_, ()| Ok(eng.borrow().platform.now()))?,
        )?;
    }
    {
        let eng = engine.clone();
        system.set(
            "set_cursor",
            lua.create_function(move |_, cursor: Option<String>| {
                let cursor = cursor.unwrap_or_else(|| "arrow".to_owned());
                eng.borrow_mut().platform.set_cursor(&cursor);
                Ok(())
            })?,
        )?;
    }
    {
        let eng = engine.clone();
        system.set(
            "set_window_title",
            lua.create_function(move |_, title: String| {
                eng.borrow_mut().platform.set_window_title(&title);
                Ok(())
            })?,
        )?;
    }
    {
        let eng = engine.clone();
        system.set(
            "set_window_mode",
            lua.create_function(move |_, mode: Option<String>| {
                let mode = mode.unwrap_or_else(|| "normal".to_owned());
                eng.borrow_mut().platform.set_window_mode(&mode);
                Ok(())
            })?,
        )?;
    }
    {
        let eng = engine.clone();
        system.set(
            "window_has_focus",
            lua.create_function(move |_, ()| Ok(eng.borrow().platform.window_has_focus()))?,
        )?;
    }
    system.set(
        "chdir",
        lua.create_function(|_, path: mlua::LuaString| {
            std::env::set_current_dir(lua_path(&path))
                .map_err(|_| mlua::Error::runtime("chdir() failed"))
        })?,
    )?;
    system.set(
        "list_dir",
        lua.create_function(|lua, path: mlua::LuaString| {
            let dir = match std::fs::read_dir(lua_path(&path)) {
                Ok(dir) => dir,
                Err(err) => {
                    return (Value::Nil, err.to_string()).into_lua_multi(lua);
                }
            };
            let list = lua.create_table()?;
            for entry in dir.flatten() {
                list.push(lua.create_string(entry.file_name().as_bytes())?)?;
            }
            list.into_lua_multi(lua)
        })?,
    )?;
    system.set(
        "absolute_path",
        lua.create_function(|lua, path: mlua::LuaString| {
            match std::fs::canonicalize(lua_path(&path)) {
                Ok(abs) => Ok(Some(lua.create_string(abs.as_os_str().as_bytes())?)),
                Err(_) => Ok(None),
            }
        })?,
    )?;
    system.set(
        "get_file_info",
        lua.create_function(|lua, path: mlua::LuaString| {
            let meta = match std::fs::metadata(lua_path(&path)) {
                Ok(meta) => meta,
                Err(err) => {
                    return (Value::Nil, err.to_string()).into_lua_multi(lua);
                }
            };
            let info = lua.create_table()?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            info.set("modified", modified)?;
            info.set("size", meta.len())?;
            if meta.is_file() {
                info.set("type", "file")?;
            } else if meta.is_dir() {
                info.set("type", "dir")?;
            }
            info.into_lua_multi(lua)
        })?,
    )?;
    {
        let eng = engine.clone();
        system.set(
            "get_clipboard",
            lua.create_function(move |_, ()| Ok(eng.borrow_mut().platform.get_clipboard()))?,
        )?;
    }
    {
        let eng = engine.clone();
        system.set(
            "set_clipboard",
            lua.create_function(move |_, text: String| {
                eng.borrow_mut().platform.set_clipboard(&text);
                Ok(())
            })?,
        )?;
    }
    system.set(
        "exec",
        lua.create_function(|_, cmd: String| {
            // mirror lite: hand the command line to the shell, backgrounded
            let result = std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("{cmd} &"))
                .spawn();
            if let Ok(mut child) = result {
                let _ = child.wait();
            }
            Ok(())
        })?,
    )?;
    system.set(
        "fuzzy_match",
        lua.create_function(|_, (s, p): (mlua::LuaString, mlua::LuaString)| {
            Ok(fuzzy_match(&s.as_bytes(), &p.as_bytes()))
        })?,
    )?;

    // --- renderer ----------------------------------------------------------

    {
        let eng = engine.clone();
        renderer.set(
            "show_debug",
            lua.create_function(move |_, enable: Value| {
                eng.borrow_mut()
                    .cache
                    .show_debug(enable.as_boolean().unwrap_or(false));
                Ok(())
            })?,
        )?;
    }
    {
        let eng = engine.clone();
        renderer.set(
            "get_size",
            lua.create_function(move |_, ()| Ok(eng.borrow().platform.window_size()))?,
        )?;
    }
    {
        let eng = engine.clone();
        renderer.set(
            "begin_frame",
            lua.create_function(move |_, ()| {
                let e = &mut *eng.borrow_mut();
                let (w, h) = e.platform.window_size();
                e.fb.resize(w, h);
                e.cache.begin_frame(e.fb.width, e.fb.height);
                Ok(())
            })?,
        )?;
    }
    {
        let eng = engine.clone();
        renderer.set(
            "end_frame",
            lua.create_function(move |_, ()| {
                let e = &mut *eng.borrow_mut();
                let rects = e.cache.end_frame(&mut e.fb);
                if !rects.is_empty() {
                    e.platform.present(&e.fb, &rects);
                }
                Ok(())
            })?,
        )?;
    }
    {
        let eng = engine.clone();
        renderer.set(
            "set_clip_rect",
            lua.create_function(move |_, (x, y, w, h): (f64, f64, f64, f64)| {
                eng.borrow_mut()
                    .cache
                    .set_clip_rect(Rect::new(x as i32, y as i32, w as i32, h as i32));
                Ok(())
            })?,
        )?;
    }
    {
        let eng = engine.clone();
        renderer.set(
            "draw_rect",
            lua.create_function(
                move |_, (x, y, w, h, color): (f64, f64, f64, f64, Option<Table>)| {
                    eng.borrow_mut().cache.draw_rect(
                        Rect::new(x as i32, y as i32, w as i32, h as i32),
                        check_color(color)?,
                    );
                    Ok(())
                },
            )?,
        )?;
    }
    {
        let eng = engine.clone();
        renderer.set(
            "draw_text",
            lua.create_function(
                move |_,
                      (font, text, x, y, color): (
                    AnyUserData,
                    mlua::LuaString,
                    f64,
                    f64,
                    Option<Table>,
                )| {
                    let font = font.borrow::<LuaFont>()?;
                    let text = String::from_utf8_lossy(&text.as_bytes()).into_owned();
                    let x = eng.borrow_mut().cache.draw_text(
                        &font.0,
                        &text,
                        x as i32,
                        y as i32,
                        check_color(color)?,
                    );
                    Ok(x)
                },
            )?,
        )?;
    }

    let font = lua.create_table()?;
    font.set(
        "load",
        lua.create_function(|_, (filename, size): (mlua::LuaString, f64)| {
            match Font::load(&lua_path(&filename), size as f32) {
                Ok(font) => Ok(LuaFont(Rc::new(font))),
                Err(err) => Err(mlua::Error::runtime(format!(
                    "failed to load font: {}: {err}",
                    Path::new(std::ffi::OsStr::from_bytes(&filename.as_bytes())).display()
                ))),
            }
        })?,
    )?;
    renderer.set("font", font)?;

    let globals = lua.globals();
    let loaded: Table = lua
        .globals()
        .get::<Table>("package")?
        .get::<Table>("loaded")?;
    loaded.set("system", &system)?;
    loaded.set("renderer", &renderer)?;
    globals.set("system", system)?;
    globals.set("renderer", renderer)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::fuzzy_match;

    // expectations computed by hand from lite's C implementation
    #[test]
    fn fuzzy_match_matches_lite_semantics() {
        // no pattern: score = 0 - remaining chars
        assert_eq!(fuzzy_match(b"hello", b""), Some(-5));
        // exact match: runs build up 0+10+20+30+40
        assert_eq!(fuzzy_match(b"hello", b"hello"), Some(100));
        // case-insensitive match costs 1 per differing char
        assert_eq!(fuzzy_match(b"Hello", b"hello"), Some(99));
        // pattern not consumed -> nil
        assert_eq!(fuzzy_match(b"abc", b"abcd"), None);
        assert_eq!(fuzzy_match(b"", b"x"), None);
        // skipped chars cost 10 each: "x" then match "a" (run 0 -> +0), then
        // trailing "yz" costs via strlen: -10 (skip x) + 0 - 2 = -12
        assert_eq!(fuzzy_match(b"xayz", b"a"), Some(-12));
        // spaces are skipped on both sides
        assert_eq!(fuzzy_match(b"  ab", b" a b"), Some(10));
    }

    #[test]
    fn fuzzy_match_trailing_spaces_do_not_crash() {
        // this walked out of bounds in lite
        assert_eq!(fuzzy_match(b"a ", b"b"), None);
        let _ = fuzzy_match(b"   ", b"   ");
        let _ = fuzzy_match(b"a   ", b"ab  ");
    }
}

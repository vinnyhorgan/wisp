//! the lua-facing api: the `system` and `renderer` tables, mirroring
//! lite's c api surface exactly -- with one deliberate omission:
//! `system.show_confirm_dialog` does not exist. wisp never draws os ui on
//! top of the editor (see DEVIATIONS.md).
//!
//! strings crossing the boundary are treated the way lite treated them:
//! paths are raw bytes (lua strings are byte strings), text to render is
//! decoded lossily so invalid utf-8 draws replacement glyphs instead of
//! crashing anything.

use std::cell::RefCell;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::rc::Rc;

use mlua::{
    AnyUserData, IntoLuaMulti, Lua, LuaString, MultiValue, Table, UserData, UserDataMethods, Value,
};

use crate::font::Font;
use crate::image::Image;
use crate::platform::{Event, Platform};
use crate::process;
use crate::rencache::RenCache;
use crate::renderer::{Color, Framebuffer, Rect};
use crate::terminal;
use crate::watch;

/// everything the api closures share: the platform, the framebuffer and
/// the draw-command cache
pub struct Engine {
    pub platform: Box<dyn Platform>,
    pub(crate) fb: Framebuffer,
    pub(crate) cache: RenCache,
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

/// the `Font` userdata lua sees; wraps a shared font so pending draw
/// commands keep it alive after lua drops it
struct LuaFont(Rc<Font>);

impl UserData for LuaFont {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("set_tab_width", |_, this, n: f64| {
            this.0.set_tab_advance(n as i32);
            Ok(())
        });
        methods.add_method("get_width", |_, this, text: LuaString| {
            Ok(this.0.width_of(&String::from_utf8_lossy(&text.as_bytes())))
        });
        methods.add_method("get_height", |_, this, ()| Ok(this.0.height()));
        // not lite: in-place resize for runtime zoom (lite-xl's shape).
        // every holder of the font sees the new metrics immediately
        methods.add_method("set_size", |_, this, size: f64| {
            this.0.set_size(size as f32);
            Ok(())
        });
        methods.add_method("get_size", |_, this, ()| Ok(this.0.size()));
    }
}

/// the `Image` userdata lua sees; wraps a shared image so pending draw
/// commands keep it alive after lua drops it, exactly like fonts
struct LuaImage(Rc<Image>);

impl UserData for LuaImage {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("get_width", |_, this, ()| Ok(this.0.width()));
        methods.add_method("get_height", |_, this, ()| Ok(this.0.height()));
    }
}

/// the process userdata behind `system.spawn`; every method polls, none
/// blocks, and reads follow the `file:read` convention: an empty string
/// means nothing buffered right now, nil means end of stream
struct LuaProcess(process::Process);

impl UserData for LuaProcess {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("read_stdout", |lua, this, max: Option<usize>| {
            this.0
                .read_stdout(max)
                .map(|chunk| lua.create_string(&chunk))
                .transpose()
        });
        methods.add_method("read_stderr", |lua, this, max: Option<usize>| {
            this.0
                .read_stderr(max)
                .map(|chunk| lua.create_string(&chunk))
                .transpose()
        });
        methods.add_method("write", |_, this, data: LuaString| {
            Ok(match this.0.write(&data.as_bytes()) {
                Ok(()) => (Some(true), None),
                Err(e) => (None, Some(e)),
            })
        });
        methods.add_method("close_stdin", |_, this, ()| {
            this.0.close_stdin();
            Ok(())
        });
        methods.add_method("running", |_, this, ()| Ok(this.0.running()));
        methods.add_method("returncode", |_, this, ()| Ok(this.0.returncode()));
        methods.add_method("pid", |_, this, ()| Ok(this.0.pid()));
        methods.add_method("terminate", |_, this, ()| {
            Ok(match this.0.terminate() {
                Ok(()) => (Some(true), None),
                Err(e) => (None, Some(e)),
            })
        });
        methods.add_method("kill", |_, this, ()| {
            Ok(match this.0.kill() {
                Ok(()) => (Some(true), None),
                Err(e) => (None, Some(e)),
            })
        });
    }
}

fn pack_rgb(c: Color) -> u32 {
    ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32
}

/// the terminal userdata behind `system.terminal`: alacritty's vt
/// engine on a polled pty (src/terminal.rs). every method polls, none
/// blocks, and rows and columns are 1-based on this side of the fence.
/// colors cross as packed 0xrrggbb integers so a frame's worth of runs
/// is cheap; the view caches the color tables it builds from them
struct LuaTerminal(RefCell<terminal::Terminal>);

impl UserData for LuaTerminal {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("update", |_, this, ()| Ok(this.0.borrow_mut().update()));
        methods.add_method("send", |_, this, data: LuaString| {
            Ok(this.0.borrow_mut().write(&data.as_bytes()))
        });
        methods.add_method(
            "resize",
            |_, this, (cols, rows, cw, ch): (f64, f64, Option<f64>, Option<f64>)| {
                this.0.borrow_mut().resize(
                    cols.clamp(2.0, 65535.0) as u16,
                    rows.clamp(2.0, 65535.0) as u16,
                    (
                        cw.unwrap_or(0.0).clamp(0.0, 65535.0) as u16,
                        ch.unwrap_or(0.0).clamp(0.0, 65535.0) as u16,
                    ),
                );
                Ok(())
            },
        );
        methods.add_method("size", |_, this, ()| {
            let t = this.0.borrow();
            Ok((t.columns(), t.screen_lines()))
        });
        // a flat sequence of run quints: text, fg, bg, flags, width.
        // any number is a valid row -- line() runs on the draw path,
        // where an error is fatal, and the core promises an empty line
        // for anything off-screen
        methods.add_method("line", |lua, this, row: f64| {
            let t = this.0.borrow();
            let out = lua.create_table()?;
            let mut i = 1;
            for run in t.line(row.max(1.0) as usize - 1) {
                out.set(i, run.text)?;
                out.set(i + 1, pack_rgb(run.fg))?;
                out.set(i + 2, pack_rgb(run.bg))?;
                out.set(i + 3, run.flags)?;
                out.set(i + 4, run.width)?;
                i += 5;
            }
            Ok(out)
        });
        // nil means everything changed; otherwise the changed rows
        methods.add_method("damage", |lua, this, ()| {
            match this.0.borrow_mut().take_damage() {
                terminal::Damage::Full => Ok(Value::Nil),
                terminal::Damage::Rows(rows) => {
                    let out = lua.create_table()?;
                    for (i, row) in rows.iter().enumerate() {
                        out.set(i + 1, row + 1)?;
                    }
                    Ok(Value::Table(out))
                }
            }
        });
        methods.add_method("cursor", |_, this, ()| {
            let (col, line, visible) = this.0.borrow().cursor();
            Ok((col + 1, line + 1, visible))
        });
        methods.add_method("scroll", |_, this, delta: f64| {
            this.0.borrow_mut().scroll_display(delta as i32);
            Ok(())
        });
        methods.add_method("scroll_reset", |_, this, ()| {
            this.0.borrow_mut().reset_scroll();
            Ok(())
        });
        methods.add_method("scroll_offset", |_, this, ()| {
            Ok(this.0.borrow().display_offset())
        });
        methods.add_method("history", |_, this, ()| Ok(this.0.borrow().history_lines()));
        methods.add_method(
            "set_palette",
            |_, this, (base, fg, bg): (Table, Table, Table)| {
                let mut colors = [Color::default(); 16];
                for (i, slot) in colors.iter_mut().enumerate() {
                    *slot = check_color(Some(base.get::<Table>(i + 1)?))?;
                }
                this.0.borrow_mut().set_palette(
                    &colors,
                    check_color(Some(fg))?,
                    check_color(Some(bg))?,
                );
                Ok(())
            },
        );
        // each event is a little {kind, data} pair, drained by asking
        methods.add_method("events", |lua, this, ()| {
            let events = this.0.borrow_mut().take_events();
            let out = lua.create_table()?;
            for (i, event) in events.into_iter().enumerate() {
                let pair = lua.create_table()?;
                match event {
                    terminal::TermEvent::Title(title) => {
                        pair.set(1, "title")?;
                        pair.set(2, title)?;
                    }
                    terminal::TermEvent::ResetTitle => pair.set(1, "reset_title")?,
                    terminal::TermEvent::Bell => pair.set(1, "bell")?,
                    terminal::TermEvent::Clipboard(data) => {
                        pair.set(1, "clipboard")?;
                        pair.set(2, data)?;
                    }
                }
                out.set(i + 1, pair)?;
            }
            Ok(out)
        });
        methods.add_method("app_cursor", |_, this, ()| Ok(this.0.borrow().app_cursor()));
        methods.add_method("bracketed_paste", |_, this, ()| {
            Ok(this.0.borrow().bracketed_paste())
        });
        methods.add_method("mouse_mode", |_, this, ()| Ok(this.0.borrow().mouse_mode()));
        // nil, "click", "drag" or "motion"
        methods.add_method("mouse_protocol", |_, this, ()| {
            Ok(this.0.borrow().mouse_protocol())
        });
        // "sgr", "utf8" or "normal"
        methods.add_method("mouse_encoding", |_, this, ()| {
            Ok(this.0.borrow().mouse_encoding())
        });
        methods.add_method("alt_screen", |_, this, ()| Ok(this.0.borrow().alt_screen()));
        methods.add_method("alternate_scroll", |_, this, ()| {
            Ok(this.0.borrow().alternate_scroll())
        });
        // whether the 1-based row flows into the next without a newline;
        // lenient about the row for the same reason line() is
        methods.add_method("wrapped", |_, this, row: f64| {
            Ok(this.0.borrow().line_wrapped(row.max(1.0) as usize - 1))
        });
        methods.add_method("running", |_, this, ()| Ok(this.0.borrow().running()));
        methods.add_method("returncode", |_, this, ()| Ok(this.0.borrow().exit_code()));
        methods.add_method("pid", |_, this, ()| Ok(this.0.borrow().pid()));
    }
}

/// the watcher userdata behind `system.watch`: native fs events,
/// drained by polling like every other background source. each change
/// is a {kind, path} pair; "rescan" means events were lost and the
/// consumer should walk the tree itself
struct LuaWatch(RefCell<watch::Watch>);

impl UserData for LuaWatch {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("poll", |lua, this, ()| {
            let out = lua.create_table()?;
            for (i, change) in this.0.borrow_mut().poll().into_iter().enumerate() {
                let pair = lua.create_table()?;
                pair.set(1, change.kind)?;
                pair.set(2, lua.create_string(change.path.as_os_str().as_bytes())?)?;
                out.set(i + 1, pair)?;
            }
            Ok(out)
        });
    }
}

/// lua strings are byte strings; on unix so are paths
fn lua_path(s: &LuaString) -> PathBuf {
    PathBuf::from(OsStr::from_bytes(&s.as_bytes()))
}

/// a color argument: a {r, g, b, a} table, alpha defaulting to 255, or
/// nothing at all (lite defaults that to white)
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

/// events become the multi-value tuples lite's core.step destructures
fn event_to_multi(lua: &Lua, event: Event) -> mlua::Result<MultiValue> {
    match event {
        Event::Quit => ("quit",).into_lua_multi(lua),
        Event::Resized(w, h) => ("resized", w, h).into_lua_multi(lua),
        Event::Exposed => ("exposed",).into_lua_multi(lua),
        Event::FileDropped(file, x, y) => {
            // the path goes to lua as raw bytes so non-utf8 names survive
            let file = lua.create_string(file.as_os_str().as_bytes())?;
            ("filedropped", file, x, y).into_lua_multi(lua)
        }
        Event::KeyPressed(key) => ("keypressed", key).into_lua_multi(lua),
        Event::KeyReleased(key) => ("keyreleased", key).into_lua_multi(lua),
        Event::TextInput(text) => ("textinput", text).into_lua_multi(lua),
        Event::MousePressed(b, x, y, clicks) => {
            ("mousepressed", b, x, y, clicks).into_lua_multi(lua)
        }
        Event::MouseReleased(b, x, y) => ("mousereleased", b, x, y).into_lua_multi(lua),
        Event::MouseMoved(x, y, dx, dy) => ("mousemoved", x, y, dx, dy).into_lua_multi(lua),
        // y comes first: lite's api only had a vertical wheel, so the
        // horizontal axis rides along as an extra value that stock lua
        // ignores until the editor learns to side-scroll
        Event::MouseWheel(x, y, phase) => ("mousewheel", y, x, phase).into_lua_multi(lua),
    }
}

/// the scoring function from lite's system.fuzzy_match, ported from c.
/// near-exact: lite walked out of bounds when the pattern ended in
/// spaces, and fixing that also shifts the *scores* (never the matches)
/// for patterns whose spaces empty the remainder mid-loop -- lite kept
/// comparing against the terminating nul there. i64 keeps the arithmetic
/// defined even for absurd inputs (lite's int could overflow, ub in c)
fn fuzzy_match(mut s: &[u8], mut p: &[u8]) -> Option<i64> {
    let mut score: i64 = 0;
    let mut run: i64 = 0;
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
            score += run * 10 - (sc != pc) as i64;
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
    Some(score - s.len() as i64)
}

pub fn register(lua: &Lua, engine: &Shared) -> mlua::Result<()> {
    let system = lua.create_table()?;
    let renderer = lua.create_table()?;

    // --- system -------------------------------------------------------

    // system.wait_event and system.sleep are deliberately absent: the
    // bootstrap prelude defines them as coroutine yields (see boot.rs)

    let eng = engine.clone();
    system.set(
        "poll_event",
        lua.create_function(move |lua, _: MultiValue| {
            let event = {
                let mut e = eng.borrow_mut();
                let event = e.platform.poll_event();
                // an exposed window has lost its contents; forget history
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

    let eng = engine.clone();
    system.set(
        "get_time",
        lua.create_function(move |_, ()| Ok(eng.borrow().platform.now()))?,
    )?;

    let eng = engine.clone();
    system.set(
        "set_cursor",
        lua.create_function(move |_, cursor: Option<String>| {
            let cursor = cursor.unwrap_or_else(|| "arrow".to_owned());
            // lite validated with luaL_checkoption: a typo'd name is a
            // loud error, not a silent arrow
            if !["arrow", "ibeam", "sizeh", "sizev", "hand"].contains(&cursor.as_str()) {
                return Err(mlua::Error::runtime(format!("invalid cursor {cursor:?}")));
            }
            eng.borrow_mut().platform.set_cursor(&cursor);
            Ok(())
        })?,
    )?;

    let eng = engine.clone();
    system.set(
        "set_window_title",
        lua.create_function(move |_, title: LuaString| {
            // titles are built from file names, which are raw bytes; a
            // latin-1 name must not error the whole editor out of core.step
            let title = String::from_utf8_lossy(&title.as_bytes()).into_owned();
            eng.borrow_mut().platform.set_window_title(&title);
            Ok(())
        })?,
    )?;

    let eng = engine.clone();
    system.set(
        "set_window_mode",
        lua.create_function(move |_, mode: Option<String>| {
            let mode = mode.unwrap_or_else(|| "normal".to_owned());
            // same contract as set_cursor: lite raised on unknown modes
            if !["normal", "maximized", "fullscreen"].contains(&mode.as_str()) {
                return Err(mlua::Error::runtime(format!("invalid window mode {mode:?}")));
            }
            eng.borrow_mut().platform.set_window_mode(&mode);
            Ok(())
        })?,
    )?;

    let eng = engine.clone();
    system.set(
        "window_has_focus",
        lua.create_function(move |_, ()| Ok(eng.borrow().platform.window_has_focus()))?,
    )?;

    system.set(
        "chdir",
        lua.create_function(|_, path: LuaString| {
            std::env::set_current_dir(lua_path(&path))
                .map_err(|_| mlua::Error::runtime("chdir() failed"))
        })?,
    )?;

    system.set(
        "list_dir",
        lua.create_function(|lua, path: LuaString| {
            let dir = match std::fs::read_dir(lua_path(&path)) {
                Ok(dir) => dir,
                Err(err) => return (Value::Nil, err.to_string()).into_lua_multi(lua),
            };
            let list = lua.create_table()?;
            for entry in dir.flatten() {
                list.push(lua.create_string(entry.file_name().as_bytes())?)?;
            }
            list.into_lua_multi(lua)
        })?,
    )?;

    // not lite: lua's os library is iso c only, which has no mkdir.
    // single level, like lite-xl's -- directory trees are a lua loop
    system.set(
        "mkdir",
        lua.create_function(
            |lua, path: LuaString| match std::fs::create_dir(lua_path(&path)) {
                Ok(()) => true.into_lua_multi(lua),
                Err(err) => (Value::Nil, err.to_string()).into_lua_multi(lua),
            },
        )?,
    )?;

    system.set(
        "absolute_path",
        lua.create_function(
            |lua, path: LuaString| match std::fs::canonicalize(lua_path(&path)) {
                Ok(abs) => Ok(Some(lua.create_string(abs.as_os_str().as_bytes())?)),
                Err(_) => Ok(None),
            },
        )?,
    )?;

    system.set(
        "get_file_info",
        lua.create_function(|lua, path: LuaString| {
            let meta = match std::fs::metadata(lua_path(&path)) {
                Ok(meta) => meta,
                Err(err) => return (Value::Nil, err.to_string()).into_lua_multi(lua),
            };
            let modified = meta.modified().ok().map_or(0, |t| {
                // whole seconds, like lite's st_mtime -- including the
                // sign for the museum piece dated before 1970
                match t.duration_since(std::time::UNIX_EPOCH) {
                    Ok(d) => d.as_secs() as i64,
                    Err(e) => -(e.duration().as_secs() as i64),
                }
            });
            let info = lua.create_table()?;
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

    let eng = engine.clone();
    system.set(
        "get_clipboard",
        lua.create_function(move |_, ()| Ok(eng.borrow_mut().platform.get_clipboard()))?,
    )?;

    let eng = engine.clone();
    system.set(
        "set_clipboard",
        lua.create_function(move |_, text: LuaString| {
            // docs hold raw file bytes; copying non-utf8 text must not fail
            let text = String::from_utf8_lossy(&text.as_bytes()).into_owned();
            eng.borrow_mut().platform.set_clipboard(&text);
            Ok(())
        })?,
    )?;

    system.set(
        "exec",
        lua.create_function(|_, cmd: LuaString| {
            // mirror lite: hand the command line to the shell, backgrounded;
            // the outer sh exits immediately so nothing is left to reap.
            // raw bytes end to end, like lite's system() call
            let mut line = cmd.as_bytes().to_vec();
            line.extend_from_slice(b" &");
            if let Ok(mut child) = std::process::Command::new("sh")
                .arg("-c")
                .arg(OsStr::from_bytes(&line))
                .spawn()
            {
                let _ = child.wait();
            }
            Ok(())
        })?,
    )?;

    system.set(
        "spawn",
        lua.create_function(|_, (argv, opts): (Vec<LuaString>, Option<Table>)| {
            // argv, cwd and env are byte strings end to end, like every
            // path in this file: a non-utf8 project dir must spawn fine
            let argv: Vec<std::ffi::OsString> = argv
                .iter()
                .map(|s| OsStr::from_bytes(&s.as_bytes()).to_owned())
                .collect();
            let mut options = process::SpawnOptions::default();
            if let Some(t) = opts {
                options.cwd = t.get::<Option<LuaString>>("cwd")?.map(|s| lua_path(&s));
                if let Some(env) = t.get::<Option<Table>>("env")? {
                    for pair in env.pairs::<LuaString, LuaString>() {
                        let (k, v) = pair?;
                        options.env.push((
                            OsStr::from_bytes(&k.as_bytes()).to_owned(),
                            OsStr::from_bytes(&v.as_bytes()).to_owned(),
                        ));
                    }
                }
                if let Some(stderr) = t.get::<Option<String>>("stderr")? {
                    if stderr != "stdout" {
                        return Ok((None, Some(format!("bad stderr option {stderr:?}"))));
                    }
                    options.merge_stderr = true;
                }
            }
            match process::spawn(&argv, options) {
                Ok(p) => Ok((Some(LuaProcess(p)), None)),
                Err(e) => Ok((None, Some(e))),
            }
        })?,
    )?;

    system.set(
        "terminal",
        lua.create_function(|_, (cols, rows, opts): (f64, f64, Option<Table>)| {
            let mut options = terminal::TerminalOptions {
                argv: Vec::new(),
                cwd: None,
                env: Vec::new(),
            };
            let mut cell = (0u16, 0u16);
            if let Some(t) = opts {
                // argv and env stay utf8 (alacritty's spawn api takes
                // String); the cwd is raw bytes like every other path
                if let Some(argv) = t.get::<Option<Vec<String>>>("argv")? {
                    options.argv = argv;
                }
                options.cwd = t.get::<Option<LuaString>>("cwd")?.map(|s| lua_path(&s));
                if let Some(env) = t.get::<Option<Table>>("env")? {
                    for pair in env.pairs::<String, String>() {
                        let (k, v) = pair?;
                        options.env.push((k, v));
                    }
                }
                cell.0 = t
                    .get::<Option<f64>>("cell_width")?
                    .unwrap_or(0.0)
                    .clamp(0.0, 65535.0) as u16;
                cell.1 = t
                    .get::<Option<f64>>("cell_height")?
                    .unwrap_or(0.0)
                    .clamp(0.0, 65535.0) as u16;
            }
            let (cols, rows) = (cols.clamp(2.0, 65535.0), rows.clamp(2.0, 65535.0));
            match terminal::Terminal::open(cols as u16, rows as u16, cell, options) {
                Ok(t) => Ok((Some(LuaTerminal(RefCell::new(t))), None)),
                Err(e) => Ok((None, Some(e))),
            }
        })?,
    )?;

    system.set(
        "watch",
        lua.create_function(
            |_, path: LuaString| match watch::Watch::open(&lua_path(&path)) {
                Ok(w) => Ok((Some(LuaWatch(RefCell::new(w))), None)),
                Err(err) => Ok((None, Some(err))),
            },
        )?,
    )?;

    system.set(
        "fuzzy_match",
        lua.create_function(|_, (s, p): (LuaString, LuaString)| {
            Ok(fuzzy_match(&s.as_bytes(), &p.as_bytes()))
        })?,
    )?;

    // --- renderer -----------------------------------------------------

    let eng = engine.clone();
    renderer.set(
        "show_debug",
        lua.create_function(move |_, enable: Value| {
            // lua truthiness, like lite's lua_toboolean: everything except
            // nil and false enables the overlay
            let enable = !matches!(enable, Value::Nil | Value::Boolean(false));
            eng.borrow_mut().cache.show_debug(enable);
            Ok(())
        })?,
    )?;

    let eng = engine.clone();
    renderer.set(
        "get_size",
        lua.create_function(move |_, ()| Ok(eng.borrow().platform.window_size()))?,
    )?;

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

    let eng = engine.clone();
    renderer.set(
        "draw_text",
        lua.create_function(
            move |_,
                  (font, text, x, y, color): (
                AnyUserData,
                LuaString,
                f64,
                f64,
                Option<Table>,
            )| {
                let font = font.borrow::<LuaFont>()?;
                // borrow as utf-8, replacing invalid bytes; the rencache
                // makes the single owned copy it needs for its command
                let bytes = text.as_bytes();
                let text = String::from_utf8_lossy(&bytes);
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

    let eng = engine.clone();
    renderer.set(
        "draw_image",
        lua.create_function(
            // draw_image(image, x, y)              natural size
            // draw_image(image, x, y, color)       natural size, tinted
            // draw_image(image, x, y, w, h)        scaled
            // draw_image(image, x, y, w, h, color) scaled, tinted
            move |_,
                  (img, x, y, fourth, h, color): (
                AnyUserData,
                f64,
                f64,
                Option<Value>,
                Option<f64>,
                Option<Table>,
            )| {
                let img = img.borrow::<LuaImage>()?;
                let (w, h, color) = match fourth {
                    None | Some(Value::Nil) => (img.0.width(), img.0.height(), None),
                    Some(Value::Table(t)) => (img.0.width(), img.0.height(), Some(t)),
                    // as_f64 alone would miss this: mlua hands integral
                    // lua numbers over as Value::Integer
                    Some(v) => match (
                        match v {
                            Value::Integer(n) => Some(n as f64),
                            Value::Number(n) => Some(n),
                            _ => None,
                        },
                        h,
                    ) {
                        (Some(w), Some(h)) => (w as i32, h as i32, color),
                        (Some(_), None) => {
                            return Err(mlua::Error::runtime(
                                "draw_image: width given without height",
                            ));
                        }
                        (None, _) => {
                            return Err(mlua::Error::runtime(
                                "draw_image: expected a width or a color as the fourth argument",
                            ));
                        }
                    },
                };
                eng.borrow_mut().cache.draw_image(
                    &img.0,
                    Rect::new(x as i32, y as i32, w, h),
                    check_color(color)?,
                );
                Ok(())
            },
        )?,
    )?;

    let font = lua.create_table()?;
    font.set(
        "load",
        lua.create_function(|_, (filename, size): (LuaString, f64)| {
            match Font::load(&lua_path(&filename), size as f32) {
                Ok(font) => Ok(LuaFont(Rc::new(font))),
                Err(err) => Err(mlua::Error::runtime(format!(
                    "failed to load font: {}: {err}",
                    lua_path(&filename).display()
                ))),
            }
        })?,
    )?;
    renderer.set("font", font)?;

    let image = lua.create_table()?;
    image.set(
        "load",
        lua.create_function(|_, filename: LuaString| {
            match Image::load(&lua_path(&filename)) {
                Ok(image) => Ok(LuaImage(Rc::new(image))),
                // raises like font.load: the renderer.*.load pair share
                // one error contract
                Err(err) => Err(mlua::Error::runtime(format!(
                    "failed to load image: {}: {err}",
                    lua_path(&filename).display()
                ))),
            }
        })?,
    )?;
    renderer.set("image", image)?;

    // register like lite's luaL_requiref: as globals and in package.loaded
    let loaded: Table = lua
        .globals()
        .get::<Table>("package")?
        .get::<Table>("loaded")?;
    loaded.set("system", &system)?;
    loaded.set("renderer", &renderer)?;
    lua.globals().set("system", system)?;
    lua.globals().set("renderer", renderer)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::fuzzy_match;
    use proptest::prelude::*;

    // expectations computed by hand from lite's c implementation
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
        // skipped chars cost 10 each: skip x (-10), match a (run 0, +0),
        // then trailing yz costs -2 via the final strlen
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

    proptest! {
        // arbitrary byte strings must never panic, and an empty pattern
        // must always match with score -len(remaining)
        #[test]
        fn fuzzy_match_total_and_sane(s in any::<Vec<u8>>(), p in any::<Vec<u8>>()) {
            let _ = fuzzy_match(&s, &p);
            prop_assert_eq!(fuzzy_match(&s, b""), Some(-(s.len() as i64)));
        }
    }
}

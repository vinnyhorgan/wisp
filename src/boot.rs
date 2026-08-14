//! Lua state creation and the coroutine protocol.
//!
//! The entire editor runs inside one Lua coroutine. lite's two blocking
//! calls -- `system.wait_event(timeout)` and `system.sleep(secs)` -- are
//! redefined in the bootstrap prelude as `coroutine.yield`, so whoever
//! drives the thread (the winit loop or the headless test driver) decides
//! how time passes and when events are available. The Lua layer runs
//! byte-identical to lite and never knows it is being suspended.

use mlua::thread::ThreadStatus;
use mlua::{Function, Lua, LuaOptions, MultiValue, StdLib, Thread, Value};

use crate::api::{self, Shared};

const BOOTSTRAP: &str = r#"
local headless = ...

PATHSEP = package.config:sub(1, 1)
package.path = EXEDIR .. '/data/?.lua;' .. package.path
package.path = EXEDIR .. '/data/?/init.lua;' .. package.path

function system.wait_event(timeout)
  return coroutine.yield("wait", timeout)
end

function system.sleep(secs)
  coroutine.yield("sleep", secs)
end

if headless then
  os.exit = function(code)
    while true do coroutine.yield("exit", code or 0) end
  end
end

return function()
  local core
  xpcall(function()
    SCALE = tonumber(os.getenv("LITE_SCALE")) or SCALE
    core = require('core')
    core.init()
    core.run()
  end, function(err)
    print('Error: ' .. tostring(err))
    print(debug.traceback(nil, 2))
    if core and core.on_error then
      pcall(core.on_error, err)
    end
    os.exit(1)
  end)
end
"#;

/// What the editor coroutine asked for when it yielded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Yield {
    /// `system.wait_event(timeout)`: wake me when an event arrives or the
    /// timeout elapses, and resume me with whether an event is available.
    Wait(f64),
    /// `system.sleep(secs)`: wake me after this long.
    Sleep(f64),
    /// The editor is done (os.exit in headless mode, coroutine finished,
    /// or a Lua error escaped).
    Exit(i32),
}

/// What to resume the editor coroutine with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Resume {
    /// First resume, or waking from sleep: no values.
    Start,
    /// Waking from `wait_event`: whether an event is available.
    EventAvailable(bool),
}

/// Creates the Lua state, registers wisp's API, sets lite's globals and
/// returns the editor coroutine, ready for its first resume.
pub fn init_lua(
    engine: &Shared,
    exedir: &str,
    exefile: &str,
    args: &[String],
    scale: f64,
    headless: bool,
) -> mlua::Result<(Lua, Thread)> {
    // lite opens all standard libraries, including debug (the bootstrap
    // error handler and core.on_error use debug.traceback)
    let lua = unsafe { Lua::unsafe_new_with(StdLib::ALL, LuaOptions::default()) };
    api::register(&lua, engine)?;

    let globals = lua.globals();
    globals.set("ARGS", lua.create_sequence_from(args.iter().cloned())?)?;
    globals.set("VERSION", "1.11")?;
    globals.set(
        "PLATFORM",
        if cfg!(windows) {
            "Windows"
        } else if cfg!(target_os = "macos") {
            "Mac OS X"
        } else {
            "Linux"
        },
    )?;
    globals.set("SCALE", scale)?;
    globals.set("EXEFILE", exefile)?;
    globals.set("EXEDIR", exedir)?;

    let main: Function = lua.load(BOOTSTRAP).set_name("=bootstrap").call(headless)?;
    let thread = lua.create_thread(main)?;
    Ok((lua, thread))
}

/// Resumes the editor coroutine once and interprets what it yielded.
pub fn resume(thread: &Thread, arg: Resume) -> Yield {
    let result = match arg {
        Resume::Start => thread.resume::<MultiValue>(()),
        Resume::EventAvailable(available) => thread.resume::<MultiValue>(available),
    };
    let values = match result {
        Ok(values) => values,
        Err(err) => {
            eprintln!("lua error: {err}");
            return Yield::Exit(1);
        }
    };
    if thread.status() != ThreadStatus::Resumable {
        return Yield::Exit(0);
    }
    let mut iter = values.into_iter();
    let kind = match iter.next() {
        Some(Value::String(s)) => s.to_string_lossy().to_string(),
        other => {
            eprintln!("unexpected yield from editor coroutine: {other:?}");
            return Yield::Exit(1);
        }
    };
    let num = match iter.next() {
        Some(Value::Number(n)) => n,
        Some(Value::Integer(i)) => i as f64,
        _ => 0.0,
    };
    match kind.as_str() {
        "wait" => Yield::Wait(num),
        "sleep" => Yield::Sleep(num),
        "exit" => Yield::Exit(num as i32),
        other => {
            eprintln!("unexpected yield kind from editor coroutine: {other:?}");
            Yield::Exit(1)
        }
    }
}

//! lua state creation and the coroutine protocol.
//!
//! the entire editor runs inside one lua coroutine. lite's two blocking
//! calls -- `system.wait_event(timeout)` and `system.sleep(secs)` -- are
//! redefined in the bootstrap prelude as `coroutine.yield`, so whoever
//! drives the thread (the winit loop or the headless test driver) decides
//! how time passes and when events are available. the lua layer runs
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

-- exit by yielding, never by exit(2): the host must get control back
-- so rust destructors run (spawned processes are killed and reaped).
-- lua 5.2 semantics for the argument: true and nil succeed, false fails
os.exit = function(code)
  if code == false then code = 1 elseif code == true or code == nil then code = 0 end
  while true do coroutine.yield("exit", code) end
end

return function()
  local core
  xpcall(function()
    -- the env override is desktop-only: headless boots must render the
    -- same pixels on every machine, whatever the environment exports
    if not headless then
      SCALE = tonumber(os.getenv("WISP_SCALE")) or SCALE
    end
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

/// what the editor coroutine asked for when it yielded
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Yield {
    /// `system.wait_event(timeout)`: wake me when an event arrives or the
    /// timeout elapses, and resume me with whether an event is available
    Wait(f64),
    /// `system.sleep(secs)`: wake me after this long
    Sleep(f64),
    /// the editor is done: os.exit in headless mode, coroutine finished,
    /// or a lua error escaped
    Exit(i32),
}

/// what to resume the editor coroutine with
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Resume {
    /// first resume, or waking from sleep: no values
    Start,
    /// waking from wait_event: whether an event is available
    EventAvailable(bool),
}

/// creates the lua state, registers wisp's api, sets lite's globals and
/// returns the editor coroutine, ready for its first resume.
///
/// this is the one place in wisp that needs `unsafe`: lite opens all lua
/// standard libraries including debug (the bootstrap error handler uses
/// debug.traceback), and mlua rightly gates the debug library behind an
/// unsafe constructor
#[allow(unsafe_code)]
pub fn init_lua(
    engine: &Shared,
    exedir: &str,
    exefile: &str,
    args: &[std::ffi::OsString],
    scale: f64,
    headless: bool,
) -> mlua::Result<(Lua, Thread)> {
    let lua = unsafe { Lua::unsafe_new_with(StdLib::ALL, LuaOptions::default()) };
    api::register(&lua, engine)?;

    let globals = lua.globals();
    // args are raw bytes: lua strings hold them as-is, so files with
    // non-utf8 names open fine (lite pushed raw argv the same way)
    use std::os::unix::ffi::OsStrExt;
    let args_table = lua.create_table()?;
    for arg in args {
        args_table.push(lua.create_string(arg.as_bytes())?)?;
    }
    globals.set("ARGS", args_table)?;
    globals.set("VERSION", "1.11")?;
    // the crate is unix-only (byte paths, signals, the pty), so the two
    // platforms a build can see are mac and everything-else-unix
    globals.set(
        "PLATFORM",
        if cfg!(target_os = "macos") {
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

/// resumes the editor coroutine once and interprets what it yielded
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
        Some(Value::String(s)) => s.to_string_lossy(),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// loads a chunk that returns a function, runs it as a coroutine and
    /// reports the first thing the protocol sees
    fn first_yield(chunk: &str) -> Yield {
        let lua = Lua::new();
        let f: Function = lua.load(chunk).eval().unwrap();
        let thread = lua.create_thread(f).unwrap();
        resume(&thread, Resume::Start)
    }

    #[test]
    fn wait_sleep_and_exit_yields_are_parsed() {
        assert!(matches!(
            first_yield("return function() coroutine.yield('wait', 0.25) end"),
            Yield::Wait(t) if t == 0.25
        ));
        // lua integers must coerce like numbers
        assert!(matches!(
            first_yield("return function() coroutine.yield('sleep', 2) end"),
            Yield::Sleep(t) if t == 2.0
        ));
        assert!(matches!(
            first_yield("return function() coroutine.yield('exit', 3) end"),
            Yield::Exit(3)
        ));
    }

    #[test]
    fn a_finished_coroutine_exits_cleanly() {
        assert!(matches!(
            first_yield("return function() end"),
            Yield::Exit(0)
        ));
    }

    #[test]
    fn errors_and_garbage_yields_exit_with_failure() {
        assert!(matches!(
            first_yield("return function() error('boom') end"),
            Yield::Exit(1)
        ));
        assert!(matches!(
            first_yield("return function() coroutine.yield('bogus') end"),
            Yield::Exit(1)
        ));
        assert!(matches!(
            first_yield("return function() coroutine.yield(42) end"),
            Yield::Exit(1)
        ));
    }
}

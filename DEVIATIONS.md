# Deviations from lite's `data/`

`data/` is a byte-per-byte copy of rxi's lite `data/` directory at master
(`38bd9b3`, v1.11 plus rxi's last fixes), except for
the intentional changes listed here. Every entry must say what changed and why.
The reference copy lives untouched in `/lite/` (git-ignored, read-only).

## 1. Quit confirmation uses the CommandView, not an OS dialog

**File:** `data/core/init.lua`, `core.quit()`

lite called `system.show_confirm_dialog()` — the only use of that API in the
entire codebase — to ask about unsaved changes on quit, putting an OS message
box on top of the editor. wisp's core does not expose an OS dialog API at all.
The quit confirmation is routed through `core.command_view:enter()` with
yes/no suggestions instead, so the editor confirms with its own UI, in its own
theme. Type `y`/`yes` to quit, anything else (or escape) to cancel.

## 2. Branding says wisp, not lite

**Files:** `data/core/init.lua`, `data/core/rootview.lua`,
`data/core/commands/core.lua`

The editor is called wisp everywhere the user can see the name: the window
title (`"file - wisp"`), the wordmark on the empty view, the project module
(`.wisp_project.lua`), and the temp file prefix (`.wisp_temp_*`). The scale
override env var is `WISP_SCALE` (was `LITE_SCALE`).

## Core behavior notes (rust core vs lite's c core)

The core fixes lite's bugs instead of reproducing them. The observable
differences, all deliberate:

- Invalid UTF-8 renders as the replacement character; lite's decoder walked
  out of bounds on malformed input.
- `SCALE` defaults to the real display scale factor; lite detected DPI only
  on Windows and hardcoded 1.0 elsewhere. `WISP_SCALE` still overrides it
  (desktop only; headless boots ignore it so tests render identically on
  every machine).
- The mousewheel event carries the horizontal axis as an extra value after
  the vertical one; stock Lua ignores it.
- Rapid clicks cycle caret, word, line, caret, ...; SDL counted up forever.
  Observable only from the fifth rapid click onward.
- Numpad navigation keys with num lock off report their meaning ("home",
  "end", ...); SDL reported "keypad 7" and friends, which no keymap binds.
- Keys are named by their unshifted character (SDL behavior), and synthetic
  key presses on focus gain are dropped (the alt-tab bug lite patched
  around SDL).
- File-drop coordinates are the last known cursor position; winit does not
  report the pointer during a drag, so a drop may land in another split.
  The file opens either way.
- `system.sleep` and `system.wait_event` are coroutine yields; calling them
  inside a `core.add_thread` coroutine is unsupported (stock Lua never
  does, third-party plugins should not either).
- The `renderer.show_debug` overlay tints the whole dirty rect; lite drew
  it under the frame's last clip, which could hide part of the overlay.

# Deviations from lite's `data/`

`data/` began as a byte-per-byte copy of rxi's lite `data/` directory at
master (`38bd9b3`, v1.11 plus rxi's last fixes) and is evolving from there.
Every intentional change is listed here, with what changed and why. The
reference copy lives untouched in `/lite/` (git-ignored, read-only).

The whole tree is formatted with stylua (config in `/stylua.toml`, tuned to
read like rustfmt output). To diff meaningfully against the reference,
format a copy of it the same way first:

    cp -r lite/data /tmp/lite-data
    stylua --config-path stylua.toml /tmp/lite-data
    diff -r /tmp/lite-data data

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

## 3. One font: jetbrains mono nerd font

**Files:** `data/fonts/`, `data/core/style.lua`, `data/core/init.lua`,
`data/core/statusview.lua`, `data/plugins/treeview.lua`

lite's three fonts (`font.ttf`, `monospace.ttf`, `icons.ttf`) are replaced by
a single `jetbrainsmono.ttf`: JetBrains Mono Regular patched by Nerd Fonts
(release v3.5.0, OFL), loaded at different sizes for UI, wordmark, code and
icons. It is the *Mono* flavor of the patch, where icons are drawn to fit
exactly one cell: their ink matches their advance, so everything the lua
layer measures with `get_width` is truthful and layouts need no fudge. The
icon font is simply loaded bigger (16px vs 14px UI) to keep the icons from
looking squeezed. The icons are Nerd Font glyphs living in the font's
private use area, named once in `style.icons` and referenced by name at
every call site instead of lite's single-letter mappings ("f", "d", "g", ...).

## 4. Catppuccin mocha color scheme, green accent

**File:** `data/core/style.lua`

The default theme is catppuccin mocha instead of lite's grayscale. Colors
come from the official palette (catppuccin/palette) and the syntax roles
follow the official style guide (keywords mauve, strings green, numbers
peach, functions blue, operators sky, comments overlay2, selection overlay2
at 25%). The accent is green: caret and highlighted UI text. Same palette
structure as lite's, only the values changed.

## 5. The treeview divider is actually draggable

**Files:** `data/core/rootview.lua`, `data/plugins/treeview.lua`

lite showed the resize cursor on the treeview divider but dragging did
nothing (open lite issue #113): the drag adjusted a proportional divider
that the layout ignores for locked splits, and the treeview re-pinned its
width every frame anyway. wisp adds a small protocol: a locked view may
implement `set_target_size(axis, value)` to accept divider drags, and the
treeview does. Dividers that nothing can resize (command view, status bar)
no longer show the resize cursor at all -- the cursor only promises what a
drag can deliver.

## 6. Treeview scrolling is clamped to its content

**File:** `data/plugins/treeview.lua`

lite's base view reports an infinite scrollable size, so the treeview
scrolled past its last item into the void forever. wisp's treeview reports
the real height of its visible items, so scrolling stops at the bottom
like every other editor.

## 7. Binary files are refused

**Files:** `data/core/doc/init.lua`, `data/core/init.lua`

lite loaded any file into a docview, rendering binaries as garbage text
(and risking corrupting them on save). wisp checks the first 4096 bytes on
load: a null byte means binary, and the open is refused with an error
message in the status bar. Files passed on the command line get the same
treatment (wrapped in `core.try`), so `wisp some.bin` starts the editor
with a message instead of dying.

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

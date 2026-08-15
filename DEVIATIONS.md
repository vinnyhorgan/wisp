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

Because the confirmation is itself a prompt, `CommandView:enter` now cancels
a pending prompt instead of silently refusing (lite's `enter` just returned).
Without this, quitting -- or anything else that needs to ask -- would appear
to do nothing while a find/rename/... prompt was open. The newest prompt
wins; the old one is cancelled exactly as if escape had been pressed.

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
drag can deliver. The width is clamped on both ends (80px minimum, 80px
short of the window at most) so the divider always stays on screen and
can be grabbed again.

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

## 8. Horizontal scrolling

**Files:** `data/core/view.lua`, `data/core/docview.lua`,
`data/core/doc/init.lua`, `data/core/init.lua`

lite had no horizontal scrolling: the wheel's x axis was dropped by the C
core and the lua layer only handled y. wisp's core always delivered both
axes; now the lua layer uses them. A view opts in by reporting its content
width through `get_h_scrollable_size()` (default 0: no sideways scrolling),
and the docview reports its widest line -- measured once and cached against
a new `Doc.change_count`, bumped on every edit, since measuring every line
per frame would be too slow. Horizontal scroll is clamped to the content on
both ends, like the vertical fix in §6. The clamp lives in the wheel handler
-- the only source of sideways scrolling -- because measuring content width
can mean scanning the whole document, which must never happen per frame
(the other setter, caret-following, is in range by construction). The
command view opts out entirely: a wheeled-away prompt had no way to ever
scroll back. Shift turns a vertical wheel into a horizontal one, as in
most editors.

## 9. All user-facing text is lowercase

**Files:** `data/core/init.lua`, `data/core/docview.lua`,
`data/core/logview.lua`, `data/core/statusview.lua`,
`data/core/commands/*.lua`, `data/plugins/*.lua`

rxi's lowercase style covers lite's code and prose, but its UI strings were
Title Case ("Open File From Project", "Save As", "Project"). wisp commits
fully: command view prompts, suggestions, log and error messages, view
names and the status bar ("crlf"/"lf") are all lowercase. Internal assert
messages (only ever visible on a bug) were left as lite wrote them.

## 10. Assorted lua-layer fixes

Bugs inherited from lite's `data/`, found in the stabilization pass. Each
fix is small and local:

- **`data/core/doc/translate.lua`** -- `previous_char`/`next_char` looped
  forever on a malformed utf-8 continuation byte at either end of the doc
  (a latin-1 file starting with a curly quote hung the editor on
  backspace). They now use the same same-position guard rxi already used
  in `start_of_word`.
- **`data/plugins/projectsearch.lua`** -- searching a project with zero
  files divided by zero while drawing the progress header; the resulting
  `inf` made `%d` error on the draw path, outside any `core.try`, killing
  the editor.
- **`data/plugins/autoreload.lua`** -- a file changing on disk silently
  replaced the buffer and marked it clean, even when it held unsaved
  edits. A dirty doc now keeps its changes (with a status message)
  instead. Reloads also refuse files that turned binary (matching §7) and
  update the doc's crlf flag to the file's actual line endings.
- **`data/core/commands/doc.lua`** -- `doc:rename` decided "same file" by
  comparing path strings, so on a case-insensitive filesystem renaming
  `Foo.txt` to `foo.txt` saved and then deleted the very same file. The
  old path is now removed only when its stats differ from the file just
  written; when in doubt nothing is deleted.
- **`data/core/init.lua`**, **`data/core/config.lua`** -- the project scan
  had no bound, so opening the editor in a huge directory tree ate memory
  without limit (lite issues #185/#208; fix modeled on lite PR #218). The
  scan now stops at `config.max_project_files` (2000) and logs that it
  did.
- **`data/plugins/tabularize.lua`** -- fields were split with a
  `[^d]+` pattern built from the delimiter's first character, which
  silently dropped empty fields (`a,,b` became `a,b`) and corrupted
  multi-character delimiters (`a->b` became `a->>b`). Splitting is now a
  plain full-delimiter split that keeps empty fields.

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

# deviations from lite's `data/`

`data/` began as a byte-per-byte copy of rxi's lite `data/` directory at
master (`38bd9b3`, v1.11 plus rxi's last fixes) and is evolving from there.
every intentional change is listed here, with what changed and why. the
reference copy lives untouched in `/lite/` (git-ignored, read-only).

the whole tree is formatted with stylua (config in `/stylua.toml`, tuned to
read like rustfmt output). to diff meaningfully against the reference,
format a copy of it the same way first:

    cp -r lite/data /tmp/lite-data
    stylua --config-path stylua.toml /tmp/lite-data
    diff -r /tmp/lite-data data

## 1. quit confirmation uses the CommandView, not an os dialog

**file:** `data/core/init.lua`, `core.quit()`

lite called `system.show_confirm_dialog()` — the only use of that api in the
entire codebase — to ask about unsaved changes on quit, putting an os message
box on top of the editor. wisp's core does not expose an os dialog api at all.
the quit confirmation is routed through `core.command_view:enter()` with
yes/no suggestions instead, so the editor confirms with its own ui, in its own
theme. type `y`/`yes` to quit, anything else (or escape) to cancel.

because the confirmation is itself a prompt, `CommandView:enter` now cancels
a pending prompt instead of silently refusing (lite's `enter` just returned).
without this, quitting -- or anything else that needs to ask -- would appear
to do nothing while a find/rename/... prompt was open. the newest prompt
wins; the old one is cancelled exactly as if escape had been pressed.

## 2. branding says wisp, not lite

**files:** `data/core/init.lua`, `data/core/rootview.lua`,
`data/core/commands/core.lua`

the editor is called wisp everywhere the user can see the name: the window
title (`"file - wisp"`), the wordmark on the empty view, the project module
(`.wisp_project.lua`), and the temp file prefix (`.wisp_temp_*`). the scale
override env var is `WISP_SCALE` (was `LITE_SCALE`).

## 3. one font: jetbrains mono nerd font

**files:** `data/fonts/`, `data/core/style.lua`, `data/core/init.lua`,
`data/core/statusview.lua`, `data/plugins/treeview.lua`

lite's three fonts (`font.ttf`, `monospace.ttf`, `icons.ttf`) are replaced by
a single `jetbrainsmono.ttf`: jetbrains mono regular patched by nerd fonts
(release v3.5.0, ofl), loaded at different sizes for ui, wordmark, code and
icons. it is the *mono* flavor of the patch, where icons are drawn to fit
exactly one cell: their ink matches their advance, so everything the lua
layer measures with `get_width` is truthful and layouts need no fudge. the
icon font is simply loaded bigger (16px vs 14px ui) to keep the icons from
looking squeezed. the icons are nerd font glyphs living in the font's
private use area, named once in `style.icons` and referenced by name at
every call site instead of lite's single-letter mappings ("f", "d", "g", ...).

## 4. catppuccin mocha color scheme, green accent

**file:** `data/core/style.lua`

the default theme is catppuccin mocha instead of lite's grayscale. colors
come from the official palette (catppuccin/palette) and the syntax roles
follow the official style guide (keywords mauve, strings green, numbers
peach, functions blue, operators sky, comments overlay2, selection overlay2
at 25%). the accent is green: caret and highlighted ui text. same palette
structure as lite's, only the values changed.

## 5. the treeview divider is actually draggable

**files:** `data/core/rootview.lua`, `data/plugins/treeview.lua`

lite showed the resize cursor on the treeview divider but dragging did
nothing (open lite issue #113): the drag adjusted a proportional divider
that the layout ignores for locked splits, and the treeview re-pinned its
width every frame anyway. wisp adds a small protocol: a locked view may
implement `set_target_size(axis, value)` to accept divider drags, and the
treeview does. dividers that nothing can resize (command view, status bar)
no longer show the resize cursor at all -- the cursor only promises what a
drag can deliver. the width is clamped on both ends (80px minimum, 80px
short of the window at most) so the divider always stays on screen and
can be grabbed again.

## 6. treeview scrolling is clamped to its content

**file:** `data/plugins/treeview.lua`

lite's base view reports an infinite scrollable size, so the treeview
scrolled past its last item into the void forever. wisp's treeview reports
the real height of its visible items, so scrolling stops at the bottom
like every other editor.

## 7. binary files are refused

**files:** `data/core/doc/init.lua`, `data/core/init.lua`

lite loaded any file into a docview, rendering binaries as garbage text
(and risking corrupting them on save). wisp checks the first 4096 bytes on
load: a null byte means binary, and the open is refused with an error
message in the status bar. files passed on the command line get the same
treatment (wrapped in `core.try`), so `wisp some.bin` starts the editor
with a message instead of dying.

## 8. horizontal scrolling

**files:** `data/core/view.lua`, `data/core/docview.lua`,
`data/core/doc/init.lua`, `data/core/init.lua`

lite had no horizontal scrolling: the wheel's x axis was dropped by the c
core and the lua layer only handled y. wisp's core always delivered both
axes; now the lua layer uses them. a view opts in by reporting its content
width through `get_h_scrollable_size()` (default 0: no sideways scrolling),
and the docview reports its widest line -- measured once and cached against
a new `Doc.change_count`, bumped on every edit, since measuring every line
per frame would be too slow. horizontal scroll is clamped to the content on
both ends, like the vertical fix in §6. the clamp lives in the wheel handler
-- the only source of sideways scrolling -- because measuring content width
can mean scanning the whole document, which must never happen per frame
(the other setter, caret-following, is in range by construction). the
command view opts out entirely: a wheeled-away prompt had no way to ever
scroll back. shift turns a vertical wheel into a horizontal one, as in
most editors; a wheel that already scrolls sideways is left untouched.

## 9. all user-facing text is lowercase

**files:** `data/core/init.lua`, `data/core/docview.lua`,
`data/core/logview.lua`, `data/core/statusview.lua`,
`data/core/commands/*.lua`, `data/plugins/*.lua`

rxi's lowercase style covers lite's code and prose, but its ui strings were
title case ("Open File From Project", "Save As", "Project"). wisp commits
fully: command view prompts, suggestions, log and error messages, view
names and the status bar ("crlf"/"lf") are all lowercase. the command
palette prettifies command names without capitalizing them ("doc: save
as", not "Doc: Save As"). internal assert messages (only ever visible on
a bug) were left as lite wrote them.

## 10. assorted lua-layer fixes

bugs inherited from lite's `data/`, found in the stabilization pass. each
fix is small and local:

- **`data/core/rootview.lua`**, **`data/core/commands/core.lua`**,
  **`data/plugins/projectsearch.lua`** -- opening a doc (or the log, or
  search results) while the treeview or a prompt held focus tripped
  "Cannot open doc on locked node": the fallback went to the last active
  view, which by submit time is the also-locked command view. a new
  `get_active_node_default()` falls back to the first unlocked leaf --
  the editing area, which always exists -- and the three open sites use
  it.
- **`data/core/docview.lua`** -- the caret-follow scroll recomputed the
  horizontal offset from the caret position alone, parking the caret at
  4/5 of the view width on every caret move. that discarded the user's
  own sideways scroll, and during a drag it fed back into the mouse
  position: the view slid under the pointer, the next mouse event
  resolved further into the line, and the selection galloped away. the
  follow now scrolls only when the caret would leave the view (the fix
  in franko's unmerged lite PR #230).
- **`data/core/doc/translate.lua`** -- `previous_char`/`next_char` looped
  forever on a malformed utf-8 continuation byte at either end of the doc
  (a latin-1 file starting with a curly quote hung the editor on
  backspace). they now use the same same-position guard rxi already used
  in `start_of_word`.
- **`data/plugins/projectsearch.lua`** -- searching a project with zero
  files divided by zero while drawing the progress header; the resulting
  `inf` made `%d` error on the draw path, outside any `core.try`, killing
  the editor.
- **`data/plugins/projectsearch.lua`** (hardening) -- a malformed lua
  pattern is rejected at the prompt with an error message instead of
  raising inside the search thread; binary files are skipped by the same
  null-byte rule the doc loader uses (§7); and a superseded search
  cancels through an explicit generation check instead of lite's weak
  thread key, which cancelled only whenever the gc got around to it (the
  old thread could cross into the next file, and the new results list,
  in the gap).
- **`data/core/init.lua`** -- a background thread that raises no longer
  takes the whole main loop down with it (lite asserted on the resume):
  the error is shown via `core.error` and the dead thread is reaped. an
  invalid search pattern used to kill the editor this way.
- **`data/plugins/autoreload.lua`** -- a file changing on disk silently
  replaced the buffer and marked it clean, even when it held unsaved
  edits. a dirty doc now keeps its changes (with a status message)
  instead. reloads also refuse files that turned binary (matching §7) and
  update the doc's crlf flag to the file's actual line endings.
- **`data/core/commands/doc.lua`** -- `doc:rename` decided "same file" by
  comparing path strings, so on a case-insensitive filesystem renaming
  `Foo.txt` to `foo.txt` saved and then deleted the very same file. the
  old path is now removed only when its stats differ from the file just
  written; when in doubt nothing is deleted.
- **`data/core/init.lua`**, **`data/core/config.lua`** -- the project scan
  had no bound, so opening the editor in a huge directory tree ate memory
  without limit (lite issues #185/#208; fix modeled on lite PR #218). the
  scan now stops at `config.max_project_files` (2000) and logs that it
  did.
- **`data/core/commands/doc.lua`** -- every whole-line command
  (select/duplicate/delete/move lines) began by materializing a phantom
  newline at the end of the doc (`append_line_if_last_line`) whenever
  the block touched the last line -- a real edit: ctrl+l on the last
  line dirtied the doc, and moving the last line down fed blank lines
  into it. each command now handles the last line explicitly and the
  helper is gone; line commands never edit what they were not asked to.
- **`data/plugins/tabularize.lua`** -- fields were split with a
  `[^d]+` pattern built from the delimiter's first character, which
  silently dropped empty fields (`a,,b` became `a,b`) and corrupted
  multi-character delimiters (`a->b` became `a->>b`). splitting is now a
  plain full-delimiter split that keeps empty fields.

## core behavior notes (rust core vs lite's c core)

the core fixes lite's bugs instead of reproducing them. the observable
differences, all deliberate:

- invalid utf-8 renders as the replacement character; lite's decoder walked
  out of bounds on malformed input.
- `SCALE` defaults to the real display scale factor; lite detected dpi only
  on windows and hardcoded 1.0 elsewhere. `WISP_SCALE` still overrides it
  (desktop only; headless boots ignore it so tests render identically on
  every machine).
- the mousewheel event carries the horizontal axis as an extra value after
  the vertical one; stock lua ignores it.
- rapid clicks cycle caret, word, line, caret, ...; sdl counted up forever.
  observable only from the fifth rapid click onward.
- numpad navigation keys with num lock off report their meaning ("home",
  "end", ...); sdl reported "keypad 7" and friends, which no keymap binds.
- keys are named by their unshifted character (sdl behavior), and synthetic
  key presses on focus gain are dropped (the alt-tab bug lite patched
  around sdl).
- file-drop coordinates are the last known cursor position; winit does not
  report the pointer during a drag, so a drop may land in another split.
  the file opens either way.
- `system.sleep` and `system.wait_event` are coroutine yields; calling them
  inside a `core.add_thread` coroutine is unsupported (stock lua never
  does, third-party plugins should not either).
- the `renderer.show_debug` overlay tints the whole dirty rect; lite drew
  it under the frame's last clip, which could hide part of the overlay.

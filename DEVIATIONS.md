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

**files:** `data/core/init.lua` (`core.quit()`), `data/core/commandview.lua`

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
`data/core/commands/core.lua` (and `src/boot.rs` for `WISP_SCALE`)

the editor is called wisp everywhere the user can see the name: the window
title (`"file - wisp"`), the wordmark on the empty view, the project module
(`.wisp_project.lua`), and the temp file prefix (`.wisp_temp_*`). the scale
override env var is `WISP_SCALE` (was `LITE_SCALE`).

## 3. one font: jetbrains mono nerd font

**files:** `data/jetbrainsmono.ttf`, `data/core/style.lua`,
`data/core/init.lua`, `data/core/statusview.lua`,
`data/plugins/treeview.lua`

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

two spacings shrink with the font: the statusview separators (three
spaces and ` | `, down from lite's six and `   |   `) and the treeview
icon gap (one space, was two). the mono font's spaces are half a cell
each, twice the width of lite's proportional ones, so lite's counts
read double.

## 4. catppuccin mocha color scheme, green accent

**files:** `data/core/style.lua`, `data/user/`

the default theme is catppuccin mocha instead of lite's grayscale. colors
come from the official palette (catppuccin/palette) and the syntax roles
follow the official style guide (keywords mauve, strings green, numbers
peach, functions blue, operators sky, comments overlay2, selection overlay2
at 25%). the accent is green: caret and highlighted ui text. same palette
structure as lite's, only the values changed. lite's two bundled
alternate schemes (`data/user/colors/` and the commented require in
`data/user/init.lua`) went with the switch: one theme, tuned, instead
of three half-tuned.

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
like every other editor. counting those items means walking the project
(up to `config.max_project_files` entries), and the clamp asks on every
update while the scrollbar asks on every draw and every mouse move, so
the count is cached and dropped when the project or a folder changes --
the same shape as the docview's widest-line cache.

## 7. binary files are refused

**files:** `data/core/doc/init.lua`, `data/core/init.lua`

lite loaded any file into a docview, rendering binaries as garbage text
(and risking corrupting them on save). wisp checks the first 4096 bytes on
load: a null byte means binary, and the open is refused with an error
message in the status bar. files passed on the command line get the same
treatment (wrapped in `core.try`), so `wisp some.bin` starts the editor
with a message instead of dying.

the refusal is the default, not a dogma: the core can draw images now
(see the core notes), so a view that claims a file type can open it --
phase d's imageview will claim pngs and jpegs, and the docview keeps
refusing everything binary that nothing claims.

## 8. horizontal scrolling

**files:** `data/core/view.lua`, `data/core/docview.lua`,
`data/core/doc/init.lua`, `data/core/init.lua`,
`data/core/commandview.lua`, `data/core/logview.lua`,
`data/plugins/projectsearch.lua`, `data/plugins/treeview.lua`

lite had no horizontal scrolling: the wheel's x axis was dropped by the c
core and the lua layer only handled y. wisp's core always delivered both
axes; now the lua layer uses them. a view opts in by reporting its content
width through `get_h_scrollable_size()` (default 0: no sideways scrolling),
and the docview reports its widest line -- measured once and cached against
a new `Doc.change_count`, bumped on every edit, since measuring every line
per frame would be too slow. horizontal scroll is clamped to the content on
both ends, like the vertical fix in §6. the clamp is an invariant over
scroll, size and content, and any of the three can move (a wheel, a
divider drag, collapsing the folder that held the widest name), so it is
enforced in update like the vertical one -- but only while actually
panned sideways, which keeps the common case free; the docview's cached
widest line keeps the panned case as cheap as the vertical measurement. the
command view opts out entirely: a wheeled-away prompt had no way to ever
scroll back. shift turns a vertical wheel into a horizontal one, as in
most editors; a wheel that already scrolls sideways is left untouched.
diagonal input follows one axis: trackpad glides drift on both axes at
once, and letting both through feels like panning a map instead of
scrolling text. a trackpad gesture (the core forwards winit's touch
phases) is railed to the axis it starts on until the fingers lift; a
discrete wheel event stands alone and its bigger axis wins -- literally
alone: it ignores any rail still latched, since the "ended" phase that
would have cleared one is not guaranteed to arrive (focus can be lost
mid-gesture). the
docview's scrollable width leaves the same three-space margin as the
caret band, so the wheel reaches exactly as far as caret-follow ever
scrolls, and no further.

## 9. all user-facing text is lowercase

**files:** `data/core/init.lua`, `data/core/command.lua`,
`data/core/docview.lua`, `data/core/logview.lua`,
`data/core/statusview.lua`, `data/core/commands/*.lua`,
`data/plugins/*.lua`

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
  the editing area, which always exists -- and the four open sites use
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
  cancels through an explicit generation check on top of lite's weak
  thread key, which on its own cancelled only whenever the gc got around
  to it (the old thread could cross into the next file, and the new
  results list, in the gap). the weak key is still passed; the check
  closes the window it left open.
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
  old path is now removed only when `system.absolute_path` resolves the
  two names differently -- the filesystem's own answer to "same file".
  the first attempt here compared stats instead and was wrong the other
  way: mtimes are whole seconds and a clean doc's re-save is
  byte-identical, so renaming inside one second read as "same file" and
  left the old file on disk.
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
- **`data/core/docview.lua`** -- triple-clicking the last line inserted
  a real newline so the line selection had a next line to end on: the
  same phantom-newline edit the line commands above were cured of,
  never filed upstream. the selection end is left to the doc's
  position sanitizing now, which clamps it to the end of the doc; the
  selection covers the same text and the doc stays clean.
- **`data/plugins/treeview.lua`** -- hover was only recomputed on mouse
  move, so wheel-scrolling under a stationary pointer left the highlight
  (and the click target) on the pre-scroll row: clicking opened the
  wrong file. the hovered item now follows the rows whenever the scroll
  moves. the treeview also gained the scrollbar every other scrolling
  view already had (clicks and drags on it reach the scrollbar now that
  the mouse handlers call their base class), and long filenames pan
  sideways through the §8 protocol, with the hover highlight pinned to
  the view width.
- **`data/plugins/autocomplete.lua`** -- the suggestion dedup walked the
  sorted matches with the wrong index (the output slot instead of the
  entry a duplicate run started at), so once two providers offered the
  same symbol the list filled up with repeats of one entry. duplicates
  are now merged through a seen-set, keeping whichever entry carries an
  info tag. latent in stock lite (the open-docs provider is a set), it
  bites as soon as a second provider exists -- which is what plugins do.
- **`data/core/commands/findreplace.lua`** -- "previous find" before any
  find popped from a nil history table; the raw lua error reached the
  user. it now reports "no previous finds" like every other empty case.
- **`data/core/logview.lua`** -- the log view scrolled into the void
  forever (the base view reports an unbounded scrollable size); it now
  measures its items, the same rule as §6.
- **`data/core/logview.lua`**, **`data/plugins/projectsearch.lua`** --
  both views held content wider than themselves with no way to reach
  it; they opt into the §8 sideways-scroll protocol now. the log
  measures its rows directly (items are capped), the results view
  caches its widest row against the result count.
- **`data/core/statusview.lua`** -- the column readout counted bytes, so
  multibyte text inflated it (lite issue #300); it counts characters
  now.
- **`data/core/commands/doc.lua`** -- the command view is a docview, so
  ctrl+s (or save-as, or rename) inside a prompt ran against the
  prompt's one-line doc and offered to write the prompt's text to disk.
  the file-facing commands now require a docview that is not a command
  view; editing and movement commands stay shared, prompts need them.
- **`data/core/docview.lua`** -- `get_line_screen_position` accepts an
  optional column (lite issue #313): plugins that passed one silently
  got the start of the line.
- **`data/core/common.lua`**, **`data/core/commands/core.lua`**,
  **`data/core/commands/doc.lua`** -- `common.home_expand` turns a
  leading "~" into the home directory, and the path prompts (open file,
  save as, rename, path suggestions) use it, like every shell would.
- **`data/plugins/language_c.lua`** -- `const` was defined twice in the
  symbol table (lite PR #224); one removed, no behavior change.
- **`data/core/common.lua`** -- `common.color`'s `rgba()` branch handed
  back gmatch's strings instead of numbers, which worked only because
  every consumer coerced them. lite's own themes never took that branch;
  wisp's `style.selection` does, so it returns numbers like the other
  branches. `path_suggest` also drops a dead capture that the 5.5 loop
  rename had quietly started shadowing.
- **`data/plugins/autoreload.lua`** -- the file can vanish between the
  stat and the open (a checkout, a build): the unchecked `io.open`
  raised inside the reload thread, which killed the thread and with it
  every later reload in the session.
- **`data/core/statusview.lua`** -- the left and right item groups are
  placed independently, one from each edge, so in a narrow window the
  filename ran straight through the line count and the two overprinted
  each other. the left group now gets the room the right one leaves less
  a `padding.x` gap, and fades out into that gap over the last `2 *
  padding.x` rather than stopping dead against the right group's icon.
- **`data/core/init.lua`** -- a `terminate` event (sigterm, sigint,
  sighup) had no handler because lite's core never delivered one. it
  cannot go through `core.quit`: that prompts about unsaved changes, and
  a session that is already ending has nobody to answer. `core.terminate`
  writes each dirty doc beside its original as `name~` -- the same rescue
  an unhandled error does -- and then force-quits.
- **`data/core/config.lua`**, **`data/core/init.lua`** -- one
  `mouse_wheel_scroll` served both devices. a wheel notch is a
  quantized command and a trackpad glide is direct manipulation
  measured in finger pixels; the core hands both over in notch units
  (a glide's pixels divided down), and lite only ever saw the wheel, so
  it only had the one number. the wheel itself was slow, too:
  50 pixels a notch is 2.4 lines at wisp's metrics (21px of line,
  measured, not guessed), where the rest of the desktop moves three or
  more, so it is 84 now -- four lines exactly, which also keeps a notch
  landing on a line boundary. and a glide's delta is multiplied by
  `config.trackpad_scroll_gain` in `core.on_event` -- the one place
  that knows which device sent the event -- so every consumer
  downstream, the terminal's scrollback included, stays
  device-agnostic.
- **`data/core/init.lua`** -- a modifier held while focus left the
  window stayed latched forever: wayland delivers no key releases on
  focus loss (x11 synthesizes them), so alt+tab with alt down turned
  every later chord into a dead alt+... variant until restart. held
  modifiers are now forgotten the moment focus goes.
- **`data/plugins/tabularize.lua`** -- fields were split with a
  `[^d]+` pattern built from the delimiter's first character, which
  silently dropped empty fields (`a,,b` became `a,b`) and corrupted
  multi-character delimiters (`a->b` became `a->>b`). splitting is now a
  plain full-delimiter split that keeps empty fields.
- **`data/core/docview.lua`**, **`data/plugins/projectsearch.lua`** --
  both cache a widest-line measurement in pixels, keyed on the content
  that produced it: the doc's `change_count`, the result list's length.
  a font that resizes under the cache invalidates every width in it
  without touching either key, so the sideways scroll range went stale
  the moment §15 landed. both caches now also key on the font's size.

## 11. launched bare, wisp opens the current directory

**file:** `data/core/init.lua`

lite fell back to EXEDIR -- the directory holding the executable -- when
the command line named no directory, so a bare `lite` opened its own
installation as the project (lite issue #153). a bare `wisp` opens the
directory it was launched from, like every other terminal program.

## 12. a real terminal

**files:** `data/plugins/terminal.lua` (new), `data/core/keymap.lua`

lite's answer to a terminal was the toy console plugin. wisp ships a
real one: the core embeds alacritty's vt engine on a polled pty
(`system.terminal`, see the core notes), and this plugin is everything
visible -- the grid drawn as style runs in the code font, catppuccin
mocha's official terminal palette, key-to-escape translation, 10k
lines of scrollback on the wheel, osc titles in the tab. while a
terminal has focus its keys belong to the shell; the handful of
editor keys that stay editor keys live in
`config.terminal_pass_through` (the palette, `ctrl+\``, tab and split
navigation). `terminal:toggle` on `ctrl+\`` jumps between the
terminal and the last view; a finished shell closes its own tab.

deciding that takes the two questions keymap asks itself on every key
-- is this key a modifier, what stroke is held -- so `modkey_map` and
`key_to_stroke` are public on the keymap module instead of copied into
the plugin. any view that takes the keyboard over (a modal mode, one
day) needs the same two.

## 13. lua 5.5

**files:** `data/core/strict.lua`, `data/core/common.lua`,
`data/core/init.lua`, `data/core/doc/init.lua`, `data/core/commandview.lua`,
`data/core/keymap.lua`, `data/core/statusview.lua`,
`data/plugins/autocomplete.lua`, `data/plugins/projectsearch.lua`,
`data/plugins/treeview.lua`, `data/plugins/language_lua.lua`

lite embedded lua 5.2; wisp embeds lua 5.5 (mlua's vendored build,
still the only c compiled). the semantics the editor's design leans on
survive the jump unchanged -- yieldable pcall/xpcall (the exit path),
ephemeron weak tables, short-string interning, `os.exit` argument
handling -- and the audit for the rest touched exactly these:

- **the 5.3 integer split.** division is always float now, and
  `string.format("%d")` refuses any non-integral float (5.2 truncated
  silently; the old guard here was only about inf). the two percent
  readouts -- statusview's document position and projectsearch's
  progress -- floor before formatting, and so does the temp-file uid
  in `core/init.lua`, which feeds `get_time() * 1000` to `%08x`. that
  third site shipped unfloored: the headless clock reads an integral
  0.0 at require time, so the suite passed while every desktop launch
  died on it. the regression test seeds a fractional virtual clock
  before boot (`Headless::set_clock`), closing the class. everything
  else feeding `%d`, `string.sub` and friends is integer-sourced.
- **for-loop variables are const in 5.5.** eight loops reassigned
  their control variable (crlf stripping in `Doc:load`/`save`, the
  advancing `x` in treeview and projectsearch draws, normalization in
  keymap/commandview/autocomplete/`common.path_suggest`); six shadow
  with a local, two read better renamed, semantics identical either way.
- **`global` is a reserved word in 5.5**, so lite's
  `function global(t)` in strict.lua no longer parses -- and neither
  would any caller, so the old name was unkeepable by definition. the
  declarator is now `declare { name = value }`; the strict-globals
  metatable machinery is unchanged. ported plugins calling `global {}`
  need the same one-word fix.
- **`common.utf8_chars` iterates with the stdlib's
  `utf8.charpattern`** instead of lite's hand-written near-copy. one
  visible difference on garbage input: invalid lead bytes f5-fd form
  their own one-byte chars instead of silently vanishing -- bytes
  should never disappear on their way to the screen.
- `language_lua` highlights `global` as the keyword it now is.

## 14. the status bar can be hidden

**files:** `data/core/statusview.lua`, `data/core/commands/core.lua`,
`data/core/keymap.lua`

`core:toggle-status-bar` on `ctrl+shift+\` collapses the status bar and
brings it back -- the sibling of the treeview's `ctrl+\`, and lite had
no way to do it at all. the bar animates its height to zero exactly as
the treeview animates its width, and nothing else in the layout needed
changing: a locked node already reports its view's own size, and
`get_locked_size` already drops the divider once a side collapses.

one thing did have to change. the message row is drawn one row-height
below the item row and scrolls up to replace it, and lite keyed that
offset to `size.y` -- fine when the height never moved. animated, both
the offset and the box the text is centred in collapse together, sliding
an expired message up into view partway through the hide. the row height
is now its own function, independent of the animated size.

## 15. runtime zoom

**files:** `data/plugins/scale.lua` (new), `data/core/keymap.lua`,
`data/core/init.lua`, `data/plugins/treeview.lua`, `data/core/view.lua`

`ctrl+=` / `ctrl+-` / `ctrl+0` and `ctrl+wheel` zoom the editor.

lite's whole font api is `load`, `get_width`, `get_height` and
`set_tab_width`: a loaded font's size is fixed. so rxi's scale plugin
monkey-patches `renderer.font.load`, keeps a weak table of every font's
path and size, and reloads each one from disk at every step -- and
seeds that table with a hardcoded copy of style.lua's four font lines
that a comment asks you to keep in sync by hand. lite-xl added
`font:set_size` to its core and its plugin uses it; wisp's core took
the same shape (landed at the freeze with this as its named consumer),
so the plugin here is only arithmetic. the difference from lite-xl is
in the arithmetic, not in the api.

three decisions differ from lite-xl's version:

- **one mode, not two.** lite-xl offers "code" (the editor font only)
  and "ui" (everything), defaulting to code. with one font at four
  sizes there is nothing to gain from leaving the chrome behind at the
  old size next to text at the new one, so zooming scales everything --
  the way a browser does. `config.plugins.scale` and its config_spec
  have no counterpart here.
- **every value is recomputed from its boot value, never from its
  current one.** lite-xl multiplies the live numbers by the step's
  ratio, so `common.round` on padding and divider sizes throws away a
  little each step and a reset lands near, but not on, where it
  started. measuring from a base makes a reset an identity, which the
  regression test asserts as whole-frame pixel equality after
  +3 steps, -6 steps, reset. that test fails against lite-xl's shape.
- **zoom is a multiple of the boot scale**, not an absolute scale
  factor, so 1 is always "normal" and a step is the same fraction of it
  whatever `WISP_SCALE` or the display said. the one-pixel details
  (divider, caret, scrollbar) keep a floor of 1px, since a hairline
  that rounds to zero stops being drawn at all.

the zoom a session ends on is not remembered: that belongs with the
wave's session-restore work. until then the way to boot at something
other than 100% is the user module -- `require("plugins.scale").set(1.3)`
in `~/.config/wisp/init.lua`, which runs after the plugins load -- and a test
pins that path against the same zoom typed in. `WISP_SCALE` is a
different knob and stays: it sets what 100% *means* on a display whose
reported scale is wrong, and a reset returns to it.

three supporting changes. `keymap.on_mouse_wheel` gives the vertical
wheel a stroke name (`wheelup` / `wheeldown`, with modifiers) so
`ctrl+wheel` is an ordinary binding rather than a special case in
`core.on_event`; an unbound wheel falls through to scrolling exactly as
before. and the treeview stores its width at the scale the editor
booted at, multiplying it up on the way out, so a zoom scales a
hand-dragged width exactly and a reset returns it to the pixel it was
dragged to. and `View:get_scrollbar_rect` scales the minimum thumb
height: lite floored it at a hardcoded 20 pixels, which is a
different-sized grab target on every display and at every zoom.

## 16. the project is watched, not polled

**file:** `data/core/init.lua`

lite walked the whole project tree every `config.project_scan_rate`
seconds, forever, and compared the result to the last one -- the
standing cost that `config.max_project_files` exists to bound. wisp
opens a `system.watch` on the project directory at startup and walks
the tree when the watcher says something under it changed. a checkout
or a build shows up within a poll interval instead of up to five
seconds later, and an idle project costs nothing at all.

the shape is deliberate:

- **the poll always follows a wait, never precedes it**, so the poll
  interval is also the fastest the tree can be walked. a build emits
  events for as long as it runs; this holds it to four walks a second
  no matter how many arrive.
- **0.25s is free.** `core.run` already waits at most that long when
  idle, so asking the watcher costs nothing it was not already paying.
- **the timer survives as a safety net, at a minute.** the backend
  thread can die -- the api reports that exactly once and then goes
  quiet forever -- and there are filesystems inotify cannot see into.
  a file list that goes stale and never heals is worse than one walk a
  minute.
- **a watcher that fails to open is a fine answer.** the scan thread
  falls back to `config.project_scan_rate` exactly as before, and the
  reason is logged quietly.

## 17. the user's files live in the xdg config directory

**files:** `data/core/init.lua`, `data/core/commands/core.lua`
(and `src/embed.rs`, `src/boot.rs` for the directories themselves)

lite kept everything beside the binary: `data/user/init.lua` for settings,
`data/plugins` for plugins, `error.txt` in the install directory. that is
correct for a folder you unzip and delete, and wrong for a program that
installs itself. wisp now uses all three xdg base directories for what
each is for:

- **`$XDG_CONFIG_HOME/wisp`** (`USERDIR`, `~/.config/wisp`) -- `init.lua`
  and `plugins/`, created and seeded on first run. this is where a person
  looks for their settings, and installing a plugin is saving a file into
  `~/.config/wisp/plugins/`. it is searched *before* the bundled plugins,
  so a file there of the same name replaces the shipped one.
- **`$XDG_DATA_HOME/wisp`** (`EXEDIR`, `~/.local/share/wisp`) -- the
  unpacked editor, replaced wholesale whenever the version changes.
  nothing of the user's is in it any more, which is what makes replacing
  it wholesale safe.
- **`$XDG_STATE_HOME/wisp`** (`STATEDIR`, `~/.local/state/wisp`) --
  what the editor writes for itself and nobody edits: `error.txt`, the
  temp files, and a restored session when that lands. lite-xl gets the
  config directory right but puts both of these in it.

the details that matter:

- **an install from before the split keeps its settings.** if
  `data/user/init.lua` exists in the old location, first run seeds the
  config directory from it rather than from the stub.
- **`data/user/` is no longer shipped.** it is the seed for a fresh
  config directory, and the user module location for headless boots,
  which never touch the real xdg directories -- a test's user module and
  user plugins live inside the tree the test handed the editor.
- **the user module is a file, not a module name.** `USERDIR/init.lua`
  is loaded by path; no `package.path` pattern can spell a fixed
  filename, and `~/.config/wisp/user.lua` would be a strange thing to
  ask someone to create.
- while adding the second plugin directory, `core.load_plugins` had its
  `gsub(".lua$", "")` escaped to `gsub("%.lua$", "")`. unescaped, the
  dot matched any character, so a plugin named `stylua.lua` would have
  loaded as `sty`.

## 18. a path that does not exist yet is a file to create

**files:** `data/core/init.lua`, `data/core/doc/init.lua`,
`data/plugins/autoreload.lua` (and `src/desktop.rs` for the flags)

`wisp newfile.txt` used to do nothing at all. lite's argument loop keeps
an argument only when `get_file_info` says it is an existing file or an
existing directory (lite #56); anything else was dropped without a word,
and you got the project with no buffer in it. every unix editor gives you
an empty buffer at that path and creates the file on the first save, so
wisp does too.

- `Doc:load` treats a path that does not exist as a new file rather than
  an error. every *other* failure -- unreadable, a directory, a dead
  symlink -- is still fatal, and the existence check is what tells them
  apart, so a permissions problem still says so instead of quietly
  handing back an empty buffer.
- the name is resolved against the directory the editor was launched
  from, before `system.chdir` moves what a relative path means.
  `absolute_path` canonicalizes, so it has no answer for a file that is
  not there; the directory holding it does exist, and resolving that is
  what pins the name down.
- if even that directory is missing, the path is refused with a message
  and the log opens. a buffer nothing could ever save is worse than a
  refusal.
- `core.open_doc` keys its doc cache on the canonical path, which is
  `nil` for a file that does not exist -- so any two such docs compared
  equal as `nil == nil` and the second one silently reused the first
  one's doc. it now falls back to the name as given.
- autoreload's `update_time` indexed the stat result unconditionally. a
  doc with no file on disk has no mtime; `nil` is both the honest answer
  and the useful one, since it differs from every real mtime and so the
  moment the file appears the reload loop picks it up.

the command line got the other half of the same manners. an unrecognized
option used to be ignored -- `wisp --bogus` opened the editor and exited
0, which looks exactly like the flag working. it is now a line on stderr
and exit 2, and the parsing lives in a pure `parse_args` so the whole
contract is unit-tested. `--` is removed once consumed rather than merely
stopped at: left in place, the editor would have opened a document called
`--`.

there is a man page now, `wisp.1`, covering the arguments, the xdg
directories from §17, `WISP_SCALE`, the signals and the exit statuses.

## 19. keymap modes, and helix mode

**files:** `data/core/keymap.lua`, `data/plugins/helix/`

lite's keymap is a flat map from stroke to commands, which is all a
non-modal editor needs. `keymap.mode` adds one level: while a mode name
is set, a stroke is looked for under `"<mode>:<stroke>"` first and falls
back to the unprefixed binding. a mode therefore overrides only the keys
it cares about and inherits `ctrl+s`, `ctrl+f` and everything else
untouched. unset -- which is how the editor starts -- the lookup is
exactly lite's.

`data/plugins/helix/` is the first consumer: selection-first modal
editing in helix's lineage, built to what `hx --tutor` teaches. it is
**opt in** (`config.helix_mode = true` in the user module, or the
`helix: toggle` command) -- one theme and one font are decisions that
can be made for someone, but whether an editor is modal is not.

the model fits lite better than it has any right to. lite's selection is
already an anchor and a head (`selection.b` and `selection.a`), which is
exactly helix's; a helix cursor is a selection one character wide, and
the block you see *is* that selection. so the plugin keeps one
invariant -- outside insert mode a selection is never empty -- and gets
motions, selections and the block cursor from machinery lite already had.
two details follow from it:

- **the block sits on a character, the head sits between two.** a
  forward selection draws its block on the character *behind* the head,
  a backward one on the character *at* it. every motion is fed the block
  position, never the exclusive head, because that is what the person
  typing is pointing at.
- **the block is drawn after the line and the character redrawn over
  it** in the background colour -- inverse video, and it covers lite's
  thin caret without having to suppress it.

typed text reaches the document only in insert mode; in normal and
select mode the letters are commands, so `DocView:on_text_input`
swallows them. the command prompt is a `DocView` subclass and is
deliberately excluded from all of this: a modal layer over a one-line
prompt helps nobody.

## kept on purpose

lite behaviors evaluated deliberately and kept, recorded so they are
not mistaken for oversights:

- saving imposes a trailing newline on files that lack one (lite
  issue #221): the doc model is a list of `"\n"`-terminated lines and
  posix agrees with it.

## core behavior notes (rust core vs lite's c core)

the core fixes lite's bugs instead of reproducing them. the observable
differences, all deliberate:

- **the window names itself.** `wisp` is set as the wayland app id and
  the x11 WM_CLASS. without one the compositor cannot find the window's
  .desktop file, so it shows a placeholder icon and cannot group the
  window in a task list. this is a hole the port opened rather than a
  bug inherited: lite-xl names itself explicitly
  (`SDL_SetAppMetadata("Lite XL", ..., "com.lite_xl.LiteXL")`), and sdl
  documents a sensible default from the application's own name for
  anyone who does not, while winit sets an app id only if asked --
  `if let Some(name) = attributes.platform_specific.name` -- and wisp
  had never asked.
- **sigterm, sigint and sighup are caught** and arrive in lua as a
  `terminate` event, distinct from `quit` (which is a user closing a
  window and may still be argued with). unhandled, they kill the
  process outright: unsaved work gone and the terminal's children
  orphaned. neither reference does this -- lite handles no signal at
  all, lite-xl only `signal(SIGPIPE, SIG_IGN)`, which rust's std
  already does for us. registering the handler through signal-hook's
  safe api keeps the crate's one `unsafe` exception at one.
- **`--help` and `--version` answer on stdout and exit**, and `--` ends
  option parsing. lite and lite-xl treat every argument as a path, so
  asking either one what it is opens a window.
- invalid utf-8 renders as the replacement character; lite's decoder walked
  out of bounds on malformed input.
- `SCALE` defaults to the real display scale factor; lite detected dpi only
  on windows and hardcoded 1.0 elsewhere. `WISP_SCALE` still overrides it
  (desktop only; headless boots ignore it so tests render identically on
  every machine).
- the mousewheel event carries the horizontal axis as an extra value after
  the vertical one, and a third value naming the touch phase of a trackpad
  gesture ("started"/"moved"/"ended"; discrete wheels have none); stock
  lua ignores both.
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
- the desktop binary carries the whole data/ tree inside it: run with no
  data/ beside the executable (and outside a checkout), it unpacks its
  own copy into `$XDG_DATA_HOME/wisp` (`~/.local/share/wisp`) and runs
  from there. one file to install; the unpacked tree stays live-editable,
  as lite intended. core files are unpacked again when the version
  changes, `user/` is never overwritten.
- `system.spawn(argv, opts)` runs a subprocess with polled, never-blocking
  pipes; lite could only fire-and-forget through `system.exec`. argv is
  executed as given -- no shell, ever. reads follow the `file:read`
  convention (an empty string means nothing buffered right now, nil means
  end of stream), writes queue against a capped buffer, and a process the
  editor lets go of is killed and reaped -- no zombies, no orphans.
  options: `cwd`, `env` (merged over the parent's), `stderr = "stdout"`
  to interleave. there is no blocking wait, by design: poll from a
  `core.add_thread` coroutine, like every other background task.
- `renderer.image.load(filename)` decodes a png or jpeg (sniffed by
  content, not extension) into an immutable image;
  `renderer.draw_image(image, x, y [, w, h] [, color])` draws it,
  scaled nearest-neighbor to `w`x`h` when given (natural size
  otherwise) and tinted by `color` (white is identity, like
  draw_text). immutability is the design: a draw command snapshots the
  image by reference, so the pixels painted are the pixels seen when
  the command was recorded -- the trap that sank lite-xl's canvas
  attempts. load raises on failure, exactly like `renderer.font.load`.
  lite had no image surface at all.
- `system.terminal(cols, rows, opts)` opens a pty running `opts.argv`
  (or the user's own shell) with alacritty's vt engine behind it --
  the emulation is byte-for-byte what alacritty ships, wisp draws the
  grid. the handle follows the process api's polling contract: every
  method returns immediately, `update()` drains the pty from a lua
  coroutine, and a terminal the editor lets go of is killed and
  reaped. TERM is xterm-256color; colors resolve through the app's
  own osc overrides first, then the palette the lua theme sets. every
  mode bit the view's input, mouse and selection work needs is a
  getter on the handle: app cursor, bracketed paste, mouse protocol
  and encoding, alt screen, alternate scroll, per-row wrap.
- `font:set_size(size)` re-scales a loaded font in place and
  `font:get_size()` reads it back -- lite-xl's shape, adopted for the
  same reason: runtime zoom must change every font everywhere, and
  references to font objects are captured all over the lua side. an
  in-place mutation means nothing has to be chased; the render cache
  hashes the size, so every text cell repaints on the next frame.
  sizes clamp to at least 1px -- a zero line height is fatal on the
  draw path, not cosmetic.
- `system.mkdir(path)` creates one directory level, returning `true` or
  `nil, error`. lua's `os` library is iso c only, which has no mkdir --
  the same hole lite-xl patched the same way. directory trees are a lua
  loop, like lite-xl's `common.mkdirp`.
- `system.watch(path)` opens a recursive native fs watcher (inotify on
  linux) returning `watcher, nil` or `nil, error`. `watcher:poll()`
  drains everything since the last poll as `{kind, path}` pairs --
  "create", "modify", "delete", "rename" (both ends when known), or
  "rescan" when events were lost and the consumer should walk the tree
  itself. never blocks; poll from a `core.add_thread` coroutine like
  the process and terminal handles. lite rescans the project on a
  timer; this exists so that rescan can one day be deleted.

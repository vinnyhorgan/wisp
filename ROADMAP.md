# roadmap

updated 2026-08-16, at the close of phase b. phases may rot -- git log
is the truth.

## done

- **phase a -- the lua audit.** all nine tier-3 items and the second
  shelf fixed with regression tests (DEVIATIONS §10). claims that died
  under verification, recorded so they are not re-chased: the gutter
  clip bleed is not a bug (Node:draw clips every view to its own
  rect), fractional y-offset (lite #275) has no effect here (integer
  line height + rounded offsets), lite #13 (non-latin startup dir) was
  already fixed by the core (test pins it), and projectsearch refresh
  was mostly safe all along -- rxi cancelled superseded searches via a
  weak thread key, wisp only closed the gc-timing window.
- **phase b, first half -- subprocesses.** `system.spawn` in
  src/process.rs: poll-based, never blocking, no shell, "" for
  nothing-yet and nil at eof, gc kills and reaps. lite-xl's own
  maintainer retrospective (lite-xl #2087) later proposed exactly this
  convention as the fix for their api -- shipped here first.
- **phase b -- the core api milestone.** the maxi-review's fix plan
  (bug fixes, polish, cwd default, ci, embedded-data single binary)
  and the image surface: pure-rust png/jpeg decode, an immutable
  `Image` snapshotted by `Rc` into the rencache and hashed by content
  (never by pointer), nearest-neighbor scaling whose source mapping is
  absolute so partial redraws are exact -- the two traps that sank
  lite-xl's canvas attempts (#1438, #2198), both impossible here by
  construction. `renderer.image.load` + `renderer.draw_image`, proven
  in-suite by a drawing view before the freeze. the terminal's pty was
  deliberately not landed: its consumer lives beyond v0.2.0, and an
  api that sits unconsumed through the whole plugin pass is exactly
  the speculation the constitution forbids. the core is re-frozen.

## phase c -- v0.1.0, the plugin baseline

- pin the philosophy in CLAUDE.md + readme: artifact-vs-tool, the
  monday morning test, the declared exceptions, "an api added
  speculatively is forever".
- tag and release. release artifact is a true single binary once
  data/ is embedded.

## phase d -- the plugin pass (road to v0.2.0)

the famous plugin pass: core plugins drawn from lite core, rxi's
lite-plugins, lite-xl core and lite-xl-plugins, plus our own. the goal
stands: really nice out of the box, still the easily extensible core
that made lite incredible. porting doctrine from the research pass:
**start from rxi's plugin, cherry-pick lite-xl's fixes** (their newer
versions depend on lite-xl-only apis), and pre-empt the two ugliest
ecosystem hacks with tiny core-lua affordances: `font:set_size` (kills
runtime zoom's font-cache monkey-patch) and per-doc `indent_info`
(kills detectindent's global config swap).

the wave, roughly easiest-first: runtime zoom, detectindent,
auto-close brackets + bracketmatch, trim whitespace on save, indent
guides, selection highlight, more languages, treeview file ops,
project-wide replace, session restore + project memory (treeview
width, last query), imageview, word wrap (lite #26), multi-cursor
last. hard-won lessons per item are in the maxi-review research
(lite-xl issue numbers recorded there): word wrap and multi-cursor
have never survived as plugins anywhere -- both are designed-in-core
features wearing plugin clothes, hence last.

## beyond v0.2.0 -- the living-inside-wisp era

ideas agreed in spirit, not yet scheduled or designed:

- **linters and basic code awareness.** `system.spawn` is the enabler:
  linter plugins spawn real linters, a diagnostics ui (gutter marks,
  underlines, a locations list) renders the results. full lsp remains
  in doubt -- decide after the diagnostics ui exists and has proven
  itself on plain linters. note the tree-sitter constraint: grammars
  compile c, which wisp refuses; lua-pattern syntax + external tools
  is the lane unless pure-rust grammars mature.
- **a real terminal.** not a toy, not rxi's console plugin -- a real
  terminal living in a wisp view. un-parks the old "terminal: parked
  forever" entry. needs core: a pty (spawn is pipes, a terminal needs
  openpty + resize + raw io) and a vt escape-sequence engine. the
  candidate is `alacritty_terminal` -- pure rust, battle-tested, used
  by other editors; wisp draws its grid through the renderer it
  already has (the mono nerd font and rencache's cell hashing are
  practically purpose-built for a terminal grid). decided at phase b's
  close: the pty enters the core the day the terminal is built,
  together with the view that consumes it -- a deliberate reopening,
  same bar as spawn cleared, never a parked api. also quietly solves
  the ai dilemma: a real terminal runs any terminal-based agent, no ai
  integration required in the editor itself.
- **fs events (a dirmonitor).** stronger than it first looks: the
  editor already pays for freshness the expensive way -- the project
  scan thread rescans the whole tree every `project_scan_rate` seconds
  (lite's design), which is the very cost the 2000-file cap exists to
  bound. native fs events would make external changes (a git branch
  switch, a build dropping files) appear instantly and delete the
  standing rescan instead of adding a capability. decide during the
  plugin pass's treeview work, staleness in hand; the pure-rust
  `notify` crate is the candidate. taking it is a deliberate core
  reopening, same bar as spawn cleared.
- **cross-platform.** the stack (winit, softbuffer, swash, vendored
  lua) is already portable; the unix-only parts are small and
  deliberate (byte paths, signals, the future pty). macos is likely
  close already -- it is unix. windows is a real port (conpty, paths,
  process semantics) and waits until someone wants it. low priority,
  not soon.
- **a proper linux citizen.** the single binary unpacks everything --
  config included -- into `$XDG_DATA_HOME/wisp` today. eventually the
  split should follow the platform: config and user plugins in
  `$XDG_CONFIG_HOME/wisp` (an obvious, stable place to drop a lua
  plugin file: installing one = saving it into
  `~/.config/wisp/plugins/`), the unpacked editor in
  `$XDG_DATA_HOME/wisp`, nothing anywhere else.
- **helix mode.** selection-first modal editing, kakoune-lineage.
  objectively cleaner than vim's operator-pending model: what you see
  selected is what the action operates on. sequencing is natural:
  helix's model *requires* multiple selections as a first-class doc
  concept, which is exactly phase d's final boss -- multi-cursor lands
  as core doc surgery, then helix mode is mostly a lua layer: keymap
  modes, a mode indicator in the statusview, block caret in normal
  mode, hint popups reusing the autocomplete/commandview machinery.

## ideas queue

filename-weighted fuzzy open (lite #151) - open nonexistent cli paths
as unsaved docs (lite #56) - `command.add` replaces instead of
asserting (plugin reload) - project-search prompt prefilled with the
selection (doc find already does it) - per-syntax symbol pattern
(fixes css autocomplete, lite #149) - long-line hang test before
deciding lite #64 alongside word wrap - copy/cut whole line on empty
selection (lite pr #209) - draw whitespace, auto-save, hide tabs /
gutter (plugin-shaped lite prs)

someday, deliberately last: a newer lua (5.4). mlua makes the swap
cheap, but 5.2 semantics are load-bearing (interned strings, ephemeron
weak tables, the yieldable exit path) and every plugin will inherit
the choice -- revisit only once the plugin ecosystem has settled.

## said no, on the record

ime - lsp (in doubt, not dead) - ai integration in the editor (the
terminal is the answer for now) - gamma-correct blending - dynamic
hidpi rescale - tree-sitter while grammars mean compiling c - the js
division-vs-regex pattern (lite #248) - lite #275 (no effect under
integer line heights) - full non-utf8 support (replacement chars,
never hang) - os dialogs, forever

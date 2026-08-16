# roadmap

updated 2026-08-16, after the terminal landed. phases may rot -- git
log is the truth.

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
- **the terminal, brought forward.** the beyond-v0.2.0 entry landed
  early, by decision, as the promised deliberate reopening: alacritty's
  vt engine (the emulation, not the app -- its event loop threads
  unused) on a non-blocking pty polled from the main thread,
  `system.terminal` under the spawn conventions (poll-based, packed
  colors, kill-and-reap on gc), and the view as a plugin: the grid
  drawn as style runs, key translation to escape sequences, catppuccin
  palette, scrollback, ctrl+` toggle, auto-close on exit. proven
  end-to-end by boot tests that run a real shell and assert exact
  palette pixels in the framebuffer.
- **the audit and the last additions.** the terminal's rust side was
  audited line by line against alacritty's source; the findings (a
  drop that could hang the editor on a sighup-proof child, eintr
  misread as eof or as a dead pty, a fatal lua error available on the
  draw path) were fixed with regression tests. then the core was
  finished in one pass, every gap the plugin wave would have hit:
  the terminal mode getters the perfection pass needs (mouse protocol
  and encoding, alt screen, alternate scroll, per-row wrap),
  `font:set_size` (runtime zoom, lite-xl's shape), `system.mkdir`
  (the one fs syscall lua's os library lacks), and `system.watch`
  (native fs events on notify, polled like everything else).

## the freeze

declared 2026-08-16, after the audit and the last additions: the core
is feature-complete, and nothing in the planned plugin wave needs a
core change. two named exceptions, decided now so they are never
argued later: the terminal's own surface may still be refined during
its perfection pass (api and consumer land together, same commit),
and a real bug in the core is always a bug. everything else is lua
from here -- the way rxi intended.

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
ecosystem hacks: `font:set_size` (kills runtime zoom's font-cache
monkey-patch) is already in the core, landed at the freeze; per-doc
`indent_info` (kills detectindent's global config swap) is a lua-side
affordance for the pass itself.

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
- **terminal perfection.** the terminal itself is built (see done);
  what remains is the polish pass, deliberately postponed until after
  the core freeze: mouse reporting to apps, in-terminal selection and
  copy, a close command that works from the keyboard, bell, osc-52
  refinement, and heavy hands-on testing -- the experience has to be
  perfect, and that bar is earned interactively, not in ci. the mode
  getters it needs landed at the freeze; the rest is lua. the terminal
  also quietly solves the ai dilemma: it runs any terminal-based
  agent, no ai integration required in the editor itself.
- **adopting fs events.** the api landed at the freeze
  (`system.watch`, notify's inotify backend, polled from a coroutine);
  what remains is the lua work of consuming it: make external changes
  (a git branch switch, a build dropping files) appear instantly and
  delete the standing project rescan -- the very cost the 2000-file
  cap exists to bound. do it during the plugin pass's treeview work,
  staleness in hand.
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
- **a hex editor.** the universal claimant for binary files: where
  imageview claims images, a hexview claims everything else, and §7's
  refusal becomes the last resort instead of the answer. reading is
  pure lua on the existing api (bytes in, a draw_text grid out, the
  mono font is a gift here); editing and saving raw bytes is the real
  design work -- the doc model is line-based, so a hexview likely
  wants its own byte-backed model, not a doc.
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

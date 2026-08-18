# roadmap

updated 2026-08-18, after per-doc indent info and the reference clones.
phases may rot -- git log is the truth.

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

- **lua 5.5.** the "someday, deliberately last" item, brought forward
  by decision before the plugin pass so every plugin inherits the
  final interpreter instead of a migration. mlua's `lua55` (vendored,
  still the only c in the build), a full data/ audit for the 5.3
  integer split and 5.5's const loop variables, strict.lua's
  declarator renamed (`global` is a reserved word now), utf8
  iteration from the stdlib's charpattern. the load-bearing 5.2
  semantics -- yieldable exit path, ephemerons, short-string
  interning -- all survive unchanged; the whole record is
  DEVIATIONS §13, and the suite passed untouched after the port.

- **helix mode.** the beyond-v0.2.0 entry, brought forward by decision
  and landed as pure lua: keymap modes in the core keymap, a block
  cursor drawn over lite's own (lite's caret suppressed, not covered),
  helix's three word classes, and the `:` line plus the space, goto,
  view and match prefixes routed onto wisp's own commands the way zed
  routes them onto its host's. built to what `hx --tutor` teaches;
  multiple selections and everything resting on them carved out rather
  than waited for. **this file's own sequencing was wrong**: it said
  helix required multi-cursor as core doc surgery first, and it did not.
- **the hex editor, and the claim registry under it.** binary files stop
  being refused and start being opened. `core.open_file` consults an
  ordered list of claims and the docview is the fallback (DEVIATIONS
  §20), which is the door §7's refusal left open. the hex view is the
  universal claimant: its own byte-addressed model, chunked so an
  overwrite rewrites one chunk and only a change of size rebuilds, two
  panes over one cursor, undo borrowed from the doc's shape. §7's
  refusal is now the last resort rather than the answer.
- **the image viewer.** the first of the audit's three predicted core
  reopenings -- and it needed nothing: `renderer.image.load` and
  `draw_image` were right as shipped. a specific claim in front of the
  hex view's universal one, a scale and a point, 100% meaning one source
  pixel per *ui* pixel, and the file followed on autoreload's own loop.
  `keymap.modes` came with it: the mode a stroke runs in is now decided
  by a list of deciders rather than assigned to a variable, because the
  second modal layer would have fought the first over one field.

## the freeze

declared 2026-08-16, after the audit and the last additions: the core is
feature-complete and everything from here is lua -- the way rxi intended.
two named exceptions, decided then so they are never argued later: the
terminal's own surface may still be refined during its perfection pass
(api and consumer land together, same commit), and a real bug in the core
is always a bug.

the record since, kept honest rather than tidy. the freeze held for the
*plugin* wave and did not hold for unix citizenship: signals, the xdg
split and argument parsing all landed after it, each individually right
and none of them predicted. the audit that followed found every addition
clustered in the first eight commits after the freeze and nothing since,
and named three plugin-wave items as the first consumers of apis nothing
had used yet -- which is where a gap hides, because an api with no
consumer is an api nobody has tested. one of the three has since landed:
the image viewer, and it needed nothing at all.

the standing agreement, from that audit: when `data/` work turns out to
need rust, **stop and ask first**. surface it, name which exception it
falls under, and let the choice be made at the moment it matters.

one core question is open and deliberately unanswered: an image is
decoded whole at four bytes a pixel and nothing knows how many pixels it
has until it has been decoded, so a png claiming 30000 square is 3.6 gb
before anything can refuse it. the honest place for that limit is a pixel
budget inside `renderer.image.load`; the alternative is a second png
header parser in lua that duplicates the decoder badly and misses jpeg
entirely. written down, not half-solved.

## phase c -- v0.1.0, the plugin baseline (done)

tagged 2026-08-17. the philosophy is pinned in CLAUDE.md and the readme
-- artifact-vs-tool, the monday morning test, the declared exceptions,
"an api added speculatively is forever" -- and v0.1.0 marks the baseline
the plugin pass builds on: lite's editor, faithfully, on a frozen rust
core, with the lua layer audited and its every divergence in
DEVIATIONS.md.

what the tag deliberately does not include: a github release with a
prebuilt binary. the single binary works (data/ is embedded), so that is
a packaging decision, not a code one -- cut it whenever it is wanted.

## phase d -- the plugin pass (road to v0.2.0)

the famous plugin pass: core plugins drawn from lite core, rxi's
lite-plugins, lite-xl core and lite-xl-plugins, plus our own. the goal
stands: really nice out of the box, still the easily extensible core
that made lite incredible. the pass is two halves: an audit of the twenty-one plugins wisp ships
today, and then selection and adaptation from rxi's lite-plugins,
lite-xl's stock `data/plugins`, and lite-xl-plugins. lite-xl's own set is
the richest seam and several items below are sitting in it under another
name -- drawwhitespace, lineguide, linewrapping, workspace, findfile,
language_cpp, language_html.

porting doctrine from the research pass:
**start from rxi's plugin, cherry-pick lite-xl's fixes** (their newer
versions depend on lite-xl-only apis), and pre-empt the two ugliest hacks
in *rxi's* plugin set -- both of which lite-xl already fixed properly in
its own core, so the work is adopting their shape, not inventing one.
`font:set_size` (kills runtime zoom's font-cache monkey-patch) landed at
the freeze. per-doc indent info (kills detectindent's global config swap,
which is wrong the moment two files with different indentation are open)
landed the same way, ahead of its consumer and in lite-xl's exact shape,
so detectindent adapts instead of being rewritten. there is no third
pre-emption of that kind: the obvious candidate was statusview
extensibility (lite-xl has `StatusView:add_item`, wisp has lite's
hardcoded `get_items`), and exactly **one** upstream plugin uses it.

two rules settled before the pass, so they are not re-decided per plugin:

- **`common.lua` grows one function at a time.** wisp's is lite's twelve
  plus `home_expand`; lite-xl added nineteen more, and the adapted
  plugins reach for `merge`, `basename`, `dirname` and `serialize` often
  enough to make a bulk import tempting. unlike indent info these are
  conveniences, not a bug being closed, so the rule the rust core lives
  by applies here too: the function arrives with its first consumer, in
  the same commit, and not before.
- **one project directory, still.** lite-xl grew
  `core.project_directories`; wisp keeps lite's single chdir. five
  upstream plugins reference the plural form, and each adaptation
  translates rather than dragging multi-root in a plugin at a time. see
  "said no, on the record".

**the decided list lives in PLUGINS.md** -- all ~200 plugins across the
two repos judged, the bundle set named in tiers, and every no filed
under the reason it repeats. the headline finding: wisp cannot
syntax-highlight its own source, and seven language files is rxi's 2020
set rather than a 2026 editor.

the wave, roughly easiest-first: ~~runtime zoom~~ (done, DEVIATIONS
§15), ~~imageview~~ (done, §20), ~~per-doc indent info~~ (done, §21),
detectindent, auto-close brackets + bracketmatch, indent guides,
selection highlight, more languages, treeview file ops, project-wide
replace, session restore + project memory (treeview width, last query),
word wrap (lite #26), multi-cursor last.

project-wide replace is wisp's own work, not an adaptation: neither
lite's nor lite-xl's projectsearch has a replace at all. the cheap shape
is to apply the edits to *open docs* -- every touched file becomes a
dirty tab, nothing reaches disk, undo is the doc's own stack and the
review ui is the editor itself. it needs a `doc:save-all` (there is only
save and save-as) and a cap on how many files one replace may open. it
goes **after** the pass, since the pass may replace projectsearch
underneath it.

**treeview file ops is the one to sequence deliberately.** wisp's
treeview has exactly one command, `toggle`; lite-xl's has delete, rename,
new-file and new-folder. that last one is the first thing in the editor
that would ever call `system.mkdir`, which the core grew at the freeze
and nothing has used since -- and an untested api is the audit's own
definition of where a gap hides. it is not that it *will* need a core
change; the image viewer was the same prediction and needed nothing. it
is that if anything does, this is the likeliest place, so it belongs at
the end of the wave where a reopening can be one deliberate session
rather than a trickle. the terminal's perfection pass is the other
predicted consumer, and that one is already a named freeze exception. "trim
whitespace on save" was on this list by mistake: lite's own
trimwhitespace plugin already hooks `Doc.save`, and it ships loaded. hard-won lessons per item are in the maxi-review research
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
- **adopting fs events.** the project scan is done (DEVIATIONS §16):
  the standing rescan is gone, the tree is walked when the watcher says
  something changed, and the timer survives only as a once-a-minute
  safety net. what remains is the rest of the consumers -- autoreload
  still stats each open doc on its own timer (cheap, a handful of
  files, but it could ride the same watch), and the treeview's file
  ops will want the watch in hand when they land.
- **cross-platform.** the stack (winit, softbuffer, swash, vendored
  lua) is already portable; the unix-only parts are small and
  deliberate (byte paths, signals, the future pty). macos is likely
  close already -- it is unix. windows is a real port (conpty, paths,
  process semantics) and waits until someone wants it. low priority,
  not soon.
- **a proper linux citizen.** landed after an audit of what a linux
  user expects and wisp did not do: the window names itself (wayland
  app id, x11 WM_CLASS), sigterm/sigint/sighup are caught and rescue
  unsaved work instead of killing the process where it stands,
  `--help`/`--version` answer on stdout, an unrecognized option is
  refused with exit 2, a path that does not exist opens as a file
  waiting to be written (lite #56, DEVIATIONS §18), there is a man
  page, and the xdg split is real
  (DEVIATIONS §17) -- config and user plugins in
  `$XDG_CONFIG_HOME/wisp`, the unpacked editor in
  `$XDG_DATA_HOME/wisp`, `error.txt` and the temp files in
  `$XDG_STATE_HOME/wisp`, nothing anywhere else. what is left is
  packaging: a `.desktop` file and an icon, which go with the release
  binary whenever that is cut -- until then the app id the window
  reports has nothing to point at.
- **the rest of helix and the hex editor.** both landed (see done), and
  both left the same kind of tail. helix: multiple selections and
  everything on them, treesitter text objects, `gw`'s jump labels. the
  hex editor: a data inspector (the bytes at the cursor as i16, u32,
  f32, ...), save-as, raw-byte copy. features, none of them urgent.

## ideas queue

filename-weighted fuzzy open (lite #151) - `command.add` replaces instead of
asserting (plugin reload) - project-search prompt prefilled with the
selection (doc find already does it) - per-syntax symbol pattern
(fixes css autocomplete, lite #149) - long-line hang test before
deciding lite #64 alongside word wrap - copy/cut whole line on empty
selection (lite pr #209) - draw whitespace, auto-save, hide tabs /
gutter (plugin-shaped lite prs)

the "someday, deliberately last" lua upgrade landed early instead
(see done): better that every plugin inherits the final interpreter
than that the swap waits under a settled ecosystem.

## said no, on the record

ime - lsp (in doubt, not dead) - ai integration in the editor (the
terminal is the answer for now) - x11/wayland PRIMARY selection
(middle-click paste; asked and declined) - screen-reader accessibility
(asked and declined, not forever) - gamma-correct blending - dynamic
hidpi rescale (still no, but the reason has changed: `font:set_size`
and the scale plugin make the lua side of it trivial now, so all that
is missing is a way for lua to hear that the window's scale factor
moved -- a core change, and the core is frozen) - tree-sitter while
grammars mean compiling c - the js
division-vs-regex pattern (lite #248) - lite #275 (no effect under
integer line heights) - full non-utf8 support (replacement chars,
never hang) - os dialogs, forever - multiple project directories
(lite-xl's `core.project_directories`; adaptations translate to the
single chdir, they do not drag multi-root in one plugin at a time) -
lite-xl-widgets (a ui toolkit, and wisp is an artifact, not a toolkit;
the four upstream plugins that need it are not adaptable and are not
meant to be) - pragtical's plugin fork as a standing reference (its
versions of shared plugins diverge by a hundred-plus lines against a
core with `core.root_project`, `core.encoding`, `core.nag_view`; worth
a spot-check for one plugin, not a third variant to carry for all)

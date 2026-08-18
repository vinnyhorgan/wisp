# the bundled plugin list

wisp has no plugin manager, no settings gui and no `config.plugins`
switch: `core.load_plugins` requires every file in `data/plugins`, and
the way to not have one is to delete it. **bundling is enabling.** so
this list is not a catalogue of what wisp *can* run -- it is the editor
itself, and every entry has to survive the monday morning test on its
own.

the universe surveyed: rxi's `lite-plugins` (77 files), lite-xl's stock
`data/plugins` (27) and their `lite-xl-plugins` (183, plus 51
third-party repos named in `manifest.json`). roughly two hundred
distinct plugins. what follows is the decision on all of them.

the porting doctrine is in ROADMAP.md: **rxi's version first, lite-xl's
fixes cherry-picked.** the size table is why. for the same feature,
measured across the two repos:

    bracketmatch        117 -> 278      indentguide      45 -> 155
    drawwhitespace       37 -> 360      lineguide        18 -> 133
    detectindent         64 -> 395      autoinsert      114 -> 121

lite-xl's extra two-to-three-x is mostly multi-cursor support, settings
gui plumbing and config surface -- three things wisp has said no to. so
rxi's version is not merely the starting point, it is usually the better
*fit*, and the cherry-picking runs the other way from what you would
expect.

## already bundled (22)

sixteen are lite's stock set, kept byte-faithful except where
DEVIATIONS.md says otherwise; six are wisp's own. nothing is dropped --
the weakest entry is `quote` (28 lines, holds `ctrl+'`, wraps a
selection as an escaped string literal), and it stays: harmless, rxi's,
and dropping a stock plugin buys nothing.

    lite's:   autocomplete autoreload macro projectsearch quote reflow
              tabularize treeview trimwhitespace + 7 language files
    wisp's:   helix/ hexview/ imageview terminal scale normalize

**done in this pass.** `autocomplete` scanned `while i < #doc.lines` and
so never read the last line of any document -- a symbol that lived only
there was never suggested. rxi's off-by-one, which lite-xl had already
fixed; the first time the porting doctrine paid off on a plugin wisp
already shipped. `normalize` and `trimwhitespace`'s markdown exception
are the house rules, DEVIATIONS §23.

**owed, and untested.** `macro` replaces `core.on_event` globally and
replays by calling it in a loop, bypassing `core.step`'s event drain and
with it the `did_keymap` handling that helix's argument-taking commands
depend on. it has **zero tests**, and both the modal layer and the
terminal now live on that path. tests first, fixes only if they fail.

four more have work owed, and it is part of this pass:

- **treeview** -- file ops (delete, rename, new file, new folder),
  adapted from lite-xl's treeview where they live as core commands.
  `new-folder` is the first-ever consumer of `system.mkdir`, so it lands
  deliberately and last.
- **treeview** -- per-filetype icons. upstream's answer (`nerdicons`,
  `nonicons`, `devicons`) is to ship a font; wisp already *is* a nerd
  font build, so this is an extension of `style.icons`, not a plugin.
  adopt the idea, reject the plugin.
- **autoreload** -- move from its own stat timer onto `system.watch`.
- **projectsearch** -- gains replace, after this pass rather than during
  it (ROADMAP has the design).

## tier 1 -- the editor is incomplete without these

**languages.** the largest gap and the least defensible: wisp cannot
syntax-highlight its own source. eighteen `.rs` files, two `.toml`, a
`.yml` -- none of them highlighted, and `.json` only by falling through
to the javascript syntax. seven language files is rxi's 2020 set, not a
2026 editor. the bundle goes to roughly twenty-six, adding, from rxi
where the file exists and lite-xl otherwise:

    rust toml json sh yaml go cpp diff ini make cmake
    java ts jsx ruby php psql zig nim

taking `language_json` means `language_js` gives up its `%.json$` claim
-- a DEVIATIONS-worthy edit to a stock file, not a drop-in. the same
goes for `language_c`, which claims `%.cpp$` and `%.hpp$` while being a
c syntax; rxi's `language_cpp` needs that claim narrowed first.

**and "lite-xl otherwise" is not safe.** wisp's tokenizer has no regex
at all -- it is lite's, lua patterns only -- while lite-xl added a pcre
module and their newer language files use it. seven of the eighteen
above need it: **rust, json, diff, java, ts, jsx, php**. rxi has
regex-free rust, java, ts and php; json and diff have no rxi version and
get written by hand, which is a morning's work each in lua patterns.

**detectindent** -- the consumer DEVIATIONS §21 was built for, and the
one entry on this list that is a **rewrite rather than an adaptation**.
rxi's 64 lines are the global-config swap §21 exists to make impossible,
so they are obsolete by construction; lite-xl's 395 are a proper
detector wrapped in settings-gui plumbing wisp has no use for. what is
left is the honest middle: count the leading-whitespace runs across the
file, take the mode, write `doc.indent_info`. call it fifty lines.

**autoinsert** (rxi, 114) -- closing brackets and quotes, and wrapping a
selection in them. the single largest "this feels finished" item on the
list. it composes with helix for free: normal mode swallows text input,
so it only ever fires in insert mode.

**bracketmatch** (rxi, 117) -- underlines the match for the bracket at
the caret. already cached on change id plus cursor and already limited
to a hundred lines of scan, which is the whole reason to prefer it to
lite-xl's 278. needs one catppuccin color added by name.

**indentguide** (rxi, 45) -- reads `doc:get_indent_info()` instead of the
config. one thing to watch rather than a bug: on a blank line it walks
outward to the nearest non-blank line, so a file with a long run of
blank lines does that walk per visible line per frame. it is a proper
tail call, so nothing overflows -- it is a draw-path cost, and the draw
path is the one that runs outside `core.try`.

**selectionhighlight** (rxi, 37) -- boxes the other occurrences of the
selection. clean as written; needs a color.

**lineguide** (rxi, 18) -- a rule at `config.line_limit`, which wisp
already has and already defaults to 80. eighteen lines.

## tier 2 -- real work, small surface

**gitstatus** -- branch and insert/delete counts in the status bar, and
the clearest showcase of what the rust core bought. rxi's version
redirects `system.exec` to a temp file and then `coroutine.yield(1)` --
it *hopes* git finished inside one second. wisp has `system.spawn` with
real poll semantics, so the adaptation deletes the race rather than
inheriting it.

**session restore** (rxi's `workspace`, 164) -- structure from rxi,
storage decision from lite-xl, location from wisp. rxi writes
`.lite_workspace.lua` **into the project directory**, which is rude;
this writes to `$XDG_STATE_HOME/wisp` keyed by project path. first
consumer of `common.serialize`, which is exactly how that function is
allowed to arrive.

**drawwhitespace** (rxi, 37) -- adopt the idea, not the loop. rxi draws
one `draw_text` per character with a `get_width` per character, on the
draw path. lite-xl's 360 lines are mostly the fix for that; wisp wants
the fix without the settings surface.

**sort** (rxi, 30) -- joins the reflow / tabularize / quote family of
selection operations lite already shipped.

**linecopypaste** (rxi, 45) -- copy, cut and paste the whole line when
nothing is selected. already sitting in the roadmap's ideas queue as
lite pr #209.

**copyfilelocation** (rxi, 17) -- with its strings lowercased per §9.

## tier 3 -- named so they are not forgotten, not scheduled

- **lfautoinsert** -- closes the block on return (`{`->`}`,
  `then`->`end`). worth less here than upstream, because wisp's
  `doc:newline` already carries the indent forward.
- **restoretabs** -- `ctrl+shift+t`. upstream patches `Node.close_view`
  from inside `RootView.update` behind an initialised flag, because
  `Node` is not exported; wisp should hook the close *command* instead
  and skip the hack entirely.
- **colorpreview** -- a swatch under `#ff00ff`. pays off in css work and
  nowhere else.
- **word wrap** (lite-xl's `linewrapping`, 600) -- material exists, but
  it reaches far enough into docview line metrics to be core-lua work
  wearing plugin clothes. sequenced with multi-cursor, at the end.

## said no, and why

grouped by the reason, because the reasons repeat.

**declared noes** (~35 entries): every `lsp_*` and `ide_*` metapackage,
`evergreen` and `tree_sitter`, `modal` and `lite-xl-vibe` (helix mode is
the answer), `snippets` via lsp.

**needs native code** -- `encoding`, `thread`, `net`, `www`,
`coro_diff`, `tree_sitter`, `widget`. wisp has no plugin c abi and is
not getting one; lua 5.5 vendored is the only c in the build.

**a toolkit, not an artifact** -- `settings`, `colorpicker`,
`search_ui`, `plugin_manager`, `toolbarview`, `contextmenu`. these are
guis over configuration, which is the shape wisp exists to avoid. the
tell is that four of them need `lite-xl-widgets`, a ui toolkit; wisp is
an artifact, not a toolkit.

**one font, one theme** -- `base16`, `theme16`, `themeselect`,
`select_colorscheme`, `wal`, `fontconfig`, `fontpreview`, the `font_*`
libraries, and `nonicons` / `nerdicons` / `devicons` (idea adopted, font
rejected: ours is already a nerd font).

**an option is a cost** -- `hidelinenumbers`, `hidestatus`, `inanimate`,
`linenumbers`, `unboundedscroll`, `extend_selection_line`, `cleanstart`,
`ephemeral_tabs`, `tabnumbers`, `scalestatus`, `custom_caret`,
`smoothcaret`, `motiontrail`. each is a preference with a plugin around
it, and several of the caret ones would fight helix's block cursor.

**status trinkets** -- `memoryusage`, `typingspeed`, `smallclock`,
`statusclock`, `bigclock`, `wordcount`.

**hard rules** -- `gui_filepicker` (os dialogs, forever), `su_save`
(pkexec), `primary_selection` (asked and declined).

**wisp already solved it, better** -- `open_ext` (the hex view claims
binaries, DEVIATIONS §20), `console` / `exterm` / lite-xl's `terminal`
(wisp's terminal has a real pty in the core), `findfile` and
`findfileimproved` (core has find-file), `autosave` (the dirty flag is
honest and quit already asks), `regexreplacepreview` (lua patterns, and
replace is coming to projectsearch), `previewer`.

**impossible by wisp's own model** -- `eofnewline`. `Doc:load` appends
`"\n"` to every line, so a wisp document always ends in exactly one
newline and there is no position past it. the plugin is a no-op here.

**needs multi-cursor** -- `align_carets`.

**toys and platform ports** -- `tetris`, `visu`, `equationgrapher`,
`easingpreview`, `svg_screenshot`, `keyhud`, `opacity`,
`immersive-title`, `discord-presence`, `litepresence`, `macmodkeys`,
`ipc`.

**tool-specific** -- `gofmt`, `black`, `formatter`, `pdfview`,
`texcompile`, `ghmarkdown`, `sortcss`, `kinc-projects`, and the
toolchain downloaders (`golang`, `jdk`, `nodejs`, `haxe`) which are
package management wearing a plugin's clothes.

**big, and not now** -- `minimap` (633), `sticky_scroll` (429),
`spellcheck` (would mean shipping a dictionary), `editorconfig` (a whole
specification), `navigate`, `markers`, `tab_switcher`, `indent_convert`,
`force_syntax`, `exec`, `eval`.

## the recurring costs

three things every adaptation pays, worth knowing before the first one:

1. **a color, by name.** most drawing plugins fall back to
   `style.selection` or `style.syntax.comment`. wisp adds the real one
   to the catppuccin theme, verified against the official palette.json
   -- never eyeballed (CLAUDE.md).
2. **lowercase strings** (DEVIATIONS §9), and a DEVIATIONS entry in the
   same commit as the change.
3. **a regression test** for anything fixed on the way in, failing on
   the old behavior. the upstream plugins are not tested; wisp's are.

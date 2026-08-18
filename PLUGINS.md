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

## already bundled (53)

**the pass is done.** nine of lite's stock plugins, kept byte-faithful
except where DEVIATIONS.md says otherwise; twenty-three language files;
and twenty-one that wisp adds. nothing is dropped -- the weakest entry
is `quote` (28 lines, holds `ctrl+'`, wraps a selection as an escaped
string literal), and it stays: harmless, rxi's, and dropping a stock
plugin buys nothing.

    lite's:      autocomplete autoreload macro projectsearch quote
                 reflow tabularize treeview trimwhitespace
    wisp's own:  helix/ hexview/ imageview terminal scale normalize
    adapted:     autoinsert bracketmatch centerdoc copyfilelocation
                 detectindent drawwhitespace gitstatus indentguide
                 linecopypaste lineguide markers motiontrail
                 selectionhighlight session sort
    languages:   c cmake cpp csharp css diff gitcommit gitignore go
                 html ini java js json lua make md python rust sh toml
                 xml yaml

three more things landed as parts of the editor rather than as plugins,
because that is what they are: the treeview's **file operations** and
its **per-filetype icons** (DEVIATIONS §25), and the **clock** on the
empty view (§27). a fourth, the **todo list**, is one command inside
`projectsearch` instead of a 740-line tree view.

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

## what landed, and what each cost

**languages -- done.** wisp could not syntax-highlight its own source;
seven files became twenty-three, and DEVIATIONS §24 is the record. what
the pass taught: rxi's repo carried more than expected (cpp, csharp, go,
java, sh, make and cmake went in as copies), lite-xl supplied html, toml
and ini, rust was a rewrite, and **five had to be written here** --
json, yaml, gitignore, gitcommit and diff -- because lite-xl's versions
need three tokenizer features wisp does not have (pcre, subsyntaxes,
position captures). the failure mode is silence: an unrunnable rule
produces no token, and an unknown token type renders *white* rather than
erroring, so the test asserts token types for every extension.

**tier 1 -- done.** `detectindent` (a rewrite, not a port: rxi's swaps
the global config around every command and every draw, which is what §21
exists to make unnecessary), `autoinsert`, `bracketmatch`, `indentguide`
(reads `doc:get_indent_info()`), `selectionhighlight` (two characters
minimum, and not just spaces), `lineguide` (a mode, off by default).

**tier 2 -- done.** `gitstatus` (the exec race deleted, not inherited),
`session` (rxi's structure, wisp's storage and crash-safety), the
treeview's file operations and icons, `drawwhitespace` (one rule, no
toggle), `sort`, `linecopypaste` (with the stale-clipboard bug fixed),
`copyfilelocation`.

**asked for and added on top.** `centerdoc` as a toggle, `markers`,
`motiontrail` (rewritten as a fade), the clock, and the todo list.

**tried and taken back out.** the end-of-file mark, twice: a `¬` glyph
and then a dimmed caret. §23 guarantees every file wisp writes ends in
exactly one newline, so the mark is drawn on every file, always -- and a
mark that is always there says nothing. DEVIATIONS §25 keeps the
reasoning; this line keeps it from being re-added.

**the three costs every adaptation paid**, all of them found the hard
way and all of them in DEVIATIONS §25:

1. **the CommandView trap.** `CommandView` extends `DocView`, so every
   plugin that patches a `DocView` draw method also paints inside the
   command prompt. upstream ships all of them that way; only
   `autoinsert` guards itself.
2. **a color, by name**, from the official catppuccin `palette.json` --
   never eyeballed. five new names.
3. **lua 5.5.** `markers` simply failed to load, because it reassigns a
   `for`-loop variable and those have been const since 5.4.

## tier 3 -- named so they are not forgotten, not scheduled

- **lfautoinsert** -- closes the block on return (`{`->`}`,
  `then`->`end`). worth less here than upstream, because wisp's
  `doc:newline` already carries the indent forward.
- **restoretabs** -- `ctrl+shift+t`. upstream patches `Node.close_view`
  from inside `RootView.update` behind an initialised flag, because
  `Node` is not exported, and keys on `doc.abs_filename`, which is a
  lite-xl field wisp does not have; wisp should hook the close *command*
  instead and skip both. **session restore takes most of the sting out
  of this one**, which is why it stayed here.
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

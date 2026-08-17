# wisp

a lightweight text editor in rust, after rxi's [lite](https://github.com/rxi/lite)

<p align="center"><img src=".github/editor.png" width="820"></p>

## what is this

[lite](https://github.com/rxi/lite) is a lovely idea: a tiny core that draws
text and reads input, and a whole editor written on top of it in lua. wisp
keeps the idea and rewrites the core -- c, sdl and stb replaced by ~5k
lines of safe rust. the lua layer is still lite's, byte for byte where possible,
bug-fixed where not. every deliberate difference is written down in
[DEVIATIONS.md](DEVIATIONS.md).

- pure rust: winit, softbuffer, swash. the only c compiled is lua itself
- everything you see is lua: views, syntax, commands, plugins -- all of it
  editable while the editor runs
- one font (jetbrains mono nerd font), one theme (catppuccin mocha, green
  caret), one binary
- a real terminal in a view: alacritty's vt engine on a polled pty, drawn
  through the same renderer as everything else
- `unsafe_code = "deny"`, with a single documented exception, and it's lua's
  fault
- the whole editor boots headless in the test suite: 66 of its 177 tests
  feed it fake input and read the pixels coming back. the screenshots on
  this page were rendered that way too

<p align="center"><img src=".github/palette.png" width="820"></p>

## the rules

wisp is an artifact, not a toolkit: one finished editor, one font, one
theme, one binary. the bar it is built against is the **monday morning
test** -- clone, build, and do a week of real work without installing
anything or wishing you had a different editor.

the core is frozen. an api added speculatively is forever, so it grows
only when a consumer lands alongside it, and everything from here is
lua. lsp and ai integration are deliberately out for now, with the door
left open and the reasons written down in [ROADMAP.md](ROADMAP.md) --
the terminal used to be on that list, and it turned out to be the answer
to the ai question. no os dialogs, ever.

## running

    cargo run --release -- <file or folder>

`ctrl+shift+p` runs commands, `ctrl+p` opens files, `WISP_SCALE=1.4` overrides
the display scale. the rest the editor will tell you itself.

## honesty

some help from the clankers went into this. love and care, however, were
certainly not missing: every line has been read, questioned, and plenty were
sent back.

## thanks

rxi, for lite -- go star it first.

## license

mit, shared with rxi. see [LICENSE](LICENSE).

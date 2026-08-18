# wisp

pure-rust rewrite of rxi's lite. the native core (`src/`) is stable and
changes rarely; the lua layer (`data/`) is being stabilized to the same
standard. the end state: all future work on wisp is basically plugins, the
way rxi intended.

## layout

- `src/` -- the rust core: renderer, rencache, font, lua api, winit
  desktop, headless platform. changes here are rare and deliberate.
- `data/` -- the entire editor, in lua. began as a byte-per-byte copy of
  lite's `data/` at master (`38bd9b3`) and evolves from there.
- `lite/` -- rxi's lite, git-ignored, READ ONLY. the reference for every
  diff. never modify it.
- `lite-xl/`, `lite-plugins/`, `lite-xl-plugins/` -- the rest of the
  reference material, same rule: git-ignored, READ ONLY, never modified.
  lite-xl is the core they grew from lite; the two plugin repos are what
  the plugin pass ports from. the porting doctrine (rxi's version first,
  lite-xl's fixes cherry-picked) lives in ROADMAP.md.
- `DEVIATIONS.md` -- the contract: every intentional difference from
  lite's `data/` is documented here, no exceptions.
- `tests/boot.rs` -- e2e tests: the real, unmodified editor lua booting on
  the headless core with a virtual clock.
- `examples/screenshot.rs` -- renders the readme screenshots with the
  headless editor.

## philosophy

the north star, written down so it is not re-litigated every session.

- **wisp is an artifact, not a toolkit.** one finished editor -- one
  font, one theme, one binary -- not a kit to be assembled. an option is
  a cost; a decision made well and written down beats a setting.
- **the monday morning test.** clone, build, and do a week of real work
  without installing anything or wishing you had a different editor.
  that is the bar, and it is what a release means.
- **an api added speculatively is forever.** the core gets a function
  when its consumer exists, in the same commit, and not before. the core
  is frozen; the two named exceptions are the terminal's own surface
  during its perfection pass, and real bugs.
- **declared exceptions, door left open.** lsp and ai integration are
  deliberately out (the terminal is the answer to ai for now; it used to
  be the third name on this list, and it landed). the full ledger of
  noes lives in ROADMAP.md so they are not re-chased.
- **everything from here is lua**, the way rxi intended.

## hard rules

- no os dialogs in the core, ever. anything that must ask the user goes
  through the editor's own ui (commandview prompts).
- every intentional `data/` change gets a DEVIATIONS.md entry, in the same
  commit as the change.
- every bug fix gets a regression test that fails on the old behavior.
- all user-facing strings are lowercase (DEVIATIONS §9); internal assert
  messages stay as lite wrote them.
- when dvh reports a bug (or an agent claims one), verify it in the code
  first, then fix. evidence before edits.

## workflow

- commits: lowercase, short, imperative, no body, no trailing period.
  commit every self-contained unit of work, then push (standing approval).
- lua is formatted with stylua (`/stylua.toml`, tuned to read like
  rustfmt): `stylua --config-path stylua.toml --check data/`.
- to diff `data/` against lite meaningfully, format a copy of the
  reference the same way first:

      cp -r lite/data /tmp/lite-data
      stylua --config-path stylua.toml /tmp/lite-data
      diff -r /tmp/lite-data data

- pre-push ritual, all three every time: `cargo fmt --check`, the stylua
  check above, and the full suite (`cargo test --release` -- fast).
- screenshots: `cargo run --release --example screenshot`, then
  `magick .github/<name>.ppm .github/<name>.png` and delete the ppm.

## architecture notes (learned the hard way)

- rencache invariant: painted pixels must be a subset of hashed cells.
  glyph ink can escape the metric box (nerd font icons overhang 1-2px),
  so DrawText hashes union(metric box, ink box) via `Font::ink_box_of`
  and carries the pen origin separately. fonts resize in place
  (`font:set_size`, for runtime zoom) and the hash includes the size,
  so a zoom dirties every text cell on the next frame.
- one font: `data/jetbrainsmono.ttf`, the *mono* flavor of jetbrains mono
  nerd font v3.5.0. mono flavor means icon ink == advance, so lua-side
  `get_width` is truthful and layouts need no fudge factors. icons are
  named pua escapes in `style.icons`; sizes 14 ui / 34 big / 16 icons /
  13.5 code (* SCALE).
- theme: catppuccin mocha from the official palette (catppuccin/palette),
  green accent for caret and highlights. colors are verified by name
  against the official palette.json -- never eyeball them.
- lua is 5.5 (mlua `lua55`, vendored, the only c in the build;
  migration record in DEVIATIONS §13). the semantics that matter:
  integers exist (since 5.3), `/` is always float, and
  `string.format("%d")` errors on *any* non-integral float -- floor
  every computed value before formatting or indexing, especially on
  the draw path (draw runs outside core.try, so an error there kills
  the editor). for-loop variables are const (shadow with a local to
  mutate), `global` is a reserved word (strict.lua's declarator is
  `declare`), weak tables are ephemerons, pcall/xpcall stay yieldable
  (the exit path depends on it), and short strings (<= 40 bytes) are
  interned.
- locked views opt into divider dragging by implementing
  `set_target_size(axis, value)`; views opt into sideways scrolling via
  `get_h_scrollable_size()` (default 0). the horizontal clamp runs in
  `clamp_scroll_position` but only while panned sideways: scroll, size
  and content can all move it out of range, and the docview's widest-line
  cache makes the measurement cheap.
- `Doc.change_count` bumps on every content change; views key caches on
  it (e.g. the docview widest-line cache).
- `WISP_SCALE` overrides display scale on desktop; headless boots ignore
  it so tests render identically everywhere.
- images are immutable by design: `renderer.image.load` decodes once
  (png/jpeg, sniffed by content), draw commands snapshot by `Rc`, and
  the rencache hashes the image's content hash, never its pointer.
  `draw_image`'s nearest-neighbor mapping is a pure function of the
  absolute offset inside the dest rect, so clipping skips pixels but
  cannot shift them -- partial redraws of a scaled image are exact
  (the artifact class that sank lite-xl's canvas, their #1438).

## headless testing patterns

- `boot()` in tests/boot.rs gives a 900x600 editor over a stable one-file
  project dir; custom scenarios boot their own dir under
  CARGO_TARGET_TMPDIR (remove_dir_all first: the dir persists across
  runs).
- every boot test starts with `let _serial = serial();` -- the editor
  chdirs the shared test process, so concurrent editors race each
  other's relative paths (project scan, doc opens). one editor at a
  time, no exceptions, or the suite flakes.
- mouse moves are coalesced and dispatched at the end of core.step: move,
  `run_steps`, then press/wheel.
- whole-frame comparisons need `editor.set_focus(false)` -- an unfocused
  editor draws no caret, so frames stop depending on the blink phase.
- hang-prone regressions: run the editor inside its own thread and
  `recv_timeout` the result, so a regression fails loudly instead of
  freezing the suite.
- fs mtimes are real even though the editor clock is virtual:
  `File::set_modified` (+/- an hour) makes autoreload-style tests
  deterministic.
- events: `KeyPressed("left ctrl")` + press("x") for chords, `TextInput`
  for typing (works in prompts too), `MouseWheel(x, y, phase)` on the
  rust side arrives as `(y, x, phase)` in lua -- phase is `Some("moved")`
  etc for trackpad gestures, `None` for discrete wheels. the command palette (`ctrl+shift+p` + type
  + return) can drive any command that has no binding.
- window title is a cheap assertion surface: `"name* - wisp"` shows the
  open doc and its dirty flag.
- `Headless::boot_with_exedir` boots against a copied `data/` tree: the
  way to inject a user module or a modified plugin (e.g. registering a
  test autocomplete provider) without touching the repo's data.
- lua randomizes the string hash seed per state: `pairs()` order
  varies per boot. anything asserted through it must be made
  order-independent (e.g. break fuzzy-score ties).

## roadmap

the roadmap lives in ROADMAP.md (git log is the truth when they
disagree). short version: phase b is done, the terminal landed right
after it, and the core is feature-frozen (ROADMAP "the freeze"): the
audit plus the last additions -- terminal mode getters, font:set_size,
system.mkdir, system.watch -- closed it out, and everything from here
is lua, with two named exceptions (the terminal's own surface during
its perfection pass, and real bugs). phase c tags v0.1.0, phase d is
the plugin pass to v0.2.0, and beyond that: terminal polish (mouse,
selection, heavy testing), linters, helix mode. ROADMAP.md also keeps the ledger of
dead claims and deliberate noes, so they are not re-chased.

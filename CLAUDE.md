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
- `DEVIATIONS.md` -- the contract: every intentional difference from
  lite's `data/` is documented here, no exceptions.
- `tests/boot.rs` -- e2e tests: the real, unmodified editor lua booting on
  the headless core with a virtual clock.
- `examples/screenshot.rs` -- renders the readme screenshots with the
  headless editor.

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
  and carries the pen origin separately.
- one font: `data/jetbrainsmono.ttf`, the *mono* flavor of jetbrains mono
  nerd font v3.5.0. mono flavor means icon ink == advance, so lua-side
  `get_width` is truthful and layouts need no fudge factors. icons are
  named pua escapes in `style.icons`; sizes 14 ui / 34 big / 16 icons /
  13.5 code (* SCALE).
- theme: catppuccin mocha from the official palette (catppuccin/palette),
  green accent for caret and highlights. colors are verified by name
  against the official palette.json -- never eyeball them.
- lua is 5.2 (mlua, vendored, the only c in the build). 5.2 semantics
  matter: all strings are interned, weak tables are ephemerons, and
  `string.format("%d")` errors on inf -- guard any division that can see
  a zero denominator, especially on the draw path (draw runs outside
  core.try, so an error there kills the editor).
- locked views opt into divider dragging by implementing
  `set_target_size(axis, value)`; views opt into sideways scrolling via
  `get_h_scrollable_size()` (default 0). the horizontal clamp lives in
  `View:on_mouse_wheel`, not per-frame update: measuring content width
  can scan a whole document.
- `Doc.change_count` bumps on every content change; views key caches on
  it (e.g. the docview widest-line cache).
- `WISP_SCALE` overrides display scale on desktop; headless boots ignore
  it so tests render identically everywhere.

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
  for typing (works in prompts too), `MouseWheel(x, y)` on the rust side
  arrives as `(y, x)` in lua. the command palette (`ctrl+shift+p` + type
  + return) can drive any command that has no binding.
- window title is a cheap assertion surface: `"name* - wisp"` shows the
  open doc and its dirty flag.

## roadmap (agreed 2026-08-15, may rot -- git log is the truth)

- **phase a -- tier 3 of the lua audit** (confirmed, being fixed):
  locked-node asserts reach the user (open a file via ctrl+p right after
  clicking a treeview folder; also core:open-log and project search),
  caret-follow x-snap discards h-scroll + long-line drag selection
  gallops (one family; prior art: franko's unmerged lite PR #230 and
  lite-xl's docview), projectsearch invalid-pattern wedge / mid-search
  refresh / binary scanning, ctrl+l at eof inserts a real newline
  (append_line_if_last_line) + move-lines-down blank stacking, treeview
  stale hover after wheel scroll / no scrollbar / no h-scroll,
  autocomplete dedup compares the wrong index and cross-contaminates
  info, shift+f3 before any find errors, logview infinite scroll, status
  bar column is bytes not chars (#300). second shelf: close-confirm nil
  item, get_line_screen_position col (#313), fractional y-offset (#275),
  gutter clip bleed, language fixes (#224/#171/#248),
  doc-commands-during-prompt (#13), ~ expansion in path prompts.
- **phase b -- core api milestone** (last core surgery, then re-freeze):
  subprocess (`system.spawn(argv)`, poll-based, no shell; study lite-xl's
  Process api) and images (pure-rust decode, draw-image rencache command
  honoring the ink invariant; §7 binary refusal becomes "refused unless a
  view claims it"). parked and staying parked: ime, lsp, ai, terminal,
  gamma-correct blending, dynamic hidpi rescale.
- **phase c -- v0.1.0, the plugin baseline**: pin the philosophy
  (artifact-vs-tool, the monday morning test, the three declared
  exceptions, "an api added speculatively is forever") in CLAUDE.md +
  readme, fix the readme's hardcoded test count, tag and release.
- **phase d -- essentials wave, road to v0.2.0** ("the monday morning
  release"), each a plugin: runtime zoom, detectindent, auto-close
  brackets + bracketmatch, trim whitespace on save, indent guides,
  selection highlight, more languages, treeview file ops, project-wide
  replace, session restore + project memory (treeview width, last
  query), imageview, word wrap (#26), multi-cursor last. ideas queue:
  filename-weighted fuzzy open (#151), open nonexistent cli paths as
  unsaved docs (#56).

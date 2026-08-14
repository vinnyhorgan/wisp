# Deviations from lite's `data/`

`data/` is a byte-per-byte copy of rxi's lite `data/` directory at master
(`38bd9b3`, v1.11 plus rxi's last fixes), except for
the intentional changes listed here. Every entry must say what changed and why.
The reference copy lives untouched in `/lite/` (git-ignored, read-only).

## 1. Quit confirmation uses the CommandView, not an OS dialog

**File:** `data/core/init.lua`, `core.quit()`

lite called `system.show_confirm_dialog()` — the only use of that API in the
entire codebase — to ask about unsaved changes on quit, putting an OS message
box on top of the editor. wisp's core does not expose an OS dialog API at all.
The quit confirmation is routed through `core.command_view:enter()` with
yes/no suggestions instead, so the editor confirms with its own UI, in its own
theme. Type `y`/`yes` to quit, anything else (or escape) to cancel.

local config = {}

config.project_scan_rate = 5
config.max_project_files = 2000
config.fps = 60
config.max_log_items = 80
config.message_timeout = 3
-- pixels per wheel notch. lite and lite-xl both scroll a flat 50, which
-- is 2.4 lines at our metrics (21px of line, one font, measured not
-- guessed) -- slow enough that people reach for the scrollbar. 84 is
-- four lines, which is where the rest of the desktop sits
config.mouse_wheel_scroll = 84 * SCALE
-- a wheel notch is a quantized command -- "go down a few lines" -- while
-- a trackpad glide is direct manipulation measured in finger pixels, and
-- one gain cannot serve both: lite only ever saw the wheel, so it only
-- had the one number. the core hands a glide over as finger pixels over
-- 40, so this lands the content at 3.5x the finger. raise it if your
-- trackpad still feels like hard work
config.trackpad_scroll_gain = 1.75
config.file_size_limit = 10
config.ignore_files = "^%."
config.symbol_pattern = "[%a_][%w_]*"
config.non_word_chars = " \t\n/\\()\"':,.;<>~!@#$%^&*|+=[]{}`?-"
config.undo_merge_timeout = 0.3
config.max_undos = 10000
config.highlight_current_line = true
config.line_height = 1.2
config.indent_size = 2
config.tab_type = "soft"
config.line_limit = 80

return config

local config = {}

config.project_scan_rate = 5
config.max_project_files = 2000
config.fps = 60
config.max_log_items = 80
config.message_timeout = 3
config.mouse_wheel_scroll = 50 * SCALE
-- a wheel notch is a quantized command -- "go down a few lines" -- while
-- a trackpad glide is direct manipulation, and one gain cannot serve
-- both: lite only ever saw the wheel, so it only had the one number. a
-- glide's delta is multiplied by this before it becomes a scroll
config.trackpad_scroll_gain = 3
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

local common = require("core.common")
local style = {}

style.padding = { x = common.round(14 * SCALE), y = common.round(7 * SCALE) }
style.divider_size = common.round(1 * SCALE)
style.scrollbar_size = common.round(4 * SCALE)
style.caret_width = common.round(2 * SCALE)
style.tab_width = common.round(170 * SCALE)

style.font = renderer.font.load(EXEDIR .. "/data/jetbrainsmono.ttf", 14 * SCALE)
style.big_font = renderer.font.load(EXEDIR .. "/data/jetbrainsmono.ttf", 34 * SCALE)
style.icon_font = renderer.font.load(EXEDIR .. "/data/jetbrainsmono.ttf", 16 * SCALE)
style.code_font = renderer.font.load(EXEDIR .. "/data/jetbrainsmono.ttf", 13.5 * SCALE)
style.clock_font = renderer.font.load(EXEDIR .. "/data/jetbrainsmono.ttf", 72 * SCALE)

-- nerd font icons, utf-8 encoded private use area codepoints
style.icons = {
    file = "\xEF\x80\x96", -- U+F016 file outline
    dir = "\xEF\x81\xBB", -- U+F07B folder
    dir_open = "\xEF\x81\xBC", -- U+F07C folder open
    collapsed = "\xEF\x81\x94", -- U+F054 chevron right
    expanded = "\xEF\x81\xB8", -- U+F078 chevron down
    gear = "\xEF\x80\x93", -- U+F013 cog
    info = "\xEF\x81\x9A", -- U+F05A info circle
    warn = "\xEF\x81\xB1", -- U+F071 warning triangle
    branch = "\xEF\x90\x98", -- U+F418 git branch (octicons)
}

-- per-filetype icons for the tree. upstream ships a *font* for this --
-- nerdicons, nonicons, devicons -- and wisp already is a nerd font
-- build, so the idea is adopted and the plugin rejected: this is one
-- more table of named codepoints, not a dependency. keys are matched
-- against the filename, longest pattern first, and anything unmatched
-- keeps `icons.file`
style.file_icons = {
    { "%.lua$", "\xEE\x98\xA0" }, -- U+E620
    { "%.rs$", "\xEE\x9E\xA8" }, -- U+E7A8
    { "%.py[wi]?$", "\xEE\x9C\xBC" }, -- U+E73C
    { "%.[mc]?js$", "\xEE\x9E\x81" }, -- U+E781
    { "%.tsx?$", "\xEE\x98\xA8" }, -- U+E628
    { "%.jsonc?$", "\xEE\x98\x8B" }, -- U+E60B
    { "%.toml$", "\xEE\x9A\xB2" }, -- U+E6B2
    { "%.ya?ml$", "\xEE\x9A\xA8" }, -- U+E6A8
    { "%.mark?down$", "\xEE\x9C\xBE" }, -- U+E73E
    { "%.md$", "\xEE\x9C\xBE" }, -- U+E73E
    { "%.html?$", "\xEE\x9C\xB6" }, -- U+E736
    { "%.css$", "\xEE\x9D\x89" }, -- U+E749
    { "%.[ch]$", "\xEE\x98\x9E" }, -- U+E61E
    { "%.[ch]pp$", "\xEE\x98\x9D" }, -- U+E61D
    { "%.cc$", "\xEE\x98\x9D" }, -- U+E61D
    { "%.go$", "\xEE\x98\xA7" }, -- U+E627
    { "%.java$", "\xEE\x9C\xB8" }, -- U+E738
    { "%.cs$", "\xEE\x9C\xB8" }, -- U+E738
    { "%.[bz]?a?sh$", "\xEE\x9E\x95" }, -- U+E795
    { "%.xml$", "\xEE\x98\x99" }, -- U+E619
    { "%.svg$", "\xEF\x87\x85" }, -- U+F1C5
    { "%.png$", "\xEF\x87\x85" }, -- U+F1C5
    { "%.jpe?g$", "\xEF\x87\x85" }, -- U+F1C5
    { "%.ttf$", "\xEF\x87\x85" }, -- U+F1C5
    { "%.zip$", "\xEF\x87\x86" }, -- U+F1C6
    { "%.tar$", "\xEF\x87\x86" }, -- U+F1C6
    { "%.gz$", "\xEF\x87\x86" }, -- U+F1C6
    { "%.txt$", "\xEF\x85\x9C" }, -- U+F15C
    { "%.lock$", "\xEF\x80\xA3" }, -- U+F023
    { "%.git", "\xEE\x9C\x82" }, -- U+E702
    { "LICENSE", "\xEF\x80\xAD" }, -- U+F02D
    { "[Mm]akefile$", "\xEE\x98\x95" }, -- U+E615
    { "%.editorconfig$", "\xEE\x98\x95" }, -- U+E615
    { "%.ini$", "\xEE\x98\x95" }, -- U+E615
    { "%.cfg$", "\xEE\x98\x95" }, -- U+E615
}

-- the icon for a filename, or nil to use the plain file icon
function style.icon_for(filename)
    for _, rule in ipairs(style.file_icons) do
        if filename:find(rule[1]) then
            return rule[2]
        end
    end
end

-- catppuccin mocha (official palette), green as the accent
style.background = { common.color("#1e1e2e") } -- base
style.background2 = { common.color("#181825") } -- mantle
style.background3 = { common.color("#181825") } -- mantle
style.text = { common.color("#a6adc8") } -- subtext0
style.caret = { common.color("#a6e3a1") } -- green
style.accent = { common.color("#a6e3a1") } -- green
style.dim = { common.color("#6c7086") } -- overlay0
style.divider = { common.color("#11111b") } -- crust
style.selection = { common.color("rgba(147, 153, 178, 0.25)") } -- overlay2, per the style guide
style.line_number = { common.color("#7f849c") } -- overlay1
style.line_number2 = { common.color("#b4befe") } -- lavender
style.line_highlight = { common.color("#313244") } -- surface0
style.scrollbar = { common.color("#45475a") } -- surface1
style.scrollbar2 = { common.color("#585b70") } -- surface2

-- the plugins that draw: each name arrives with the plugin that reads
-- it, and each is a palette color chosen by name, never eyeballed
style.guide = { common.color("#45475a") } -- surface1 (indent + line limit)
style.whitespace = { common.color("#585b70") } -- surface2 (the marks in a selection)
style.selectionhighlight = { common.color("#7f849c") } -- overlay1
style.bracketmatch = { common.color("#89dceb") } -- sky, like the operators it underlines
style.marker = { common.color("#f9e2af") } -- yellow

style.syntax = {}
style.syntax["normal"] = { common.color("#cdd6f4") } -- text
style.syntax["symbol"] = { common.color("#cdd6f4") } -- text
style.syntax["comment"] = { common.color("#9399b2") } -- overlay2
style.syntax["keyword"] = { common.color("#cba6f7") } -- mauve
style.syntax["keyword2"] = { common.color("#f9e2af") } -- yellow (types)
style.syntax["number"] = { common.color("#fab387") } -- peach
style.syntax["literal"] = { common.color("#fab387") } -- peach
style.syntax["string"] = { common.color("#a6e3a1") } -- green
style.syntax["operator"] = { common.color("#89dceb") } -- sky
style.syntax["function"] = { common.color("#89b4fa") } -- blue

return style

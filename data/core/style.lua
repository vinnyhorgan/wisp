local common = require "core.common"
local style = {}

style.padding = { x = common.round(14 * SCALE), y = common.round(7 * SCALE) }
style.divider_size = common.round(1 * SCALE)
style.scrollbar_size = common.round(4 * SCALE)
style.caret_width = common.round(2 * SCALE)
style.tab_width = common.round(170 * SCALE)

style.font = renderer.font.load(EXEDIR .. "/data/fonts/jetbrainsmono.ttf", 14 * SCALE)
style.big_font = renderer.font.load(EXEDIR .. "/data/fonts/jetbrainsmono.ttf", 34 * SCALE)
style.icon_font = renderer.font.load(EXEDIR .. "/data/fonts/jetbrainsmono.ttf", 16 * SCALE)
style.code_font = renderer.font.load(EXEDIR .. "/data/fonts/jetbrainsmono.ttf", 13.5 * SCALE)

-- nerd font icons, utf-8 encoded private use area codepoints
style.icons = {
  file      = "\xEF\x80\x96",  -- U+F016 file outline
  dir       = "\xEF\x81\xBB",  -- U+F07B folder
  dir_open  = "\xEF\x81\xBC",  -- U+F07C folder open
  collapsed = "\xEF\x81\x94",  -- U+F054 chevron right
  expanded  = "\xEF\x81\xB8",  -- U+F078 chevron down
  gear      = "\xEF\x80\x93",  -- U+F013 cog
  info      = "\xEF\x81\x9A",  -- U+F05A info circle
  warn      = "\xEF\x81\xB1",  -- U+F071 warning triangle
}

-- catppuccin mocha (official palette), green as the accent
style.background = { common.color "#1e1e2e" }   -- base
style.background2 = { common.color "#181825" }  -- mantle
style.background3 = { common.color "#181825" }  -- mantle
style.text = { common.color "#a6adc8" }         -- subtext0
style.caret = { common.color "#a6e3a1" }        -- green
style.accent = { common.color "#a6e3a1" }       -- green
style.dim = { common.color "#6c7086" }          -- overlay0
style.divider = { common.color "#11111b" }      -- crust
style.selection = { common.color "rgba(147, 153, 178, 0.25)" }  -- overlay2, per the style guide
style.line_number = { common.color "#7f849c" }  -- overlay1
style.line_number2 = { common.color "#b4befe" } -- lavender
style.line_highlight = { common.color "#313244" }  -- surface0
style.scrollbar = { common.color "#45475a" }    -- surface1
style.scrollbar2 = { common.color "#585b70" }   -- surface2

style.syntax = {}
style.syntax["normal"] = { common.color "#cdd6f4" }    -- text
style.syntax["symbol"] = { common.color "#cdd6f4" }    -- text
style.syntax["comment"] = { common.color "#9399b2" }   -- overlay2
style.syntax["keyword"] = { common.color "#cba6f7" }   -- mauve
style.syntax["keyword2"] = { common.color "#f9e2af" }  -- yellow (types)
style.syntax["number"] = { common.color "#fab387" }    -- peach
style.syntax["literal"] = { common.color "#fab387" }   -- peach
style.syntax["string"] = { common.color "#a6e3a1" }    -- green
style.syntax["operator"] = { common.color "#89dceb" }  -- sky
style.syntax["function"] = { common.color "#89b4fa" }  -- blue

return style

local common = require "core.common"
local style = {}

style.padding = { x = common.round(14 * SCALE), y = common.round(7 * SCALE) }
style.divider_size = common.round(1 * SCALE)
style.scrollbar_size = common.round(4 * SCALE)
style.caret_width = common.round(2 * SCALE)
style.tab_width = common.round(170 * SCALE)

style.font = renderer.font.load(EXEDIR .. "/data/fonts/jetbrainsmono.ttf", 14 * SCALE)
style.big_font = renderer.font.load(EXEDIR .. "/data/fonts/jetbrainsmono.ttf", 34 * SCALE)
style.icon_font = renderer.font.load(EXEDIR .. "/data/fonts/jetbrainsmono.ttf", 14 * SCALE)
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

-- everforest dark (medium), sunset in the pines
style.background = { common.color "#2D353B" }
style.background2 = { common.color "#232A2E" }
style.background3 = { common.color "#232A2E" }
style.text = { common.color "#9DA9A0" }
style.caret = { common.color "#A7C080" }
style.accent = { common.color "#D3C6AA" }
style.dim = { common.color "#56635F" }
style.divider = { common.color "#1E2326" }
style.selection = { common.color "#425047" }
style.line_number = { common.color "#56635F" }
style.line_number2 = { common.color "#859289" }
style.line_highlight = { common.color "#343F44" }
style.scrollbar = { common.color "#475258" }
style.scrollbar2 = { common.color "#56635F" }

style.syntax = {}
style.syntax["normal"] = { common.color "#D3C6AA" }
style.syntax["symbol"] = { common.color "#D3C6AA" }
style.syntax["comment"] = { common.color "#7A8478" }
style.syntax["keyword"] = { common.color "#E67E80" }
style.syntax["keyword2"] = { common.color "#E69875" }
style.syntax["number"] = { common.color "#D699B6" }
style.syntax["literal"] = { common.color "#D699B6" }
style.syntax["string"] = { common.color "#A7C080" }
style.syntax["operator"] = { common.color "#E69875" }
style.syntax["function"] = { common.color "#83C092" }

return style

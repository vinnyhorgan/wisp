local core = require("core")
local config = require("core.config")
local command = require("core.command")
local keymap = require("core.keymap")
local DocView = require("core.docview")

-- centers the text column in the view, at the same width the line-limit
-- rule is drawn at -- so centerdoc and lineguide agree without either
-- being told about the other.
--
-- it is a mode you enter, not a setting you carry: DEVIATIONS §19 made
-- that call for keymap modes and it holds here. the command toggles,
-- the editor redraws, and nothing has to be configured
local centered = false

local draw_line_gutter = DocView.draw_line_gutter
local get_gutter_width = DocView.get_gutter_width

function DocView:draw_line_gutter(idx, x, y)
    local offset = self:get_gutter_width() - get_gutter_width(self)
    draw_line_gutter(self, idx, x + offset, y)
end

function DocView:get_gutter_width()
    local real = get_gutter_width(self)
    -- CommandView extends DocView, and a centered prompt is a prompt
    -- with its text somewhere in the middle of the window
    if not centered or getmetatable(self) ~= DocView then
        return real
    end
    local width = real + self:get_font():get_width("n") * config.line_limit
    return math.max((self.size.x - width) / 2, real)
end

command.add(nil, {
    ["center-doc:toggle"] = function()
        centered = not centered
        core.redraw = true
    end,
})

keymap.add({ ["ctrl+alt+c"] = "center-doc:toggle" })

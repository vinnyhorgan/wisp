local core = require("core")
local style = require("core.style")
local config = require("core.config")
local command = require("core.command")
local keymap = require("core.keymap")
local DocView = require("core.docview")

-- a rule at config.line_limit, which wisp already ships defaulting to
-- 80 -- the same column centerdoc centers on, so the two agree without
-- being told to.
--
-- it is **off until you ask for it**. eighty columns is a house style,
-- not a fact about the file in front of you, and a permanent line down
-- the middle of the screen is a ruler held against work that may not be
-- measured that way. when you are wrapping a paragraph of prose or
-- lining up a comment block you want it, and then you want it gone.
-- so it is a mode you enter, like centerdoc: the command toggles, and
-- nothing is configured (§19).
local shown = false

local draw = DocView.draw

function DocView:draw(...)
    draw(self, ...)
    -- CommandView extends DocView, and a rule down the middle of the
    -- prompt is nonsense: every drawing plugin here checks this
    if not shown or getmetatable(self) ~= DocView then
        return
    end

    local offset = self:get_font():get_width("n") * config.line_limit
    local x = self:get_line_screen_position(1) + offset
    local y = self.position.y
    local w = math.ceil(SCALE * 1)
    local h = self.size.y

    renderer.draw_rect(x, y, w, h, style.guide)
end

command.add(nil, {
    ["line-guide:toggle"] = function()
        shown = not shown
        core.redraw = true
    end,
})

keymap.add({ ["ctrl+alt+g"] = "line-guide:toggle" })

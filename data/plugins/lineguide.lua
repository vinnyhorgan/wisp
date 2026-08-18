local style = require("core.style")
local config = require("core.config")
local DocView = require("core.docview")

-- a rule at config.line_limit, which wisp already ships defaulting to
-- 80 -- the same column centerdoc centers on, so the two agree without
-- being told to
local draw = DocView.draw

function DocView:draw(...)
    draw(self, ...)
    -- CommandView extends DocView, and a rule down the middle of the
    -- prompt is nonsense: every drawing plugin here checks this
    if getmetatable(self) ~= DocView then
        return
    end

    local offset = self:get_font():get_width("n") * config.line_limit
    local x = self:get_line_screen_position(1) + offset
    local y = self.position.y
    local w = math.ceil(SCALE * 1)
    local h = self.size.y

    renderer.draw_rect(x, y, w, h, style.guide)
end

local core = require("core")
local config = require("core.config")
local style = require("core.style")
local DocView = require("core.docview")

config.motiontrail_steps = 50

local function lerp(a, b, t)
    return a + (b - a) * t
end

-- helix's normal mode draws a block on the character under the head
-- instead of lite's thin caret, and a block cursor leaving a hairline
-- trail looks like a bug. the trail asks what shape the caret is rather
-- than assuming, by reading the module rather than requiring it: load
-- order between two bundled plugins is directory order, so a require at
-- the top would be a coin flip
local function caret_is_a_block()
    local helix = package.loaded["plugins.helix"]
    return helix ~= nil and helix.active() and helix.mode ~= "insert"
end

local function get_caret_rect(dv)
    local line, col = dv.doc:get_selection()
    local x, y = dv:get_line_screen_position(line)
    local offset = dv:get_col_x_offset(line, col)
    local w = style.caret_width
    if caret_is_a_block() then
        w = math.max(w, dv:get_col_x_offset(line, col + 1) - offset)
    end
    return x + offset, y, w, dv:get_line_height()
end

local last_x, last_y, last_view

local draw = DocView.draw

function DocView:draw(...)
    draw(self, ...)
    if self ~= core.active_view or getmetatable(self) ~= DocView then
        return
    end

    local x, y, w, h = get_caret_rect(self)

    if last_view == self and (x ~= last_x or y ~= last_y) then
        local lx = x
        for i = 0, 1, 1 / config.motiontrail_steps do
            local ix = lerp(x, last_x, i)
            local iy = lerp(y, last_y, i)
            local iw = math.max(w, math.ceil(math.abs(ix - lx)))
            renderer.draw_rect(ix, iy, iw, h, style.caret)
            lx = ix
        end
        core.redraw = true
    end

    last_view, last_x, last_y = self, x, y
end

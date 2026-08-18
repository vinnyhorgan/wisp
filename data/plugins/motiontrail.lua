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

-- the trail is one frame of motion blur along the line the caret is
-- already on, and nothing else.
--
-- upstream lerps both axes, so a jump between lines drags the smear
-- diagonally across the text -- and because each step is a rect as wide
-- as its own horizontal travel, the result is a staircase of blocks
-- over whatever was on the lines in between. it is the worst-looking
-- thing in the editor and it fires on every arrow press.
--
-- the effect only ever read as motion when the motion was sideways
-- along a line of text, so that is all it does now. vertical movement
-- snaps, which is what every editor that ships a smooth caret does with
-- a line change. the position is remembered as a document position as
-- well as a screen one: a caret that stayed put while the view scrolled
-- under it has not moved, and used to leave a trail saying it had
local last

local draw = DocView.draw

function DocView:draw(...)
    draw(self, ...)
    if self ~= core.active_view or getmetatable(self) ~= DocView then
        return
    end

    local line, col = self.doc:get_selection()
    local x, y, w, h = get_caret_rect(self)

    if last and last.view == self and last.line == line and last.col ~= col and last.y == y then
        local lx = x
        for i = 0, 1, 1 / config.motiontrail_steps do
            local ix = lerp(x, last.x, i)
            local iw = math.max(w, math.ceil(math.abs(ix - lx)))
            renderer.draw_rect(ix, y, iw, h, style.caret)
            lx = ix
        end
        core.redraw = true
    end

    last = { view = self, line = line, col = col, x = x, y = y }
end

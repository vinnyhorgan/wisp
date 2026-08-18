local core = require("core")
local config = require("core.config")
local style = require("core.style")
local DocView = require("core.docview")

config.motiontrail_steps = 50

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

-- the trail goes wherever the caret went, sideways or across lines --
-- the shape was never the problem, the paint was.
--
-- upstream draws every step of the interpolation in the solid caret
-- color, on top of itself: fifty opaque rects a full line tall,
-- overlapping five deep, laid along the path. a jump between lines
-- lands that as a staircase of green blocks over the text it crossed,
-- and no amount of interpolation fixes it, because the pixels are
-- simply painted over.
--
-- so the trail is drawn as what it is meant to look like: an
-- afterimage. one ghost per caret-length of travel along whichever axis
-- moved most, laid edge to edge rather than on top of each other -- so
-- every pixel of the path is painted exactly once, at the alpha it has
-- earned. full under the caret, nothing at the far end. each ghost is
-- also widened to cover its own sideways travel, so a diagonal is a
-- ribbon and not a dotted line.
--
-- the caret's position is remembered as a document position as well as
-- a screen one: a caret that held still while the view scrolled under
-- it has not moved, and used to leave a trail saying it had
local last
local ghost = { 0, 0, 0, 0 }

local draw = DocView.draw

function DocView:draw(...)
    draw(self, ...)
    if self ~= core.active_view or getmetatable(self) ~= DocView then
        return
    end

    local line, col = self.doc:get_selection()
    local x, y, w, h = get_caret_rect(self)

    if last and last.view == self and (last.line ~= line or last.col ~= col) then
        local dx, dy = last.x - x, last.y - y
        local vertical = math.abs(dy) > math.abs(dx)
        -- the ribbon is cut into bands a quarter of the caret's own
        -- length along the axis that moved most -- fine enough that the
        -- gradient reads as a gradient rather than a row of tiles --
        -- and capped, so a jump across the whole window is still one
        -- loop of fifty
        local length = vertical and math.abs(dy) or math.abs(dx)
        local n =
            math.min(math.ceil(length / ((vertical and h or w) / 4)), config.motiontrail_steps)
        local caret = style.caret
        ghost[1], ghost[2], ghost[3] = caret[1], caret[2], caret[3]
        local alpha = caret[4] or 255

        -- the samples are floored before they are measured against each
        -- other, so each ghost begins exactly where the last one ended:
        -- no gaps to show as dark seams, no overlaps to show as bright
        -- ones. an overlap is what banded the trail when the ghosts were
        -- drawn as caret rects that each covered their own travel
        local fx, fy = math.floor(x), math.floor(y)
        for i = 1, n do
            local t = i / n
            local nx, ny = math.floor(x + dx * t), math.floor(y + dy * t)
            ghost[4] = alpha * (1 - (i - 0.5) / n)
            if vertical then
                local rw = math.max(w, math.abs(nx - fx))
                renderer.draw_rect(math.min(nx, fx), math.min(ny, fy), rw, math.abs(ny - fy), ghost)
            else
                renderer.draw_rect(math.min(nx, fx), fy, math.abs(nx - fx), h, ghost)
            end
            fx, fy = nx, ny
        end
        core.redraw = true
    end

    last = { view = self, line = line, col = col, x = x, y = y }
end

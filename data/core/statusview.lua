local core = require("core")
local common = require("core.common")
local command = require("core.command")
local config = require("core.config")
local style = require("core.style")
local DocView = require("core.docview")
local LogView = require("core.logview")
local View = require("core.view")

local StatusView = View:extend()

-- lite sized these for a proportional ui font with ~3.5px spaces; our
-- mono spaces are 8px wide, so fewer of them keep the same visual rhythm
StatusView.separator = "   "
StatusView.separator2 = " | "

function StatusView:new()
    StatusView.super.new(self)
    self.message_timeout = 0
    self.message = {}
    self.visible = true
    self.init_size = true
end

-- the bar's natural height. the message row sits exactly this far below
-- the item row and scrolls up to replace it, so it must not follow
-- `size.y` while a toggle animates that to zero: both the row's offset
-- and the box it is centred in would collapse, sliding a stale message
-- up into view on the way down
function StatusView:get_row_height()
    return style.font:get_height() + style.padding.y * 2
end

function StatusView:on_mouse_pressed()
    core.set_active_view(core.last_active_view)
    if system.get_time() < self.message_timeout and not core.active_view:is(LogView) then
        command.perform("core:open-log")
    end
end

function StatusView:show_message(icon, icon_color, text)
    self.message = {
        icon_color,
        style.icon_font,
        icon,
        style.dim,
        style.font,
        StatusView.separator2,
        style.text,
        text,
    }
    self.message_timeout = system.get_time() + config.message_timeout
end

function StatusView:update()
    -- hiding animates the height to zero the way the treeview animates
    -- its width; the locked node reads this size, and get_locked_size
    -- already drops the divider once a side collapses
    local dest = 0
    if self.visible then
        dest = self:get_row_height()
    end
    if self.init_size then
        self.size.y = dest
        self.init_size = false
    else
        self:move_towards(self.size, "y", dest)
    end

    if system.get_time() < self.message_timeout then
        self.scroll.to.y = self:get_row_height()
    else
        self.scroll.to.y = 0
    end

    StatusView.super.update(self)
end

local function draw_items(self, items, x, y, draw_fn)
    local font = style.font
    local color = style.text

    for _, item in ipairs(items) do
        if type(item) == "userdata" then
            font = item
        elseif type(item) == "table" then
            color = item
        else
            x = draw_fn(font, color, item, nil, x, y, 0, self.size.y)
        end
    end

    return x
end

local function text_width(font, _, text, _, x)
    return x + font:get_width(text)
end

function StatusView:draw_items(items, right_align, yoffset)
    local x, y = self:get_content_offset()
    y = y + (yoffset or 0)
    if right_align then
        local w = draw_items(self, items, 0, 0, text_width)
        x = x + self.size.x - w - style.padding.x
        draw_items(self, items, x, y, common.draw_text)
    else
        x = x + style.padding.x
        draw_items(self, items, x, y, common.draw_text)
    end
end

function StatusView:get_items()
    if getmetatable(core.active_view) == DocView then
        local dv = core.active_view
        local line, col = dv.doc:get_selection()
        local dirty = dv.doc:is_dirty()
        -- col is a byte offset into the line; count characters so
        -- multibyte text does not inflate the number (lite issue #300)
        local _, chars = dv.doc.lines[line]:sub(1, col - 1):gsub("[^\128-\191]", "")
        col = chars + 1

        return {
            dirty and style.accent or style.text,
            style.icon_font,
            style.icons.file,
            style.dim,
            style.font,
            self.separator2,
            style.text,
            dv.doc.filename and style.text or style.dim,
            dv.doc:get_name(),
            style.text,
            self.separator,
            "line: ",
            line,
            self.separator,
            col > config.line_limit and style.accent or style.text,
            "col: ",
            col,
            style.text,
            self.separator,
            -- floored: since 5.3, %d refuses any non-integral float
            string.format("%d%%", math.floor(line / #dv.doc.lines * 100)),
        }, {
            style.icon_font,
            style.icons.gear,
            style.font,
            style.dim,
            self.separator2,
            style.text,
            #dv.doc.lines,
            " lines",
            self.separator,
            dv.doc.crlf and "crlf" or "lf",
        }
    end

    return {}, {
        style.icon_font,
        style.icons.gear,
        style.font,
        style.dim,
        self.separator2,
        #core.docs,
        style.text,
        " / ",
        #core.project_files,
        " files",
    }
end

function StatusView:draw()
    self:draw_background(style.background2)

    if self.message then
        self:draw_items(self.message, false, self:get_row_height())
    end

    -- the two groups are placed independently -- one from each edge --
    -- so a narrow window ran them straight through each other. the left
    -- group gets the room the right one leaves, less a gap, and fades
    -- out into it rather than stopping dead against the icon
    local left, right = self:get_items()
    local rw = draw_items(self, right, 0, 0, text_width)
    local avail = math.max(0, self.size.x - rw - style.padding.x * 2)
    core.push_clip_rect(self.position.x, self.position.y, avail, self.size.y)
    self:draw_items(left)
    core.pop_clip_rect()

    if draw_items(self, left, 0, 0, text_width) + style.padding.x > avail then
        local _, y = self:get_content_offset()
        local bg = style.background2
        local fade = math.min(avail, style.padding.x * 2)
        for i = 1, fade do
            local a = math.floor(255 * i / fade)
            renderer.draw_rect(self.position.x + avail - fade + i - 1, y, 1, self.size.y, {
                bg[1],
                bg[2],
                bg[3],
                a,
            })
        end
    end

    self:draw_items(right, true)
end

return StatusView

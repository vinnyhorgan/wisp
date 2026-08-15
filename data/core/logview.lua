local core = require("core")
local style = require("core.style")
local View = require("core.view")

local LogView = View:extend()

function LogView:new()
    LogView.super.new(self)
    self.last_item = core.log_items[#core.log_items]
    self.scrollable = true
    self.yoffset = 0
end

function LogView:get_name()
    return "log"
end

-- long messages pan sideways through the §8 protocol; the widest drawn
-- row is measured the way draw lays rows out (log items are capped at
-- config.max_log_items, so measuring on each wheel event is cheap)
function LogView:get_h_scrollable_size()
    local w = 0
    for _, item in ipairs(core.log_items) do
        local x = style.padding.x * 2 + style.font:get_width(os.date(nil, item.time))
        local last_w = 0
        for line in item.text:gmatch("[^\n]+") do
            last_w = style.font:get_width(line)
            w = math.max(w, x + last_w)
        end
        w = math.max(w, x + last_w + style.font:get_width(" at " .. item.at))
        if item.info then
            for line in item.info:gmatch("[^\n]+") do
                w = math.max(w, x + style.font:get_width(line))
            end
        end
    end
    return w > 0 and w + style.padding.x or 0
end

-- the base view reports an unbounded scrollable size, which let the log
-- scroll into the void forever; measure the real height of the items so
-- scrolling clamps to them, mirroring how draw lays them out
function LogView:get_scrollable_size()
    local th = style.font:get_height()
    local h = style.padding.y * 2
    for _, item in ipairs(core.log_items) do
        local lines = 0
        for _ in item.text:gmatch("[^\n]+") do
            lines = lines + 1
        end
        if item.info then
            for _ in item.info:gmatch("[^\n]+") do
                lines = lines + 1
            end
        end
        h = h + lines * th + style.padding.y
    end
    return h
end

function LogView:update()
    local item = core.log_items[#core.log_items]
    if self.last_item ~= item then
        self.last_item = item
        self.scroll.to.y = 0
        self.yoffset = -(style.font:get_height() + style.padding.y)
    end

    self:move_towards("yoffset", 0)

    LogView.super.update(self)
end

local function draw_text_multiline(font, text, x, y, color)
    local th = font:get_height()
    local resx, resy = x, y
    for line in text:gmatch("[^\n]+") do
        resy = y
        resx = renderer.draw_text(style.font, line, x, y, color)
        y = y + th
    end
    return resx, resy
end

function LogView:draw()
    self:draw_background(style.background)

    local ox, oy = self:get_content_offset()
    local th = style.font:get_height()
    local y = oy + style.padding.y + self.yoffset

    for i = #core.log_items, 1, -1 do
        local x = ox + style.padding.x
        local item = core.log_items[i]
        local time = os.date(nil, item.time)
        x = renderer.draw_text(style.font, time, x, y, style.dim)
        x = x + style.padding.x
        local subx = x
        x, y = draw_text_multiline(style.font, item.text, x, y, style.text)
        renderer.draw_text(style.font, " at " .. item.at, x, y, style.dim)
        y = y + th
        if item.info then
            subx, y = draw_text_multiline(style.font, item.info, subx, y, style.dim)
            y = y + th
        end
        y = y + style.padding.y
    end
end

return LogView

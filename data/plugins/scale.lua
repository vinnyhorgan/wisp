local core = require("core")
local common = require("core.common")
local command = require("core.command")
local config = require("core.config")
local keymap = require("core.keymap")
local style = require("core.style")

-- one decision instead of a setting: zooming scales the whole editor,
-- the way a browser does. lite-xl offers "code" and "ui" modes; with a
-- single font at four sizes there is nothing to gain from leaving the
-- chrome behind at the old size next to text at the new one
local MIN_ZOOM, MAX_ZOOM, ZOOM_STEP = 0.5, 4, 0.1

-- zoom is a multiple of the scale the editor booted at (the display's,
-- or WISP_SCALE), so 1 is always "normal" and every step is the same
-- fraction of it on any display
local zoom = 1

-- every value below is recomputed from what it was at boot, never from
-- what it currently is. lite-xl multiplies the live numbers by a ratio
-- each step, so integer rounding compounds and a reset lands near --
-- but not on -- where it started; measured from a base, every step is
-- exact and a reset is an identity
local base = {
    scale = SCALE,
    padding_x = style.padding.x,
    padding_y = style.padding.y,
    divider_size = style.divider_size,
    scrollbar_size = style.scrollbar_size,
    caret_width = style.caret_width,
    tab_width = style.tab_width,
    mouse_wheel_scroll = config.mouse_wheel_scroll,
}

local fonts = { "font", "big_font", "icon_font", "code_font" }
local base_size = {}
for _, name in ipairs(fonts) do
    base_size[name] = style[name]:get_size()
end

local scale = {}

function scale.get()
    return zoom
end

function scale.set(value)
    value = common.clamp(value, MIN_ZOOM, MAX_ZOOM)
    if value == zoom then
        return
    end

    -- everything the views measure in is about to change under them, so
    -- each scroll offset is saved as a fraction of its scrollable range
    -- and put back after; otherwise a zoom drops the reader somewhere
    -- else in the file
    local views = core.root_view.root_node:get_children()
    local vscroll, hscroll = {}, {}
    for _, view in ipairs(views) do
        local n = view:get_scrollable_size()
        if n ~= math.huge and n > view.size.y then
            vscroll[view] = view.scroll.y / (n - view.size.y)
        end
        local hn = view:get_h_scrollable_size()
        if hn ~= math.huge and hn > view.size.x then
            hscroll[view] = view.scroll.x / (hn - view.size.x)
        end
    end

    zoom = value
    SCALE = base.scale * value

    style.padding.x = common.round(base.padding_x * value)
    style.padding.y = common.round(base.padding_y * value)
    style.tab_width = common.round(base.tab_width * value)
    -- a hairline that rounds down to zero stops being drawn at all, so
    -- the one-pixel details keep a floor
    style.divider_size = math.max(1, common.round(base.divider_size * value))
    style.scrollbar_size = math.max(1, common.round(base.scrollbar_size * value))
    style.caret_width = math.max(1, common.round(base.caret_width * value))
    config.mouse_wheel_scroll = base.mouse_wheel_scroll * value

    for _, name in ipairs(fonts) do
        style[name]:set_size(base_size[name] * value)
    end

    for view, n in pairs(vscroll) do
        view.scroll.y = n * (view:get_scrollable_size() - view.size.y)
        view.scroll.to.y = view.scroll.y
    end
    for view, n in pairs(hscroll) do
        view.scroll.x = n * (view:get_h_scrollable_size() - view.size.x)
        view.scroll.to.x = view.scroll.x
    end

    core.redraw = true
end

command.add(nil, {
    ["scale:increase"] = function()
        scale.set(zoom + ZOOM_STEP)
    end,
    ["scale:decrease"] = function()
        scale.set(zoom - ZOOM_STEP)
    end,
    ["scale:reset"] = function()
        scale.set(1)
    end,
})

keymap.add({
    ["ctrl+="] = "scale:increase",
    -- shift+= is "+" on most layouts, and the plus is what people reach
    -- for; both spellings zoom in
    ["ctrl+shift++"] = "scale:increase",
    ["ctrl+-"] = "scale:decrease",
    ["ctrl+0"] = "scale:reset",
    ["ctrl+wheelup"] = "scale:increase",
    ["ctrl+wheeldown"] = "scale:decrease",
})

return scale

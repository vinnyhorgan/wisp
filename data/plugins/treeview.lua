local core = require("core")
local common = require("core.common")
local command = require("core.command")
local config = require("core.config")
local keymap = require("core.keymap")
local style = require("core.style")
local View = require("core.view")

config.treeview_size = 200 * SCALE

-- the width is the one piece of layout the user sets by hand, so it is
-- kept at the scale the editor booted at and multiplied up on the way
-- out. zooming then scales a dragged width exactly, and returning to
-- 100% returns it to the pixel it was dragged to
local boot_scale = SCALE

local function to_screen(size)
    return size * SCALE / boot_scale
end

local function get_depth(filename)
    local n = 0
    for sep in filename:gmatch("[\\/]") do
        n = n + 1
    end
    return n
end

local TreeView = View:extend()

function TreeView:new()
    TreeView.super.new(self)
    self.scrollable = true
    self.visible = true
    self.init_size = true
    self.cache = {}
    self.target_size = config.treeview_size
end

-- opts into divider dragging: the rootview calls this when the divider
-- of a locked split is dragged
function TreeView:set_target_size(axis, value)
    if axis == "x" then
        self.target_size = math.max(value, 80 * SCALE) * boot_scale / SCALE
        return true
    end
end

function TreeView:get_cached(item)
    local t = self.cache[item.filename]
    if not t then
        t = {}
        t.filename = item.filename
        t.abs_filename = system.absolute_path(item.filename)
        t.name = t.filename:match("[^\\/]+$")
        t.depth = get_depth(t.filename)
        t.type = item.type
        self.cache[t.filename] = t
    end
    return t
end

function TreeView:get_name()
    return "project"
end

function TreeView:get_item_height()
    return style.font:get_height() + style.padding.y
end

-- the view base class reports math.huge (it cannot know its content),
-- which let the treeview scroll into the void forever; report the real
-- height of the visible items so scrolling clamps to them. the clamp
-- asks on every update and the scrollbar on every draw and every mouse
-- move, so the walk -- up to config.max_project_files entries -- is
-- counted once and kept until the project or a folder changes
function TreeView:get_scrollable_size()
    self:check_cache()
    if not self.item_count then
        local count = 0
        for _ in self:each_item() do
            count = count + 1
        end
        self.item_count = count
    end
    return style.padding.y * 2 + self.item_count * self:get_item_height()
end

-- long filenames pan sideways like any other view (the clamp lives in
-- the wheel handler); the widest row is measured the way draw lays
-- rows out
function TreeView:get_h_scrollable_size()
    local w = 0
    local icon_width = style.icon_font:get_width(style.icons.dir)
    local spacing = style.font:get_width(" ")
    for item in self:each_item() do
        local x = (item.depth + 2) * style.padding.x + icon_width + spacing
        w = math.max(w, x + style.font:get_width(item.name))
    end
    return w > 0 and w + style.padding.x or 0
end

function TreeView:check_cache()
    -- invalidate cache's skip values if project_files has changed
    if core.project_files ~= self.last_project_files then
        for _, v in pairs(self.cache) do
            v.skip = nil
        end
        self.last_project_files = core.project_files
        self.item_count = nil
    end
end

function TreeView:each_item()
    return coroutine.wrap(function()
        self:check_cache()
        local ox, oy = self:get_content_offset()
        local y = oy + style.padding.y
        local w = self.size.x
        local h = self:get_item_height()

        local i = 1
        while i <= #core.project_files do
            local item = core.project_files[i]
            local cached = self:get_cached(item)

            coroutine.yield(cached, ox, y, w, h)
            y = y + h
            i = i + 1

            if not cached.expanded then
                if cached.skip then
                    i = cached.skip
                else
                    local depth = cached.depth
                    while i <= #core.project_files do
                        local filename = core.project_files[i].filename
                        if get_depth(filename) <= depth then
                            break
                        end
                        i = i + 1
                    end
                    cached.skip = i
                end
            end
        end
    end)
end

function TreeView:update_hovered()
    self.hovered_item = nil
    if not self.mouse_x then
        return
    end
    -- rows span the view horizontally no matter how far the content is
    -- panned sideways, so only y comes from the item
    local px, py = self.mouse_x, self.mouse_y
    if px <= self.position.x or px > self.position.x + self.size.x then
        return
    end
    for item, _, y, _, h in self:each_item() do
        if py > y and py <= y + h then
            self.hovered_item = item
            break
        end
    end
end

function TreeView:on_mouse_moved(px, py, ...)
    TreeView.super.on_mouse_moved(self, px, py, ...)
    self.mouse_x, self.mouse_y = px, py
    self:update_hovered()
end

function TreeView:on_mouse_pressed(button, x, y, clicks)
    local caught = TreeView.super.on_mouse_pressed(self, button, x, y, clicks)
    if caught then
        return true
    end
    if not self.hovered_item then
        return
    elseif self.hovered_item.type == "dir" then
        self.hovered_item.expanded = not self.hovered_item.expanded
        self.item_count = nil
    else
        core.try(function()
            core.root_view:open_doc(core.open_doc(self.hovered_item.filename))
        end)
    end
end

function TreeView:update()
    -- update width; cap it below the window width so the divider always
    -- stays on screen and can be grabbed again
    local dest = 0
    if self.visible then
        dest = common.clamp(
            to_screen(self.target_size),
            80 * SCALE,
            core.root_view.size.x - 80 * SCALE
        )
    end
    if self.init_size then
        self.size.x = dest
        self.init_size = false
    else
        self:move_towards(self.size, "x", dest)
    end

    -- wheel scrolling slides the rows under a stationary pointer; the
    -- hovered item must follow the rows, not the last mouse event
    if self.scroll.y ~= self.last_scroll_y then
        self.last_scroll_y = self.scroll.y
        self:update_hovered()
    end

    TreeView.super.update(self)
end

function TreeView:draw()
    self:draw_background(style.background2)

    local icon_width = style.icon_font:get_width(style.icons.dir)
    local spacing = style.font:get_width(" ")

    local doc = core.active_view.doc
    local active_filename = doc and system.absolute_path(doc.filename or "")

    for item, x, y, w, h in self:each_item() do
        local x = x -- loop vars are const since lua 5.5
        local color = style.text

        -- highlight active_view doc
        if item.abs_filename == active_filename then
            color = style.accent
        end

        -- hovered item background: always the full view width, however
        -- far the content is panned sideways
        if item == self.hovered_item then
            renderer.draw_rect(self.position.x, y, self.size.x, h, style.line_highlight)
            color = style.accent
        end

        -- icons
        x = x + item.depth * style.padding.x + style.padding.x
        if item.type == "dir" then
            local icon1 = item.expanded and style.icons.expanded or style.icons.collapsed
            local icon2 = item.expanded and style.icons.dir_open or style.icons.dir
            common.draw_text(style.icon_font, color, icon1, nil, x, y, 0, h)
            x = x + style.padding.x
            common.draw_text(style.icon_font, color, icon2, nil, x, y, 0, h)
            x = x + icon_width
        else
            x = x + style.padding.x
            common.draw_text(style.icon_font, color, style.icons.file, nil, x, y, 0, h)
            x = x + icon_width
        end

        -- text
        x = x + spacing
        x = common.draw_text(style.font, color, item.name, nil, x, y, 0, h)
    end

    self:draw_scrollbar()
end

-- init
local view = TreeView()
local node = core.root_view:get_active_node()
node:split("left", view, true)

-- register commands and keymap
command.add(nil, {
    ["treeview:toggle"] = function()
        view.visible = not view.visible
    end,
})

keymap.add({ ["ctrl+\\"] = "treeview:toggle" })

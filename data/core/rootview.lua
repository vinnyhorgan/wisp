local core = require("core")
local common = require("core.common")
local style = require("core.style")
local keymap = require("core.keymap")
local Object = require("core.object")
local View = require("core.view")
local DocView = require("core.docview")

local EmptyView = View:extend()

-- with nothing open, the space is otherwise dead, so it is a clock, and
-- a clock is all it is: the time, the date under it, nothing else. the
-- wordmark and the two key bindings were there first and they made the
-- screen look like a splash page -- an editor showing you its own name
-- is telling you the one thing you already know.
--
-- the clock is skipped in headless boots. it is the only thing the
-- editor draws that is not a function of its own state, and every
-- whole-frame test in the suite stands on two boots drawing the same
-- pixels -- so the one surface that reads the wall clock is the one
-- surface a test can never see. the wordmark is what stands in for it
-- there, which is also what the readme's screenshot wants
local function clock_text()
    if HEADLESS then
        return nil
    end
    return os.date("%H:%M:%S"), os.date("%A, %d %B"):lower()
end

function EmptyView:update()
    EmptyView.super.update(self)
    local time = clock_text()
    if time ~= self.time then
        self.time = time
        core.redraw = true
    end
end

local function draw_wordmark(x, y, color)
    local th = style.big_font:get_height()
    local dh = th + style.padding.y * 2
    x = renderer.draw_text(style.big_font, "wisp", x, y + (dh - th) / 2, color)
    x = x + style.padding.x
    renderer.draw_rect(x, y, math.ceil(1 * SCALE), dh, color)
    local lines = {
        { fmt = "%s to run a command", cmd = "core:find-command" },
        { fmt = "%s to open a file from the project", cmd = "core:find-file" },
    }
    th = style.font:get_height()
    y = y + (dh - th * 2 - style.padding.y) / 2
    local w = 0
    for _, line in ipairs(lines) do
        local text = string.format(line.fmt, keymap.get_binding(line.cmd))
        w = math.max(w, renderer.draw_text(style.font, text, x + style.padding.x, y, color))
        y = y + th + style.padding.y
    end
    return w, dh
end

function EmptyView:draw()
    self:draw_background(style.background)

    local time, date = clock_text()
    if not time then
        -- the wordmark block is measured by drawing it transparent,
        -- which is lite's own trick and is kept
        local w, h = draw_wordmark(0, 0, { 0, 0, 0, 0 })
        local x = self.position.x + math.max(style.padding.x, (self.size.x - w) / 2)
        draw_wordmark(x, self.position.y + (self.size.y - h) / 2, style.dim)
        return
    end

    local function center(font, text, ty, color)
        local tw = font:get_width(text)
        renderer.draw_text(font, text, self.position.x + (self.size.x - tw) / 2, ty, color)
    end

    local ch = style.clock_font:get_height()
    local dh = style.font:get_height()
    local y = self.position.y + (self.size.y - ch - style.padding.y - dh) / 2
    center(style.clock_font, time, y, style.text)
    center(style.font, date, y + ch + style.padding.y, style.dim)
end

local Node = Object:extend()

function Node:new(type)
    self.type = type or "leaf"
    self.position = { x = 0, y = 0 }
    self.size = { x = 0, y = 0 }
    self.views = {}
    self.divider = 0.5
    if self.type == "leaf" then
        self:add_view(EmptyView())
    end
end

function Node:propagate(fn, ...)
    self.a[fn](self.a, ...)
    self.b[fn](self.b, ...)
end

function Node:on_mouse_moved(x, y, ...)
    self.hovered_tab = self:get_tab_overlapping_point(x, y)
    if self.type == "leaf" then
        self.active_view:on_mouse_moved(x, y, ...)
    else
        self:propagate("on_mouse_moved", x, y, ...)
    end
end

function Node:on_mouse_released(...)
    if self.type == "leaf" then
        self.active_view:on_mouse_released(...)
    else
        self:propagate("on_mouse_released", ...)
    end
end

function Node:consume(node)
    for k, _ in pairs(self) do
        self[k] = nil
    end
    for k, v in pairs(node) do
        self[k] = v
    end
end

local type_map = { up = "vsplit", down = "vsplit", left = "hsplit", right = "hsplit" }

function Node:split(dir, view, locked)
    assert(self.type == "leaf", "Tried to split non-leaf node")
    local type = assert(type_map[dir], "Invalid direction")
    local last_active = core.active_view
    local child = Node()
    child:consume(self)
    self:consume(Node(type))
    self.a = child
    self.b = Node()
    if view then
        self.b:add_view(view)
    end
    if locked then
        self.b.locked = locked
        core.set_active_view(last_active)
    end
    if dir == "up" or dir == "left" then
        self.a, self.b = self.b, self.a
    end
    return child
end

function Node:close_active_view(root)
    local do_close = function()
        if #self.views > 1 then
            local idx = self:get_view_idx(self.active_view)
            table.remove(self.views, idx)
            self:set_active_view(self.views[idx] or self.views[#self.views])
        else
            local parent = self:get_parent_node(root)
            local is_a = (parent.a == self)
            local other = parent[is_a and "b" or "a"]
            if other:get_locked_size() then
                self.views = {}
                self:add_view(EmptyView())
            else
                parent:consume(other)
                local p = parent
                while p.type ~= "leaf" do
                    p = p[is_a and "a" or "b"]
                end
                p:set_active_view(p.active_view)
            end
        end
        core.last_active_view = nil
    end
    self.active_view:try_close(do_close)
end

function Node:add_view(view)
    assert(self.type == "leaf", "Tried to add view to non-leaf node")
    assert(not self.locked, "Tried to add view to locked node")
    if self.views[1] and self.views[1]:is(EmptyView) then
        table.remove(self.views)
    end
    table.insert(self.views, view)
    self:set_active_view(view)
end

function Node:set_active_view(view)
    assert(self.type == "leaf", "Tried to set active view on non-leaf node")
    self.active_view = view
    core.set_active_view(view)
end

function Node:get_view_idx(view)
    for i, v in ipairs(self.views) do
        if v == view then
            return i
        end
    end
end

function Node:get_node_for_view(view)
    for _, v in ipairs(self.views) do
        if v == view then
            return self
        end
    end
    if self.type ~= "leaf" then
        return self.a:get_node_for_view(view) or self.b:get_node_for_view(view)
    end
end

function Node:get_parent_node(root)
    if root.a == self or root.b == self then
        return root
    elseif root.type ~= "leaf" then
        return self:get_parent_node(root.a) or self:get_parent_node(root.b)
    end
end

function Node:get_children(t)
    t = t or {}
    for _, view in ipairs(self.views) do
        table.insert(t, view)
    end
    if self.a then
        self.a:get_children(t)
    end
    if self.b then
        self.b:get_children(t)
    end
    return t
end

-- a divider is draggable if no side is pinned by a locked view, or if
-- every locked view along it opts in by implementing set_target_size
function Node:is_resizable()
    if self.type == "leaf" then
        return not self.locked or self.active_view.set_target_size ~= nil
    end
    return self.a:is_resizable() and self.b:is_resizable()
end

function Node:get_divider_overlapping_point(px, py)
    if self.type ~= "leaf" then
        local p = 6
        local x, y, w, h = self:get_divider_rect()
        x, y = x - p, y - p
        w, h = w + p * 2, h + p * 2
        if px > x and py > y and px < x + w and py < y + h and self:is_resizable() then
            return self
        end
        return self.a:get_divider_overlapping_point(px, py)
            or self.b:get_divider_overlapping_point(px, py)
    end
end

function Node:get_tab_overlapping_point(px, py)
    if #self.views == 1 then
        return nil
    end
    local x, y, w, h = self:get_tab_rect(1)
    if px >= x and py >= y and px < x + w * #self.views and py < y + h then
        return math.floor((px - x) / w) + 1
    end
end

function Node:get_child_overlapping_point(x, y)
    local child
    if self.type == "leaf" then
        return self
    elseif self.type == "hsplit" then
        child = (x < self.b.position.x) and self.a or self.b
    elseif self.type == "vsplit" then
        child = (y < self.b.position.y) and self.a or self.b
    end
    return child:get_child_overlapping_point(x, y)
end

function Node:get_tab_rect(idx)
    local tw = math.min(style.tab_width, math.ceil(self.size.x / #self.views))
    local h = style.font:get_height() + style.padding.y * 2
    return self.position.x + (idx - 1) * tw, self.position.y, tw, h
end

function Node:get_divider_rect()
    local x, y = self.position.x, self.position.y
    if self.type == "hsplit" then
        return x + self.a.size.x, y, style.divider_size, self.size.y
    elseif self.type == "vsplit" then
        return x, y + self.a.size.y, self.size.x, style.divider_size
    end
end

function Node:get_locked_size()
    if self.type == "leaf" then
        if self.locked then
            local size = self.active_view.size
            return size.x, size.y
        end
    else
        local x1, y1 = self.a:get_locked_size()
        local x2, y2 = self.b:get_locked_size()
        if x1 and x2 then
            local dsx = (x1 < 1 or x2 < 1) and 0 or style.divider_size
            local dsy = (y1 < 1 or y2 < 1) and 0 or style.divider_size
            return x1 + x2 + dsx, y1 + y2 + dsy
        end
    end
end

local function copy_position_and_size(dst, src)
    dst.position.x, dst.position.y = src.position.x, src.position.y
    dst.size.x, dst.size.y = src.size.x, src.size.y
end

-- calculating the sizes is the same for hsplits and vsplits, except the x/y
-- axis are swapped; this function lets us use the same code for both
local function calc_split_sizes(self, x, y, x1, x2)
    local n
    local ds = (x1 and x1 < 1 or x2 and x2 < 1) and 0 or style.divider_size
    if x1 then
        n = x1 + ds
    elseif x2 then
        n = self.size[x] - x2
    else
        n = math.floor(self.size[x] * self.divider)
    end
    self.a.position[x] = self.position[x]
    self.a.position[y] = self.position[y]
    self.a.size[x] = n - ds
    self.a.size[y] = self.size[y]
    self.b.position[x] = self.position[x] + n
    self.b.position[y] = self.position[y]
    self.b.size[x] = self.size[x] - n
    self.b.size[y] = self.size[y]
end

function Node:update_layout()
    if self.type == "leaf" then
        local av = self.active_view
        if #self.views > 1 then
            local _, _, _, th = self:get_tab_rect(1)
            av.position.x, av.position.y = self.position.x, self.position.y + th
            av.size.x, av.size.y = self.size.x, self.size.y - th
        else
            copy_position_and_size(av, self)
        end
    else
        local x1, y1 = self.a:get_locked_size()
        local x2, y2 = self.b:get_locked_size()
        if self.type == "hsplit" then
            calc_split_sizes(self, "x", "y", x1, x2)
        elseif self.type == "vsplit" then
            calc_split_sizes(self, "y", "x", y1, y2)
        end
        self.a:update_layout()
        self.b:update_layout()
    end
end

function Node:update()
    if self.type == "leaf" then
        for _, view in ipairs(self.views) do
            view:update()
        end
    else
        self.a:update()
        self.b:update()
    end
end

function Node:draw_tabs()
    local x, y, _, h = self:get_tab_rect(1)
    local ds = style.divider_size
    core.push_clip_rect(x, y, self.size.x, h)
    renderer.draw_rect(x, y, self.size.x, h, style.background2)
    renderer.draw_rect(x, y + h - ds, self.size.x, ds, style.divider)

    for i, view in ipairs(self.views) do
        local x, y, w, h = self:get_tab_rect(i)
        local text = view:get_name()
        local color = style.dim
        if view == self.active_view then
            color = style.text
            renderer.draw_rect(x, y, w, h, style.background)
            renderer.draw_rect(x + w, y, ds, h, style.divider)
            renderer.draw_rect(x - ds, y, ds, h, style.divider)
        end
        if i == self.hovered_tab then
            color = style.text
        end
        core.push_clip_rect(x, y, w, h)
        x, w = x + style.padding.x, w - style.padding.x * 2
        local align = style.font:get_width(text) > w and "left" or "center"
        common.draw_text(style.font, color, text, align, x, y, w, h)
        core.pop_clip_rect()
    end

    core.pop_clip_rect()
end

function Node:draw()
    if self.type == "leaf" then
        if #self.views > 1 then
            self:draw_tabs()
        end
        local pos, size = self.active_view.position, self.active_view.size
        core.push_clip_rect(pos.x, pos.y, size.x + pos.x % 1, size.y + pos.y % 1)
        self.active_view:draw()
        core.pop_clip_rect()
    else
        local x, y, w, h = self:get_divider_rect()
        renderer.draw_rect(x, y, w, h, style.divider)
        self:propagate("draw")
    end
end

local RootView = View:extend()

function RootView:new()
    RootView.super.new(self)
    self.root_node = Node()
    self.deferred_draws = {}
    self.mouse = { x = 0, y = 0 }
end

function RootView:defer_draw(fn, ...)
    table.insert(self.deferred_draws, 1, { fn = fn, ... })
end

function RootView:get_active_node()
    return self.root_node:get_node_for_view(core.active_view)
end

local function get_unlocked_leaf(node)
    if node.type == "leaf" then
        return not node.locked and node or nil
    end
    return get_unlocked_leaf(node.a) or get_unlocked_leaf(node.b)
end

-- the active node, but never a locked one: a view opened while the
-- treeview or a prompt holds focus must land in the editing area, not
-- assert (click a treeview folder, then ctrl+p: the active view and the
-- last active view are both locked)
function RootView:get_active_node_default()
    local node = self:get_active_node()
    if node.locked and core.last_active_view then
        node = self.root_node:get_node_for_view(core.last_active_view) or node
    end
    if node.locked then
        node = assert(get_unlocked_leaf(self.root_node))
    end
    return node
end

function RootView:open_doc(doc)
    local node = self:get_active_node_default()
    for i, view in ipairs(node.views) do
        if view.doc == doc then
            node:set_active_view(node.views[i])
            return view
        end
    end
    local view = DocView(doc)
    node:add_view(view)
    self.root_node:update_layout()
    view:scroll_to_line(view.doc:get_selection(), true, true)
    return view
end

function RootView:on_mouse_pressed(button, x, y, clicks)
    local div = self.root_node:get_divider_overlapping_point(x, y)
    if div then
        self.dragged_divider = div
        return
    end
    local node = self.root_node:get_child_overlapping_point(x, y)
    local idx = node:get_tab_overlapping_point(x, y)
    if idx then
        node:set_active_view(node.views[idx])
        if button == "middle" then
            node:close_active_view(self.root_node)
        end
    else
        core.set_active_view(node.active_view)
        node.active_view:on_mouse_pressed(button, x, y, clicks)
    end
end

function RootView:on_mouse_released(...)
    if self.dragged_divider then
        self.dragged_divider = nil
    end
    self.root_node:on_mouse_released(...)
end

function RootView:on_mouse_moved(x, y, dx, dy)
    if self.dragged_divider then
        local node = self.dragged_divider
        local axis, mouse, delta = "x", x, dx
        if node.type == "vsplit" then
            axis, mouse, delta = "y", y, dy
        end
        -- a locked view pins the split's layout, so resize it directly
        -- (to the absolute mouse position, so the divider tracks the
        -- cursor exactly); free splits keep lite's proportional divider
        if node.a.type == "leaf" and node.a.locked then
            node.a.active_view:set_target_size(axis, mouse - node.position[axis])
        elseif node.b.type == "leaf" and node.b.locked then
            node.b.active_view:set_target_size(axis, node.position[axis] + node.size[axis] - mouse)
        else
            node.divider = common.clamp(node.divider + delta / node.size[axis], 0.01, 0.99)
        end
        return
    end

    self.mouse.x, self.mouse.y = x, y
    self.root_node:on_mouse_moved(x, y, dx, dy)

    local node = self.root_node:get_child_overlapping_point(x, y)
    local div = self.root_node:get_divider_overlapping_point(x, y)
    if div then
        system.set_cursor(div.type == "hsplit" and "sizeh" or "sizev")
    elseif node:get_tab_overlapping_point(x, y) then
        system.set_cursor("arrow")
    else
        system.set_cursor(node.active_view.cursor)
    end
end

function RootView:on_mouse_wheel(...)
    local x, y = self.mouse.x, self.mouse.y
    local node = self.root_node:get_child_overlapping_point(x, y)
    node.active_view:on_mouse_wheel(...)
end

function RootView:on_text_input(...)
    core.active_view:on_text_input(...)
end

function RootView:update()
    copy_position_and_size(self.root_node, self)
    self.root_node:update()
    self.root_node:update_layout()
end

function RootView:draw()
    self.root_node:draw()
    while #self.deferred_draws > 0 do
        local t = table.remove(self.deferred_draws)
        t.fn(table.unpack(t))
    end
end

return RootView

-- an image viewer.
--
-- the second claim on DEVIATIONS §20's door, and the specific one: a png
-- is a binary file too, so this has to be asked before the hex view's
-- universal claim or every image would open as bytes. one that fails to
-- decode falls through to the hex view on purpose -- a truncated png is
-- best looked at as the bytes it actually is.
--
-- there is nothing to edit, so the whole view is one question: where is
-- the image and how big. that is a scale and a point, and every key here
-- moves one of the two.

local core = require("core")
local common = require("core.common")
local command = require("core.command")
local keymap = require("core.keymap")
local style = require("core.style")
local View = require("core.view")
local StatusView = require("core.statusview")

-- the zooms a viewer offers: powers of two, because at every one of them
-- a source pixel lands on a whole number of screen pixels and the image
-- is shown rather than resampled.
--
-- every scale here is screen pixels per source pixel, and `NATURAL` is
-- the one that means 100%: one source pixel covering one *ui* pixel, not
-- one hardware pixel. on a hidpi display the second would draw a picture
-- at half the size of every other thing on screen, which is not what
-- anyone means by actual size
local NATURAL = SCALE
local STEPS = { 0.0625, 0.125, 0.25, 0.5, 1, 2, 4, 8, 16, 32 }
for i = 1, #STEPS do
    STEPS[i] = STEPS[i] * NATURAL
end

local ImageView = View:extend()

function ImageView:new(filename, image)
    ImageView.super.new(self)
    self.filename = filename
    self.image = image or renderer.image.load(filename)
    -- the image point held at the middle of the view, and how many screen
    -- pixels a source pixel takes. panning moves the first, zooming the
    -- second, and everything on screen falls out of the two
    self.center = { x = self.image:get_width() / 2, y = self.image:get_height() / 2 }
    self.scale = 1
    -- until something is zoomed by hand, the image follows the window
    self.fitting = true
end

function ImageView:get_name()
    return self.filename and self.filename:match("[^/\\]*$") or "image"
end

-- fit, but never enlarge: a 16x16 icon blown up to fill a window is not
-- what anyone opening it wanted to see
function ImageView:fit_scale()
    local w, h = self.image:get_width(), self.image:get_height()
    local pad = style.padding.x * 2
    if self.size.x <= pad or self.size.y <= pad then
        return NATURAL
    end
    return math.min(NATURAL, (self.size.x - pad) / w, (self.size.y - pad) / h)
end

function ImageView:center_of_view()
    return self.position.x + self.size.x / 2, self.position.y + self.size.y / 2
end

--- zoom to `scale`, keeping whatever is under `(px, py)` under it. that
--- fixed point is what makes wheel-zoom feel like moving a lens rather
--- than jumping somewhere else in the picture
function ImageView:set_scale(scale, px, py)
    scale = common.clamp(scale, STEPS[1], STEPS[#STEPS])
    local cx, cy = self:center_of_view()
    px, py = px or cx, py or cy
    local ix = self.center.x + (px - cx) / self.scale
    local iy = self.center.y + (py - cy) / self.scale
    self.scale = scale
    self.center.x = ix - (px - cx) / scale
    self.center.y = iy - (py - cy) / scale
    self.fitting = false
    core.redraw = true
end

local function next_step(scale, dir)
    if dir > 0 then
        for _, s in ipairs(STEPS) do
            if s > scale + 1e-9 then
                return s
            end
        end
        return STEPS[#STEPS]
    end
    for i = #STEPS, 1, -1 do
        if STEPS[i] < scale - 1e-9 then
            return STEPS[i]
        end
    end
    return STEPS[1]
end

-- an axis that fits inside the view is centred and cannot be dragged; one
-- that does not is held so its edges never come past the edge of the view
function ImageView:clamp_pan()
    local size = { x = self.image:get_width(), y = self.image:get_height() }
    for _, axis in ipairs({ "x", "y" }) do
        local half = self.size[axis] / 2 / self.scale
        if size[axis] <= half * 2 then
            self.center[axis] = size[axis] / 2
        else
            self.center[axis] = common.clamp(self.center[axis], half, size[axis] - half)
        end
    end
end

function ImageView:update()
    if self.fitting then
        self.scale = self:fit_scale()
        self.center.x = self.image:get_width() / 2
        self.center.y = self.image:get_height() / 2
    end
    self:clamp_pan()
    ImageView.super.update(self)
end

-- where the image lands on screen, floored: a source pixel has to begin
-- on a whole screen pixel or a 1:1 image would be resampled by a fraction
function ImageView:get_image_rect()
    local w = math.max(1, math.floor(self.image:get_width() * self.scale))
    local h = math.max(1, math.floor(self.image:get_height() * self.scale))
    local cx, cy = self:center_of_view()
    return math.floor(cx - self.center.x * self.scale),
        math.floor(cy - self.center.y * self.scale),
        w,
        h
end

function ImageView:draw()
    self:draw_background(style.background)
    local x, y, w, h = self:get_image_rect()
    -- a panel behind the image rather than a checkerboard: transparency
    -- reads against it, the image's extent is always visible, and it is
    -- one rectangle instead of the thousands a checkerboard would cost
    -- on the draw path every frame
    renderer.draw_rect(x, y, w, h, style.background2)
    renderer.draw_image(self.image, x, y, w, h)
    local t = math.max(1, math.floor(SCALE))
    renderer.draw_rect(x - t, y - t, w + t * 2, t, style.divider)
    renderer.draw_rect(x - t, y + h, w + t * 2, t, style.divider)
    renderer.draw_rect(x - t, y, t, h, style.divider)
    renderer.draw_rect(x + w, y, t, h, style.divider)
end

function ImageView:on_mouse_pressed(button, x, y, clicks)
    if ImageView.super.on_mouse_pressed(self, button, x, y, clicks) then
        return true
    end
    -- a double click toggles between the whole picture and its pixels,
    -- which is the only thing anyone ever wants from an image viewer
    if clicks == 2 then
        if self.fitting then
            self:set_scale(NATURAL, x, y)
        else
            self.fitting = true
            core.redraw = true
        end
        return true
    end
    self.panning = true
    return true
end

function ImageView:on_mouse_released(...)
    ImageView.super.on_mouse_released(self, ...)
    self.panning = false
end

function ImageView:on_mouse_moved(x, y, dx, dy)
    ImageView.super.on_mouse_moved(self, x, y, dx, dy)
    self.hover = { x = x, y = y }
    if self.panning then
        self.center.x = self.center.x - dx / self.scale
        self.center.y = self.center.y - dy / self.scale
        self.fitting = false
        core.redraw = true
    end
    self.cursor = self.panning and "hand" or "arrow"
end

-- the plain wheel zooms, at the pointer. ctrl+wheel is the editor's own
-- zoom and the keymap takes it before the view ever sees it, so the two
-- never have to be told apart here
function ImageView:on_mouse_wheel(y)
    if y == 0 then
        return
    end
    local at = self.hover
    self:set_scale(next_step(self.scale, y), at and at.x, at and at.y)
end

--- opens `filename` in an image view, or focuses the one already showing it
function ImageView.open(filename, image)
    filename = system.absolute_path(filename) or filename
    for _, view in ipairs(core.root_view.root_node:get_children()) do
        if view:is(ImageView) and view.filename == filename then
            local node = core.root_view.root_node:get_node_for_view(view)
            if node then
                node:set_active_view(view)
                return view
            end
        end
    end
    local view = ImageView(filename, image)
    core.root_view:get_active_node_default():add_view(view)
    core.root_view.root_node:update_layout()
    return view
end

-- the two the core's decoder reads, sniffed the way it sniffs them: by
-- content, never by extension
local function is_image(filename)
    local fp = io.open(filename, "rb")
    if not fp then
        return false
    end
    local magic = fp:read(8) or ""
    fp:close()
    return magic:sub(1, 8) == "\137PNG\r\n\26\n" or magic:sub(1, 3) == "\255\216\255"
end

-- inserted at the front: the hex view claims every binary file, and a
-- specific claim is worth nothing behind a universal one
table.insert(core.file_openers, 1, function(filename)
    if not is_image(filename) then
        return
    end
    local ok, image = pcall(renderer.image.load, filename)
    if not ok then
        -- a picture that will not decode is best looked at as the bytes
        -- it actually is, so the hex view behind us gets it
        core.log("%q could not be decoded; opening its bytes instead", filename)
        return
    end
    return ImageView.open(filename, image)
end)

-- ---------------------------------------------------------- commands

local function active()
    return core.active_view and core.active_view:is(ImageView)
end

table.insert(keymap.modes, function()
    return active() and "image" or nil
end)

local function pan(dx, dy)
    return function()
        local v = core.active_view
        v.center.x = v.center.x + dx * 64 / v.scale
        v.center.y = v.center.y + dy * 64 / v.scale
        v.fitting = false
        core.redraw = true
    end
end

command.add(active, {
    ["image:zoom-in"] = function()
        local v = core.active_view
        v:set_scale(next_step(v.scale, 1))
    end,
    ["image:zoom-out"] = function()
        local v = core.active_view
        v:set_scale(next_step(v.scale, -1))
    end,
    ["image:actual-size"] = function()
        core.active_view:set_scale(NATURAL)
    end,
    ["image:fit"] = function()
        core.active_view.fitting = true
        core.redraw = true
    end,
    ["image:pan-left"] = pan(-1, 0),
    ["image:pan-right"] = pan(1, 0),
    ["image:pan-up"] = pan(0, -1),
    ["image:pan-down"] = pan(0, 1),
})

-- the status bar answers what an image viewer is asked: how big is it,
-- and how much of that am i looking at
local get_items = StatusView.get_items

function StatusView:get_items()
    if not active() then
        return get_items(self)
    end
    local v = core.active_view
    return {
        style.text,
        style.icon_font,
        style.icons.file,
        style.dim,
        style.font,
        self.separator2,
        style.text,
        v:get_name(),
        self.separator,
        string.format("%d x %d", v.image:get_width(), v.image:get_height()),
    }, {
        style.icon_font,
        style.icons.gear,
        style.font,
        style.dim,
        self.separator2,
        style.text,
        v.fitting and "fit" or "zoom",
        self.separator,
        string.format("%d%%", math.floor(v.scale / NATURAL * 100 + 0.5)),
    }
end

-- a mode of its own, so bare keys are free here without being bound
-- anywhere a document might want to type them
keymap.add({
    ["image:="] = "image:zoom-in",
    ["image:shift+="] = "image:zoom-in",
    ["image:-"] = "image:zoom-out",
    ["image:1"] = "image:actual-size",
    ["image:0"] = "image:fit",
    ["image:f"] = "image:fit",
    ["image:left"] = "image:pan-left",
    ["image:right"] = "image:pan-right",
    ["image:up"] = "image:pan-up",
    ["image:down"] = "image:pan-down",
})

return ImageView

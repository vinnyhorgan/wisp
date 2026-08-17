-- a hex editor.
--
-- DEVIATIONS §7 refuses binary files rather than rendering them as
-- garbage, and left one door open: "a view that claims a file type can
-- open it". this is the universal claimant -- where an imageview will
-- claim pngs, a hexview claims everything else, and the refusal becomes
-- the last resort instead of the answer.
--
-- the model is `plugins/hexview/buffer.lua`, a byte-addressed buffer
-- rather than a doc: a doc holds lines, and there is no arrangement of
-- lines that survives a round trip through arbitrary bytes.
--
-- the view is two panes over one cursor, the way every hex editor since
-- `xxd` has been: hex on the left, the same bytes as text on the right,
-- tab moves between them. editing overwrites by default, because a file
-- format usually has a shape and typing into it should not move
-- everything after the cursor; inserting and deleting bytes are their
-- own keys.

local core = require("core")
local common = require("core.common")
local command = require("core.command")
local config = require("core.config")
local keymap = require("core.keymap")
local style = require("core.style")
local View = require("core.view")
local StatusView = require("core.statusview")
local Buffer = require("plugins.hexview.buffer")

-- the classic sixteen. it is not a setting: sixteen bytes a row is what
-- every hex dump anyone has ever read is laid out in, and an offset
-- ending in 0 is a landmark you can count from
local COLUMNS = 16

-- the layout, in character cells -- the font is mono, so the whole view
-- is a grid and every position is arithmetic
local HEX_COL = 10

local function hex_column(i)
    -- a wider gap halfway along, so the eye can find byte 8 without
    -- counting to it
    return HEX_COL + i * 3 + (i >= COLUMNS // 2 and 1 or 0)
end

local TEXT_COL = hex_column(COLUMNS - 1) + 4
local TOTAL_COLS = TEXT_COL + COLUMNS

local HexView = View:extend()

-- bytes are coloured by what they are, which is most of what reading a
-- hex dump is: padding fades, text reads as text, the whitespace that
-- separates records stands apart from the rest of the noise
local function byte_color(b)
    if b == 0 then
        return style.dim
    end
    if b >= 0x20 and b <= 0x7e then
        return style.text
    end
    if b == 0x09 or b == 0x0a or b == 0x0d then
        return style.syntax["operator"]
    end
    return style.syntax["number"]
end

local function read_file(filename)
    local fp, err = io.open(filename, "rb")
    if not fp then
        -- a path that does not exist is a file waiting to be written,
        -- not an error -- the same rule the doc follows (DEVIATIONS §18)
        assert(not system.get_file_info(filename), err)
        return ""
    end
    local data = fp:read("a")
    fp:close()
    return data
end

function HexView:new(filename)
    HexView.super.new(self)
    self.filename = filename
    self.buffer = Buffer(filename and read_file(filename) or "")
    self.offset = 0
    self.anchor = 0
    self.pane = "hex"
    -- which half of the byte the next hex digit lands in
    self.nibble = 0
    self.scrollable = true
end

function HexView:get_name()
    local name = self.filename and self.filename:match("[^/\\]*$") or "untitled"
    return name .. (self.buffer:is_dirty() and "*" or "")
end

function HexView:get_font()
    return style.code_font
end

function HexView:get_line_height()
    return math.floor(self:get_font():get_height() * config.line_height)
end

function HexView:get_char_width()
    return self:get_font():get_width("0")
end

function HexView:get_row_count()
    return math.max(1, (self.buffer.size + COLUMNS - 1) // COLUMNS)
end

function HexView:get_scrollable_size()
    return self:get_line_height() * (self:get_row_count() - 1) + self.size.y
end

function HexView:get_h_scrollable_size()
    return TOTAL_COLS * self:get_char_width() + style.padding.x * 2
end

function HexView:get_visible_rows()
    local lh = self:get_line_height()
    local first = math.max(0, math.floor(self.scroll.y / lh))
    local last = math.min(self:get_row_count() - 1, math.floor((self.scroll.y + self.size.y) / lh))
    return first, last
end

-- the top-left of the character grid, scroll and padding already applied
function HexView:get_grid_offset()
    local x, y = self:get_content_offset()
    return x + style.padding.x, y
end

-- ------------------------------------------------------------- cursor

function HexView:max_offset()
    return math.max(0, self.buffer.size - 1)
end

function HexView:move_to(offset, extend)
    self.offset = common.clamp(math.floor(offset), 0, self:max_offset())
    if not extend then
        self.anchor = self.offset
    end
    self.nibble = 0
    self:scroll_to_offset(self.offset)
    core.redraw = true
end

--- the selected range as `from, to` inclusive, or nil when the cursor is
--- alone. a hex editor's cursor is not a selection: one byte is where you
--- are, not what you have picked out
function HexView:get_selection()
    if self.offset == self.anchor then
        return nil
    end
    return math.min(self.offset, self.anchor), math.max(self.offset, self.anchor)
end

function HexView:scroll_to_offset(offset)
    local lh = self:get_line_height()
    local y = (offset // COLUMNS) * lh
    if y < self.scroll.to.y then
        self.scroll.to.y = y
    elseif y + lh > self.scroll.to.y + self.size.y then
        self.scroll.to.y = y + lh - self.size.y
    end
    -- sideways too. seventy-six columns is a wide view, and on a window
    -- that cannot hold both panes at once, tabbing to the other one has
    -- to bring it with you
    local cw = self:get_char_width()
    local i = offset % COLUMNS
    local x = (self.pane == "hex" and hex_column(i) or TEXT_COL + i) * cw
    local width = self.size.x - style.padding.x * 2
    if x < self.scroll.to.x then
        self.scroll.to.x = x
    elseif x + cw * 2 > self.scroll.to.x + width then
        self.scroll.to.x = x + cw * 2 - width
    end
end

-- ------------------------------------------------------------- editing

function HexView:write(offset, text)
    self.buffer:splice(offset, #text, text, self.offset)
end

function HexView:type_nibble(value)
    local b = self.buffer:byte(self.offset)
    if not b then
        return
    end
    if self.nibble == 0 then
        b = value * 16 + b % 16
    else
        b = b - b % 16 + value
    end
    self:write(self.offset, string.char(b))
    -- the first digit stays put so the second can land in the same byte;
    -- the second moves on, which is what makes typing a run of bytes feel
    -- like typing
    if self.nibble == 0 then
        self.nibble = 1
        self.anchor = self.offset
    else
        self:move_to(self.offset + 1)
    end
    core.redraw = true
end

function HexView:on_text_input(text)
    if self.pane == "hex" then
        local digit = text:match("^%x$")
        if digit then
            self:type_nibble(tonumber(digit, 16))
        end
        return
    end
    -- the text pane writes exactly the byte you typed, so it is ascii
    -- only: anything else is more than one byte and would mean deciding
    -- an encoding on the user's behalf
    local b = text:byte()
    if not b or #text ~= 1 or b > 0x7e or b < 0x20 then
        return
    end
    if self.buffer:byte(self.offset) then
        self:write(self.offset, text)
        self:move_to(self.offset + 1)
    end
end

-- ------------------------------------------------------------- drawing

function HexView:resolve_position(x, y)
    local gx, gy = self:get_grid_offset()
    local cw, lh = self:get_char_width(), self:get_line_height()
    local row = math.floor((y - gy) / lh)
    local col = math.floor((x - gx) / cw)
    row = common.clamp(row, 0, self:get_row_count() - 1)
    local pane, i = "hex", nil
    if col >= TEXT_COL then
        pane, i = "text", common.clamp(col - TEXT_COL, 0, COLUMNS - 1)
    else
        local rel = math.max(0, col - HEX_COL)
        -- undo the wider gap halfway along before dividing
        if rel >= (COLUMNS // 2) * 3 then
            rel = rel - 1
        end
        i = common.clamp(rel // 3, 0, COLUMNS - 1)
    end
    return common.clamp(row * COLUMNS + i, 0, self:max_offset()), pane
end

function HexView:on_mouse_pressed(button, x, y, clicks)
    if HexView.super.on_mouse_pressed(self, button, x, y, clicks) then
        return true
    end
    local offset, pane = self:resolve_position(x, y)
    self.pane = pane
    self:move_to(offset, keymap.modkeys["shift"])
    self.mouse_selecting = true
    return true
end

function HexView:on_mouse_moved(x, y, ...)
    HexView.super.on_mouse_moved(self, x, y, ...)
    self.cursor = self:scrollbar_overlaps_point(x, y) and "arrow" or "ibeam"
    if self.mouse_selecting then
        local offset = self:resolve_position(x, y)
        self.offset = offset
        core.redraw = true
    end
end

function HexView:on_mouse_released(...)
    HexView.super.on_mouse_released(self, ...)
    self.mouse_selecting = false
end

-- a filled block on the byte the cursor is on, the character redrawn over
-- it in the background colour. the pane that does not have the keyboard
-- gets an outline of the same box instead, so both halves always show
-- where you are and only one of them claims to be listening
function HexView:draw_cursor(x, y, active)
    local cw, lh = self:get_char_width(), self:get_line_height()
    local width = active and self.pane == "hex" and cw * 2 or cw
    if active then
        renderer.draw_rect(x, y, width, lh, style.caret)
    else
        local t = math.max(1, math.floor(SCALE))
        renderer.draw_rect(x, y, width, t, style.caret)
        renderer.draw_rect(x, y + lh - t, width, t, style.caret)
        renderer.draw_rect(x, y, t, lh, style.caret)
        renderer.draw_rect(x + width - t, y, t, lh, style.caret)
    end
end

function HexView:draw()
    self:draw_background(style.background)
    local font = self:get_font()
    local cw, lh = self:get_char_width(), self:get_line_height()
    local gx, gy = self:get_grid_offset()
    local first, last = self:get_visible_rows()
    local from, to = self:get_selection()
    local focused = core.active_view == self
    local ty = math.floor((lh - font:get_height()) / 2)

    for row = first, last do
        local y = gy + row * lh
        local base = row * COLUMNS
        if
            config.highlight_current_line
            and focused
            and not from
            and self.offset // COLUMNS == row
        then
            renderer.draw_rect(
                self.position.x,
                y,
                math.max(self.size.x, TOTAL_COLS * cw),
                lh,
                style.line_highlight
            )
        end
        renderer.draw_text(font, string.format("%08x", base), gx, y + ty, style.line_number)

        -- the selection is drawn as one run per row rather than a box a
        -- byte: the gaps between the columns belong to the run too, or a
        -- span of bytes reads as a row of separate things
        if from and to >= base and from < base + COLUMNS then
            local i0 = math.max(from, base) - base
            local i1 = math.min(to, base + COLUMNS - 1) - base
            local hx = gx + hex_column(i0) * cw
            renderer.draw_rect(hx, y, gx + (hex_column(i1) + 2) * cw - hx, lh, style.selection)
            local tx = gx + (TEXT_COL + i0) * cw
            renderer.draw_rect(tx, y, (i1 - i0 + 1) * cw, lh, style.selection)
        end

        for i = 0, COLUMNS - 1 do
            local offset = base + i
            local b = self.buffer:byte(offset)
            if not b then
                break
            end
            local hx = gx + hex_column(i) * cw
            local tx = gx + (TEXT_COL + i) * cw

            local color = byte_color(b)
            local hex_color, text_color = color, color
            if offset == self.offset then
                self:draw_cursor(hx, y, focused and self.pane == "hex")
                self:draw_cursor(tx, y, focused and self.pane == "text")
                if focused then
                    hex_color = self.pane == "hex" and style.background or color
                    text_color = self.pane == "text" and style.background or color
                end
            end
            renderer.draw_text(font, string.format("%02x", b), hx, y + ty, hex_color)
            local ch = (b >= 0x20 and b <= 0x7e) and string.char(b) or "."
            renderer.draw_text(font, ch, tx, y + ty, text_color)
        end
    end

    self:draw_scrollbar()
end

-- ------------------------------------------------------------- closing

function HexView:try_close(do_close)
    if not self.buffer:is_dirty() then
        return do_close()
    end
    core.command_view:enter("unsaved changes; confirm close", function(_, item)
        if item.text:match("^[cC]") then
            do_close()
        elseif item.text:match("^[sS]") then
            -- not through the command: the prompt is the active view
            -- while this runs, so `hex:save`'s predicate would refuse
            if core.try(self.save, self) then
                do_close()
            end
        end
    end, function(text)
        local items = {}
        if not text:find("^[^cC]") then
            table.insert(items, "close without saving")
        end
        if not text:find("^[^sS]") then
            table.insert(items, "save and close")
        end
        return items
    end)
end

-- ------------------------------------------------------------- opening

local function each_hexview()
    local views = {}
    for _, view in ipairs(core.root_view.root_node:get_children()) do
        if view:is(HexView) then
            table.insert(views, view)
        end
    end
    return views
end

--- opens `filename` in a hex view, or focuses the one already showing it
function HexView.open(filename)
    filename = system.absolute_path(filename) or filename
    for _, view in ipairs(each_hexview()) do
        if view.filename == filename then
            local node = core.root_view.root_node:get_node_for_view(view)
            if node then
                node:set_active_view(view)
                return view
            end
        end
    end
    local view = HexView(filename)
    core.root_view:get_active_node_default():add_view(view)
    core.root_view.root_node:update_layout()
    return view
end

-- exactly the test the doc refuses on, asked before the doc is built
-- rather than after it has raised: a null byte in the first 4096 is what
-- "binary" means here, so the hexview claims precisely the files the
-- docview would have turned away
local function is_binary(filename)
    local fp = io.open(filename, "rb")
    if not fp then
        return false
    end
    local chunk = fp:read(4096)
    fp:close()
    return chunk ~= nil and chunk:find("\0", 1, true) ~= nil
end

table.insert(core.file_openers, function(filename)
    if is_binary(filename) then
        return HexView.open(filename)
    end
end)

-- unsaved bytes are as easy to lose as unsaved text, and neither the
-- quit prompt nor the signal rescue walks anything but `core.docs`
local function dirty_hexviews()
    local dirty = {}
    for _, view in ipairs(each_hexview()) do
        if view.buffer:is_dirty() and view.filename then
            table.insert(dirty, view)
        end
    end
    return dirty
end

local quit = core.quit
local bytes_confirmed = false

function core.quit(force)
    local dirty = (not force and not bytes_confirmed) and dirty_hexviews() or {}
    if #dirty > 0 then
        local text = #dirty == 1
                and string.format('"%s" has unsaved bytes. quit anyway?', dirty[1]:get_name())
            or string.format("%d hex views have unsaved bytes. quit anyway?", #dirty)
        core.command_view:enter(text, function(answer)
            if answer:lower():find("^y") then
                -- answered once: the docs still get their own prompt on
                -- the way past, which is the one core already raises
                bytes_confirmed = true
                core.quit()
            end
        end, function(answer)
            return common.fuzzy_match({ "no", "yes" }, answer)
        end)
        return
    end
    return quit(force)
end

local terminate = core.terminate

function core.terminate()
    for _, view in ipairs(dirty_hexviews()) do
        core.try(view.save, view, view.filename .. "~")
    end
    return terminate()
end

function HexView:save(filename)
    filename = filename or assert(self.filename, "no filename set to default to")
    local fp = assert(io.open(filename, "wb"))
    fp:write(self.buffer:tostring())
    fp:close()
    if filename == self.filename then
        self.buffer:clean()
    end
end

-- ------------------------------------------------------------ commands

local function view()
    return core.active_view
end

local function active()
    return core.active_view and core.active_view:is(HexView)
end

-- `"text"` is the bytes as typed; anything else is hex pairs with the
-- spaces ignored. hex is the unquoted default because in a hex editor it
-- is what is on the screen in front of you
local function parse_bytes(text)
    local quoted = text:match('^"(.*)"$')
    if quoted then
        return quoted
    end
    local digits = text:gsub("%s", "")
    if digits == "" or #digits % 2 ~= 0 or digits:find("%X") then
        return nil
    end
    return (
        digits:gsub("%x%x", function(pair)
            return string.char(tonumber(pair, 16))
        end)
    )
end

local function move(delta, extend)
    return function()
        view():move_to(view().offset + delta, extend)
    end
end

local last_search

local function search(backward)
    local v = view()
    if not last_search then
        return
    end
    local at = backward and v.buffer:rfind(last_search, v.offset)
        or v.buffer:find(last_search, v.offset + 1)
    if not at then
        core.error("couldn't find those bytes")
        return
    end
    v:move_to(at)
    v.anchor = math.min(at + #last_search - 1, v:max_offset())
end

command.add(active, {
    ["hex:move-left"] = move(-1),
    ["hex:move-right"] = move(1),
    ["hex:move-up"] = move(-COLUMNS),
    ["hex:move-down"] = move(COLUMNS),
    ["hex:select-left"] = move(-1, true),
    ["hex:select-right"] = move(1, true),
    ["hex:select-up"] = move(-COLUMNS, true),
    ["hex:select-down"] = move(COLUMNS, true),

    ["hex:move-to-row-start"] = function()
        view():move_to(view().offset - view().offset % COLUMNS)
    end,
    ["hex:move-to-row-end"] = function()
        view():move_to(view().offset - view().offset % COLUMNS + COLUMNS - 1)
    end,
    ["hex:move-to-start"] = function()
        view():move_to(0)
    end,
    ["hex:move-to-end"] = function()
        view():move_to(view():max_offset())
    end,
    ["hex:move-page-up"] = function()
        local v = view()
        v:move_to(v.offset - (v.size.y // v:get_line_height()) * COLUMNS)
    end,
    ["hex:move-page-down"] = function()
        local v = view()
        v:move_to(v.offset + (v.size.y // v:get_line_height()) * COLUMNS)
    end,
    ["hex:select-all"] = function()
        view().anchor = 0
        view():move_to(view():max_offset(), true)
    end,

    ["hex:switch-pane"] = function()
        local v = view()
        v.pane = v.pane == "hex" and "text" or "hex"
        v.nibble = 0
        v:scroll_to_offset(v.offset)
        core.redraw = true
    end,

    ["hex:undo"] = function()
        local at = view().buffer:undo()
        if at then
            view():move_to(at)
        end
    end,
    ["hex:redo"] = function()
        local at = view().buffer:redo()
        if at then
            view():move_to(at)
        end
    end,

    ["hex:save"] = function()
        local v = view()
        v:save()
        core.log("saved %q (%d bytes)", v.filename, v.buffer.size)
    end,

    -- inserting and deleting move every byte after the cursor, so they
    -- are deliberately their own keys rather than what typing does
    ["hex:insert-byte"] = function()
        local v = view()
        v.buffer:splice(v.offset, 0, "\0", v.offset)
        v:move_to(v.offset)
    end,
    ["hex:append-byte"] = function()
        local v = view()
        v.buffer:splice(v.buffer.size, 0, "\0", v.offset)
        v:move_to(v.buffer.size - 1)
    end,
    ["hex:delete"] = function()
        local v = view()
        local from, to = v:get_selection()
        from, to = from or v.offset, to or v.offset
        if not v.buffer:byte(from) then
            return
        end
        v.buffer:splice(from, to - from + 1, "", v.offset)
        v:move_to(from)
    end,
    ["hex:delete-previous"] = function()
        local v = view()
        if v.offset == 0 or v.buffer.size == 0 then
            return
        end
        v.buffer:splice(v.offset - 1, 1, "", v.offset)
        v:move_to(v.offset - 1)
    end,

    ["hex:copy"] = function()
        local v = view()
        local from, to = v:get_selection()
        from, to = from or v.offset, to or v.offset
        local bytes = v.buffer:sub(from, to - from + 1)
        system.set_clipboard((bytes
            :gsub(".", function(c)
                return string.format("%02x ", c:byte())
            end)
            :gsub(" $", "")))
        core.log("copied %d bytes as hex", #bytes)
    end,
    ["hex:paste"] = function()
        local v = view()
        local bytes = parse_bytes(system.get_clipboard() or "")
        if not bytes or bytes == "" then
            core.error("the clipboard does not hold hex bytes")
            return
        end
        -- pasting overwrites, like typing does
        local room = math.min(#bytes, v.buffer.size - v.offset)
        if room <= 0 then
            return
        end
        v:write(v.offset, bytes:sub(1, room))
        v:move_to(v.offset + room - 1)
    end,

    ["hex:go-to-offset"] = function()
        core.command_view:enter("go to offset (hex)", function(text)
            local at = tonumber((text:gsub("^0[xX]", "")), 16)
            if not at then
                core.error("%q is not an offset", text)
                return
            end
            view():move_to(at)
        end)
    end,

    ["hex:find"] = function()
        core.command_view:enter('find bytes (hex, or "text")', function(text)
            local bytes = parse_bytes(text)
            if not bytes or bytes == "" then
                core.error("%q is neither hex pairs nor a quoted string", text)
                return
            end
            last_search = bytes
            search(false)
        end)
    end,
    ["hex:find-next"] = function()
        search(false)
    end,
    ["hex:find-previous"] = function()
        search(true)
    end,
})

command.add(nil, {
    ["hex:open-file"] = function()
        core.command_view:enter("open file as hex", function(text)
            HexView.open(common.home_expand(text))
        end, common.path_suggest)
    end,
})

-- the status bar answers the two questions a hex editor is asked: where
-- am i, and what is under the cursor
local get_items = StatusView.get_items

function StatusView:get_items()
    if not active() then
        return get_items(self)
    end
    local v = core.active_view
    local b = v.buffer:byte(v.offset)
    local from, to = v:get_selection()
    return {
        v.buffer:is_dirty() and style.accent or style.text,
        style.icon_font,
        style.icons.file,
        style.dim,
        style.font,
        self.separator2,
        style.text,
        v:get_name(),
        self.separator,
        "offset: ",
        string.format("%08x", v.offset),
        self.separator,
        "byte: ",
        b and string.format("%02x %3d", b, b) or "--",
    }, {
        style.icon_font,
        style.icons.gear,
        style.font,
        style.dim,
        self.separator2,
        style.text,
        v.pane,
        self.separator,
        from and string.format("%d selected", to - from + 1)
            or string.format("%d bytes", v.buffer.size),
    }
end

keymap.add({
    ["left"] = "hex:move-left",
    ["right"] = "hex:move-right",
    ["up"] = "hex:move-up",
    ["down"] = "hex:move-down",
    ["shift+left"] = "hex:select-left",
    ["shift+right"] = "hex:select-right",
    ["shift+up"] = "hex:select-up",
    ["shift+down"] = "hex:select-down",
    ["home"] = "hex:move-to-row-start",
    ["end"] = "hex:move-to-row-end",
    ["ctrl+home"] = "hex:move-to-start",
    ["ctrl+end"] = "hex:move-to-end",
    ["pageup"] = "hex:move-page-up",
    ["pagedown"] = "hex:move-page-down",
    ["ctrl+a"] = "hex:select-all",
    ["tab"] = "hex:switch-pane",
    ["ctrl+z"] = "hex:undo",
    ["ctrl+y"] = "hex:redo",
    ["ctrl+s"] = "hex:save",
    ["ctrl+c"] = "hex:copy",
    ["ctrl+v"] = "hex:paste",
    ["ctrl+g"] = "hex:go-to-offset",
    ["ctrl+f"] = "hex:find",
    ["f3"] = "hex:find-next",
    ["shift+f3"] = "hex:find-previous",
    ["delete"] = "hex:delete",
    ["backspace"] = "hex:delete-previous",
    ["ctrl+return"] = "hex:insert-byte",
    ["ctrl+shift+return"] = "hex:append-byte",
})

return HexView

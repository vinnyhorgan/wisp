-- a real terminal in a wisp view: alacritty's vt engine lives in the
-- core (system.terminal), this plugin is everything you see and type.
-- the view draws the grid as style runs, translates keys to escape
-- sequences, and polls the pty from a core thread like every other
-- background task. keys bound in config.terminal_pass_through stay
-- editor keys; everything else belongs to the shell while a terminal
-- has focus.

local core = require("core")
local common = require("core.common")
local command = require("core.command")
local config = require("core.config")
local keymap = require("core.keymap")
local style = require("core.style")
local View = require("core.view")

config.terminal_argv = nil -- nil runs the user's own shell
config.terminal_pass_through = {
    ["ctrl+shift+p"] = true,
    ["ctrl+`"] = true,
    ["ctrl+shift+v"] = true,
    ["alt+return"] = true,
    ["ctrl+tab"] = true,
    ["ctrl+shift+tab"] = true,
    ["alt+shift+j"] = true,
    ["alt+shift+l"] = true,
    ["alt+shift+i"] = true,
    ["alt+shift+k"] = true,
    ["alt+j"] = true,
    ["alt+l"] = true,
    ["alt+i"] = true,
    ["alt+k"] = true,
}
for i = 1, 9 do
    config.terminal_pass_through["alt+" .. i] = true
end

-- catppuccin mocha, same source as style.lua: the official palette,
-- with the terminal mapping the catppuccin ports use everywhere
-- stylua: ignore
local palette = {
    { common.color("#45475a") }, -- surface1 (black)
    { common.color("#f38ba8") }, -- red
    { common.color("#a6e3a1") }, -- green
    { common.color("#f9e2af") }, -- yellow
    { common.color("#89b4fa") }, -- blue
    { common.color("#f5c2e7") }, -- pink (magenta)
    { common.color("#94e2d5") }, -- teal (cyan)
    { common.color("#bac2de") }, -- subtext1 (white)
    { common.color("#585b70") }, -- surface2 (bright black)
    { common.color("#f38ba8") }, -- red
    { common.color("#a6e3a1") }, -- green
    { common.color("#f9e2af") }, -- yellow
    { common.color("#89b4fa") }, -- blue
    { common.color("#f5c2e7") }, -- pink
    { common.color("#94e2d5") }, -- teal
    { common.color("#a6adc8") }, -- subtext0 (bright white)
}
local fg_color = { common.color("#cdd6f4") } -- text
local bg_color = { common.color("#1e1e2e") } -- base

-- the core hands colors over packed; cells on the default background
-- need no rect at all, draw_background already painted it
local bg_packed = bg_color[1] * 65536 + bg_color[2] * 256 + bg_color[3]

local TerminalView = View:extend()

function TerminalView:new()
    TerminalView.super.new(self)
    self.cursor = "ibeam"
    self.scrollable = false
    self.title = "terminal"
    local cw, ch = self:get_cell_size()
    -- no cwd: the shell inherits the editor's, which core.init already
    -- pointed at the project
    local term, err = system.terminal(80, 24, {
        argv = config.terminal_argv,
        cell_width = cw,
        cell_height = ch,
    })
    if not term then
        error("could not open a terminal: " .. err, 0)
    end
    self.terminal = term
    self.terminal:set_palette(palette, fg_color, bg_color)
    self.color_cache, self.color_count = {}, 0
    -- the pty poller; the weak key ties its lifetime to the view
    core.add_thread(function()
        while self.terminal do
            if self.terminal:update() then
                core.redraw = true
            end
            if not self.terminal:running() then
                self:close()
                break
            end
            coroutine.yield(0.01)
        end
    end, self)
end

function TerminalView:get_name()
    return self.title
end

function TerminalView:get_cell_size()
    return style.code_font:get_width("a"), style.code_font:get_height()
end

-- packed 0xrrggbb from the core becomes a color table exactly once. a
-- truecolor program can name sixteen million of them, so the cache
-- starts over instead of growing for the life of the view
function TerminalView:color_of(packed)
    local color = self.color_cache[packed]
    if not color then
        if self.color_count >= 4096 then
            self.color_cache, self.color_count = {}, 0
        end
        color = {
            math.floor(packed / 65536) % 256,
            math.floor(packed / 256) % 256,
            packed % 256,
            255,
        }
        self.color_cache[packed] = color
        self.color_count = self.color_count + 1
    end
    return color
end

function TerminalView:close()
    self.terminal = nil
    local node = core.root_view.root_node:get_node_for_view(self)
    if node then
        node:set_active_view(self)
        node:close_active_view(core.root_view.root_node)
    end
end

function TerminalView:update()
    TerminalView.super.update(self)
    if not self.terminal then
        return
    end
    local cw, ch = self:get_cell_size()
    local cols = math.max(2, math.floor(self.size.x / cw))
    local rows = math.max(2, math.floor(self.size.y / ch))
    if cols ~= self.cols or rows ~= self.rows then
        self.cols, self.rows = cols, rows
        self.terminal:resize(cols, rows, cw, ch)
    end
    for _, event in ipairs(self.terminal:events()) do
        if event[1] == "title" and event[2] ~= "" then
            self.title = event[2]
        elseif event[1] == "reset_title" then
            self.title = "terminal"
        elseif event[1] == "clipboard" then
            system.set_clipboard(event[2])
        end
    end
end

function TerminalView:on_text_input(text)
    if self.terminal then
        self.terminal:scroll_reset()
        self.terminal:send(text)
    end
end

function TerminalView:on_mouse_wheel(y)
    if self.terminal then
        self.terminal:scroll(y * 3)
        core.redraw = true
    end
end

function TerminalView:draw()
    self:draw_background(bg_color)
    if not self.terminal then
        return
    end
    local cw, ch = self:get_cell_size()
    local ox, oy = self.position.x, self.position.y
    for row = 1, self.rows or 0 do
        local runs = self.terminal:line(row)
        local x = ox
        local y = oy + (row - 1) * ch
        for i = 1, #runs, 5 do
            local text, fg, bg = runs[i], runs[i + 1], runs[i + 2]
            local flags, width = runs[i + 3], runs[i + 4]
            local w = width * cw
            if bg ~= bg_packed then
                renderer.draw_rect(x, y, w, ch, self:color_of(bg))
            end
            renderer.draw_text(style.code_font, text, x, y, self:color_of(fg))
            if flags % 8 >= 4 then
                renderer.draw_rect(x, y + ch - 1, w, 1, self:color_of(fg))
            end
            x = x + w
        end
    end
    local col, row, visible = self.terminal:cursor()
    if visible and core.active_view == self and system.window_has_focus() then
        local x = ox + (col - 1) * cw
        local y = oy + (row - 1) * ch
        renderer.draw_rect(x, y, cw, ch, style.caret)
    end
end

-- keys become escape sequences while a terminal has focus ------------

local tilde_keys = {
    ["insert"] = 2,
    ["delete"] = 3,
    ["pageup"] = 5,
    ["pagedown"] = 6,
    ["f5"] = 15,
    ["f6"] = 17,
    ["f7"] = 18,
    ["f8"] = 19,
    ["f9"] = 20,
    ["f10"] = 21,
    ["f11"] = 23,
    ["f12"] = 24,
}
local arrow_keys = { up = "A", down = "B", right = "C", left = "D" }
local homeend_keys = { home = "H", ["end"] = "F" }
local fn_keys = { f1 = "\27OP", f2 = "\27OQ", f3 = "\27OR", f4 = "\27OS" }
local ctrl_punct = { ["["] = 27, ["\\"] = 28, ["]"] = 29, ["space"] = 0 }

local function key_sequence(term, k)
    local mods = keymap.modkeys
    local code = 1
        + (mods["shift"] and 1 or 0)
        + (mods["alt"] and 2 or 0)
        + (mods["ctrl"] and 4 or 0)
    if arrow_keys[k] then
        if code > 1 then
            return "\27[1;" .. code .. arrow_keys[k]
        end
        return (term:app_cursor() and "\27O" or "\27[") .. arrow_keys[k]
    end
    if homeend_keys[k] then
        if code > 1 then
            return "\27[1;" .. code .. homeend_keys[k]
        end
        return (term:app_cursor() and "\27O" or "\27[") .. homeend_keys[k]
    end
    if tilde_keys[k] then
        if code > 1 then
            return "\27[" .. tilde_keys[k] .. ";" .. code .. "~"
        end
        return "\27[" .. tilde_keys[k] .. "~"
    end
    if fn_keys[k] then
        return fn_keys[k]
    end
    if k == "return" or k == "keypad enter" then
        return "\r"
    end
    if k == "escape" then
        return "\27"
    end
    if k == "backspace" then
        return mods["ctrl"] and "\8" or "\127"
    end
    if k == "tab" then
        return mods["shift"] and "\27[Z" or "\t"
    end
    if mods["ctrl"] then
        if #k == 1 and k >= "a" and k <= "z" then
            local seq = string.char(k:byte() - 96)
            return mods["alt"] and "\27" .. seq or seq
        end
        if ctrl_punct[k] then
            return string.char(ctrl_punct[k])
        end
    end
    if mods["alt"] and #k == 1 then
        -- meta: escape-prefixed. safe to consume, the editor drops the
        -- textinput that follows a handled keypress
        return "\27" .. k
    end
end

local on_key_pressed = keymap.on_key_pressed
function keymap.on_key_pressed(k)
    local view = core.active_view
    if not (view.terminal and view:is(TerminalView)) or keymap.modkey_map[k] then
        return on_key_pressed(k)
    end
    if config.terminal_pass_through[keymap.key_to_stroke(k)] then
        return on_key_pressed(k)
    end
    local seq = key_sequence(view.terminal, k)
    if seq then
        view.terminal:scroll_reset()
        view.terminal:send(seq)
        core.redraw = true
        return true
    end
    return on_key_pressed(k)
end

-- commands ------------------------------------------------------------

local last_terminal

local function open_terminal()
    local node = core.root_view:get_active_node_default()
    local view = TerminalView()
    node:add_view(view)
    last_terminal = view
    return view
end

command.add(nil, {
    ["terminal:open"] = function()
        core.try(open_terminal)
    end,

    ["terminal:toggle"] = function()
        local view = core.active_view
        if view:is(TerminalView) then
            -- back to where you were
            local prev = core.last_active_view
            local node = prev and core.root_view.root_node:get_node_for_view(prev)
            if node then
                node:set_active_view(prev)
            end
            return
        end
        local node = last_terminal and core.root_view.root_node:get_node_for_view(last_terminal)
        if node then
            node:set_active_view(last_terminal)
        else
            core.try(open_terminal)
        end
    end,
})

command.add(TerminalView, {
    ["terminal:paste"] = function()
        local view = core.active_view
        if not view.terminal then
            return
        end
        local text = system.get_clipboard() or ""
        view.terminal:scroll_reset()
        if view.terminal:bracketed_paste() then
            view.terminal:send("\27[200~" .. text .. "\27[201~")
        else
            view.terminal:send(text)
        end
    end,
})

keymap.add({
    ["ctrl+`"] = "terminal:toggle",
    ["ctrl+shift+v"] = "terminal:paste",
})

return TerminalView

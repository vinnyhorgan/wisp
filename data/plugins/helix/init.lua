-- helix mode: selection-first modal editing, kakoune lineage.
--
-- the model is helix's, not vim's: you select first and act second, so
-- every action reads off a selection that is already visible. lite's doc
-- happens to be built the same way -- `selection.a` is the head (what
-- lite calls the caret) and `selection.b` is the anchor -- so a helix
-- cursor is nothing more exotic than a selection one character wide, and
-- the block you see is that selection.
--
-- opt in with `config.helix_mode = true` in your user module, or the
-- `helix: toggle` command. modal editing is the one thing that cannot be
-- chosen for someone else.

local core = require("core")
local command = require("core.command")
local config = require("core.config")
local keymap = require("core.keymap")
local style = require("core.style")
local common = require("core.common")
local translate = require("core.doc.translate")
local motions = require("plugins.helix.motions")
local DocView = require("core.docview")
local StatusView = require("core.statusview")

config.helix_mode = false

local helix = {}

helix.enabled = false
helix.mode = "normal"

-- a count typed before a motion (`3w`), consumed by whatever runs next
helix.count = nil

function helix.take_count()
    local n = helix.count or 1
    helix.count = nil
    return n
end

-- ---------------------------------------------------------------- state

local function is_editable_view(view)
    -- the command prompt is a DocView subclass and must keep its own
    -- keys: a modal layer over a one-line prompt helps nobody
    return view and getmetatable(view) == DocView
end

function helix.active()
    return helix.enabled and is_editable_view(core.active_view)
end

function helix.set_mode(mode)
    helix.mode = mode
    keymap.mode = helix.enabled and ("helix-" .. mode) or nil
    core.redraw = true
end

function helix.enable()
    helix.enabled = true
    helix.set_mode("normal")
    if is_editable_view(core.active_view) then
        helix.widen(core.active_view.doc)
    end
end

function helix.disable()
    helix.enabled = false
    keymap.mode = nil
    core.redraw = true
end

-- the user module runs after the plugins load, so the setting is read on
-- the first frame rather than at require time
core.add_thread(function()
    if config.helix_mode then
        helix.enable()
    end
end)

-- ----------------------------------------------------------- selections

-- lite's caret sits *between* characters; a helix cursor sits *on* one.
-- the bridge is an invariant: outside insert mode a selection is never
-- empty, so the head always has a character under it. a bare caret takes
-- the character in front of it, or behind it at the very end of the doc
function helix.widen(doc)
    local hl, hc, al, ac = doc:get_selection()
    if hl ~= al or hc ~= ac then
        return
    end
    local nl, nc = doc:position_offset(hl, hc, translate.next_char)
    if nl ~= hl or nc ~= hc then
        doc:set_selection(nl, nc, hl, hc)
        return
    end
    -- end of the document: there is nothing in front, so sit on what is
    -- behind instead. an empty doc has neither and stays a bare caret
    local pl, pc = doc:position_offset(hl, hc, translate.previous_char)
    if pl ~= hl or pc ~= hc then
        doc:set_selection(pl, pc, hl, hc)
    end
end

-- the character the block sits on. a forward selection (head past the
-- anchor) shows the block on the character *behind* the head, a backward
-- one on the character *at* it -- the usual block-cursor convention, and
-- the reason `hl > al` has to be answered before drawing anything
function helix.head_char(doc)
    local hl, hc, al, ac = doc:get_selection()
    if hl > al or (hl == al and hc > ac) then
        return doc:position_offset(hl, hc, translate.previous_char)
    end
    return hl, hc
end

-- the anchor block, the mirror of `head_char`: the character the far end
-- of the selection is standing on
function helix.anchor_char(doc)
    local hl, hc, al, ac = doc:get_selection()
    if hl > al or (hl == al and hc > ac) then
        return al, ac
    end
    return doc:position_offset(al, ac, translate.previous_char)
end

-- lay the selection down in block terms: it runs from the character at
-- `(al, ac)` to the character at `(line, col)`, both included. lite's
-- head lives *between* characters, so whichever end is further on gets
-- pushed one character past the block it covers
function helix.place(doc, al, ac, line, col)
    if line > al or (line == al and col >= ac) then
        local hl, hc = translate.next_char(doc, line, col)
        -- at the very end of the document there is nothing in front to
        -- take, so the block is as wide as it can be
        doc:set_selection(hl, hc, al, ac)
    else
        local nl, nc = translate.next_char(doc, al, ac)
        doc:set_selection(line, col, nl, nc)
    end
end

-- helix has two flavours of motion and they are not interchangeable.
-- `h j k l` *move*: the block jumps and the selection collapses to the
-- one character under it. `w e b` *select*: the selection is laid from
-- where the cursor was to wherever the motion landed. in select mode
-- both keep the anchor they already had, which is the whole point of it.
--
-- `kind` is "select" for the second flavour, nil for the first
function helix.motion(fn, kind, ...)
    local doc = core.active_view.doc
    local line, col = helix.head_char(doc)
    local al, ac
    if helix.mode == "select" then
        al, ac = helix.anchor_char(doc)
    elseif kind == "select" then
        al, ac = line, col
    end
    for i = 1, helix.take_count() do
        -- unpacked into locals first: a call in any but the last
        -- argument position is truncated to one value, which would
        -- silently drop the column. a motion may also name the anchor it
        -- wants, which is how `w` skips a leading gap
        local dline, dcol, sline, scol = fn(doc, line, col, ...)
        if i == 1 and sline and not al then
            al, ac = sline, scol
        end
        line, col = dline, dcol
    end
    if not al then
        al, ac = line, col
    end
    helix.place(doc, al, ac, line, col)
end

-- ---------------------------------------------------------------- edits

-- insert mode collapses the block to a bare caret, on one side of the
-- selection or the other, and hands the keyboard back to the editor
local function enter_insert(where)
    local doc = core.active_view.doc
    local hl, hc, al, ac = doc:get_selection()
    if where == "before" then
        if hl > al or (hl == al and hc > ac) then
            hl, hc = al, ac
        end
    else
        if hl < al or (hl == al and hc < ac) then
            hl, hc = al, ac
        end
    end
    doc:set_selection(hl, hc, hl, hc)
    helix.set_mode("insert")
end

local function delete_selection()
    local doc = core.active_view.doc
    if not doc:has_selection() then
        return
    end
    -- the start of the range, taken *before* the removal: sanitize only
    -- clamps the old head, and a head one line down is still a valid
    -- position afterwards, so it would survive as a cursor in the wrong
    -- place rather than collapsing to where the text used to be
    local line, col = doc:get_selection(true)
    doc:remove(doc:get_selection())
    doc:set_selection(line, col, line, col)
end

-- ------------------------------------------------------------- drawing

-- the block is drawn after the line, over the thin caret lite already
-- drew, and the character under it is redrawn in the background colour:
-- inverse video, the way every modal editor shows a normal-mode cursor
local draw_line_body = DocView.draw_line_body

function DocView:draw_line_body(idx, x, y)
    draw_line_body(self, idx, x, y)
    if not helix.active() or helix.mode == "insert" or core.active_view ~= self then
        return
    end
    local line, col = helix.head_char(self.doc)
    if line ~= idx then
        return
    end
    local text = self.doc.lines[idx]
    local nline, ncol = self.doc:position_offset(line, col, translate.next_char)
    local x1 = x + self:get_col_x_offset(idx, col)
    -- the newline at the end of a line has no width of its own; show the
    -- block as one space so the cursor is visible past the last character
    local x2 = nline == idx and x + self:get_col_x_offset(idx, ncol)
        or x1 + self:get_font():get_width(" ")
    renderer.draw_rect(x1, y, math.max(1, x2 - x1), self:get_line_height(), style.caret)
    local ch = nline == idx and text:sub(col, ncol - 1) or nil
    if ch and ch ~= "" then
        renderer.draw_text(
            self:get_font(),
            ch,
            x1,
            y + self:get_line_text_y_offset(),
            style.background
        )
    end
end

-- the mode goes first in the status bar, where a modal editor puts it:
-- the one piece of state you cannot infer from what is on screen
local get_items = StatusView.get_items

function StatusView:get_items()
    local left, right = get_items(self)
    if helix.active() then
        table.insert(left, 1, style.accent)
        table.insert(left, 2, helix.mode)
        table.insert(left, 3, style.dim)
        table.insert(left, 4, self.separator2)
    end
    return left, right
end

-- typed text belongs to the document only in insert mode; everywhere
-- else the letters are commands and must not also arrive as text
local on_text_input = DocView.on_text_input

function DocView:on_text_input(text)
    if helix.enabled and helix.mode ~= "insert" and is_editable_view(self) then
        return
    end
    on_text_input(self, text)
end

-- ------------------------------------------------------------- commands

local function doc()
    return core.active_view.doc
end

-- j and k as ordinary motions, so a count applies to them like any other
local function vertical(step)
    return function(d, line, col)
        line = common.clamp(line + step, 1, #d.lines)
        return line, math.min(col, math.max(1, #d.lines[line] - 1))
    end
end

command.add(nil, {
    ["helix:toggle"] = function()
        if helix.enabled then
            helix.disable()
            core.log("helix mode off")
        else
            helix.enable()
            core.log("helix mode on")
        end
    end,
})

command.add(helix.active, {
    ["helix:normal-mode"] = function()
        helix.set_mode("normal")
        helix.widen(doc())
    end,

    ["helix:insert-before"] = function()
        enter_insert("before")
    end,
    ["helix:insert-after"] = function()
        enter_insert("after")
    end,
    ["helix:insert-at-line-start"] = function()
        local line = helix.head_char(doc())
        local col = doc().lines[line]:find("%S") or 1
        doc():set_selection(line, col, line, col)
        helix.set_mode("insert")
    end,
    ["helix:insert-at-line-end"] = function()
        local line = helix.head_char(doc())
        local col = #doc().lines[line]
        doc():set_selection(line, col, line, col)
        helix.set_mode("insert")
    end,

    ["helix:open-below"] = function()
        local line = helix.head_char(doc())
        doc():set_selection(line, #doc().lines[line], line, #doc().lines[line])
        command.perform("doc:newline")
        helix.set_mode("insert")
    end,
    ["helix:open-above"] = function()
        local line = helix.head_char(doc())
        doc():set_selection(line, 1, line, 1)
        command.perform("doc:newline-above")
        helix.set_mode("insert")
    end,

    ["helix:delete"] = function()
        delete_selection()
        helix.set_mode("normal")
        helix.widen(doc())
    end,

    ["helix:move-left"] = function()
        helix.motion(translate.previous_char)
    end,
    ["helix:move-right"] = function()
        helix.motion(translate.next_char)
    end,
    ["helix:move-up"] = function()
        helix.motion(vertical(-1))
    end,
    ["helix:move-down"] = function()
        helix.motion(vertical(1))
    end,

    ["helix:next-word-start"] = function()
        helix.motion(motions.next_word_start, "select", false)
    end,
    ["helix:next-word-end"] = function()
        helix.motion(motions.next_word_end, "select", false)
    end,
    ["helix:previous-word-start"] = function()
        helix.motion(motions.previous_word_start, "select", false)
    end,
    ["helix:next-long-word-start"] = function()
        helix.motion(motions.next_word_start, "select", true)
    end,
    ["helix:next-long-word-end"] = function()
        helix.motion(motions.next_word_end, "select", true)
    end,
    ["helix:previous-long-word-start"] = function()
        helix.motion(motions.previous_word_start, "select", true)
    end,

    ["helix:select-mode"] = function()
        helix.set_mode(helix.mode == "select" and "normal" or "select")
    end,

    -- `x` takes the whole line the cursor is on, and takes one more each
    -- time it is pressed -- the anchor stays at the top of the run
    ["helix:select-line"] = function()
        local d = doc()
        local hl, hc = helix.head_char(d)
        local al = helix.anchor_char(d)
        local top = math.min(al, hl)
        local bottom = math.max(al, hl)
        local n = helix.take_count()
        -- the first press squares up whatever lines the selection
        -- touches; once it already reaches a line end, each press takes
        -- one more line
        if hc >= #d.lines[bottom] and hl >= al then
            bottom = math.min(#d.lines, bottom + n)
        elseif n > 1 then
            bottom = math.min(#d.lines, bottom + n - 1)
        end
        -- the line's newline is taken too, so `x d` removes the line
        -- rather than emptying it
        helix.place(d, top, 1, bottom, #d.lines[bottom])
    end,

    -- `;` throws the selection away and keeps the cursor where it is
    ["helix:collapse-selection"] = function()
        local line, col = helix.head_char(doc())
        helix.place(doc(), line, col, line, col)
    end,

    -- `alt-;` keeps the selection and moves the cursor to its other end
    ["helix:flip-selections"] = function()
        local hl, hc, al, ac = doc():get_selection()
        doc():set_selection(al, ac, hl, hc)
    end,

    ["helix:change"] = function()
        delete_selection()
        helix.set_mode("insert")
    end,
})

-- digits before a motion build a count; `0` only continues one, so it
-- stays free for the line-start binding helix gives it
for digit = 0, 9 do
    command.add(helix.active, {
        ["helix:count-" .. digit] = function()
            if digit == 0 and not helix.count then
                return
            end
            helix.count = math.min((helix.count or 0) * 10 + digit, 10000)
        end,
    })
end

keymap.add({
    ["helix-normal:h"] = "helix:move-left",
    ["helix-normal:l"] = "helix:move-right",
    ["helix-normal:k"] = "helix:move-up",
    ["helix-normal:j"] = "helix:move-down",
    ["helix-normal:left"] = "helix:move-left",
    ["helix-normal:right"] = "helix:move-right",
    ["helix-normal:up"] = "helix:move-up",
    ["helix-normal:down"] = "helix:move-down",

    ["helix-normal:d"] = "helix:delete",
    ["helix-normal:i"] = "helix:insert-before",
    ["helix-normal:a"] = "helix:insert-after",
    ["helix-normal:shift+i"] = "helix:insert-at-line-start",
    ["helix-normal:shift+a"] = "helix:insert-at-line-end",
    ["helix-normal:o"] = "helix:open-below",
    ["helix-normal:shift+o"] = "helix:open-above",

    ["helix-normal:w"] = "helix:next-word-start",
    ["helix-normal:e"] = "helix:next-word-end",
    ["helix-normal:b"] = "helix:previous-word-start",
    ["helix-normal:shift+w"] = "helix:next-long-word-start",
    ["helix-normal:shift+e"] = "helix:next-long-word-end",
    ["helix-normal:shift+b"] = "helix:previous-long-word-start",

    ["helix-normal:v"] = "helix:select-mode",
    ["helix-normal:x"] = "helix:select-line",
    ["helix-normal:;"] = "helix:collapse-selection",
    ["helix-normal:alt+;"] = "helix:flip-selections",
    ["helix-normal:c"] = "helix:change",

    ["helix-insert:escape"] = "helix:normal-mode",
    ["helix-select:escape"] = "helix:normal-mode",
})

for digit = 0, 9 do
    keymap.add({ ["helix-normal:" .. digit] = "helix:count-" .. digit })
end

-- select mode is normal mode with every motion extending instead of
-- replacing, so it shares the bindings outright: `helix.mode` is the
-- only thing that makes them behave differently. collected first and
-- added after, since keymap.add writes to the table being walked
local extending = {}
for stroke, commands in pairs(keymap.map) do
    local key = stroke:match("^helix%-normal:(.*)$")
    if key then
        extending["helix-select:" .. key] = commands
    end
end
keymap.add(extending)

return helix

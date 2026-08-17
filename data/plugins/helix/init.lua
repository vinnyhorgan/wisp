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
local ex = require("plugins.helix.ex")
local DocView = require("core.docview")
local StatusView = require("core.statusview")

config.helix_mode = false

local helix = {}

helix.enabled = false
helix.mode = "normal"

-- a count typed before a motion (`3w`), consumed by whatever runs next
helix.count = nil

-- a one-stroke prefix: space, and later g / m / z. it names a keymap
-- mode of its own and lasts exactly one key
helix.pending = nil

-- helix yanks into its own register, not the system clipboard; `space y`
-- and `space p` are the ones that reach outside the editor. whether the
-- yank was linewise is carried alongside the text rather than sniffed
-- back out of it: the last line of a document has no position past its
-- newline, so a whole-line yank there comes back without one
helix.register = ""
helix.register_linewise = false

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
    core.redraw = true
end

-- the mode is decided fresh on every keystroke rather than stored, so a
-- prompt taking the keyboard (find, the `:` line, the command palette)
-- gets its own keys back without anything having to remember to put
-- them back afterwards
local on_key_pressed = keymap.on_key_pressed

function keymap.on_key_pressed(k)
    keymap.mode = helix.active() and ("helix-" .. (helix.pending or helix.mode)) or nil
    local handled = on_key_pressed(k)
    -- a prefix lasts one stroke, whether or not what followed meant
    -- anything; modifiers are not that stroke
    if helix.pending and not keymap.modkey_map[k] then
        helix.pending = nil
        core.redraw = true
    end
    return handled
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
    helix.pending = nil
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

-- helix selects what it pasted, so the block ends up on the new text
-- rather than back where it started
-- does the selection cover whole lines? the two shapes that mean it are
-- ending at the start of a later line, and ending at the end of the last
-- one -- which is all the end of the document allows
local function is_linewise(d)
    local l1, c1, l2, c2 = d:get_selection(true)
    if c1 ~= 1 then
        return false
    end
    return (l2 > l1 and c2 == 1) or c2 >= #d.lines[l2]
end

local function paste(after, text, linewise)
    local d = core.active_view.doc
    if text == nil then
        text, linewise = helix.register, helix.register_linewise
    end
    if text == "" then
        return
    end
    local l1, c1, l2, c2 = d:get_selection(true)
    local sl, sc
    if linewise then
        -- a linewise yank lands on a line of its own, the way it was taken
        if after then
            sl, sc = l2, #d.lines[l2]
            d:insert(sl, sc, "\n" .. text:sub(1, -2))
            sl, sc = sl + 1, 1
        else
            sl, sc = l1, 1
            d:insert(sl, sc, text)
        end
    else
        sl, sc = after and l2 or l1, after and c2 or c1
        d:insert(sl, sc, text)
    end
    local el, ec = d:position_offset(sl, sc, #text)
    helix.place(d, sl, sc, translate.previous_char(d, el, ec))
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
    local block = helix.active() and helix.mode ~= "insert" and core.active_view == self
    if block then
        -- lite draws its own thin caret at the head, and in helix the
        -- head sits one character *past* the block -- so a block on a
        -- line's newline put a second cursor at the start of the line
        -- below. park the blink for the call so only the block is drawn
        local blink = self.blink_timer
        self.blink_timer = math.huge
        draw_line_body(self, idx, x, y)
        self.blink_timer = blink
    else
        draw_line_body(self, idx, x, y)
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
        table.insert(left, 2, helix.pending and (helix.mode .. " " .. helix.pending) or helix.mode)
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

    -- undo restores whatever selection the edit was made with, which may
    -- be a bare caret; the block invariant has to be put back
    ["helix:undo"] = function()
        for _ = 1, helix.take_count() do
            doc():undo()
        end
        helix.widen(doc())
    end,
    ["helix:redo"] = function()
        for _ = 1, helix.take_count() do
            doc():redo()
        end
        helix.widen(doc())
    end,

    -- helix yanks into a register of its own. the system clipboard is
    -- deliberately a different key (`space y`), so copying in the editor
    -- does not trample what you copied out of a browser
    ["helix:yank"] = function()
        local d = doc()
        helix.register_linewise = is_linewise(d)
        helix.register = d:get_text(d:get_selection())
        if helix.register_linewise and not helix.register:find("\n$") then
            helix.register = helix.register .. "\n"
        end
        core.log("yanked %d characters", #helix.register)
    end,
    ["helix:paste-after"] = function()
        paste(true)
    end,
    ["helix:paste-before"] = function()
        paste(false)
    end,
    ["helix:yank-to-clipboard"] = function()
        system.set_clipboard(doc():get_text(doc():get_selection()))
        core.log("yanked to the system clipboard")
    end,
    ["helix:paste-from-clipboard"] = function()
        -- nothing outside carries a linewise flag, so it is read back
        -- off the text the way every other editor does
        local text = (system.get_clipboard() or ""):gsub("\r", "")
        paste(true, text, text:find("\n$") ~= nil)
    end,

    -- searching is the host's, not the model's: wisp already has a find
    -- prompt and a repeat, and helix's keys are wired onto them
    ["helix:search"] = function()
        command.perform("find-replace:find")
    end,
    ["helix:search-next"] = function()
        for _ = 1, helix.take_count() do
            command.perform("find-replace:repeat-find")
        end
        helix.widen(doc())
    end,
    ["helix:search-previous"] = function()
        for _ = 1, helix.take_count() do
            command.perform("find-replace:previous-find")
        end
        helix.widen(doc())
    end,

    ["helix:ex"] = function()
        ex.enter()
    end,
    ["helix:space-mode"] = function()
        helix.pending = "space"
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

    ["helix-normal:u"] = "helix:undo",
    ["helix-normal:shift+u"] = "helix:redo",
    ["helix-normal:y"] = "helix:yank",
    ["helix-normal:p"] = "helix:paste-after",
    ["helix-normal:shift+p"] = "helix:paste-before",
    ["helix-normal:/"] = "helix:search",
    ["helix-normal:n"] = "helix:search-next",
    ["helix-normal:shift+n"] = "helix:search-previous",
    ["helix-normal:;"] = "helix:collapse-selection",
    ["helix-normal:space"] = "helix:space-mode",

    ["helix-insert:escape"] = "helix:normal-mode",
    ["helix-select:escape"] = "helix:normal-mode",
})

-- `:` is not a motion and must not be mirrored into select mode's map
-- as one, so it is added on its own alongside the space menu
keymap.add({
    ["helix-normal:shift+;"] = "helix:ex",
    ["helix-select:shift+;"] = "helix:ex",

    -- the space menu is where helix talks to the editor around it rather
    -- than to the buffer, so these follow zed's lead and route straight
    -- into wisp's own commands
    ["helix-space:f"] = "core:find-file",
    ["helix-space:shift+f"] = "core:open-file",
    ["helix-space:/"] = "project-search:find",
    ["helix-space:y"] = "helix:yank-to-clipboard",
    ["helix-space:p"] = "helix:paste-from-clipboard",
    ["helix-space:k"] = "core:find-command",
    ["helix-space:e"] = "treeview:toggle",
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

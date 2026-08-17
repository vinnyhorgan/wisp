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
local surround = require("plugins.helix.surround")
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

-- some keys take a literal character as their argument -- `f`, `r`, and
-- the whole of match mode. while one is waiting, the keyboard belongs to
-- it: the keystroke is swallowed and the character arrives as text input
helix.awaiting = nil
helix.awaiting_label = nil

-- `.` repeats the last insertion and `alt-.` the last `f`/`t`, so both
-- are remembered as they happen
helix.insertion = nil
helix.last_insertion = nil
helix.last_find = nil

-- `*` puts the selection into helix's own search register. while it is
-- empty, `n` and `N` repeat the host's find prompt instead, so the two
-- ways of searching do not each need their own history
helix.search_text = nil

-- the jumplist: `ctrl-s` parks the selection, `ctrl-o` walks back
-- through the parked ones and `ctrl-i` forward again
helix.jumps = {}
helix.jump_index = 1

function helix.take_count()
    local n = helix.count or 1
    helix.count = nil
    return n
end

-- hand the next typed character to `fn`. `label` is what the status bar
-- shows while the editor is waiting for it
function helix.await(label, fn)
    helix.awaiting = fn
    helix.awaiting_label = label
    core.redraw = true
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
    -- `.` repeats the last insertion, so an insert session opens a fresh
    -- recording and closes it on the way out
    if mode == "insert" then
        helix.insertion = ""
    elseif helix.mode == "insert" then
        helix.last_insertion = helix.insertion or ""
    end
    helix.mode = mode
    core.redraw = true
end

-- the mode is decided fresh on every keystroke rather than stored, so a
-- prompt taking the keyboard (find, the `:` line, the command palette)
-- gets its own keys back without anything having to remember to put
-- them back afterwards
table.insert(keymap.modes, function()
    return helix.active() and ("helix-" .. (helix.pending or helix.mode)) or nil
end)

local on_key_pressed = keymap.on_key_pressed

function keymap.on_key_pressed(k)
    -- a key waiting for its character argument owns the keyboard: the
    -- stroke runs no command, so `f` `w` does not also walk a word, and
    -- the character itself arrives a moment later as text input.
    --
    -- false, not true: core drops the text event that follows a keystroke
    -- the keymap claimed, and that text event is the whole point of this
    if helix.awaiting and helix.active() and not keymap.modkey_map[k] then
        if k == "escape" then
            helix.awaiting = nil
            core.redraw = true
        end
        return false
    end
    -- read before the stroke runs: `space` sets the prefix from inside
    -- this very call, and clearing it afterwards would end it before the
    -- key it exists to qualify was ever pressed
    local pending = helix.pending
    local handled = on_key_pressed(k)
    -- a prefix lasts one stroke, whether or not what followed meant
    -- anything; modifiers are not that stroke
    if pending and helix.pending == pending and not keymap.modkey_map[k] then
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
    helix.awaiting = nil
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
    local extending = helix.mode == "select"
    if extending then
        al, ac = helix.anchor_char(doc)
    end
    for i = 1, helix.take_count() do
        -- unpacked into locals first: a call in any but the last
        -- argument position is truncated to one value, which would
        -- silently drop the column
        local dline, dcol, sline, scol = fn(doc, line, col, ...)
        -- a motion may name where the selection should start, and it
        -- wins: `w` moves its own anchor to skip a gap or a token the
        -- cursor had already finished. select mode ignores all of that,
        -- since keeping the anchor is the whole point of it
        if i == 1 and not extending then
            if sline then
                al, ac = sline, scol
            elseif kind == "select" then
                al, ac = line, col
            end
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
        local qualifier = helix.awaiting_label or helix.pending
        table.insert(left, 1, style.accent)
        table.insert(left, 2, qualifier and (helix.mode .. " " .. qualifier) or helix.mode)
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
        local fn = helix.awaiting
        if fn then
            -- cleared first: the callback is free to ask for another
            -- character of its own, which `mr` does
            helix.awaiting, helix.awaiting_label = nil, nil
            core.try(fn, text)
            core.redraw = true
        end
        return
    end
    -- `.` repeats the last insertion, so what is typed is remembered as
    -- it goes. only text: a backspace is not part of what was inserted
    if helix.enabled and helix.mode == "insert" and is_editable_view(self) then
        helix.insertion = (helix.insertion or "") .. text
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

-- `f` / `F` / `t` / `T` all wait for the character they are looking for.
-- a count typed before them survives the wait untouched: the keystroke
-- carrying that character never reaches a command
function helix.find_char(forward, till)
    local label = forward and (till and "t" or "f") or (till and "T" or "F")
    helix.await(label, function(char)
        helix.last_find = { char = char, forward = forward, till = till }
        helix.motion(motions.find_char, "select", char, forward, till)
    end)
end

-- `zt` / `zb`: put the cursor's line at the top or the bottom of the
-- view. `zz` is core's own `scroll_to_line`, which already centres
function helix.scroll_line(fraction)
    local view = core.active_view
    local lh = view:get_line_height()
    local line = helix.head_char(view.doc)
    view.scroll.to.y = math.max(0, lh * (line - 1) - (view.size.y - lh) * fraction)
    view.scroll.y = view.scroll.to.y
end

-- `g`'s destinations, all of them plain moves: the block lands there and
-- the selection collapses behind it, unless select mode is extending.
-- a negative column means the end of the line, which is not a number the
-- caller can know
local function goto_at(line, col)
    helix.count = nil
    helix.motion(function(d)
        local l = common.clamp(line, 1, #d.lines)
        -- every line ends in a newline, and that newline is not a place
        -- the block can sit
        local last = math.max(1, #d.lines[l] - 1)
        return l, common.clamp(col < 0 and last or col, 1, last)
    end)
end

local function line_of_cursor()
    return (helix.head_char(doc()))
end

-- `ctrl-a` / `ctrl-x`: the number under the cursor, or the next one on
-- the line, moved by the count. the block ends up on the new number,
-- which is how helix shows you what it changed
local function bump(by)
    local d = doc()
    local line, col = helix.head_char(d)
    local text = d.lines[line]
    local from = 1
    while true do
        local s, e = text:find("%-?%d+", from)
        if not s then
            return
        end
        if e >= col then
            local n = math.tointeger(tonumber(text:sub(s, e)))
            if not n then
                return
            end
            local new = tostring(n + by)
            d:remove(line, s, line, e + 1)
            d:insert(line, s, new)
            helix.place(d, line, s, line, s + #new - 1)
            return
        end
        from = e + 1
    end
end

-- every match of the search register, in document order. `n` and `N`
-- then pick the neighbour of the cursor, which is the only way to walk
-- backwards over a search: core's own find only scans forward
local function matches(d, text)
    local res = {}
    for line = 1, #d.lines do
        local from = 1
        while true do
            local s, e = d.lines[line]:find(text, from, true)
            if not s then
                break
            end
            table.insert(res, { s, e + 1, line = line })
            from = s + 1
        end
    end
    return res
end

local function search_step(backward)
    local d = doc()
    local text = helix.search_text
    if not text or text == "" then
        -- nothing has been `*`-ed, so the host's find prompt still owns
        -- what "the last search" means
        command.perform(backward and "find-replace:previous-find" or "find-replace:repeat-find")
        helix.widen(d)
        return
    end
    local all = matches(d, text)
    if #all == 0 then
        core.error("couldn't find %q", text)
        return
    end
    local line, col = helix.head_char(d)
    local pick
    if backward then
        for i = #all, 1, -1 do
            local m = all[i]
            if m.line < line or (m.line == line and m[1] < col) then
                pick = m
                break
            end
        end
        pick = pick or all[#all]
    else
        for _, m in ipairs(all) do
            if m.line > line or (m.line == line and m[1] > col) then
                pick = m
                break
            end
        end
        pick = pick or all[1]
    end
    helix.place(d, pick.line, pick[1], translate.previous_char(d, pick.line, pick[2]))
    core.active_view:scroll_to_line(pick.line, true)
end

-- the jumplist. entries carry their document, and a jump into one that
-- is no longer on screen is not one this list can honour: reopening is
-- the host's business, not the editing model's
local function push_jump()
    local d = doc()
    local hl, hc, al, ac = d:get_selection()
    for i = #helix.jumps, helix.jump_index, -1 do
        table.remove(helix.jumps, i)
    end
    table.insert(helix.jumps, { doc = d, hl = hl, hc = hc, al = al, ac = ac })
    helix.jump_index = #helix.jumps + 1
end

local function goto_jump(step)
    local i = helix.jump_index + step
    local j = helix.jumps[i]
    if not j or j.doc ~= doc() then
        return
    end
    helix.jump_index = i
    j.doc:set_selection(j.hl, j.hc, j.al, j.ac)
    helix.widen(j.doc)
    core.active_view:scroll_to_line(j.hl, true)
end

-- match mode's text objects. `inner` drops the two delimiters; the
-- bracket kinds are the ones helix falls back on without a parser, plus
-- `w`, `W` and `p`, which need none
local function select_object(char, inner)
    local d = doc()
    local line, col = helix.head_char(d)
    if char == "w" or char == "W" then
        local sl, sc, el, ec = motions.word_range(d, line, col, char == "W")
        helix.place(d, sl, sc, el, ec)
        return
    end
    if char == "p" then
        local first, last = motions.paragraph_range(d, line)
        helix.place(d, first, 1, last, math.max(1, #d.lines[last] - 1))
        return
    end
    local ol, oc, cl, cc = surround.find_pair(d, line, col, char)
    if not ol then
        core.error("no surrounding %s", char)
        return
    end
    if not inner then
        helix.place(d, ol, oc, cl, cc)
        return
    end
    local sl, sc = translate.next_char(d, ol, oc)
    local el, ec = translate.previous_char(d, cl, cc)
    -- an empty pair has nothing inside; the delimiters themselves are
    -- the closest thing to an answer
    if sl > el or (sl == el and sc > ec) then
        helix.place(d, ol, oc, cl, cc)
        return
    end
    helix.place(d, sl, sc, el, ec)
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
        -- typing a new search hands "the last search" back to the host's
        -- prompt, which is the one that just took it
        helix.search_text = nil
        push_jump()
        command.perform("find-replace:find")
    end,
    ["helix:search-next"] = function()
        for _ = 1, helix.take_count() do
            search_step(false)
        end
    end,
    ["helix:search-previous"] = function()
        for _ = 1, helix.take_count() do
            search_step(true)
        end
    end,

    -- `%` takes the whole file, newline at the end and all
    ["helix:select-all"] = function()
        local d = doc()
        -- exactly the span lite's own select-all takes. a document always
        -- ends in a newline and there is no position past it, so `%d`
        -- leaves that last newline behind, the way ctrl+a delete does
        helix.place(d, 1, 1, #d.lines, #d.lines[#d.lines])
    end,

    ["helix:search-selection"] = function()
        helix.search_text = doc():get_text(doc():get_selection())
        core.log("searching for %q", helix.search_text)
    end,

    ["helix:ex"] = function()
        ex.enter()
    end,
    ["helix:space-mode"] = function()
        helix.pending = "space"
    end,
    ["helix:goto-mode"] = function()
        -- the count is deliberately not taken: `10gg` is helix's
        -- go-to-line, so the digits have to survive the prefix
        helix.pending = "goto"
    end,
    ["helix:view-mode"] = function()
        helix.pending = "view"
    end,
    ["helix:match-mode"] = function()
        helix.pending = "match"
    end,

    -- ------------------------------------------------------------ goto

    ["helix:goto-file-start"] = function()
        push_jump()
        goto_at(helix.count or 1, 1)
    end,
    ["helix:goto-file-end"] = function()
        push_jump()
        goto_at(#doc().lines, -1)
    end,
    ["helix:goto-line-start"] = function()
        goto_at(line_of_cursor(), 1)
    end,
    ["helix:goto-line-end"] = function()
        goto_at(line_of_cursor(), -1)
    end,
    ["helix:goto-first-nonblank"] = function()
        local line = line_of_cursor()
        goto_at(line, doc().lines[line]:find("%S") or 1)
    end,

    -- ------------------------------------------------------------ view

    ["helix:center-line"] = function()
        core.active_view:scroll_to_line(line_of_cursor(), false, true)
    end,
    ["helix:line-to-top"] = function()
        helix.scroll_line(0)
    end,
    ["helix:line-to-bottom"] = function()
        helix.scroll_line(1)
    end,

    -- ------------------------------------------------------ find a char

    ["helix:find-next-char"] = function()
        helix.find_char(true, false)
    end,
    ["helix:find-previous-char"] = function()
        helix.find_char(false, false)
    end,
    ["helix:till-next-char"] = function()
        helix.find_char(true, true)
    end,
    ["helix:till-previous-char"] = function()
        helix.find_char(false, true)
    end,
    ["helix:repeat-last-find"] = function()
        local f = helix.last_find
        if not f then
            return
        end
        helix.motion(motions.find_char, "select", f.char, f.forward, f.till)
    end,

    -- ----------------------------------------------------------- edits

    ["helix:replace"] = function()
        helix.await("r", function(char)
            local d = doc()
            -- newlines survive: `r` changes characters, it does not
            -- glue lines together
            -- a function, not a replacement string: `%` in the typed
            -- character would otherwise be read as a capture reference
            d:replace(function(text)
                return (
                    text:gsub("[^\n]", function()
                        return char
                    end)
                )
            end)
            helix.widen(d)
        end)
    end,

    ["helix:repeat-insert"] = function()
        local text = helix.last_insertion
        if not text or text == "" then
            return
        end
        local d = doc()
        -- where `i` would have started: the front of the selection
        local line, col = d:get_selection(true)
        d:insert(line, col, text)
        local el, ec = d:position_offset(line, col, #text)
        helix.place(d, line, col, translate.previous_char(d, el, ec))
    end,

    ["helix:replace-with-yanked"] = function()
        if helix.register == "" then
            return
        end
        delete_selection()
        paste(false, helix.register, helix.register_linewise)
    end,

    ["helix:join-lines"] = function()
        for _ = 1, helix.take_count() do
            command.perform("doc:join-lines")
        end
        helix.widen(doc())
    end,
    ["helix:indent"] = function()
        for _ = 1, helix.take_count() do
            command.perform("doc:indent")
        end
        helix.widen(doc())
    end,
    ["helix:unindent"] = function()
        for _ = 1, helix.take_count() do
            command.perform("doc:unindent")
        end
        helix.widen(doc())
    end,
    ["helix:toggle-comments"] = function()
        command.perform("doc:toggle-line-comments")
        helix.widen(doc())
    end,

    ["helix:increment"] = function()
        bump(helix.take_count())
    end,
    ["helix:decrement"] = function()
        bump(-helix.take_count())
    end,

    ["helix:switch-case"] = function()
        doc():replace(function(text)
            return (
                text:gsub("%a", function(c)
                    return c:find("%l") and c:upper() or c:lower()
                end)
            )
        end)
        helix.widen(doc())
    end,
    ["helix:to-lowercase"] = function()
        doc():replace(string.lower)
        helix.widen(doc())
    end,
    ["helix:to-uppercase"] = function()
        doc():replace(string.upper)
        helix.widen(doc())
    end,

    -- ------------------------------------------------------- match mode

    ["helix:match-bracket"] = function()
        local d = doc()
        local line, col = helix.head_char(d)
        local ml, mc = surround.match_at(d, line, col)
        if not ml then
            -- not standing on a bracket: go to the closer of whatever
            -- pair the cursor is inside, which is what helix does
            local _, _, cl, cc = surround.nearest_pair(d, line, col)
            if not cl then
                return
            end
            ml, mc = cl, cc
        end
        goto_at(ml, mc)
    end,
    ["helix:select-inside"] = function()
        helix.await("mi", function(char)
            select_object(char, true)
        end)
    end,
    ["helix:select-around"] = function()
        helix.await("ma", function(char)
            select_object(char, false)
        end)
    end,
    ["helix:surround-add"] = function()
        helix.await("ms", function(char)
            local d = doc()
            local open = surround.closing[char] or char
            local close = surround.opening[open] or (surround.quotes[open] and open)
            if not close then
                core.error("%q is not a pair", char)
                return
            end
            local l1, c1, l2, c2 = d:get_selection(true)
            -- the far end first, so inserting at the near one does not
            -- move the position the far one was measured at
            d:insert(l2, c2, close)
            d:insert(l1, c1, open)
            -- the closer moved one along only if the opener went in on
            -- the same line as it
            helix.place(d, l1, c1, l2, c2 + (l1 == l2 and 1 or 0))
        end)
    end,
    ["helix:surround-delete"] = function()
        helix.await("md", function(char)
            local d = doc()
            local line, col = helix.head_char(d)
            local ol, oc, cl, cc = surround.find_pair(d, line, col, char)
            if not ol then
                core.error("no surrounding %s", char)
                return
            end
            d:remove(cl, cc, cl, cc + 1)
            d:remove(ol, oc, ol, oc + 1)
            helix.widen(d)
        end)
    end,
    ["helix:surround-replace"] = function()
        helix.await("mr", function(from)
            helix.await("mr" .. from, function(to)
                local d = doc()
                local line, col = helix.head_char(d)
                local ol, oc, cl, cc = surround.find_pair(d, line, col, from)
                if not ol then
                    core.error("no surrounding %s", from)
                    return
                end
                local open = surround.closing[to] or to
                local close = surround.opening[open] or (surround.quotes[open] and open)
                if not close then
                    core.error("%q is not a pair", to)
                    return
                end
                d:remove(cl, cc, cl, cc + 1)
                d:insert(cl, cc, close)
                d:remove(ol, oc, ol, oc + 1)
                d:insert(ol, oc, open)
                helix.widen(d)
            end)
        end)
    end,

    -- ------------------------------------------------------- jumplist

    ["helix:save-selection"] = function()
        push_jump()
        core.log("selection saved to the jumplist")
    end,
    ["helix:jump-backward"] = function()
        goto_jump(-1)
    end,
    ["helix:jump-forward"] = function()
        goto_jump(1)
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
    ["helix-normal:shift+8"] = "helix:search-selection",
    ["helix-normal:shift+5"] = "helix:select-all",
    ["helix-normal:space"] = "helix:space-mode",
    ["helix-normal:g"] = "helix:goto-mode",
    ["helix-normal:z"] = "helix:view-mode",
    ["helix-normal:m"] = "helix:match-mode",

    ["helix-normal:f"] = "helix:find-next-char",
    ["helix-normal:shift+f"] = "helix:find-previous-char",
    ["helix-normal:t"] = "helix:till-next-char",
    ["helix-normal:shift+t"] = "helix:till-previous-char",
    ["helix-normal:alt+."] = "helix:repeat-last-find",

    ["helix-normal:r"] = "helix:replace",
    ["helix-normal:."] = "helix:repeat-insert",
    ["helix-normal:shift+r"] = "helix:replace-with-yanked",
    ["helix-normal:shift+j"] = "helix:join-lines",
    ["helix-normal:shift+."] = "helix:indent",
    ["helix-normal:shift+,"] = "helix:unindent",
    ["helix-normal:ctrl+c"] = "helix:toggle-comments",
    ["helix-normal:ctrl+a"] = "helix:increment",
    ["helix-normal:ctrl+x"] = "helix:decrement",
    ["helix-normal:shift+`"] = "helix:switch-case",
    ["helix-normal:`"] = "helix:to-lowercase",
    ["helix-normal:alt+`"] = "helix:to-uppercase",

    -- helix parks the selection on ctrl+s, but ctrl+s means save
    -- everywhere else on the planet and losing that is not worth
    -- the fidelity: the jumplist gets the shifted stroke instead
    ["helix-normal:ctrl+shift+s"] = "helix:save-selection",
    ["helix-normal:ctrl+o"] = "helix:jump-backward",
    ["helix-normal:ctrl+i"] = "helix:jump-forward",
    ["helix-normal:shift+q"] = "macro:toggle-record",
    ["helix-normal:q"] = "macro:play",

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

    ["helix-goto:g"] = "helix:goto-file-start",
    ["helix-goto:e"] = "helix:goto-file-end",
    ["helix-goto:h"] = "helix:goto-line-start",
    ["helix-goto:l"] = "helix:goto-line-end",
    ["helix-goto:s"] = "helix:goto-first-nonblank",
    ["helix-goto:n"] = "root:switch-to-next-tab",
    ["helix-goto:p"] = "root:switch-to-previous-tab",

    ["helix-view:z"] = "helix:center-line",
    ["helix-view:c"] = "helix:center-line",
    ["helix-view:t"] = "helix:line-to-top",
    ["helix-view:b"] = "helix:line-to-bottom",

    ["helix-match:m"] = "helix:match-bracket",
    ["helix-match:i"] = "helix:select-inside",
    ["helix-match:a"] = "helix:select-around",
    ["helix-match:s"] = "helix:surround-add",
    ["helix-match:d"] = "helix:surround-delete",
    ["helix-match:r"] = "helix:surround-replace",
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

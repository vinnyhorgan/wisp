local core = require("core")
local command = require("core.command")

local function doc()
    return core.active_view.doc
end

-- copy and cut with nothing selected take the whole line, and a paste
-- of such a line lands above the caret rather than inside it (lite
-- pr #209).
--
-- upstream tracks "the clipboard holds a line" in a boolean, which goes
-- stale the moment you copy something in another application: the flag
-- still says line, and the next paste opens a new line for text that
-- never was one. wisp remembers the *text* it put there and compares,
-- so anything else in the clipboard pastes normally
local line_in_clipboard = nil

local function take_line()
    local line = doc():get_selection()
    local text = doc().lines[line]
    system.set_clipboard(text)
    line_in_clipboard = text
    return line
end

local function clipboard_holds_a_line()
    return line_in_clipboard and system.get_clipboard() == line_in_clipboard
end

local doc_copy = command.map["doc:copy"].perform
command.map["doc:copy"].perform = function()
    if doc():has_selection() then
        line_in_clipboard = nil
        doc_copy()
    else
        take_line()
    end
end

local doc_cut = command.map["doc:cut"].perform
command.map["doc:cut"].perform = function()
    if doc():has_selection() then
        line_in_clipboard = nil
        doc_cut()
    else
        local line = take_line()
        -- the last line has no line after it to remove up to; removing
        -- to the end of this one leaves the document with the empty
        -- line a document must always have
        if line < #doc().lines then
            doc():remove(line, 1, line + 1, 1)
        else
            doc():remove(line, 1, line, math.huge)
        end
        doc():set_selection(math.min(line, #doc().lines), 1)
    end
end

local doc_paste = command.map["doc:paste"].perform
command.map["doc:paste"].perform = function()
    if not clipboard_holds_a_line() then
        doc_paste()
    else
        local line, col = doc():get_selection()
        doc():insert(line, 1, system.get_clipboard():gsub("\r", ""))
        doc():set_selection(line + 1, col)
    end
end

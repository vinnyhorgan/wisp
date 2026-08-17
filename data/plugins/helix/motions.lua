-- helix's word motions.
--
-- lite's translate.lua splits the world in two -- word characters and
-- `config.non_word_chars` -- which is all its own motions need. helix
-- needs three classes, because `w` stops at every change of class: the
-- tutor's own test is that `one-of-a-kind` takes seven `w` presses (word,
-- punctuation, word, ...) while `W` takes one.
--
-- every function here speaks in block positions -- the character the
-- cursor sits *on* -- and returns the character the head should land on,
-- which is what a helix motion means by "select to here".

local translate = require("core.doc.translate")

local motions = {}

local function class_of(char, big)
    if char == "" or char == "\n" or char:find("^%s") then
        return "space"
    end
    -- a WORD is anything that is not whitespace, so the two non-space
    -- classes collapse into one
    if big or char:find("^[%w_]") or char:byte() > 127 then
        return "word"
    end
    return "punct"
end

local function at(doc, line, col)
    return doc:get_char(line, col)
end

-- one step forward, or nil at the end of the document
local function fwd(doc, line, col)
    local l, c = translate.next_char(doc, line, col)
    if l == line and c == col then
        return nil
    end
    return l, c
end

-- one step back, or nil at the start
local function back(doc, line, col)
    local l, c = translate.previous_char(doc, line, col)
    if l == line and c == col then
        return nil
    end
    return l, c
end

--- `w` / `W`: forward to the end of the next token, taking any gap that
--- follows it. returns the anchor as well as the head, because where the
--- selection *starts* is not always where the cursor was: a cursor
--- sitting in a gap belongs to the word after it, and a cursor already
--- on the last character of its token belongs to the token after that
function motions.next_word_start(doc, line, col, big)
    local sl, sc = line, col
    local here = class_of(at(doc, sl, sc), big)
    -- the token under the cursor is finished when the next character
    -- changes class. without this, a one-character token like `(` never
    -- lets the cursor past it: the walk below ends where it started and
    -- there is no gap to cross
    if here ~= "space" then
        local nl, nc = fwd(doc, sl, sc)
        if not nl then
            return sl, sc, sl, sc
        end
        if class_of(at(doc, nl, nc), big) ~= here then
            sl, sc = nl, nc
            here = class_of(at(doc, sl, sc), big)
        end
    end
    -- a cursor in the gap belongs to the word after it, which is what
    -- makes pressing `w` twice walk two words
    while here == "space" do
        local nl, nc = fwd(doc, sl, sc)
        if not nl then
            break
        end
        sl, sc = nl, nc
        here = class_of(at(doc, sl, sc), big)
    end
    -- run to the end of that token
    local pl, pc = sl, sc
    local l, c = fwd(doc, sl, sc)
    while l and class_of(at(doc, l, c), big) == here do
        pl, pc = l, c
        l, c = fwd(doc, l, c)
    end
    -- and take the gap after it, stopping before the next token starts
    while l and class_of(at(doc, l, c), big) == "space" do
        pl, pc = l, c
        l, c = fwd(doc, l, c)
    end
    return pl, pc, sl, sc
end

--- `e` / `E`: forward to the last character of the next word
function motions.next_word_end(doc, line, col, big)
    local l, c = fwd(doc, line, col)
    if not l then
        return line, col
    end
    -- skip any gap first, so pressing `e` twice moves on
    while l and class_of(at(doc, l, c), big) == "space" do
        local nl, nc = fwd(doc, l, c)
        if not nl then
            return l, c
        end
        l, c = nl, nc
    end
    local kind = class_of(at(doc, l, c), big)
    local pl, pc = l, c
    l, c = fwd(doc, l, c)
    while l and class_of(at(doc, l, c), big) == kind do
        pl, pc = l, c
        l, c = fwd(doc, l, c)
    end
    return pl, pc
end

--- `f` / `F` / `t` / `T`: to the next occurrence of a character on this
--- line, or onto the character beside it for the `till` flavours. helix
--- stays put when there is none, so a mistyped `f` costs nothing
function motions.find_char(doc, line, col, char, forward, till)
    local text = doc.lines[line]
    if forward then
        local from = col + 1
        -- a `t` already parked in front of its target has to look past
        -- that one, or pressing it again would never move
        if till and text:sub(from, from) == char then
            from = from + 1
        end
        local at = text:find(char, from, true)
        if not at then
            return line, col
        end
        return line, till and at - 1 or at
    end
    -- backward: the last match before the cursor, found by walking
    -- forward, since patterns only search one way
    local limit = col
    if till and text:sub(col - 1, col - 1) == char then
        limit = col - 1
    end
    local from, at = 1, nil
    while true do
        local found = text:find(char, from, true)
        if not found or found >= limit then
            break
        end
        at = found
        from = found + 1
    end
    if not at then
        return line, col
    end
    return line, till and at + 1 or at
end

--- the run of same-class characters the cursor stands in -- what `miw`
--- means by "this word". words do not cross lines, so neither does this
function motions.word_range(doc, line, col, big)
    local kind = class_of(at(doc, line, col), big)
    local sl, sc = line, col
    while true do
        local l, c = back(doc, sl, sc)
        if not l or l ~= sl or class_of(at(doc, l, c), big) ~= kind then
            break
        end
        sl, sc = l, c
    end
    local el, ec = line, col
    while true do
        local l, c = fwd(doc, el, ec)
        if not l or l ~= el or class_of(at(doc, l, c), big) ~= kind then
            break
        end
        el, ec = l, c
    end
    return sl, sc, el, ec
end

--- the run of non-blank lines the cursor stands in -- `mip`. returns
--- line numbers, since a paragraph is only ever whole lines
function motions.paragraph_range(doc, line)
    local first, last = line, line
    while first > 1 and not doc.lines[first - 1]:find("^%s*$") do
        first = first - 1
    end
    while last < #doc.lines and not doc.lines[last + 1]:find("^%s*$") do
        last = last + 1
    end
    return first, last
end

--- `b` / `B`: backward to the first character of the previous word
function motions.previous_word_start(doc, line, col, big)
    local l, c = back(doc, line, col)
    if not l then
        return line, col
    end
    while l and class_of(at(doc, l, c), big) == "space" do
        local pl, pc = back(doc, l, c)
        if not pl then
            return l, c
        end
        l, c = pl, pc
    end
    local kind = class_of(at(doc, l, c), big)
    local pl, pc = l, c
    l, c = back(doc, l, c)
    while l and class_of(at(doc, l, c), big) == kind do
        pl, pc = l, c
        l, c = back(doc, l, c)
    end
    return pl, pc
end

return motions

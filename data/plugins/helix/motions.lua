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

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

--- `w` / `W`: forward to just before the beginning of the next word.
--- returns the anchor as well as the head: a cursor sitting in the gap
--- between two words belongs to the word *after* it, which is what makes
--- pressing `w` twice walk two words instead of selecting a gap on its own
function motions.next_word_start(doc, line, col, big)
    local sl, sc = line, col
    while class_of(at(doc, sl, sc), big) == "space" do
        local nl, nc = fwd(doc, sl, sc)
        if not nl then
            break
        end
        sl, sc = nl, nc
    end
    local start = class_of(at(doc, sl, sc), big)
    local pl, pc = sl, sc
    local l, c = fwd(doc, sl, sc)
    -- leave the token the cursor is standing in
    while l and class_of(at(doc, l, c), big) == start do
        pl, pc = l, c
        l, c = fwd(doc, l, c)
    end
    -- then cross the gap, stopping on the last character before the
    -- next word rather than on the word itself
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

-- helix's match mode: brackets, and the text between them.
--
-- `mm` walks to the other end of a pair, `mi` and `ma` select what a
-- pair holds, and `ms` / `md` / `mr` add, remove and swap the pair
-- itself. all of it rests on one operation -- find the pair that
-- surrounds a position -- so that is what this file is.
--
-- the scan is character-by-character over the document rather than a
-- parse: it counts nesting and nothing else, which is what helix's own
-- non-treesitter fallback does and is right far more often than it is
-- wrong.

local translate = require("core.doc.translate")

local surround = {}

surround.opening = { ["("] = ")", ["["] = "]", ["{"] = "}", ["<"] = ">" }
surround.closing = { [")"] = "(", ["]"] = "[", ["}"] = "{", [">"] = "<" }
surround.quotes = { ['"'] = true, ["'"] = true, ["`"] = true }

local function fwd(doc, line, col)
    local l, c = translate.next_char(doc, line, col)
    if l == line and c == col then
        return nil
    end
    return l, c
end

local function back(doc, line, col)
    local l, c = translate.previous_char(doc, line, col)
    if l == line and c == col then
        return nil
    end
    return l, c
end

-- walk out to the first unmatched `target`, counting the nested pairs
-- on the way. `other` is the character that opens a nesting level;
-- `dir` is 1 forward or -1 back
local function scan(doc, line, col, target, other, dir)
    local step = dir > 0 and fwd or back
    local depth = 0
    local l, c = line, col
    while true do
        l, c = step(doc, l, c)
        if not l then
            return nil
        end
        local x = doc:get_char(l, c)
        if x == other then
            depth = depth + 1
        elseif x == target then
            if depth == 0 then
                return l, c
            end
            depth = depth - 1
        end
    end
end

-- quotes have no direction, so nesting cannot be counted: they are
-- paired off along the line in the order they appear, first with second,
-- third with fourth. that is what quoting means on a single line
local function quote_pair(doc, line, col, char)
    local found = {}
    local from = 1
    while true do
        local p = doc.lines[line]:find(char, from, true)
        if not p then
            break
        end
        table.insert(found, p)
        from = p + 1
    end
    for i = 1, #found - 1, 2 do
        if col >= found[i] and col <= found[i + 1] then
            return line, found[i], line, found[i + 1]
        end
    end
    return nil
end

--- `mm`: the other end of the pair the character under the cursor is
--- half of, or nil when it is not half of one
function surround.match_at(doc, line, col)
    local ch = doc:get_char(line, col)
    local close = surround.opening[ch]
    if close then
        return scan(doc, line, col, close, ch, 1)
    end
    local open = surround.closing[ch]
    if open then
        return scan(doc, line, col, open, ch, -1)
    end
    return nil
end

--- the innermost pair of `char`'s kind surrounding the cursor, as the
--- four positions of its two characters. either side of a pair names it,
--- so `mi(` and `mi)` mean the same thing, exactly as in helix
function surround.find_pair(doc, line, col, char)
    if surround.quotes[char] then
        return quote_pair(doc, line, col, char)
    end
    local open = surround.closing[char] or char
    local close = surround.opening[open]
    if not close then
        return nil
    end
    local ol, oc
    -- a cursor sitting on the opener is already inside its pair
    if doc:get_char(line, col) == open then
        ol, oc = line, col
    else
        ol, oc = scan(doc, line, col, open, close, -1)
    end
    if not ol then
        return nil
    end
    local cl, cc = scan(doc, ol, oc, close, open, 1)
    if not cl then
        return nil
    end
    return ol, oc, cl, cc
end

--- the nearest pair of any kind surrounding the cursor, used by `mm`
--- when the cursor is not standing on a bracket itself. the closest
--- opener wins, which is the innermost pair
function surround.nearest_pair(doc, line, col)
    local best
    for open in pairs(surround.opening) do
        local ol, oc, cl, cc = surround.find_pair(doc, line, col, open)
        if ol and (not best or ol > best[1] or (ol == best[1] and oc > best[2])) then
            best = { ol, oc, cl, cc }
        end
    end
    if best then
        return table.unpack(best)
    end
    return nil
end

return surround

-- the house rules for every file wisp writes: utf-8, lf line endings,
-- and exactly one newline at the end.
--
-- lite round-tripped whatever it found -- crlf in, crlf out -- which is
-- the polite answer and the wrong one for an editor that is supposed to
-- have opinions. these three are not preferences, they are what the rest
-- of the toolchain already assumes, and a file that disagrees is a file
-- that will bite someone downstream. so wisp fixes them on the way out
-- rather than offering a setting for each.
--
-- the two rules that can *lose* something say so when the file opens,
-- not when it is saved: by the time you press ctrl+s the decision was
-- made minutes ago, and a warning then is an ambush.

local core = require("core")
local Doc = require("core.doc")

-- lua's utf8.len reports the position of the first invalid byte, so the
-- text can be walked in valid runs: keep the run, replace one bad byte
-- with u+fffd, continue. that is exactly what the renderer already
-- shows for those bytes (String::from_utf8_lossy on the rust side), so
-- normalizing makes the file agree with what you have been looking at
local function to_utf8(text)
    if utf8.len(text) then
        return text
    end
    local out, i = {}, 1
    while i <= #text do
        local ok, bad = utf8.len(text, i)
        if ok then
            table.insert(out, text:sub(i))
            break
        end
        table.insert(out, text:sub(i, bad - 1))
        table.insert(out, "\u{FFFD}")
        i = bad + 1
    end
    return table.concat(out)
end

local function is_valid_utf8(doc)
    for _, line in ipairs(doc.lines) do
        if not utf8.len(line) then
            return false
        end
    end
    return true
end

-- rewrites a line in place, keeping its newline. the doc's own edit
-- methods are used rather than touching `lines` directly so the change
-- is undoable and the highlighter hears about it
local function set_line(doc, i, text)
    if doc.lines[i] == text then
        return
    end
    local body = text:gsub("\n$", "")
    doc:insert(i, 1, body)
    doc:remove(i, #body + 1, i, math.huge)
end

local function normalize(doc)
    for i = 1, #doc.lines do
        set_line(doc, i, to_utf8(doc.lines[i]))
    end

    -- a doc always ends in a newline (Doc:load appends one to every
    -- line), so "one at the end" only ever means collapsing the blank
    -- lines above it. a document that is nothing but blank lines keeps
    -- one, because a doc must have a line
    local last = #doc.lines
    while last > 1 and doc.lines[last] == "\n" do
        last = last - 1
    end
    if last < #doc.lines then
        doc:remove(last, #doc.lines[last], #doc.lines, math.huge)
    end

    -- and the file goes out with unix line endings whatever came in
    doc.crlf = false
end

local load = Doc.load
function Doc:load(...)
    load(self, ...)
    -- both of these lose something on the next save, so they are said
    -- out loud at the one moment the choice is still yours
    if self.crlf then
        core.log("%s has crlf line endings; wisp saves lf", self.filename)
    end
    if not is_valid_utf8(self) then
        core.error("%s is not valid utf-8; the bad bytes are replaced on save", self.filename)
    end
end

local save = Doc.save
function Doc:save(...)
    normalize(self)
    save(self, ...)
end

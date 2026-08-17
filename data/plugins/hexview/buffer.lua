-- the byte buffer behind the hex view.
--
-- lite's doc is a list of `"\n"`-terminated lines, which is the wrong
-- shape for a file that has no lines: a hex editor addresses *bytes*, and
-- a doc cannot even hold an arbitrary one -- loading strips the newlines
-- and saving puts them back. so the hex view brings its own model.
--
-- it is a list of fixed-size chunks of a lua string. an overwrite, which
-- is what nearly every keystroke does, rewrites one chunk; only a change
-- of *size* rebuilds, which is an honest price for a model where every
-- byte after the edit really does move.
--
-- offsets are 0-based here and nowhere else in wisp. that is what every
-- hex editor, debugger and file-format spec in the world uses, and
-- translating once at the view's edge is cheaper than translating in the
-- reader's head on every line.

local Object = require("core.object")
local config = require("core.config")

local CHUNK = 8192

local Buffer = Object:extend()

function Buffer:new(data)
    self:set_data(data or "")
    self.undo_stack = { idx = 1 }
    self.redo_stack = { idx = 1 }
    self.clean_idx = 1
end

function Buffer:set_data(data)
    self.chunks = {}
    self.size = #data
    -- `version` is bumped by every mutation, including the ones that
    -- merge into an existing undo entry and so leave `idx` alone. the
    -- flat-string cache keys on it, and nothing else may
    self.version = (self.version or 0) + 1
    self.flat = nil
    local i = 1
    repeat
        table.insert(self.chunks, data:sub(i, i + CHUNK - 1))
        i = i + CHUNK
    until i > #data
end

--- the byte at `offset`, or nil past the end
function Buffer:byte(offset)
    if offset < 0 or offset >= self.size then
        return nil
    end
    return self.chunks[offset // CHUNK + 1]:byte(offset % CHUNK + 1)
end

--- `count` bytes from `offset`, clipped to what exists
function Buffer:sub(offset, count)
    offset = math.max(0, offset)
    count = math.min(count, self.size - offset)
    if count <= 0 then
        return ""
    end
    local first = offset // CHUNK + 1
    local last = (offset + count - 1) // CHUNK + 1
    local i = offset % CHUNK + 1
    if first == last then
        return self.chunks[first]:sub(i, i + count - 1)
    end
    return table.concat(self.chunks, "", first, last):sub(i, i + count - 1)
end

--- the whole file as one string. cached: saving and searching both want
--- it, and neither changes the buffer on the way past
function Buffer:tostring()
    if not self.flat or self.flat_version ~= self.version then
        self.flat = table.concat(self.chunks)
        self.flat_version = self.version
    end
    return self.flat
end

-- the one mutator: `count` bytes at `offset` become `text`. a same-length
-- overwrite patches the chunks it touches and leaves every other byte's
-- address alone; anything else rebuilds, because everything after the
-- edit has genuinely moved
function Buffer:raw_splice(offset, count, text)
    if count ~= #text then
        local all = self:tostring()
        self:set_data(all:sub(1, offset) .. text .. all:sub(offset + count + 1))
        return
    end
    local written = 0
    while written < count do
        local at = offset + written
        local ci, i = at // CHUNK + 1, at % CHUNK + 1
        local n = math.min(count - written, CHUNK - i + 1)
        local chunk = self.chunks[ci]
        -- the tail resumes at `i + n`: the bytes replaced are `i` through
        -- `i + n - 1`, and lua's sub is inclusive at both ends
        self.chunks[ci] = chunk:sub(1, i - 1)
            .. text:sub(written + 1, written + n)
            .. chunk:sub(i + n)
        written = written + n
    end
    self.version = self.version + 1
    self.flat = nil
end

-- an undo entry is the splice that puts things back, so undoing and
-- redoing are the same operation reading from opposite stacks
local function push(stack, entry)
    stack[stack.idx] = entry
    stack.idx = stack.idx + 1
end

-- a run of keystrokes is one undo, the way it is in the doc. two shapes
-- merge: typing on past the end of the last edit, and going back over a
-- byte the last edit already wrote -- which is what the second hex digit
-- of a byte does, and it must not cost an undo step of its own
local function merge(stack, offset, text, removed, time)
    local top = stack[stack.idx - 1]
    if not top or time - top.time > config.undo_merge_timeout then
        return false
    end
    if offset == top.offset + top.count then
        top.count = top.count + #text
        top.text = top.text .. removed
        top.time = time
        return true
    end
    if #text == #removed and offset >= top.offset and offset + #text <= top.offset + top.count then
        top.time = time
        return true
    end
    return false
end

--- replace `count` bytes at `offset` with `text`, recording the undo.
--- `cursor` is where the cursor was, so undo can put it back
function Buffer:splice(offset, count, text, cursor)
    local removed = self:sub(offset, count)
    if removed == text then
        return
    end
    local time = system.get_time()
    if not merge(self.undo_stack, offset, text, removed, time) then
        push(self.undo_stack, {
            offset = offset,
            count = #text,
            text = removed,
            cursor = cursor or offset,
            time = time,
        })
    end
    self.redo_stack = { idx = 1 }
    self:raw_splice(offset, count, text)
end

local function pop(self, stack, other)
    local entry = stack[stack.idx - 1]
    if not entry then
        return nil
    end
    stack.idx = stack.idx - 1
    stack[stack.idx] = nil
    push(other, {
        offset = entry.offset,
        count = #entry.text,
        text = self:sub(entry.offset, entry.count),
        cursor = entry.cursor,
        time = 0,
    })
    self:raw_splice(entry.offset, entry.count, entry.text)
    return entry.cursor
end

--- undo one edit, returning where the cursor was when it was made
function Buffer:undo()
    return pop(self, self.undo_stack, self.redo_stack)
end

function Buffer:redo()
    return pop(self, self.redo_stack, self.undo_stack)
end

-- dirtiness is the doc's own trick: the undo stack's index walks back
-- down as edits are undone, so undoing to where the file was saved is
-- clean again rather than merely older
function Buffer:is_dirty()
    return self.clean_idx ~= self.undo_stack.idx
end

function Buffer:clean()
    self.clean_idx = self.undo_stack.idx
end

--- the first occurrence of `text` at or after `from`, wrapping once.
--- returns the offset, or nil
function Buffer:find(text, from)
    if text == "" then
        return nil
    end
    local all = self:tostring()
    local at = all:find(text, math.max(0, from) + 1, true)
    if not at then
        at = all:find(text, 1, true)
    end
    return at and at - 1 or nil
end

--- and the last occurrence strictly before `from`, wrapping once
function Buffer:rfind(text, from)
    if text == "" then
        return nil
    end
    local all = self:tostring()
    local best, i = nil, 1
    while true do
        local at = all:find(text, i, true)
        if not at then
            break
        end
        if at - 1 < from then
            best = at - 1
        end
        i = at + 1
    end
    if best then
        return best
    end
    -- nothing behind: wrap to the last match in the file
    i = 1
    while true do
        local at = all:find(text, i, true)
        if not at then
            break
        end
        best = at - 1
        i = at + 1
    end
    return best
end

return Buffer

local Doc = require("core.doc")

-- the detector DEVIATIONS §21 was built for. indentation is a property
-- of the file; this is what measures it and writes the answer where
-- every indent-aware site already reads from.
--
-- rxi's version is not adapted here, it is replaced. his swapped
-- config.tab_type and config.indent_size around every command.perform
-- and every DocView:draw and restored them afterwards -- the global
-- swap §21 exists to make unnecessary, and wrong the moment two files
-- with different indentation are open at once. his detector also took
-- the *first* indented line it found; this takes the file's habit.

-- how far a line is indented, and with what. blank lines answer nil:
-- they indent nothing and interrupt nothing
local function indent_of(text)
    if text:find("^[ \t]*\n") then
        return nil
    end
    local ws = text:match("^[ \t]*")
    if ws:find("\t") then
        return "hard", #ws
    end
    return "soft", #ws
end

-- the winner of a histogram, smallest key breaking a tie. the tie-break
-- is not cosmetic: lua 5.5 randomizes the string hash seed per state,
-- so `pairs` order changes between boots and an unbroken tie would make
-- a file's indentation depend on which run opened it
local function mode_of(counts)
    local best, n = nil, 0
    for key, count in pairs(counts) do
        if count > n or (count == n and key < best) then
            best, n = key, count
        end
    end
    return best
end

local function detect(lines)
    local hard, soft = 0, 0
    local steps, widths = {}, {}
    local prev

    for _, text in ipairs(lines) do
        local type, width = indent_of(text)
        if type == "hard" then
            hard = hard + 1
            prev = nil
        elseif type == "soft" then
            soft = soft + (width > 0 and 1 or 0)
            if width > 0 then
                widths[width] = (widths[width] or 0) + 1
            end
            -- a step is what the file does when it goes one level
            -- deeper, which is the only thing that names a size
            if prev and width > prev then
                local step = width - prev
                steps[step] = (steps[step] or 0) + 1
            end
            prev = width
        end
    end

    if hard == 0 and soft == 0 then
        return nil
    end
    if hard > soft then
        -- how wide a tab is drawn stays a preference, so hard indents
        -- take no size and get_indent_info falls back to the config
        return "hard", nil
    end
    -- a file with a single indent level never takes a step; the
    -- narrowest indent it does use is the honest guess
    return "soft", mode_of(steps) or mode_of(widths)
end

local load = Doc.load

function Doc:load(...)
    load(self, ...)
    local type, size = detect(self.lines)
    if type then
        self.indent_info = { type = type, size = size, confirmed = true }
    end
end

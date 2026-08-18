local core = require("core")
local config = require("core.config")
local style = require("core.style")
local StatusView = require("core.statusview")

local git = {
    branch = nil,
    inserts = 0,
    deletes = 0,
}

-- run a command to completion and hand back its stdout.
--
-- rxi's version redirected `system.exec` into a temp file and then did
-- `coroutine.yield(1)` -- it *hoped* git had finished inside one second
-- and read the file either way. that is a race with no upper bound and
-- no error path, and it is the clearest thing in the whole plugin set
-- that the rust core bought outright: `system.spawn` polls, so this
-- waits for the actual exit instead of for a guess
local function read(argv)
    local proc = system.spawn(argv)
    if not proc then
        return nil
    end
    local chunks = {}
    while true do
        -- "" means nothing buffered yet, nil means end of stream
        local chunk = proc:read_stdout()
        if not chunk then
            break
        end
        chunks[#chunks + 1] = chunk
        coroutine.yield()
    end
    while proc:running() do
        coroutine.yield()
    end
    if proc:returncode() ~= 0 then
        return nil
    end
    return table.concat(chunks)
end

core.add_thread(function()
    while true do
        -- `.git` is a directory in a checkout and a file in a worktree,
        -- so its kind is not checked, only that it is there
        if system.get_file_info(".git") then
            local head = read({ "git", "rev-parse", "--abbrev-ref", "HEAD" })
            git.branch = head and head:match("[^\n]+")

            local stat = read({ "git", "diff", "--shortstat" }) or ""
            git.inserts = tonumber(stat:match("(%d+) insertion")) or 0
            git.deletes = tonumber(stat:match("(%d+) deletion")) or 0
        else
            git.branch = nil
        end

        coroutine.yield(config.project_scan_rate)
    end
end)

local get_items = StatusView.get_items

function StatusView:get_items()
    local left, right = get_items(self)
    if not git.branch then
        return left, right
    end

    local dirty = git.inserts ~= 0 or git.deletes ~= 0
    local t = {
        style.dim,
        self.separator,
        style.icon_font,
        style.icons.branch,
        style.font,
        dirty and style.accent or style.text,
        " " .. git.branch,
        style.dim,
        "  ",
        git.inserts ~= 0 and style.accent or style.text,
        "+" .. git.inserts,
        style.dim,
        " / ",
        git.deletes ~= 0 and style.accent or style.text,
        "-" .. git.deletes,
    }
    for _, item in ipairs(t) do
        table.insert(right, item)
    end

    return left, right
end

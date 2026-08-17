-- helix's `:` line, mapped onto wisp's own commands.
--
-- this is the half of helix that is not about editing text: it is how
-- the editing model asks the editor it lives in to save, close and open
-- things. zed's helix mode routes the same keys into its host's
-- commands; so does this. someone typing `:wq` is not looking for a
-- command palette, so the prompt speaks helix's vocabulary and the table
-- below is the whole translation.

local core = require("core")
local command = require("core.command")
local common = require("core.common")

local ex = {}

local function run(...)
    local names = { ... }
    return function()
        for _, name in ipairs(names) do
            command.perform(name)
        end
    end
end

ex.commands = {
    ["w"] = run("doc:save"),
    ["write"] = run("doc:save"),
    ["q"] = run("root:close"),
    ["quit"] = run("root:close"),
    ["wq"] = run("doc:save", "root:close"),
    ["x"] = run("doc:save", "root:close"),
    ["write-quit"] = run("doc:save", "root:close"),
    ["qa"] = run("core:quit"),
    ["quit-all"] = run("core:quit"),
    ["qa!"] = run("core:force-quit"),
    ["quit-all!"] = run("core:force-quit"),
    ["o"] = run("core:open-file"),
    ["open"] = run("core:open-file"),
    ["new"] = run("core:new-doc"),
    ["bc"] = run("root:close"),
    ["buffer-close"] = run("root:close"),
    ["config-open"] = run("core:open-user-module"),
    ["log-open"] = run("core:open-log"),
}

-- sorted, not `pairs` order: lua reseeds its string hash every boot, so
-- an unsorted list would suggest differently on every run
local function names()
    local res = {}
    for name in pairs(ex.commands) do
        table.insert(res, name)
    end
    table.sort(res)
    return res
end

function ex.enter()
    core.command_view:enter("", function(text, item)
        -- the command view wraps a plain suggestion into a table, so the
        -- highlighted one arrives as `item.text`; typing a name in full
        -- and submitting without a suggestion falls back to the text
        local name = item and item.text or text
        local fn = ex.commands[name]
        if not fn then
            core.error("no such command: %q", name)
            return
        end
        fn()
    end, function(text)
        return common.fuzzy_match(names(), text)
    end)
end

return ex

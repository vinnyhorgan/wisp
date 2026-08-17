local core = require("core")
local config = require("core.config")
local Doc = require("core.doc")

local times = setmetatable({}, { __mode = "k" })

local function update_time(doc)
    local info = system.get_file_info(doc.filename)
    times[doc] = info.modified
end

local function reload_doc(doc)
    -- the file can vanish between the stat and the open (a checkout, a
    -- build): raising here would kill the thread and with it every
    -- later reload
    local fp = io.open(doc.filename, "rb")
    if not fp then
        return
    end
    local text = fp:read("*a")
    fp:close()

    -- the file may have been replaced by binary output; keep the doc
    if text:find("\0", 1, true) then
        core.error("not auto-reloading %q: it is a binary file now", doc.filename)
        return
    end

    local sel = { doc:get_selection() }
    doc:remove(1, 1, math.huge, math.huge)
    doc:insert(1, 1, text:gsub("\r", ""):gsub("\n$", ""))
    doc:set_selection(table.unpack(sel))
    doc.crlf = text:find("\r\n", 1, true) ~= nil

    doc:clean()
    core.log_quiet('auto-reloaded doc "%s"', doc.filename)
end

core.add_thread(function()
    while true do
        -- check all doc modified times
        for _, doc in ipairs(core.docs) do
            local info = system.get_file_info(doc.filename or "")
            if info and times[doc] ~= info.modified then
                update_time(doc)
                if doc:is_dirty() then
                    -- reloading would silently discard the unsaved changes
                    core.error("%q changed on disk, keeping the unsaved changes", doc.filename)
                else
                    reload_doc(doc)
                end
            end
            coroutine.yield()
        end

        -- wait for next scan
        coroutine.yield(config.project_scan_rate)
    end
end)

-- patch `Doc.save|load` to store modified time
local load = Doc.load
local save = Doc.save

Doc.load = function(self, ...)
    local res = load(self, ...)
    update_time(self)
    return res
end

Doc.save = function(self, ...)
    local res = save(self, ...)
    update_time(self)
    return res
end

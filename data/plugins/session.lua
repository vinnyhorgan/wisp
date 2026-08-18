local core = require("core")
local common = require("core.common")
local config = require("core.config")
local DocView = require("core.docview")

-- the editor comes back the way you left it: the split tree, the tabs
-- in each split and which one was in front, and for every document its
-- selection and its scroll.
--
-- the structure is rxi's `workspace`. three things are wisp's own.
--
-- **where it is written.** rxi drops `.lite_workspace.lua` into the
-- project directory, which is rude: it is a file you did not create, in
-- a repository you did not want it in. this writes to `STATEDIR`, keyed
-- by the project's absolute path.
--
-- **when it is written.** rxi saves by monkeypatching `os.exit`, and
-- reads the file and immediately `os.remove`s it -- so a crash loses
-- the session, which is the one time you most wanted it back. this
-- saves whenever the state has actually changed and never deletes.
--
-- **what is not restored.** views that are not documents are skipped
-- rather than reconstructed from their module name. reopening a
-- terminal would spawn a shell you did not ask for, and reopening a
-- hex or image view costs a file read for something you may not want;
-- documents are the thing you meant. unsaved changes to *named* files
-- are not kept either -- see DEVIATIONS §26 for why that is a decision
-- and not an omission.

local function session_file()
    local dir = STATEDIR .. PATHSEP .. "sessions"
    system.mkdir(dir)
    local key = system.absolute_path(".") or "unknown"
    return dir .. PATHSEP .. key:gsub("[^%w]", "-") .. ".lua"
end

local function has_no_locked_children(node)
    if node.locked then
        return false
    end
    if node.type == "leaf" then
        return true
    end
    return has_no_locked_children(node.a) and has_no_locked_children(node.b)
end

local function get_unlocked_root(node)
    if node.type == "leaf" then
        return not node.locked and node
    end
    if has_no_locked_children(node) then
        return node
    end
    return get_unlocked_root(node.a) or get_unlocked_root(node.b)
end

local function save_view(view)
    if getmetatable(view) ~= DocView then
        return nil
    end
    return {
        active = core.active_view == view,
        filename = view.doc.filename,
        selection = { view.doc:get_selection() },
        scroll = { x = view.scroll.to.x, y = view.scroll.to.y },
        -- a scratch buffer has no file to be read back out of, so its
        -- text is the only thing that can carry it
        text = not view.doc.filename and view.doc:get_text(1, 1, math.huge, math.huge) or nil,
    }
end

local function load_view(t)
    local doc
    if t.filename then
        local ok, opened = pcall(core.open_doc, t.filename)
        if not ok then
            return nil
        end
        doc = opened
    else
        doc = core.open_doc()
        if t.text then
            doc:insert(1, 1, t.text)
            doc:clean()
        end
    end
    local dv = DocView(doc)
    doc:set_selection(table.unpack(t.selection))
    dv.last_line, dv.last_col = doc:get_selection()
    dv.scroll.x, dv.scroll.to.x = t.scroll.x, t.scroll.x
    dv.scroll.y, dv.scroll.to.y = t.scroll.y, t.scroll.y
    return dv
end

local function save_node(node)
    local res = { type = node.type }
    if node.type == "leaf" then
        res.views = {}
        for _, view in ipairs(node.views) do
            local t = save_view(view)
            if t then
                table.insert(res.views, t)
                if node.active_view == view then
                    res.active_view = #res.views
                end
            end
        end
    else
        res.divider = node.divider
        res.a = save_node(node.a)
        res.b = save_node(node.b)
    end
    return res
end

local function load_node(node, t)
    if t.type == "leaf" then
        local active
        for _, v in ipairs(t.views) do
            local view = load_view(v)
            if view then
                if v.active then
                    active = view
                end
                node:add_view(view)
            end
        end
        if t.active_view and node.views[t.active_view] then
            node:set_active_view(node.views[t.active_view])
        end
        return active
    end
    node:split(t.type == "hsplit" and "right" or "down")
    node.divider = t.divider
    local a = load_node(node.a, t.a)
    local b = load_node(node.b, t.b)
    return a or b
end

-- the treeview and the markers are not part of the node tree the loop
-- above walks: one is a locked view, the other is a weak table keyed by
-- document. both are asked for their state directly
local function save_treeview()
    local view = package.loaded["plugins.treeview"]
    if not view then
        return nil
    end
    return { visible = view.visible, size = view.target_size }
end

local function load_treeview(t)
    local view = package.loaded["plugins.treeview"]
    if view and t then
        view.visible = t.visible
        view.target_size = t.size
    end
end

local function save_markers()
    local markers = package.loaded["plugins.markers"]
    if not markers then
        return nil
    end
    local res = {}
    for _, doc in ipairs(core.docs) do
        local set = doc.filename and rawget(markers.cache, doc)
        if set and next(set) then
            local lines = {}
            for line in pairs(set) do
                table.insert(lines, line)
            end
            table.sort(lines)
            res[doc.filename] = lines
        end
    end
    return next(res) and res or nil
end

local function load_markers(t)
    local markers = package.loaded["plugins.markers"]
    if not markers or not t then
        return
    end
    for _, doc in ipairs(core.docs) do
        local lines = doc.filename and t[doc.filename]
        if lines then
            for _, line in ipairs(lines) do
                markers.cache[doc][line] = true
            end
        end
    end
end

local function serialize_state()
    return common.serialize({
        node = save_node(get_unlocked_root(core.root_view.root_node)),
        treeview = save_treeview(),
        markers = save_markers(),
    })
end

local last_written

local function save_session()
    local ok, text = pcall(serialize_state)
    if not ok or text == last_written then
        return
    end
    local fp = io.open(session_file(), "wb")
    if not fp then
        return
    end
    fp:write("return ", text, "\n")
    fp:close()
    last_written = text
end

local function load_session()
    local fp = io.open(session_file(), "rb")
    if not fp then
        return
    end
    local text = fp:read("*a")
    fp:close()
    local chunk = load(text, "=session", "t", {})
    if not chunk then
        return
    end
    local ok, t = pcall(chunk)
    if not ok or type(t) ~= "table" then
        return
    end
    last_written = text:match("^return (.*)\n$")
    load_treeview(t.treeview)
    local active = load_node(get_unlocked_root(core.root_view.root_node), t.node)
    load_markers(t.markers)
    if active then
        core.set_active_view(active)
    end
end

-- written on a slow tick rather than only at exit, so a crash costs at
-- most one interval instead of the whole session. the compare against
-- the last write means an idle editor never touches the disk
core.add_thread(function()
    while true do
        coroutine.yield(config.project_scan_rate)
        core.try(save_session)
    end
end)

local run = core.run

function core.run(...)
    -- files named on the command line are an explicit instruction and
    -- win over whatever was open last time
    if #core.docs == 0 then
        core.try(load_session)
    end

    local exit = os.exit
    function os.exit(...)
        core.try(save_session)
        exit(...)
    end

    core.run = run
    return core.run(...)
end

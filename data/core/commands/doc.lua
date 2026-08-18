local core = require("core")
local command = require("core.command")
local common = require("core.common")
local translate = require("core.doc.translate")
local DocView = require("core.docview")

local function dv()
    return core.active_view
end

local function doc()
    return core.active_view.doc
end

local function get_indent_string()
    local indent_type, indent_size = doc():get_indent_info()
    if indent_type == "hard" then
        return "\t"
    end
    return string.rep(" ", indent_size)
end

local function insert_at_start_of_selected_lines(text, skip_empty)
    local line1, col1, line2, col2, swap = doc():get_selection(true)
    for line = line1, line2 do
        local line_text = doc().lines[line]
        if not skip_empty or line_text:find("%S") then
            doc():insert(line, 1, text)
        end
    end
    doc():set_selection(line1, col1 + #text, line2, col2 + #text, swap)
end

local function remove_from_start_of_selected_lines(text, skip_empty)
    local line1, col1, line2, col2, swap = doc():get_selection(true)
    for line = line1, line2 do
        local line_text = doc().lines[line]
        if line_text:sub(1, #text) == text and (not skip_empty or line_text:find("%S")) then
            doc():remove(line, 1, line, #text + 1)
        end
    end
    doc():set_selection(line1, col1 - #text, line2, col2 - #text, swap)
end

local function save(filename)
    doc():save(filename)
    core.log('saved "%s"', doc().filename)
end

local commands = {
    ["doc:undo"] = function()
        doc():undo()
    end,

    ["doc:redo"] = function()
        doc():redo()
    end,

    ["doc:cut"] = function()
        if doc():has_selection() then
            local text = doc():get_text(doc():get_selection())
            system.set_clipboard(text)
            doc():delete_to(0)
        end
    end,

    ["doc:copy"] = function()
        if doc():has_selection() then
            local text = doc():get_text(doc():get_selection())
            system.set_clipboard(text)
        end
    end,

    ["doc:paste"] = function()
        doc():text_input(system.get_clipboard():gsub("\r", ""))
    end,

    ["doc:newline"] = function()
        local line, col = doc():get_selection()
        local indent = doc().lines[line]:match("^[\t ]*")
        if col <= #indent then
            indent = indent:sub(#indent + 2 - col)
        end
        doc():text_input("\n" .. indent)
    end,

    ["doc:newline-below"] = function()
        local line = doc():get_selection()
        local indent = doc().lines[line]:match("^[\t ]*")
        doc():insert(line, math.huge, "\n" .. indent)
        doc():set_selection(line + 1, math.huge)
    end,

    ["doc:newline-above"] = function()
        local line = doc():get_selection()
        local indent = doc().lines[line]:match("^[\t ]*")
        doc():insert(line, 1, indent .. "\n")
        doc():set_selection(line, math.huge)
    end,

    ["doc:delete"] = function()
        local line, col = doc():get_selection()
        if not doc():has_selection() and doc().lines[line]:find("^%s*$", col) then
            doc():remove(line, col, line, math.huge)
        end
        doc():delete_to(translate.next_char)
    end,

    ["doc:backspace"] = function()
        local line, col = doc():get_selection()
        if not doc():has_selection() then
            local text = doc():get_text(line, 1, line, col)
            local _, indent_size = doc():get_indent_info()
            if #text >= indent_size and text:find("^ *$") then
                doc():delete_to(0, -indent_size)
                return
            end
        end
        doc():delete_to(translate.previous_char)
    end,

    ["doc:select-all"] = function()
        doc():set_selection(1, 1, math.huge, math.huge)
    end,

    ["doc:select-none"] = function()
        local line, col = doc():get_selection()
        doc():set_selection(line, col)
    end,

    ["doc:select-lines"] = function()
        local line1, _, line2, _, swap = doc():get_selection(true)
        -- lite materialized a line below the selection (a real edit that
        -- dirtied the doc) so the selection could span the trailing
        -- newline; on the last line, clamping to its end selects the
        -- same text without touching the doc
        if line2 >= #doc().lines then
            doc():set_selection(line1, 1, line2, math.huge, swap)
        else
            doc():set_selection(line1, 1, line2 + 1, 1, swap)
        end
    end,

    ["doc:select-word"] = function()
        local line1, col1 = doc():get_selection(true)
        local line1, col1 = translate.start_of_word(doc(), line1, col1)
        local line2, col2 = translate.end_of_word(doc(), line1, col1)
        doc():set_selection(line2, col2, line1, col1)
    end,

    ["doc:join-lines"] = function()
        local line1, _, line2 = doc():get_selection(true)
        if line1 == line2 then
            line2 = line2 + 1
        end
        local text = doc():get_text(line1, 1, line2, math.huge)
        text = text:gsub("(.-)\n[\t ]*", function(x)
            return x:find("^%s*$") and x or x .. " "
        end)
        doc():insert(line1, 1, text)
        doc():remove(line1, #text + 1, line2, math.huge)
        if doc():has_selection() then
            doc():set_selection(line1, math.huge)
        end
    end,

    ["doc:indent"] = function()
        local text = get_indent_string()
        if doc():has_selection() then
            insert_at_start_of_selected_lines(text)
        else
            doc():text_input(text)
        end
    end,

    ["doc:unindent"] = function()
        local text = get_indent_string()
        remove_from_start_of_selected_lines(text)
    end,

    ["doc:duplicate-lines"] = function()
        local line1, col1, line2, col2, swap = doc():get_selection(true)
        local n = line2 - line1 + 1
        if line2 >= #doc().lines then
            -- the block ends on the last line: copy up to its end and
            -- reinsert below with a leading newline, so no phantom
            -- line has to be materialized first
            local text = doc():get_text(line1, 1, line2, math.huge)
            doc():insert(line2, math.huge, "\n" .. text)
        else
            local text = doc():get_text(line1, 1, line2 + 1, 1)
            doc():insert(line2 + 1, 1, text)
        end
        doc():set_selection(line1 + n, col1, line2 + n, col2, swap)
    end,

    ["doc:delete-lines"] = function()
        local line1, col1, line2 = doc():get_selection(true)
        if line2 >= #doc().lines then
            -- the block ends on the last line: there is no newline
            -- after it, so eat the one before the block instead
            if line1 == 1 then
                doc():remove(1, 1, line2, math.huge)
            else
                doc():remove(line1 - 1, math.huge, line2, math.huge)
            end
        else
            doc():remove(line1, 1, line2 + 1, 1)
        end
        doc():set_selection(line1, col1)
    end,

    ["doc:move-lines-up"] = function()
        local line1, col1, line2, col2, swap = doc():get_selection(true)
        if line1 > 1 then
            local text = doc().lines[line1 - 1]
            if line2 >= #doc().lines then
                -- the moved line lands at the end of the doc, where
                -- its newline moves in front of it
                doc():insert(line2, math.huge, "\n" .. text:sub(1, -2))
            else
                doc():insert(line2 + 1, 1, text)
            end
            doc():remove(line1 - 1, 1, line1, 1)
            doc():set_selection(line1 - 1, col1, line2 - 1, col2, swap)
        end
    end,

    ["doc:move-lines-down"] = function()
        local line1, col1, line2, col2, swap = doc():get_selection(true)
        if line2 < #doc().lines then
            local text = doc().lines[line2 + 1]
            if line2 + 1 >= #doc().lines then
                -- swapping with the last line: it has no newline of its
                -- own to give, so the block's trailing one moves down
                doc():remove(line2, math.huge, line2 + 1, math.huge)
            else
                doc():remove(line2 + 1, 1, line2 + 2, 1)
            end
            doc():insert(line1, 1, text)
            doc():set_selection(line1 + 1, col1, line2 + 1, col2, swap)
        end
    end,

    ["doc:toggle-line-comments"] = function()
        local comment = doc().syntax.comment
        if not comment then
            return
        end
        local comment_text = comment .. " "
        local line1, _, line2 = doc():get_selection(true)
        local uncomment = true
        for line = line1, line2 do
            local text = doc().lines[line]
            if text:find("%S") and text:find(comment_text, 1, true) ~= 1 then
                uncomment = false
            end
        end
        if uncomment then
            remove_from_start_of_selected_lines(comment_text, true)
        else
            insert_at_start_of_selected_lines(comment_text, true)
        end
    end,

    ["doc:upper-case"] = function()
        doc():replace(string.upper)
    end,

    ["doc:lower-case"] = function()
        doc():replace(string.lower)
    end,

    ["doc:go-to-line"] = function()
        local dv = dv()

        local items
        local function init_items()
            if items then
                return
            end
            items = {}
            local mt = {
                __tostring = function(x)
                    return x.text
                end,
            }
            for i, line in ipairs(dv.doc.lines) do
                local item = { text = line:sub(1, -2), line = i, info = "line: " .. i }
                table.insert(items, setmetatable(item, mt))
            end
        end

        core.command_view:enter("go to line", function(text, item)
            local line = item and item.line or tonumber(text)
            if not line then
                core.error("invalid line number or unmatched string")
                return
            end
            dv.doc:set_selection(line, 1)
            dv:scroll_to_line(line, true)
        end, function(text)
            if not text:find("^%d*$") then
                init_items()
                return common.fuzzy_match(items, text)
            end
        end)
    end,
}

-- saving and renaming act on the file behind the doc. the command view
-- is a docview too, so with the shared predicate a ctrl+s inside a
-- prompt would offer to write the prompt's text to disk
local file_commands = {
    ["doc:save-as"] = function()
        if doc().filename then
            core.command_view:set_text(doc().filename)
        end
        core.command_view:enter("save as", function(filename)
            save(common.home_expand(filename))
        end, common.path_suggest)
    end,

    ["doc:save"] = function()
        if doc().filename then
            save()
        else
            command.perform("doc:save-as")
        end
    end,

    ["doc:rename"] = function()
        local old_filename = doc().filename
        if not old_filename then
            core.error("cannot rename unsaved doc")
            return
        end
        core.command_view:set_text(old_filename)
        core.command_view:enter("rename", function(filename)
            filename = common.home_expand(filename)
            doc():save(filename)
            core.log('renamed "%s" to "%s"', old_filename, filename)
            -- on a case-insensitive filesystem a different string can
            -- still be the same file, and removing it would delete the
            -- doc that was just saved; absolute_path resolves both to
            -- the name the filesystem really stores. stats cannot tell
            -- them apart: mtimes are whole seconds and a clean doc's
            -- re-save is byte-identical, so a rename in the same second
            -- looked like the same file and left the old one behind
            local old_path = system.absolute_path(old_filename)
            local new_path = system.absolute_path(filename)
            if old_path and new_path and old_path ~= new_path then
                os.remove(old_filename)
            end
        end, common.path_suggest)
    end,
}

local translations = {
    ["previous-char"] = translate.previous_char,
    ["next-char"] = translate.next_char,
    ["previous-word-start"] = translate.previous_word_start,
    ["next-word-end"] = translate.next_word_end,
    ["previous-block-start"] = translate.previous_block_start,
    ["next-block-end"] = translate.next_block_end,
    ["start-of-doc"] = translate.start_of_doc,
    ["end-of-doc"] = translate.end_of_doc,
    ["start-of-line"] = translate.start_of_line,
    ["end-of-line"] = translate.end_of_line,
    ["start-of-word"] = translate.start_of_word,
    ["end-of-word"] = translate.end_of_word,
    ["previous-line"] = DocView.translate.previous_line,
    ["next-line"] = DocView.translate.next_line,
    ["previous-page"] = DocView.translate.previous_page,
    ["next-page"] = DocView.translate.next_page,
}

for name, fn in pairs(translations) do
    commands["doc:move-to-" .. name] = function()
        doc():move_to(fn, dv())
    end
    commands["doc:select-to-" .. name] = function()
        doc():select_to(fn, dv())
    end
    commands["doc:delete-to-" .. name] = function()
        doc():delete_to(fn, dv())
    end
end

commands["doc:move-to-previous-char"] = function()
    if doc():has_selection() then
        local line, col = doc():get_selection(true)
        doc():set_selection(line, col)
    else
        doc():move_to(translate.previous_char)
    end
end

commands["doc:move-to-next-char"] = function()
    if doc():has_selection() then
        local _, _, line, col = doc():get_selection(true)
        doc():set_selection(line, col)
    else
        doc():move_to(translate.next_char)
    end
end

command.add("core.docview", commands)

local CommandView = require("core.commandview")
command.add(function()
    return core.active_view:is(DocView) and not core.active_view:is(CommandView)
end, file_commands)

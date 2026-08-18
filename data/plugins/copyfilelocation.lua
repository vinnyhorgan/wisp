local core = require("core")
local command = require("core.command")

command.add("core.docview", {
    ["copy-file-location:copy-file-location"] = function()
        local doc = core.active_view.doc
        if not doc.filename then
            core.error("cannot copy the location of an unsaved doc")
            return
        end
        local filename = system.absolute_path(doc.filename)
        core.log("copying %q to the clipboard", filename)
        system.set_clipboard(filename)
    end,
})

local syntax = require("core.syntax")

syntax.add({
    files = { "[Mm]akefile$", "GNUmakefile$", "%.mk$" },
    comment = "#",
    patterns = {
        { pattern = "#.*\n", type = "comment" },
        { pattern = [[\.]], type = "normal" },
        { pattern = "$[@^<%%?+|*]", type = "keyword2" },
        { pattern = "$%(.-%)", type = "keyword2" },
        { pattern = "%f[%w_][%d%.]+%f[^%w_]", type = "number" },
        { pattern = "%..*:", type = "keyword2" },
        { pattern = ".*:", type = "function" },
    },
    symbols = {},
})

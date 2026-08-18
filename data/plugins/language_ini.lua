local syntax = require("core.syntax")

-- lite-xl's ini file. it earns its place three times over: `.ini` and
-- `.cfg`, the `.editorconfig` wisp's own repo carries, and git's own
-- config files, which are ini in everything but name. upstream guards
-- the editorconfig claim with a leading PATHSEP, which only matches a
-- file opened by path; wisp opens it by name from the tree, so the
-- guard is dropped
syntax.add({
    files = {
        "%.ini$",
        "%.inf$",
        "%.cfg$",
        "%.conf$",
        "%.editorconfig$",
        "%.gitconfig$",
        "%.gitmodules$",
        "%.desktop$",
    },
    comment = "#",
    patterns = {
        { pattern = ";.*", type = "comment" },
        { pattern = "#.*", type = "comment" },
        { pattern = { "%[", "%]" }, type = "keyword" },

        { pattern = { '"""', '"""', "\\" }, type = "string" },
        { pattern = { '"', '"', "\\" }, type = "string" },
        { pattern = { "'''", "'''" }, type = "string" },
        { pattern = { "'", "'" }, type = "string" },

        { pattern = "[A-Za-z0-9_%.%-]+%s*%f[=]", type = "function" },
        { pattern = "[%-+]?[0-9_]+%.[0-9_]+", type = "number" },
        { pattern = "[%-+]?[0-9_]+", type = "number" },
        { pattern = "[a-z]+", type = "symbol" },
    },
    symbols = {
        ["true"] = "literal",
        ["false"] = "literal",
    },
})

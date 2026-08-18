local syntax = require("core.syntax")

-- lite-xl's toml file with two upstream problems fixed. its number rule
-- read `[$d_]` where it meant `[%d_]`, and its table header was written
-- `^%s*%[`: wisp's tokenizer anchors every pattern itself, so a leading
-- `^` becomes a literal caret and the rule never fires. a header is
-- matched whole instead, and required to start with a key character so
-- that an array of numbers is not mistaken for one
syntax.add({
    files = { "%.toml$" },
    comment = "#",
    patterns = {
        { pattern = "#.*", type = "comment" },

        { pattern = { '"""', '"""', "\\" }, type = "string" },
        { pattern = { "'''", "'''" }, type = "string" },
        { pattern = { '"', '"', "\\" }, type = "string" },
        { pattern = { "'", "'" }, type = "string" },

        { pattern = "%[%[?[%a_\"'][%w_%-%.\"'%s]*%]%]?", type = "keyword" },
        { pattern = "[%w_%.%-]+%s*%f[=]", type = "function" },

        { pattern = "0x[%x_]+", type = "number" },
        { pattern = "0o[0-7_]+", type = "number" },
        { pattern = "0b[01_]+", type = "number" },
        { pattern = "%d[%d_]*%.?[%d_]*[eE][%-+]?[%d_]+", type = "number" },
        { pattern = "%d[%d_]*%.?[%d_]*", type = "number" },
        { pattern = "%f[-+%w_][-+]%f[%w%.]", type = "number" },

        { pattern = "[%+%-:TZ]", type = "operator" },
        { pattern = "%a+", type = "symbol" },
    },
    symbols = {
        ["true"] = "literal",
        ["false"] = "literal",
        ["nan"] = "number",
        ["inf"] = "number",
    },
})

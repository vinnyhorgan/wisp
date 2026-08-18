local syntax = require("core.syntax")

-- json had no file in rxi's set, and lite-xl's is two pcre rules, which
-- wisp's tokenizer cannot run. so it is written here, in lua patterns.
-- the only thing worth more than a plain string rule is telling a key
-- from a value, which is what the frontier on `:` does: a quoted run
-- that ends at a colon is a key. `.jsonc` is claimed too, and the
-- comment rules are harmless in strict json -- a `//` inside a string
-- has already been eaten by the string rule
syntax.add({
    files = { "%.json$", "%.jsonc$" },
    patterns = {
        { pattern = "//.-\n", type = "comment" },
        { pattern = { "/%*", "%*/" }, type = "comment" },
        { pattern = '"[^"]*"%s*%f[:]', type = "function" },
        { pattern = { '"', '"', "\\" }, type = "string" },
        { pattern = "-?%d+%.%d+[eE][-+]?%d+", type = "number" },
        { pattern = "-?%d+%.%d+", type = "number" },
        { pattern = "-?%d+[eE][-+]?%d+", type = "number" },
        { pattern = "-?%d+", type = "number" },
        { pattern = "[%[%]{},:]", type = "operator" },
        { pattern = "%a+", type = "symbol" },
    },
    symbols = {
        ["true"] = "literal",
        ["false"] = "literal",
        ["null"] = "literal",
    },
})

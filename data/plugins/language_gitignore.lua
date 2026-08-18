local syntax = require("core.syntax")

-- lite-xl's version of this is three pcre rules; these are the same
-- three ideas in lua patterns. a gitignore is a comment, a negation and
-- a glob, and there is nothing else in the format
syntax.add({
    files = { "%.gitignore$", "%.dockerignore$", "%.npmignore$", "%.eslintignore$" },
    comment = "#",
    patterns = {
        { pattern = "#.*", type = "comment" },
        { pattern = "!", type = "keyword" },
        { pattern = "%*%*", type = "operator" },
        { pattern = { "%[", "%]" }, type = "operator" },
        { pattern = "[%*%?/]", type = "operator" },
        { pattern = "[%w_%-%.]+", type = "symbol" },
    },
    symbols = {},
})

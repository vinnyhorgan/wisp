local syntax = require("core.syntax")

-- a diff is decided entirely by the first character of each line, and
-- wisp's tokenizer anchors every pattern at the position it is trying,
-- so a rule cannot ask for the start of a line. the catch-all `.+` at
-- the bottom is what makes that work anyway: on any line no earlier
-- rule claimed, it swallows the line whole, so the tokenizer never
-- tries a later position and a `+` in the middle of a line of prose is
-- never mistaken for an added line
syntax.add({
    files = { "%.diff$", "%.patch$" },
    patterns = {
        { pattern = "diff .*", type = "comment" },
        { pattern = "index .*", type = "comment" },
        { pattern = "new file.*", type = "comment" },
        { pattern = "deleted file.*", type = "comment" },
        { pattern = "similarity index.*", type = "comment" },
        { pattern = "rename .*", type = "comment" },
        { pattern = "%-%-%-.*", type = "function" },
        { pattern = "%+%+%+.*", type = "function" },
        { pattern = "@@.*", type = "keyword" },
        { pattern = "%+.*", type = "string" },
        { pattern = "%-.*", type = "number" },
        { pattern = "\\.*", type = "comment" },
        { pattern = ".+", type = "normal" },
    },
    symbols = {},
})

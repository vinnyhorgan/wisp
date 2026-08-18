local syntax = require("core.syntax")

-- the buffers git opens in $EDITOR. for a commit message the whole
-- point is the commented half: everything below the `#` lines is
-- instructions and status and none of it is committed. nothing else in
-- the file is markup, so nothing else is colored -- a message is prose
syntax.add({
    files = { "COMMIT_EDITMSG$", "MERGE_MSG$", "TAG_EDITMSG$", "NOTES_EDITMSG$" },
    comment = "#",
    patterns = {
        { pattern = "#.*", type = "comment" },
        { pattern = ".+", type = "normal" },
    },
    symbols = {},
})

-- the rebase todo is the opposite: it is all markup, and picking the
-- wrong verb costs more here than anywhere else git puts you. it gets
-- its own entry rather than sharing the one above, so that "drop" and
-- "merge" are keywords in the todo and ordinary words in a message
syntax.add({
    files = { "git%-rebase%-todo$" },
    comment = "#",
    patterns = {
        { pattern = "#.*", type = "comment" },
        { pattern = "%x%x%x%x%x%x%x+%f[%s]", type = "number" },
        { pattern = "[%a_][%w_%-]*", type = "symbol" },
    },
    symbols = {
        ["pick"] = "keyword",
        ["p"] = "keyword",
        ["reword"] = "keyword",
        ["edit"] = "keyword",
        ["squash"] = "keyword",
        ["fixup"] = "keyword",
        ["exec"] = "keyword",
        ["break"] = "keyword",
        ["drop"] = "keyword",
        ["label"] = "keyword",
        ["reset"] = "keyword",
        ["merge"] = "keyword",
        ["update-ref"] = "keyword",
    },
})

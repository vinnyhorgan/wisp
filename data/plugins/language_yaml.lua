local syntax = require("core.syntax")

-- lite-xl's yaml file is written against their tokenizer: it nests a
-- subsyntax for bracket lists, splits one pattern across several token
-- types with position captures, and anchors rules with `^`. wisp's
-- tokenizer does none of the three, so this is yaml's own set of rules
-- written for the tokenizer wisp has. keys are colored like the keys in
-- toml and ini, because in this editor those three files are the same
-- job
syntax.add({
    files = { "%.yml$", "%.yaml$" },
    comment = "#",
    patterns = {
        { pattern = "#.*", type = "comment" },

        { pattern = { '"', '"', "\\" }, type = "string" },
        { pattern = { "'", "'" }, type = "string" },

        -- document and directive markers
        { pattern = "%-%-%-", type = "operator" },
        { pattern = "%.%.%.", type = "operator" },
        { pattern = "%%%a+", type = "keyword" },

        -- an explicit type tag, then an anchor and the alias that uses it
        { pattern = "!!?[%w_%-%./]+", type = "keyword2" },
        { pattern = "[&%*][%w_%-]+", type = "keyword2" },
        { pattern = "<<", type = "keyword" },

        -- a key is a plain scalar that ends at a colon
        { pattern = "[%w_][%w_%-%.]-%f[:]", type = "function" },

        -- block scalar introducers, `|` and `>`, with their chomping
        { pattern = "[|>][%-+]?%d?", type = "keyword" },

        { pattern = "%-?%.inf", type = "number" },
        { pattern = "%.[Nn]a[Nn]", type = "number" },
        { pattern = "[%+%-]?0x%x+", type = "number" },
        { pattern = "[%+%-]?%d+%.%d+[eE][%+%-]?%d+", type = "number" },
        { pattern = "[%+%-]?%d+%.%d+", type = "number" },
        { pattern = "[%+%-]?%d+", type = "number" },

        { pattern = "[%-%?:,%[%]{}]", type = "operator" },
        { pattern = "[%a_][%w_%-]*", type = "symbol" },
    },
    symbols = {
        ["true"] = "literal",
        ["false"] = "literal",
        ["yes"] = "literal",
        ["no"] = "literal",
        ["on"] = "literal",
        ["off"] = "literal",
        ["null"] = "literal",
    },
})

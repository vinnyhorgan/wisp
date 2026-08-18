local syntax = require("core.syntax")

-- lite-xl's html file with its two nesting rules removed. they hand a
-- `<script>` or `<style>` body to another syntax, and wisp's tokenizer
-- is lite's: it has no subsyntaxes, so the key is silently ignored and
-- the whole block paints as one function-colored span -- worse than
-- having no rule at all. without them the bodies fall through to the
-- tag rules and read as plain text, which is honest
syntax.add({
    files = { "%.html?$" },
    patterns = {
        { pattern = { "<!%-%-", "%-%->" }, type = "comment" },
        { pattern = { "%f[^>][^<]", "%f[<]" }, type = "normal" },
        { pattern = { '"', '"', "\\" }, type = "string" },
        { pattern = { "'", "'", "\\" }, type = "string" },
        { pattern = "0x[%da-fA-F]+", type = "number" },
        { pattern = "-?%d+[%d%.]*f?", type = "number" },
        { pattern = "-?%.?%d+f?", type = "number" },
        { pattern = "%f[^<]![%a_][%w_]*", type = "keyword2" },
        { pattern = "%f[^<][%a_][%w_]*", type = "function" },
        { pattern = "%f[^<]/[%a_][%w_]*", type = "function" },
        { pattern = "[%a_][%w_]*", type = "keyword" },
        { pattern = "[/<>=]", type = "operator" },
    },
    symbols = {},
})

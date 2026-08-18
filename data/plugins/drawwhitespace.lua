local common = require("core.common")
local style = require("core.style")
local DocView = require("core.docview")

-- whitespace you can see, in the one place it is worth seeing: inside
-- a selection, where you are looking at it on purpose.
--
-- there is no toggle, no whitespace-everywhere mode, and no mark at the
-- end of the file. that last one was tried twice -- a `¬` glyph, then a
-- dimmed caret -- and both were wrong for the same reason, which is
-- clearer now than it was: **every file wisp saves ends in exactly one
-- newline** (§23), so a mark saying so is drawn on every file, always,
-- and a mark that is always there carries no information. the dimmed
-- caret was worse than the glyph, because at the end of a file it sits
-- exactly where the real caret sits and looks exactly like it.
--
-- dots under every space are wallpaper too; the indent guides already
-- answer the question they were being asked.
--
-- upstream draws one `renderer.draw_text` and one `font:get_width` per
-- character of every visible line, on the draw path -- the cost
-- lite-xl's 360-line rewrite exists to remove. wisp draws one string
-- per line instead, built to line up with the text underneath it. that
-- only works because there is exactly one font and it is monospaced:
-- every glyph in it advances by one cell, so a mask of the same length
-- sits exactly where the characters do. a tab is the one exception,
-- and the renderer gives it a fixed advance (`Font::tab_advance`,
-- itself `space * indent_size`), so the mark plus that many spaces
-- less one is exactly a tab wide
local SPACE = "\u{00B7}" -- middle dot
local TAB = "\u{00BB}" -- right double angle quote

local function selection_range(doc, idx)
    local line1, col1, line2, col2 = doc:get_selection(true)
    if line1 == line2 and col1 == col2 then
        return nil
    end
    if idx < line1 or idx > line2 then
        return nil
    end
    return idx == line1 and col1 or 1, idx == line2 and col2 or math.huge
end

local draw_line_text = DocView.draw_line_text

function DocView:draw_line_text(idx, x, y)
    draw_line_text(self, idx, x, y)
    if getmetatable(self) ~= DocView then
        return
    end

    local from, to = selection_range(self.doc, idx)
    if not from then
        return
    end

    local text = self.doc.lines[idx]

    local _, indent_size = self.doc:get_indent_info()
    local out, marked, col = {}, false, 1
    for chr in common.utf8_chars(text) do
        local selected = col >= from and col < to
        if selected and chr == " " then
            out[#out + 1] = SPACE
            marked = true
        elseif selected and chr == "\t" then
            out[#out + 1] = TAB .. string.rep(" ", indent_size - 1)
            marked = true
        elseif chr == "\t" then
            out[#out + 1] = string.rep(" ", indent_size)
        elseif chr ~= "\n" then
            out[#out + 1] = " "
        end
        col = col + #chr
    end

    if marked then
        local ty = y + self:get_line_text_y_offset()
        renderer.draw_text(self:get_font(), table.concat(out), x, ty, style.whitespace)
    end
end

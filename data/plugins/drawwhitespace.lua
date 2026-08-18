local common = require("core.common")
local style = require("core.style")
local DocView = require("core.docview")

-- whitespace you can see, in the two places it is worth seeing: inside
-- a selection, where you are looking at it on purpose, and at the very
-- end of the file, where the mark is DEVIATIONS §23 made visible --
-- everything wisp saves ends in exactly one newline, and this is the
-- editor showing its work.
--
-- that end-of-file mark is a dimmed caret, not a glyph. a `¬` sitting
-- in the text flow reads as a character in the file, which is the one
-- thing it is not, and it moves the eye every time you scroll to the
-- bottom. vscode's `renderFinalNewline = dimmed` draws a faded cursor
-- there instead, and it is right: the mark is a position, so it is
-- drawn like one.
--
-- there is no toggle and no whitespace-everywhere mode. dots under
-- every space are wallpaper; the indent guides already answer the
-- question they were being asked to answer.
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

    local text = self.doc.lines[idx]

    -- the last line's newline is its last character, so the mark goes
    -- exactly where that character is. the real caret is drawn after
    -- this, and over it, so the two never argue about the same pixels
    if idx == #self.doc.lines then
        local mx = x + self:get_col_x_offset(idx, #text)
        renderer.draw_rect(mx, y, style.caret_width, self:get_line_height(), style.whitespace)
    end

    local from, to = selection_range(self.doc, idx)
    if not from then
        return
    end

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

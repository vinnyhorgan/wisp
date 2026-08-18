local style = require("core.style")
local DocView = require("core.docview")

-- rxi's indent guides, reading the document's own indent size (§21)
-- rather than the global config: two files with different indentation
-- open side by side draw different guides, which is the whole point of
-- indent info existing.
--
-- the blank-line walk is rxi's and is kept: a blank line borrows the
-- deeper of its neighbours so a guide does not break across it. it
-- recurses, but as a proper tail call, so a long run of blank lines
-- costs time and not stack -- and the draw path is the one that runs
-- outside core.try, so the difference matters
local function get_line_spaces(doc, idx, dir, indent_size)
    local text = doc.lines[idx]
    if not text then
        return 0
    end
    local s, e = text:find("^%s*")
    if e == #text then
        return get_line_spaces(doc, idx + dir, dir, indent_size)
    end
    local n = 0
    for i = s, e do
        n = n + (text:byte(i) == 9 and indent_size or 1)
    end
    return n
end

local function get_line_indent_guide_spaces(doc, idx, indent_size)
    if doc.lines[idx]:find("^%s*\n") then
        return math.max(
            get_line_spaces(doc, idx - 1, -1, indent_size),
            get_line_spaces(doc, idx + 1, 1, indent_size)
        )
    end
    return get_line_spaces(doc, idx, nil, indent_size)
end

local draw_line_text = DocView.draw_line_text

function DocView:draw_line_text(idx, x, y)
    if getmetatable(self) ~= DocView then
        return draw_line_text(self, idx, x, y)
    end

    local _, indent_size = self.doc:get_indent_info()
    local spaces = get_line_indent_guide_spaces(self.doc, idx, indent_size)
    local sw = self:get_font():get_width(" ")
    local w = math.ceil(1 * SCALE)
    local h = self:get_line_height()
    for i = 0, spaces - 1, indent_size do
        renderer.draw_rect(x + sw * i, y, w, h, style.guide)
    end
    draw_line_text(self, idx, x, y)
end

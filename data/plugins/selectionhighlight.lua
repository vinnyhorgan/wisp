local style = require("core.style")
local DocView = require("core.docview")

-- originally written by luveti, for lite

-- the rule for what counts as a search term is vscode's, and it is the
-- right one: a selection of nothing but spaces or tabs is not something
-- you are looking for, it is somewhere the caret happened to stop. one
-- selected space boxing every space on the screen is the single worst
-- thing this plugin can do, and it is what upstream does.
--
-- the selection itself is skipped too. it is already drawn -- boxing it
-- as well says "here is another one of these" about the one you are
-- looking at
local function is_a_search_term(text)
    return not text:find("^[ \t]*$")
end

local function draw_box(x, y, w, h, color)
    local r = renderer.draw_rect
    local s = math.ceil(SCALE)
    r(x, y, w, s, color)
    r(x, y + h - s, w, s, color)
    r(x, y + s, s, h - s * 2, color)
    r(x + w - s, y + s, s, h - s * 2, color)
end

local draw_line_body = DocView.draw_line_body

function DocView:draw_line_body(idx, x, y)
    if getmetatable(self) ~= DocView then
        return draw_line_body(self, idx, x, y)
    end

    local line1, col1, line2, col2 = self.doc:get_selection(true)
    local selected_text = line1 == line2 and self.doc.lines[line1]:sub(col1, col2 - 1)
    if selected_text and is_a_search_term(selected_text) then
        local lh = self:get_line_height()
        local current_line_text = self.doc.lines[idx]
        local last_col = 1
        while true do
            local start_col, end_col = current_line_text:find(selected_text, last_col, true)
            if start_col == nil then
                break
            end
            if not (idx == line1 and start_col == col1) then
                local x1 = x + self:get_col_x_offset(idx, start_col)
                local x2 = x + self:get_col_x_offset(idx, end_col + 1)
                draw_box(x1, y, x2 - x1, lh, style.selectionhighlight)
            end
            last_col = end_col + 1
        end
    end
    draw_line_body(self, idx, x, y)
end

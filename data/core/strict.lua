local strict = {}
strict.defined = {}

-- used to define a global variable. lite named this `global`, which
-- lua 5.5 made a reserved word -- old callers could not even parse
function declare(t)
    for k, v in pairs(t) do
        strict.defined[k] = true
        rawset(_G, k, v)
    end
end

function strict.__newindex(t, k, v)
    error("cannot set undefined variable: " .. k, 2)
end

function strict.__index(t, k)
    if not strict.defined[k] then
        error("cannot get undefined variable: " .. k, 2)
    end
end

setmetatable(_G, strict)

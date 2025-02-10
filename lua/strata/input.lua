---@class strata
---@field input strata.input

---@class strata.input
local input = {}

function input.setup(config) end

---
---@param mod strata.input.Modifier
---@param key strata.input.Key
---@param action fun()
function input.keybind(mod, key, action) end

---
---@param mod strata.input.Modifier
---@param button strata.input.Button
---@param action fun()
function input.mousebind(mod, button, action) end

---@enum strata.input.Modifier
input.Modifier = {
}

---@enum strata.input.Key
input.Key = {
}

---@enum strata.input.Button
input.Button = {
}

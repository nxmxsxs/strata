local k = strata.input.Key
local m = strata.input.Modifier

strata.input.setup {
	repeat_info = {
		rate = 30,
		delay = 150,
	},

	xkbconfig = {
		layout = "it",
		rules = "",
		model = "",
		options = "caps:swapescape",
		variant = "",
	},
}

strata.input.keybind(m.Control_L + m.Alt_L, k.Return, function()
	print("spawning kitty")
	strata.proc.spawn("kitty")
end)

strata.input.keybind(m.Control_L + m.Alt_L, k.Escape, function()
	print("quitting strata")
	strata.quit()
end)

local p = strata.proc.spawn { "pactl", "subscribe" }
p:on_line_stdout(function(line) print("(lua) stdout=" .. line) end)
p:on_line_stderr(function(line) print("(lua) stderr=" .. line) end)
p:on_exit(function(status, signal) print("(lua) exited with", status, signal) end)

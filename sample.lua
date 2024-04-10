local beeper = instruments.HoldButton(signals.Sine())
local kicker = instruments.PressButton(signals.BrownNoise())

time.SetTempo(120)
time.OnBeat(4, function ()
	kicker:play(100)
end)

for i, key in ipairs({"q", "w", "e", "r", "t", "y", "u", "i", "o", "p", "[", "]", "\\"}) do
	modes.Normal:bind(key, beeper:makeButton(440 * pow(2, i / 12), i))
end

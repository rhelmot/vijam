local beeper = instruments.HoldButton(signals.Sine())
local kicker = instruments.PressButton(signals.BrownNoise())

time.SetTempo(120)
time.OnBeat(4, function ()
	kicker:play(100)
end)

for i, key in ipairs({"`", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "<DASH>", "="}) do
	modes.Normal:bind(key, beeper:makeButton(440 * (2 ^ (i / 12)), i))
end

local M = {}

local function mkInstrument(baseType, signalType)
	id = native.mkInstrument(baseType, signalType)
	return {
		_cls = "instrument",
		id = id,
		play = function(self, pitch, voice, duration)
			return native.play(self.id, pitch, voice, duration)
		end,
		mute = function(self, voice)
			return native.mute(self.id, voice)
		end,
		makeButton = function(self, pitch, voice)
			return {
				_cls = "button",
				instrument = self,
				pitch = pitch,
				voice = voice,
			}
		end,
	}
end

M.instruments = {
	HoldButton = function (signalType) 
		return mkInstrument(native.instruments.HoldButton, signalType)
	end,
	PressButton = function (signalType)
		return mkInstrument(native.instruments.PressButton, signalType)
	end,
}

M.signals = {
	Sine = function ()
		return native.signals.Sine
	end,
	BrownNoise = function ()
		return native.signals.BrownNoise
	end,
}

M.time = {
	SetTempo = native.setTempo,
	GetTempo = native.getTempo,
	OnBeat = native.onBeat,
	OnTimeout = native.onTimeout,
	Cancel = native.cancelTimer,
}

M.modes = {
	new = function(self, name, defaultTarget, defaultAction)
		self[name] = {
			_cls = "mode",
			id = native.mkMode(name, defaultTarget, defaultAction),
			name = name,
			bind = function(self, key, action, next)
				if action._cls == "button" then
					local nativeAction = native.mkPlay(action.instrument.id, action.pitch, action.voice, nil)
					local nativeActionUp = native.mkMute(action.instrument.id, action.voice)
					native.bind(self.id, key, nativeAction, next)
					native.bindUp(self.id, key, nativeActionUp)
				else
					native.bind(self.id, key, action, next)
				end
			end,
			bindUp = function(self, key, action)
				native.bindUp(self.id, key, action)
			end,
			unbind = function(self, key)
				native.unbind(self.id, key)
			end,
		}
	end,
}

M.modes:new("Normal")

return M

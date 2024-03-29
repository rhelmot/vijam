use std::time::Duration;

#[non_exhaustive]
pub enum NoteParam {
    Pitch(f32),
    Amplitude(f32),
    Articulation(f32),
    OtherFloat(String, f32),
    OtherString(String, String),
}

#[non_exhaustive]
pub enum InstrumentParam {
    NextNote(NoteParam),
    OtherFloat(String, f32),
    OtherString(String, String),
}

pub trait Note {
    fn set_param(&mut self, param: NoteParam);
    fn mute(&mut self);
    fn render(&mut self, time: Duration) -> f32;
    fn finished(&mut self, time: Duration) -> bool;
}

pub trait Instrument: Send {
    fn set_param(&mut self, param: InstrumentParam);
    fn note(&mut self, voice: u32) -> Box<dyn Note>;
}

pub enum InstrumentEvent {
    SetParam { param: InstrumentParam },
    NoteEvent { voice: u32, event: NoteEvent },
}

pub enum NoteEvent {
    Hit {},
    SetParam { param: NoteParam },
    Mute {},
}

pub struct HeldButtonInstrument {
    next_pitch: f32,
    amplitude: f32,
}

impl HeldButtonInstrument {
    pub fn new() -> Self {
        Self {
            next_pitch: 440.0,
            amplitude: 0.1,
        }
    }
}

pub struct HeldButtonNote {
    pitch: f32,
    amplitude: f32,
    mute_at: Option<Duration>,
    change_at: Duration,
    change_phase: f32,
    change_pending: Option<HeldButtonNoteChange>,
}

#[derive(Default)]
struct HeldButtonNoteChange {
    pitch: Option<f32>,
    amplitude: Option<f32>,
    mute: bool,
}

impl HeldButtonNote {
    fn with_change<F: FnOnce(&mut HeldButtonNoteChange) -> ()>(&mut self, func: F) {
        let mut thing = self.change_pending.take().unwrap_or_else(HeldButtonNoteChange::default);
        func(&mut thing);
        self.change_pending = Some(thing);
    }

    fn phase(&self, time: Duration) -> f32 {
        self.change_phase + (time - self.change_at).as_secs_f32() / (1.0 / self.pitch) * std::f32::consts::TAU
    }
}

impl Note for HeldButtonNote {
    fn set_param(&mut self, param: NoteParam) {
        self.with_change(|change| 
            match param {
                NoteParam::Pitch(pitch) => {
                    change.pitch = Some(pitch);
                },
                NoteParam::Amplitude(amp) => {
                    change.amplitude = Some(amp);
                },
                _ => {}
            }
        );
    }

    fn mute(&mut self) {
        self.with_change(|change| change.mute = true);
    }

    fn render(&mut self, time: Duration) -> f32 {
        if let Some(change) = self.change_pending.take() {
            // convert the old change_at/change_phase into new time/phase
            self.change_phase = self.phase(time) % std::f32::consts::TAU;
            self.change_at = time;
            if let Some(pitch) = change.pitch {
                self.pitch = pitch;
            }
            if let Some(amplitude) = change.amplitude {
                self.amplitude = amplitude;
            }
            if change.mute {
                self.mute_at = Some(time);
            }
        }

        let amp = self.phase(time).sin();
        let adsr = if let Some(release) = self.mute_at {
            if time >= release {
                (1.0 - (time - release).as_secs_f32() / Duration::from_millis(500).as_secs_f32()) * 0.5
            } else if time < Duration::from_millis(50) {
                time.as_secs_f32() / Duration::from_millis(50).as_secs_f32()
            } else if time < Duration::from_millis(100) {
                (1.0 - (time - Duration::from_millis(50)).as_secs_f32() / Duration::from_millis(50).as_secs_f32()) * 0.5 + 0.5
            } else {
                0.5
            }
        } else {
            if time < Duration::from_millis(50) {
                time.as_secs_f32() / Duration::from_millis(50).as_secs_f32()
            } else if time < Duration::from_millis(100) {
                (1.0 - (time - Duration::from_millis(50)).as_secs_f32() / Duration::from_millis(50).as_secs_f32()) * 0.5 + 0.5
            } else {
                0.5
            }
        };
        amp * adsr * self.amplitude
    }

    fn finished(&mut self, time: Duration) -> bool {
        if let Some(mute_at) = self.mute_at {
            mute_at + Duration::from_millis(500) < time
        } else {
            false
        }
    }
}

impl Instrument for HeldButtonInstrument {
    fn set_param(&mut self, param: InstrumentParam) {
        match param {
            InstrumentParam::NextNote(NoteParam::Pitch(pitch)) => {
                self.next_pitch = pitch;
            },
            _ => {}
        }
    }

    fn note(&mut self, _voice: u32) -> Box<dyn Note> {
        Box::new(HeldButtonNote {
            pitch: self.next_pitch,
            amplitude: self.amplitude,
            mute_at: None,
            change_phase: 0.0,
            change_at: Duration::from_secs(0),
            change_pending: None,
        })
    }
}

pub fn setup_instruments() -> Vec<Box<dyn Instrument>> {
    vec![Box::new(HeldButtonInstrument::new())]
}

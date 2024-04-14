use std::{time::Duration, collections::HashMap};
use dasp::Signal;

use crate::render::FrameInstant;

pub trait CloneSignal: Signal {
    fn clone(&self) -> Box<dyn CloneSignal<Frame = Self::Frame>>;
}

impl<T, S> CloneSignal for T where T: 'static + Clone + Signal<Frame = S> {
    fn clone(&self) -> Box<dyn CloneSignal<Frame = Self::Frame>> {
        Box::new(self.clone())
    }
}

pub enum NoteParam {
    Pitch(f32),
    Amplitude(f32),
    Articulation(f32),
    Other(String, MiscValue),
}

#[derive(Clone, Debug)]
pub enum MiscValue {
    Float(f32),
    String(String),
}

#[derive(Clone, Debug)]
pub struct NoteParams {
    pub pitch: f32,
    pub amplitude: f32,
    pub articulation: f32,
    pub other: HashMap<String, MiscValue>,
}

impl Default for NoteParams {
    fn default() -> Self {
        Self { pitch: 44.0, amplitude: 0.1, articulation: 0.5, other: HashMap::new() }
    }
}

impl NoteParams {
    pub fn apply(&mut self, param: NoteParam) {
        match param {
            NoteParam::Pitch(v) => self.pitch = v,
            NoteParam::Amplitude(v) => self.amplitude = v,
            NoteParam::Articulation(v) => self.articulation = v,
            NoteParam::Other(k, v) => { self.other.insert(k, v); }
        }
    }
}

pub enum InstrumentParam {
    NextNote(NoteParam),
    Other(String, MiscValue),
}

pub trait Note {
    fn set_param(&mut self, param: NoteParam);
    fn mute(&mut self);
    fn render(&mut self, time: FrameInstant) -> f32;
    fn finished(&mut self, time: FrameInstant) -> bool;
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

type MySignal = Box<dyn CloneSignal<Frame = f32>>;
type SignalMaker = Box<dyn Send + Fn(&NoteParams, FrameInstant, f32, &NoteParams, FrameInstant) -> (MySignal, f32)>; // (old_params, old_time, phase, new_params, new_time)
type SignalMakerMaker = Box<dyn Send + Fn(u32) -> SignalMaker>; // (sample_rate) -> maker

pub struct HeldButtonInstrument {
    next_note: NoteParams,
    signal_maker_maker: SignalMakerMaker,
    sample_rate: u32,
}

impl HeldButtonInstrument {
    pub fn new(sample_rate: u32, signal_maker_maker: SignalMakerMaker) -> Self {
        Self {
            next_note: NoteParams::default(),
            signal_maker_maker,
            sample_rate,
        }
    }
}

pub struct HeldButtonNote {
    signal: MySignal,
    next_frame: FrameInstant,
    params: NoteParams,
    mute_at: Option<FrameInstant>,
    change_at: FrameInstant,
    change_pending: bool,
    mute_pending: bool,
    change_params: NoteParams,
    change_phase: f32,
    signal_maker: SignalMaker,
    sample_rate: u32,
}

impl HeldButtonNote {
    fn to_fsecs(&self, duration: FrameInstant) -> f32 {
        (duration as f32) * (self.sample_rate as f32)
    }

    fn from_duration(&self, duration: Duration) -> FrameInstant {
        (duration.as_secs_f32() / (self.sample_rate as f32)) as FrameInstant
    }
}

impl Note for HeldButtonNote {
    fn set_param(&mut self, param: NoteParam) {
        self.change_pending = true;
        self.change_params.apply(param);
    }

    fn mute(&mut self) {
        self.mute_pending = true;
    }

    fn render(&mut self, time: FrameInstant) -> f32 {
        if self.change_pending {
            (self.signal, self.change_phase) = (self.signal_maker)(&self.params, self.change_at, self.change_phase, &self.change_params, time);
            self.params = self.change_params.clone();
            self.change_at = time;
            self.next_frame = time;
            self.change_pending = false;
        }
        if self.mute_pending {
            self.mute_at = Some(time);
            self.mute_pending = false;
        }

        if self.next_frame == time {
            self.next_frame += 1;
        } else {
            (self.signal, self.change_phase) = (self.signal_maker)(&self.params, self.change_at, self.change_phase, &self.params, time);
            self.next_frame = time + 1;
        }

        let amp = self.signal.next();
        let adsr = if let Some(release) = self.mute_at {
            if time >= release {
                (1.0 - self.to_fsecs(time - release) / Duration::from_millis(500).as_secs_f32()) * 0.5
            } else if time < self.from_duration(Duration::from_millis(50)) {
                self.to_fsecs(time) / Duration::from_millis(50).as_secs_f32()
            } else if time < self.from_duration(Duration::from_millis(100)) {
                (1.0 - self.to_fsecs(time - self.from_duration(Duration::from_millis(50))) / Duration::from_millis(50).as_secs_f32()) * 0.5 + 0.5
            } else {
                0.5
            }
        } else {
            if time < self.from_duration(Duration::from_millis(50)) {
                self.to_fsecs(time) / Duration::from_millis(50).as_secs_f32()
            } else if time < self.from_duration(Duration::from_millis(100)) {
                (1.0 - self.to_fsecs(time - self.from_duration(Duration::from_millis(50))) / Duration::from_millis(50).as_secs_f32()) * 0.5 + 0.5
            } else {
                0.5
            }
        };
        amp * adsr
    }

    fn finished(&mut self, time: FrameInstant) -> bool {
        if let Some(mute_at) = self.mute_at {
            mute_at + self.from_duration(Duration::from_millis(500)) < time
        } else {
            false
        }
    }
}

impl Instrument for HeldButtonInstrument {
    fn set_param(&mut self, param: InstrumentParam) {
        match param {
            InstrumentParam::NextNote(note_param) => {
                self.next_note.apply(note_param);
            },
            _ => {}
        }
    }

    fn note(&mut self, _voice: u32) -> Box<dyn Note> {
        let signal_maker = (self.signal_maker_maker)(self.sample_rate);
        let (signal, change_phase) = (signal_maker)(&self.next_note, 0, 0.0, &self.next_note, 0);
        Box::new(HeldButtonNote {
            mute_at: None,
            change_phase,
            change_at: 0,
            change_pending: false,
            signal_maker,
            signal,
            next_frame: 0,
            params: self.next_note.clone(),
            mute_pending: false,
            change_params: self.next_note.clone(),
            sample_rate: self.sample_rate,
        })
    }
}

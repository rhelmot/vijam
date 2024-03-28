use std::collections::{BTreeMap, HashSet};
use std::sync::{mpsc, Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleRate, SizedSample};
use thread_priority::{ThreadBuilderExt, ThreadPriority};
use vizia::prelude::*;

const MAX_BUFFER_CONSUME_SIZE: usize = 256; // this corresponds to a little more than 5ms at 44100Hz
const MAX_BUFFER_SPECULATE_SIZE: usize = 1024;
const BACKOFF_SLEEP: Duration = Duration::from_millis(1);
//const IDEAL_LATENCY: f32 = 0.01; // 10ms

pub struct RenderQueue {
    pub buffer: dasp::ring_buffer::Bounded<Box<[f32]>>,
    pub tail_frame: u64,
    pub last_consumed_size: u64,
    pub sample_rate: SampleRate,
    pub start_time: Instant,
}

impl RenderQueue {
    pub fn new(sample_rate: SampleRate, start_time: Instant) -> Self {
        RenderQueue {
            buffer: dasp::ring_buffer::Bounded::from_raw_parts(
                0,
                0,
                Box::from([0f32; MAX_BUFFER_SPECULATE_SIZE]),
            ),
            last_consumed_size: 0,
            tail_frame: 0,
            sample_rate,
            start_time,
        }
    }

    pub fn sample_length(&self) -> Duration {
        Duration::from_secs_f32(1f32 / self.sample_rate.0 as f32)
    }

    fn plus_sample_time(&self, samples_elapsed: u64) -> Instant {
        let frame = self.tail_frame + samples_elapsed;
        self.start_time
            + self.sample_length() * frame as u32
            + self.sample_length().mul_f32((frame >> 32) as f32)
    }

    /// The current timestamp at the head of the buffer, i.e. the insertion point
    pub fn head_time(&self) -> Instant {
        self.plus_sample_time(self.last_consumed_size + self.buffer.len() as u64)
    }

    /// The current timestamp at the head of the buffer, i.e. the extraction point
    pub fn tail_time(&self) -> Instant {
        self.plus_sample_time(self.last_consumed_size)
    }
}

#[derive(Lens)]
pub struct VizData {
    pressed: HashSet<Code>,
    event_submission: mpsc::Sender<Option<JamEvent>>,
}

/*
pub enum Scale {
    Major,
    Minor,
}

pub enum EqualTempermentKey {
    A,
    Bb,
    B,
    C,
    Db,
    D,
    Eb,
    E,
    F,
    Gb,
    G,
    Ab,
}

impl EqualTempermentKey {
    pub fn pitch(&self, octave: ) -> f32 {
        match self {
        }
    }
}
*/

fn code_to_pitch_and_voice(code: Code) -> Option<(f32, u32)> {
    let note = match code {
        Code::KeyA => 0,
        Code::KeyS => 2,
        Code::KeyD => 4,
        Code::KeyF => 5,
        Code::KeyJ => 7,
        Code::KeyK => 9,
        Code::KeyL => 11,
        Code::Semicolon => 12,
        _ => return None,
    };
    Some((440.0 * f32::powf(2.0, note as f32 / 12.0), note))
}

impl Model for VizData {
    fn event(&mut self, _cx: &mut EventContext, event: &mut Event) {
        event.map(|window_event, _| match window_event {
            WindowEvent::KeyDown(code, _) => {
                let code = *code;
                if !self.pressed.insert(code) {
                    return;
                }
                let Some((pitch, voice)) = code_to_pitch_and_voice(code) else {
                    return;
                };
                self.event_submission
                    .send(Some(JamEvent::InstrumentEvent {
                        instrument: 0,
                        event: InstrumentEvent::SetParam {
                            param: InstrumentParam::NextNote(NoteParam::Pitch(pitch)),
                        },
                    }))
                    .unwrap();
                self.event_submission
                    .send(Some(JamEvent::InstrumentEvent {
                        instrument: 0,
                        event: InstrumentEvent::NoteEvent {
                            voice,
                            event: NoteEvent::Hit {},
                        },
                    }))
                    .unwrap();
            }
            WindowEvent::KeyUp(code, _) => {
                let code = *code;
                if !self.pressed.remove(&code) {
                    return;
                }
                let Some((_, voice)) = code_to_pitch_and_voice(code) else {
                    return;
                };
                self.event_submission
                    .send(Some(JamEvent::InstrumentEvent {
                        instrument: 0,
                        event: InstrumentEvent::NoteEvent {
                            voice,
                            event: NoteEvent::Mute {},
                        },
                    }))
                    .unwrap();
            }
            _ => {}
        });
    }
}

impl VizData {
    fn new(event_submission: mpsc::Sender<Option<JamEvent>>) -> Self {
        Self {
            pressed: HashSet::new(),
            event_submission,
        }
    }
}

pub fn stream_setup_for() -> Result<(cpal::Stream, Arc<Mutex<RenderQueue>>), anyhow::Error>
where
{
    let (_host, device, config) = host_device_setup()?;
    let fmt = config.sample_format();
    let mut config: cpal::StreamConfig = config.into();
    config.buffer_size = cpal::BufferSize::Fixed(MAX_BUFFER_CONSUME_SIZE as u32);

    match fmt {
        cpal::SampleFormat::I8 => make_stream::<i8>(&device, &config),
        cpal::SampleFormat::I16 => make_stream::<i16>(&device, &config),
        cpal::SampleFormat::I32 => make_stream::<i32>(&device, &config),
        cpal::SampleFormat::I64 => make_stream::<i64>(&device, &config),
        cpal::SampleFormat::U8 => make_stream::<u8>(&device, &config),
        cpal::SampleFormat::U16 => make_stream::<u16>(&device, &config),
        cpal::SampleFormat::U32 => make_stream::<u32>(&device, &config),
        cpal::SampleFormat::U64 => make_stream::<u64>(&device, &config),
        cpal::SampleFormat::F32 => make_stream::<f32>(&device, &config),
        cpal::SampleFormat::F64 => make_stream::<f64>(&device, &config),
        sample_format => Err(anyhow::Error::msg(format!(
            "Unsupported sample format '{sample_format}'"
        ))),
    }
}

pub fn host_device_setup(
) -> Result<(cpal::Host, cpal::Device, cpal::SupportedStreamConfig), anyhow::Error> {
    let host = cpal::default_host();

    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow::Error::msg("Default output device is not available"))?;
    println!("Output device : {}", device.name()?);

    let config = device.default_output_config()?;
    println!("Default output config : {:?}", config);

    Ok((host, device, config))
}

pub fn make_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
) -> Result<(cpal::Stream, Arc<Mutex<RenderQueue>>), anyhow::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let num_channels = config.channels as usize;

    let buf = Arc::new(Mutex::new(RenderQueue::new(
        config.sample_rate,
        Instant::now(),
    )));

    let stream = device.build_output_stream(
        config,
        {
            let buf = buf.clone();
            move |output: &mut [T], info: &cpal::OutputCallbackInfo| {
                let num_frames = output.len() / num_channels;
                assert!(num_frames <= MAX_BUFFER_CONSUME_SIZE);
                loop {
                    let buf = buf.lock().unwrap();
                    if buf.buffer.len() >= num_frames {
                        break;
                    }
                    let ts = info.timestamp();
                    if ts.playback.sub(BACKOFF_SLEEP) > Some(ts.callback) {
                        std::thread::sleep(BACKOFF_SLEEP);
                    } else {
                        break;
                    }
                }
                let mut buf = buf.lock().unwrap();

                if buf.buffer.len() >= num_frames {
                    for frame in output.chunks_mut(num_channels) {
                        let rawval = buf.buffer.pop().unwrap();
                        let value = T::from_sample(rawval);
                        for sample in frame.iter_mut() {
                            *sample = value;
                        }
                    }
                    buf.last_consumed_size = num_frames as u64;
                    buf.tail_frame += num_frames as u64;
                } else {
                    buf.last_consumed_size = 0;
                }
            }
        },
        |err| {
            panic!("{:?}", err);
        },
        None,
    )?;

    Ok((stream, buf))
}

#[non_exhaustive]
pub enum JamParam {
    Tempo(f32),
    OtherFloat(String, f32),
    OtherString(String, String),
}

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

pub enum JamEvent {
    InstrumentEvent {
        instrument: u32,
        event: InstrumentEvent,
    },
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

pub fn setup_rendering(
    buf: Arc<Mutex<RenderQueue>>,
    mut instruments: Vec<Box<dyn Instrument>>,
) -> mpsc::Sender<Option<JamEvent>> {
    let (send, recv) = mpsc::channel();

    std::thread::Builder::new()
        .name("rendering".to_string())
        .spawn_with_priority(ThreadPriority::Max, move |result| {
            if let Err(e) = result {
                eprintln!("Warning: Could not set thread priority: {e}")
            }
            let mut voices = BTreeMap::<(u32, u32), (Instant, Box<dyn Note>)>::new();
            loop {
                for event in recv.try_iter() {
                    let Some(event) = event else { return };
                    let now = buf.lock().unwrap().head_time();
                    match event {
                        JamEvent::InstrumentEvent {
                            instrument: iid,
                            event,
                        } => {
                            let Some(instrument) = instruments.get_mut(iid as usize) else {
                                eprintln!("Warning: event on nonexistent instrument");
                                continue;
                            };
                            match event {
                                InstrumentEvent::SetParam { param } => {
                                    instrument.set_param(param);
                                }
                                InstrumentEvent::NoteEvent { voice, event } => {
                                    match event {
                                        NoteEvent::Hit {} => {
                                            let note = instrument.note(voice);
                                            if let Some((_, mut oldnote)) =
                                                voices.insert((iid, voice), (now, note))
                                            {
                                                // idk if necessary lol
                                                oldnote.mute();
                                            }
                                        }
                                        NoteEvent::SetParam { param } => {
                                            let Some((_, note)) = voices.get_mut(&(iid, voice)) else {
                                                eprintln!("Warning: event on nonexistent note");
                                                continue;
                                            };
                                            note.set_param(param);
                                        }
                                        NoteEvent::Mute {} => {
                                            let Some((_, note)) = voices.get_mut(&(iid, voice)) else {
                                                eprintln!("Warning: event on nonexistent note");
                                                continue;
                                            };
                                            note.mute();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let mut buf = buf.lock().unwrap();
                let now = buf.head_time();
                let retired = buf.tail_time();
                if buf.buffer.len() == buf.buffer.max_len() {
                    continue;
                }

                let mut result = 0f32;
                voices.retain(|(_, _), (ts, note)| {
                    if note.finished(retired - *ts) {
                        return false;
                    }
                    result += note.render(now - *ts);
                    true
                });
                buf.buffer.push(result);
            }
        })
        .unwrap();

    send
}

fn setup_instruments() -> Vec<Box<dyn Instrument>> {
    vec![Box::new(HeldButtonInstrument::new())]
}

fn main() -> anyhow::Result<()> {
    let (stream, buf) = stream_setup_for()?;
    let instruments = setup_instruments();
    let event_submission = setup_rendering(buf, instruments);
    stream.play()?;
    Application::new(|cx| {
        VizData::new(event_submission).build(cx);
        HStack::new(cx, |_| {})
            .size(Pixels(50.0))
            .lock_focus_to_within();
    })
    .run()
    .unwrap();
    Ok(())
}

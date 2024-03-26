use std::sync::{mpsc, Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleRate, SizedSample};
use vizia::prelude::*;
use thread_priority::{ThreadBuilderExt, ThreadPriority};

const MAX_BUFFER_CONSUME_SIZE: usize = 256; // this corresponds to a little more than 5ms at 44100Hz
const MAX_BUFFER_SPECULATE_SIZE: usize = 1024;
const BACKOFF_SLEEP: Duration = Duration::from_millis(1);
//const IDEAL_LATENCY: f32 = 0.01; // 10ms

pub struct RenderQueue {
    pub buffer: dasp::ring_buffer::Bounded<Box<[f32]>>,
    pub tail_frame: usize,
    pub last_consumed_size: usize,
    pub sample_rate: SampleRate,
}

impl RenderQueue {
    pub fn new(sample_rate: SampleRate) -> Self {
        RenderQueue {
            buffer: dasp::ring_buffer::Bounded::from_raw_parts(
                0,
                0,
                Box::from([0f32; MAX_BUFFER_SPECULATE_SIZE]),
            ),
            last_consumed_size: 0,
            tail_frame: 0,
            sample_rate,
        }
    }

    pub fn sample_length(&self) -> f32 {
        1f32 / self.sample_rate.0 as f32
    }

    fn plus_sample_time(&self, samples_elapsed: usize) -> f32 {
        let frame = self.tail_frame + samples_elapsed;
        self.sample_length() * frame as f32
    }

    /// The current timestamp at the head of the buffer, i.e. the insertion point
    pub fn head_time(&self) -> f32 {
        self.plus_sample_time(self.last_consumed_size + self.buffer.len())
    }

    /// The current timestamp at the head of the buffer, i.e. the extraction point
    pub fn tail_time(&self) -> f32 {
        self.plus_sample_time(self.last_consumed_size)
    }
}

#[derive(Lens)]
pub struct VizData {
    tone: bool,
    event_submission: mpsc::Sender<Option<JamEvent>>,
}

impl Model for VizData {
    fn event(&mut self, _cx: &mut EventContext, event: &mut Event) {
        event.map(|window_event, _| match window_event {
            WindowEvent::KeyDown(Code::Space, _) => {
                if self.tone {
                    return;
                }
                self.tone = true;
                self.event_submission.send(Some(JamEvent::Press)).unwrap();
            }
            WindowEvent::KeyUp(Code::Space, _) => {
                self.tone = false;
                self.event_submission.send(Some(JamEvent::Release)).unwrap();
            }
            _ => {}
        });
    }
}

impl VizData {
    fn new(event_submission: mpsc::Sender<Option<JamEvent>>) -> Self {
        Self {
            tone: false,
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

    let buf = Arc::new(Mutex::new(RenderQueue::new(config.sample_rate)));

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
                    buf.last_consumed_size = num_frames;
                    buf.tail_frame += num_frames;
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

pub enum JamEvent {
    Press,
    Release,
}

pub fn setup_rendering(buf: Arc<Mutex<RenderQueue>>) -> mpsc::Sender<Option<JamEvent>> {
    let (send, recv) = mpsc::channel();

    std::thread::Builder::new().name("rendering".to_string()).spawn_with_priority(ThreadPriority::Max, move |result| {
        if let Err(e) = result {
            eprintln!("Warning: Could not set thread priority: {e}")
        }
        let mut time_on = None;
        let mut time_off = time_on;
        loop {
            for event in recv.try_iter() {
                let Some(event) = event else { return };
                let now = buf.lock().unwrap().head_time();
                match event {
                    JamEvent::Press => {
                        time_on = Some(now);
                        time_off = None;
                    }
                    JamEvent::Release => {
                        time_off = Some(now);
                    }
                }
            }

            let mut buf = buf.lock().unwrap();
            let now = buf.head_time();
            if buf.buffer.len() == buf.buffer.max_len() {
                continue;
            }

            if let Some(time_on) = time_on {
                let time_float = now - time_on;
                let period = 1.0 / 440.0;
                let amp = (time_float * std::f32::consts::TAU / period).sin() * 0.7;

                if let Some(time_off) = time_off {
                    let decay_time = now - time_off;
                    let envelope = (1.0 - decay_time / 0.3).clamp(0.0, 1.0);
                    buf.buffer.push(amp * envelope);
                } else {
                    buf.buffer.push(amp);
                }
            } else {
                buf.buffer.push(0f32);
            }
        }
    }).unwrap();

    send
}

fn main() -> anyhow::Result<()> {
    let (stream, buf) = stream_setup_for()?;
    stream.play()?;
    let event_submission = setup_rendering(buf);
    Application::new(|cx| {
        VizData::new(event_submission).build(cx);
        HStack::new(cx, |_| {})
            .background_color(VizData::tone.map(
                |tone| {
                    if *tone {
                        Color::blue()
                    } else {
                        Color::white()
                    }
                },
            ))
            .size(Pixels(50.0))
            .lock_focus_to_within();
    })
    .run()
    .unwrap();
    Ok(())
}

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{FromSample, SizedSample};
use std::time::Duration;
use std::sync::{Arc, Mutex};
use crate::render::RenderQueue;

const MAX_BUFFER_CONSUME_SIZE: usize = 256; // this corresponds to a little more than 5ms at 44100Hz
const BACKOFF_SLEEP: Duration = Duration::from_millis(1);

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

    let buf = Arc::new(Mutex::new(RenderQueue::new()));

    let stream = device.build_output_stream(
        config,
        {
            let buf = buf.clone();
            move |output: &mut [T], info: &cpal::OutputCallbackInfo| {
                let num_frames = output.len() / num_channels;
                println!("{num_frames}");
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


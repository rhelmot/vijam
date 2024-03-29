use cpal::traits::StreamTrait;

mod render;
mod output;
mod input;
mod instrument;

#[non_exhaustive]
pub enum JamParam {
    Tempo(f32),
    OtherFloat(String, f32),
    OtherString(String, String),
}

pub enum JamEvent {
    InstrumentEvent {
        instrument: u32,
        event: instrument::InstrumentEvent,
    },
}

fn main() -> anyhow::Result<()> {
    let (stream, buf) = output::stream_setup_for()?;
    let instruments = instrument::setup_instruments();
    let event_submission = render::setup_rendering(buf, instruments);
    stream.play()?;
    input::setup_input(event_submission).run().unwrap();
    Ok(())
}

use std::path::PathBuf;

use cpal::traits::StreamTrait;
use clap::{Parser, Subcommand};

mod config;
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

#[derive(Parser, Debug)]
#[command(version)]
struct Cli {
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Start {
        config: PathBuf,
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start { config } => {
            main_start(config)
        },
    }
}

fn main_start(config: PathBuf) -> anyhow::Result<()> {
    let (config, instruments) = config::JamConfig::new(config)?;
    let (stream, buf) = output::stream_setup_for()?;
    let event_submission = render::setup_rendering(buf, instruments);
    stream.play()?;
    input::setup_input(event_submission, config).run().unwrap();
    Ok(())
}

use std::path::PathBuf;

use cpal::traits::StreamTrait;
use clap::{Parser, Subcommand};

mod config;
mod render;
mod output;
mod input;
mod instrument;

#[non_exhaustive]
#[derive(Debug)]
pub enum JamParam {
    Tempo(f64),
    OtherFloat(String, f64),
    OtherString(String, String),
}

#[derive(Debug)]
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
    let (stream, buf, sample_rate) = output::stream_setup_for()?;
    let (mut config, instruments) = config::JamConfig::new(config, sample_rate)?;
    let event_submission = render::setup_rendering(buf, instruments);
    config.setup(event_submission);
    stream.play()?;
    input::setup_input(config).run().unwrap();
    Ok(())
}

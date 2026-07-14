mod audio;
mod commands;
mod config;
mod context;
mod dashboard;
mod dictation;
mod events;
mod feedback;
mod keyboard;
mod moonshine;
mod parakeet;
mod paste;
mod recognition;
mod suppression;

use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::sync::Mutex;

use clap::{Parser, Subcommand};
use color_eyre::Result;
use tracing_subscriber::fmt::writer::MakeWriterExt;

#[derive(Parser)]
#[command(version, about = "Local, observable voice control")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Listen and transcribe until interrupted.
    Listen {
        /// Override the configured microphone preference order.
        #[arg(long)]
        device: Option<String>,
    },
    /// Show the live recognition dashboard.
    Status,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let log_dir = root.join("logs");
    fs::create_dir_all(&log_dir)?;
    let process_log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("process.log"))?;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("voice_control=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr.and(Mutex::new(process_log)))
        .init();

    let event_path = root.join("logs/live.ndjson");
    match Cli::parse()
        .command
        .unwrap_or(Command::Listen { device: None })
    {
        Command::Listen { device } => recognition::listen(
            &root,
            &event_path,
            device.as_deref(),
            config::voice_control(),
        ),
        Command::Status => dashboard::run(event_path, config::voice_control()),
    }
}

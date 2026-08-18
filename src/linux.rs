use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clap::{Parser, Subcommand};
use color_eyre::Result;
use tracing_subscriber::fmt::writer::MakeWriterExt;

use crate::audio::{AudioInput, AudioInputEvent};
use crate::events::{EventLog, TranscriptPhase, VoiceEvent, VoiceState, now_ms};
use crate::moonshine::Moonshine;

#[derive(Parser)]
#[command(version, about = "Local, observable voice recognition (Linux beta)")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Open the Linux X11 transcription shell.
    App {
        /// Start with only the tray icon visible.
        #[arg(long)]
        hidden: bool,
    },
    /// Listen to the microphone and print local transcripts until interrupted.
    Listen {
        /// Select an input device by a case-insensitive name fragment.
        #[arg(long)]
        device: Option<String>,
    },
    /// Run X11 hotkey dictation and automatic paste until interrupted.
    Dictate {
        /// Select an input device by a case-insensitive name fragment.
        #[arg(long)]
        device: Option<String>,
    },
    /// List microphone devices visible through ALSA.
    Devices,
    /// Print recent recognition observations.
    Status {
        /// Maximum number of observations to print.
        #[arg(long, default_value_t = 40)]
        lines: usize,
    },
    /// Install, inspect, and validate the local dictation model.
    Model {
        #[command(subcommand)]
        command: ModelCommand,
    },
}

#[derive(Subcommand)]
enum ModelCommand {
    /// Show whether the default dictation model is installed.
    Status,
    /// Download and checksum-verify the default dictation model.
    Install,
    /// List transcribe.cpp compute devices available to HEX.
    Devices,
    /// Load and prewarm the installed model.
    Check,
    /// Transcribe a WAV file with the installed model.
    Transcribe { wav: PathBuf },
}

pub fn run(shutdown: &'static AtomicBool) -> Result<()> {
    color_eyre::install()?;
    let log_dir = crate::app_paths::logs_dir()?;
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
    ctrlc::set_handler(|| shutdown.store(true, Ordering::Relaxed))?;

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let event_path = log_dir.join("live.ndjson");
    match Cli::parse()
        .command
        .unwrap_or(Command::App { hidden: false })
    {
        Command::App { hidden } => crate::linux_app::open(event_path, hidden),
        Command::Listen { device } => {
            let _instance = crate::instance::acquire("listener")?;
            listen(&root, &event_path, device.as_deref(), shutdown)
        }
        Command::Dictate { device } => {
            let _instance = crate::instance::acquire("listener")?;
            crate::linux_dictation::run(&event_path, device.as_deref(), shutdown)
        }
        Command::Devices => {
            for device in crate::audio::input_device_names()? {
                println!("{device}");
            }
            Ok(())
        }
        Command::Status { lines } => print_status(&event_path, lines),
        Command::Model { command } => match command {
            ModelCommand::Status => {
                let model = crate::local_transcriber::default_model();
                println!(
                    "{}\t{}\t{}",
                    if crate::transcription_models::is_installed(model, "en") {
                        "installed"
                    } else {
                        "missing"
                    },
                    model.name,
                    crate::transcription_models::model_path(model)?.display()
                );
                Ok(())
            }
            ModelCommand::Install => {
                crate::local_transcriber::install(
                    &crate::transcription_models::TranscriptionSelection::default(),
                    shutdown,
                )?;
                Ok(())
            }
            ModelCommand::Devices => {
                for device in crate::local_transcriber::devices()? {
                    println!("{device}");
                }
                Ok(())
            }
            ModelCommand::Check => {
                let transcriber = crate::local_transcriber::LocalTranscriber::load_default()?;
                println!("ready\t{}", transcriber.device_label());
                Ok(())
            }
            ModelCommand::Transcribe { wav } => {
                println!("{}", crate::local_transcriber::transcribe_wav(&wav)?);
                Ok(())
            }
        },
    }
}

pub(crate) fn listen(
    project_root: &Path,
    event_path: &Path,
    device: Option<&str>,
    shutdown: &AtomicBool,
) -> Result<()> {
    shutdown.store(false, Ordering::Relaxed);
    let mut recognizer = Moonshine::load(project_root)?;
    let input = match device {
        Some(name) => AudioInput::open_matching(name)?,
        None => AudioInput::open(&[])?,
    };
    let events = EventLog::create(event_path)?;
    events.emit(&VoiceEvent::SessionStarted {
        timestamp_ms: now_ms(),
    })?;
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state: VoiceState::Listening,
        device: input.device_name.clone(),
    })?;
    println!("Listening on {}. Press Ctrl-C to stop.", input.device_name);

    while !shutdown.load(Ordering::Relaxed) {
        let chunk = match input.recv_timeout(Duration::from_millis(100)) {
            AudioInputEvent::Chunk { samples, .. } => samples,
            AudioInputEvent::Timeout => continue,
            AudioInputEvent::StreamFailed(error) => {
                return Err(color_eyre::eyre::eyre!(
                    "microphone stream stopped: {error}"
                ));
            }
        };
        recognizer.add_audio(&chunk, input.sample_rate)?;
        for update in recognizer.update()? {
            events.emit(&VoiceEvent::Transcript {
                timestamp_ms: now_ms(),
                phase: update.phase,
                latency_ms: update.latency_ms,
                text: update.text.clone(),
            })?;
            if update.phase == TranscriptPhase::Completed {
                println!("{}", update.text);
            } else if !update.text.is_empty() {
                print!("\r{}", update.text);
                use std::io::Write;
                std::io::stdout().flush()?;
            }
        }
    }

    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state: VoiceState::Stopping,
        device: input.device_name.clone(),
    })?;
    println!("\nStopped.");
    Ok(())
}

fn print_status(path: &Path, limit: usize) -> Result<()> {
    if !path.exists() {
        println!("No recognition observations yet at {}", path.display());
        return Ok(());
    }
    let lines = BufReader::new(std::fs::File::open(path)?)
        .lines()
        .collect::<std::io::Result<Vec<_>>>()?;
    for line in lines.iter().skip(lines.len().saturating_sub(limit)) {
        println!("{line}");
    }
    Ok(())
}

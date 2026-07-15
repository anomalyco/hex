mod app_settings;
mod app_window;
mod audio;
mod commands;
mod config;
mod context;
mod dashboard;
mod dictation;
mod dictation_indicator;
mod events;
mod feedback;
mod instance;
mod keyboard;
mod meeting;
mod meeting_detection;
mod meeting_watcher;
mod microphone_activity;
mod moonshine;
mod parakeet;
mod paste;
mod recognition;
mod suppression;

use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::{Parser, Subcommand};
use color_eyre::Result;
use tracing_subscriber::fmt::writer::MakeWriterExt;

#[derive(Parser)]
#[command(version, about = "Local, observable voice control")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

#[derive(Subcommand)]
enum Command {
    /// Run the GPUI desktop app with recognition and meeting detection.
    App {
        /// Override the configured microphone preference order.
        #[arg(long)]
        device: Option<String>,
        /// Preview the dictation HUD without starting recognition.
        #[arg(long)]
        preview_dictation: bool,
    },
    /// Listen and transcribe until interrupted.
    Listen {
        /// Override the configured microphone preference order.
        #[arg(long)]
        device: Option<String>,
    },
    /// Show the live recognition dashboard.
    Status,
    /// Record, transcribe, and browse local meetings.
    Meeting {
        #[command(subcommand)]
        command: MeetingCommand,
    },
}

#[derive(Subcommand)]
enum MeetingCommand {
    /// Record microphone and system audio until Ctrl-C.
    Record {
        /// Human-readable meeting title.
        #[arg(long)]
        title: Option<String>,
    },
    /// List recorded meetings.
    List,
    /// Print one meeting transcript.
    Show { id: String },
    /// Watch supported meeting applications and offer local recording.
    Watch {
        /// Show a non-recording UI preview immediately.
        #[arg(long)]
        preview: bool,
    },
    /// Print applications currently using microphone input.
    Probe,
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
    ctrlc::set_handler(|| SHUTDOWN.store(true, Ordering::Relaxed))?;

    let event_path = root.join("logs/live.ndjson");
    let cli = Cli::parse();
    let bundled_watcher = std::env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(|name| name == "voice-control-watch"))
        .unwrap_or(false);
    let command = cli.command.unwrap_or({
        if bundled_watcher {
            Command::App {
                device: None,
                preview_dictation: false,
            }
        } else {
            Command::Listen { device: None }
        }
    });
    match command {
        Command::App {
            device,
            preview_dictation,
        } => {
            let _instance = instance::acquire("listener")?;
            meeting_watcher::run(
                &SHUTDOWN,
                false,
                (!preview_dictation).then_some(meeting_watcher::ListenerConfig {
                    project_root: root,
                    event_path,
                    device,
                }),
                preview_dictation,
            )
        }
        Command::Listen { device } => {
            let _instance = instance::acquire("listener")?;
            let settings = app_settings::AppSettings::load()?;
            feedback::set_enabled(settings.sound_effects);
            recognition::listen(
                &root,
                &event_path,
                device.as_deref(),
                config::voice_control(),
                &SHUTDOWN,
                None,
            )
        }
        Command::Status => dashboard::run(event_path, config::voice_control()),
        Command::Meeting {
            command: MeetingCommand::Record { title },
        } => meeting::record(title, &SHUTDOWN).map(|_| ()),
        Command::Meeting {
            command: MeetingCommand::List,
        } => {
            for meeting in meeting::list()? {
                println!(
                    "{}\t{:?}\t{}ms\t{}",
                    meeting.id,
                    meeting.status,
                    meeting.duration_ms.unwrap_or_default(),
                    meeting.title
                );
            }
            Ok(())
        }
        Command::Meeting {
            command: MeetingCommand::Show { id },
        } => {
            print!("{}", meeting::show(&id)?);
            Ok(())
        }
        Command::Meeting {
            command: MeetingCommand::Watch { preview },
        } => meeting_watcher::run(&SHUTDOWN, preview, None, false),
        Command::Meeting {
            command: MeetingCommand::Probe,
        } => meeting_watcher::probe(),
    }
}

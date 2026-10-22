use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clap::{Parser, Subcommand};
use color_eyre::Result;

use crate::audio::{AudioInput, AudioInputEvent};
use crate::events::{EventLog, TranscriptPhase, VoiceEvent, VoiceState, now_ms};
use crate::linux_input::ShortcutCommand;
use crate::moonshine::Moonshine;

#[derive(Parser)]
#[command(version, about = "Local, observable voice recognition (Linux beta)")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Open Settings and connect to the per-user dictation service.
    App {
        /// Start the service without opening Settings (legacy autostart compatibility).
        #[arg(long)]
        hidden: bool,
    },
    /// Run the per-user runtime in the foreground (normally started by systemd).
    Service,
    /// Start the background service and enable listening.
    Start,
    /// Stop the background service.
    Stop,
    /// Restart the background service in this graphical session.
    Restart,
    /// Listen to the microphone and print local transcripts until interrupted.
    Listen {
        /// Select an input device by a case-insensitive name fragment.
        #[arg(long)]
        device: Option<String>,
    },
    /// Start, stop, or cancel a dictation in the running service. Bind these
    /// in your desktop's keyboard shortcuts when it has no shortcuts portal.
    Dictation {
        #[command(subcommand)]
        action: DictationAction,
    },
    /// Start or stop a Voice Action instruction in the running service.
    VoiceAction {
        #[command(subcommand)]
        action: VoiceActionAction,
    },
    /// Paste the last dictation again through the running service.
    PasteLast,
    /// Run X11 or Wayland hotkey dictation and automatic paste until interrupted.
    Dictate {
        /// Select an input device by a case-insensitive name fragment.
        #[arg(long)]
        device: Option<String>,
    },
    /// List microphone devices visible through ALSA.
    Devices,
    /// Query the live service, or print observations with --lines.
    Status {
        /// Maximum number of observations to print.
        #[arg(long)]
        lines: Option<usize>,
    },
    /// Install, inspect, and validate the local dictation model.
    Model {
        #[command(subcommand)]
        command: ModelCommand,
    },
}

#[derive(Subcommand)]
enum DictationAction {
    /// Start recording; does nothing while recording. Bind to a key press.
    Start,
    /// Stop and transcribe; does nothing while idle. Bind to the key's release.
    Stop,
    /// Start when idle, stop and transcribe while recording.
    Toggle,
    /// Discard the recording.
    Cancel,
}

#[derive(Subcommand)]
enum VoiceActionAction {
    /// Start recording an instruction. Bind to a key press.
    Start,
    /// Stop and run the instruction. Bind to the key's release.
    Stop,
    /// Start when idle, stop and run the instruction while recording.
    Toggle,
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
    // This is the Linux entry point, before signal handlers or any worker threads.
    // GTK and GPUI must connect to the same display backend.
    unsafe {
        std::env::set_var(
            "GDK_BACKEND",
            crate::linux_session::LinuxSession::detect().as_str(),
        );
    }
    color_eyre::install()?;
    let log_dir = crate::app_paths::init_process_logging(shutdown)?;

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let event_path = log_dir.join("live.ndjson");
    let result = (|| match Cli::parse()
        .command
        .unwrap_or(Command::App { hidden: false })
    {
        Command::App { hidden } => crate::linux_app::open(hidden, event_path, shutdown),
        Command::Service => crate::linux_app::run_service(event_path, shutdown),
        Command::Start => crate::linux_service::start(),
        Command::Stop => crate::linux_service::stop(),
        Command::Restart => crate::linux_service::restart(),
        Command::Listen { device } => {
            let _instance = crate::instance::acquire("listener")?;
            listen(&root, &event_path, device.as_deref(), shutdown)
        }
        Command::Dictation { action } => shortcut(match action {
            DictationAction::Start => ShortcutCommand::DictationStart,
            DictationAction::Stop => ShortcutCommand::DictationStop,
            DictationAction::Toggle => ShortcutCommand::DictationToggle,
            DictationAction::Cancel => ShortcutCommand::DictationCancel,
        }),
        Command::VoiceAction { action } => shortcut(match action {
            VoiceActionAction::Start => ShortcutCommand::VoiceActionStart,
            VoiceActionAction::Stop => ShortcutCommand::VoiceActionStop,
            VoiceActionAction::Toggle => ShortcutCommand::VoiceActionToggle,
        }),
        Command::PasteLast => shortcut(ShortcutCommand::PasteLast),
        Command::Dictate { device } => {
            let _instance = crate::instance::acquire("listener")?;
            let settings = crate::app_settings::AppSettings::load()?;
            crate::linux_dictation::initialize_feedback(&settings);
            crate::linux_dictation::run(
                &event_path,
                device.as_deref(),
                shutdown,
                &settings,
                crate::linux_dictation::DictationOutput::from_settings(
                    &settings,
                    crate::history::History::open_default(settings.history_retention).ok(),
                ),
                None,
                Default::default(),
                // Only shortcuts drive a foreground listener.
                std::sync::mpsc::channel().1,
            )
        }
        Command::Devices => {
            for device in crate::audio::input_device_names()? {
                println!("{device}");
            }
            Ok(())
        }
        Command::Status { lines: Some(lines) } => print_status(&event_path, lines),
        Command::Status { lines: None } => {
            let state = crate::linux_service::request(crate::linux_service::Request::Snapshot)?;
            println!("{}", serde_json::to_string_pretty(&state)?);
            Ok(())
        }
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
                crate::local_transcriber::install_default(shutdown)?;
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
    })();
    crate::linux_desktop::shutdown();
    result
}

/// Asks the running service; never starts one, so a keybinding fails fast.
fn shortcut(command: ShortcutCommand) -> Result<()> {
    crate::linux_service::request(crate::linux_service::Request::Shortcut(command))?;
    Ok(())
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

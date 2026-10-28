use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clap::{Parser, Subcommand};
use color_eyre::Result;

use crate::audio::{AudioInput, AudioInputEvent};
use crate::events::{EventLog, TranscriptPhase, VoiceEvent, VoiceState, now_ms};
use crate::linux_input::ShortcutCommand;
use crate::linux_service::{Activation, Launch, Request};
use crate::moonshine::Moonshine;

#[derive(Parser)]
#[command(version = crate::VERSION, about = "Local, observable voice recognition (Linux beta)")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Open the Hex window, starting Hex when it is not running (the default).
    App {
        /// Start Hex in the background without opening the window.
        #[arg(long)]
        hidden: bool,
        /// Arguments for a headless render, such as `--snap out.png --scene dark`.
        #[arg(last = true)]
        rune_args: Vec<String>,
    },
    /// Run Hex in the foreground without opening the window (systemd's entry point).
    Service,
    /// Start Hex in the background.
    Start,
    /// Quit Hex after its accepted work finishes (same as `hex quit`).
    Stop,
    /// Quit Hex after its accepted work finishes.
    Quit,
    /// Restart Hex's systemd unit in this graphical session.
    Restart,
    /// Listen to the microphone and print local transcripts until interrupted.
    Listen {
        /// Select an input device by a case-insensitive name fragment.
        #[arg(long)]
        device: Option<String>,
    },
    /// Start, stop, or cancel a dictation in the running Hex. Bind these
    /// in your desktop's keyboard shortcuts when it has no shortcuts portal.
    Dictation {
        #[command(subcommand)]
        action: DictationAction,
    },
    /// Start or stop a Voice Action instruction in the running Hex.
    VoiceAction {
        #[command(subcommand)]
        action: VoiceActionAction,
    },
    /// Paste the last dictation again through the running Hex.
    PasteLast,
    /// Run X11 or Wayland hotkey dictation and automatic paste until interrupted.
    Dictate {
        /// Select an input device by a case-insensitive name fragment.
        #[arg(long)]
        device: Option<String>,
    },
    /// List microphone devices visible through ALSA.
    Devices,
    /// Query the running Hex, or print observations with --lines.
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
    /// Read and change settings (macOS only for now; use the Hex window).
    Settings {
        #[command(subcommand)]
        command: crate::control::cli::SettingsCommand,
    },
    /// Turn unfinished, unsupported features on or off for your Hex.
    Experiments {
        #[command(subcommand)]
        command: crate::experiments::ExperimentsCommand,
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
    /// List transcribe.cpp compute devices available to Hex.
    Devices,
    /// Load and prewarm the installed model.
    Check,
    /// Transcribe a WAV file with the installed model.
    Transcribe { wav: PathBuf },
}

pub(crate) fn run(shutdown: &'static AtomicBool) -> Result<()> {
    let activation = crate::linux_service::Activation::take_from_environment();
    color_eyre::install()?;
    let command = Cli::parse().command.unwrap_or(Command::App {
        hidden: false,
        rune_args: Vec::new(),
    });
    // Like a read command: no log, lock, or running Hex.
    if let Command::Experiments { command } = &command {
        return crate::experiments::run(command);
    }
    let log_dir = crate::app_paths::init_process_logging(shutdown)?;

    let event_path = log_dir.join("live.ndjson");
    let client = command.is_client();
    let result = (|| match command {
        // Rune reads its own flags (`--snap`, `--scene`, …) from the process arguments.
        Command::App { rune_args, .. } if !rune_args.is_empty() => {
            crate::rune_ui::render();
            Ok(())
        }
        Command::App { hidden, .. } => launch(&activation, !hidden, event_path, shutdown),
        Command::Start => launch(&activation, false, event_path, shutdown),
        Command::Service => {
            match crate::linux_app::run(event_path, shutdown, None) {
                // A unit start that lost the race to another launch has nothing to do.
                Err(_) if crate::linux_service::another_instance_answers() => Ok(()),
                result => result,
            }
        }
        Command::Stop | Command::Quit => crate::linux_service::quit(),
        Command::Restart => crate::linux_service::restart(),
        Command::Listen { device } => {
            let _instance = crate::instance::acquire("listener")?;
            crate::experiments::log_launch();
            listen(&event_path, device.as_deref(), shutdown)
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
            crate::experiments::log_launch();
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
                std::sync::mpsc::channel().1.into(),
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
            let state = crate::linux_service::request(Request::Status)?;
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
        Command::Settings { command } => {
            crate::control::cli::finish(crate::control::cli::settings(command))
        }
        Command::Experiments { .. } => unreachable!("handled before logging"),
    })();
    match result {
        // A keybinding or script wants one line, not a report.
        Err(error) if client => {
            eprintln!("hex: {error:#}");
            std::process::exit(1);
        }
        result => result,
    }
}

impl Command {
    /// Commands that only talk to the running Hex or its systemd unit.
    fn is_client(&self) -> bool {
        matches!(
            self,
            Self::Stop
                | Self::Quit
                | Self::Restart
                | Self::Dictation { .. }
                | Self::VoiceAction { .. }
                | Self::PasteLast
                | Self::Status { lines: None }
        )
    }
}

/// Asks the running Hex; never starts one, so a keybinding fails fast.
fn shortcut(command: ShortcutCommand) -> Result<()> {
    crate::linux_service::request(Request::Shortcut(command))?;
    Ok(())
}

/// Forwards to the running Hex, starts its systemd unit, or becomes Hex.
fn launch(
    activation: &Activation,
    show: bool,
    event_path: PathBuf,
    shutdown: &'static AtomicBool,
) -> Result<()> {
    loop {
        match crate::linux_service::launch(activation, show)? {
            Launch::Forwarded => return Ok(()),
            Launch::BecomeInstance => {}
        }
        match crate::linux_app::run(
            event_path.clone(),
            shutdown,
            show.then(|| activation.clone()),
        ) {
            // Another launch claimed the data root first; forward to it.
            Err(_) if crate::linux_service::another_instance_answers() => continue,
            result => return result,
        }
    }
}

fn listen(event_path: &Path, device: Option<&str>, shutdown: &AtomicBool) -> Result<()> {
    shutdown.store(false, Ordering::Relaxed);
    let mut recognizer = Moonshine::load(Path::new(env!("CARGO_MANIFEST_DIR")))?;
    let input = match device {
        Some(name) => AudioInput::open_matching(name)?,
        None => AudioInput::open_system_default()?,
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

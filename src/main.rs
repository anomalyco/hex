#[cfg(target_os = "macos")]
mod accessibility;
mod app_paths;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod app_settings;
#[cfg(target_os = "macos")]
mod apple_intelligence;
#[cfg(target_os = "macos")]
mod apple_speech;
mod application_catalog;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod audio;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod command_grammar;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod commands;
#[cfg(target_os = "macos")]
mod config;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod context;
#[cfg(all(debug_assertions, target_os = "macos"))]
mod dashboard;
mod desktop_activity;
mod desktop_host;
mod desktop_i18n;
#[cfg(target_os = "windows")]
mod desktop_indicator_model;
#[cfg(target_os = "windows")]
mod desktop_indicator_shader;
#[cfg(target_os = "macos")]
mod developer_control;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod dictation;
#[cfg(target_os = "macos")]
mod dictation_audio;
#[cfg(target_os = "macos")]
mod dictation_diagnostics;
#[cfg(target_os = "macos")]
mod dictation_indicator;
#[cfg_attr(target_os = "macos", allow(dead_code))]
mod dictation_modes;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod dictation_processor;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod events;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod feedback;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod gguf_session;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod hex_microphone;
mod history;
mod instance;
#[cfg(target_os = "macos")]
mod keyboard;
#[cfg_attr(target_os = "linux", allow(dead_code))]
mod keys;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod linux_app;
#[cfg(target_os = "linux")]
mod linux_audio_control;
#[cfg(target_os = "linux")]
mod linux_context;
#[cfg(target_os = "linux")]
mod linux_dictation;
#[cfg(target_os = "linux")]
mod linux_file_chooser;
#[cfg(target_os = "linux")]
mod linux_input;
#[cfg(target_os = "linux")]
mod linux_migration;
#[cfg(target_os = "linux")]
mod linux_paste;
#[cfg(target_os = "linux")]
mod linux_portal;
#[cfg(target_os = "linux")]
mod linux_rune_hud;
#[cfg(target_os = "linux")]
mod linux_service;
#[cfg(target_os = "linux")]
mod linux_session;
#[cfg(target_os = "linux")]
mod linux_updater;
#[cfg(target_os = "macos")]
mod local_api;
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod local_transcriber;
#[cfg_attr(target_os = "linux", allow(dead_code))]
mod login_item;
#[cfg(target_os = "macos")]
mod macos_rune;
#[cfg(target_os = "macos")]
mod meeting;
#[cfg(target_os = "macos")]
mod meeting_detection;
#[cfg(target_os = "macos")]
mod meeting_mcp;
#[cfg(target_os = "macos")]
mod meeting_watcher;
#[cfg(target_os = "macos")]
mod microphone_activity;
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[cfg_attr(target_os = "linux", allow(dead_code))]
mod moonshine;
#[cfg(all(target_os = "macos", debug_assertions))]
mod moonshine_lab;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod onboarding;
mod opencode_catalog;
#[cfg(target_os = "macos")]
mod parakeet;
#[cfg(target_os = "macos")]
mod paste;
#[cfg(any(target_os = "macos", test))]
mod pause_segmenter;
#[cfg(target_os = "macos")]
mod permission_guide;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod personal_commands;
#[cfg(target_os = "macos")]
mod recognition;
#[cfg(target_os = "macos")]
mod recording_environment;
mod rune_ui;
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod self_update;
#[cfg(target_os = "macos")]
mod sparkle;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod spoken_text;
#[cfg(target_os = "macos")]
mod spring;
#[cfg(target_os = "macos")]
mod status_item;
#[cfg(target_os = "macos")]
mod suppression;
mod text_replacements;
#[cfg(target_os = "macos")]
mod transcript_cli;
#[cfg(target_os = "macos")]
mod transcription;
#[cfg(target_os = "macos")]
mod transcription_benchmark;
mod transcription_engine;
mod transcription_models;
#[cfg(any(target_os = "macos", target_os = "windows", test))]
mod transcription_preparation;
#[cfg(target_os = "macos")]
mod transcription_service;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod voice_filter;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
mod windows_app;
#[cfg(target_os = "windows")]
mod windows_audio_control;
#[cfg(target_os = "windows")]
mod windows_context;
#[cfg(target_os = "windows")]
mod windows_dictation;
#[cfg(target_os = "windows")]
mod windows_input;
#[cfg(target_os = "windows")]
mod windows_login_item;
#[cfg(target_os = "windows")]
mod windows_migration;
#[cfg(target_os = "windows")]
mod windows_paste;
#[cfg(target_os = "windows")]
mod windows_rune;
#[cfg(target_os = "windows")]
mod windows_rune_hud;
#[cfg(target_os = "windows")]
mod windows_updater;

#[cfg(target_os = "macos")]
use std::fs;
#[cfg(target_os = "macos")]
use std::io::{Read, Write};
#[cfg(target_os = "macos")]
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "macos")]
use std::sync::atomic::Ordering;

#[cfg(target_os = "macos")]
use clap::{Parser, Subcommand, ValueEnum};
use color_eyre::Result;
#[cfg(target_os = "macos")]
use color_eyre::eyre::eyre;

#[cfg(target_os = "macos")]
#[derive(Parser)]
#[command(version, about = "Local, observable voice control")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

static SHUTDOWN: AtomicBool = AtomicBool::new(false);
pub(crate) const DEVELOPER_FEATURES_ENABLED: bool = cfg!(debug_assertions);

#[cfg(target_os = "macos")]
#[derive(Subcommand)]
enum Command {
    /// Read retained dictation history without starting the app.
    History(transcript_cli::HistoryArgs),
    /// Read and search recorded meetings without starting the app.
    Meetings {
        #[command(subcommand)]
        command: transcript_cli::MeetingsCommand,
    },
    /// Run the desktop app with local dictation.
    App {
        /// Override the configured microphone preference order.
        #[arg(long)]
        device: Option<String>,
        /// Arguments for a headless render, such as `--snap out.png --scene modes`.
        #[arg(last = true)]
        rune_args: Vec<String>,
    },
    /// Listen and transcribe until interrupted.
    Listen {
        /// Override the configured microphone preference order.
        #[arg(long)]
        device: Option<String>,
    },
    /// Serve recorded meetings to agents over MCP (stdio). Read-only.
    Mcp {
        /// Read meetings from this folder instead of Application Support, such
        /// as a mirrored copy on another Mac.
        #[arg(long)]
        meetings_dir: Option<PathBuf>,
    },
    /// Manage the personal command workspace.
    Commands {
        #[command(subcommand)]
        command: CommandsCommand,
    },
    /// Run the headless local API service.
    #[command(hide = true)]
    Service {
        /// Run as a direct child whose stdin is owned by the host application.
        #[arg(long)]
        embedded: bool,
    },
    #[cfg(debug_assertions)]
    /// Show the developer recognition dashboard.
    Status,
    /// Inspect and control the running desktop app or offline audio tools.
    Dev {
        #[command(subcommand)]
        command: DevCommand,
    },
    #[cfg(debug_assertions)]
    /// Record, transcribe, and browse local meetings.
    Meeting {
        #[command(subcommand)]
        command: MeetingCommand,
    },
    /// Measure the local transcription runtime against a fixed WAV corpus.
    #[command(hide = true)]
    BenchmarkTranscription {
        /// JSON manifest containing audio paths and reference transcripts.
        manifest: PathBuf,
        /// Runtime to measure.
        #[arg(long, value_enum, default_value = "transcribe-cpp")]
        backend: TranscriptionBenchmarkBackend,
        /// Override the default GGUF model path for the transcribe.cpp backend.
        #[arg(long)]
        model: Option<PathBuf>,
        /// Full-corpus passes discarded before measurement.
        #[arg(long, default_value_t = 1)]
        warmups: usize,
        /// Full-corpus measured passes.
        #[arg(long, default_value_t = 7)]
        runs: usize,
    },
    #[cfg(debug_assertions)]
    /// Record and evaluate command-recognition fixtures interactively.
    MoonshineLab {
        /// Corpus directory containing manifest.json and audio/*.wav.
        #[arg(default_value = "perf/moonshine-corpus")]
        directory: PathBuf,
        /// Override the configured microphone preference order.
        #[arg(long)]
        device: Option<String>,
        /// Evaluate every recorded fixture across every Moonshine profile.
        #[arg(long)]
        batch: bool,
    },
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, ValueEnum)]
enum TranscriptionBenchmarkBackend {
    Onnx,
    TranscribeCpp,
}

#[cfg(target_os = "macos")]
#[derive(Subcommand)]
enum CommandsCommand {
    /// Create or refresh ~/.config/hex and install its pinned dependencies.
    Init,
}

#[cfg(target_os = "macos")]
#[derive(Subcommand)]
enum DevCommand {
    #[cfg(debug_assertions)]
    /// Inspect the running app and window state.
    Status,
    #[cfg(debug_assertions)]
    /// Drive a deterministic HUD state.
    Hud {
        #[arg(value_enum)]
        state: developer_control::DeveloperHudState,
    },
    #[cfg(debug_assertions)]
    /// Open the app and select a pane.
    Show {
        #[arg(value_enum)]
        pane: developer_control::DeveloperPane,
    },
    #[cfg(debug_assertions)]
    /// Enable or disable voice commands.
    Commands {
        #[arg(value_enum)]
        state: DevToggle,
    },
    /// Render offline audio through the Hex Microphone voice filter.
    VoiceFilter {
        #[command(subcommand)]
        command: VoiceFilterCommand,
    },
}

#[cfg(target_os = "macos")]
#[derive(Subcommand)]
enum VoiceFilterCommand {
    /// Apply a voice-filter mode across one or more dictation spans in a WAV file.
    Render {
        /// Input WAV path (16-bit PCM or 32-bit float).
        input: PathBuf,
        /// Voice filter mode id (muffled, muted, quiet, normal, chipmunk, robot, walkie, underwater, far).
        #[arg(long, default_value = "muffled")]
        mode: String,
        /// Optional JSON mode spec override (matching presets.json).
        #[arg(long)]
        params: Option<String>,
        /// Dictation span(s) in milliseconds, e.g. `1000-3500`.
        #[arg(long = "dictation")]
        dictation: Vec<String>,
        /// Effect strength (`0.0` to `1.0`).
        #[arg(long, default_value_t = 1.0)]
        strength: f64,
        /// Output WAV path.
        #[arg(short = 'o', long = "output")]
        output: PathBuf,
    },
}

#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, ValueEnum)]
enum DevToggle {
    On,
    Off,
}

#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
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
    /// Print applications currently using microphone input.
    Probe,
    /// Transcribe a meeting's recorded WAV tracks and publish its transcript.
    Transcribe { id: String },
    /// Write an Apple Intelligence title and summary for a meeting id, or
    /// time the provisional title and the final title and summary for a
    /// transcript Markdown file without saving them.
    Summarize {
        target: String,
        /// Override the excerpt character budget (file targets only).
        #[arg(long)]
        excerpt_chars: Option<usize>,
    },
    /// Replay a meeting folder's tracks and report which microphone lines
    /// echo suppression keeps.
    Echo {
        directory: PathBuf,
        /// Microphone start after the system track, when the manifest has
        /// no arrival times.
        #[arg(long)]
        microphone_offset_ms: Option<u64>,
        /// Also transcribe both tracks with the selected model.
        #[arg(long)]
        transcribe: bool,
    },
    /// Run live notes over a `transcript.ndjson` or transcript Markdown as a
    /// recording would, and report each pass's latency and prompt size.
    LiveNotes {
        path: PathBuf,
        /// Only the first minutes of the transcript.
        #[arg(long)]
        minutes: Option<u64>,
        /// Use the deterministic stand-in instead of Apple Intelligence.
        #[arg(long)]
        canned: bool,
        /// Save Apple Intelligence's raw output for each pass.
        #[arg(long, conflicts_with_all = ["canned", "recorded"])]
        record: Option<PathBuf>,
        /// Replay raw output saved by `--record` instead of running a model.
        #[arg(long, conflicts_with = "canned")]
        recorded: Option<PathBuf>,
        /// Save the outline after each pass, for `meeting capsule-lab`.
        #[arg(long)]
        snapshots: Option<PathBuf>,
    },
    /// Render the recording capsule headless over outline snapshots saved by
    /// `meeting live-notes --snapshots`. Rune arguments follow `--`
    /// (`--scene`, `--snap`, `--frames`, `--script`).
    CapsuleLab {
        snapshots: PathBuf,
        /// Meeting time per second of the scene.
        #[arg(long, default_value_t = 30.0)]
        speed: f32,
        #[arg(last = true)]
        rune_args: Vec<String>,
    },
    /// Replay a transcript as a recording meeting, with the recording capsule
    /// and the Meetings window. Needs an empty `HEX_APPLICATION_SUPPORT_DIR`.
    Replay {
        path: PathBuf,
        /// Meeting time per wall-clock second.
        #[arg(long, default_value_t = 30.0)]
        speed: f32,
        #[arg(long)]
        minutes: Option<u64>,
        #[arg(long)]
        canned: bool,
        /// Open the capsule's notes panel.
        #[arg(long)]
        notes_open: bool,
        #[arg(long, default_value = "Design sync")]
        title: String,
    },
}

#[cfg(target_os = "macos")]
fn main() -> Result<()> {
    moonshine::run_sessions_single_threaded();
    color_eyre::install()?;
    let cli = Cli::parse_from(mcp_alias_arguments(std::env::args_os().collect()));
    // Before process logging, the instance lock, or any GUI or permission
    // setup: an agent may start this over SSH while the app is running.
    if let Some(Command::Mcp { meetings_dir }) = &cli.command {
        return run_mcp(meetings_dir.clone());
    }
    match &cli.command {
        Some(Command::History(args)) => return transcript_cli::history(args),
        Some(Command::Meetings { command }) => return transcript_cli::meetings(command),
        #[cfg(debug_assertions)]
        Some(Command::Meeting {
            command: MeetingCommand::List,
        }) => {
            return transcript_cli::meetings(&transcript_cli::MeetingsCommand::List(
                Default::default(),
            ));
        }
        #[cfg(debug_assertions)]
        Some(Command::Meeting {
            command: MeetingCommand::Show { id },
        }) => {
            return transcript_cli::meetings(&transcript_cli::MeetingsCommand::Show {
                id: id.clone(),
                md: true,
                json: false,
            });
        }
        _ => {}
    }
    if let Some(Command::Dev {
        command:
            DevCommand::VoiceFilter {
                command:
                    VoiceFilterCommand::Render {
                        input,
                        mode,
                        params,
                        dictation,
                        strength,
                        output,
                    },
            },
    }) = &cli.command
    {
        return voice_filter::render_wav_command(
            input,
            mode,
            params.as_deref(),
            dictation,
            *strength,
            output,
        );
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let data_folder = app_paths::migrate_data_folder();
    let log_dir = app_paths::init_process_logging(&SHUTDOWN)?;
    if let Some(outcome) = &data_folder {
        app_paths::log_data_folder_migration(outcome);
    }

    let event_path = log_dir.join("live.ndjson");
    let executable_name = std::env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(ToOwned::to_owned));
    let bundled_watcher = is_bundled_app_executable(executable_name.as_deref());
    let bundled_service = executable_name.as_deref() == Some(std::ffi::OsStr::new("hex-service"));
    let command = cli.command.unwrap_or({
        if bundled_watcher {
            Command::App {
                device: None,
                rune_args: Vec::new(),
            }
        } else if bundled_service {
            Command::Service { embedded: false }
        } else {
            Command::Listen { device: None }
        }
    });
    match command {
        Command::App { device, rune_args } => open_app(root, event_path, device, rune_args),
        Command::Listen { device } => {
            let _instance = instance::acquire("listener")?;
            let settings = app_settings::AppSettings::load()?;
            hex_microphone::host_service();
            let events = events::EventLog::create(&event_path)?;
            let history = match history::History::open_default(settings.history_retention) {
                Ok(history) => Some(history),
                Err(error) => {
                    tracing::warn!(%error, "dictation history is unavailable");
                    None
                }
            };
            recognition::listen(
                &root,
                events,
                device.as_deref(),
                config::voice_control(),
                &SHUTDOWN,
                None,
                None,
                history,
                None,
            )
        }
        Command::Service { embedded } => {
            if !embedded {
                app_settings::AppSettings::load()?;
            }
            let service_event_path = if embedded {
                log_dir.join(format!("embedded-{}.ndjson", std::process::id()))
            } else {
                event_path
            };
            let events = events::EventLog::create(&service_event_path)?;
            let local_api = if embedded {
                let api = local_api::LocalApi::start_embedded(events)?;
                std::thread::Builder::new()
                    .name("embedded-host-lease".into())
                    .spawn(|| {
                        let mut stdin = std::io::stdin().lock();
                        let mut buffer = [0_u8; 1];
                        loop {
                            match stdin.read(&mut buffer) {
                                Ok(0) => {
                                    SHUTDOWN.store(true, Ordering::Release);
                                    std::thread::sleep(std::time::Duration::from_secs(5));
                                    tracing::warn!(
                                        "forcing embedded service shutdown after host lease closed"
                                    );
                                    std::process::exit(0);
                                }
                                Ok(_) => {}
                                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                                Err(error) => {
                                    tracing::warn!(%error, "embedded host lease failed");
                                    SHUTDOWN.store(true, Ordering::Release);
                                    break;
                                }
                            }
                        }
                    })?;
                let mut stdout = std::io::stdout().lock();
                serde_json::to_writer(&mut stdout, &api.embedded_endpoint())?;
                stdout.write_all(b"\n")?;
                stdout.flush()?;
                drop(stdout);
                api
            } else {
                local_api::LocalApi::start(events, local_api::LocalApiOptions::default())?
            };
            while !SHUTDOWN.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            drop(local_api);
            if embedded
                && let Err(error) = fs::remove_file(&service_event_path)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(%error, "could not remove embedded service event log");
            }
            Ok(())
        }
        Command::Mcp { .. } | Command::History(_) | Command::Meetings { .. } => {
            unreachable!("handled before startup")
        }
        Command::Commands {
            command: CommandsCommand::Init,
        } => {
            let workspace = personal_commands::initialize_workspace()?;
            println!("{}", workspace.display());
            Ok(())
        }
        #[cfg(debug_assertions)]
        Command::Status => dashboard::run(event_path, config::voice_control()),
        Command::Dev { command } => {
            #[cfg(debug_assertions)]
            {
                use developer_control::{DeveloperCommand, DeveloperReply};
                let command = match command {
                    DevCommand::Status => DeveloperCommand::Status,
                    DevCommand::Hud { state } => DeveloperCommand::Hud { state },
                    DevCommand::Show { pane } => DeveloperCommand::ShowPane { pane },
                    DevCommand::Commands { state } => DeveloperCommand::SetCommandsEnabled {
                        enabled: matches!(state, DevToggle::On),
                    },
                    DevCommand::VoiceFilter { .. } => unreachable!("handled before startup"),
                };
                let reply = local_api::call_developer(&command)?;
                if let DeveloperReply::Error { code, message } = &reply {
                    return Err(eyre!("{code}: {message}"));
                }
                println!("{}", serde_json::to_string_pretty(&reply)?);
                Ok(())
            }
            #[cfg(not(debug_assertions))]
            {
                match command {
                    DevCommand::VoiceFilter { .. } => unreachable!("handled before startup"),
                }
            }
        }
        #[cfg(debug_assertions)]
        Command::Meeting {
            command: MeetingCommand::Record { title },
        } => {
            app_settings::AppSettings::load()?;
            meeting::record(meeting::MeetingStart { title, app: None }, &SHUTDOWN, None).map(|_| ())
        }
        #[cfg(debug_assertions)]
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
        #[cfg(debug_assertions)]
        Command::Meeting {
            command: MeetingCommand::Show { id },
        } => {
            print!("{}", meeting::show(&id)?);
            Ok(())
        }
        #[cfg(debug_assertions)]
        Command::Meeting {
            command: MeetingCommand::Probe,
        } => meeting_watcher::probe(),
        #[cfg(debug_assertions)]
        Command::Meeting {
            command: MeetingCommand::Transcribe { id },
        } => {
            app_settings::AppSettings::load()?;
            let manifest = meeting::transcribe_meeting(&id)?;
            println!("{}", meeting::display_title(&manifest));
            if let Some(path) = manifest.export_path {
                println!("{}", path.display());
            }
            if let Some(error) = manifest.export_error {
                println!("{error}");
            }
            Ok(())
        }
        #[cfg(debug_assertions)]
        Command::Meeting {
            command:
                MeetingCommand::Summarize {
                    target,
                    excerpt_chars,
                },
        } => {
            let path = PathBuf::from(&target);
            if path.is_file() {
                print!("{}", meeting::summarize_file(&path, excerpt_chars)?);
            } else {
                app_settings::AppSettings::load()?;
                let manifest = meeting::summarize(&target)?;
                println!("{}", meeting::display_title(&manifest));
                if let Some(path) = manifest.export_path {
                    println!("{}", path.display());
                }
                if let Some(error) = manifest.notes_error {
                    println!("{error}");
                }
            }
            Ok(())
        }
        #[cfg(debug_assertions)]
        Command::Meeting {
            command:
                MeetingCommand::Echo {
                    directory,
                    microphone_offset_ms,
                    transcribe,
                },
        } => {
            if transcribe {
                app_settings::AppSettings::load()?;
            }
            print!(
                "{}",
                meeting::echo_report(&directory, microphone_offset_ms, transcribe)?
            );
            Ok(())
        }
        #[cfg(debug_assertions)]
        Command::Meeting {
            command:
                MeetingCommand::LiveNotes {
                    path,
                    minutes,
                    canned,
                    record,
                    recorded,
                    snapshots,
                },
        } => {
            use meeting::SimulatedModel;
            let entries = meeting::replay_entries(&path, minutes.map(|minutes| minutes * 60_000))?;
            let model = match (&recorded, canned) {
                (Some(recorded), _) => SimulatedModel::Recorded(recorded),
                (None, true) => SimulatedModel::Canned,
                (None, false) => SimulatedModel::AppleIntelligence {
                    record: record.as_deref(),
                },
            };
            print!(
                "{}",
                meeting::simulate_live_notes(&entries, model, snapshots.as_deref())?
            );
            Ok(())
        }
        #[cfg(debug_assertions)]
        Command::Meeting {
            command:
                MeetingCommand::Replay {
                    path,
                    speed,
                    minutes,
                    canned,
                    notes_open,
                    title,
                },
        } => {
            let entries = meeting::replay_entries(&path, minutes.map(|minutes| minutes * 60_000))?;
            keyboard::initialize_layout()?;
            rune_ui::meeting_capsules::replay(entries, speed, &title, canned, notes_open)
        }
        #[cfg(debug_assertions)]
        Command::Meeting {
            command:
                MeetingCommand::CapsuleLab {
                    snapshots,
                    speed,
                    rune_args,
                },
        } => rune_ui::meeting_capsules::lab(&snapshots, speed, rune_args),
        Command::BenchmarkTranscription {
            manifest,
            backend,
            model,
            warmups,
            runs,
        } => {
            let backend = match backend {
                TranscriptionBenchmarkBackend::Onnx => transcription_benchmark::Backend::Onnx,
                TranscriptionBenchmarkBackend::TranscribeCpp => {
                    transcription_benchmark::Backend::TranscribeCpp {
                        model: match model {
                            Some(model) => model,
                            None => parakeet::default_model_path()?,
                        },
                    }
                }
            };
            transcription_benchmark::run(&manifest, warmups, runs, backend)
        }
        #[cfg(debug_assertions)]
        Command::MoonshineLab {
            directory,
            device,
            batch,
        } => match batch {
            true => moonshine_lab::run_batch(&root, &directory),
            false => moonshine_lab::run(&root, &directory, device.as_deref()),
        },
    }
}

/// Rune reads its own flags (`--snap`, `--scene`, …) from `rune_args`; a
/// headless run draws fixtures without starting the runtime.
#[cfg(target_os = "macos")]
fn open_app(
    root: PathBuf,
    event_path: PathBuf,
    device: Option<String>,
    rune_args: Vec<String>,
) -> Result<()> {
    if !rune_args.is_empty() {
        crate::rune_ui::app(Default::default())
            .args(rune_args)
            .run();
        return Ok(());
    }
    let _instance = instance::acquire("listener")?;
    hex_microphone::host_service();
    macos_rune::run(
        &SHUTDOWN,
        meeting_watcher::ListenerConfig {
            project_root: root,
            event_path,
            device,
        },
    )
}

/// Logs to stderr only (stdout carries MCP messages) and touches no app state.
#[cfg(target_os = "macos")]
fn run_mcp(meetings_dir: Option<PathBuf>) -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("HEX_MCP_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();
    let root = match meetings_dir {
        Some(directory) => directory,
        None => meeting::root()?,
    };
    meeting_mcp::run(root)
}

/// A copy installed as `hex-meetings-mcp` only serves meetings: its
/// arguments are those of `hex mcp`, and it never falls back to listening.
#[cfg(target_os = "macos")]
fn mcp_alias_arguments(mut arguments: Vec<std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    let alias = arguments
        .first()
        .and_then(|program| std::path::Path::new(program).file_stem())
        .is_some_and(|stem| stem == "hex-meetings-mcp");
    if alias && arguments.get(1).is_none_or(|argument| argument != "mcp") {
        arguments.insert(1, "mcp".into());
    }
    arguments
}

#[cfg(target_os = "macos")]
fn is_bundled_app_executable(name: Option<&std::ffi::OsStr>) -> bool {
    name.is_some_and(|name| name == "voice-control-watch" || name == "hex")
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{Cli, Command, is_bundled_app_executable, mcp_alias_arguments};
    use clap::Parser;

    #[test]
    fn the_meetings_mcp_alias_always_serves_meetings() {
        let parse = |arguments: &[&str]| {
            Cli::parse_from(mcp_alias_arguments(
                arguments.iter().map(Into::into).collect(),
            ))
            .command
        };
        assert!(matches!(
            parse(&["/Users/kit/.local/bin/hex-meetings-mcp"]),
            Some(Command::Mcp { meetings_dir: None })
        ));
        assert!(matches!(
            parse(&["hex-meetings-mcp", "--meetings-dir", "/tmp/mirror"]),
            Some(Command::Mcp { meetings_dir: Some(directory) }) if directory.ends_with("mirror")
        ));
        assert!(matches!(
            parse(&["hex-meetings-mcp", "mcp"]),
            Some(Command::Mcp { .. })
        ));
        assert!(parse(&["hex"]).is_none());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn developer_show_accepts_every_developer_pane_spelling() {
        use super::{DevCommand, developer_control::DeveloperPane};

        for (pane, expected) in [
            ("hud-lab", DeveloperPane::HudLab),
            ("voice-action", DeveloperPane::VoiceAction),
            ("history", DeveloperPane::History),
        ] {
            let cli = Cli::try_parse_from(["hex", "dev", "show", pane]).unwrap();
            let Some(Command::Dev {
                command: DevCommand::Show { pane },
            }) = cli.command
            else {
                panic!("expected dev show command");
            };
            assert_eq!(pane, expected);
        }
        assert!(Cli::try_parse_from(["hex", "dev", "show", "hudlab"]).is_err());
        assert!(Cli::try_parse_from(["hex", "dev", "hud", "recording"]).is_ok());
    }

    #[test]
    fn both_packaged_executable_names_launch_the_app() {
        assert!(is_bundled_app_executable(Some(
            "voice-control-watch".as_ref()
        )));
        assert!(is_bundled_app_executable(Some("hex".as_ref())));
        assert!(!is_bundled_app_executable(Some("voice-control".as_ref())));
        assert!(!is_bundled_app_executable(Some("hex-service".as_ref())));
    }
}

#[cfg(target_os = "linux")]
fn main() -> Result<()> {
    linux::run(&SHUTDOWN)
}

#[cfg(target_os = "windows")]
fn main() -> Result<()> {
    windows::run(&SHUTDOWN)
}

/// The version Hex reports: the release version, or `0.0.0-dev-local-<commit>`
/// for a build outside the Release workflow (build.rs).
const VERSION: &str = env!("HEX_BUILD_VERSION");

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
mod background_process;
#[cfg(target_os = "macos")]
mod browser_media;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod calendar;
#[cfg(hex_capture)]
mod capture_host;
#[cfg(target_os = "macos")]
mod config;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod context;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod control;
#[cfg(all(debug_assertions, target_os = "macos"))]
mod dashboard;
mod desktop_activity;
mod desktop_host;
mod desktop_i18n;
#[cfg(target_os = "macos")]
mod developer_control;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod dictation;
#[cfg(target_os = "macos")]
mod dictation_audio;
#[cfg(target_os = "macos")]
mod dictation_diagnostics;
mod dictation_indicator;
#[cfg_attr(target_os = "macos", allow(dead_code))]
mod dictation_modes;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod dictation_processor;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod dot_scope;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod events;
mod experiments;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod feedback;
mod file_dialog;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod gguf_session;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod hex_microphone;
mod history;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod hud_placement;
mod input_level;
mod instance;
#[cfg(target_os = "macos")]
mod keyboard;
#[cfg(target_os = "macos")]
mod keyboard_input_sources;
mod keyboard_model_switching;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod linux_app;
#[cfg(target_os = "linux")]
mod linux_audio_control;
#[cfg(target_os = "linux")]
mod linux_clipboard;
#[cfg(target_os = "linux")]
mod linux_context;
#[cfg(target_os = "linux")]
mod linux_dictation;
#[cfg(target_os = "linux")]
mod linux_input;
#[cfg(target_os = "linux")]
mod linux_paste;
#[cfg(target_os = "linux")]
mod linux_portal;
#[cfg(target_os = "linux")]
mod linux_remote_desktop;
#[cfg(target_os = "linux")]
mod linux_rune_hud;
#[cfg(target_os = "linux")]
mod linux_service;
#[cfg(target_os = "linux")]
mod linux_session;
#[cfg(target_os = "linux")]
mod linux_tray;
mod live_text;
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
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod onboarding;
mod opencode_catalog;
#[cfg(target_os = "macos")]
mod parakeet;
#[cfg(target_os = "macos")]
mod paste;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod pause_segmenter;
#[cfg(target_os = "macos")]
mod permission_guide;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
mod personal_commands;
#[cfg(target_os = "macos")]
mod process_exit;
#[cfg(target_os = "macos")]
mod recognition;
#[cfg(target_os = "macos")]
mod recording_environment;
mod rune_hud;
mod rune_ui;
#[cfg(target_os = "macos")]
mod smart_turn;
mod speech_detector;
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
#[cfg(target_os = "macos")]
mod transcription_stream;
mod updater;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod vocabulary;
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
mod windows_paste;
#[cfg(target_os = "windows")]
mod windows_rune;

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
#[command(
    name = "hex",
    version = crate::VERSION,
    about = "Local, observable voice control",
    long_about = "Local, observable voice control.\n\n\
        Control commands (status, dictation, meetings record, settings, model, window, …) drive \
        the one running Hex over its owner-only local API and never launch it, except `start` \
        and `restart`. Every one takes --json for a versioned reply (docs/cli.md). Exit codes: \
        0 done or nothing to change, 1 refused or failed, 2 usage, 3 Hex isn't running.",
    after_help = "Run `hex <command> --help` for details, or `hex settings list` for every setting."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

// Debug builds count the heap so `/profile/vitals` can split the footprint.
#[cfg(all(debug_assertions, target_os = "macos"))]
#[global_allocator]
static ALLOC: procwatch::alloc::Counting = procwatch::alloc::Counting::system();

static SHUTDOWN: AtomicBool = AtomicBool::new(false);
pub(crate) const DEVELOPER_FEATURES_ENABLED: bool = cfg!(debug_assertions);

#[cfg(target_os = "macos")]
#[derive(Subcommand)]
enum Command {
    #[command(flatten)]
    Control(control::cli::ControlCli),
    /// Read retained dictation history without starting the app, or clear
    /// it in the running Hex (`hex history clear --yes`).
    #[command(args_conflicts_with_subcommands = true)]
    History(transcript_cli::HistoryArgs),
    /// Read and search recorded meetings without starting the app; record
    /// and stop them in the running Hex.
    Meetings {
        #[command(subcommand)]
        command: transcript_cli::MeetingsCommand,
    },
    /// Turn unfinished, unsupported features on or off for this Mac's Hex.
    Experiments {
        #[command(subcommand)]
        command: experiments::ExperimentsCommand,
    },
    /// Run the desktop app with local dictation.
    App {
        /// Capture from this microphone instead of the saved one or the system default.
        #[arg(long)]
        device: Option<String>,
        /// Arguments for a headless render, such as `--snap out.png --scene modes`.
        #[arg(last = true)]
        rune_args: Vec<String>,
    },
    /// Listen and transcribe until interrupted.
    Listen {
        /// Capture from this microphone instead of the saved one or the system default.
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
    /// Manage the ~/.config/hex personal transformations workspace.
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
    /// Inspect and control the running desktop app or offline audio tools.
    Dev {
        #[command(subcommand)]
        command: DevCommand,
    },
    /// Annotate: record your screen with drawing and narration in the
    /// running Hex, then list and send what you recorded (`hex capture`
    /// still works).
    #[command(name = "annotate", alias = "capture")]
    Capture {
        #[command(subcommand)]
        command: control::cli::CaptureCommand,
    },
    /// The Scrawl capture capsule in a window of its own, driven by a fake
    /// host, with live tuning. Records and sends nothing.
    #[cfg(hex_capture)]
    #[command(name = "capsule-playground", hide = true)]
    CapsulePlayground {
        /// Rune's flags, such as `--snap out.png --scene rule`.
        #[arg(last = true)]
        rune_args: Vec<String>,
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
    /// Create ~/.config/hex with a starter config, or refresh its typings.
    Init,
    /// Load ~/.config/hex's config as Hex would and report what it registers
    /// or why it fails.
    Check,
}

#[cfg(target_os = "macos")]
#[derive(Subcommand)]
enum DevCommand {
    #[cfg(debug_assertions)]
    /// Inspect the running app and window state.
    Status,
    #[cfg(debug_assertions)]
    /// Show the developer recognition dashboard.
    Dashboard,
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
    #[cfg(all(debug_assertions, hex_capture))]
    /// Run a capture headlessly, feeding WAV files in place of the
    /// microphone, and write its bundle (needs the `capture` experiment).
    CaptureReplay {
        /// WAV files, joined with 300 ms of silence.
        #[arg(required = true)]
        wavs: Vec<PathBuf>,
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
    /// Run live chapter passes and the boundary pass over a transcript
    /// Markdown file as a recording would, with timings, without saving.
    Chapters { path: PathBuf },
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
}

#[cfg(target_os = "macos")]
fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse_from(mcp_alias_arguments(std::env::args_os().collect()));
    let executable_name = std::env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(ToOwned::to_owned));
    let bundled_watcher = is_bundled_app_executable(executable_name.as_deref());
    // The app's GPU device and render pipeline start on a thread now, while
    // the rest of launch runs; its first window takes them, and Rune drops
    // them when the app opens none.
    if opens_app(cli.command.as_ref(), bundled_watcher) {
        rune::prewarm_gpu();
    }
    // Before process logging, the instance lock, or any GUI or permission
    // setup: an agent may start this over SSH while the app is running.
    if let Some(Command::Mcp { meetings_dir }) = &cli.command {
        return run_mcp(meetings_dir.clone());
    }
    // A window of its own: no data folder, log, lock, or settings, so it
    // runs beside the installed Hex.
    #[cfg(hex_capture)]
    if let Some(Command::CapsulePlayground { rune_args }) = &cli.command {
        keyboard::initialize_layout()?;
        rune_ui::capture_capsule::playground::run(rune_args.clone());
        return Ok(());
    }
    // Control commands talk to the running Hex; like the read commands, they
    // touch no data folder, log, or lock of their own.
    let cli = match cli.command {
        Some(Command::Control(command)) => control::cli::finish(control::cli::run(command)),
        Some(Command::History(transcript_cli::HistoryArgs {
            action: Some(transcript_cli::HistoryAction::Clear { yes, output }),
            ..
        })) => control::cli::finish(control::cli::clear_history(yes, output)),
        Some(Command::Capture { command }) => control::cli::finish(control::cli::capture(command)),
        command => Cli { command },
    };
    match &cli.command {
        Some(Command::History(args)) => return transcript_cli::history(args),
        Some(Command::Meetings { command }) => {
            use transcript_cli::MeetingsCommand;
            let control = match command {
                MeetingsCommand::Record { title, output } => Some((
                    control::ControlCommand::MeetingRecord {
                        title: title.clone(),
                    },
                    *output,
                )),
                MeetingsCommand::Stop { output } => {
                    Some((control::ControlCommand::MeetingStop, *output))
                }
                MeetingsCommand::CopyLast { output } => {
                    Some((control::ControlCommand::MeetingCopyLast, *output))
                }
                _ => None,
            };
            if let Some((command, output)) = control {
                control::cli::finish(control::cli::send(command, output));
            }
            return transcript_cli::meetings(command);
        }
        Some(Command::Experiments { command }) => return experiments::run(command),
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
                from: None,
                to: None,
                chapter: None,
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
    let log_dir = app_paths::init_process_logging(&SHUTDOWN)?;

    let event_path = log_dir.join("live.ndjson");
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
            experiments::log_launch();
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
                &SHUTDOWN,
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
        Command::Mcp { .. }
        | Command::History(_)
        | Command::Meetings { .. }
        | Command::Experiments { .. }
        | Command::Control(_) => {
            unreachable!("handled before startup")
        }
        #[cfg(any(debug_assertions, hex_capture))]
        Command::Capture { .. } => unreachable!("handled before startup"),
        #[cfg(hex_capture)]
        Command::CapsulePlayground { .. } => unreachable!("handled before startup"),
        Command::Commands {
            command: CommandsCommand::Init,
        } => {
            let workspace = personal_commands::initialize_workspace()?;
            println!("{}", workspace.display());
            Ok(())
        }
        Command::Commands {
            command: CommandsCommand::Check,
        } => {
            println!("{}", personal_commands::check_workspace()?);
            Ok(())
        }
        Command::Dev { command } => {
            #[cfg(debug_assertions)]
            {
                use developer_control::{DeveloperCommand, DeveloperReply};
                let command = match command {
                    DevCommand::Dashboard => {
                        return dashboard::run(event_path);
                    }
                    DevCommand::Status => DeveloperCommand::Status,
                    DevCommand::Hud { state } => DeveloperCommand::Hud { state },
                    DevCommand::Show { pane } => DeveloperCommand::ShowPane { pane },
                    #[cfg(all(debug_assertions, hex_capture))]
                    DevCommand::CaptureReplay { wavs } => {
                        experiments::log_launch();
                        app_settings::AppSettings::load()?;
                        let bundle = capture_host::replay(&wavs)?;
                        println!("{}", bundle.display());
                        return Ok(());
                    }
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
            meeting::record(
                meeting::MeetingStart {
                    title,
                    ..Default::default()
                },
                &SHUTDOWN,
                None,
            )
            .map(|_| ())
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
            command: MeetingCommand::Chapters { path },
        } => {
            print!("{}", meeting::chapters_file(&path)?);
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
        crate::rune_ui::render_app(rune_args).run();
        return Ok(());
    }
    let _instance = instance::acquire("listener")?;
    experiments::log_launch();
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
    name.is_some_and(|name| name == "hex")
}

/// Whether this launch runs the app: `hex app` without Rune's own (headless)
/// flags, or the bundled executable with no command. A hidden or login launch
/// counts too, though it opens no window and Rune drops the prewarmed GPU:
/// whether the window opens depends on settings, which may load only after
/// the keyboard layout (`macos_rune::run`).
#[cfg(target_os = "macos")]
fn opens_app(command: Option<&Command>, bundled_watcher: bool) -> bool {
    match command {
        Some(Command::App { rune_args, .. }) => rune_args.is_empty(),
        None => bundled_watcher,
        Some(_) => false,
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{Cli, Command, is_bundled_app_executable, mcp_alias_arguments, opens_app};
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
    fn control_commands_parse_beside_the_read_commands() {
        use super::{CommandsCommand, control::cli::ControlCli, transcript_cli};
        use clap::CommandFactory;

        Cli::command().debug_assert();
        let parse = |arguments: &[&str]| Cli::try_parse_from(arguments).map(|cli| cli.command);
        #[cfg(any(debug_assertions, hex_capture))]
        {
            assert!(matches!(
                parse(&["hex", "capture", "stop", "--json"]),
                Ok(Some(Command::Capture { command: crate::control::cli::CaptureCommand::Stop { output } })) if output.json
            ));
            assert!(matches!(
                parse(&["hex", "capture", "send", "latest", "--to", "ses_1"]),
                Ok(Some(Command::Capture {
                    command: crate::control::cli::CaptureCommand::Send { capture, to: Some(to), .. }
                })) if capture == "latest" && to == "ses_1"
            ));
            assert!(matches!(
                parse(&["hex", "capture", "send", "latest", "--always"]),
                Ok(Some(Command::Capture {
                    command: crate::control::cli::CaptureCommand::Send {
                        to: None,
                        always: true,
                        ..
                    }
                }))
            ));
            assert!(parse(&["hex", "capture", "route", "latest", "--explain"]).is_ok());
        }
        assert!(matches!(
            parse(&["hex", "status", "--json"]),
            Ok(Some(Command::Control(ControlCli::Status { output }))) if output.json
        ));
        assert!(matches!(
            parse(&["hex", "dictation", "toggle"]),
            Ok(Some(Command::Control(ControlCli::Dictation { .. })))
        ));
        assert!(matches!(
            parse(&[
                "hex",
                "settings",
                "set",
                "release_microphone_while_idle",
                "on"
            ]),
            Ok(Some(Command::Control(ControlCli::Settings { .. })))
        ));
        assert!(matches!(
            parse(&["hex", "window", "show", "history"]),
            Ok(Some(Command::Control(ControlCli::Window { .. })))
        ));
        assert!(matches!(
            parse(&["hex", "history", "clear", "--yes"]),
            Ok(Some(Command::History(transcript_cli::HistoryArgs {
                action: Some(transcript_cli::HistoryAction::Clear { yes: true, .. }),
                ..
            })))
        ));
        assert!(matches!(
            parse(&["hex", "history", "--json"]),
            Ok(Some(Command::History(transcript_cli::HistoryArgs {
                action: None,
                ..
            })))
        ));
        assert!(parse(&["hex", "history", "--limit", "5", "clear", "--yes"]).is_err());
        assert!(matches!(
            parse(&["hex", "meetings", "record", "--title", "Design sync"]),
            Ok(Some(Command::Meetings {
                command: transcript_cli::MeetingsCommand::Record { title: Some(_), .. }
            }))
        ));
        assert!(matches!(
            parse(&["hex", "commands", "check"]),
            Ok(Some(Command::Commands {
                command: CommandsCommand::Check
            }))
        ));
        assert!(parse(&["hex", "meetings", "cancel"]).is_err());
        assert!(matches!(
            parse(&[
                "hex",
                "vocabulary",
                "add",
                "abattoir",
                "--sounds-like",
                "avatar,a bat war"
            ]),
            Ok(Some(Command::Control(ControlCli::Vocabulary {
                command: crate::control::cli::VocabularyCommand::Add {
                    sounds_like: Some(_),
                    ..
                }
            })))
        ));
        assert!(matches!(
            parse(&["hex", "vocabulary", "list", "--json"]),
            Ok(Some(Command::Control(ControlCli::Vocabulary {
                command: crate::control::cli::VocabularyCommand::List { output }
            }))) if output.json
        ));
        assert!(parse(&["hex", "vocabulary", "remove"]).is_err());
    }

    #[test]
    fn experiments_parse_as_a_read_command() {
        use super::experiments::ExperimentsCommand;

        assert!(matches!(
            Cli::try_parse_from(["hex", "experiments", "list", "--json"]).map(|cli| cli.command),
            Ok(Some(Command::Experiments {
                command: ExperimentsCommand::List { json: true }
            }))
        ));
        assert!(matches!(
            Cli::try_parse_from(["hex", "experiments", "enable", "smart-turn"])
                .map(|cli| cli.command),
            Ok(Some(Command::Experiments {
                command: ExperimentsCommand::Enable { name }
            })) if name == "smart-turn"
        ));
        assert!(Cli::try_parse_from(["hex", "experiments", "disable"]).is_err());
    }

    #[test]
    fn the_packaged_executable_launches_the_app() {
        assert!(is_bundled_app_executable(Some("hex".as_ref())));
        assert!(!is_bundled_app_executable(Some("hex-service".as_ref())));
    }

    #[test]
    fn only_app_launches_prewarm_the_gpu() {
        let opens = |arguments: &[&str]| {
            let arguments = mcp_alias_arguments(arguments.iter().map(Into::into).collect());
            let bundled =
                is_bundled_app_executable(std::path::Path::new(&arguments[0]).file_stem());
            opens_app(Cli::parse_from(arguments).command.as_ref(), bundled)
        };
        assert!(opens(&["hex"]));
        assert!(opens(&["/Applications/Hex.app/Contents/MacOS/hex"]));
        assert!(opens(&["hex", "app"]));
        assert!(opens(&["hex", "app", "--device", "MacBook Pro Microphone"]));
        assert!(!opens(&["hex", "app", "--", "--snap", "/tmp/a.png"]));
        assert!(!opens(&["hex-service"]));
        assert!(!opens(&["hex-meetings-mcp"]));
        for command in [
            &["status"][..],
            &["quit"],
            &["mcp"],
            &["history"],
            &["listen"],
            &["meetings", "list"],
            &["settings", "list"],
            &["capture", "status"],
            &["dev", "dashboard"],
        ] {
            let arguments = [&["hex"][..], command].concat();
            assert!(!opens(&arguments), "{command:?}");
        }
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

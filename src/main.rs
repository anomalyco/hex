mod app_paths;
#[cfg(target_os = "macos")]
mod app_settings;
#[cfg(target_os = "macos")]
mod app_window;
#[cfg(target_os = "macos")]
mod apple_speech;
#[cfg(target_os = "macos")]
mod application_catalog;
#[cfg_attr(target_os = "linux", allow(dead_code))]
mod audio;
#[cfg(target_os = "macos")]
mod command_grammar;
#[cfg(target_os = "macos")]
mod commands;
#[cfg(target_os = "macos")]
mod config;
#[cfg(target_os = "macos")]
mod context;
#[cfg(all(debug_assertions, target_os = "macos"))]
mod dashboard;
#[cfg_attr(target_os = "linux", allow(dead_code))]
mod dictation;
#[cfg(target_os = "macos")]
mod dictation_indicator;
#[cfg(target_os = "macos")]
pub mod dictation_processor;
#[cfg_attr(target_os = "linux", allow(dead_code))]
mod events;
#[cfg(target_os = "macos")]
mod feedback;
mod instance;
#[cfg(target_os = "macos")]
mod keyboard;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod linux_app;
#[cfg(target_os = "linux")]
mod linux_dictation;
#[cfg(target_os = "linux")]
mod linux_input;
#[cfg(target_os = "linux")]
mod linux_paste;
#[cfg(target_os = "linux")]
mod linux_settings;
#[cfg(target_os = "linux")]
mod linux_transcriber;
#[cfg(target_os = "linux")]
mod linux_updater;
#[cfg(target_os = "macos")]
mod login_item;
#[cfg(target_os = "macos")]
mod meeting;
#[cfg(target_os = "macos")]
mod meeting_detection;
#[cfg(target_os = "macos")]
mod meeting_watcher;
#[cfg(target_os = "macos")]
mod microphone_activity;
#[cfg_attr(target_os = "linux", allow(dead_code))]
mod moonshine;
#[cfg(target_os = "macos")]
mod onboarding;
#[cfg(target_os = "macos")]
mod parakeet;
#[cfg(target_os = "macos")]
mod paste;
#[cfg(target_os = "macos")]
mod recognition;
#[cfg(target_os = "macos")]
mod recording_environment;
#[cfg(target_os = "macos")]
mod selected_text;
#[cfg(target_os = "macos")]
mod sparkle;
#[cfg(target_os = "macos")]
mod suppression;
#[cfg(target_os = "macos")]
mod text_input;
#[cfg(target_os = "macos")]
mod text_replacements;
#[cfg(target_os = "macos")]
mod transcription;
#[cfg(target_os = "macos")]
mod transcription_benchmark;
#[cfg_attr(target_os = "linux", allow(dead_code))]
mod transcription_models;

#[cfg(target_os = "macos")]
use std::fs::{self, OpenOptions};
#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "macos")]
use std::sync::atomic::Ordering;

#[cfg(target_os = "macos")]
use clap::{Parser, Subcommand, ValueEnum};
use color_eyre::Result;
#[cfg(target_os = "macos")]
use color_eyre::eyre::eyre;
#[cfg(target_os = "macos")]
use tracing_subscriber::fmt::writer::MakeWriterExt;

#[cfg(target_os = "macos")]
#[derive(Parser)]
#[command(version, about = "Local, observable voice control")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

static SHUTDOWN: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
pub(crate) const DEVELOPER_FEATURES_ENABLED: bool = cfg!(debug_assertions);

#[cfg(target_os = "macos")]
#[derive(Subcommand)]
enum Command {
    /// Run the GPUI desktop app with local dictation.
    App {
        /// Override the configured microphone preference order.
        #[arg(long)]
        device: Option<String>,
        /// Preview the dictation HUD without starting recognition.
        #[arg(long)]
        preview_dictation: bool,
    },
    /// Launch an isolated, deterministic UI preview without app services.
    Preview {
        /// Production UI surface to open.
        #[arg(value_enum)]
        target: AppPreviewTarget,
        /// Language selected in the transcription picker.
        #[arg(long, default_value = "en")]
        language: String,
        /// Deterministic model installation state shown by the picker.
        #[arg(long, value_enum, default_value = "actual")]
        model_state: AppPreviewModelState,
    },
    /// Listen and transcribe until interrupted.
    Listen {
        /// Override the configured microphone preference order.
        #[arg(long)]
        device: Option<String>,
    },
    #[cfg(debug_assertions)]
    /// Show the developer recognition dashboard.
    Status,
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
#[derive(Clone, Copy, ValueEnum)]
enum AppPreviewTarget {
    DictationHud,
    HudLab,
    Onboarding,
    Settings,
    Modes,
    Replacements,
    Commands,
    Meetings,
    Activity,
    TranscriptionPicker,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, ValueEnum)]
enum AppPreviewModelState {
    Actual,
    Installed,
    Missing,
    Downloading,
    Error,
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
    /// Watch supported meeting applications and offer local recording.
    Watch {
        /// Show a non-recording UI preview immediately.
        #[arg(long)]
        preview: bool,
    },
    /// Print applications currently using microphone input.
    Probe,
}

#[cfg(target_os = "macos")]
fn main() -> Result<()> {
    color_eyre::install()?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let log_dir = app_paths::logs_dir()?;
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

    let event_path = log_dir.join("live.ndjson");
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
        Command::Preview {
            target,
            language,
            model_state,
        } => {
            if matches!(target, AppPreviewTarget::DictationHud) {
                return meeting_watcher::run(&SHUTDOWN, false, None, true);
            }
            if !transcription_models::LANGUAGES
                .iter()
                .any(|(code, _)| *code == language)
            {
                return Err(eyre!("unsupported preview language: {language}"));
            }
            let pane = match target {
                AppPreviewTarget::HudLab => app_window::PreviewPane::HudLab,
                AppPreviewTarget::Onboarding
                | AppPreviewTarget::Settings
                | AppPreviewTarget::TranscriptionPicker => app_window::PreviewPane::Settings,
                AppPreviewTarget::Modes => app_window::PreviewPane::Modes,
                AppPreviewTarget::Replacements => app_window::PreviewPane::Replacements,
                AppPreviewTarget::Commands => app_window::PreviewPane::Commands,
                AppPreviewTarget::Meetings => app_window::PreviewPane::Meetings,
                AppPreviewTarget::Activity => app_window::PreviewPane::Activity,
                AppPreviewTarget::DictationHud => unreachable!(),
            };
            let model_state = match model_state {
                AppPreviewModelState::Actual => app_window::PreviewModelState::Actual,
                AppPreviewModelState::Installed => app_window::PreviewModelState::Installed,
                AppPreviewModelState::Missing => app_window::PreviewModelState::Missing,
                AppPreviewModelState::Downloading => app_window::PreviewModelState::Downloading,
                AppPreviewModelState::Error => app_window::PreviewModelState::Error,
            };
            meeting_watcher::preview_shell(
                &SHUTDOWN,
                app_window::AppWindowPreview {
                    pane,
                    transcription_picker: matches!(target, AppPreviewTarget::TranscriptionPicker)
                        .then_some((language, model_state)),
                    onboarding: matches!(target, AppPreviewTarget::Onboarding),
                },
            )
        }
        Command::Listen { device } => {
            let _instance = instance::acquire("listener")?;
            app_settings::AppSettings::load()?;
            recognition::listen(
                &root,
                &event_path,
                device.as_deref(),
                config::voice_control(),
                &SHUTDOWN,
                None,
                None,
            )
        }
        #[cfg(debug_assertions)]
        Command::Status => dashboard::run(event_path, config::voice_control()),
        #[cfg(debug_assertions)]
        Command::Meeting {
            command: MeetingCommand::Record { title },
        } => {
            app_settings::AppSettings::load()?;
            meeting::record(title, &SHUTDOWN, &root, None).map(|_| ())
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
            command: MeetingCommand::Watch { preview },
        } => meeting_watcher::run(&SHUTDOWN, preview, None, false),
        #[cfg(debug_assertions)]
        Command::Meeting {
            command: MeetingCommand::Probe,
        } => meeting_watcher::probe(),
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

#[cfg(target_os = "linux")]
fn main() -> Result<()> {
    linux::run(&SHUTDOWN)
}

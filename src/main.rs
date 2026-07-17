mod app_paths;
mod app_settings;
mod app_window;
mod application_catalog;
mod audio;
mod command_grammar;
mod commands;
mod config;
mod context;
#[cfg(debug_assertions)]
mod dashboard;
mod dictation;
mod dictation_indicator;
pub mod dictation_processor;
mod events;
mod feedback;
mod instance;
mod keyboard;
mod login_item;
mod meeting;
mod meeting_detection;
mod meeting_watcher;
mod microphone_activity;
mod moonshine;
mod onboarding;
mod parakeet;
mod paste;
mod recognition;
mod recording_environment;
mod selected_text;
mod sparkle;
mod suppression;
mod text_input;
mod text_replacements;
mod transcription_benchmark;
mod transcription_models;

use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::{Parser, Subcommand, ValueEnum};
use color_eyre::Result;
use color_eyre::eyre::eyre;
use tracing_subscriber::fmt::writer::MakeWriterExt;

#[derive(Parser)]
#[command(version, about = "Local, observable voice control")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

static SHUTDOWN: AtomicBool = AtomicBool::new(false);
pub(crate) const DEVELOPER_FEATURES_ENABLED: bool = cfg!(debug_assertions);

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

#[derive(Clone, Copy, ValueEnum)]
enum TranscriptionBenchmarkBackend {
    Onnx,
    TranscribeCpp,
}

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

#[derive(Clone, Copy, ValueEnum)]
enum AppPreviewModelState {
    Actual,
    Installed,
    Missing,
    Downloading,
    Error,
}

#[cfg(debug_assertions)]
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

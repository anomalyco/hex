mod common;
mod desktop;
mod platform;
mod speech;

// The historical flat module names, re-exported so existing `crate::` paths
// keep resolving while call sites migrate to the folder tree.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub(crate) use common::command_context;
#[cfg(target_os = "macos")]
pub(crate) use common::command_grammar;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub(crate) use common::command_grammar as command_grammar_common;
#[cfg(target_os = "macos")]
pub(crate) use common::commands_engine as commands;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub(crate) use common::commands_engine;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) use common::dictation_processing;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub(crate) use common::keys;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) use common::opencode;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) use common::self_update;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub(crate) use common::spoken_text;
pub(crate) use common::{app_paths, audio, dictation, events, history, instance};
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) use common::{feedback, text_replacements};
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) use desktop::activity_pane as desktop_activity_pane;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) use desktop::hud_lab as desktop_hud_lab;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) use desktop::i18n as desktop_i18n;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) use desktop::model_catalog as desktop_model_catalog;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) use desktop::onboarding as desktop_onboarding;
pub(crate) use desktop::{
    activity as desktop_activity, host as desktop_host, shell as desktop_shell,
    transcription_picker as desktop_transcription_picker, ui as desktop_ui,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) use desktop::{
    history_pane as desktop_history_pane, replacement_editor as desktop_replacement_editor,
    text_input,
};
#[cfg(target_os = "linux")]
pub(crate) use platform::linux::{
    app as linux_app, command_executor as linux_command_executor, dictation as linux_dictation,
    input as linux_input, paste as linux_paste, run as linux, settings as linux_settings,
    updater as linux_updater,
};
#[cfg(all(target_os = "macos", debug_assertions))]
pub(crate) use platform::macos::dashboard;
#[cfg(target_os = "macos")]
pub(crate) use platform::macos::{
    accessibility, app_settings, app_window, application_catalog, config, context,
    developer_control, dictation_audio, dictation_diagnostics, dictation_indicator,
    dictation_processor, keyboard, local_api, login_item, meeting, meeting_detection,
    meeting_watcher, microphone_activity, onboarding, paste, permission_guide, personal_commands,
    recording_environment, sparkle, status_item, suppression, swift_settings_import,
};
// The Windows shell's historical i18n path; the table itself is shared.
#[cfg(target_os = "windows")]
pub(crate) use desktop::i18n as windows_i18n;
#[cfg(target_os = "windows")]
pub(crate) use platform::windows::{
    app as windows_app, audio_control as windows_audio_control, context as windows_context,
    dictation as windows_dictation, indicator as windows_indicator, input as windows_input,
    login_item as windows_login_item, paste as windows_paste, run as windows,
    settings as windows_settings, ui as windows_ui, updater as windows_updater,
    voice_action as windows_voice_action,
};
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) use speech::local_transcriber;
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) use speech::moonshine;
#[cfg(all(target_os = "macos", debug_assertions))]
pub(crate) use speech::moonshine_lab;
pub(crate) use speech::transcription_models;
#[cfg(target_os = "macos")]
pub(crate) use speech::{
    apple_speech, parakeet, recognition, transcription, transcription_benchmark,
    transcription_service,
};

#[cfg(target_os = "macos")]
use std::fs::{self, OpenOptions};
#[cfg(target_os = "macos")]
use std::io::{Read, Write};
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
        /// Collapse OpenCode settings in the representative Modes preview.
        #[arg(long)]
        collapse_mode_processing: bool,
        /// Open the transformation picker in the representative Modes preview.
        #[arg(long)]
        open_transformation_picker: bool,
        /// Select the Global row in the representative Modes preview.
        #[arg(long)]
        select_global_mode: bool,
        /// Preview OpenCode-dependent controls without an available installation.
        #[arg(long)]
        opencode_unavailable: bool,
        /// Show missing post-onboarding permissions in Settings.
        #[arg(long)]
        permissions_missing: bool,
    },
    /// Listen and transcribe until interrupted.
    Listen {
        /// Override the configured microphone preference order.
        #[arg(long)]
        device: Option<String>,
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
    #[cfg(debug_assertions)]
    /// Inspect and control the running desktop app.
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

#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
#[derive(Subcommand)]
enum DevCommand {
    /// Inspect the running app and window state.
    Status,
    /// Drive a deterministic HUD state.
    Hud {
        #[arg(value_enum)]
        state: DevHudState,
    },
    /// Open the app and select a pane.
    Show {
        #[arg(value_enum)]
        pane: DevPane,
    },
    /// Enable or disable voice commands.
    Commands {
        #[arg(value_enum)]
        state: DevToggle,
    },
}

#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, ValueEnum)]
enum DevHudState {
    Reset,
    Recording,
    Transcribing,
    Processing,
}

#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, ValueEnum)]
enum DevPane {
    Settings,
    Modes,
    VoiceAction,
    Replacements,
    HudLab,
    Commands,
    Meetings,
    Activity,
}

#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, ValueEnum)]
enum DevToggle {
    On,
    Off,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, ValueEnum)]
enum AppPreviewTarget {
    DictationHud,
    HudLab,
    Onboarding,
    Settings,
    Modes,
    VoiceAction,
    Replacements,
    Commands,
    Meetings,
    Activity,
    History,
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
    let executable_name = std::env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(ToOwned::to_owned));
    let bundled_watcher = is_bundled_app_executable(executable_name.as_deref());
    let bundled_service = executable_name.as_deref() == Some(std::ffi::OsStr::new("hex-service"));
    let command = cli.command.unwrap_or({
        if bundled_watcher {
            Command::App {
                device: None,
                preview_dictation: false,
            }
        } else if bundled_service {
            Command::Service { embedded: false }
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
            collapse_mode_processing,
            open_transformation_picker,
            select_global_mode,
            opencode_unavailable,
            permissions_missing,
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
                AppPreviewTarget::VoiceAction => app_window::PreviewPane::VoiceAction,
                AppPreviewTarget::Replacements => app_window::PreviewPane::Replacements,
                AppPreviewTarget::Commands => app_window::PreviewPane::Commands,
                AppPreviewTarget::Meetings => app_window::PreviewPane::Meetings,
                AppPreviewTarget::Activity => app_window::PreviewPane::Activity,
                AppPreviewTarget::History => app_window::PreviewPane::History,
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
                    collapse_mode_processing,
                    open_transformation_picker,
                    select_global_mode,
                    opencode_unavailable,
                    permissions_missing,
                },
            )
        }
        Command::Listen { device } => {
            let _instance = instance::acquire("listener")?;
            let settings = app_settings::AppSettings::load()?;
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
                local_api::LocalApi::start(events)?
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
        Command::Commands {
            command: CommandsCommand::Init,
        } => {
            let workspace = personal_commands::initialize_workspace()?;
            println!("{}", workspace.display());
            Ok(())
        }
        #[cfg(debug_assertions)]
        Command::Status => dashboard::run(event_path, config::voice_control()),
        #[cfg(debug_assertions)]
        Command::Dev { command } => {
            use developer_control::{
                DeveloperCommand, DeveloperHudState as RpcHudState, DeveloperPane as RpcPane,
                DeveloperReply,
            };
            let command = match command {
                DevCommand::Status => DeveloperCommand::Status,
                DevCommand::Hud { state } => DeveloperCommand::Hud {
                    state: match state {
                        DevHudState::Reset => RpcHudState::Reset,
                        DevHudState::Recording => RpcHudState::Recording,
                        DevHudState::Transcribing => RpcHudState::Transcribing,
                        DevHudState::Processing => RpcHudState::Processing,
                    },
                },
                DevCommand::Show { pane } => DeveloperCommand::ShowPane {
                    pane: match pane {
                        DevPane::Settings => RpcPane::Settings,
                        DevPane::Modes => RpcPane::Modes,
                        DevPane::VoiceAction => RpcPane::VoiceAction,
                        DevPane::Replacements => RpcPane::Replacements,
                        DevPane::HudLab => RpcPane::HudLab,
                        DevPane::Commands => RpcPane::Commands,
                        DevPane::Meetings => RpcPane::Meetings,
                        DevPane::Activity => RpcPane::Activity,
                    },
                },
                DevCommand::Commands { state } => DeveloperCommand::SetCommandsEnabled {
                    enabled: matches!(state, DevToggle::On),
                },
            };
            let reply = local_api::call_developer(&command)?;
            if let DeveloperReply::Error { code, message } = &reply {
                return Err(eyre!("{code}: {message}"));
            }
            println!("{}", serde_json::to_string_pretty(&reply)?);
            Ok(())
        }
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

#[cfg(target_os = "macos")]
fn is_bundled_app_executable(name: Option<&std::ffi::OsStr>) -> bool {
    name.is_some_and(|name| name == "voice-control-watch" || name == "hex")
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::is_bundled_app_executable;

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

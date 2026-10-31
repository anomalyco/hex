//! The `hex` control client: parses a command, sends it to the running Hex,
//! and prints the answer for a person or, with `--json`, the versioned reply.
//!
//! Exit codes: 0 when the command ran (or there was nothing to change), 1
//! when Hex refused or failed it, 2 for usage errors (clap), 3 when Hex
//! isn't running, and 4 when `dictation stop --print` heard no speech. Nothing here launches Hex except `start` and `restart`.

use std::io::Write;

use clap::{Args, Subcommand};
use serde_json::Value;

use super::{
    CONTROL_VERSION, CaptureAction, ControlCommand, ControlReply, ControlRequest, ControlResponse,
    ModelInfo, Pane, StatusReport, settings::SettingValue,
};
use crate::dictation::vocabulary::VocabularyEntry;

pub const EXIT_REFUSED: u8 = 1;
pub const EXIT_NOT_RUNNING: u8 = 3;
pub const EXIT_NO_SPEECH: u8 = 4;

#[derive(Args, Clone, Copy, Debug, Default)]
pub struct Output {
    /// Print the versioned JSON reply (see docs/cli.md).
    #[arg(long)]
    pub json: bool,
}

/// The control commands at the top of `hex`.
#[derive(Subcommand, Debug)]
pub enum ControlCli {
    /// Show what the running Hex is doing.
    ///
    /// Recording, meeting, model, features, permissions, window, version,
    /// pid, and memory footprint.
    Status {
        #[command(flatten)]
        output: Output,
    },
    /// Open Hex if it isn't running, and wait until it answers.
    Start {
        #[command(flatten)]
        output: Output,
    },
    /// Quit Hex.
    ///
    /// Refused while a meeting or dictation is in progress unless --force.
    Quit {
        /// Quit even while recording; the recording is lost.
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        output: Output,
    },
    /// Quit and reopen Hex (or open it if it isn't running).
    Restart {
        /// Restart even while recording; the recording is lost.
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        output: Output,
    },
    /// Start, stop, cancel, or toggle a dictation, as the shortcut would.
    ///
    /// `start` locks the recording: `stop` or the shortcut finishes it,
    /// `cancel` or Escape discards it. Repeating a command changes nothing.
    Dictation {
        #[arg(value_enum)]
        action: CaptureAction,
        /// With `stop`: wait for the transcript and print it instead of
        /// pasting it. Exits 4, printing nothing, when no speech was heard.
        #[arg(long)]
        print: bool,
        /// With `start` or `toggle`: paste the transcript as heard,
        /// skipping the Output pane's steps, like Dictate without
        /// processing.
        #[arg(long)]
        raw: bool,
        #[command(flatten)]
        output: Output,
    },
    /// Paste the last dictation at the current focus (Option-Shift-V).
    PasteLast {
        #[command(flatten)]
        output: Output,
    },
    /// Read and change any persisted setting, shortcuts included.
    Settings {
        #[command(subcommand)]
        command: SettingsCommand,
    },
    /// List, switch, and install transcription models.
    Model {
        #[command(subcommand)]
        command: ModelCommand,
    },
    /// Open the Hex window at a pane, or close it.
    Window {
        #[command(subcommand)]
        command: WindowCommand,
    },
    /// Replay the first-run onboarding (needs Labs on).
    Onboarding {
        #[command(subcommand)]
        command: OnboardingCommand,
    },
    /// Check for a Hex update now; Hex shows what it finds.
    CheckForUpdates {
        #[command(flatten)]
        output: Output,
    },
    /// Show Launch at Login, or turn it on or off.
    LaunchAtLogin {
        #[arg(value_enum)]
        state: Option<Toggle>,
        #[command(flatten)]
        output: Output,
    },
    /// List, add, and remove vocabulary terms.
    Vocabulary {
        #[command(subcommand)]
        command: VocabularyCommand,
    },
    /// Read retained dictation history without starting the app, or clear
    /// it in the running Hex (`hex history clear --yes`).
    #[command(args_conflicts_with_subcommands = true)]
    History(crate::control::transcript_cli::HistoryArgs),
    /// Read and search recorded meetings without starting the app; record
    /// and stop them in the running Hex.
    Meetings {
        #[command(subcommand)]
        command: crate::control::transcript_cli::MeetingsCommand,
    },
    /// Serve recorded meetings to agents over MCP (stdio). Read-only.
    Mcp {
        /// Read meetings from this folder instead of Application Support, such
        /// as a mirrored copy on another computer.
        #[arg(long)]
        meetings_dir: Option<std::path::PathBuf>,
    },
    /// Manage the ~/.config/hex personal transformations workspace.
    Commands {
        #[command(subcommand)]
        command: CommandsCommand,
    },
    /// List the Labs features, or turn one on or off.
    Labs {
        #[command(subcommand)]
        command: Option<crate::labs::LabsCommand>,
    },
    /// Replaced by `hex labs`.
    #[command(hide = true)]
    Experiments {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Annotate: record your screen with drawing and narration in the
    /// running Hex, then list and send what you recorded (`hex capture`
    /// still works).
    #[command(name = "annotate", alias = "capture")]
    Annotate {
        #[command(subcommand)]
        command: CaptureCommand,
    },
    /// Print Hex's recent log lines; works without Hex running.
    Logs {
        /// Read process.log (Rust and native diagnostics) instead of the
        /// live.ndjson event log.
        #[arg(long)]
        process: bool,
        /// How many lines from the end.
        #[arg(short = 'n', long, default_value_t = 50)]
        lines: usize,
        /// Keep printing new lines until interrupted.
        #[arg(short, long)]
        follow: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum CommandsCommand {
    /// Create ~/.config/hex with a starter config, or refresh its typings.
    Init,
    /// Load ~/.config/hex's config as Hex would and report what it registers
    /// or why it fails.
    Check,
}

#[derive(Subcommand, Debug)]
pub enum SettingsCommand {
    /// Every setting with its value, kind, and whether it can be set.
    List {
        #[command(flatten)]
        output: Output,
    },
    /// One setting's value, or every setting under a prefix (`meetings`).
    Get {
        key: String,
        #[command(flatten)]
        output: Output,
    },
    /// Change a setting as the window would.
    ///
    /// Booleans take on/off, shortcuts take text such as `option+shift+v`,
    /// `right-command`, `fn`, or `f5`, and optional settings take `none`.
    Set {
        key: String,
        value: String,
        #[command(flatten)]
        output: Output,
    },
    /// Set several settings at once from a JSON object of `key: value`,
    /// all or none; other settings keep their values.
    ///
    /// Values are JSON (`true`, `0.4`, `"option+space"`, `null` for none);
    /// `transcription.model` and `transcription.language` select a model
    /// without downloading it. With Hex stopped, writes settings.json
    /// directly.
    Apply {
        /// A JSON file, or `-` for standard input.
        file: std::path::PathBuf,
        #[command(flatten)]
        output: Output,
    },
}

#[derive(Subcommand, Debug)]
pub enum ModelCommand {
    /// Every model with its languages, size, and whether it's installed.
    List {
        #[command(flatten)]
        output: Output,
    },
    /// Switch to an installed model. Never downloads.
    Use {
        /// A model id from `hex model list`.
        model: String,
        /// A language the model supports (default: its best match for the
        /// current language).
        #[arg(long)]
        language: Option<String>,
        #[command(flatten)]
        output: Output,
    },
    /// Download a model, then switch to it. Without a model, the selected
    /// one. On Linux and Windows it downloads here when Hex isn't running.
    Install {
        model: Option<String>,
        #[arg(long)]
        language: Option<String>,
        #[command(flatten)]
        output: Output,
    },
    /// Stop a download or load in progress.
    Cancel {
        #[command(flatten)]
        output: Output,
    },
    /// Whether a model (the selected one unless given) is installed and
    /// checksum-verified. Runs here; Linux and Windows.
    Status {
        model: Option<String>,
        #[arg(long)]
        language: Option<String>,
    },
    /// The transcribe.cpp compute devices Hex can use. Linux and Windows.
    Devices,
    /// Load and prewarm an installed model here. Linux and Windows.
    Check {
        model: Option<String>,
        #[arg(long)]
        language: Option<String>,
    },
    /// Transcribe a WAV file with an installed model here. Linux and Windows.
    Transcribe {
        wav: std::path::PathBuf,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        language: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum OnboardingCommand {
    /// Open the first-run onboarding in the window now, as on a new install.
    ///
    /// Forgets that onboarding finished. Settings, history, models, and permissions stay,
    /// so their steps pass at once.
    #[command(visible_alias = "restart")]
    Reset {
        #[command(flatten)]
        output: Output,
    },
}

#[derive(Subcommand, Debug)]
pub enum WindowCommand {
    /// Open the window, at a pane if given.
    Show {
        #[arg(value_enum)]
        pane: Option<Pane>,
        #[command(flatten)]
        output: Output,
    },
    /// Close the window. Hex keeps running in the menu bar.
    Hide {
        #[command(flatten)]
        output: Output,
    },
    /// Labs: open the window on a playground story by its path, name, or
    /// slug (`Prototypes/HUD/Processing`, `prototypes-hud-processing`).
    Story {
        story: String,
        /// The variant, by number from 1 or by name.
        #[arg(long)]
        variant: Option<String>,
        /// Run the story's `r` trigger once it's open, as pressing R does.
        #[arg(long)]
        play: bool,
        #[command(flatten)]
        output: Output,
    },
}

#[derive(Subcommand, Debug)]
pub enum VocabularyCommand {
    /// Every term with its "sounds like" spellings.
    List {
        #[command(flatten)]
        output: Output,
    },
    /// Add a term, or more spellings to a term already in the list.
    Add {
        term: String,
        /// Spellings a transcript may show instead, comma-separated
        /// (`--sounds-like "capuchin,cap a chino"`).
        #[arg(long, value_name = "SPELLINGS")]
        sounds_like: Option<String>,
        #[command(flatten)]
        output: Output,
    },
    /// Remove a term and its spellings.
    Remove {
        term: String,
        #[command(flatten)]
        output: Output,
    },
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Toggle {
    On,
    Off,
}

/// Exits with `code` once stdout is flushed.
pub fn finish(code: u8) -> ! {
    let _ = std::io::stdout().flush();
    std::process::exit(code.into())
}

pub fn run(command: ControlCli) -> u8 {
    match command {
        ControlCli::Status { output } => send(ControlCommand::Status, output),
        ControlCli::Start { output } => start(output),
        ControlCli::Quit { force, output } => send(ControlCommand::Quit { force }, output),
        ControlCli::Restart { force, output } => {
            if !running() {
                return start(output);
            }
            #[cfg(target_os = "linux")]
            if let Err(error) = crate::linux::service::prepare_restart() {
                return print(ControlReply::error("failed", format!("{error:#}")), output);
            }
            #[cfg(target_os = "macos")]
            return send(ControlCommand::Restart { force }, output);
            #[cfg(not(target_os = "macos"))]
            match call(ControlRequest::new(ControlCommand::Restart { force })) {
                Ok(ControlResponse {
                    reply: ControlReply::Done { .. },
                    ..
                }) => print(reopen_after_restart(), output),
                Ok(response) => print(response.reply, output),
                Err(reply) => print(reply, output),
            }
        }
        ControlCli::Dictation {
            action,
            print,
            raw,
            output,
        } => {
            if let Err(error) = super::check_flags(action, print, raw) {
                eprintln!("hex: {}", error.message);
                return 2;
            }
            send(ControlCommand::Dictation { action, print, raw }, output)
        }
        ControlCli::PasteLast { output } => send(ControlCommand::PasteLast, output),
        ControlCli::Settings { command } => settings(command),
        ControlCli::Model { command } => match command {
            ModelCommand::List { output } => send(ControlCommand::ModelList, output),
            ModelCommand::Use {
                model,
                language,
                output,
            } => send(ControlCommand::ModelUse { model, language }, output),
            ModelCommand::Install {
                model,
                language,
                output,
            } => install_model(model, language, output),
            ModelCommand::Cancel { output } => send(ControlCommand::ModelCancel, output),
            command => offline_model(command),
        },
        ControlCli::Window { command } => match command {
            WindowCommand::Show { pane, output } => {
                send(ControlCommand::WindowShow { pane }, output)
            }
            WindowCommand::Hide { output } => send(ControlCommand::WindowHide, output),
            WindowCommand::Story {
                story,
                variant,
                play,
                output,
            } => send(
                ControlCommand::WindowStory {
                    story,
                    variant,
                    play,
                },
                output,
            ),
        },
        ControlCli::Onboarding {
            command: OnboardingCommand::Reset { output },
        } => send(ControlCommand::OnboardingReset, output),
        ControlCli::CheckForUpdates { output } => send(ControlCommand::CheckForUpdates, output),
        ControlCli::LaunchAtLogin { state, output } => send(
            ControlCommand::LaunchAtLogin {
                enabled: state.map(|state| matches!(state, Toggle::On)),
            },
            output,
        ),
        ControlCli::Vocabulary { command } => match command {
            VocabularyCommand::List { output } => send(ControlCommand::VocabularyList, output),
            VocabularyCommand::Add {
                term,
                sounds_like,
                output,
            } => send(
                ControlCommand::VocabularyAdd {
                    term,
                    sounds_like: sounds_like.into_iter().collect(),
                },
                output,
            ),
            VocabularyCommand::Remove { term, output } => {
                send(ControlCommand::VocabularyRemove { term }, output)
            }
        },
        ControlCli::Logs {
            process,
            lines,
            follow,
        } => logs(process, lines, follow),
        ControlCli::History(crate::control::transcript_cli::HistoryArgs {
            action: Some(crate::control::transcript_cli::HistoryAction::Clear { yes, output }),
            ..
        }) => clear_history(yes, output),
        ControlCli::History(args) => report(crate::control::transcript_cli::history(&args)),
        ControlCli::Meetings { command } => meetings(command),
        ControlCli::Mcp { meetings_dir } => report(mcp(meetings_dir)),
        ControlCli::Commands { command } => report(match command {
            CommandsCommand::Init => crate::personal_commands::initialize_workspace()
                .map(|workspace| println!("{}", workspace.display())),
            CommandsCommand::Check => {
                crate::personal_commands::check_workspace().map(|report| println!("{report}"))
            }
        }),
        ControlCli::Labs { command } => report(crate::labs::run(command.as_ref())),
        ControlCli::Experiments { .. } => report(crate::labs::run_retired_experiments()),
        ControlCli::Annotate { command } => capture(command),
    }
}

/// `hex model install`: the running Hex downloads in the background;
/// without one, Linux and Windows download here.
fn install_model(model: Option<String>, language: Option<String>, output: Output) -> u8 {
    let model = match model {
        Some(model) => model,
        None => match crate::app_settings::AppSettings::load() {
            Ok(settings) => settings.transcription.model.as_str().to_owned(),
            Err(error) => return fail(&format!("couldn't read settings: {error:#}")),
        },
    };
    #[cfg(not(target_os = "macos"))]
    if !running() {
        return report(selection(Some(model), language).and_then(|selection| {
            crate::transcription::local_transcriber::install(
                &selection,
                &std::sync::atomic::AtomicBool::new(false),
            )
        }));
    }
    send(ControlCommand::ModelInstall { model, language }, output)
}

/// The selection a local model command names: `model` with its remembered
/// language unless `language` is given, or the selected model.
#[cfg(not(target_os = "macos"))]
fn selection(
    model: Option<String>,
    language: Option<String>,
) -> color_eyre::Result<crate::transcription::models::TranscriptionSelection> {
    let settings = crate::app_settings::AppSettings::load().unwrap_or_default();
    let model = model.unwrap_or_else(|| settings.transcription.model.as_str().to_owned());
    super::model_selection(&settings, &model, language.as_deref())
        .map_err(|error| color_eyre::eyre::eyre!(error.message))
}

/// `hex model status|devices|check|transcribe`, run in this process.
#[cfg(not(target_os = "macos"))]
fn offline_model(command: ModelCommand) -> u8 {
    use crate::transcription::models::{definition, is_installed, is_verified, model_path};
    report((|| match command {
        ModelCommand::Status { model, language } => {
            let selection = selection(model, language)?;
            let model = definition(selection.model);
            let installed = is_installed(model, &selection.language);
            println!(
                "{}\t{}\t{}\t{}",
                if installed { "installed" } else { "missing" },
                if installed && is_verified(model) {
                    "verified"
                } else {
                    "unverified"
                },
                model.name,
                model_path(model)?.display()
            );
            Ok(())
        }
        ModelCommand::Devices => {
            for device in crate::transcription::local_transcriber::devices()? {
                println!("{device}");
            }
            Ok(())
        }
        ModelCommand::Check { model, language } => {
            let transcriber = crate::transcription::local_transcriber::LocalTranscriber::load(
                &selection(model, language)?,
            )?;
            println!("ready\t{}", transcriber.device_label());
            Ok(())
        }
        ModelCommand::Transcribe {
            wav,
            model,
            language,
        } => {
            println!(
                "{}",
                crate::transcription::local_transcriber::transcribe_wav_with_selection(
                    &wav,
                    &selection(model, language)?
                )?
            );
            Ok(())
        }
        _ => unreachable!("Hex carries out the other model commands"),
    })())
}

/// macOS runs models only inside the app.
#[cfg(target_os = "macos")]
fn offline_model(_: ModelCommand) -> u8 {
    print(
        ControlReply::error(
            "unsupported",
            "this `hex model` command is for Linux and Windows; use `hex model list`",
        ),
        Output::default(),
    )
}

/// The exit code of a command that runs here rather than in Hex.
fn report(result: color_eyre::Result<()>) -> u8 {
    match result {
        Ok(()) => 0,
        Err(error) => fail(&format!("{error:#}")),
    }
}

/// `hex meetings`: record, stop, and copy in the running Hex; the rest
/// read the meetings folder here.
fn meetings(command: crate::control::transcript_cli::MeetingsCommand) -> u8 {
    use crate::control::transcript_cli::MeetingsCommand;
    match command {
        MeetingsCommand::Record { title, output } => {
            send(ControlCommand::MeetingRecord { title }, output)
        }
        MeetingsCommand::Stop { output } => send(ControlCommand::MeetingStop, output),
        MeetingsCommand::CopyLast { output } => send(ControlCommand::MeetingCopyLast, output),
        command => report(crate::control::transcript_cli::meetings(&command)),
    }
}

/// `hex mcp`: logs to stderr only (stdout carries MCP messages) and touches
/// no app state.
fn mcp(meetings_dir: Option<std::path::PathBuf>) -> color_eyre::Result<()> {
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
        None => crate::meeting::root()?,
    };
    crate::meeting::mcp::run(root)
}

/// `hex annotate` (alias `hex capture`).
#[derive(Subcommand, Debug)]
pub enum CaptureCommand {
    /// Start recording a capture.
    Start {
        /// Linux: record this area instead of the screen, as X,Y,WIDTH,HEIGHT in pixels (on
        /// X11; Wayland's portal records the screen).
        #[cfg(target_os = "linux")]
        #[arg(long, value_delimiter = ',', value_name = "X,Y,W,H")]
        area: Option<Vec<f64>>,
        #[command(flatten)]
        output: Output,
    },
    /// End the capture and write it in the background.
    Stop {
        #[command(flatten)]
        output: Output,
    },
    /// End the capture and discard it.
    Cancel {
        #[command(flatten)]
        output: Output,
    },
    /// What records, what is being written, and the last capture.
    Status {
        #[command(flatten)]
        output: Output,
    },
    /// The newest captures, newest first.
    List {
        /// How many to show (20 unless given).
        #[arg(long)]
        limit: Option<usize>,
        #[command(flatten)]
        output: Output,
    },
    /// One capture's words, files, and where it went.
    Show {
        /// A bundle name, path, or `latest`.
        capture: String,
        #[command(flatten)]
        output: Output,
    },
    /// Send a capture to an OpenCode session.
    Send {
        /// A bundle name, path, or `latest`.
        capture: String,
        /// A session ID, or for the newest capture a session its route
        /// lists by name. Without it, where its route or last send says.
        #[arg(long)]
        to: Option<String>,
        /// Also save a rule that always sends this capture's site or app
        /// there (the newest routed capture only).
        #[arg(long)]
        always: bool,
        #[command(flatten)]
        output: Output,
    },
    /// Turn the pen on or off while a recording runs (as the capsule's pen),
    /// or start one that draws. Bind it in your desktop's keyboard shortcuts
    /// when it has no shortcuts portal.
    #[cfg(target_os = "linux")]
    Draw {
        #[command(flatten)]
        output: Output,
    },
    /// Take back the newest send while it can be undone (as the capsule's
    /// Undo).
    Undo {
        #[command(flatten)]
        output: Output,
    },
    /// What routing sees for a capture and what it decides (Scrawl's
    /// `route --explain`). Sends nothing.
    Route {
        /// A bundle name, path, or `latest`.
        capture: String,
        /// Accepted for Scrawl's spelling; `route` only ever explains.
        #[arg(long, hide = true)]
        explain: bool,
        /// Ask Jev again now (one call).
        #[arg(long)]
        ask_jev: bool,
        #[command(flatten)]
        output: Output,
    },
}

/// Runs `hex capture`: on macOS through the running Hex; on Linux the
/// folder reads (`list`, `show`, `route`, `send`) run here, and the
/// recording commands go to the running Hex's listener; on Windows every
/// command goes to the running Hex. A build without the capture engine
/// answers `unsupported`.
pub fn capture(command: CaptureCommand) -> u8 {
    use crate::control::{ControlCommand as C, ScrawlCaptureAction as A};
    let (command, output) = match command {
        #[cfg(target_os = "linux")]
        CaptureCommand::Start {
            area: Some(area),
            output,
        } => {
            let [x, y, width, height] = area[..] else {
                return print(
                    ControlReply::error("invalid", "--area takes X,Y,WIDTH,HEIGHT"),
                    output,
                );
            };
            return print(
                crate::linux::service::capture_area([x, y, width, height]),
                output,
            );
        }
        CaptureCommand::Start { output, .. } => (C::ScrawlCapture { action: A::Start }, output),
        CaptureCommand::Stop { output } => (C::ScrawlCapture { action: A::Stop }, output),
        CaptureCommand::Cancel { output } => (C::ScrawlCapture { action: A::Cancel }, output),
        CaptureCommand::Status { output } => (C::ScrawlCapture { action: A::Status }, output),
        CaptureCommand::List { limit, output } => (C::ScrawlCaptureList { limit }, output),
        CaptureCommand::Show { capture, output } => (C::ScrawlCaptureShow { capture }, output),
        CaptureCommand::Send {
            capture,
            to,
            always,
            output,
        } => (
            C::ScrawlCaptureSend {
                capture,
                to,
                always,
            },
            output,
        ),
        CaptureCommand::Route {
            capture,
            explain: _,
            ask_jev,
            output,
        } => (C::ScrawlCaptureRoute { capture, ask_jev }, output),
        CaptureCommand::Undo { output } => (C::ScrawlCaptureUndo, output),
        #[cfg(target_os = "linux")]
        CaptureCommand::Draw { output } => {
            return print(crate::linux::service::capture_draw(), output);
        }
    };
    #[cfg(not(hex_capture))]
    {
        let _ = command;
        print(
            ControlReply::error("unsupported", "Annotate isn't available on this OS yet"),
            output,
        )
    }
    #[cfg(all(hex_capture, any(target_os = "macos", target_os = "windows")))]
    return send(command, output);
    #[cfg(all(hex_capture, target_os = "linux"))]
    {
        let reply = match command {
            C::ScrawlCaptureList { limit } => crate::capture_host::list_captures(limit),
            C::ScrawlCaptureShow { capture } => crate::capture_host::show_capture(&capture),
            // The running Hex's router sends, so its Undo can take it back.
            C::ScrawlCaptureSend {
                capture,
                to,
                always,
            } => Ok(crate::linux::service::capture_send(capture, to, always)),
            C::ScrawlCaptureUndo => Ok(crate::linux::service::capture_undo()),
            C::ScrawlCaptureRoute { capture, ask_jev } => {
                crate::capture_host::explain_capture(&capture, ask_jev)
            }
            C::ScrawlCapture { action } => Ok(crate::linux::service::capture(action)),
            _ => unreachable!("only capture commands"),
        };
        print(reply.unwrap_or_else(ControlReply::from), output)
    }
}

pub fn settings(command: SettingsCommand) -> u8 {
    match command {
        SettingsCommand::List { output } => send(ControlCommand::SettingsGet { key: None }, output),
        SettingsCommand::Get { key, output } => {
            send(ControlCommand::SettingsGet { key: Some(key) }, output)
        }
        SettingsCommand::Set { key, value, output } => {
            send(ControlCommand::SettingsSet { key, value }, output)
        }
        SettingsCommand::Apply { file, output } => apply_settings(&file, output),
    }
}

/// `hex settings apply`: the running Hex saves and adopts the settings, so
/// it never overwrites them from memory; with no Hex running they are
/// written to settings.json here.
fn apply_settings(file: &std::path::Path, output: Output) -> u8 {
    use std::io::Read;

    let mut text = String::new();
    let read = if file == std::path::Path::new("-") {
        std::io::stdin().read_to_string(&mut text).map(drop)
    } else {
        std::fs::read_to_string(file).map(|read| text = read)
    };
    if let Err(error) = read {
        return fail(&format!("couldn't read {}: {error}", file.display()));
    }
    let values: serde_json::Map<String, serde_json::Value> = match serde_json::from_str(&text) {
        Ok(values) => values,
        Err(error) => return fail(&format!("expected a JSON object of settings: {error}")),
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(35);
    loop {
        if running() {
            return send(ControlCommand::SettingsApply { values }, output);
        }
        // A Hex that holds the service lock may have read settings.json
        // already; wait for it to answer instead of writing behind it.
        #[cfg(target_os = "linux")]
        let lock = crate::desktop::instance::acquire("service");
        #[cfg(not(target_os = "linux"))]
        let lock: color_eyre::Result<()> = Ok(());
        match lock {
            Ok(_lock) if !running() => return print(apply_offline(&values), output),
            Ok(_) => {}
            Err(_) if std::time::Instant::now() >= deadline => {
                return print(
                    ControlReply::error(
                        "busy",
                        "Hex is starting but didn't answer; run `hex settings apply` again",
                    ),
                    output,
                );
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
}

fn apply_offline(values: &serde_json::Map<String, serde_json::Value>) -> ControlReply {
    let result = crate::app_settings::AppSettings::load()
        .map_err(|error| {
            super::ControlError::new("internal", format!("couldn't read settings: {error:#}"))
        })
        .and_then(|before| {
            let change =
                super::settings::apply_all(&before, values, &crate::rune_ui::shortcut_rules())?;
            if !change.changed(&before) {
                return Ok(ControlReply::unchanged(
                    "Every setting already has its value",
                ));
            }
            change.settings.save().map_err(|error| {
                super::ControlError::new("internal", format!("couldn't save: {error:#}"))
            })?;
            Ok(ControlReply::done(change.message))
        });
    result.unwrap_or_else(ControlReply::from)
}

/// `hex history clear`; the confirmation is asked here, before Hex is.
pub fn clear_history(confirm: bool, output: Output) -> u8 {
    if !confirm {
        return print(
            ControlReply::error(
                "confirmation-required",
                "this deletes every retained dictation; pass --yes to clear history",
            ),
            output,
        );
    }
    send(ControlCommand::HistoryClear, output)
}

pub fn send(command: ControlCommand, output: Output) -> u8 {
    match call(ControlRequest::new(command)) {
        Ok(response) => print(response.reply, output),
        Err(reply) => print(reply, output),
    }
}

/// Every OS reaches the running Hex through its local API's owner-only
/// discovery file and bearer-authenticated `POST /control`.
fn running() -> bool {
    crate::local_api::running_pid().is_some()
}

/// `hex settings set key value` in the running Hex, which saves and applies
/// it; `None` when no Hex runs.
pub(crate) fn set_in_running_hex(key: &str, value: &str) -> Option<ControlReply> {
    running().then(|| {
        call(ControlRequest::new(ControlCommand::SettingsSet {
            key: key.into(),
            value: value.into(),
        }))
        .map_or_else(|reply| reply, |response| response.reply)
    })
}

fn call(request: ControlRequest) -> Result<ControlResponse, ControlReply> {
    if !running() {
        return Err(not_running());
    }
    crate::local_api::call_control(&request)
        .map_err(|error| ControlReply::error("transport", format!("{error:#}")))
}

pub(crate) fn not_running() -> ControlReply {
    ControlReply::error("not-running", "Hex is not running; run `hex start`")
}

#[cfg(target_os = "macos")]
fn start(output: Output) -> u8 {
    use std::time::{Duration, Instant};

    if let Some(pid) = crate::local_api::running_pid() {
        return print(
            ControlReply::unchanged(format!("Hex is already running (pid {pid})")),
            output,
        );
    }
    let arguments = open_arguments(
        installed_bundle().as_deref(),
        crate::app_paths::is_macos_test_build(),
        std::env::vars_os(),
    );
    match std::process::Command::new("/usr/bin/open")
        .args(arguments)
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(_) | Err(_) => {
            return print(
                ControlReply::error("failed", "couldn't open Hex.app; is it installed?"),
                output,
            );
        }
    }
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(20) {
        if let Some(pid) = crate::local_api::running_pid() {
            return print(
                ControlReply::done(format!("Started Hex (pid {pid})")),
                output,
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    print(
        ControlReply::done("Opened Hex; it isn't answering yet (setup may be waiting)"),
        output,
    )
}

/// `hex start` on Linux and Windows: a detached `hex app --hidden`, which
/// forwards to the running Hex, starts its systemd unit, or becomes Hex,
/// then waits until it answers.
#[cfg(not(target_os = "macos"))]
fn start(output: Output) -> u8 {
    print(start_reply(), output)
}

#[cfg(not(target_os = "macos"))]
fn start_reply() -> ControlReply {
    use std::time::{Duration, Instant};

    if running() {
        return ControlReply::unchanged("Hex is already running");
    }
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => return ControlReply::error("failed", format!("couldn't find hex: {error}")),
    };
    let mut command = std::process::Command::new(executable);
    command
        .args(["app", "--hidden"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(target_os = "linux")]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP: no console, and
        // Ctrl-C in this terminal doesn't reach it.
        command.creation_flags(0x0000_0008 | 0x0000_0200);
    }
    if let Err(error) = command.spawn() {
        return ControlReply::error("failed", format!("couldn't start Hex: {error}"));
    }
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(20) {
        if running() {
            return ControlReply::done("Started Hex");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    ControlReply::done("Started Hex; it isn't answering yet (it may be loading its model)")
}

/// After a restart's quit on Linux and Windows: waits for the old Hex to
/// go, then starts the new one. A Linux unit restarts by itself.
#[cfg(not(target_os = "macos"))]
fn reopen_after_restart() -> ControlReply {
    use std::time::{Duration, Instant};

    #[cfg(target_os = "linux")]
    if crate::linux::service::supervised() {
        return ControlReply::done("Restarting Hex");
    }
    let started = Instant::now();
    while running() {
        if started.elapsed() > Duration::from_secs(30) {
            return ControlReply::error(
                "failed",
                "Hex is still finishing its work; run `hex start` once it quits",
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    start_reply()
}

/// `open` arguments for `hex start`: the caller's `HEX_*` environment, then
/// the Hex.app this `hex` belongs to, or the installed app by bundle id.
#[cfg(target_os = "macos")]
fn open_arguments(
    bundle: Option<&std::path::Path>,
    test_build: bool,
    vars: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
) -> Vec<std::ffi::OsString> {
    let mut arguments = crate::desktop::onboarding::open_env_arguments(vars);
    match bundle {
        Some(bundle) => arguments.push(bundle.into()),
        None => arguments.extend([
            "-b".into(),
            if test_build {
                "anomaly.hex.test"
            } else {
                "anomaly.hex"
            }
            .into(),
        ]),
    }
    arguments
}

/// The Hex.app this `hex` belongs to, when it runs from (or links into) one.
#[cfg(target_os = "macos")]
fn installed_bundle() -> Option<std::path::PathBuf> {
    let executable = std::fs::canonicalize(std::env::current_exe().ok()?).ok()?;
    executable
        .ancestors()
        .nth(3)
        .filter(|path| path.extension().is_some_and(|extension| extension == "app"))
        .map(ToOwned::to_owned)
}

fn logs(process: bool, lines: usize, follow: bool) -> u8 {
    use std::io::{BufRead, Seek, SeekFrom};

    let path = match crate::app_paths::logs_dir() {
        Ok(directory) => directory.join(if process {
            "process.log"
        } else {
            "live.ndjson"
        }),
        Err(error) => return fail(&format!("{error:#}")),
    };
    let mut file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) => return fail(&format!("couldn't read {}: {error}", path.display())),
    };
    let length = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    // Read at most the last 4 MiB for the tail.
    let start = length.saturating_sub(4 * 1024 * 1024);
    let mut reader = std::io::BufReader::new(&mut file);
    if reader.seek(SeekFrom::Start(start)).is_err() {
        return fail("couldn't read the log");
    }
    let mut tail = std::collections::VecDeque::with_capacity(lines.min(10_000));
    let mut line = String::new();
    let mut first = start > 0;
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        if !std::mem::take(&mut first) {
            if tail.len() == lines {
                tail.pop_front();
            }
            if lines > 0 {
                tail.push_back(line.clone());
            }
        }
        line.clear();
    }
    let mut stdout = std::io::stdout().lock();
    for line in tail {
        let _ = stdout.write_all(line.as_bytes());
    }
    let _ = stdout.flush();
    if !follow {
        return 0;
    }
    let mut position = reader.stream_position().unwrap_or(length);
    drop(reader);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let length = match std::fs::metadata(&path) {
            Ok(metadata) => metadata.len(),
            Err(_) => continue,
        };
        if length < position {
            // Rotated or truncated: start over from the new file.
            match std::fs::File::open(&path) {
                Ok(reopened) => file = reopened,
                Err(_) => continue,
            }
            position = 0;
        }
        if length == position {
            continue;
        }
        if file.seek(SeekFrom::Start(position)).is_err() {
            continue;
        }
        let mut reader = std::io::BufReader::new(&mut file);
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            if !line.ends_with('\n') {
                break;
            }
            position += line.len() as u64;
            if stdout.write_all(line.as_bytes()).is_err() {
                return 0;
            }
            line.clear();
        }
        line.clear();
        let _ = stdout.flush();
    }
}

fn fail(message: &str) -> u8 {
    eprintln!("hex: {message}");
    EXIT_REFUSED
}

/// Prints `reply` and returns its exit code.
pub(crate) fn print(reply: ControlReply, output: Output) -> u8 {
    let code = exit_code(&reply);
    if output.json {
        let response = ControlResponse {
            version: CONTROL_VERSION,
            reply,
        };
        match serde_json::to_string_pretty(&response) {
            Ok(json) => println!("{json}"),
            Err(error) => return fail(&error.to_string()),
        }
        return code;
    }
    match reply {
        ControlReply::Done { message, .. } => println!("{message}"),
        ControlReply::Status(report) => print!("{}", human_status(&report)),
        ControlReply::Settings { settings } => print!("{}", human_settings(&settings)),
        ControlReply::Models { models } => print!("{}", human_models(&models)),
        ControlReply::Vocabulary { entries } => print!("{}", human_vocabulary(&entries)),
        ControlReply::ScrawlCapture(report) => print!("{}", human_scrawl_capture(&report)),
        ControlReply::ScrawlCaptures { captures } => print!("{}", human_captures(&captures)),
        ControlReply::ScrawlCaptureDetail(detail) => print!("{}", human_capture_detail(&detail)),
        ControlReply::ScrawlCaptureRoute { explanation } => print!("{explanation}"),
        ControlReply::Transcript { text } => println!("{text}"),
        ControlReply::Error { message, .. } => eprintln!("hex: {message}"),
    }
    code
}

fn human_scrawl_capture(report: &super::ScrawlCaptureReport) -> String {
    use super::ScrawlCaptureState;
    let mut out = String::new();
    let bundle = |bundle: &Option<std::path::PathBuf>| {
        bundle
            .as_ref()
            .map_or_else(String::new, |bundle| bundle.display().to_string())
    };
    match (report.state, report.changed) {
        (ScrawlCaptureState::Recording, true) => {
            out.push_str(&format!("Capturing into {}\n", bundle(&report.bundle)));
        }
        (ScrawlCaptureState::Recording, false) => out.push_str(&format!(
            "Capturing into {} ({} ms)\n",
            bundle(&report.bundle),
            report.elapsed_ms.unwrap_or_default()
        )),
        (ScrawlCaptureState::Idle, true) => {
            out.push_str(&format!("Ended the capture {}\n", bundle(&report.bundle)));
        }
        (ScrawlCaptureState::Idle, false) => out.push_str("No capture is recording\n"),
    }
    for finishing in &report.finishing {
        out.push_str(&format!("Writing {}\n", finishing.display()));
    }
    if let Some(last) = &report.last {
        let words = last
            .words
            .map_or_else(String::new, |words| format!(", {words} words"));
        let gaps = last
            .gaps
            .filter(|gaps| *gaps > 0)
            .map_or_else(String::new, |gaps| format!(", {gaps} audio gaps"));
        let missing = last
            .missing_transcript
            .as_ref()
            .map_or_else(String::new, |why| format!(", no transcript ({why})"));
        out.push_str(&format!(
            "Last: {} {}{words}{gaps}{missing}\n",
            last.stage,
            last.bundle.display()
        ));
    }
    out
}

fn human_captures(captures: &[super::ScrawlCaptureSummary]) -> String {
    if captures.is_empty() {
        return "No captures\n".into();
    }
    captures
        .iter()
        .map(|capture| format!("{}\n", capture_line(capture)))
        .collect()
}

/// `1792933491953.scrawl  2026-10-26 14:02  0:04  window · Brave  12 words  → Hex`
fn capture_line(capture: &super::ScrawlCaptureSummary) -> String {
    let seconds = capture.duration_ms / 1_000;
    let mut line = format!(
        "{}  {}  {}:{:02}  {}",
        capture.id,
        local_minute(capture.created_at_ms),
        seconds / 60,
        seconds % 60,
        capture.mode
    );
    if let Some(app) = &capture.app {
        line.push_str(&format!(" · {app}"));
    }
    line.push_str(&format!("  {} words", capture.words));
    if let Some(sent) = &capture.sent_to {
        line.push_str(&format!("  → {}", sent.title));
    }
    if capture.queued {
        line.push_str(" (queued)");
    }
    line
}

fn human_capture_detail(detail: &super::ScrawlCaptureDetail) -> String {
    let mut out = format!(
        "{}\n{}\n",
        capture_line(&detail.summary),
        detail.summary.bundle.display()
    );
    if let Some(title) = &detail.window_title {
        out.push_str(&format!("Window: {title}\n"));
    }
    if let Some(sent) = &detail.summary.sent_to {
        out.push_str(&format!("Sent to: {} ({})\n", sent.title, sent.session));
    }
    out.push_str(&format!(
        "{} marks, {} keyframes\n",
        detail.marks, detail.keyframes
    ));
    for (label, path) in [
        ("Video", &detail.video),
        ("Audio", &detail.audio),
        ("Contact sheet", &detail.contact_sheet),
    ] {
        if let Some(path) = path {
            out.push_str(&format!("{label}: {}\n", path.display()));
        }
    }
    if let Some(note) = &detail.note {
        out.push_str(&format!("\nNote: {note}\n"));
    }
    match (&detail.missing_transcript, detail.transcript.trim()) {
        (Some(why), _) => out.push_str(&format!("\nNo transcript: {why}\n")),
        (None, "") => out.push_str("\nNo words\n"),
        (None, text) => out.push_str(&format!("\n{text}\n")),
    }
    out
}

/// `2026-10-26 14:02` in local time where Hex knows it.
fn local_minute(ms: u64) -> String {
    let iso = crate::meeting::query::iso_local(ms);
    iso.get(..16)
        .map_or(iso.clone(), |minute| minute.replace('T', " "))
}

pub fn exit_code(reply: &ControlReply) -> u8 {
    match reply {
        ControlReply::Error { code, .. } if code == "not-running" => EXIT_NOT_RUNNING,
        ControlReply::Error { code, .. } if code == "no-speech" => EXIT_NO_SPEECH,
        ControlReply::Error { .. } => EXIT_REFUSED,
        _ => 0,
    }
}

fn human_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "none".into(),
        other => other.to_string(),
    }
}

fn human_settings(settings: &[SettingValue]) -> String {
    if let [setting] = settings {
        return format!("{}\n", human_value(&setting.value));
    }
    let width = settings
        .iter()
        .map(|setting| setting.key.len())
        .max()
        .unwrap_or(0);
    settings
        .iter()
        .map(|setting| {
            let mut value = human_value(&setting.value);
            if value.len() > 60 {
                value = format!("{}…", value.chars().take(59).collect::<String>());
            }
            format!(
                "{:width$}  {value}{}\n",
                setting.key,
                if !setting.available {
                    "  (not on this OS)"
                } else if setting.writable {
                    ""
                } else {
                    "  (read-only)"
                }
            )
        })
        .collect()
}

fn human_models(models: &[ModelInfo]) -> String {
    let width = models.iter().map(|model| model.id.len()).max().unwrap_or(0);
    let name_width = models
        .iter()
        .map(|model| model.name.len())
        .max()
        .unwrap_or(0);
    models
        .iter()
        .map(|model| {
            let languages = if model.languages.len() > 6 {
                format!("{} languages", model.languages.len())
            } else {
                model.languages.join(", ")
            };
            format!(
                "{} {:width$}  {:name_width$}  {:>8}  {:13}  {languages}\n",
                if model.selected { "*" } else { " " },
                model.id,
                model.name,
                model.size,
                if model.installed {
                    "installed"
                } else {
                    "not installed"
                },
            )
        })
        .collect()
}

fn human_vocabulary(entries: &[VocabularyEntry]) -> String {
    if entries.is_empty() {
        return "The vocabulary is empty; `hex vocabulary add <term>` adds one\n".into();
    }
    entries
        .iter()
        .map(|entry| match entry.sounds_like.as_slice() {
            [] => format!("{}\n", entry.term),
            spellings => format!("{}  (sounds like {})\n", entry.term, spellings.join(", ")),
        })
        .collect()
}

fn duration(seconds: u64) -> String {
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

fn on(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}

fn human_status(report: &StatusReport) -> String {
    let memory = report
        .memory_footprint_bytes
        .map(|bytes| format!(", {} MB", bytes / 1_000_000))
        .unwrap_or_default();
    let capture = match report.capture.recording {
        Some(kind) => format!(
            "recording {}",
            serde_json::to_value(kind)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default()
        ),
        None => serde_json::to_value(report.capture.state)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default(),
    };
    let pending = match report.capture.pending_jobs {
        0 => String::new(),
        jobs => format!(" ({jobs} pending)"),
    };
    let meeting = match (&report.meeting.title, report.meeting.elapsed_seconds) {
        (Some(title), Some(elapsed)) => {
            format!(
                "{} “{title}” for {}",
                report.meeting.state,
                duration(elapsed)
            )
        }
        _ if report.features.meetings => report.meeting.state.clone(),
        _ => "off".into(),
    };
    let model = &report.model;
    let mut model_line = format!(
        "{} ({}){}",
        model.name,
        model.language,
        if model.installed {
            ""
        } else {
            ", not installed"
        }
    );
    if let Some(preparing) = &model.preparing {
        model_line.push_str(&format!(
            "; preparing {preparing} ({} MB)",
            model.downloaded_bytes / 1_000_000
        ));
    }
    if let Some(error) = &model.error {
        model_line.push_str(&format!("; error: {error}"));
    }
    let features = &report.features;
    let permissions = &report.permissions;
    let permission_line = if permissions.ready {
        "ready".to_string()
    } else {
        format!(
            "microphone {}, accessibility {}, model {}",
            permissions.microphone,
            permissions.accessibility,
            if permissions.transcription_model {
                "ready"
            } else {
                "missing"
            }
        )
    };
    let window = match (&report.window.pane, report.window.open) {
        (Some(pane), true) => format!("open ({pane})"),
        (None, true) => "open".into(),
        _ => "closed".into(),
    };
    format!(
        "Hex {} (pid {}{memory})\n\
         Listener     {}\n\
         Capture      {capture}{pending}\n\
         Meeting      {meeting}\n\
         Model        {model_line}\n\
         Features     meetings {}, release mic while idle {}, history {}\n\
         Permissions  {permission_line}\n\
         Window       {window}\n\
         Update       {}\n\
         Labs         {}\n",
        report.app_version,
        report.pid,
        if report.listener.running {
            format!("running: {}", report.listener.status)
        } else {
            format!("waiting: {}", report.listener.status)
        },
        on(features.meetings),
        on(features.release_microphone_while_idle),
        features.history_retention,
        report.update,
        match report.labs.as_slice() {
            [] => "none".to_string(),
            names => names.join(", "),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_are_stable() {
        assert_eq!(exit_code(&ControlReply::done("ok")), 0);
        assert_eq!(exit_code(&ControlReply::unchanged("ok")), 0);
        assert_eq!(exit_code(&ControlReply::error("busy", "x")), EXIT_REFUSED);
        assert_eq!(exit_code(&not_running()), EXIT_NOT_RUNNING);
        assert_eq!(
            exit_code(&ControlReply::error("no-speech", "x")),
            EXIT_NO_SPEECH
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn start_forwards_the_callers_hex_environment() {
        let vars = || {
            [
                ("HOME", "/Users/kit"),
                ("HEX_RETAIN_DICTATION_AUDIO", "1"),
                ("HEX_ENABLE_LLM", "1"),
            ]
            .map(|(key, value)| (key.into(), value.into()))
        };
        let strings = |arguments: Vec<std::ffi::OsString>| {
            arguments
                .into_iter()
                .map(|argument| argument.into_string().unwrap())
                .collect::<Vec<_>>()
        };
        let env = [
            "--env",
            "HEX_ENABLE_LLM=1",
            "--env",
            "HEX_RETAIN_DICTATION_AUDIO=1",
        ];
        let bundle = std::path::Path::new("/Applications/Hex.app");
        assert_eq!(
            strings(open_arguments(Some(bundle), false, vars())),
            [&env[..], &["/Applications/Hex.app"]].concat()
        );
        assert_eq!(
            strings(open_arguments(None, false, vars())),
            [&env[..], &["-b", "anomaly.hex"]].concat()
        );
        assert_eq!(
            strings(open_arguments(None, true, [])),
            ["-b", "anomaly.hex.test"]
        );
    }

    #[test]
    fn one_setting_prints_its_bare_value() {
        let setting = |key: &str, value: Value| SettingValue {
            key: key.into(),
            value,
            kind: "text".into(),
            choices: vec![],
            writable: true,
            about: String::new(),
            available: true,
        };
        assert_eq!(
            human_settings(&[setting("dictation_hotkey", "option".into())]),
            "option\n"
        );
        assert_eq!(human_settings(&[setting("a", Value::Null)]), "none\n");
        let listed = human_settings(&[
            setting("sound_effects", true.into()),
            setting("a", 0.5.into()),
        ]);
        assert_eq!(listed, "sound_effects  true\na              0.5\n");
    }

    #[test]
    fn vocabulary_lists_terms_with_their_spellings() {
        assert!(human_vocabulary(&[]).starts_with("The vocabulary is empty"));
        let entries = [
            VocabularyEntry {
                term: "cappuccino".into(),
                sounds_like: vec!["capuchin".into(), "cap a chino".into()],
            },
            VocabularyEntry {
                term: "Rune".into(),
                sounds_like: vec![],
            },
        ];
        assert_eq!(
            human_vocabulary(&entries),
            "cappuccino  (sounds like capuchin, cap a chino)\nRune\n"
        );
    }

    #[test]
    fn status_reads_for_a_person() {
        let mut report = StatusReport {
            app_version: "2.3.1".into(),
            pid: 42,
            memory_footprint_bytes: Some(412_000_000),
            ..Default::default()
        };
        report.meeting.state = "recording".into();
        report.meeting.title = Some("Design sync".into());
        report.meeting.elapsed_seconds = Some(3723);
        report.features.meetings = true;
        let text = human_status(&report);
        assert!(text.starts_with("Hex 2.3.1 (pid 42, 412 MB)\n"));
        assert!(text.contains("Meeting      recording “Design sync” for 1:02:03\n"));
        assert!(text.ends_with("Labs         none\n"));
        report.labs = vec!["live-text".into(), "meetings".into()];
        assert!(human_status(&report).ends_with("Labs         live-text, meetings\n"));
    }

    #[test]
    fn status_reports_labs_and_reads_older_replies_without_it() {
        let report = StatusReport {
            labs: vec!["live-text".into()],
            ..Default::default()
        };
        let mut json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["labs"], serde_json::json!(["live-text"]));
        json.as_object_mut().unwrap().remove("labs");
        let older: StatusReport = serde_json::from_value(json).unwrap();
        assert!(older.labs.is_empty());
    }
}

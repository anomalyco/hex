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
use crate::vocabulary::VocabularyEntry;

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
    /// Replay the first-run onboarding (dev, test, and local builds).
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
    /// Download a model, then switch to it.
    Install {
        model: String,
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
}

#[derive(Subcommand, Debug)]
pub enum OnboardingCommand {
    /// Open the first-run onboarding in the window now, as on a new Mac.
    ///
    /// Turns the new onboarding on for this install and forgets that
    /// onboarding finished. Settings, history, models, and permissions stay,
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
    /// Experiment: open the window on a playground story by its path, name,
    /// or slug (`Prototypes/HUD/Processing`, `prototypes-hud-processing`; `hex experiments enable
    /// playground`).
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
        /// (`--sounds-like "avatar,a bat war"`).
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
            if running() {
                send(ControlCommand::Restart { force }, output)
            } else {
                start(output)
            }
        }
        ControlCli::Dictation {
            action,
            print,
            output,
        } => {
            if let Err(error) = super::check_print(action, print) {
                eprintln!("hex: {}", error.message);
                return 2;
            }
            send(ControlCommand::Dictation { action, print }, output)
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
            } => send(ControlCommand::ModelInstall { model, language }, output),
            ModelCommand::Cancel { output } => send(ControlCommand::ModelCancel, output),
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
    }
}

/// `hex capture` (the `capture` experiment).
#[cfg(hex_capture)]
#[derive(Subcommand)]
pub enum CaptureCommand {
    /// Start recording a capture.
    Start {
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
/// recording commands go to the running Hex's listener.
#[cfg(hex_capture)]
pub fn capture(command: CaptureCommand) -> u8 {
    use crate::control::{ControlCommand as C, ScrawlCaptureAction as A};
    let (command, output) = match command {
        CaptureCommand::Start { output } => (C::ScrawlCapture { action: A::Start }, output),
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
            return print(crate::linux_service::capture_draw(), output);
        }
    };
    #[cfg(target_os = "macos")]
    return send(command, output);
    #[cfg(target_os = "linux")]
    {
        let reply = match command {
            C::ScrawlCaptureList { limit } => crate::capture_host::list_captures(limit),
            C::ScrawlCaptureShow { capture } => crate::capture_host::show_capture(&capture),
            // The running Hex's router sends, so its Undo can take it back.
            C::ScrawlCaptureSend {
                capture,
                to,
                always,
            } => Ok(crate::linux_service::capture_send(capture, to, always)),
            C::ScrawlCaptureUndo => Ok(crate::linux_service::capture_undo()),
            C::ScrawlCaptureRoute { capture, ask_jev } => {
                crate::capture_host::explain_capture(&capture, ask_jev)
            }
            C::ScrawlCapture { action } => Ok(crate::linux_service::capture(action)),
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
    }
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

#[cfg(target_os = "macos")]
fn running() -> bool {
    crate::local_api::running_pid().is_some()
}

#[cfg(not(target_os = "macos"))]
fn running() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn call(request: ControlRequest) -> Result<ControlResponse, ControlReply> {
    if !running() {
        return Err(not_running());
    }
    crate::local_api::call_control(&request)
        .map_err(|error| ControlReply::error("transport", format!("{error:#}")))
}

#[cfg(not(target_os = "macos"))]
fn call(_request: ControlRequest) -> Result<ControlResponse, ControlReply> {
    Err(unsupported())
}

fn not_running() -> ControlReply {
    ControlReply::error("not-running", "Hex is not running; run `hex start`")
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn unsupported() -> ControlReply {
    ControlReply::error(
        "unsupported",
        "this command is not supported on this OS yet; use the Hex window",
    )
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

#[cfg(not(target_os = "macos"))]
fn start(output: Output) -> u8 {
    print(unsupported(), output)
}

/// `open` arguments for `hex start`: the caller's `HEX_*` environment, then
/// the Hex.app this `hex` belongs to, or the installed app by bundle id.
#[cfg(target_os = "macos")]
fn open_arguments(
    bundle: Option<&std::path::Path>,
    test_build: bool,
    vars: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
) -> Vec<std::ffi::OsString> {
    let mut arguments = crate::onboarding::open_env_arguments(vars);
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
    #[cfg(target_os = "macos")]
    {
        let iso = crate::meeting::query::iso_local(ms);
        iso.get(..16)
            .map_or(iso.clone(), |minute| minute.replace('T', " "))
    }
    #[cfg(not(target_os = "macos"))]
    {
        format!("{ms} ms")
    }
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
                if setting.writable {
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
    // Shown only when on: experiments are unsupported and off for nearly everyone.
    let experiments = match report.experiments.as_slice() {
        [] => String::new(),
        names => format!("Experiments  {}\n", names.join(", ")),
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
         {experiments}",
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
                term: "abattoir".into(),
                sounds_like: vec!["avatar".into(), "a bat war".into()],
            },
            VocabularyEntry {
                term: "Rune".into(),
                sounds_like: vec![],
            },
        ];
        assert_eq!(
            human_vocabulary(&entries),
            "abattoir  (sounds like avatar, a bat war)\nRune\n"
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
        assert!(!text.contains("Experiments"));

        report.experiments = vec!["live-chapters".into(), "live-text".into()];
        assert!(human_status(&report).ends_with("Experiments  live-chapters, live-text\n"));
    }

    #[test]
    fn status_reports_experiments_and_reads_older_replies_without_them() {
        let report = StatusReport {
            experiments: vec!["live-text".into()],
            ..Default::default()
        };
        let mut json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["experiments"], serde_json::json!(["live-text"]));
        json.as_object_mut().unwrap().remove("experiments");
        let older: StatusReport = serde_json::from_value(json).unwrap();
        assert!(older.experiments.is_empty());
    }
}

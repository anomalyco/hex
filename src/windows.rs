use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use clap::{Args, Parser, Subcommand};
use color_eyre::Result;
use color_eyre::eyre::{WrapErr, bail};
use windows_sys::Win32::System::Console::{
    FreeConsole, GetConsoleMode, GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE,
    STD_OUTPUT_HANDLE, SetStdHandle,
};

use crate::audio::{AudioInput, AudioInputEvent};
use crate::events::{DictationPhase, EventLog, TranscriptPhase, VoiceEvent, VoiceState, now_ms};
use crate::local_transcriber::LocalTranscriber;
use crate::transcription_models::{
    TranscriptionModelId, TranscriptionSelection, is_installed, is_verified, model_path, validate,
};

const DEFAULT_MODEL: &str = "parakeet_unified_en";
const DEFAULT_LANGUAGE: &str = "en";

#[derive(Parser)]
#[command(version, about = "Local Hex dictation for Windows")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Open the native Windows application.
    App {
        /// Start the resident listener without showing the Settings window.
        #[arg(long)]
        hidden: bool,
        /// Arguments for a headless render, such as `--snap out.png --scene modes`.
        #[arg(last = true)]
        rune_args: Vec<String>,
    },
    /// Listen globally: hold the configured shortcut to dictate and release to paste.
    Listen {
        /// Select an input device by a case-insensitive name fragment.
        #[arg(long)]
        device: Option<String>,
        #[command(flatten)]
        selection: SelectionArgs,
    },
    /// Record one microphone capture and transcribe it locally.
    Capture {
        /// Select an input device by a case-insensitive name fragment.
        #[arg(long)]
        device: Option<String>,
        /// Copy a successful transcript to the clipboard.
        #[arg(long)]
        copy: bool,
        #[command(flatten)]
        selection: SelectionArgs,
    },
    /// List microphone devices visible through WASAPI.
    Devices,
    /// Print recent local observations.
    Status {
        /// Maximum number of observations to print.
        #[arg(long, default_value_t = 40)]
        lines: usize,
    },
    /// Install, inspect, and validate a local dictation model.
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
enum ModelCommand {
    /// Show whether a model is installed and checksum-verified.
    Status {
        #[command(flatten)]
        selection: SelectionArgs,
    },
    /// Download and checksum-verify a model.
    Install {
        #[command(flatten)]
        selection: SelectionArgs,
    },
    /// List transcribe.cpp compute devices available to Hex.
    Devices,
    /// Load and prewarm an installed model.
    Check {
        #[command(flatten)]
        selection: SelectionArgs,
    },
    /// Transcribe a WAV file with an installed model.
    Transcribe {
        wav: PathBuf,
        #[command(flatten)]
        selection: SelectionArgs,
    },
    /// Compare live-stream and full-clip transcription of a WAV file.
    #[command(hide = true)]
    StreamBench {
        wav: PathBuf,
        /// Feed audio this many times faster than real time.
        #[arg(long, default_value_t = 1.0)]
        speed: f64,
        #[command(flatten)]
        selection: SelectionArgs,
    },
}

#[derive(Args, Clone)]
struct SelectionArgs {
    /// Model id, for example parakeet_v3 or whisper_large_v3_turbo.
    #[arg(long, default_value = DEFAULT_MODEL)]
    model: String,
    /// BCP-47 language code supported by the model, for example en or pl.
    #[arg(long, default_value = DEFAULT_LANGUAGE)]
    language: String,
}

impl SelectionArgs {
    fn resolve(&self) -> Result<TranscriptionSelection> {
        let selection = TranscriptionSelection {
            model: self
                .model
                .parse::<TranscriptionModelId>()
                .wrap_err_with(|| format!("invalid model id: {}", self.model))?,
            language: self.language.clone(),
            recognition_hints: String::new(),
        };
        validate(&selection)?;
        Ok(selection)
    }
}

pub fn run(shutdown: &'static AtomicBool) -> Result<()> {
    color_eyre::install()?;
    // Parse first so `--version` and `--help` exit without creating app data.
    let command = Cli::parse().command.unwrap_or(Command::App {
        hidden: false,
        rune_args: Vec::new(),
    });
    if let Command::Settings { command } = command {
        crate::control::cli::finish(crate::control::cli::settings(command));
    }
    // Like a read command: no log, lock, or running Hex.
    if let Command::Experiments { command } = &command {
        return crate::experiments::run(command);
    }
    let log_dir = crate::app_paths::init_process_logging(shutdown)?;

    let event_path = log_dir.join("live.ndjson");
    match command {
        Command::App { hidden, rune_args } => {
            // A headless render (`--snap`) draws fixtures without starting
            // the runtime and prints to the terminal that ran it.
            if !rune_args.is_empty() {
                crate::rune_ui::render_app(rune_args).run();
                return Ok(());
            }
            detach_from_console();
            crate::windows_rune::open(event_path, shutdown, hidden)
        }
        Command::Listen { device, selection } => {
            shutdown.store(false, Ordering::Relaxed);
            let _instance = crate::instance::acquire("windows-listener")?;
            crate::experiments::log_launch();
            let settings = crate::app_settings::AppSettings::load()?;
            let history = crate::history::History::open_default(settings.history_retention)?;
            let config = crate::windows_dictation::WindowsDictationConfig {
                device,
                history: Some(history),
                fallback_to_default_device: false,
                ..crate::windows_dictation::WindowsDictationConfig::from_settings(&settings)
            };
            let _personal_commands =
                crate::app_settings::transformations_used(&settings.dictation_processing)
                    .then(|| {
                        crate::personal_commands::PersonalCommands::start(
                            crate::commands::CommandConfig::new(),
                            config.transformations.clone(),
                        )
                    })
                    .flatten();
            // Settings a foreground listener loaded stay as they are.
            crate::windows_dictation::run(
                &event_path,
                &selection.resolve()?,
                config,
                std::sync::mpsc::channel().1,
                shutdown,
            )
        }
        Command::Capture {
            device,
            copy,
            selection,
        } => capture(
            &event_path,
            device.as_deref(),
            copy,
            &selection.resolve()?,
            shutdown,
        ),
        Command::Devices => {
            for device in crate::audio::input_device_names()? {
                println!("{device}");
            }
            Ok(())
        }
        Command::Status { lines } => print_status(&event_path, lines),
        Command::Model { command } => run_model_command(command, shutdown),
        Command::Settings { .. } => unreachable!("handled before logging"),
        Command::Experiments { .. } => unreachable!("handled before logging"),
    }
}

fn detach_from_console() {
    // The binary also exposes terminal-oriented subcommands, so it cannot use a
    // crate-wide Windows GUI subsystem. Detach only for the native app: an
    // Explorer-launched console disappears, while a parent terminal remains.
    // FreeConsole leaves the standard handles holding closed console handle
    // values that Windows soon reuses for unrelated objects, so a later print
    // panics (a read-only file) or writes into whatever file took the value.
    // Null handles make std discard the output; redirected handles are kept.
    unsafe {
        let console_handles: Vec<_> = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE]
            .into_iter()
            .filter(|&id| {
                let mut mode = 0;
                GetConsoleMode(GetStdHandle(id), &mut mode) != 0
            })
            .collect();
        FreeConsole();
        for id in console_handles {
            SetStdHandle(id, std::ptr::null_mut());
        }
    }
}

fn run_model_command(command: ModelCommand, shutdown: &AtomicBool) -> Result<()> {
    match command {
        ModelCommand::Status { selection } => {
            let selection = selection.resolve()?;
            let model = validate(&selection)?;
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
        ModelCommand::Install { selection } => {
            crate::local_transcriber::install(&selection.resolve()?, shutdown)
        }
        ModelCommand::Devices => {
            for device in crate::local_transcriber::devices()? {
                println!("{device}");
            }
            Ok(())
        }
        ModelCommand::Check { selection } => {
            let transcriber = LocalTranscriber::load(&selection.resolve()?)?;
            println!("ready\t{}", transcriber.device_label());
            Ok(())
        }
        ModelCommand::Transcribe { wav, selection } => {
            println!(
                "{}",
                crate::local_transcriber::transcribe_wav_with_selection(
                    &wav,
                    &selection.resolve()?
                )?
            );
            Ok(())
        }
        ModelCommand::StreamBench {
            wav,
            speed,
            selection,
        } => stream_bench(&wav, speed, &selection.resolve()?),
    }
}

/// Feeds the clip to a live stream as capture would, holding back the last
/// 200 ms as the release tail, then measures each path's release latency.
fn stream_bench(wav: &Path, speed: f64, selection: &TranscriptionSelection) -> Result<()> {
    let samples = crate::local_transcriber::read_wav_for_parakeet(wav)?;
    let audio_ms = samples.len() / 16;
    let mut transcriber = LocalTranscriber::load(selection)?;
    // A warm app has already run inputs of this size; exclude first-run setup.
    transcriber.transcribe(&samples)?;
    let started = Instant::now();
    let offline = transcriber.transcribe(&samples)?;
    let offline_ms = started.elapsed().as_millis();

    let engine = crate::transcription_engine::TranscriptionEngine::start(transcriber)?;
    let mut stream = engine
        .begin()
        .ok_or_else(|| color_eyre::eyre::eyre!("this model cannot stream on this device"))?;
    let held = samples.len().saturating_sub(3_200);
    let feeding = Instant::now();
    for (index, block) in samples[..held].chunks(2_560).enumerate() {
        let due = Duration::from_secs_f64(index as f64 * 0.16 / speed.max(0.01));
        std::thread::sleep(due.saturating_sub(feeding.elapsed()));
        stream.feed(block);
    }
    // The release tail arrives 200 ms after the key goes up.
    std::thread::sleep(Duration::from_secs_f64(0.2 / speed.max(0.01)));
    let streamed = engine
        .finish(Some(stream), samples)?
        .wait()
        .map_err(|error| color_eyre::eyre::eyre!(error))?;

    let differences = word_differences(&offline, &streamed.text);
    println!("audio_ms\t{audio_ms}");
    println!("offline_ms\t{offline_ms}");
    println!(
        "release_ms\t{} ({})",
        streamed.finish_ms,
        streamed.source.label()
    );
    println!(
        "word_differences\t{differences} of {}",
        offline.split_whitespace().count()
    );
    println!("offline\t{}", offline.trim());
    println!("stream\t{}", streamed.text);
    Ok(())
}

/// Word-level edit distance, ignoring case and punctuation.
fn word_differences(left: &str, right: &str) -> usize {
    let words = |text: &str| {
        text.split_whitespace()
            .map(|word| {
                word.chars()
                    .filter(|character| character.is_alphanumeric())
                    .flat_map(char::to_lowercase)
                    .collect::<String>()
            })
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>()
    };
    let (left, right) = (words(left), words(right));
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    for (index, left_word) in left.iter().enumerate() {
        let mut current = vec![index + 1];
        for (offset, right_word) in right.iter().enumerate() {
            let substitution = previous[offset] + usize::from(left_word != right_word);
            current.push(
                substitution
                    .min(previous[offset + 1] + 1)
                    .min(current[offset] + 1),
            );
        }
        previous = current;
    }
    previous[right.len()]
}

fn capture(
    event_path: &Path,
    device: Option<&str>,
    copy: bool,
    selection: &TranscriptionSelection,
    shutdown: &AtomicBool,
) -> Result<()> {
    shutdown.store(false, Ordering::Relaxed);
    let _instance = crate::instance::acquire("windows-capture")?;
    let mut transcriber = LocalTranscriber::load(selection)?;
    let input = match device {
        Some(name) => AudioInput::open_matching(name)?,
        None => AudioInput::open_system_default()?,
    };
    let sample_rate = input.sample_rate;
    let device_name = input.device_name.clone();
    let events = EventLog::create(event_path)?;

    wait_for_enter(&format!(
        "Ready on {device_name}. Press Enter to start recording."
    ))?;
    if shutdown.load(Ordering::Relaxed) {
        return Ok(());
    }

    events.emit(&VoiceEvent::SessionStarted {
        timestamp_ms: now_ms(),
    })?;
    emit_state(&events, VoiceState::Dictating, &device_name)?;
    events.dictation(DictationPhase::Started, "")?;

    let stopped = Arc::new(AtomicBool::new(false));
    let reader_stopped = stopped.clone();
    std::thread::Builder::new()
        .name("windows-capture-console".into())
        .spawn(move || {
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            reader_stopped.store(true, Ordering::Release);
        })?;
    println!("Recording. Press Enter to stop; Ctrl-C cancels.");

    let mut samples = Vec::new();
    while !stopped.load(Ordering::Acquire) && !shutdown.load(Ordering::Relaxed) {
        match input.recv_timeout(Duration::from_millis(50)) {
            AudioInputEvent::Chunk { samples: chunk, .. } => samples.extend(chunk),
            AudioInputEvent::Timeout => {}
            AudioInputEvent::StreamFailed(error) => {
                let error = format!("microphone stream stopped: {error}");
                fail_capture(&events, &device_name, &error)?;
                bail!(error);
            }
        }
    }
    drop(input);

    if shutdown.load(Ordering::Relaxed) {
        events.dictation(DictationPhase::Cancelled, "")?;
        emit_state(&events, VoiceState::Stopping, &device_name)?;
        println!("Capture canceled.");
        return Ok(());
    }

    let audio_ms = samples.len() as u64 * 1_000 / u64::from(sample_rate);
    if audio_ms < crate::dictation::MINIMUM_HOLD_DURATION.as_millis() as u64 {
        events.dictation(DictationPhase::Discarded, "")?;
        emit_state(&events, VoiceState::Stopping, &device_name)?;
        println!("Capture shorter than 300 ms; discarded.");
        return Ok(());
    }

    emit_state(&events, VoiceState::Transcribing, &device_name)?;
    events.dictation(DictationPhase::Transcribing, "")?;
    let started = Instant::now();
    let mut samples = crate::dictation::resample_for_parakeet(&samples, sample_rate);
    crate::dictation::pad_for_parakeet(&mut samples);
    let text = match transcriber.transcribe(&samples) {
        Ok(text) => text.trim().to_string(),
        Err(error) => {
            fail_capture(&events, &device_name, &format!("{error:#}"))?;
            return Err(error);
        }
    };
    if text.is_empty() {
        fail_capture(&events, &device_name, "transcription was empty")?;
        bail!("transcription was empty");
    }
    let latency_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    events.emit(&VoiceEvent::Transcript {
        timestamp_ms: now_ms(),
        phase: TranscriptPhase::Completed,
        latency_ms,
        text: text.clone(),
    })?;

    if copy {
        let copy_result = arboard::Clipboard::new()
            .wrap_err("could not open the Windows clipboard")
            .and_then(|mut clipboard| {
                clipboard
                    .set_text(text.clone())
                    .wrap_err("could not copy the transcript")
            });
        if let Err(error) = copy_result {
            fail_capture(&events, &device_name, &format!("{error:#}"))?;
            return Err(error);
        }
    }
    emit_state(&events, VoiceState::Stopping, &device_name)?;
    events.flush()?;
    tracing::info!(
        audio_ms,
        latency_ms,
        copied = copy,
        "completed Windows capture"
    );
    println!("{text}");
    if copy {
        eprintln!("Copied to clipboard.");
    }
    Ok(())
}

fn wait_for_enter(message: &str) -> Result<()> {
    print!("{message}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(())
}

fn emit_state(events: &EventLog, state: VoiceState, device: &str) -> Result<()> {
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state,
        device: device.into(),
    })?;
    Ok(())
}

fn fail_capture(events: &EventLog, device: &str, error: &str) -> Result<()> {
    events.dictation(DictationPhase::Failed(error.into()), "")?;
    emit_state(events, VoiceState::Stopping, device)
}

fn print_status(path: &Path, limit: usize) -> Result<()> {
    // Read from the sources now; the running Hex read them at its launch.
    let experiments = crate::experiments::active_names();
    if !experiments.is_empty() {
        println!("experiments\t{}", experiments.join(","));
    }
    if !path.exists() {
        println!("No observations yet at {}", path.display());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polish_selection_uses_the_multilingual_parakeet_model() {
        let selection = SelectionArgs {
            model: "parakeet_v3".into(),
            language: "pl".into(),
        }
        .resolve()
        .unwrap();

        assert_eq!(selection.model, TranscriptionModelId::ParakeetV3);
        assert_eq!(selection.language, "pl");
    }

    #[test]
    fn word_differences_ignore_case_and_punctuation() {
        assert_eq!(word_differences("Hello, world.", "hello world"), 0);
        assert_eq!(word_differences("a b c", "a x c d"), 2);
        assert_eq!(word_differences("", "one two"), 2);
    }

    #[test]
    fn incompatible_model_and_language_are_rejected() {
        let result = SelectionArgs {
            model: DEFAULT_MODEL.into(),
            language: "pl".into(),
        }
        .resolve();

        assert!(result.is_err());
    }
}

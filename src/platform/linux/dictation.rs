use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::Result;
use color_eyre::eyre::eyre;

use crate::audio::{AudioInput, AudioInputEvent, CaptureInstant};
use crate::command_context::ContextSnapshot;
use crate::commands_engine::{
    Action, ActionExecutor, Command, CommandConfig, ConfiguredCommand, Decision, Mode,
};
use crate::dictation::{DictationCapture, Finish};
use crate::events::{
    CommandOutcome, DictationPhase, EventLog, TranscriptPhase, VoiceEvent, VoiceState, now_ms,
};
use crate::linux_input::{HotkeyEvent, X11HotkeyMonitor};
use crate::linux_paste::X11Paster;
use crate::local_transcriber::LocalTranscriber;

const UPDATE_INTERVAL: Duration = Duration::from_millis(20);

struct Job {
    samples: Vec<f32>,
    audio_ms: u64,
}

pub(crate) type SharedReplacements = Arc<RwLock<crate::text_replacements::ReplacementSet>>;
pub(crate) type SharedModes = Arc<RwLock<LinuxModeRuntime>>;

struct OutputRuntime {
    history: Option<crate::history::History>,
    replacements: SharedReplacements,
    modes: SharedModes,
}

#[derive(Clone, Default)]
pub(crate) struct LinuxModeRuntime {
    default: crate::dictation_processing::PostProcessingSettings,
    modes: Vec<(
        crate::linux_settings::LinuxMode,
        crate::text_replacements::ReplacementSet,
    )>,
}

impl LinuxModeRuntime {
    pub(crate) fn from_settings(settings: &crate::linux_settings::LinuxSettings) -> Self {
        Self {
            default: settings.dictation_post_processing.clone(),
            modes: settings
                .modes
                .iter()
                .cloned()
                .map(|mode| {
                    let corrections =
                        crate::text_replacements::ReplacementSet::new(&mode.corrections);
                    (mode, corrections)
                })
                .collect(),
        }
    }

    fn process(
        &self,
        text: &str,
        context: &ContextSnapshot,
    ) -> crate::dictation_processing::Processed {
        let mode = context.application.as_deref().and_then(|application| {
            self.modes
                .iter()
                .find(|(mode, _)| mode.matches_application(application))
        });
        let profile = mode.map_or_else(
            || {
                crate::dictation_processing::Profile::configured(
                    "Global",
                    crate::text_replacements::ReplacementSet::default(),
                    Vec::new(),
                    &self.default,
                )
            },
            |(mode, corrections)| {
                let name = if mode.name.trim().is_empty() {
                    "Untitled mode"
                } else {
                    &mode.name
                };
                crate::dictation_processing::Profile::configured(
                    name,
                    corrections.clone(),
                    Vec::new(),
                    &mode.post_processing,
                )
            },
        );
        crate::dictation_processing::Profiles::new(profile).process_cancellable(
            text,
            context,
            None,
            &AtomicBool::new(false),
        )
    }
}

/// The opt-in streaming command loop: Moonshine hears idle-time audio,
/// the shared engine resolves wake, sleep, and command phrases, and the
/// X11 executor performs keystrokes. Dictation-start commands hand
/// control back to the listener's ordinary capture path.
const COMMAND_AUDIO_SLICE_MS: usize = 100;
const COMMAND_AUDIO_QUEUE_SLICES: usize = 10;

struct CommandAudio {
    generation: u64,
    samples: Vec<f32>,
    sample_rate: u32,
}

enum CommandRecognition {
    Ready,
    Failed(String),
    Woke {
        generation: u64,
        heard: String,
    },
    Slept {
        generation: u64,
        heard: String,
    },
    StartDictation {
        generation: u64,
        heard: String,
        id: String,
    },
    Execute {
        generation: u64,
        heard: String,
        commands: Vec<(String, Action)>,
    },
}

struct CommandRuntime {
    audio: SyncSender<CommandAudio>,
    recognition: mpsc::Receiver<CommandRecognition>,
    executor: ActionExecutor,
    generation: u64,
    pressure_invalidated: bool,
}

impl CommandRuntime {
    fn start() -> Result<Self> {
        let x11 = crate::linux_command_executor::X11CommandExecutor::new()?;
        let project_root = Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
        let (audio, audio_receiver) = mpsc::sync_channel(COMMAND_AUDIO_QUEUE_SLICES);
        let (recognition_sender, recognition) = mpsc::channel();
        thread::Builder::new()
            .name("linux-command-recognition".into())
            .spawn(move || {
                command_recognition_worker(project_root, audio_receiver, recognition_sender)
            })?;
        Ok(Self {
            audio,
            recognition,
            executor: ActionExecutor::start_with(move |action| match action {
                Action::Keystroke { key, modifiers } => x11.keystroke(key, modifiers),
                Action::RepeatedKeystroke {
                    key,
                    modifiers,
                    count,
                } => x11.repeated_keystroke(key, modifiers, count),
                other => crate::commands_engine::execute(other),
            }),
            generation: 0,
            pressure_invalidated: false,
        })
    }

    /// Drop any partial phrase; dictation audio must not leak into the
    /// command stream.
    fn reset(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pressure_invalidated = false;
    }

    /// Project at most one second of idle audio to the command worker.
    /// Pressure invalidates that recognition generation without touching
    /// authoritative dictation capture.
    fn ingest(&mut self, samples: &[f32], sample_rate: u32, events: &mut EventLog) -> Result<bool> {
        let mut start_dictation = self.drain(events)?;
        let slice_samples = ((sample_rate as usize * COMMAND_AUDIO_SLICE_MS) / 1_000).max(1);
        for slice in samples.chunks(slice_samples) {
            let message = CommandAudio {
                generation: self.generation,
                samples: slice.to_vec(),
                sample_rate,
            };
            match self.audio.try_send(message) {
                Ok(()) => self.pressure_invalidated = false,
                Err(TrySendError::Full(_)) => {
                    if !self.pressure_invalidated {
                        self.generation = self.generation.wrapping_add(1);
                        self.pressure_invalidated = true;
                        tracing::warn!(
                            "command audio pressure invalidated the recognition generation"
                        );
                    }
                    break;
                }
                Err(TrySendError::Disconnected(_)) => {
                    return Err(eyre!("voice command recognizer is unavailable"));
                }
            }
        }
        start_dictation |= self.drain(events)?;
        Ok(start_dictation)
    }

    fn drain(&self, events: &mut EventLog) -> Result<bool> {
        let mut start_dictation = false;
        while let Ok(update) = self.recognition.try_recv() {
            match update {
                CommandRecognition::Ready => {
                    tracing::info!("Linux voice command model loaded");
                }
                CommandRecognition::Failed(error) => return Err(eyre!(error)),
                CommandRecognition::Woke { generation, heard } if generation == self.generation => {
                    log_command(events, &heard, None, CommandOutcome::Woke)?;
                }
                CommandRecognition::Slept { generation, heard }
                    if generation == self.generation =>
                {
                    log_command(events, &heard, None, CommandOutcome::Slept)?;
                }
                CommandRecognition::StartDictation {
                    generation,
                    heard,
                    id,
                } if generation == self.generation => {
                    log_command(events, &heard, Some(&id), CommandOutcome::Executed)?;
                    start_dictation = true;
                }
                CommandRecognition::Execute {
                    generation,
                    heard,
                    commands,
                } if generation == self.generation => {
                    let ids: Vec<String> = commands.iter().map(|(id, _)| id.clone()).collect();
                    if let Err(error) =
                        self.executor
                            .submit_sequence(commands, &heard, String::new())
                    {
                        tracing::warn!(%error, "voice command queue rejected an action");
                        for id in ids {
                            log_command(
                                events,
                                &heard,
                                Some(&id),
                                CommandOutcome::Failed(error.into()),
                            )?;
                        }
                    }
                }
                _ => {}
            }
        }
        while let Some(outcome) = self.executor.try_recv() {
            match outcome.result {
                Ok(()) => log_command(
                    events,
                    &outcome.heard,
                    Some(&outcome.id),
                    CommandOutcome::Executed,
                )?,
                Err(error) => {
                    tracing::warn!(%error, command = %outcome.id, "voice command failed");
                    log_command(
                        events,
                        &outcome.heard,
                        Some(&outcome.id),
                        CommandOutcome::Failed(error),
                    )?;
                }
            }
        }
        Ok(start_dictation)
    }
}

fn command_recognition_worker(
    project_root: std::path::PathBuf,
    audio: mpsc::Receiver<CommandAudio>,
    updates: mpsc::Sender<CommandRecognition>,
) {
    let mut moonshine = match crate::moonshine::Moonshine::load(&project_root) {
        Ok(moonshine) => moonshine,
        Err(error) => {
            let _ = updates.send(CommandRecognition::Failed(format!("{error:#}")));
            return;
        }
    };
    if updates.send(CommandRecognition::Ready).is_err() {
        return;
    }
    let config = command_config();
    let mut mode = Mode::Sleeping;
    let mut generation = None;
    let mut last_completed_line = None;
    while let Ok(chunk) = audio.recv() {
        if generation != Some(chunk.generation) {
            if let Err(error) = moonshine.reset_stream() {
                let _ = updates.send(CommandRecognition::Failed(format!("{error:#}")));
                return;
            }
            generation = Some(chunk.generation);
            last_completed_line = None;
        }
        if let Err(error) = moonshine.add_audio(&chunk.samples, chunk.sample_rate) {
            let _ = updates.send(CommandRecognition::Failed(format!("{error:#}")));
            return;
        }
        let recognized = match moonshine.update() {
            Ok(recognized) => recognized,
            Err(error) => {
                let _ = updates.send(CommandRecognition::Failed(format!("{error:#}")));
                return;
            }
        };
        for update in recognized {
            if update.phase != TranscriptPhase::Completed
                || last_completed_line.is_some_and(|line| update.line_id <= line)
            {
                continue;
            }
            last_completed_line = Some(update.line_id);
            let heard = update.text.trim().to_string();
            if heard.is_empty() {
                continue;
            }
            let message = match config.resolve(mode, &heard, &ContextSnapshot::empty()) {
                Decision::Ignore => continue,
                Decision::Wake => {
                    mode = Mode::Listening;
                    CommandRecognition::Woke {
                        generation: chunk.generation,
                        heard,
                    }
                }
                Decision::Sleep => {
                    mode = Mode::Sleeping;
                    CommandRecognition::Slept {
                        generation: chunk.generation,
                        heard,
                    }
                }
                Decision::Execute {
                    id,
                    action: Action::StartDictation,
                } => CommandRecognition::StartDictation {
                    generation: chunk.generation,
                    heard,
                    id: id.to_string(),
                },
                Decision::Execute { id, action } => CommandRecognition::Execute {
                    generation: chunk.generation,
                    heard,
                    commands: vec![(id.to_string(), action)],
                },
                Decision::ExecuteSequence { commands } => CommandRecognition::Execute {
                    generation: chunk.generation,
                    heard,
                    commands: commands
                        .into_iter()
                        .map(|command| (command.id.to_string(), command.action))
                        .collect(),
                },
            };
            if updates.send(message).is_err() {
                return;
            }
        }
    }
}

fn log_command(
    events: &mut EventLog,
    heard: &str,
    command: Option<&str>,
    outcome: CommandOutcome,
) -> Result<()> {
    events.emit(&VoiceEvent::Command {
        timestamp_ms: now_ms(),
        heard: heard.into(),
        command: command.map(str::to_string),
        outcome,
        context: String::new(),
    })?;
    Ok(())
}

/// The built-in Linux command set, mirroring the macOS taxonomy minus
/// Meetings; TypeScript personal commands arrive with a later slice.
fn command_config() -> CommandConfig {
    use crate::commands_engine::Digit;
    use crate::keys::{Key, Modifiers};

    CommandConfig::new()
        .wake_with(["voice control", "wake up", "start voice control"])
        .sleep_with(["go to sleep", "stop voice control"])
        .command(
            Command::new("dictation.start", "Start dictation")
                .phrases(["start dictation", "take dictation", "start typing"])
                .protected()
                .action(|()| Action::StartDictation),
        )
        .command(
            Command::new("shortcut.command-number", "Use keyboard shortcut")
                .spoken(("command", Digit))
                .spoken(("key", "command", Digit))
                .spoken(("terminal", Digit))
                .spoken(("tab", Digit))
                .action(|digit| Action::Keystroke {
                    key: Key::Character(digit.as_char()),
                    modifiers: Modifiers::COMMAND,
                }),
        )
        .command(directional_command(
            "edit.move",
            "Move the cursor",
            "go",
            Modifiers::NONE,
        ))
        .command(directional_command(
            "edit.select",
            "Extend the selection",
            "select",
            Modifiers::SHIFT,
        ))
}

fn directional_command(
    id: &'static str,
    description: &'static str,
    verb: &'static str,
    modifiers: crate::keys::Modifiers,
) -> ConfiguredCommand {
    use crate::commands_engine::{Count, Direction};

    Command::new(id, description)
        .spoken((verb, Direction, Count.optional()))
        .action(move |(direction, count)| Action::RepeatedKeystroke {
            key: direction.key(),
            modifiers,
            count: count.map_or(1, |count| count.get()),
        })
}

struct JobResult {
    text: Option<String>,
    result: Result<(), String>,
}

pub(crate) fn run(event_path: &Path, device: Option<&str>, shutdown: &AtomicBool) -> Result<()> {
    let settings = crate::linux_settings::LinuxSettings::load()?;
    let transcriber = LocalTranscriber::load(&settings.transcription)?;
    let history = open_history(&settings);
    let replacements = Arc::new(RwLock::new(crate::text_replacements::ReplacementSet::new(
        &settings.text_replacements,
    )));
    let modes = Arc::new(RwLock::new(LinuxModeRuntime::from_settings(&settings)));
    run_with_settings(
        event_path,
        device,
        shutdown,
        settings,
        transcriber,
        OutputRuntime {
            history,
            replacements,
            modes,
        },
    )
}

pub(crate) fn run_with_history(
    event_path: &Path,
    device: Option<&str>,
    shutdown: &AtomicBool,
    history: Option<crate::history::History>,
    replacements: SharedReplacements,
    modes: SharedModes,
) -> Result<()> {
    let settings = crate::linux_settings::LinuxSettings::load()?;
    let transcriber = LocalTranscriber::load(&settings.transcription)?;
    run_with_settings(
        event_path,
        device,
        shutdown,
        settings,
        transcriber,
        OutputRuntime {
            history,
            replacements,
            modes,
        },
    )
}

pub(crate) fn run_with_transcriber(
    event_path: &Path,
    device: Option<&str>,
    shutdown: &AtomicBool,
    transcriber: LocalTranscriber,
    history: Option<crate::history::History>,
    replacements: SharedReplacements,
    modes: SharedModes,
) -> Result<()> {
    let settings = crate::linux_settings::LinuxSettings::load()?;
    run_with_settings(
        event_path,
        device,
        shutdown,
        settings,
        transcriber,
        OutputRuntime {
            history,
            replacements,
            modes,
        },
    )
}

fn run_with_settings(
    event_path: &Path,
    device: Option<&str>,
    shutdown: &AtomicBool,
    settings: crate::linux_settings::LinuxSettings,
    transcriber: LocalTranscriber,
    output: OutputRuntime,
) -> Result<()> {
    // A CLI device override stays a strict substring query; the settings
    // choice is an exact enumerated name and falls back to automatic
    // selection when the device is missing, like other platforms.
    let input = match (device, settings.microphone.as_deref()) {
        (Some(name), _) => AudioInput::open_matching(name)?,
        (None, Some(name)) => match AudioInput::open_named(name) {
            Ok(input) => input,
            Err(error) => {
                tracing::warn!(
                    %error,
                    device = name,
                    "selected microphone is unavailable; using automatic selection"
                );
                AudioInput::open(&[])?
            }
        },
        (None, None) => AudioInput::open(&[])?,
    };
    let hotkey_label = settings.dictation_hotkey.label();
    let hotkey = X11HotkeyMonitor::start(settings.dictation_hotkey, settings.double_tap_lock)?;
    let mut commands = if crate::DEVELOPER_FEATURES_ENABLED && settings.commands_enabled {
        match CommandRuntime::start() {
            Ok(runtime) => {
                println!("Voice commands are loading; dictation is already ready.");
                Some(runtime)
            }
            Err(error) => {
                tracing::warn!(%error, "voice commands are unavailable");
                None
            }
        }
    } else {
        None
    };
    let (jobs, job_receiver) = mpsc::sync_channel::<Job>(2);
    let (result_sender, results) = mpsc::channel();
    let OutputRuntime {
        history,
        replacements,
        modes,
    } = output;
    let worker = thread::spawn(move || {
        let mut transcriber = transcriber;
        let mut paster = X11Paster::new();
        let context_reader = match crate::linux_context::LinuxContext::new() {
            Ok(reader) => Some(reader),
            Err(error) => {
                tracing::warn!(%error, "Linux foreground context is unavailable");
                None
            }
        };
        while let Ok(job) = job_receiver.recv() {
            let started = Instant::now();
            let context = context_reader
                .as_ref()
                .and_then(|reader| match reader.capture() {
                    Ok(context) => Some(context),
                    Err(error) => {
                        tracing::debug!(%error, "could not capture Linux foreground context");
                        None
                    }
                })
                .unwrap_or_default();
            let inference_started = Instant::now();
            let transcription = transcriber
                .transcribe(&job.samples)
                .map(|text| text.trim().to_string())
                .map_err(|error| format!("{error:#}"));
            let inference_ms = inference_started.elapsed().as_millis() as u64;
            let result = transcription.and_then(|raw_text| {
                if raw_text.is_empty() {
                    return Err("transcription was empty".into());
                }
                let crate::dictation_processing::Processed {
                    text: final_text,
                    observation,
                } = process_text(&raw_text, &context, &replacements, &modes)?;
                let paste_result = paster
                    .as_mut()
                    .map_err(|error| format!("{error:#}"))?
                    .paste(&final_text)
                    .map_err(|error| format!("{error:#}"));
                paste_result?;
                retain_successful_dictation(
                    history.as_ref(),
                    RetainedDictation {
                        raw_text: &raw_text,
                        final_text: &final_text,
                        application: context.application.as_deref(),
                        processing: observation,
                        audio_ms: job.audio_ms,
                        inference_ms,
                        total_ms: started.elapsed().as_millis() as u64,
                    },
                );
                Ok(final_text)
            });
            tracing::info!(
                audio_ms = job.audio_ms,
                elapsed_ms = started.elapsed().as_millis(),
                "completed Linux dictation job"
            );
            let event = match result {
                Ok(text) => JobResult {
                    text: Some(text),
                    result: Ok(()),
                },
                Err(error) => JobResult {
                    text: None,
                    result: Err(error),
                },
            };
            if result_sender.send(event).is_err() {
                break;
            }
        }
    });

    let mut events = EventLog::create(event_path)?;
    let mut capture = DictationCapture::new(input.sample_rate);
    let mut recording = false;
    let mut captured_through = CaptureInstant::ZERO;
    let mut pending = 0_usize;
    events.emit(&VoiceEvent::SessionStarted {
        timestamp_ms: now_ms(),
    })?;
    emit_state(&mut events, VoiceState::Listening, &input.device_name)?;
    println!(
        "HEX dictation is ready on {}. Hold {} to dictate; Ctrl-C stops.",
        input.device_name, hotkey_label
    );

    while !shutdown.load(Ordering::Relaxed) {
        if let Ok(error) = hotkey.errors.try_recv() {
            return Err(eyre!("X11 hotkey monitor stopped: {error}"));
        }
        while let Ok(action) = hotkey.events.try_recv() {
            match action {
                HotkeyEvent::Start if !recording => {
                    if let Some(runtime) = &mut commands {
                        runtime.reset();
                    }
                    capture.start(captured_through);
                    recording = true;
                    events.dictation(DictationPhase::Started, "")?;
                    emit_state(&mut events, VoiceState::Dictating, &input.device_name)?;
                }
                HotkeyEvent::Finish if recording => {
                    recording = false;
                    submit_capture(
                        &mut capture,
                        captured_through,
                        &jobs,
                        &mut events,
                        &mut pending,
                    )?;
                    emit_state(
                        &mut events,
                        if pending > 0 {
                            VoiceState::Transcribing
                        } else {
                            VoiceState::Listening
                        },
                        &input.device_name,
                    )?;
                }
                HotkeyEvent::Cancel if recording => {
                    capture.cancel();
                    recording = false;
                    events.dictation(DictationPhase::Cancelled, "")?;
                    emit_state(&mut events, VoiceState::Listening, &input.device_name)?;
                }
                _ => {}
            }
        }
        while let Ok(result) = results.try_recv() {
            pending = pending.saturating_sub(1);
            match result.result {
                Ok(()) => {
                    let text = result.text.unwrap_or_default();
                    events.emit(&VoiceEvent::Transcript {
                        timestamp_ms: now_ms(),
                        phase: TranscriptPhase::Completed,
                        latency_ms: 0,
                        text: text.clone(),
                    })?;
                    events.dictation(DictationPhase::Pasted, text)?;
                }
                Err(error) => events.dictation(DictationPhase::Failed(error), "")?,
            }
            emit_state(
                &mut events,
                if recording {
                    VoiceState::Dictating
                } else if pending > 0 {
                    VoiceState::Transcribing
                } else {
                    VoiceState::Listening
                },
                &input.device_name,
            )?;
        }

        let chunk = match input.recv_timeout(UPDATE_INTERVAL) {
            AudioInputEvent::Chunk {
                samples,
                captured_through: chunk_captured_through,
            } => {
                captured_through = chunk_captured_through;
                samples
            }
            AudioInputEvent::Timeout => continue,
            AudioInputEvent::StreamFailed(error) => {
                return Err(eyre!("microphone stream stopped: {error}"));
            }
        };
        if recording {
            capture.ingest(&chunk, captured_through);
            capture.become_intentional(captured_through);
        } else {
            capture.keep_warm(&chunk);
            let mut disable_commands = false;
            let mut start_dictation = false;
            if let Some(runtime) = &mut commands {
                match runtime.ingest(&chunk, input.sample_rate, &mut events) {
                    Ok(requested) => start_dictation = requested,
                    Err(error) => {
                        tracing::warn!(%error, "voice commands stopped");
                        disable_commands = true;
                    }
                }
            }
            if disable_commands {
                commands = None;
            }
            if start_dictation {
                if let Some(runtime) = &mut commands {
                    runtime.reset();
                }
                capture.start_voice_at(captured_through);
                recording = true;
                events.dictation(DictationPhase::Started, "")?;
                emit_state(&mut events, VoiceState::Dictating, &input.device_name)?;
            }
        }
    }

    emit_state(&mut events, VoiceState::Stopping, &input.device_name)?;
    drop(jobs);
    if worker.join().is_err() {
        return Err(eyre!("Linux dictation worker panicked"));
    }
    println!("Stopped.");
    Ok(())
}

fn open_history(
    settings: &crate::linux_settings::LinuxSettings,
) -> Option<crate::history::History> {
    match crate::history::History::open_default(settings.history_retention) {
        Ok(history) => Some(history),
        Err(error) => {
            tracing::warn!(%error, "Linux retained history is unavailable");
            None
        }
    }
}

struct RetainedDictation<'a> {
    raw_text: &'a str,
    final_text: &'a str,
    application: Option<&'a str>,
    processing: Option<crate::dictation_processing::ProcessingObservation>,
    audio_ms: u64,
    inference_ms: u64,
    total_ms: u64,
}

fn retain_successful_dictation(
    history: Option<&crate::history::History>,
    dictation: RetainedDictation<'_>,
) {
    let Some(history) = history else {
        return;
    };
    let draft = crate::history::HistoryDraft {
        kind: crate::history::HistoryKind::Dictation,
        raw_text: dictation.raw_text.to_string(),
        final_text: dictation.final_text.to_string(),
        application: dictation.application.map(str::to_string),
        processing: dictation
            .processing
            .map(|processing| crate::history::HistoryProcessing {
                profile: processing.profile,
                latency_ms: processing.latency_ms,
                fallback: processing.fallback,
            }),
        audio_ms: dictation.audio_ms,
        inference_ms: dictation.inference_ms,
        total_ms: dictation.total_ms,
    };
    if let Err(error) = history.record(draft) {
        tracing::warn!(%error, "could not retain Linux dictation history");
    }
}

fn process_text(
    raw_text: &str,
    context: &ContextSnapshot,
    replacements: &RwLock<crate::text_replacements::ReplacementSet>,
    modes: &RwLock<LinuxModeRuntime>,
) -> std::result::Result<crate::dictation_processing::Processed, String> {
    let text = replacements
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .replace(raw_text);
    let modes = modes
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    let processed = modes.process(&text, context);
    if processed.text.trim().is_empty() {
        return Err("text replacements produced empty output".into());
    }
    Ok(processed)
}

fn submit_capture(
    capture: &mut DictationCapture,
    ended_at: CaptureInstant,
    jobs: &SyncSender<Job>,
    events: &mut EventLog,
    pending: &mut usize,
) -> Result<()> {
    match capture.finish(ended_at) {
        Finish::Discard => events.dictation(DictationPhase::Discarded, "")?,
        Finish::Transcribe(clip) => {
            let audio_ms = clip.duration_ms();
            let job = Job {
                samples: clip.into_parakeet_samples(),
                audio_ms,
            };
            match jobs.try_send(job) {
                Ok(()) => {
                    *pending += 1;
                    events.dictation(DictationPhase::Transcribing, "")?;
                }
                Err(TrySendError::Full(_)) => events
                    .dictation(DictationPhase::Failed("dictation queue is full".into()), "")?,
                Err(TrySendError::Disconnected(_)) => {
                    return Err(eyre!("dictation worker is unavailable"));
                }
            }
        }
    }
    Ok(())
}

fn emit_state(events: &mut EventLog, state: VoiceState, device: &str) -> Result<()> {
    events.emit(&VoiceEvent::State {
        timestamp_ms: now_ms(),
        state,
        device: device.into(),
    })?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::fs;
    use std::sync::RwLock;

    use crate::history::{History, HistoryRetention, HistoryStore};

    use super::{LinuxModeRuntime, RetainedDictation, process_text, retain_successful_dictation};

    #[test]
    fn successful_paste_retains_only_text_and_bounded_metadata() {
        let directory =
            std::env::temp_dir().join(format!("hex-linux-history-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let history = History::new(HistoryStore::open(
            directory.join("history.json"),
            HistoryRetention::Week,
            0,
        ));

        retain_successful_dictation(
            Some(&history),
            RetainedDictation {
                raw_text: "hello open code",
                final_text: "hello OpenCode",
                application: Some("firefox"),
                processing: Some(crate::dictation_processing::ProcessingObservation {
                    profile: "Browser".into(),
                    latency_ms: 85,
                    fallback: Some("processor timed out".into()),
                }),
                audio_ms: 1_250,
                inference_ms: 310,
                total_ms: 420,
            },
        );

        let entries = history.search("");
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.raw_text, "hello open code");
        assert_eq!(entry.final_text, "hello OpenCode");
        assert_eq!(entry.application.as_deref(), Some("firefox"));
        assert_eq!(
            entry.processing,
            Some(crate::history::HistoryProcessing {
                profile: "Browser".into(),
                latency_ms: 85,
                fallback: Some("processor timed out".into()),
            })
        );
        assert_eq!(entry.audio_ms, 1_250);
        assert_eq!(entry.inference_ms, 310);
        assert_eq!(entry.total_ms, 420);

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn foreground_application_selects_live_mode_corrections_after_global_replacements() {
        let replacements = RwLock::new(crate::text_replacements::ReplacementSet::new(&[
            crate::text_replacements::TextReplacement {
                matched_phrase: "open code".into(),
                output: "OpenCode".into(),
            },
        ]));
        let mut settings = crate::linux_settings::LinuxSettings {
            modes: vec![crate::linux_settings::LinuxMode {
                name: "Browser".into(),
                applications: vec!["firefox".into()],
                corrections: vec![crate::text_replacements::TextReplacement {
                    matched_phrase: "full stop".into(),
                    output: ".".into(),
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let modes = RwLock::new(LinuxModeRuntime::from_settings(&settings));
        let firefox = crate::command_context::ContextSnapshot {
            application: Some("Firefox".into()),
            ..Default::default()
        };
        let code = crate::command_context::ContextSnapshot {
            application: Some("code".into()),
            ..Default::default()
        };

        assert_eq!(
            process_text("open code full stop", &firefox, &replacements, &modes)
                .unwrap()
                .text,
            "OpenCode ."
        );
        assert_eq!(
            process_text("open code full stop", &code, &replacements, &modes)
                .unwrap()
                .text,
            "OpenCode full stop"
        );
        settings.modes = vec![crate::linux_settings::LinuxMode {
            name: "Editor".into(),
            applications: vec!["code".into()],
            corrections: vec![crate::text_replacements::TextReplacement {
                matched_phrase: "full stop".into(),
                output: ".".into(),
            }],
            ..Default::default()
        }];
        *modes.write().unwrap() = LinuxModeRuntime::from_settings(&settings);
        assert_eq!(
            process_text("open code full stop", &code, &replacements, &modes)
                .unwrap()
                .text,
            "OpenCode ."
        );
    }
}

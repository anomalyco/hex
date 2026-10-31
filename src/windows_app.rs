//! The Windows listener host, which outlives the window while Hex hides in
//! the tray, and the single-instance claim for the login session. The tray
//! and the window are `windows_rune`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use color_eyre::Result;
use color_eyre::eyre::eyre;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, SetEvent, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};

use crate::app_settings::AppSettings;
use crate::desktop_activity::DesktopActivity;
use crate::desktop_host::{
    DesktopAction, DesktopListenerSnapshot, DesktopShortcuts, DesktopTranscriptionSnapshot,
};
use crate::desktop_i18n::tr_fill;
use crate::events::{EventReader, TranscriptPhase, VoiceEvent};
use crate::history::History;
use crate::transcription_models::{ModelPreparationStage, TranscriptionModelId};
use crate::transcription_preparation::TranscriptionPreparation;
use crate::windows_rune::{Published, Request};

type ListenerResult = std::result::Result<(), String>;

struct RunningListener {
    stop: Arc<AtomicBool>,
    result: Receiver<ListenerResult>,
    worker: JoinHandle<()>,
    /// Saved settings the listener applies in place.
    updates: mpsc::Sender<Box<AppSettings>>,
    swap_failures: Receiver<crate::local_transcriber::SwapFailed>,
}

impl RunningListener {
    fn stop_and_join(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.worker.join();
    }
}

/// The Windows runtime: owns the listener, model preparation, OpenCode
/// catalog, personal transformations, and self-updater.
pub(crate) struct WindowsDesktopHost {
    event_path: PathBuf,
    event_reader: EventReader,
    activity: DesktopActivity,
    /// When the current session's newest dictation completed.
    last_completed_at: Option<u64>,
    listener: Option<RunningListener>,
    session_before_start: Option<u64>,
    awaiting_session_start: bool,
    listen_when_ready: bool,
    /// Enable on launch waits for first-run setup.
    listen_after_setup: bool,
    status: String,
    error: Option<String>,
    settings: AppSettings,
    history: Option<History>,
    last_dictation: Arc<Mutex<Option<String>>>,
    indicator: crate::rune_hud::HudSender,
    mode_runtime: Arc<RwLock<crate::dictation_modes::ModeRuntime>>,
    transformations: Arc<crate::personal_commands::TransformationClient>,
    personal_commands: Option<crate::personal_commands::PersonalCommands>,
    personal_commands_checked_at: Instant,
    preparation: TranscriptionPreparation,
    keyboard_switcher: crate::keyboard_model_switching::Switcher,
    /// Present only while keyboard model switching is on.
    input_sources: Option<crate::keyboard_input_sources::Watcher>,
    /// The preparation running is a keyboard switch, which a manual choice
    /// or a later source change supersedes.
    automatic_preparation: bool,
    updater: crate::updater::Updater,
}

/// Names the event that marks the running Hex app in this login session.
const APP_INSTANCE_EVENT: &str = r"Local\anomaly.hex.app";

/// Ownership of the one Hex app in this login session.
///
/// Windows does not bring a running app forward when it is launched again, so
/// a Start menu launch while the login item runs in the tray would start a
/// second app whose listener cannot take `windows-listener.lock`. The first
/// app process creates a named auto-reset event and keeps it for its
/// lifetime; a later launch signals it to show the window and exits.
pub(crate) struct AppInstance(HANDLE);

impl AppInstance {
    /// Returns `None` when a Hex app already runs in this session, after
    /// asking it to show its window unless this launch is a hidden (login)
    /// launch, which must never pop up an app that is already running.
    pub(crate) fn claim(start_hidden: bool) -> Result<Option<Self>> {
        let name: Vec<u16> = APP_INSTANCE_EVENT.encode_utf16().chain([0]).collect();
        // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the call.
        let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
        if event.is_null() {
            return Err(eyre!(
                "could not create the Hex instance event: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: read immediately after the call whose result it describes.
        if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
            return Ok(Some(Self(event)));
        }
        if !start_hidden {
            tracing::info!("Hex is already running; showing its window");
            // SAFETY: `event` is a valid handle owned by this function. A
            // launched app may take the foreground; pass that right on so
            // the running app can activate its window.
            unsafe {
                AllowSetForegroundWindow(ASFW_ANY);
                SetEvent(event);
            }
        }
        // SAFETY: `event` is a valid handle owned by this function.
        unsafe { CloseHandle(event) };
        Ok(None)
    }

    /// Calls `on_launch` whenever a later launch signals the event, until it
    /// returns false. The handle stays open until the process exits, keeping
    /// this process the session's Hex app.
    pub(crate) fn forward_launches(self, on_launch: impl Fn() -> bool + Send + 'static) {
        let event = self.0 as usize;
        let spawned = std::thread::Builder::new()
            .name("hex-relaunch".into())
            .spawn(move || {
                // SAFETY: the handle is never closed while the process runs.
                while unsafe { WaitForSingleObject(event as HANDLE, INFINITE) } == WAIT_OBJECT_0 {
                    if !on_launch() {
                        return;
                    }
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "relaunching Hex will not show the running window");
        }
    }
}

/// Names the event `hex onboarding reset` signals to replay the first run in
/// the running Hex.
const ONBOARDING_EVENT: &str = r"Local\anomaly.hex.onboarding";

/// Opens the session's event called `name`; also says whether it existed.
fn named_event(name: &str) -> Result<(HANDLE, bool)> {
    let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
    // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the call.
    let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
    if event.is_null() {
        return Err(eyre!(
            "could not create a Hex event: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: read immediately after the call whose result it describes.
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    Ok((event, existed))
}

/// `hex onboarding reset`: asks the running Hex to replay its first run.
/// Returns false when no Hex app runs in this session.
pub(crate) fn request_onboarding() -> Result<bool> {
    let (event, existed) = named_event(ONBOARDING_EVENT)?;
    if existed {
        // SAFETY: `event` is a valid handle owned by this function; the
        // running app may bring its window forward.
        unsafe {
            AllowSetForegroundWindow(ASFW_ANY);
            SetEvent(event);
        }
    }
    // SAFETY: `event` is a valid handle owned by this function.
    unsafe { CloseHandle(event) };
    Ok(existed)
}

/// Calls `on_request` for each `hex onboarding reset` while Hex runs.
pub(crate) fn serve_onboarding_requests(on_request: impl Fn() + Send + 'static) -> Result<()> {
    let (event, _) = named_event(ONBOARDING_EVENT)?;
    let event = event as usize;
    std::thread::Builder::new()
        .name("hex-onboarding".into())
        .spawn(move || {
            // SAFETY: the handle is never closed while the process runs.
            while unsafe { WaitForSingleObject(event as HANDLE, INFINITE) } == WAIT_OBJECT_0 {
                on_request();
            }
        })?;
    Ok(())
}

impl WindowsDesktopHost {
    pub(crate) fn new(
        event_path: PathBuf,
        settings: AppSettings,
        settings_error: Option<String>,
        history: Option<History>,
        indicator: crate::rune_hud::HudSender,
    ) -> Self {
        crate::desktop_i18n::apply(settings.ui_language.as_deref());
        let mut event_reader = EventReader::open(&event_path);
        let mut activity = DesktopActivity::default();
        activity.refresh(&mut event_reader);
        let last_completed_at = last_completed_at(event_reader.events().iter());
        let mode_runtime = Arc::new(RwLock::new(
            crate::dictation_modes::ModeRuntime::from_settings(&settings),
        ));
        // Known before the first poll, so launching keeps the saved model
        // instead of applying the current layout's mapping.
        let keyboard_switcher = crate::keyboard_model_switching::Switcher::new(
            settings.keyboard_model_switching.clone(),
            settings
                .keyboard_model_switching
                .enabled
                .then(crate::keyboard_input_sources::current)
                .flatten(),
        );
        let mut host = Self {
            event_path,
            event_reader,
            activity,
            last_completed_at,
            listener: None,
            session_before_start: None,
            awaiting_session_start: false,
            listen_when_ready: false,
            listen_after_setup: false,
            status: "Disabled".into(),
            error: settings_error,
            settings,
            history,
            last_dictation: Arc::new(Mutex::new(None)),
            indicator,
            mode_runtime,
            transformations: Arc::new(crate::personal_commands::TransformationClient::default()),
            personal_commands: None,
            personal_commands_checked_at: Instant::now(),
            preparation: TranscriptionPreparation::default(),
            keyboard_switcher,
            input_sources: None,
            automatic_preparation: false,
            updater: crate::updater::Updater::start(),
        };
        crate::transcription_models::delete_set_aside();
        host.refresh_personal_commands_host();
        // The microphone stays cold until first-run setup finishes.
        if host.settings.enable_on_launch {
            if crate::onboarding::completion_recorded() {
                host.start();
            } else {
                host.listen_after_setup = true;
            }
        }
        host
    }

    /// Handles one window or tray request; returns `true` when Hex should quit.
    pub(crate) fn handle(&mut self, request: Request) -> bool {
        match request {
            Request::Desktop(action) => return self.dispatch(action),
            Request::ToggleListening => {
                if self.is_running() {
                    self.stop();
                } else {
                    self.start();
                }
            }
            Request::ChooseModel { model, language } => self.choose_model(model, language),
            Request::CancelModel => {
                self.manual_choice();
                self.preparation.cancel();
            }
            Request::Quit => return true,
            Request::ReloadSettings => match AppSettings::load() {
                Ok(settings) => self.apply_settings(&settings),
                Err(error) => tracing::warn!(%error, "could not reload settings"),
            },
            Request::CaptureShortcutFor(_)
            | Request::CancelShortcut
            | Request::SetShortcut { .. } => {}
        }
        false
    }

    fn dispatch(&mut self, action: DesktopAction) -> bool {
        match action {
            DesktopAction::StartListening => self.start(),
            DesktopAction::StopListening => self.stop(),
            DesktopAction::ClearError => self.error = None,
            DesktopAction::CheckForUpdates
            | DesktopAction::OpenUpdate
            | DesktopAction::RestartIntoUpdate
            | DesktopAction::ConfirmUpdateSwitch
            | DesktopAction::WindowFocused => {
                if let Err(error) = self.updater.act(&action) {
                    self.error = Some(tr_fill("Could not restart Hex: {}", &format!("{error:#}")));
                }
            }
            // Windows saves shortcuts from the window.
            DesktopAction::SetDictationShortcut(_) | DesktopAction::ConfigureShortcuts => {}
        }
        false
    }

    fn choose_model(&mut self, model: TranscriptionModelId, language: String) {
        self.manual_choice();
        let definition = crate::transcription_models::definition(model);
        let selection = crate::transcription_models::TranscriptionSelection {
            model,
            language,
            // Hints persist across models that can use them.
            recognition_hints: if definition.supports_recognition_hints {
                self.settings.transcription.recognition_hints.clone()
            } else {
                String::new()
            },
        };
        self.preparation.start(selection, false);
    }

    /// A layout change not yet polled counts as before this choice, so it
    /// cannot later undo it.
    fn manual_choice(&mut self) {
        if let Some(source) = self
            .input_sources
            .as_mut()
            .and_then(|watcher| watcher.poll())
        {
            self.keyboard_switcher
                .source_changed(source, Instant::now());
        }
        self.keyboard_switcher.manual_choice();
        if self.automatic_preparation {
            self.preparation.cancel();
            self.automatic_preparation = false;
        }
    }

    /// A layout being cycled through cancels only automatic work; a manual
    /// operation is superseded once the debounced choice is due.
    fn supersede_automatic(&mut self) {
        if self.automatic_preparation {
            self.preparation.cancel();
            self.automatic_preparation = false;
        }
    }

    /// Follows the foreground layout while switching is on, and starts the
    /// installed-only preparation of a mapping once it is due.
    fn switch_with_keyboard(&mut self) {
        let enabled = self.settings.keyboard_model_switching.enabled;
        if enabled != self.input_sources.is_some() {
            self.input_sources = enabled.then(Default::default);
        }
        if let Some(source) = self
            .input_sources
            .as_mut()
            .and_then(|watcher| watcher.poll())
            && self
                .keyboard_switcher
                .source_changed(source, Instant::now())
        {
            self.supersede_automatic();
        }
        let Some(choice) = self.keyboard_switcher.take_due(Instant::now()) else {
            return;
        };
        let selection = crate::transcription_models::TranscriptionSelection {
            language: choice.language,
            ..self.settings.transcription_for_model(choice.model)
        };
        if selection.model != self.settings.transcription.model
            || selection.language != self.settings.transcription.language
        {
            self.preparation.start(selection, true);
            self.automatic_preparation = true;
        } else {
            // The current model stays, but an older manual preparation must
            // not replace it.
            self.preparation.cancel();
            self.automatic_preparation = false;
        }
    }

    fn model_ready(&self) -> bool {
        let model = crate::transcription_models::definition(self.settings.transcription.model);
        crate::transcription_models::is_installed(model, &self.settings.transcription.language)
            && crate::transcription_models::is_verified(model)
    }

    pub(crate) fn start(&mut self) {
        self.listen_when_ready = true;
        if self.is_running() {
            return;
        }
        if !self.model_ready() {
            self.status = "Model required".into();
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (result_sender, result_receiver) = mpsc::channel();
        let (updates, update_receiver) = mpsc::channel();
        let (swap_failure_sender, swap_failures) = mpsc::channel();
        let event_path = self.event_path.clone();
        let selection = self.runtime_selection();
        let config = crate::windows_dictation::WindowsDictationConfig {
            swap_failures: Some(swap_failure_sender),
            history: self.history.clone(),
            last_dictation: self.last_dictation.clone(),
            indicator: Some(self.indicator.clone()),
            mode_runtime: self.mode_runtime.clone(),
            transformations: self.transformations.clone(),
            ..crate::windows_dictation::WindowsDictationConfig::from_settings(&self.settings)
        };
        let worker = std::thread::Builder::new()
            .name("windows-listener".into())
            .spawn(move || {
                let result = crate::instance::acquire("windows-listener").and_then(|_instance| {
                    crate::windows_dictation::run(
                        &event_path,
                        &selection,
                        config,
                        update_receiver,
                        &worker_stop,
                    )
                });
                let _ = result_sender.send(result.map_err(|error| format!("{error:#}")));
            });
        match worker {
            Ok(worker) => {
                self.listener = Some(RunningListener {
                    stop,
                    result: result_receiver,
                    worker,
                    updates,
                    swap_failures,
                });
                self.session_before_start = self.activity.session_started_at;
                self.awaiting_session_start = true;
                self.status = "Enabling…".into();
                self.error = None;
            }
            Err(error) => {
                self.status = "Couldn't enable".into();
                self.error = Some(tr_fill("Could not start listener: {}", &error.to_string()));
            }
        }
    }

    pub(crate) fn stop(&mut self) {
        self.listen_when_ready = false;
        self.signal_stop("Disabling…");
    }

    fn is_running(&self) -> bool {
        self.listener.is_some()
    }

    fn signal_stop(&mut self, status: &str) {
        if let Some(listener) = &self.listener {
            listener.stop.store(true, Ordering::Relaxed);
            self.status = status.into();
        }
    }

    /// Applies the settings the shared window saved. Shared state the
    /// listener reads changes in place; anything the listener captured at
    /// start restarts a running listener.
    fn apply_settings(&mut self, settings: &AppSettings) {
        if self
            .keyboard_switcher
            .configure(settings.keyboard_model_switching.clone(), Instant::now())
        {
            self.supersede_automatic();
        }
        let previous = std::mem::replace(&mut self.settings, settings.clone());
        let current = settings;
        crate::feedback::set_enabled(current.sound_effects);
        crate::feedback::set_volume(current.sound_effect_volume.clamp(0.0, 1.0));
        crate::feedback::set_sound_set(current.sound_theme);
        if previous.indicator_position != current.indicator_position {
            self.indicator.set_position(current.indicator_position);
        }
        if previous.history_retention != current.history_retention
            && let Some(history) = &self.history
            && let Err(error) = history.set_retention(current.history_retention)
        {
            tracing::warn!(%error, "could not update history retention");
        }
        let modes_changed = previous.dictation_processing != current.dictation_processing;
        if modes_changed {
            *self
                .mode_runtime
                .write()
                .unwrap_or_else(|error| error.into_inner()) =
                crate::dictation_modes::ModeRuntime::from_settings(current);
        }
        if previous.ui_language != current.ui_language {
            crate::desktop_i18n::apply(current.ui_language.as_deref());
        }
        if modes_changed {
            self.refresh_personal_commands_host();
        }
        let model_changed = previous.transcription.model != current.transcription.model
            || previous.transcription.language != current.transcription.language;
        // A running listener applies them in place, so listening never stops
        // for a settings change.
        if let Some(listener) = &self.listener {
            let _ = listener.updates.send(Box::new(current.clone()));
        } else if model_changed && self.listen_when_ready {
            // A listener waiting for a model starts once one is chosen.
            self.start();
        }
    }

    /// The selection with recognition hints stripped when the model cannot
    /// use them, so validation never rejects a load because hints linger
    /// from a Whisper session.
    fn runtime_selection(&self) -> crate::transcription_models::TranscriptionSelection {
        let mut selection = self.settings.transcription.clone();
        if !crate::transcription_models::definition(selection.model).supports_recognition_hints {
            selection.recognition_hints = String::new();
        }
        selection
    }

    /// The listener could not load a newly chosen model and still runs the
    /// previous one. Settings go back to it, so they never name a model that
    /// is not running and the picker never offers to delete the one in use.
    fn keep_running_model(&mut self, failed: crate::local_transcriber::SwapFailed) {
        // A choice made after the failed one stands.
        if self.runtime_selection() != failed.wanted {
            return;
        }
        let mut candidate = self.settings.clone();
        candidate.remember_transcription(failed.kept);
        match candidate.save() {
            Ok(()) => self.apply_settings(&candidate),
            Err(error) => tracing::warn!(%error, "could not restore the running model setting"),
        }
        self.preparation.set_error(Some(failed.error));
    }

    /// Hosts the personal transformations workspace while any mode uses a
    /// transformation or the workspace exists.
    fn refresh_personal_commands_host(&mut self) {
        let enabled =
            crate::app_settings::transformations_used(&self.settings.dictation_processing)
                || crate::personal_commands::workspace_configured();
        match (enabled, self.personal_commands.is_some()) {
            (true, false) => {
                self.personal_commands =
                    crate::personal_commands::PersonalCommands::start(self.transformations.clone());
            }
            (false, true) => self.personal_commands = None,
            _ => {}
        }
    }

    /// True once Restart to update has handed off to the new version, after
    /// the updater's final check; Hex then quits so it can take the named
    /// event.
    pub(crate) fn update_restarted(&mut self) -> bool {
        match self.updater.take_restart() {
            Some(Ok(())) => {
                self.stop();
                true
            }
            Some(Err(error)) => {
                self.error = Some(tr_fill("Could not restart Hex: {}", &format!("{error:#}")));
                false
            }
            None => false,
        }
    }

    pub(crate) fn refresh(&mut self) {
        self.switch_with_keyboard();
        if let Some(result) = self.preparation.poll() {
            self.automatic_preparation = false;
            let result = result.and_then(|selection| {
                crate::transcription_models::validate(&selection).map_err(|e| e.to_string())?;
                let mut candidate = self.settings.clone();
                candidate.remember_transcription(selection);
                candidate.save().map_err(|e| format!("{e:#}"))?;
                self.apply_settings(&candidate);
                Ok(())
            });
            if let Err(error) = result {
                self.preparation.set_error(Some(error));
            }
        }
        let swap_failures = self
            .listener
            .as_ref()
            .map(|listener| listener.swap_failures.try_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        for failed in swap_failures {
            self.keep_running_model(failed);
        }
        if self.listen_after_setup && crate::onboarding::completion_recorded() {
            self.listen_after_setup = false;
            self.start();
        }
        // Settings may set up the workspace at any time.
        if self.personal_commands_checked_at.elapsed() >= Duration::from_secs(2) {
            self.personal_commands_checked_at = Instant::now();
            self.refresh_personal_commands_host();
        }
        let mut start_listener = false;
        if let Some(listener) = &self.listener {
            let finished = match listener.result.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    Some(Err("listener worker stopped unexpectedly".into()))
                }
            };
            if let Some(result) = finished {
                self.awaiting_session_start = false;
                if let Some(listener) = self.listener.take() {
                    let _ = listener.worker.join();
                }
                match result {
                    Ok(()) => {
                        self.status = "Disabled".into();
                        start_listener = self.listen_when_ready;
                    }
                    Err(error) => {
                        self.status = "Couldn't enable".into();
                        self.error = Some(error);
                    }
                }
            }
        }
        if start_listener {
            self.start();
        }

        self.activity.refresh(&mut self.event_reader);
        self.last_completed_at = last_completed_at(self.event_reader.events().iter());
        if self.awaiting_session_start
            && self.activity.session_started_at != self.session_before_start
        {
            self.awaiting_session_start = false;
        }
        if self.is_running()
            && !self.awaiting_session_start
            && let Some(status) = self.activity.state_label()
        {
            self.status = status.into();
        }
    }

    pub(crate) fn publish(&self) -> Published {
        let preparation = self.preparation.status();
        Published {
            listener: DesktopListenerSnapshot {
                running: self.is_running(),
                status: self.status.clone(),
                warning: visible_failure(&self.activity, self.last_completed_at),
                shortcuts: DesktopShortcuts::Registered,
                shortcuts_configurable: false,
            },
            operation_error: self.error.clone(),
            transcription: DesktopTranscriptionSnapshot {
                downloaded_bytes: preparation.downloaded_bytes,
                error: preparation.error,
                preparation_stage: preparation
                    .model
                    .map(|_| ModelPreparationStage::Downloading),
                selection: self.settings.transcription.clone(),
                preparing: preparation.model,
            },
            update_status: self.updater.status(),
            settings: self.settings.clone(),
        }
    }
}

impl Drop for WindowsDesktopHost {
    fn drop(&mut self) {
        self.updater.stop();
        if let Some(listener) = self.listener.take() {
            listener.stop_and_join();
        }
    }
}

/// When the current session's newest dictation completed. `events` is
/// oldest first, as `EventReader` keeps them.
fn last_completed_at<'a>(events: impl DoubleEndedIterator<Item = &'a VoiceEvent>) -> Option<u64> {
    for event in events.rev() {
        match event {
            VoiceEvent::SessionStarted { .. } => return None,
            VoiceEvent::Transcript {
                timestamp_ms,
                phase: TranscriptPhase::Completed,
                text,
                ..
            } if !text.trim().is_empty() => return Some(*timestamp_ms),
            _ => {}
        }
    }
    None
}

/// The newest failure, hidden once a later dictation completed, so a silent
/// failure still reaches the Settings window without sticking around.
fn visible_failure(activity: &DesktopActivity, last_completed_at: Option<u64>) -> Option<String> {
    activity
        .last_failure
        .as_ref()
        .filter(|(failed_at, _)| last_completed_at.is_none_or(|completed| completed < *failed_at))
        .map(|(_, message)| message.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcript(timestamp_ms: u64, text: &str) -> VoiceEvent {
        VoiceEvent::Transcript {
            timestamp_ms,
            phase: TranscriptPhase::Completed,
            latency_ms: 10,
            text: text.into(),
        }
    }

    #[test]
    fn the_newest_completion_is_scoped_to_the_current_session() {
        let events = [
            transcript(1, "Old session"),
            VoiceEvent::SessionStarted { timestamp_ms: 2 },
        ];
        assert_eq!(last_completed_at(events.iter()), None);
        let events = [
            VoiceEvent::SessionStarted { timestamp_ms: 2 },
            transcript(3, "First"),
            transcript(4, "Second"),
            transcript(5, "  "),
        ];
        assert_eq!(last_completed_at(events.iter()), Some(4));
    }

    #[test]
    fn newest_failure_shows_until_a_later_transcript_completes() {
        let activity = DesktopActivity {
            last_failure: Some((5, "OpenCode generation failed".into())),
            ..DesktopActivity::default()
        };
        assert_eq!(
            visible_failure(&activity, Some(4)).as_deref(),
            Some("OpenCode generation failed")
        );
        assert_eq!(visible_failure(&activity, Some(6)), None);
    }
}

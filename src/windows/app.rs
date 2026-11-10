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
use crate::desktop::activity::{DesktopActivity, last_completed_at};
use crate::desktop::events::EventReader;
use crate::desktop::host::{
    DesktopAction, DesktopListenerSnapshot, DesktopShortcuts, DesktopTranscriptionSnapshot,
};
use crate::desktop::i18n::tr_fill;
use crate::history::History;
use crate::transcription::models::{ModelPreparationStage, TranscriptionModelId};
use crate::transcription::preparation::TranscriptionPreparation;
use crate::windows::rune::{Published, Request};

type ListenerResult = std::result::Result<(), String>;

struct RunningListener {
    stop: Arc<AtomicBool>,
    result: Receiver<ListenerResult>,
    worker: JoinHandle<()>,
    /// Saved settings the listener applies in place.
    updates: mpsc::Sender<Box<AppSettings>>,
    swap_failures: Receiver<crate::transcription::local_transcriber::SwapFailed>,
    /// The tray's and `hex` commands, handled as the shortcut would be.
    commands: mpsc::Sender<crate::windows::dictation::ListenerCommand>,
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
    /// When this host started, so a previous session's failure is ignored.
    started_at: u64,
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
    mode_runtime: Arc<RwLock<crate::dictation::modes::ModeRuntime>>,
    transformations: Arc<crate::personal_commands::TransformationClient>,
    personal_commands: Option<crate::personal_commands::PersonalCommands>,
    personal_commands_checked_at: Instant,
    preparation: TranscriptionPreparation,
    keyboard_switcher: crate::transcription::keyboard_model_switching::Switcher,
    /// Labs › Switch model with app: the foreground app's mode choice.
    app_switcher: crate::transcription::app_model_switching::Switcher,
    /// Present only while keyboard model switching is on.
    input_sources: Option<crate::keyboard_input_sources::Watcher>,
    /// The preparation running is a keyboard switch, which a manual choice
    /// or a later source change supersedes.
    automatic_preparation: bool,
    updater: crate::updater::Updater,
    /// The newest foreground window of another app, where the tray's Paste
    /// last dictation goes once its menu has taken the foreground.
    foreground: ForeignWindow,
    /// The local API's `/dictations` controls, for whichever listener runs.
    dictations: crate::dictation::service::DictationQueue,
}

/// Names the event that marks the running Hex app in this login session.
const APP_INSTANCE_EVENT: &str = r"Local\anomaly.hex.app";

/// The data root `HEX_APPLICATION_SUPPORT_DIR` names, unless it is the
/// default one, which keeps the default event.
fn custom_root() -> Option<std::path::PathBuf> {
    let canonical = |path: std::path::PathBuf| path.canonicalize().unwrap_or(path);
    let root = canonical(std::env::var_os("HEX_APPLICATION_SUPPORT_DIR")?.into());
    let default = dirs::data_dir().map(|data| canonical(data.join("hex")));
    (default.as_ref() != Some(&root)).then_some(root)
}

/// The instance event for a data root: [`APP_INSTANCE_EVENT`] for the
/// default one, and its own event for another root that
/// `HEX_APPLICATION_SUPPORT_DIR` names, so a copy of Hex with its own data,
/// such as a benchmark, neither signals nor yields to the person's Hex.
fn instance_event_name(custom_root: Option<&std::path::Path>) -> String {
    use sha2::{Digest, Sha256};
    match custom_root {
        None => APP_INSTANCE_EVENT.into(),
        Some(root) => {
            // Windows paths compare without case.
            let path = root.to_string_lossy().to_lowercase();
            let digest = Sha256::digest(path.as_bytes());
            let hash: String = digest[..8]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            format!("{APP_INSTANCE_EVENT}.{hash}")
        }
    }
}

/// Ownership of the one Hex app in this login session (per data root).
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
        let name = instance_event_name(custom_root().as_deref());
        let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
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

impl WindowsDesktopHost {
    pub(crate) fn new(
        event_path: PathBuf,
        settings: AppSettings,
        settings_error: Option<String>,
        history: Option<History>,
        indicator: crate::rune_hud::HudSender,
        dictations: crate::dictation::service::DictationQueue,
        processing: crate::dictation::modes::ProcessingState,
    ) -> Self {
        crate::desktop::i18n::apply(settings.ui_language.as_deref());
        let mut event_reader = EventReader::open(&event_path);
        let mut activity = DesktopActivity::default();
        activity.refresh(&mut event_reader);
        let last_completed_at = last_completed_at(event_reader.events().iter());
        let started_at = crate::desktop::events::now_ms();
        // Known before the first poll, so launching keeps the saved model
        // instead of applying the current layout's mapping.
        let keyboard_switcher = crate::transcription::keyboard_model_switching::Switcher::new(
            settings.keyboard_model_switching.clone(),
            settings
                .keyboard_model_switching
                .enabled
                .then(crate::keyboard_input_sources::current)
                .flatten(),
        );
        // Known before the first poll, so launching keeps the saved model
        // instead of applying the frontmost app's mode choice.
        let app_switcher = crate::transcription::app_model_switching::Switcher::new(
            settings.labs.app_model_switching,
            settings
                .labs
                .app_model_switching
                .then(|| crate::transcription::app_model_switching::startup_choice(&settings))
                .flatten(),
        );
        let mut host = Self {
            event_path,
            event_reader,
            activity,
            last_completed_at,
            started_at,
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
            mode_runtime: processing.modes,
            transformations: processing.transformations,
            personal_commands: None,
            personal_commands_checked_at: Instant::now(),
            preparation: TranscriptionPreparation::default(),
            keyboard_switcher,
            app_switcher,
            input_sources: None,
            automatic_preparation: false,
            updater: crate::updater::Updater::start(),
            foreground: ForeignWindow::default(),
            dictations,
        };
        crate::transcription::models::delete_set_aside();
        host.refresh_personal_commands_host();
        // The microphone stays cold until first-run setup finishes.
        if host.settings.enable_on_launch {
            if crate::desktop::onboarding::completion_recorded() {
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
                self.remember_listening(self.listen_when_ready);
            }
            Request::ChooseModel { model, language } => self.choose_model(model, language),
            Request::CancelModel => {
                self.manual_choice();
                self.preparation.cancel();
            }
            Request::PasteLast => {
                if let Some(listener) = &self.listener {
                    self.foreground.activate();
                    if listener
                        .commands
                        .send(crate::windows::dictation::ListenerCommand::PasteLast)
                        .is_ok()
                    {
                        crate::windows::dictation::wake();
                    }
                }
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
            DesktopAction::StartListening => {
                self.start();
                self.remember_listening(true);
            }
            DesktopAction::StopListening => {
                self.stop();
                self.remember_listening(false);
            }
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
        let selection = crate::transcription::models::TranscriptionSelection { model, language };
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
        self.app_switcher.manual_choice();
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
        let selection = crate::transcription::models::TranscriptionSelection {
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

    /// Follows the foreground app while switching is on, and starts the
    /// installed-only preparation of a mode's speech choice once it is due.
    /// Hex's own window and a moment with no window in front name no mode,
    /// so they leave the last match and any switch it scheduled alone. The
    /// host cannot see a listener recording, so a due switch is held only
    /// while the shared engine's dictation streams or finishes run.
    fn switch_with_app(&mut self) {
        if !self.settings.labs.app_model_switching {
            return;
        }
        let application = foreground_other_process_stem();
        let now = Instant::now();
        if self.app_switcher.observe(
            crate::transcription::app_model_switching::Observation::frontmost(
                application.as_deref(),
            ),
            |application| {
                crate::transcription::app_model_switching::mode_choice(&self.settings, application)
            },
            now,
        ) {
            self.supersede_automatic();
        }
        if crate::transcription::engine::dictation_busy() {
            return;
        }
        let Some(choice) = self
            .app_switcher
            .take_due(now, &self.settings.transcription)
        else {
            return;
        };
        let selection = crate::transcription::models::TranscriptionSelection {
            language: choice.language,
            ..self.settings.transcription_for_model(choice.model)
        };
        self.preparation.start(selection, true);
        self.automatic_preparation = true;
    }

    fn model_ready(&self) -> bool {
        let model = crate::transcription::models::definition(self.settings.transcription.model);
        crate::transcription::models::is_installed(model, &self.settings.transcription.language)
            && crate::transcription::models::is_verified(model)
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
        let (commands, command_receiver) = mpsc::channel();
        let event_path = self.event_path.clone();
        let selection = self.runtime_selection();
        let config = crate::windows::dictation::WindowsDictationConfig {
            swap_failures: Some(swap_failure_sender),
            commands: Some(command_receiver),
            dictations: self.dictations.clone(),
            history: self.history.clone(),
            last_dictation: self.last_dictation.clone(),
            indicator: Some(self.indicator.clone()),
            mode_runtime: self.mode_runtime.clone(),
            transformations: self.transformations.clone(),
            ..crate::windows::dictation::WindowsDictationConfig::from_settings(&self.settings)
        };
        let worker = std::thread::Builder::new()
            .name("windows-listener".into())
            .spawn(move || {
                let result =
                    crate::desktop::instance::acquire("windows-listener").and_then(|_instance| {
                        crate::windows::dictation::run(
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
                    commands,
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

    /// Enable and Disable also decide whether Hex listens at its next launch,
    /// so a disabled Hex stays disabled after a restart (#575).
    fn remember_listening(&mut self, enabled: bool) {
        if self.settings.enable_on_launch == enabled {
            return;
        }
        let mut candidate = self.settings.clone();
        candidate.enable_on_launch = enabled;
        match save_listening(&candidate) {
            Ok(()) => self.apply_settings(&candidate),
            Err(error) => tracing::warn!(%error, "could not save Enable on launch"),
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
        if self
            .app_switcher
            .configure(settings.labs.app_model_switching)
        {
            self.supersede_automatic();
        }
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
                crate::dictation::modes::ModeRuntime::from_settings(current);
        }
        if previous.ui_language != current.ui_language {
            crate::desktop::i18n::apply(current.ui_language.as_deref());
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

    /// The saved selection, or the default model while it names an import
    /// that can't run.
    fn runtime_selection(&self) -> crate::transcription::models::TranscriptionSelection {
        crate::transcription::custom_models::runnable(&self.settings.transcription)
    }

    /// The listener could not load a newly chosen model and still runs the
    /// previous one. Settings go back to it, so they never name a model that
    /// is not running and the picker never offers to delete the one in use.
    fn keep_running_model(&mut self, failed: crate::transcription::local_transcriber::SwapFailed) {
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
        self.switch_with_app();
        if let Some(result) = self.preparation.poll() {
            self.automatic_preparation = false;
            let result = result.and_then(|selection| {
                crate::transcription::models::validate(&selection).map_err(|e| e.to_string())?;
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
        if self.listen_after_setup && crate::desktop::onboarding::completion_recorded() {
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

        self.foreground.track();
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

    /// Where `hex` dictation commands go: the running listener, once its
    /// session started (a listener still loading its model would replay
    /// them late).
    pub(crate) fn listener_commands(
        &self,
    ) -> Option<mpsc::Sender<crate::windows::dictation::ListenerCommand>> {
        self.listener
            .as_ref()
            .filter(|_| !self.awaiting_session_start)
            .map(|listener| listener.commands.clone())
    }

    pub(crate) fn publish(&self) -> Published {
        let preparation = self.preparation.status();
        Published {
            activity: self.activity.state,
            listener: DesktopListenerSnapshot {
                running: self.is_running(),
                warming: self.is_running() && self.awaiting_session_start,
                status: self.status.clone(),
                warning: self.indicator.failure(),
                last_failure: self
                    .activity
                    .visible_failure_since(self.started_at, self.last_completed_at),
                notice: None,
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
            update_notes: self.updater.notes(),
            update_blocked: self.updater.blocked(),
            settings: self.settings.clone(),
            state: self.activity.state.filter(|_| self.is_running()),
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

/// The foreground application's executable stem, or `None` when Hex owns the
/// foreground window or none is focused: Labs › Switch model with app matches
/// applications only, never Hex's own window.
pub(crate) fn foreground_other_process_stem() -> Option<String> {
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };
    // SAFETY: plain queries; a null window owns no process.
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return None;
        }
        let mut process_id = 0;
        GetWindowThreadProcessId(window, &mut process_id);
        if process_id == GetCurrentProcessId() {
            return None;
        }
        let path = crate::windows::input::window_process_path(window)?;
        Some(path.file_stem()?.to_string_lossy().to_lowercase())
    }
}

/// The newest foreground window another process owns, polled with the
/// runtime's refresh.
#[derive(Default)]
struct ForeignWindow(Option<isize>);

impl ForeignWindow {
    fn track(&mut self) {
        use windows_sys::Win32::System::Threading::GetCurrentProcessId;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        // SAFETY: plain queries; a null window owns no process.
        unsafe {
            let window = GetForegroundWindow();
            if window.is_null() {
                return;
            }
            let mut process_id = 0;
            GetWindowThreadProcessId(window, &mut process_id);
            if process_id != GetCurrentProcessId() {
                self.0 = Some(window as isize);
            }
        }
    }

    /// Brings that window back to the foreground, where a paste lands.
    fn activate(&self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindow, SetForegroundWindow};
        let Some(window) = self.0 else { return };
        let window = window as windows_sys::Win32::Foundation::HWND;
        // SAFETY: `IsWindow` vets a handle that may have closed since.
        unsafe {
            if IsWindow(window) != 0 {
                SetForegroundWindow(window);
            }
        }
    }
}

#[cfg(not(test))]
fn save_listening(settings: &AppSettings) -> Result<()> {
    settings.save()
}

/// Tests never write the person's `settings.json`.
#[cfg(test)]
fn save_listening(_: &AppSettings) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_isolated_data_root_has_its_own_instance_event() {
        assert_eq!(instance_event_name(None), APP_INSTANCE_EVENT);
        let isolated = instance_event_name(Some(std::path::Path::new(r"C:\bench\home\hex")));
        assert!(isolated.starts_with(r"Local\anomaly.hex.app."));
        assert_eq!(isolated.len(), APP_INSTANCE_EVENT.len() + 17);
        assert_eq!(
            isolated,
            instance_event_name(Some(std::path::Path::new(r"c:\Bench\Home\HEX")))
        );
        assert_ne!(
            isolated,
            instance_event_name(Some(std::path::Path::new(r"C:\bench\other\hex")))
        );
    }
}

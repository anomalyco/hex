//! Linux processes: `hex.service`, which owns the listener, shortcut
//! registration, model preparation, and updates, and the Settings process,
//! which opens the shared app window and talks to the service over its
//! socket.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use color_eyre::Result;
use gpui::{App, Application, Timer};

use crate::app_settings::{AppSettings, HotkeyBinding};
use crate::desktop_activity::DesktopActivity;
use crate::desktop_host::{
    DesktopAction, DesktopListenerSnapshot, DesktopShortcut, DesktopSnapshot,
    DesktopTranscriptionSnapshot, DesktopUpdateStatus,
};
use crate::events::EventReader;
use crate::linux_input::LinuxHotkey;
use crate::linux_service::{
    self, Event as ServiceEvent, Request, ShortcutTarget, Snapshot as ServiceSnapshot,
};
use crate::linux_session::LinuxSession;
use crate::linux_updater::InstalledUpdate;

const UPDATE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

type ListenerResult = std::result::Result<(), String>;
type PreparationResult = std::result::Result<PreparedTranscription, String>;

struct PreparedTranscription {
    selection: crate::transcription_models::TranscriptionSelection,
    transcriber: crate::local_transcriber::LocalTranscriber,
}

struct TranscriptionPreparation {
    canceled: Arc<AtomicBool>,
    model: crate::transcription_models::TranscriptionModelId,
    progress: Arc<AtomicU64>,
    stage: Arc<AtomicU8>,
    worker: JoinHandle<PreparationResult>,
}

#[derive(Clone, PartialEq)]
enum SettingsChange {
    Capture(ShortcutTarget),
    Hotkey(ShortcutTarget, LinuxHotkey),
    /// Settings the Settings window already saved.
    Reload(Box<AppSettings>),
}

struct SettingsEdit {
    change: SettingsChange,
    resume: bool,
    canceled: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<AppSettings>>>,
}

struct Listener {
    stop: Arc<AtomicBool>,
    worker: JoinHandle<ListenerResult>,
    startup: ListenerStartup,
    hotkey_status: crate::linux_input::HotkeyStatus,
}

enum ListenerStartup {
    Awaiting { previous_session: Option<u64> },
    Observed,
}

impl Listener {
    fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    fn finish(self) -> ListenerResult {
        self.worker
            .join()
            .unwrap_or_else(|_| Err("listener worker stopped unexpectedly".into()))
    }

    fn observe_session(&mut self, session: Option<u64>) {
        if let ListenerStartup::Awaiting { previous_session } = self.startup
            && session != previous_session
        {
            self.startup = ListenerStartup::Observed;
        }
    }
}

struct LinuxDesktopHost {
    event_path: PathBuf,
    event_reader: EventReader,
    activity: DesktopActivity,
    listener: Option<Listener>,
    listen_when_ready: bool,
    status: String,
    error: Option<String>,
    dismissed_failure_at: Option<u64>,
    settings_error: Option<String>,
    settings: AppSettings,
    // Dismissing a load error must not authorize recording with fallback defaults.
    settings_valid: bool,
    settings_edit: Option<SettingsEdit>,
    prepared_transcriber: Option<crate::local_transcriber::LocalTranscriber>,
    transcription_preparation: Option<TranscriptionPreparation>,
    model_downloads: crate::model_downloads::ModelDownloads,
    pending_transcription: Option<crate::transcription_models::TranscriptionSelection>,
    transcription_error: Option<String>,
    update: UpdateState,
    output: crate::linux_dictation::DictationOutput,
    /// Hosts the personal transformations workspace while a mode uses one.
    personal_commands: Option<crate::personal_commands::PersonalCommands>,
    personal_commands_checked_at: Instant,
}

enum UpdateState {
    Unmanaged,
    Checking(Receiver<Result<Option<InstalledUpdate>, String>>),
    Failed(Instant),
    Waiting(Instant),
    Ready(InstalledUpdate),
}

fn service_snapshot(host: &LinuxDesktopHost, owns_editor: bool) -> ServiceSnapshot {
    ServiceSnapshot {
        desktop: host.snapshot(),
        settings: host.settings.clone(),
        editing: host.settings_edit.is_some(),
        capturing: owns_editor && host.capturing_hotkey(),
        hud_limitation: crate::linux_desktop::hud_limitation(),
        pid: std::process::id(),
        session: linux_service::Session::current(),
    }
}

fn disconnect_editor(
    host: &mut LinuxDesktopHost,
    editor: &mut Option<(u64, Instant)>,
    client: u64,
) {
    if editor.is_some_and(|(owner, _)| owner == client) {
        host.cancel_settings_edit();
        *editor = None;
    }
}

/// The service owns this host independently of any GPUI application or Settings window.
pub fn run_service(event_path: PathBuf, shutdown: &'static AtomicBool) -> Result<()> {
    let mut server = linux_service::Server::bind()?;
    let (settings, error) = match AppSettings::load() {
        Ok(settings) => (settings, None),
        Err(error) => (
            AppSettings::default(),
            Some(format!("Could not load settings: {error:#}")),
        ),
    };
    crate::linux_dictation::initialize_feedback(&settings);
    let update = if crate::linux_updater::managed_install() {
        UpdateState::Checking(start_update_check())
    } else {
        UpdateState::Unmanaged
    };
    let enable_on_launch = settings.enable_on_launch;
    let mut host = LinuxDesktopHost::new(event_path, settings, error, update);
    if enable_on_launch {
        host.start();
    }
    let mut editor: Option<(u64, Instant)> = None;
    let mut stopping = false;
    loop {
        if shutdown.load(Ordering::Acquire) && !stopping {
            stopping = true;
            host.stop();
            host.cancel_transcription_preparation();
        }
        host.refresh();
        if stopping && !host.has_pending_work() {
            break;
        }
        if !host.capturing_hotkey() {
            editor = None;
        }
        if let Some((owner, deadline)) = editor
            && Instant::now() >= deadline
        {
            disconnect_editor(&mut host, &mut editor, owner);
        }
        server.accept()?;
        match server.events.recv_timeout(Duration::from_millis(20)) {
            Ok(ServiceEvent::Disconnected(client)) => {
                disconnect_editor(&mut host, &mut editor, client)
            }
            Ok(ServiceEvent::Request {
                client,
                request,
                reply,
            }) => {
                let result = (|| -> Result<()> {
                    if stopping {
                        return Err(color_eyre::eyre::eyre!("Hex service is stopping"));
                    }
                    if let Some((owner, deadline)) = &mut editor {
                        if *owner == client {
                            *deadline = Instant::now() + Duration::from_secs(3);
                        } else if matches!(
                            request,
                            Request::CaptureShortcut
                                | Request::CaptureShortcutFor(_)
                                | Request::CancelShortcut
                                | Request::SetShortcut { .. }
                                | Request::Desktop(DesktopAction::SetDictationShortcut(_))
                        ) {
                            return Err(color_eyre::eyre::eyre!(
                                "Another Settings client is capturing a shortcut"
                            ));
                        }
                    }
                    match request {
                        Request::Snapshot => {}
                        Request::Desktop(action) => host.dispatch(action)?,
                        Request::CaptureShortcut | Request::CaptureShortcutFor(_) => {
                            let target = match request {
                                Request::CaptureShortcutFor(target) => target,
                                _ => ShortcutTarget::Dictation,
                            };
                            host.begin_settings_edit(SettingsChange::Capture(target));
                            if host.capturing_hotkey() {
                                editor = Some((client, Instant::now() + Duration::from_secs(3)));
                            }
                        }
                        Request::CancelShortcut => host.cancel_settings_edit(),
                        Request::SetShortcut { target, shortcut } => {
                            host.set_hotkey(target, shortcut)?
                        }
                        Request::ChooseModel { model, language } => {
                            host.choose_transcription(model, language)?
                        }
                        Request::CancelModel => host.cancel_transcription_preparation(),
                        Request::CancelModelFor { model } => host.cancel_transcription_model(model),
                        Request::ReloadSettings => host.reload_settings()?,
                        Request::Shutdown => shutdown.store(true, Ordering::Release),
                    }
                    Ok(())
                })();
                let _ = reply.try_send((
                    service_snapshot(&host, editor.is_some_and(|(owner, _)| owner == client)),
                    result.err().map(|e| format!("{e:#}")),
                ));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

pub fn open(start_hidden: bool, event_path: PathBuf, shutdown: &'static AtomicBool) -> Result<()> {
    if start_hidden {
        return linux_service::start();
    }
    open_settings(event_path, shutdown)
}

/// The Settings process: the shared app window, which saves `settings.json`
/// and asks the service to reload it. Closing the window ends the process.
fn open_settings(event_path: PathBuf, shutdown: &'static AtomicBool) -> Result<()> {
    Application::new().run(move |cx: &mut App| {
        cx.bind_keys(crate::text_input::key_bindings());
        let window =
            crate::app_window::open(event_path, cx).expect("could not open the Hex window");
        window
            .update(cx, |_, window, cx| {
                window.set_window_title("Hex");
                // Quit without letting GPUI close the window here: on Wayland
                // it destroys the Vulkan surface at once, and a configure the
                // compositor queued behind the close (as sway does when it
                // retiles) then resizes the destroyed surface and segfaults.
                // Quitting stops the event loop after this dispatch instead.
                window.on_window_should_close(cx, |_, cx| {
                    cx.quit();
                    false
                });
                window.activate_window();
            })
            .ok();
        cx.spawn(async move |cx| {
            loop {
                Timer::after(Duration::from_millis(250)).await;
                if shutdown.load(Ordering::Relaxed) {
                    let _ = cx.update(|cx| cx.quit());
                    break;
                }
            }
        })
        .detach();
        cx.activate(true);
    });
    Ok(())
}

impl LinuxDesktopHost {
    fn new(
        event_path: PathBuf,
        settings: AppSettings,
        error: Option<String>,
        update: UpdateState,
    ) -> Self {
        let mut event_reader = EventReader::open(&event_path);
        let mut activity = DesktopActivity::default();
        activity.refresh(&mut event_reader);
        // Tests never touch the user's history.
        let history = if cfg!(test) {
            None
        } else {
            crate::history::History::open_default(settings.history_retention)
                .inspect_err(|error| tracing::warn!(%error, "could not open retained history"))
                .ok()
        };
        let output = crate::linux_dictation::DictationOutput::from_settings(&settings, history);
        let mut host = Self {
            event_reader,
            event_path,
            activity,
            listener: None,
            listen_when_ready: false,
            status: "Disabled".into(),
            error: None,
            dismissed_failure_at: None,
            settings_valid: error.is_none(),
            settings_error: error,
            settings,
            settings_edit: None,
            prepared_transcriber: None,
            transcription_preparation: None,
            model_downloads: crate::model_downloads::ModelDownloads::default(),
            pending_transcription: None,
            transcription_error: None,
            update,
            output,
            personal_commands: None,
            personal_commands_checked_at: Instant::now(),
        };
        host.refresh_personal_commands();
        host
    }

    /// Applies saved settings the running listener reads live: modes,
    /// history retention, and the transformations workspace.
    fn apply_output_settings(&mut self) {
        *self
            .output
            .modes
            .write()
            .unwrap_or_else(|error| error.into_inner()) =
            crate::dictation_modes::ModeRuntime::from_settings(&self.settings);
        if let Some(history) = &self.output.history
            && let Err(error) = history.set_retention(self.settings.history_retention)
        {
            tracing::warn!(%error, "could not apply the history retention");
        }
        self.refresh_personal_commands();
    }

    fn refresh_personal_commands(&mut self) {
        if cfg!(test) {
            return;
        }
        let enabled =
            crate::app_settings::transformations_used(&self.settings.dictation_processing)
                || crate::personal_commands::workspace_configured();
        match (enabled, self.personal_commands.is_some()) {
            (true, false) => {
                self.personal_commands = crate::personal_commands::PersonalCommands::start(
                    crate::commands::CommandConfig::new(),
                    self.output.transformations.clone(),
                );
            }
            (false, true) => self.personal_commands = None,
            _ => {}
        }
    }

    fn start(&mut self) {
        if self.settings_edit.is_some() {
            return;
        }
        self.listen_when_ready = true;
        if self.is_running() || self.transcription_preparation.is_some() {
            return;
        }
        // Defaults are only an editable recovery projection after a failed load.
        // They must never become a recording configuration until saved successfully.
        if !self.settings_valid {
            self.status = "Couldn't enable".into();
            return;
        }
        let model = crate::transcription_models::definition(self.settings.transcription.model);
        if !crate::transcription_models::is_installed(model, &self.settings.transcription.language)
        {
            self.status = "Model required".into();
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let settings = self.settings.clone();
        let output = self.output.clone();
        let event_path = self.event_path.clone();
        let prepared_transcriber = self.prepared_transcriber.take();
        let hotkey_status = crate::linux_input::HotkeyStatus::default();
        let worker_hotkey_status = hotkey_status.clone();
        let worker = std::thread::spawn(move || {
            let result = crate::instance::acquire("listener").and_then(|_instance| {
                crate::linux_dictation::run(
                    &event_path,
                    None,
                    &worker_stop,
                    &settings,
                    output,
                    prepared_transcriber,
                    worker_hotkey_status,
                )
            });
            result.map_err(|error| format!("{error:#}"))
        });
        self.listener = Some(Listener {
            stop,
            worker,
            startup: ListenerStartup::Awaiting {
                previous_session: self.activity.session_started_at,
            },
            hotkey_status,
        });
        self.status = "Enabling…".into();
    }

    fn stop(&mut self) {
        self.listen_when_ready = false;
        if let Some(edit) = &mut self.settings_edit {
            edit.resume = false;
            edit.canceled.store(true, Ordering::Release);
        }
        if let Some(listener) = &self.listener {
            listener.stop();
            self.status = "Disabling…".into();
        }
    }

    fn refresh(&mut self) {
        // Settings may set up the transformations workspace at any time.
        if self.personal_commands_checked_at.elapsed() >= Duration::from_secs(2) {
            self.personal_commands_checked_at = Instant::now();
            self.refresh_personal_commands();
        }
        if matches!(&self.update, UpdateState::Waiting(at) | UpdateState::Failed(at) if Instant::now() >= *at)
        {
            self.update = UpdateState::Checking(start_update_check());
        }
        let update_result = match &self.update {
            UpdateState::Checking(receiver) => match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Disconnected) => {
                    Some(Err("update worker stopped unexpectedly".into()))
                }
                Err(TryRecvError::Empty) => None,
            },
            _ => None,
        };
        if let Some(result) = update_result {
            self.update = match result {
                Ok(Some(executable)) => UpdateState::Ready(executable),
                Ok(None) => UpdateState::Waiting(Instant::now() + UPDATE_INTERVAL),
                Err(error) => {
                    tracing::warn!(%error, "Linux update check failed");
                    UpdateState::Failed(Instant::now() + UPDATE_INTERVAL)
                }
            };
        }

        for (model, result) in self.model_downloads.poll() {
            if let Err(error) = result {
                if self
                    .pending_transcription
                    .as_ref()
                    .is_some_and(|selection| selection.model == model)
                {
                    self.pending_transcription = None;
                }
                self.transcription_error = Some(format!(
                    "{}: {error}",
                    crate::transcription_models::definition(model).name
                ));
            }
        }
        let mut start_listener = false;
        if let Some(preparation) = self
            .transcription_preparation
            .take_if(|preparation| preparation.worker.is_finished())
        {
            let was_canceled = preparation.canceled.load(Ordering::Relaxed);
            let result = preparation
                .worker
                .join()
                .unwrap_or_else(|_| Err("model preparation worker stopped unexpectedly".into()));
            if !was_canceled {
                match result {
                    Ok(prepared) => {
                        let selection = prepared.selection.clone();
                        match save_with(|settings| settings.remember_transcription(selection)) {
                            Ok(candidate) => {
                                self.settings = candidate;
                                self.settings_valid = true;
                                self.prepared_transcriber = Some(prepared.transcriber);
                                self.transcription_error = None;
                                self.settings_error = None;
                                self.status = "Disabled".into();
                                start_listener = self.listen_when_ready;
                            }
                            Err(error) => {
                                self.transcription_error = Some(format!(
                                    "Could not save transcription selection: {error:#}"
                                ));
                            }
                        }
                    }
                    Err(error) => {
                        self.transcription_error = Some(error);
                    }
                }
            }
        }
        if self.transcription_preparation.is_none()
            && self.settings_edit.is_none()
            && self
                .pending_transcription
                .as_ref()
                .is_some_and(|selection| !self.model_downloads.contains(selection.model))
        {
            let selection = self
                .pending_transcription
                .take()
                .expect("pending selection exists");
            self.prepare_transcription(selection);
        }
        if start_listener {
            self.start();
        }

        if let Some(listener) = self
            .listener
            .take_if(|listener| listener.worker.is_finished())
        {
            let result = listener.finish();
            match result {
                Ok(()) => self.status = "Disabled".into(),
                Err(error) => {
                    self.status = "Couldn't enable".into();
                    self.error = Some(error);
                }
            }
        }

        self.refresh_settings_edit();

        self.activity.refresh(&mut self.event_reader);
        if let Some(listener) = &mut self.listener {
            listener.observe_session(self.activity.session_started_at);
        }
        if self
            .listener
            .as_ref()
            .is_some_and(|listener| matches!(listener.startup, ListenerStartup::Observed))
            && self.listen_when_ready
            && let Some(status) = self.activity.state_label()
        {
            self.status = status.into();
        }
    }

    fn is_running(&self) -> bool {
        self.listener.is_some()
    }

    fn set_hotkey(&mut self, target: ShortcutTarget, shortcut: DesktopShortcut) -> Result<()> {
        if shortcut.function {
            self.error = Some("The Fn modifier cannot be registered on Linux".into());
            self.cancel_settings_edit();
            return Err(color_eyre::eyre::eyre!(
                "the Fn modifier cannot be registered on Linux"
            ));
        }
        let binding = LinuxHotkey {
            control: shortcut.control,
            alt: shortcut.alt,
            shift: shortcut.shift,
            super_key: shortcut.platform,
            key: if shortcut.key == " " {
                "space".into()
            } else {
                shortcut.key.to_ascii_lowercase()
            },
        };
        if let Some(edit) = &mut self.settings_edit {
            if edit.change == SettingsChange::Capture(target) && edit.worker.is_none() {
                edit.change = SettingsChange::Hotkey(target, binding);
            }
        } else {
            self.begin_settings_edit(SettingsChange::Hotkey(target, binding));
        }
        Ok(())
    }

    fn begin_settings_edit(&mut self, change: SettingsChange) {
        if self.settings_edit.is_some() {
            return;
        }
        if self.transcription_preparation.is_some() {
            self.error = Some("Wait for model preparation before editing shortcut settings".into());
            return;
        }
        // A listener waiting for a model still starts once one is ready.
        let resume = self.listen_when_ready;
        self.stop();
        self.error = None;
        self.settings_edit = Some(SettingsEdit {
            change,
            resume,
            canceled: Arc::new(AtomicBool::new(false)),
            worker: None,
        });
    }

    fn cancel_settings_edit(&mut self) {
        if let Some(edit) = &self.settings_edit {
            edit.canceled.store(true, Ordering::Release);
        }
    }

    /// Adopts the settings the Settings window saved. A running listener
    /// restarts when a setting it captured at start changed; the sound
    /// volume already applied when the settings loaded.
    fn reload_settings(&mut self) -> Result<()> {
        let settings = match AppSettings::load() {
            Ok(settings) => settings,
            Err(error) => {
                self.settings_error = Some(format!("Could not load settings: {error:#}"));
                return Err(error);
            }
        };
        if self.settings_valid && settings == self.settings {
            return Ok(());
        }
        let model_changed = settings.transcription != self.settings.transcription;
        let restart = model_changed
            || settings.dictation_hotkey != self.settings.dictation_hotkey
            || settings.double_tap_lock != self.settings.double_tap_lock
            || settings.shortcut_mode != self.settings.shortcut_mode
            || settings.paste_with_shift != self.settings.paste_with_shift
            || settings.microphone != self.settings.microphone
            || settings.paste_last_hotkey != self.settings.paste_last_hotkey
            || settings.double_tap_only != self.settings.double_tap_only
            || settings.release_microphone_while_idle
                != self.settings.release_microphone_while_idle
            || settings.recording_audio_behavior != self.settings.recording_audio_behavior
            || settings.indicator_position != self.settings.indicator_position
            || settings.voice_action.enabled != self.settings.voice_action.enabled
            || settings.edit_hotkey != self.settings.edit_hotkey;
        if restart && self.is_running() && self.settings_edit.is_none() {
            self.begin_settings_edit(SettingsChange::Reload(Box::new(settings)));
            return Ok(());
        }
        self.settings = settings;
        self.settings_valid = true;
        self.settings_error = None;
        self.apply_output_settings();
        if model_changed && self.listen_when_ready && !self.is_running() {
            self.start();
        }
        Ok(())
    }

    fn capturing_hotkey(&self) -> bool {
        self.settings_edit.as_ref().is_some_and(|edit| {
            !edit.canceled.load(Ordering::Acquire)
                && matches!(edit.change, SettingsChange::Capture(_))
        })
    }

    fn has_pending_work(&self) -> bool {
        self.is_running()
            || self.settings_edit.is_some()
            || self.transcription_preparation.is_some()
            || self.pending_transcription.is_some()
            || !self.model_downloads.is_idle()
    }

    fn refresh_settings_edit(&mut self) {
        if self.is_running() {
            return;
        }
        let Some(edit) = &mut self.settings_edit else {
            return;
        };
        if edit
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            return;
        }
        let canceled = edit.canceled.load(Ordering::Acquire);
        if let Some(worker) = edit.worker.take() {
            match worker.join() {
                // Once persistence completed, keep the in-memory projection consistent even
                // if Cancel raced with the final atomic save.
                Ok(Ok(settings)) => {
                    self.settings = settings;
                    self.settings_valid = true;
                    self.settings_error = None;
                    self.apply_output_settings();
                }
                Ok(Err(error)) if !canceled => {
                    self.error = Some(format!("Could not update dictation settings: {error:#}"));
                }
                Err(_) if !canceled => {
                    self.error = Some("Settings worker stopped unexpectedly".into())
                }
                _ => {}
            }
        } else if !canceled {
            let wayland = LinuxSession::detect().is_wayland();
            if !wayland && matches!(edit.change, SettingsChange::Capture(_)) {
                // X11 uses focused GPUI keystrokes, never raw input-device access.
                self.status = "Press a shortcut".into();
                return;
            }
            let change = edit.change.clone();
            let canceled = edit.canceled.clone();
            edit.worker = Some(std::thread::spawn(move || {
                let (target, binding) = match change {
                    SettingsChange::Capture(target) => (
                        target,
                        crate::linux_input::capture_wayland_binding(&canceled)?,
                    ),
                    SettingsChange::Hotkey(target, binding) => (target, binding),
                    SettingsChange::Reload(settings) => return Ok(*settings),
                };
                binding.validate()?;
                crate::linux_input::LinuxHotkeyMonitor::start(
                    binding.clone(),
                    false,
                    false,
                    Default::default(),
                )?;
                if canceled.load(Ordering::Acquire) {
                    return Err(color_eyre::eyre::eyre!("Shortcut edit canceled"));
                }
                save_with(|settings| {
                    let binding = HotkeyBinding::from(&binding);
                    match target {
                        ShortcutTarget::Dictation => settings.dictation_hotkey = binding,
                        ShortcutTarget::PasteLast => settings.paste_last_hotkey = Some(binding),
                        ShortcutTarget::VoiceAction => settings.edit_hotkey = binding,
                    }
                })
            }));
            self.status = if self.capturing_hotkey() {
                "Press a shortcut"
            } else {
                "Saving settings"
            }
            .into();
            return;
        }
        let resume = self.settings_edit.take().is_some_and(|edit| edit.resume);
        self.status = "Disabled".into();
        if resume {
            self.start();
        }
    }

    fn restart_into_update(&self) -> Result<()> {
        let UpdateState::Ready(update) = &self.update else {
            return Err(color_eyre::eyre::eyre!("no installed update is ready"));
        };
        crate::linux_updater::relaunch(update)
    }

    fn choose_transcription(
        &mut self,
        model: crate::transcription_models::TranscriptionModelId,
        language: String,
    ) -> Result<()> {
        if self.settings_edit.is_some() {
            let error = "Finish the shortcut change before changing the model".to_string();
            self.transcription_error = Some(error.clone());
            return Err(color_eyre::eyre::eyre!(error));
        }
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
        crate::transcription_models::validate(&selection)?;
        self.model_downloads
            .start(model)
            .map_err(|error| color_eyre::eyre::eyre!(error))?;
        if let Some(preparation) = &self.transcription_preparation {
            preparation.canceled.store(true, Ordering::Release);
        }
        self.pending_transcription = Some(selection);
        self.transcription_error = None;
        Ok(())
    }

    fn prepare_transcription(
        &mut self,
        selection: crate::transcription_models::TranscriptionSelection,
    ) {
        let model = selection.model;
        let definition = crate::transcription_models::definition(model);
        // A running listener stops for the swap and starts again with the
        // prepared model.
        if self.is_running() {
            self.stop();
            self.listen_when_ready = true;
        }
        let canceled = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU64::new(0));
        let stage = Arc::new(AtomicU8::new(
            crate::transcription_models::ModelPreparationStage::Downloading as u8,
        ));
        let worker_canceled = canceled.clone();
        let worker_progress = progress.clone();
        let worker_stage = stage.clone();
        let worker = std::thread::spawn(move || {
            (|| {
                crate::transcription_models::verify_installed(definition, &worker_canceled)?;
                worker_progress.store(definition.download_bytes().unwrap_or(0), Ordering::Relaxed);
                if worker_canceled.load(Ordering::Relaxed) {
                    return Err(color_eyre::eyre::eyre!("model activation canceled"));
                }
                crate::transcription_models::ModelPreparationStage::Loading.store(&worker_stage);
                let transcriber = crate::local_transcriber::LocalTranscriber::load(&selection)?;
                if worker_canceled.load(Ordering::Relaxed) {
                    return Err(color_eyre::eyre::eyre!("model activation canceled"));
                }
                Ok(PreparedTranscription {
                    selection,
                    transcriber,
                })
            })()
            .map_err(|error| format!("{error:#}"))
        });
        self.transcription_preparation = Some(TranscriptionPreparation {
            canceled,
            model,
            progress,
            stage,
            worker,
        });
        self.transcription_error = None;
    }

    fn cancel_transcription_model(
        &mut self,
        model: crate::transcription_models::TranscriptionModelId,
    ) {
        self.model_downloads.cancel(model);
        if self
            .pending_transcription
            .as_ref()
            .is_some_and(|selection| selection.model == model)
        {
            self.pending_transcription = None;
        }
        if let Some(preparation) = &self.transcription_preparation
            && preparation.model == model
        {
            preparation.canceled.store(true, Ordering::Release);
        }
        self.transcription_error = None;
    }

    fn cancel_transcription_preparation(&mut self) {
        self.pending_transcription = None;
        for download in self.model_downloads.status() {
            self.model_downloads.cancel(download.model);
        }
        if let Some(preparation) = &self.transcription_preparation {
            preparation.canceled.store(true, Ordering::Relaxed);
        }
        self.transcription_error = None;
    }
}

/// Saves one service-owned change on top of the current `settings.json`, so
/// edits the Settings window saved meanwhile survive.
fn save_with(change: impl FnOnce(&mut AppSettings)) -> Result<AppSettings> {
    let mut settings = AppSettings::load()?;
    change(&mut settings);
    settings.save()?;
    Ok(settings)
}

fn start_update_check() -> Receiver<Result<Option<InstalledUpdate>, String>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = crate::linux_updater::install_latest().map_err(|error| format!("{error:#}"));
        let _ = sender.send(result);
    });
    receiver
}

impl Drop for LinuxDesktopHost {
    fn drop(&mut self) {
        self.stop();
        self.cancel_transcription_preparation();
        if let Some(listener) = self
            .listener
            .take_if(|listener| listener.worker.is_finished())
        {
            let _ = listener.finish();
        }
        if let Some(preparation) = self
            .transcription_preparation
            .take_if(|preparation| preparation.worker.is_finished())
        {
            let _ = preparation.worker.join();
        }
        if let Some(edit) = &mut self.settings_edit
            && let Some(worker) = edit.worker.take_if(|worker| worker.is_finished())
        {
            let _ = worker.join();
        }
    }
}

impl LinuxDesktopHost {
    fn snapshot(&self) -> DesktopSnapshot {
        let update_status = match &self.update {
            UpdateState::Unmanaged => DesktopUpdateStatus::Unavailable,
            UpdateState::Checking(_) => DesktopUpdateStatus::Checking,
            UpdateState::Failed(_) => DesktopUpdateStatus::Failed,
            UpdateState::Waiting(_) => DesktopUpdateStatus::Current,
            UpdateState::Ready(_) => DesktopUpdateStatus::ReadyToRestart,
        };
        DesktopSnapshot {
            activity: self.activity.clone(),
            listener: DesktopListenerSnapshot {
                running: self.is_running(),
                status: self.status.clone(),
                warning: self
                    .listener
                    .as_ref()
                    .and_then(|listener| listener.hotkey_status.warning()),
            },
            operation_error: self
                .error
                .clone()
                .or_else(|| self.settings_error.clone())
                .or_else(|| {
                    self.activity
                        .last_failure
                        .as_ref()
                        .filter(|(at, _)| Some(*at) != self.dismissed_failure_at)
                        .map(|(_, message)| message.clone())
                }),
            transcription: DesktopTranscriptionSnapshot {
                downloaded_bytes: self
                    .transcription_preparation
                    .as_ref()
                    .map_or(0, |preparation| {
                        preparation.progress.load(Ordering::Relaxed)
                    }),
                error: self.transcription_error.clone(),
                preparation_stage: self.transcription_preparation.as_ref().map(|preparation| {
                    crate::transcription_models::ModelPreparationStage::load(&preparation.stage)
                }),
                selection: self.settings.transcription.clone(),
                preparing: self
                    .pending_transcription
                    .as_ref()
                    .map(|selection| selection.model)
                    .or_else(|| {
                        self.transcription_preparation
                            .as_ref()
                            .filter(|preparation| !preparation.canceled.load(Ordering::Acquire))
                            .map(|preparation| preparation.model)
                    }),
                downloads: self.model_downloads.status(),
            },
            update_status,
        }
    }

    fn dispatch(&mut self, action: DesktopAction) -> Result<()> {
        match action {
            DesktopAction::ClearError => {
                self.error = None;
                self.settings_error = None;
                self.dismissed_failure_at = self.activity.last_failure.as_ref().map(|(at, _)| *at);
            }
            DesktopAction::RestartIntoUpdate => {
                if let Err(error) = self.restart_into_update() {
                    self.error = Some(format!("Could not restart Hex: {error:#}"));
                    return Err(error);
                }
            }
            DesktopAction::SetDictationShortcut(shortcut) => {
                self.set_hotkey(ShortcutTarget::Dictation, shortcut)?;
            }
            DesktopAction::StartListening => {
                self.error = None;
                self.dismissed_failure_at = self.activity.last_failure.as_ref().map(|(at, _)| *at);
                self.start();
            }
            DesktopAction::StopListening => self.stop(),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription_models::{
        ModelPreparationStage, TranscriptionModelId, TranscriptionSelection,
    };

    fn host_for_edit(running: bool) -> LinuxDesktopHost {
        let settings = AppSettings {
            transcription: TranscriptionSelection {
                // Prevent the restart assertions from opening a real microphone.
                model: TranscriptionModelId::AppleSpeech,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut host =
            LinuxDesktopHost::new(PathBuf::new(), settings, None, UpdateState::Unmanaged);
        if running {
            host.listener = Some(Listener {
                worker: std::thread::spawn(|| Ok(())),
                stop: Arc::new(AtomicBool::new(false)),
                startup: ListenerStartup::Observed,
                hotkey_status: Default::default(),
            });
            host.listen_when_ready = true;
        }
        host
    }

    fn set_finished_edit(host: &mut LinuxDesktopHost, result: Result<AppSettings>) {
        let worker = std::thread::spawn(move || result);
        while !worker.is_finished() {
            std::thread::yield_now();
        }
        host.settings_edit.as_mut().unwrap().worker = Some(worker);
    }

    #[test]
    fn malformed_settings_cannot_start_even_after_dismissing_the_error() {
        let mut host = LinuxDesktopHost::new(
            PathBuf::new(),
            Default::default(),
            Some("Could not load Linux settings: invalid JSON".into()),
            UpdateState::Unmanaged,
        );
        host.start();
        assert!(!host.is_running());
        assert_eq!(host.status, "Couldn't enable");
        host.dispatch(DesktopAction::ClearError).unwrap();
        host.start();
        assert!(!host.is_running());
        assert_eq!(host.status, "Couldn't enable");
        assert!(!host.settings_valid);

        let mut recovered = host.settings.clone();
        recovered.double_tap_lock = false;
        host.begin_settings_edit(SettingsChange::Reload(Box::new(recovered.clone())));
        set_finished_edit(&mut host, Ok(recovered.clone()));
        host.refresh_settings_edit();
        assert!(host.settings_valid);
        assert_eq!(host.settings, recovered);
        assert!(!host.is_running());
    }

    #[test]
    fn listener_startup_ignores_the_previous_session_until_a_new_one_arrives() {
        for previous_session in [None, Some(10)] {
            let mut listener = Listener {
                stop: Arc::new(AtomicBool::new(false)),
                worker: std::thread::spawn(|| Ok(())),
                startup: ListenerStartup::Awaiting { previous_session },
                hotkey_status: Default::default(),
            };
            listener.observe_session(previous_session);
            assert!(matches!(listener.startup, ListenerStartup::Awaiting { .. }));
            listener.observe_session(Some(20));
            assert!(matches!(listener.startup, ListenerStartup::Observed));
            listener.observe_session(previous_session);
            assert!(matches!(listener.startup, ListenerStartup::Observed));
            listener.stop();
            assert!(listener.stop.load(Ordering::Relaxed));
            listener.finish().unwrap();
        }
    }

    #[test]
    fn listener_warning_is_nonfatal_and_round_trips_in_service_snapshots() {
        let mut host = host_for_edit(true);
        host.status = "Enabled".into();
        let status = host.listener.as_ref().unwrap().hotkey_status.clone();
        status.update_escape_availability(false);
        let snapshot = service_snapshot(&host, false);
        let serialized = serde_json::to_vec(&snapshot).unwrap();
        let received: ServiceSnapshot = serde_json::from_slice(&serialized).unwrap();
        let listener = received.desktop.listener;
        assert!(listener.running);
        assert_eq!(listener.status, "Enabled");
        assert_eq!(
            listener.warning.as_deref(),
            Some(crate::linux_input::ESCAPE_CANCEL_WARNING),
        );
        assert!(received.desktop.operation_error.is_none());

        // Dismissing an unrelated failure must not hide an unresolved limitation.
        host.error = Some("previous transcription failed".into());
        assert_eq!(
            host.snapshot().operation_error.as_deref(),
            Some("previous transcription failed"),
        );
        host.dispatch(DesktopAction::ClearError).unwrap();
        assert!(host.snapshot().listener.warning.is_some());
        assert!(host.snapshot().operation_error.is_none());
        status.update_escape_availability(true);
        assert!(host.snapshot().listener.warning.is_none());
        status.update_escape_availability(false);
        host.stop();
        while !host.listener.as_ref().unwrap().worker.is_finished() {
            std::thread::yield_now();
        }
        host.refresh();
        let listener = host.snapshot().listener;
        assert!(!listener.running);
        assert!(listener.warning.is_none());
    }

    #[test]
    fn listener_completion_waits_for_the_worker_and_preserves_errors() {
        for outcome in [Some(Ok(())), Some(Err("listener failed".into())), None] {
            let expected = outcome
                .clone()
                .unwrap_or_else(|| Err("listener worker stopped unexpectedly".into()))
                .err();
            let mut host = host_for_edit(true);
            let (release, held) = mpsc::channel();
            host.listener.as_mut().unwrap().worker = std::thread::spawn(move || {
                held.recv_timeout(Duration::from_secs(5)).unwrap();
                outcome.expect("fixture listener panic")
            });
            host.refresh();
            assert!(host.is_running());
            assert!(!host.listener.as_ref().unwrap().stop.load(Ordering::Relaxed));
            release.send(()).unwrap();
            while !host.listener.as_ref().unwrap().worker.is_finished() {
                std::thread::yield_now();
            }
            host.refresh();
            assert!(!host.is_running());
            assert!(host.listener.is_none());
            assert_eq!(host.error, expected);
            assert_eq!(
                host.status,
                if expected.is_some() {
                    "Couldn't enable"
                } else {
                    "Disabled"
                }
            );
        }
    }

    #[test]
    fn preparation_completion_preserves_selection_and_honors_cancellation() {
        for (canceled, panics) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut host = host_for_edit(false);
            let original = host.settings.clone();
            let (release, held) = mpsc::channel();
            host.transcription_preparation = Some(TranscriptionPreparation {
                canceled: Arc::new(AtomicBool::new(false)),
                model: TranscriptionModelId::default(),
                progress: Arc::new(AtomicU64::new(0)),
                stage: Arc::new(AtomicU8::new(ModelPreparationStage::Loading as u8)),
                worker: std::thread::spawn(move || {
                    held.recv_timeout(Duration::from_secs(5)).unwrap();
                    assert!(!panics, "fixture preparation panic");
                    Err("model unavailable".into())
                }),
            });
            if canceled {
                host.cancel_transcription_preparation();
            }
            host.refresh();
            assert!(host.has_pending_work());
            release.send(()).unwrap();
            while !host
                .transcription_preparation
                .as_ref()
                .unwrap()
                .worker
                .is_finished()
            {
                std::thread::yield_now();
            }
            host.refresh();
            assert!(!host.has_pending_work());
            assert_eq!(host.settings, original);
            assert_eq!(
                host.transcription_error.as_deref(),
                if canceled {
                    None
                } else if panics {
                    Some("model preparation worker stopped unexpectedly")
                } else {
                    Some("model unavailable")
                }
            );
        }
    }

    #[test]
    fn canceled_activation_preserves_a_newer_download_error() {
        let mut host = host_for_edit(false);
        host.transcription_preparation = Some(TranscriptionPreparation {
            canceled: Arc::new(AtomicBool::new(true)),
            model: TranscriptionModelId::ParakeetV2,
            progress: Arc::new(AtomicU64::new(0)),
            stage: Arc::new(AtomicU8::new(ModelPreparationStage::Loading as u8)),
            worker: std::thread::spawn(|| Err("old activation canceled".into())),
        });
        host.transcription_error = Some("newer download failed".into());
        let deadline = Instant::now() + Duration::from_secs(3);
        while host.transcription_preparation.is_some() {
            host.refresh();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(
            host.transcription_error.as_deref(),
            Some("newer download failed")
        );
    }

    #[test]
    fn preference_edits_wait_for_stop_and_only_restart_a_previously_running_listener() {
        for running in [false, true] {
            let mut host = host_for_edit(running);
            let mut candidate = host.settings.clone();
            candidate.paste_with_shift = !candidate.paste_with_shift;
            host.begin_settings_edit(SettingsChange::Reload(Box::new(candidate.clone())));
            assert_eq!(host.settings_edit.as_ref().unwrap().resume, running);
            if running {
                host.refresh_settings_edit();
                assert!(host.listener.as_ref().unwrap().stop.load(Ordering::Relaxed));
                assert!(host.settings_edit.as_ref().unwrap().worker.is_none());
                assert_ne!(host.settings, candidate);
            }
            host.listener = None;
            set_finished_edit(&mut host, Ok(candidate.clone()));
            host.refresh_settings_edit();
            assert_eq!(host.settings, candidate);
            assert!(host.settings_edit.is_none());
            assert_eq!(host.listen_when_ready, running);
        }
    }

    #[test]
    fn canceled_or_failed_capture_restores_prior_listening_without_changing_settings() {
        for running in [false, true] {
            for cancel in [false, true] {
                let mut host = host_for_edit(running);
                let original = host.settings.clone();
                host.begin_settings_edit(SettingsChange::Capture(ShortcutTarget::Dictation));
                assert!(host.capturing_hotkey());
                host.listener = None;
                if cancel {
                    host.cancel_settings_edit();
                } else {
                    set_finished_edit(
                        &mut host,
                        Err(color_eyre::eyre::eyre!("input permission denied")),
                    );
                }
                host.refresh_settings_edit();
                assert_eq!(host.settings, original);
                assert_eq!(host.listen_when_ready, running);
                assert_eq!(host.error.is_some(), !cancel);
                assert!(host.settings_edit.is_none());
                assert!(!host.capturing_hotkey());
            }
        }
    }

    #[test]
    fn cancel_signals_worker_and_does_not_wait_or_restart_until_it_finishes() {
        let mut host = host_for_edit(true);
        host.begin_settings_edit(SettingsChange::Capture(ShortcutTarget::Dictation));
        host.listener = None;
        let (release, held) = mpsc::channel();
        host.settings_edit.as_mut().unwrap().worker = Some(std::thread::spawn(move || {
            let _ = held.recv();
            Err(color_eyre::eyre::eyre!("capture canceled"))
        }));
        host.cancel_settings_edit();
        host.refresh_settings_edit();
        let edit = host.settings_edit.as_mut().unwrap();
        assert!(edit.canceled.load(Ordering::Acquire));
        assert!(edit.worker.is_some());
        assert!(!host.listen_when_ready);
        release.send(()).unwrap();
        let _ = host
            .settings_edit
            .as_mut()
            .unwrap()
            .worker
            .take()
            .unwrap()
            .join()
            .unwrap()
            .unwrap_err();
        host.refresh_settings_edit();
        assert!(host.listen_when_ready);
        assert!(host.settings_edit.is_none());
    }

    #[test]
    fn client_disconnect_cancels_its_edit_and_restores_previous_listening() {
        let mut host = host_for_edit(true);
        host.begin_settings_edit(SettingsChange::Capture(ShortcutTarget::Dictation));
        let mut editor = Some((1, Instant::now()));
        disconnect_editor(&mut host, &mut editor, 2);
        assert!(
            !host
                .settings_edit
                .as_ref()
                .unwrap()
                .canceled
                .load(Ordering::Acquire)
        );
        disconnect_editor(&mut host, &mut editor, 1);
        let edit = host.settings_edit.as_ref().unwrap();
        assert!(edit.resume);
        assert!(edit.canceled.load(Ordering::Acquire));
        host.listener = None;
        host.refresh_settings_edit();
        assert!(host.listen_when_ready);
        assert!(editor.is_none());
    }

    #[test]
    fn model_downloads_survive_client_close_and_cancel_independently() {
        let mut host = host_for_edit(true);
        let original = host.settings.transcription.clone();
        let (started, starts) = mpsc::channel();
        let (release_a, releases_a) = mpsc::channel();
        let (release_b, releases_b) = mpsc::channel();
        let releases_a = std::sync::Mutex::new(releases_a);
        let releases_b = std::sync::Mutex::new(releases_b);
        let a = TranscriptionModelId::ParakeetV2;
        let b = TranscriptionModelId::ParakeetV3;
        host.model_downloads = crate::model_downloads::ModelDownloads::with_download(Arc::new(
            move |model, canceled, progress| {
                progress.store(123, Ordering::Relaxed);
                started.send(model).unwrap();
                if model == a {
                    releases_a.lock().unwrap().recv().unwrap();
                    assert!(canceled.load(Ordering::Acquire));
                } else {
                    releases_b.lock().unwrap().recv().unwrap();
                    assert!(!canceled.load(Ordering::Acquire));
                }
                Err("fixture download failed".into())
            },
        ));
        host.choose_transcription(a, "en".into()).unwrap();
        assert_eq!(starts.recv_timeout(Duration::from_secs(3)).unwrap(), a);
        host.choose_transcription(b, "fr".into()).unwrap();
        assert_eq!(starts.recv_timeout(Duration::from_secs(3)).unwrap(), b);
        disconnect_editor(&mut host, &mut None, 1);
        let snapshot = service_snapshot(&host, false);
        let received: ServiceSnapshot =
            serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
        assert_eq!(received.desktop.transcription.downloads.len(), 2);
        assert_eq!(received.desktop.transcription.preparing, Some(b));
        assert!(!host.listener.as_ref().unwrap().stop.load(Ordering::Acquire));
        host.cancel_transcription_model(a);
        assert_eq!(host.pending_transcription.as_ref().unwrap().model, b);
        release_a.send(()).unwrap();
        release_b.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !host.model_downloads.is_idle() {
            host.refresh();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(host.settings.transcription, original);
        assert!(
            host.transcription_error
                .as_deref()
                .unwrap()
                .contains("fixture download failed")
        );
        assert!(host.pending_transcription.is_none());
        assert!(host.transcription_preparation.is_none());
        assert!(!host.listener.as_ref().unwrap().stop.load(Ordering::Acquire));
    }

    #[test]
    fn closing_a_client_does_not_stop_normal_dictation() {
        let mut host = host_for_edit(true);
        disconnect_editor(&mut host, &mut None, 1);
        assert!(host.is_running());
        assert!(host.listen_when_ready);
        assert!(!host.listener.as_ref().unwrap().stop.load(Ordering::Acquire));
    }

    #[test]
    fn dismiss_clears_operational_and_settings_errors() {
        let mut host = host_for_edit(false);
        host.error = Some("listener failed".into());
        host.settings_error = Some("settings failed".into());
        host.dispatch(DesktopAction::ClearError).unwrap();
        assert!(host.snapshot().operation_error.is_none());
    }

    #[test]
    fn output_failures_are_visible_and_dismissal_only_acknowledges_that_failure() {
        let mut host = host_for_edit(false);
        host.activity.last_failure = Some((10, "paste helper failed".into()));
        assert_eq!(
            host.snapshot().operation_error.as_deref(),
            Some("paste helper failed")
        );
        host.dispatch(DesktopAction::ClearError).unwrap();
        assert!(host.snapshot().operation_error.is_none());
        host.activity.last_failure = Some((11, "paste helper failed".into()));
        assert_eq!(
            host.snapshot().operation_error.as_deref(),
            Some("paste helper failed")
        );
        host.dispatch(DesktopAction::StartListening).unwrap();
        assert!(host.snapshot().operation_error.is_none());
    }

    #[test]
    fn start_during_preparation_defers_listening_and_stop_clears_the_request() {
        let settings = AppSettings {
            // An unavailable model prevents a regressed guard from opening audio.
            transcription: TranscriptionSelection {
                model: TranscriptionModelId::AppleSpeech,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut host =
            LinuxDesktopHost::new(PathBuf::new(), settings, None, UpdateState::Unmanaged);
        host.transcription_preparation = Some(TranscriptionPreparation {
            canceled: Arc::new(AtomicBool::new(false)),
            model: TranscriptionModelId::default(),
            progress: Arc::new(AtomicU64::new(0)),
            stage: Arc::new(AtomicU8::new(ModelPreparationStage::Downloading as u8)),
            worker: std::thread::spawn(|| Err("fixture preparation".into())),
        });

        host.dispatch(DesktopAction::StartListening).unwrap();

        assert!(host.listen_when_ready);
        assert_eq!(host.status, "Disabled");
        assert!(!host.is_running());
        assert!(host.listener.is_none());
        assert!(host.transcription_preparation.is_some());

        host.dispatch(DesktopAction::StopListening).unwrap();

        assert!(!host.listen_when_ready);
        assert!(host.transcription_preparation.is_some());
    }
}

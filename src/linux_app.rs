//! The one Linux Hex process: the dictation host (listener, shortcut
//! registration, model preparation, updates, History writes, paste), the
//! single-instance socket, and the window and HUD, opened on demand and
//! closed without stopping anything.
//!
//! Threads: the main thread runs Rune's event loop; the host thread
//! refreshes [`LinuxDesktopHost`] every 20 ms and publishes its status;
//! the socket's workers answer `hex` commands from that published status
//! and hand shortcut commands straight to the listener.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use color_eyre::Result;

use crate::app_settings::{AppSettings, HotkeyBinding};
use crate::control::{CaptureAction, ControlError, ControlReply, Pane};
use crate::desktop_activity::DesktopActivity;
use crate::desktop_host::{
    DesktopAction, DesktopListenerSnapshot, DesktopShortcut, DesktopSnapshot,
    DesktopTranscriptionSnapshot,
};
use crate::events::EventReader;
use crate::linux_dictation::ListenerUpdate;
use crate::linux_input::{LinuxHotkey, ListenerCommand, ShortcutCommand};
use crate::linux_service::{self, Activation, Control, Published, ShortcutRoute, Status};

const REFRESH_INTERVAL: Duration = Duration::from_millis(20);
/// How often the host asks X11 for the active keyboard group.
const INPUT_SOURCE_POLL: Duration = Duration::from_millis(250);
/// Left by "Update" so the restarted process shows the window again; a
/// crash restart finds none and stays in the background.
const REOPEN_MARKER: &str = "reopen-window";

type ListenerResult = std::result::Result<(), String>;
type PreparationResult = std::result::Result<PreparedTranscription, String>;
pub(crate) type Waker = Arc<dyn Fn() + Send + Sync>;

/// A shortcut Settings records for the listener.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ShortcutTarget {
    #[default]
    Dictation,
    PasteLast,
    Capture,
}

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
    /// A keyboard switch, which a manual choice or a later layout change
    /// supersedes.
    automatic: bool,
}

struct ShortcutEdit {
    target: ShortcutTarget,
    hotkey: Option<LinuxHotkey>,
    canceled: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<AppSettings>>>,
}

struct Listener {
    stop: Arc<AtomicBool>,
    worker: JoinHandle<ListenerResult>,
    startup: ListenerStartup,
    hotkey_status: crate::linux_input::HotkeyStatus,
    /// `hex` commands for the listener's shortcut state machine.
    commands: mpsc::SyncSender<ListenerCommand>,
    /// Settings, models, and shortcut suspension the listener applies in place.
    updates: mpsc::Sender<crate::linux_dictation::ListenerUpdate>,
}

enum ListenerStartup {
    Awaiting { previous_session: Option<u64> },
    Observed,
}

impl Listener {
    fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// A listener that already stopped has nothing to apply.
    fn update(&self, update: crate::linux_dictation::ListenerUpdate) {
        let _ = self.updates.send(update);
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
    shortcut_edit: Option<ShortcutEdit>,
    prepared_transcriber: Option<crate::local_transcriber::LocalTranscriber>,
    transcription_preparation: Option<TranscriptionPreparation>,
    transcription_error: Option<String>,
    updater: crate::updater::Updater,
    output: crate::linux_dictation::DictationOutput,
    /// Hosts the personal transformations workspace while a mode uses one.
    personal_commands: Option<crate::personal_commands::PersonalCommands>,
    personal_commands_checked_at: Instant,
    keyboard_switcher: crate::keyboard_model_switching::Switcher,
    /// The XKB connection, only while keyboard model switching is on.
    input_sources: Option<crate::keyboard_input_sources::Watcher>,
    input_sources_polled_at: Instant,
    /// A due keyboard switch waiting for the preparation it superseded to stop.
    keyboard_switch: Option<crate::transcription_models::TranscriptionSelection>,
}

/// What the window shows of the host.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HostSnapshot {
    pub(crate) desktop: DesktopSnapshot,
    pub(crate) settings: AppSettings,
    /// The host is recording or saving a shortcut.
    pub(crate) editing: bool,
}

/// What the window asks of the host. Settings edits go through
/// [`HostHandle::save_settings`].
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum HostRequest {
    Desktop(DesktopAction),
    /// Stops listening while the window records a shortcut.
    CaptureShortcutFor(ShortcutTarget),
    CancelShortcut,
    /// The chord the window recorded, for the host to register and save.
    SetShortcut {
        target: ShortcutTarget,
        shortcut: DesktopShortcut,
    },
    ChooseModel {
        model: crate::transcription_models::TranscriptionModelId,
        language: String,
    },
    CancelModel,
}

struct Shared {
    host: Mutex<LinuxDesktopHost>,
    waker: Mutex<Option<Waker>>,
    /// Whether the window shows, as its app reports it.
    window_visible: Mutex<Option<WindowVisible>>,
    shutdown: &'static AtomicBool,
}

type WindowVisible = Arc<dyn Fn() -> bool + Send + Sync>;

/// The window's handle to the host it runs beside. Every call is a short
/// lock on the host; nothing here waits for the listener or a model.
#[derive(Clone)]
pub(crate) struct HostHandle(Arc<Shared>);

impl HostHandle {
    fn new(host: LinuxDesktopHost, shutdown: &'static AtomicBool) -> Self {
        Self(Arc::new(Shared {
            host: Mutex::new(host),
            waker: Mutex::new(None),
            window_visible: Mutex::new(None),
            shutdown,
        }))
    }

    /// The window's "Quit Hex": drains accepted work, then exits.
    pub(crate) fn quit(&self) {
        self.0.shutdown.store(true, Ordering::Release);
    }

    fn lock(&self) -> MutexGuard<'_, LinuxDesktopHost> {
        self.0
            .host
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    pub(crate) fn snapshot(&self) -> HostSnapshot {
        self.lock().host_snapshot()
    }

    pub(crate) fn request(&self, request: HostRequest) -> Result<()> {
        let result = {
            let mut host = self.lock();
            match request {
                HostRequest::Desktop(action) => host.dispatch(action),
                HostRequest::CaptureShortcutFor(target) => {
                    host.begin_shortcut_edit(target, None);
                    Ok(())
                }
                HostRequest::CancelShortcut => {
                    host.cancel_shortcut_edit();
                    Ok(())
                }
                HostRequest::SetShortcut { target, shortcut } => host.set_hotkey(target, shortcut),
                HostRequest::ChooseModel { model, language } => {
                    host.choose_transcription(model, language)
                }
                HostRequest::CancelModel => {
                    host.manual_choice();
                    host.cancel_transcription_preparation();
                    Ok(())
                }
            }
        };
        self.wake();
        result
    }

    /// Saves the window's settings, then adopts them. A failed save leaves
    /// the host's settings alone.
    pub(crate) fn save_settings(&self, settings: AppSettings) -> Result<()> {
        settings.save()?;
        self.lock().adopt_settings(settings);
        self.wake();
        Ok(())
    }

    /// The host's retained-history store; one store per process.
    pub(crate) fn history(&self) -> Option<crate::history::History> {
        self.lock().output.history.clone()
    }

    /// The window wakes on host changes through `waker`.
    pub(crate) fn set_waker(&self, waker: Waker) {
        *self.0.waker.lock().unwrap() = Some(waker);
    }

    /// `hex status` asks `visible` whether the window shows.
    pub(crate) fn set_window_visible(&self, visible: WindowVisible) {
        *self.0.window_visible.lock().unwrap() = Some(visible);
    }

    fn window_open(&self) -> bool {
        let visible = self.0.window_visible.lock().unwrap().clone();
        visible.is_some_and(|visible| visible())
    }

    /// A closed window leaves no shortcut capture behind.
    pub(crate) fn window_closed(&self) {
        let mut host = self.lock();
        if host.capturing_hotkey() {
            host.cancel_shortcut_edit();
        }
    }

    fn wake(&self) {
        let waker = self.0.waker.lock().unwrap().clone();
        if let Some(waker) = waker {
            waker();
        }
    }

    fn status(&self, snapshot: HostSnapshot, session: linux_service::Session) -> Status {
        let hud_limitation = crate::linux_rune_hud::limitation();
        let mut desktop = snapshot.desktop;
        // A running listener also reports the missing HUD as its warning.
        if desktop.listener.running && desktop.listener.warning.is_none() {
            desktop.listener.warning = hud_limitation.clone();
        }
        Status {
            desktop,
            labs: crate::labs::active_names(&snapshot.settings),
            settings: snapshot.settings,
            editing: snapshot.editing,
            hud_limitation,
            window_open: self.window_open(),
            pid: std::process::id(),
            session,
        }
    }
}

/// Becomes the Linux instance: claims the data root, starts the host, and
/// runs the window's event loop on this (the main) thread until `hex quit`,
/// a signal, or the window's Quit. `activation` opens the window at once.
pub(crate) fn run(
    event_path: PathBuf,
    shutdown: &'static AtomicBool,
    activation: Option<Activation>,
) -> Result<()> {
    let mut server = linux_service::Server::bind()?;
    // Installs before #579 named the entries hex.desktop.
    if let Err(error) = crate::linux_portal::tidy_desktop_entries() {
        tracing::warn!(%error, "could not move Hex's desktop entry");
    }
    if let Err(error) = crate::linux_login_item::move_legacy_entry() {
        tracing::warn!(%error, "could not move Hex's autostart entry");
    }
    crate::labs::log_launch();
    crate::hex_microphone::host_service();
    let (settings, error) = match AppSettings::load() {
        Ok(settings) => (settings, None),
        Err(error) => (
            AppSettings::default(),
            Some(format!("Could not load settings: {error:#}")),
        ),
    };
    crate::linux_dictation::initialize_feedback(&settings);
    // Before any thread can load a model or the window can create its GPU.
    crate::local_transcriber::initialize_backends();
    let (control_calls, control_receiver) = mpsc::sync_channel(8);
    let local_api = crate::local_api::LocalApi::start_for_app(&event_path, control_calls);
    let enable_on_launch = settings.enable_on_launch;
    let mut host = LinuxDesktopHost::new(
        event_path,
        settings,
        error,
        crate::updater::Updater::start(),
    );
    if enable_on_launch {
        host.start();
    }
    let host = HostHandle::new(host, shutdown);
    let session = linux_service::Session::current();
    let published = Arc::new(Published::default());
    *published.status.lock().unwrap() = Some(host.status(host.snapshot(), session.clone()));
    let (control, requests) = mpsc::sync_channel(8);
    crate::control::host::serve(
        control_receiver,
        Arc::new(LinuxControl {
            host: host.clone(),
            published: Arc::downgrade(&published),
            window: control.clone(),
        }),
    );
    server.serve(published.clone(), control.clone());
    let tray = crate::linux_tray::start(host.clone(), published.clone(), control.clone(), shutdown)
        .map(Arc::new);
    linux_service::notify("READY=1");
    tracing::info!(pid = std::process::id(), "Hex instance ready");

    let worker = {
        let host = host.clone();
        let tray = tray.clone();
        std::thread::Builder::new()
            .name("hex-host".into())
            .spawn(move || {
                run_host(
                    host,
                    published,
                    server,
                    control,
                    session,
                    tray.as_deref(),
                    shutdown,
                )
            })?
    };
    let reopen = take_reopen_marker();
    let first = activation.or(reopen.then(Activation::default));
    run_window(&host, requests, first, shutdown);
    shutdown.store(true, Ordering::Release);
    let _ = worker.join();
    drop(local_api);
    drop(tray);
    Ok(())
}

/// The host thread: refreshes the host, publishes what the socket answers
/// with, wakes the window on changes, and drains accepted work on quit.
fn run_host(
    host: HostHandle,
    published: Arc<Published>,
    server: linux_service::Server,
    control: mpsc::SyncSender<Control>,
    session: linux_service::Session,
    tray: Option<&rune::Tray>,
    shutdown: &'static AtomicBool,
) {
    let mut stopping = false;
    let mut shown: Option<HostSnapshot> = None;
    let mut tray_view = None;
    let mut tray_meetings = None;
    loop {
        let (done, route, snapshot) = {
            let mut guard = host.lock();
            if shutdown.load(Ordering::Acquire) && !stopping {
                stopping = true;
                linux_service::notify("STOPPING=1");
                guard.stop();
                guard.cancel_transcription_preparation();
                // The main thread may be waiting for the next window request.
                let _ = control.try_send(Control::Quit);
            }
            guard.refresh();
            (
                stopping && !guard.has_pending_work(),
                guard.shortcut_route(),
                guard.host_snapshot(),
            )
        };
        *published.shortcuts.lock().unwrap() = route;
        *published.status.lock().unwrap() = Some(host.status(snapshot.clone(), session.clone()));
        let meetings = crate::meeting_runtime::tray_state();
        if shown.as_ref() != Some(&snapshot) || tray_meetings != meetings {
            tray_meetings = meetings;
            if let Some(tray) = tray {
                let view = crate::linux_tray::view(&snapshot);
                if tray_view.as_ref() != Some(&view) {
                    crate::linux_tray::show(tray, &view);
                    tray_view = Some(view);
                }
            }
            shown = Some(snapshot);
            host.wake();
        }
        if done {
            break;
        }
        std::thread::sleep(REFRESH_INTERVAL);
    }
    *published.shortcuts.lock().unwrap() = None;
    drop(server);
}

/// The one seam between the process and its window, on the main thread
/// until Hex quits. `first` opens the window at once. A missing display or
/// GPU fails the window, never Hex.
fn run_window(
    host: &HostHandle,
    requests: Receiver<Control>,
    first: Option<Activation>,
    shutdown: &'static AtomicBool,
) {
    let app = host.clone();
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::rune_ui::run_app(app, requests, first, shutdown)
    }))
    .is_err()
    {
        tracing::error!("the Hex window failed; Hex keeps running without it");
        while !shutdown.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn reopen_marker() -> Option<PathBuf> {
    crate::app_paths::support_dir()
        .ok()
        .map(|directory| directory.join(REOPEN_MARKER))
}

/// True when "Update" restarted this process with its window open.
fn take_reopen_marker() -> bool {
    let Some(path) = reopen_marker() else {
        return false;
    };
    let fresh = std::fs::metadata(&path)
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| {
            modified
                .elapsed()
                .is_ok_and(|age| age < Duration::from_secs(60))
        });
    let _ = std::fs::remove_file(&path);
    fresh
}

impl LinuxDesktopHost {
    fn new(
        event_path: PathBuf,
        settings: AppSettings,
        error: Option<String>,
        updater: crate::updater::Updater,
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
            shortcut_edit: None,
            prepared_transcriber: None,
            transcription_preparation: None,
            transcription_error: None,
            updater,
            output,
            personal_commands: None,
            personal_commands_checked_at: Instant::now(),
            keyboard_switcher,
            input_sources: None,
            input_sources_polled_at: Instant::now(),
            keyboard_switch: None,
        };
        host.refresh_personal_commands();
        host
    }

    /// Applies saved settings the host owns: sounds, modes, history
    /// retention, and the transformations workspace.
    fn apply_output_settings(&mut self) {
        crate::feedback::set_enabled(self.settings.sound_effects);
        crate::feedback::set_volume(self.settings.sound_effect_volume.clamp(0.0, 1.0));
        crate::feedback::set_sound_set(self.settings.sound_theme);
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
                    self.output.transformations.clone(),
                );
            }
            (false, true) => self.personal_commands = None,
            _ => {}
        }
    }

    fn start(&mut self) {
        if self.shortcut_edit.is_some() {
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
        let (commands, command_receiver) = mpsc::sync_channel(16);
        let (updates, update_receiver) = mpsc::channel();
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
                    command_receiver.into(),
                    update_receiver,
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
            commands,
            updates,
        });
        self.status = "Enabling…".into();
    }

    fn stop(&mut self) {
        self.listen_when_ready = false;
        if let Some(edit) = &self.shortcut_edit {
            edit.canceled.store(true, Ordering::Release);
        }
        if let Some(listener) = &self.listener {
            listener.stop();
            self.status = "Disabling…".into();
        }
    }

    fn refresh(&mut self) {
        self.switch_with_keyboard();
        // Restart to update asks systemd to restart Hex after the updater's
        // final check; only a failure needs showing.
        if let Some(Err(error)) = self.updater.take_restart() {
            self.error = Some(format!("Could not restart Hex: {error:#}"));
        }
        // Settings may set up the transformations workspace at any time.
        if self.personal_commands_checked_at.elapsed() >= Duration::from_secs(2) {
            self.personal_commands_checked_at = Instant::now();
            self.refresh_personal_commands();
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
            if was_canceled {
                self.transcription_error = None;
            } else {
                match result {
                    Ok(prepared) => {
                        let selection = prepared.selection.clone();
                        match save_with(|settings| settings.remember_transcription(selection)) {
                            Ok(candidate) => {
                                self.settings = candidate;
                                self.settings_valid = true;
                                self.transcription_error = None;
                                self.settings_error = None;
                                // A running listener swaps the model in place.
                                if let Some(listener) = &self.listener {
                                    listener.update(ListenerUpdate::Transcriber {
                                        selection: prepared.selection,
                                        transcriber: Box::new(prepared.transcriber),
                                    });
                                    listener.update(ListenerUpdate::Settings(Box::new(
                                        self.settings.clone(),
                                    )));
                                } else {
                                    self.prepared_transcriber = Some(prepared.transcriber);
                                    start_listener = self.listen_when_ready;
                                }
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
        if start_listener {
            self.start();
        }

        if let Some(listener) = self
            .listener
            .take_if(|listener| listener.worker.is_finished())
        {
            match listener.finish() {
                Ok(()) => self.status = "Disabled".into(),
                Err(error) => {
                    self.status = "Couldn't enable".into();
                    self.error = Some(error);
                }
            }
        }

        self.refresh_shortcut_edit();

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
        let binding = match LinuxHotkey::try_from(shortcut) {
            Ok(binding) => binding,
            Err(error) => {
                self.error = Some("The Fn modifier cannot be registered on Linux".into());
                self.cancel_shortcut_edit();
                return Err(error);
            }
        };
        if let Some(edit) = &mut self.shortcut_edit {
            if edit.target == target && edit.hotkey.is_none() && edit.worker.is_none() {
                edit.hotkey = Some(binding);
            }
        } else {
            self.begin_shortcut_edit(target, Some(binding));
        }
        Ok(())
    }

    fn begin_shortcut_edit(&mut self, target: ShortcutTarget, hotkey: Option<LinuxHotkey>) {
        if self.shortcut_edit.is_some() {
            return;
        }
        if self.transcription_preparation.is_some() {
            self.error = Some("Wait for model preparation before editing shortcut settings".into());
            return;
        }
        // Global matching stops while the window records; listening goes on.
        if let Some(listener) = &self.listener {
            listener.update(ListenerUpdate::SuspendShortcuts(true));
        }
        self.error = None;
        self.shortcut_edit = Some(ShortcutEdit {
            target,
            hotkey,
            canceled: Arc::new(AtomicBool::new(false)),
            worker: None,
        });
    }

    fn cancel_shortcut_edit(&mut self) {
        if let Some(edit) = &self.shortcut_edit {
            edit.canceled.store(true, Ordering::Release);
        }
    }

    /// Adopts settings the window saved. A running listener applies them in
    /// place, so listening never stops for a settings change.
    fn adopt_settings(&mut self, settings: AppSettings) {
        if self.settings_valid && settings == self.settings {
            return;
        }
        if self
            .keyboard_switcher
            .configure(settings.keyboard_model_switching.clone(), Instant::now())
        {
            self.supersede_automatic();
        }
        let was_valid = self.settings_valid;
        self.settings = settings;
        self.settings_valid = true;
        self.settings_error = None;
        self.apply_output_settings();
        if let Some(listener) = &self.listener {
            listener.update(ListenerUpdate::Settings(Box::new(self.settings.clone())));
        } else if self.listen_when_ready && !was_valid {
            self.start();
        }
    }

    fn capturing_hotkey(&self) -> bool {
        self.shortcut_edit
            .as_ref()
            .is_some_and(|edit| !edit.canceled.load(Ordering::Acquire) && edit.hotkey.is_none())
    }

    fn has_pending_work(&self) -> bool {
        self.is_running()
            || self.shortcut_edit.is_some()
            || self.transcription_preparation.is_some()
    }

    fn refresh_shortcut_edit(&mut self) {
        let Some(edit) = &mut self.shortcut_edit else {
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
            let Some(binding) = edit.hotkey.clone() else {
                // The window records the chord from its own key events.
                return;
            };
            let target = edit.target;
            let canceled = edit.canceled.clone();
            edit.worker = Some(std::thread::spawn(move || {
                binding.validate()?;
                if crate::linux_session::LinuxSession::detect().is_wayland() {
                    // The listener asks the desktop again and reports what it granted.
                    binding.portal_trigger()?;
                } else {
                    // Fail before saving when another X11 client holds the chord.
                    crate::linux_input::LinuxHotkeyMonitor::start(
                        binding.clone(),
                        Default::default(),
                        Default::default(),
                        mpsc::channel().1.into(),
                    )?;
                }
                if canceled.load(Ordering::Acquire) {
                    return Err(color_eyre::eyre::eyre!("Shortcut edit canceled"));
                }
                save_with(|settings| {
                    let binding = HotkeyBinding::from(&binding);
                    match target {
                        ShortcutTarget::Dictation => settings.dictation_hotkey = binding,
                        ShortcutTarget::PasteLast => settings.paste_last_hotkey = Some(binding),
                        ShortcutTarget::Capture => settings.capture.hotkey = binding,
                    }
                })
            }));
            return;
        }
        self.shortcut_edit = None;
        // The listener registers the saved (or unchanged) shortcuts again.
        if let Some(listener) = &self.listener {
            listener.update(ListenerUpdate::Settings(Box::new(self.settings.clone())));
            listener.update(ListenerUpdate::SuspendShortcuts(false));
        }
    }

    fn choose_transcription(
        &mut self,
        model: crate::transcription_models::TranscriptionModelId,
        language: String,
    ) -> Result<()> {
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
        self.prepare_transcription(selection, false)
    }

    /// Downloads (or, for a keyboard switch, only verifies) and loads a
    /// model off the host thread.
    fn prepare_transcription(
        &mut self,
        selection: crate::transcription_models::TranscriptionSelection,
        automatic: bool,
    ) -> Result<()> {
        if self.shortcut_edit.is_some() {
            let error = "Finish the shortcut change before changing the model".to_string();
            self.transcription_error = Some(error.clone());
            return Err(color_eyre::eyre::eyre!(error));
        }
        if self.transcription_preparation.is_some() {
            self.cancel_transcription_preparation();
            let error = "The previous model preparation is still stopping".to_string();
            self.transcription_error = Some(error.clone());
            return Err(color_eyre::eyre::eyre!(error));
        }
        let model = selection.model;
        let definition = crate::transcription_models::validate(&selection)?;
        // A running listener keeps the current model until the new one is ready.
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
                if automatic {
                    crate::transcription_models::verify_installed(definition, &worker_canceled)?;
                } else {
                    crate::transcription_models::download_with_stage_progress(
                        definition,
                        &worker_canceled,
                        &worker_progress,
                        &worker_stage,
                    )?;
                }
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
            automatic,
        });
        self.transcription_error = None;
        Ok(())
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
        self.supersede_automatic();
    }

    /// Cancels only keyboard-switch work; a manual operation is superseded
    /// once the debounced choice is due.
    fn supersede_automatic(&mut self) {
        self.keyboard_switch = None;
        if let Some(preparation) = &self.transcription_preparation
            && preparation.automatic
        {
            preparation.canceled.store(true, Ordering::Relaxed);
        }
    }

    /// Follows the XKB group while switching is on, and prepares a
    /// mapping's installed model once it is due.
    fn switch_with_keyboard(&mut self) {
        let enabled = self.settings.keyboard_model_switching.enabled;
        if !enabled {
            self.input_sources = None;
        } else if self.input_sources_polled_at.elapsed() >= INPUT_SOURCE_POLL {
            self.input_sources_polled_at = Instant::now();
            if self.input_sources.is_none() {
                self.input_sources = crate::keyboard_input_sources::Watcher::connect();
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
        }
        if let Some(choice) = self.keyboard_switcher.take_due(Instant::now()) {
            let selection = crate::transcription_models::TranscriptionSelection {
                language: choice.language,
                ..self.settings.transcription_for_model(choice.model)
            };
            // Whatever runs is superseded; the current model and language
            // need no preparation.
            if let Some(preparation) = &self.transcription_preparation {
                preparation.canceled.store(true, Ordering::Relaxed);
            }
            self.keyboard_switch = (selection.model != self.settings.transcription.model
                || selection.language != self.settings.transcription.language)
                .then_some(selection);
        }
        if self.transcription_preparation.is_none()
            && self.shortcut_edit.is_none()
            && let Some(selection) = self.keyboard_switch.take()
            && let Err(error) = self.prepare_transcription(selection, true)
        {
            self.transcription_error = Some(format!("{error:#}"));
        }
    }

    fn cancel_transcription_preparation(&mut self) {
        if let Some(preparation) = &self.transcription_preparation {
            preparation.canceled.store(true, Ordering::Relaxed);
        }
        self.transcription_error = None;
    }
}

/// Saves one host-owned change on top of the current `settings.json`, so
/// edits the window saved meanwhile survive.
fn save_with(change: impl FnOnce(&mut AppSettings)) -> Result<AppSettings> {
    let mut settings = AppSettings::load()?;
    change(&mut settings);
    settings.save()?;
    Ok(settings)
}

impl Drop for LinuxDesktopHost {
    fn drop(&mut self) {
        self.updater.stop();
        self.stop();
        self.cancel_transcription_preparation();
    }
}

/// The Linux adapter `hex` control commands from the local API run through
/// ([`crate::control::host`]).
struct LinuxControl {
    host: HostHandle,
    published: std::sync::Weak<Published>,
    window: mpsc::SyncSender<Control>,
}

impl LinuxControl {
    fn published(&self) -> std::result::Result<Arc<Published>, ControlError> {
        self.published
            .upgrade()
            .ok_or_else(|| ControlError::new("unavailable", "Hex is quitting"))
    }

    fn route(&self) -> std::result::Result<ShortcutRoute, ControlError> {
        self.published()?
            .shortcuts
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| {
                ControlError::new(
                    "not-ready",
                    "Hex is not listening; enable it in Settings or run `hex start`",
                )
            })
    }

    fn shortcut(&self, command: ShortcutCommand) -> std::result::Result<(), ControlError> {
        linux_service::route_shortcut(Some(&self.route()?), command)
            .map_err(|error| ControlError::new("busy", format!("{error:#}")))
    }

    fn window(&self, control: Control) -> std::result::Result<(), ControlError> {
        self.window
            .try_send(control)
            .map_err(|_| ControlError::new("busy", "Hex is busy; try again"))
    }
}

impl crate::control::host::Host for LinuxControl {
    fn state(&self) -> std::result::Result<crate::control::host::HostState, ControlError> {
        let status = self
            .published()?
            .status
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| ControlError::new("not-ready", "Hex is starting; try again"))?;
        Ok(crate::control::host::HostState {
            capture: crate::control::host::capture_from_state(status.desktop.activity.state),
            listener: status.desktop.listener,
            transcription: status.desktop.transcription,
            update: status.desktop.update_status,
            settings: status.settings,
            window_open: status.window_open,
        })
    }

    fn dictation(
        &self,
        action: CaptureAction,
        print: Option<crate::control::PrintReply>,
    ) -> std::result::Result<(), ControlError> {
        if let Some(print) = print {
            return self
                .route()?
                .commands
                .try_send(ListenerCommand::StopAndPrint(print))
                .map_err(|_| ControlError::new("busy", "Hex is busy; try again"));
        }
        self.shortcut(match action {
            CaptureAction::Start => ShortcutCommand::DictationStart,
            CaptureAction::Stop => ShortcutCommand::DictationStop,
            CaptureAction::Cancel => ShortcutCommand::DictationCancel,
            CaptureAction::Toggle => ShortcutCommand::DictationToggle,
        })
    }

    fn paste_last(&self) -> std::result::Result<(), ControlError> {
        self.shortcut(ShortcutCommand::PasteLast)
    }

    fn save(&self, settings: AppSettings) -> std::result::Result<(), ControlError> {
        self.host
            .save_settings(settings)
            .map_err(|error| ControlError::new("internal", format!("couldn't save: {error:#}")))
    }

    fn history(&self) -> Option<crate::history::History> {
        self.host.history()
    }

    fn show_window(&self, pane: Option<Pane>) -> std::result::Result<(), ControlError> {
        self.window(Control::Show(pane))
    }

    fn hide_window(&self) -> bool {
        self.host.window_open() && self.window(Control::Hide).is_ok()
    }

    fn onboarding_reset(&self) -> ControlReply {
        let (sender, receiver) = mpsc::sync_channel(1);
        if let Err(error) = self.window(Control::OnboardingReset(sender)) {
            return error.into();
        }
        match receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => ControlReply::done("Opened onboarding"),
            Ok(Err(why)) => ControlReply::error("unsupported", why),
            Err(_) => ControlReply::error("unavailable", "the window didn't answer; try again"),
        }
    }

    fn choose_model(
        &self,
        selection: crate::transcription_models::TranscriptionSelection,
    ) -> std::result::Result<(), ControlError> {
        self.host
            .request(HostRequest::ChooseModel {
                model: selection.model,
                language: selection.language,
            })
            .map_err(|error| ControlError::new("failed", format!("{error:#}")))
    }

    fn cancel_model(&self) {
        let _ = self.host.request(HostRequest::CancelModel);
    }

    fn check_for_updates(&self) -> std::result::Result<(), ControlError> {
        self.host
            .request(HostRequest::Desktop(DesktopAction::CheckForUpdates))
            .map_err(|error| ControlError::new("unavailable", format!("{error:#}")))
    }

    fn launch_at_login(&self, enabled: Option<bool>) -> Option<std::result::Result<bool, String>> {
        let format = |error: color_eyre::Report| format!("{error:#}");
        Some(match enabled {
            None => crate::linux_login_item::is_enabled().map_err(format),
            Some(enabled) => crate::linux_login_item::set_enabled(enabled)
                .and_then(|()| crate::linux_login_item::is_enabled())
                .map_err(format),
        })
    }

    fn quit(&self, restart: bool) -> std::result::Result<(), ControlError> {
        // A unit restarts by itself; otherwise `hex restart` reopens Hex
        // once this one has drained.
        if restart && linux_service::supervised() {
            return linux_service::schedule_restart()
                .map_err(|error| ControlError::new("failed", format!("{error:#}")));
        }
        self.host.quit();
        Ok(())
    }

    fn shortcut_rules(&self) -> crate::control::settings::ShortcutRules {
        crate::rune_ui::shortcut_rules()
    }

    fn annotate(&self, command: crate::control::ControlCommand) -> ControlReply {
        match command {
            crate::control::ControlCommand::ScrawlCapture {
                action: crate::control::ScrawlCaptureAction::Status,
            } => linux_service::capture_status(),
            crate::control::ControlCommand::ScrawlCapture { action } => {
                match crate::capture_host::send_to_listener(
                    crate::capture_host::CaptureControl::Scrawl(action),
                ) {
                    Ok(()) => ControlReply::done("Sent to Hex"),
                    Err(error) => ControlReply::error("failed", format!("{error:#}")),
                }
            }
            _ => crate::control::host::unsupported(
                "run this `hex annotate` command directly; it reads the captures folder itself",
            ),
        }
    }
}

impl LinuxDesktopHost {
    fn host_snapshot(&self) -> HostSnapshot {
        HostSnapshot {
            desktop: self.snapshot(),
            settings: self.settings.clone(),
            editing: self.shortcut_edit.is_some(),
        }
    }

    fn snapshot(&self) -> DesktopSnapshot {
        let update_status = self.updater.status();
        DesktopSnapshot {
            activity: self.activity.clone(),
            listener: DesktopListenerSnapshot {
                running: self.is_running(),
                status: self.status.clone(),
                warning: self
                    .listener
                    .as_ref()
                    .and_then(|listener| listener.hotkey_status.warning()),
                shortcuts: self
                    .listener
                    .as_ref()
                    .map(|listener| listener.hotkey_status.shortcuts())
                    .unwrap_or_default(),
                shortcuts_configurable: self
                    .listener
                    .as_ref()
                    .is_some_and(|listener| listener.hotkey_status.configurable()),
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
                    .transcription_preparation
                    .as_ref()
                    .map(|preparation| preparation.model),
            },
            update_status,
        }
    }

    /// Where `hex` shortcut commands go: the listener, once it reports its
    /// session (a listener still loading its model would replay them late).
    fn shortcut_route(&self) -> Option<ShortcutRoute> {
        self.listener
            .as_ref()
            .filter(|listener| matches!(listener.startup, ListenerStartup::Observed))
            .map(|listener| ShortcutRoute {
                commands: listener.commands.clone(),
            })
    }

    fn dispatch(&mut self, action: DesktopAction) -> Result<()> {
        match action {
            DesktopAction::ClearError => {
                self.error = None;
                self.settings_error = None;
                self.dismissed_failure_at = self.activity.last_failure.as_ref().map(|(at, _)| *at);
            }
            DesktopAction::CheckForUpdates
            | DesktopAction::OpenUpdate
            | DesktopAction::RestartIntoUpdate
            | DesktopAction::ConfirmUpdateSwitch
            | DesktopAction::WindowFocused => {
                if matches!(action, DesktopAction::RestartIntoUpdate)
                    && let Some(marker) = reopen_marker()
                {
                    let _ = std::fs::write(marker, b"");
                }
                if let Err(error) = self.updater.act(&action) {
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
            DesktopAction::ConfigureShortcuts => {
                if let Some(listener) = &self.listener {
                    listener.hotkey_status.configure();
                }
            }
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
        let mut host = LinuxDesktopHost::new(
            PathBuf::new(),
            settings,
            None,
            crate::updater::Updater::idle(),
        );
        if running {
            host.listener = Some(Listener {
                worker: std::thread::spawn(|| Ok(())),
                stop: Arc::new(AtomicBool::new(false)),
                startup: ListenerStartup::Observed,
                hotkey_status: Default::default(),
                commands: mpsc::sync_channel(1).0,
                updates: mpsc::channel().0,
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
        host.shortcut_edit.as_mut().unwrap().worker = Some(worker);
    }

    #[test]
    fn malformed_settings_cannot_start_even_after_dismissing_the_error() {
        let mut host = LinuxDesktopHost::new(
            PathBuf::new(),
            // A model never installed on Linux, so the resumed start below
            // cannot open the microphone where the real model is installed.
            host_for_edit(false).settings.clone(),
            Some("Could not load Linux settings: invalid JSON".into()),
            crate::updater::Updater::idle(),
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
        host.adopt_settings(recovered.clone());
        assert!(host.settings_valid);
        assert_eq!(host.settings, recovered);
        assert!(!host.is_running());
        assert_eq!(host.status, "Model required");
    }

    #[test]
    fn listener_startup_ignores_the_previous_session_until_a_new_one_arrives() {
        for previous_session in [None, Some(10)] {
            let mut listener = Listener {
                stop: Arc::new(AtomicBool::new(false)),
                worker: std::thread::spawn(|| Ok(())),
                startup: ListenerStartup::Awaiting { previous_session },
                hotkey_status: Default::default(),
                commands: mpsc::sync_channel(1).0,
                updates: mpsc::channel().0,
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
    fn listener_warning_is_nonfatal_and_round_trips_in_status() {
        let mut host = host_for_edit(true);
        host.status = "Enabled".into();
        let status = host.listener.as_ref().unwrap().hotkey_status.clone();
        status.update_escape_availability(false);
        let serialized = serde_json::to_vec(&host.snapshot()).unwrap();
        let received: DesktopSnapshot = serde_json::from_slice(&serialized).unwrap();
        let listener = received.listener;
        assert!(listener.running);
        assert_eq!(listener.status, "Enabled");
        assert_eq!(
            listener.warning.as_deref(),
            Some(crate::linux_input::ESCAPE_CANCEL_WARNING),
        );
        assert!(received.operation_error.is_none());

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
                automatic: false,
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

    /// A running fixture listener that keeps what the host sends it.
    fn listening_host() -> (LinuxDesktopHost, Receiver<ListenerUpdate>) {
        let mut host = host_for_edit(true);
        let (updates, received) = mpsc::channel();
        let listener = host.listener.as_mut().unwrap();
        listener.updates = updates;
        // Runs until stopped, as a real listener does.
        let stop = listener.stop.clone();
        listener.worker = std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(())
        });
        host.status = "Enabled".into();
        (host, received)
    }

    #[test]
    fn settings_changes_apply_in_place_without_stopping_the_listener() {
        let (mut host, received) = listening_host();
        let before = host.snapshot();
        let mut candidate = host.settings.clone();
        candidate.paste_with_shift = !candidate.paste_with_shift;
        candidate.indicator_position = crate::app_settings::IndicatorPosition::Bottom;
        candidate.sound_effect_volume = 0.25;
        host.adopt_settings(candidate.clone());
        assert_eq!(host.settings, candidate);
        assert!(!host.listener.as_ref().unwrap().stop.load(Ordering::Relaxed));
        assert!(matches!(
            received.try_iter().collect::<Vec<_>>().as_slice(),
            [ListenerUpdate::Settings(sent)] if **sent == candidate
        ));
        host.refresh();
        assert!(host.is_running());
        assert_eq!(host.snapshot().listener, before.listener);
    }

    #[test]
    fn a_shortcut_edit_suspends_matching_without_stopping_the_listener() {
        let (mut host, received) = listening_host();
        host.begin_shortcut_edit(ShortcutTarget::Dictation, None);
        assert!(host.listen_when_ready);
        assert!(!host.listener.as_ref().unwrap().stop.load(Ordering::Relaxed));
        host.cancel_shortcut_edit();
        host.refresh_shortcut_edit();
        assert!(host.shortcut_edit.is_none());
        assert_eq!(host.status, "Enabled");
        let updates = received.try_iter().collect::<Vec<_>>();
        assert!(matches!(
            updates.as_slice(),
            [
                ListenerUpdate::SuspendShortcuts(true),
                ListenerUpdate::Settings(_),
                ListenerUpdate::SuspendShortcuts(false)
            ]
        ));
    }

    #[test]
    fn canceled_or_failed_capture_restores_prior_listening_without_changing_settings() {
        for running in [false, true] {
            for cancel in [false, true] {
                let mut host = host_for_edit(running);
                let original = host.settings.clone();
                host.begin_shortcut_edit(ShortcutTarget::Dictation, None);
                assert!(host.capturing_hotkey());
                host.listener = None;
                if cancel {
                    host.cancel_shortcut_edit();
                } else {
                    set_finished_edit(
                        &mut host,
                        Err(color_eyre::eyre::eyre!("input permission denied")),
                    );
                }
                host.refresh_shortcut_edit();
                assert_eq!(host.settings, original);
                assert_eq!(host.listen_when_ready, running);
                assert_eq!(host.error.is_some(), !cancel);
                assert!(host.shortcut_edit.is_none());
                assert!(!host.capturing_hotkey());
            }
        }
    }

    #[test]
    fn cancel_signals_worker_and_does_not_wait_or_restart_until_it_finishes() {
        let mut host = host_for_edit(true);
        host.begin_shortcut_edit(ShortcutTarget::Dictation, None);
        host.listener = None;
        let (release, held) = mpsc::channel();
        host.shortcut_edit.as_mut().unwrap().worker = Some(std::thread::spawn(move || {
            let _ = held.recv();
            Err(color_eyre::eyre::eyre!("capture canceled"))
        }));
        host.cancel_shortcut_edit();
        host.refresh_shortcut_edit();
        let edit = host.shortcut_edit.as_mut().unwrap();
        assert!(edit.canceled.load(Ordering::Acquire));
        assert!(edit.worker.is_some());
        assert!(host.listen_when_ready);
        release.send(()).unwrap();
        let _ = host
            .shortcut_edit
            .as_mut()
            .unwrap()
            .worker
            .take()
            .unwrap()
            .join()
            .unwrap()
            .unwrap_err();
        host.refresh_shortcut_edit();
        assert!(host.listen_when_ready);
        assert!(host.shortcut_edit.is_none());
    }

    #[test]
    fn closing_the_window_cancels_only_an_uncommitted_capture() {
        static SHUTDOWN: AtomicBool = AtomicBool::new(false);
        let mut host = host_for_edit(true);
        host.begin_shortcut_edit(ShortcutTarget::Dictation, None);
        let handle = HostHandle::new(host, &SHUTDOWN);
        handle.window_closed();
        {
            let mut host = handle.lock();
            let edit = host.shortcut_edit.as_ref().unwrap();
            assert!(edit.canceled.load(Ordering::Acquire));
            host.listener = None;
            host.refresh_shortcut_edit();
            assert!(host.listen_when_ready);
        }

        // Without a capture, closing the window leaves dictation alone.
        let handle = HostHandle::new(host_for_edit(true), &SHUTDOWN);
        handle.window_closed();
        let host = handle.lock();
        assert!(host.is_running());
        assert!(host.listen_when_ready);
        assert!(!host.listener.as_ref().unwrap().stop.load(Ordering::Acquire));
    }

    #[test]
    fn commands_route_only_to_a_started_listener() {
        let mut host = host_for_edit(false);
        assert!(host.shortcut_route().is_none());
        let (commands, received) = mpsc::sync_channel(4);
        host.listener = Some(Listener {
            worker: std::thread::spawn(|| Ok(())),
            stop: Arc::new(AtomicBool::new(false)),
            startup: ListenerStartup::Awaiting {
                previous_session: None,
            },
            hotkey_status: Default::default(),
            commands,
            updates: mpsc::channel().0,
        });
        // A listener still loading its model would replay the command late.
        assert!(host.shortcut_route().is_none());
        host.listener.as_mut().unwrap().startup = ListenerStartup::Observed;
        let route = host.shortcut_route().unwrap();
        linux_service::route_shortcut(Some(&route), crate::linux_input::ShortcutCommand::PasteLast)
            .unwrap();
        assert_eq!(
            received.try_iter().collect::<Vec<_>>(),
            [ListenerCommand::from(
                crate::linux_input::ShortcutCommand::PasteLast
            )]
        );
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
        let mut host = LinuxDesktopHost::new(
            PathBuf::new(),
            settings,
            None,
            crate::updater::Updater::idle(),
        );
        host.transcription_preparation = Some(TranscriptionPreparation {
            canceled: Arc::new(AtomicBool::new(false)),
            model: TranscriptionModelId::default(),
            progress: Arc::new(AtomicU64::new(0)),
            stage: Arc::new(AtomicU8::new(ModelPreparationStage::Downloading as u8)),
            worker: std::thread::spawn(|| Err("fixture preparation".into())),
            automatic: false,
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

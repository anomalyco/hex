//! Hex on macOS (`hex app`): one menu-bar process.
//!
//! - The main thread runs Rune's event loop (`rune::App`, windowless and
//!   kept running, as an accessory app) and, beside it on the same AppKit
//!   run loop, the native status item, the updater, the main menu, the Dock
//!   reopen handler, and the Metal dictation HUD. A 16 ms main-loop timer
//!   ([`Pump`]) drives them, so they keep working with no window open.
//! - The runtime thread owns model preparation, starts the listener once
//!   setup is ready, and publishes the listener, model, update, and settings
//!   state the window shows ([`RuntimeHandle`]).
//! - The listener (`recognition::listen`), the meeting controller, and the
//!   local API keep their own threads.
//!
//! Closing the window hides it; Quit (menu bar, ⌘Q, `hex quit`, `hex restart`
//! and the onboarding relaunch after scheduling the new copy, or Ctrl-C) ends
//! the loop, then stops and joins everything and ends the process with
//! `_exit` ([`crate::desktop::process_exit`]). AppKit's own terminate
//! (the quit Apple event, logout) does the same from its
//! will-terminate notification.
//!
//! The meeting capsules (offer, call ended, ready, recording) are Rune
//! overlays (`rune_ui::meeting_capsules`): non-activating panels fed from
//! the controller events here.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use block2::RcBlock;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, Sel};
use objc2::{MainThreadOnly, msg_send, sel};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSAppleEventManager, NSObject, NSString, NSTimer};

use crate::app_settings::AppSettings;
use crate::desktop::host::{
    DesktopAction, DesktopListenerSnapshot, DesktopShortcut, DesktopShortcuts,
    DesktopTranscriptionSnapshot, DesktopUpdateStatus,
};
use crate::dictation::indicator::DictationIndicatorEvent;
use crate::macos_rune::status_item::StatusItemAction;
use crate::meeting::MeetingRequest;
use crate::meeting::watcher::{ControllerEvent, ListenerConfig, MeetingMenuState};
use crate::recognition::RecognitionControl;
use crate::rune_hud::DictationIndicatorUi;
use crate::rune_ui::{Destination, Ui};
use crate::transcription::models::{
    ModelPreparationStage, TranscriptionModelId, TranscriptionSelection,
};
use crate::transcription::preparation::TranscriptionPreparation;

mod control;
pub(crate) mod status_item;

#[cfg(test)]
mod tests;

/// How often the runtime refreshes while nothing is asked.
const RUNTIME_TICK: Duration = Duration::from_millis(250);
/// The main-loop timer: the HUD follows the pointer and channels drain.
const PUMP_INTERVAL: f64 = 0.016;
/// Seconds a window opened at launch has to draw its first frame before the
/// status item, HUD, and menu are set up anyway.
const LAUNCH_SETUP_DEADLINE: f64 = 1.0;
const MODEL_MENU_REFRESH: Duration = Duration::from_secs(1);
const MEETING_MENU_REFRESH: Duration = Duration::from_millis(500);
/// 'aevt' / 'rapp': the Apple event LaunchServices sends when the app is
/// opened again while it runs (Dock, Finder, `open`).
const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
const REOPEN_APPLICATION: u32 = u32::from_be_bytes(*b"rapp");

/// What the window asks the runtime, in the Linux service's vocabulary so
/// the shared panes send the same requests on every platform.
#[derive(Clone, Debug)]
pub(crate) enum Request {
    /// Update actions go to the updater from the window (`rune_ui::host`).
    Desktop(#[allow(dead_code)] DesktopAction),
    /// Handled in the window (`rune_ui::host`): an AppKit event monitor.
    CaptureShortcutFor(#[allow(dead_code)] ShortcutTarget),
    CancelShortcut,
    /// Linux's service registers recorded shortcuts; on macOS the window
    /// saves them and the listener follows `settings.json`.
    #[allow(dead_code)]
    SetShortcut {
        target: ShortcutTarget,
        shortcut: DesktopShortcut,
    },
    ChooseModel {
        model: TranscriptionModelId,
        language: String,
    },
    /// The menu bar's model list: only installed models, never a download.
    SelectInstalledModel(TranscriptionModelId),
    /// `hex model use`: this exact selection, installed only, never a download.
    UseInstalledSelection(TranscriptionSelection),
    /// Snapshot from the main-thread TIS observer, not a shortcut event.
    InputSourceChanged(Option<String>),
    CancelModel,
    /// The window saved `settings.json`; adopt it.
    ReloadSettings,
}

/// A shortcut the listener registers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ShortcutTarget {
    #[default]
    Dictation,
    PasteLast,
    #[cfg(hex_capture)]
    Capture,
    RawDictation,
    /// Labs › Second dictation shortcut.
    SecondDictation,
}

/// What the runtime last published for the window and the menu bar.
#[derive(Clone, PartialEq)]
pub(crate) struct Published {
    pub(crate) listener: DesktopListenerSnapshot,
    pub(crate) transcription: DesktopTranscriptionSnapshot,
    pub(crate) update_status: DesktopUpdateStatus,
    pub(crate) update_notes: Option<crate::desktop::host::UpdateNotes>,
    pub(crate) settings: AppSettings,
    preparation: crate::transcription::preparation::PreparationStatus,
}

type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Shared {
    published: Mutex<Option<Arc<Published>>>,
    wakers: Mutex<Vec<Wake>>,
}

/// The window's and the menu bar's connection to the runtime.
#[derive(Clone)]
pub(crate) struct RuntimeHandle {
    requests: Sender<Request>,
    shared: Arc<Shared>,
    meeting_requests: SyncSender<MeetingRequest>,
    recognition_controls: SyncSender<RecognitionControl>,
    history: Option<crate::history::History>,
    event_path: PathBuf,
    indicator: crate::dictation::indicator::DictationIndicatorSender,
}

impl RuntimeHandle {
    /// The listener's event log, which the tour reads for practice dictations.
    pub(crate) fn event_path(&self) -> &Path {
        &self.event_path
    }

    pub(crate) fn history(&self) -> Option<crate::history::History> {
        self.history.clone()
    }

    /// The dictation HUD, for HUD Lab.
    pub(crate) fn indicator(&self, event: DictationIndicatorEvent) {
        self.indicator.send(event);
    }

    pub(crate) fn send(&self, request: Request) -> Result<()> {
        if matches!(
            &request,
            Request::ChooseModel { .. }
                | Request::SelectInstalledModel(_)
                | Request::UseInstalledSelection(_)
                | Request::CancelModel
                | Request::ReloadSettings
        ) && let Some(mtm) = MainThreadMarker::new()
            && let Some(source) = crate::keyboard_input_sources::take_change(mtm)
        {
            // Deliver a source change not yet pumped before this UI/control
            // choice, so it cannot later undo the newer manual choice.
            self.requests
                .send(Request::InputSourceChanged(source))
                .map_err(|_| eyre!("Hex's runtime stopped"))?;
        }
        self.requests
            .send(request)
            .map_err(|_| eyre!("Hex's runtime stopped"))
    }

    pub(crate) fn published(&self) -> Option<Arc<Published>> {
        lock(&self.shared.published).clone()
    }

    /// Wakes `wake` after every publication.
    pub(crate) fn subscribe(&self, wake: Wake) {
        lock(&self.shared.wakers).push(wake);
    }

    pub(crate) fn meeting(&self, request: MeetingRequest) -> Result<(), String> {
        self.meeting_requests
            .try_send(request)
            .map_err(|error| format!("could not reach the meeting recorder: {error}"))
    }

    pub(crate) fn recognition(&self, control: RecognitionControl) -> Result<(), String> {
        self.recognition_controls
            .try_send(control)
            .map_err(|error| format!("could not reach the listener: {error}"))
    }
}

static RUNTIME: OnceLock<RuntimeHandle> = OnceLock::new();
static WINDOW: OnceLock<rune::AppHandle<Ui>> = OnceLock::new();
static UPDATER: OnceLock<crate::updater::Updater> = OnceLock::new();

/// The running app's runtime; `None` outside [`run`] (headless snapshots).
pub(crate) fn runtime() -> Option<RuntimeHandle> {
    RUNTIME.get().cloned()
}

/// Carries out an update control from the window, a menu, or `hex`. Restart
/// to update finishes on the updater's worker; the pump then quits.
pub(crate) fn update_action(action: &crate::desktop::host::DesktopAction) -> Result<()> {
    UPDATER
        .get()
        .ok_or_else(|| eyre!("the updater is not running"))?
        .act(action)
}

pub(crate) fn update_status() -> DesktopUpdateStatus {
    UPDATER.get().map_or(
        DesktopUpdateStatus::Unavailable,
        crate::updater::Updater::status,
    )
}

/// Quits Hex from any thread.
pub(crate) fn quit() {
    if let Some(window) = WINDOW.get() {
        window.quit();
    }
}

/// The window closed (hidden): the Dock follows the saved preference again.
pub(crate) fn window_closed() {
    let show_dock_icon = AppSettings::load()
        .map(|settings| settings.show_dock_icon)
        .unwrap_or(false);
    crate::app_settings::set_dock_icon_visible(crate::app_settings::dock_icon_visible(
        show_dock_icon,
        crate::macos_rune::status_item::installed(),
    ));
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    let detail = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown cause");
    format!("the listener panicked: {detail}")
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// Samples footprint and CPU for the life of the app (every 10 s in release, 2 s in debug
/// builds) and logs a warning on a large footprint, steady growth, or a busy core.
fn watch_process() {
    let config = if cfg!(debug_assertions) {
        procwatch::Config::dev("hex")
    } else {
        procwatch::Config::release("hex")
    };
    procwatch::watch_with(config, |event, _| tracing::warn!(%event, "process watch"));
}

pub fn run(shutdown: &'static AtomicBool, listener: ListenerConfig) -> Result<()> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Err(eyre!("desktop startup requires the main thread"));
    };
    shutdown.store(false, Ordering::Relaxed);
    watch_process();
    crate::keyboard::initialize_layout()?;
    crate::updater::finish_on_launch();
    let _ = UPDATER.set(crate::updater::Updater::start());
    // Never a silent launch failure: unreadable settings start with defaults
    // and the window says why (#744).
    let (mut settings, settings_error) = AppSettings::load_for_launch();
    // Preload bundled feedback sounds on a worker so onboarding and the Sounds
    // sheet play immediately even before permissions let `recognition` start.
    // Opens no audio output device until a tone actually plays.
    let _ = crate::feedback::preload();
    // Known before the observer's first report, so launching keeps the saved
    // model instead of applying the current source's mapping.
    let initial_input_source = settings
        .keyboard_model_switching
        .enabled
        .then(|| crate::keyboard_input_sources::current(mtm))
        .flatten();
    let setup_ready = crate::desktop::onboarding::status(&settings.transcription).ready();
    crate::desktop::onboarding::recheck_after_identity_change(&mut settings, setup_ready);
    let onboarding_completed = crate::desktop::onboarding::completion_recorded();
    let history = match crate::history::History::open_default(settings.history_retention) {
        Ok(history) => Some(history),
        Err(error) => {
            tracing::warn!(%error, "dictation history is unavailable");
            None
        }
    };
    let event_path = listener.event_path.clone();
    let event_log = crate::desktop::events::EventLog::create(&event_path)?;
    let drained_event_log = event_log.clone();
    let (developer_sender, developer_receiver) = mpsc::sync_channel(8);
    let (control_sender, control_receiver) = mpsc::sync_channel(8);
    let (recognition_controls, recognition_control_receiver) = mpsc::sync_channel(8);
    let mut local_api = crate::local_api::LocalApi::start(
        event_log.clone(),
        crate::local_api::LocalApiOptions {
            developer_control: crate::DEVELOPER_FEATURES_ENABLED.then(|| developer_sender.clone()),
            dictation_control: Some(recognition_controls.clone()),
            control: Some(control_sender),
            ..Default::default()
        },
    )?;
    let (controller_events_sender, controller_events) = mpsc::sync_channel(32);
    let (controller_commands, controller_command_receiver) = mpsc::sync_channel(8);
    let (meeting_requests, meeting_request_receiver) = mpsc::sync_channel(8);
    let (indicator, indicator_events) = crate::dictation::indicator::channel();

    // The controller idles without inspecting microphone activity until the
    // user opts in to meetings (always available in developer builds).
    let controller = thread::Builder::new()
        .name("hex-meetings".into())
        .spawn(move || {
            if let Err(error) = crate::meeting::watcher::controller_loop(
                shutdown,
                controller_events_sender.clone(),
                controller_command_receiver,
                meeting_request_receiver,
            ) {
                tracing::error!(%error, "meeting controller stopped");
                let _ = controller_events_sender.send(ControllerEvent::Failed {
                    title: "Meeting detection stopped".into(),
                    error: error.to_string(),
                });
            }
        })?;

    // The microphone stays cold until setup is ready; the runtime starts it.
    let (recognition_start, start_receiver) = mpsc::sync_channel(1);
    let recognition_indicator = indicator.clone();
    let recognition_history = history.clone();
    let listener_failure = Arc::new(Mutex::new(None::<String>));
    let recognition_failure = listener_failure.clone();
    let recognition = thread::Builder::new()
        .name("hex-listener".into())
        .spawn(move || {
            if !setup_ready {
                loop {
                    if shutdown.load(Ordering::Relaxed) {
                        return;
                    }
                    match start_receiver.recv_timeout(Duration::from_millis(100)) {
                        Ok(()) => break,
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::recognition::listen(
                    event_log,
                    listener.device.as_deref(),
                    shutdown,
                    Some(recognition_indicator.clone()),
                    recognition_history,
                    Some(recognition_control_receiver),
                )
            }));
            let failure = match result {
                Ok(Ok(())) => return,
                Ok(Err(error)) => {
                    tracing::error!(%error, "desktop recognition stopped");
                    let message = format!("{error:#}");
                    if message.contains("event tap") {
                        crate::desktop::onboarding::note_listener_failure();
                    }
                    message
                }
                // The panic hook has already logged it.
                Err(payload) => panic_message(payload.as_ref()),
            };
            *lock(&recognition_failure) = Some(failure);
            recognition_indicator.send(DictationIndicatorEvent::Failed);
        })?;

    let (requests, request_receiver) = mpsc::channel();
    let shared = Arc::new(Shared::default());
    let handle = RuntimeHandle {
        requests: requests.clone(),
        shared: shared.clone(),
        meeting_requests: meeting_requests.clone(),
        recognition_controls: recognition_controls.clone(),
        history: history.clone(),
        event_path,
        indicator: indicator.clone(),
    };
    RUNTIME
        .set(handle.clone())
        .map_err(|_| eyre!("the Hex runtime already started"))?;
    let runtime_settings = settings.clone();
    let runtime = thread::Builder::new()
        .name("hex-runtime".into())
        .spawn(move || {
            run_runtime(
                runtime_settings,
                initial_input_source,
                request_receiver,
                &shared,
                shutdown,
                (!setup_ready).then_some(recognition_start),
                listener_failure,
            )
        })?;
    // A closed window is only ordered out, and AppKit keeps it on the Space
    // it was last shown on: reopening it from the menu bar would switch back
    // to that Space. Bring it to the active Space instead.
    let native = rune::MacWindowOptions::default()
        .collection_behavior(objc2_app_kit::NSWindowCollectionBehavior::MoveToActiveSpace.0);
    let mut app = crate::rune_ui::app(Default::default())
        .args(Vec::new())
        .accessory()
        .macos_window(native);
    // The first-run tour opens centred at one size; a tiling window manager
    // would otherwise stretch it to half the screen.
    if !onboarding_completed {
        app = app.window_options(crate::rune_ui::fixed_window());
    }
    let window = app.handle();
    let _ = WINDOW.set(window.clone());
    // The status item is created inside the loop; Hex can't run without a
    // way back to its window, so a failed item opens the window there.
    let open_at_launch = settings_error.is_some()
        || crate::meeting::watcher::should_open_app_on_launch(
            settings.show_dock_icon,
            setup_ready,
            onboarding_completed,
            true,
        );
    if let Some(error) = settings_error {
        window.update(move |ui, _| crate::rune_ui::show_error(ui, error));
    }
    app = if open_at_launch {
        app.keep_running()
    } else {
        app.windowless()
    };
    let capsules = {
        let window = window.clone();
        crate::rune_ui::meeting_capsules::MeetingCapsules::install(
            &mut app,
            controller_commands,
            {
                let window = window.clone();
                move |choice| {
                    crate::app_settings::save_meeting_provider(choice.clone());
                    window.update(move |ui, _| crate::rune_ui::meeting_provider_chosen(ui, choice));
                }
            },
            move |corner| {
                crate::app_settings::save_meeting_indicator_corner(corner);
                window.update(move |ui, _| crate::rune_ui::meeting_corner_moved(ui, corner));
            },
        )
    };
    let hud = crate::rune_hud::install(&mut app);
    crate::rune_ui::live_text::install(&mut app, crate::rune_hud::live_text_place);
    // The capture capsule, picker, and ink.
    #[cfg(hex_capture)]
    let capture_overlays =
        Some(crate::rune_ui::capture_capsule::feed::CaptureOverlays::install(&mut app));
    let pump = Pump {
        window: window.clone(),
        capsules,
        #[cfg(hex_capture)]
        capture_overlays,
        runtime: handle,
        shutdown,
        indicator_events,
        hud,
        status_actions: None,
        controller_events,
        developer_calls: developer_receiver,
        control_calls: control_receiver,
        meeting_waiters: Vec::new(),
        meeting_state: MeetingMenuState::Idle,
        meeting_menu_at: Instant::now(),
        model_menu_at: Instant::now(),
        show_dock_icon: settings.show_dock_icon,
    };
    // Quit ends the loop and finishes below; AppKit's own terminate (the quit
    // Apple event, logout) would `exit` from inside it, so its
    // will-terminate notification finishes there instead.
    DRAIN.set(move || {
        shutdown.store(true, Ordering::Relaxed);
        if let Some(updater) = UPDATER.get() {
            updater.stop();
            updater.install_on_quit();
        }
        drop(requests);
        local_api.shutdown();
        // The listener owns the transcription engine; joining it frees the
        // model after its last use.
        for (name, worker) in [
            ("recognition", recognition),
            ("meeting controller", controller),
            ("runtime", runtime),
        ] {
            if worker.join().is_err() {
                tracing::error!("{name} worker panicked during shutdown");
            }
        }
        crate::audio::recording_environment::wait_until_restored(Duration::from_secs(2));
        if let Err(error) = drained_event_log.flush_within(Duration::from_millis(500)) {
            tracing::warn!(%error, "could not flush the event log before quitting");
        }
    });
    // AppKit's terminate may come from the loop's first turn, so its observer
    // is in place before the loop runs; the rest of `target` waits below.
    let target = MenuTarget::new(mtm);
    observe_termination(&target);
    // The status item, HUD, and main menu wait for the window's first frame
    // so it reaches the screen without them; a launch without a window sets
    // them up in the loop's first turn. None of these updates change what the
    // window shows, so none asks for a frame.
    LAUNCH.with(|slot| *slot.borrow_mut() = Some(Launch { pump, target }));
    if open_at_launch {
        let ready = window.clone();
        app = app.on_ready(move |_| {
            ready.update_if(|_, _| {
                start_main_thread();
                false
            });
        });
        window.update_if(|_, _| {
            start_main_thread_after(LAUNCH_SETUP_DEADLINE);
            false
        });
    } else {
        window.update_if(|_, _| {
            start_main_thread();
            false
        });
    }
    app.run();

    PUMP.with(|slot| {
        if let Some(state) = slot.borrow_mut().take() {
            state.timer.invalidate();
        }
    });
    crate::desktop::process_exit::finish(&DRAIN)
}

/// Stops and joins the runtime, listener, meeting controller, and local API.
static DRAIN: crate::desktop::process_exit::Drain = crate::desktop::process_exit::Drain::new();

// ---------------------------------------------------------------------------
// Runtime thread

fn run_runtime(
    settings: AppSettings,
    initial_input_source: Option<String>,
    requests: Receiver<Request>,
    shared: &Shared,
    shutdown: &AtomicBool,
    recognition_start: Option<SyncSender<()>>,
    listener_failure: Arc<Mutex<Option<String>>>,
) {
    let mut runtime = Runtime {
        keyboard_switcher: crate::transcription::keyboard_model_switching::Switcher::new(
            settings.keyboard_model_switching.clone(),
            initial_input_source,
        ),
        automatic_preparation: false,
        settings,
        preparation: TranscriptionPreparation::default(),
        recognition_start,
        listener_failure,
    };
    loop {
        let mut next = match requests.recv_timeout(RUNTIME_TICK) {
            Ok(request) => Some(request),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        while let Some(request) = next {
            runtime.handle(request);
            next = requests.try_recv().ok();
        }
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        runtime.tick();
        let published = Arc::new(runtime.publish());
        let changed = {
            let mut current = lock(&shared.published);
            let changed = current.as_deref() != Some(&published);
            if changed {
                *current = Some(published);
            }
            changed
        };
        if changed {
            for wake in lock(&shared.wakers).iter() {
                wake();
            }
        }
    }
    runtime.preparation.cancel();
}

/// Two selections that load the same thing.
fn same_model_and_language(left: &TranscriptionSelection, right: &TranscriptionSelection) -> bool {
    left.model == right.model && left.language == right.language
}

struct Runtime {
    settings: AppSettings,
    preparation: TranscriptionPreparation,
    keyboard_switcher: crate::transcription::keyboard_model_switching::Switcher,
    automatic_preparation: bool,
    /// Starts the listener once setup is ready; `None` once started.
    recognition_start: Option<SyncSender<()>>,
    /// Why the listener stopped, once it has.
    listener_failure: Arc<Mutex<Option<String>>>,
}

impl Runtime {
    fn handle(&mut self, request: Request) {
        match request {
            Request::ChooseModel { model, language } => {
                self.manual_choice();
                let selection = TranscriptionSelection { model, language };
                self.preparation.start(selection, false);
            }
            Request::SelectInstalledModel(model) => {
                self.reload_settings();
                self.manual_choice();
                self.preparation
                    .start(self.settings.transcription_for_model(model), true);
            }
            Request::UseInstalledSelection(selection) => {
                self.manual_choice();
                // `hex model use` replies "already in use" on the same test.
                if !same_model_and_language(&selection, &self.settings.transcription) {
                    self.preparation.start(selection, true);
                }
            }
            Request::InputSourceChanged(source) => {
                if self
                    .keyboard_switcher
                    .source_changed(source, Instant::now())
                {
                    self.supersede_automatic();
                }
            }
            Request::CancelModel => {
                self.manual_choice();
                self.preparation.cancel();
            }
            Request::ReloadSettings => self.reload_settings(),
            Request::Desktop(_)
            | Request::CaptureShortcutFor(_)
            | Request::CancelShortcut
            | Request::SetShortcut { .. } => {}
        }
    }

    fn reload_settings(&mut self) {
        match AppSettings::load() {
            Ok(settings) => {
                if self
                    .keyboard_switcher
                    .configure(settings.keyboard_model_switching.clone(), Instant::now())
                {
                    self.supersede_automatic();
                }
                self.settings = settings;
            }
            Err(error) => tracing::warn!(%error, "could not reload settings"),
        }
    }

    fn manual_choice(&mut self) {
        self.keyboard_switcher.manual_choice();
        if self.automatic_preparation {
            self.preparation.cancel();
            self.automatic_preparation = false;
        }
    }

    fn supersede_automatic(&mut self) {
        // A source being cycled through must not cancel a manual operation.
        // Manual work is superseded only when the debounced choice is due.
        if self.automatic_preparation {
            self.preparation.cancel();
            self.automatic_preparation = false;
        }
    }

    /// Commits a prepared model and starts a listener that waited for setup.
    fn tick(&mut self) {
        self.tick_at(Instant::now());
    }

    fn tick_at(&mut self, now: Instant) {
        if let Some(choice) = self.keyboard_switcher.take_due(now) {
            let selection = TranscriptionSelection {
                language: choice.language,
                ..self.settings.transcription_for_model(choice.model)
            };
            if !same_model_and_language(&selection, &self.settings.transcription) {
                self.preparation.start(selection, true);
                self.automatic_preparation = true;
            } else {
                // The debounced choice keeps the current model, but must
                // still prevent an older manual preparation replacing it.
                self.preparation.cancel();
                self.automatic_preparation = false;
            }
        }
        if let Some(result) = self.preparation.poll() {
            self.automatic_preparation = false;
            let result = result.and_then(|selection| {
                crate::transcription::models::validate(&selection).map_err(|e| e.to_string())?;
                let mut settings = AppSettings::load().map_err(|e| format!("{e:#}"))?;
                settings
                    .save_transcription(selection)
                    .map_err(|e| format!("{e:#}"))?;
                self.settings = settings;
                Ok(())
            });
            match result {
                Ok(()) => self.preparation.set_error(None),
                Err(error) => {
                    tracing::warn!(%error, "could not select transcription model");
                    self.preparation.set_error(Some(error));
                }
            }
        }
        if self.recognition_start.is_some()
            && crate::desktop::onboarding::status(&self.settings.transcription).ready()
            && let Some(start) = self.recognition_start.take()
        {
            let _ = start.try_send(());
        }
    }

    fn publish(&self) -> Published {
        let preparation = self.preparation.status();
        let failure = lock(&self.listener_failure).clone();
        let running = self.recognition_start.is_none() && failure.is_none();
        Published {
            listener: DesktopListenerSnapshot {
                running,
                status: if failure.is_some() {
                    "Stopped"
                } else if running {
                    "Enabled"
                } else {
                    "Waiting for setup"
                }
                .into(),
                warning: failure,
                shortcuts: DesktopShortcuts::Registered,
                shortcuts_configurable: false,
            },
            transcription: DesktopTranscriptionSnapshot {
                downloaded_bytes: preparation.downloaded_bytes,
                error: preparation.error.clone(),
                preparation_stage: preparation
                    .model
                    .map(|_| ModelPreparationStage::Downloading),
                selection: self.settings.transcription.clone(),
                preparing: preparation.model,
            },
            update_status: update_status(),
            update_notes: UPDATER.get().and_then(crate::updater::Updater::notes),
            settings: self.settings.clone(),
            preparation,
        }
    }
}

// ---------------------------------------------------------------------------
// Main thread: status item, menu, reopen, HUD

struct Pump {
    window: rune::AppHandle<Ui>,
    capsules: crate::rune_ui::meeting_capsules::MeetingCapsules,
    #[cfg(hex_capture)]
    capture_overlays: Option<crate::rune_ui::capture_capsule::feed::CaptureOverlays>,
    runtime: RuntimeHandle,
    shutdown: &'static AtomicBool,
    indicator_events: Receiver<DictationIndicatorEvent>,
    hud: crate::rune_hud::HudSender,
    status_actions: Option<Receiver<StatusItemAction>>,
    controller_events: Receiver<ControllerEvent>,
    developer_calls: Receiver<crate::control::developer_control::DeveloperCall>,
    control_calls: Receiver<crate::control::ControlCall>,
    meeting_waiters: Vec<control::MeetingWaiter>,
    meeting_state: MeetingMenuState,
    meeting_menu_at: Instant,
    model_menu_at: Instant,
    show_dock_icon: bool,
}

struct PumpState {
    pump: Pump,
    /// Created on the main thread, so it stays out of the `Send` [`Pump`].
    indicator: DictationIndicatorUi,
    timer: Retained<NSTimer>,
    _menu_target: Retained<MenuTarget>,
}

thread_local! {
    static PUMP: RefCell<Option<PumpState>> = const { RefCell::new(None) };
    /// What [`start_main_thread`] takes, once.
    static LAUNCH: RefCell<Option<Launch>> = const { RefCell::new(None) };
}

struct Launch {
    pump: Pump,
    /// Already observing AppKit's terminate ([`observe_termination`]).
    target: Retained<MenuTarget>,
}

/// AppKit's terminate (the quit Apple event, logout) calls `exit` right after
/// this notification, so Hex finishes there ([`crate::desktop::process_exit`]).
fn observe_termination(target: &MenuTarget) {
    // SAFETY: the target outlives the observer (kept in LAUNCH, then PUMP,
    // for the app's life), and the selector matches its method.
    unsafe {
        let _: () = msg_send![
            &*objc2_foundation::NSNotificationCenter::defaultCenter(),
            addObserver: target,
            selector: sel!(applicationWillTerminate:),
            name: objc2_app_kit::NSApplicationWillTerminateNotification,
            object: std::ptr::null::<AnyObject>(),
        ];
    }
}

/// Starts the main-thread setup after `seconds` if the window's first frame
/// hasn't by then, so a window that never draws (no GPU, displays asleep)
/// still leaves a status item.
fn start_main_thread_after(seconds: f64) {
    let block = RcBlock::new(|_: std::ptr::NonNull<NSTimer>| start_main_thread());
    // SAFETY: the timer runs its block on this (main) thread's run loop.
    let _ =
        unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(seconds, false, &block) };
}

/// Sets up the status item, Dock policy, HUD, main menu, reopen handler, and
/// the pump's timer; the first call does, later ones do nothing.
fn start_main_thread() {
    let Some(mtm) = MainThreadMarker::new() else {
        tracing::error!("Hex's main-thread setup ran off the main thread");
        return;
    };
    let Some(Launch { mut pump, target }) = LAUNCH.with(|slot| slot.borrow_mut().take()) else {
        return;
    };
    pump.status_actions = match crate::macos_rune::status_item::install() {
        Ok(actions) => Some(actions),
        Err(error) => {
            tracing::error!(%error, "could not install Hex status item; opening the window");
            show(&pump.window, Destination::Settings);
            None
        }
    };
    // An open window always shows the Dock icon (see [`show`]).
    crate::app_settings::set_dock_icon_visible(
        window_visible()
            || crate::app_settings::dock_icon_visible(
                pump.show_dock_icon,
                pump.status_actions.is_some(),
            ),
    );
    let indicator = DictationIndicatorUi::new(pump.hud.clone());
    install_main_menu(&target, mtm);
    // Inside the loop, once AppKit has started, so its own Apple event setup
    // can't replace this handler.
    // SAFETY: the target outlives the handler (kept in PUMP for the app's
    // life), and the selector matches its method.
    unsafe {
        let _: () = msg_send![
            &*NSAppleEventManager::sharedAppleEventManager(),
            setEventHandler: &*target,
            andSelector: sel!(handleReopen:withReplyEvent:),
            forEventClass: CORE_EVENT_CLASS,
            andEventID: REOPEN_APPLICATION,
        ];
    }
    let block = RcBlock::new(|_: std::ptr::NonNull<NSTimer>| {
        PUMP.with(|slot| {
            if let Some(state) = slot.borrow_mut().as_mut() {
                if let Some(mtm) = MainThreadMarker::new() {
                    let switching = state.pump.runtime.published().is_some_and(|published| {
                        published.settings.keyboard_model_switching.enabled
                    });
                    crate::keyboard_input_sources::observe(mtm, switching);
                    if let Some(source) = crate::keyboard_input_sources::take_change(mtm) {
                        let _ = state.pump.runtime.send(Request::InputSourceChanged(source));
                    }
                }
                state.pump.tick(&mut state.indicator);
            }
        });
    });
    // SAFETY: the timer runs its block on this (main) thread's run loop.
    let timer = unsafe {
        NSTimer::scheduledTimerWithTimeInterval_repeats_block(PUMP_INTERVAL, true, &block)
    };
    // Keep firing while a menu is open (event-tracking run loop mode).
    // SAFETY: adding a scheduled timer to the main run loop's common modes.
    unsafe {
        let run_loop = objc2_foundation::NSRunLoop::mainRunLoop();
        run_loop.addTimer_forMode(&timer, objc2_foundation::NSRunLoopCommonModes);
    }
    PUMP.with(|slot| {
        *slot.borrow_mut() = Some(PumpState {
            pump,
            indicator,
            timer,
            _menu_target: target,
        });
    });
}

/// Whether the window is on screen.
fn window_visible() -> bool {
    WINDOW.get().is_some_and(rune::AppHandle::is_visible)
}

/// Opens the window at `destination`, in front of the app that was active.
fn show(window: &rune::AppHandle<Ui>, destination: Destination) {
    window.update(move |ui, tx| crate::rune_ui::show(ui, destination, tx));
    bring_forward(window);
}

/// Opens the window in front, from the main thread.
pub(crate) fn bring_window_forward() {
    with_window(bring_forward);
}

fn bring_forward(window: &rune::AppHandle<Ui>) {
    // An open window always shows the Dock icon: hiding it
    // would deactivate Hex and drop the window behind other apps.
    crate::app_settings::set_dock_icon_visible(true);
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        follow_active_space(&app);
        app.activate();
    }
    window.open();
}

/// Lets Hex's main window open on the Space the user is on, instead of
/// activating Hex switching back to the Space it was first shown on (#596).
/// Overlays and panels (which can't become main, or already join every
/// Space) are left alone: AppKit refuses `MoveToActiveSpace` beside
/// `CanJoinAllSpaces`.
fn follow_active_space(app: &NSApplication) {
    use objc2_app_kit::NSWindowCollectionBehavior;
    for window in app.windows().iter() {
        let behavior = window.collectionBehavior();
        if !window.canBecomeMainWindow()
            || behavior.contains(NSWindowCollectionBehavior::CanJoinAllSpaces)
            || behavior.contains(NSWindowCollectionBehavior::MoveToActiveSpace)
        {
            continue;
        }
        window.setCollectionBehavior(behavior | NSWindowCollectionBehavior::MoveToActiveSpace);
    }
}

impl Pump {
    fn tick(&mut self, indicator: &mut DictationIndicatorUi) {
        if self.shutdown.load(Ordering::Relaxed) {
            self.runtime_cancel();
            self.window.quit();
            return;
        }
        // Restart to update swapped in the new version after its final check.
        match UPDATER
            .get()
            .and_then(crate::updater::Updater::take_restart)
        {
            Some(Ok(())) => {
                self.window.quit();
                return;
            }
            Some(Err(error)) => {
                tracing::warn!(error = %format!("{error:#}"), "could not restart into the update");
            }
            None => {}
        }
        while let Ok(event) = self.indicator_events.try_recv() {
            indicator.handle(event);
        }
        indicator.follow_pointer();
        while let Ok(call) = self.developer_calls.try_recv() {
            let reply = self.developer(call.command, indicator);
            let _ = call.reply.send(reply);
        }
        while let Ok(call) = self.control_calls.try_recv() {
            self.control(call);
        }
        while let Some(action) = self
            .status_actions
            .as_ref()
            .and_then(|actions| actions.try_recv().ok())
        {
            self.status_action(action);
        }
        while let Ok(event) = self.controller_events.try_recv() {
            self.settle_meeting_waiters(Some(&event));
            self.controller_event(event);
            self.meeting_menu_at = Instant::now();
        }
        self.settle_meeting_waiters(None);
        #[cfg(hex_capture)]
        if let Some(overlays) = &mut self.capture_overlays
            && let Some(shown) = overlays.follow_recordings()
        {
            crate::macos_rune::status_item::update_overlays_in_recordings(shown);
        }
        if self.status_actions.is_none() {
            return;
        }
        let now = Instant::now();
        if now >= self.model_menu_at {
            if let Some(published) = self.runtime.published() {
                crate::macos_rune::status_item::update_transcription(
                    &published.settings,
                    &published.preparation,
                );
            }
            self.model_menu_at = now + MODEL_MENU_REFRESH;
        }
        if now >= self.meeting_menu_at {
            crate::macos_rune::status_item::update_meeting(
                &self.meeting_state,
                crate::app_settings::meetings_enabled(),
            );
            crate::macos_rune::status_item::update_software(&update_status());
            self.meeting_menu_at = now + MEETING_MENU_REFRESH;
        }
    }

    fn runtime_cancel(&self) {
        let _ = self.runtime.send(Request::CancelModel);
    }

    fn status_action(&mut self, action: StatusItemAction) {
        match action {
            StatusItemAction::OpenSettings => show(&self.window, Destination::Settings),
            StatusItemAction::OpenModels => show(&self.window, Destination::Models),
            StatusItemAction::SelectModel(model) => {
                let _ = self.runtime.send(Request::SelectInstalledModel(model));
                self.model_menu_at = Instant::now();
            }
            StatusItemAction::CancelModelPreparation => {
                let _ = self.runtime.send(Request::CancelModel);
                self.model_menu_at = Instant::now();
            }
            StatusItemAction::PasteLast => {
                let _ = self
                    .runtime
                    .recognition(RecognitionControl::PasteLast { reply: None });
            }
            // Labs › Copy recent dictations: copied, never pasted.
            StatusItemAction::CopyRecentDictation(id) => {
                if !crate::labs::recent_dictations() {
                    return;
                }
                let copied = self
                    .runtime
                    .history()
                    .and_then(|history| history.final_text(id))
                    .ok_or_else(|| eyre!("that dictation is no longer in History"))
                    .and_then(|text| {
                        arboard::Clipboard::new()?.set_text(text)?;
                        Ok(())
                    });
                if let Err(error) = copied {
                    tracing::warn!(%error, "could not copy a recent dictation");
                }
            }
            StatusItemAction::Update => {
                if let Some(updater) = UPDATER.get()
                    && let Err(error) = update_action(&updater.menu_action())
                {
                    tracing::warn!(error = %format!("{error:#}"), "the update menu item failed");
                }
            }
            StatusItemAction::StartMeeting => {
                if let Err(error) = self.runtime.meeting(MeetingRequest::Start { title: None }) {
                    tracing::warn!(%error, "could not start meeting from status item");
                }
            }
            StatusItemAction::StopMeeting => {
                if let Err(error) = self.runtime.meeting(MeetingRequest::Stop) {
                    tracing::warn!(%error, "could not stop meeting from status item");
                }
            }
            StatusItemAction::CopyLastMeetingTranscript => {
                let copied = crate::meeting::latest_transcript_markdown().and_then(|markdown| {
                    arboard::Clipboard::new()?.set_text(markdown)?;
                    Ok(())
                });
                if let Err(error) = copied {
                    tracing::warn!(%error, "could not copy last meeting transcript");
                }
            }
            StatusItemAction::CopyChapter(index) => {
                let MeetingMenuState::Recording { id, .. } = &self.meeting_state else {
                    return;
                };
                let copied = crate::meeting::section_for_id(id, index).and_then(|markdown| {
                    arboard::Clipboard::new()?.set_text(markdown)?;
                    Ok(())
                });
                if let Err(error) = copied {
                    tracing::warn!(%error, "could not copy a meeting section");
                }
            }
            StatusItemAction::OpenMeetings => show(&self.window, Destination::Meetings(None)),
            #[cfg(hex_capture)]
            StatusItemAction::ToggleOverlaysInRecordings => self.toggle_overlays_in_recordings(),
            StatusItemAction::Quit => {
                self.runtime_cancel();
                self.window.quit();
            }
        }
    }

    /// Saves the flipped setting as the window would; the overlays and the
    /// menu item follow it from [`Pump::tick`].
    #[cfg(hex_capture)]
    fn toggle_overlays_in_recordings(&self) {
        let saved = AppSettings::load().and_then(|mut settings| {
            settings.capture.show_overlays_in_recordings =
                !settings.capture.show_overlays_in_recordings;
            settings.save()
        });
        match saved {
            Ok(()) => {
                let _ = self.runtime.send(Request::ReloadSettings);
            }
            Err(error) => {
                tracing::warn!(error = %format!("{error:#}"), "could not save the recordings setting");
            }
        }
    }

    fn controller_event(&mut self, event: ControllerEvent) {
        self.capsules.event(&event);
        match event {
            ControllerEvent::Offer(_)
            | ControllerEvent::Reminder { .. }
            | ControllerEvent::ReminderWithdrawn { .. } => {}
            ControllerEvent::Starting => self.meeting_state = MeetingMenuState::Starting,
            ControllerEvent::RecordingStarted { id, title, .. } => {
                self.meeting_state = MeetingMenuState::Recording {
                    started: Instant::now(),
                    id,
                    title,
                    chapters: Vec::new(),
                };
            }
            ControllerEvent::Chapters(chapters) => {
                if let MeetingMenuState::Recording {
                    chapters: shown, ..
                } = &mut self.meeting_state
                {
                    *shown = chapters;
                }
            }
            ControllerEvent::Retitled { title } => {
                if let MeetingMenuState::Recording { title: shown, .. } = &mut self.meeting_state {
                    *shown = title;
                }
            }
            ControllerEvent::OpenMeeting(id) => {
                show(&self.window, Destination::Meetings(Some(id)));
            }
            ControllerEvent::CallEnded { app } => {
                tracing::info!(%app, "the call ended; the recording continues until Stop");
            }
            ControllerEvent::Transcribing => self.meeting_state = MeetingMenuState::Transcribing,
            ControllerEvent::Finished(manifest) => {
                tracing::info!(id = %manifest.id, "meeting transcript ready");
                self.meeting_state = MeetingMenuState::Idle;
            }
            ControllerEvent::NotesStarted | ControllerEvent::NotesFinished(_) => {}
            ControllerEvent::Failed { title, error } => {
                tracing::error!(%title, %error, "meeting failed");
                self.meeting_state = MeetingMenuState::Idle;
                let message = format!("{title}: {error}");
                self.window
                    .update(move |ui, _| crate::rune_ui::meeting_failed(ui, message));
                show(&self.window, Destination::Meetings(None));
            }
        }
    }

    fn developer(
        &mut self,
        command: crate::control::developer_control::DeveloperCommand,
        indicator: &mut DictationIndicatorUi,
    ) -> crate::control::developer_control::DeveloperReply {
        use crate::control::developer_control::{
            DeveloperCommand, DeveloperHudState, DeveloperPane, DeveloperReply,
        };
        match command {
            DeveloperCommand::Status => DeveloperReply::State {
                window_open: window_visible(),
                pane: None,
            },
            DeveloperCommand::Hud { state } => {
                indicator.handle(DictationIndicatorEvent::Reset);
                match state {
                    DeveloperHudState::Reset => {}
                    DeveloperHudState::Recording => {
                        indicator.handle(DictationIndicatorEvent::Started);
                        indicator.handle(DictationIndicatorEvent::Meter {
                            average: 0.08,
                            peak: 0.42,
                        });
                    }
                    DeveloperHudState::Transcribing => {
                        indicator.handle(DictationIndicatorEvent::Submitted { job_id: 0 });
                        indicator.handle(DictationIndicatorEvent::Transcribing { job_id: 0 });
                    }
                    DeveloperHudState::Processing => {
                        indicator.handle(DictationIndicatorEvent::Submitted { job_id: 0 });
                        indicator.handle(DictationIndicatorEvent::Processing { job_id: 0 });
                    }
                }
                DeveloperReply::Ok
            }
            DeveloperCommand::ShowPane { pane } => {
                let destination = match pane {
                    DeveloperPane::Settings => Destination::Settings,
                    DeveloperPane::Modes => Destination::Modes,
                    DeveloperPane::History => Destination::History,
                    DeveloperPane::Meetings => Destination::Meetings(None),
                    other => {
                        return DeveloperReply::error(
                            "pane-unavailable",
                            format!("the Rune window has no {other:?} pane"),
                        );
                    }
                };
                show(&self.window, destination);
                DeveloperReply::Ok
            }
        }
    }
}

objc2::define_class!(
    /// The main menu's, the reopen Apple event's, and AppKit terminate's target.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    struct MenuTarget;

    unsafe impl NSObjectProtocol for MenuTarget {}

    impl MenuTarget {
        #[unsafe(method(showSettings:))]
        fn show_settings(&self, _sender: Option<&AnyObject>) {
            with_window(|window| show(window, Destination::Settings));
        }

        #[unsafe(method(checkForUpdates:))]
        fn check_for_updates(&self, _sender: Option<&AnyObject>) {
            let _ = update_action(&crate::desktop::host::DesktopAction::CheckForUpdates);
        }

        #[unsafe(method(hideHex:))]
        fn hide_hex(&self, _sender: Option<&AnyObject>) {
            crate::app_settings::hide_application();
        }

        #[unsafe(method(quitHex:))]
        fn quit_hex(&self, _sender: Option<&AnyObject>) {
            with_window(|window| window.quit());
        }

        #[unsafe(method(closeWindow:))]
        fn close_window(&self, _sender: Option<&AnyObject>) {
            with_window(|window| window.close());
        }

        #[unsafe(method(togglePlayground:))]
        fn toggle_playground(&self, _sender: Option<&AnyObject>) {
            if crate::rune_ui::playground::enabled() {
                with_window(|window| window.update(|_, tx| tx.toggle_playground()));
            }
        }

        #[unsafe(method(minimizeWindow:))]
        fn minimize_window(&self, _sender: Option<&AnyObject>) {
            let mtm = self.mtm();
            if let Some(window) = NSApplication::sharedApplication(mtm).keyWindow() {
                window.miniaturize(None);
            }
        }

        #[unsafe(method(handleReopen:withReplyEvent:))]
        fn handle_reopen(&self, _event: Option<&AnyObject>, _reply: Option<&AnyObject>) {
            with_window(|window| show(window, Destination::Settings));
        }

        /// `-[NSApplication terminate:]` calls `exit` right after this.
        #[unsafe(method(applicationWillTerminate:))]
        fn application_will_terminate(&self, _notification: Option<&AnyObject>) {
            crate::desktop::process_exit::finish(&DRAIN);
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's init on a freshly allocated instance.
        unsafe { msg_send![super(this), init] }
    }
}

/// Runs `f` on the AppKit main thread through the window's event loop.
pub(crate) fn on_main_thread(f: impl FnOnce() + Send + 'static) {
    with_window(|window| window.update(move |_, _| f()));
}

fn with_window(f: impl FnOnce(&rune::AppHandle<Ui>)) {
    if let Some(window) = WINDOW.get() {
        f(window);
    }
}

/// The menu bar while the window is active: Hex, File, View (with the
/// Labs), Window.
/// Rune draws no Edit menu items, so ⌘C/⌘V reach its text fields as keys.
fn install_main_menu(target: &MenuTarget, mtm: MainThreadMarker) {
    let main = NSMenu::new(mtm);
    let name = crate::app_paths::app_name();
    let (hide, quit) = (format!("Hide {name}"), format!("Quit {name}"));
    let mut application = vec![];
    if !crate::updater::managed_by_policy() {
        application.push(Some(("Check for Updates…", sel!(checkForUpdates:), "")));
        application.push(None);
    }
    application.extend([
        Some(("Settings…", sel!(showSettings:), ",")),
        None,
        Some((hide.as_str(), sel!(hideHex:), "h")),
        None,
        Some((quit.as_str(), sel!(quitHex:), "q")),
    ]);
    let mut menus = vec![
        (name, application),
        (
            "File",
            vec![Some(("Close Window", sel!(closeWindow:), "w"))],
        ),
    ];
    // Rune's own View › Playground item, which this menu bar replaces. A
    // capital letter adds Shift: ⇧⌘D. It opens the playground only while
    // Labs › Playground is on.
    menus.push((
        "View",
        vec![Some(("Playground", sel!(togglePlayground:), "D"))],
    ));
    menus.push((
        "Window",
        vec![Some(("Minimize", sel!(minimizeWindow:), "m"))],
    ));
    for (title, items) in menus {
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
        for item in items {
            match item {
                Some((title, action, key)) => {
                    menu.addItem(&menu_item(target, title, action, key, mtm));
                }
                None => menu.addItem(&NSMenuItem::separatorItem(mtm)),
            }
        }
        let heading = NSMenuItem::new(mtm);
        heading.setSubmenu(Some(&menu));
        main.addItem(&heading);
    }
    NSApplication::sharedApplication(mtm).setMainMenu(Some(&main));
}

fn menu_item(
    target: &MenuTarget,
    title: &str,
    action: Sel,
    key: &str,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    // SAFETY: a plain menu item whose action the target implements.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    };
    item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    // SAFETY: the target lives as long as the app (kept in PUMP).
    unsafe { item.setTarget(Some(target)) };
    item
}

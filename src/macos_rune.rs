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
//! `_exit` ([`crate::process_exit`]). AppKit's own terminate
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
use crate::desktop_host::{
    DesktopAction, DesktopListenerSnapshot, DesktopShortcut, DesktopShortcuts,
    DesktopTranscriptionSnapshot, DesktopUpdateStatus,
};
use crate::dictation_indicator::{DictationIndicatorEvent, DictationIndicatorUi};
use crate::meeting::MeetingRequest;
use crate::meeting_watcher::{ControllerEvent, ListenerConfig, MeetingMenuState};
use crate::recognition::RecognitionControl;
use crate::rune_ui::{Destination, Ui};
use crate::status_item::StatusItemAction;
use crate::transcription_models::{
    ModelPreparationStage, TranscriptionModelId, TranscriptionSelection,
};
use crate::transcription_preparation::TranscriptionPreparation;

mod control;

/// How often the runtime refreshes while nothing is asked.
const RUNTIME_TICK: Duration = Duration::from_millis(250);
/// The main-loop timer: the HUD follows the pointer and channels drain.
const PUMP_INTERVAL: f64 = 0.016;
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
    CancelModel,
    /// The window saved `settings.json`; adopt it.
    ReloadSettings,
}

/// A shortcut the listener registers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ShortcutTarget {
    #[default]
    Dictation,
    /// The optional second physical trigger for dictation.
    DictationAlternate(usize),
    PasteLast,
    VoiceAction,
}

/// What the runtime last published for the window and the menu bar.
#[derive(Clone, PartialEq)]
pub(crate) struct Published {
    pub(crate) listener: DesktopListenerSnapshot,
    pub(crate) transcription: DesktopTranscriptionSnapshot,
    pub(crate) update_status: DesktopUpdateStatus,
    pub(crate) settings: AppSettings,
    preparation: crate::transcription_preparation::PreparationStatus,
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
    indicator: crate::dictation_indicator::DictationIndicatorSender,
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

/// Carries out an update control from the window, a menu, or `hex`;
/// switching to a staged update quits so it can start.
pub(crate) fn update_action(action: &crate::desktop_host::DesktopAction) -> Result<()> {
    let updater = UPDATER
        .get()
        .ok_or_else(|| eyre!("the updater is not running"))?;
    if updater.act(action)? {
        quit();
    }
    Ok(())
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
        crate::status_item::installed(),
    ));
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

pub fn run(shutdown: &'static AtomicBool, listener: ListenerConfig) -> Result<()> {
    if MainThreadMarker::new().is_none() {
        return Err(eyre!("desktop startup requires the main thread"));
    }
    shutdown.store(false, Ordering::Relaxed);
    crate::keyboard::initialize_layout()?;
    crate::updater::finish_on_launch();
    let _ = UPDATER.set(crate::updater::Updater::start());
    let settings = AppSettings::load()?;
    let setup_ready = crate::onboarding::status(&settings.transcription).ready();
    let onboarding_completed = crate::onboarding::completion_recorded();
    let history = match crate::history::History::open_default(settings.history_retention) {
        Ok(history) => Some(history),
        Err(error) => {
            tracing::warn!(%error, "dictation history is unavailable");
            None
        }
    };
    let event_path = listener.event_path.clone();
    let event_log = crate::events::EventLog::create(&event_path)?;
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
        },
    )?;
    let (controller_events_sender, controller_events) = mpsc::sync_channel(32);
    let (controller_commands, controller_command_receiver) = mpsc::sync_channel(8);
    let (meeting_requests, meeting_request_receiver) = mpsc::sync_channel(8);
    let (indicator, indicator_events) = crate::dictation_indicator::channel();

    // The controller idles without inspecting microphone activity until the
    // user opts in to meetings (always available in developer builds).
    let controller = thread::Builder::new()
        .name("hex-meetings".into())
        .spawn(move || {
            if let Err(error) = crate::meeting_watcher::controller_loop(
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
    let recognition_meetings = meeting_requests.clone();
    let recognition_history = history.clone();
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
            if let Err(error) = crate::recognition::listen(
                &listener.project_root,
                event_log,
                listener.device.as_deref(),
                crate::config::voice_control(),
                shutdown,
                Some(recognition_indicator.clone()),
                Some(recognition_meetings),
                recognition_history,
                Some(recognition_control_receiver),
            ) {
                tracing::error!(%error, "desktop recognition stopped");
                if format!("{error:#}").contains("event tap") {
                    crate::onboarding::note_listener_failure();
                }
                recognition_indicator.send(DictationIndicatorEvent::Failed);
            }
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
                request_receiver,
                &shared,
                shutdown,
                (!setup_ready).then_some(recognition_start),
            )
        })?;
    let mut app = crate::rune_ui::app(Default::default())
        .args(Vec::new())
        .accessory();
    // The first-run tour opens centred at one size; a tiling window manager
    // would otherwise stretch it to half the screen.
    if !onboarding_completed {
        app = app.window_options(crate::rune_ui::fixed_window());
    }
    let window = app.handle();
    let _ = WINDOW.set(window.clone());
    // The status item is created inside the loop; Hex can't run without a
    // way back to its window, so a failed item opens the window there.
    let open_at_launch = crate::meeting_watcher::should_open_app_on_launch(
        settings.show_dock_icon,
        setup_ready,
        onboarding_completed,
        true,
    );
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
    let pump = Pump {
        window: window.clone(),
        capsules,
        runtime: handle,
        shutdown,
        indicator_events,
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
        // The listener owns Moonshine and the transcription engine; joining
        // it frees both after their last use.
        for (name, worker) in [
            ("recognition", recognition),
            ("meeting controller", controller),
            ("runtime", runtime),
        ] {
            if worker.join().is_err() {
                tracing::error!("{name} worker panicked during shutdown");
            }
        }
        crate::recording_environment::wait_until_restored(Duration::from_secs(2));
        if let Err(error) = drained_event_log.flush_within(Duration::from_millis(500)) {
            tracing::warn!(%error, "could not flush the event log before quitting");
        }
    });
    // Runs first on the main thread once the loop (and NSApp) is live.
    window.update(move |_, _| start_main_thread(pump));
    app.run();

    PUMP.with(|slot| {
        if let Some(state) = slot.borrow_mut().take() {
            state.timer.invalidate();
        }
    });
    crate::process_exit::finish(&DRAIN)
}

/// Stops and joins the runtime, listener, meeting controller, and local API.
static DRAIN: crate::process_exit::Drain = crate::process_exit::Drain::new();

// ---------------------------------------------------------------------------
// Runtime thread

fn run_runtime(
    settings: AppSettings,
    requests: Receiver<Request>,
    shared: &Shared,
    shutdown: &AtomicBool,
    recognition_start: Option<SyncSender<()>>,
) {
    let mut runtime = Runtime {
        settings,
        preparation: TranscriptionPreparation::default(),
        recognition_start,
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

struct Runtime {
    settings: AppSettings,
    preparation: TranscriptionPreparation,
    /// Starts the listener once setup is ready; `None` once started.
    recognition_start: Option<SyncSender<()>>,
}

impl Runtime {
    fn handle(&mut self, request: Request) {
        match request {
            Request::ChooseModel { model, language } => {
                let definition = crate::transcription_models::definition(model);
                let selection = TranscriptionSelection {
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
            Request::SelectInstalledModel(model) => {
                self.reload_settings();
                self.preparation
                    .start(self.settings.transcription_for_model(model), true);
            }
            Request::UseInstalledSelection(selection) => self.preparation.start(selection, true),
            Request::CancelModel => self.preparation.cancel(),
            Request::ReloadSettings => self.reload_settings(),
            Request::Desktop(_)
            | Request::CaptureShortcutFor(_)
            | Request::CancelShortcut
            | Request::SetShortcut { .. } => {}
        }
    }

    fn reload_settings(&mut self) {
        match AppSettings::load() {
            Ok(settings) => self.settings = settings,
            Err(error) => tracing::warn!(%error, "could not reload settings"),
        }
    }

    /// Commits a prepared model and starts a listener that waited for setup.
    fn tick(&mut self) {
        if let Some(result) = self.preparation.poll() {
            let result = result.and_then(|selection| {
                crate::transcription_models::validate(&selection).map_err(|e| e.to_string())?;
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
            && crate::onboarding::status(&self.settings.transcription).ready()
            && let Some(start) = self.recognition_start.take()
        {
            let _ = start.try_send(());
        }
    }

    fn publish(&self) -> Published {
        let preparation = self.preparation.status();
        let running = self.recognition_start.is_none();
        Published {
            listener: DesktopListenerSnapshot {
                running,
                status: if running {
                    "Enabled"
                } else {
                    "Waiting for setup"
                }
                .into(),
                warning: None,
                shortcuts: DesktopShortcuts::Registered,
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
    runtime: RuntimeHandle,
    shutdown: &'static AtomicBool,
    indicator_events: Receiver<DictationIndicatorEvent>,
    status_actions: Option<Receiver<StatusItemAction>>,
    controller_events: Receiver<ControllerEvent>,
    developer_calls: Receiver<crate::developer_control::DeveloperCall>,
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
}

fn start_main_thread(mut pump: Pump) {
    let Some(mtm) = MainThreadMarker::new() else {
        tracing::error!("Hex's main-thread setup ran off the main thread");
        return;
    };
    pump.status_actions = match crate::status_item::install() {
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
    let indicator = DictationIndicatorUi::new();
    let target = MenuTarget::new(mtm);
    install_main_menu(&target, mtm);
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
        let _: () = msg_send![
            &*objc2_foundation::NSNotificationCenter::defaultCenter(),
            addObserver: &*target,
            selector: sel!(applicationWillTerminate:),
            name: objc2_app_kit::NSApplicationWillTerminateNotification,
            object: std::ptr::null::<AnyObject>(),
        ];
    }
    let block = RcBlock::new(|_: std::ptr::NonNull<NSTimer>| {
        PUMP.with(|slot| {
            if let Some(state) = slot.borrow_mut().as_mut() {
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
    // An open window always shows the Dock icon: hiding it
    // would deactivate Hex and drop the window behind other apps.
    crate::app_settings::set_dock_icon_visible(true);
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm).activate();
    }
    window.open();
}

impl Pump {
    fn tick(&mut self, indicator: &mut DictationIndicatorUi) {
        if self.shutdown.load(Ordering::Relaxed) {
            self.runtime_cancel();
            self.window.quit();
            return;
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
        if self.status_actions.is_none() {
            return;
        }
        let now = Instant::now();
        if now >= self.model_menu_at {
            if let Some(published) = self.runtime.published() {
                crate::status_item::update_transcription(
                    &published.settings,
                    &published.preparation,
                );
            }
            self.model_menu_at = now + MODEL_MENU_REFRESH;
        }
        if now >= self.meeting_menu_at {
            crate::status_item::update_meeting(
                &self.meeting_state,
                crate::app_settings::meetings_enabled(),
            );
            crate::status_item::update_software(&update_status());
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
            StatusItemAction::Quit => {
                self.runtime_cancel();
                self.window.quit();
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
        command: crate::developer_control::DeveloperCommand,
        indicator: &mut DictationIndicatorUi,
    ) -> crate::developer_control::DeveloperReply {
        use crate::developer_control::{
            DeveloperCommand, DeveloperHudState, DeveloperPane, DeveloperReply,
        };
        match command {
            DeveloperCommand::Status => DeveloperReply::State {
                window_open: window_visible(),
                pane: None,
                commands_enabled: crate::app_settings::commands_enabled(),
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
                    DeveloperPane::Commands => Destination::Commands,
                    DeveloperPane::VoiceAction => Destination::VoiceAction,
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
            DeveloperCommand::SetCommandsEnabled { enabled } => {
                let mut settings = match AppSettings::load() {
                    Ok(settings) => settings,
                    Err(error) => {
                        return DeveloperReply::error("settings-load-failed", error.to_string());
                    }
                };
                if let Err(error) = settings.set_commands_enabled(enabled) {
                    return DeveloperReply::error("settings-conflict", error.to_string());
                }
                match settings.save() {
                    Ok(()) => {
                        let _ = self.runtime.send(Request::ReloadSettings);
                        DeveloperReply::Ok
                    }
                    Err(error) => DeveloperReply::error("settings-save-failed", error.to_string()),
                }
            }
        }
    }
}

objc2::define_class!(
    /// The main menu's and the reopen Apple event's target.
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
            let _ = update_action(&crate::desktop_host::DesktopAction::CheckForUpdates);
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
            crate::process_exit::finish(&DRAIN);
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

/// The menu bar while the window is active: Hex, File, Window.
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
    for (title, items) in [
        (name, application),
        (
            "File",
            vec![Some(("Close Window", sel!(closeWindow:), "w"))],
        ),
        (
            "Window",
            vec![Some(("Minimize", sel!(minimizeWindow:), "m"))],
        ),
    ] {
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

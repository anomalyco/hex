//! Hex on macOS (`hex app`): one menu-bar process.
//!
//! - The main thread runs Rune's event loop (`rune::App`, windowless and
//!   kept running, as an accessory app) and, beside it on the same AppKit
//!   run loop, the native status item, the updater, the main menu, the Dock
//!   reopen handler, and the Metal dictation HUD. A main-loop timer
//!   ([`Pump`]) drives them, so they keep working with no window open: every
//!   frame (16 ms) while the HUD animates, otherwise every 250 ms, and
//!   whatever sends it work (HUD events, menu actions, control calls,
//!   meeting events, input-source changes) wakes it at once ([`wake_pump`]).
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

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::Result;
use color_eyre::eyre::eyre;
use objc2_foundation::{MainThreadMarker, NSString};

use crate::app_settings::AppSettings;
use crate::desktop::host::{
    DesktopAction, DesktopListenerSnapshot, DesktopShortcut, DesktopTranscriptionSnapshot,
    DesktopUpdateStatus,
};
use crate::dictation::indicator::DictationIndicatorEvent;
use crate::macos_rune::status_item::StatusItemAction;
use crate::meeting::MeetingRequest;
use crate::meeting::watcher::{ControllerEvent, ListenerConfig, MeetingMenuState};
use crate::recognition::RecognitionControl;
use crate::rune_ui::Ui;
use crate::transcription::models::{TranscriptionModelId, TranscriptionSelection};

mod activation;
mod control;
mod launch;
mod main_menu;
mod main_thread;
mod runtime;
pub(crate) mod status_item;
mod wake;

use launch::{LaunchWindow, observe_launch};
use main_thread::{
    LAUNCH, Launch, MenuTarget, PUMP, observe_termination, start_main_thread,
    start_main_thread_after,
};
use runtime::run_runtime;
use wake::relay;

pub(crate) use main_thread::{bring_window_forward, on_main_thread};
pub(crate) use wake::wake_pump;

#[cfg(test)]
use wake::pump_interval;

#[cfg(test)]
mod tests;

/// How often the runtime refreshes while nothing is asked.
const RUNTIME_TICK: Duration = Duration::from_millis(250);
/// Seconds a window opened at launch has to draw its first frame before the
/// status item, HUD, and menu are set up anyway.
const LAUNCH_SETUP_DEADLINE: f64 = 1.0;

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
    /// Labs › Annotate 15 s replay shortcut.
    #[cfg(hex_capture)]
    CaptureReplay,
    RawDictation,
    /// Labs › Second dictation shortcut.
    SecondDictation,
    /// Toggle mode's cancel shortcut.
    Cancel,
    /// Labs › Screenshots.
    Screenshot,
    /// Labs › Screenshots' record shortcut.
    ScreenshotRecord,
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
    /// The listener's event log, which the runtime reads and the tour shares.
    event_path: PathBuf,
    /// When this run began, in the event log's wall-clock ms, captured before
    /// the listener thread starts.
    started_at: u64,
}

impl Shared {
    fn event_path(&self) -> &Path {
        &self.event_path
    }
}

/// The window's and the menu bar's connection to the runtime.
#[derive(Clone)]
pub(crate) struct RuntimeHandle {
    requests: Sender<Request>,
    shared: Arc<Shared>,
    meeting_requests: SyncSender<MeetingRequest>,
    recognition_controls: SyncSender<RecognitionControl>,
    history: Option<crate::history::History>,
    indicator: crate::dictation::indicator::DictationIndicatorSender,
}

impl RuntimeHandle {
    /// The listener's event log, which the tour reads for practice dictations.
    pub(crate) fn event_path(&self) -> &Path {
        self.shared.event_path()
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

/// Asks a running 2.3.1 or Swift Hex to quit once this Hex is ready to
/// listen, so the two never answer the same shortcut. The old app stays
/// installed.
fn quit_legacy_hex() {
    objc2::rc::autoreleasepool(|_| {
        for bundle_id in crate::desktop::onboarding::LEGACY_MAC_BUNDLE_IDS {
            for app in objc2_app_kit::NSRunningApplication::runningApplicationsWithBundleIdentifier(
                &NSString::from_str(bundle_id),
            ) {
                if app.terminate() {
                    tracing::info!(bundle_id, "asked the old Hex to quit");
                } else {
                    tracing::warn!(bundle_id, "the old Hex did not agree to quit");
                }
            }
        }
    });
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

pub fn run(shutdown: &'static AtomicBool, listener: ListenerConfig, hidden: bool) -> Result<()> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Err(eyre!("desktop startup requires the main thread"));
    };
    shutdown.store(false, Ordering::Relaxed);
    watch_process();
    // Observing first means a source change during startup still rebuilds
    // the snapshot at the pump's first turn.
    crate::keyboard_input_sources::start(mtm);
    crate::keyboard::initialize_layout()?;
    crate::updater::finish_on_launch();
    let _ = UPDATER.set(crate::updater::Updater::start());
    // Never a silent launch failure: invalid settings start with defaults
    // and the window says why (#744).
    let launch = AppSettings::load_for_launch();
    let mut settings = launch.settings;
    let settings_error = launch
        .note
        .map(crate::app_settings::LaunchNote::into_message);
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
    let transformations = Arc::new(crate::personal_commands::TransformationClient::default());
    let (developer_sender, developer_receiver) = mpsc::sync_channel(8);
    let (control_sender, control_receiver) = mpsc::sync_channel(8);
    let (recognition_controls, recognition_control_receiver) = mpsc::sync_channel(8);
    let mut local_api = crate::local_api::LocalApi::start(
        event_log.clone(),
        crate::local_api::LocalApiOptions {
            developer_control: crate::DEVELOPER_FEATURES_ENABLED.then(|| developer_sender.clone()),
            dictation_control: Some(recognition_controls.clone()),
            control: Some(control_sender),
            processing: crate::local_api::processing::profiles_processing(transformations.clone()),
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
    let started_at = crate::desktop::events::now_ms();
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
            quit_legacy_hex();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::recognition::listen(
                    event_log,
                    listener.device.as_deref(),
                    shutdown,
                    Some(recognition_indicator.clone()),
                    recognition_history,
                    Some(recognition_control_receiver),
                    transformations,
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
            recognition_indicator.send(DictationIndicatorEvent::Failed { message: None });
        })?;

    let (requests, request_receiver) = mpsc::channel();
    let shared = Arc::new(Shared {
        event_path: event_path.clone(),
        started_at,
        ..Shared::default()
    });
    let handle = RuntimeHandle {
        requests: requests.clone(),
        shared: shared.clone(),
        meeting_requests: meeting_requests.clone(),
        recognition_controls: recognition_controls.clone(),
        history: history.clone(),
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
    // With the Dock icon shown, Hex starts windowless and opens the window
    // once AppKit says the launch wasn't a login item.
    let launch_window = if settings_error.is_some() {
        LaunchWindow::Open
    } else {
        LaunchWindow::decide(
            hidden,
            settings.show_dock_icon,
            setup_ready,
            onboarding_completed,
            true,
        )
    };
    let open_at_launch = launch_window == LaunchWindow::Open;
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
    // Labs › Screenshots: its own picker and the thumbnail.
    #[cfg(hex_capture)]
    crate::screenshots::install(&mut app);
    // `hex dictation` and `hex paste-last` go straight to the listener; the
    // rest reach the main thread's pump.
    let (pump_calls, pump_call_receiver) = mpsc::sync_channel(8);
    let control_runtime = handle.clone();
    thread::Builder::new()
        .name("hex-control".into())
        .spawn(move || {
            for call in control_receiver {
                if let Some(call) = control::answer_off_main(&control_runtime, call) {
                    if pump_calls.send(call).is_err() {
                        return;
                    }
                    wake_pump();
                }
            }
        })?;
    // The pump idles between events, so its other senders wake it.
    let controller_events = relay("hex-meeting-events", controller_events, 32)?;
    let developer_receiver = relay("hex-developer-calls", developer_receiver, 8)?;
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
        control_calls: pump_call_receiver,
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
    if launch_window == LaunchWindow::OpenUnlessLoginItem {
        observe_launch(&target);
    }
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

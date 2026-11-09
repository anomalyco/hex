//! The AppKit main thread beside Rune's loop: the status item, the main
//! menu, the Dock reopen handlers, the HUD, and the pump timer that drives
//! them ([`start_main_thread`]). It reads what the runtime publishes and
//! never waits on another thread.

use std::cell::RefCell;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use block2::RcBlock;
use color_eyre::eyre::eyre;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{MainThreadOnly, msg_send, sel};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::{MainThreadMarker, NSAppleEventManager, NSObject, NSTimer};

use crate::desktop::host::DesktopAction;
use crate::dictation::indicator::DictationIndicatorEvent;
use crate::meeting::MeetingRequest;
use crate::meeting::watcher::{ControllerEvent, MeetingMenuState};
use crate::recognition::RecognitionControl;
use crate::rune_hud::DictationIndicatorUi;
use crate::rune_ui::{Destination, Ui};

use super::activation::{observe_activation, reopen_on_activation};
use super::launch::{LaunchWindow, launched_as_login_item, take_login_launch_activation};
use super::main_menu::install_main_menu;
use super::status_item::StatusItemAction;
use super::wake::{pump_now, relay, start_pump_timer};
use super::{DRAIN, Pump, Request, UPDATER, WINDOW, update_action, update_status};

const MODEL_MENU_REFRESH: Duration = Duration::from_secs(1);
const MEETING_MENU_REFRESH: Duration = Duration::from_millis(500);
/// 'aevt' / 'rapp': the Apple event LaunchServices sends when the app is
/// opened again while it runs (Dock, Finder, `open`).
const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
const REOPEN_APPLICATION: u32 = u32::from_be_bytes(*b"rapp");

pub(super) struct PumpState {
    pub(super) pump: Pump,
    /// Created on the main thread, so it stays out of the `Send` [`Pump`].
    pub(super) indicator: DictationIndicatorUi,
    pub(super) timer: Retained<NSTimer>,
    /// The timer runs every frame, not at the idle interval ([`super::wake`]).
    pub(super) fast: bool,
    _menu_target: Retained<MenuTarget>,
}

thread_local! {
    pub(super) static PUMP: RefCell<Option<PumpState>> = const { RefCell::new(None) };
    /// What [`start_main_thread`] takes, once.
    pub(super) static LAUNCH: RefCell<Option<Launch>> = const { RefCell::new(None) };
}

pub(super) struct Launch {
    pub(super) pump: Pump,
    /// Already observing AppKit's terminate ([`observe_termination`]).
    pub(super) target: Retained<MenuTarget>,
}

/// AppKit's terminate (the quit Apple event, logout) calls `exit` right after
/// this notification, so Hex finishes there ([`crate::desktop::process_exit`]).
pub(super) fn observe_termination(target: &MenuTarget) {
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
pub(super) fn start_main_thread_after(seconds: f64) {
    let block = RcBlock::new(|_: std::ptr::NonNull<NSTimer>| start_main_thread());
    // SAFETY: the timer runs its block on this (main) thread's run loop.
    let _ =
        unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(seconds, false, &block) };
}

/// Sets up the status item, Dock policy, HUD, main menu, reopen handler, and
/// the pump's timer; the first call does, later ones do nothing.
pub(super) fn start_main_thread() {
    let Some(mtm) = MainThreadMarker::new() else {
        tracing::error!("Hex's main-thread setup ran off the main thread");
        return;
    };
    let Some(Launch { mut pump, target }) = LAUNCH.with(|slot| slot.borrow_mut().take()) else {
        return;
    };
    pump.status_actions = match crate::macos_rune::status_item::install().and_then(|actions| {
        relay("hex-status-actions", actions, 8).map_err(color_eyre::Report::from)
    }) {
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
    observe_activation(&target);
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
    let fast = indicator.animating();
    let timer = start_pump_timer(fast);
    PUMP.with(|slot| {
        *slot.borrow_mut() = Some(PumpState {
            pump,
            indicator,
            timer,
            fast,
            _menu_target: target,
        });
    });
    // Anything sent before the pump existed is drained now.
    pump_now();
}

/// Rebuilds the keyboard layout snapshot after the selected input source
/// changed, here on the main thread, so a later ⌘V posts the key that types
/// V now (t1304). Shortcuts on letter keys follow it; the listener rebinds a
/// shortcut only once its machine is idle, so a recording goes on.
pub(super) fn refresh_keyboard_layout(mtm: MainThreadMarker) {
    match crate::keyboard::refresh_layout(mtm) {
        Ok(true) => {
            tracing::info!("keyboard layout changed");
            crate::app_settings::refresh_layout_hotkeys();
        }
        Ok(false) => {}
        Err(error) => tracing::warn!(%error, "could not read the new keyboard layout"),
    }
}

/// Whether the window is on screen.
pub(super) fn window_visible() -> bool {
    WINDOW.get().is_some_and(rune::AppHandle::is_visible)
}

/// Opens the window at `destination`, in front of the app that was active.
pub(super) fn show(window: &rune::AppHandle<Ui>, destination: Destination) {
    window.update(move |ui, tx| crate::rune_ui::show(ui, destination, tx));
    bring_forward(window);
}

/// Opens the window where its footer shows the update check's outcome.
fn show_update_check(window: &rune::AppHandle<Ui>) {
    window.update(|ui, _| crate::rune_ui::update_check_asked(ui));
    show(window, Destination::Settings);
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
        app.activate();
    }
    window.open();
}

impl Pump {
    pub(super) fn tick(&mut self, indicator: &mut DictationIndicatorUi) {
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
        if let Some(overlays) = &mut self.capture_overlays {
            overlays.follow_recordings();
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
                crate::macos_rune::status_item::update_paste_last(&published.settings);
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
                let Some(updater) = UPDATER.get() else {
                    return;
                };
                let action = updater.menu_action();
                // A check from the menu bar opens the window, whose footer
                // says how it went: up to date or couldn't check (#1120).
                if action == DesktopAction::CheckForUpdates {
                    show_update_check(&self.window);
                }
                if let Err(error) = update_action(&action) {
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
                    warning: None,
                };
            }
            ControllerEvent::SourceWarning { warning } => {
                self.meeting_state.source_warning(warning)
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
            ControllerEvent::CallEnded { app, auto_stop } => {
                if auto_stop {
                    tracing::info!(%app, "the call ended; the recording stops after its grace unless kept");
                } else {
                    tracing::info!(%app, "the call ended; the recording continues until Stop");
                }
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
            ControllerEvent::ImportFailed { error } => {
                tracing::error!(%error, "meeting audio import failed");
                let message = error.clone();
                self.window
                    .update(move |ui, _| crate::rune_ui::meeting_import_failed(ui, message));
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
    pub(super) struct MenuTarget;

    unsafe impl NSObjectProtocol for MenuTarget {}

    impl MenuTarget {
        #[unsafe(method(showSettings:))]
        fn show_settings(&self, _sender: Option<&AnyObject>) {
            with_window(|window| show(window, Destination::Settings));
        }

        #[unsafe(method(checkForUpdates:))]
        fn check_for_updates(&self, _sender: Option<&AnyObject>) {
            with_window(show_update_check);
            let _ = update_action(&crate::desktop::host::DesktopAction::CheckForUpdates);
        }

        #[unsafe(method(hideHex:))]
        fn hide_hex(&self, _sender: Option<&AnyObject>) {
            crate::app_settings::hide_application();
        }

        #[unsafe(method(quitHex:))]
        fn quit_hex(&self, _sender: Option<&AnyObject>) {
            // Labs › ⌘Q closes Settings: the window hides and Hex keeps
            // dictating; the status item's Quit Hex still quits.
            if crate::labs::quit_closes_window() {
                with_window(|window| window.close());
            } else {
                with_window(|window| window.quit());
            }
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

        /// Observed only when the Dock icon is shown and setup is done: a
        /// manual launch opens the window, a login-item launch stays quiet.
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn application_did_finish_launching(&self, _notification: Option<&AnyObject>) {
            let login_item = launched_as_login_item();
            if LaunchWindow::OpenUnlessLoginItem.opens(login_item) {
                with_window(bring_forward);
            } else {
                tracing::info!("launched as a login item; starting without the window");
            }
        }

        /// ⌘Tab (or any activation) with the window closed opens it where
        /// it was, as clicking the Dock icon does.
        #[unsafe(method(applicationDidBecomeActive:))]
        fn application_did_become_active(&self, _notification: Option<&AnyObject>) {
            let app = NSApplication::sharedApplication(self.mtm());
            if !take_login_launch_activation() && reopen_on_activation(
                app.activationPolicy() == NSApplicationActivationPolicy::Regular,
                app.keyWindow().is_some(),
                window_visible(),
            ) {
                with_window(bring_forward);
            }
        }

        /// `-[NSApplication terminate:]` calls `exit` right after this.
        #[unsafe(method(applicationWillTerminate:))]
        fn application_will_terminate(&self, _notification: Option<&AnyObject>) {
            crate::desktop::process_exit::finish(&DRAIN);
        }
    }
);

impl MenuTarget {
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
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

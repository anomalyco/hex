//! The core's optional listen-only `CGEventTap` for mouse buttons ([`Input::Tap`](crate::Input)).
//! Hosts that run their own tap forward events instead and never start this one.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use objc2_core_foundation::{CFMachPort, CFRunLoop, kCFRunLoopDefaultMode};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventMask, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventType, CGPreflightListenEventAccess, CGRequestListenEventAccess,
};

use crate::cursor::{CursorButton, CursorButtonState, CursorClickCaptureStatus};
use crate::input::{InputLog, Modifiers, tap_button};
use crate::time::HostTime;

const MONITOR_START_TIMEOUT: Duration = Duration::from_secs(2);
const RUN_LOOP_POLL_INTERVAL_SECONDS: f64 = 0.05;

enum MonitorCommand {
    Stop,
}

trait ClickTapFactory: Send + Sync + 'static {
    fn preflight_access(&self) -> bool;
    fn request_access(&self) -> bool;
    fn run(
        &self,
        log: Arc<InputLog>,
        commands: Receiver<MonitorCommand>,
        ready: SyncSender<Result<(), ()>>,
    );
}

struct NativeClickTapFactory;

impl ClickTapFactory for NativeClickTapFactory {
    fn preflight_access(&self) -> bool {
        CGPreflightListenEventAccess()
    }

    fn request_access(&self) -> bool {
        CGRequestListenEventAccess()
    }

    fn run(
        &self,
        log: Arc<InputLog>,
        commands: Receiver<MonitorCommand>,
        ready: SyncSender<Result<(), ()>>,
    ) {
        run_native_tap(&log, &commands, &ready);
    }
}

pub(crate) struct ClickMonitor {
    log: Arc<InputLog>,
    commands: Option<mpsc::Sender<MonitorCommand>>,
    thread: Option<JoinHandle<()>>,
}

impl ClickMonitor {
    pub(crate) fn start(log: Arc<InputLog>) -> Self {
        Self::start_with(log, Arc::new(NativeClickTapFactory))
    }

    fn start_with<F>(log: Arc<InputLog>, factory: Arc<F>) -> Self
    where
        F: ClickTapFactory,
    {
        Self::start_with_timeout(log, factory, MONITOR_START_TIMEOUT)
    }

    fn start_with_timeout<F>(log: Arc<InputLog>, factory: Arc<F>, timeout: Duration) -> Self
    where
        F: ClickTapFactory,
    {
        let unavailable = |log: Arc<InputLog>| Self {
            log,
            commands: None,
            thread: None,
        };
        if !factory.preflight_access() && !factory.request_access() {
            log.collectors()
                .set_click_status(CursorClickCaptureStatus::PermissionDenied);
            return unavailable(log);
        }

        let (command_sender, commands) = mpsc::channel();
        let (ready_sender, ready) = mpsc::sync_channel(1);
        let thread_log = Arc::clone(&log);
        let thread = thread::Builder::new()
            .name("capture-click-monitor".into())
            .spawn(move || factory.run(thread_log, commands, ready_sender));
        let Ok(thread) = thread else {
            log.collectors().monitor_unavailable();
            return unavailable(log);
        };

        if !matches!(ready.recv_timeout(timeout), Ok(Ok(()))) {
            let _ = command_sender.send(MonitorCommand::Stop);
            // A timed-out worker may be wedged before it can observe Stop. Dropping the handle
            // detaches that failed startup instead of turning the timeout into an unbounded join.
            drop(thread);
            log.collectors().monitor_unavailable();
            return unavailable(log);
        }

        log.collectors()
            .set_click_status(CursorClickCaptureStatus::Active);
        Self {
            log,
            commands: Some(command_sender),
            thread: Some(thread),
        }
    }

    pub(crate) fn status(&self) -> CursorClickCaptureStatus {
        self.log.collectors().click_status()
    }

    pub(crate) fn stop(&mut self) -> bool {
        if let Some(commands) = self.commands.take() {
            let _ = commands.send(MonitorCommand::Stop);
        }
        let joined = self
            .thread
            .take()
            .is_none_or(|thread| thread.join().is_ok());
        if !joined {
            self.log.collectors().monitor_stop_failed();
        }
        joined
    }
}

impl Drop for ClickMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

fn event_host_time(event: &CGEvent) -> u64 {
    CGEvent::timestamp(Some(event))
}

fn modifiers(flags: CGEventFlags) -> Modifiers {
    Modifiers {
        command: flags.contains(CGEventFlags::MaskCommand),
        control: flags.contains(CGEventFlags::MaskControl),
        option: flags.contains(CGEventFlags::MaskAlternate),
        shift: flags.contains(CGEventFlags::MaskShift),
    }
}

struct TapContext {
    log: Arc<InputLog>,
    reenable_requested: AtomicBool,
}

unsafe extern "C-unwind" fn event_tap_callback(
    _proxy: objc2_core_graphics::CGEventTapProxy,
    event_type: CGEventType,
    event: std::ptr::NonNull<CGEvent>,
    user_info: *mut core::ffi::c_void,
) -> *mut CGEvent {
    // SAFETY: `user_info` is the `TapContext` the tap thread keeps alive while the tap exists.
    let context = unsafe { &*user_info.cast::<TapContext>() };
    if let Some(timeout) = tap_disable_was_timeout(event_type) {
        context.reenable_requested.store(true, Ordering::Release);
        context.log.collectors().tap_disabled(timeout);
        return event.as_ptr();
    }

    // SAFETY: The tap passes a live event for the duration of the callback.
    let event_ref = unsafe { event.as_ref() };
    let button_number =
        CGEvent::integer_value_field(Some(event_ref), CGEventField::MouseEventButtonNumber);
    if let Some((button, state)) = map_mouse_event(event_type, button_number) {
        let location = CGEvent::location(Some(event_ref));
        // The event's timestamp is on the same Mach clock as frame display times.
        context.log.record(
            HostTime::from_ticks(event_host_time(event_ref)),
            tap_button(
                button,
                state,
                location.x,
                location.y,
                modifiers(CGEvent::flags(Some(event_ref))),
            ),
        );
    }
    event.as_ptr()
}

fn run_native_tap(
    log: &Arc<InputLog>,
    commands: &Receiver<MonitorCommand>,
    ready: &SyncSender<Result<(), ()>>,
) {
    let mut context = Box::new(TapContext {
        log: Arc::clone(log),
        reenable_requested: AtomicBool::new(false),
    });
    let context_ptr = (&raw mut *context).cast();
    // SAFETY: The callback matches the tap signature and `context` outlives the tap, which is
    // disabled and removed before this function returns.
    let Some(tap) = (unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::HIDEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            click_event_mask(),
            Some(event_tap_callback),
            context_ptr,
        )
    }) else {
        let _ = ready.send(Err(()));
        return;
    };
    let Some(source) = CFMachPort::new_run_loop_source(None, Some(&tap), 0) else {
        let _ = ready.send(Err(()));
        return;
    };
    let Some(run_loop) = CFRunLoop::current() else {
        let _ = ready.send(Err(()));
        return;
    };
    // SAFETY: An immutable process-lifetime CoreFoundation constant.
    let mode = unsafe { kCFRunLoopDefaultMode };
    run_loop.add_source(Some(&source), mode);
    CGEvent::tap_enable(&tap, true);
    if ready.send(Ok(())).is_err() {
        CGEvent::tap_enable(&tap, false);
        run_loop.remove_source(Some(&source), mode);
        return;
    }

    loop {
        if matches!(
            commands.try_recv(),
            Ok(MonitorCommand::Stop) | Err(mpsc::TryRecvError::Disconnected)
        ) {
            break;
        }
        CFRunLoop::run_in_mode(mode, RUN_LOOP_POLL_INTERVAL_SECONDS, false);
        if context.reenable_requested.swap(false, Ordering::AcqRel) {
            CGEvent::tap_enable(&tap, true);
            let enabled = CGEvent::tap_is_enabled(&tap);
            log.collectors().tap_reenabled(enabled);
            if !enabled {
                break;
            }
        }
    }
    CGEvent::tap_enable(&tap, false);
    run_loop.remove_source(Some(&source), mode);
}

fn click_event_mask() -> CGEventMask {
    [
        CGEventType::LeftMouseDown,
        CGEventType::LeftMouseUp,
        CGEventType::RightMouseDown,
        CGEventType::RightMouseUp,
        CGEventType::OtherMouseDown,
        CGEventType::OtherMouseUp,
    ]
    .into_iter()
    .fold(0, |mask, event| mask | (1_u64 << event.0))
}

fn tap_disable_was_timeout(event_type: CGEventType) -> Option<bool> {
    match event_type {
        CGEventType::TapDisabledByTimeout => Some(true),
        CGEventType::TapDisabledByUserInput => Some(false),
        _ => None,
    }
}

fn map_mouse_event(
    event_type: CGEventType,
    button_number: i64,
) -> Option<(CursorButton, CursorButtonState)> {
    match event_type {
        CGEventType::LeftMouseDown => Some((CursorButton::Left, CursorButtonState::Pressed)),
        CGEventType::LeftMouseUp => Some((CursorButton::Left, CursorButtonState::Released)),
        CGEventType::RightMouseDown => Some((CursorButton::Right, CursorButtonState::Pressed)),
        CGEventType::RightMouseUp => Some((CursorButton::Right, CursorButtonState::Released)),
        CGEventType::OtherMouseDown if button_number == 2 => {
            Some((CursorButton::Middle, CursorButtonState::Pressed))
        }
        CGEventType::OtherMouseUp if button_number == 2 => {
            Some((CursorButton::Middle, CursorButtonState::Released))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::cursor::{Collectors, CursorCollector, Point, Rect};

    type SharedCollector = Arc<Mutex<CursorCollector>>;

    struct FakeFactory {
        access: bool,
        request_called: AtomicBool,
        run_called: AtomicBool,
        stopped: Arc<AtomicBool>,
    }

    impl FakeFactory {
        fn new(access: bool) -> Self {
            Self {
                access,
                request_called: AtomicBool::new(false),
                run_called: AtomicBool::new(false),
                stopped: Arc::new(AtomicBool::new(false)),
            }
        }
    }

    impl ClickTapFactory for FakeFactory {
        fn preflight_access(&self) -> bool {
            self.access
        }

        fn request_access(&self) -> bool {
            self.request_called.store(true, Ordering::Release);
            self.access
        }

        fn run(
            &self,
            _log: Arc<InputLog>,
            commands: Receiver<MonitorCommand>,
            ready: SyncSender<Result<(), ()>>,
        ) {
            self.run_called.store(true, Ordering::Release);
            let _ = ready.send(Ok(()));
            let _ = commands.recv();
            self.stopped.store(true, Ordering::Release);
        }
    }

    struct FailingFactory;

    impl ClickTapFactory for FailingFactory {
        fn preflight_access(&self) -> bool {
            true
        }

        fn request_access(&self) -> bool {
            panic!("granted preflight must not request permission")
        }

        fn run(
            &self,
            _log: Arc<InputLog>,
            _commands: Receiver<MonitorCommand>,
            ready: SyncSender<Result<(), ()>>,
        ) {
            let _ = ready.send(Err(()));
        }
    }

    struct WedgedStartupFactory {
        release: Arc<AtomicBool>,
    }

    impl ClickTapFactory for WedgedStartupFactory {
        fn preflight_access(&self) -> bool {
            true
        }

        fn request_access(&self) -> bool {
            unreachable!()
        }

        fn run(
            &self,
            _log: Arc<InputLog>,
            _commands: Receiver<MonitorCommand>,
            _ready: SyncSender<Result<(), ()>>,
        ) {
            while !self.release.load(Ordering::Acquire) {
                thread::yield_now();
            }
        }
    }

    fn collector() -> SharedCollector {
        Arc::new(Mutex::new(CursorCollector::new(Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        })))
    }

    fn log_for(collector: &SharedCollector) -> Arc<InputLog> {
        InputLog::new(
            HostTime::from_ticks(0),
            Collectors(vec![Arc::clone(collector)]),
        )
    }

    #[test]
    fn maps_only_supported_mouse_buttons() {
        assert_eq!(
            map_mouse_event(CGEventType::LeftMouseDown, 0),
            Some((CursorButton::Left, CursorButtonState::Pressed))
        );
        assert_eq!(
            map_mouse_event(CGEventType::RightMouseUp, 1),
            Some((CursorButton::Right, CursorButtonState::Released))
        );
        assert_eq!(
            map_mouse_event(CGEventType::OtherMouseDown, 2),
            Some((CursorButton::Middle, CursorButtonState::Pressed))
        );
        assert_eq!(map_mouse_event(CGEventType::OtherMouseDown, 3), None);
        assert_eq!(map_mouse_event(CGEventType::MouseMoved, 0), None);
    }

    #[test]
    fn click_timestamp_comes_from_the_native_event() {
        let event = CGEvent::new(None).expect("create event");
        CGEvent::set_timestamp(Some(&event), 42);

        assert_eq!(event_host_time(&event), 42);
    }

    #[test]
    fn tap_mask_and_reenable_policy_cover_only_required_events() {
        let expected_mask = [
            CGEventType::LeftMouseDown,
            CGEventType::LeftMouseUp,
            CGEventType::RightMouseDown,
            CGEventType::RightMouseUp,
            CGEventType::OtherMouseDown,
            CGEventType::OtherMouseUp,
        ]
        .into_iter()
        .fold(0, |mask, event| mask | (1_u64 << event.0));
        assert_eq!(click_event_mask(), expected_mask);
        assert_eq!(
            tap_disable_was_timeout(CGEventType::TapDisabledByTimeout),
            Some(true)
        );
        assert_eq!(
            tap_disable_was_timeout(CGEventType::TapDisabledByUserInput),
            Some(false)
        );
        assert_eq!(tap_disable_was_timeout(CGEventType::MouseMoved), None);
    }

    #[test]
    fn denied_permission_never_creates_a_native_monitor() {
        let collector = collector();
        let factory = Arc::new(FakeFactory::new(false));
        let monitor = ClickMonitor::start_with(log_for(&collector), Arc::clone(&factory));

        assert_eq!(monitor.status(), CursorClickCaptureStatus::PermissionDenied);
        assert!(factory.request_called.load(Ordering::Acquire));
        assert!(!factory.run_called.load(Ordering::Acquire));
        let mut collector = collector
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        collector.arm(100);
        collector.observe_cm_frame(
            Some(screencapturekit::cm::SCFrameStatus::Complete),
            screencapturekit::cm::CMTime::new(0, 1_000),
            Some(100),
            None,
            Some(Point { x: 50.0, y: 50.0 }),
        );
        let capture = collector.close();
        assert_eq!(
            capture.diagnostics.click_status,
            CursorClickCaptureStatus::PermissionDenied
        );
        assert_eq!(capture.telemetry.positions.len(), 1);
    }

    #[test]
    fn stop_signals_and_joins_the_monitor_thread() {
        let collector = collector();
        let factory = Arc::new(FakeFactory::new(true));
        let stopped = Arc::clone(&factory.stopped);
        let mut monitor = ClickMonitor::start_with(log_for(&collector), factory);

        assert_eq!(monitor.status(), CursorClickCaptureStatus::Active);
        assert!(monitor.stop());
        assert!(stopped.load(Ordering::Acquire));
        assert!(monitor.stop());
    }

    #[test]
    fn tap_startup_failure_is_non_fatal() {
        let collector = collector();
        let monitor = ClickMonitor::start_with(log_for(&collector), Arc::new(FailingFactory));

        assert_eq!(
            monitor.status(),
            CursorClickCaptureStatus::MonitorUnavailable
        );
        let capture = collector
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .close();
        assert_eq!(
            capture.diagnostics.click_status,
            CursorClickCaptureStatus::MonitorUnavailable
        );
        assert_eq!(capture.diagnostics.click_monitor_failures, 1);
    }

    #[test]
    fn startup_timeout_does_not_join_a_wedged_worker() {
        let release = Arc::new(AtomicBool::new(false));
        let factory = Arc::new(WedgedStartupFactory {
            release: Arc::clone(&release),
        });
        let (finished_sender, finished) = mpsc::sync_channel(1);
        let start = thread::spawn(move || {
            let monitor = ClickMonitor::start_with_timeout(
                log_for(&collector()),
                factory,
                Duration::from_millis(1),
            );
            let _ = finished_sender.send(monitor.status());
        });

        let status = finished.recv_timeout(Duration::from_secs(1));
        release.store(true, Ordering::Release);
        start.join().unwrap();
        assert_eq!(status, Ok(CursorClickCaptureStatus::MonitorUnavailable));
    }

    #[test]
    fn status_reflects_permanent_tap_failure() {
        let collector = collector();
        let factory = Arc::new(FakeFactory::new(true));
        let mut monitor = ClickMonitor::start_with(log_for(&collector), factory);
        collector
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tap_reenabled(false);

        assert_eq!(
            monitor.status(),
            CursorClickCaptureStatus::MonitorUnavailable
        );
        assert!(monitor.stop());
    }
}

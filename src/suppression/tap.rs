//! The macOS event taps: attaching the key, observation, and HID taps to
//! their run loop, the masks each listens for, re-enabling a tap macOS
//! disabled, and the health timer and App Nap opt-out that keep them alive.

use super::*;

pub(super) struct AttachedTap {
    pub(super) tap: *mut c_void,
    pub(super) source: *mut c_void,
    pub(super) run_loop: *mut c_void,
}

impl AttachedTap {
    /// # Safety
    /// `context` and `run_loop` must outlive the returned `AttachedTap`.
    unsafe fn create(
        location: u32,
        options: u32,
        mask: u64,
        callback: EventTapCallback,
        context: *mut EventTapContext,
        run_loop: *mut c_void,
        slot: &AtomicPtr<c_void>,
    ) -> Result<Self, &'static str> {
        let tap = unsafe {
            CGEventTapCreate(
                location,
                HEAD_INSERT_EVENT_TAP,
                options,
                mask,
                callback,
                context.cast(),
            )
        };
        if tap.is_null() {
            return Err("could not create event tap");
        }
        let source = unsafe { CFMachPortCreateRunLoopSource(ptr::null(), tap, 0) };
        if source.is_null() {
            unsafe { CFRelease(tap.cast_const()) };
            return Err("could not create input run-loop sources");
        }
        slot.store(tap, Ordering::Release);
        unsafe {
            CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
        }
        Ok(Self {
            tap,
            source,
            run_loop,
        })
    }
}

impl Drop for AttachedTap {
    fn drop(&mut self) {
        unsafe {
            CGEventTapEnable(self.tap, false);
            CFRunLoopRemoveSource(self.run_loop, self.source, kCFRunLoopCommonModes);
            CFRelease(self.source.cast_const());
            CFRelease(self.tap.cast_const());
        }
    }
}

pub(super) const KEY_MASK: u64 = (1_u64 << EVENT_KEY_DOWN) | (1_u64 << EVENT_KEY_UP);
pub(super) const MOUSE_MASK: u64 = (1_u64 << EVENT_LEFT_MOUSE_DOWN)
    | (1_u64 << EVENT_LEFT_MOUSE_UP)
    | (1_u64 << EVENT_RIGHT_MOUSE_DOWN)
    | (1_u64 << EVENT_RIGHT_MOUSE_UP)
    | (1_u64 << EVENT_OTHER_MOUSE_DOWN)
    | (1_u64 << EVENT_OTHER_MOUSE_UP)
    | (1_u64 << EVENT_LEFT_MOUSE_DRAGGED)
    | (1_u64 << EVENT_RIGHT_MOUSE_DRAGGED)
    | (1_u64 << EVENT_OTHER_MOUSE_DRAGGED);
pub(super) const OBSERVATION_MASK: u64 = 1_u64 << EVENT_FLAGS_CHANGED;

/// The shortcuts whose mouse button the session tap may consume.
fn mouse_bindable(hotkeys: RuntimeHotkeys) -> [Option<RuntimeHotkey>; 5] {
    [
        Some(hotkeys.dictation),
        hotkeys.raw_dictation,
        hotkeys.second_dictation,
        hotkeys.paste_last,
        hotkeys.paste_meeting,
    ]
}

/// Whether any shortcut is a mouse button, so clicks must pass through the
/// filtering key tap to be consumed. Otherwise mouse events only ever reach
/// the passive tap: a filtering tap on clicks and drags, even one returning
/// every event unchanged, breaks dragging windows between displays and
/// Spaces (#1215).
pub(super) fn filters_mouse(hotkeys: RuntimeHotkeys) -> bool {
    mouse_bindable(hotkeys).iter().flatten().any(|binding| {
        binding
            .key_code
            .is_some_and(|code| crate::app_settings::HotkeyKey::mouse_button(code).is_some())
    })
}

/// The key tap's and the passive observation tap's event masks. Mouse
/// events go to exactly one of them.
pub(super) fn tap_masks(filter_mouse: bool) -> (u64, u64) {
    if filter_mouse {
        (KEY_MASK | MOUSE_MASK, observation_mask())
    } else {
        (KEY_MASK, observation_mask() | MOUSE_MASK)
    }
}

/// Translate only configured mouse triggers into the shared shortcut reducer.
/// Ordinary clicks retain their cancellation/activity semantics.
pub(super) fn mouse_input(
    button: u16,
    down: bool,
    flags: u64,
    hotkeys: RuntimeHotkeys,
) -> Option<InputEvent> {
    let code = crate::app_settings::HotkeyKey::mouse_code(button)?;
    let bound = mouse_bindable(hotkeys)
        .iter()
        .flatten()
        .any(|binding| binding.key_code == Some(code));
    if bound {
        Some(InputEvent::Key { code, down, flags })
    } else {
        down.then_some(InputEvent::MouseDown)
    }
}

pub(super) fn input_is_down(state: u32, code: u16) -> bool {
    // SAFETY: both functions are read-only CoreGraphics state queries; mouse
    // codes are decoded before crossing the native keyboard API boundary.
    unsafe {
        match crate::app_settings::HotkeyKey::mouse_button(code) {
            Some(button) => CGEventSourceButtonState(state, u32::from(button)),
            None => CGEventSourceKeyState(state, code),
        }
    }
}

/// Whether a key or mouse button the shortcut machine tracks still reads as
/// held. Combined-session state can keep a key down for good after a system
/// shortcut swallows its release (Tab after Command-Tab, Q after Command-Q;
/// #612), which blocked every shortcut press until Hex restarted, so a
/// keyboard key also needs HID state, the physical keyboard, to agree.
pub(super) fn input_is_held(code: u16) -> bool {
    reads_held(code, input_is_down)
}

pub(super) fn reads_held(code: u16, down: impl Fn(u32, u16) -> bool) -> bool {
    down(COMBINED_SESSION_STATE, code)
        && (crate::app_settings::HotkeyKey::mouse_button(code).is_some()
            || down(HID_SYSTEM_STATE, code))
}

/// What the observation tap adds in builds with captures, while the
/// `capture` experiment is on: the pointer moving and scrolling. Clicks and
/// drags already reach one of the session taps (`tap_masks`). The callback drops
/// them at once unless a capture records (`capture_host::wants_input`); they
/// never reach the listener.
#[cfg(hex_capture)]
pub(super) const CAPTURE_POINTER_MASK: u64 =
    (1_u64 << EVENT_MOUSE_MOVED) | (1_u64 << EVENT_SCROLL_WHEEL);

pub(super) fn observation_mask() -> u64 {
    #[cfg(hex_capture)]
    return OBSERVATION_MASK | CAPTURE_POINTER_MASK;
    #[cfg(not(hex_capture))]
    OBSERVATION_MASK
}

pub(super) fn run_event_tap(
    sender: Sender<ObservedInputEvent>,
    activity: InputActivity,
    takes_escape: Arc<AtomicBool>,
    takes_cancel: Arc<AtomicBool>,
    pending: PendingInputEvents,
    run_loop: Arc<AtomicPtr<c_void>>,
    ready: SyncSender<Result<()>>,
) {
    let mut context = Box::new(EventTapContext {
        sender,
        activity,
        takes_escape,
        takes_cancel,
        key_tap: AtomicPtr::new(ptr::null_mut()),
        observation_tap: AtomicPtr::new(ptr::null_mut()),
        physical_tap: AtomicPtr::new(ptr::null_mut()),
        shortcut_suppression: Mutex::new(ShortcutSuppression::default()),
        pending,
        next_sequence: AtomicU64::new(0),
    });
    let context_ptr = &raw mut *context;
    // SAFETY: This is the dedicated event-tap thread's current run loop.
    let current_run_loop = unsafe { CFRunLoopGetCurrent() };
    let filter_mouse = filters_mouse(crate::app_settings::runtime_hotkeys());
    let mut taps = Box::new(TapSet {
        context: context_ptr,
        run_loop: current_run_loop,
        filter_mouse,
        taps: TapKind::ALL.map(|kind| ManagedTap {
            kind,
            attached: None,
            failing: false,
        }),
    });
    for tap in &mut taps.taps {
        // SAFETY: `context` outlives `taps`, which is declared after it.
        if let Err(message) = unsafe { tap.attach(context_ptr, current_run_loop, filter_mouse) } {
            match tap.kind {
                TapKind::Key => {
                    let _ = ready.send(Err(eyre!(
                        "could not create keyboard event tap; grant Accessibility permission"
                    )));
                    return;
                }
                TapKind::Observation => {
                    let _ = ready.send(Err(eyre!(message)));
                    return;
                }
                TapKind::Physical => {
                    tap.failing = true;
                    tracing::warn!(
                        "could not observe HID key presses; global-shortcut chords cannot cancel"
                    );
                }
            }
        }
    }
    // SAFETY: `taps` outlives the timer, which is declared after it and
    // invalidated on drop, and both live on this run loop's thread.
    let _health = unsafe { HealthTimer::start(&raw mut *taps, current_run_loop) };
    let _app_nap = AppNapOptOut::begin();
    run_loop.store(current_run_loop, Ordering::Release);
    let _ = ready.send(Ok(()));
    // SAFETY: This dedicated thread exists solely to dispatch the event tap.
    unsafe { CFRunLoopRun() };
    run_loop.store(ptr::null_mut(), Ordering::Release);
}

/// How often the tap thread confirms its taps still run. macOS can disable
/// or invalidate a tap without delivering a disable event (after sleep,
/// secure input, or an Accessibility change), which leaves the shortcut dead.
pub(super) const TAP_HEALTH_INTERVAL_SECONDS: f64 = 5.0;

#[derive(Clone, Copy, Debug)]
pub(super) enum TapKind {
    Key,
    Observation,
    Physical,
}

impl TapKind {
    const ALL: [Self; 3] = [Self::Key, Self::Observation, Self::Physical];

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Key => "key",
            Self::Observation => "modifier",
            Self::Physical => "hid",
        }
    }

    pub(super) fn slot(self, context: &EventTapContext) -> &AtomicPtr<c_void> {
        match self {
            Self::Key => &context.key_tap,
            Self::Observation => &context.observation_tap,
            Self::Physical => &context.physical_tap,
        }
    }

    /// The key and modifier taps carry shortcut state; a gap in them makes
    /// the input history untrustworthy. The HID tap only adds chord evidence.
    pub(super) fn tracks_shortcuts(self) -> bool {
        !matches!(self, Self::Physical)
    }
}

pub(super) struct ManagedTap {
    pub(super) kind: TapKind,
    pub(super) attached: Option<AttachedTap>,
    /// Creation failed and was logged; log again only once it recovers.
    pub(super) failing: bool,
}

impl ManagedTap {
    /// # Safety
    /// `context` and `run_loop` must outlive the attached tap.
    unsafe fn attach(
        &mut self,
        context: *mut EventTapContext,
        run_loop: *mut c_void,
        filter_mouse: bool,
    ) -> Result<(), &'static str> {
        // SAFETY: the caller keeps `context` alive; the slot lives in it.
        let slot = self.kind.slot(unsafe { &*context });
        let (key_mask, observation_mask) = tap_masks(filter_mouse);
        let (location, options, mask, callback): (u32, u32, u64, EventTapCallback) = match self.kind
        {
            TapKind::Key => (
                ANNOTATED_SESSION_EVENT_TAP,
                DEFAULT_EVENT_TAP,
                key_mask,
                event_callback,
            ),
            // A modifying tap disrupts Finder's Option-driven alternate menu items even
            // when it returns flagsChanged events unchanged. Observe modifiers with a
            // passive tap; mouse events too, unless a mouse shortcut needs the
            // filtering tap to consume its button.
            TapKind::Observation => (
                ANNOTATED_SESSION_EVENT_TAP,
                LISTEN_ONLY_EVENT_TAP,
                observation_mask,
                event_callback,
            ),
            // Listens for key presses at the HID level, where shortcuts that other apps
            // register globally are still visible. Without it those chords are invisible
            // to the session taps; dictation still works.
            TapKind::Physical => (
                HID_EVENT_TAP,
                LISTEN_ONLY_EVENT_TAP,
                1_u64 << EVENT_KEY_DOWN,
                physical_event_callback,
            ),
        };
        // SAFETY: forwarded from the caller.
        let attached = unsafe {
            AttachedTap::create(location, options, mask, callback, context, run_loop, slot)
        }?;
        self.attached = Some(attached);
        Ok(())
    }
}

/// The taps on the event-tap thread, checked by its health timer.
pub(super) struct TapSet {
    pub(super) context: *mut EventTapContext,
    pub(super) run_loop: *mut c_void,
    /// Whether the key tap, rather than the passive one, receives mouse
    /// events (`tap_masks`).
    pub(super) filter_mouse: bool,
    pub(super) taps: [ManagedTap; 3],
}

impl TapSet {
    /// Re-enables a disabled tap and recreates an invalidated or missing one.
    /// Also moves mouse events between the session taps once a mouse
    /// shortcut is bound or unbound.
    pub(super) fn check(&mut self) {
        // SAFETY: `context` outlives this set (see `run_event_tap`).
        let context = unsafe { &*self.context };
        let mut interrupted = false;
        let filter_mouse = filters_mouse(crate::app_settings::runtime_hotkeys());
        let remask = filter_mouse != self.filter_mouse;
        if remask {
            tracing::info!(filter_mouse, "moving mouse events between event taps");
            self.filter_mouse = filter_mouse;
        }
        for tap in &mut self.taps {
            let kind = tap.kind.as_str();
            let recreate = match &tap.attached {
                None => true,
                Some(_) if remask && !matches!(tap.kind, TapKind::Physical) => true,
                // SAFETY: `tap.tap` is a live CFMachPort owned by `AttachedTap`.
                Some(attached) if unsafe { CFMachPortIsValid(attached.tap) } == 0 => {
                    tracing::warn!(tap = kind, "event tap was invalidated; recreating it");
                    true
                }
                // SAFETY: as above.
                Some(attached) if !unsafe { CGEventTapIsEnabled(attached.tap) } => {
                    // SAFETY: as above.
                    unsafe { CGEventTapEnable(attached.tap, true) };
                    // SAFETY: as above.
                    let enabled = unsafe { CGEventTapIsEnabled(attached.tap) };
                    tracing::warn!(
                        tap = kind,
                        reenabled = enabled,
                        "event tap was disabled without notice; re-enabled it"
                    );
                    interrupted |= tap.kind.tracks_shortcuts();
                    !enabled
                }
                Some(_) => false,
            };
            if !recreate {
                continue;
            }
            tap.kind
                .slot(context)
                .store(ptr::null_mut(), Ordering::Release);
            let was_attached = tap.attached.take().is_some();
            // SAFETY: `context` and the run loop outlive this set.
            match unsafe { tap.attach(self.context, self.run_loop, filter_mouse) } {
                Ok(()) => {
                    if was_attached || tap.failing {
                        tracing::warn!(tap = kind, "recreated event tap");
                    }
                    tap.failing = false;
                    interrupted |= was_attached && tap.kind.tracks_shortcuts();
                }
                Err(message) => {
                    if !tap.failing {
                        tracing::error!(
                            tap = kind,
                            error = message,
                            "could not recreate event tap"
                        );
                    }
                    tap.failing = true;
                    interrupted |= was_attached && tap.kind.tracks_shortcuts();
                }
            }
        }
        if interrupted {
            send_input(context, InputEvent::TapDisabled, CaptureInstant::ZERO);
        }
    }
}

/// The repeating run-loop timer that runs `TapSet::check`.
pub(super) struct HealthTimer(*mut c_void);

impl HealthTimer {
    /// # Safety
    /// `taps` must outlive the returned timer, and `run_loop` must be the
    /// current thread's run loop.
    unsafe fn start(taps: *mut TapSet, run_loop: *mut c_void) -> Option<Self> {
        let mut context = RunLoopTimerContext {
            version: 0,
            info: taps.cast(),
            retain: ptr::null(),
            release: ptr::null(),
            copy_description: ptr::null(),
        };
        // SAFETY: CoreFoundation copies the context; `info` outlives the timer.
        let timer = unsafe {
            CFRunLoopTimerCreate(
                ptr::null(),
                CFAbsoluteTimeGetCurrent() + TAP_HEALTH_INTERVAL_SECONDS,
                TAP_HEALTH_INTERVAL_SECONDS,
                0,
                0,
                check_tap_health,
                &raw mut context,
            )
        };
        if timer.is_null() {
            tracing::warn!("could not schedule event tap health checks");
            return None;
        }
        // SAFETY: `timer` is a valid CFRunLoopTimer for this thread's run loop.
        unsafe { CFRunLoopAddTimer(run_loop, timer, kCFRunLoopCommonModes) };
        Some(Self(timer))
    }
}

impl Drop for HealthTimer {
    fn drop(&mut self) {
        // SAFETY: the timer was created by `HealthTimer::start` and is owned here.
        unsafe {
            CFRunLoopTimerInvalidate(self.0);
            CFRelease(self.0.cast_const());
        }
    }
}

unsafe extern "C" fn check_tap_health(_timer: *mut c_void, info: *mut c_void) {
    // SAFETY: `info` is the `TapSet` that outlives this timer, used only on
    // the event-tap thread, so no other reference is live during the call.
    unsafe { &mut *info.cast::<TapSet>() }.check();
}

/// Keeps App Nap from throttling the event-tap thread while Hex sits idle in
/// the background: a napped callback can miss its deadline, and its health
/// timer would stretch. Idle system sleep stays allowed.
pub(super) struct AppNapOptOut(
    objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>,
);

impl AppNapOptOut {
    pub(super) fn begin() -> Self {
        use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
        let activity = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
            NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
            &NSString::from_str("Hex listens for its dictation shortcut"),
        );
        Self(activity)
    }
}

impl Drop for AppNapOptOut {
    fn drop(&mut self) {
        // SAFETY: the activity came from `beginActivityWithOptions:reason:`.
        unsafe { objc2_foundation::NSProcessInfo::processInfo().endActivity(&self.0) };
    }
}

/// Re-enables every tap after macOS disables one and records why.
pub(super) fn reenable_disabled_taps(
    context: &EventTapContext,
    event_type: u32,
    tap: &'static str,
) {
    let reason = if event_type == EVENT_TAP_DISABLED_BY_TIMEOUT {
        "timeout"
    } else {
        "user input"
    };
    for kind in TapKind::ALL {
        let tap = kind.slot(context).load(Ordering::Acquire);
        if !tap.is_null() {
            // SAFETY: `tap` is a CFMachPort returned by `CGEventTapCreate`.
            unsafe { CGEventTapEnable(tap, true) };
        }
    }
    tracing::warn!(tap, reason, "macOS disabled the event tap; re-enabled it");
}

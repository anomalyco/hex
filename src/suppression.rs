use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use color_eyre::eyre::{Result, eyre};

#[cfg(test)]
use crate::app_settings::HotkeyBinding;
use crate::app_settings::{HOTKEY_MODIFIERS_MASK, RuntimeHotkey, RuntimeHotkeys};
use crate::audio::CaptureInstant;
use crate::dictation::{BOTH_SIDES_WINDOW, MINIMUM_HOLD_DURATION};

const DOUBLE_TAP_WINDOW: Duration = MINIMUM_HOLD_DURATION;
const ESCAPE_KEY_CODE: u16 = 53;
/// The key code the Globe (Fn) key reports in its own key events.
const GLOBE_KEY_CODE: u16 = 179;

const EVENT_LEFT_MOUSE_DOWN: u32 = 1;
const EVENT_LEFT_MOUSE_UP: u32 = 2;
const EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
const EVENT_RIGHT_MOUSE_UP: u32 = 4;
const EVENT_KEY_DOWN: u32 = 10;
const EVENT_KEY_UP: u32 = 11;
const EVENT_FLAGS_CHANGED: u32 = 12;
const EVENT_OTHER_MOUSE_DOWN: u32 = 25;
const EVENT_OTHER_MOUSE_UP: u32 = 26;
const EVENT_LEFT_MOUSE_DRAGGED: u32 = 6;
const EVENT_RIGHT_MOUSE_DRAGGED: u32 = 7;
const EVENT_OTHER_MOUSE_DRAGGED: u32 = 27;
const MOUSE_EVENT_BUTTON_NUMBER: u32 = 3;
#[cfg(hex_capture)]
const EVENT_MOUSE_MOVED: u32 = 5;
#[cfg(hex_capture)]
const EVENT_SCROLL_WHEEL: u32 = 22;
#[cfg(hex_capture)]
const KEYBOARD_EVENT_AUTOREPEAT: u32 = 8;
#[cfg(hex_capture)]
const SCROLL_WHEEL_DELTA_AXIS_1: u32 = 11;
#[cfg(hex_capture)]
const SCROLL_WHEEL_DELTA_AXIS_2: u32 = 12;
const EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = u32::MAX - 1;
const EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = u32::MAX;
const KEYBOARD_EVENT_KEYCODE: u32 = 9;
const EVENT_SOURCE_USER_DATA: u32 = 42;

// Assistive keyboards inject downstream of HID; observe them alongside hardware input.
const ANNOTATED_SESSION_EVENT_TAP: u32 = 2;
// Another app's global shortcut (Carbon hotkeys such as a window manager's
// Option-1) consumes its key before the session, so only HID sees that press.
const HID_EVENT_TAP: u32 = 0;
const HEAD_INSERT_EVENT_TAP: u32 = 0;
const DEFAULT_EVENT_TAP: u32 = 0;
const LISTEN_ONLY_EVENT_TAP: u32 = 1;
const HID_SYSTEM_STATE: u32 = 1;
const COMBINED_SESSION_STATE: u32 = 0;
const STALE_KEY_NEUTRAL_DURATION: Duration = Duration::from_millis(100);
const PHYSICAL_KEY_DOWN_HISTORY: usize = 8;

type EventRef = *mut c_void;
type EventTapCallback = unsafe extern "C" fn(
    proxy: *mut c_void,
    event_type: u32,
    event: EventRef,
    user_info: *mut c_void,
) -> EventRef;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceFlagsState(state_id: u32) -> u64;
    fn CGEventSourceKeyState(state_id: u32, key: u16) -> bool;
    fn CGEventSourceButtonState(state_id: u32, button: u32) -> bool;
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: EventTapCallback,
        user_info: *mut c_void,
    ) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGEventGetFlags(event: EventRef) -> u64;
    fn CGEventGetTimestamp(event: EventRef) -> u64;
    fn CGEventGetIntegerValueField(event: EventRef, field: u32) -> i64;
    #[cfg(hex_capture)]
    fn CGEventGetLocation(event: EventRef) -> CgPoint;
    #[cfg(hex_capture)]
    fn CGEventKeyboardGetUnicodeString(
        event: EventRef,
        max_length: usize,
        actual_length: *mut usize,
        buffer: *mut u16,
    );
}

#[cfg(hex_capture)]
#[repr(C)]
#[derive(Clone, Copy)]
struct CgPoint {
    x: f64,
    y: f64,
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port: *mut c_void,
        order: isize,
    ) -> *mut c_void;
    fn CFRunLoopAddSource(run_loop: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopRemoveSource(run_loop: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopRun();
    fn CFRunLoopStop(run_loop: *mut c_void);
    fn CFRelease(value: *const c_void);
    static kCFRunLoopCommonModes: *const c_void;
}

#[derive(Clone, Copy, Debug)]
pub enum InputEvent {
    Flags(u64),
    Key {
        code: u16,
        down: bool,
        flags: u64,
    },
    /// A key press seen at the HID level. It only signals a chord: the session
    /// `Key` events, when they arrive, remain the source of key state.
    PhysicalKeyDown {
        code: u16,
        timing: HidTiming,
    },
    MouseDown,
    TapDisabled,
}

/// How a HID-tap timestamp was read, kept only for diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HidTiming {
    pub raw: u64,
    pub clock: HidClock,
    /// When the tap callback ran.
    pub received_at: CaptureInstant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HidClock {
    MachTicks,
    Nanos,
    /// Neither reading was recent, so the event took its arrival time.
    Arrival,
}

impl HidClock {
    fn as_str(self) -> &'static str {
        match self {
            Self::MachTicks => "mach",
            Self::Nanos => "nanos",
            Self::Arrival => "arrival",
        }
    }
}

/// What ended a press as a chord before it became a dictation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscardCause {
    Key,
    HidKey,
    Mouse,
    ExtraModifier,
}

impl DiscardCause {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Key => "key",
            Self::HidKey => "hid-key",
            Self::Mouse => "mouse",
            Self::ExtraModifier => "extra-modifier",
        }
    }
}

impl InputEvent {
    pub fn is_escape_down(self) -> bool {
        matches!(
            self,
            Self::Key {
                code: ESCAPE_KEY_CODE,
                down: true,
                ..
            }
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ObservedInputEvent {
    sequence: u64,
    pub event: InputEvent,
    pub capture_at: CaptureInstant,
}

#[derive(Clone, Default)]
pub struct PendingInputEvents(Arc<Mutex<VecDeque<(u64, CaptureInstant)>>>);

impl PendingInputEvents {
    #[cfg(test)]
    pub fn with_pending_for_test(at: CaptureInstant) -> (Self, impl FnOnce()) {
        let pending = Self::default();
        pending.push(0, at);
        let acknowledge = pending.clone();
        (pending, move || acknowledge.acknowledge(0))
    }

    pub fn oldest(&self) -> Option<CaptureInstant> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .front()
            .map(|(_, at)| *at)
    }

    fn push(&self, sequence: u64, at: CaptureInstant) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push_back((sequence, at));
    }

    fn acknowledge(&self, sequence: u64) {
        let mut pending = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if pending.front().is_some_and(|(next, _)| *next == sequence) {
            pending.pop_front();
        }
    }
}

pub struct InputMonitor {
    pub events: Receiver<ObservedInputEvent>,
    pub activity: InputActivity,
    pending: PendingInputEvents,
    escape_cancels: Arc<AtomicBool>,
    run_loop: Arc<AtomicPtr<c_void>>,
    worker: Option<JoinHandle<()>>,
}

pub struct PendingInputAcknowledgement<'a> {
    pending: &'a PendingInputEvents,
    sequence: u64,
}

impl Drop for PendingInputAcknowledgement<'_> {
    fn drop(&mut self) {
        self.pending.acknowledge(self.sequence);
    }
}

#[derive(Clone, Default)]
pub struct InputActivity(Arc<AtomicU64>);

impl InputActivity {
    pub fn revision(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }

    pub fn invalidate(&self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }

    fn observe(&self, input: InputEvent, suppressed: bool) {
        if !suppressed
            && matches!(
                input,
                InputEvent::Key { down: true, .. } | InputEvent::MouseDown
            )
        {
            self.invalidate();
        }
    }
}

impl InputMonitor {
    pub fn start() -> Result<Self> {
        let (sender, events) = mpsc::channel::<ObservedInputEvent>();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let activity = InputActivity::default();
        let tap_activity = activity.clone();
        let run_loop = Arc::new(AtomicPtr::new(ptr::null_mut()));
        let tap_run_loop = run_loop.clone();
        let escape_cancels = Arc::new(AtomicBool::new(false));
        let tap_escape_cancels = escape_cancels.clone();
        let pending = PendingInputEvents::default();
        let tap_pending = pending.clone();
        let worker = thread::spawn(move || {
            run_event_tap(
                sender,
                tap_activity,
                tap_escape_cancels,
                tap_pending,
                tap_run_loop,
                ready_sender,
            )
        });
        ready_receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| eyre!("timed out starting the keyboard event tap"))??;
        Ok(Self {
            events,
            activity,
            pending,
            escape_cancels,
            run_loop,
            worker: Some(worker),
        })
    }

    pub fn set_escape_cancels(&self, enabled: bool) {
        self.escape_cancels.store(enabled, Ordering::Release);
    }

    pub fn pending_events(&self) -> PendingInputEvents {
        self.pending.clone()
    }

    pub fn acknowledge_after(&self, event: ObservedInputEvent) -> PendingInputAcknowledgement<'_> {
        PendingInputAcknowledgement {
            pending: &self.pending,
            sequence: event.sequence,
        }
    }
}

impl Drop for InputMonitor {
    fn drop(&mut self) {
        let run_loop = self.run_loop.load(Ordering::Acquire);
        if !run_loop.is_null() {
            // SAFETY: Core Foundation run loops may be stopped from any thread.
            unsafe { CFRunLoopStop(run_loop) };
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct EventTapContext {
    sender: Sender<ObservedInputEvent>,
    activity: InputActivity,
    escape_cancels: Arc<AtomicBool>,
    key_tap: AtomicPtr<c_void>,
    observation_tap: AtomicPtr<c_void>,
    physical_tap: AtomicPtr<c_void>,
    shortcut_suppression: Mutex<ShortcutSuppression>,
    pending: PendingInputEvents,
    next_sequence: AtomicU64,
}

struct AttachedTap {
    tap: *mut c_void,
    source: *mut c_void,
    run_loop: *mut c_void,
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

const KEY_MASK: u64 = (1_u64 << EVENT_KEY_DOWN)
    | (1_u64 << EVENT_KEY_UP)
    | (1_u64 << EVENT_LEFT_MOUSE_DOWN)
    | (1_u64 << EVENT_LEFT_MOUSE_UP)
    | (1_u64 << EVENT_RIGHT_MOUSE_DOWN)
    | (1_u64 << EVENT_RIGHT_MOUSE_UP)
    | (1_u64 << EVENT_OTHER_MOUSE_DOWN)
    | (1_u64 << EVENT_OTHER_MOUSE_UP)
    | (1_u64 << EVENT_LEFT_MOUSE_DRAGGED)
    | (1_u64 << EVENT_RIGHT_MOUSE_DRAGGED)
    | (1_u64 << EVENT_OTHER_MOUSE_DRAGGED);
const OBSERVATION_MASK: u64 = 1_u64 << EVENT_FLAGS_CHANGED;

/// Translate only configured mouse triggers into the shared shortcut reducer.
/// Ordinary clicks retain their cancellation/activity semantics.
fn mouse_input(button: u16, down: bool, flags: u64, hotkeys: RuntimeHotkeys) -> Option<InputEvent> {
    let code = crate::app_settings::HotkeyKey::mouse_code(button)?;
    let bound = [
        Some(hotkeys.dictation),
        hotkeys.paste_last,
        hotkeys.paste_meeting,
    ]
    .iter()
    .flatten()
    .any(|binding| binding.key_code == Some(code));
    if bound {
        Some(InputEvent::Key { code, down, flags })
    } else {
        down.then_some(InputEvent::MouseDown)
    }
}

fn input_is_down(state: u32, code: u16) -> bool {
    // SAFETY: both functions are read-only CoreGraphics state queries; mouse
    // codes are decoded before crossing the native keyboard API boundary.
    unsafe {
        match crate::app_settings::HotkeyKey::mouse_button(code) {
            Some(button) => CGEventSourceButtonState(state, u32::from(button)),
            None => CGEventSourceKeyState(state, code),
        }
    }
}

/// What the observation tap adds in builds with captures, while the
/// `capture` experiment is on: the pointer moving and scrolling. Clicks and
/// drags already reach the key tap, for mouse shortcuts. The callback drops
/// them at once unless a capture records (`capture_host::wants_input`); they
/// never reach the listener.
#[cfg(hex_capture)]
const CAPTURE_POINTER_MASK: u64 = (1_u64 << EVENT_MOUSE_MOVED) | (1_u64 << EVENT_SCROLL_WHEEL);

fn observation_mask() -> u64 {
    #[cfg(hex_capture)]
    return OBSERVATION_MASK | CAPTURE_POINTER_MASK;
    #[cfg(not(hex_capture))]
    OBSERVATION_MASK
}

fn run_event_tap(
    sender: Sender<ObservedInputEvent>,
    activity: InputActivity,
    escape_cancels: Arc<AtomicBool>,
    pending: PendingInputEvents,
    run_loop: Arc<AtomicPtr<c_void>>,
    ready: SyncSender<Result<()>>,
) {
    let mut context = Box::new(EventTapContext {
        sender,
        activity,
        escape_cancels,
        key_tap: AtomicPtr::new(ptr::null_mut()),
        observation_tap: AtomicPtr::new(ptr::null_mut()),
        physical_tap: AtomicPtr::new(ptr::null_mut()),
        shortcut_suppression: Mutex::new(ShortcutSuppression::default()),
        pending,
        next_sequence: AtomicU64::new(0),
    });
    let context_ptr = &raw mut *context;
    // SAFETY: This is the dedicated event-tap thread's current run loop, and
    // `context` outlives all `AttachedTap`s created below.
    let current_run_loop = unsafe { CFRunLoopGetCurrent() };
    let _key_tap = match unsafe {
        AttachedTap::create(
            ANNOTATED_SESSION_EVENT_TAP,
            DEFAULT_EVENT_TAP,
            KEY_MASK,
            event_callback,
            context_ptr,
            current_run_loop,
            &context.key_tap,
        )
    } {
        Ok(tap) => tap,
        Err(_) => {
            let _ = ready.send(Err(eyre!(
                "could not create keyboard event tap; grant Accessibility permission"
            )));
            return;
        }
    };
    // A modifying tap disrupts Finder's Option-driven alternate menu items even when it returns
    // flagsChanged events unchanged. Observe modifiers with a passive tap;
    // bound mouse clicks use the filtering tap.
    let _observation_tap = match unsafe {
        AttachedTap::create(
            ANNOTATED_SESSION_EVENT_TAP,
            LISTEN_ONLY_EVENT_TAP,
            observation_mask(),
            event_callback,
            context_ptr,
            current_run_loop,
            &context.observation_tap,
        )
    } {
        Ok(tap) => tap,
        Err(message) => {
            let _ = ready.send(Err(eyre!(message)));
            return;
        }
    };
    // Listens for key presses at the HID level, where shortcuts that other apps
    // register globally are still visible. Without it those chords are invisible
    // to the session taps; dictation still works.
    let _physical_tap = match unsafe {
        AttachedTap::create(
            HID_EVENT_TAP,
            LISTEN_ONLY_EVENT_TAP,
            1_u64 << EVENT_KEY_DOWN,
            physical_event_callback,
            context_ptr,
            current_run_loop,
            &context.physical_tap,
        )
    } {
        Ok(tap) => Some(tap),
        Err(_) => {
            tracing::warn!(
                "could not observe HID key presses; global-shortcut chords cannot cancel"
            );
            None
        }
    };
    run_loop.store(current_run_loop, Ordering::Release);
    let _ = ready.send(Ok(()));
    // SAFETY: This dedicated thread exists solely to dispatch the event tap.
    unsafe { CFRunLoopRun() };
    run_loop.store(ptr::null_mut(), Ordering::Release);
}

unsafe extern "C" fn physical_event_callback(
    _proxy: *mut c_void,
    event_type: u32,
    event: EventRef,
    user_info: *mut c_void,
) -> EventRef {
    if user_info.is_null() {
        return event;
    }
    // SAFETY: `user_info` points to the context allocated by `run_event_tap`.
    let context = unsafe { &*(user_info.cast::<EventTapContext>()) };
    if matches!(
        event_type,
        EVENT_TAP_DISABLED_BY_TIMEOUT | EVENT_TAP_DISABLED_BY_USER_INPUT
    ) {
        // Missing evidence only lets a chord count as a hold, so just resume.
        let tap = context.physical_tap.load(Ordering::Acquire);
        if !tap.is_null() {
            // SAFETY: `tap` is a CFMachPort returned by `CGEventTapCreate`.
            unsafe { CGEventTapEnable(tap, true) };
        }
        return event;
    }
    if event.is_null()
        || event_type != EVENT_KEY_DOWN
        || crate::app_settings::hotkey_capture_active()
        // SAFETY: CoreGraphics supplied a valid event to this callback.
        || unsafe { CGEventGetIntegerValueField(event, EVENT_SOURCE_USER_DATA) }
            == crate::keyboard::SYNTHETIC_EVENT_MARKER
    {
        return event;
    }
    // SAFETY: CoreGraphics supplied a valid keyboard event.
    let code = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) as u16 };
    // SAFETY: CoreGraphics supplied a valid event.
    let flags = unsafe { CGEventGetFlags(event) };
    if !is_chord_evidence(code, flags, crate::app_settings::runtime_hotkeys()) {
        return event;
    }
    // SAFETY: CoreGraphics supplied a valid event.
    let raw = unsafe { CGEventGetTimestamp(event) };
    let received_at = CaptureInstant::now();
    let (at, clock) = physical_event_instant(raw, received_at);
    send_input(
        context,
        InputEvent::PhysicalKeyDown {
            code,
            timing: HidTiming {
                raw,
                clock,
                received_at,
            },
        },
        at,
    );
    event
}

/// Whether a HID key press can reveal a chord Hex would otherwise miss.
/// Escape and Hex's own shortcuts already act through their session events.
fn is_chord_evidence(code: u16, flags: u64, hotkeys: RuntimeHotkeys) -> bool {
    code != ESCAPE_KEY_CODE
        && !is_own_globe_key(code, hotkeys.dictation)
        && !hotkeys
            .all()
            .iter()
            .flatten()
            .any(|hotkey| hotkey.matches_key_press(code, flags))
}

/// The Globe key's own key event, which some keyboards and remappers
/// (Karabiner-Elements) send beside the Fn modifier change. For a binding
/// that holds Fn, it is the shortcut itself, never a chord.
fn is_own_globe_key(code: u16, binding: RuntimeHotkey) -> bool {
    code == GLOBE_KEY_CODE && binding.modifiers.function && binding.key_code != Some(code)
}

/// HID-tap timestamps arrive as Mach ticks on Apple Silicon and as
/// nanoseconds on some events; keep whichever reading is recent, else `now`.
fn physical_event_instant(raw: u64, now: CaptureInstant) -> (CaptureInstant, HidClock) {
    const RECENT: Duration = Duration::from_secs(5);
    [
        (CaptureInstant::from_mach_ticks(raw), HidClock::MachTicks),
        (CaptureInstant::from_nanos(raw), HidClock::Nanos),
    ]
    .into_iter()
    .find(|(at, _)| {
        now.checked_duration_since(*at)
            .is_some_and(|age| age <= RECENT)
    })
    .unwrap_or((now, HidClock::Arrival))
}

/// Signed milliseconds from `from` to `to`.
fn signed_ms(to: CaptureInstant, from: CaptureInstant) -> f64 {
    (i128::from(to.as_nanos()) - i128::from(from.as_nanos())) as f64 / 1e6
}

unsafe extern "C" fn event_callback(
    _proxy: *mut c_void,
    event_type: u32,
    event: EventRef,
    user_info: *mut c_void,
) -> EventRef {
    if user_info.is_null() {
        return event;
    }
    // SAFETY: `user_info` points to the context allocated by `run_event_tap`.
    let context = unsafe { &*(user_info.cast::<EventTapContext>()) };
    if matches!(
        event_type,
        EVENT_TAP_DISABLED_BY_TIMEOUT | EVENT_TAP_DISABLED_BY_USER_INPUT
    ) {
        for tap in [&context.key_tap, &context.observation_tap] {
            let tap = tap.load(Ordering::Acquire);
            if !tap.is_null() {
                // SAFETY: `tap` is a CFMachPort returned by `CGEventTapCreate`.
                unsafe { CGEventTapEnable(tap, true) };
            }
        }
        send_input(context, InputEvent::TapDisabled, CaptureInstant::ZERO);
        return event;
    }
    if event.is_null() {
        return event;
    }
    // SAFETY: CoreGraphics supplied a valid event to this callback.
    if unsafe { CGEventGetIntegerValueField(event, EVENT_SOURCE_USER_DATA) }
        == crate::keyboard::SYNTHETIC_EVENT_MARKER
    {
        // The chord breaker has already passed the HID and session taps and
        // global monitors that watch for modifier taps; the focused app would
        // only beep at it.
        let chord_breaker = matches!(event_type, EVENT_KEY_DOWN | EVENT_KEY_UP)
            // SAFETY: CoreGraphics supplied a valid keyboard event.
            && unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) }
                == i64::from(crate::keyboard::CHORD_BREAKER_KEY_CODE);
        return if chord_breaker {
            ptr::null_mut()
        } else {
            event
        };
    }
    #[cfg(hex_capture)]
    let list_swallows = match list_tap(event_type, event) {
        ListSwallow::No => false,
        ListSwallow::Only => return ptr::null_mut(),
        ListSwallow::AfterListener => true,
    };
    #[cfg(not(hex_capture))]
    let list_swallows = false;
    #[cfg(hex_capture)]
    let capture_key = if crate::capture_host::wants_input() {
        match report_capture_input(event_type, event) {
            CaptureTap::Only => return event,
            CaptureTap::Shared => None,
            CaptureTap::Key(key) => Some(key),
        }
    } else {
        None
    };
    // A press anywhere while the list or suggestions are open may fold it; the click still
    // goes to the app and to the listener as chord evidence.
    #[cfg(hex_capture)]
    if matches!(
        event_type,
        EVENT_LEFT_MOUSE_DOWN | EVENT_RIGHT_MOUSE_DOWN | EVENT_OTHER_MOUSE_DOWN
    ) && (crate::capture_host::list_open() || crate::capture_host::suggesting())
    {
        crate::capture_host::capsule_key(crate::capture_host::CapsuleKey::Press);
    }
    if matches!(
        event_type,
        EVENT_LEFT_MOUSE_DRAGGED | EVENT_RIGHT_MOUSE_DRAGGED | EVENT_OTHER_MOUSE_DRAGGED
    ) {
        // A swallowed press must not leak a drag into the foreground app.
        // SAFETY: CoreGraphics supplied a valid mouse event.
        let button = unsafe { CGEventGetIntegerValueField(event, MOUSE_EVENT_BUTTON_NUMBER) };
        let suppress = !crate::app_settings::hotkey_capture_active()
            && u16::try_from(button).ok().is_some_and(|button| {
                context
                    .shortcut_suppression
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .suppresses_mouse_drag(button)
            });
        return if suppress { ptr::null_mut() } else { event };
    }
    let input = match event_type {
        EVENT_FLAGS_CHANGED => {
            // SAFETY: CoreGraphics supplied a valid event to this callback.
            InputEvent::Flags(unsafe { CGEventGetFlags(event) })
        }
        EVENT_KEY_DOWN | EVENT_KEY_UP => InputEvent::Key {
            // SAFETY: CoreGraphics supplied a valid keyboard event.
            code: unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) as u16 },
            down: event_type == EVENT_KEY_DOWN,
            // SAFETY: CoreGraphics supplied a valid event.
            flags: unsafe { CGEventGetFlags(event) },
        },
        EVENT_LEFT_MOUSE_DOWN
        | EVENT_RIGHT_MOUSE_DOWN
        | EVENT_OTHER_MOUSE_DOWN
        | EVENT_LEFT_MOUSE_UP
        | EVENT_RIGHT_MOUSE_UP
        | EVENT_OTHER_MOUSE_UP => {
            // SAFETY: CoreGraphics supplied a valid mouse event.
            let button = unsafe { CGEventGetIntegerValueField(event, MOUSE_EVENT_BUTTON_NUMBER) };
            let Ok(button) = u16::try_from(button) else {
                return event;
            };
            let down = matches!(
                event_type,
                EVENT_LEFT_MOUSE_DOWN | EVENT_RIGHT_MOUSE_DOWN | EVENT_OTHER_MOUSE_DOWN
            );
            // SAFETY: CoreGraphics supplied a valid event.
            let flags = unsafe { CGEventGetFlags(event) };
            let input = mouse_input(button, down, flags, crate::app_settings::runtime_hotkeys())
                .or_else(|| {
                    // A settings change can remove the binding before release.
                    // Finish the original press's bookkeeping and suppression.
                    (!down)
                        .then(|| {
                            context
                                .shortcut_suppression
                                .lock()
                                .unwrap_or_else(|error| error.into_inner())
                                .tracked_mouse_release(button, flags)
                        })
                        .flatten()
                });
            let Some(input) = input else {
                return event;
            };
            input
        }
        _ => return event,
    };
    // Both taps are annotated-session taps: their timestamps are nanoseconds.
    // Physical HID events used raw Mach ticks on the tested Apple Silicon system.
    // SAFETY: CoreGraphics supplied a valid event to this callback.
    let capture_at = CaptureInstant::from_nanos(unsafe { CGEventGetTimestamp(event) });
    if crate::app_settings::hotkey_capture_active() {
        context.activity.observe(input, false);
        context
            .shortcut_suppression
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .reset();
        return event;
    }
    let delivered = send_input(context, input, capture_at);
    let mut suppression = context
        .shortcut_suppression
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let suppress = suppression.process_all(
        input,
        crate::app_settings::runtime_hotkeys(),
        delivered,
        context.escape_cancels.load(Ordering::Acquire),
    );
    context.activity.observe(input, suppress);
    // A key Hex consumed (its shortcuts, Escape) was never the app's chord.
    #[cfg(hex_capture)]
    if let Some(key) = capture_key
        && !suppress
        && !list_swallows
    {
        crate::capture_host::report_input(capture_at, key);
    }
    if suppress || list_swallows {
        ptr::null_mut()
    } else {
        event
    }
}

/// What the session tap does with a key while the capture capsule's list
/// is open (`capture_host::list_open`).
#[cfg(hex_capture)]
enum ListSwallow {
    /// Not the list's.
    No,
    /// The list's Escape: kept from the app and from the listener, so it
    /// cancels nothing else.
    Only,
    /// The list's, or the release of a key it took: kept from the app, but
    /// the listener still sees it, so a held ⌥ typing a character stays a
    /// chord rather than a dictation.
    AfterListener,
}

/// Whether the capsule's list or suggestions may take a key press: only
/// while captures are on, something is open, and no shortcut is being
/// recorded. With captures turned off, a capsule left open takes nothing.
#[cfg(hex_capture)]
fn list_takes_keys(capture_on: bool, open: bool, recording_shortcut: bool) -> bool {
    capture_on && open && !recording_shortcut
}

/// Hands the open list its keys (`capture_host::list_key`) on the tap's
/// thread; their releases stay with it even after it closes.
#[cfg(hex_capture)]
fn list_tap(event_type: u32, event: EventRef) -> ListSwallow {
    use crate::capture_host::{CapsuleKey, ListTap, capsule_key, list_key, list_open, suggesting};
    static TAKEN: Mutex<u128> = Mutex::new(0);
    // Shift is followed always, so a release while nothing showed never
    // leaves it stuck down; the list or suggestions hear each change.
    static SHIFT: AtomicBool = AtomicBool::new(false);
    if event_type == EVENT_FLAGS_CHANGED {
        const SHIFT_MASK: u64 = 1 << 17;
        // SAFETY: CoreGraphics supplied a valid event.
        let down = unsafe { CGEventGetFlags(event) } & SHIFT_MASK != 0;
        if SHIFT.swap(down, Ordering::Relaxed) != down
            && list_takes_keys(
                crate::app_settings::capture_on(),
                list_open() || suggesting(),
                crate::app_settings::hotkey_capture_active(),
            )
        {
            capsule_key(CapsuleKey::Shift(down));
        }
        return ListSwallow::No;
    }
    if !matches!(event_type, EVENT_KEY_DOWN | EVENT_KEY_UP) {
        return ListSwallow::No;
    }
    // SAFETY: CoreGraphics supplied a valid keyboard event.
    let code = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) } as u16;
    let Some(bit) = 1u128.checked_shl(u32::from(code)) else {
        return ListSwallow::No;
    };
    let mut taken = TAKEN.lock().unwrap_or_else(|error| error.into_inner());
    if event_type == EVENT_KEY_UP {
        let ours = *taken & bit != 0;
        *taken &= !bit;
        return if ours {
            ListSwallow::AfterListener
        } else {
            ListSwallow::No
        };
    }
    let suggesting = suggesting();
    if !list_takes_keys(
        crate::app_settings::capture_on(),
        list_open() || suggesting,
        crate::app_settings::hotkey_capture_active(),
    ) {
        return ListSwallow::No;
    }
    // SAFETY: CoreGraphics supplied a valid event.
    let flags = unsafe { CGEventGetFlags(event) };
    if crate::app_settings::runtime_hotkeys()
        .all()
        .iter()
        .flatten()
        .any(|hotkey| hotkey.matches_key_press(code, flags))
    {
        return ListSwallow::No;
    }
    let mut typed = [0u16; 8];
    let mut length = 0usize;
    // SAFETY: the buffer holds `typed.len()` UTF-16 units; CoreGraphics
    // writes at most that many and reports how many in `length`.
    unsafe { CGEventKeyboardGetUnicodeString(event, typed.len(), &mut length, typed.as_mut_ptr()) };
    let typed = String::from_utf16_lossy(&typed[..length.min(typed.len())]);
    match list_key(code, flags, &typed) {
        // The suggestions take only the toast's Return and Tab; the rest
        // goes on (the capture shortcut reaches them through the listener).
        ListTap::Key(key @ (CapsuleKey::Return { .. } | CapsuleKey::Tab { .. })) if suggesting => {
            *taken |= bit;
            drop(taken);
            if capsule_key(key) {
                ListSwallow::AfterListener
            } else {
                *TAKEN.lock().unwrap_or_else(|error| error.into_inner()) &= !bit;
                ListSwallow::No
            }
        }
        _ if suggesting => ListSwallow::No,
        ListTap::Pass => ListSwallow::No,
        ListTap::Swallow => {
            *taken |= bit;
            ListSwallow::AfterListener
        }
        ListTap::Key(key) => {
            *taken |= bit;
            drop(taken);
            let escape = key == CapsuleKey::Escape;
            capsule_key(key);
            if escape {
                ListSwallow::Only
            } else {
                ListSwallow::AfterListener
            }
        }
    }
}

/// What the session tap does with an event while a capture records.
#[cfg(hex_capture)]
enum CaptureTap {
    /// Reported; only a capture wants it, so the listener never sees it.
    Only,
    /// Reported, and it goes on to the listener too (mouse downs, modifier
    /// changes) or isn't a capture's (key ups, other buttons).
    Shared,
    /// A fresh key press: reported after suppression, only if it reaches
    /// the app, and never while a shortcut is being recorded.
    Key(crate::capture_host::Tapped),
}

/// Reports a pointer or modifier event to a recording capture
/// (`capture_host::report_input`) and says what else happens to the event.
#[cfg(hex_capture)]
fn report_capture_input(event_type: u32, event: EventRef) -> CaptureTap {
    use crate::capture_host::{Tapped, modifiers, report_input};
    use scrawl_capture::hosted_input::HostInput;
    // SAFETY: CoreGraphics supplied a valid event to this callback.
    let at = CaptureInstant::from_nanos(unsafe { CGEventGetTimestamp(event) });
    // SAFETY: CoreGraphics supplied a valid event.
    let flags = unsafe { CGEventGetFlags(event) };
    // SAFETY: CoreGraphics supplied a valid event; every event has a location.
    let CgPoint { x, y } = unsafe { CGEventGetLocation(event) };
    let held = modifiers(flags);
    let (input, capture_only) = match event_type {
        EVENT_LEFT_MOUSE_DOWN
        | EVENT_RIGHT_MOUSE_DOWN
        | EVENT_LEFT_MOUSE_UP
        | EVENT_RIGHT_MOUSE_UP => (
            Tapped::Pointer(HostInput::Button {
                down: matches!(event_type, EVENT_LEFT_MOUSE_DOWN | EVENT_RIGHT_MOUSE_DOWN),
                right: matches!(event_type, EVENT_RIGHT_MOUSE_DOWN | EVENT_RIGHT_MOUSE_UP),
                x,
                y,
                modifiers: held,
            }),
            !matches!(event_type, EVENT_LEFT_MOUSE_DOWN | EVENT_RIGHT_MOUSE_DOWN),
        ),
        EVENT_MOUSE_MOVED
        | EVENT_LEFT_MOUSE_DRAGGED
        | EVENT_RIGHT_MOUSE_DRAGGED
        | EVENT_OTHER_MOUSE_DRAGGED => (
            Tapped::Pointer(HostInput::Moved {
                x,
                y,
                modifiers: held,
            }),
            true,
        ),
        EVENT_SCROLL_WHEEL => (
            Tapped::Pointer(HostInput::Scroll {
                x,
                y,
                // SAFETY: CoreGraphics supplied a valid scroll event.
                dx: unsafe { CGEventGetIntegerValueField(event, SCROLL_WHEEL_DELTA_AXIS_2) } as f64,
                // SAFETY: CoreGraphics supplied a valid scroll event.
                dy: unsafe { CGEventGetIntegerValueField(event, SCROLL_WHEEL_DELTA_AXIS_1) } as f64,
            }),
            true,
        ),
        EVENT_FLAGS_CHANGED => (
            Tapped::Pointer(HostInput::ModifiersChanged {
                x,
                y,
                modifiers: held,
            }),
            false,
        ),
        // SAFETY: CoreGraphics supplied a valid keyboard event.
        EVENT_KEY_DOWN
            if unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_AUTOREPEAT) } == 0 =>
        {
            return CaptureTap::Key(Tapped::Key {
                // SAFETY: CoreGraphics supplied a valid keyboard event.
                code: unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) as u16 },
                flags,
            });
        }
        EVENT_OTHER_MOUSE_DOWN | EVENT_KEY_DOWN | EVENT_KEY_UP => return CaptureTap::Shared,
        _ => return CaptureTap::Only,
    };
    report_input(at, input);
    if capture_only {
        CaptureTap::Only
    } else {
        CaptureTap::Shared
    }
}

fn send_input(context: &EventTapContext, event: InputEvent, capture_at: CaptureInstant) -> bool {
    let sequence = context.next_sequence.fetch_add(1, Ordering::Relaxed);
    context.pending.push(sequence, capture_at);
    match context.sender.send(ObservedInputEvent {
        sequence,
        event,
        capture_at,
    }) {
        Ok(()) => true,
        Err(_) => {
            context.pending.acknowledge(sequence);
            // SAFETY: The callback runs on the event-tap thread's run loop.
            unsafe { CFRunLoopStop(CFRunLoopGetCurrent()) };
            false
        }
    }
}

#[derive(Default)]
struct ShortcutSuppression {
    // Repeats and releases keep the original press's suppression decision.
    key_presses: HashMap<u16, bool>,
}

impl ShortcutSuppression {
    fn tracked_mouse_release(&self, button: u16, flags: u64) -> Option<InputEvent> {
        let code = crate::app_settings::HotkeyKey::mouse_code(button)?;
        self.key_presses
            .contains_key(&code)
            .then_some(InputEvent::Key {
                code,
                down: false,
                flags,
            })
    }

    fn suppresses_mouse_drag(&self, button: u16) -> bool {
        crate::app_settings::HotkeyKey::mouse_code(button)
            .and_then(|code| self.key_presses.get(&code))
            .copied()
            .unwrap_or(false)
    }

    fn reset(&mut self) {
        self.key_presses.clear();
    }

    fn process_all(
        &mut self,
        input: InputEvent,
        hotkeys: RuntimeHotkeys,
        delivered: bool,
        escape_cancels: bool,
    ) -> bool {
        let bindings = hotkeys.all();
        match input {
            InputEvent::Key {
                code,
                down: true,
                flags,
            } => *self.key_presses.entry(code).or_insert_with(|| {
                delivered
                    && ((code == ESCAPE_KEY_CODE && escape_cancels)
                        || bindings
                            .iter()
                            .flatten()
                            .any(|hotkey| hotkey.matches_key_press(code, flags)))
            }),
            InputEvent::Key {
                code, down: false, ..
            } => self.key_presses.remove(&code).unwrap_or(false),
            _ => false,
        }
    }

    #[cfg(test)]
    fn process(
        &mut self,
        input: InputEvent,
        paste_key_code: u16,
        hotkey: RuntimeHotkey,
        delivered: bool,
    ) -> bool {
        let mut paste_last = HotkeyBinding::paste_last_default().runtime();
        paste_last.key_code = Some(paste_key_code);
        let mut paste_meeting = HotkeyBinding::paste_meeting_default().runtime();
        paste_meeting.key_code = Some(paste_key_code);
        self.process_all(
            input,
            RuntimeHotkeys {
                dictation: hotkey,
                paste_last: Some(paste_last),
                paste_meeting: Some(paste_meeting),
                capture: None,
            },
            delivered,
            false,
        )
    }
}

fn paste_action(input: InputEvent, hotkeys: RuntimeHotkeys) -> Option<HotkeyAction> {
    let InputEvent::Key {
        code,
        down: true,
        flags,
    } = input
    else {
        return None;
    };
    hotkeys
        .paste_last
        .filter(|binding| binding.matches_key_press(code, flags))
        .map(|_| HotkeyAction::PasteLast)
        .or_else(|| {
            hotkeys
                .paste_meeting
                .filter(|binding| binding.matches_key_press(code, flags))
                .map(|_| HotkeyAction::PasteMeeting)
        })
}

#[cfg(test)]
fn paste_action_with_meetings(
    input: InputEvent,
    paste_key_code: u16,
    meetings_enabled: bool,
) -> Option<HotkeyAction> {
    let mut hotkeys = RuntimeHotkeys::default();
    if let Some(binding) = &mut hotkeys.paste_last {
        binding.key_code = Some(paste_key_code);
    }
    hotkeys.paste_meeting = meetings_enabled.then(|| {
        let mut binding = HotkeyBinding::paste_meeting_default().runtime();
        binding.key_code = Some(paste_key_code);
        binding
    });
    paste_action(input, hotkeys)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeyAction {
    Start,
    Finish,
    Discard(DiscardCause),
    Cancel,
    PasteLast,
    PasteMeeting,
}

impl HotkeyAction {
    pub fn discard_cause(self) -> Option<DiscardCause> {
        match self {
            Self::Discard(cause) => Some(cause),
            _ => None,
        }
    }
}

#[derive(Debug)]
enum State {
    Idle {
        // The last Recording release, kept only while double-tap lock may
        // still pair the next press with it.
        last_release_at: Option<CaptureInstant>,
    },
    FirstTapPressed,
    AwaitingSecondTap {
        released_at: CaptureInstant,
    },
    SecondTapPressed {
        first_released_at: CaptureInstant,
    },
    Recording {
        started_at: CaptureInstant,
        previous_release: Option<CaptureInstant>,
    },
    Locked,
    Dirty,
}

impl State {
    const IDLE: Self = Self::Idle {
        last_release_at: None,
    };

    /// A double-tap-only gesture that has not yet locked.
    fn is_pending_gesture(&self) -> bool {
        matches!(
            self,
            Self::FirstTapPressed | Self::AwaitingSecondTap { .. } | Self::SecondTapPressed { .. }
        )
    }
}

/// Modifier changes seen since the keyboard last reported no modifiers.
#[derive(Clone, Copy)]
struct ModifierHistory {
    flags: u64,
    flags_at: CaptureInstant,
    since_neutral: u64,
    neutral_at: Option<CaptureInstant>,
}

impl ModifierHistory {
    /// No evidence, and none from events stamped before `at`.
    const fn reset(at: CaptureInstant) -> Self {
        Self {
            flags: 0,
            flags_at: at,
            since_neutral: 0,
            neutral_at: None,
        }
    }
}

/// Live keyboard state, read only by the stale-press check.
#[derive(Clone, Copy)]
struct PhysicalKeyboard {
    flags: fn() -> u64,
    key_down: fn(u16) -> bool,
}

impl PhysicalKeyboard {
    // Same source as `recover_stale_keys`. Unit tests never read the real
    // keyboard (see `trigger_is_physically_down`); their default reads as
    // everything up, so every test can take the fast path.
    const SESSION: Self = if cfg!(test) {
        Self {
            flags: || 0,
            key_down: |_| false,
        }
    } else {
        Self {
            // SAFETY: Read-only queries of a documented event-source state.
            flags: || unsafe { CGEventSourceFlagsState(COMBINED_SESSION_STATE) },
            key_down: |code| unsafe { CGEventSourceKeyState(COMBINED_SESSION_STATE, code) },
        }
    };
}

pub struct DictationHotkey {
    state: State,
    pressed_keys: HashSet<u16>,
    double_tap_enabled: bool,
    double_tap_only: bool,
    /// Press to start, press again to finish: releasing locks the capture.
    toggle: bool,
    binding: RuntimeHotkey,
    ignore_before: Option<CaptureInstant>,
    stale_keys_neutral_since: Option<CaptureInstant>,
    recovery_ignore_through: Option<CaptureInstant>,
    recovery_updated_keys: HashSet<u16>,
    // Fn evidence comes only from modifier events; None marks a blind period.
    // The timestamp prevents delayed events from restoring invalidated evidence.
    function_modifier: (CaptureInstant, Option<bool>),
    // Recent HID key presses, which another app's shortcut may have hidden.
    physical_key_downs: VecDeque<(CaptureInstant, u16, HidTiming)>,
    // When the held modifier-only press Hex owns is due its chord breaker.
    chord_break_at: Option<CaptureInstant>,
    modifier_history: ModifierHistory,
    // When each tracked key last went down.
    key_down_at: HashMap<u16, CaptureInstant>,
    // When each modifier key (by its device flag) went down, for chords
    // that hold both sides of one.
    side_down_at: HashMap<u64, CaptureInstant>,
    physical: PhysicalKeyboard,
}

impl DictationHotkey {
    pub fn new(now: CaptureInstant, double_tap_enabled: bool, binding: RuntimeHotkey) -> Self {
        Self::with_binding(
            trigger_is_physically_down(binding),
            now,
            double_tap_enabled,
            binding,
        )
    }

    fn with_binding(
        trigger_down: bool,
        now: CaptureInstant,
        double_tap_enabled: bool,
        binding: RuntimeHotkey,
    ) -> Self {
        Self {
            state: if trigger_down {
                State::Recording {
                    started_at: now,
                    previous_release: None,
                }
            } else {
                State::IDLE
            },
            pressed_keys: HashSet::new(),
            double_tap_enabled,
            double_tap_only: false,
            toggle: false,
            binding,
            ignore_before: None,
            stale_keys_neutral_since: None,
            recovery_ignore_through: None,
            recovery_updated_keys: HashSet::new(),
            function_modifier: (CaptureInstant::ZERO, None),
            physical_key_downs: VecDeque::new(),
            chord_break_at: None,
            modifier_history: ModifierHistory::reset(CaptureInstant::ZERO),
            key_down_at: HashMap::new(),
            side_down_at: HashMap::new(),
            physical: PhysicalKeyboard::SESSION,
        }
    }

    pub fn is_recording(&self) -> bool {
        matches!(self.state, State::Recording { .. } | State::Locked)
    }

    /// Recording hands-free: locked by a double tap, a Toggle release, or
    /// `hex dictation start`.
    #[cfg(test)]
    pub fn is_locked(&self) -> bool {
        matches!(self.state, State::Locked)
    }

    pub fn suppresses_recognition(&self) -> bool {
        !matches!(self.state, State::Idle { .. })
    }

    pub fn set_double_tap_enabled(&mut self, enabled: bool) {
        self.double_tap_enabled = enabled;
        if !enabled {
            match &mut self.state {
                State::Idle { last_release_at } => *last_release_at = None,
                State::Recording {
                    previous_release, ..
                } => *previous_release = None,
                _ => {}
            }
        }
    }

    pub fn set_double_tap_only(&mut self, enabled: bool) {
        self.double_tap_only =
            enabled && self.binding.key_code.is_some() && self.double_tap_enabled;
        // Active captures still need their release or Escape to reach the audio owner.
        if !self.double_tap_only && self.state.is_pending_gesture() {
            self.state = State::IDLE;
        }
    }

    /// Takes effect from the next press; an active capture keeps its gesture.
    pub fn set_toggle(&mut self, enabled: bool) {
        self.toggle = enabled;
    }

    pub fn set_binding(&mut self, binding: RuntimeHotkey) {
        if matches!(self.state, State::Idle { .. }) && self.binding != binding {
            self.binding = binding;
            self.state = State::IDLE;
        }
    }

    pub fn suspend(&mut self) -> Option<HotkeyAction> {
        let was_recording = self.is_recording();
        let now = CaptureInstant::now();
        self.function_modifier = (now, None);
        self.modifier_history = ModifierHistory::reset(now);
        self.state = State::IDLE;
        self.pressed_keys.clear();
        self.key_down_at.clear();
        self.side_down_at.clear();
        self.stale_keys_neutral_since = None;
        self.chord_break_at = None;
        was_recording.then_some(HotkeyAction::Cancel)
    }

    /// Locks an idle machine without a press, as a `hex dictation start`
    /// does: the next shortcut press finishes it and Escape cancels it, the
    /// same as a double-tap lock. Returns false while a press is in flight.
    pub fn lock(&mut self) -> bool {
        if !matches!(self.state, State::Idle { .. }) && !self.state.is_pending_gesture() {
            return false;
        }
        self.state = State::Locked;
        self.pressed_keys.clear();
        self.key_down_at.clear();
        self.stale_keys_neutral_since = None;
        self.chord_break_at = None;
        true
    }

    pub fn disarm_pending_gesture(&mut self) {
        if !self.is_recording() {
            if matches!(self.state, State::Idle { .. }) || self.state.is_pending_gesture() {
                self.state = State::IDLE;
            }
            self.stale_keys_neutral_since = None;
        }
    }

    #[cfg(test)]
    pub fn suppress_until_release(&mut self) {
        self.state = State::Dirty;
        self.stale_keys_neutral_since = None;
        self.chord_break_at = None;
    }

    // Call only after draining input. Polling repairs bookkeeping, never capture boundaries.
    pub fn recover_stale_keys(&mut self) {
        self.recover_stale_keys_with(
            CaptureInstant::now,
            // Match the annotated-session tap, including assistive keyboard input.
            || unsafe { CGEventSourceFlagsState(COMBINED_SESSION_STATE) },
            |code| input_is_down(COMBINED_SESSION_STATE, code),
        );
    }

    fn recover_stale_keys_with(
        &mut self,
        now: impl FnOnce() -> CaptureInstant,
        mut flags: impl FnMut() -> u64,
        mut key_down: impl FnMut(u16) -> bool,
    ) {
        let recoverable = match self.state {
            State::Dirty => true,
            // A lock captures while the keyboard sits idle. A missed key-up would
            // otherwise block the fresh shortcut press that finishes it.
            State::Idle { .. } | State::Locked => !self.pressed_keys.is_empty(),
            _ => false,
        };
        if !recoverable {
            self.stale_keys_neutral_since = None;
            return;
        }
        // A missing modifier release can leave Dirty with no ordinary keys tracked.
        // Avoid a full scan when a tracked key is still physically held.
        // Arrow key metadata can leave SecondaryFn set after release. Only ignore
        // it when modifier-change events independently say Fn is not held.
        let modifiers_mask = if self.function_modifier.1 == Some(false) {
            HOTKEY_MODIFIERS_MASK & !crate::app_settings::FUNCTION_KEY_MASK
        } else {
            HOTKEY_MODIFIERS_MASK
        };
        if flags() & modifiers_mask != 0
            || (!self.pressed_keys.is_empty()
                && self.pressed_keys.iter().all(|&code| key_down(code)))
            || (0..128).any(&mut key_down)
            // Mouse triggers share pressed_keys; a stale keyboard entry must
            // not cause recovery while a tracked mouse button is still down.
            || self.pressed_keys.iter().any(|&code| {
                crate::app_settings::HotkeyKey::mouse_button(code).is_some() && key_down(code)
            })
            || flags() & modifiers_mask != 0
        {
            self.stale_keys_neutral_since = None;
            return;
        }
        let sampled_through = now();
        let Some(neutral_since) = self.stale_keys_neutral_since else {
            self.stale_keys_neutral_since = Some(sampled_through);
            return;
        };
        if sampled_through.duration_since(neutral_since) < STALE_KEY_NEUTRAL_DURATION {
            return;
        }
        let locked = matches!(self.state, State::Locked);
        // A system shortcut such as Command-Tab swallows its key's release.
        tracing::warn!(
            key_count = self.pressed_keys.len(),
            key_codes = ?self.pressed_keys,
            locked,
            "resynchronized stale input tracking after neutral keyboard"
        );
        self.pressed_keys.clear();
        self.key_down_at.clear();
        // Repair bookkeeping only: a lock keeps capturing until a fresh press or Escape.
        if !locked {
            self.state = State::IDLE;
        }
        self.chord_break_at = None;
        self.stale_keys_neutral_since = None;
        self.recovery_ignore_through = Some(sampled_through);
        self.recovery_updated_keys.clear();
    }

    #[cfg(test)]
    fn wait_for_release_with_state(&mut self, now: CaptureInstant, flags: u64, key_down: bool) {
        self.suspend();
        self.ignore_before = Some(now);
        if key_down && let Some(code) = self.binding.key_code {
            self.pressed_keys.insert(code);
        }
        if flags & HOTKEY_MODIFIERS_MASK != 0 || !self.pressed_keys.is_empty() {
            self.state = State::Dirty;
        }
    }

    // Suspended shortcut matching must still observe releases of previously held keys.
    pub fn track_key_state(&mut self, event: InputEvent, at: CaptureInstant) -> Option<bool> {
        if let InputEvent::Key { code, .. } | InputEvent::PhysicalKeyDown { code, .. } = event
            && is_own_globe_key(code, self.binding)
        {
            return None;
        }
        self.stale_keys_neutral_since = None;
        let fenced = self.ignore_before.is_some_and(|changed_at| at < changed_at)
            || self
                .recovery_ignore_through
                .is_some_and(|fence| at <= fence);
        if let InputEvent::Flags(flags) = event {
            for mask in MODIFIER_SIDE_MASKS {
                if flags & mask == 0 {
                    self.side_down_at.remove(&mask);
                } else {
                    self.side_down_at.entry(mask).or_insert(at);
                }
            }
        } else if matches!(event, InputEvent::TapDisabled) {
            self.side_down_at.clear();
        }
        if let InputEvent::Flags(flags) = event
            && at >= self.function_modifier.0
        {
            self.function_modifier = (
                at,
                Some(flags & crate::app_settings::FUNCTION_KEY_MASK != 0),
            );
        } else if matches!(event, InputEvent::TapDisabled) {
            let now = CaptureInstant::now();
            self.function_modifier = (now, None);
            self.modifier_history = ModifierHistory::reset(now);
        }
        // Only modifier changes: key events can carry Fn as navigation metadata.
        // Fenced edges predate the evidence that replaced them.
        if let InputEvent::Flags(flags) = event
            && !fenced
            && at >= self.modifier_history.flags_at
        {
            let history = &mut self.modifier_history;
            history.flags = flags;
            history.flags_at = at;
            if flags & HOTKEY_MODIFIERS_MASK == 0 {
                history.since_neutral = 0;
                history.neutral_at = Some(at);
            } else {
                history.since_neutral |= flags & HOTKEY_MODIFIERS_MASK;
            }
        }
        if let InputEvent::Key { code, .. } = event
            && let Some(fence) = self.recovery_ignore_through
        {
            // A delayed pre-recovery release must not erase a newer press of that key.
            if at <= fence && self.recovery_updated_keys.contains(&code) {
                return None;
            }
            if at > fence {
                self.recovery_updated_keys.insert(code);
            }
        }
        Some(match event {
            InputEvent::Key { code, down, .. } => {
                if down {
                    let down_at = self.key_down_at.entry(code).or_insert(at);
                    *down_at = (*down_at).max(at);
                    self.pressed_keys.insert(code)
                } else {
                    self.pressed_keys.remove(&code);
                    self.key_down_at.remove(&code);
                    false
                }
            }
            InputEvent::Flags(_)
            | InputEvent::PhysicalKeyDown { .. }
            | InputEvent::MouseDown
            | InputEvent::TapDisabled => false,
        })
    }

    fn log_hid(
        &self,
        message: &'static str,
        code: u16,
        at: CaptureInstant,
        timing: HidTiming,
        press_at: CaptureInstant,
    ) {
        tracing::info!(
            shortcut = "dictation",
            cause = DiscardCause::HidKey.as_str(),
            key_code = code,
            after_press_ms = signed_ms(at, press_at),
            hid_raw = timing.raw,
            hid_clock = timing.clock.as_str(),
            hid_received_after_press_ms = signed_ms(timing.received_at, press_at),
            hid_arrival_delay_ms = signed_ms(timing.received_at, at),
            "{message}"
        );
    }

    pub fn process(&mut self, event: InputEvent, now: CaptureInstant) -> Option<HotkeyAction> {
        let was_locked = matches!(self.state, State::Locked);
        let action = self.process_event(event, now);
        self.track_chord_break(event, now, action, was_locked);
        action
    }

    /// A bare modifier passes through to other apps, so a hold reads to them
    /// as a tap on release (Raycast's Option-tap hotkey, for one). Once Hex
    /// owns the press (a hold past the threshold, a Toggle start past the
    /// chord grace, or the press that finishes a lock) it is owed one chord
    /// breaker while still held.
    fn track_chord_break(
        &mut self,
        event: InputEvent,
        now: CaptureInstant,
        action: Option<HotkeyAction>,
        was_locked: bool,
    ) {
        if self.binding.key_code.is_some() || self.binding.modifiers.function {
            self.chord_break_at = None;
            return;
        }
        let released = match event {
            InputEvent::Flags(flags) | InputEvent::Key { flags, .. } => {
                !self.binding.required_modifiers_down(flags)
            }
            InputEvent::TapDisabled => true,
            InputEvent::PhysicalKeyDown { .. } | InputEvent::MouseDown => false,
        };
        match action {
            Some(HotkeyAction::Start) if matches!(self.state, State::Recording { .. }) => {
                // Toggle's press is usually a short tap, so it can't wait out
                // the hold; the grace lets an extra modifier pressed with it
                // discard it first.
                self.chord_break_at = Some(if self.toggle {
                    now + crate::feedback::CHORD_GRACE
                } else {
                    now + MINIMUM_HOLD_DURATION
                });
            }
            Some(HotkeyAction::Finish) if was_locked && !released => {
                self.chord_break_at = Some(now);
            }
            Some(_) => self.chord_break_at = None,
            None if released => self.chord_break_at = None,
            None => {}
        }
    }

    /// Whether the held press is due its chord breaker at `now`; true at most
    /// once per press.
    pub fn take_chord_break(&mut self, now: CaptureInstant) -> bool {
        let due = self.chord_break_at.is_some_and(|due_at| now >= due_at);
        if due {
            self.chord_break_at = None;
        }
        due
    }

    /// Posts the chord breaker the held press is due, if exactly its modifiers
    /// are still physically down; any other modifier already breaks the tap.
    /// The posted key carries Hex's marker, so neither shortcut machine sees
    /// it, and the key tap drops it before the focused app.
    pub fn break_chord(&mut self, now: CaptureInstant) {
        if !self.take_chord_break(now) {
            return;
        }
        // SAFETY: A read-only query of the documented HID system state.
        let flags = unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) };
        if !self.binding.exact_modifiers(flags) {
            return;
        }
        if let Err(error) = crate::keyboard::post_chord_breaker(flags) {
            tracing::warn!(shortcut = "dictation", %error, "chord breaker failed");
        }
    }

    fn process_event(&mut self, event: InputEvent, now: CaptureInstant) -> Option<HotkeyAction> {
        self.stale_keys_neutral_since = None;
        if matches!(event, InputEvent::TapDisabled) {
            let was_recording = self.is_recording();
            let now = CaptureInstant::now();
            self.function_modifier = (now, None);
            self.modifier_history = ModifierHistory::reset(now);
            self.state = State::Dirty;
            return was_recording.then_some(HotkeyAction::Cancel);
        }
        // Queued edges from before an opt-in transition cannot start a new capture.
        if self
            .ignore_before
            .is_some_and(|changed_at| now < changed_at)
        {
            return None;
        }
        let prior = self.modifier_history;
        let fresh_key_down = self.track_key_state(event, now)?;
        let binding_press = self.binding_press(event, fresh_key_down, prior.flags);

        if self
            .recovery_ignore_through
            .is_some_and(|sampled_through| now <= sampled_through)
        {
            if binding_press {
                self.log_ignored_press("inside a stale-key recovery window");
            }
            // A key can go down during the non-atomic scan. Retain its edge but
            // require a later neutral event before accepting another gesture.
            // An old neutral release cannot invalidate the recovery's neutral sample.
            let neutral = matches!(
                event,
                InputEvent::Flags(flags) | InputEvent::Key { flags, .. }
                    if flags & HOTKEY_MODIFIERS_MASK == 0 && self.pressed_keys.is_empty()
            );
            if !neutral && matches!(self.state, State::Idle { .. } | State::Dirty) {
                self.state = State::Dirty;
            }
            return None;
        }

        if fresh_key_down
            && let Some(action) = paste_action(event, crate::app_settings::runtime_hotkeys())
        {
            self.state = State::Dirty;
            return Some(action);
        }
        if event.is_escape_down() && self.is_recording() {
            self.state = State::Dirty;
            return Some(HotkeyAction::Cancel);
        }

        let ignored_because = binding_press
            .then(|| self.resync_stale_press(now))
            .flatten();
        let trigger_pressed = self.trigger_pressed(event, fresh_key_down);
        if let InputEvent::PhysicalKeyDown { code, timing } = event
            && self.binding.key_code != Some(code)
        {
            if self.physical_key_downs.len() == PHYSICAL_KEY_DOWN_HISTORY {
                self.physical_key_downs.pop_front();
            }
            self.physical_key_downs.push_back((now, code, timing));
        }
        // HID evidence can overtake the session event of the press it follows.
        if trigger_pressed
            && matches!(
                self.state,
                State::Idle { .. } | State::AwaitingSecondTap { .. }
            )
            && let Some(&(at, code, timing)) = self.physical_key_downs.iter().find(|(at, ..)| {
                at.checked_duration_since(now)
                    .is_some_and(|after| after > Duration::ZERO && after < MINIMUM_HOLD_DURATION)
            })
        {
            self.log_hid(
                "dictation press dropped: HID key evidence overtook it",
                code,
                at,
                timing,
                now,
            );
            self.state = State::Dirty;
            return None;
        }
        let flags = match event {
            InputEvent::Flags(flags) | InputEvent::Key { flags, .. } => Some(flags),
            InputEvent::PhysicalKeyDown { .. }
            | InputEvent::MouseDown
            | InputEvent::TapDisabled => None,
        };
        let trigger_down = flags.is_some_and(|flags| {
            self.binding.required_modifiers_down(flags)
                && self
                    .binding
                    .key_code
                    .is_none_or(|code| self.pressed_keys.contains(&code))
        });
        let trigger_released = flags.is_some() && !trigger_down;
        let unrelated_key_down = matches!(
            event,
            InputEvent::Key {
                code,
                down: true,
                ..
            } | InputEvent::PhysicalKeyDown { code, .. } if self.binding.key_code != Some(code)
        );
        let extra_modifiers = flags.is_some_and(|flags| {
            self.binding.required_modifiers_down(flags) && !self.binding.exact_modifiers(flags)
        });

        let action = match self.state {
            State::Idle { .. } if self.double_tap_only && !self.toggle && trigger_pressed => {
                self.state = State::FirstTapPressed;
                None
            }
            State::FirstTapPressed if trigger_released => {
                self.state = State::AwaitingSecondTap { released_at: now };
                None
            }
            State::AwaitingSecondTap { released_at }
                if trigger_pressed && now.duration_since(released_at) < DOUBLE_TAP_WINDOW =>
            {
                self.state = State::SecondTapPressed {
                    first_released_at: released_at,
                };
                None
            }
            State::SecondTapPressed { first_released_at }
                if trigger_released
                    && now.duration_since(first_released_at) < DOUBLE_TAP_WINDOW =>
            {
                self.state = State::Locked;
                Some(HotkeyAction::Start)
            }
            State::SecondTapPressed { .. } if trigger_released => {
                self.state = State::IDLE;
                None
            }
            State::AwaitingSecondTap { .. }
            | State::FirstTapPressed
            | State::SecondTapPressed { .. }
                if unrelated_key_down
                    || extra_modifiers
                    || matches!(event, InputEvent::MouseDown) =>
            {
                self.state = State::Dirty;
                None
            }
            State::AwaitingSecondTap { released_at }
                if now.duration_since(released_at) >= DOUBLE_TAP_WINDOW =>
            {
                self.state = if trigger_pressed {
                    State::FirstTapPressed
                } else {
                    State::IDLE
                };
                None
            }
            State::Locked if trigger_pressed => {
                self.state = State::Dirty;
                Some(HotkeyAction::Finish)
            }
            State::Idle { last_release_at } if trigger_pressed => {
                let previous_release = last_release_at.filter(|released| {
                    self.double_tap_enabled && now.duration_since(*released) < DOUBLE_TAP_WINDOW
                });
                self.state = State::Recording {
                    started_at: now,
                    previous_release,
                };
                Some(HotkeyAction::Start)
            }
            State::Recording { .. } if trigger_released && self.toggle => {
                self.state = State::Locked;
                None
            }
            State::Recording {
                previous_release: Some(released),
                ..
            } if trigger_released && now.duration_since(released) < DOUBLE_TAP_WINDOW => {
                self.state = State::Locked;
                None
            }
            State::Recording { .. } if trigger_released => {
                self.state = State::Idle {
                    last_release_at: self.double_tap_enabled.then_some(now),
                };
                Some(HotkeyAction::Finish)
            }
            State::Recording { started_at, .. }
                // Delayed HID evidence from before the press is not a chord.
                if !(matches!(event, InputEvent::PhysicalKeyDown { .. }) && now < started_at)
                    && ((now.duration_since(started_at) < MINIMUM_HOLD_DURATION
                        && (unrelated_key_down
                            || matches!(event, InputEvent::MouseDown)
                            || extra_modifiers))
                        // A held modifier-only shortcut plus any key is a chord at any time.
                        || (unrelated_key_down
                            && self.binding.key_code.is_none()
                            && !self.toggle)) =>
            {
                self.state = State::Dirty;
                let cause = match event {
                    InputEvent::PhysicalKeyDown { code, timing } => {
                        self.log_hid(
                            "dictation press discarded as a chord",
                            code,
                            now,
                            timing,
                            started_at,
                        );
                        DiscardCause::HidKey
                    }
                    InputEvent::Key { code, .. } if unrelated_key_down => {
                        tracing::info!(
                            shortcut = "dictation",
                            cause = DiscardCause::Key.as_str(),
                            key_code = code,
                            after_press_ms = signed_ms(now, started_at),
                            "dictation press discarded as a chord"
                        );
                        DiscardCause::Key
                    }
                    _ => {
                        let cause = if matches!(event, InputEvent::MouseDown) {
                            DiscardCause::Mouse
                        } else {
                            DiscardCause::ExtraModifier
                        };
                        tracing::info!(
                            shortcut = "dictation",
                            cause = cause.as_str(),
                            modifier_flags =
                                format_args!("{:#x}", flags.unwrap_or(0) & HOTKEY_MODIFIERS_MASK),
                            after_press_ms = signed_ms(now, started_at),
                            "dictation press discarded as a chord"
                        );
                        cause
                    }
                };
                Some(HotkeyAction::Discard(cause))
            }
            State::Dirty
                if flags.is_some_and(|flags| flags & HOTKEY_MODIFIERS_MASK == 0)
                    && self.pressed_keys.is_empty() =>
            {
                self.state = State::IDLE;
                None
            }
            _ => None,
        };
        if action.is_none()
            && let Some(reason) = ignored_because
        {
            self.log_ignored_press(reason);
        }
        action
    }

    /// A down edge of the binding, whether or not tracked keys block it.
    fn binding_press(&self, event: InputEvent, fresh_key_down: bool, prior_flags: u64) -> bool {
        match self.binding.key_code {
            Some(_) => self.trigger_pressed(event, fresh_key_down),
            None => {
                matches!(event, InputEvent::Flags(flags) if self.binding.exact_modifiers(flags))
                    && !self.binding.modifiers.is_empty()
                    && !self.binding.required_modifiers_down(prior_flags)
                    && self.both_sides_went_down_together()
            }
        }
    }

    /// A system shortcut such as Command-Tab swallows its key's release, and
    /// the stale key blocks the next press until `recover_stale_keys` has
    /// sampled 100 ms of neutral keyboard. A press of the binding itself
    /// resynchronizes at once, as a fresh press, when the keyboard reported no
    /// modifiers after every tracked key went down, only the binding's
    /// modifiers went down since, and both the tracked keys and every other
    /// modifier read up now. A lock stays locked, so the press finishes it.
    /// Returns why the press stays ignored.
    fn resync_stale_press(&mut self, now: CaptureInstant) -> Option<&'static str> {
        let binding_key = self.binding.key_code;
        let stale = |code: &u16| Some(*code) != binding_key;
        let has_stale = self.pressed_keys.iter().any(stale);
        match self.state {
            State::Dirty if has_stale => {}
            State::Dirty => return Some("waiting for a neutral keyboard"),
            // Tracked keys block only modifier-only presses outside Dirty.
            State::Idle { .. } | State::Locked if has_stale && binding_key.is_none() => {}
            _ => return None,
        }
        let history = self.modifier_history;
        // Typing after the switch is fine; only the stale keys' own presses count.
        let neutral_since_down = |code: &u16| {
            history
                .neutral_at
                .zip(self.key_down_at.get(code).copied())
                .is_some_and(|(neutral, down)| neutral > down)
        };
        if !self
            .pressed_keys
            .iter()
            .filter(|code| stale(code))
            .all(neutral_since_down)
        {
            return Some("no neutral keyboard since the tracked keys went down");
        }
        let binding_mask = modifier_mask(self.binding.modifiers);
        if history.since_neutral & !binding_mask != 0 {
            return Some("another modifier went down since the keyboard was neutral");
        }
        if self
            .pressed_keys
            .iter()
            .filter(|code| stale(code))
            .any(|&code| (self.physical.key_down)(code))
        {
            return Some("a tracked key is physically down");
        }
        // As in recovery, arrow keys can leave SecondaryFn set after release.
        let mut others = HOTKEY_MODIFIERS_MASK & !binding_mask;
        if self.function_modifier.1 == Some(false) {
            others &= !crate::app_settings::FUNCTION_KEY_MASK;
        }
        if (self.physical.flags)() & others != 0 {
            return Some("another modifier is physically down");
        }
        tracing::info!(
            shortcut = "dictation",
            key_codes = ?self.pressed_keys,
            "resynchronized stale input tracking on a shortcut press"
        );
        self.pressed_keys.retain(|code| !stale(code));
        let pressed = &self.pressed_keys;
        self.key_down_at.retain(|code, _| pressed.contains(code));
        // Command-Tab while hands-free: the press finishes the lock.
        if !matches!(self.state, State::Locked) {
            self.state = State::IDLE;
        }
        self.stale_keys_neutral_since = None;
        // Edges stamped before the press must not finish or restart it, and a
        // delayed older release must not untrack the binding's key. Edges at
        // the press itself still count, so a same-tick chord discards it.
        self.recovery_ignore_through = now.checked_sub(Duration::from_nanos(1));
        self.recovery_updated_keys = binding_key
            .filter(|code| self.pressed_keys.contains(code))
            .into_iter()
            .collect();
        None
    }

    fn log_ignored_press(&self, reason: &'static str) {
        tracing::info!(
            shortcut = "dictation",
            reason,
            key_codes = ?self.pressed_keys,
            "shortcut press ignored"
        );
    }

    fn trigger_pressed(&self, event: InputEvent, fresh_key_down: bool) -> bool {
        match self.binding.key_code {
            Some(key_code) => {
                fresh_key_down
                    && matches!(
                        event,
                        InputEvent::Key {
                            code,
                            down: true,
                            flags,
                        } if code == key_code && self.binding.exact_modifiers(flags)
                    )
            }
            None => {
                matches!(event, InputEvent::Flags(flags) if self.binding.exact_modifiers(flags))
                    && !self.binding.modifiers.is_empty()
                    && self.pressed_keys.is_empty()
                    && self.both_sides_went_down_together()
            }
        }
    }

    /// A modifier-only chord on both sides of a modifier (both Options)
    /// presses only when its two keys go down within `BOTH_SIDES_WINDOW`,
    /// in either order, as one gesture; one held and the other added later
    /// presses nothing. Other chords press as soon as their keys are down.
    fn both_sides_went_down_together(&self) -> bool {
        self.binding.both_sides_masks().all(|(left, right)| {
            match (self.side_down_at.get(&left), self.side_down_at.get(&right)) {
                (Some(&left), Some(&right)) => {
                    left.max(right).duration_since(left.min(right)) <= BOTH_SIDES_WINDOW
                }
                _ => false,
            }
        })
    }
}

/// Each modifier key's device flag, left and right.
const MODIFIER_SIDE_MASKS: [u64; 8] = {
    use crate::app_settings::{
        LEFT_COMMAND_MASK, LEFT_CONTROL_MASK, LEFT_OPTION_MASK, LEFT_SHIFT_MASK,
        RIGHT_COMMAND_MASK, RIGHT_CONTROL_MASK, RIGHT_OPTION_MASK, RIGHT_SHIFT_MASK,
    };
    [
        LEFT_CONTROL_MASK,
        RIGHT_CONTROL_MASK,
        LEFT_OPTION_MASK,
        RIGHT_OPTION_MASK,
        LEFT_SHIFT_MASK,
        RIGHT_SHIFT_MASK,
        LEFT_COMMAND_MASK,
        RIGHT_COMMAND_MASK,
    ]
};

/// The device-independent flag bits of `modifiers`, either side.
fn modifier_mask(modifiers: crate::app_settings::HotkeyModifiers) -> u64 {
    use crate::app_settings::{
        COMMAND_KEY_MASK, CONTROL_KEY_MASK, FUNCTION_KEY_MASK, OPTION_KEY_MASK, SHIFT_KEY_MASK,
    };
    [
        (modifiers.control.is_some(), CONTROL_KEY_MASK),
        (modifiers.option.is_some(), OPTION_KEY_MASK),
        (modifiers.shift.is_some(), SHIFT_KEY_MASK),
        (modifiers.command.is_some(), COMMAND_KEY_MASK),
        (modifiers.function, FUNCTION_KEY_MASK),
    ]
    .into_iter()
    .filter(|(held, _)| *held)
    .fold(0, |mask, (_, bit)| mask | bit)
}

fn trigger_is_physically_down(binding: RuntimeHotkey) -> bool {
    // Unit tests never read the real keyboard: a key held while they run must
    // not change their outcome, and the first HID query can deadlock in a
    // process without a window-server session.
    if cfg!(test) {
        return false;
    }
    // HID state reflects the physical keyboard. Combined-session state can
    // remain latched after synthetic events or application switching.
    // SAFETY: These are pure system queries with a documented state id.
    unsafe {
        !binding.is_empty()
            && binding.exact_modifiers(CGEventSourceFlagsState(HID_SYSTEM_STATE))
            && binding
                .key_code
                .is_none_or(|code| input_is_down(HID_SYSTEM_STATE, code))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::{
        COMMAND_KEY_MASK, CONTROL_KEY_MASK, FUNCTION_KEY_MASK, OPTION_KEY_MASK, SHIFT_KEY_MASK,
    };

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreate(source: *mut c_void) -> EventRef;
        fn CGEventSetTimestamp(event: EventRef, timestamp: u64);
        fn CGEventSetType(event: EventRef, event_type: u32);
        fn CGEventSetFlags(event: EventRef, flags: u64);
        fn CGEventSetIntegerValueField(event: EventRef, field: u32, value: i64);
    }

    fn test_tap_context(
        escape_cancels: bool,
    ) -> (EventTapContext, mpsc::Receiver<ObservedInputEvent>) {
        let (sender, receiver) = mpsc::channel();
        let context = EventTapContext {
            sender,
            activity: InputActivity::default(),
            escape_cancels: Arc::new(AtomicBool::new(escape_cancels)),
            key_tap: AtomicPtr::new(ptr::null_mut()),
            observation_tap: AtomicPtr::new(ptr::null_mut()),
            physical_tap: AtomicPtr::new(ptr::null_mut()),
            shortcut_suppression: Mutex::new(ShortcutSuppression::default()),
            pending: PendingInputEvents::default(),
            next_sequence: AtomicU64::new(0),
        };
        (context, receiver)
    }

    // Drive the real callback without installing a tap or posting global input.
    fn callback_input(timestamps_and_events: &[(u64, InputEvent)]) -> Vec<ObservedInputEvent> {
        callback_input_with(false, timestamps_and_events)
    }

    fn callback_input_with(
        escape_cancels: bool,
        timestamps_and_events: &[(u64, InputEvent)],
    ) -> Vec<ObservedInputEvent> {
        let (mut context, receiver) = test_tap_context(escape_cancels);
        for &(timestamp, input) in timestamps_and_events {
            // SAFETY: This locally owned event and context live through the callback.
            unsafe {
                let event = CGEventCreate(ptr::null_mut());
                assert!(!event.is_null());
                CGEventSetTimestamp(event, timestamp);
                let event_type = match input {
                    InputEvent::Flags(flags) => {
                        CGEventSetType(event, EVENT_FLAGS_CHANGED);
                        CGEventSetFlags(event, flags);
                        EVENT_FLAGS_CHANGED
                    }
                    InputEvent::Key { code, down, flags } => {
                        CGEventSetType(event, if down { EVENT_KEY_DOWN } else { EVENT_KEY_UP });
                        CGEventSetFlags(event, flags);
                        CGEventSetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE, i64::from(code));
                        assert_eq!(
                            CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE),
                            i64::from(code)
                        );
                        if down { EVENT_KEY_DOWN } else { EVENT_KEY_UP }
                    }
                    _ => panic!("expected a keyboard event"),
                };
                let returned = event_callback(
                    ptr::null_mut(),
                    event_type,
                    event,
                    (&mut context as *mut EventTapContext).cast(),
                );
                CFRelease(event.cast_const());
                if matches!(input, InputEvent::Flags(_)) {
                    assert_eq!(returned, event, "modifier observation must remain passive");
                }
            }
        }
        receiver.try_iter().collect()
    }

    #[cfg(hex_capture)]
    #[test]
    fn a_capture_logs_only_keys_that_reach_the_app() {
        const ESCAPE: u16 = 53;
        const K: u16 = 40;
        let now = capture_time().as_nanos();
        let tapped = crate::capture_host::tapped_while(|| {
            callback_input_with(
                true,
                &[
                    // Escape cancels the capture: Hex consumes it.
                    (
                        now,
                        InputEvent::Key {
                            code: ESCAPE,
                            down: true,
                            flags: 0,
                        },
                    ),
                    // ⌘K goes to the app, so the capture logs it.
                    (
                        now + 1,
                        InputEvent::Key {
                            code: K,
                            down: true,
                            flags: COMMAND_KEY_MASK,
                        },
                    ),
                ],
            );
        });
        assert!(
            matches!(
                tapped.as_slice(),
                [crate::capture_host::Tapped::Key { code: K, .. }]
            ),
            "{tapped:?}"
        );
    }

    #[test]
    fn callback_timestamps_preserve_nanoseconds_and_short_tap_discard() {
        let start = 60_000_000_000;
        let edges = callback_input(&[
            (start, InputEvent::Flags(OPTION_KEY_MASK)),
            (start + 80_000_000, InputEvent::Flags(0)),
        ]);
        assert_eq!(edges.len(), 2);
        let mut capture = crate::dictation::DictationCapture::new(16_000);
        capture.start_at(edges[0].capture_at);
        assert!(matches!(
            capture.finish(edges[1].capture_at),
            crate::dictation::Finish::Discard
        ));
        assert_eq!(edges[0].capture_at.as_nanos(), start);
        assert_eq!(edges[1].capture_at.as_nanos(), start + 80_000_000);
    }

    #[test]
    fn callback_press_reaches_intentional_hold_on_the_audio_clock() {
        let now = capture_time();
        let press = callback_input(&[(now.as_nanos(), InputEvent::Flags(OPTION_KEY_MASK))])[0];
        // This capture has no recording environment, so threshold checks cannot mute audio.
        let mut capture = crate::dictation::DictationCapture::new(16_000);
        capture.start_at(press.capture_at);
        assert!(!capture.become_intentional(now + Duration::from_millis(299)));
        assert!(capture.become_intentional(now + Duration::from_millis(300)));
        assert!(!capture.become_intentional(now + Duration::from_millis(301)));
    }

    #[test]
    fn suspended_key_tracking_preserves_held_keys_and_observes_releases() {
        let now = capture_time();
        let ordinary = |down| InputEvent::Key {
            code: 0,
            down,
            flags: 0,
        };
        for pressed_before in [false, true] {
            for released_during in [false, true] {
                let mut hotkey = test_hotkey(false, now);
                if pressed_before {
                    assert_eq!(hotkey.process(ordinary(true), now), None);
                } else {
                    assert_eq!(hotkey.track_key_state(ordinary(true), now), Some(true));
                }
                hotkey.track_key_state(InputEvent::Flags(OPTION_KEY_MASK), now);
                hotkey.track_key_state(InputEvent::Flags(0), now);
                if released_during {
                    hotkey.track_key_state(ordinary(false), now);
                }
                assert!(!hotkey.is_recording());
                assert_eq!(
                    hotkey.process(
                        InputEvent::Flags(OPTION_KEY_MASK),
                        now + Duration::from_secs(1)
                    ),
                    released_during.then_some(HotkeyAction::Start)
                );
                if !released_during {
                    hotkey.process(InputEvent::Flags(0), now + Duration::from_secs(1));
                    hotkey.process(ordinary(false), now + Duration::from_secs(1));
                    assert_eq!(
                        hotkey.process(
                            InputEvent::Flags(OPTION_KEY_MASK),
                            now + Duration::from_secs(2)
                        ),
                        Some(HotkeyAction::Start)
                    );
                }
            }
        }
    }

    #[test]
    fn suspended_escape_and_double_taps_never_activate_a_gesture() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        for event in [
            InputEvent::Flags(OPTION_KEY_MASK),
            InputEvent::Flags(0),
            InputEvent::Flags(OPTION_KEY_MASK),
            InputEvent::Flags(0),
            InputEvent::Key {
                code: ESCAPE_KEY_CODE,
                down: true,
                flags: 0,
            },
        ] {
            hotkey.track_key_state(event, now);
            assert!(!hotkey.is_recording());
        }
        assert_eq!(
            hotkey.process(
                InputEvent::Key {
                    code: ESCAPE_KEY_CODE,
                    down: false,
                    flags: 0
                },
                now
            ),
            None
        );
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            hotkey.process(InputEvent::Flags(0), now + Duration::from_millis(80)),
            Some(HotkeyAction::Finish)
        );
    }

    #[test]
    fn callback_release_trims_delayed_audio_at_the_physical_boundary() {
        let start = 60_000_000_000;
        let edges = callback_input(&[
            (start, InputEvent::Flags(OPTION_KEY_MASK)),
            (start + 1_000_000_000, InputEvent::Flags(0)),
        ]);
        // Handle both edges late: the timeline already includes post-release audio.
        let mut capture = ingest_timeline(CaptureInstant::from_nanos(start));
        capture.start_at(edges[0].capture_at);
        let crate::dictation::Finish::Transcribe(clip) = capture.finish(edges[1].capture_at) else {
            panic!("one second hold must transcribe");
        };
        assert_eq!(clip.duration_ms(), 1_450);
        let samples = clip.into_transcription_samples();
        assert_eq!(&samples[..7_200], &[0.25; 7_200]);
        assert_eq!(&samples[7_200..], &[0.5; 16_000]);
    }

    #[test]
    fn callback_double_tap_locks_for_modifier_and_key_bindings() {
        let command = COMMAND_KEY_MASK | crate::app_settings::RIGHT_COMMAND_MASK;
        let right_command = RuntimeHotkey {
            modifiers: crate::app_settings::HotkeyModifiers {
                command: Some(crate::app_settings::ModifierSide::Right),
                ..Default::default()
            },
            key_code: None,
        };
        let key_chord = RuntimeHotkey {
            key_code: Some(49),
            ..right_command
        };
        for (binding, flags) in [
            (option_binding(), OPTION_KEY_MASK),
            (right_command, command),
            (function_binding(), FUNCTION_KEY_MASK),
            (key_chord, command),
        ] {
            let start = 60_000_000_000;
            let mut hotkey = DictationHotkey::with_binding(false, capture_time(), true, binding);
            let press = binding
                .key_code
                .map_or(InputEvent::Flags(flags), |code| InputEvent::Key {
                    code,
                    down: true,
                    flags,
                });
            let release = binding
                .key_code
                .map_or(InputEvent::Flags(0), |code| InputEvent::Key {
                    code,
                    down: false,
                    flags,
                });
            let edges = callback_input(&[
                (start, press),
                (start + 80_000_000, release),
                (start + 180_000_000, press),
                (start + 260_000_000, release),
                (start + 1_000_000_000, press),
                (start + 1_080_000_000, release),
            ]);
            let mut capture = ingest_timeline(CaptureInstant::from_nanos(start));
            let mut discarded = 0;
            let mut clips = Vec::new();
            let actions: Vec<_> = edges
                .into_iter()
                .map(|edge| {
                    let action = hotkey.process(edge.event, edge.capture_at);
                    match action {
                        Some(HotkeyAction::Start) => capture.start_at(edge.capture_at),
                        Some(HotkeyAction::Finish) => match capture.finish(edge.capture_at) {
                            crate::dictation::Finish::Discard => discarded += 1,
                            crate::dictation::Finish::Transcribe(clip) => clips.push(clip),
                        },
                        None => {}
                        _ => panic!("unexpected hotkey action"),
                    }
                    assert_eq!(hotkey.is_recording(), capture.is_recording());
                    action
                })
                .collect();
            assert_eq!(
                actions,
                vec![
                    Some(HotkeyAction::Start),
                    Some(HotkeyAction::Finish),
                    Some(HotkeyAction::Start),
                    None,
                    Some(HotkeyAction::Finish),
                    None,
                ]
            );
            assert!(!hotkey.is_recording());
            assert_eq!(discarded, 1);
            assert_eq!(clips.len(), 1);
            let clip = clips.pop().unwrap();
            assert_eq!(clip.duration_ms(), 1_270);
            let samples = clip.into_transcription_samples();
            assert_eq!(&samples[..4_320], &[0.25; 4_320]);
            assert_eq!(&samples[4_320..], &[0.5; 16_000]);
        }
    }

    #[test]
    fn function_keys_match_despite_the_fn_flag_macos_adds() {
        const F14: u16 = 107;
        let bare = RuntimeHotkey {
            modifiers: crate::app_settings::HotkeyModifiers::default(),
            key_code: Some(F14),
        };
        let command = RuntimeHotkey {
            modifiers: crate::app_settings::HotkeyModifiers {
                command: Some(crate::app_settings::ModifierSide::Either),
                ..Default::default()
            },
            key_code: Some(F14),
        };
        for (binding, held) in [(bare, 0), (command, COMMAND_KEY_MASK)] {
            let flags = held | FUNCTION_KEY_MASK;
            assert!(binding.matches_key_press(F14, flags));
            assert!(!binding.matches_key_press(F14, held | SHIFT_KEY_MASK | FUNCTION_KEY_MASK));
            let now = capture_time();
            let mut hotkey = DictationHotkey::with_binding(false, now, false, binding);
            if held != 0 {
                assert_eq!(hotkey.process(InputEvent::Flags(held), now), None);
            }
            let key = |down| InputEvent::Key {
                code: F14,
                down,
                flags,
            };
            assert_eq!(
                hotkey.process(key(true), now + Duration::from_millis(10)),
                Some(HotkeyAction::Start)
            );
            // Autorepeat carries the same flags and is not a chord.
            assert_eq!(
                hotkey.process(key(true), now + Duration::from_millis(100)),
                None
            );
            assert_eq!(
                hotkey.process(key(false), now + Duration::from_secs(1)),
                Some(HotkeyAction::Finish)
            );
        }
        // Fn stays significant for keys macOS does not mark with it.
        let space = RuntimeHotkey {
            key_code: Some(49),
            ..command
        };
        assert!(!space.matches_key_press(49, COMMAND_KEY_MASK | FUNCTION_KEY_MASK));
    }

    #[test]
    fn callback_hold_and_double_tap_boundaries_are_milliseconds() {
        for duration_ms in [299, 300, 301] {
            let start = 60_000_000_000;
            let press = InputEvent::Flags(OPTION_KEY_MASK);
            let release = InputEvent::Flags(0);
            let edges =
                callback_input(&[(start, press), (start + duration_ms * 1_000_000, release)]);
            let mut capture = crate::dictation::DictationCapture::new(16_000);
            capture.start_at(edges[0].capture_at);
            assert_eq!(
                matches!(
                    capture.finish(edges[1].capture_at),
                    crate::dictation::Finish::Discard
                ),
                duration_ms < 300
            );
            let edges = callback_input(&[
                (start, press),
                (start + 80_000_000, release),
                (start + 180_000_000, press),
                (start + (80 + duration_ms) * 1_000_000, release),
            ]);
            let mut hotkey = test_hotkey(false, capture_time());
            for edge in edges {
                hotkey.process(edge.event, edge.capture_at);
            }
            assert_eq!(hotkey.is_recording(), duration_ms < 300);
        }
    }

    fn mouse_hotkeys() -> RuntimeHotkeys {
        RuntimeHotkeys {
            dictation: HotkeyBinding {
                modifiers: Default::default(),
                key: crate::app_settings::HotkeyKey::mouse(3),
            }
            .runtime(),
            paste_last: None,
            paste_meeting: None,
            capture: None,
        }
    }

    #[test]
    fn mouse_hold_and_release_use_the_dictation_state_machine() {
        let now = capture_time();
        let bindings = mouse_hotkeys();
        let mut hotkey = DictationHotkey::new(now, false, bindings.dictation);
        assert_eq!(
            hotkey.process(mouse_input(3, true, 0, bindings).unwrap(), now),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            hotkey.process(
                mouse_input(3, false, 0, bindings).unwrap(),
                now + Duration::from_secs(1)
            ),
            Some(HotkeyAction::Finish)
        );
        assert!(matches!(
            mouse_input(0, true, 0, bindings),
            Some(InputEvent::MouseDown)
        ));
        assert!(mouse_input(0, false, 0, bindings).is_none());
    }

    #[test]
    fn mouse_toggle_and_escape_use_the_same_capture_controls() {
        let now = capture_time();
        let bindings = mouse_hotkeys();
        let mut hotkey = DictationHotkey::new(now, false, bindings.dictation);
        hotkey.set_toggle(true);
        assert_eq!(
            hotkey.process(mouse_input(3, true, 0, bindings).unwrap(), now),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            hotkey.process(
                mouse_input(3, false, 0, bindings).unwrap(),
                now + Duration::from_millis(100)
            ),
            None
        );
        assert!(hotkey.is_recording());
        assert_eq!(
            hotkey.process(
                InputEvent::Key {
                    code: ESCAPE_KEY_CODE,
                    down: true,
                    flags: 0
                },
                now + Duration::from_secs(1)
            ),
            Some(HotkeyAction::Cancel)
        );
    }

    #[test]
    fn mouse_held_during_stale_keyboard_recovery_is_not_forgotten() {
        let now = capture_time();
        let bindings = mouse_hotkeys();
        let mut hotkey = DictationHotkey::new(now, false, bindings.dictation);
        let code = bindings.dictation.key_code.unwrap();
        hotkey.state = State::Dirty;
        hotkey.pressed_keys.extend([0, code]);
        for millis in [0, 200] {
            hotkey.recover_stale_keys_with(
                || now + Duration::from_millis(millis),
                || 0,
                |key| key == code,
            );
        }
        assert!(hotkey.pressed_keys.contains(&code));
        assert!(matches!(hotkey.state, State::Dirty));
        recover_after_neutral_window(&mut hotkey, now + Duration::from_secs(1), 0);
        assert!(hotkey.pressed_keys.is_empty());
    }

    #[test]
    fn mouse_double_tap_locks_and_a_fresh_press_finishes() {
        let now = capture_time();
        let bindings = mouse_hotkeys();
        let mut hotkey = DictationHotkey::new(now, true, bindings.dictation);
        for (millis, down, expected) in [
            (0, true, Some(HotkeyAction::Start)),
            (50, false, Some(HotkeyAction::Finish)),
            (100, true, Some(HotkeyAction::Start)),
            (150, false, None),
            (1000, true, Some(HotkeyAction::Finish)),
        ] {
            assert_eq!(
                hotkey.process(
                    mouse_input(3, down, 0, bindings).unwrap(),
                    now + Duration::from_millis(millis)
                ),
                expected
            );
        }
    }

    #[test]
    fn mouse_release_stays_balanced_after_its_binding_is_removed() {
        let bindings = mouse_hotkeys();
        let mut suppression = ShortcutSuppression::default();
        assert!(suppression.process_all(
            mouse_input(3, true, 0, bindings).unwrap(),
            bindings,
            true,
            false
        ));
        let removed = RuntimeHotkeys::default();
        assert!(mouse_input(3, false, 0, removed).is_none());
        let release = suppression.tracked_mouse_release(3, 0).unwrap();
        assert!(suppression.process_all(release, removed, true, false));
        assert!(suppression.tracked_mouse_release(3, 0).is_none());
        assert!(!suppression.suppresses_mouse_drag(3));
    }

    #[test]
    fn mouse_suppression_balances_release_and_drag_even_after_modifiers_change() {
        let mut bindings = mouse_hotkeys();
        bindings.dictation.modifiers = crate::app_settings::HotkeyModifiers::option();
        let mut suppression = ShortcutSuppression::default();
        let activity = InputActivity::default();
        let down = mouse_input(3, true, OPTION_KEY_MASK, bindings).unwrap();
        let suppressed = suppression.process_all(down, bindings, true, false);
        assert!(suppressed);
        activity.observe(down, suppressed);
        assert_eq!(activity.revision(), 0);
        assert!(suppression.suppresses_mouse_drag(3));
        assert!(!suppression.suppresses_mouse_drag(0));
        assert!(suppression.process_all(
            mouse_input(3, false, 0, bindings).unwrap(),
            bindings,
            true,
            false
        ));
        assert!(!suppression.suppresses_mouse_drag(3));
        // Wrong modifiers and failed delivery must leave the ordinary click alone.
        for (flags, delivered) in [(0, true), (OPTION_KEY_MASK, false)] {
            assert!(!suppression.process_all(
                mouse_input(3, true, flags, bindings).unwrap(),
                bindings,
                delivered,
                false
            ));
            assert!(!suppression.suppresses_mouse_drag(3));
            assert!(!suppression.process_all(
                mouse_input(3, false, flags, bindings).unwrap(),
                bindings,
                true,
                false
            ));
        }
    }

    const NO_FLAGS: u64 = 0;

    fn capture_time() -> CaptureInstant {
        CaptureInstant::from_nanos(60_000_000_000)
    }

    // One second of 0.25 from `start`, one second of 0.5, then half a second of
    // 0.75 past the release at `start + 1s`, so clips expose their boundaries.
    fn ingest_timeline(start: CaptureInstant) -> crate::dictation::DictationCapture {
        let mut capture = crate::dictation::DictationCapture::new(16_000);
        capture.ingest(&vec![0.25; 16_000], start);
        capture.ingest(&vec![0.5; 16_000], start + Duration::from_secs(1));
        capture.ingest(&vec![0.75; 8_000], start + Duration::from_millis(1_500));
        capture
    }

    fn option_binding() -> RuntimeHotkey {
        RuntimeHotkey {
            modifiers: crate::app_settings::modifiers_from_flags(OPTION_KEY_MASK),
            key_code: None,
        }
    }

    fn function_binding() -> RuntimeHotkey {
        RuntimeHotkey {
            modifiers: crate::app_settings::modifiers_from_flags(FUNCTION_KEY_MASK),
            key_code: None,
        }
    }

    #[test]
    fn consumed_dictation_keys_preserve_continuation_but_typing_and_clicks_do_not() {
        for flags in [NO_FLAGS, SHIFT_KEY_MASK] {
            let activity = InputActivity::default();
            let mut suppression = ShortcutSuppression::default();
            let binding = RuntimeHotkey {
                modifiers: crate::app_settings::modifiers_from_flags(flags),
                key_code: Some(96),
            };
            let revision = activity.revision();
            for down in [true, true, false] {
                let input = InputEvent::Key {
                    code: 96,
                    down,
                    flags,
                };
                let suppressed = suppression.process(input, 47, binding, true);
                assert!(suppressed);
                activity.observe(input, suppressed);
            }
            assert_eq!(activity.revision(), revision);

            let typed = InputEvent::Key {
                code: 0,
                down: true,
                flags: NO_FLAGS,
            };
            activity.observe(typed, suppression.process(typed, 47, binding, true));
            assert_eq!(activity.revision(), revision + 1);
            activity.observe(InputEvent::MouseDown, false);
            assert_eq!(activity.revision(), revision + 2);

            let undelivered = InputEvent::Key {
                code: 96,
                down: true,
                flags,
            };
            activity.observe(
                undelivered,
                suppression.process(undelivered, 47, binding, false),
            );
            assert_eq!(activity.revision(), revision + 3);
        }
    }

    #[test]
    fn standalone_function_modifier_starts_and_finishes_dictation() {
        let now = capture_time();
        let mut hotkey = DictationHotkey::with_binding(false, now, true, function_binding());

        assert_eq!(
            hotkey.process(InputEvent::Flags(FUNCTION_KEY_MASK), now),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            hotkey.process(InputEvent::Flags(NO_FLAGS), now + Duration::from_secs(1)),
            Some(HotkeyAction::Finish)
        );
    }

    #[test]
    fn globe_key_events_are_not_a_chord_with_the_function_shortcut() {
        let now = capture_time();
        let globe = |down| InputEvent::Key {
            code: GLOBE_KEY_CODE,
            down,
            flags: FUNCTION_KEY_MASK,
        };
        let hid_globe = |at| InputEvent::PhysicalKeyDown {
            code: GLOBE_KEY_CODE,
            timing: HidTiming {
                raw: 0,
                clock: HidClock::Arrival,
                received_at: at,
            },
        };
        let mut hotkey = DictationHotkey::with_binding(false, now, false, function_binding());
        hotkey.set_toggle(true);
        let ms = |millis| now + Duration::from_millis(millis);
        for (at, event, expected) in [
            (ms(0), globe(true), None),
            (
                ms(0),
                InputEvent::Flags(FUNCTION_KEY_MASK),
                Some(HotkeyAction::Start),
            ),
            (ms(60), hid_globe(ms(60)), None),
            (ms(80), globe(false), None),
            (ms(90), InputEvent::Flags(NO_FLAGS), None),
            (ms(1_000), globe(true), None),
            (
                ms(1_000),
                InputEvent::Flags(FUNCTION_KEY_MASK),
                Some(HotkeyAction::Finish),
            ),
        ] {
            assert_eq!(hotkey.process(event, at), expected);
        }
        let hotkeys = RuntimeHotkeys {
            dictation: function_binding(),
            ..RuntimeHotkeys::default()
        };
        assert!(!is_chord_evidence(
            GLOBE_KEY_CODE,
            FUNCTION_KEY_MASK,
            hotkeys
        ));
        // Another key held with Fn is still a chord, and Globe is evidence for
        // a shortcut without Fn.
        assert!(is_chord_evidence(0, FUNCTION_KEY_MASK, hotkeys));
        let option = RuntimeHotkeys {
            dictation: option_binding(),
            ..RuntimeHotkeys::default()
        };
        assert!(is_chord_evidence(GLOBE_KEY_CODE, FUNCTION_KEY_MASK, option));
    }

    #[test]
    fn modifier_only_shortcuts_pass_through_after_delivery() {
        for (flags, binding) in [
            (OPTION_KEY_MASK, option_binding()),
            (FUNCTION_KEY_MASK, function_binding()),
        ] {
            let mut suppression = ShortcutSuppression::default();

            assert!(!suppression.process(InputEvent::Flags(flags), 47, binding, true));
            assert!(!suppression.process(InputEvent::Flags(NO_FLAGS), 47, binding, true));
        }
    }

    fn test_hotkey(trigger_down: bool, now: CaptureInstant) -> DictationHotkey {
        DictationHotkey::with_binding(trigger_down, now, true, option_binding())
    }

    fn key_binding(code: u16) -> RuntimeHotkey {
        RuntimeHotkey {
            modifiers: Default::default(),
            key_code: Some(code),
        }
    }

    fn shift_space_binding() -> RuntimeHotkey {
        RuntimeHotkey {
            modifiers: crate::app_settings::modifiers_from_flags(SHIFT_KEY_MASK),
            key_code: Some(49),
        }
    }

    fn shift_space_event(down: bool) -> InputEvent {
        InputEvent::Key {
            code: 49,
            down,
            flags: SHIFT_KEY_MASK,
        }
    }

    fn recover_after_neutral_window(
        hotkey: &mut DictationHotkey,
        start: CaptureInstant,
        flags: u64,
    ) -> CaptureInstant {
        let repaired = start + STALE_KEY_NEUTRAL_DURATION;
        hotkey.recover_stale_keys_with(|| start, || flags, |_| false);
        hotkey.recover_stale_keys_with(|| repaired, || flags, |_| false);
        repaired
    }

    #[test]
    fn stale_key_recovery_restores_a_fresh_hold_without_changing_its_boundaries() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        let down = InputEvent::Key {
            code: 0,
            down: true,
            flags: 0,
        };
        assert_eq!(hotkey.process(down, now), None);
        // The ordinary key's release never reached the reducer.
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now),
            None
        );
        let first = now + Duration::from_secs(1);
        hotkey.recover_stale_keys_with(|| first, || 0, |_| false);
        hotkey.recover_stale_keys_with(|| first + Duration::from_millis(99), || 0, |_| false);
        assert!(hotkey.pressed_keys.contains(&0));
        let repaired = first + STALE_KEY_NEUTRAL_DURATION;
        hotkey.recover_stale_keys_with(|| repaired, || 0, |_| false);
        assert!(hotkey.pressed_keys.is_empty());
        let press = repaired + Duration::from_millis(1);
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), press),
            Some(HotkeyAction::Start)
        );
        let mut capture = crate::dictation::DictationCapture::new(16_000);
        capture.start_at(press);
        capture.ingest(&vec![0.5; 16_000], press + Duration::from_secs(1));
        let release = press + Duration::from_millis(500);
        assert_eq!(
            hotkey.process(InputEvent::Flags(0), release),
            Some(HotkeyAction::Finish)
        );
        let crate::dictation::Finish::Transcribe(clip) = capture.finish(release) else {
            panic!("the fresh hold must transcribe");
        };
        assert_eq!(clip.duration_ms(), 500);
    }

    #[test]
    fn delayed_neutral_callback_after_recovery_preserves_the_next_hold() {
        for double_tap_enabled in [false, true] {
            for (neutral_event, offset_ms) in [
                (InputEvent::Flags(0), 0),
                (InputEvent::Flags(0), 100),
                (
                    InputEvent::Key {
                        code: 48,
                        down: false,
                        flags: 0,
                    },
                    0,
                ),
                (
                    InputEvent::Key {
                        code: 48,
                        down: false,
                        flags: 0,
                    },
                    100,
                ),
            ] {
                let now = capture_time();
                let mut hotkey = test_hotkey(false, now);
                hotkey.set_double_tap_enabled(double_tap_enabled);
                let down = callback_input(&[(
                    now.as_nanos(),
                    InputEvent::Key {
                        code: 48,
                        down: true,
                        flags: COMMAND_KEY_MASK,
                    },
                )])[0];
                hotkey.process(down.event, down.capture_at);

                // Two native neutral samples repair the missing key-up, but an old
                // neutral release has not reached its tap callback yet.
                let first_sample = now + Duration::from_secs(1);
                let repaired = recover_after_neutral_window(&mut hotkey, first_sample, 0);
                let neutral_at = first_sample + Duration::from_millis(offset_ms);
                let neutral = callback_input(&[(neutral_at.as_nanos(), neutral_event)])[0];
                assert_eq!(hotkey.process(neutral.event, neutral.capture_at), None);

                for attempt in 0..3 {
                    let press = repaired + Duration::from_secs(1 + attempt * 2);
                    let release = press + Duration::from_secs(1);
                    let edges = callback_input(&[
                        (press.as_nanos(), InputEvent::Flags(OPTION_KEY_MASK)),
                        (release.as_nanos(), InputEvent::Flags(0)),
                    ]);
                    let mut capture = ingest_timeline(press);
                    assert_eq!(
                        hotkey.process(edges[0].event, edges[0].capture_at),
                        Some(HotkeyAction::Start),
                        "attempt {attempt}, double_tap_enabled={double_tap_enabled}"
                    );
                    capture.start_at(edges[0].capture_at);
                    assert_eq!(
                        hotkey.process(edges[1].event, edges[1].capture_at),
                        Some(HotkeyAction::Finish)
                    );
                    let crate::dictation::Finish::Transcribe(clip) =
                        capture.finish(edges[1].capture_at)
                    else {
                        panic!("the hold must produce a clip");
                    };
                    assert_eq!(clip.duration_ms(), 1_450);
                    let samples = clip.into_transcription_samples();
                    assert_eq!(&samples[..7_200], &[0.25; 7_200]);
                    assert_eq!(&samples[7_200..], &[0.5; 16_000]);
                    assert!(!hotkey.is_recording());
                }
            }
        }
    }

    /// Command-Tab: the app switcher swallows Tab's key-up, leaving Tab tracked.
    fn command_tab(hotkey: &mut DictationHotkey, at: CaptureInstant) -> CaptureInstant {
        let tab = InputEvent::Key {
            code: 48,
            down: true,
            flags: COMMAND_KEY_MASK,
        };
        for (offset, event) in [
            (0, InputEvent::Flags(COMMAND_KEY_MASK)),
            (40, tab),
            (120, InputEvent::Flags(NO_FLAGS)),
        ] {
            assert_eq!(
                hotkey.process(event, at + Duration::from_millis(offset)),
                None
            );
        }
        assert!(hotkey.pressed_keys.contains(&48));
        at + Duration::from_millis(120)
    }

    #[test]
    fn option_soon_after_command_tab_records() {
        for double_tap_enabled in [false, true] {
            let now = capture_time();
            let mut hotkey = test_hotkey(false, now);
            hotkey.set_double_tap_enabled(double_tap_enabled);
            let released = command_tab(&mut hotkey, now);
            assert!(matches!(hotkey.state, State::Idle { .. }));
            // Well inside the 100 ms that `recover_stale_keys` needs.
            let press = released + Duration::from_millis(30);
            assert_eq!(
                hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), press),
                Some(HotkeyAction::Start)
            );
            assert!(hotkey.pressed_keys.is_empty());
            let mut capture = ingest_timeline(press);
            capture.start_at(press);
            // An older queued edge cannot finish the fresh press.
            assert_eq!(hotkey.process(InputEvent::Flags(NO_FLAGS), released), None);
            assert!(hotkey.is_recording());
            let release = press + Duration::from_secs(1);
            assert_eq!(
                hotkey.process(InputEvent::Flags(NO_FLAGS), release),
                Some(HotkeyAction::Finish)
            );
            let crate::dictation::Finish::Transcribe(clip) = capture.finish(release) else {
                panic!("the hold must transcribe");
            };
            assert_eq!(clip.duration_ms(), 1_450);
        }
    }

    #[test]
    fn shortcut_press_after_command_tab_from_dirty_records() {
        let now = capture_time();
        for binding in [option_binding(), key_binding(96)] {
            let mut hotkey = DictationHotkey::with_binding(false, now, true, binding);
            let (press, release) = match binding.key_code {
                None => (
                    InputEvent::Flags(OPTION_KEY_MASK),
                    InputEvent::Flags(NO_FLAGS),
                ),
                Some(code) => (
                    InputEvent::Key {
                        code,
                        down: true,
                        flags: NO_FLAGS,
                    },
                    InputEvent::Key {
                        code,
                        down: false,
                        flags: NO_FLAGS,
                    },
                ),
            };
            let (extra, lift) = match binding.key_code {
                None => (
                    OPTION_KEY_MASK | COMMAND_KEY_MASK,
                    InputEvent::Flags(COMMAND_KEY_MASK),
                ),
                Some(code) => (
                    COMMAND_KEY_MASK,
                    InputEvent::Key {
                        code,
                        down: false,
                        flags: COMMAND_KEY_MASK,
                    },
                ),
            };
            // Command-Tab while holding the shortcut discards it, then the
            // switcher swallows Tab's release.
            assert_eq!(hotkey.process(press, now), Some(HotkeyAction::Start));
            assert_eq!(
                hotkey.process(InputEvent::Flags(extra), now + Duration::from_millis(50)),
                Some(HotkeyAction::Discard(DiscardCause::ExtraModifier))
            );
            for (offset, event) in [
                (
                    100,
                    InputEvent::Key {
                        code: 48,
                        down: true,
                        flags: extra,
                    },
                ),
                (200, lift),
                (250, InputEvent::Flags(NO_FLAGS)),
            ] {
                assert_eq!(
                    hotkey.process(event, now + Duration::from_millis(offset)),
                    None
                );
            }
            assert!(matches!(hotkey.state, State::Dirty));
            let again = now + Duration::from_millis(280);
            assert_eq!(hotkey.process(press, again), Some(HotkeyAction::Start));
            assert_eq!(
                hotkey.process(release, again + Duration::from_secs(1)),
                Some(HotkeyAction::Finish)
            );
        }
    }

    #[test]
    fn stale_press_waits_for_recovery_while_anything_else_is_held() {
        let now = capture_time();
        let option = InputEvent::Flags(OPTION_KEY_MASK);
        for keyboard in [
            // Tab really is still down.
            PhysicalKeyboard {
                flags: || NO_FLAGS,
                key_down: |code| code == 48,
            },
            // Another modifier went down after the press was queued.
            PhysicalKeyboard {
                flags: || OPTION_KEY_MASK | SHIFT_KEY_MASK,
                key_down: |_| false,
            },
        ] {
            let mut hotkey = test_hotkey(false, now);
            hotkey.physical = keyboard;
            let released = command_tab(&mut hotkey, now);
            let press = released + Duration::from_millis(30);
            assert_eq!(hotkey.process(option, press), None);
            assert!(hotkey.pressed_keys.contains(&48));
            // The held press stays ignored; recovery rearms after release.
            let release = press + Duration::from_secs(1);
            assert_eq!(hotkey.process(InputEvent::Flags(NO_FLAGS), release), None);
            recover_after_neutral_window(&mut hotkey, release, 0);
            let next = release + Duration::from_secs(1);
            assert_eq!(hotkey.process(option, next), Some(HotkeyAction::Start));
        }
    }

    #[test]
    fn stale_press_requires_a_neutral_keyboard_after_the_tracked_keys() {
        let now = capture_time();
        // A key pressed alone and then Option is a chord even if the key
        // reads up by the time the press is processed.
        let mut hotkey = test_hotkey(false, now);
        let tab = InputEvent::Key {
            code: 48,
            down: true,
            flags: NO_FLAGS,
        };
        assert_eq!(hotkey.process(InputEvent::Flags(NO_FLAGS), now), None);
        assert_eq!(hotkey.process(tab, now + Duration::from_millis(10)), None);
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(OPTION_KEY_MASK),
                now + Duration::from_millis(40)
            ),
            None
        );

        // Shift went down and up again while Option-Command was assembled.
        let binding = RuntimeHotkey {
            modifiers: crate::app_settings::HotkeyModifiers::option_command(),
            key_code: None,
        };
        let mut hotkey = DictationHotkey::with_binding(false, now, true, binding);
        let released = command_tab(&mut hotkey, now);
        for (offset, flags) in [
            (10, SHIFT_KEY_MASK),
            (20, SHIFT_KEY_MASK | OPTION_KEY_MASK),
            (30, OPTION_KEY_MASK),
            (40, OPTION_KEY_MASK | COMMAND_KEY_MASK),
        ] {
            assert_eq!(
                hotkey.process(
                    InputEvent::Flags(flags),
                    released + Duration::from_millis(offset)
                ),
                None
            );
        }
        // Without Shift, the same chord starts.
        let mut hotkey = DictationHotkey::with_binding(false, now, true, binding);
        let released = command_tab(&mut hotkey, now);
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(OPTION_KEY_MASK),
                released + Duration::from_millis(10)
            ),
            None
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(OPTION_KEY_MASK | COMMAND_KEY_MASK),
                released + Duration::from_millis(20)
            ),
            Some(HotkeyAction::Start)
        );
    }

    #[test]
    fn stale_press_finishes_a_lock_and_leaves_escape_unchanged() {
        let now = capture_time();
        let option = InputEvent::Flags(OPTION_KEY_MASK);
        let neutral = InputEvent::Flags(NO_FLAGS);
        let escape = |down| InputEvent::Key {
            code: ESCAPE_KEY_CODE,
            down,
            flags: NO_FLAGS,
        };

        // Command-Tab while locked: the next press finishes the lock at once.
        let mut hotkey = test_hotkey(false, now);
        assert!(hotkey.lock());
        let released = command_tab(&mut hotkey, now);
        assert!(hotkey.is_locked());
        let press = released + Duration::from_millis(30);
        assert_eq!(hotkey.process(option, press), Some(HotkeyAction::Finish));
        assert!(!hotkey.is_recording());
        assert!(hotkey.pressed_keys.is_empty());
        // An older queued edge cannot restart it.
        assert_eq!(hotkey.process(option, released), None);
        assert_eq!(
            hotkey.process(neutral, press + Duration::from_millis(80)),
            None
        );
        assert_eq!(
            hotkey.process(option, press + Duration::from_secs(1)),
            Some(HotkeyAction::Start)
        );

        // Escape still cancels a lock that tracks a stale key.
        let mut hotkey = test_hotkey(false, now);
        assert!(hotkey.lock());
        let released = command_tab(&mut hotkey, now);
        assert_eq!(
            hotkey.process(escape(true), released + Duration::from_millis(50)),
            Some(HotkeyAction::Cancel)
        );

        // After a resynchronized press, double-tap lock and Escape behave as
        // they do for any fresh press.
        let mut hotkey = test_hotkey(false, now);
        let released = command_tab(&mut hotkey, now);
        let first = released + Duration::from_millis(30);
        let ms = |offset| first + Duration::from_millis(offset);
        assert_eq!(hotkey.process(option, first), Some(HotkeyAction::Start));
        assert_eq!(hotkey.process(neutral, ms(80)), Some(HotkeyAction::Finish));
        assert_eq!(hotkey.process(option, ms(180)), Some(HotkeyAction::Start));
        assert_eq!(hotkey.process(neutral, ms(260)), None);
        assert!(hotkey.is_locked());
        assert_eq!(
            hotkey.process(escape(true), ms(1_000)),
            Some(HotkeyAction::Cancel)
        );
        assert_eq!(hotkey.process(escape(false), ms(1_050)), None);
        assert_eq!(hotkey.process(option, ms(2_000)), Some(HotkeyAction::Start));
    }

    #[test]
    fn typing_after_command_tab_does_not_block_the_next_press() {
        let now = capture_time();
        let key = |code, down, flags| InputEvent::Key { code, down, flags };
        for binding in [option_binding(), key_binding(96)] {
            let mut hotkey = DictationHotkey::with_binding(false, now, true, binding);
            let released = command_tab(&mut hotkey, now);
            if binding.key_code.is_some() {
                // Tracked keys block a key binding only from Dirty, as a
                // Command-Tab during the press leaves it.
                hotkey.suppress_until_release();
            }
            // "hI" and Return in the new app.
            for (offset, event) in [
                (20, key(4, true, NO_FLAGS)),
                (40, key(4, false, NO_FLAGS)),
                (60, InputEvent::Flags(SHIFT_KEY_MASK)),
                (70, key(34, true, SHIFT_KEY_MASK)),
                (90, key(34, false, SHIFT_KEY_MASK)),
                (100, InputEvent::Flags(NO_FLAGS)),
                (150, key(36, true, NO_FLAGS)),
                (170, key(36, false, NO_FLAGS)),
            ] {
                assert_eq!(
                    hotkey.process(event, released + Duration::from_millis(offset)),
                    None
                );
            }
            assert!(hotkey.pressed_keys.contains(&48));
            let (press, release) = match binding.key_code {
                None => (
                    InputEvent::Flags(OPTION_KEY_MASK),
                    InputEvent::Flags(NO_FLAGS),
                ),
                Some(code) => (key(code, true, NO_FLAGS), key(code, false, NO_FLAGS)),
            };
            let pressed = released + Duration::from_millis(200);
            assert_eq!(hotkey.process(press, pressed), Some(HotkeyAction::Start));
            assert_eq!(
                hotkey.process(release, pressed + Duration::from_secs(1)),
                Some(HotkeyAction::Finish)
            );
        }
    }

    #[test]
    fn a_delayed_release_cannot_untrack_a_resynchronized_key_press() {
        let now = capture_time();
        let f5 = |down| InputEvent::Key {
            code: 96,
            down,
            flags: NO_FLAGS,
        };
        let mut hotkey = DictationHotkey::with_binding(false, now, true, key_binding(96));
        let released = command_tab(&mut hotkey, now);
        hotkey.suppress_until_release();
        let press = released + Duration::from_millis(30);
        assert_eq!(hotkey.process(f5(true), press), Some(HotkeyAction::Start));
        // An older release delivered late.
        assert_eq!(
            hotkey.process(
                f5(false),
                press.checked_sub(Duration::from_millis(10)).unwrap()
            ),
            None
        );
        assert!(hotkey.pressed_keys.contains(&96));
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(NO_FLAGS),
                press + Duration::from_millis(50)
            ),
            None
        );
        assert!(hotkey.is_recording());
        assert_eq!(
            hotkey.process(f5(false), press + Duration::from_secs(1)),
            Some(HotkeyAction::Finish)
        );
    }

    #[test]
    fn a_chord_at_the_resynchronized_press_still_discards_it() {
        let now = capture_time();
        for (chord, cause) in [
            (InputEvent::MouseDown, DiscardCause::Mouse),
            (hid_key(18), DiscardCause::HidKey),
            (
                InputEvent::Flags(OPTION_KEY_MASK | COMMAND_KEY_MASK),
                DiscardCause::ExtraModifier,
            ),
        ] {
            let mut hotkey = test_hotkey(false, now);
            let released = command_tab(&mut hotkey, now);
            let press = released + Duration::from_millis(30);
            assert_eq!(
                hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), press),
                Some(HotkeyAction::Start)
            );
            assert_eq!(
                hotkey.process(chord, press),
                Some(HotkeyAction::Discard(cause))
            );
        }
    }

    #[test]
    fn fenced_and_pre_reset_modifier_edges_are_not_neutral_evidence() {
        let option = InputEvent::Flags(OPTION_KEY_MASK);
        let neutral = InputEvent::Flags(NO_FLAGS);

        // A key and a neutral edge from before a recovery arrive after it.
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        command_tab(&mut hotkey, now);
        let repaired = recover_after_neutral_window(&mut hotkey, now + Duration::from_secs(1), 0);
        let before = |ms| repaired.checked_sub(Duration::from_millis(ms)).unwrap();
        let a = InputEvent::Key {
            code: 0,
            down: true,
            flags: NO_FLAGS,
        };
        assert_eq!(hotkey.process(a, before(50)), None);
        assert_eq!(hotkey.process(neutral, before(20)), None);
        let press = repaired + Duration::from_millis(30);
        assert_eq!(hotkey.process(option, press), None);
        let release = press + Duration::from_secs(1);
        assert_eq!(hotkey.process(neutral, release), None);
        recover_after_neutral_window(&mut hotkey, release, 0);
        assert_eq!(
            hotkey.process(option, release + Duration::from_secs(1)),
            Some(HotkeyAction::Start)
        );

        // A neutral edge queued before the tap was disabled.
        let now = CaptureInstant::now();
        let mut hotkey = test_hotkey(false, now);
        command_tab(
            &mut hotkey,
            now.checked_sub(Duration::from_secs(1)).unwrap(),
        );
        assert_eq!(hotkey.process(InputEvent::TapDisabled, now), None);
        assert_eq!(
            hotkey.process(
                neutral,
                now.checked_sub(Duration::from_millis(500)).unwrap()
            ),
            None
        );
        let press = CaptureInstant::now() + Duration::from_secs(1);
        assert_eq!(hotkey.process(option, press), None);
        assert!(hotkey.pressed_keys.contains(&48));

        // Suspending forgets a modifier it may never see released.
        let mut hotkey = test_hotkey(false, now);
        hotkey.track_key_state(option, now);
        hotkey.suspend();
        assert_eq!(hotkey.modifier_history.flags, NO_FLAGS);
        assert_eq!(hotkey.modifier_history.neutral_at, None);
    }

    #[test]
    fn neutral_keyboard_rearms_after_a_missing_modifier_release() {
        for double_tap_enabled in [false, true] {
            for finish_locked in [false, true] {
                if finish_locked && !double_tap_enabled {
                    continue;
                }
                let now = capture_time();
                let mut hotkey = test_hotkey(false, now);
                hotkey.set_double_tap_enabled(double_tap_enabled);
                let mut trace = vec![(0, OPTION_KEY_MASK, Some(HotkeyAction::Start))];
                if finish_locked {
                    trace.extend([
                        (80, 0, Some(HotkeyAction::Finish)),
                        (180, OPTION_KEY_MASK, Some(HotkeyAction::Start)),
                        (260, 0, None),
                        (1_000, OPTION_KEY_MASK, Some(HotkeyAction::Finish)),
                    ]);
                } else {
                    trace.extend([
                        (
                            50,
                            OPTION_KEY_MASK | SHIFT_KEY_MASK,
                            Some(HotkeyAction::Discard(DiscardCause::ExtraModifier)),
                        ),
                        (100, SHIFT_KEY_MASK, None),
                    ]);
                }
                let edges = callback_input(
                    &trace
                        .iter()
                        .map(|(ms, flags, _)| {
                            (
                                (now + Duration::from_millis(*ms)).as_nanos(),
                                InputEvent::Flags(*flags),
                            )
                        })
                        .collect::<Vec<_>>(),
                );
                for (edge, (_, _, expected)) in edges.into_iter().zip(trace) {
                    assert_eq!(hotkey.process(edge.event, edge.capture_at), expected);
                }
                assert!(matches!(hotkey.state, State::Dirty));
                assert!(hotkey.pressed_keys.is_empty());

                // The final modifier release never reached either callback. Native
                // neutrality must repair suppression without synthesizing a capture.
                let first = now + Duration::from_secs(2);
                recover_after_neutral_window(&mut hotkey, first, 0);
                assert!(!hotkey.is_recording());
                let press = now + Duration::from_secs(5);
                assert_eq!(
                    hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), press),
                    Some(HotkeyAction::Start),
                    "double_tap_enabled={double_tap_enabled}, finish_locked={finish_locked}"
                );
                assert_eq!(
                    hotkey.process(InputEvent::Flags(0), press + Duration::from_secs(1)),
                    Some(HotkeyAction::Finish)
                );
                assert!(!hotkey.is_recording());
            }
        }
    }

    #[test]
    fn arrow_function_metadata_does_not_block_the_next_option_hold() {
        // Observed after an arrow key-up with no physical keys held: numeric-pad,
        // secondary-Fn, and noncoalesced flags remain in native session state.
        let arrow_flags = 0x00a0_0100;
        for double_tap_enabled in [false, true] {
            let now = capture_time();
            let mut hotkey = test_hotkey(false, now);
            hotkey.set_double_tap_enabled(double_tap_enabled);
            let trace = [
                (0, InputEvent::Flags(OPTION_KEY_MASK)),
                (15, InputEvent::Flags(OPTION_KEY_MASK | SHIFT_KEY_MASK)),
                (
                    30,
                    InputEvent::Key {
                        code: 123,
                        down: true,
                        flags: arrow_flags | OPTION_KEY_MASK | SHIFT_KEY_MASK,
                    },
                ),
                (150, InputEvent::Flags(OPTION_KEY_MASK)),
                (155, InputEvent::Flags(0x100)),
                (
                    180,
                    InputEvent::Key {
                        code: 123,
                        down: false,
                        flags: arrow_flags,
                    },
                ),
            ];
            let edges = callback_input(
                &trace.map(|(ms, event)| ((now + Duration::from_millis(ms)).as_nanos(), event)),
            );
            let actions = edges
                .into_iter()
                .filter_map(|edge| hotkey.process(edge.event, edge.capture_at))
                .collect::<Vec<_>>();
            assert_eq!(
                actions,
                [
                    HotkeyAction::Start,
                    HotkeyAction::Discard(DiscardCause::ExtraModifier)
                ]
            );
            assert!(matches!(hotkey.state, State::Dirty));
            assert!(hotkey.pressed_keys.is_empty());

            let first = now + Duration::from_secs(1);
            recover_after_neutral_window(&mut hotkey, first, arrow_flags);
            assert!(!hotkey.is_recording());
            let press = now + Duration::from_secs(5);
            assert_eq!(
                hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), press),
                Some(HotkeyAction::Start),
                "double_tap_enabled={double_tap_enabled}"
            );
            assert_eq!(
                hotkey.process(InputEvent::Flags(0x100), press + Duration::from_secs(1)),
                Some(HotkeyAction::Finish)
            );
        }
    }

    #[test]
    fn function_recovery_preserves_unknown_and_flags_only_fn_holds() {
        let now = capture_time();
        for observed in [false, true] {
            let mut hotkey = test_hotkey(false, now);
            if observed {
                hotkey.track_key_state(InputEvent::Flags(FUNCTION_KEY_MASK), now);
                // An older neutral modifier event cannot erase the Fn press.
                hotkey.track_key_state(InputEvent::Flags(0), CaptureInstant::ZERO);
            }
            hotkey.suppress_until_release();
            for offset in [0, 100, 1_000] {
                hotkey.recover_stale_keys_with(
                    || now + Duration::from_millis(offset),
                    || FUNCTION_KEY_MASK,
                    |_| false,
                );
                assert!(matches!(hotkey.state, State::Dirty));
            }
            // A missed Fn release must not turn the observation itself into a latch.
            let neutral = now + Duration::from_secs(2);
            recover_after_neutral_window(&mut hotkey, neutral, 0);
            assert!(matches!(hotkey.state, State::Idle { .. }));
            hotkey.suppress_until_release();
            // A later explicit modifier release allows the navigation metadata
            // to be ignored without changing ordinary key-event Fn matching.
            let released = now + Duration::from_secs(3);
            hotkey.track_key_state(InputEvent::Flags(0), released);
            recover_after_neutral_window(&mut hotkey, released, FUNCTION_KEY_MASK);
            assert!(matches!(hotkey.state, State::Idle { .. }));
        }
    }

    #[test]
    fn blind_input_periods_invalidate_function_modifier_evidence() {
        for reset in 0..3 {
            let mut hotkey = test_hotkey(false, CaptureInstant::ZERO);
            hotkey.track_key_state(InputEvent::Flags(0), CaptureInstant::ZERO);
            match reset {
                0 => {
                    hotkey.suspend();
                }
                1 => {
                    hotkey.process(InputEvent::TapDisabled, CaptureInstant::ZERO);
                }
                _ => {
                    hotkey.track_key_state(InputEvent::TapDisabled, CaptureInstant::ZERO);
                }
            }
            let invalidated = hotkey.function_modifier.0;
            hotkey.track_key_state(
                InputEvent::Flags(0),
                invalidated.checked_sub(Duration::from_nanos(1)).unwrap(),
            );
            hotkey.suppress_until_release();
            recover_after_neutral_window(&mut hotkey, invalidated, FUNCTION_KEY_MASK);
            assert!(matches!(hotkey.state, State::Dirty));
        }
    }

    #[test]
    fn function_key_chord_still_uses_function_flags_on_key_events() {
        let now = capture_time();
        let binding = RuntimeHotkey {
            key_code: Some(49),
            ..function_binding()
        };
        let mut hotkey = DictationHotkey::with_binding(false, now, false, binding);
        assert_eq!(
            hotkey.process(InputEvent::Flags(FUNCTION_KEY_MASK), now),
            None
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Key {
                    code: 49,
                    down: true,
                    flags: FUNCTION_KEY_MASK
                },
                now
            ),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Key {
                    code: 49,
                    down: false,
                    flags: FUNCTION_KEY_MASK
                },
                now + Duration::from_secs(1)
            ),
            Some(HotkeyAction::Finish)
        );
    }

    #[test]
    fn blocked_modifier_recovery_requires_a_fully_neutral_keyboard() {
        let now = capture_time();
        for (flags, held_key) in [
            (OPTION_KEY_MASK, None),
            (0, Some(48)),
            (FUNCTION_KEY_MASK, Some(63)),
            (FUNCTION_KEY_MASK | (1 << 21), Some(63)),
        ] {
            let mut hotkey = test_hotkey(false, now);
            hotkey.track_key_state(InputEvent::Flags(0), now);
            hotkey.suppress_until_release();
            for offset in [0, 100, 1_000] {
                hotkey.recover_stale_keys_with(
                    || now + Duration::from_millis(offset),
                    || flags,
                    |code| held_key == Some(code),
                );
                assert!(matches!(hotkey.state, State::Dirty));
            }
            let neutral = now + Duration::from_secs(2);
            hotkey.recover_stale_keys_with(|| neutral, || 0, |_| false);
            hotkey.recover_stale_keys_with(|| neutral + Duration::from_millis(99), || 0, |_| false);
            assert!(matches!(hotkey.state, State::Dirty));
            hotkey.recover_stale_keys_with(
                || neutral + STALE_KEY_NEUTRAL_DURATION,
                || 0,
                |_| false,
            );
            assert!(matches!(hotkey.state, State::Idle { .. }));
        }
    }

    #[test]
    fn stale_key_recovery_does_not_reinterpret_delayed_chords() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        let key = |down| InputEvent::Key {
            code: 0,
            down,
            flags: OPTION_KEY_MASK,
        };
        hotkey.process(key(true), now);
        let first = now + Duration::from_secs(1);
        let repaired = recover_after_neutral_window(&mut hotkey, first, 0);
        for (offset, event) in [
            (10, key(true)),
            (20, InputEvent::Flags(OPTION_KEY_MASK)),
            (30, key(false)),
            (40, InputEvent::Flags(0)),
        ] {
            assert_eq!(
                hotkey.process(event, now + Duration::from_millis(offset)),
                None
            );
        }
        assert!(!hotkey.is_recording());
        assert!(hotkey.pressed_keys.is_empty());
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(OPTION_KEY_MASK),
                repaired + Duration::from_millis(1)
            ),
            None
        );
        assert_eq!(
            hotkey.process(InputEvent::Flags(0), repaired + Duration::from_millis(2)),
            None
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(OPTION_KEY_MASK),
                repaired + Duration::from_millis(3)
            ),
            Some(HotkeyAction::Start)
        );
    }

    #[test]
    fn stale_key_recovery_retains_edges_inside_the_sample_window() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        hotkey.process(
            InputEvent::Key {
                code: 0,
                down: true,
                flags: 0,
            },
            now,
        );
        let repaired = recover_after_neutral_window(&mut hotkey, now, 0);
        // This new key went down after its native query but before sampling ended.
        let key = |down| InputEvent::Key {
            code: 1,
            down,
            flags: 0,
        };
        assert_eq!(hotkey.process(key(true), repaired), None);
        assert!(hotkey.pressed_keys.contains(&1));
        let later = repaired + Duration::from_millis(1);
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), later),
            None
        );
        assert_eq!(hotkey.process(InputEvent::Flags(0), later), None);
        assert!(matches!(hotkey.state, State::Dirty));
        assert_eq!(hotkey.process(key(false), later), None);
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), later),
            Some(HotkeyAction::Start)
        );
    }

    #[test]
    fn stale_key_recovery_old_release_cannot_erase_a_fresh_press() {
        for binding in [option_binding(), key_binding(97)] {
            let now = capture_time();
            let mut hotkey = DictationHotkey::with_binding(false, now, true, binding);
            let key = |down| InputEvent::Key {
                code: 97,
                down,
                flags: 0,
            };
            hotkey.process(
                InputEvent::Key {
                    code: 0,
                    down: true,
                    flags: 0,
                },
                now,
            );
            let fence = recover_after_neutral_window(&mut hotkey, now, 0);
            let fresh = fence + Duration::from_millis(1);
            hotkey.process(key(true), fresh);
            assert_eq!(hotkey.process(key(false), now), None);
            assert!(hotkey.pressed_keys.contains(&97));
            assert_eq!(hotkey.process(InputEvent::Flags(0), fresh), None);
            if binding.key_code.is_none() {
                assert_eq!(
                    hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), fresh),
                    None
                );
            } else {
                assert!(hotkey.is_recording());
                assert_eq!(
                    hotkey.process(key(false), fresh + Duration::from_secs(1)),
                    Some(HotkeyAction::Finish)
                );
            }
        }
    }

    #[test]
    fn stale_key_recovery_protects_programmatic_key_transitions() {
        for down in [false, true] {
            let now = capture_time();
            let key = |down| InputEvent::Key {
                code: 0,
                down,
                flags: 0,
            };
            let mut hotkey = test_hotkey(false, now);
            hotkey.process(key(true), now);
            let fence = recover_after_neutral_window(&mut hotkey, now, 0);
            hotkey.track_key_state(key(down), fence + Duration::from_millis(1));
            assert_eq!(hotkey.track_key_state(key(!down), now), None);
            assert_eq!(hotkey.pressed_keys.contains(&0), down);
        }
    }

    #[test]
    fn programmatic_ownership_disarms_pending_double_taps_without_losing_held_keys() {
        let now = capture_time();
        let binding = key_binding(97);
        let mut hotkey = DictationHotkey::with_binding(false, now, true, binding);
        hotkey.set_double_tap_only(true);
        let key = |code, down| InputEvent::Key {
            code,
            down,
            flags: 0,
        };
        assert_eq!(hotkey.process(key(97, true), now), None);
        assert!(matches!(hotkey.state, State::FirstTapPressed));
        hotkey.disarm_pending_gesture();
        assert!(hotkey.pressed_keys.contains(&97));
        hotkey.track_key_state(key(97, false), now + Duration::from_millis(80));
        let later = now + Duration::from_secs(5);
        for (offset, event) in [
            (0, key(0, true)),
            (10, key(0, false)),
            (20, key(97, true)),
            (80, key(97, false)),
        ] {
            assert_eq!(
                hotkey.process(event, later + Duration::from_millis(offset)),
                None
            );
        }
        assert!(!hotkey.is_recording());
        assert_eq!(
            hotkey.process(key(97, true), later + Duration::from_millis(180)),
            None
        );
        assert_eq!(
            hotkey.process(key(97, false), later + Duration::from_millis(260)),
            Some(HotkeyAction::Start)
        );
        hotkey.disarm_pending_gesture();
        assert!(
            hotkey.is_recording(),
            "disarming pending gestures must not end a capture"
        );
    }

    #[test]
    fn programmatic_ownership_preserves_dirty_chord_suppression() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(OPTION_KEY_MASK | SHIFT_KEY_MASK),
                now + Duration::from_millis(50)
            ),
            Some(HotkeyAction::Discard(DiscardCause::ExtraModifier))
        );
        hotkey.disarm_pending_gesture();
        let later = now + Duration::from_secs(1);
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), later),
            None
        );
        assert_eq!(hotkey.process(InputEvent::Flags(0), later), None);
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), later),
            Some(HotkeyAction::Start)
        );
    }

    #[test]
    fn stale_key_recovery_requires_complete_neutrality_and_no_intervening_input() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        hotkey.process(
            InputEvent::Key {
                code: 0,
                down: true,
                flags: 0,
            },
            now,
        );
        hotkey.recover_stale_keys_with(|| now, || 0, |_| false);
        let later = now + Duration::from_secs(1);
        // An untracked held key must prevent repair as well.
        hotkey.recover_stale_keys_with(|| later, || 0, |code| code == 1);
        assert!(hotkey.stale_keys_neutral_since.is_none());
        assert!(hotkey.pressed_keys.contains(&0));
        hotkey.recover_stale_keys_with(|| later, || 0, |_| false);
        hotkey.process(InputEvent::MouseDown, later);
        hotkey.recover_stale_keys_with(|| later + STALE_KEY_NEUTRAL_DURATION, || 0, |_| false);
        assert!(hotkey.pressed_keys.contains(&0));
        let mut flags = [0, OPTION_KEY_MASK].into_iter();
        hotkey.recover_stale_keys_with(
            || later + Duration::from_secs(1),
            || flags.next().unwrap(),
            |_| false,
        );
        assert!(hotkey.stale_keys_neutral_since.is_none());
        assert!(hotkey.pressed_keys.contains(&0));
    }

    #[test]
    fn stale_key_recovery_never_polls_holds_or_pending_gestures() {
        let now = capture_time();
        for state in [
            State::Recording {
                started_at: now,
                previous_release: None,
            },
            State::FirstTapPressed,
            State::AwaitingSecondTap { released_at: now },
            State::SecondTapPressed {
                first_released_at: now,
            },
        ] {
            let mut hotkey = test_hotkey(false, now);
            hotkey.state = state;
            hotkey.pressed_keys.insert(0);
            let was_recording = hotkey.is_recording();
            hotkey.recover_stale_keys_with(
                || panic!("clock must not be polled"),
                || panic!("flags must not be polled"),
                |_| panic!("keys must not be polled"),
            );
            assert_eq!(hotkey.is_recording(), was_recording);
            assert!(hotkey.pressed_keys.contains(&0));
        }
        // A lock with accurate tracking has nothing to repair.
        let mut hotkey = test_hotkey(false, now);
        hotkey.state = State::Locked;
        hotkey.recover_stale_keys_with(
            || panic!("clock must not be polled"),
            || panic!("flags must not be polled"),
            |_| panic!("keys must not be polled"),
        );
        assert!(hotkey.is_recording());
    }

    #[test]
    fn programmatic_lock_finishes_on_the_next_press() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        assert!(hotkey.lock());
        assert!(hotkey.is_recording());
        let option = InputEvent::Flags(OPTION_KEY_MASK);
        let press = now + Duration::from_millis(500);
        assert_eq!(hotkey.process(option, press), Some(HotkeyAction::Finish));
        assert!(!hotkey.is_recording());

        // A held shortcut is not interrupted by a programmatic lock.
        let mut held = test_hotkey(false, now);
        assert_eq!(held.process(option, now), Some(HotkeyAction::Start));
        assert!(!held.lock());
    }

    #[test]
    fn locked_dictation_finishes_after_a_missing_key_release() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        let option = InputEvent::Flags(OPTION_KEY_MASK);
        let neutral = InputEvent::Flags(NO_FLAGS);
        assert_eq!(hotkey.process(option, now), Some(HotkeyAction::Start));
        let released = now + Duration::from_millis(80);
        assert_eq!(
            hotkey.process(neutral, released),
            Some(HotkeyAction::Finish)
        );
        let second = now + Duration::from_millis(180);
        assert_eq!(hotkey.process(option, second), Some(HotkeyAction::Start));
        let locked = now + Duration::from_millis(260);
        assert_eq!(hotkey.process(neutral, locked), None);
        assert!(hotkey.is_recording());

        // A key pressed while locked whose release never reaches the reducer.
        let down = InputEvent::Key {
            code: 0,
            down: true,
            flags: 0,
        };
        assert_eq!(hotkey.process(down, now + Duration::from_secs(1)), None);
        let blocked = now + Duration::from_secs(2);
        assert_eq!(hotkey.process(option, blocked), None);
        assert_eq!(
            hotkey.process(neutral, blocked + Duration::from_millis(80)),
            None
        );
        assert!(hotkey.is_recording());

        // Repair leaves the capture locked and emits no action of its own.
        let first_sample = now + Duration::from_secs(3);
        hotkey.recover_stale_keys_with(|| first_sample, || 0, |_| false);
        let held = first_sample + Duration::from_millis(50);
        hotkey.recover_stale_keys_with(|| held, || 0, |code| code == 0);
        assert!(hotkey.pressed_keys.contains(&0));
        let repaired = recover_after_neutral_window(&mut hotkey, held, 0);
        assert!(hotkey.pressed_keys.is_empty());
        assert!(hotkey.is_recording());

        let finish = repaired + Duration::from_millis(1);
        assert_eq!(hotkey.process(option, finish), Some(HotkeyAction::Finish));
        assert!(!hotkey.is_recording());
    }

    #[test]
    fn stale_key_recovery_does_not_scan_a_held_key_or_disturb_a_normal_double_tap() {
        let now = capture_time();
        let binding = key_binding(96);
        let mut hotkey = DictationHotkey::with_binding(false, now, true, binding);
        hotkey.process(
            InputEvent::Key {
                code: 0,
                down: true,
                flags: 0,
            },
            now,
        );
        let mut queried = Vec::new();
        hotkey.recover_stale_keys_with(
            || now,
            || 0,
            |code| {
                queried.push(code);
                true
            },
        );
        assert_eq!(queried, vec![0]);
        hotkey.process(
            InputEvent::Key {
                code: 0,
                down: false,
                flags: 0,
            },
            now,
        );
        let key = |down| InputEvent::Key {
            code: 96,
            down,
            flags: 0,
        };
        assert_eq!(hotkey.process(key(true), now), Some(HotkeyAction::Start));
        assert_eq!(
            hotkey.process(key(false), now + Duration::from_millis(50)),
            Some(HotkeyAction::Finish)
        );
        hotkey.recover_stale_keys_with(
            || panic!("no stale key"),
            || panic!("no stale key"),
            |_| panic!("no stale key"),
        );
        assert_eq!(
            hotkey.process(key(true), now + Duration::from_millis(100)),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            hotkey.process(key(false), now + Duration::from_millis(150)),
            None
        );
        assert!(hotkey.is_recording());
    }

    #[test]
    fn option_press_and_release_finishes() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            hotkey.process(InputEvent::Flags(NO_FLAGS), now + Duration::from_secs(1)),
            Some(HotkeyAction::Finish)
        );
    }

    #[test]
    fn key_chord_starts_on_key_down_and_finishes_on_key_up() {
        let now = capture_time();
        let binding = shift_space_binding();
        let mut hotkey = DictationHotkey::with_binding(false, now, true, binding);
        let key_down = shift_space_event(true);

        assert_eq!(hotkey.process(key_down, now), Some(HotkeyAction::Start));
        assert_eq!(
            hotkey.process(shift_space_event(false), now + Duration::from_secs(1)),
            Some(HotkeyAction::Finish)
        );

        let mut suppression = ShortcutSuppression::default();
        assert!(suppression.process(key_down, 42, binding, true));
        assert!(suppression.process(shift_space_event(false), 42, binding, true));
    }

    #[test]
    fn key_chord_double_tap_locks_until_the_chord_is_pressed_again() {
        let now = capture_time();
        let binding = shift_space_binding();
        let mut hotkey = DictationHotkey::with_binding(false, now, true, binding);
        let event = shift_space_event;

        assert_eq!(hotkey.process(event(true), now), Some(HotkeyAction::Start));
        assert_eq!(
            hotkey.process(event(false), now + Duration::from_millis(70)),
            Some(HotkeyAction::Finish)
        );
        assert_eq!(
            hotkey.process(event(true), now + Duration::from_millis(150)),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            hotkey.process(event(false), now + Duration::from_millis(230)),
            None
        );
        assert!(hotkey.is_recording());
        assert_eq!(
            hotkey.process(event(true), now + Duration::from_secs(1)),
            Some(HotkeyAction::Finish)
        );
    }

    #[test]
    fn toggle_mode_starts_on_a_press_and_finishes_on_the_next() {
        let command = COMMAND_KEY_MASK | crate::app_settings::RIGHT_COMMAND_MASK;
        let key_chord = RuntimeHotkey {
            modifiers: crate::app_settings::HotkeyModifiers {
                command: Some(crate::app_settings::ModifierSide::Right),
                ..Default::default()
            },
            key_code: Some(49),
        };
        for (binding, flags, double_tap_only) in [
            (option_binding(), OPTION_KEY_MASK, false),
            (function_binding(), FUNCTION_KEY_MASK, false),
            (key_chord, command, false),
            (key_chord, command, true),
        ] {
            let now = capture_time();
            let mut hotkey = DictationHotkey::with_binding(false, now, true, binding);
            hotkey.set_double_tap_only(double_tap_only);
            hotkey.set_toggle(true);
            let event = |down: bool| {
                binding
                    .key_code
                    .map_or(InputEvent::Flags(if down { flags } else { 0 }), |code| {
                        InputEvent::Key { code, down, flags }
                    })
            };

            assert_eq!(hotkey.process(event(true), now), Some(HotkeyAction::Start));
            // Releasing keeps recording, however short the press was.
            assert_eq!(
                hotkey.process(event(false), now + Duration::from_millis(80)),
                None
            );
            assert!(hotkey.is_recording());
            assert_eq!(
                hotkey.process(event(true), now + Duration::from_secs(3)),
                Some(HotkeyAction::Finish)
            );
            assert_eq!(
                hotkey.process(event(false), now + Duration::from_millis(3_080)),
                None
            );
            assert!(!hotkey.is_recording());
        }
    }

    #[test]
    fn toggle_mode_finishes_after_command_tab_while_recording() {
        let now = capture_time();
        let option = InputEvent::Flags(OPTION_KEY_MASK);
        let neutral = InputEvent::Flags(NO_FLAGS);
        for physical_still_down in [false, true] {
            let mut hotkey = test_hotkey(false, now);
            hotkey.set_toggle(true);
            assert_eq!(hotkey.process(option, now), Some(HotkeyAction::Start));
            assert_eq!(
                hotkey.process(neutral, now + Duration::from_millis(80)),
                None
            );
            assert!(hotkey.is_locked());
            if physical_still_down {
                // Tab really is still down: the press waits, as before.
                hotkey.physical = PhysicalKeyboard {
                    flags: || NO_FLAGS,
                    key_down: |code| code == 48,
                };
            }
            let released = command_tab(&mut hotkey, now + Duration::from_secs(1));
            assert!(hotkey.is_locked());
            let press = released + Duration::from_secs(1);
            let expected = (!physical_still_down).then_some(HotkeyAction::Finish);
            assert_eq!(hotkey.process(option, press), expected);
            assert_eq!(hotkey.is_recording(), physical_still_down);
            assert_eq!(
                hotkey.process(neutral, press + Duration::from_millis(80)),
                None
            );
            assert_eq!(hotkey.is_recording(), physical_still_down);
        }
    }

    #[test]
    fn toggle_mode_escape_cancels_a_locked_capture() {
        let now = capture_time();
        let binding = shift_space_binding();
        let mut hotkey = DictationHotkey::with_binding(false, now, true, binding);
        hotkey.set_toggle(true);
        let event = shift_space_event;

        assert_eq!(hotkey.process(event(true), now), Some(HotkeyAction::Start));
        assert_eq!(
            hotkey.process(event(false), now + Duration::from_millis(60)),
            None
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Key {
                    code: ESCAPE_KEY_CODE,
                    down: true,
                    flags: 0,
                },
                now + Duration::from_secs(1)
            ),
            Some(HotkeyAction::Cancel)
        );
        assert!(!hotkey.is_recording());
    }

    #[test]
    fn double_tap_only_waits_for_two_complete_key_chord_taps() {
        let now = capture_time();
        let binding = shift_space_binding();
        let mut hotkey = DictationHotkey::with_binding(false, now, true, binding);
        hotkey.set_double_tap_only(true);
        let event = shift_space_event;

        assert_eq!(hotkey.process(event(true), now), None);
        assert!(!hotkey.is_recording());
        assert_eq!(
            hotkey.process(event(false), now + Duration::from_millis(50)),
            None
        );
        assert_eq!(
            hotkey.process(event(true), now + Duration::from_millis(150)),
            None
        );
        assert_eq!(
            hotkey.process(event(false), now + Duration::from_millis(200)),
            Some(HotkeyAction::Start)
        );
        assert!(hotkey.is_recording());
        assert_eq!(
            hotkey.process(event(true), now + Duration::from_secs(1)),
            Some(HotkeyAction::Finish)
        );
    }

    #[test]
    fn double_tap_only_restarts_with_the_first_press_after_timeout() {
        let now = capture_time();
        let binding = shift_space_binding();
        for delay in [DOUBLE_TAP_WINDOW, Duration::from_secs(1)] {
            let mut hotkey = DictationHotkey::with_binding(false, now, true, binding);
            hotkey.set_double_tap_only(true);
            let event = shift_space_event;

            assert_eq!(hotkey.process(event(true), now), None);
            let released_at = now + Duration::from_millis(50);
            assert_eq!(hotkey.process(event(false), released_at), None);

            let first_press = released_at + delay;
            assert_eq!(hotkey.process(event(true), first_press), None);
            assert_eq!(
                hotkey.process(event(false), first_press + Duration::from_millis(50)),
                None
            );
            assert!(!hotkey.is_recording());
            assert_eq!(
                hotkey.process(event(true), first_press + Duration::from_millis(150)),
                None
            );
            assert_eq!(
                hotkey.process(event(false), first_press + Duration::from_millis(200)),
                Some(HotkeyAction::Start)
            );
            assert!(hotkey.is_recording());
        }
    }

    #[test]
    fn double_tap_only_timeout_and_unrelated_chord_never_start_capture() {
        let now = capture_time();
        let binding = shift_space_binding();
        let mut hotkey = DictationHotkey::with_binding(false, now, true, binding);
        hotkey.set_double_tap_only(true);
        let trigger = shift_space_event;

        hotkey.process(trigger(true), now);
        hotkey.process(trigger(false), now + Duration::from_millis(50));
        assert_eq!(
            hotkey.process(trigger(true), now + Duration::from_millis(400)),
            None
        );
        assert!(!hotkey.is_recording());

        hotkey.process(trigger(true), now + Duration::from_millis(500));
        hotkey.process(trigger(false), now + Duration::from_millis(550));
        assert_eq!(
            hotkey.process(
                InputEvent::Key {
                    code: 11,
                    down: true,
                    flags: 0,
                },
                now + Duration::from_millis(600),
            ),
            None
        );
        assert!(!hotkey.is_recording());

        let mut hotkey = DictationHotkey::with_binding(false, now, true, binding);
        hotkey.set_double_tap_only(true);
        hotkey.process(trigger(true), now);
        hotkey.process(trigger(false), now + Duration::from_millis(50));
        hotkey.process(trigger(true), now + Duration::from_millis(150));
        assert_eq!(
            hotkey.process(trigger(false), now + Duration::from_millis(400)),
            None
        );
        assert!(!hotkey.is_recording());
    }

    #[test]
    fn modifier_chord_requires_the_exact_binding_to_start() {
        let now = capture_time();
        let binding = RuntimeHotkey {
            modifiers: crate::app_settings::modifiers_from_flags(CONTROL_KEY_MASK | SHIFT_KEY_MASK),
            key_code: None,
        };
        let mut hotkey = DictationHotkey::with_binding(false, now, true, binding);

        assert_eq!(
            hotkey.process(InputEvent::Flags(CONTROL_KEY_MASK), now),
            None
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(CONTROL_KEY_MASK | SHIFT_KEY_MASK),
                now + Duration::from_millis(50),
            ),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(SHIFT_KEY_MASK),
                now + Duration::from_secs(1),
            ),
            Some(HotkeyAction::Finish)
        );
    }

    #[test]
    fn remapped_control_does_not_start_or_hold_right_control_dictation() {
        use crate::app_settings::{HotkeyModifiers, ModifierSide, RIGHT_CONTROL_MASK};

        let now = capture_time();
        let binding = RuntimeHotkey {
            modifiers: HotkeyModifiers {
                control: Some(ModifierSide::Right),
                ..Default::default()
            },
            key_code: None,
        };
        let mut hotkey = DictationHotkey::with_binding(false, now, false, binding);
        // Remapped Caps Lock may carry the general Control flag without a side.
        assert_eq!(
            hotkey.process(InputEvent::Flags(CONTROL_KEY_MASK), now),
            None
        );
        assert!(!hotkey.is_recording());
        assert_eq!(hotkey.process(InputEvent::Flags(0), now), None);
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(CONTROL_KEY_MASK | RIGHT_CONTROL_MASK),
                now
            ),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(CONTROL_KEY_MASK),
                now + Duration::from_secs(1)
            ),
            Some(HotkeyAction::Finish)
        );

        let mut either = DictationHotkey::with_binding(
            false,
            now,
            false,
            RuntimeHotkey {
                modifiers: HotkeyModifiers {
                    control: Some(ModifierSide::Either),
                    ..Default::default()
                },
                key_code: None,
            },
        );
        assert_eq!(
            either.process(InputEvent::Flags(CONTROL_KEY_MASK), now),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            either.process(InputEvent::Flags(0), now + Duration::from_secs(1)),
            Some(HotkeyAction::Finish)
        );
    }

    #[test]
    fn tap_failure_cancels_recording_after_live_opt_in() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        hotkey.wait_for_release_with_state(now, 0, false);
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            hotkey.process(InputEvent::TapDisabled, CaptureInstant::ZERO),
            Some(HotkeyAction::Cancel)
        );
        assert!(!hotkey.is_recording());
    }

    #[test]
    fn enabling_a_held_key_binding_waits_for_a_fresh_press() {
        let now = capture_time();
        let binding = RuntimeHotkey {
            modifiers: Default::default(),
            key_code: Some(40),
        };
        let mut hotkey = DictationHotkey::with_binding(false, now, false, binding);
        let event = |down| InputEvent::Key {
            code: 40,
            down,
            flags: 0,
        };
        hotkey.wait_for_release_with_state(now, 0, true);
        assert_eq!(hotkey.process(event(true), now), None);
        assert_eq!(hotkey.process(InputEvent::Flags(0), now), None);
        assert_eq!(hotkey.process(event(false), now), None);
        assert_eq!(hotkey.process(event(true), now), Some(HotkeyAction::Start));
    }

    #[test]
    fn suppressing_until_release_does_not_activate_a_remaining_option_modifier() {
        let now = capture_time();
        let chord = OPTION_KEY_MASK | COMMAND_KEY_MASK;
        let mut hotkey = test_hotkey(false, now);
        hotkey.suppress_until_release();
        assert_eq!(hotkey.process(InputEvent::Flags(chord), now), None);
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now),
            None
        );
        assert_eq!(hotkey.process(InputEvent::Flags(0), now), None);
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now),
            Some(HotkeyAction::Start)
        );
    }

    #[test]
    fn waiting_for_release_does_not_block_explicit_paste() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        hotkey.suppress_until_release();
        let binding = HotkeyBinding::paste_last_default();
        assert_eq!(
            hotkey.process(
                InputEvent::Key {
                    code: binding.key.unwrap().code,
                    down: true,
                    flags: OPTION_KEY_MASK | SHIFT_KEY_MASK,
                },
                now
            ),
            Some(HotkeyAction::PasteLast)
        );
    }

    #[test]
    fn enabling_with_no_keys_held_accepts_the_next_gesture() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        hotkey.wait_for_release_with_state(now, 0, false);
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now),
            Some(HotkeyAction::Start)
        );
    }

    #[test]
    fn opt_in_transitions_ignore_queued_old_shortcut_edges() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        let changed_at = now + Duration::from_secs(1);
        hotkey.wait_for_release_with_state(changed_at, 0, false);
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now),
            None
        );
        assert_eq!(
            hotkey.process(InputEvent::Flags(0), now + Duration::from_millis(500)),
            None
        );
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), changed_at),
            Some(HotkeyAction::Start)
        );
    }

    #[test]
    fn a_held_modifier_only_shortcut_breaks_the_chord_once_after_the_hold_threshold() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        for press in [now, now + Duration::from_secs(5)] {
            assert_eq!(
                hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), press),
                Some(HotkeyAction::Start)
            );
            assert!(!hotkey.take_chord_break(press + Duration::from_millis(299)));
            assert!(hotkey.take_chord_break(press + MINIMUM_HOLD_DURATION));
            assert!(!hotkey.take_chord_break(press + Duration::from_secs(2)));
            assert_eq!(
                hotkey.process(InputEvent::Flags(NO_FLAGS), press + Duration::from_secs(3)),
                Some(HotkeyAction::Finish)
            );
            assert!(!hotkey.take_chord_break(press + Duration::from_secs(4)));
        }
    }

    #[test]
    fn short_taps_chords_and_other_bindings_never_break_the_chord() {
        let now = capture_time();
        let later = now + Duration::from_secs(1);

        let mut tap = test_hotkey(false, now);
        tap.process(InputEvent::Flags(OPTION_KEY_MASK), now);
        tap.process(
            InputEvent::Flags(NO_FLAGS),
            now + Duration::from_millis(200),
        );
        assert!(!tap.take_chord_break(later));

        for (chord, cause) in [
            (option_key(0, true, OPTION_KEY_MASK), DiscardCause::Key),
            (hid_key(18), DiscardCause::HidKey),
            (InputEvent::MouseDown, DiscardCause::Mouse),
            (
                InputEvent::Flags(OPTION_KEY_MASK | SHIFT_KEY_MASK),
                DiscardCause::ExtraModifier,
            ),
        ] {
            let mut hotkey = test_hotkey(false, now);
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now);
            assert_eq!(
                hotkey.process(chord, now + Duration::from_millis(100)),
                Some(HotkeyAction::Discard(cause))
            );
            assert!(!hotkey.take_chord_break(later), "{cause:?}");
        }

        // A key held past the threshold is already a chord to other apps.
        let mut held_key = test_hotkey(false, now);
        held_key.process(InputEvent::Flags(OPTION_KEY_MASK), now);
        held_key.process(
            option_key(0, true, OPTION_KEY_MASK),
            now + Duration::from_millis(400),
        );
        assert!(!held_key.take_chord_break(later));

        // Keyed shortcuts are consumed, and Globe's own tap action is the system's.
        let mut keyed = DictationHotkey::with_binding(false, now, true, shift_space_binding());
        assert_eq!(
            keyed.process(shift_space_event(true), now),
            Some(HotkeyAction::Start)
        );
        let mut globe = DictationHotkey::with_binding(false, now, true, function_binding());
        assert_eq!(
            globe.process(InputEvent::Flags(FUNCTION_KEY_MASK), now),
            Some(HotkeyAction::Start)
        );
        assert!(!keyed.take_chord_break(later));
        assert!(!globe.take_chord_break(later));

        // A suspended press (such as a refused start) owes nothing.
        let mut suspended = test_hotkey(false, now);
        suspended.process(InputEvent::Flags(OPTION_KEY_MASK), now);
        suspended.suspend();
        assert!(!suspended.take_chord_break(later));
    }

    #[test]
    fn locked_and_toggle_captures_break_the_chord_only_for_presses_hex_owns() {
        let now = capture_time();
        let at = |ms| now + Duration::from_millis(ms);

        // Double-tap lock: both presses are taps, which stay taps.
        let mut locked = test_hotkey(false, now);
        locked.process(InputEvent::Flags(OPTION_KEY_MASK), now);
        locked.process(InputEvent::Flags(NO_FLAGS), at(100));
        locked.process(InputEvent::Flags(OPTION_KEY_MASK), at(200));
        locked.process(InputEvent::Flags(NO_FLAGS), at(300));
        assert!(locked.is_locked());
        assert!(!locked.take_chord_break(at(2_000)));
        // The press that finishes the lock is Hex's.
        assert_eq!(
            locked.process(InputEvent::Flags(OPTION_KEY_MASK), at(3_000)),
            Some(HotkeyAction::Finish)
        );
        assert!(locked.take_chord_break(at(3_000)));
        assert!(!locked.take_chord_break(at(3_050)));
        // Released before the loop got to it: nothing to break.
        let mut programmatic = test_hotkey(false, now);
        assert!(programmatic.lock());
        programmatic.process(InputEvent::Flags(OPTION_KEY_MASK), at(1_000));
        programmatic.process(InputEvent::Flags(NO_FLAGS), at(1_060));
        assert!(!programmatic.take_chord_break(at(1_060)));

        // Toggle: the starting press is Hex's once past the chord grace.
        let grace = crate::feedback::CHORD_GRACE;
        let mut toggle = test_hotkey(false, now);
        toggle.set_toggle(true);
        toggle.process(InputEvent::Flags(OPTION_KEY_MASK), now);
        assert!(!toggle.take_chord_break(now));
        assert!(!toggle.take_chord_break(now + (grace - Duration::from_millis(1))));
        assert!(toggle.take_chord_break(now + grace));
        toggle.process(InputEvent::Flags(NO_FLAGS), at(80));
        assert!(toggle.is_locked());
        assert!(!toggle.take_chord_break(at(1_000)));
        toggle.process(InputEvent::Flags(OPTION_KEY_MASK), at(2_000));
        assert!(toggle.take_chord_break(at(2_000)));
        // Shift pressed with it, as for Option-Shift-S, discards the press first.
        let mut chord = test_hotkey(false, now);
        chord.set_toggle(true);
        chord.process(InputEvent::Flags(OPTION_KEY_MASK), now);
        assert_eq!(
            chord.process(InputEvent::Flags(OPTION_KEY_MASK | SHIFT_KEY_MASK), at(20)),
            Some(HotkeyAction::Discard(DiscardCause::ExtraModifier))
        );
        assert!(!chord.take_chord_break(at(1_000)));
    }

    /// The chord breaker is posted through both taps; its mark keeps it from
    /// becoming the user's chord or HID chord evidence, and the key tap drops
    /// it so the focused app never beeps at it. Hex's other synthetic keys,
    /// such as the paste shortcut, still reach the app.
    #[test]
    fn marked_chord_breaker_never_reaches_the_shortcut_machines_or_the_app() {
        let breaker = crate::keyboard::CHORD_BREAKER_KEY_CODE;
        for (code, marked) in [(breaker, true), (breaker, false), (9, true)] {
            let (mut context, receiver) = test_tap_context(false);
            // SAFETY: This locally owned event and context live through both callbacks.
            unsafe {
                let user_info = (&mut context as *mut EventTapContext).cast();
                for event_type in [EVENT_KEY_DOWN, EVENT_KEY_UP] {
                    let event = CGEventCreate(ptr::null_mut());
                    assert!(!event.is_null());
                    CGEventSetType(event, event_type);
                    CGEventSetFlags(event, OPTION_KEY_MASK);
                    CGEventSetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE, i64::from(code));
                    if marked {
                        CGEventSetIntegerValueField(
                            event,
                            EVENT_SOURCE_USER_DATA,
                            crate::keyboard::SYNTHETIC_EVENT_MARKER,
                        );
                    }
                    let returned = event_callback(ptr::null_mut(), event_type, event, user_info);
                    if marked && code == breaker {
                        assert!(returned.is_null());
                    } else {
                        assert_eq!(returned, event);
                    }
                    if event_type == EVENT_KEY_DOWN {
                        let returned =
                            physical_event_callback(ptr::null_mut(), event_type, event, user_info);
                        assert_eq!(returned, event);
                    }
                    CFRelease(event.cast_const());
                }
            }
            let observed = receiver.try_iter().count();
            if marked {
                assert_eq!(observed, 0);
            } else {
                assert_eq!(observed, 3, "an unmarked key is the user's chord");
            }
        }
    }

    #[test]
    fn option_double_tap_locks_until_option_is_pressed_again() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);

        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            hotkey.process(InputEvent::Flags(NO_FLAGS), now + Duration::from_millis(80)),
            Some(HotkeyAction::Finish)
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(OPTION_KEY_MASK),
                now + Duration::from_millis(180)
            ),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(NO_FLAGS),
                now + Duration::from_millis(260)
            ),
            None
        );
        assert!(hotkey.is_recording());
        assert_eq!(
            hotkey.process(InputEvent::MouseDown, now + Duration::from_secs(1)),
            None
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(OPTION_KEY_MASK),
                now + Duration::from_secs(2)
            ),
            Some(HotkeyAction::Finish)
        );
        assert!(!hotkey.is_recording());
    }

    #[test]
    fn second_tap_finishes_as_press_and_hold_when_double_tap_cannot_lock() {
        let now = capture_time();
        // Double-tap lock disabled, or enabled with the second release outside its window.
        for (double_tap_enabled, second_release_ms) in [(false, 260), (true, 450)] {
            let mut hotkey =
                DictationHotkey::with_binding(false, now, double_tap_enabled, option_binding());

            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now);
            hotkey.process(InputEvent::Flags(NO_FLAGS), now + Duration::from_millis(80));
            hotkey.process(
                InputEvent::Flags(OPTION_KEY_MASK),
                now + Duration::from_millis(180),
            );
            assert_eq!(
                hotkey.process(
                    InputEvent::Flags(NO_FLAGS),
                    now + Duration::from_millis(second_release_ms)
                ),
                Some(HotkeyAction::Finish),
                "double_tap_enabled={double_tap_enabled}"
            );
            assert!(!hotkey.is_recording());
        }
    }

    #[test]
    fn live_double_tap_only_change_preserves_capture_until_release_or_escape() {
        use crate::dictation::{DictationCapture, Finish};

        let now = capture_time();
        let binding = shift_space_binding();
        for cancel in [false, true] {
            let mut hotkey = DictationHotkey::with_binding(false, now, true, binding);
            let mut capture = DictationCapture::new(16_000);
            let key = |code, down| InputEvent::Key {
                code,
                down,
                flags: SHIFT_KEY_MASK,
            };
            assert_eq!(
                hotkey.process(key(49, true), now),
                Some(HotkeyAction::Start)
            );
            capture.start_at(now);
            capture.ingest(&[0.5; 8_000], now + Duration::from_millis(500));

            hotkey.set_double_tap_only(true);
            hotkey.set_double_tap_only(true);
            assert!(hotkey.is_recording());
            assert!(capture.is_recording());

            let ended_at = now + Duration::from_millis(600);
            capture.ingest(&[0.5; 1_600], ended_at);
            let event = if cancel {
                key(ESCAPE_KEY_CODE, true)
            } else {
                key(49, false)
            };
            let action = hotkey.process(event, ended_at);
            if cancel {
                assert_eq!(action, Some(HotkeyAction::Cancel));
                capture.cancel();
                hotkey.process(key(ESCAPE_KEY_CODE, false), ended_at);
                hotkey.process(key(49, false), ended_at);
            } else {
                assert_eq!(action, Some(HotkeyAction::Finish));
                let Finish::Transcribe(clip) = capture.finish(ended_at) else {
                    panic!("the active hold must finish after changing double-tap-only");
                };
                assert_eq!(clip.duration_ms(), 600);
            }
            assert!(!hotkey.is_recording());
            assert!(!capture.is_recording());

            hotkey.process(InputEvent::Flags(0), ended_at);
            assert_eq!(
                hotkey.process(key(49, true), now + Duration::from_secs(1)),
                None,
                "the next gesture must use double-tap-only"
            );
            assert!(!hotkey.is_recording());
        }
    }

    #[test]
    fn live_double_tap_disable_prevents_the_second_release_from_locking() {
        use crate::dictation::{DictationCapture, Finish};

        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        let mut capture = DictationCapture::new(16_000);
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now),
            Some(HotkeyAction::Start)
        );
        capture.start_at(now);
        let first_release = now + Duration::from_millis(80);
        capture.ingest(&[0.5; 1_280], first_release);
        assert_eq!(
            hotkey.process(InputEvent::Flags(0), first_release),
            Some(HotkeyAction::Finish)
        );
        assert!(matches!(capture.finish(first_release), Finish::Discard));

        let second_press = now + Duration::from_millis(180);
        capture.ingest(&[0.5; 1_600], second_press);
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), second_press),
            Some(HotkeyAction::Start)
        );
        capture.start_at(second_press);
        hotkey.set_double_tap_enabled(false);
        hotkey.set_double_tap_only(false);
        assert!(hotkey.is_recording());
        assert!(capture.is_recording());

        let second_release = now + Duration::from_millis(260);
        capture.ingest(&[0.5; 1_280], second_release);
        assert_eq!(
            hotkey.process(InputEvent::Flags(0), second_release),
            Some(HotkeyAction::Finish)
        );
        assert!(matches!(capture.finish(second_release), Finish::Discard));
        assert!(!hotkey.is_recording());
        assert!(!capture.is_recording());
    }

    #[test]
    fn early_chord_discards_until_everything_is_released() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now);
        assert_eq!(
            hotkey.process(
                InputEvent::Key {
                    code: 0,
                    down: true,
                    flags: OPTION_KEY_MASK,
                },
                now + Duration::from_millis(100),
            ),
            Some(HotkeyAction::Discard(DiscardCause::Key))
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(NO_FLAGS),
                now + Duration::from_millis(150)
            ),
            None
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Key {
                    code: 0,
                    down: false,
                    flags: NO_FLAGS,
                },
                now + Duration::from_millis(200),
            ),
            None
        );
        assert!(!hotkey.is_recording());
    }

    fn option_key(code: u16, down: bool, flags: u64) -> InputEvent {
        InputEvent::Key { code, down, flags }
    }

    fn hid_key(code: u16) -> InputEvent {
        InputEvent::PhysicalKeyDown {
            code,
            timing: HidTiming {
                raw: 0,
                clock: HidClock::Nanos,
                received_at: CaptureInstant::from_nanos(0),
            },
        }
    }

    // Another app's global Option-1 hides the digit from the session tap, so
    // the HID press is the only sign of the chord.
    #[test]
    fn hid_evidence_discards_a_chord_hidden_by_a_global_shortcut() {
        let now = capture_time();
        for (key, chord_ms) in [(18, 20), (19, 100), (20, 200), (49, 150)] {
            let mut hotkey = test_hotkey(false, now);
            assert_eq!(
                hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now),
                Some(HotkeyAction::Start)
            );
            assert_eq!(
                hotkey.process(hid_key(key), now + Duration::from_millis(chord_ms)),
                Some(HotkeyAction::Discard(DiscardCause::HidKey)),
                "key {key} at {chord_ms} ms"
            );
            for release_ms in [chord_ms + 50, 1_500] {
                assert_eq!(
                    hotkey.process(
                        InputEvent::Flags(NO_FLAGS),
                        now + Duration::from_millis(release_ms)
                    ),
                    None
                );
            }
            assert!(!hotkey.is_recording());
            // The discarded chord is not the first tap of a double tap.
            let again = now + Duration::from_millis(1_600);
            assert_eq!(
                hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), again),
                Some(HotkeyAction::Start)
            );
            assert_eq!(
                hotkey.process(InputEvent::Flags(NO_FLAGS), again + Duration::from_secs(1)),
                Some(HotkeyAction::Finish)
            );
        }
    }

    #[test]
    fn hid_evidence_and_visible_chord_keys_discard_once_in_either_release_order() {
        let now = capture_time();
        for key_released_first in [true, false] {
            let mut hotkey = test_hotkey(false, now);
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now);
            let chord = now + Duration::from_millis(60);
            assert_eq!(
                hotkey.process(hid_key(18), chord),
                Some(HotkeyAction::Discard(DiscardCause::HidKey))
            );
            assert_eq!(
                hotkey.process(option_key(18, true, OPTION_KEY_MASK), chord),
                None
            );
            let mut releases = vec![
                option_key(18, false, OPTION_KEY_MASK),
                InputEvent::Flags(NO_FLAGS),
            ];
            if !key_released_first {
                releases = vec![InputEvent::Flags(NO_FLAGS), option_key(18, false, NO_FLAGS)];
            }
            for (offset, event) in releases.into_iter().enumerate() {
                let at = chord + Duration::from_millis(40 * (offset as u64 + 1));
                assert_eq!(hotkey.process(event, at), None);
            }
            assert!(!hotkey.is_recording());
            assert!(hotkey.pressed_keys.is_empty());
            let again = now + Duration::from_secs(1);
            assert_eq!(
                hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), again),
                Some(HotkeyAction::Start),
                "key_released_first={key_released_first}"
            );
        }
    }

    // On 2026-10-22 two quick Option-1/Option-2 switches locked dictation:
    // with the digits hidden, the two Option taps looked like a double tap.
    #[test]
    fn hidden_chords_in_quick_succession_never_double_tap_lock() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        let mut actions = Vec::new();
        for (at_ms, event) in [
            (0, InputEvent::Flags(OPTION_KEY_MASK)),
            (40, hid_key(18)),
            (160, InputEvent::Flags(NO_FLAGS)),
            (280, InputEvent::Flags(OPTION_KEY_MASK)),
            (310, hid_key(19)),
            (400, InputEvent::Flags(NO_FLAGS)),
        ] {
            actions.extend(hotkey.process(event, now + Duration::from_millis(at_ms)));
        }
        assert_eq!(
            actions,
            [
                HotkeyAction::Start,
                HotkeyAction::Discard(DiscardCause::HidKey),
                HotkeyAction::Start,
                HotkeyAction::Discard(DiscardCause::HidKey)
            ]
        );
        assert!(!hotkey.is_recording());
    }

    #[test]
    fn hid_evidence_that_overtakes_its_press_prevents_the_capture() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        let chord = now + Duration::from_millis(15);
        assert_eq!(hotkey.process(hid_key(18), chord), None);
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now),
            None
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(NO_FLAGS),
                now + Duration::from_millis(500)
            ),
            None
        );
        assert!(!hotkey.is_recording());

        // A later overtaking press must not hide the early one.
        let mut hotkey = test_hotkey(false, now);
        for chord_ms in [20, 400] {
            hotkey.process(hid_key(18), now + Duration::from_millis(chord_ms));
        }
        assert_eq!(
            hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now),
            None
        );

        // Delayed evidence of a key pressed before the shortcut is no chord.
        let mut hotkey = test_hotkey(false, now);
        hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now);
        assert_eq!(
            hotkey.process(
                hid_key(0),
                now.checked_sub(Duration::from_millis(5)).unwrap()
            ),
            None
        );
        assert!(hotkey.is_recording());

        // A key pressed before the shortcut does not block it.
        let mut hotkey = test_hotkey(false, now);
        let later = now + Duration::from_secs(2);
        hotkey.process(hid_key(0), later);
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(OPTION_KEY_MASK),
                later + Duration::from_millis(5)
            ),
            Some(HotkeyAction::Start)
        );
    }

    // Kit's Studio, 2026-10-25: holds discarded ~20 ms after the press. Suspect:
    // the HID press of a key typed just before Option, delivered late in Mach
    // ticks, reads as a chord. The conversion and the guard keep it a hold.
    #[test]
    fn late_hid_evidence_in_mach_ticks_of_a_key_before_the_press_keeps_the_hold() {
        let now = CaptureInstant::now();
        let press = now.checked_sub(Duration::from_millis(100)).unwrap();
        for typed_before_ms in [1, 5, 30] {
            let typed = press
                .checked_sub(Duration::from_millis(typed_before_ms))
                .unwrap();
            for delivered_after_press_ms in [1, 20, 60] {
                let received = press + Duration::from_millis(delivered_after_press_ms);
                let raw = mach_ticks_for(typed);
                let (at, clock) = physical_event_instant(raw, received);
                assert_eq!(clock, HidClock::MachTicks);
                assert!(signed_ms(at, typed).abs() < 0.001);
                let mut hotkey = test_hotkey(false, press);
                assert_eq!(
                    hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), press),
                    Some(HotkeyAction::Start)
                );
                let event = InputEvent::PhysicalKeyDown {
                    code: 0,
                    timing: HidTiming {
                        raw,
                        clock,
                        received_at: received,
                    },
                };
                assert_eq!(hotkey.process(event, at), None);
                assert!(hotkey.is_recording());
                assert_eq!(
                    hotkey.process(InputEvent::Flags(NO_FLAGS), press + Duration::from_secs(1)),
                    Some(HotkeyAction::Finish)
                );
            }
        }
    }

    #[test]
    fn early_mouse_down_discards_with_its_cause() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now);
        assert_eq!(
            hotkey.process(InputEvent::MouseDown, now + Duration::from_millis(20)),
            Some(HotkeyAction::Discard(DiscardCause::Mouse))
        );
    }

    #[test]
    fn held_modifier_only_shortcut_plus_any_key_discards_after_the_hold_threshold() {
        let now = capture_time();
        for chord_ms in [300, 450, 2_000] {
            for (key, hidden) in [(18, true), (19, true), (20, true), (49, true), (18, false)] {
                let mut hotkey = test_hotkey(false, now);
                hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now);
                let chord = now + Duration::from_millis(chord_ms);
                let event = if hidden {
                    hid_key(key)
                } else {
                    option_key(key, true, OPTION_KEY_MASK)
                };
                assert_eq!(
                    hotkey.process(event, chord),
                    Some(HotkeyAction::Discard(if hidden {
                        DiscardCause::HidKey
                    } else {
                        DiscardCause::Key
                    })),
                    "key {key} hidden={hidden} at {chord_ms} ms"
                );
                if !hidden {
                    hotkey.process(option_key(key, false, OPTION_KEY_MASK), chord);
                }
                assert_eq!(
                    hotkey.process(
                        InputEvent::Flags(NO_FLAGS),
                        chord + Duration::from_millis(50)
                    ),
                    None
                );
                assert!(!hotkey.is_recording());
            }
        }

        // Toggle captures keep recording, and keyed shortcuts keep the threshold.
        let mut hotkey = test_hotkey(false, now);
        hotkey.set_toggle(true);
        hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now);
        assert_eq!(
            hotkey.process(hid_key(18), now + Duration::from_millis(500)),
            None
        );
        assert!(hotkey.is_recording());
        let mut hotkey = DictationHotkey::with_binding(false, now, true, shift_space_binding());
        hotkey.process(shift_space_event(true), now);
        assert_eq!(
            hotkey.process(
                InputEvent::Key {
                    code: 0,
                    down: true,
                    flags: SHIFT_KEY_MASK
                },
                now + Duration::from_millis(500)
            ),
            None
        );
        assert!(hotkey.is_recording());
    }

    #[test]
    fn hid_evidence_while_locked_is_ignored() {
        let now = capture_time();
        let mut hotkey = test_hotkey(false, now);
        hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now);
        hotkey.process(InputEvent::Flags(NO_FLAGS), now + Duration::from_millis(80));
        hotkey.process(
            InputEvent::Flags(OPTION_KEY_MASK),
            now + Duration::from_millis(180),
        );
        hotkey.process(
            InputEvent::Flags(NO_FLAGS),
            now + Duration::from_millis(260),
        );
        assert!(hotkey.is_recording());
        assert_eq!(
            hotkey.process(hid_key(18), now + Duration::from_millis(400)),
            None
        );
        assert!(hotkey.is_recording());
    }

    #[test]
    fn chord_evidence_skips_escape_and_hex_shortcuts() {
        let hotkeys = RuntimeHotkeys::default();
        assert!(is_chord_evidence(18, OPTION_KEY_MASK, hotkeys));
        assert!(is_chord_evidence(49, OPTION_KEY_MASK, hotkeys));
        assert!(!is_chord_evidence(
            ESCAPE_KEY_CODE,
            OPTION_KEY_MASK,
            hotkeys
        ));
        let paste_last = hotkeys
            .paste_last
            .expect("paste last has a default binding");
        let code = paste_last.key_code.expect("paste last binds a key");
        assert!(!is_chord_evidence(
            code,
            OPTION_KEY_MASK | SHIFT_KEY_MASK,
            hotkeys
        ));
        assert!(is_chord_evidence(code, OPTION_KEY_MASK, hotkeys));
    }

    #[test]
    fn physical_event_instants_accept_mach_ticks_or_nanoseconds() {
        let now = CaptureInstant::now();
        let earlier = now.checked_sub(Duration::from_millis(8)).unwrap();
        let (converted, clock) = physical_event_instant(earlier.as_nanos(), now);
        assert_eq!(converted, earlier);
        // With a 1:1 timebase (Intel) both readings agree and Mach wins.
        let unit_timebase =
            CaptureInstant::from_mach_ticks(1_000) == CaptureInstant::from_nanos(1_000);
        assert_eq!(
            clock,
            if unit_timebase {
                HidClock::MachTicks
            } else {
                HidClock::Nanos
            }
        );
        let (converted, clock) = physical_event_instant(mach_ticks_for(earlier), now);
        assert_eq!(clock, HidClock::MachTicks);
        assert!(now.duration_since(converted) < Duration::from_millis(9));
        assert!(converted <= now);
        assert_eq!(physical_event_instant(1, now), (now, HidClock::Arrival));
    }

    fn mach_ticks_for(instant: CaptureInstant) -> u64 {
        // Binary search the tick count whose conversion reaches `instant`.
        let (mut low, mut high) = (0_u64, instant.as_nanos().max(1));
        while low < high {
            let mid = low + (high - low) / 2;
            if CaptureInstant::from_mach_ticks(mid) < instant {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        low
    }

    #[test]
    fn escape_cancels_active_capture() {
        let now = capture_time();
        let mut hotkey = test_hotkey(true, now);
        assert_eq!(
            hotkey.process(
                InputEvent::Key {
                    code: ESCAPE_KEY_CODE,
                    down: true,
                    flags: OPTION_KEY_MASK,
                },
                now,
            ),
            Some(HotkeyAction::Cancel)
        );
    }

    /// A capsule list left open when captures are turned off takes no keys.
    #[cfg(hex_capture)]
    #[test]
    fn the_list_takes_keys_only_while_captures_are_on() {
        assert!(list_takes_keys(true, true, false));
        assert!(!list_takes_keys(false, true, false));
        assert!(!list_takes_keys(true, false, false));
        assert!(!list_takes_keys(true, true, true));
    }

    /// Escape during a dictation hold while a screen capture records cancels
    /// only the dictation: the capture shortcut lets it pass, and the
    /// listener never hands it to the recording.
    #[cfg(hex_capture)]
    #[test]
    fn escape_during_a_capture_cancels_only_the_dictation_hold() {
        use crate::capture_host::{CaptureShortcut, ShortcutAction, ShortcutActivity};
        let now = capture_time();
        let mut hotkey = test_hotkey(true, now);
        let escape = InputEvent::Key {
            code: ESCAPE_KEY_CODE,
            down: true,
            flags: OPTION_KEY_MASK,
        };
        let mut capture = CaptureShortcut::default();
        let recording = ShortcutActivity::Recording {
            elapsed: std::time::Duration::from_secs(5),
        };
        assert_eq!(
            capture.input(escape, RuntimeHotkeys::default(), recording),
            ShortcutAction::None
        );
        assert!(!capture.takes_escape());
        assert_eq!(hotkey.process(escape, now), Some(HotkeyAction::Cancel));
    }

    #[test]
    fn escape_is_suppressed_while_it_controls_dictation() {
        let mut suppression = ShortcutSuppression::default();
        let hotkeys = RuntimeHotkeys::default();
        let down = InputEvent::Key {
            code: ESCAPE_KEY_CODE,
            down: true,
            flags: NO_FLAGS,
        };
        let up = InputEvent::Key {
            code: ESCAPE_KEY_CODE,
            down: false,
            flags: NO_FLAGS,
        };

        assert!(!suppression.process_all(down, hotkeys, true, false));
        assert!(!suppression.process_all(up, hotkeys, true, false));
        assert!(!suppression.process_all(down, hotkeys, false, true));
        assert!(!suppression.process_all(up, hotkeys, false, true));

        // Cancelling the capture clears escape_cancels before the key lifts;
        // the repeat and release keep the press's suppression decision.
        assert!(suppression.process_all(down, hotkeys, true, true));
        assert!(suppression.process_all(down, hotkeys, true, false));
        assert!(suppression.process_all(up, hotkeys, true, false));
        assert!(!suppression.process_all(down, hotkeys, true, false));
    }

    #[test]
    fn extra_modifier_after_threshold_is_ignored() {
        let now = capture_time();
        let mut hotkey = test_hotkey(true, now);
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(OPTION_KEY_MASK | SHIFT_KEY_MASK),
                now + Duration::from_millis(500),
            ),
            None
        );
        assert!(hotkey.is_recording());
    }

    #[test]
    fn option_shift_v_pastes_last_transcript() {
        let now = capture_time();
        let mut hotkey = test_hotkey(true, now);
        let paste_key_code = HotkeyBinding::paste_last_default()
            .key
            .expect("default paste key")
            .code;
        let key_down = InputEvent::Key {
            code: paste_key_code,
            down: true,
            flags: OPTION_KEY_MASK | SHIFT_KEY_MASK,
        };
        assert_eq!(
            hotkey.process(key_down, now + Duration::from_millis(50)),
            Some(HotkeyAction::PasteLast)
        );
        assert_eq!(
            hotkey.process(key_down, now + Duration::from_millis(60)),
            None
        );
        assert!(!hotkey.is_recording());
        let mut suppression = ShortcutSuppression::default();
        assert!(suppression.process(key_down, paste_key_code, option_binding(), true));
        assert!(suppression.process(key_down, paste_key_code, option_binding(), true));
        assert!(suppression.process(
            InputEvent::Key {
                code: paste_key_code,
                down: false,
                flags: OPTION_KEY_MASK | SHIFT_KEY_MASK,
            },
            paste_key_code,
            option_binding(),
            true,
        ));
    }

    #[test]
    fn the_capture_shortcut_is_suppressed_and_never_chord_evidence() {
        let hotkeys = RuntimeHotkeys {
            capture: Some(RuntimeHotkey {
                modifiers: crate::app_settings::HotkeyModifiers {
                    option: Some(crate::app_settings::ModifierSide::Either),
                    shift: Some(crate::app_settings::ModifierSide::Either),
                    ..Default::default()
                },
                key_code: Some(1),
            }),
            ..RuntimeHotkeys::default()
        };
        let flags = OPTION_KEY_MASK | SHIFT_KEY_MASK;
        assert!(!is_chord_evidence(1, flags, hotkeys));
        assert!(is_chord_evidence(1, flags, RuntimeHotkeys::default()));
        let mut suppression = ShortcutSuppression::default();
        let key = |down| InputEvent::Key {
            code: 1,
            down,
            flags,
        };
        assert!(suppression.process_all(key(true), hotkeys, true, false));
        assert!(suppression.process_all(key(false), hotkeys, true, false));
        assert!(!suppression.process_all(key(true), RuntimeHotkeys::default(), true, false));
    }

    #[test]
    fn paste_last_can_be_disabled_or_bound_to_a_side_specific_shortcut() {
        let key_code = 0;
        let input = InputEvent::Key {
            code: key_code,
            down: true,
            flags: OPTION_KEY_MASK | crate::app_settings::RIGHT_OPTION_MASK,
        };
        let disabled = RuntimeHotkeys {
            paste_last: None,
            paste_meeting: None,
            ..RuntimeHotkeys::default()
        };
        assert_eq!(paste_action(input, disabled), None);

        let configured = RuntimeHotkeys {
            paste_last: Some(RuntimeHotkey {
                modifiers: crate::app_settings::HotkeyModifiers {
                    option: Some(crate::app_settings::ModifierSide::Right),
                    ..Default::default()
                },
                key_code: Some(key_code),
            }),
            paste_meeting: None,
            ..RuntimeHotkeys::default()
        };
        assert_eq!(
            paste_action(input, configured),
            Some(HotkeyAction::PasteLast)
        );
        assert_eq!(
            paste_action(
                InputEvent::Key {
                    code: key_code,
                    down: true,
                    flags: OPTION_KEY_MASK | crate::app_settings::LEFT_OPTION_MASK,
                },
                configured,
            ),
            None
        );
    }

    #[test]
    fn meeting_paste_is_only_active_in_developer_builds() {
        let now = capture_time();
        let mut hotkey = test_hotkey(true, now);
        let paste_key_code = HotkeyBinding::paste_last_default()
            .key
            .expect("default paste key")
            .code;
        assert_eq!(
            hotkey.process(
                InputEvent::Key {
                    code: paste_key_code,
                    down: true,
                    flags: OPTION_KEY_MASK | CONTROL_KEY_MASK,
                },
                now + Duration::from_millis(50),
            ),
            // Release treats this as an ordinary chord, not a reserved meeting action.
            if crate::DEVELOPER_FEATURES_ENABLED {
                Some(HotkeyAction::PasteMeeting)
            } else {
                Some(HotkeyAction::Discard(DiscardCause::Key))
            }
        );
        assert!(!hotkey.is_recording());
        assert_eq!(
            paste_action_with_meetings(
                InputEvent::Key {
                    code: paste_key_code,
                    down: true,
                    flags: OPTION_KEY_MASK | SHIFT_KEY_MASK,
                },
                paste_key_code,
                true,
            ),
            Some(HotkeyAction::PasteLast)
        );
        assert_eq!(
            paste_action_with_meetings(
                InputEvent::Key {
                    code: paste_key_code,
                    down: true,
                    flags: OPTION_KEY_MASK,
                },
                paste_key_code,
                true,
            ),
            None
        );
        let key_down = InputEvent::Key {
            code: paste_key_code,
            down: true,
            flags: OPTION_KEY_MASK | CONTROL_KEY_MASK,
        };
        assert_eq!(
            paste_action_with_meetings(key_down, paste_key_code, false),
            None
        );
        let mut suppression = ShortcutSuppression::default();
        assert!(suppression.process(key_down, paste_key_code, option_binding(), true));
        assert!(!ShortcutSuppression::default().process(
            key_down,
            paste_key_code,
            option_binding(),
            false,
        ));
    }

    #[test]
    fn input_activity_revision_invalidates_continuation() {
        let activity = InputActivity::default();
        let revision = activity.revision();

        activity.invalidate();

        assert_ne!(activity.revision(), revision);
    }

    fn both_options() -> RuntimeHotkey {
        use crate::app_settings::{HotkeyModifiers, ModifierSide};
        RuntimeHotkey {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Both),
                ..Default::default()
            },
            key_code: None,
        }
    }

    const LEFT_OPTION: u64 = OPTION_KEY_MASK | crate::app_settings::LEFT_OPTION_MASK;
    const RIGHT_OPTION: u64 = OPTION_KEY_MASK | crate::app_settings::RIGHT_OPTION_MASK;
    const BOTH_OPTIONS: u64 = LEFT_OPTION | RIGHT_OPTION;

    #[test]
    fn both_options_press_on_the_second_key_and_release_on_either() {
        let now = capture_time();
        let ms = Duration::from_millis;
        for (first, release_to) in [(LEFT_OPTION, RIGHT_OPTION), (RIGHT_OPTION, LEFT_OPTION)] {
            let mut hotkey = DictationHotkey::with_binding(false, now, false, both_options());
            assert_eq!(hotkey.process(InputEvent::Flags(first), now), None);
            assert!(!hotkey.is_recording(), "one Option is not the chord");
            // The press is the second key's own edge: the hold threshold and
            // pre-roll count from it, never from an earlier key.
            let down = now + ms(30);
            assert_eq!(
                hotkey.process(InputEvent::Flags(BOTH_OPTIONS), down),
                Some(HotkeyAction::Start)
            );
            assert!(
                hotkey
                    .process(InputEvent::Flags(BOTH_OPTIONS), down + ms(500))
                    .is_none()
            );
            assert_eq!(
                hotkey.process(InputEvent::Flags(release_to), down + ms(800)),
                Some(HotkeyAction::Finish),
                "lifting either key ends the chord"
            );
            assert_eq!(hotkey.process(InputEvent::Flags(0), down + ms(900)), None);
        }
    }

    #[test]
    fn both_options_must_go_down_together() {
        let now = capture_time();
        let mut hotkey = DictationHotkey::with_binding(false, now, false, both_options());
        assert_eq!(hotkey.process(InputEvent::Flags(LEFT_OPTION), now), None);
        let late = now + BOTH_SIDES_WINDOW + Duration::from_millis(1);
        assert_eq!(
            hotkey.process(InputEvent::Flags(BOTH_OPTIONS), late),
            None,
            "an Option held while the other is added later presses nothing"
        );
        assert_eq!(hotkey.process(InputEvent::Flags(LEFT_OPTION), late), None);
        assert_eq!(hotkey.process(InputEvent::Flags(0), late), None);
        let again = late + Duration::from_secs(1);
        assert_eq!(hotkey.process(InputEvent::Flags(RIGHT_OPTION), again), None);
        assert_eq!(
            hotkey.process(InputEvent::Flags(BOTH_OPTIONS), again + BOTH_SIDES_WINDOW),
            Some(HotkeyAction::Start),
            "the window's edge still counts"
        );
    }

    #[test]
    fn both_options_discard_an_extra_key_within_the_hold() {
        let now = capture_time();
        let mut hotkey = DictationHotkey::with_binding(false, now, false, both_options());
        hotkey.process(InputEvent::Flags(LEFT_OPTION), now);
        assert_eq!(
            hotkey.process(InputEvent::Flags(BOTH_OPTIONS), now),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(BOTH_OPTIONS | COMMAND_KEY_MASK),
                now + Duration::from_millis(100)
            ),
            Some(HotkeyAction::Discard(DiscardCause::ExtraModifier))
        );
        let mut typed = DictationHotkey::with_binding(false, now, false, both_options());
        typed.process(InputEvent::Flags(BOTH_OPTIONS), now);
        assert_eq!(
            typed.process(
                InputEvent::Key {
                    code: 0,
                    down: true,
                    flags: BOTH_OPTIONS,
                },
                now + Duration::from_secs(2)
            ),
            Some(HotkeyAction::Discard(DiscardCause::Key)),
            "a held modifier-only chord plus a key is a chord at any time"
        );
    }

    #[test]
    fn both_options_lock_on_a_double_tap_and_in_toggle_mode() {
        let ms = Duration::from_millis;
        let now = capture_time();
        let mut hotkey = DictationHotkey::with_binding(false, now, true, both_options());
        let tap = |hotkey: &mut DictationHotkey, at: CaptureInstant| {
            let pressed = hotkey.process(InputEvent::Flags(BOTH_OPTIONS), at);
            let released = hotkey.process(InputEvent::Flags(0), at + ms(60));
            (pressed, released)
        };
        assert_eq!(
            tap(&mut hotkey, now),
            (Some(HotkeyAction::Start), Some(HotkeyAction::Finish))
        );
        assert_eq!(
            tap(&mut hotkey, now + ms(150)),
            (Some(HotkeyAction::Start), None)
        );
        assert!(hotkey.is_locked());
        assert_eq!(
            hotkey.process(InputEvent::Flags(RIGHT_OPTION), now + ms(2_000)),
            None,
            "one Option does not finish the lock"
        );
        hotkey.process(InputEvent::Flags(0), now + ms(2_050));
        assert_eq!(
            hotkey.process(InputEvent::Flags(BOTH_OPTIONS), now + ms(3_000)),
            Some(HotkeyAction::Finish)
        );

        let mut toggle = DictationHotkey::with_binding(false, now, false, both_options());
        toggle.set_toggle(true);
        assert_eq!(tap(&mut toggle, now), (Some(HotkeyAction::Start), None));
        assert!(toggle.is_locked());
        assert_eq!(
            toggle.process(InputEvent::Flags(BOTH_OPTIONS), now + ms(5_000)),
            Some(HotkeyAction::Finish)
        );
    }

    #[test]
    fn one_sided_options_never_start_with_both_held_but_either_side_does() {
        use crate::app_settings::{HotkeyModifiers, ModifierSide};
        let now = capture_time();
        let sided = |side| RuntimeHotkey {
            modifiers: HotkeyModifiers {
                option: Some(side),
                ..Default::default()
            },
            key_code: None,
        };
        let mut right =
            DictationHotkey::with_binding(false, now, false, sided(ModifierSide::Right));
        assert_eq!(right.process(InputEvent::Flags(BOTH_OPTIONS), now), None);
        let mut either =
            DictationHotkey::with_binding(false, now, false, sided(ModifierSide::Either));
        assert_eq!(
            either.process(InputEvent::Flags(BOTH_OPTIONS), now),
            Some(HotkeyAction::Start),
            "Option keeps matching both held, as before chords"
        );
        // Right Option held, then the left one added: the extra key
        // discards a press still inside its hold.
        let mut promoted =
            DictationHotkey::with_binding(false, now, false, sided(ModifierSide::Right));
        assert_eq!(
            promoted.process(InputEvent::Flags(RIGHT_OPTION), now),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            promoted.process(
                InputEvent::Flags(BOTH_OPTIONS),
                now + Duration::from_millis(40)
            ),
            Some(HotkeyAction::Discard(DiscardCause::ExtraModifier))
        );
    }
}

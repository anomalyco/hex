use std::collections::HashSet;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};

use crate::app_settings::RuntimeHotkey;
use crate::dictation::MINIMUM_HOLD_DURATION;

const DOUBLE_TAP_WINDOW: Duration = MINIMUM_HOLD_DURATION;
const ESCAPE_KEY_CODE: u16 = 53;

const EVENT_LEFT_MOUSE_DOWN: u32 = 1;
const EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
const EVENT_KEY_DOWN: u32 = 10;
const EVENT_KEY_UP: u32 = 11;
const EVENT_FLAGS_CHANGED: u32 = 12;
const EVENT_OTHER_MOUSE_DOWN: u32 = 25;
const EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = u32::MAX - 1;
const EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = u32::MAX;
const KEYBOARD_EVENT_KEYCODE: u32 = 9;
const EVENT_SOURCE_USER_DATA: u32 = 42;

const HID_EVENT_TAP: u32 = 0;
const HEAD_INSERT_EVENT_TAP: u32 = 0;
const DEFAULT_EVENT_TAP: u32 = 0;
const OPTION_KEY_MASK: u64 = 1 << 19;
const SHIFT_KEY_MASK: u64 = 1 << 17;
const CONTROL_KEY_MASK: u64 = 1 << 18;
const DEVICE_INDEPENDENT_MODIFIERS_MASK: u64 = 0xffff_0000;
const HID_SYSTEM_STATE: u32 = 1;

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
    fn CGEventGetIntegerValueField(event: EventRef, field: u32) -> i64;
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
    Key { code: u16, down: bool, flags: u64 },
    MouseDown,
    TapDisabled,
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

pub struct InputMonitor {
    pub events: Receiver<InputEvent>,
    pub activity: InputActivity,
    pub paste_key_code: u16,
    missed_event: Arc<AtomicBool>,
    escape_cancels: Arc<AtomicBool>,
    run_loop: Arc<AtomicPtr<c_void>>,
    worker: Option<JoinHandle<()>>,
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
}

impl InputMonitor {
    pub fn start() -> Result<Self> {
        let (sender, events) = mpsc::sync_channel(64);
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let activity = InputActivity::default();
        let tap_activity = activity.clone();
        let run_loop = Arc::new(AtomicPtr::new(ptr::null_mut()));
        let tap_run_loop = run_loop.clone();
        let escape_cancels = Arc::new(AtomicBool::new(false));
        let tap_escape_cancels = escape_cancels.clone();
        let missed_event = Arc::new(AtomicBool::new(false));
        let tap_missed_event = missed_event.clone();
        let paste_key_code = crate::keyboard::key_code_for('v').unwrap_or(9);
        tracing::info!(
            paste_key_code,
            "resolved paste key for active keyboard layout"
        );
        let worker = thread::spawn(move || {
            run_event_tap(
                sender,
                tap_activity,
                paste_key_code,
                tap_escape_cancels,
                tap_missed_event,
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
            paste_key_code,
            missed_event,
            escape_cancels,
            run_loop,
            worker: Some(worker),
        })
    }

    pub fn set_escape_cancels(&self, enabled: bool) {
        self.escape_cancels.store(enabled, Ordering::Release);
    }

    pub fn take_missed_event(&self) -> bool {
        self.missed_event.swap(false, Ordering::AcqRel)
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
    sender: SyncSender<InputEvent>,
    activity: InputActivity,
    paste_key_code: u16,
    escape_cancels: Arc<AtomicBool>,
    missed_event: Arc<AtomicBool>,
    tap: AtomicPtr<c_void>,
    shortcut_suppression: Mutex<ShortcutSuppression>,
}

fn run_event_tap(
    sender: SyncSender<InputEvent>,
    activity: InputActivity,
    paste_key_code: u16,
    escape_cancels: Arc<AtomicBool>,
    missed_event: Arc<AtomicBool>,
    run_loop: Arc<AtomicPtr<c_void>>,
    ready: SyncSender<Result<()>>,
) {
    let context = Box::into_raw(Box::new(EventTapContext {
        sender,
        activity,
        paste_key_code,
        escape_cancels,
        missed_event,
        tap: AtomicPtr::new(ptr::null_mut()),
        shortcut_suppression: Mutex::new(ShortcutSuppression::default()),
    }));
    let mask = [
        EVENT_LEFT_MOUSE_DOWN,
        EVENT_RIGHT_MOUSE_DOWN,
        EVENT_KEY_DOWN,
        EVENT_KEY_UP,
        EVENT_FLAGS_CHANGED,
        EVENT_OTHER_MOUSE_DOWN,
    ]
    .into_iter()
    .fold(0, |mask, event| mask | 1_u64 << event);
    // SAFETY: The callback context remains allocated until this run loop stops.
    let tap = unsafe {
        CGEventTapCreate(
            HID_EVENT_TAP,
            HEAD_INSERT_EVENT_TAP,
            DEFAULT_EVENT_TAP,
            mask,
            event_callback,
            context.cast(),
        )
    };
    if tap.is_null() {
        // SAFETY: No callback can run because tap creation failed.
        unsafe { drop(Box::from_raw(context)) };
        let _ = ready.send(Err(eyre!(
            "could not create keyboard event tap; grant Input Monitoring and Accessibility permissions"
        )));
        return;
    }
    // SAFETY: `context` remains owned by this event-tap thread.
    unsafe { (*context).tap.store(tap, Ordering::Release) };
    // SAFETY: `tap` is a valid CFMachPort returned above.
    let source = unsafe { CFMachPortCreateRunLoopSource(ptr::null(), tap, 0) };
    if source.is_null() {
        // SAFETY: The source was not created, so the tap cannot be attached to a run loop.
        unsafe {
            CGEventTapEnable(tap, false);
            CFRelease(tap.cast_const());
            drop(Box::from_raw(context));
        }
        let _ = ready.send(Err(eyre!("could not create keyboard run-loop source")));
        return;
    }
    // SAFETY: This is the dedicated event-tap thread's current run loop.
    let current_run_loop = unsafe { CFRunLoopGetCurrent() };
    run_loop.store(current_run_loop, Ordering::Release);
    // SAFETY: All values belong to this thread and remain alive while the run loop runs.
    unsafe {
        CFRunLoopAddSource(current_run_loop, source, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
    }
    let _ = ready.send(Ok(()));
    // SAFETY: This dedicated thread exists solely to dispatch the event tap.
    unsafe { CFRunLoopRun() };
    // SAFETY: Disable the tap before releasing its callback context.
    unsafe {
        CGEventTapEnable(tap, false);
        CFRunLoopRemoveSource(current_run_loop, source, kCFRunLoopCommonModes);
    }
    run_loop.store(ptr::null_mut(), Ordering::Release);
    // SAFETY: The source is detached and the tap is disabled, so no callback can use context.
    unsafe {
        CFRelease(source.cast_const());
        CFRelease(tap.cast_const());
        drop(Box::from_raw(context));
    }
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
        let tap = context.tap.load(Ordering::Acquire);
        if !tap.is_null() {
            // SAFETY: `tap` is the CFMachPort returned by `CGEventTapCreate`.
            unsafe { CGEventTapEnable(tap, true) };
        }
        send_input(context, InputEvent::TapDisabled);
        return event;
    }
    if event.is_null() {
        return event;
    }
    // SAFETY: CoreGraphics supplied a valid event to this callback.
    if unsafe { CGEventGetIntegerValueField(event, EVENT_SOURCE_USER_DATA) }
        == crate::keyboard::SYNTHETIC_EVENT_MARKER
    {
        return event;
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
        EVENT_LEFT_MOUSE_DOWN | EVENT_RIGHT_MOUSE_DOWN | EVENT_OTHER_MOUSE_DOWN => {
            InputEvent::MouseDown
        }
        _ => return event,
    };
    if matches!(
        input,
        InputEvent::Key { down: true, .. } | InputEvent::MouseDown
    ) {
        context.activity.invalidate();
    }
    if crate::app_settings::hotkey_capture_active() {
        context
            .shortcut_suppression
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .reset();
        return event;
    }
    let delivered = send_input(context, input);
    let mut suppression = context
        .shortcut_suppression
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let suppress =
        suppression.process_escape(
            input,
            delivered,
            context.escape_cancels.load(Ordering::Acquire),
        ) || suppression.process(
            input,
            context.paste_key_code,
            crate::app_settings::dictation_hotkey(),
            delivered,
        ) || suppression.process_edit(input, crate::app_settings::edit_hotkey(), delivered);
    if suppress { ptr::null_mut() } else { event }
}

fn send_input(context: &EventTapContext, input: InputEvent) -> bool {
    match context.sender.try_send(input) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) => {
            context.missed_event.store(true, Ordering::Release);
            false
        }
        Err(TrySendError::Disconnected(_)) => {
            // SAFETY: The callback runs on the event-tap thread's run loop.
            unsafe { CFRunLoopStop(CFRunLoopGetCurrent()) };
            false
        }
    }
}

#[derive(Default)]
struct ShortcutSuppression {
    paste_key_pressed: bool,
    dictation_key_pressed: Option<u16>,
    escape_pressed: bool,
}

impl ShortcutSuppression {
    fn reset(&mut self) {
        self.paste_key_pressed = false;
        self.dictation_key_pressed = None;
        self.escape_pressed = false;
    }

    fn process(
        &mut self,
        input: InputEvent,
        paste_key_code: u16,
        dictation_hotkey: RuntimeHotkey,
        delivered: bool,
    ) -> bool {
        self.process_shortcut(input, paste_key_code, dictation_hotkey, delivered, true)
    }

    fn process_edit(
        &mut self,
        input: InputEvent,
        edit_hotkey: RuntimeHotkey,
        delivered: bool,
    ) -> bool {
        self.process_shortcut(input, 0, edit_hotkey, delivered, false)
    }

    fn process_shortcut(
        &mut self,
        input: InputEvent,
        paste_key_code: u16,
        hotkey: RuntimeHotkey,
        delivered: bool,
        paste_enabled: bool,
    ) -> bool {
        match input {
            InputEvent::Key {
                code, down: true, ..
            } if paste_enabled && delivered && paste_action(input, paste_key_code).is_some() => {
                self.paste_key_pressed = code == paste_key_code;
                true
            }
            InputEvent::Key {
                code,
                down: true,
                flags,
            } if delivered && hotkey.matches_key_press(code, flags) => {
                self.dictation_key_pressed = Some(code);
                true
            }
            InputEvent::Key {
                code, down: false, ..
            } => {
                let paste = code == paste_key_code && std::mem::take(&mut self.paste_key_pressed);
                let dictation = self.dictation_key_pressed == Some(code);
                if dictation {
                    self.dictation_key_pressed = None;
                }
                paste || dictation
            }
            _ => false,
        }
    }

    fn process_escape(&mut self, input: InputEvent, delivered: bool, escape_cancels: bool) -> bool {
        match input {
            InputEvent::Key {
                code: ESCAPE_KEY_CODE,
                down: true,
                ..
            } if delivered && escape_cancels => {
                self.escape_pressed = true;
                true
            }
            InputEvent::Key {
                code: ESCAPE_KEY_CODE,
                down: false,
                ..
            } if std::mem::take(&mut self.escape_pressed) => true,
            _ => false,
        }
    }
}

fn paste_action(input: InputEvent, paste_key_code: u16) -> Option<HotkeyAction> {
    paste_action_with_meetings(input, paste_key_code, crate::DEVELOPER_FEATURES_ENABLED)
}

fn paste_action_with_meetings(
    input: InputEvent,
    paste_key_code: u16,
    meetings_enabled: bool,
) -> Option<HotkeyAction> {
    let InputEvent::Key {
        code,
        down: true,
        flags,
    } = input
    else {
        return None;
    };
    if code != paste_key_code {
        return None;
    }
    match flags & DEVICE_INDEPENDENT_MODIFIERS_MASK {
        flags if flags == OPTION_KEY_MASK | SHIFT_KEY_MASK => Some(HotkeyAction::PasteLast),
        flags if meetings_enabled && flags == OPTION_KEY_MASK | CONTROL_KEY_MASK => {
            Some(HotkeyAction::PasteMeeting)
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeyAction {
    Start,
    Finish,
    Discard,
    Cancel,
    PasteLast,
    PasteMeeting,
}

#[derive(Debug)]
enum State {
    Idle,
    Recording {
        started_at: Instant,
        previous_release: Option<Instant>,
    },
    Locked,
    Dirty,
}

pub struct DictationHotkey {
    state: State,
    pressed_keys: HashSet<u16>,
    paste_key_code: u16,
    double_tap_enabled: bool,
    last_release_at: Option<Instant>,
    binding: RuntimeHotkey,
    paste_actions_enabled: bool,
}

impl DictationHotkey {
    pub fn new(
        now: Instant,
        double_tap_enabled: bool,
        paste_key_code: u16,
        binding: RuntimeHotkey,
    ) -> Self {
        Self::with_binding(
            trigger_is_physically_down(binding, true),
            now,
            paste_key_code,
            double_tap_enabled,
            binding,
        )
    }

    pub fn new_without_paste(now: Instant, paste_key_code: u16, binding: RuntimeHotkey) -> Self {
        let mut hotkey = Self::with_binding(
            trigger_is_physically_down(binding, true),
            now,
            paste_key_code,
            false,
            binding,
        );
        hotkey.paste_actions_enabled = false;
        hotkey
    }

    fn with_binding(
        trigger_down: bool,
        now: Instant,
        paste_key_code: u16,
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
                State::Idle
            },
            pressed_keys: HashSet::new(),
            paste_key_code,
            double_tap_enabled,
            last_release_at: None,
            binding,
            paste_actions_enabled: true,
        }
    }

    pub fn is_recording(&self) -> bool {
        matches!(self.state, State::Recording { .. } | State::Locked)
    }

    pub fn suppresses_recognition(&self) -> bool {
        !matches!(self.state, State::Idle)
    }

    pub fn set_double_tap_enabled(&mut self, enabled: bool) {
        self.double_tap_enabled = enabled;
        if !enabled {
            self.last_release_at = None;
        }
    }

    pub fn set_binding(&mut self, binding: RuntimeHotkey) {
        if matches!(self.state, State::Idle) && self.binding != binding {
            self.binding = binding;
            self.last_release_at = None;
        }
    }

    pub fn suspend(&mut self) -> Option<HotkeyAction> {
        let was_recording = self.is_recording();
        self.state = State::Idle;
        self.pressed_keys.clear();
        self.last_release_at = None;
        was_recording.then_some(HotkeyAction::Cancel)
    }

    pub fn process(&mut self, event: InputEvent, now: Instant) -> Option<HotkeyAction> {
        let fresh_key_down = match event {
            InputEvent::Key { code, down, .. } => {
                if down {
                    self.pressed_keys.insert(code)
                } else {
                    self.pressed_keys.remove(&code);
                    false
                }
            }
            InputEvent::Flags(_) | InputEvent::MouseDown | InputEvent::TapDisabled => false,
        };

        if matches!(event, InputEvent::TapDisabled) {
            let was_recording = self.is_recording();
            self.state = State::Dirty;
            self.last_release_at = None;
            return was_recording.then_some(HotkeyAction::Cancel);
        }
        if self.paste_actions_enabled
            && fresh_key_down
            && let Some(action) = paste_action(event, self.paste_key_code)
        {
            self.state = State::Dirty;
            self.last_release_at = None;
            return Some(action);
        }
        if matches!(
            event,
            InputEvent::Key {
                code: ESCAPE_KEY_CODE,
                down: true,
                ..
            }
        ) && self.is_recording()
        {
            self.state = State::Dirty;
            self.last_release_at = None;
            return Some(HotkeyAction::Cancel);
        }

        let trigger_pressed = self.trigger_pressed(event, fresh_key_down);
        let flags = match event {
            InputEvent::Flags(flags) | InputEvent::Key { flags, .. } => Some(flags),
            InputEvent::MouseDown | InputEvent::TapDisabled => None,
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
            } if self.binding.key_code != Some(code)
        );
        let extra_modifiers = flags.is_some_and(|flags| {
            self.binding.required_modifiers_down(flags) && !self.binding.exact_modifiers(flags)
        });

        match self.state {
            State::Locked if trigger_pressed => {
                self.state = State::Dirty;
                self.last_release_at = None;
                Some(HotkeyAction::Finish)
            }
            State::Idle if trigger_pressed => {
                let previous_release = self.last_release_at.take().filter(|released| {
                    self.double_tap_enabled && now.duration_since(*released) < DOUBLE_TAP_WINDOW
                });
                self.state = State::Recording {
                    started_at: now,
                    previous_release,
                };
                Some(HotkeyAction::Start)
            }
            State::Recording {
                previous_release: Some(released),
                ..
            } if trigger_released && now.duration_since(released) < DOUBLE_TAP_WINDOW => {
                self.state = State::Locked;
                None
            }
            State::Recording { .. } if trigger_released => {
                self.state = State::Idle;
                self.last_release_at = self.double_tap_enabled.then_some(now);
                Some(HotkeyAction::Finish)
            }
            State::Recording { started_at, .. }
                if now.duration_since(started_at) < MINIMUM_HOLD_DURATION
                    && (unrelated_key_down
                        || matches!(event, InputEvent::MouseDown)
                        || extra_modifiers) =>
            {
                self.state = State::Dirty;
                self.last_release_at = None;
                Some(HotkeyAction::Discard)
            }
            State::Dirty
                if flags.is_some_and(|flags| flags & DEVICE_INDEPENDENT_MODIFIERS_MASK == 0)
                    && self.pressed_keys.is_empty() =>
            {
                self.state = State::Idle;
                None
            }
            _ => None,
        }
    }

    pub fn reconcile(&mut self, missed_input_event: bool) -> Option<HotkeyAction> {
        if !missed_input_event {
            return None;
        }
        self.reconcile_with_trigger_state(trigger_is_physically_down(self.binding, false))
    }

    fn reconcile_with_trigger_state(&mut self, trigger_down: bool) -> Option<HotkeyAction> {
        match self.state {
            State::Recording {
                previous_release: Some(released),
                ..
            } if !trigger_down && released.elapsed() < DOUBLE_TAP_WINDOW => {
                self.state = State::Locked;
                None
            }
            State::Recording { .. } if !trigger_down => {
                self.state = State::Idle;
                self.last_release_at = self.double_tap_enabled.then_some(Instant::now());
                Some(HotkeyAction::Finish)
            }
            _ => None,
        }
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
                    && self.binding.modifiers != 0
                    && self.pressed_keys.is_empty()
            }
        }
    }
}

fn trigger_is_physically_down(binding: RuntimeHotkey, exact_modifiers: bool) -> bool {
    // HID state reflects the physical keyboard. Combined-session state can
    // remain latched after synthetic events or application switching.
    // SAFETY: These are pure system queries with a documented state id.
    unsafe {
        !binding.is_empty()
            && if exact_modifiers {
                binding.exact_modifiers(CGEventSourceFlagsState(HID_SYSTEM_STATE))
            } else {
                binding.required_modifiers_down(CGEventSourceFlagsState(HID_SYSTEM_STATE))
            }
            && binding
                .key_code
                .is_none_or(|code| CGEventSourceKeyState(HID_SYSTEM_STATE, code))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_FLAGS: u64 = 0;
    const SHIFT: u64 = 1 << 17;

    fn option_binding() -> RuntimeHotkey {
        RuntimeHotkey {
            modifiers: OPTION_KEY_MASK,
            key_code: None,
        }
    }

    fn test_hotkey(trigger_down: bool, now: Instant) -> DictationHotkey {
        DictationHotkey::with_binding(trigger_down, now, 42, true, option_binding())
    }

    #[test]
    fn option_press_and_release_finishes() {
        let now = Instant::now();
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
        let now = Instant::now();
        let binding = RuntimeHotkey {
            modifiers: SHIFT_KEY_MASK,
            key_code: Some(49),
        };
        let mut hotkey = DictationHotkey::with_binding(false, now, 42, true, binding);
        let key_down = InputEvent::Key {
            code: 49,
            down: true,
            flags: SHIFT_KEY_MASK,
        };

        assert_eq!(hotkey.process(key_down, now), Some(HotkeyAction::Start));
        assert_eq!(
            hotkey.process(
                InputEvent::Key {
                    code: 49,
                    down: false,
                    flags: SHIFT_KEY_MASK,
                },
                now + Duration::from_secs(1),
            ),
            Some(HotkeyAction::Finish)
        );

        let mut suppression = ShortcutSuppression::default();
        assert!(suppression.process(key_down, 42, binding, true));
        assert!(suppression.process(
            InputEvent::Key {
                code: 49,
                down: false,
                flags: SHIFT_KEY_MASK,
            },
            42,
            binding,
            true,
        ));
    }

    #[test]
    fn key_chord_double_tap_locks_until_the_chord_is_pressed_again() {
        let now = Instant::now();
        let binding = RuntimeHotkey {
            modifiers: SHIFT_KEY_MASK,
            key_code: Some(49),
        };
        let mut hotkey = DictationHotkey::with_binding(false, now, 42, true, binding);
        let event = |down| InputEvent::Key {
            code: 49,
            down,
            flags: SHIFT_KEY_MASK,
        };

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
    fn modifier_chord_requires_the_exact_binding_to_start() {
        let now = Instant::now();
        let binding = RuntimeHotkey {
            modifiers: CONTROL_KEY_MASK | SHIFT_KEY_MASK,
            key_code: None,
        };
        let mut hotkey = DictationHotkey::with_binding(false, now, 42, true, binding);

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
    fn option_command_edit_takes_over_from_option_dictation() {
        let now = Instant::now();
        let mut dictation = test_hotkey(false, now);
        let edit_binding = RuntimeHotkey {
            modifiers: OPTION_KEY_MASK | (1 << 20),
            key_code: None,
        };
        let mut edit = DictationHotkey::new_without_paste(now, 42, edit_binding);

        let option = InputEvent::Flags(OPTION_KEY_MASK);
        assert_eq!(dictation.process(option, now), Some(HotkeyAction::Start));
        assert_eq!(edit.process(option, now), None);

        let chord = InputEvent::Flags(OPTION_KEY_MASK | (1 << 20));
        assert_eq!(
            dictation.process(chord, now + Duration::from_millis(40)),
            Some(HotkeyAction::Discard)
        );
        assert_eq!(
            edit.process(chord, now + Duration::from_millis(40)),
            Some(HotkeyAction::Start)
        );
        assert_eq!(
            edit.process(InputEvent::Flags(NO_FLAGS), now + Duration::from_secs(1)),
            Some(HotkeyAction::Finish)
        );
    }

    #[test]
    fn option_double_tap_locks_until_option_is_pressed_again() {
        let now = Instant::now();
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
    fn disabling_double_tap_keeps_both_taps_as_press_and_hold() {
        let now = Instant::now();
        let mut hotkey = DictationHotkey::with_binding(false, now, 42, false, option_binding());

        hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now);
        hotkey.process(InputEvent::Flags(NO_FLAGS), now + Duration::from_millis(80));
        hotkey.process(
            InputEvent::Flags(OPTION_KEY_MASK),
            now + Duration::from_millis(180),
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(NO_FLAGS),
                now + Duration::from_millis(260)
            ),
            Some(HotkeyAction::Finish)
        );
        assert!(!hotkey.is_recording());
    }

    #[test]
    fn a_slow_second_release_does_not_lock() {
        let now = Instant::now();
        let mut hotkey = test_hotkey(false, now);

        hotkey.process(InputEvent::Flags(OPTION_KEY_MASK), now);
        hotkey.process(InputEvent::Flags(NO_FLAGS), now + Duration::from_millis(80));
        hotkey.process(
            InputEvent::Flags(OPTION_KEY_MASK),
            now + Duration::from_millis(180),
        );
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(NO_FLAGS),
                now + Duration::from_millis(450)
            ),
            Some(HotkeyAction::Finish)
        );
        assert!(!hotkey.is_recording());
    }

    #[test]
    fn early_chord_discards_until_everything_is_released() {
        let now = Instant::now();
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
            Some(HotkeyAction::Discard)
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

    #[test]
    fn escape_cancels_and_watchdog_finishes_missed_release() {
        let now = Instant::now();
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

        let mut hotkey = test_hotkey(true, now);
        assert_eq!(
            hotkey.reconcile_with_trigger_state(false),
            Some(HotkeyAction::Finish)
        );
    }

    #[test]
    fn healthy_input_delivery_never_reconciles_an_active_capture() {
        let now = Instant::now();
        let mut hotkey = test_hotkey(true, now);

        assert_eq!(hotkey.reconcile(false), None);
        assert!(hotkey.is_recording());
    }

    #[test]
    fn dropped_event_recovery_accepts_extra_held_modifiers() {
        let now = Instant::now();
        let mut hotkey = test_hotkey(true, now);

        assert_eq!(hotkey.reconcile_with_trigger_state(true), None);
        assert!(hotkey.is_recording());
    }

    #[test]
    fn escape_is_suppressed_while_it_controls_dictation() {
        let mut suppression = ShortcutSuppression::default();
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

        assert!(!suppression.process_escape(down, true, false));
        assert!(suppression.process_escape(down, true, true));
        assert!(suppression.process_escape(up, true, false));
    }

    #[test]
    fn extra_modifier_after_threshold_is_ignored() {
        let now = Instant::now();
        let mut hotkey = test_hotkey(true, now);
        assert_eq!(
            hotkey.process(
                InputEvent::Flags(OPTION_KEY_MASK | SHIFT),
                now + Duration::from_millis(500),
            ),
            None
        );
        assert!(hotkey.is_recording());
    }

    #[test]
    fn option_shift_v_pastes_last_transcript() {
        let now = Instant::now();
        let mut hotkey = test_hotkey(true, now);
        let paste_key_code = hotkey.paste_key_code;
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
    fn option_control_v_pastes_new_meeting_transcript() {
        let now = Instant::now();
        let mut hotkey = test_hotkey(true, now);
        let paste_key_code = hotkey.paste_key_code;
        assert_eq!(
            hotkey.process(
                InputEvent::Key {
                    code: paste_key_code,
                    down: true,
                    flags: OPTION_KEY_MASK | CONTROL_KEY_MASK,
                },
                now + Duration::from_millis(50),
            ),
            Some(HotkeyAction::PasteMeeting)
        );
        assert!(!hotkey.is_recording());
        assert_eq!(
            paste_action(
                InputEvent::Key {
                    code: paste_key_code,
                    down: true,
                    flags: OPTION_KEY_MASK | SHIFT_KEY_MASK,
                },
                paste_key_code,
            ),
            Some(HotkeyAction::PasteLast)
        );
        assert_eq!(
            paste_action(
                InputEvent::Key {
                    code: paste_key_code,
                    down: true,
                    flags: OPTION_KEY_MASK,
                },
                paste_key_code,
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
}

use std::collections::HashSet;
use std::ffi::c_void;
use std::ptr;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};

const MINIMUM_HOLD: Duration = Duration::from_millis(300);
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
const LISTEN_ONLY: u32 = 1;
const OPTION_KEY_MASK: u64 = 1 << 19;
const SHIFT_KEY_MASK: u64 = 1 << 17;
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
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopRun();
    static kCFRunLoopCommonModes: *const c_void;
}

#[derive(Clone, Copy, Debug)]
pub enum InputEvent {
    Flags(u64),
    Key { code: u16, down: bool, flags: u64 },
    MouseDown,
    TapDisabled,
}

pub struct InputMonitor {
    pub events: Receiver<InputEvent>,
}

impl InputMonitor {
    pub fn start() -> Result<Self> {
        let (sender, events) = mpsc::sync_channel(64);
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        thread::spawn(move || run_event_tap(sender, ready_sender));
        ready_receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| eyre!("timed out starting the keyboard event tap"))??;
        Ok(Self { events })
    }
}

fn run_event_tap(sender: SyncSender<InputEvent>, ready: SyncSender<Result<()>>) {
    let context = Box::into_raw(Box::new(sender));
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
    // SAFETY: The callback context remains allocated for the process lifetime,
    // and the dedicated thread owns the tap and its run loop.
    let tap = unsafe {
        CGEventTapCreate(
            HID_EVENT_TAP,
            HEAD_INSERT_EVENT_TAP,
            LISTEN_ONLY,
            mask,
            event_callback,
            context.cast(),
        )
    };
    if tap.is_null() {
        // SAFETY: No callback can run because tap creation failed.
        unsafe { drop(Box::from_raw(context)) };
        let _ = ready.send(Err(eyre!(
            "could not create keyboard event tap; grant Input Monitoring permission"
        )));
        return;
    }
    // SAFETY: `tap` is a valid CFMachPort returned above.
    let source = unsafe { CFMachPortCreateRunLoopSource(ptr::null(), tap, 0) };
    if source.is_null() {
        let _ = ready.send(Err(eyre!("could not create keyboard run-loop source")));
        return;
    }
    // SAFETY: All values belong to this thread and remain alive while the run loop runs.
    unsafe {
        CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
    }
    let _ = ready.send(Ok(()));
    // SAFETY: This dedicated thread exists solely to dispatch the event tap.
    unsafe { CFRunLoopRun() };
}

unsafe extern "C" fn event_callback(
    proxy: *mut c_void,
    event_type: u32,
    event: EventRef,
    user_info: *mut c_void,
) -> EventRef {
    if event.is_null() || user_info.is_null() {
        return event;
    }
    // SAFETY: CoreGraphics supplied a valid event to this callback.
    if unsafe { CGEventGetIntegerValueField(event, EVENT_SOURCE_USER_DATA) }
        == crate::keyboard::SYNTHETIC_EVENT_MARKER
    {
        return event;
    }
    // SAFETY: `user_info` points to the sender allocated by `run_event_tap`.
    let sender = unsafe { &*(user_info.cast::<SyncSender<InputEvent>>()) };
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
        EVENT_TAP_DISABLED_BY_TIMEOUT | EVENT_TAP_DISABLED_BY_USER_INPUT => {
            if !proxy.is_null() {
                // SAFETY: CoreGraphics supplied the event-tap proxy for this callback.
                unsafe { CGEventTapEnable(proxy, true) };
            }
            InputEvent::TapDisabled
        }
        _ => return event,
    };
    let _ = sender.try_send(input);
    event
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeyAction {
    Start,
    Finish,
    Discard,
    Cancel,
    PasteLast,
}

#[derive(Debug)]
enum State {
    Idle,
    Recording { started_at: Instant },
    Dirty,
}

pub struct OptionHotkey {
    state: State,
    pressed_keys: HashSet<u16>,
    paste_key_code: u16,
}

impl OptionHotkey {
    pub fn new(option_down: bool, now: Instant) -> Self {
        let paste_key_code = crate::keyboard::key_code_for('v').unwrap_or(9);
        tracing::info!(
            paste_key_code,
            "resolved paste key for active keyboard layout"
        );
        Self::with_paste_key_code(option_down, now, paste_key_code)
    }

    fn with_paste_key_code(option_down: bool, now: Instant, paste_key_code: u16) -> Self {
        Self {
            state: if option_down {
                State::Recording { started_at: now }
            } else {
                State::Idle
            },
            pressed_keys: HashSet::new(),
            paste_key_code,
        }
    }

    pub fn is_recording(&self) -> bool {
        matches!(self.state, State::Recording { .. })
    }

    pub fn process(&mut self, event: InputEvent, now: Instant) -> Option<HotkeyAction> {
        match event {
            InputEvent::Key { code, down, .. } => {
                if down {
                    self.pressed_keys.insert(code);
                } else {
                    self.pressed_keys.remove(&code);
                }
            }
            InputEvent::Flags(_) | InputEvent::MouseDown | InputEvent::TapDisabled => {}
        }

        if matches!(event, InputEvent::TapDisabled) {
            return self.is_recording().then_some(HotkeyAction::Cancel);
        }
        if matches!(
            event,
            InputEvent::Key {
                code,
                down: true,
                flags,
            } if code == self.paste_key_code
                && flags & DEVICE_INDEPENDENT_MODIFIERS_MASK
                == OPTION_KEY_MASK | SHIFT_KEY_MASK
        ) {
            self.state = State::Dirty;
            return Some(HotkeyAction::PasteLast);
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
            return Some(HotkeyAction::Cancel);
        }

        let flags = match event {
            InputEvent::Flags(flags) | InputEvent::Key { flags, .. } => Some(flags),
            InputEvent::MouseDown | InputEvent::TapDisabled => None,
        };
        let option_down = flags.is_some_and(|flags| flags & OPTION_KEY_MASK != 0);
        let exact_option =
            flags.is_some_and(|flags| flags & DEVICE_INDEPENDENT_MODIFIERS_MASK == OPTION_KEY_MASK);

        match self.state {
            State::Idle if exact_option && self.pressed_keys.is_empty() => {
                self.state = State::Recording { started_at: now };
                Some(HotkeyAction::Start)
            }
            State::Recording { .. } if flags.is_some() && !option_down => {
                self.state = State::Idle;
                Some(HotkeyAction::Finish)
            }
            State::Recording { started_at }
                if now.duration_since(started_at) < MINIMUM_HOLD
                    && (matches!(
                        event,
                        InputEvent::Key { down: true, .. } | InputEvent::MouseDown
                    ) || (flags.is_some() && option_down && !exact_option)) =>
            {
                self.state = State::Dirty;
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

    pub fn reconcile(&mut self, option_down: bool) -> Option<HotkeyAction> {
        if self.is_recording() && !option_down {
            self.state = State::Idle;
            Some(HotkeyAction::Finish)
        } else {
            None
        }
    }
}

pub fn option_key_is_down() -> bool {
    // HID state reflects the physical keyboard. Combined-session state can
    // remain latched after synthetic events or application switching.
    // SAFETY: This is a pure system query with a documented state id.
    unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) & OPTION_KEY_MASK != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_FLAGS: u64 = 0;
    const SHIFT: u64 = 1 << 17;

    fn test_hotkey(option_down: bool, now: Instant) -> OptionHotkey {
        OptionHotkey::with_paste_key_code(option_down, now, 42)
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
        assert_eq!(hotkey.reconcile(false), Some(HotkeyAction::Finish));
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
        assert_eq!(
            hotkey.process(
                InputEvent::Key {
                    code: paste_key_code,
                    down: true,
                    flags: OPTION_KEY_MASK | SHIFT_KEY_MASK,
                },
                now + Duration::from_millis(50),
            ),
            Some(HotkeyAction::PasteLast)
        );
        assert!(!hotkey.is_recording());
    }
}

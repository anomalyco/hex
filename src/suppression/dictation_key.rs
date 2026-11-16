//! Labs › Microphone key alone: the laptop's microphone (dictation) key, taken
//! at the session tap.
//!
//! macOS sends the key as a normal keyDown/keyUp for virtual key code 176 with
//! SecondaryFn set, but its own Dictation shortcut is a symbolic hot key the
//! window server matches before the app and before Hex's annotated-session key
//! tap. This tap sits at the session boundary, where the press still exists,
//! and exists only while the lab is on.
//!
//! It passes the key through untouched unless it is one of Hex's shortcuts
//! (`RuntimeHotkey::matches_key_press`, so modifiers count) or the window is
//! recording a shortcut. Then it consumes the press *and its release* while
//! this tap is still there, and hands the press to the listener or to the
//! recorder. The app never sees a press Hex took, and no second tap can deliver
//! it twice. Whether macOS's own Dictation still opens is unobserved on
//! hardware; blocking the key here is what handy-keys does to keep it shut.

use super::*;

/// A press the tap took while the window was recording a shortcut. The window
/// drains these through [`take_captured`] and hands them to the same
/// `ChordCapture` its own key events use, so one press records one chord.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CapturedMicrophoneKey {
    pub(crate) code: u16,
    pub(crate) repeat: bool,
    pub(crate) flags: u64,
}

/// Presses the tap took for the recorder. The window drains them on every
/// poll, and the cap only keeps a window that stopped polling from growing it.
const CAPTURED_LIMIT: usize = 8;

/// The presses the tap took for the window's recorder, drained by
/// [`take_captured`].
static CAPTURED: Mutex<VecDeque<CapturedMicrophoneKey>> = Mutex::new(VecDeque::new());

/// The window's wake: the tap runs on its own thread, so a press it takes
/// must ask the window for a frame or it would wait for the next one.
type Wake = Arc<dyn Fn() + Send + Sync>;

/// The recorder watching, and the id that tells it from a replacement.
struct Watching {
    id: u64,
    wake: Wake,
}

static CAPTURE_WAKE: Mutex<Option<Watching>> = Mutex::new(None);
static NEXT_WAKE: AtomicU64 = AtomicU64::new(0);

/// Watch the microphone key for the window's recorder, waking the window for
/// each press it takes. Returns the id [`stop_watching`] takes.
pub(crate) fn watch(wake: Wake) -> u64 {
    let id = NEXT_WAKE.fetch_add(1, Ordering::Relaxed) + 1;
    *CAPTURE_WAKE
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(Watching { id, wake });
    id
}

/// Stop watching, unless a newer recorder already took over; a replaced
/// recorder must not silence its successor.
pub(crate) fn stop_watching(id: u64) {
    let mut slot = CAPTURE_WAKE
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if slot.as_ref().is_some_and(|watching| watching.id == id) {
        *slot = None;
    }
}

/// Forget presses a recorder that stopped polling never read.
pub(crate) fn clear_captured() {
    CAPTURED
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();
}

/// Every press the tap took for the recorder since the window last polled.
pub(crate) fn take_captured() -> Vec<CapturedMicrophoneKey> {
    let mut captured = CAPTURED.lock().unwrap_or_else(|error| error.into_inner());
    let mut keys = Vec::with_capacity(captured.len());
    while let Some(key) = captured.pop_front() {
        keys.push(key);
    }
    keys
}

fn capture(key: CapturedMicrophoneKey) {
    let wake = {
        let mut captured = CAPTURED.lock().unwrap_or_else(|error| error.into_inner());
        if captured.len() >= CAPTURED_LIMIT {
            captured.pop_front();
        }
        captured.push_back(key);
        CAPTURE_WAKE
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .map(|watching| watching.wake.clone())
    };
    // Outside the locks: the window's wake must never run under them.
    if let Some(wake) = wake {
        wake();
    }
}

/// Whether one of Hex's shortcuts matches this press, with the same matcher
/// and the same flags the annotated-session tap uses. The cancel shortcut is
/// not among `RuntimeHotkeys::all`, so it is read beside them like the
/// listener's own suppression does.
fn is_hexs_press(context: &EventTapContext, code: u16, flags: u64) -> bool {
    let mut hotkeys = crate::app_settings::runtime_hotkeys();
    hotkeys.cancel = *context
        .cancel
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    hotkeys
        .all()
        .iter()
        .flatten()
        .chain(hotkeys.cancel.iter())
        .any(|hotkey| hotkey.matches_key_press(code, flags))
}

pub(super) unsafe extern "C" fn dictation_key_callback(
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
    // Disable notifications first, like the annotated tap: no real events run
    // while macOS holds the tap down, so recovery cannot wait on an event.
    if matches!(
        event_type,
        EVENT_TAP_DISABLED_BY_TIMEOUT | EVENT_TAP_DISABLED_BY_USER_INPUT
    ) {
        reenable_disabled_taps(context, event_type, TapKind::DictationKey.as_str());
        // Events were missed while it was off: a press this tap took may never
        // see its release.
        context
            .microphone_key
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
        send_input(context, InputEvent::TapDisabled, CaptureInstant::ZERO);
        return event;
    }
    if event.is_null() {
        return event;
    }
    if !matches!(event_type, EVENT_KEY_DOWN | EVENT_KEY_UP) {
        return event;
    }
    // SAFETY: CoreGraphics supplied a valid event to this callback.
    if unsafe { CGEventGetIntegerValueField(event, EVENT_SOURCE_USER_DATA) }
        == crate::keyboard::SYNTHETIC_EVENT_MARKER
    {
        return event;
    }
    // SAFETY: CoreGraphics supplied a valid keyboard event.
    let code = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) as u16 };
    // The tap watches every key, but only this one is ever its business.
    if code != crate::app_settings::MICROPHONE_KEY_CODE {
        return event;
    }
    // SAFETY: CoreGraphics supplied a valid event.
    let flags = crate::app_settings::normalized_modifier_flags(
        unsafe { CGEventGetFlags(event) },
        physical_keys::snapshot(),
    );
    let down = event_type == EVENT_KEY_DOWN;
    let decision = context
        .microphone_key
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take(
            code,
            down,
            crate::labs::microphone_key(),
            crate::shortcut_pause::suspended(),
            crate::app_settings::hotkey_capture_active(),
            is_hexs_press(context, code, flags),
        );
    match decision {
        crate::app_settings::MicrophoneKeyTap::Pass => event,
        // SAFETY: CoreGraphics supplied a valid event.
        crate::app_settings::MicrophoneKeyTap::Listener { down } => {
            // SAFETY: CoreGraphics supplied a valid event.
            let raw = unsafe { CGEventGetTimestamp(event) };
            let (at, _) = physical_event_instant(raw, CaptureInstant::now());
            if send_input(context, InputEvent::Key { code, down, flags }, at) {
                ptr::null_mut()
            } else {
                // The listener is gone, so the press is not Hex's after all:
                // forget it and let the event, and its release, through.
                context
                    .microphone_key
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .clear();
                event
            }
        }
        crate::app_settings::MicrophoneKeyTap::Recorder { down } => {
            if down {
                // SAFETY: CoreGraphics supplied a valid keyboard event.
                let repeat =
                    unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_AUTOREPEAT) } != 0;
                capture(CapturedMicrophoneKey {
                    code,
                    repeat,
                    flags,
                });
            }
            ptr::null_mut()
        }
    }
}

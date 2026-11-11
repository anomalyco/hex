//! Hex's key shortcuts while another app holds Secure Event Input.
//!
//! A password field, a terminal's Secure Keyboard Entry, or an app that
//! never turned it off stops macOS from handing key presses to event taps,
//! Hex's included; modifier changes still reach them. Carbon hotkeys
//! (`RegisterEventHotKey`) still fire then, so while secure input is on the
//! key shortcuts that start something (Annotate and its replay, Paste last,
//! Paste meeting, and the Screenshots shortcuts) are registered as Carbon
//! hotkeys, and each press and release goes where the tap would have sent
//! it: a `Key` event into the listener's input, or `screenshots::key`.
//! Only while secure input is on: a Carbon hotkey takes its key before the
//! session taps, so otherwise the tap keeps its shortcuts and chord rules.
//!
//! Carbon can't tell a modifier's sides apart or hold Fn, so a binding
//! needing Fn isn't registered, and a sided binding matches either side.
//! Escape and plain typing still never arrive while secure input is on.

use std::cell::RefCell;
use std::sync::Weak;

use objc2_foundation::{MainThreadMarker, NSTimer};

use super::*;
use crate::app_settings::ModifierSide;

const POLL_SECONDS: f64 = 0.25;
const SIGNATURE: u32 = u32::from_be_bytes(*b"Hex!");
const EVENT_CLASS_KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
const EVENT_HOT_KEY_PRESSED: u32 = 5;
const EVENT_HOT_KEY_RELEASED: u32 = 6;
const EVENT_PARAM_DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");
const TYPE_EVENT_HOT_KEY_ID: u32 = u32::from_be_bytes(*b"hkid");
const CARBON_COMMAND: u32 = 1 << 8;
const CARBON_SHIFT: u32 = 1 << 9;
const CARBON_OPTION: u32 = 1 << 11;
const CARBON_CONTROL: u32 = 1 << 12;
/// Virtual key codes past this are Hex's mouse buttons or the Globe key.
const LAST_KEYBOARD_CODE: u16 = 127;

#[repr(C)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

type EventHandler = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: EventHandler,
        count: usize,
        types: *const EventTypeSpec,
        user_data: *mut c_void,
        handler_ref: *mut *mut c_void,
    ) -> i32;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        id: EventHotKeyId,
        target: *mut c_void,
        options: u32,
        hot_key: *mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(hot_key: *mut c_void) -> i32;
    fn GetEventKind(event: *mut c_void) -> u32;
    fn GetEventParameter(
        event: *mut c_void,
        name: u32,
        desired_type: u32,
        actual_type: *mut u32,
        size: usize,
        actual_size: *mut usize,
        data: *mut c_void,
    ) -> i32;
}

/// The listener's input while an input monitor runs.
static SINK: Mutex<Weak<InputSink>> = Mutex::new(Weak::new());

/// Points the secure-input hotkeys at a new input monitor's channel.
pub(super) fn attach(sink: &Arc<InputSink>) {
    *SINK.lock().unwrap_or_else(|error| error.into_inner()) = Arc::downgrade(sink);
}

fn sink() -> Option<Arc<InputSink>> {
    SINK.lock()
        .unwrap_or_else(|error| error.into_inner())
        .upgrade()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Target {
    Capture,
    CaptureReplay,
    PasteLast,
    PasteMeeting,
    #[cfg_attr(not(hex_capture), allow(dead_code))]
    Screenshot,
    #[cfg_attr(not(hex_capture), allow(dead_code))]
    ScreenshotRecord,
}

impl Target {
    fn name(self) -> &'static str {
        match self {
            Self::Capture => "annotate",
            Self::CaptureReplay => "annotate-replay",
            Self::PasteLast => "paste-last",
            Self::PasteMeeting => "paste-meeting",
            Self::Screenshot => "screenshot",
            Self::ScreenshotRecord => "screenshot-record",
        }
    }
}

/// One shortcut as a Carbon hotkey.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Chord {
    target: Target,
    binding: RuntimeHotkey,
    code: u16,
    modifiers: u32,
}

/// The Carbon key code and modifiers for `binding`, when Carbon can hold
/// it: a keyboard key, without Fn.
fn carbon_chord(binding: RuntimeHotkey) -> Option<(u16, u32)> {
    let code = binding
        .key_code
        .filter(|code| *code <= LAST_KEYBOARD_CODE)?;
    let modifiers = binding.modifiers;
    if modifiers.function {
        return None;
    }
    let mut carbon = 0;
    for (side, bit) in [
        (modifiers.command, CARBON_COMMAND),
        (modifiers.shift, CARBON_SHIFT),
        (modifiers.option, CARBON_OPTION),
        (modifiers.control, CARBON_CONTROL),
    ] {
        if side.is_some() {
            carbon |= bit;
        }
    }
    Some((code, carbon))
}

/// The shortcuts to register while secure input is on, the first of any
/// two on the same chord kept.
fn chords(hotkeys: RuntimeHotkeys, screenshots: [Option<RuntimeHotkey>; 2]) -> Vec<Chord> {
    let [screenshot, screenshot_record] = screenshots;
    let candidates = [
        (Target::Capture, hotkeys.capture),
        (Target::CaptureReplay, hotkeys.capture_replay),
        (Target::PasteLast, hotkeys.paste_last),
        (Target::PasteMeeting, hotkeys.paste_meeting),
        (Target::Screenshot, screenshot),
        (Target::ScreenshotRecord, screenshot_record),
    ];
    let mut chords = Vec::<Chord>::new();
    for (target, binding) in candidates {
        let Some(binding) = binding else { continue };
        let Some((code, modifiers)) = carbon_chord(binding) else {
            continue;
        };
        if !chords
            .iter()
            .any(|chord| chord.code == code && chord.modifiers == modifiers)
        {
            chords.push(Chord {
                target,
                binding,
                code,
                modifiers,
            });
        }
    }
    chords
}

/// Device flags for exactly `binding`'s modifiers, sides included.
fn binding_flags(binding: RuntimeHotkey) -> u64 {
    use crate::app_settings::{
        COMMAND_KEY_MASK, CONTROL_KEY_MASK, LEFT_COMMAND_MASK, LEFT_CONTROL_MASK, LEFT_OPTION_MASK,
        LEFT_SHIFT_MASK, OPTION_KEY_MASK, RIGHT_COMMAND_MASK, RIGHT_CONTROL_MASK,
        RIGHT_OPTION_MASK, RIGHT_SHIFT_MASK, SHIFT_KEY_MASK,
    };
    let modifiers = binding.modifiers;
    [
        (
            modifiers.control,
            CONTROL_KEY_MASK,
            LEFT_CONTROL_MASK,
            RIGHT_CONTROL_MASK,
        ),
        (
            modifiers.option,
            OPTION_KEY_MASK,
            LEFT_OPTION_MASK,
            RIGHT_OPTION_MASK,
        ),
        (
            modifiers.shift,
            SHIFT_KEY_MASK,
            LEFT_SHIFT_MASK,
            RIGHT_SHIFT_MASK,
        ),
        (
            modifiers.command,
            COMMAND_KEY_MASK,
            LEFT_COMMAND_MASK,
            RIGHT_COMMAND_MASK,
        ),
    ]
    .into_iter()
    .fold(0, |flags, (side, general, left, right)| {
        flags
            | match side {
                None => 0,
                Some(ModifierSide::Either) => general,
                Some(ModifierSide::Left) => general | left,
                Some(ModifierSide::Right) => general | right,
                Some(ModifierSide::Both) => general | left | right,
            }
    })
}

/// The flags a press carries: the keyboard's own when they match the
/// binding, else the binding's (Carbon already matched the chord, but
/// can't see sides).
fn press_flags(chord: Chord, current: u64) -> u64 {
    if chord.binding.matches_key_press(chord.code, current) {
        current
    } else {
        binding_flags(chord.binding)
    }
}

/// What reaches the listener for one Carbon edge: the key, then, when its
/// modifiers already came up (the tap may have handed the listener that
/// change before this press), the current modifiers again, so an armed
/// Annotate press still records.
fn listener_events(chord: Chord, down: bool, current: u64) -> Vec<InputEvent> {
    if !down {
        return vec![InputEvent::Key {
            code: chord.code,
            down,
            flags: current,
        }];
    }
    let mut events = vec![InputEvent::Key {
        code: chord.code,
        down,
        flags: press_flags(chord, current),
    }];
    if !chord.binding.modifiers.is_empty() && !chord.binding.required_modifiers_down(current) {
        events.push(InputEvent::Flags(current));
    }
    events
}

/// The Carbon hotkeys held down, so each press has its release: a press
/// whose release never came sends it first, and an unregistered hotkey
/// sends its own.
#[derive(Default)]
struct Held(Vec<Target>);

impl Held {
    /// The edges to send, `true` for down.
    fn pressed(&mut self, target: Target) -> &'static [bool] {
        if self.0.contains(&target) {
            &[false, true]
        } else {
            self.0.push(target);
            &[true]
        }
    }

    fn released(&mut self, target: Target) -> &'static [bool] {
        match self.0.iter().position(|held| *held == target) {
            Some(index) => {
                self.0.remove(index);
                &[false]
            }
            None => &[],
        }
    }
}

struct Registered {
    id: u32,
    chord: Chord,
    hot_key: *mut c_void,
}

#[derive(Default)]
struct State {
    secure: bool,
    registered: Vec<Registered>,
    held: Held,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Set once the hotkey handler is installed, so [`refresh`] never registers
/// hotkeys nothing would hear.
static STARTED: AtomicBool = AtomicBool::new(false);

/// Installs the hotkey handler and starts watching for secure input, on
/// the main thread, whose run loop dispatches Carbon events.
pub(crate) fn start(_mtm: MainThreadMarker) {
    let types = [
        EventTypeSpec {
            event_class: EVENT_CLASS_KEYBOARD,
            event_kind: EVENT_HOT_KEY_PRESSED,
        },
        EventTypeSpec {
            event_class: EVENT_CLASS_KEYBOARD,
            event_kind: EVENT_HOT_KEY_RELEASED,
        },
    ];
    // SAFETY: the main thread's application target; the handler is a
    // static function and the type list is copied by the call.
    let status = unsafe {
        InstallEventHandler(
            GetApplicationEventTarget(),
            on_hot_key,
            types.len(),
            types.as_ptr(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if status != 0 {
        tracing::warn!(
            status,
            "could not install the secure-input shortcut handler"
        );
        return;
    }
    STARTED.store(true, Ordering::Release);
    let block = block2::RcBlock::new(|_: std::ptr::NonNull<NSTimer>| poll());
    // SAFETY: the timer runs its block on this (main) thread's run loop,
    // which keeps it for the life of the process.
    let timer = unsafe {
        NSTimer::scheduledTimerWithTimeInterval_repeats_block(POLL_SECONDS, true, &block)
    };
    timer.setTolerance(POLL_SECONDS / 2.0);
    poll();
}

/// Re-reads secure input and the shortcuts now, on the main thread, so a
/// pause or a resume reaches the Carbon hotkeys without waiting for the next
/// poll. Does nothing before [`start`] installed the handler: a registered
/// hotkey nothing listens for would swallow the key.
pub(crate) fn refresh() {
    if STARTED.load(Ordering::Acquire) {
        poll();
    }
}

fn poll() {
    // SAFETY: a read-only HIToolbox query.
    let secure = unsafe { IsSecureEventInputEnabled() } != 0;
    let sink = sink();
    let wanted = if secure && sink.is_some() && !crate::app_settings::shortcuts_suspended() {
        chords(crate::app_settings::runtime_hotkeys(), screenshot_hotkeys())
    } else {
        Vec::new()
    };
    let mut releases = Vec::new();
    STATE.with_borrow_mut(|state| {
        if state.secure != secure {
            state.secure = secure;
            log_secure(secure);
        }
        let registered: Vec<Chord> = state.registered.iter().map(|r| r.chord).collect();
        if registered == wanted {
            return;
        }
        for Registered { chord, hot_key, .. } in state.registered.drain(..) {
            // SAFETY: a hotkey this module registered and hasn't released.
            unsafe { UnregisterEventHotKey(hot_key) };
            if !state.held.released(chord.target).is_empty() {
                releases.push(chord);
            }
        }
        for (index, chord) in wanted.into_iter().enumerate() {
            let mut hot_key = ptr::null_mut();
            let id = EventHotKeyId {
                signature: SIGNATURE,
                id: index as u32,
            };
            // SAFETY: the application target, and a live out-pointer.
            let status = unsafe {
                RegisterEventHotKey(
                    u32::from(chord.code),
                    chord.modifiers,
                    id,
                    GetApplicationEventTarget(),
                    0,
                    &mut hot_key,
                )
            };
            if status == 0 && !hot_key.is_null() {
                state.registered.push(Registered {
                    id: id.id,
                    chord,
                    hot_key,
                });
            } else {
                tracing::warn!(
                    shortcut = chord.target.name(),
                    status,
                    "could not register a shortcut for secure input; another app may own it"
                );
            }
        }
    });
    for chord in releases {
        deliver(chord, false);
    }
}

#[cfg(hex_capture)]
fn screenshot_hotkeys() -> [Option<RuntimeHotkey>; 2] {
    if crate::labs::screenshots() {
        [
            crate::app_settings::screenshot_hotkey(),
            crate::app_settings::screenshot_record_hotkey(),
        ]
    } else {
        [None, None]
    }
}

#[cfg(not(hex_capture))]
fn screenshot_hotkeys() -> [Option<RuntimeHotkey>; 2] {
    [None, None]
}

unsafe extern "C" fn on_hot_key(
    _call: *mut c_void,
    event: *mut c_void,
    _user_data: *mut c_void,
) -> i32 {
    const EVENT_NOT_HANDLED: i32 = -9874;
    let mut id = EventHotKeyId::default();
    // SAFETY: Carbon handed this handler a live hotkey event; the buffer is
    // an `EventHotKeyID`.
    let (status, kind) = unsafe {
        (
            GetEventParameter(
                event,
                EVENT_PARAM_DIRECT_OBJECT,
                TYPE_EVENT_HOT_KEY_ID,
                ptr::null_mut(),
                std::mem::size_of::<EventHotKeyId>(),
                ptr::null_mut(),
                (&raw mut id).cast(),
            ),
            GetEventKind(event),
        )
    };
    if status != 0 || id.signature != SIGNATURE {
        return EVENT_NOT_HANDLED;
    }
    let down = kind == EVENT_HOT_KEY_PRESSED;
    let edges = STATE.with_borrow_mut(|state| {
        let chord = state
            .registered
            .iter()
            .find(|registered| registered.id == id.id)?
            .chord;
        let edges = if down {
            state.held.pressed(chord.target)
        } else {
            state.held.released(chord.target)
        };
        Some((chord, edges))
    });
    let Some((chord, edges)) = edges else {
        return EVENT_NOT_HANDLED;
    };
    if down {
        tracing::info!(
            shortcut = chord.target.name(),
            "shortcut arrived through secure input"
        );
    }
    for &edge in edges {
        deliver(chord, edge);
    }
    0
}

fn deliver(chord: Chord, down: bool) {
    // SAFETY: a read-only CoreGraphics state query.
    let current = crate::app_settings::normalized_modifier_flags(
        unsafe { CGEventSourceFlagsState(COMBINED_SESSION_STATE) },
        physical_keys::snapshot(),
    );
    match chord.target {
        #[cfg(hex_capture)]
        Target::Screenshot | Target::ScreenshotRecord => {
            crate::screenshots::key(crate::screenshots::Key {
                code: chord.code,
                down,
                repeat: false,
                flags: if down {
                    press_flags(chord, current)
                } else {
                    current
                },
            });
        }
        _ => {
            let Some(sink) = sink() else { return };
            for event in listener_events(chord, down, current) {
                sink.send(event, CaptureInstant::now());
            }
        }
    }
}

fn log_secure(secure: bool) {
    if !secure {
        tracing::info!("secure input is off; key shortcuts are back on the event tap");
        return;
    }
    match frontmost::pid() {
        Some(pid) => tracing::info!(
            frontmost_pid = pid,
            frontmost = %frontmost::name(pid).unwrap_or_default(),
            "secure input is on and blocks key events; key shortcuts arrive as Carbon hotkeys; \
             the app named here was in front when it turned on, which is not necessarily the \
             one holding secure input"
        ),
        None => tracing::info!(
            "secure input is on and blocks key events; key shortcuts arrive as Carbon hotkeys"
        ),
    }
}

/// The app that was in front when secure input turned on:
/// `kCGSSessionSecureInputPID` in the registry's console users, as `ioreg -l`
/// shows it. macOS fills it with the frontmost app's pid, not the process that
/// called `EnableSecureEventInput`, so it is evidence of who holds secure
/// input, not proof.
mod frontmost {
    use std::ffi::c_void;

    use objc2_core_foundation::CFString;

    const MAIN_PORT_DEFAULT: u32 = 0;
    const CF_NUMBER_SINT32: isize = 3;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IORegistryGetRootEntry(main_port: u32) -> u32;
        fn IORegistryEntryCreateCFProperty(
            entry: u32,
            key: *const c_void,
            allocator: *const c_void,
            options: u32,
        ) -> *const c_void;
        fn IOObjectRelease(object: u32) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: *const c_void);
        fn CFGetTypeID(value: *const c_void) -> usize;
        fn CFArrayGetTypeID() -> usize;
        fn CFDictionaryGetTypeID() -> usize;
        fn CFNumberGetTypeID() -> usize;
        fn CFArrayGetCount(array: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
        fn CFDictionaryGetValue(dictionary: *const c_void, key: *const c_void) -> *const c_void;
        fn CFNumberGetValue(number: *const c_void, kind: isize, value: *mut c_void) -> u8;
    }

    pub(super) fn pid() -> Option<i32> {
        let users_key = CFString::from_str("IOConsoleUsers");
        let pid_key = CFString::from_str("kCGSSessionSecureInputPID");
        let users_key: *const CFString = &*users_key;
        let pid_key: *const CFString = &*pid_key;
        // SAFETY: the root entry and the copied property are released here;
        // every value read is type-checked before use.
        unsafe {
            let root = IORegistryGetRootEntry(MAIN_PORT_DEFAULT);
            if root == 0 {
                return None;
            }
            let users =
                IORegistryEntryCreateCFProperty(root, users_key.cast(), std::ptr::null(), 0);
            IOObjectRelease(root);
            if users.is_null() {
                return None;
            }
            let mut found = None;
            if CFGetTypeID(users) == CFArrayGetTypeID() {
                for index in 0..CFArrayGetCount(users) {
                    let user = CFArrayGetValueAtIndex(users, index);
                    if user.is_null() || CFGetTypeID(user) != CFDictionaryGetTypeID() {
                        continue;
                    }
                    let number = CFDictionaryGetValue(user, pid_key.cast());
                    let mut pid = 0i32;
                    if !number.is_null()
                        && CFGetTypeID(number) == CFNumberGetTypeID()
                        && CFNumberGetValue(number, CF_NUMBER_SINT32, (&raw mut pid).cast()) != 0
                        && pid > 0
                    {
                        found = Some(pid);
                        break;
                    }
                }
            }
            CFRelease(users);
            found
        }
    }

    /// The front app's bundle name, else its executable's.
    pub(super) fn name(pid: i32) -> Option<String> {
        let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: the buffer holds the size passed.
        let length =
            unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
        let length = usize::try_from(length).ok().filter(|length| *length > 0)?;
        Some(super::process_name(&String::from_utf8_lossy(
            &buffer[..length],
        )))
    }
}

/// `…/OpenCode.app/Contents/MacOS/opencode` names `OpenCode`; a bare
/// executable names itself.
fn process_name(path: &str) -> String {
    path.split('/')
        .find_map(|part| part.strip_suffix(".app"))
        .or_else(|| path.rsplit('/').next())
        .unwrap_or(path)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::{
        HotkeyBinding, LEFT_OPTION_MASK, LEFT_SHIFT_MASK, OPTION_KEY_MASK, SHIFT_KEY_MASK,
    };

    const S: u16 = 1;

    fn option_shift_s() -> RuntimeHotkey {
        RuntimeHotkey {
            modifiers: crate::app_settings::HotkeyModifiers {
                option: Some(ModifierSide::Either),
                shift: Some(ModifierSide::Either),
                ..Default::default()
            },
            key_code: Some(S),
        }
    }

    fn chord(target: Target, binding: RuntimeHotkey) -> Chord {
        let (code, modifiers) = carbon_chord(binding).unwrap();
        Chord {
            target,
            binding,
            code,
            modifiers,
        }
    }

    #[test]
    fn only_keyboard_keys_without_fn_become_carbon_hotkeys() {
        assert_eq!(
            carbon_chord(option_shift_s()),
            Some((S, CARBON_OPTION | CARBON_SHIFT))
        );
        let mut function = option_shift_s();
        function.modifiers.function = true;
        assert_eq!(carbon_chord(function), None);
        let mut mouse = option_shift_s();
        mouse.key_code = crate::app_settings::HotkeyKey::mouse_code(3);
        assert_eq!(carbon_chord(mouse), None);
        let keyless = RuntimeHotkey {
            key_code: None,
            ..option_shift_s()
        };
        assert_eq!(carbon_chord(keyless), None);
        let mut left = option_shift_s();
        left.modifiers.option = Some(ModifierSide::Left);
        assert_eq!(carbon_chord(left), carbon_chord(option_shift_s()));
    }

    #[test]
    fn the_starting_shortcuts_register_while_unbound_ones_do_not() {
        let hotkeys = RuntimeHotkeys {
            capture: Some(option_shift_s()),
            ..RuntimeHotkeys::default()
        };
        let screenshot = HotkeyBinding::screenshot_default().runtime();
        let targets: Vec<Target> = chords(hotkeys, [Some(screenshot), None])
            .iter()
            .map(|chord| chord.target)
            .collect();
        assert!(targets.contains(&Target::Capture));
        assert!(targets.contains(&Target::PasteLast));
        assert!(targets.contains(&Target::Screenshot));
        assert!(
            chords(RuntimeHotkeys::default(), [None, None])
                .iter()
                .all(|chord| chord.target != Target::Capture)
        );
    }

    #[test]
    fn two_shortcuts_on_one_chord_register_once() {
        let hotkeys = RuntimeHotkeys {
            capture: Some(option_shift_s()),
            paste_last: Some(option_shift_s()),
            paste_meeting: None,
            ..RuntimeHotkeys::default()
        };
        let registered = chords(hotkeys, [None, None]);
        assert_eq!(registered.len(), 1);
        assert_eq!(registered[0].target, Target::Capture);
    }

    #[test]
    fn a_press_without_its_release_sends_the_release_first() {
        let mut held = Held::default();
        assert_eq!(held.pressed(Target::Capture), &[true]);
        assert_eq!(held.pressed(Target::Capture), &[false, true]);
        assert_eq!(held.released(Target::Capture), &[false]);
        assert!(held.released(Target::Capture).is_empty());
        assert_eq!(held.pressed(Target::PasteLast), &[true]);
        assert_eq!(held.pressed(Target::Capture), &[true]);
    }

    #[test]
    fn a_press_carries_the_keyboards_flags_or_the_bindings() {
        let either = chord(Target::Capture, option_shift_s());
        let real = OPTION_KEY_MASK | LEFT_OPTION_MASK | SHIFT_KEY_MASK | LEFT_SHIFT_MASK;
        assert_eq!(press_flags(either, real), real);
        let synthesized = press_flags(either, 0);
        assert!(option_shift_s().matches_key_press(S, synthesized));
        let mut right = option_shift_s();
        right.modifiers.option = Some(ModifierSide::Right);
        let right = chord(Target::Capture, right);
        assert!(right.binding.matches_key_press(S, press_flags(right, real)));
    }

    #[cfg(hex_capture)]
    #[test]
    fn an_annotate_press_still_records_when_its_modifiers_came_up_first() {
        use crate::capture_host::{CaptureShortcut, ShortcutAction, ShortcutActivity};
        let binding = option_shift_s();
        let hotkeys = RuntimeHotkeys {
            capture: Some(binding),
            ..RuntimeHotkeys::default()
        };
        let chord = chord(Target::Capture, binding);
        let held = OPTION_KEY_MASK | SHIFT_KEY_MASK;

        // Modifiers still down: the press arms, and the tap's modifier
        // change records.
        let mut shortcut = CaptureShortcut::default();
        let actions: Vec<_> = listener_events(chord, true, held)
            .into_iter()
            .chain(listener_events(chord, false, held))
            .chain([InputEvent::Flags(OPTION_KEY_MASK)])
            .map(|event| shortcut.input(event, hotkeys, ShortcutActivity::Free))
            .collect();
        assert_eq!(
            actions,
            [
                ShortcutAction::None,
                ShortcutAction::None,
                ShortcutAction::Record
            ]
        );

        // The tap's modifier change reached the listener before the press.
        let mut shortcut = CaptureShortcut::default();
        assert_eq!(
            shortcut.input(InputEvent::Flags(0), hotkeys, ShortcutActivity::Free),
            ShortcutAction::None
        );
        let actions: Vec<_> = listener_events(chord, true, 0)
            .into_iter()
            .map(|event| shortcut.input(event, hotkeys, ShortcutActivity::Free))
            .collect();
        assert_eq!(actions.last(), Some(&ShortcutAction::Record));

        // A second press after a lost release still counts.
        let mut shortcut = CaptureShortcut::default();
        let mut held_keys = Held::default();
        let mut actions = Vec::new();
        for _ in 0..2 {
            for &down in held_keys.pressed(Target::Capture) {
                for event in listener_events(chord, down, if down { held } else { 0 }) {
                    actions.push(shortcut.input(event, hotkeys, ShortcutActivity::Free));
                }
            }
            actions.push(shortcut.input(InputEvent::Flags(0), hotkeys, ShortcutActivity::Free));
        }
        assert_eq!(
            actions
                .iter()
                .filter(|action| **action == ShortcutAction::Record)
                .count(),
            2
        );
    }

    #[test]
    fn the_frontmost_app_is_named_by_its_app_bundle() {
        assert_eq!(
            process_name("/Applications/OpenCode.app/Contents/MacOS/opencode"),
            "OpenCode"
        );
        assert_eq!(process_name("/usr/bin/login"), "login");
    }
}

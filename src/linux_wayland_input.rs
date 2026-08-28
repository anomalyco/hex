use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use color_eyre::eyre::{Result, WrapErr, eyre};
use evdev::raw_stream::RawDevice;
use evdev::{EventType, KeyCode, SynchronizationCode};

use crate::linux_input::{DOUBLE_TAP_WINDOW, HotkeyEvent, send_event};
use crate::linux_session::LinuxSession;
use crate::linux_settings::LinuxHotkey;

const POLL_INTERVAL: Duration = Duration::from_millis(5);
const RESCAN_INTERVAL: Duration = Duration::from_secs(1);

// Physical evdev keys, not layout-dependent characters. Use this same table in both directions.
const KEYS: &[(&str, KeyCode)] = &[
    ("space", KeyCode::KEY_SPACE),
    ("enter", KeyCode::KEY_ENTER),
    ("tab", KeyCode::KEY_TAB),
    ("backspace", KeyCode::KEY_BACKSPACE),
    ("a", KeyCode::KEY_A),
    ("b", KeyCode::KEY_B),
    ("c", KeyCode::KEY_C),
    ("d", KeyCode::KEY_D),
    ("e", KeyCode::KEY_E),
    ("f", KeyCode::KEY_F),
    ("g", KeyCode::KEY_G),
    ("h", KeyCode::KEY_H),
    ("i", KeyCode::KEY_I),
    ("j", KeyCode::KEY_J),
    ("k", KeyCode::KEY_K),
    ("l", KeyCode::KEY_L),
    ("m", KeyCode::KEY_M),
    ("n", KeyCode::KEY_N),
    ("o", KeyCode::KEY_O),
    ("p", KeyCode::KEY_P),
    ("q", KeyCode::KEY_Q),
    ("r", KeyCode::KEY_R),
    ("s", KeyCode::KEY_S),
    ("t", KeyCode::KEY_T),
    ("u", KeyCode::KEY_U),
    ("v", KeyCode::KEY_V),
    ("w", KeyCode::KEY_W),
    ("x", KeyCode::KEY_X),
    ("y", KeyCode::KEY_Y),
    ("z", KeyCode::KEY_Z),
    ("0", KeyCode::KEY_0),
    ("1", KeyCode::KEY_1),
    ("2", KeyCode::KEY_2),
    ("3", KeyCode::KEY_3),
    ("4", KeyCode::KEY_4),
    ("5", KeyCode::KEY_5),
    ("6", KeyCode::KEY_6),
    ("7", KeyCode::KEY_7),
    ("8", KeyCode::KEY_8),
    ("9", KeyCode::KEY_9),
    ("f1", KeyCode::KEY_F1),
    ("f2", KeyCode::KEY_F2),
    ("f3", KeyCode::KEY_F3),
    ("f4", KeyCode::KEY_F4),
    ("f5", KeyCode::KEY_F5),
    ("f6", KeyCode::KEY_F6),
    ("f7", KeyCode::KEY_F7),
    ("f8", KeyCode::KEY_F8),
    ("f9", KeyCode::KEY_F9),
    ("f10", KeyCode::KEY_F10),
    ("f11", KeyCode::KEY_F11),
    ("f12", KeyCode::KEY_F12),
    ("f13", KeyCode::KEY_F13),
    ("f14", KeyCode::KEY_F14),
    ("f15", KeyCode::KEY_F15),
    ("f16", KeyCode::KEY_F16),
    ("f17", KeyCode::KEY_F17),
    ("f18", KeyCode::KEY_F18),
    ("f19", KeyCode::KEY_F19),
    ("f20", KeyCode::KEY_F20),
    ("f21", KeyCode::KEY_F21),
    ("f22", KeyCode::KEY_F22),
    ("f23", KeyCode::KEY_F23),
    ("f24", KeyCode::KEY_F24),
    ("-", KeyCode::KEY_MINUS),
    ("=", KeyCode::KEY_EQUAL),
    ("[", KeyCode::KEY_LEFTBRACE),
    ("]", KeyCode::KEY_RIGHTBRACE),
    (";", KeyCode::KEY_SEMICOLON),
    ("'", KeyCode::KEY_APOSTROPHE),
    ("`", KeyCode::KEY_GRAVE),
    ("\\", KeyCode::KEY_BACKSLASH),
    (",", KeyCode::KEY_COMMA),
    (".", KeyCode::KEY_DOT),
    ("/", KeyCode::KEY_SLASH),
];

const MODIFIERS: [[KeyCode; 2]; 4] = [
    [KeyCode::KEY_LEFTCTRL, KeyCode::KEY_RIGHTCTRL],
    [KeyCode::KEY_LEFTALT, KeyCode::KEY_RIGHTALT],
    [KeyCode::KEY_LEFTSHIFT, KeyCode::KEY_RIGHTSHIFT],
    [KeyCode::KEY_LEFTMETA, KeyCode::KEY_RIGHTMETA],
];

fn key_code(name: &str) -> Result<KeyCode> {
    let name = name.to_ascii_lowercase();
    let name = if name == "return" { "enter" } else { &name };
    KEYS.iter()
        .find_map(|(key, code)| (*key == name).then_some(*code))
        .ok_or_else(|| eyre!("unsupported Wayland physical hotkey key: {name}"))
}

enum Input {
    Connected(PathBuf, HashSet<KeyCode>),
    Disconnected(PathBuf),
    Key(PathBuf, KeyCode, i32),
}

#[derive(Default)]
struct Pressed(HashMap<PathBuf, HashSet<KeyCode>>);

impl Pressed {
    fn contains(&self, key: KeyCode) -> bool {
        self.0.values().any(|keys| keys.contains(&key))
    }

    fn modifiers(&self) -> [bool; 4] {
        MODIFIERS.map(|pair| pair.into_iter().any(|key| self.contains(key)))
    }

    // Emit only aggregate edges: a key stays down until every keyboard releases it.
    fn update(&mut self, input: Input) -> Option<(KeyCode, bool)> {
        match input {
            Input::Connected(path, keys) => {
                self.0.insert(path, keys);
            }
            Input::Disconnected(path) => {
                self.0.remove(&path);
            }
            Input::Key(path, key, value @ (0 | 1)) => {
                let before = self.contains(key);
                let keys = self.0.get_mut(&path)?;
                if value == 1 {
                    keys.insert(key);
                } else {
                    keys.remove(&key);
                }
                let after = self.contains(key);
                return (before != after).then_some((key, after));
            }
            Input::Key(..) => {}
        }
        None
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Capture {
    Idle,
    Held { second_tap: bool },
    Locked,
}

struct HotkeyState {
    pressed: Pressed,
    trigger: KeyCode,
    required: [bool; 4],
    double_tap: bool,
    capture: Capture,
    last_release: Option<Instant>,
}

impl HotkeyState {
    fn new(binding: &LinuxHotkey, double_tap: bool) -> Result<Self> {
        Ok(Self {
            pressed: Pressed::default(),
            trigger: key_code(&binding.key)?,
            required: [
                binding.control,
                binding.alt,
                binding.shift,
                binding.super_key,
            ],
            double_tap,
            capture: Capture::Idle,
            last_release: None,
        })
    }

    fn cancel(&mut self) -> Option<HotkeyEvent> {
        let active = self.capture != Capture::Idle;
        self.capture = Capture::Idle;
        self.last_release = None;
        active.then_some(HotkeyEvent::Cancel)
    }

    fn update(&mut self, input: Input, now: Instant) -> Option<HotkeyEvent> {
        if matches!(input, Input::Connected(..) | Input::Disconnected(..)) {
            self.pressed.update(input);
            // Never infer presses or releases from a device snapshot or lost event stream.
            return self.cancel();
        }
        let (key, down) = self.pressed.update(input)?;
        if key == KeyCode::KEY_ESC && down {
            return self.cancel();
        }
        let modifiers = self.pressed.modifiers();
        if let Capture::Held { second_tap } = self.capture {
            if modifiers
                .iter()
                .zip(self.required)
                .any(|(held, required)| *held && !required)
            {
                return self.cancel();
            }
            // Releasing a required modifier first ends the hold just like releasing the trigger.
            if !self.pressed.contains(self.trigger) || modifiers != self.required {
                if second_tap {
                    self.capture = Capture::Locked;
                    self.last_release = None;
                    return None;
                }
                self.capture = Capture::Idle;
                self.last_release = self.double_tap.then_some(now);
                return Some(HotkeyEvent::Finish);
            }
        }
        if key != self.trigger || !down {
            return None;
        }
        if modifiers != self.required || self.pressed.contains(KeyCode::KEY_ESC) {
            self.last_release = None;
            return None;
        }
        match self.capture {
            Capture::Locked => {
                self.capture = Capture::Idle;
                self.last_release = None;
                Some(HotkeyEvent::Finish)
            }
            Capture::Idle => {
                let second_tap = self.last_release.take().is_some_and(|released| {
                    now.checked_duration_since(released)
                        .is_some_and(|gap| gap < DOUBLE_TAP_WINDOW)
                });
                self.capture = Capture::Held { second_tap };
                Some(HotkeyEvent::Start)
            }
            Capture::Held { .. } => None,
        }
    }

    fn dispatch(
        &mut self,
        batch: Result<Vec<(Instant, Input)>>,
        sender: &SyncSender<HotkeyEvent>,
        stop: &AtomicBool,
    ) -> Result<bool> {
        let inputs = match batch {
            Ok(inputs) => inputs,
            Err(error) => {
                if let Some(event) = self.cancel() {
                    let _ = send_event(sender, event);
                }
                return Err(error);
            }
        };
        for (at, input) in inputs {
            if stop.load(Ordering::Acquire) {
                return Ok(false);
            }
            if let Some(event) = self.update(input, at)
                && !send_event(sender, event)?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

struct EventClock {
    monotonic: Duration,
    instant: Instant,
}

impl EventClock {
    fn new() -> Result<Self> {
        let instant = Instant::now();
        let mut time = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // Sample the same clock selected on every evdev descriptor, once per monitor.
        if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) } < 0 {
            return Err(std::io::Error::last_os_error())
                .wrap_err("could not read the input event clock");
        }
        Ok(Self {
            monotonic: Duration::new(time.tv_sec as u64, time.tv_nsec as u32),
            instant,
        })
    }

    fn at(&self, timestamp: SystemTime) -> Result<Instant> {
        // evdev wraps timeval in SystemTime even when EVIOCSCLOCKID selects monotonic time.
        let time = timestamp.duration_since(SystemTime::UNIX_EPOCH)?;
        if time >= self.monotonic {
            self.instant.checked_add(time - self.monotonic)
        } else {
            self.instant.checked_sub(self.monotonic - time)
        }
        .ok_or_else(|| eyre!("input event timestamp is outside the monotonic clock range"))
    }
}

fn check_device_error(path: &Path, error: std::io::Error) -> Result<()> {
    if matches!(error.raw_os_error(), Some(libc::ENOENT | libc::ENODEV)) {
        return Ok(());
    }
    Err(error).wrap_err_with(|| format!(
        "Wayland input requires read access to every /dev/input/event* node to check modifier state safely; could not inspect {}. An unreadable device cannot be classified as a keyboard or ruled out. Configure system input permissions (often the input group); this grants access to all keystrokes, not just HEX shortcuts",
        path.display()
    ))
}

struct Keyboard {
    path: PathBuf,
    device: RawDevice,
}

struct Keyboards {
    devices: Vec<Keyboard>,
    next_scan: Instant,
    clock: EventClock,
}

impl Keyboards {
    fn open(stop: &AtomicBool) -> Result<(Self, Vec<Input>)> {
        if stop.load(Ordering::Acquire) {
            return Err(eyre!("keyboard monitoring canceled"));
        }
        let mut keyboards = Self {
            devices: Vec::new(),
            next_scan: Instant::now(),
            clock: EventClock::new()?,
        };
        let initial = keyboards.scan(stop)?;
        if stop.load(Ordering::Acquire) {
            return Err(eyre!("keyboard monitoring canceled"));
        }
        if keyboards.devices.is_empty() {
            return Err(eyre!(
                "no keyboard event devices with supported shortcut keys found in /dev/input"
            ));
        }
        Ok((keyboards, initial))
    }

    fn scan(&mut self, stop: &AtomicBool) -> Result<Vec<Input>> {
        self.next_scan = Instant::now() + RESCAN_INTERVAL;
        let mut connected = Vec::new();
        let directory = Path::new("/dev/input");
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) => {
                check_device_error(directory, error)?;
                return Ok(connected);
            }
        };
        for entry in entries {
            if stop.load(Ordering::Acquire) {
                break;
            }
            let path = match entry {
                Ok(entry) => entry.path(),
                Err(error) => {
                    check_device_error(directory, error)?;
                    continue;
                }
            };
            if !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("event"))
                || self.devices.iter().any(|keyboard| keyboard.path == path)
            {
                continue;
            }
            let opened = (|| -> std::io::Result<_> {
                // Open read-only and nonblocking from the outset; never grab or inject input.
                let file = OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
                    .open(&path)?;
                let device = RawDevice::from_fd(file.into())?;
                if !device.supported_keys().is_some_and(|keys| {
                    keys.contains(KeyCode::KEY_ESC)
                        || KEYS.iter().any(|(_, key)| keys.contains(*key))
                        || MODIFIERS.iter().flatten().any(|key| keys.contains(*key))
                }) {
                    return Ok(None);
                }
                let clock = libc::CLOCK_MONOTONIC;
                // EVIOCSCLOCKID takes a pointer to an int and changes only this reader's clock.
                if unsafe {
                    libc::ioctl(
                        device.as_raw_fd(),
                        libc::_IOW::<libc::c_int>(b'E'.into(), 0xa0),
                        &clock,
                    )
                } < 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                let pressed = device.get_key_state()?.iter().collect();
                Ok(Some((device, pressed)))
            })();
            match opened {
                Ok(Some((device, pressed))) => {
                    connected.push(Input::Connected(path.clone(), pressed));
                    self.devices.push(Keyboard { path, device });
                }
                Ok(None) => {}
                Err(error) => check_device_error(&path, error)?,
            }
        }
        Ok(connected)
    }

    fn poll(&mut self, stop: &AtomicBool) -> Result<Vec<(Instant, Input)>> {
        let mut changes = Vec::new();
        if Instant::now() >= self.next_scan {
            changes.extend(self.scan(stop)?);
        }
        let mut pending = Vec::new();
        self.devices.retain_mut(|keyboard| {
            if stop.load(Ordering::Acquire) {
                return true;
            }
            let events = match keyboard.device.fetch_events() {
                Ok(events) => events.collect::<Vec<_>>(),
                Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {
                    return true;
                }
                Err(error) => {
                    tracing::warn!(path = %keyboard.path.display(), %error, "keyboard disconnected; retrying discovery");
                    changes.push(Input::Disconnected(keyboard.path.clone()));
                    return false;
                }
            };
            if events.iter().any(|event| {
                event.event_type() == EventType::SYNCHRONIZATION
                    && event.code() == SynchronizationCode::SYN_DROPPED.0
            }) {
                // Reopen instead of treating evdev's synthetic resync edges as real shortcuts.
                tracing::warn!(path = %keyboard.path.display(), "keyboard events lost; canceling capture and reopening");
                changes.push(Input::Disconnected(keyboard.path.clone()));
                return false;
            }
            pending.extend(events.into_iter().filter(|event| event.event_type() == EventType::KEY && matches!(event.value(), 0 | 1))
                .map(|event| (event.timestamp(), Input::Key(keyboard.path.clone(), KeyCode::new(event.code()), event.value()))));
            true
        });
        // Modifiers may be on a different keyboard from the trigger.
        pending.sort_by_key(|(time, _)| *time);
        let now = Instant::now();
        let mut inputs = changes
            .into_iter()
            .map(|input| (now, input))
            .collect::<Vec<_>>();
        for (time, input) in pending {
            inputs.push((self.clock.at(time)?, input));
        }
        Ok(inputs)
    }
}

pub(crate) fn run(
    sender: SyncSender<HotkeyEvent>,
    stop: Arc<AtomicBool>,
    ready: SyncSender<Result<()>>,
    binding: LinuxHotkey,
    double_tap_enabled: bool,
    started: Arc<AtomicBool>,
) -> Result<()> {
    let mut state = HotkeyState::new(&binding, double_tap_enabled)?;
    let (mut keyboards, initial) = Keyboards::open(&stop)?;
    for input in initial {
        state.update(input, Instant::now());
    }
    started.store(true, Ordering::Release);
    if stop.load(Ordering::Acquire) || ready.try_send(Ok(())).is_err() {
        return Ok(());
    }
    while !stop.load(Ordering::Acquire) {
        if !state.dispatch(keyboards.poll(&stop), &sender, &stop)? {
            return Ok(());
        }
        thread::sleep(POLL_INTERVAL);
    }
    Ok(())
}

fn capture_binding(pressed: &mut Pressed, input: Input) -> Result<Option<LinuxHotkey>> {
    if matches!(input, Input::Disconnected(..)) {
        return Err(eyre!(
            "keyboard disconnected or events were lost during shortcut capture; try again"
        ));
    }
    let Some((key, true)) = pressed.update(input) else {
        return Ok(None);
    };
    if key == KeyCode::KEY_ESC {
        return Err(eyre!("shortcut capture canceled"));
    }
    let Some((name, _)) = KEYS.iter().find(|(_, code)| *code == key) else {
        return Ok(None);
    };
    let [control, alt, shift, super_key] = pressed.modifiers();
    Ok(Some(LinuxHotkey {
        control,
        alt,
        shift,
        super_key,
        key: (*name).into(),
    }))
}

pub(crate) fn capture_wayland_binding(stop: &AtomicBool) -> Result<LinuxHotkey> {
    if !LinuxSession::detect().is_wayland() {
        return Err(eyre!(
            "raw keyboard shortcut capture is only available on Wayland"
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut pressed = Pressed::default();
    let (mut keyboards, initial) = Keyboards::open(stop)?;
    for input in initial {
        capture_binding(&mut pressed, input)?;
    }
    loop {
        if stop.load(Ordering::Acquire) {
            return Err(eyre!("shortcut capture canceled"));
        }
        if Instant::now() >= deadline {
            return Err(eyre!("timed out waiting for a shortcut"));
        }
        for (_, input) in keyboards.poll(stop)? {
            if stop.load(Ordering::Acquire) {
                return Err(eyre!("shortcut capture canceled"));
            }
            if let Some(binding) = capture_binding(&mut pressed, input)? {
                return Ok(binding);
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
}

pub(crate) fn wayland_modifiers_held(stop: &AtomicBool) -> Result<bool> {
    if !LinuxSession::detect().is_wayland() {
        return Err(eyre!(
            "raw keyboard modifier checks are only available on Wayland"
        ));
    }
    // Use the same fail-closed permissions check as monitor startup, not a weaker paste-only view.
    let (_keyboards, initial) = Keyboards::open(stop)?;
    Ok(initial.iter().any(|input| {
        matches!(input, Input::Connected(_, keys) if MODIFIERS.iter().flatten().any(|key| keys.contains(key)))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use HotkeyEvent::{Cancel, Finish, Start};

    struct Test {
        state: HotkeyState,
        now: Instant,
        events: Vec<HotkeyEvent>,
    }

    impl Test {
        fn new(double_tap: bool) -> Self {
            let mut test = Self {
                state: HotkeyState::new(&LinuxHotkey::default(), double_tap).unwrap(),
                now: Instant::now(),
                events: Vec::new(),
            };
            test.input(Input::Connected("keyboard".into(), HashSet::new()));
            test
        }

        fn input(&mut self, input: Input) {
            if let Some(event) = self.state.update(input, self.now) {
                self.events.push(event);
            }
        }

        fn key(&mut self, key: KeyCode, value: i32) {
            self.input(Input::Key("keyboard".into(), key, value));
        }

        fn tap(&mut self) {
            self.key(KeyCode::KEY_LEFTALT, 1);
            self.key(KeyCode::KEY_SPACE, 1);
            self.key(KeyCode::KEY_SPACE, 0);
            self.key(KeyCode::KEY_LEFTALT, 0);
        }

        fn lock(&mut self) {
            self.tap();
            self.now += Duration::from_millis(20);
            self.tap();
            assert_eq!(self.events, [Start, Finish, Start]);
            assert!(self.state.capture == Capture::Locked);
            self.events.clear();
        }
    }

    #[test]
    fn every_supported_key_round_trips_through_capture() {
        let mut names = HashSet::new();
        let mut codes = HashSet::new();
        for &(name, code) in KEYS {
            assert!(names.insert(name));
            assert!(codes.insert(code));
            assert_eq!(key_code(name).unwrap(), code);
            let mut pressed = Pressed::default();
            pressed.update(Input::Connected("keyboard".into(), HashSet::new()));
            let captured = capture_binding(&mut pressed, Input::Key("keyboard".into(), code, 1))
                .unwrap()
                .unwrap();
            assert_eq!(captured.key, name);
            assert_eq!(key_code(&captured.key).unwrap(), code);
        }
        for (name, code) in [
            ("s", KeyCode::KEY_S),
            ("q", KeyCode::KEY_Q),
            ("z", KeyCode::KEY_Z),
            ("0", KeyCode::KEY_0),
            ("1", KeyCode::KEY_1),
            ("9", KeyCode::KEY_9),
            ("f11", KeyCode::KEY_F11),
            ("f13", KeyCode::KEY_F13),
        ] {
            assert_eq!(key_code(name).unwrap(), code);
        }
        assert_eq!(key_code("RETURN").unwrap(), KeyCode::KEY_ENTER);
        assert!(key_code("f25").is_err());
        assert!(key_code("escape").is_err());
    }

    #[test]
    fn quick_release_and_repress_never_coalesce_without_locking() {
        let mut test = Test::new(false);
        test.key(KeyCode::KEY_LEFTALT, 1);
        test.key(KeyCode::KEY_SPACE, 1);
        test.now += Duration::from_millis(500);
        test.key(KeyCode::KEY_SPACE, 0);
        test.now += Duration::from_millis(20);
        test.key(KeyCode::KEY_SPACE, 1);
        test.now += Duration::from_secs(1);
        test.key(KeyCode::KEY_SPACE, 0);
        assert_eq!(test.events, [Start, Finish, Start, Finish]);
    }

    #[test]
    fn repeats_and_duplicate_edges_do_not_start_finish_or_lock() {
        let mut test = Test::new(true);
        test.key(KeyCode::KEY_LEFTALT, 1);
        test.key(KeyCode::KEY_SPACE, 2);
        assert!(test.events.is_empty());
        test.key(KeyCode::KEY_SPACE, 1);
        test.key(KeyCode::KEY_SPACE, 1);
        test.key(KeyCode::KEY_SPACE, 2);
        test.key(KeyCode::KEY_SPACE, 0);
        test.key(KeyCode::KEY_SPACE, 0);
        assert_eq!(test.events, [Start, Finish]);
    }

    #[test]
    fn escape_from_locked_capture_does_not_swallow_the_next_shortcut() {
        let mut test = Test::new(true);
        test.lock();
        test.key(KeyCode::KEY_ESC, 1);
        test.key(KeyCode::KEY_ESC, 0);
        test.tap();
        assert_eq!(test.events, [Cancel, Start, Finish]);
    }

    #[test]
    fn escape_during_hold_suppresses_remaining_releases_and_repeat() {
        let mut test = Test::new(true);
        test.key(KeyCode::KEY_LEFTALT, 1);
        test.key(KeyCode::KEY_SPACE, 1);
        test.key(KeyCode::KEY_ESC, 1);
        test.key(KeyCode::KEY_ESC, 0);
        test.key(KeyCode::KEY_SPACE, 2);
        test.key(KeyCode::KEY_SPACE, 0);
        test.key(KeyCode::KEY_SPACE, 1);
        test.key(KeyCode::KEY_SPACE, 0);
        assert_eq!(test.events, [Start, Cancel, Start, Finish]);
    }

    #[test]
    fn held_escape_does_not_allow_a_new_capture() {
        let mut test = Test::new(false);
        test.key(KeyCode::KEY_ESC, 1);
        test.tap();
        assert!(test.events.is_empty());
        test.key(KeyCode::KEY_ESC, 0);
        test.tap();
        assert_eq!(test.events, [Start, Finish]);
    }

    #[test]
    fn only_the_exact_chord_stops_locked_recording() {
        let mut test = Test::new(true);
        test.lock();
        test.key(KeyCode::KEY_SPACE, 1);
        test.key(KeyCode::KEY_SPACE, 0);
        test.key(KeyCode::KEY_LEFTCTRL, 1);
        test.tap();
        test.key(KeyCode::KEY_LEFTCTRL, 0);
        assert!(test.events.is_empty());
        test.tap();
        assert_eq!(test.events, [Finish]);
        test.tap();
        assert_eq!(test.events, [Finish, Start, Finish]);
    }

    #[test]
    fn modifier_first_release_finishes_once_and_requires_a_fresh_trigger() {
        let mut test = Test::new(false);
        test.key(KeyCode::KEY_LEFTALT, 1);
        test.key(KeyCode::KEY_SPACE, 1);
        test.key(KeyCode::KEY_LEFTALT, 0);
        assert_eq!(test.events, [Start, Finish]);
        test.key(KeyCode::KEY_LEFTALT, 1);
        test.key(KeyCode::KEY_SPACE, 2);
        test.key(KeyCode::KEY_SPACE, 0);
        assert_eq!(test.events, [Start, Finish]);
        test.key(KeyCode::KEY_SPACE, 1);
        test.key(KeyCode::KEY_SPACE, 0);
        assert_eq!(test.events, [Start, Finish, Start, Finish]);
    }

    #[test]
    fn modifier_first_release_can_lock_the_second_tap() {
        let mut test = Test::new(true);
        test.tap();
        test.key(KeyCode::KEY_LEFTALT, 1);
        test.key(KeyCode::KEY_SPACE, 1);
        test.key(KeyCode::KEY_LEFTALT, 0);
        test.key(KeyCode::KEY_SPACE, 0);
        assert_eq!(test.events, [Start, Finish, Start]);
        assert!(test.state.capture == Capture::Locked);
        test.tap();
        assert_eq!(test.events, [Start, Finish, Start, Finish]);
    }

    #[test]
    fn extra_modifiers_prevent_start_and_cancel_an_existing_hold() {
        let mut test = Test::new(true);
        test.key(KeyCode::KEY_LEFTSHIFT, 1);
        test.tap();
        assert!(test.events.is_empty());
        test.key(KeyCode::KEY_LEFTSHIFT, 0);
        test.key(KeyCode::KEY_LEFTALT, 1);
        test.key(KeyCode::KEY_SPACE, 1);
        test.key(KeyCode::KEY_LEFTSHIFT, 1);
        test.key(KeyCode::KEY_LEFTSHIFT, 0);
        test.key(KeyCode::KEY_SPACE, 0);
        test.tap();
        assert_eq!(test.events, [Start, Cancel, Start, Finish]);
    }

    #[test]
    fn double_tap_window_has_an_explicit_boundary() {
        for (delay, expected) in [
            (299, vec![Start, Finish, Start]),
            (300, vec![Start, Finish, Start, Finish]),
        ] {
            let mut test = Test::new(true);
            test.tap();
            test.now += Duration::from_millis(delay);
            test.tap();
            assert_eq!(test.events, expected);
        }
    }

    #[test]
    fn buffered_taps_use_physical_time_not_the_time_the_batch_is_replayed() {
        for (gap_ms, expected) in [
            (200, vec![Start, Finish, Start]),
            (500, vec![Start, Finish, Start, Finish]),
        ] {
            let mut test = Test::new(true);
            let clock = EventClock {
                monotonic: Duration::from_secs(100),
                instant: test.now,
            };
            let replayed_at = test.now + Duration::from_secs(10);
            let batch = [
                (100, KeyCode::KEY_LEFTALT, 1),
                (110, KeyCode::KEY_SPACE, 1),
                (120, KeyCode::KEY_SPACE, 0),
                (120 + gap_ms, KeyCode::KEY_SPACE, 1),
                (130 + gap_ms, KeyCode::KEY_SPACE, 0),
            ]
            .into_iter()
            .map(|(millis, key, value)| {
                let timestamp =
                    SystemTime::UNIX_EPOCH + clock.monotonic + Duration::from_millis(millis);
                let at = clock.at(timestamp).unwrap();
                assert!(at < replayed_at);
                (at, Input::Key("keyboard".into(), key, value))
            })
            .collect();
            let (sender, receiver) = std::sync::mpsc::sync_channel(16);
            assert!(
                test.state
                    .dispatch(Ok(batch), &sender, &AtomicBool::new(false))
                    .unwrap()
            );
            assert_eq!(receiver.try_iter().collect::<Vec<_>>(), expected);
        }
    }

    #[test]
    fn newly_unreadable_devices_cancel_capture_and_surface_the_permission_error() {
        for locked in [false, true] {
            let mut test = Test::new(true);
            if locked {
                test.lock();
            } else {
                test.key(KeyCode::KEY_LEFTALT, 1);
                test.key(KeyCode::KEY_SPACE, 1);
            }
            let batch = check_device_error(
                Path::new("/dev/input/event99"),
                std::io::Error::from_raw_os_error(libc::EACCES),
            )
            .map(|()| Vec::new());
            let (sender, receiver) = std::sync::mpsc::sync_channel(16);
            let error = test
                .state
                .dispatch(batch, &sender, &AtomicBool::new(false))
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("read access to every /dev/input/event* node")
            );
            assert!(error.to_string().contains("/dev/input/event99"));
            assert_eq!(receiver.try_iter().collect::<Vec<_>>(), [Cancel]);
            assert!(test.state.capture == Capture::Idle);
            assert!(test.state.last_release.is_none());
        }
    }

    #[test]
    fn device_removal_races_are_retryable_but_other_inspection_errors_are_not() {
        for code in [libc::ENOENT, libc::ENODEV] {
            let mut test = Test::new(true);
            test.lock();
            let batch = check_device_error(
                Path::new("/dev/input/event99"),
                std::io::Error::from_raw_os_error(code),
            )
            .map(|()| Vec::new());
            let (sender, receiver) = std::sync::mpsc::sync_channel(16);
            assert!(
                test.state
                    .dispatch(batch, &sender, &AtomicBool::new(false))
                    .unwrap()
            );
            assert!(receiver.try_iter().next().is_none());
            assert!(test.state.capture == Capture::Locked);
        }
        for code in [libc::EACCES, libc::EPERM, libc::EIO] {
            assert!(
                check_device_error(
                    Path::new("/dev/input/event99"),
                    std::io::Error::from_raw_os_error(code),
                )
                .is_err()
            );
        }
    }

    #[test]
    fn trigger_held_on_open_does_not_start_until_released_and_repressed() {
        let mut test = Test::new(false);
        test.input(Input::Connected(
            "keyboard".into(),
            HashSet::from([KeyCode::KEY_LEFTALT, KeyCode::KEY_SPACE]),
        ));
        test.key(KeyCode::KEY_SPACE, 1);
        test.key(KeyCode::KEY_SPACE, 2);
        test.key(KeyCode::KEY_SPACE, 0);
        assert!(test.events.is_empty());
        test.key(KeyCode::KEY_SPACE, 1);
        test.key(KeyCode::KEY_SPACE, 0);
        assert_eq!(test.events, [Start, Finish]);
    }

    #[test]
    fn held_initial_modifiers_are_used_for_a_fresh_trigger() {
        let mut test = Test::new(false);
        test.input(Input::Connected(
            "keyboard".into(),
            HashSet::from([KeyCode::KEY_RIGHTALT]),
        ));
        test.key(KeyCode::KEY_SPACE, 1);
        test.key(KeyCode::KEY_SPACE, 0);
        assert_eq!(test.events, [Start, Finish]);
    }

    #[test]
    fn multi_keyboard_modifiers_and_trigger_remain_held_until_all_release() {
        let mut test = Test::new(false);
        test.input(Input::Connected("other".into(), HashSet::new()));
        test.input(Input::Key("other".into(), KeyCode::KEY_LEFTALT, 1));
        test.key(KeyCode::KEY_LEFTALT, 1);
        test.key(KeyCode::KEY_SPACE, 1);
        test.input(Input::Key("other".into(), KeyCode::KEY_SPACE, 1));
        test.key(KeyCode::KEY_LEFTALT, 0);
        test.key(KeyCode::KEY_SPACE, 0);
        assert_eq!(test.events, [Start]);
        test.input(Input::Key("other".into(), KeyCode::KEY_SPACE, 0));
        test.input(Input::Key("other".into(), KeyCode::KEY_LEFTALT, 0));
        assert_eq!(test.events, [Start, Finish]);
    }

    #[test]
    fn lost_keyboard_cancels_and_reconnect_resets_held_keys_and_taps() {
        for locked in [false, true] {
            let mut test = Test::new(true);
            if locked {
                test.lock();
            } else {
                test.key(KeyCode::KEY_LEFTALT, 1);
                test.key(KeyCode::KEY_SPACE, 1);
                test.events.clear();
            }
            test.input(Input::Disconnected("keyboard".into()));
            assert_eq!(test.events, [Cancel]);
            assert!(!test.state.pressed.contains(KeyCode::KEY_LEFTALT));
            assert!(!test.state.pressed.contains(KeyCode::KEY_SPACE));
            // Reuse of the same event path is a new keyboard, not a continuation of its old keys.
            test.input(Input::Connected("keyboard".into(), HashSet::new()));
            test.key(KeyCode::KEY_SPACE, 1);
            test.key(KeyCode::KEY_SPACE, 0);
            assert_eq!(test.events, [Cancel]);
            test.tap();
            assert_eq!(test.events, [Cancel, Start, Finish]);
        }
    }

    #[test]
    fn reconnect_with_trigger_down_does_not_resume_canceled_recording() {
        let mut test = Test::new(false);
        test.key(KeyCode::KEY_LEFTALT, 1);
        test.key(KeyCode::KEY_SPACE, 1);
        test.input(Input::Disconnected("keyboard".into()));
        test.input(Input::Connected(
            "keyboard".into(),
            HashSet::from([KeyCode::KEY_LEFTALT, KeyCode::KEY_SPACE]),
        ));
        test.key(KeyCode::KEY_SPACE, 2);
        test.key(KeyCode::KEY_SPACE, 0);
        test.key(KeyCode::KEY_SPACE, 1);
        test.key(KeyCode::KEY_SPACE, 0);
        assert_eq!(test.events, [Start, Cancel, Start, Finish]);
    }

    #[test]
    fn losing_one_keyboard_preserves_the_other_keyboards_modifiers() {
        let mut test = Test::new(false);
        test.input(Input::Connected(
            "other".into(),
            HashSet::from([KeyCode::KEY_RIGHTALT]),
        ));
        test.key(KeyCode::KEY_LEFTALT, 1);
        test.key(KeyCode::KEY_SPACE, 1);
        test.input(Input::Disconnected("keyboard".into()));
        test.input(Input::Key("other".into(), KeyCode::KEY_SPACE, 1));
        test.input(Input::Key("other".into(), KeyCode::KEY_SPACE, 0));
        assert_eq!(test.events, [Start, Cancel, Start, Finish]);
    }

    #[test]
    fn standalone_function_key_uses_the_same_state_machine() {
        let mut test = Test::new(false);
        test.state.trigger = key_code("f13").unwrap();
        test.state.required = [false; 4];
        test.key(KeyCode::KEY_F13, 1);
        test.key(KeyCode::KEY_F13, 0);
        test.key(KeyCode::KEY_LEFTALT, 1);
        test.key(KeyCode::KEY_F13, 1);
        test.key(KeyCode::KEY_F13, 0);
        assert_eq!(test.events, [Start, Finish]);
    }

    #[test]
    fn binding_capture_uses_initial_and_multi_keyboard_modifiers() {
        let mut pressed = Pressed::default();
        capture_binding(
            &mut pressed,
            Input::Connected(
                "one".into(),
                HashSet::from([KeyCode::KEY_RIGHTALT, KeyCode::KEY_RIGHTCTRL]),
            ),
        )
        .unwrap();
        capture_binding(
            &mut pressed,
            Input::Connected(
                "two".into(),
                HashSet::from([
                    KeyCode::KEY_RIGHTSHIFT,
                    KeyCode::KEY_RIGHTMETA,
                    KeyCode::KEY_S,
                ]),
            ),
        )
        .unwrap();
        assert!(
            capture_binding(&mut pressed, Input::Key("two".into(), KeyCode::KEY_S, 2))
                .unwrap()
                .is_none()
        );
        assert!(
            capture_binding(&mut pressed, Input::Key("two".into(), KeyCode::KEY_S, 0))
                .unwrap()
                .is_none()
        );
        let binding = capture_binding(&mut pressed, Input::Key("two".into(), KeyCode::KEY_Q, 1))
            .unwrap()
            .unwrap();
        assert_eq!(
            binding,
            LinuxHotkey {
                control: true,
                alt: true,
                shift: true,
                super_key: true,
                key: "q".into(),
            }
        );
    }

    #[test]
    fn binding_capture_cancels_on_escape_or_device_loss() {
        let mut pressed = Pressed::default();
        pressed.update(Input::Connected("keyboard".into(), HashSet::new()));
        assert!(
            capture_binding(
                &mut pressed,
                Input::Key("keyboard".into(), KeyCode::KEY_ESC, 1)
            )
            .is_err()
        );
        assert!(capture_binding(&mut pressed, Input::Disconnected("keyboard".into())).is_err());
    }
}

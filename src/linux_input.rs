use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{ConnectionExt, GrabMode, ModMask, Window};
use x11rb::rust_connection::RustConnection;

use crate::linux_settings::LinuxHotkey;

const XK_SPACE: u32 = 0x20;
const XK_ESCAPE: u32 = 0xff1b;
pub(crate) const XK_ALT_L: u32 = 0xffe9;
pub(crate) const XK_ALT_R: u32 = 0xffea;
const XK_NUM_LOCK: u32 = 0xff7f;
const RELEASE_GRACE: Duration = Duration::from_millis(50);
const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeyEvent {
    Start,
    Finish,
    Cancel,
}

pub struct LinuxHotkeyMonitor {
    pub events: Receiver<HotkeyEvent>,
    pub errors: Receiver<String>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LinuxHotkeyMonitor {
    pub fn start(binding: LinuxHotkey, double_tap_enabled: bool) -> Result<Self> {
        match crate::linux_session::LinuxSession::detect() {
            crate::linux_session::LinuxSession::Wayland => {
                start_monitor(binding, double_tap_enabled, wayland_run, "Wayland")
            }
            crate::linux_session::LinuxSession::X11 => {
                start_monitor(binding, double_tap_enabled, run, "X11")
            }
        }
    }

    pub fn start_x11(binding: LinuxHotkey, double_tap_enabled: bool) -> Result<Self> {
        start_monitor(binding, double_tap_enabled, run, "X11")
    }
}

impl Drop for LinuxHotkeyMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn start_monitor(
    binding: LinuxHotkey,
    double_tap_enabled: bool,
    run_backend: fn(
        SyncSender<HotkeyEvent>,
        Arc<AtomicBool>,
        SyncSender<Result<()>>,
        LinuxHotkey,
        bool,
        Arc<AtomicBool>,
    ) -> Result<()>,
    session: &str,
) -> Result<LinuxHotkeyMonitor> {
    let (events_sender, events) = mpsc::sync_channel(16);
    let (error_sender, errors) = mpsc::channel();
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let stop = Arc::new(AtomicBool::new(false));
    let started = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker_started = started.clone();
    let worker = thread::spawn(move || {
        let result = run_backend(
            events_sender,
            worker_stop,
            ready_sender.clone(),
            binding,
            double_tap_enabled,
            worker_started.clone(),
        );
        if let Err(error) = result {
            let message = format!("{error:#}");
            if worker_started.load(Ordering::Acquire) {
                let _ = error_sender.send(message);
            } else {
                let _ = ready_sender.send(Err(eyre!(message)));
            }
        }
    });
    ready_receiver
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| eyre!("timed out registering the dictation shortcut on {session}"))??;
    Ok(LinuxHotkeyMonitor {
        events,
        errors,
        stop,
        worker: Some(worker),
    })
}

fn run(
    sender: SyncSender<HotkeyEvent>,
    stop: Arc<AtomicBool>,
    ready: SyncSender<Result<()>>,
    binding: LinuxHotkey,
    double_tap_enabled: bool,
    started: Arc<AtomicBool>,
) -> Result<()> {
    let (connection, screen) =
        RustConnection::connect(None).wrap_err("could not connect to X11")?;
    let root = connection.setup().roots[screen].root;
    let keymap = Keymap::read(&connection)?;
    let trigger = keymap.keycode(keysym(&binding.key)?)?;
    let escape = keymap.keycode(XK_ESCAPE)?;
    let modifiers = binding.modifier_mask(&connection, &keymap)?;
    let num_lock = keymap
        .modifier_for(&connection, &[XK_NUM_LOCK])
        .unwrap_or(ModMask::M2);
    let trigger_modifiers = lock_variants(modifiers, num_lock);
    grab_variants(&connection, root, trigger, &trigger_modifiers).wrap_err_with(|| {
        format!(
            "{} is already in use by another X11 client",
            binding.label()
        )
    })?;
    connection.flush()?;
    ready.send(Ok(())).ok();
    started.store(true, Ordering::Release);

    let mut active = false;
    let mut second_tap = false;
    let mut locked = false;
    let mut dirty = false;
    let mut last_release = None;
    let mut escape_grabbed = false;
    let mut pending_release: Option<Instant> = None;
    while !stop.load(Ordering::Acquire) {
        if let Some(released_at) = pending_release
            && released_at.elapsed() >= RELEASE_GRACE
        {
            pending_release = None;
            active = false;
            if second_tap {
                second_tap = false;
                locked = true;
                last_release = None;
            } else {
                last_release = double_tap_enabled.then_some(Instant::now());
                release_escape(&connection, root, escape, &mut escape_grabbed)?;
                if !send_event(&sender, HotkeyEvent::Finish)? {
                    break;
                }
            }
        }
        let Some(event) = connection.poll_for_event()? else {
            thread::sleep(Duration::from_millis(5));
            continue;
        };
        match event {
            Event::KeyPress(event) if event.detail == trigger => {
                if pending_release.take().is_some() {
                    continue;
                }
                if locked {
                    locked = false;
                    dirty = true;
                    release_escape(&connection, root, escape, &mut escape_grabbed)?;
                    if !send_event(&sender, HotkeyEvent::Finish)? {
                        break;
                    }
                } else if !active && !dirty {
                    active = true;
                    second_tap = last_release
                        .take()
                        .is_some_and(|released: Instant| released.elapsed() < DOUBLE_TAP_WINDOW);
                    connection
                        .grab_key(
                            false,
                            root,
                            ModMask::ANY,
                            escape,
                            GrabMode::ASYNC,
                            GrabMode::ASYNC,
                        )?
                        .check()
                        .wrap_err("Escape is already in use by another X11 client")?;
                    connection.flush()?;
                    escape_grabbed = true;
                    if !send_event(&sender, HotkeyEvent::Start)? {
                        break;
                    }
                }
            }
            Event::KeyRelease(event) if event.detail == trigger && active => {
                pending_release = Some(Instant::now());
            }
            Event::KeyRelease(event) if event.detail == trigger && dirty => {
                dirty = false;
            }
            Event::KeyRelease(event) if event.detail == escape && dirty => {
                dirty = false;
                release_escape(&connection, root, escape, &mut escape_grabbed)?;
            }
            Event::KeyPress(event) if event.detail == escape && (active || locked) => {
                pending_release = None;
                active = false;
                locked = false;
                dirty = true;
                second_tap = false;
                last_release = None;
                if !send_event(&sender, HotkeyEvent::Cancel)? {
                    break;
                }
            }
            _ => {}
        }
    }
    if escape_grabbed {
        let _ = connection.ungrab_key(escape, root, ModMask::ANY);
    }
    for modifiers in trigger_modifiers {
        let _ = connection.ungrab_key(trigger, root, modifiers);
    }
    let _ = connection.flush();
    Ok(())
}

fn wayland_run(
    sender: SyncSender<HotkeyEvent>,
    stop: Arc<AtomicBool>,
    ready: SyncSender<Result<()>>,
    binding: LinuxHotkey,
    double_tap_enabled: bool,
    started: Arc<AtomicBool>,
) -> Result<()> {
    let trigger = evdev_key(&binding.key)?;
    let required = evdev_modifier_pairs(&binding);
    let mut devices = open_keyboards().wrap_err_with(|| {
        format!(
            "could not open keyboard devices for {}; add your user to the input group",
            binding.label()
        )
    })?;
    tracing::info!(
        keyboards = devices.len(),
        binding = binding.label(),
        "watching evdev keyboards for Wayland dictation"
    );
    ready.send(Ok(())).ok();
    started.store(true, Ordering::Release);

    let mut pressed = std::collections::HashSet::new();
    let mut trigger_down = false;
    let mut active = false;
    let mut second_tap = false;
    let mut locked = false;
    let mut dirty = false;
    let mut last_release = None;
    let mut pending_release: Option<Instant> = None;
    while !stop.load(Ordering::Acquire) {
        if let Some(released_at) = pending_release
            && released_at.elapsed() >= RELEASE_GRACE
        {
            pending_release = None;
            active = false;
            if second_tap {
                second_tap = false;
                locked = true;
                last_release = None;
            } else {
                last_release = double_tap_enabled.then_some(Instant::now());
                if !send_event(&sender, HotkeyEvent::Finish)? {
                    break;
                }
            }
        }

        let mut saw_event = false;
        for device in &mut devices {
            let events = match device.fetch_events() {
                Ok(events) => events,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::Interrupted =>
                {
                    continue;
                }
                Err(error) => {
                    tracing::warn!(%error, "ignoring a failed keyboard device");
                    continue;
                }
            };
            for event in events {
                saw_event = true;
                if event.event_type() != evdev::EventType::KEY {
                    continue;
                }
                if event.value() == 2 {
                    continue;
                }
                let key = evdev::KeyCode::new(event.code());
                let down = event.value() == 1;
                if down {
                    pressed.insert(key);
                } else {
                    pressed.remove(&key);
                }
                if key == evdev::KeyCode::KEY_ESC && down && (active || locked) {
                    pending_release = None;
                    active = false;
                    locked = false;
                    dirty = true;
                    second_tap = false;
                    last_release = None;
                    if !send_event(&sender, HotkeyEvent::Cancel)? {
                        return Ok(());
                    }
                    continue;
                }
                if key != trigger {
                    continue;
                }
                if down {
                    if pending_release.take().is_some() {
                        continue;
                    }
                    if locked {
                        locked = false;
                        dirty = true;
                        if !send_event(&sender, HotkeyEvent::Finish)? {
                            return Ok(());
                        }
                    } else if !active && !dirty && modifiers_match(&pressed, &required) {
                        active = true;
                        second_tap = last_release.take().is_some_and(|released: Instant| {
                            released.elapsed() < DOUBLE_TAP_WINDOW
                        });
                        if !send_event(&sender, HotkeyEvent::Start)? {
                            return Ok(());
                        }
                    }
                    trigger_down = true;
                } else {
                    if active && trigger_down {
                        pending_release = Some(Instant::now());
                    }
                    if dirty {
                        dirty = false;
                    }
                    trigger_down = false;
                }
            }
        }
        if !saw_event {
            thread::sleep(Duration::from_millis(5));
        }
    }
    Ok(())
}

fn open_keyboards() -> Result<Vec<evdev::Device>> {
    let mut devices = Vec::new();
    let entries = std::fs::read_dir("/dev/input").wrap_err("could not read /dev/input")?;
    for entry in entries {
        let path = entry?.path();
        let Some(name) = path.file_name() else {
            continue;
        };
        if !name.to_string_lossy().starts_with("event") {
            continue;
        }
        let Ok(device) = evdev::Device::open(&path) else {
            continue;
        };
        let name = device.name().unwrap_or("");
        if name.to_ascii_lowercase().contains("uinput")
            || name.to_ascii_lowercase().contains("virtual")
        {
            continue;
        }
        let keyboard = device.supported_keys().is_some_and(|keys| {
            keys.contains(evdev::KeyCode::KEY_SPACE)
                && keys.contains(evdev::KeyCode::KEY_A)
                && keys.contains(evdev::KeyCode::KEY_LEFTALT)
        });
        if !keyboard {
            continue;
        }
        device.set_nonblocking(true)?;
        devices.push(device);
    }
    if devices.is_empty() {
        return Err(eyre!(
            "no keyboard event devices are readable; add your user to the input group"
        ));
    }
    Ok(devices)
}

pub(crate) fn capture_next_binding() -> Result<crate::linux_settings::LinuxHotkey> {
    let mut devices = open_keyboards()?;
    let mut pressed = std::collections::HashSet::new();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if Instant::now() >= deadline {
            return Err(eyre!("timed out waiting for a shortcut"));
        }
        let mut saw_event = false;
        for device in &mut devices {
            let events = match device.fetch_events() {
                Ok(events) => events,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::Interrupted =>
                {
                    continue;
                }
                Err(_) => continue,
            };
            for event in events {
                saw_event = true;
                if event.event_type() != evdev::EventType::KEY || event.value() == 2 {
                    continue;
                }
                let key = evdev::KeyCode::new(event.code());
                let down = event.value() == 1;
                if down {
                    pressed.insert(key);
                } else {
                    pressed.remove(&key);
                    continue;
                }
                if key == evdev::KeyCode::KEY_ESC {
                    return Err(eyre!("shortcut capture canceled"));
                }
                if is_evdev_modifier(key) {
                    continue;
                }
                let Some(name) = evdev_key_name(key) else {
                    continue;
                };
                return Ok(crate::linux_settings::LinuxHotkey {
                    control: pressed_modifier(
                        &pressed,
                        evdev::KeyCode::KEY_LEFTCTRL,
                        evdev::KeyCode::KEY_RIGHTCTRL,
                    ),
                    alt: pressed_modifier(
                        &pressed,
                        evdev::KeyCode::KEY_LEFTALT,
                        evdev::KeyCode::KEY_RIGHTALT,
                    ),
                    shift: pressed_modifier(
                        &pressed,
                        evdev::KeyCode::KEY_LEFTSHIFT,
                        evdev::KeyCode::KEY_RIGHTSHIFT,
                    ),
                    super_key: pressed_modifier(
                        &pressed,
                        evdev::KeyCode::KEY_LEFTMETA,
                        evdev::KeyCode::KEY_RIGHTMETA,
                    ),
                    key: name,
                });
            }
        }
        if !saw_event {
            thread::sleep(Duration::from_millis(5));
        }
    }
}

fn is_evdev_modifier(key: evdev::KeyCode) -> bool {
    matches!(
        key,
        evdev::KeyCode::KEY_LEFTCTRL
            | evdev::KeyCode::KEY_RIGHTCTRL
            | evdev::KeyCode::KEY_LEFTALT
            | evdev::KeyCode::KEY_RIGHTALT
            | evdev::KeyCode::KEY_LEFTSHIFT
            | evdev::KeyCode::KEY_RIGHTSHIFT
            | evdev::KeyCode::KEY_LEFTMETA
            | evdev::KeyCode::KEY_RIGHTMETA
            | evdev::KeyCode::KEY_CAPSLOCK
            | evdev::KeyCode::KEY_NUMLOCK
    )
}

fn pressed_modifier(
    pressed: &std::collections::HashSet<evdev::KeyCode>,
    left: evdev::KeyCode,
    right: evdev::KeyCode,
) -> bool {
    pressed.contains(&left) || pressed.contains(&right)
}

fn evdev_key_name(code: evdev::KeyCode) -> Option<String> {
    if code == evdev::KeyCode::KEY_SPACE {
        return Some("space".into());
    }
    if code == evdev::KeyCode::KEY_ENTER {
        return Some("enter".into());
    }
    if code == evdev::KeyCode::KEY_TAB {
        return Some("tab".into());
    }
    if code == evdev::KeyCode::KEY_BACKSPACE {
        return Some("backspace".into());
    }
    if code == evdev::KeyCode::KEY_F5 {
        return Some("f5".into());
    }
    if code == evdev::KeyCode::KEY_F12 {
        return Some("f12".into());
    }
    let value = code.code();
    if (evdev::KeyCode::KEY_A.code()..=evdev::KeyCode::KEY_Z.code()).contains(&value) {
        let letter = b'a' + (value - evdev::KeyCode::KEY_A.code()) as u8;
        return Some(char::from(letter).to_string());
    }
    if (evdev::KeyCode::KEY_F1.code()..=evdev::KeyCode::KEY_F24.code()).contains(&value) {
        return Some(format!("f{}", value - evdev::KeyCode::KEY_F1.code() + 1));
    }
    None
}

fn evdev_key(key: &str) -> Result<evdev::KeyCode> {
    let key = key.to_ascii_lowercase();
    match key.as_str() {
        "space" => Ok(evdev::KeyCode::KEY_SPACE),
        "enter" | "return" => Ok(evdev::KeyCode::KEY_ENTER),
        "tab" => Ok(evdev::KeyCode::KEY_TAB),
        "backspace" => Ok(evdev::KeyCode::KEY_BACKSPACE),
        "escape" | "esc" => Ok(evdev::KeyCode::KEY_ESC),
        "a" => Ok(evdev::KeyCode::KEY_A),
        "b" => Ok(evdev::KeyCode::KEY_B),
        "c" => Ok(evdev::KeyCode::KEY_C),
        "d" => Ok(evdev::KeyCode::KEY_D),
        "e" => Ok(evdev::KeyCode::KEY_E),
        "f" => Ok(evdev::KeyCode::KEY_F),
        "g" => Ok(evdev::KeyCode::KEY_G),
        "h" => Ok(evdev::KeyCode::KEY_H),
        "i" => Ok(evdev::KeyCode::KEY_I),
        "j" => Ok(evdev::KeyCode::KEY_J),
        "k" => Ok(evdev::KeyCode::KEY_K),
        "l" => Ok(evdev::KeyCode::KEY_L),
        "m" => Ok(evdev::KeyCode::KEY_M),
        "n" => Ok(evdev::KeyCode::KEY_N),
        "o" => Ok(evdev::KeyCode::KEY_O),
        "p" => Ok(evdev::KeyCode::KEY_P),
        "q" => Ok(evdev::KeyCode::KEY_Q),
        "r" => Ok(evdev::KeyCode::KEY_R),
        "s" => Ok(evdev::KeyCode::KEY_S),
        "t" => Ok(evdev::KeyCode::KEY_T),
        "u" => Ok(evdev::KeyCode::KEY_U),
        "v" => Ok(evdev::KeyCode::KEY_V),
        "w" => Ok(evdev::KeyCode::KEY_W),
        "x" => Ok(evdev::KeyCode::KEY_X),
        "y" => Ok(evdev::KeyCode::KEY_Y),
        "z" => Ok(evdev::KeyCode::KEY_Z),
        key if key.len() == 1 && key.as_bytes()[0].is_ascii_digit() => {
            let digit = key.as_bytes()[0];
            Ok(if digit == b'0' {
                evdev::KeyCode::KEY_0
            } else {
                evdev::KeyCode::new(evdev::KeyCode::KEY_1.code() + u16::from(digit - b'1'))
            })
        }
        "f1" => Ok(evdev::KeyCode::KEY_F1),
        "f2" => Ok(evdev::KeyCode::KEY_F2),
        "f3" => Ok(evdev::KeyCode::KEY_F3),
        "f4" => Ok(evdev::KeyCode::KEY_F4),
        "f5" => Ok(evdev::KeyCode::KEY_F5),
        "f6" => Ok(evdev::KeyCode::KEY_F6),
        "f7" => Ok(evdev::KeyCode::KEY_F7),
        "f8" => Ok(evdev::KeyCode::KEY_F8),
        "f9" => Ok(evdev::KeyCode::KEY_F9),
        "f10" => Ok(evdev::KeyCode::KEY_F10),
        "f11" => Ok(evdev::KeyCode::KEY_F11),
        "f12" => Ok(evdev::KeyCode::KEY_F12),
        "f13" => Ok(evdev::KeyCode::KEY_F13),
        "f14" => Ok(evdev::KeyCode::KEY_F14),
        "f15" => Ok(evdev::KeyCode::KEY_F15),
        "f16" => Ok(evdev::KeyCode::KEY_F16),
        "f17" => Ok(evdev::KeyCode::KEY_F17),
        "f18" => Ok(evdev::KeyCode::KEY_F18),
        "f19" => Ok(evdev::KeyCode::KEY_F19),
        "f20" => Ok(evdev::KeyCode::KEY_F20),
        "f21" => Ok(evdev::KeyCode::KEY_F21),
        "f22" => Ok(evdev::KeyCode::KEY_F22),
        "f23" => Ok(evdev::KeyCode::KEY_F23),
        "f24" => Ok(evdev::KeyCode::KEY_F24),
        _ => Err(eyre!("unsupported Wayland hotkey key: {key}")),
    }
}

fn evdev_modifier_pairs(binding: &LinuxHotkey) -> Vec<[evdev::KeyCode; 2]> {
    let mut required = Vec::new();
    if binding.control {
        required.push([evdev::KeyCode::KEY_LEFTCTRL, evdev::KeyCode::KEY_RIGHTCTRL]);
    }
    if binding.alt {
        required.push([evdev::KeyCode::KEY_LEFTALT, evdev::KeyCode::KEY_RIGHTALT]);
    }
    if binding.shift {
        required.push([
            evdev::KeyCode::KEY_LEFTSHIFT,
            evdev::KeyCode::KEY_RIGHTSHIFT,
        ]);
    }
    if binding.super_key {
        required.push([evdev::KeyCode::KEY_LEFTMETA, evdev::KeyCode::KEY_RIGHTMETA]);
    }
    required
}

fn modifiers_match(
    pressed: &std::collections::HashSet<evdev::KeyCode>,
    required: &[[evdev::KeyCode; 2]],
) -> bool {
    required
        .iter()
        .all(|pair| pair.iter().any(|key| pressed.contains(key)))
}

fn send_event(sender: &SyncSender<HotkeyEvent>, event: HotkeyEvent) -> Result<bool> {
    match sender.try_send(event) {
        Ok(()) => Ok(true),
        Err(TrySendError::Full(_)) => Err(eyre!("hotkey event queue overflow")),
        Err(TrySendError::Disconnected(_)) => Ok(false),
    }
}

fn release_escape(
    connection: &RustConnection,
    root: Window,
    escape: u8,
    grabbed: &mut bool,
) -> Result<()> {
    if *grabbed {
        connection.ungrab_key(escape, root, ModMask::ANY)?.check()?;
        connection.flush()?;
        *grabbed = false;
    }
    Ok(())
}

fn keysym(key: &str) -> Result<u32> {
    let key = key.to_ascii_lowercase();
    match key.as_str() {
        "space" => Ok(XK_SPACE),
        "enter" | "return" => Ok(0xff0d),
        "tab" => Ok(0xff09),
        "backspace" => Ok(0xff08),
        key if key.len() == 1 && key.as_bytes()[0].is_ascii_graphic() => {
            Ok(u32::from(key.as_bytes()[0]))
        }
        key if key.starts_with('f') => key[1..]
            .parse::<u32>()
            .ok()
            .filter(|number| (1..=24).contains(number))
            .map(|number| 0xffbd + number)
            .ok_or_else(|| eyre!("unsupported X11 function key: {key}")),
        _ => Err(eyre!("unsupported X11 hotkey key: {key}")),
    }
}

fn grab_variants(
    connection: &RustConnection,
    root: Window,
    key: u8,
    modifiers: &[ModMask],
) -> Result<()> {
    let mut grabbed = Vec::new();
    for &modifiers in modifiers {
        let result = connection
            .grab_key(
                false,
                root,
                modifiers,
                key,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )?
            .check();
        if let Err(error) = result {
            for modifiers in grabbed {
                let _ = connection.ungrab_key(key, root, modifiers);
            }
            let _ = connection.flush();
            return Err(error.into());
        }
        grabbed.push(modifiers);
    }
    Ok(())
}

fn lock_variants(base: ModMask, num_lock: ModMask) -> [ModMask; 4] {
    [
        base,
        base | ModMask::LOCK,
        base | num_lock,
        base | ModMask::LOCK | num_lock,
    ]
}

pub(crate) struct Keymap {
    min: u8,
    symbols_per_keycode: usize,
    symbols: Vec<u32>,
}

impl Keymap {
    pub(crate) fn read(connection: &RustConnection) -> Result<Self> {
        let setup = connection.setup();
        let min = setup.min_keycode;
        let count = setup.max_keycode - min + 1;
        let mapping = connection.get_keyboard_mapping(min, count)?.reply()?;
        Ok(Self {
            min,
            symbols_per_keycode: usize::from(mapping.keysyms_per_keycode),
            symbols: mapping.keysyms,
        })
    }

    pub(crate) fn keycode(&self, keysym: u32) -> Result<u8> {
        self.symbols
            .chunks(self.symbols_per_keycode)
            .position(|symbols| symbols.contains(&keysym))
            .map(|index| self.min + index as u8)
            .ok_or_else(|| eyre!("active X11 keymap has no key for keysym {keysym:#x}"))
    }

    pub(crate) fn modifier_for(
        &self,
        connection: &RustConnection,
        keysyms: &[u32],
    ) -> Result<ModMask> {
        let keycodes = keysyms
            .iter()
            .filter_map(|keysym| self.keycode(*keysym).ok())
            .collect::<Vec<_>>();
        let mapping = connection.get_modifier_mapping()?.reply()?;
        let width = usize::from(mapping.keycodes_per_modifier());
        mapping
            .keycodes
            .chunks(width)
            .position(|codes| codes.iter().any(|code| keycodes.contains(code)))
            .map(|index| ModMask::from(1_u16 << index))
            .ok_or_else(|| eyre!("active X11 keymap does not map the requested modifier"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use x11rb::protocol::xtest;

    const KEY_PRESS: u8 = 2;
    const KEY_RELEASE: u8 = 3;
    static X11_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn send_key(connection: &RustConnection, root: Window, type_: u8, key: u8) {
        xtest::fake_input(connection, type_, key, 0, root, 0, 0, 0)
            .unwrap()
            .check()
            .unwrap();
        connection.flush().unwrap();
    }

    #[test]
    fn lock_variants_preserve_the_configured_modifier() {
        assert_eq!(
            lock_variants(ModMask::M1, ModMask::M2),
            [
                ModMask::M1,
                ModMask::M1 | ModMask::LOCK,
                ModMask::M1 | ModMask::M2,
                ModMask::M1 | ModMask::LOCK | ModMask::M2,
            ]
        );
    }

    #[test]
    fn standalone_f12_resolves_to_the_x11_keysym() {
        assert_eq!(keysym("f12").unwrap(), 0xffc9);
    }

    #[test]
    fn wayland_space_and_f12_resolve_to_evdev_codes() {
        assert_eq!(evdev_key("space").unwrap(), evdev::KeyCode::KEY_SPACE);
        assert_eq!(evdev_key("f12").unwrap(), evdev::KeyCode::KEY_F12);
        assert_eq!(evdev_key("a").unwrap(), evdev::KeyCode::KEY_A);
    }

    #[test]
    fn wayland_modifiers_require_configured_keys() {
        let mut pressed = std::collections::HashSet::new();
        let required = evdev_modifier_pairs(&LinuxHotkey::default());
        assert!(!modifiers_match(&pressed, &required));
        pressed.insert(evdev::KeyCode::KEY_LEFTALT);
        assert!(modifiers_match(&pressed, &required));
    }

    #[test]
    #[ignore = "requires the active X11 desktop"]
    fn grabbed_alt_space_delivers_press_and_release() {
        let _guard = X11_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let monitor = LinuxHotkeyMonitor::start_x11(LinuxHotkey::default(), true).unwrap();
        let (connection, screen) = RustConnection::connect(None).unwrap();
        let root = connection.setup().roots[screen].root;
        let keymap = Keymap::read(&connection).unwrap();
        let alt = keymap.keycode(XK_ALT_L).unwrap();
        let space = keymap.keycode(XK_SPACE).unwrap();
        for (type_, key) in [
            (KEY_PRESS, alt),
            (KEY_PRESS, space),
            (KEY_RELEASE, space),
            (KEY_RELEASE, alt),
        ] {
            send_key(&connection, root, type_, key);
        }
        assert_eq!(
            monitor.events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HotkeyEvent::Start
        );
        assert_eq!(
            monitor.events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HotkeyEvent::Finish
        );
    }

    #[test]
    #[ignore = "requires the active X11 desktop"]
    fn second_tap_locks_until_the_trigger_is_pressed_again() {
        let _guard = X11_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let monitor = LinuxHotkeyMonitor::start_x11(LinuxHotkey::default(), true).unwrap();
        let (connection, screen) = RustConnection::connect(None).unwrap();
        let root = connection.setup().roots[screen].root;
        let keymap = Keymap::read(&connection).unwrap();
        let alt = keymap.keycode(XK_ALT_L).unwrap();
        let space = keymap.keycode(XK_SPACE).unwrap();
        let tap = || {
            send_key(&connection, root, KEY_PRESS, alt);
            send_key(&connection, root, KEY_PRESS, space);
            send_key(&connection, root, KEY_RELEASE, space);
            send_key(&connection, root, KEY_RELEASE, alt);
        };

        tap();
        assert_eq!(
            monitor.events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HotkeyEvent::Start
        );
        assert_eq!(
            monitor.events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HotkeyEvent::Finish
        );
        tap();
        assert_eq!(
            monitor.events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HotkeyEvent::Start
        );
        assert!(
            monitor
                .events
                .recv_timeout(Duration::from_millis(100))
                .is_err()
        );
        tap();
        assert_eq!(
            monitor.events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HotkeyEvent::Finish
        );
    }

    #[test]
    #[ignore = "requires the active X11 desktop"]
    fn standalone_function_key_delivers_press_and_release() {
        let _guard = X11_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let binding = LinuxHotkey {
            alt: false,
            key: "f24".into(),
            ..LinuxHotkey::default()
        };
        let monitor = LinuxHotkeyMonitor::start_x11(binding, false).unwrap();
        let (connection, screen) = RustConnection::connect(None).unwrap();
        let root = connection.setup().roots[screen].root;
        let trigger = Keymap::read(&connection)
            .unwrap()
            .keycode(keysym("f24").unwrap())
            .unwrap();
        send_key(&connection, root, KEY_PRESS, trigger);
        send_key(&connection, root, KEY_RELEASE, trigger);

        assert_eq!(
            monitor.events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HotkeyEvent::Start
        );
        assert_eq!(
            monitor.events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HotkeyEvent::Finish
        );
    }
}

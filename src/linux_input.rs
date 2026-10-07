use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::ErrorKind;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{ConnectionExt, GrabMode, ModMask, Window};
use x11rb::rust_connection::RustConnection;

use crate::linux_session::LinuxSession;
use crate::linux_settings::LinuxHotkey;

pub(crate) use crate::linux_wayland_input::{WaylandModifierState, capture_wayland_binding};

const XK_SPACE: u32 = 0x20;
const XK_ESCAPE: u32 = 0xff1b;
pub(crate) const XK_SHIFT_L: u32 = 0xffe1;
pub(crate) const XK_SHIFT_R: u32 = 0xffe2;
pub(crate) const XK_CONTROL_L: u32 = 0xffe3;
pub(crate) const XK_CONTROL_R: u32 = 0xffe4;
pub(crate) const XK_ALT_L: u32 = 0xffe9;
pub(crate) const XK_ALT_R: u32 = 0xffea;
pub(crate) const XK_SUPER_L: u32 = 0xffeb;
pub(crate) const XK_SUPER_R: u32 = 0xffec;
const XK_NUM_LOCK: u32 = 0xff7f;
pub(crate) const KEY_PRESS: u8 = 2;
pub(crate) const KEY_RELEASE: u8 = 3;
const RELEASE_GRACE: Duration = Duration::from_millis(50);
pub(crate) const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(300);

pub(crate) fn connect_x11() -> Result<(RustConnection, Window, Keymap)> {
    let (connection, screen) =
        RustConnection::connect(None).wrap_err("could not connect to X11")?;
    let root = connection.setup().roots[screen].root;
    let keymap = Keymap::read(&connection)?;
    Ok((connection, root, keymap))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeyEvent {
    Start,
    Finish,
    Cancel,
}

pub(crate) const ESCAPE_CANCEL_WARNING: &str =
    "Escape cancel unavailable: another application holds Escape";

#[derive(Clone, Debug, Default)]
pub struct HotkeyStatus {
    escape_unavailable: Arc<AtomicBool>,
}

impl HotkeyStatus {
    pub(crate) fn warning(&self) -> Option<String> {
        self.escape_unavailable
            .load(Ordering::Acquire)
            .then(|| ESCAPE_CANCEL_WARNING.into())
    }

    pub(crate) fn update_escape_availability(&self, available: bool) -> bool {
        let was_unavailable = self.escape_unavailable.swap(!available, Ordering::AcqRel);
        !available && !was_unavailable
    }
}

pub struct LinuxHotkeyMonitor {
    pub events: Receiver<HotkeyEvent>,
    pub errors: Receiver<String>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    wayland_modifiers: Option<WaylandModifierState>,
}

impl LinuxHotkeyMonitor {
    pub fn start(
        binding: LinuxHotkey,
        double_tap_enabled: bool,
        status: HotkeyStatus,
    ) -> Result<Self> {
        Self::start_for(binding, double_tap_enabled, LinuxSession::detect(), status)
    }

    fn start_for(
        binding: LinuxHotkey,
        double_tap_enabled: bool,
        session: LinuxSession,
        status: HotkeyStatus,
    ) -> Result<Self> {
        let (events_sender, events) = mpsc::sync_channel(16);
        let (error_sender, errors) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let started = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_started = started.clone();
        let wayland_modifiers = session.is_wayland().then(WaylandModifierState::default);
        let worker_modifiers = wayland_modifiers.clone();
        let worker = thread::spawn(move || {
            let result = match session {
                LinuxSession::X11 => run(
                    events_sender,
                    worker_stop,
                    ready_sender.clone(),
                    binding,
                    double_tap_enabled,
                    worker_started.clone(),
                    status,
                ),
                LinuxSession::Wayland => crate::linux_wayland_input::run(
                    events_sender,
                    worker_stop,
                    ready_sender.clone(),
                    binding,
                    double_tap_enabled,
                    worker_started.clone(),
                    worker_modifiers.expect("Wayland modifier state exists"),
                ),
            };
            if let Err(error) = result {
                let message = format!("{error:#}");
                if worker_started.load(Ordering::Acquire) {
                    let _ = error_sender.send(message);
                } else {
                    let _ = ready_sender.try_send(Err(eyre!(message)));
                }
            }
        });
        // Own the worker before waiting so every startup failure stops and joins it.
        let monitor = Self {
            events,
            errors,
            stop,
            worker: Some(worker),
            wayland_modifiers,
        };
        ready_receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| eyre!("timed out registering the Linux dictation shortcut"))??;
        Ok(monitor)
    }

    pub(crate) fn wayland_modifiers(&self) -> Option<WaylandModifierState> {
        self.wayland_modifiers.clone()
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

fn run(
    sender: SyncSender<HotkeyEvent>,
    stop: Arc<AtomicBool>,
    ready: SyncSender<Result<()>>,
    binding: LinuxHotkey,
    double_tap_enabled: bool,
    started: Arc<AtomicBool>,
    status: HotkeyStatus,
) -> Result<()> {
    let (connection, root, keymap) = connect_x11()?;
    let trigger = keymap.keycode(keysym(&binding.key)?)?;
    let escape = keymap.keycode(XK_ESCAPE)?;
    let modifiers = binding.modifier_mask(&connection, &keymap)?;
    let num_lock = keymap
        .modifier_for(&connection, &[XK_NUM_LOCK])
        .unwrap_or(ModMask::M2);
    let trigger_modifiers = lock_variants(modifiers, num_lock);
    let escape_modifiers = lock_variants(ModMask::default(), num_lock);
    grab_variants(&connection, root, trigger, &trigger_modifiers).wrap_err_with(|| {
        format!(
            "{} is already in use by another X11 client",
            binding.label()
        )
    })?;
    connection.flush()?;
    started.store(true, Ordering::Release);
    ready.try_send(Ok(())).ok();

    let mut active = false;
    let mut second_tap = false;
    let mut locked = false;
    let mut dirty = false;
    let mut last_release = None;
    let mut escape_grabbed = Vec::new();
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
                    escape_grabbed = grab_escape(&connection, root, escape, &escape_modifiers)?;
                    if status.update_escape_availability(!escape_grabbed.is_empty()) {
                        tracing::warn!("{ESCAPE_CANCEL_WARNING}; continuing without Escape-cancel");
                    }
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
            Event::KeyPress(event)
                if event.detail == escape && !escape_grabbed.is_empty() && (active || locked) =>
            {
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
    for modifiers in escape_grabbed {
        let _ = connection.ungrab_key(escape, root, modifiers);
    }
    for modifiers in trigger_modifiers {
        let _ = connection.ungrab_key(trigger, root, modifiers);
    }
    let _ = connection.flush();
    Ok(())
}

pub(crate) fn send_event(sender: &SyncSender<HotkeyEvent>, event: HotkeyEvent) -> Result<bool> {
    match sender.try_send(event) {
        Ok(()) => Ok(true),
        Err(TrySendError::Full(_)) => Err(eyre!("Linux hotkey event queue overflow")),
        Err(TrySendError::Disconnected(_)) => Ok(false),
    }
}

/// True when the server refused a passive grab because another client holds it.
/// `GrabKey` with a conflicting grab fails with `BadAccess` (`Access`); any
/// other error (connection loss, bad value) must stay fatal.
fn is_grab_conflict(error: &ReplyError) -> bool {
    matches!(
        error,
        ReplyError::X11Error(error) if error.error_kind == ErrorKind::Access
    )
}

fn grab_escape(
    connection: &RustConnection,
    root: Window,
    escape: u8,
    bare_modifiers: &[ModMask],
) -> Result<Vec<ModMask>> {
    match grab_variants(connection, root, escape, &[ModMask::ANY]) {
        Ok(()) => Ok(vec![ModMask::ANY]),
        Err(error) if is_grab_conflict(&error) => {
            // A modified Escape shortcut must not prevent grabbing plain Escape.
            match grab_variants(connection, root, escape, bare_modifiers) {
                Ok(()) => Ok(bare_modifiers.to_vec()),
                Err(error) if is_grab_conflict(&error) => Ok(Vec::new()),
                Err(error) => Err(error).wrap_err("could not grab the Escape key for cancel"),
            }
        }
        Err(error) => Err(error).wrap_err("could not grab the Escape key for cancel"),
    }
}

fn release_escape(
    connection: &RustConnection,
    root: Window,
    escape: u8,
    grabbed: &mut Vec<ModMask>,
) -> Result<()> {
    if !grabbed.is_empty() {
        for &modifiers in grabbed.iter() {
            connection.ungrab_key(escape, root, modifiers)?.check()?;
        }
        connection.flush()?;
        grabbed.clear();
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
) -> Result<(), ReplyError> {
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
            )
            .map_err(ReplyError::from)
            .and_then(|cookie| cookie.check());
        if let Err(error) = result {
            for modifiers in grabbed {
                let _ = connection.ungrab_key(key, root, modifiers);
            }
            let _ = connection.flush();
            return Err(error);
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
    use std::sync::{Mutex, MutexGuard};
    use x11rb::protocol::xtest;

    static X11_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct X11TestHarness {
        _guard: MutexGuard<'static, ()>,
        connection: RustConnection,
        root: Window,
        keymap: Keymap,
        alt: u8,
        space: u8,
        escape: u8,
    }

    impl X11TestHarness {
        fn new() -> Self {
            let guard = X11_TEST_LOCK
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let (connection, root, keymap) = connect_x11().unwrap();
            let alt = keymap.keycode(XK_ALT_L).unwrap();
            let space = keymap.keycode(XK_SPACE).unwrap();
            let escape = keymap.keycode(XK_ESCAPE).unwrap();
            Self {
                _guard: guard,
                connection,
                root,
                keymap,
                alt,
                space,
                escape,
            }
        }

        fn send_key(&self, type_: u8, key: u8) {
            send_key(&self.connection, self.root, type_, key);
        }

        fn tap_alt_space(&self) {
            for (type_, key) in [
                (KEY_PRESS, self.alt),
                (KEY_PRESS, self.space),
                (KEY_RELEASE, self.space),
                (KEY_RELEASE, self.alt),
            ] {
                self.send_key(type_, key);
            }
        }
    }

    fn recv_event(monitor: &LinuxHotkeyMonitor) -> HotkeyEvent {
        monitor.events.recv_timeout(Duration::from_secs(1)).unwrap()
    }

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
    fn full_event_queue_returns_an_error_instead_of_blocking_the_worker() {
        let (sender, receiver) = mpsc::sync_channel(1);
        assert!(send_event(&sender, HotkeyEvent::Start).unwrap());
        assert!(send_event(&sender, HotkeyEvent::Finish).is_err());
        assert_eq!(receiver.try_recv().unwrap(), HotkeyEvent::Start);
        drop(receiver);
        assert!(!send_event(&sender, HotkeyEvent::Cancel).unwrap());
    }

    fn grab_error(kind: ErrorKind) -> ReplyError {
        ReplyError::X11Error(x11rb::x11_utils::X11Error {
            error_kind: kind,
            error_code: 10,
            sequence: 12,
            bad_value: 1722,
            minor_opcode: 0,
            major_opcode: 33,
            extension_name: None,
            request_name: Some("GrabKey"),
        })
    }

    #[test]
    fn escape_grab_conflict_is_not_fatal() {
        assert!(is_grab_conflict(&grab_error(ErrorKind::Access)));
    }

    #[test]
    fn non_conflict_grab_errors_stay_fatal() {
        for kind in [ErrorKind::Value, ErrorKind::Window, ErrorKind::Match] {
            assert!(
                !is_grab_conflict(&grab_error(kind)),
                "{kind:?} must stay fatal"
            );
        }
    }

    #[test]
    fn escape_warning_is_logged_once_per_episode_and_clears_on_recovery() {
        let status = HotkeyStatus::default();
        let observer = status.clone();
        assert!(observer.warning().is_none());
        assert!(status.update_escape_availability(false));
        assert_eq!(observer.warning().as_deref(), Some(ESCAPE_CANCEL_WARNING));
        assert!(!status.update_escape_availability(false));
        assert!(!status.update_escape_availability(true));
        assert!(observer.warning().is_none());
        assert!(!status.update_escape_availability(true));
        assert!(status.update_escape_availability(false));
    }

    #[test]
    fn dropping_a_monitor_stops_and_joins_its_worker() {
        let (_, events) = mpsc::sync_channel(1);
        let (_, errors) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_stopped = stopped.clone();
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                thread::yield_now();
            }
            worker_stopped.store(true, Ordering::Release);
        });
        drop(LinuxHotkeyMonitor {
            events,
            errors,
            stop,
            worker: Some(worker),
            wayland_modifiers: None,
        });
        assert!(stopped.load(Ordering::Acquire));
    }

    #[test]
    #[ignore = "requires the active X11 desktop"]
    fn grabbed_alt_space_delivers_press_and_release() {
        let harness = X11TestHarness::new();
        let monitor = LinuxHotkeyMonitor::start_for(
            LinuxHotkey::default(),
            true,
            LinuxSession::X11,
            Default::default(),
        )
        .unwrap();
        harness.tap_alt_space();
        assert_eq!(recv_event(&monitor), HotkeyEvent::Start);
        assert_eq!(recv_event(&monitor), HotkeyEvent::Finish);
    }

    #[test]
    #[ignore = "requires the active X11 desktop"]
    fn second_tap_locks_until_the_trigger_is_pressed_again() {
        let harness = X11TestHarness::new();
        let monitor = LinuxHotkeyMonitor::start_for(
            LinuxHotkey::default(),
            true,
            LinuxSession::X11,
            Default::default(),
        )
        .unwrap();

        harness.tap_alt_space();
        assert_eq!(recv_event(&monitor), HotkeyEvent::Start);
        assert_eq!(recv_event(&monitor), HotkeyEvent::Finish);
        harness.tap_alt_space();
        assert_eq!(recv_event(&monitor), HotkeyEvent::Start);
        assert!(
            monitor
                .events
                .recv_timeout(Duration::from_millis(100))
                .is_err()
        );
        harness.tap_alt_space();
        assert_eq!(recv_event(&monitor), HotkeyEvent::Finish);
    }

    #[test]
    #[ignore = "requires the active X11 desktop"]
    fn standalone_function_key_delivers_press_and_release() {
        let harness = X11TestHarness::new();
        let binding = LinuxHotkey {
            alt: false,
            key: "f24".into(),
            ..LinuxHotkey::default()
        };
        let monitor =
            LinuxHotkeyMonitor::start_for(binding, false, LinuxSession::X11, Default::default())
                .unwrap();
        let trigger = harness.keymap.keycode(keysym("f24").unwrap()).unwrap();
        harness.send_key(KEY_PRESS, trigger);
        harness.send_key(KEY_RELEASE, trigger);

        assert_eq!(recv_event(&monitor), HotkeyEvent::Start);
        assert_eq!(recv_event(&monitor), HotkeyEvent::Finish);
    }

    #[test]
    #[ignore = "requires the active X11 desktop"]
    fn conflicted_escape_still_dictates_without_cancel() {
        let harness = X11TestHarness::new();
        grab_variants(
            &harness.connection,
            harness.root,
            harness.escape,
            &[ModMask::ANY],
        )
        .unwrap();
        harness.connection.flush().unwrap();
        let status = HotkeyStatus::default();
        let monitor = LinuxHotkeyMonitor::start_for(
            LinuxHotkey::default(),
            false,
            LinuxSession::X11,
            status.clone(),
        )
        .unwrap();
        for _ in 0..2 {
            harness.tap_alt_space();
            assert_eq!(recv_event(&monitor), HotkeyEvent::Start);
            assert_eq!(status.warning().as_deref(), Some(ESCAPE_CANCEL_WARNING));
            assert_eq!(recv_event(&monitor), HotkeyEvent::Finish);
            assert!(
                monitor
                    .events
                    .recv_timeout(Duration::from_millis(100))
                    .is_err()
            );
            assert!(monitor.errors.try_recv().is_err());
        }
        harness
            .connection
            .ungrab_key(harness.escape, harness.root, ModMask::ANY)
            .unwrap();
        harness.connection.flush().unwrap();
        harness.send_key(KEY_PRESS, harness.alt);
        harness.send_key(KEY_PRESS, harness.space);
        assert_eq!(recv_event(&monitor), HotkeyEvent::Start);
        assert!(status.warning().is_none());
        harness.send_key(KEY_PRESS, harness.escape);
        assert_eq!(recv_event(&monitor), HotkeyEvent::Cancel);
        harness.send_key(KEY_RELEASE, harness.escape);
        harness.send_key(KEY_RELEASE, harness.space);
        harness.send_key(KEY_RELEASE, harness.alt);
    }

    #[test]
    #[ignore = "requires an isolated X11 display"]
    fn modified_escape_conflict_keeps_plain_escape_cancel() {
        let harness = X11TestHarness::new();
        grab_variants(
            &harness.connection,
            harness.root,
            harness.escape,
            &[ModMask::M4],
        )
        .unwrap();
        let status = HotkeyStatus::default();
        let monitor = LinuxHotkeyMonitor::start_for(
            LinuxHotkey::default(),
            true,
            LinuxSession::X11,
            status.clone(),
        )
        .unwrap();
        for expected in [HotkeyEvent::Start, HotkeyEvent::Finish, HotkeyEvent::Start] {
            if expected != HotkeyEvent::Finish {
                harness.tap_alt_space();
            }
            assert_eq!(recv_event(&monitor), expected);
        }
        // The second tap locks recording and releases the shortcut modifiers.
        assert!(
            monitor
                .events
                .recv_timeout(Duration::from_millis(100))
                .is_err()
        );
        assert!(status.warning().is_none());
        harness.send_key(KEY_PRESS, harness.escape);
        assert_eq!(recv_event(&monitor), HotkeyEvent::Cancel);
        harness.send_key(KEY_RELEASE, harness.escape);
        drop(monitor);
        let variants = lock_variants(ModMask::default(), ModMask::M2);
        grab_variants(&harness.connection, harness.root, harness.escape, &variants).unwrap();
    }

    #[test]
    #[ignore = "requires an isolated X11 display"]
    fn conflicted_escape_fallback_releases_partial_grabs() {
        let harness = X11TestHarness::new();
        grab_variants(
            &harness.connection,
            harness.root,
            harness.escape,
            &[ModMask::LOCK],
        )
        .unwrap();
        let (connection, _) = RustConnection::connect(None).unwrap();
        let variants = lock_variants(ModMask::default(), ModMask::M2);
        assert!(
            grab_escape(&connection, harness.root, harness.escape, &variants)
                .unwrap()
                .is_empty()
        );
        // Plain Escape was grabbed before the lock variant failed. It must be freed.
        grab_variants(&harness.connection, harness.root, harness.escape, &variants).unwrap();
        assert!(grab_escape(&connection, 0, harness.escape, &variants).is_err());
    }
}

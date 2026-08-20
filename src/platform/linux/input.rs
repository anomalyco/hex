use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{ConnectionExt, GrabMode, KeyButMask, ModMask, Window};
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
pub enum CaptureTarget {
    Dictation,
    VoiceAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeyEvent {
    Start(CaptureTarget),
    Finish(CaptureTarget),
    Cancel(CaptureTarget),
}

pub struct X11HotkeyMonitor {
    pub events: Receiver<HotkeyEvent>,
    pub errors: Receiver<String>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl X11HotkeyMonitor {
    pub fn start(
        dictation: LinuxHotkey,
        voice_action: Option<LinuxHotkey>,
        double_tap_enabled: bool,
    ) -> Result<Self> {
        let (events_sender, events) = mpsc::sync_channel(16);
        let (error_sender, errors) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let started = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_started = started.clone();
        let worker = thread::spawn(move || {
            let result = run(
                events_sender,
                worker_stop,
                ready_sender.clone(),
                dictation,
                voice_action,
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
            .map_err(|_| eyre!("timed out registering shortcuts with X11"))??;
        Ok(Self {
            events,
            errors,
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for X11HotkeyMonitor {
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
    dictation: LinuxHotkey,
    voice_action: Option<LinuxHotkey>,
    double_tap_enabled: bool,
    started: Arc<AtomicBool>,
) -> Result<()> {
    let (connection, screen) =
        RustConnection::connect(None).wrap_err("could not connect to X11")?;
    let root = connection.setup().roots[screen].root;
    let keymap = Keymap::read(&connection)?;
    let escape = keymap.keycode(XK_ESCAPE)?;
    let num_lock = keymap
        .modifier_for(&connection, &[XK_NUM_LOCK])
        .unwrap_or(ModMask::M2);
    let dictation = Trigger::new(
        CaptureTarget::Dictation,
        dictation,
        &connection,
        &keymap,
        num_lock,
    )?;
    dictation.grab(&connection, root)?;
    let voice_action = voice_action
        .map(|binding| {
            Trigger::new(
                CaptureTarget::VoiceAction,
                binding,
                &connection,
                &keymap,
                num_lock,
            )
        })
        .transpose()?;
    if let Some(voice_action) = &voice_action
        && let Err(error) = voice_action.grab(&connection, root)
    {
        dictation.ungrab(&connection, root);
        return Err(error);
    }
    let triggers = [Some(dictation), voice_action];
    connection.flush()?;
    ready.send(Ok(())).ok();
    started.store(true, Ordering::Release);

    let mut active = None;
    let mut second_tap = false;
    let mut locked = false;
    let mut dirty_until = None;
    let mut last_release = None;
    let mut escape_grabbed = false;
    let mut pending_release: Option<(Instant, CaptureTarget)> = None;
    while !stop.load(Ordering::Acquire) {
        if let Some((released_at, target)) = pending_release
            && released_at.elapsed() >= RELEASE_GRACE
        {
            pending_release = None;
            active = None;
            if second_tap {
                second_tap = false;
                locked = true;
                last_release = None;
            } else {
                last_release = (target == CaptureTarget::Dictation && double_tap_enabled)
                    .then_some(Instant::now());
                release_escape(&connection, root, escape, &mut escape_grabbed)?;
                if !send_event(&sender, HotkeyEvent::Finish(target))? {
                    break;
                }
            }
        }
        let Some(event) = connection.poll_for_event()? else {
            thread::sleep(Duration::from_millis(5));
            continue;
        };
        match event {
            Event::KeyPress(event) if trigger_for_press(&triggers, &event, num_lock).is_some() => {
                let trigger =
                    trigger_for_press(&triggers, &event, num_lock).expect("guard found a trigger");
                if pending_release.is_some_and(|(_, target)| target == trigger.target) {
                    pending_release = None;
                    continue;
                }
                if locked {
                    if trigger.target == CaptureTarget::Dictation {
                        locked = false;
                        dirty_until = Some(trigger.key);
                        release_escape(&connection, root, escape, &mut escape_grabbed)?;
                        if !send_event(&sender, HotkeyEvent::Finish(CaptureTarget::Dictation))? {
                            break;
                        }
                    }
                } else if active.is_none() && dirty_until.is_none() {
                    active = Some(trigger.target);
                    second_tap = trigger.target == CaptureTarget::Dictation
                        && last_release.take().is_some_and(|released: Instant| {
                            released.elapsed() < DOUBLE_TAP_WINDOW
                        });
                    if trigger.target == CaptureTarget::VoiceAction {
                        last_release = None;
                    }
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
                    if !send_event(&sender, HotkeyEvent::Start(trigger.target))? {
                        break;
                    }
                }
            }
            Event::KeyRelease(event)
                if active.is_some_and(|target| {
                    trigger_for_target(&triggers, target).key == event.detail
                }) =>
            {
                pending_release = active.map(|target| (Instant::now(), target));
            }
            Event::KeyRelease(event) if dirty_until == Some(event.detail) => {
                dirty_until = None;
            }
            Event::KeyPress(event) if event.detail == escape && (active.is_some() || locked) => {
                let target = active.unwrap_or(CaptureTarget::Dictation);
                let dirty_key = active.map(|target| trigger_for_target(&triggers, target).key);
                pending_release = None;
                active = None;
                locked = false;
                dirty_until = dirty_key;
                second_tap = false;
                last_release = None;
                release_escape(&connection, root, escape, &mut escape_grabbed)?;
                if !send_event(&sender, HotkeyEvent::Cancel(target))? {
                    break;
                }
            }
            _ => {}
        }
    }
    if escape_grabbed {
        let _ = connection.ungrab_key(escape, root, ModMask::ANY);
    }
    for trigger in triggers.into_iter().flatten() {
        trigger.ungrab(&connection, root);
    }
    let _ = connection.flush();
    Ok(())
}

struct Trigger {
    target: CaptureTarget,
    label: String,
    key: u8,
    modifiers: ModMask,
    variants: [ModMask; 4],
}

impl Trigger {
    fn new(
        target: CaptureTarget,
        binding: LinuxHotkey,
        connection: &RustConnection,
        keymap: &Keymap,
        num_lock: ModMask,
    ) -> Result<Self> {
        let key = keymap.keycode(keysym(&binding.key)?)?;
        let modifiers = binding.modifier_mask(connection, keymap)?;
        Ok(Self {
            target,
            label: binding.label(),
            key,
            modifiers,
            variants: lock_variants(modifiers, num_lock),
        })
    }

    fn grab(&self, connection: &RustConnection, root: Window) -> Result<()> {
        grab_variants(connection, root, self.key, &self.variants)
            .wrap_err_with(|| format!("{} is already in use by another X11 client", self.label))
    }

    fn ungrab(&self, connection: &RustConnection, root: Window) {
        for modifiers in self.variants {
            let _ = connection.ungrab_key(self.key, root, modifiers);
        }
    }

    fn matches_press(&self, detail: u8, state: KeyButMask, num_lock: ModMask) -> bool {
        if detail != self.key {
            return false;
        }
        let ignored = u16::from(ModMask::LOCK | num_lock);
        let state = u16::from(state) & 0xff & !ignored;
        state == u16::from(self.modifiers)
    }
}

fn trigger_for_press<'a>(
    triggers: &'a [Option<Trigger>; 2],
    event: &x11rb::protocol::xproto::KeyPressEvent,
    num_lock: ModMask,
) -> Option<&'a Trigger> {
    triggers
        .iter()
        .flatten()
        .find(|trigger| trigger.matches_press(event.detail, event.state, num_lock))
}

fn trigger_for_target(triggers: &[Option<Trigger>; 2], target: CaptureTarget) -> &Trigger {
    triggers
        .iter()
        .flatten()
        .find(|trigger| trigger.target == target)
        .expect("active capture target has a registered trigger")
}

fn send_event(sender: &SyncSender<HotkeyEvent>, event: HotkeyEvent) -> Result<bool> {
    match sender.try_send(event) {
        Ok(()) => Ok(true),
        Err(TrySendError::Full(_)) => Err(eyre!("X11 hotkey event queue overflow")),
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
    fn trigger_matching_ignores_locks_but_not_other_shortcut_modifiers() {
        let trigger = Trigger {
            target: CaptureTarget::VoiceAction,
            label: "Alt+I".into(),
            key: 31,
            modifiers: ModMask::M1,
            variants: lock_variants(ModMask::M1, ModMask::M2),
        };

        assert!(trigger.matches_press(
            31,
            KeyButMask::MOD1 | KeyButMask::LOCK | KeyButMask::MOD2,
            ModMask::M2,
        ));
        assert!(!trigger.matches_press(31, KeyButMask::MOD1 | KeyButMask::MOD4, ModMask::M2,));
    }

    #[test]
    fn standalone_f12_resolves_to_the_x11_keysym() {
        assert_eq!(keysym("f12").unwrap(), 0xffc9);
    }

    #[test]
    #[ignore = "requires the active X11 desktop"]
    fn grabbed_alt_space_delivers_press_and_release() {
        let _guard = X11_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let monitor = X11HotkeyMonitor::start(LinuxHotkey::default(), None, true).unwrap();
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
            HotkeyEvent::Start(CaptureTarget::Dictation)
        );
        assert_eq!(
            monitor.events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HotkeyEvent::Finish(CaptureTarget::Dictation)
        );
    }

    #[test]
    #[ignore = "requires the active X11 desktop"]
    fn second_tap_locks_until_the_trigger_is_pressed_again() {
        let _guard = X11_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let monitor = X11HotkeyMonitor::start(LinuxHotkey::default(), None, true).unwrap();
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
            HotkeyEvent::Start(CaptureTarget::Dictation)
        );
        assert_eq!(
            monitor.events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HotkeyEvent::Finish(CaptureTarget::Dictation)
        );
        tap();
        assert_eq!(
            monitor.events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HotkeyEvent::Start(CaptureTarget::Dictation)
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
            HotkeyEvent::Finish(CaptureTarget::Dictation)
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
        let monitor = X11HotkeyMonitor::start(binding, None, false).unwrap();
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
            HotkeyEvent::Start(CaptureTarget::Dictation)
        );
        assert_eq!(
            monitor.events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HotkeyEvent::Finish(CaptureTarget::Dictation)
        );
    }

    #[test]
    #[ignore = "requires the active X11 desktop"]
    fn voice_action_binding_delivers_targeted_press_and_release() {
        let _guard = X11_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let voice_action = LinuxHotkey {
            alt: false,
            key: "f24".into(),
            ..LinuxHotkey::default()
        };
        let monitor =
            X11HotkeyMonitor::start(LinuxHotkey::default(), Some(voice_action), true).unwrap();
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
            HotkeyEvent::Start(CaptureTarget::VoiceAction)
        );
        assert_eq!(
            monitor.events.recv_timeout(Duration::from_secs(1)).unwrap(),
            HotkeyEvent::Finish(CaptureTarget::VoiceAction)
        );
    }
}

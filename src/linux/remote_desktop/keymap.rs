//! The compositor's keymap, read from a `wl_seat` keyboard, and the one
//! question paste asks of it: which key types a letter in every layout group.
//!
//! A keycode names a physical key, so injecting one reaches the focused app the
//! way pressing that key does. Which key carries a letter still differs between
//! layouts — Dvorak puts `v` on `KEY_DOT`, AZERTY puts `a` on `KEY_Q` — so a
//! fixed code is wrong there. [`resolve`] names one code only when every group
//! that has the letter agrees on it; otherwise paste sends the keysym, which
//! the desktop resolves in the group that is active.

use std::io::{ErrorKind, Read};
use std::os::fd::OwnedFd;
use std::thread;
use std::time::{Duration, Instant};

use wayland_client::backend::WaylandError;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_keyboard::{self, WlKeyboard};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_seat::{self, WlSeat};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use xkbcommon::xkb;

/// One layout group: each key's evdev code and the symbol it types at level 1.
pub(crate) type Group = Vec<(u16, u32)>;
/// Every layout group of one keymap, in the keymap's order.
pub(crate) type Groups = Vec<Group>;

/// X11 keycodes count from 8 and evdev from 0, which the portal takes.
const EVDEV_OFFSET: u32 = 8;
/// The whole read: a seat's capabilities and its keymap in one roundtrip on a
/// healthy compositor, and no paste ever waits longer for it.
const TIMEOUT: Duration = Duration::from_millis(250);
/// How often the bounded wait retries its non-blocking read.
const POLL: Duration = Duration::from_millis(2);
/// No compositor sends a keymap anywhere near this, and an absurd size must not
/// be allocated.
const MAX_KEYMAP: usize = 4 * 1024 * 1024;

/// Reads the desktop's keymap, or `None` for any failure: no Wayland
/// connection, no keyboard, no keymap event within [`TIMEOUT`], or a keymap
/// xkbcommon rejects. Every caller has a keysym path to fall back on.
pub(crate) fn read() -> Option<Groups> {
    let connection = Connection::connect_to_env().ok()?;
    let (globals, mut queue) = registry_queue_init::<Seat>(&connection).ok()?;
    let handle = queue.handle();
    // Kept for the whole read: the keyboard comes from this seat's
    // capabilities, which `Seat` handles.
    let _seat: WlSeat = globals
        .bind(&handle, 1..=WlSeat::interface().version, ())
        .ok()?;
    let mut state = Seat::default();
    wait(&connection, &mut queue, &mut state)?;
    release(&mut state);
    let (fd, size) = state.keymap?;
    parse_keymap(fd, size)
}

/// Waits for the keymap event without ever blocking a paste: the socket is
/// read non-blocking, and the wait ends at [`TIMEOUT`].
fn wait(connection: &Connection, queue: &mut EventQueue<Seat>, state: &mut Seat) -> Option<()> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        queue.dispatch_pending(state).ok()?;
        if state.keymap.is_some() {
            return Some(());
        }
        if Instant::now() >= deadline {
            return None;
        }
        connection.flush().ok()?;
        let guard = connection.prepare_read()?;
        match guard.read() {
            Ok(_) => {}
            Err(WaylandError::Io(error)) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(POLL);
            }
            Err(_) => return None,
        }
    }
}

/// Destroys the keyboard now that its keymap is in hand; dropping the
/// connection destroys anything left.
fn release(state: &mut Seat) {
    if let Some(keyboard) = state.keyboard.take()
        && keyboard.version() >= 3
    {
        keyboard.release();
    }
}

fn parse_keymap(fd: OwnedFd, size: u32) -> Option<Groups> {
    let size = usize::try_from(size)
        .ok()
        .filter(|size| (1..=MAX_KEYMAP).contains(size))?;
    // A safe, bounded read: the compositor's fd becomes a File, and `take`
    // keeps a keymap that lied about its size from growing the string.
    let mut text = String::with_capacity(size);
    std::fs::File::from(fd)
        .take(size as u64)
        .read_to_string(&mut text)
        .ok()?;
    parse(&text)
}

/// Parses an xkb keymap into each layout group's level-1 positions.
///
/// Level 1 is what a key types with no Shift held, which is the symbol a
/// Ctrl+letter chord names; a letter reachable only through Shift or a level
/// the person changed is not the letter that chord means.
fn parse(text: &str) -> Option<Groups> {
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let keymap = xkb::Keymap::new_from_string(
        &context,
        text.to_owned(),
        xkb::KEYMAP_FORMAT_TEXT_V1,
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    )?;
    let mut groups = Groups::new();
    for layout in 0..keymap.num_layouts() {
        let mut keys = Group::new();
        for raw in keymap.min_keycode().raw()..=keymap.max_keycode().raw() {
            let Some(code) = raw
                .checked_sub(EVDEV_OFFSET)
                .and_then(|code| u16::try_from(code).ok())
            else {
                continue;
            };
            let symbols = keymap.key_get_syms_by_level(xkb::Keycode::new(raw), layout, 0);
            let Some(keysym) = symbols.first() else {
                continue;
            };
            keys.push((code, keysym.raw()));
        }
        if !keys.is_empty() {
            groups.push(keys);
        }
    }
    (!groups.is_empty()).then_some(groups)
}

/// The one evdev code that carries `keysym` at level 1 in every group that has
/// it. `None` when no group has it, one group has it on two keys, or groups
/// disagree (`us,us(dvorak)`), so the caller sends the keysym instead.
pub(crate) fn resolve(groups: &Groups, keysym: u32) -> Option<u16> {
    let mut agreed = None;
    for group in groups {
        let mut codes = group
            .iter()
            .filter(|(_, symbol)| *symbol == keysym)
            .map(|(code, _)| *code);
        let Some(code) = codes.next() else {
            continue;
        };
        if codes.next().is_some() {
            return None;
        }
        match agreed {
            Some(agreed) if agreed != code => return None,
            _ => agreed = Some(code),
        }
    }
    agreed
}

/// The state a one-shot keymap read collects: the seat's keyboard and its
/// keymap event.
#[derive(Default)]
struct Seat {
    keyboard: Option<WlKeyboard>,
    keymap: Option<(OwnedFd, u32)>,
}

impl Dispatch<WlRegistry, GlobalListContents> for Seat {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for Seat {
    fn event(
        state: &mut Self,
        seat: &WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities } = event
            && let Ok(capabilities) = capabilities.into_result()
            && capabilities.contains(wl_seat::Capability::Keyboard)
            && state.keyboard.is_none()
        {
            state.keyboard = Some(seat.get_keyboard(handle, ()));
        }
    }
}

impl Dispatch<WlKeyboard, ()> for Seat {
    fn event(
        state: &mut Self,
        _: &WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Keymap { format, fd, size } = event
            && matches!(format.into_result(), Ok(wl_keyboard::KeymapFormat::XkbV1))
        {
            state.keymap = Some((fd, size));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `KEY_A`, `KEY_C`, and `KEY_V` in evdev numbering, the letters paste
    /// needs. Dvorak keeps `a` and `c` here and moves `v` to `KEY_DOT` (52);
    /// AZERTY moves `a` to `KEY_Q` (16).
    const KEY_A: u16 = 30;
    const KEY_C: u16 = 46;
    const KEY_V: u16 = 47;
    const KEY_Q: u16 = 16;
    const KEY_DOT: u16 = 52;

    const XK_A: u32 = 0x61;
    const XK_C: u32 = 0x63;
    const XK_V: u32 = 0x76;

    /// A Cyrillic letter as xkbcommon reports it, so a group without the Latin
    /// letter shares no symbol with the one that has it.
    fn unicode(character: char) -> u32 {
        0x0100_0000 + u32::from(character)
    }

    fn group(keys: &[(u16, u32)]) -> Group {
        keys.to_vec()
    }

    /// `us,ru`: only the Latin group has `v`, so one code carries it in every
    /// group that has it at all.
    #[test]
    fn a_letter_only_one_group_has_resolves() {
        let groups = vec![
            group(&[(KEY_A, XK_A), (KEY_C, XK_C), (KEY_V, XK_V)]),
            group(&[
                (KEY_A, unicode('ф')),
                (KEY_C, unicode('с')),
                (KEY_V, unicode('м')),
            ]),
        ];
        assert_eq!(resolve(&groups, XK_V), Some(KEY_V));
    }

    /// Dvorak keeps `v` on the physical dot key.
    #[test]
    fn dvorak_resolves_v_on_its_own_key() {
        let groups = vec![group(&[(KEY_A, XK_A), (KEY_DOT, XK_V)])];
        assert_eq!(resolve(&groups, XK_V), Some(KEY_DOT));
    }

    /// AZERTY keeps `a` on the physical Q key, so a fixed `KEY_A` would send
    /// Ctrl+Q there.
    #[test]
    fn azerty_resolves_a_on_the_q_key() {
        let groups = vec![group(&[(KEY_Q, XK_A), (KEY_V, XK_V)])];
        assert_eq!(resolve(&groups, XK_A), Some(KEY_Q));
    }

    /// `us,us(dvorak)`: the groups disagree about `v`, and a symbol two keys
    /// claim in one group is no better, so both fall back to the keysym.
    #[test]
    fn disagreeing_groups_fall_back_to_the_keysym() {
        let groups = vec![
            group(&[(KEY_A, XK_A), (KEY_V, XK_V)]),
            group(&[(KEY_A, XK_A), (KEY_DOT, XK_V)]),
        ];
        assert_eq!(resolve(&groups, XK_V), None);
        // `a` is on KEY_A in both.
        assert_eq!(resolve(&groups, XK_A), Some(KEY_A));

        let doubled = vec![group(&[(KEY_V, XK_V), (KEY_DOT, XK_V)])];
        assert_eq!(resolve(&doubled, XK_V), None);
    }

    /// A keymap with no such letter anywhere names no code for it.
    #[test]
    fn a_letter_no_group_has_falls_back_to_the_keysym() {
        let groups = vec![group(&[(KEY_Q, XK_A)])];
        assert_eq!(resolve(&groups, XK_V), None);
    }
}

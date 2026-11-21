//! The compositor's keymap, read from a `wl_seat` keyboard, and the one
//! question paste asks of it: which key types a letter in every layout group.
//!
//! A keycode names a physical key, so injecting one reaches the focused app the
//! way pressing that key does. Which key carries a letter still differs between
//! layouts — Dvorak puts `v` on `KEY_DOT`, AZERTY puts `a` on `KEY_Q` — so a
//! fixed code is wrong there. [`resolve`] names one code only when every group
//! that has the letter agrees on it, and [`every_group`] confirms the modifier
//! and Return keys the keymap options may have moved. When [`resolve`] names no
//! key, paste sends the keysym, which the desktop resolves in the active group,
//! unless no group has the letter at all, when it uses the letter's pc105 key.

use std::io::ErrorKind;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::FileExt;
use std::time::{Duration, Instant};

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_callback::{self, WlCallback};
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
/// How long the keymap wait may take once the registry has answered. The
/// registry roundtrip before it is Wayland's startup one, the same one
/// `linux/paste.rs`'s virtual-keyboard probe makes, and stays unbounded: a
/// compositor that never answers it never shows a window either.
const TIMEOUT: Duration = Duration::from_millis(250);
/// No compositor sends a keymap anywhere near this, and an absurd size must not
/// be allocated.
const MAX_KEYMAP: usize = 4 * 1024 * 1024;

/// Reads the desktop's keymap, or `None` for any failure: no Wayland
/// connection, no keyboard, no keymap before the sync or the deadline, or a
/// keymap xkbcommon rejects. Every caller has a keysym path to fall back on.
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
    parse_keymap(&fd, size)
}

/// Waits for the keymap event without ever blocking a paste. Once the keyboard
/// exists the wait asks for a sync: the compositor answers it only after
/// everything before it, its keymap included. A seat with no keyboard stops the
/// wait at once, and [`TIMEOUT`] bounds the rest.
fn wait(connection: &Connection, queue: &mut EventQueue<Seat>, state: &mut Seat) -> Option<()> {
    let backend = connection.backend();
    let deadline = Instant::now() + TIMEOUT;
    loop {
        queue.dispatch_pending(state).ok()?;
        if state.keymap.is_some() {
            return Some(());
        }
        if state.no_keyboard {
            return None;
        }
        if state.keyboard.is_some() && !state.sync_asked {
            connection.display().sync(&queue.handle(), ());
            state.sync_asked = true;
        }
        // The sync answered and carried no keymap: there is none to wait for.
        if state.synced || Instant::now() >= deadline {
            return None;
        }
        connection.flush().ok()?;
        if !readable(backend.poll_fd(), deadline) {
            return None;
        }
        let guard = connection.prepare_read()?;
        // A read that finds another guard's events, or blocks on a racing one,
        // only means the next turn dispatches them.
        let _ = guard.read();
    }
}

/// Waits for the Wayland socket to be readable, the pattern `linux/clipboard.rs`
/// uses for its own fds.
fn readable(fd: std::os::fd::BorrowedFd<'_>, deadline: Instant) -> bool {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return false;
    }
    let mut poll = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let timeout = remaining.as_millis().clamp(1, i32::MAX as u128) as libc::c_int;
    // SAFETY: `poll` reads one valid `pollfd` for the Wayland socket fd.
    let ready = unsafe { libc::poll(&mut poll, 1, timeout) };
    if ready < 0 {
        return std::io::Error::last_os_error().kind() == ErrorKind::Interrupted;
    }
    ready > 0
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

fn parse_keymap(fd: &OwnedFd, size: u32) -> Option<Groups> {
    let size = usize::try_from(size)
        .ok()
        .filter(|size| (1..=MAX_KEYMAP).contains(size))?;
    let mut bytes = vec![0; size];
    // Positional, never a `read` through the fd: compositors hand every client
    // the same keymap fd, and an fd's offset is shared, so a sequential read
    // would leave every later client, and every later paste, reading nothing.
    std::fs::File::from(fd.try_clone().ok()?)
        .read_exact_at(&mut bytes, 0)
        .ok()?;
    // The desktop ends its keymap with a NUL that is not part of the text.
    while bytes.last() == Some(&0) {
        bytes.pop();
    }
    parse(std::str::from_utf8(&bytes).ok()?)
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
/// disagree (`us,us(dvorak)`); the caller decides what a missing code means.
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

/// Whether any group types `keysym` at level 1, on any key. A letter no group
/// has cannot be typed by keysym, which the desktop resolves in the active
/// group alone.
pub(crate) fn anywhere(groups: &Groups, keysym: u32) -> bool {
    groups
        .iter()
        .any(|group| group.iter().any(|(_, symbol)| *symbol == keysym))
}

/// Whether every group types `keysym` at level 1 on the key `code`. Options
/// like `ctrl:swapcaps` move `Control_L`, and `ctrl:nocaps` leaves it on two
/// keys, so paste asks what a fixed key does rather than assuming it.
pub(crate) fn every_group(groups: &Groups, code: u16, keysym: u32) -> bool {
    !groups.is_empty()
        && groups.iter().all(|group| {
            group
                .iter()
                .any(|(key, symbol)| *key == code && *symbol == keysym)
        })
}

/// The state a one-shot keymap read collects: the seat's keyboard, its keymap,
/// and the sync that says the compositor has sent everything before it.
#[derive(Default)]
struct Seat {
    keyboard: Option<WlKeyboard>,
    /// The seat said it has no keyboard, so there is nothing to wait for.
    no_keyboard: bool,
    /// The sync after `get_keyboard` is out.
    sync_asked: bool,
    /// That sync answered: no more events are coming for this read.
    synced: bool,
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
        {
            state.no_keyboard = !capabilities.contains(wl_seat::Capability::Keyboard);
            if state.no_keyboard {
                state.keyboard = None;
            } else if state.keyboard.is_none() {
                state.keyboard = Some(seat.get_keyboard(handle, ()));
            }
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

impl Dispatch<WlCallback, ()> for Seat {
    fn event(
        state: &mut Self,
        _: &WlCallback,
        event: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            state.synced = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `KEY_A`, `KEY_C`, and `KEY_V` in evdev numbering, the letters paste
    /// needs, plus `KEY_Q` and `KEY_DOT` where other layouts put them. Dvorak
    /// moves `v` to `KEY_DOT` (52) and AZERTY `a` to `KEY_Q` (16).
    const KEY_A: u16 = 30;
    const KEY_C: u16 = 46;
    const KEY_V: u16 = 47;
    const KEY_Q: u16 = 16;
    const KEY_DOT: u16 = 52;
    /// `KEY_LEFTCTRL`, `KEY_LEFTSHIFT`, and `KEY_ENTER`.
    const KEY_LEFTCTRL: u16 = 29;
    const KEY_LEFTSHIFT: u16 = 42;
    const KEY_ENTER: u16 = 28;
    /// `KEY_CAPSLOCK`, where `ctrl:swapcaps` puts Control_L (and Caps_Lock on
    /// 29).
    const KEY_CAPSLOCK: u16 = 58;

    const XK_A: u32 = 0x61;
    const XK_C: u32 = 0x63;
    const XK_V: u32 = 0x76;
    const XK_CONTROL_L: u32 = 0xffe3;
    const XK_SHIFT_L: u32 = 0xffe1;
    const XK_CAPS_LOCK: u32 = 0xffe5;
    const XK_RETURN: u32 = 0xff0d;
    /// The legacy Cyrillic keysyms xkbcommon reports for `ф`, `с`, and `м`, so
    /// a Russian group shares no symbol with the Latin one.
    const CYRILLIC_EF: u32 = 0x6c6;
    const CYRILLIC_ES: u32 = 0x6d3;
    const CYRILLIC_EM: u32 = 0x6cd;

    fn group(keys: &[(u16, u32)]) -> Group {
        keys.to_vec()
    }

    /// `ru,us`: only the Latin group has `v`, so one code carries it in every
    /// group that has it at all.
    #[test]
    fn a_letter_only_one_group_has_resolves() {
        let groups = vec![
            group(&[
                (KEY_A, CYRILLIC_EF),
                (KEY_C, CYRILLIC_ES),
                (KEY_V, CYRILLIC_EM),
            ]),
            group(&[(KEY_A, XK_A), (KEY_C, XK_C), (KEY_V, XK_V)]),
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
    fn a_letter_no_group_has_resolves_to_nothing() {
        let groups = vec![group(&[(KEY_Q, XK_A)])];
        assert_eq!(resolve(&groups, XK_V), None);
    }

    /// `anywhere` separates a letter no group has from one the groups disagree
    /// about, which the caller acts on differently; `resolve` alone cannot.
    #[test]
    fn a_letter_is_anywhere_only_when_a_group_types_it() {
        let translated = vec![
            group(&[(KEY_A, CYRILLIC_EF), (KEY_V, CYRILLIC_EM)]),
            group(&[(KEY_A, XK_A), (KEY_V, XK_V)]),
        ];
        assert!(anywhere(&translated, XK_V));
        assert!(anywhere(&translated, CYRILLIC_EM));
        assert!(!anywhere(&translated, XK_C));

        let disagreeing = vec![group(&[(KEY_V, XK_V)]), group(&[(KEY_DOT, XK_V)])];
        assert!(anywhere(&disagreeing, XK_V));
        assert_eq!(resolve(&disagreeing, XK_V), None);
    }

    /// `ctrl:swapcaps` puts Caps_Lock on 29, where `Control_L` is not, so the
    /// fixed keycode is refused even though `Control_L` exists elsewhere.
    #[test]
    fn a_moved_modifier_fails_the_fixed_key_check() {
        let swapped = vec![group(&[
            (KEY_LEFTCTRL, XK_CAPS_LOCK),
            (KEY_CAPSLOCK, XK_CONTROL_L),
            (KEY_V, XK_V),
        ])];
        assert!(!every_group(&swapped, KEY_LEFTCTRL, XK_CONTROL_L));
        assert!(every_group(&swapped, KEY_CAPSLOCK, XK_CONTROL_L));

        let plain = vec![group(&[
            (KEY_LEFTCTRL, XK_CONTROL_L),
            (KEY_LEFTSHIFT, XK_SHIFT_L),
            (KEY_ENTER, XK_RETURN),
        ])];
        assert!(every_group(&plain, KEY_LEFTCTRL, XK_CONTROL_L));
        assert!(every_group(&plain, KEY_ENTER, XK_RETURN));
        // Every group must agree, not just one.
        let mixed = vec![plain[0].clone(), swapped[0].clone()];
        assert!(!every_group(&mixed, KEY_LEFTCTRL, XK_CONTROL_L));
    }

    /// A minimal keymap with nothing to include, so the test needs no
    /// xkeyboard-config.
    const KEYMAP: &str = r#"xkb_keymap {
    xkb_keycodes "test" {
        minimum = 8;
        maximum = 255;
        <A> = 38;
        <C> = 54;
        <V> = 55;
        <RTRN> = 36;
        <LCTL> = 37;
        <LFSH> = 50;
    };
    xkb_types "test" {
        type "ONE_LEVEL" {
            modifiers = None;
            map[None] = Level1;
            level_name[Level1] = "Any";
        };
    };
    xkb_compatibility "test" { };
    xkb_symbols "test" {
        key <A> { [ a ] };
        key <C> { [ c ] };
        key <V> { [ v ] };
        key <RTRN> { [ Return ] };
        key <LCTL> { [ Control_L ] };
        key <LFSH> { [ Shift_L ] };
    };
};
"#;

    /// Compositors share one keymap fd with every client, so its offset must
    /// never move: the same fd reads the same keymap twice, and the parser sees
    /// `KEY_V` where the keymap puts it.
    #[test]
    fn the_same_keymap_fd_reads_twice() {
        let path = std::env::temp_dir().join(format!("hex-keymap-{}", std::process::id()));
        let mut bytes = KEYMAP.as_bytes().to_vec();
        // The desktop ends the keymap with a NUL, as the size it advertises
        // counts it.
        bytes.push(0);
        std::fs::write(&path, &bytes).unwrap();
        let size = u32::try_from(bytes.len()).unwrap();
        let fd = OwnedFd::from(std::fs::File::open(&path).unwrap());

        let first = parse_keymap(&fd, size);
        let second = parse_keymap(&fd, size);
        let _ = std::fs::remove_file(&path);

        assert_eq!(first, second);
        let groups = first.expect("the inline keymap parses");
        assert_eq!(resolve(&groups, XK_V), Some(KEY_V));
        assert!(every_group(&groups, KEY_LEFTCTRL, XK_CONTROL_L));
    }
}

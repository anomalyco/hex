//! Wayland paste through the desktop's RemoteDesktop portal
//! (`org.freedesktop.portal.RemoteDesktop`), for compositors without the
//! virtual keyboard protocol `wtype` needs, such as KWin and Mutter.
//!
//! One keyboard-only session lives as long as its `RemoteDesktop`. The
//! user approves it once: the portal's restore token, kept owner-only in
//! Application Support, restores later sessions without a dialog, so the
//! paster can close an idle session and reopen it at the next paste.
//!
//! A chord goes out as evdev keycodes when the desktop's keymap confirms every
//! key of it — each letter on one key in every layout group, and Ctrl, Shift,
//! and Enter where their keymap options leave them — and otherwise as keysyms,
//! which the desktop resolves in the active group; KWin's lookup for a Latin
//! `v` on a Russian layout can lose the held Ctrl. [`keymap`] reads and
//! resolves the keymap; a chord never mixes the two.

mod keymap;

use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use zbus::{OwnedObjectPath, Variant};

use crate::linux::paste::Chord;
use crate::linux::portal::{Event, PortalBus};

pub(crate) const INTERFACE: &str = "org.freedesktop.portal.RemoteDesktop";
const KEYBOARD: u32 = 1;
/// Persist the grant until the user revokes it.
const PERSIST_UNTIL_REVOKED: u32 = 2;
/// Keysyms, for a chord the keymap can't confirm keycode by keycode.
const XK_CONTROL_L: i32 = 0xffe3;
const XK_SHIFT_L: i32 = 0xffe1;
const XK_A: i32 = 0x61;
const XK_C: i32 = 0x63;
const XK_V: i32 = 0x76;
const XK_RETURN: i32 = 0xff0d;
/// Evdev codes of the keys a chord holds; the keymap says whether they still
/// carry what they are named for.
const EVDEV_KEY_LEFTCTRL: i32 = 29;
const EVDEV_KEY_LEFTSHIFT: i32 = 42;
const EVDEV_KEY_ENTER: i32 = 28;
const PRESSED: u32 = 1;
const RELEASED: u32 = 0;
/// A restored grant starts in tens of milliseconds; a start that takes
/// longer waited for the person in the desktop's dialog.
const DIALOG_THRESHOLD: Duration = Duration::from_millis(500);

pub(crate) struct RemoteDesktop {
    bus: PortalBus,
    session: OwnedObjectPath,
    /// The portal handed out a restore token, so a later session restores
    /// this grant without a dialog.
    restorable: bool,
    /// The desktop showed its approval dialog for this session.
    prompted: bool,
    /// The desktop already ended the session, so there is nothing to close.
    ended: bool,
}

impl RemoteDesktop {
    /// Starts a keyboard session, which shows the desktop's approval
    /// dialog unless a saved grant restores it.
    pub(crate) fn start(stop: &AtomicBool) -> Result<Self> {
        let mut bus = PortalBus::open(INTERFACE, stop)?
            .ok_or_else(|| eyre!("the desktop has no RemoteDesktop portal ({INTERFACE})"))?;
        let began = Instant::now();
        let session = bus.create_session(stop)?;
        let persists = bus.version >= 2;
        let restore = persists.then(load_restore_token).flatten();
        bus.request(
            "SelectDevices",
            |token| {
                let mut options = HashMap::from([
                    ("handle_token", Variant::from(token.to_owned())),
                    ("types", Variant::from(KEYBOARD)),
                ]);
                if persists {
                    options.insert("persist_mode", Variant::from(PERSIST_UNTIL_REVOKED));
                }
                if let Some(restore) = restore.clone() {
                    options.insert("restore_token", Variant::from(restore));
                }
                (session.clone(), options)
            },
            stop,
        )?;
        let started = bus.request(
            "Start",
            |token| {
                (
                    session.clone(),
                    "",
                    HashMap::from([("handle_token", Variant::from(token.to_owned()))]),
                )
            },
            stop,
        )?;
        let devices = started
            .get("devices")
            .and_then(|devices| u32::try_from(devices).ok())
            .unwrap_or(0);
        if devices & KEYBOARD == 0 {
            bus.close_session(&session);
            return Err(eyre!("the desktop did not allow Hex to use the keyboard"));
        }
        let prompted = restore.is_none() || began.elapsed() >= DIALOG_THRESHOLD;
        // Restore tokens are single-use: each start hands out the next one.
        let mut restorable = false;
        if let Some(token) = started
            .get("restore_token")
            .and_then(|token| <&str>::try_from(token).ok())
        {
            match save_restore_token(token) {
                Ok(()) => restorable = true,
                Err(error) => tracing::warn!(
                    error = format!("{error:#}"),
                    "could not save the RemoteDesktop grant"
                ),
            }
        }
        Ok(Self {
            bus,
            session,
            restorable,
            prompted,
            ended: false,
        })
    }

    /// Asks the desktop for the keyboard ahead of the first paste, from
    /// onboarding or Settings. The saved grant lets later pastes start
    /// without a dialog.
    pub(crate) fn ask() -> Result<()> {
        Self::start(&AtomicBool::new(false)).map(drop)
    }

    /// Whether the desktop granted the keyboard before: a saved grant.
    pub(crate) fn granted() -> bool {
        load_restore_token().is_some()
    }

    /// Whether a later session restores this grant without a dialog.
    pub(crate) fn restorable(&self) -> bool {
        self.restorable
    }

    /// Whether the desktop showed its approval dialog for this session.
    pub(crate) fn prompted(&self) -> bool {
        self.prompted
    }

    /// Whether the desktop ended the session, as when the user revokes it.
    pub(crate) fn closed(&mut self) -> bool {
        if !self.ended {
            let session = self.session.as_str().to_owned();
            self.ended = self
                .bus
                .events()
                .into_iter()
                .any(|(closed, event)| closed == session && event == Event::Closed);
        }
        self.ended
    }

    /// Presses and releases a clipboard chord: Ctrl+A, Ctrl+C, or Ctrl+V
    /// (Ctrl+Shift+V for terminals).
    pub(crate) fn send(&self, chord: Chord, with_shift: bool) -> Result<()> {
        // Read per chord, not per session: a session can outlive the keymap it
        // started under (a grant that can't restore keeps it for the paster's
        // lifetime), and changing a layout or variant replaces the keymap.
        let keymap = keymap::read();
        let (encoding, keys) = chord_keys(chord, with_shift, keymap.as_ref());
        if keymap.is_none() {
            tracing::debug!(
                chord = ?chord,
                "could not read the desktop's keymap; sending keysyms"
            );
        } else if encoding == Encoding::Keysym {
            tracing::debug!(
                chord = ?chord,
                "the keymap names no single key for this chord; sending keysyms"
            );
        }
        send_keys(&keys, encoding, |key, state| {
            self.bus.call_portal(
                encoding.method(),
                &(
                    &self.session,
                    HashMap::<&str, Variant<'_>>::new(),
                    key,
                    state,
                ),
            )
        })
    }
}

/// How a chord names its keys to the portal. One chord never mixes the two: a
/// keycode and a keysym for the same letter would disagree about which key is
/// held.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Encoding {
    Keycode,
    Keysym,
}

impl Encoding {
    fn method(self) -> &'static str {
        match self {
            Self::Keycode => "NotifyKeyboardKeycode",
            Self::Keysym => "NotifyKeyboardKeysym",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Keycode => "keycode",
            Self::Keysym => "keysym",
        }
    }
}

/// Sends each key in order. A call that fails may still have reached the
/// desktop, so every key still held is released before the error returns.
fn send_keys(
    keys: &[(i32, u32)],
    encoding: Encoding,
    mut notify: impl FnMut(i32, u32) -> Result<()>,
) -> Result<()> {
    let mut pressed = Vec::new();
    for &(key, state) in keys {
        if state == PRESSED {
            // Recorded before the call: a timeout does not mean it never ran.
            pressed.push(key);
        }
        if let Err(error) = notify(key, state) {
            tracing::warn!(
                encoding = encoding.name(),
                key,
                state,
                error = format!("{error:#}"),
                "could not send a paste key"
            );
            for key in pressed.iter().rev() {
                if let Err(error) = notify(*key, RELEASED) {
                    tracing::warn!(
                        encoding = encoding.name(),
                        key,
                        error = format!("{error:#}"),
                        "could not release a paste key"
                    );
                }
            }
            return Err(error).wrap_err_with(|| {
                format!("could not send {} {key} in state {state}", encoding.name())
            });
        }
        // A failed release stays recorded: the key may still be down, and
        // releasing one already up is harmless.
        if state == RELEASED
            && let Some(index) = pressed.iter().rposition(|held| *held == key)
        {
            pressed.remove(index);
        }
    }
    Ok(())
}

impl Drop for RemoteDesktop {
    fn drop(&mut self) {
        // Closing a session the desktop already ended (the user stopped it
        // from GNOME's remote-control indicator) only fails (#718).
        if !self.closed() {
            self.bus.close_session(&self.session);
        }
    }
}

/// Presses and releases a chord's keys in order: as evdev keycodes when the
/// keymap confirms every key of the chord, and as keysyms otherwise.
fn chord_keys(
    chord: Chord,
    with_shift: bool,
    keymap: Option<&keymap::Groups>,
) -> (Encoding, Vec<(i32, u32)>) {
    if chord == Chord::Return {
        return match keymap.filter(|groups| types(groups, EVDEV_KEY_ENTER, XK_RETURN)) {
            Some(_) => (
                Encoding::Keycode,
                vec![(EVDEV_KEY_ENTER, PRESSED), (EVDEV_KEY_ENTER, RELEASED)],
            ),
            None => (
                Encoding::Keysym,
                vec![(XK_RETURN, PRESSED), (XK_RETURN, RELEASED)],
            ),
        };
    }
    let (letter, shift) = match chord {
        Chord::SelectAll => (XK_A, false),
        Chord::Copy => (XK_C, false),
        Chord::Paste => (XK_V, with_shift),
        Chord::Return => unreachable!("returned above"),
    };
    let keycode = keymap.and_then(|groups| {
        // Options such as `ctrl:swapcaps` put Control_L on another key, so the
        // fixed code is only used when the keymap still types it there.
        if !types(groups, EVDEV_KEY_LEFTCTRL, XK_CONTROL_L) {
            return None;
        }
        if shift && !types(groups, EVDEV_KEY_LEFTSHIFT, XK_SHIFT_L) {
            return None;
        }
        keymap::resolve(groups, u32::try_from(letter).ok()?)
    });
    match keycode {
        Some(code) => (
            Encoding::Keycode,
            presses(
                EVDEV_KEY_LEFTCTRL,
                EVDEV_KEY_LEFTSHIFT,
                i32::from(code),
                shift,
            ),
        ),
        None => (
            Encoding::Keysym,
            presses(XK_CONTROL_L, XK_SHIFT_L, letter, shift),
        ),
    }
}

/// Whether the keymap types `keysym` on the fixed key `code` in every group.
fn types(groups: &keymap::Groups, code: i32, keysym: i32) -> bool {
    let (Ok(code), Ok(keysym)) = (u16::try_from(code), u32::try_from(keysym)) else {
        return false;
    };
    keymap::every_group(groups, code, keysym)
}

/// Ctrl, [Shift], key, key, [Shift], Ctrl.
fn presses(control: i32, shift: i32, key: i32, with_shift: bool) -> Vec<(i32, u32)> {
    let mut keys = vec![(control, PRESSED)];
    if with_shift {
        keys.push((shift, PRESSED));
    }
    keys.extend([(key, PRESSED), (key, RELEASED)]);
    if with_shift {
        keys.push((shift, RELEASED));
    }
    keys.push((control, RELEASED));
    keys
}

fn restore_token_path() -> Result<PathBuf> {
    Ok(crate::app_paths::support_dir()?.join("remote-desktop-token"))
}

fn load_restore_token() -> Option<String> {
    let token = std::fs::read_to_string(restore_token_path().ok()?).ok()?;
    let token = token.trim();
    (!token.is_empty() && token.len() <= 1024).then(|| token.to_owned())
}

fn save_restore_token(token: &str) -> Result<()> {
    let path = restore_token_path()?;
    let directory = path
        .parent()
        .ok_or_else(|| eyre!("the grant has no folder"))?;
    std::fs::create_dir_all(directory)?;
    let staged = directory.join(format!(".remote-desktop-token.{}", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&staged)
        .wrap_err("could not stage the grant")?;
    file.write_all(token.as_bytes())?;
    file.sync_all()?;
    std::fs::rename(&staged, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `us` keymap: `a`, `c`, and `v` on their QWERTY keys, and the
    /// modifiers and Return where a plain layout leaves them.
    fn qwerty() -> keymap::Groups {
        vec![vec![
            (29, u32::try_from(XK_CONTROL_L).unwrap()),
            (42, u32::try_from(XK_SHIFT_L).unwrap()),
            (28, u32::try_from(XK_RETURN).unwrap()),
            (30, u32::try_from(XK_A).unwrap()),
            (46, u32::try_from(XK_C).unwrap()),
            (47, u32::try_from(XK_V).unwrap()),
        ]]
    }

    /// `ctrl:swapcaps`: Caps_Lock on 29 and Control_L on 58, with `v` still on
    /// its QWERTY key.
    fn swapcaps() -> keymap::Groups {
        vec![vec![
            (29, 0xffe5),
            (58, u32::try_from(XK_CONTROL_L).unwrap()),
            (42, u32::try_from(XK_SHIFT_L).unwrap()),
            (28, u32::try_from(XK_RETURN).unwrap()),
            (47, u32::try_from(XK_V).unwrap()),
        ]]
    }

    /// Both ways a chord can go out: no keymap at all, and a keymap that names
    /// the letters' keys.
    fn ways() -> [Option<keymap::Groups>; 2] {
        [None, Some(qwerty())]
    }

    #[test]
    fn paste_keys_balance_their_modifiers() {
        // Without a keymap the chord is keysyms, as it always was.
        assert_eq!(
            chord_keys(Chord::Paste, false, None),
            (
                Encoding::Keysym,
                vec![
                    (XK_CONTROL_L, PRESSED),
                    (XK_V, PRESSED),
                    (XK_V, RELEASED),
                    (XK_CONTROL_L, RELEASED)
                ]
            )
        );
        assert_eq!(
            chord_keys(Chord::Paste, true, None),
            (
                Encoding::Keysym,
                vec![
                    (XK_CONTROL_L, PRESSED),
                    (XK_SHIFT_L, PRESSED),
                    (XK_V, PRESSED),
                    (XK_V, RELEASED),
                    (XK_SHIFT_L, RELEASED),
                    (XK_CONTROL_L, RELEASED)
                ]
            )
        );
        // A keymap names the key each layout puts `v` on.
        let keymap = qwerty();
        assert_eq!(
            chord_keys(Chord::Paste, false, Some(&keymap)),
            (
                Encoding::Keycode,
                vec![
                    (EVDEV_KEY_LEFTCTRL, PRESSED),
                    (47, PRESSED),
                    (47, RELEASED),
                    (EVDEV_KEY_LEFTCTRL, RELEASED)
                ]
            )
        );
        assert_eq!(
            chord_keys(Chord::Paste, true, Some(&keymap)),
            (
                Encoding::Keycode,
                vec![
                    (EVDEV_KEY_LEFTCTRL, PRESSED),
                    (EVDEV_KEY_LEFTSHIFT, PRESSED),
                    (47, PRESSED),
                    (47, RELEASED),
                    (EVDEV_KEY_LEFTSHIFT, RELEASED),
                    (EVDEV_KEY_LEFTCTRL, RELEASED)
                ]
            )
        );
    }

    /// Select-all and copy never take the terminal paste's Shift.
    #[test]
    fn select_all_and_copy_ignore_the_terminal_paste_preference() {
        let keymap = qwerty();
        for (chord, keysym, code) in [(Chord::SelectAll, XK_A, 30), (Chord::Copy, XK_C, 46)] {
            for with_shift in [false, true] {
                assert_eq!(
                    chord_keys(chord, with_shift, None),
                    (
                        Encoding::Keysym,
                        vec![
                            (XK_CONTROL_L, PRESSED),
                            (keysym, PRESSED),
                            (keysym, RELEASED),
                            (XK_CONTROL_L, RELEASED)
                        ]
                    )
                );
                assert_eq!(
                    chord_keys(chord, with_shift, Some(&keymap)),
                    (
                        Encoding::Keycode,
                        vec![
                            (EVDEV_KEY_LEFTCTRL, PRESSED),
                            (code, PRESSED),
                            (code, RELEASED),
                            (EVDEV_KEY_LEFTCTRL, RELEASED)
                        ]
                    )
                );
            }
        }
    }

    /// Return goes out as a keycode only where the keymap still types Return
    /// on it; without a keymap it stays a keysym, as it always was.
    #[test]
    fn return_follows_the_keymap() {
        assert_eq!(
            chord_keys(Chord::Return, false, None),
            (
                Encoding::Keysym,
                vec![(XK_RETURN, PRESSED), (XK_RETURN, RELEASED)]
            )
        );
        let keymap = qwerty();
        assert_eq!(
            chord_keys(Chord::Return, false, Some(&keymap)),
            (
                Encoding::Keycode,
                vec![(EVDEV_KEY_ENTER, PRESSED), (EVDEV_KEY_ENTER, RELEASED)]
            )
        );
        // A layout that moved Return off 28 keeps the keysym.
        let moved = vec![vec![(29, 0xffe3), (47, 0x76), (24, 0xff0d)]];
        assert_eq!(
            chord_keys(Chord::Return, false, Some(&moved)),
            (
                Encoding::Keysym,
                vec![(XK_RETURN, PRESSED), (XK_RETURN, RELEASED)]
            )
        );
    }

    /// `ctrl:swapcaps` moves Control_L off 29, where a keycode chord would
    /// hold Caps_Lock instead, so the whole chord falls back to keysyms.
    #[test]
    fn moved_modifiers_fall_back_to_keysyms() {
        let keymap = swapcaps();
        assert_eq!(
            chord_keys(Chord::Paste, false, Some(&keymap)),
            (
                Encoding::Keysym,
                vec![
                    (XK_CONTROL_L, PRESSED),
                    (XK_V, PRESSED),
                    (XK_V, RELEASED),
                    (XK_CONTROL_L, RELEASED)
                ]
            )
        );
        assert_eq!(
            chord_keys(Chord::Paste, true, Some(&keymap)),
            (
                Encoding::Keysym,
                vec![
                    (XK_CONTROL_L, PRESSED),
                    (XK_SHIFT_L, PRESSED),
                    (XK_V, PRESSED),
                    (XK_V, RELEASED),
                    (XK_SHIFT_L, RELEASED),
                    (XK_CONTROL_L, RELEASED)
                ]
            )
        );

        // A moved Shift only matters to the chords that hold it.
        let moved_shift = vec![vec![(29, 0xffe3), (42, 0xffe5), (47, 0x76)]];
        assert_eq!(
            chord_keys(Chord::Paste, false, Some(&moved_shift)),
            (
                Encoding::Keycode,
                vec![
                    (EVDEV_KEY_LEFTCTRL, PRESSED),
                    (47, PRESSED),
                    (47, RELEASED),
                    (EVDEV_KEY_LEFTCTRL, RELEASED)
                ]
            )
        );
        assert_eq!(
            chord_keys(Chord::Paste, true, Some(&moved_shift)),
            (
                Encoding::Keysym,
                vec![
                    (XK_CONTROL_L, PRESSED),
                    (XK_SHIFT_L, PRESSED),
                    (XK_V, PRESSED),
                    (XK_V, RELEASED),
                    (XK_SHIFT_L, RELEASED),
                    (XK_CONTROL_L, RELEASED)
                ]
            )
        );
    }

    #[test]
    fn a_successful_paste_sends_each_key_once() {
        for keymap in ways() {
            for chord in [Chord::Paste, Chord::SelectAll, Chord::Copy] {
                for with_shift in [false, true] {
                    let (encoding, keys) = chord_keys(chord, with_shift, keymap.as_ref());
                    let mut sent = Vec::new();
                    send_keys(&keys, encoding, |key, state| {
                        sent.push((key, state));
                        Ok(())
                    })
                    .unwrap();
                    assert_eq!(sent, keys);
                }
            }
        }
    }

    #[test]
    fn a_failed_paste_releases_the_keys_it_still_holds() {
        for keymap in ways() {
            for chord in [Chord::Paste, Chord::SelectAll, Chord::Copy] {
                for with_shift in [false, true] {
                    let (encoding, keys) = chord_keys(chord, with_shift, keymap.as_ref());
                    for fail_at in 0..keys.len() {
                        let mut sent = Vec::new();
                        let result = send_keys(&keys, encoding, |key, state| {
                            sent.push((key, state));
                            if sent.len() == fail_at + 1 {
                                return Err(eyre!("injected failure"));
                            }
                            Ok(())
                        });
                        assert!(result.is_err());
                        // Cleanup releases every key still held: each press up to
                        // and including the failed call, minus keys whose release
                        // already succeeded before it. A failed release stays in,
                        // since it may not have reached the desktop.
                        let mut held: Vec<_> = keys[..=fail_at]
                            .iter()
                            .filter(|(_, state)| *state == PRESSED)
                            .map(|(key, _)| *key)
                            .collect();
                        for (key, state) in &keys[..fail_at] {
                            if *state == RELEASED
                                && let Some(index) = held.iter().rposition(|held| held == key)
                            {
                                held.remove(index);
                            }
                        }
                        let mut expected: Vec<_> =
                            held.into_iter().map(|key| (key, RELEASED)).collect();
                        expected.reverse();
                        assert_eq!(&sent[fail_at + 1..], expected, "failed at {fail_at}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_failed_cleanup_release_keeps_the_original_error() {
        for (keymap, tail) in [
            (
                None,
                vec![
                    (XK_V, RELEASED),
                    (XK_SHIFT_L, RELEASED),
                    (XK_CONTROL_L, RELEASED),
                ],
            ),
            (
                Some(qwerty()),
                vec![
                    (47, RELEASED),
                    (EVDEV_KEY_LEFTSHIFT, RELEASED),
                    (EVDEV_KEY_LEFTCTRL, RELEASED),
                ],
            ),
        ] {
            let (encoding, keys) = chord_keys(Chord::Paste, true, keymap.as_ref());
            let mut sent = Vec::new();
            let error = send_keys(&keys, encoding, |key, state| {
                sent.push((key, state));
                match sent.len() {
                    // The V press fails partway.
                    3 => Err(eyre!("the original failure")),
                    // Its release fails too, which must not stop the cleanup.
                    4 => Err(eyre!("a cleanup failure")),
                    _ => Ok(()),
                }
            })
            .unwrap_err();
            let error = format!("{error:#}");
            assert!(error.contains("the original failure"), "{error}");
            assert!(!error.contains("a cleanup failure"), "{error}");
            assert!(error.contains(encoding.name()), "{error}");
            assert_eq!(&sent[3..], tail);
        }
    }
}

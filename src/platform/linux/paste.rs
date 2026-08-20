use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use x11_clipboard::Clipboard;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt, KeyButMask, Window};
use x11rb::protocol::xtest;
use x11rb::rust_connection::RustConnection;

use crate::linux_input::Keymap;

const XK_CONTROL_L: u32 = 0xffe3;
const XK_SHIFT_L: u32 = 0xffe1;
const XK_V: u32 = 0x76;
const KEY_PRESS: u8 = 2;
const KEY_RELEASE: u8 = 3;
const SELECTION_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_SELECTED_TEXT_BYTES: usize = 64 * 1024;
const MAX_WINDOW_ANCESTORS: usize = 16;

pub struct X11Paster {
    clipboard: Clipboard,
    connection: RustConnection,
    root: u32,
    active_window: Atom,
    window_pid: Atom,
    control: u8,
    shift: u8,
    v: u8,
}

impl X11Paster {
    pub fn new() -> Result<Self> {
        let clipboard = Clipboard::new().wrap_err("could not open the X11 clipboard")?;
        let (connection, screen) =
            RustConnection::connect(None).wrap_err("could not connect to X11")?;
        let root = connection.setup().roots[screen].root;
        let keymap = Keymap::read(&connection)?;
        let active_window = connection
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")?
            .reply()?
            .atom;
        let window_pid = connection.intern_atom(false, b"_NET_WM_PID")?.reply()?.atom;
        Ok(Self {
            clipboard,
            connection,
            root,
            active_window,
            window_pid,
            control: keymap.keycode(XK_CONTROL_L)?,
            shift: keymap.keycode(XK_SHIFT_L)?,
            v: keymap.keycode(XK_V)?,
        })
    }

    pub fn paste(&mut self, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Err(eyre!("refusing to paste an empty transcript"));
        }
        self.wait_for_modifier_release()?;
        let atoms = &self.clipboard.getter.atoms;
        self.clipboard
            .store(atoms.clipboard, atoms.utf8_string, text.as_bytes())
            .wrap_err("could not own the X11 clipboard")?;
        thread::sleep(Duration::from_millis(60));
        for (type_, key) in [
            (KEY_PRESS, self.control),
            (KEY_PRESS, self.shift),
            (KEY_PRESS, self.v),
            (KEY_RELEASE, self.v),
            (KEY_RELEASE, self.shift),
            (KEY_RELEASE, self.control),
        ] {
            xtest::fake_input(&self.connection, type_, key, 0, self.root, 0, 0, 0)?.check()?;
        }
        self.connection.flush()?;
        Ok(())
    }

    /// Read the X11 PRIMARY selection without sending a copy shortcut,
    /// changing CLIPBOARD ownership, or activating another window. Toolkits
    /// that do not publish PRIMARY simply contribute no optional context. A
    /// selection is accepted only while its owner belongs to the active X11
    /// client, preventing a stale selection from another application from
    /// entering the prompt.
    pub fn selected_text(&self) -> Result<Option<String>> {
        let atoms = &self.clipboard.getter.atoms;
        let Some((active, owner)) = self.active_selection_owner(atoms.primary)? else {
            return Ok(None);
        };
        let utf8 = self
            .clipboard
            .load(
                atoms.primary,
                atoms.utf8_string,
                atoms.property,
                SELECTION_TIMEOUT,
            )
            .wrap_err("could not read the UTF-8 X11 PRIMARY selection")?;
        let selection = if !utf8.is_empty() {
            bounded_selection(utf8, true)?
        } else {
            let legacy = self
                .clipboard
                .load(
                    atoms.primary,
                    atoms.string,
                    atoms.property,
                    SELECTION_TIMEOUT,
                )
                .wrap_err("could not read the legacy X11 PRIMARY selection")?;
            bounded_selection(legacy, false)?
        };
        if self.active_selection_owner(atoms.primary)? == Some((active, owner)) {
            Ok(selection)
        } else {
            Ok(None)
        }
    }

    fn active_selection_owner(&self, primary: Atom) -> Result<Option<(Window, Window)>> {
        let active = self
            .connection
            .get_property(false, self.root, self.active_window, AtomEnum::WINDOW, 0, 1)?
            .reply()?
            .value32()
            .and_then(|mut values| values.next())
            .filter(|window| *window != x11rb::NONE);
        let owner = self.connection.get_selection_owner(primary)?.reply()?.owner;
        let Some(active) = active else {
            return Ok(None);
        };
        if owner == x11rb::NONE || !self.same_application_client(active, owner)? {
            return Ok(None);
        }
        Ok(Some((active, owner)))
    }

    fn same_application_client(&self, active: Window, owner: Window) -> Result<bool> {
        let active_pid = self.window_pid(active)?;
        let owner_pid = self.window_pid(owner)?;
        Ok(match (active_pid, owner_pid) {
            (Some(active), Some(owner)) => active == owner,
            _ => same_x11_client(active, owner, self.connection.setup().resource_id_mask),
        })
    }

    fn window_pid(&self, mut window: Window) -> Result<Option<u32>> {
        for _ in 0..MAX_WINDOW_ANCESTORS {
            let pid = self
                .connection
                .get_property(false, window, self.window_pid, AtomEnum::CARDINAL, 0, 1)?
                .reply()?
                .value32()
                .and_then(|mut values| values.next());
            if pid.is_some() {
                return Ok(pid);
            }
            let parent = self.connection.query_tree(window)?.reply()?.parent;
            if parent == self.root || parent == window || parent == x11rb::NONE {
                break;
            }
            window = parent;
        }
        Ok(None)
    }

    fn wait_for_modifier_release(&self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            let mask = self.connection.query_pointer(self.root)?.reply()?.mask;
            let held = mask
                & (KeyButMask::SHIFT | KeyButMask::CONTROL | KeyButMask::MOD1 | KeyButMask::MOD4);
            if held == KeyButMask::default() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(10));
        }
        Err(eyre!(
            "release the dictation shortcut before HEX inserts the transcript"
        ))
    }
}

fn bounded_selection(bytes: Vec<u8>, utf8: bool) -> Result<Option<String>> {
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.len() > MAX_SELECTED_TEXT_BYTES {
        return Err(eyre!(
            "the X11 PRIMARY selection exceeds {MAX_SELECTED_TEXT_BYTES} bytes"
        ));
    }
    let text = if utf8 {
        String::from_utf8(bytes).wrap_err("the X11 PRIMARY selection was not valid UTF-8")?
    } else {
        bytes.into_iter().map(char::from).collect()
    };
    Ok((!text.trim().is_empty()).then_some(text))
}

fn same_x11_client(first: Window, second: Window, resource_id_mask: u32) -> bool {
    first & !resource_id_mask == second & !resource_id_mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_text_is_bounded_and_keeps_exact_utf8_content() {
        assert_eq!(
            bounded_selection("  żółw\n".as_bytes().to_vec(), true)
                .unwrap()
                .as_deref(),
            Some("  żółw\n")
        );
        assert!(bounded_selection(vec![b'x'; MAX_SELECTED_TEXT_BYTES + 1], true).is_err());
    }

    #[test]
    fn empty_or_whitespace_primary_selection_is_absent() {
        assert_eq!(bounded_selection(Vec::new(), true).unwrap(), None);
        assert_eq!(bounded_selection(b" \n\t".to_vec(), true).unwrap(), None);
    }

    #[test]
    fn resource_ids_distinguish_selection_owners_from_other_x11_clients() {
        let mask = 0x001f_ffff;
        assert!(same_x11_client(0x0420_0001, 0x0420_1234, mask));
        assert!(!same_x11_client(0x0420_0001, 0x0440_0001, mask));
    }
}

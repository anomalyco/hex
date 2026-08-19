//! Executes command actions on X11: keystroke synthesis through XTest,
//! mirroring the macOS CoreGraphics executor's contract. Dictation
//! control and desktop opens are handled by the command engine and the
//! listener loop; this owns only the key events.

use color_eyre::eyre::{Result, WrapErr};
use x11rb::connection::Connection;
use x11rb::protocol::xtest;
use x11rb::rust_connection::RustConnection;

use crate::keys::{Key, Modifiers};
use crate::linux_input::Keymap;

const KEY_PRESS: u8 = 2;
const KEY_RELEASE: u8 = 3;
const XK_SHIFT_L: u32 = 0xffe1;
const XK_CONTROL_L: u32 = 0xffe3;
const XK_ALT_L: u32 = 0xffe9;

pub struct X11CommandExecutor {
    connection: RustConnection,
    root: u32,
    keymap: Keymap,
}

impl X11CommandExecutor {
    pub fn new() -> Result<Self> {
        let (connection, screen) =
            RustConnection::connect(None).wrap_err("could not connect to X11 for commands")?;
        let root = connection.setup().roots[screen].root;
        let keymap = Keymap::read(&connection)?;
        Ok(Self {
            connection,
            root,
            keymap,
        })
    }

    pub fn keystroke(&self, key: Key, modifiers: Modifiers) -> Result<()> {
        self.repeated_keystroke(key, modifiers, 1)
    }

    pub fn repeated_keystroke(&self, key: Key, modifiers: Modifiers, count: u8) -> Result<()> {
        let key = self.keymap.keycode(keysym_for(key))?;
        // COMMAND is the platform primary shortcut modifier - Ctrl here -
        // so mac-authored shortcuts drive the same application shortcuts.
        let held: Vec<u8> = [
            (Modifiers::COMMAND, XK_CONTROL_L),
            (Modifiers::CONTROL, XK_CONTROL_L),
            (Modifiers::OPTION, XK_ALT_L),
            (Modifiers::SHIFT, XK_SHIFT_L),
        ]
        .into_iter()
        .filter(|(modifier, _)| modifiers.contains(*modifier))
        .map(|(_, keysym)| self.keymap.keycode(keysym))
        .collect::<Result<Vec<u8>>>()?;
        // COMMAND and CONTROL share a keycode here; press each key once.
        let mut held = held;
        held.dedup();
        for &modifier in &held {
            self.fake(KEY_PRESS, modifier)?;
        }
        for _ in 0..count.max(1) {
            self.fake(KEY_PRESS, key)?;
            self.fake(KEY_RELEASE, key)?;
        }
        for &modifier in held.iter().rev() {
            self.fake(KEY_RELEASE, modifier)?;
        }
        self.connection.flush()?;
        Ok(())
    }

    fn fake(&self, event: u8, keycode: u8) -> Result<()> {
        xtest::fake_input(&self.connection, event, keycode, 0, self.root, 0, 0, 0)?.check()?;
        Ok(())
    }
}

/// The X11 keysym for a neutral command key. Latin-1 characters map
/// directly; anything else uses the Unicode keysym plane.
fn keysym_for(key: Key) -> u32 {
    match key {
        Key::Character(character) => {
            let code = character as u32;
            if code < 0x100 {
                code
            } else {
                0x0100_0000 + code
            }
        }
        Key::Home => 0xff50,
        Key::Left => 0xff51,
        Key::Up => 0xff52,
        Key::Right => 0xff53,
        Key::Down => 0xff54,
        Key::End => 0xff57,
        Key::Enter => 0xff0d,
        Key::Escape => 0xff1b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keysyms_cover_the_neutral_key_set() {
        assert_eq!(keysym_for(Key::Character('a')), 0x61);
        assert_eq!(keysym_for(Key::Character('ż')), 0x0100_0000 + 'ż' as u32);
        assert_eq!(keysym_for(Key::Enter), 0xff0d);
        assert_eq!(keysym_for(Key::Home), 0xff50);
    }
}

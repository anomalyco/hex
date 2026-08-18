use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use x11_clipboard::Clipboard;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, KeyButMask};
use x11rb::protocol::xtest;
use x11rb::rust_connection::RustConnection;

use crate::linux_input::Keymap;

const XK_CONTROL_L: u32 = 0xffe3;
const XK_SHIFT_L: u32 = 0xffe1;
const XK_V: u32 = 0x76;
const KEY_PRESS: u8 = 2;
const KEY_RELEASE: u8 = 3;

pub struct X11Paster {
    clipboard: Clipboard,
    connection: RustConnection,
    root: u32,
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
        Ok(Self {
            clipboard,
            connection,
            root,
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

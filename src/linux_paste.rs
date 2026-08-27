use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use x11_clipboard::Clipboard;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, KeyButMask};
use x11rb::protocol::xtest;
use x11rb::rust_connection::RustConnection;

use crate::linux_input::Keymap;
use crate::linux_session::LinuxSession;

const XK_CONTROL_L: u32 = 0xffe3;
const XK_SHIFT_L: u32 = 0xffe1;
const XK_V: u32 = 0x76;
const KEY_PRESS: u8 = 2;
const KEY_RELEASE: u8 = 3;

pub enum LinuxPaster {
    X11(X11Paster),
    Wayland,
}

pub struct X11Paster {
    clipboard: Clipboard,
    connection: RustConnection,
    root: u32,
    control: u8,
    shift: u8,
    v: u8,
}

impl LinuxPaster {
    pub fn new() -> Result<Self> {
        if LinuxSession::detect().is_wayland() {
            Ok(Self::Wayland)
        } else {
            Ok(Self::X11(X11Paster::new()?))
        }
    }

    pub fn paste(&mut self, text: &str) -> Result<()> {
        match self {
            Self::X11(paster) => paster.paste(text),
            Self::Wayland => paste_wayland(text),
        }
    }
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

fn paste_wayland(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Err(eyre!("refusing to paste an empty transcript"));
    }
    wait_for_idle_modifiers()?;
    if type_wayland(text).is_ok() {
        return Ok(());
    }
    copy_wayland(text)?;
    thread::sleep(Duration::from_millis(80));
    paste_wayland_keys().wrap_err("could not insert the transcript on Wayland")
}

fn copy_wayland(text: &str) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .args(["--type", "text/plain"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .wrap_err("wl-copy is required to paste on Wayland")?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| eyre!("wl-copy stdin is unavailable"))?;
        stdin.write_all(text.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(eyre!(
            "wl-copy failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

fn type_wayland(text: &str) -> Result<()> {
    let status = Command::new("wtype")
        .arg("--")
        .arg(text)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .wrap_err("could not start wtype")?;
    if status.success() {
        Ok(())
    } else {
        Err(eyre!("wtype failed"))
    }
}

fn paste_wayland_keys() -> Result<()> {
    if run_silent(
        "wtype",
        &["-M", "ctrl", "-k", "v", "-m", "ctrl"],
    )
    .is_ok()
    {
        return Ok(());
    }
    let mut child = Command::new("dotool")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .wrap_err("install wtype or dotool to paste on Wayland")?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| eyre!("dotool stdin is unavailable"))?
        .write_all(b"key ctrl+v\n")?;
    let output = child.wait_with_output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(eyre!(
        "wtype and dotool failed; install wtype for Hyprland paste"
    ))
}

fn wait_for_idle_modifiers() -> Result<()> {
    thread::sleep(Duration::from_millis(80));
    Ok(())
}

fn run_silent(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .wrap_err_with(|| format!("could not start {program}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(eyre!("{program} failed"))
    }
}

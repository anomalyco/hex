use std::io::{ErrorKind, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use x11_clipboard::Clipboard;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, KeyButMask};
use x11rb::protocol::xtest;
use x11rb::rust_connection::RustConnection;

use crate::linux_clipboard::{Selection, Snapshot};
use crate::linux_input::{
    KEY_PRESS, KEY_RELEASE, ShortcutKeys, XK_CONTROL_L, XK_SHIFT_L, connect_x11,
};
use crate::linux_remote_desktop::RemoteDesktop;
use crate::linux_session::LinuxSession;

const XK_V: u32 = 0x76;
const HELPER_TIMEOUT: Duration = Duration::from_secs(3);
const PASTE_SETTLE: Duration = Duration::from_millis(100);
const PORTAL_FOCUS_SETTLE: Duration = Duration::from_millis(250);
/// How long after the paste shortcut the previous clipboard comes back, as
/// on macOS. Nothing acknowledges that the target read the paste.
const RESTORE_DELAY: Duration = Duration::from_millis(500);

pub(crate) struct LinuxPaster {
    backend: Backend,
    stop: Arc<AtomicBool>,
    /// The terminal-paste preference, which the window changes live.
    paste_with_shift: Arc<AtomicBool>,
    /// When the shortcut that asked for the paste has let go of its keys.
    keys: ShortcutKeys,
    selection: Selection,
    restore: Arc<Mutex<Restore>>,
}

/// The clipboard a paste replaced, waiting to come back.
#[derive(Default)]
struct Restore {
    /// Advances with every clipboard write, so only the latest paste restores.
    generation: u64,
    original: Option<Snapshot>,
    /// What the latest paste put on the clipboard, until it restores.
    inserted: Option<(Mime, Vec<u8>)>,
}

enum Backend {
    X11(Box<X11Paster>),
    Wayland(WaylandKeys),
}

/// How Wayland sends the paste shortcut, chosen at the first paste.
enum WaylandKeys {
    Unchosen,
    /// `wtype`, when the compositor offers the virtual keyboard protocol.
    /// Its virtual keyboard sends its own modifiers, but some compositors
    /// (Hyprland) add every held physical modifier to them, so a still-held
    /// Alt turns its Ctrl+V into Ctrl+Alt+V. [`ShortcutKeys::settle`] waits
    /// for the shortcut to let go first.
    Wtype,
    /// The desktop's RemoteDesktop portal everywhere else (KWin, Mutter).
    /// Its session starts at the first paste and lives with the paster.
    Portal(Option<Box<RemoteDesktop>>),
}

impl WaylandKeys {
    fn choose() -> Self {
        if offers_virtual_keyboard() && on_path("wtype") {
            Self::Wtype
        } else {
            Self::Portal(None)
        }
    }

    fn send(&mut self, stop: &AtomicBool, paste_with_shift: bool) -> Result<()> {
        if matches!(self, Self::Unchosen) {
            *self = Self::choose();
        }
        match self {
            Self::Unchosen => unreachable!("chosen above"),
            Self::Wtype => {
                let mut keys = Command::new("wtype");
                keys.args(paste_keys(paste_with_shift));
                run_helper(keys, &[], stop, HELPER_TIMEOUT)
                    .wrap_err("wtype could not send the paste shortcut")
            }
            Self::Portal(session) => {
                if session.as_mut().is_some_and(|session| session.closed()) {
                    *session = None;
                }
                let started = session.is_none();
                if started {
                    *session = Some(Box::new(RemoteDesktop::start(stop)?));
                    // The approval dialog hands focus back to the target.
                    thread::sleep(PORTAL_FOCUS_SETTLE);
                }
                let result = session
                    .as_ref()
                    .expect("started above")
                    .paste(paste_with_shift);
                if result.is_err() {
                    *session = None;
                }
                result
            }
        }
    }
}

/// Whether the compositor advertises `zwp_virtual_keyboard_manager_v1`.
fn offers_virtual_keyboard() -> bool {
    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::wl_registry::{self, WlRegistry};
    use wayland_client::{Connection, Dispatch, QueueHandle};

    struct Globals;
    impl Dispatch<WlRegistry, GlobalListContents> for Globals {
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
    let Ok(connection) = Connection::connect_to_env() else {
        return false;
    };
    let Ok((globals, _queue)) = registry_queue_init::<Globals>(&connection) else {
        return false;
    };
    globals.contents().with_list(|list| {
        list.iter()
            .any(|global| global.interface == "zwp_virtual_keyboard_manager_v1")
    })
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| {
            std::fs::metadata(dir.join(program))
                .is_ok_and(|file| file.is_file() && file.permissions().mode() & 0o111 != 0)
        })
    })
}

/// A Wayland paste that only reached the clipboard.
fn copied_only(
    error: color_eyre::eyre::Report,
    paste_with_shift: bool,
) -> color_eyre::eyre::Report {
    let shortcut = if paste_with_shift {
        "Ctrl+Shift+V"
    } else {
        "Ctrl+V"
    };
    eyre!(
        "Copied to the clipboard; press {shortcut} to paste. Automatic paste needs the desktop's RemoteDesktop portal ({}), or wtype with a compositor offering the virtual keyboard protocol: {error:#}",
        crate::linux_remote_desktop::INTERFACE
    )
}

impl LinuxPaster {
    pub(crate) fn new(
        stop: Arc<AtomicBool>,
        paste_with_shift: Arc<AtomicBool>,
        keys: ShortcutKeys,
    ) -> Result<Self> {
        let (backend, selection) = match LinuxSession::detect() {
            LinuxSession::X11 => {
                let paster = X11Paster::new()?;
                let transcript_owner = paster.clipboard.setter.window;
                (
                    Backend::X11(Box::new(paster)),
                    Selection::X11 { transcript_owner },
                )
            }
            LinuxSession::Wayland => (Backend::Wayland(WaylandKeys::Unchosen), Selection::Wayland),
        };
        Ok(Self {
            backend,
            stop,
            paste_with_shift,
            keys,
            selection,
            restore: Arc::default(),
        })
    }

    /// Puts `text` on the clipboard without pasting it. On X11 the clipboard
    /// is served for as long as this paster lives.
    pub(crate) fn copy(&mut self, text: &str) -> Result<()> {
        let mut restore = self.restore.lock().unwrap_or_else(PoisonError::into_inner);
        *restore = Restore {
            generation: restore.generation.wrapping_add(1),
            ..Restore::default()
        };
        match &mut self.backend {
            Backend::X11(paster) => {
                let atoms = &paster.clipboard.getter.atoms;
                paster
                    .clipboard
                    .store(atoms.clipboard, atoms.utf8_string, text.as_bytes())
                    .wrap_err("could not own the X11 clipboard")
            }
            Backend::Wayland(_) => {
                let mut copy = Command::new("wl-copy");
                copy.args(["--type", "text/plain;charset=utf-8"]);
                run_helper(copy, text.as_bytes(), &self.stop, HELPER_TIMEOUT)
                    .wrap_err("could not own the Wayland clipboard; install wl-clipboard")
            }
        }
    }

    pub(crate) fn paste(&mut self, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Err(eyre!("refusing to paste an empty transcript"));
        }
        self.paste_as(Mime::Text, text.as_bytes())
    }

    /// Pastes a PNG (a capture's contact sheet) where the focus is, as the
    /// Mac's capture Paste does before its summary.
    pub(crate) fn paste_png(&mut self, png: &[u8]) -> Result<()> {
        self.paste_as(Mime::Png, png)
    }

    /// Pastes through the clipboard, then puts the previous clipboard back
    /// unless something newer replaced the paste first.
    fn paste_as(&mut self, mime: Mime, bytes: &[u8]) -> Result<()> {
        let restore = self.restore.clone();
        let mut state = restore.lock().unwrap_or_else(PoisonError::into_inner);
        state.generation = state.generation.wrapping_add(1);
        // A paste before the previous one restored keeps that one's original.
        let previous_pending = state
            .inserted
            .take()
            .is_some_and(|(mime, bytes)| self.selection.holds(mime.wayland(), &bytes));
        if !previous_pending {
            state.original = match self.selection.snapshot() {
                Ok(snapshot) => Some(snapshot),
                Err(error) => {
                    tracing::info!(
                        error = format!("{error:#}"),
                        "not restoring the clipboard after this paste"
                    );
                    None
                }
            };
        }
        let result = self.send_paste(mime, bytes);
        if result.is_ok() {
            state.inserted = Some((mime, bytes.to_vec()));
            schedule_restore(restore.clone(), state.generation, self.selection);
        } else {
            // A paste that only reached the clipboard leaves it for Ctrl+V.
            state.original = None;
        }
        drop(state);
        // Give the target time to request the selection before the next queued
        // transcript replaces it. This is a bounded settling policy, not an ACK.
        thread::sleep(PASTE_SETTLE);
        result
    }

    fn send_paste(&mut self, mime: Mime, bytes: &[u8]) -> Result<()> {
        let paste_with_shift = self.paste_with_shift.load(Ordering::Relaxed);
        match &mut self.backend {
            Backend::X11(paster) => paster.paste(mime, bytes, &self.stop, paste_with_shift)?,
            Backend::Wayland(wayland) => {
                let mut copy = Command::new("wl-copy");
                copy.args(["--type", mime.wayland()]);
                run_helper(copy, bytes, &self.stop, HELPER_TIMEOUT)
                    .wrap_err("could not own the Wayland clipboard; install wl-clipboard and check compositor support")?;
                // wl-copy's parent exits only after the selection is installed.
                // Never read a pipe inherited by its clipboard-serving daemon.
                self.keys.settle(&self.stop)?;
                wayland
                    .send(&self.stop, paste_with_shift)
                    .map_err(|error| copied_only(error, paste_with_shift))?;
            }
        }
        Ok(())
    }
}

/// Puts the paste's original clipboard back once the target has had time to
/// read the paste, if no later paste, copy, or other client replaced it.
fn schedule_restore(restore: Arc<Mutex<Restore>>, generation: u64, selection: Selection) {
    thread::spawn(move || {
        thread::sleep(RESTORE_DELAY);
        let mut state = restore.lock().unwrap_or_else(PoisonError::into_inner);
        if state.generation != generation {
            return;
        }
        let (Some((mime, bytes)), Some(original)) = (state.inserted.take(), state.original.take())
        else {
            return;
        };
        if !selection.holds(mime.wayland(), &bytes) {
            tracing::debug!("the clipboard changed after the paste; leaving it");
            return;
        }
        if let Err(error) = selection.restore(original) {
            tracing::warn!(
                error = format!("{error:#}"),
                "could not restore the clipboard after a paste"
            );
        }
    });
}

/// What a paste puts on the clipboard.
#[derive(Clone, Copy)]
enum Mime {
    Text,
    Png,
}

impl Mime {
    fn wayland(self) -> &'static str {
        match self {
            Self::Text => "text/plain;charset=utf-8",
            Self::Png => "image/png",
        }
    }
}

struct X11Paster {
    clipboard: Clipboard,
    connection: RustConnection,
    root: u32,
    control: u8,
    shift: u8,
    v: u8,
}

impl X11Paster {
    fn new() -> Result<Self> {
        let clipboard = Clipboard::new().wrap_err("could not open the X11 clipboard")?;
        let (connection, root, keymap) = connect_x11()?;
        Ok(Self {
            clipboard,
            connection,
            root,
            control: keymap.keycode(XK_CONTROL_L)?,
            shift: keymap.keycode(XK_SHIFT_L)?,
            v: keymap.keycode(XK_V)?,
        })
    }

    fn paste(
        &mut self,
        mime: Mime,
        bytes: &[u8],
        stop: &AtomicBool,
        paste_with_shift: bool,
    ) -> Result<()> {
        self.wait_for_modifier_release(stop)?;
        let atoms = &self.clipboard.getter.atoms;
        let target = match mime {
            Mime::Text => atoms.utf8_string,
            Mime::Png => self
                .clipboard
                .getter
                .get_atom("image/png")
                .wrap_err("could not name the X11 image/png target")?,
        };
        self.clipboard
            .store(atoms.clipboard, target, bytes)
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
            if key == self.shift && !paste_with_shift {
                continue;
            }
            xtest::fake_input(&self.connection, type_, key, 0, self.root, 0, 0, 0)?.check()?;
        }
        self.connection.flush()?;
        Ok(())
    }

    fn wait_for_modifier_release(&self, stop: &AtomicBool) -> Result<()> {
        wait_for_modifiers(stop, || {
            let mask = self.connection.query_pointer(self.root)?.reply()?.mask;
            let held = mask
                & (KeyButMask::SHIFT | KeyButMask::CONTROL | KeyButMask::MOD1 | KeyButMask::MOD4);
            Ok(held != KeyButMask::default())
        })
    }
}

fn paste_keys(with_shift: bool) -> &'static [&'static str] {
    // GTK 3's reverse keymap lookup skips the final keycode. Keep V before a
    // neutral VoidSymbol release so its shortcut remains discoverable (#28).
    if with_shift {
        &[
            "-M",
            "ctrl",
            "-M",
            "shift",
            "-k",
            "v",
            "-m",
            "shift",
            "-m",
            "ctrl",
            "-p",
            "VoidSymbol",
        ]
    } else {
        &["-M", "ctrl", "-k", "v", "-m", "ctrl", "-p", "VoidSymbol"]
    }
}

fn wait_for_modifiers(stop: &AtomicBool, mut held: impl FnMut() -> Result<bool>) -> Result<()> {
    loop {
        if stop.load(Ordering::Acquire) {
            return Err(eyre!("paste cancelled while stopping the listener"));
        }
        if !held()? {
            return Ok(());
        }
        // Another capture may own these modifiers. Keep its predecessor queued
        // instead of throwing away accepted output after an arbitrary timeout.
        thread::sleep(Duration::from_millis(20));
    }
}

fn run_helper(
    mut command: Command,
    input: &[u8],
    stop: &AtomicBool,
    timeout: Duration,
) -> Result<()> {
    if stop.load(Ordering::Acquire) {
        return Err(eyre!("paste cancelled before starting a helper"));
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .wrap_err("could not start the paste helper")?;
    let process_group = child.id();
    let result = (|| {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| eyre!("missing helper stdin"))?;
        let fd = stdin.as_raw_fd();
        // Nonblocking writes keep a stalled helper from bypassing cancellation
        // or the deadline when a long transcript fills its stdin pipe.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let deadline = Instant::now() + timeout;
        let mut remaining = input;
        while !remaining.is_empty() {
            if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err(eyre!("paste helper cancelled or timed out"));
            }
            match stdin.write(remaining) {
                Ok(0) => return Err(eyre!("paste helper stopped reading its input")),
                Ok(written) => remaining = &remaining[written..],
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error.into()),
            }
        }
        drop(stdin);
        loop {
            if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
                return Err(eyre!("paste helper cancelled or timed out"));
            }
            if let Some(status) = child.try_wait()? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(eyre!("paste helper failed with {status}"))
                };
            }
            thread::sleep(Duration::from_millis(10));
        }
    })();
    if result.is_err() {
        // Terminate descendants too: a failed wl-copy must not claim the
        // clipboard later, after this request has been reported as cancelled.
        unsafe { libc::kill(-(process_group as i32), libc::SIGKILL) };
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn helper_failure_is_not_reported_as_a_successful_paste() {
        assert!(
            run_helper(
                shell("exit 7"),
                &[],
                &AtomicBool::new(false),
                HELPER_TIMEOUT
            )
            .is_err()
        );
    }

    #[test]
    fn standard_and_terminal_paste_balance_their_modifiers() {
        assert_eq!(
            paste_keys(false),
            ["-M", "ctrl", "-k", "v", "-m", "ctrl", "-p", "VoidSymbol"]
        );
        assert_eq!(
            paste_keys(true),
            [
                "-M",
                "ctrl",
                "-M",
                "shift",
                "-k",
                "v",
                "-m",
                "shift",
                "-m",
                "ctrl",
                "-p",
                "VoidSymbol"
            ]
        );
    }

    #[test]
    fn helper_can_read_a_large_transcript_without_argv_or_files() {
        let text = b"--help\nplain text\n".repeat(32_768);
        let mut command = shell("test \"$(wc -c)\" -eq \"$1\"");
        command.args(["hex-paste-test", &text.len().to_string()]);
        assert!(run_helper(command, &text, &AtomicBool::new(false), HELPER_TIMEOUT).is_ok());
    }

    #[test]
    fn helper_timeout_includes_a_full_stdin_pipe() {
        let start = Instant::now();
        assert!(
            run_helper(
                shell("sleep 10"),
                &vec![b'x'; 1_048_576],
                &AtomicBool::new(false),
                Duration::from_millis(100),
            )
            .is_err()
        );
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn stop_cancels_a_helper_that_never_finishes() {
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = stop.clone();
        let worker = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            cancel.store(true, Ordering::Release);
        });
        let start = Instant::now();
        let result = run_helper(shell("sleep 10"), &[], &stop, HELPER_TIMEOUT);
        worker.join().unwrap();
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn a_clipboard_daemon_inheriting_stderr_does_not_block_parent_completion() {
        let path = std::env::temp_dir().join(format!("hex-paste-daemon-{}", std::process::id()));
        let mut command = shell("cat >/dev/null; sleep 10 >&2 & echo $! >\"$1\"");
        command.arg("hex-paste-test").arg(&path);
        let result = run_helper(
            command,
            b"example",
            &AtomicBool::new(false),
            Duration::from_secs(2),
        );
        if let Ok(pid) = std::fs::read_to_string(&path) {
            if let Ok(pid) = pid.trim().parse::<i32>() {
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
            let _ = std::fs::remove_file(path);
        }
        assert!(result.is_ok());
    }

    #[test]
    fn accepted_output_waits_through_another_capture() {
        let start = Instant::now();
        let mut checks = 0;
        wait_for_modifiers(&AtomicBool::new(false), || {
            checks += 1;
            Ok(start.elapsed() < Duration::from_millis(550))
        })
        .unwrap();
        assert!(checks > 1);
    }

    #[test]
    fn stopping_breaks_the_modifier_wait_without_checking_the_keyboard() {
        assert!(
            wait_for_modifiers(&AtomicBool::new(true), || panic!("should not read devices"))
                .is_err()
        );
        assert!(run_helper(shell("exit 0"), &[], &AtomicBool::new(true), HELPER_TIMEOUT).is_err());
    }

    #[test]
    #[ignore = "run scripts/test-wayland-paste.sh with the compiled test executable"]
    fn native_wayland_clipboard_shortcut() {
        assert!(LinuxSession::detect().is_wayland());
        let path = std::env::var_os("HEX_WAYLAND_PASTE_OUTPUT")
            .expect("the isolated Wayland test runner must supply its output file");
        let expected = "Example dictation: --flags, punctuation, \u{6f22}\u{5b57}.\nSecond line.";
        let wait_for = |what: &str, done: &mut dyn FnMut() -> bool| {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !done() {
                assert!(Instant::now() < deadline, "timed out waiting for {what}");
                thread::sleep(Duration::from_millis(20));
            }
        };
        let wl_paste = |args: &[&str]| {
            Command::new("wl-paste")
                .args(args)
                .stderr(Stdio::null())
                .output()
                .unwrap()
        };
        let original = vec![
            ("text/html".to_owned(), b"<b>original</b>".to_vec()),
            ("text/plain;charset=utf-8".to_owned(), b"original".to_vec()),
        ];
        Selection::Wayland
            .restore(Snapshot::Wayland(original.clone()))
            .unwrap();
        let mut paster = LinuxPaster::new(
            Arc::new(AtomicBool::new(false)),
            Default::default(),
            Default::default(),
        )
        .unwrap();
        paster.paste(expected).unwrap();
        wait_for("GTK to receive the transcript", &mut || {
            std::fs::read_to_string(&path).unwrap_or_default() == expected
        });
        // Every format of the previous clipboard comes back.
        wait_for("the original clipboard", &mut || {
            original.iter().all(|(mime, data)| {
                let output = wl_paste(&["--no-newline", "--type", mime]);
                output.status.success() && output.stdout == *data
            })
        });

        // An empty clipboard stays empty.
        assert!(
            Command::new("wl-copy")
                .arg("--clear")
                .status()
                .unwrap()
                .success()
        );
        wait_for("the clipboard to clear", &mut || {
            !wl_paste(&["--list-types"]).status.success()
        });
        paster.paste(expected).unwrap();
        let twice = expected.repeat(2);
        wait_for("GTK to receive the second transcript", &mut || {
            std::fs::read_to_string(&path).unwrap_or_default() == twice
        });
        thread::sleep(RESTORE_DELAY * 2);
        assert!(
            !wl_paste(&["--list-types"]).status.success(),
            "the transcript stayed on a clipboard that was empty"
        );
    }
}

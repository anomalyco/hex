use std::io::{ErrorKind, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
const HELPER_TIMEOUT: Duration = Duration::from_secs(3);
const PASTE_SETTLE: Duration = Duration::from_millis(100);

pub struct LinuxPaster {
    backend: Backend,
    stop: Arc<AtomicBool>,
    paste_with_shift: bool,
}

enum Backend {
    X11(Box<X11Paster>),
    Wayland,
}

impl LinuxPaster {
    pub fn new(stop: Arc<AtomicBool>, paste_with_shift: bool) -> Result<Self> {
        let backend = match LinuxSession::detect() {
            LinuxSession::X11 => Backend::X11(Box::new(X11Paster::new()?)),
            LinuxSession::Wayland => Backend::Wayland,
        };
        Ok(Self {
            backend,
            stop,
            paste_with_shift,
        })
    }

    pub fn paste(&mut self, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Err(eyre!("refusing to paste an empty transcript"));
        }
        match &mut self.backend {
            Backend::X11(paster) => paster.paste(text, &self.stop, self.paste_with_shift)?,
            Backend::Wayland => {
                wait_for_modifiers(&self.stop, || {
                    crate::linux_input::wayland_modifiers_held(&self.stop)
                })?;
                let mut copy = Command::new("wl-copy");
                copy.args(["--type", "text/plain;charset=utf-8"]);
                run_helper(copy, text.as_bytes(), &self.stop, HELPER_TIMEOUT)
                    .wrap_err("could not own the Wayland clipboard; install wl-clipboard and check compositor support")?;
                // wl-copy's parent exits only after the selection is installed.
                // Never read a pipe inherited by its clipboard-serving daemon.
                wait_for_modifiers(&self.stop, || {
                    crate::linux_input::wayland_modifiers_held(&self.stop)
                })?;
                let mut keys = Command::new("wtype");
                keys.args(paste_keys(self.paste_with_shift));
                run_helper(keys, &[], &self.stop, HELPER_TIMEOUT)
                    .wrap_err("could not send the Wayland paste shortcut; install wtype and use a compositor supporting virtual keyboards")?;
            }
        }
        // Give the target time to request the selection before the next queued
        // transcript replaces it. This is a bounded settling policy, not an ACK.
        thread::sleep(PASTE_SETTLE);
        Ok(())
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

    fn paste(&mut self, text: &str, stop: &AtomicBool, paste_with_shift: bool) -> Result<()> {
        self.wait_for_modifier_release(stop)?;
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
        let stop = AtomicBool::new(false);
        let mut copy = Command::new("wl-copy");
        copy.args(["--type", "text/plain;charset=utf-8"]);
        run_helper(copy, expected.as_bytes(), &stop, HELPER_TIMEOUT).unwrap();
        let mut keys = Command::new("wtype");
        keys.args(paste_keys(false));
        run_helper(keys, &[], &stop, HELPER_TIMEOUT).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let actual = std::fs::read_to_string(&path).unwrap_or_default();
            if actual == expected {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "GTK did not receive the transcript: {actual:?}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}

//! The local API a one-shot command talks to: the running Hex's, chosen once
//! from its discovery file, or with no Hex running an embedded
//! `hex service --embedded` this command starts and owns, the way the
//! TypeScript SDK owns its direct child. The child never publishes itself,
//! exits when its stdin closes, and is released on every exit: success,
//! failure, and Ctrl-C.

use std::io::{BufRead as _, BufReader, Read as _};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::local_api::{Discovered, Endpoint};

/// The child prints its ready line once its listener is bound; it loads no
/// model first.
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_READY_LINE: u64 = 16 * 1024;
/// An embedded service exits soon after its stdin closes (and forces its
/// own exit after 5 s); past this it is killed.
const RELEASE_WAIT: Duration = Duration::from_secs(3);

static OWNED: Mutex<Option<Owned>> = Mutex::new(None);
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

struct Owned {
    child: Child,
    /// The child's lease: it runs while this stays open.
    stdin: Option<ChildStdin>,
}

/// The running Hex's endpoint, or a started embedded service's when no Hex
/// runs. A Hex that runs but can't be used is an error, never a reason to
/// start a second service beside it.
pub(super) fn endpoint() -> Result<Endpoint, String> {
    match crate::local_api::discover() {
        Ok(Discovered::Live(endpoint)) => Ok(endpoint),
        Ok(Discovered::Absent) => start(),
        Err(error) => Err(format!("{error:#}")),
    }
}

fn lock() -> std::sync::MutexGuard<'static, Option<Owned>> {
    OWNED.lock().unwrap_or_else(|error| error.into_inner())
}

fn start() -> Result<Endpoint, String> {
    let executable =
        std::env::current_exe().map_err(|error| format!("couldn't find Hex itself: {error}"))?;
    let mut command = crate::desktop::background_process::command(executable);
    command
        .args(["service", "--embedded"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // Its own process group, so a terminal's Ctrl-C reaches only this
    // command, which then releases the child in order.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command
        .spawn()
        .map_err(|error| format!("couldn't start Hex's transcription service: {error}"))?;
    let pid = child.id();
    let stdout = child.stdout.take();
    let stdin = child.stdin.take();
    *lock() = Some(Owned { child, stdin });
    let line = stdout
        .ok_or_else(|| "Hex's transcription service has no output".to_owned())
        .and_then(ready_line);
    let endpoint =
        line.and_then(|line| Endpoint::embedded(&line, pid).map_err(|error| format!("{error:#}")));
    if endpoint.is_err() {
        release();
    }
    endpoint.map_err(|error| format!("Hex's transcription service didn't start: {error}"))
}

/// The one line the child prints once it listens, read on a thread so a
/// child that never prints can't hold this command past the deadline.
fn ready_line(stdout: std::process::ChildStdout) -> Result<Vec<u8>, String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = Vec::new();
        let read = BufReader::new(stdout)
            .take(MAX_READY_LINE + 1)
            .read_until(b'\n', &mut line)
            .map(|_| line);
        let _ = sender.send(read);
    });
    match receiver.recv_timeout(READY_TIMEOUT) {
        Ok(Ok(line)) if line.len() as u64 > MAX_READY_LINE => {
            Err("its ready line is too long".into())
        }
        Ok(Ok(line)) if line.ends_with(b"\n") => Ok(line),
        Ok(Ok(_)) => Err("it exited before it was ready".into()),
        Ok(Err(error)) => Err(format!("couldn't read it: {error}")),
        Err(_) => Err(format!(
            "it wasn't ready within {} s",
            READY_TIMEOUT.as_secs()
        )),
    }
}

/// Ends an owned child: closes its stdin, waits a moment for it to exit,
/// then kills it, and reaps it either way. Without one it does nothing.
pub(super) fn release() {
    let Some(mut owned) = lock().take() else {
        return;
    };
    drop(owned.stdin.take());
    let deadline = Instant::now() + RELEASE_WAIT;
    loop {
        match owned.child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => break,
        }
    }
    let _ = owned.child.kill();
    let _ = owned.child.wait();
}

/// Ctrl-C (and termination) releases the child, clears any progress line,
/// and exits 130 with nothing on stdout.
pub(super) fn exit_on_interrupt() {
    let _ = ctrlc::set_handler(|| {
        INTERRUPTED.store(true, Ordering::Release);
        release();
        super::progress::clear();
        std::process::exit(130);
    });
}

/// Releases the child once the command's work is done. A request that
/// failed because Ctrl-C ended the child exits 130 here instead of reporting
/// that failure.
pub(super) fn done() {
    release();
    if INTERRUPTED.load(Ordering::Acquire) {
        super::progress::clear();
        std::process::exit(130);
    }
}

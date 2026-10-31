//! Hex Microphone on Linux: a PipeWire virtual source that `pw-cat`
//! (shipped with PipeWire) plays the filtered microphone into. Apps list it
//! as an input like any microphone. Nothing is installed and no root is
//! needed: it exists while the Voice Filter is on and Hex runs, since `pw-cat`
//! ends when Hex closes its input.

use super::*;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::AsRawFd;
use std::process::{Child, ChildStdin, Command, Stdio};

/// A low session priority keeps WirePlumber from choosing Hex
/// Microphone as the default input, which Hex itself would record.
const PROPERTIES: &str = "{ media.class = Audio/Source node.name = hex_microphone \
    node.description = \"Hex Microphone\" application.name = Hex \
    priority.session = 1 priority.driver = 1 }";
/// Frames per write (11 ms): half the pipe, so one block waits in the
/// pipe while the next gathers, and large enough that the ring's
/// latency cap never trims audio between a graph cycle's reads.
const BLOCK_FRAMES: usize = 512;
/// How long a write waits for the microphone to queue a whole block
/// before it pads with silence: one block, so a released microphone
/// still delivers silence in real time.
const BLOCK_WAIT: Duration = Duration::from_millis(11);
/// The smallest pipe Linux allows (one page, 21 ms of audio), so
/// `pw-cat` never reads far behind the microphone.
const PIPE_BYTES: libc::c_int = 4096;
/// A source that ran this long started; an earlier failure stays shown.
const STARTED_AFTER: Duration = Duration::from_secs(1);
const RETRY: Duration = Duration::from_secs(2);

fn spawn() -> std::io::Result<Child> {
    Command::new("pw-cat")
        .args([
            "--playback",
            "--raw",
            "--format",
            "f32",
            "--rate",
            "48000",
            "--channels",
            "1",
            "--latency",
            "10ms",
            "--properties",
            PROPERTIES,
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
}

/// Plays the ring through the filter until `pw-cat` goes away. Writes
/// block while no app records Hex Microphone; the ring then drops the
/// oldest audio, and its latency cap skips ahead when an app starts.
fn feed(mut input: ChildStdin) {
    let mut engine = current_tuned_config().build_engine();
    let mut block = [0.0_f32; BLOCK_FRAMES];
    let mut bytes = [[0_u8; 4]; BLOCK_FRAMES];
    loop {
        let deadline = Instant::now() + BLOCK_WAIT;
        while CAPTURE_RING.queued_frames() < BLOCK_FRAMES && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        render(&mut engine, &mut block);
        for (out, sample) in bytes.iter_mut().zip(block) {
            *out = sample.to_le_bytes();
        }
        if input.write_all(bytes.as_flattened()).is_err() {
            return;
        }
    }
}

/// The last line `pw-cat` wrote before it exited, as a failure reason.
fn reason(stderr: &str, status: std::process::ExitStatus) -> String {
    stderr
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(|line| line.chars().take(140).collect())
        .unwrap_or_else(|| format!("pw-cat exited ({status})"))
}

pub(super) fn run_supervisor() {
    loop {
        if !crate::app_settings::hex_microphone_enabled() {
            set_driver_action_state(DriverActionState::Idle);
            thread::sleep(Duration::from_millis(500));
            continue;
        }
        let mut child = match spawn() {
            Ok(child) => child,
            Err(error) => {
                let reason = if error.kind() == ErrorKind::NotFound {
                    "pw-cat was not found; Hex Microphone needs PipeWire".to_string()
                } else {
                    error.to_string()
                };
                tracing::warn!(%reason, "could not start the Hex Microphone source");
                set_driver_action_state(DriverActionState::Failed(reason));
                thread::sleep(RETRY);
                continue;
            }
        };
        let (Some(input), Some(mut stderr)) = (child.stdin.take(), child.stderr.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            thread::sleep(RETRY);
            continue;
        };
        // SAFETY: `input` is this process's open write end of the pipe.
        unsafe { libc::fcntl(input.as_raw_fd(), libc::F_SETPIPE_SZ, PIPE_BYTES) };
        let errors = thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });
        CAPTURE_RING.clear();
        SINK_STREAMING.store(true, Ordering::Release);
        let writer = thread::Builder::new()
            .name("hex-mic-feed".into())
            .spawn(move || feed(input));
        tracing::info!("Hex Microphone virtual source started");
        let started = Instant::now();
        let mut confirmed = false;
        let exited = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {}
                Err(_) => break None,
            }
            if !crate::app_settings::hex_microphone_enabled() {
                break None;
            }
            if !confirmed && started.elapsed() >= STARTED_AFTER {
                confirmed = true;
                set_driver_action_state(DriverActionState::Idle);
            }
            thread::sleep(Duration::from_millis(250));
        };
        SINK_STREAMING.store(false, Ordering::Release);
        if exited.is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
        if let Ok(writer) = writer {
            let _ = writer.join();
        }
        let stderr = errors.join().unwrap_or_default();
        CAPTURE_RING.clear();
        match exited {
            Some(status) => {
                let reason = reason(&stderr, status);
                tracing::warn!(%reason, "Hex Microphone virtual source stopped; restarting");
                set_driver_action_state(DriverActionState::Failed(reason));
                thread::sleep(RETRY);
            }
            None => tracing::info!("Hex Microphone virtual source stopped"),
        }
    }
}

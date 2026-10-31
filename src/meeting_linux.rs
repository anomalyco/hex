//! The Linux side of meetings: system audio from a PipeWire monitor of the
//! default output, and the file manager and Trash.
//!
//! `pw-record` captures the monitor (`stream.capture.sink`), which the
//! session manager links to whatever the default output is, following it
//! when it changes. It runs as a child process so Hex links no PipeWire
//! library and a desktop without PipeWire fails only this source.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Instant, SystemTime};

use color_eyre::eyre::{Result, WrapErr, eyre};

use super::SAMPLE_RATE;
use super::capture::{LiveSource, POLL_INTERVAL, PacketSink, STALL_TIMEOUT};

const PW_RECORD: &str = "pw-record";
/// 100 ms of 16 kHz mono `f32`.
const READ_BYTES: usize = SAMPLE_RATE as usize / 10 * 4;
const QUEUE_CAPACITY: usize = 128;

/// Fails until `pw-record`, which hears the other participants, is installed.
pub(super) fn ensure_system_audio() -> Result<()> {
    if find_executable(PW_RECORD).is_some() {
        Ok(())
    } else {
        Err(eyre!(
            "Install PipeWire's pw-record (the pipewire-audio package) so Hex can hear the other participants, then try again"
        ))
    }
}

/// Ends `command`'s process with the thread that starts it, so a helper
/// never outlives a Hex that stopped without dropping it.
fn tied_to_thread(command: &mut Command) -> &mut Command {
    // SAFETY: `prctl` is async-signal-safe and touches only the child.
    unsafe {
        command.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        })
    }
}

fn find_executable(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .chain(["/usr/bin".into(), "/bin".into()])
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

pub(super) fn open_system_audio(_sink: &PacketSink) -> Result<Box<dyn LiveSource>> {
    MonitorSource::start().map(|source| Box::new(source) as Box<dyn LiveSource>)
}

/// The default output's monitor through `pw-record`, as 16 kHz mono `f32`.
struct MonitorSource {
    child: Child,
    buffers: Receiver<Vec<f32>>,
    last_audio: Instant,
}

impl MonitorSource {
    fn start() -> Result<Self> {
        let executable =
            find_executable(PW_RECORD).ok_or_else(|| eyre!("{PW_RECORD} is not installed"))?;
        let mut child = tied_to_thread(&mut Command::new(executable))
            .args([
                "--raw",
                "--rate",
                &SAMPLE_RATE.to_string(),
                "--channels",
                "1",
                "--format",
                "f32",
                "--media-category",
                "Capture",
                "-P",
                "{ stream.capture.sink = true node.name = hex-meeting-system-audio \
                 node.description = \"Hex meeting system audio\" }",
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .wrap_err("could not start pw-record")?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| eyre!("pw-record has no output"))?;
        let (sender, buffers) = mpsc::sync_channel::<Vec<f32>>(QUEUE_CAPACITY);
        thread::Builder::new()
            .name("meeting-pw-record".into())
            .spawn(move || {
                let mut bytes = vec![0_u8; READ_BYTES];
                let mut carry = Vec::<u8>::new();
                loop {
                    let read = match stdout.read(&mut bytes) {
                        Ok(0) | Err(_) => return,
                        Ok(read) => read,
                    };
                    carry.extend_from_slice(&bytes[..read]);
                    let whole = carry.len() / 4 * 4;
                    let samples = carry[..whole]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|sample| f32::from_le_bytes(*sample))
                        .collect::<Vec<_>>();
                    carry.drain(..whole);
                    if sender.send(samples).is_err() {
                        return;
                    }
                }
            })
            .wrap_err("could not start the pw-record reader")?;
        tracing::info!("system audio monitor started");
        Ok(Self {
            child,
            buffers,
            last_audio: Instant::now(),
        })
    }
}

impl LiveSource for MonitorSource {
    fn poll(&mut self, sink: &PacketSink) -> std::result::Result<(), String> {
        match self.buffers.recv_timeout(POLL_INTERVAL) {
            Ok(mut samples) => {
                while let Ok(next) = self.buffers.try_recv() {
                    samples.extend(next);
                }
                self.last_audio = Instant::now();
                sink.send(SAMPLE_RATE, samples, None);
                Ok(())
            }
            Err(mpsc::RecvTimeoutError::Timeout) if self.last_audio.elapsed() > STALL_TIMEOUT => {
                Err("the system audio monitor stopped delivering audio".into())
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(()),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let status = self.child.try_wait().ok().flatten();
                Err(match status {
                    Some(status) => format!("pw-record exited ({status})"),
                    None => "pw-record closed its output".into(),
                })
            }
        }
    }

    fn describe(&self) -> &'static str {
        "pipewire-monitor"
    }
}

impl Drop for MonitorSource {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Opens the folder holding `path` (or `path` itself, when a folder).
pub(crate) fn reveal(path: &Path) {
    let folder = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    if let Err(error) = Command::new("xdg-open")
        .arg(folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        tracing::error!(%error, "could not reveal meeting file");
    }
}

/// Moves `path` to the home Trash as the freedesktop.org Trash
/// specification lays it out, so file managers can restore it.
pub(super) fn move_to_trash(path: &Path) -> Result<()> {
    let data = dirs::data_dir().ok_or_else(|| eyre!("no data directory for the Trash"))?;
    move_to_trash_in(&data.join("Trash"), path, SystemTime::now())
}

fn move_to_trash_in(trash: &Path, path: &Path, now: SystemTime) -> Result<()> {
    let path = std::path::absolute(path)?;
    let files = trash.join("files");
    let info = trash.join("info");
    std::fs::create_dir_all(&files)?;
    std::fs::create_dir_all(&info)?;
    let name = path
        .file_name()
        .ok_or_else(|| eyre!("{} has no name", path.display()))?
        .to_string_lossy()
        .into_owned();
    let deleted_ms = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let deleted = super::transcript::LocalTime::from_ms(deleted_ms).iso();
    for attempt in 0.. {
        let candidate = if attempt == 0 {
            name.clone()
        } else {
            format!("{name}.{attempt}")
        };
        let info_path = info.join(format!("{candidate}.trashinfo"));
        let claimed = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&info_path);
        let mut file = match claimed {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        };
        std::io::Write::write_all(
            &mut file,
            format!(
                "[Trash Info]\nPath={}\nDeletionDate={deleted}\n",
                percent_encode(&path.to_string_lossy())
            )
            .as_bytes(),
        )?;
        if let Err(error) = std::fs::rename(&path, files.join(&candidate)) {
            let _ = std::fs::remove_file(&info_path);
            return Err(eyre!(
                "could not move {} to the Trash: {error}",
                path.display()
            ));
        }
        return Ok(());
    }
    unreachable!("the loop returns")
}

/// Percent-encodes a path for a `.trashinfo` file, keeping `/`.
fn percent_encode(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-_.~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trashing_writes_restorable_info_and_never_overwrites() {
        let root = std::env::temp_dir().join(format!("hex-trash-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let trash = root.join("Trash");
        let meeting = root.join("my meeting");
        for _ in 0..2 {
            std::fs::create_dir_all(&meeting).unwrap();
            std::fs::write(meeting.join("manifest.json"), "{}").unwrap();
            move_to_trash_in(&trash, &meeting, SystemTime::UNIX_EPOCH).unwrap();
        }
        assert!(!meeting.exists());
        assert!(trash.join("files/my meeting/manifest.json").exists());
        assert!(trash.join("files/my meeting.1/manifest.json").exists());
        let info = std::fs::read_to_string(trash.join("info/my meeting.trashinfo")).unwrap();
        assert!(info.contains("/my%20meeting\n"), "{info}");
        assert!(info.contains("DeletionDate="), "{info}");
        std::fs::remove_dir_all(&root).unwrap();
    }
}

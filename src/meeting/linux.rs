//! The Linux side of meetings: system audio from every output's monitor, and
//! the file manager and Trash.
//!
//! One `pw-record` per output captures that output's monitor
//! (`stream.capture.sink`), and the source mixes them, so a call that plays on
//! a headset or dock instead of the default output is still heard (#1801).
//! Each runs as a child process so Hex links no PipeWire library and a desktop
//! without PipeWire fails only this source.

use std::collections::BTreeSet;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use color_eyre::eyre::{Result, WrapErr, eyre};

use super::SAMPLE_RATE;
use super::capture::{LiveSource, MIX_HOLD, POLL_INTERVAL, PacketSink, STALL_TIMEOUT, mix};

const PW_RECORD: &str = "pw-record";
/// 100 ms of 16 kHz mono `f32`.
const READ_BYTES: usize = SAMPLE_RATE as usize / 10 * 4;
/// Messages buffered for each capture, and the most the source buffers in
/// total however many outputs it hears.
const QUEUE_PER_CAPTURE: usize = 128;
const QUEUE_CAPACITY: usize = 1_024;
/// How often the source checks that the outputs to capture have not changed.
const ROUTE_CHECK: Duration = Duration::from_secs(2);
/// The PipeWire node every system audio monitor presents.
const NODE_NAME: &str = "hex-meeting-system-audio";
const NODE_DESCRIPTION: &str = "Hex meeting system audio";

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

pub(super) fn open_system_audio(
    _sink: &PacketSink,
    _application: Option<&str>,
) -> Result<Box<dyn LiveSource>> {
    MonitorSource::start().map(|source| Box::new(source) as Box<dyn LiveSource>)
}

/// Every captured output's `pw-record`, mixed into one 16 kHz mono `f32`
/// system track.
struct MonitorSource {
    /// One capture per output, in capture order.
    outputs: Vec<Output>,
    /// The sinks asked for, empty when one untargeted `pw-record` follows the
    /// default output. A changed list reopens the source.
    sinks: Vec<String>,
    buffers: Receiver<MonitorEvent>,
    last_audio: Instant,
    route_checked: Instant,
    /// The last capture's exit, so the all-closed error can name it.
    last_exit: Option<String>,
}

/// One captured output: its `pw-record`, what it hears, and its audio not yet
/// mixed.
struct Output {
    /// The sink it captures, or `None` when it follows the default output.
    sink: Option<String>,
    /// `None` once this capture ended; the others keep the track going.
    child: Option<Child>,
    pending: Vec<f32>,
}

/// What one capture sends: its 16 kHz audio as `index` into `outputs`, or that
/// it ended.
enum MonitorEvent {
    Audio(usize, Vec<f32>),
    Closed(usize),
}

/// Whether a sink name can be handed to `pw-record`'s property string as it
/// is. `pactl` names sinks without spaces or quotes; anything else is skipped
/// rather than allowed to change that string.
fn capturable(sink: &str) -> bool {
    !sink.is_empty()
        && !sink.chars().any(|character| {
            character.is_whitespace() || matches!(character, '"' | '\'' | '\\' | '{' | '}')
        })
}

/// The sinks to capture, default first, skipping any name `pw-record` cannot
/// be given; `None` when the sound server cannot be read or lists nothing.
fn capturable_sinks() -> Option<Vec<String>> {
    Some(
        crate::audio::output_route::meeting_sinks()?
            .into_iter()
            .filter(|sink| {
                let usable = capturable(sink);
                if !usable {
                    tracing::warn!(%sink, "skipping an output whose name cannot be captured");
                }
                usable
            })
            .collect(),
    )
}

/// Whether the sinks captured are still the ones listed, compared as a set so
/// the default moving between them is not a change.
fn sinks_changed(open: &[String], listed: &[String]) -> bool {
    let open = open.iter().collect::<BTreeSet<_>>();
    let listed = listed.iter().collect::<BTreeSet<_>>();
    open != listed
}

/// Whether the source should reopen: its captures moved with the listed sinks,
/// or it is still on the untargeted fallback and sinks can now be listed.
/// `None` is a sound server Hex could not read, which never reopens.
fn route_needs_reopen(open: &[String], listed: Option<&[String]>) -> bool {
    match listed {
        None => false,
        Some(listed) if open.is_empty() => !listed.is_empty(),
        Some(listed) => sinks_changed(open, listed),
    }
}

impl MonitorSource {
    fn start() -> Result<Self> {
        let executable =
            find_executable(PW_RECORD).ok_or_else(|| eyre!("{PW_RECORD} is not installed"))?;
        let sinks = capturable_sinks().unwrap_or_else(|| {
            tracing::debug!(
                "could not read the sound server's sinks; following the default output"
            );
            Vec::new()
        });
        // One capture per output, or one that follows the default output when
        // the sound server cannot be read.
        let targets = if sinks.is_empty() {
            vec![None]
        } else {
            sinks.iter().map(String::as_str).map(Some).collect()
        };
        let queue = (QUEUE_PER_CAPTURE * targets.len()).min(QUEUE_CAPACITY);
        let (sender, buffers) = mpsc::sync_channel(queue);
        let mut outputs = Vec::with_capacity(targets.len());
        let mut first_error = None;
        for target in targets {
            match spawn_monitor(&executable, target, outputs.len(), sender.clone()) {
                Ok(child) => outputs.push(Output {
                    sink: target.map(str::to_owned),
                    child: Some(child),
                    pending: Vec::new(),
                }),
                Err(error) => {
                    tracing::warn!(
                        sink = target.unwrap_or("the default output"),
                        error = %format!("{error:#}"),
                        "could not hear an audio output"
                    );
                    first_error.get_or_insert(error);
                }
            }
        }
        // Every reader thread holds its own sender, so a source with no
        // capture left reports it as disconnected.
        drop(sender);
        if outputs.is_empty() {
            return Err(
                first_error.unwrap_or_else(|| eyre!("no audio output is available to capture"))
            );
        }
        if sinks.is_empty() {
            tracing::info!("system audio monitor follows the default output");
        } else {
            tracing::info!(sinks = ?sinks, "system audio monitors started");
        }
        Ok(Self {
            outputs,
            sinks,
            buffers,
            last_audio: Instant::now(),
            route_checked: Instant::now(),
            last_exit: None,
        })
    }

    /// Applies one event from a capture.
    fn take(&mut self, event: MonitorEvent) {
        match event {
            MonitorEvent::Audio(index, samples) => {
                if let Some(output) = self.outputs.get_mut(index) {
                    output.pending.extend(samples);
                    self.last_audio = Instant::now();
                }
            }
            MonitorEvent::Closed(index) => self.close(index),
        }
    }

    /// Drops one capture that ended, so the others keep recording and its
    /// unpicked audio never replays later.
    fn close(&mut self, index: usize) {
        let reason = {
            let Some(output) = self.outputs.get_mut(index) else {
                return;
            };
            let Some(mut child) = output.child.take() else {
                return;
            };
            let status = child.try_wait().ok().flatten();
            let _ = child.kill();
            let _ = child.wait();
            output.pending.clear();
            let sink = output.sink.as_deref().unwrap_or("the default output");
            match status {
                Some(status) => format!("{sink}: pw-record exited ({status})"),
                None => format!("{sink}: pw-record closed its output"),
            }
        };
        tracing::warn!(%reason, "system audio monitor stopped");
        self.last_exit = Some(reason);
    }

    /// Whether the outputs to capture changed since they were opened. A sound
    /// server that cannot be read, or lists nothing, keeps what is open.
    fn route_changed(&self) -> bool {
        route_needs_reopen(&self.sinks, capturable_sinks().as_deref())
    }

    /// Why no capture is left, for the source's error.
    fn stopped(&self) -> String {
        self.last_exit
            .clone()
            .unwrap_or_else(|| "every system audio monitor stopped".into())
    }
}

/// The stream properties that capture `target`'s monitor as capture `index`,
/// or the default output's when `target` is `None`.
fn properties(target: Option<&str>, index: usize) -> String {
    let node = match target {
        // One node per output, so the graph lists each capture separately.
        Some(_) => format!("{NODE_NAME}-{index}"),
        None => NODE_NAME.to_owned(),
    };
    let mut properties = format!(
        "{{ stream.capture.sink = true node.name = {node} \
         node.description = \"{NODE_DESCRIPTION}\""
    );
    if target.is_some() {
        // A target that will not link must not be moved to the default
        // output: that would mix the default monitor's audio in a second time.
        properties.push_str(" node.dont-fallback = true node.dont-reconnect = true");
    }
    if let Some(target) = target {
        properties.push_str(&format!(" target.object = \"{target}\""));
    }
    properties.push_str(" }");
    properties
}

/// Starts one `pw-record` for `target`, or for whatever the session manager
/// links to the default output when `target` is `None`, and the thread that
/// reads its audio as `index`.
fn spawn_monitor(
    executable: &Path,
    target: Option<&str>,
    index: usize,
    sender: SyncSender<MonitorEvent>,
) -> Result<Child> {
    let properties = properties(target, index);
    let rate = SAMPLE_RATE.to_string();
    let mut child = tied_to_thread(&mut Command::new(executable))
        .args(crate::linux::pipewire::raw_args(executable))
        .args([
            "--rate",
            rate.as_str(),
            "--channels",
            "1",
            "--format",
            "f32",
            "--media-category",
            "Capture",
            "-P",
            properties.as_str(),
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .wrap_err("could not start pw-record")?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(eyre!("pw-record has no output"));
        }
    };
    let reader = thread::Builder::new()
        .name("meeting-pw-record".into())
        .spawn(move || read_monitor(stdout, index, sender));
    if let Err(error) = reader {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error).wrap_err("could not start the pw-record reader");
    }
    Ok(child)
}

/// Reads one capture's raw `f32` audio until it ends, tagging it with `index`.
fn read_monitor(mut stdout: ChildStdout, index: usize, sender: SyncSender<MonitorEvent>) {
    let mut bytes = vec![0_u8; READ_BYTES];
    let mut carry = Vec::<u8>::new();
    loop {
        let read = match stdout.read(&mut bytes) {
            Ok(0) | Err(_) => break,
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
        if sender.send(MonitorEvent::Audio(index, samples)).is_err() {
            return;
        }
    }
    let _ = sender.send(MonitorEvent::Closed(index));
}

impl LiveSource for MonitorSource {
    fn poll(&mut self, sink: &PacketSink) -> std::result::Result<(), String> {
        if self.route_checked.elapsed() >= ROUTE_CHECK {
            self.route_checked = Instant::now();
            if self.route_changed() {
                return Err("the audio outputs changed".into());
            }
        }
        match self.buffers.recv_timeout(POLL_INTERVAL) {
            Ok(first) => {
                self.take(first);
                while let Ok(next) = self.buffers.try_recv() {
                    self.take(next);
                }
                if self.outputs.iter().all(|output| output.child.is_none()) {
                    return Err(self.stopped());
                }
                let mut pending = self
                    .outputs
                    .iter_mut()
                    .map(|output| &mut output.pending)
                    .collect::<Vec<_>>();
                sink.send(SAMPLE_RATE, mix(&mut pending, MIX_HOLD), None);
                Ok(())
            }
            Err(mpsc::RecvTimeoutError::Timeout) if self.last_audio.elapsed() > STALL_TIMEOUT => {
                Err("the system audio monitors stopped delivering audio".into())
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(()),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(self.stopped()),
        }
    }

    fn describe(&self) -> &'static str {
        "pipewire-monitor"
    }
}

impl Drop for MonitorSource {
    fn drop(&mut self) {
        for output in &mut self.outputs {
            if let Some(mut child) = output.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

/// Opens the folder holding the meeting file `path`.
pub(crate) fn reveal(path: &Path) {
    open(path.parent().unwrap_or(path));
}

/// Opens the folder `path` itself.
pub(crate) fn open_folder(path: &Path) {
    open(path);
}

fn open(folder: &Path) {
    if let Err(error) = Command::new("xdg-open")
        .arg(folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        tracing::error!(%error, "could not open meeting path");
    }
}

/// Moves `path` to the home Trash as the freedesktop.org Trash
/// specification lays it out, so file managers can restore it.
pub(crate) fn move_to_trash(path: &Path) -> Result<()> {
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
    fn a_capture_targets_its_own_sink_and_the_fallback_keeps_the_default() {
        // The untargeted capture is today's exactly: the session manager links
        // it to the default output.
        assert_eq!(
            properties(None, 0),
            "{ stream.capture.sink = true node.name = hex-meeting-system-audio \
             node.description = \"Hex meeting system audio\" }"
        );
        // A targeted capture names its own node, never falls back to the
        // default output, and folds those flags into the property string.
        assert_eq!(
            properties(Some("bluez_output.AA_BB_CC_DD_EE_FF.1"), 2),
            "{ stream.capture.sink = true node.name = hex-meeting-system-audio-2 \
             node.description = \"Hex meeting system audio\" \
             node.dont-fallback = true node.dont-reconnect = true \
             target.object = \"bluez_output.AA_BB_CC_DD_EE_FF.1\" }"
        );
    }

    #[test]
    fn a_sink_name_that_would_change_the_property_string_is_skipped() {
        for sink in [
            "alsa_output.pci-0000_00_1f.3.analog-stereo",
            "alsa_output.usb-USB_Audio-00.analog-stereo",
            "bluez_output.AA_BB_CC_DD_EE_FF.1",
        ] {
            assert!(capturable(sink), "{sink}");
        }
        for sink in [
            "",
            " ",
            "with space",
            "quote\"",
            "brace}",
            "brace{",
            "slash\\",
        ] {
            assert!(!capturable(sink), "{sink:?}");
        }
    }

    #[test]
    fn a_reordered_sink_list_is_not_a_change() {
        // The default moving first is the same set of outputs; a sink added or
        // gone is a change the source reopens for.
        let open = ["speakers".to_owned(), "headset".to_owned()];
        assert!(!sinks_changed(
            &open,
            &["headset".to_owned(), "speakers".to_owned()]
        ));
        assert!(sinks_changed(&open, &["speakers".to_owned()]));
        assert!(sinks_changed(
            &open,
            &[
                "speakers".to_owned(),
                "headset".to_owned(),
                "dock".to_owned()
            ]
        ));
    }

    #[test]
    fn a_fallback_capture_reopens_once_the_sinks_can_be_read() {
        // The untargeted capture follows the default output; it moves to one
        // capture per output only when a non-empty list can be targeted.
        assert!(!route_needs_reopen(&[], None));
        assert!(!route_needs_reopen(&[], Some(&[])));
        assert!(route_needs_reopen(&[], Some(&["speakers".to_owned()])));
        // Opened captures survive a failed read or the same list, and reopen
        // when nothing listed can be targeted any more.
        let open = ["speakers".to_owned()];
        assert!(!route_needs_reopen(&open, None));
        assert!(!route_needs_reopen(&open, Some(&["speakers".to_owned()])));
        assert!(route_needs_reopen(&open, Some(&[])));
    }

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

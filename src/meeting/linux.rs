//! The Linux side of meetings: system audio, and the file manager and Trash.
//!
//! One `pw-record` per output captures that output's monitor
//! (`stream.capture.sink`), and the source mixes them, so a call that plays on
//! a headset or dock instead of the default output is still heard (#1801).
//! With Labs › Choose meeting audio on, the source instead opens one capture
//! for the chosen output, or one per running output stream of the chosen app,
//! and records silence while that source isn't present. An app's captures are
//! added and removed in place as its streams start and stop, so one that
//! opens another stream mid-call is heard without a gap; a changed choice
//! reopens the source. Each runs as a child process so Hex links no PipeWire
//! library and a desktop without PipeWire fails only this source. A capture
//! that ends while its target is still listed starts again, so a Bluetooth
//! card switching from A2DP to HFP does not silence that output for the rest
//! of the meeting.

use std::collections::BTreeSet;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use color_eyre::eyre::{Result, WrapErr, eyre};

use super::SAMPLE_RATE;
use super::audio_source::{SourceChoice, effective};
use super::capture::{
    LiveSource, MIX_HOLD, POLL_INTERVAL, PacketSink, Restart, STALL_TIMEOUT, mix,
};
use crate::microphone_activity::PlayingApplication;

const PW_RECORD: &str = "pw-record";
/// 100 ms of 16 kHz mono `f32`.
const READ_BYTES: usize = SAMPLE_RATE as usize / 10 * 4;
/// Messages buffered for each capture, and the most the source buffers in
/// total however many outputs it hears.
const QUEUE_PER_CAPTURE: usize = 128;
const QUEUE_CAPACITY: usize = 1_024;
/// How often the source re-reads what it captures: a changed list reopens
/// it, and a capture that ended starts again while its target is still
/// listed.
const ROUTE_CHECK: Duration = Duration::from_secs(2);
/// 100 ms of 16 kHz silence, recorded while a chosen source isn't present so
/// the system track keeps its place on the meeting's timeline.
const SILENT_PACKET: usize = SAMPLE_RATE as usize / 10;
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
    // A source re-reads the choice on its route check and reopens when it
    // differs, so turning the lab off, or choosing another source, applies
    // mid-recording.
    let choice = effective();
    MonitorSource::start(choice).map(|source| Box::new(source) as Box<dyn LiveSource>)
}

/// One thing the system track records: an output's monitor, or one running
/// output stream of a chosen application.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Target {
    /// An output, named as the sound server names it.
    Sink(String),
    /// One running output stream node, named as PipeWire names it for
    /// `target.object`.
    Node(String),
}

impl Target {
    /// The target's name, for a property string or a log line.
    fn label(&self) -> &str {
        match self {
            Self::Sink(name) | Self::Node(name) => name,
        }
    }
}

/// The targets a choice records, given the sinks and apps listed now. All
/// records every sink, an output only while it is listed, and an app only
/// its running output streams: a chosen source that isn't present records
/// nothing until it returns.
fn plan_targets(
    choice: &SourceChoice,
    sinks: &[String],
    apps: &[PlayingApplication],
) -> Vec<Target> {
    match choice {
        SourceChoice::All => sinks.iter().cloned().map(Target::Sink).collect(),
        SourceChoice::Output { id, .. } => {
            if sinks.iter().any(|sink| sink == id) {
                vec![Target::Sink(id.clone())]
            } else {
                Vec::new()
            }
        }
        SourceChoice::App { id, .. } => apps
            .iter()
            .find(|app| app.id == *id)
            .map(|app| app.targets.iter().cloned().map(Target::Node).collect())
            .unwrap_or_default(),
    }
}

/// Whether the plan changed since the source opened: compared as a set, so
/// the default moving first is not a change, and a stream node replaced by
/// another of the same app is.
fn targets_changed(open: &[Target], listed: &[Target]) -> bool {
    let open = open.iter().collect::<BTreeSet<_>>();
    let listed = listed.iter().collect::<BTreeSet<_>>();
    open != listed
}

/// The targets to record now, or `None` when the sinks cannot be read and
/// the choice needs them (an unreadable sound server never reopens). An app
/// choice reads only the PipeWire graph, never the sound server's sinks.
fn listed_targets(choice: &SourceChoice) -> Option<Vec<Target>> {
    match choice {
        SourceChoice::App { .. } => app_targets(
            choice,
            crate::microphone_activity::playing_applications().as_deref(),
        ),
        SourceChoice::All | SourceChoice::Output { .. } => {
            let sinks = crate::audio::output_route::meeting_sinks()?;
            Some(plan_targets(choice, &sinks, &[]))
        }
    }
}

/// The targets an app choice records, or `None` when the PipeWire graph
/// cannot be read. An unreadable graph must keep the captures already
/// running rather than look like every stream of the app stopped.
fn app_targets(choice: &SourceChoice, apps: Option<&[PlayingApplication]>) -> Option<Vec<Target>> {
    Some(plan_targets(choice, &[], apps?))
}

/// The targets to start capturing: the listed ones no running capture names.
fn targets_to_add(open: &[Option<Target>], listed: &[Target]) -> Vec<Target> {
    listed
        .iter()
        .filter(|target| !open.iter().flatten().any(|open| open == *target))
        .cloned()
        .collect()
}

/// The slots to stop: a capture whose target left the listing.
fn slots_to_stop(open: &[Option<Target>], listed: &[Target]) -> Vec<usize> {
    open.iter()
        .enumerate()
        .filter(|(_, target)| {
            target
                .as_ref()
                .is_some_and(|target| !listed.contains(target))
        })
        .map(|(index, _)| index)
        .collect()
}

/// Every captured output's `pw-record`, mixed into one 16 kHz mono `f32`
/// system track.
struct MonitorSource {
    /// One capture per target, in capture order.
    outputs: Vec<Output>,
    /// What it records, read from the lab as it opened.
    choice: SourceChoice,
    /// The plan now, empty when one untargeted `pw-record` follows the default
    /// output. Another plan reopens the source, except for an app, whose
    /// streams are followed in place.
    targets: Vec<Target>,
    /// The program every capture starts, kept to start one again.
    executable: PathBuf,
    /// The capture events' sender, kept so a restarted capture can send.
    sender: SyncSender<MonitorEvent>,
    buffers: Receiver<MonitorEvent>,
    /// When a capture last delivered audio, or a fresh capture started.
    stall: StallWatch,
    route_checked: Instant,
    /// The last capture's exit, so the all-closed error can name it.
    last_exit: Option<String>,
}

/// When the system audio last proved it is alive. A poll that times out is a
/// stall only while the chosen source is present: an absent source records
/// synthesized silence on purpose, and a capture that just started (because
/// the chosen app began playing after a long silence) gets
/// [`STALL_TIMEOUT`] to deliver its first audio rather than being killed
/// and reopened immediately.
#[derive(Clone, Copy, Debug)]
struct StallWatch {
    last_audio: Instant,
}

impl StallWatch {
    fn new(now: Instant) -> Self {
        Self { last_audio: now }
    }

    /// A capture started or delivered audio at `now`, so waiting for its
    /// first packet is not a stall. Never moves back, so a route check's
    /// stamp cannot undo a later packet.
    fn heard(&mut self, now: Instant) {
        self.last_audio = self.last_audio.max(now);
    }

    /// Whether a poll at `now` that carried no audio is a stall. A source
    /// that isn't present records silence by design and never stalls.
    fn stalled(&self, silent: bool, now: Instant) -> bool {
        !silent && now.saturating_duration_since(self.last_audio) > STALL_TIMEOUT
    }
}

/// One captured output: its `pw-record`, what it hears, and its audio not yet
/// mixed.
struct Output {
    /// The target it captures, or `None` when it follows the default output.
    target: Option<Target>,
    /// `None` once this capture ended; the others keep the track going.
    child: Option<Child>,
    /// Which capture this slot runs now. A slot reused by a newer capture
    /// ignores its predecessor's reader, whose exit must not kill the child
    /// that took its place, and whose buffered audio belongs to the stream
    /// that went away.
    generation: u64,
    /// When a capture that ended starts again.
    restart: Restart,
    pending: Vec<f32>,
}

impl Output {
    /// Whether `generation` names the capture this slot runs now.
    fn current(&self, generation: u64) -> bool {
        belongs_to_current(self.child.is_some().then_some(self.generation), generation)
    }
}

/// Whether an event from capture `generation` belongs to the slot, whose
/// running capture is `current` (a generation while a child runs, `None`
/// once it ended). A replaced capture's late packets, and its exit, are
/// ignored, so nothing from the old stream reaches the one that took its
/// place.
fn belongs_to_current(current: Option<u64>, generation: u64) -> bool {
    current == Some(generation)
}

/// A fresh generation per spawned capture, so an event always names the child
/// that sent it.
fn next_generation() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// What one capture sends: its 16 kHz audio as `slot` into `outputs`, tagged
/// with its own generation, or that it ended.
enum MonitorEvent {
    Audio {
        slot: usize,
        generation: u64,
        samples: Vec<f32>,
    },
    Closed {
        slot: usize,
        generation: u64,
    },
}

/// The outputs to start again: a capture that ended and is due, whose target
/// is still in the listed plan. A listing Hex could not read (`None`) keeps
/// every such output; the untargeted fallback never restarts here, since the
/// source reopens for it instead.
fn restart_due(outputs: &[Output], listed: Option<&[Target]>, now: Instant) -> Vec<usize> {
    outputs
        .iter()
        .enumerate()
        .filter(|(_, output)| {
            output.child.is_none()
                && output.restart.is_due(now)
                && output
                    .target
                    .as_ref()
                    .is_some_and(|target| listed.is_none_or(|listed| listed.contains(target)))
        })
        .map(|(index, _)| index)
        .collect()
}

impl MonitorSource {
    fn start(choice: SourceChoice) -> Result<Self> {
        let executable =
            find_executable(PW_RECORD).ok_or_else(|| eyre!("{PW_RECORD} is not installed"))?;
        let targets = listed_targets(&choice).unwrap_or_else(|| {
            tracing::debug!(
                "could not read the sound server's sinks; following the default output"
            );
            Vec::new()
        });
        let silent = !choice.is_all() && targets.is_empty();
        // One capture per target, or one that follows the default output when
        // all computer audio is chosen and the sound server cannot be read.
        let captures: Vec<Option<Target>> = if choice.is_all() && targets.is_empty() {
            vec![None]
        } else {
            targets.iter().cloned().map(Some).collect()
        };
        // An app's streams start and stop after this source opens, and a sync
        // channel's capacity cannot grow, so an app choice opens with the
        // most this source ever buffers; every other choice captures what it
        // opened with.
        let queue = if matches!(choice, SourceChoice::App { .. }) {
            QUEUE_CAPACITY
        } else {
            (QUEUE_PER_CAPTURE * captures.len().max(1)).min(QUEUE_CAPACITY)
        };
        let (sender, buffers) = mpsc::sync_channel(queue);
        let mut outputs = Vec::with_capacity(captures.len());
        let mut first_error = None;
        for target in captures {
            let generation = next_generation();
            match spawn_monitor(
                &executable,
                target.as_ref(),
                outputs.len(),
                generation,
                sender.clone(),
            ) {
                Ok(child) => outputs.push(Output {
                    target,
                    child: Some(child),
                    generation,
                    restart: Restart::running(Instant::now()),
                    pending: Vec::new(),
                }),
                Err(error) => {
                    tracing::warn!(
                        target = target
                            .as_ref()
                            .map_or("the default output", Target::label),
                        error = %format!("{error:#}"),
                        "could not hear an audio output"
                    );
                    first_error.get_or_insert(error);
                }
            }
        }
        if outputs.is_empty() && !silent {
            return Err(
                first_error.unwrap_or_else(|| eyre!("no audio output is available to capture"))
            );
        }
        if silent {
            tracing::info!(
                ?choice,
                "the chosen meeting audio source isn't playing; recording silence until it returns"
            );
        } else if choice.is_all() && targets.is_empty() {
            tracing::info!("system audio monitor follows the default output");
        } else {
            tracing::info!(?choice, ?targets, "system audio monitors started");
        }
        Ok(Self {
            outputs,
            choice,
            targets,
            executable,
            sender,
            buffers,
            stall: StallWatch::new(Instant::now()),
            route_checked: Instant::now(),
            last_exit: None,
        })
    }

    /// Whether the chosen source isn't present, so the track records silence
    /// until it appears. It is judged by what the choice records, not by the
    /// captures left in their slots.
    fn silent(&self) -> bool {
        !self.choice.is_all() && self.targets.is_empty()
    }

    /// Applies one event from a capture. An event whose generation is no
    /// longer the slot's running capture is stale: its audio was recorded
    /// from a stream that went away, and its exit must not touch the child
    /// that replaced it.
    fn take(&mut self, event: MonitorEvent) {
        match event {
            MonitorEvent::Audio {
                slot,
                generation,
                samples,
            } => {
                let Some(output) = self.outputs.get_mut(slot) else {
                    return;
                };
                if !output.current(generation) {
                    return;
                }
                output.pending.extend(samples);
                self.stall.heard(Instant::now());
            }
            MonitorEvent::Closed { slot, generation } => self.close(slot, generation),
        }
    }

    /// Drops one capture that ended so the others keep recording and its
    /// unpicked audio never replays later, and schedules it to start again.
    fn close(&mut self, slot: usize, generation: u64) {
        let now = Instant::now();
        let reason = {
            let Some(output) = self.outputs.get_mut(slot) else {
                return;
            };
            if !output.current(generation) {
                return;
            }
            let Some(mut child) = output.child.take() else {
                return;
            };
            let status = child.try_wait().ok().flatten();
            let _ = child.kill();
            let _ = child.wait();
            output.pending.clear();
            output.restart.exited(now);
            let target = output
                .target
                .as_ref()
                .map_or("the default output", Target::label);
            match status {
                Some(status) => format!("{target}: pw-record exited ({status})"),
                None => format!("{target}: pw-record closed its output"),
            }
        };
        tracing::warn!(%reason, "system audio monitor stopped");
        self.last_exit = Some(reason);
    }

    /// Starts again every capture whose target is still listed, at its own
    /// index so its audio keeps its place in the mix. A failed start waits for
    /// the next due time, still bounded by the shared retry cap.
    fn restart_exited(&mut self, listed: Option<&[Target]>) {
        let now = Instant::now();
        for index in restart_due(&self.outputs, listed, now) {
            let target = self.outputs[index].target.clone();
            let generation = next_generation();
            let spawned = spawn_monitor(
                &self.executable,
                target.as_ref(),
                index,
                generation,
                self.sender.clone(),
            );
            let output = &mut self.outputs[index];
            match spawned {
                Ok(child) => {
                    output.child = Some(child);
                    output.generation = generation;
                    output.restart.started(now);
                    self.stall.heard(now);
                    tracing::info!(
                        target = target.as_ref().map_or("the default output", Target::label),
                        "system audio monitor restarted"
                    );
                }
                Err(error) => {
                    output.restart.exited(now);
                    tracing::warn!(
                        target = target.as_ref().map_or("the default output", Target::label),
                        error = %format!("{error:#}"),
                        "could not restart a system audio monitor; trying again"
                    );
                }
            }
        }
    }

    /// Why no capture is left, for the source's error.
    fn stopped(&self) -> String {
        self.last_exit
            .clone()
            .unwrap_or_else(|| "every system audio monitor stopped".into())
    }

    /// Applies one route check: a changed choice reopens the source, an app's
    /// streams are followed in place, and another set of sinks reopens it as
    /// it always has.
    fn route_check(&mut self) -> std::result::Result<(), String> {
        // Another choice, or a turned-off lab, reopens into it: a choice
        // applies mid-recording, not from the next stream.
        if effective() != self.choice {
            return Err("the meeting audio choice changed".into());
        }
        match listed_targets(&self.choice) {
            Some(listed) => {
                if matches!(self.choice, SourceChoice::App { .. }) {
                    self.reconcile(&listed);
                } else if targets_changed(&self.targets, &listed) {
                    return Err("the audio outputs changed".into());
                }
                self.targets = listed.clone();
                self.restart_exited(Some(&listed));
            }
            None => self.restart_exited(None),
        }
        Ok(())
    }

    /// Follows the chosen app's running output streams: a capture for a node
    /// that appeared starts, one whose node went away stops, and every other
    /// keeps recording, so a stream opened mid-call is heard without the gap a
    /// reopened source would leave.
    fn reconcile(&mut self, listed: &[Target]) {
        let now = Instant::now();
        let open = self
            .outputs
            .iter()
            .map(|output| output.target.clone())
            .collect::<Vec<_>>();
        for index in slots_to_stop(&open, listed) {
            let output = &mut self.outputs[index];
            output.target = None;
            if let Some(mut child) = output.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            output.pending.clear();
            tracing::info!("a system audio monitor's stream stopped; no longer recording it");
        }
        for target in targets_to_add(&open, listed) {
            self.start_target(&target, now);
        }
    }

    /// Starts one capture for `target`, in a stopped slot where there is one
    /// so the mix keeps its order, or a new slot. A spawn that fails keeps the
    /// target and a due time, so it is retried at its own bounded backoff.
    fn start_target(&mut self, target: &Target, now: Instant) {
        let slot = self
            .outputs
            .iter()
            .position(|output| output.target.is_none() && output.child.is_none());
        let index = slot.unwrap_or(self.outputs.len());
        let mut restart = Restart::running(now);
        // A slot this app's stream just left gets a fresh generation, so its
        // old reader's late packets and exit are ignored here.
        let generation = next_generation();
        match spawn_monitor(
            &self.executable,
            Some(target),
            index,
            generation,
            self.sender.clone(),
        ) {
            Ok(child) => {
                let output = Output {
                    target: Some(target.clone()),
                    child: Some(child),
                    generation,
                    restart,
                    pending: Vec::new(),
                };
                match slot {
                    Some(slot) => self.outputs[slot] = output,
                    None => self.outputs.push(output),
                }
                // The capture just opened, so the silence before it (the
                // chosen app was not playing) is not a stall of this capture.
                self.stall.heard(now);
                tracing::info!(target = target.label(), "system audio monitor started");
            }
            Err(error) => {
                restart.exited(now);
                let output = Output {
                    target: Some(target.clone()),
                    child: None,
                    generation,
                    restart,
                    pending: Vec::new(),
                };
                match slot {
                    Some(slot) => self.outputs[slot] = output,
                    None => self.outputs.push(output),
                }
                tracing::warn!(
                    target = target.label(),
                    error = %format!("{error:#}"),
                    "could not hear the chosen app's audio stream; trying again"
                );
            }
        }
    }
}

/// The stream properties that capture `target`'s monitor or stream node as
/// capture `index`, or the default output's monitor when `target` is `None`.
fn properties(target: Option<&Target>, index: usize) -> String {
    let node = match target {
        // One node per target, so the graph lists each capture separately.
        Some(_) => format!("{NODE_NAME}-{index}"),
        None => NODE_NAME.to_owned(),
    };
    // A monitor asks for the sink's monitor ports; a chosen app's running
    // output stream is captured as the node itself.
    let capture_sink = match target {
        Some(Target::Node(_)) => "",
        Some(Target::Sink(_)) | None => " stream.capture.sink = true",
    };
    let mut properties =
        format!("{{{capture_sink} node.name = {node} node.description = \"{NODE_DESCRIPTION}\"");
    if let Some(target) = target {
        // A target that will not link must not be moved to the default
        // output: that would mix the default monitor's audio in a second time.
        properties.push_str(" node.dont-fallback = true node.dont-reconnect = true");
        properties.push_str(&format!(" target.object = \"{}\"", target.label()));
    }
    properties.push_str(" }");
    properties
}

/// Starts one `pw-record` for `target`, or for whatever the session manager
/// links to the default output when `target` is `None`, and the thread that
/// reads its audio as `index`.
fn spawn_monitor(
    executable: &Path,
    target: Option<&Target>,
    index: usize,
    generation: u64,
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
        .spawn(move || read_monitor(stdout, index, generation, sender));
    if let Err(error) = reader {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error).wrap_err("could not start the pw-record reader");
    }
    Ok(child)
}

/// Reads one capture's raw `f32` audio until it ends, tagging it with its
/// `slot` and `generation`.
fn read_monitor(
    mut stdout: ChildStdout,
    slot: usize,
    generation: u64,
    sender: SyncSender<MonitorEvent>,
) {
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
        if sender
            .send(MonitorEvent::Audio {
                slot,
                generation,
                samples,
            })
            .is_err()
        {
            return;
        }
    }
    let _ = sender.send(MonitorEvent::Closed { slot, generation });
}

impl LiveSource for MonitorSource {
    fn poll(&mut self, sink: &PacketSink) -> std::result::Result<(), String> {
        if self.route_checked.elapsed() >= ROUTE_CHECK {
            self.route_checked = Instant::now();
            self.route_check()?;
        }
        match self.buffers.recv_timeout(POLL_INTERVAL) {
            Ok(first) => {
                self.take(first);
                while let Ok(next) = self.buffers.try_recv() {
                    self.take(next);
                }
                // An app's capture that ended starts again in its own slot, so
                // only another choice or a lost channel fails this source.
                if !matches!(self.choice, SourceChoice::App { .. })
                    && self.outputs.iter().all(|output| output.child.is_none())
                {
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
            // The chosen source isn't present: record silence so the track
            // keeps its place, and keep checking for it every 2 s. It is
            // synthesized, not stream silence, so it never arms the
            // digital-silence watch.
            Err(mpsc::RecvTimeoutError::Timeout) if self.silent() => {
                sink.send_synthesized_silence(SAMPLE_RATE, vec![0.0; SILENT_PACKET]);
                Ok(())
            }
            Err(mpsc::RecvTimeoutError::Timeout)
                if self.stall.stalled(self.silent(), Instant::now()) =>
            {
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
    use crate::meeting::capture::{RETRY_INITIAL, RETRY_MAX};

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
        let sink = Target::Sink("bluez_output.AA_BB_CC_DD_EE_FF.1".into());
        assert_eq!(
            properties(Some(&sink), 2),
            "{ stream.capture.sink = true node.name = hex-meeting-system-audio-2 \
             node.description = \"Hex meeting system audio\" \
             node.dont-fallback = true node.dont-reconnect = true \
             target.object = \"bluez_output.AA_BB_CC_DD_EE_FF.1\" }"
        );
        // A chosen app's running output stream is the node itself, not a
        // sink's monitor, and keeps the same no-fallback guarantees.
        let node = Target::Node("118".into());
        assert_eq!(
            properties(Some(&node), 3),
            "{ node.name = hex-meeting-system-audio-3 \
             node.description = \"Hex meeting system audio\" \
             node.dont-fallback = true node.dont-reconnect = true \
             target.object = \"118\" }"
        );
    }

    /// A chosen app that starts playing after a long silence starts a fresh
    /// capture; the silence before it must not make the first poll kill and
    /// reopen that capture, and a source that is merely absent never stalls.
    #[test]
    fn a_capture_that_starts_after_silence_is_not_a_stall() {
        let opened = Instant::now();
        let mut stall = StallWatch::new(opened);
        assert!(
            !stall.stalled(true, opened + Duration::from_secs(30)),
            "an absent source records silence by design"
        );
        let started = opened + Duration::from_secs(30);
        stall.heard(started);
        assert!(!stall.stalled(false, started));
        assert!(
            !stall.stalled(false, started + STALL_TIMEOUT),
            "a fresh capture gets the whole timeout to deliver audio"
        );
        assert!(
            stall.stalled(false, started + STALL_TIMEOUT + Duration::from_millis(1)),
            "a present source that then delivers nothing is still a stall"
        );
        // Audio keeps the watch fresh.
        let later = started + Duration::from_secs(1);
        stall.heard(later);
        assert!(!stall.stalled(false, later + STALL_TIMEOUT));
    }

    /// A PipeWire graph Hex could not read is not an app that stopped
    /// playing: the source keeps the captures it has instead of stopping
    /// them all.
    #[test]
    fn an_unreadable_graph_keeps_the_apps_captures() {
        let chrome = SourceChoice::App {
            id: "chrome".into(),
            name: "Google Chrome".into(),
        };
        assert_eq!(app_targets(&chrome, None), None);
        assert_eq!(
            app_targets(&chrome, Some(&[])),
            Some(Vec::new()),
            "a readable graph that lists no stream stops the capture"
        );
        let apps = [app("chrome", "Google Chrome", &["41"])];
        assert_eq!(
            app_targets(&chrome, Some(&apps)),
            Some(vec![Target::Node("41".into())])
        );
    }

    #[test]
    fn a_reordered_sink_list_is_not_a_change() {
        // The default moving first is the same set of outputs; a sink added or
        // gone is a change the source reopens for.
        let targets = |names: &[&str]| {
            names
                .iter()
                .map(|name| Target::Sink((*name).into()))
                .collect::<Vec<_>>()
        };
        assert!(!targets_changed(
            &targets(&["speakers", "headset"]),
            &targets(&["headset", "speakers"]),
        ));
        assert!(targets_changed(
            &targets(&["speakers", "headset"]),
            &targets(&["speakers"]),
        ));
        assert!(targets_changed(
            &targets(&["speakers", "headset"]),
            &targets(&["speakers", "headset", "dock"]),
        ));
        assert!(!targets_changed(&[], &[]), "the fallback stays put");
        assert!(targets_changed(&[], &targets(&["speakers"])));
    }

    fn app(id: &str, name: &str, targets: &[&str]) -> PlayingApplication {
        PlayingApplication {
            id: id.into(),
            name: name.into(),
            targets: targets.iter().map(|target| (*target).into()).collect(),
        }
    }

    /// The chosen source decides exactly what records: one output's monitor,
    /// one app's running streams, or (for all) every listed sink. A source
    /// that isn't present records nothing rather than falling back to every
    /// output.
    #[test]
    fn a_choice_records_only_its_own_source() {
        let sinks = ["speakers".to_owned(), "headset".to_owned()];
        let apps = [
            app("spotify", "Spotify", &["41"]),
            app("chrome", "Google Chrome", &["42", "43"]),
        ];
        let all = plan_targets(&SourceChoice::All, &sinks, &apps);
        assert_eq!(
            all,
            [
                Target::Sink("speakers".into()),
                Target::Sink("headset".into())
            ]
        );
        // An output records only while it is listed; gone, it records
        // nothing and the source keeps looking.
        let headset = SourceChoice::Output {
            id: "headset".into(),
            name: "Headset".into(),
        };
        assert_eq!(
            plan_targets(&headset, &sinks, &apps),
            [Target::Sink("headset".into())]
        );
        let dock = SourceChoice::Output {
            id: "dock".into(),
            name: "Dock".into(),
        };
        assert!(plan_targets(&dock, &sinks, &apps).is_empty());
        // An app records every running output stream it has, and nothing
        // once it stops playing.
        let chrome = SourceChoice::App {
            id: "chrome".into(),
            name: "Google Chrome".into(),
        };
        assert_eq!(
            plan_targets(&chrome, &sinks, &apps),
            [Target::Node("42".into()), Target::Node("43".into())]
        );
        let slack = SourceChoice::App {
            id: "slack".into(),
            name: "Slack".into(),
        };
        assert!(plan_targets(&slack, &sinks, &apps).is_empty());
    }

    /// An app opens and closes output streams as it plays, so its captures
    /// follow them in place: only a node that appeared starts, only one that
    /// went away stops, and every capture that stayed keeps recording.
    #[test]
    fn an_apps_streams_are_added_and_removed_without_reopening_the_source() {
        let node = |serial: &str| Some(Target::Node(serial.into()));
        let open = [node("41"), node("42"), None];
        let listed = [Target::Node("42".into()), Target::Node("43".into())];
        assert_eq!(slots_to_stop(&open, &listed), [0]);
        assert_eq!(targets_to_add(&open, &listed), [Target::Node("43".into())]);
        // Nothing to change is no change at all.
        assert!(
            slots_to_stop(
                &open,
                &[Target::Node("41".into()), Target::Node("42".into())]
            )
            .is_empty()
        );
        assert!(
            targets_to_add(
                &open,
                &[Target::Node("41".into()), Target::Node("42".into())]
            )
            .is_empty()
        );
        // The app stopped playing: every capture stops and none starts.
        assert_eq!(slots_to_stop(&open, &[]), [0, 1]);
        assert!(targets_to_add(&open, &[]).is_empty());
        // It started again: a new capture starts.
        assert_eq!(
            targets_to_add(&[None, None], &[Target::Node("7".into())]),
            [Target::Node("7".into())]
        );
    }

    #[test]
    fn an_exited_monitor_is_restarted_while_its_sink_keeps_its_name() {
        let now = Instant::now();
        let due = now + RETRY_INITIAL;
        // A Bluetooth card changing profile ends that sink's pw-record while
        // the sink comes back under the same name, so the set is unchanged and
        // the source stays open; only that capture starts again.
        let targets = |names: &[&str]| {
            names
                .iter()
                .map(|name| Target::Sink((*name).into()))
                .collect::<Vec<_>>()
        };
        assert!(!targets_changed(
            &targets(&["speakers", "headset"]),
            &targets(&["speakers", "headset"]),
        ));
        let exited = |at| {
            let mut restart = Restart::running(now);
            restart.exited(at);
            restart
        };
        let output = |target: Option<Target>, restart| Output {
            target,
            child: None,
            generation: 1,
            restart,
            pending: Vec::new(),
        };
        let sink = |name: &str| Some(Target::Sink(name.into()));
        let outputs = [
            // Ended, due, and still listed: the one that starts again.
            output(sink("speakers"), exited(now)),
            // Ended and due, but gone from the listed sinks.
            output(sink("headset"), exited(now)),
            // Ended, but not due yet.
            output(sink("dock"), exited(due)),
            // The fallback output never restarts here.
            output(None, exited(now)),
            // Still running.
            output(sink("speakers"), Restart::running(now)),
        ];
        let listed = targets(&["speakers", "dock"]);
        assert_eq!(restart_due(&outputs, Some(&listed), due), [0]);
        // A listing Hex could not read keeps every output that is due by now;
        // the fallback still never restarts.
        let later = due + RETRY_INITIAL;
        assert_eq!(restart_due(&outputs, None, later), [0, 1, 2]);
    }

    /// A route check stops a capture whose stream went away and starts the
    /// stream that replaced it in the same slot. The old reader's late
    /// packets, and its exit, must not reach the new stream: it would mix the
    /// stream that went away into it, or kill the child that replaced it.
    #[test]
    fn an_event_from_a_replaced_capture_never_reaches_its_slot() {
        assert!(belongs_to_current(Some(7), 7));
        assert!(
            !belongs_to_current(Some(8), 7),
            "the slot runs a newer capture"
        );
        assert!(
            !belongs_to_current(None, 7),
            "the capture ended and the slot holds no child"
        );

        let (sender, buffers) = mpsc::sync_channel(4);
        let mut source = MonitorSource {
            outputs: vec![Output {
                target: Some(Target::Node("7".into())),
                child: None,
                generation: 8,
                restart: Restart::running(Instant::now()),
                pending: Vec::new(),
            }],
            choice: SourceChoice::App {
                id: "chrome".into(),
                name: "Chrome".into(),
            },
            targets: vec![Target::Node("7".into())],
            executable: PathBuf::from(PW_RECORD),
            sender,
            buffers,
            stall: StallWatch::new(Instant::now()),
            route_checked: Instant::now(),
            last_exit: None,
        };
        source.take(MonitorEvent::Audio {
            slot: 0,
            generation: 7,
            samples: vec![0.5],
        });
        assert!(
            source.outputs[0].pending.is_empty(),
            "the replaced capture's buffered audio is not mixed into the new stream"
        );
        source.take(MonitorEvent::Closed {
            slot: 0,
            generation: 7,
        });
        assert!(
            source.last_exit.is_none(),
            "the replaced capture's exit is not the new capture's"
        );
    }

    #[test]
    fn a_monitor_that_ends_right_after_each_restart_keeps_backing_off() {
        let mut now = Instant::now();
        let mut restart = Restart::running(now);
        let mut waits = Vec::new();
        for _ in 0..6 {
            restart.exited(now);
            let due = restart.due_at().expect("an ended capture is due again");
            assert!(!restart.is_due(now), "not due the moment it ended");
            assert!(restart.is_due(due));
            waits.push(due.saturating_duration_since(now));
            now = due;
            restart.started(now);
        }
        assert_eq!(
            waits,
            [
                RETRY_INITIAL,
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
                RETRY_MAX,
                RETRY_MAX,
            ]
        );
        // A capture that ran for at least a second starts the wait over.
        restart.exited(now + Duration::from_secs(1));
        assert_eq!(
            restart.due_at().expect("due again"),
            now + Duration::from_secs(1) + RETRY_INITIAL
        );
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

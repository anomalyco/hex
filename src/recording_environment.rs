use std::ffi::c_void;
use std::mem::size_of;
use std::path::Path;
use std::process::{Child, Command};
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectPropertyAddress, AudioObjectSetPropertyData,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
};

use crate::app_settings::{self, RecordingAudioBehavior, RecordingVolumeReduction};

const VIRTUAL_MAIN_VOLUME: u32 = u32::from_be_bytes(*b"vmvc");
/// Ignore Core Audio rounding while still detecting a single volume-key step.
const MANUAL_VOLUME_THRESHOLD: f32 = 0.025;
/// Playback already at or below the reduced level (within this headroom) is
/// left alone rather than nudged and later "restored".
const REDUCTION_HEADROOM: f32 = 0.005;
/// Interval between volume writes while fading; short enough to sound smooth,
/// long enough that a cancelled fade stops promptly.
const VOLUME_RAMP_STEP: Duration = Duration::from_millis(25);
const PAUSE_MUSIC: &str = r#"
try
  if application "Music" is running then
    tell application "Music"
      if player state is playing then
        pause
        set end of pausedPlayers to "Music"
      end if
    end tell
  end if
end try
"#;
const PAUSE_SPOTIFY: &str = r#"
try
  if application "Spotify" is running then
    tell application "Spotify"
      if player state is playing then
        pause
        set end of pausedPlayers to "Spotify"
      end if
    end tell
  end if
end try
"#;
const PAUSE_VLC: &str = r#"
try
  if application "VLC" is running then
    tell application "VLC"
      if playing then
        pause
        set end of pausedPlayers to "VLC"
      end if
    end tell
  end if
end try
"#;

struct RecordingEnvironment {
    _sleep: Option<PreventSleep>,
    _audio: AudioBehaviorGuard,
}

impl RecordingEnvironment {
    pub fn start() -> Self {
        Self {
            _sleep: prevent_sleep(),
            _audio: AudioBehaviorGuard::start(app_settings::recording_audio_behavior()),
        }
    }
}

enum EnvironmentCommand {
    Start,
    Stop,
    #[cfg(test)]
    Barrier(Sender<()>),
}

#[derive(Clone)]
pub struct RecordingEnvironmentController {
    commands: Sender<EnvironmentCommand>,
}

impl RecordingEnvironmentController {
    pub fn start() -> Self {
        Self::with_environment(RecordingEnvironment::start)
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self::with_environment(|| ())
    }

    fn with_environment<E>(start: impl Fn() -> E + Send + 'static) -> Self {
        let (commands, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut sessions = 0_u32;
            let mut _environment = None;
            while let Ok(command) = receiver.recv() {
                match command {
                    EnvironmentCommand::Start => {
                        sessions = sessions.saturating_add(1);
                        if sessions == 1 {
                            _environment = Some(start());
                        }
                    }
                    EnvironmentCommand::Stop => {
                        sessions = sessions.saturating_sub(1);
                        if sessions == 0 {
                            _environment = None;
                        }
                    }
                    #[cfg(test)]
                    EnvironmentCommand::Barrier(reply) => {
                        let _ = reply.send(());
                    }
                }
            }
        });
        Self { commands }
    }

    pub fn begin(&self) -> RecordingEnvironmentSession {
        let _ = self.commands.send(EnvironmentCommand::Start);
        RecordingEnvironmentSession {
            commands: self.commands.clone(),
        }
    }
}

pub struct RecordingEnvironmentSession {
    commands: Sender<EnvironmentCommand>,
}

impl Drop for RecordingEnvironmentSession {
    fn drop(&mut self) {
        let _ = self.commands.send(EnvironmentCommand::Stop);
    }
}

pub fn prevent_sleep() -> Option<PreventSleep> {
    match PreventSleep::start() {
        Ok(prevention) => Some(prevention),
        Err(error) => {
            tracing::warn!(%error, "could not prevent idle system sleep");
            None
        }
    }
}

pub struct PreventSleep {
    process: Child,
}

impl PreventSleep {
    fn start() -> std::io::Result<Self> {
        Ok(Self {
            process: Command::new("/usr/bin/caffeinate")
                .args(["-i", "-w"])
                .arg(std::process::id().to_string())
                .spawn()?,
        })
    }
}

impl Drop for PreventSleep {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

enum AudioBehaviorGuard {
    Muted { device: u32, previous: f32 },
    Reduced(VolumeReduction<CoreAudioOutput>),
    Paused { players: Vec<String> },
    None,
}

impl AudioBehaviorGuard {
    fn start(behavior: RecordingAudioBehavior) -> Self {
        match behavior {
            RecordingAudioBehavior::Mute => {
                mute_output().map_or(Self::None, |(device, previous)| {
                    tracing::info!(previous, "muted system output for dictation");
                    Self::Muted { device, previous }
                })
            }
            RecordingAudioBehavior::ReduceVolume => {
                reduce_output(app_settings::recording_volume_reduction())
                    .map_or(Self::None, Self::Reduced)
            }
            RecordingAudioBehavior::PauseMedia => {
                let players = pause_media();
                if players.is_empty() {
                    Self::None
                } else {
                    tracing::info!(?players, "paused media for dictation");
                    Self::Paused { players }
                }
            }
            RecordingAudioBehavior::DoNothing => Self::None,
        }
    }
}

impl Drop for AudioBehaviorGuard {
    fn drop(&mut self) {
        match self {
            Self::Muted { device, previous } => {
                if output_volume(*device).is_some_and(|volume| volume <= 0.001)
                    && set_output_volume(*device, *previous)
                {
                    tracing::info!(volume = *previous, "restored system output after dictation");
                }
            }
            // `VolumeReduction` restores the output in its own `Drop`.
            Self::Reduced(reduction) => {
                tracing::debug!(previous = reduction.previous, "ending output reduction")
            }
            Self::Paused { players } => resume_media(players),
            Self::None => {}
        }
    }
}

fn mute_output() -> Option<(u32, f32)> {
    let device = default_output_device()?;
    let previous = output_volume(device)?;
    set_output_volume(device, 0.0).then_some((device, previous))
}

/// The output whose volume a reduction owns. Core Audio in the app; a scripted
/// fake in tests so ramp, takeover, and restore ordering can be checked without
/// a device.
trait OutputVolume: Clone + Send + 'static {
    fn read(&self) -> Option<f32>;
    fn write(&self, volume: f32) -> bool;
}

#[derive(Clone, Copy)]
struct CoreAudioOutput {
    device: u32,
}

impl OutputVolume for CoreAudioOutput {
    fn read(&self) -> Option<f32> {
        output_volume(self.device)
    }

    fn write(&self, volume: f32) -> bool {
        set_output_volume(self.device, volume)
    }
}

/// Lowers the default output to the configured level for the duration of a
/// recording environment, optionally fading in each direction.
///
/// The previous volume is captured once, when the environment starts, and the
/// controller only starts one environment for overlapping sessions, so a
/// double-tap lock that overlaps a release grace period restores the original
/// level rather than the already-reduced one (kitlangton/Hex#220). A manual
/// volume change while reduced, or during either fade, is treated as the user
/// taking over: HEX stops writing and restores nothing afterward.
struct VolumeReduction<O: OutputVolume> {
    output: O,
    previous: f32,
    fade_in: Duration,
    shared: Arc<ReductionShared>,
    fade_out: Option<JoinHandle<()>>,
}

struct ReductionShared {
    cancelled: AtomicBool,
    released: AtomicBool,
    last_applied: AtomicU32,
}

impl ReductionShared {
    fn new(volume: f32) -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            released: AtomicBool::new(false),
            last_applied: AtomicU32::new(volume.to_bits()),
        }
    }

    fn last_applied(&self) -> f32 {
        f32::from_bits(self.last_applied.load(Ordering::Acquire))
    }

    fn record_applied(&self, volume: f32) {
        self.last_applied.store(volume.to_bits(), Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RampOutcome {
    Completed,
    Cancelled,
    /// The user changed the volume mid-ramp; HEX stops touching the output.
    Released,
    Failed,
}

fn reduce_output(settings: RecordingVolumeReduction) -> Option<VolumeReduction<CoreAudioOutput>> {
    let device = default_output_device()?;
    reduce_output_of(CoreAudioOutput { device }, settings)
}

fn reduce_output_of<O: OutputVolume>(
    output: O,
    settings: RecordingVolumeReduction,
) -> Option<VolumeReduction<O>> {
    let previous = output.read()?;
    let target = settings.volume;
    if !needs_reduction(previous, target) {
        tracing::info!(
            volume = previous,
            target,
            "kept system output for dictation; already at or below the reduced level"
        );
        return None;
    }
    let shared = Arc::new(ReductionShared::new(previous));
    let fade_out = Duration::from_secs_f32(settings.fade_out_seconds);
    let ramp = volume_ramp(previous, target, fade_out);
    let fade_out = if ramp.len() == 1 {
        match run_ramp(&output, ramp, &shared, RampGuard::CancelOrManualChange) {
            RampOutcome::Completed => {}
            RampOutcome::Failed => {
                tracing::warn!("could not reduce system output for dictation");
                return None;
            }
            RampOutcome::Cancelled | RampOutcome::Released => return None,
        }
        None
    } else {
        let worker = Arc::clone(&shared);
        let fading = output.clone();
        Some(thread::spawn(move || {
            match run_ramp(&fading, ramp, &worker, RampGuard::CancelOrManualChange) {
                RampOutcome::Completed | RampOutcome::Cancelled => {}
                RampOutcome::Released => {
                    tracing::info!("stopped reducing system output after a manual volume change")
                }
                RampOutcome::Failed => {
                    tracing::warn!("could not finish reducing system output for dictation")
                }
            }
        }))
    };
    tracing::info!(
        previous,
        target,
        fade_out_seconds = settings.fade_out_seconds,
        "reduced system output for dictation"
    );
    Some(VolumeReduction {
        output,
        previous,
        fade_in: Duration::from_secs_f32(settings.fade_in_seconds),
        shared,
        fade_out,
    })
}

impl<O: OutputVolume> Drop for VolumeReduction<O> {
    fn drop(&mut self) {
        self.shared.cancelled.store(true, Ordering::Release);
        if let Some(fade_out) = self.fade_out.take() {
            let _ = fade_out.join();
        }
        if self.shared.released.load(Ordering::Acquire) {
            return;
        }
        let Some(current) = self.output.read() else {
            tracing::warn!("could not read system output after dictation; leaving it unchanged");
            return;
        };
        let expected = self.shared.last_applied();
        if is_manual_adjustment(current, expected) {
            tracing::info!(
                current,
                expected,
                "left system output alone after a manual volume change during dictation"
            );
            return;
        }
        let ramp = volume_ramp(current, self.previous, self.fade_in);
        match run_ramp(&self.output, ramp, &self.shared, RampGuard::ManualChange) {
            RampOutcome::Completed => {
                tracing::info!(
                    volume = self.previous,
                    "restored system output after dictation"
                )
            }
            RampOutcome::Released => {
                tracing::info!("stopped restoring system output after a manual volume change")
            }
            RampOutcome::Failed => {
                tracing::warn!("could not restore system output after dictation")
            }
            RampOutcome::Cancelled => {}
        }
    }
}

#[derive(Clone, Copy)]
enum RampGuard {
    /// Stop when the environment ends or the user moves the volume themselves.
    CancelOrManualChange,
    /// Stop only for the user; the environment-end flag is already set while
    /// restoring.
    ManualChange,
}

/// Writes each ramp step, sleeping between steps. Before every write the
/// current level is compared with the last level HEX applied; a difference
/// beyond the manual threshold means the user took over. The level read back
/// after each write becomes the new expected level, so devices that quantize
/// volume to coarse steps do not look like manual changes. A level that cannot
/// be read means HEX cannot prove ownership, so it stops rather than write
/// blind. A user change that lands between a write and its read-back is
/// adopted as HEX's own level; that interval is short but not bounded, and the
/// trade-off is accepted.
fn run_ramp<O: OutputVolume>(
    output: &O,
    ramp: VolumeRamp,
    shared: &ReductionShared,
    guard: RampGuard,
) -> RampOutcome {
    let last = ramp.len().saturating_sub(1);
    for (index, volume) in ramp.enumerate() {
        if matches!(guard, RampGuard::CancelOrManualChange)
            && shared.cancelled.load(Ordering::Acquire)
        {
            return RampOutcome::Cancelled;
        }
        let Some(current) = output.read() else {
            return RampOutcome::Failed;
        };
        if is_manual_adjustment(current, shared.last_applied()) {
            shared.released.store(true, Ordering::Release);
            return RampOutcome::Released;
        }
        if !output.write(volume) {
            return RampOutcome::Failed;
        }
        shared.record_applied(output.read().unwrap_or(volume));
        if index < last {
            thread::sleep(VOLUME_RAMP_STEP);
        }
    }
    RampOutcome::Completed
}

fn needs_reduction(current: f32, target: f32) -> bool {
    current > target + REDUCTION_HEADROOM
}

fn is_manual_adjustment(current: f32, expected: f32) -> bool {
    (current - expected).abs() > MANUAL_VOLUME_THRESHOLD
}

/// Evenly spaced volumes from just after `from` to exactly `to`; a zero
/// duration yields the single target step.
#[derive(Clone, Debug)]
struct VolumeRamp {
    from: f32,
    to: f32,
    steps: usize,
    next: usize,
}

fn volume_ramp(from: f32, to: f32, duration: Duration) -> VolumeRamp {
    let steps = (duration.as_secs_f32() / VOLUME_RAMP_STEP.as_secs_f32()).ceil();
    VolumeRamp {
        from,
        to,
        steps: if steps.is_finite() && steps >= 1.0 {
            steps as usize
        } else {
            1
        },
        next: 1,
    }
}

impl Iterator for VolumeRamp {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.next > self.steps {
            return None;
        }
        let volume = if self.next == self.steps {
            // Land exactly on the target so a restore returns the precise
            // level observed at start rather than a rounded neighbor.
            self.to
        } else {
            let progress = self.next as f32 / self.steps as f32;
            self.from + (self.to - self.from) * progress
        };
        self.next += 1;
        Some(volume.clamp(0.0, 1.0))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.steps.saturating_sub(self.next - 1);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for VolumeRamp {}

fn default_output_device() -> Option<u32> {
    let mut address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut size = size_of::<u32>() as u32;
    let mut device = 0_u32;
    // SAFETY: All pointers reference initialized, correctly sized stack values.
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject as u32,
            NonNull::from(&mut address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut device).cast::<c_void>(),
        )
    };
    (status == 0 && device != 0).then_some(device)
}

fn output_volume(device: u32) -> Option<f32> {
    let mut address = volume_address();
    let mut size = size_of::<f32>() as u32;
    let mut volume = 0.0_f32;
    // SAFETY: All pointers reference initialized, correctly sized stack values.
    let status = unsafe {
        AudioObjectGetPropertyData(
            device,
            NonNull::from(&mut address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut volume).cast::<c_void>(),
        )
    };
    (status == 0).then_some(volume)
}

fn set_output_volume(device: u32, volume: f32) -> bool {
    let mut address = volume_address();
    let mut volume = volume;
    // SAFETY: All pointers reference initialized, correctly sized stack values.
    unsafe {
        AudioObjectSetPropertyData(
            device,
            NonNull::from(&mut address),
            0,
            std::ptr::null(),
            size_of::<f32>() as u32,
            NonNull::from(&mut volume).cast::<c_void>(),
        ) == 0
    }
}

fn volume_address() -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: VIRTUAL_MAIN_VOLUME,
        mScope: kAudioObjectPropertyScopeOutput,
        mElement: kAudioObjectPropertyElementMain,
    }
}

fn pause_media() -> Vec<String> {
    let mut script = format!("set pausedPlayers to {{}}\n{PAUSE_MUSIC}");
    if Path::new("/Applications/Spotify.app").exists() {
        script.push_str(PAUSE_SPOTIFY);
    }
    if Path::new("/Applications/VLC.app").exists() {
        script.push_str(PAUSE_VLC);
    }
    script.push_str("return pausedPlayers\n");
    let output = Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        tracing::warn!(
            error = %String::from_utf8_lossy(&output.stderr).trim(),
            "could not pause media for dictation"
        );
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .split(',')
        .map(str::trim)
        .filter(|player| matches!(*player, "Music" | "Spotify" | "VLC"))
        .map(str::to_string)
        .collect()
}

fn resume_media(players: &[String]) {
    let script = players
        .iter()
        .filter_map(|player| match player.as_str() {
            "Music" => {
                Some("if application \"Music\" is running then tell application \"Music\" to play")
            }
            "Spotify" => Some(
                "if application \"Spotify\" is running then tell application \"Spotify\" to play",
            ),
            "VLC" => {
                Some("if application \"VLC\" is running then tell application \"VLC\" to play")
            }
            _ => None,
        })
        .map(|command| format!("try\n  {command}\nend try"))
        .collect::<Vec<_>>()
        .join("\n");
    if script.is_empty() {
        return;
    }
    match Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .output()
    {
        Ok(output) if output.status.success() => {
            tracing::info!(?players, "resumed media after dictation")
        }
        Ok(output) => tracing::warn!(
            error = %String::from_utf8_lossy(&output.stderr).trim(),
            "could not resume media after dictation"
        ),
        Err(error) => tracing::warn!(%error, "could not resume media after dictation"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{Receiver, RecvTimeoutError};
    use std::time::Duration;

    #[derive(Debug, PartialEq)]
    enum Event {
        Started,
        Restored,
    }

    struct ObservedEnvironment(Sender<Event>);

    impl Drop for ObservedEnvironment {
        fn drop(&mut self) {
            let _ = self.0.send(Event::Restored);
        }
    }

    fn observed_controller() -> (RecordingEnvironmentController, Receiver<Event>) {
        let (events, receiver) = mpsc::channel();
        let controller = RecordingEnvironmentController::with_environment(move || {
            let _ = events.send(Event::Started);
            ObservedEnvironment(events.clone())
        });
        (controller, receiver)
    }

    fn assert_events(
        commands: &Sender<EnvironmentCommand>,
        events: &Receiver<Event>,
        expected: &[Event],
    ) {
        let (reply, response) = mpsc::channel();
        commands.send(EnvironmentCommand::Barrier(reply)).unwrap();
        response.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(events.try_iter().collect::<Vec<_>>(), expected);
    }

    /// Scripted output: a shared level plus optional user changes injected
    /// either right after a given write (before HEX reads it back) or right
    /// before a given read (between ramp steps).
    #[derive(Clone, Default)]
    struct FakeOutput {
        state: Arc<std::sync::Mutex<FakeOutputState>>,
    }

    #[derive(Default)]
    struct FakeOutputState {
        volume: f32,
        writes: Vec<f32>,
        reads: usize,
        change_after_write: Option<(usize, f32)>,
        change_before_read: Option<(usize, f32)>,
        fail_reads_from: Option<usize>,
        write_observer: Option<Sender<usize>>,
    }

    impl FakeOutput {
        fn at(volume: f32) -> Self {
            let output = Self::default();
            output.state.lock().unwrap().volume = volume;
            output
        }

        fn user_changes_after_write(self, write: usize, volume: f32) -> Self {
            self.state.lock().unwrap().change_after_write = Some((write, volume));
            self
        }

        fn user_changes_before_read(self, read: usize, volume: f32) -> Self {
            self.state.lock().unwrap().change_before_read = Some((read, volume));
            self
        }

        fn reads_fail_from(self, read: usize) -> Self {
            self.state.lock().unwrap().fail_reads_from = Some(read);
            self
        }

        /// Reports the running write count after each write, so a test can
        /// wait for a fade-out worker to make progress before acting.
        fn observe_writes(self) -> (Self, Receiver<usize>) {
            let (sender, receiver) = mpsc::channel();
            self.state.lock().unwrap().write_observer = Some(sender);
            (self, receiver)
        }

        fn volume(&self) -> f32 {
            self.state.lock().unwrap().volume
        }

        fn writes(&self) -> Vec<f32> {
            self.state.lock().unwrap().writes.clone()
        }
    }

    impl OutputVolume for FakeOutput {
        fn read(&self) -> Option<f32> {
            let mut state = self.state.lock().unwrap();
            state.reads += 1;
            if let Some((read, volume)) = state.change_before_read
                && state.reads == read
            {
                state.volume = volume;
            }
            if state
                .fail_reads_from
                .is_some_and(|read| state.reads >= read)
            {
                return None;
            }
            Some(state.volume)
        }

        fn write(&self, volume: f32) -> bool {
            let mut state = self.state.lock().unwrap();
            state.volume = volume;
            state.writes.push(volume);
            if let Some((write, volume)) = state.change_after_write
                && state.writes.len() == write
            {
                state.volume = volume;
            }
            if let Some(observer) = &state.write_observer {
                let _ = observer.send(state.writes.len());
            }
            true
        }
    }

    fn reduction(
        volume: f32,
        fade_out_seconds: f32,
        fade_in_seconds: f32,
    ) -> RecordingVolumeReduction {
        RecordingVolumeReduction {
            volume,
            fade_out_seconds,
            fade_in_seconds,
        }
    }

    #[test]
    fn reduction_lowers_the_output_and_dropping_it_restores_exactly() {
        let output = FakeOutput::at(0.8);
        let reduction = reduce_output_of(output.clone(), reduction(0.2, 0.0, 0.0)).unwrap();
        assert_eq!(output.writes(), vec![0.2]);
        drop(reduction);
        assert_eq!(output.writes(), vec![0.2, 0.8]);
        assert_eq!(output.volume(), 0.8);
    }

    #[test]
    fn output_already_at_or_below_the_level_is_left_untouched() {
        let output = FakeOutput::at(0.15);
        assert!(reduce_output_of(output.clone(), reduction(0.2, 0.0, 0.0)).is_none());
        assert!(output.writes().is_empty());
    }

    #[test]
    fn a_manual_change_while_reduced_skips_the_restore() {
        let output = FakeOutput::at(0.8);
        let reduction = reduce_output_of(output.clone(), reduction(0.2, 0.0, 0.0)).unwrap();
        output.state.lock().unwrap().volume = 0.5;
        drop(reduction);
        assert_eq!(output.writes(), vec![0.2]);
        assert_eq!(output.volume(), 0.5);
    }

    #[test]
    fn restore_fade_yields_to_a_manual_change_between_steps() {
        // Reads so far: 1 (start), 2 (pre-write check), 3 (read-back).
        // Restore: 4 (end check), 5 (step 1 pre-write), 6 (read-back),
        // 7 (step 2 pre-write) sees the user's change and stops.
        let output = FakeOutput::at(0.8).user_changes_before_read(7, 0.05);
        let reduction = reduce_output_of(output.clone(), reduction(0.2, 0.0, 0.075)).unwrap();
        let shared = Arc::clone(&reduction.shared);
        drop(reduction);
        let writes = output.writes();
        assert_eq!(writes.len(), 2, "{writes:?}");
        assert_eq!(writes[0], 0.2);
        assert!(writes[1] > 0.2 && writes[1] < 0.8, "{writes:?}");
        assert_eq!(output.volume(), 0.05);
        assert!(shared.released.load(Ordering::Acquire));
    }

    #[test]
    fn dropping_during_a_fade_out_cancels_it_and_restores_from_the_current_level() {
        let (output, writes_seen) = FakeOutput::at(0.8).observe_writes();
        let reduction = reduce_output_of(output.clone(), reduction(0.2, 2.0, 0.0)).unwrap();
        assert!(reduction.fade_out.is_some());
        // Wait for the worker to land its first step so the drop provably
        // interrupts a reduction in progress rather than racing its start.
        assert_eq!(writes_seen.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        drop(reduction);
        let writes = output.writes();
        let (fade_out, restore) = writes.split_at(writes.len() - 1);
        assert!(!fade_out.is_empty() && fade_out.len() < 80, "{writes:?}");
        assert!(
            fade_out.iter().all(|volume| *volume > 0.2 && *volume < 0.8),
            "fade-out steps must be partial: {writes:?}"
        );
        assert_eq!(restore, [0.8]);
        assert_eq!(output.volume(), 0.8);
    }

    #[test]
    fn a_failed_read_before_a_write_stops_the_ramp_instead_of_writing_blind() {
        // Reads: 1 (start), 2 (pre-write), 3 (read-back); the restore end
        // check is read 4 and fails, so nothing more is written.
        let output = FakeOutput::at(0.8).reads_fail_from(4);
        let owned = reduce_output_of(output.clone(), reduction(0.2, 0.0, 0.0)).unwrap();
        drop(owned);
        assert_eq!(output.writes(), vec![0.2]);

        // A read failure before the first write stops the reduction too.
        let output = FakeOutput::at(0.8).reads_fail_from(2);
        assert!(reduce_output_of(output.clone(), reduction(0.2, 0.0, 0.0)).is_none());
        assert!(output.writes().is_empty());
    }

    #[test]
    fn a_change_between_a_write_and_its_read_back_is_adopted_not_released() {
        // Documented, accepted trade-off: HEX treats the read-back as its own level.
        let output = FakeOutput::at(0.8).user_changes_after_write(1, 0.3);
        let reduction = reduce_output_of(output.clone(), reduction(0.2, 0.0, 0.0)).unwrap();
        assert_eq!(reduction.shared.last_applied(), 0.3);
        drop(reduction);
        assert_eq!(output.volume(), 0.8);
    }

    fn assert_volumes(actual: &[f32], expected: &[f32]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?} vs {expected:?}");
        for (actual, expected) in actual.iter().zip(expected) {
            assert!(
                (actual - expected).abs() < 1e-6,
                "{actual} differs from {expected}"
            );
        }
    }

    #[test]
    fn zero_duration_ramp_is_a_single_target_step() {
        assert_eq!(
            volume_ramp(0.8, 0.2, Duration::ZERO).collect::<Vec<_>>(),
            vec![0.2]
        );
    }

    #[test]
    fn timed_ramp_descends_evenly_and_ends_exactly_on_target() {
        let ramp = volume_ramp(1.0, 0.2, Duration::from_millis(100));
        assert_eq!(ramp.len(), 4);
        let volumes = ramp.collect::<Vec<_>>();
        assert_volumes(&volumes, &[0.8, 0.6, 0.4, 0.2]);
        assert_eq!(volumes.last(), Some(&0.2));
        assert!(volumes.windows(2).all(|pair| pair[1] < pair[0]));
    }

    #[test]
    fn partial_step_durations_round_up_and_still_end_on_target() {
        let volumes = volume_ramp(0.35, 0.7, Duration::from_millis(30)).collect::<Vec<_>>();
        assert_eq!(volumes.len(), 2);
        assert_eq!(volumes.last(), Some(&0.7));
    }

    #[test]
    fn restore_ramp_ascends_and_clamps_to_the_device_range() {
        let volumes = volume_ramp(0.2, 1.0, Duration::from_millis(50)).collect::<Vec<_>>();
        assert_volumes(&volumes, &[0.6, 1.0]);
        assert!(volume_ramp(0.0, 1.5, Duration::ZERO).all(|volume| (0.0..=1.0).contains(&volume)));
    }

    #[test]
    fn reduction_skips_playback_already_at_or_below_the_target() {
        assert!(needs_reduction(0.6, 0.2));
        assert!(!needs_reduction(0.2, 0.2));
        assert!(!needs_reduction(0.203, 0.2));
        assert!(needs_reduction(0.21, 0.2));
    }

    #[test]
    fn manual_adjustment_ignores_core_audio_rounding_but_catches_a_key_step() {
        assert!(!is_manual_adjustment(0.2, 0.2));
        assert!(!is_manual_adjustment(0.2001, 0.2));
        assert!(!is_manual_adjustment(0.22, 0.2));
        assert!(is_manual_adjustment(0.2625, 0.2));
        assert!(is_manual_adjustment(0.0, 0.2));
    }

    #[test]
    fn sessions_are_harmless_after_the_environment_worker_disconnects() {
        let (commands, receiver) = mpsc::channel();
        drop(receiver);
        let controller = RecordingEnvironmentController { commands };
        drop(controller.begin());
    }

    #[test]
    fn overlapping_sessions_restore_only_after_the_last_session() {
        let (controller, events) = observed_controller();
        assert_events(&controller.commands, &events, &[]);

        let first = controller.begin();
        let second = controller.begin();
        assert_events(&controller.commands, &events, &[Event::Started]);

        drop(first);
        assert_events(&controller.commands, &events, &[]);

        let third = controller.begin();
        drop(second);
        assert_events(&controller.commands, &events, &[]);

        drop(third);
        assert_events(&controller.commands, &events, &[Event::Restored]);
    }

    #[test]
    fn a_new_session_reacquires_the_environment_after_restoration() {
        let (controller, events) = observed_controller();
        for _ in 0..2 {
            let session = controller.begin();
            assert_events(&controller.commands, &events, &[Event::Started]);
            drop(session);
            assert_events(&controller.commands, &events, &[Event::Restored]);
        }
    }

    #[test]
    fn controller_moves_and_clones_do_not_restore_a_live_session() {
        let (controller, events) = observed_controller();
        let session = controller.begin();
        assert_events(&controller.commands, &events, &[Event::Started]);

        let cloned = controller.clone();
        let moved = controller;
        drop(moved);
        assert_events(&cloned.commands, &events, &[]);

        let overlapping = cloned.begin();
        drop(cloned);
        assert_events(&session.commands, &events, &[]);

        drop(session);
        assert_events(&overlapping.commands, &events, &[]);

        drop(overlapping);
        assert_eq!(
            events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Event::Restored
        );
        assert_eq!(
            events.recv_timeout(Duration::from_secs(2)),
            Err(RecvTimeoutError::Disconnected)
        );
    }
}

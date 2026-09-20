use std::io::Cursor;
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use rodio::buffer::SamplesBuffer;
use rodio::{Decoder, DeviceSinkBuilder, Source};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tone {
    #[cfg(target_os = "macos")]
    Wake,
    #[cfg(target_os = "macos")]
    Sleep,
    #[cfg(target_os = "macos")]
    Error,
    DictationStart,
    DictationStop,
    Cancel,
}

static DICTATION_PLAYER: OnceLock<SyncSender<Tone>> = OnceLock::new();
static LOADER_STARTED: AtomicBool = AtomicBool::new(false);
static ENABLED: AtomicBool = AtomicBool::new(true);
static VOLUME: AtomicU32 = AtomicU32::new(0.5_f32.to_bits());

/// Extra time the device sink stays open after the queued audio's nominal
/// duration, so short sounds and scheduling jitter never cut off playback.
const PLAYBACK_MARGIN: Duration = Duration::from_millis(120);
const DEFAULT_TONE_DURATION: Duration = Duration::from_millis(300);

#[cfg(target_os = "macos")]
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn set_volume(volume: f32) {
    VOLUME.store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
}

fn volume() -> f32 {
    f32::from_bits(VOLUME.load(Ordering::Relaxed))
}

fn sounds_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed) && volume() > 0.0
}

// Only the playback worker owns a device. Turning sounds off drops it within
// one observation interval, and opening/retrying never blocks capture or UI.
struct FeedbackOutput<S> {
    sink: Option<S>,
    retry_at: Instant,
    busy_until: Option<Instant>,
}

impl<S> FeedbackOutput<S> {
    fn new() -> Self {
        Self {
            sink: None,
            retry_at: Instant::now(),
            busy_until: None,
        }
    }

    /// Whether playback (or an imminent one) still needs the device.
    fn busy(&self, now: Instant) -> bool {
        self.busy_until.is_some_and(|until| now < until)
    }

    /// Drops the device sink once playback finished so the OS audio engine can
    /// power down and the system can idle sleep again. Reopened on demand.
    fn release_when_idle(&mut self, now: Instant) {
        if self.sink.is_some() && !self.busy(now) {
            self.sink = None;
            self.busy_until = None;
            // Allow an immediate reopen when the next tone arrives.
            self.retry_at = now;
        }
    }

    fn update(
        &mut self,
        requested: bool,
        now: Instant,
        open: impl FnOnce() -> Result<S>,
    ) -> Result<()> {
        if !requested {
            self.sink = None;
            self.busy_until = None;
            self.retry_at = now;
        } else if self.sink.is_none() && now >= self.retry_at {
            self.retry_at = now + Duration::from_secs(2);
            self.sink = Some(open()?);
        }
        Ok(())
    }
}

/// Maps the admission wait outcome to an error. A timeout does not stop the
/// loader thread: the audio stack may simply be cold, and the loader keeps
/// running so tones recover once the default output finally opens.
fn admission_error(outcome: Result<Result<()>, mpsc::RecvTimeoutError>) -> Result<()> {
    match outcome {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(error),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(eyre!("timed out preloading feedback audio")),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(eyre!("feedback audio loader exited before initializing"))
        }
    }
}

fn publish_player(
    player: &OnceLock<SyncSender<Tone>>,
    sender: SyncSender<Tone>,
    ready: SyncSender<Result<()>>,
    outcome: Result<()>,
) {
    let _ = player.set(sender);
    let _ = ready.send(outcome);
}

pub fn preload() -> Result<()> {
    if DICTATION_PLAYER.get().is_some() {
        return Ok(());
    }
    if LOADER_STARTED.swap(true, Ordering::AcqRel) {
        // A loader is already running from an earlier admission; it registers
        // the player itself, so report the same slow-audio outcome.
        return Err(eyre!("feedback audio is still initializing"));
    }
    let (sender, receiver) = mpsc::sync_channel(8);
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = (|| -> Result<_> {
            let start = decode(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/resources/audio/startRecording.mp3"
            )))?;
            let stop = decode(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/resources/audio/stopRecording.mp3"
            )))?;
            let cancel = decode(include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/resources/audio/cancel.mp3"
            )))?;
            Ok((start, stop, cancel))
        })();
        let Ok((start, stop, cancel)) = result else {
            let _ = ready_sender.send(result.map(|_| ()));
            LOADER_STARTED.store(false, Ordering::Release);
            return;
        };
        let mut output = FeedbackOutput::new();
        let open =
            || DeviceSinkBuilder::open_default_sink().wrap_err("could not open the audio output");
        // Lazy: the device sink is only opened when a tone actually plays and
        // is dropped again right after, so macOS keeps its no-idle-sleep
        // audio assertion (coreaudiod) out of the picture while hex is idle.
        let initial = output.update(false, Instant::now(), open);
        // Publish even after an admission timeout or output-open failure. The
        // same worker keeps observing preferences and retrying the device.
        publish_player(&DICTATION_PLAYER, sender, ready_sender, initial);
        loop {
            let tone = match receiver.recv_timeout(Duration::from_millis(250)) {
                Ok(tone) => Some(tone),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            if let Some(tone) = tone
                && sounds_enabled()
            {
                if let Err(error) = output.update(true, Instant::now(), open) {
                    tracing::warn!(%error, "recording audio output unavailable; retrying");
                }
                let now = Instant::now();
                let sound = match tone {
                    Tone::DictationStart => Some(&start),
                    Tone::DictationStop => Some(&stop),
                    Tone::Cancel => Some(&cancel),
                    #[cfg(target_os = "macos")]
                    Tone::Wake | Tone::Sleep | Tone::Error => None,
                };
                if let (Some(sound), Some(sink)) = (sound, output.sink.as_ref()) {
                    let duration = sound.total_duration().unwrap_or(DEFAULT_TONE_DURATION);
                    sink.mixer().add(sound.clone().amplify(volume()));
                    output.busy_until = Some(now + duration + PLAYBACK_MARGIN);
                }
            }
            // Release the device as soon as playback queue drains.
            output.release_when_idle(Instant::now());
            if !sounds_enabled() {
                let _ = output.update(false, Instant::now(), open);
            }
        }
    });
    let outcome = ready_receiver.recv_timeout(Duration::from_secs(2));
    admission_error(outcome)?;
    Ok(())
}

pub fn play(tone: Tone) {
    if !sounds_enabled() {
        return;
    }
    match tone {
        Tone::DictationStart | Tone::DictationStop | Tone::Cancel => {
            enqueue(DICTATION_PLAYER.get(), tone);
        }
        #[cfg(target_os = "macos")]
        Tone::Wake | Tone::Sleep | Tone::Error => play_system_sound(tone),
    }
}

fn enqueue(player: Option<&SyncSender<Tone>>, tone: Tone) {
    if let Some(player) = player {
        let _ = player.try_send(tone);
    }
}

#[cfg(target_os = "macos")]
fn play_system_sound(tone: Tone) {
    let sound = match tone {
        Tone::Wake => "Pop",
        Tone::Sleep => "Tink",
        Tone::Error => "Basso",
        Tone::DictationStart | Tone::DictationStop | Tone::Cancel => return,
    };
    let volume = volume().to_string();
    let child = Command::new("/usr/bin/afplay")
        .args([
            "-v",
            &volume,
            &format!("/System/Library/Sounds/{sound}.aiff"),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = child {
        thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

fn decode(bytes: &'static [u8]) -> Result<SamplesBuffer> {
    let decoder = Decoder::new(Cursor::new(bytes)).wrap_err("could not decode feedback audio")?;
    let channels = decoder.channels();
    let sample_rate = decoder.sample_rate();
    Ok(SamplesBuffer::new(
        channels,
        sample_rate,
        decoder.collect::<Vec<_>>(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_out_admission_still_publishes_a_usable_player() {
        let player = OnceLock::new();
        let (sender, receiver) = mpsc::sync_channel(8);
        let (ready, waiting) = mpsc::sync_channel(1);
        assert!(admission_error(waiting.recv_timeout(Duration::ZERO)).is_err());
        drop(waiting);
        publish_player(&player, sender, ready, Ok(()));
        enqueue(player.get(), Tone::DictationStart);
        assert_eq!(receiver.try_recv().unwrap(), Tone::DictationStart);
    }

    #[test]
    fn sink_is_released_once_playback_finishes_and_reopens_on_demand() {
        use std::cell::Cell;
        struct Sink<'a>(&'a Cell<usize>);
        impl Drop for Sink<'_> {
            fn drop(&mut self) {
                self.0.set(self.0.get() - 1);
            }
        }
        let live = Cell::new(0);
        let open = || {
            live.set(live.get() + 1);
            Ok(Sink(&live))
        };
        let mut output = FeedbackOutput::new();
        let now = Instant::now();

        // Requested open while a tone is "playing".
        output.update(true, now, open).unwrap();
        output.busy_until = Some(now + Duration::from_millis(500));
        assert_eq!(live.get(), 1);

        // Still busy: the sink is retained.
        output.release_when_idle(now + Duration::from_millis(400));
        assert_eq!(live.get(), 1);

        // Playback drained: the device sink is dropped.
        let later = now + Duration::from_millis(600);
        output.release_when_idle(later);
        assert_eq!(live.get(), 0);

        // And it can be reopened afterwards without waiting for backoff.
        output.update(true, later, open).unwrap();
        assert_eq!(live.get(), 1);
    }

    #[test]
    fn sound_off_releases_output_and_reenable_reopens_it() {
        use std::cell::Cell;
        struct Sink<'a>(&'a Cell<usize>);
        impl Drop for Sink<'_> {
            fn drop(&mut self) {
                self.0.set(self.0.get() - 1);
            }
        }
        let live = Cell::new(0);
        let opens = Cell::new(0);
        let open = || {
            live.set(live.get() + 1);
            opens.set(opens.get() + 1);
            Ok(Sink(&live))
        };
        let mut output = FeedbackOutput::new();
        let now = Instant::now();
        output.update(false, now, open).unwrap();
        assert_eq!(opens.get(), 0);
        output.update(true, now, open).unwrap();
        output.update(true, now, open).unwrap();
        assert_eq!(opens.get(), 1);
        assert_eq!(live.get(), 1);
        output.update(false, now, open).unwrap();
        assert_eq!(live.get(), 0);
        output.update(true, now, open).unwrap();
        assert_eq!(opens.get(), 2);
        drop(output);
        assert_eq!(live.get(), 0);
    }

    #[test]
    fn failed_output_initialization_retries_with_a_bounded_backoff() {
        let mut output = FeedbackOutput::new();
        let now = Instant::now();
        assert!(
            output
                .update(true, now, || Err(eyre!("device unavailable")))
                .is_err()
        );
        output
            .update(true, now + Duration::from_secs(1), || -> Result<()> {
                panic!("must not spin on device failure")
            })
            .unwrap();
        assert!(output.sink.is_none());
        output
            .update(true, now + Duration::from_secs(2), || Ok(()))
            .unwrap();
        assert!(output.sink.is_some());
    }

    #[test]
    fn bundled_recording_sounds_decode_without_an_audio_device() {
        for bytes in [
            include_bytes!("../resources/audio/startRecording.mp3").as_slice(),
            include_bytes!("../resources/audio/stopRecording.mp3").as_slice(),
            include_bytes!("../resources/audio/cancel.mp3").as_slice(),
        ] {
            let mut sound = decode(bytes).unwrap();
            assert!(sound.total_duration().unwrap() > Duration::ZERO);
            assert!(sound.any(|sample| sample.abs() > 0.0));
        }
    }

    #[test]
    fn feedback_admission_never_waits_for_playback() {
        let (sender, receiver) = mpsc::sync_channel(2);
        enqueue(Some(&sender), Tone::DictationStart);
        enqueue(Some(&sender), Tone::DictationStop);
        enqueue(Some(&sender), Tone::Cancel);
        assert_eq!(receiver.try_recv().unwrap(), Tone::DictationStart);
        assert_eq!(receiver.try_recv().unwrap(), Tone::DictationStop);
        assert!(receiver.try_recv().is_err());
        drop(receiver);
        enqueue(Some(&sender), Tone::Cancel);
        enqueue(None, Tone::DictationStart);
    }

    #[test]
    fn timeout_and_loader_exit_report_distinct_errors() {
        // A silent channel models a loader still warming the audio stack; a
        // dropped one models a loader that exited before loading.
        assert_eq!(
            admission_error(Err(mpsc::RecvTimeoutError::Timeout))
                .unwrap_err()
                .to_string(),
            "timed out preloading feedback audio"
        );
        assert_eq!(
            admission_error(Err(mpsc::RecvTimeoutError::Disconnected))
                .unwrap_err()
                .to_string(),
            "feedback audio loader exited before initializing"
        );

        // Real channels exercise the same admission wait the loader drives.
        let (slow_sender, slow_receiver) = mpsc::sync_channel::<Result<()>>(1);
        assert!(admission_error(slow_receiver.recv_timeout(Duration::ZERO)).is_err());
        drop(slow_sender);

        let (done_sender, done_receiver) = mpsc::sync_channel::<Result<()>>(1);
        done_sender.send(Ok(())).unwrap();
        assert!(admission_error(done_receiver.recv_timeout(Duration::ZERO)).is_ok());

        let (failed_sender, failed_receiver) = mpsc::sync_channel::<Result<()>>(1);
        failed_sender
            .send(Err(eyre!("could not open the audio output")))
            .unwrap();
        assert_eq!(
            admission_error(failed_receiver.recv_timeout(Duration::ZERO))
                .unwrap_err()
                .to_string(),
            "could not open the audio output"
        );
    }
}

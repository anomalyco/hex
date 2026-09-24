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

/// How long the output device stays open after the last tone finishes. Holding
/// the default output open keeps macOS from idle-sleeping, so an idle app must
/// release it. A tone implies recent user activity, so a grace shorter than any
/// idle-sleep timer costs nothing while sparing bursts of dictation the cold
/// device reopen.
const IDLE_RELEASE_GRACE: Duration = Duration::from_secs(5 * 60);
const OPEN_RETRY_BACKOFF: Duration = Duration::from_secs(2);

// Only the playback worker owns a device. It opens lazily for a tone, stays
// open through the idle grace, and drops within one observation interval once
// sounds turn off or the grace expires. Opening never blocks capture or UI.
struct FeedbackOutput<S> {
    sink: Option<S>,
    retry_at: Instant,
    playing_until: Option<Instant>,
}

impl<S> FeedbackOutput<S> {
    fn new() -> Self {
        Self {
            sink: None,
            retry_at: Instant::now(),
            playing_until: None,
        }
    }

    /// Opens the device for a tone unless a recent failure is still backing off.
    fn open_for_tone(&mut self, now: Instant, open: impl FnOnce() -> Result<S>) -> Result<()> {
        if self.sink.is_none() && now >= self.retry_at {
            self.retry_at = now + OPEN_RETRY_BACKOFF;
            self.sink = Some(open()?);
        }
        Ok(())
    }

    fn mark_playing(&mut self, now: Instant, duration: Duration) {
        let until = now + duration;
        self.playing_until = Some(self.playing_until.unwrap_or(until).max(until));
    }

    /// Releases the device when sounds are off or nothing has played within the
    /// idle grace. A released device reopens immediately for the next tone.
    fn release_when_idle(&mut self, enabled: bool, now: Instant) {
        let idle = self
            .playing_until
            .is_none_or(|until| now >= until + IDLE_RELEASE_GRACE);
        if !enabled || idle {
            self.playing_until = None;
            if self.sink.take().is_some() {
                self.retry_at = now;
            }
        }
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
        let open = || {
            let mut sink = DeviceSinkBuilder::open_default_sink()
                .wrap_err("could not open the audio output")?;
            sink.log_on_drop(false);
            Ok(sink)
        };
        // The device opens for the first tone, not at startup, so an idle app
        // holds no output stream. Publish before any tone arrives.
        publish_player(&DICTATION_PLAYER, sender, ready_sender, Ok(()));
        loop {
            let tone = match receiver.recv_timeout(Duration::from_millis(250)) {
                Ok(tone) => Some(tone),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            let enabled = sounds_enabled();
            output.release_when_idle(enabled, Instant::now());
            let Some(tone) = tone.filter(|_| enabled) else {
                continue;
            };
            let sound = match tone {
                Tone::DictationStart => &start,
                Tone::DictationStop => &stop,
                Tone::Cancel => &cancel,
                #[cfg(target_os = "macos")]
                Tone::Wake | Tone::Sleep | Tone::Error => continue,
            };
            if let Err(error) = output.open_for_tone(Instant::now(), open) {
                tracing::warn!(%error, "recording audio output unavailable; retrying");
            }
            // Opening may have been slow; recheck the latest preference before
            // retaining the output or playing the queued sound.
            if !sounds_enabled() {
                output.release_when_idle(false, Instant::now());
                continue;
            }
            let Some(sink) = output.sink.as_ref() else {
                continue;
            };
            sink.mixer().add(sound.clone().amplify(volume()));
            output.mark_playing(
                Instant::now(),
                sound.total_duration().unwrap_or(Duration::ZERO),
            );
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

    impl<S> FeedbackOutput<S> {
        fn is_open(&self) -> bool {
            self.sink.is_some()
        }
    }

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
    fn output_opens_for_a_tone_and_releases_after_the_idle_grace() {
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
        let tone = Duration::from_secs(1);

        // Idle observation without a tone never opens the device.
        output.release_when_idle(true, now);
        assert_eq!(opens.get(), 0);

        output.open_for_tone(now, open).unwrap();
        output.mark_playing(now, tone);
        output.open_for_tone(now, open).unwrap();
        assert_eq!((opens.get(), live.get()), (1, 1));

        // Retained while playing and through the grace, released afterwards.
        output.release_when_idle(true, now + tone);
        output.release_when_idle(true, now + tone + IDLE_RELEASE_GRACE / 2);
        assert!(output.is_open());
        output.release_when_idle(true, now + tone + IDLE_RELEASE_GRACE);
        assert!(!output.is_open());
        assert_eq!(live.get(), 0);

        // The next tone reopens immediately, without a failure backoff.
        let later = now + tone + IDLE_RELEASE_GRACE;
        output.open_for_tone(later, open).unwrap();
        assert_eq!((opens.get(), live.get()), (2, 1));

        // Turning sounds off releases even while a tone is nominally playing.
        output.mark_playing(later, tone);
        output.release_when_idle(false, later);
        assert!(!output.is_open());
        drop(output);
        assert_eq!(live.get(), 0);
    }

    #[test]
    fn overlapping_tones_extend_playback_instead_of_truncating_it() {
        let mut output = FeedbackOutput::new();
        let now = Instant::now();
        output.open_for_tone(now, || Ok(())).unwrap();
        output.mark_playing(now, Duration::from_secs(2));
        output.mark_playing(now + Duration::from_millis(300), Duration::from_secs(1));
        assert_eq!(output.playing_until, Some(now + Duration::from_secs(2)));

        output.release_when_idle(true, now + Duration::from_secs(1) + IDLE_RELEASE_GRACE);
        assert!(output.is_open());
        output.release_when_idle(true, now + Duration::from_secs(2) + IDLE_RELEASE_GRACE);
        assert!(!output.is_open());
    }

    #[test]
    fn failed_output_initialization_retries_with_a_bounded_backoff() {
        let mut output = FeedbackOutput::new();
        let now = Instant::now();
        assert!(
            output
                .open_for_tone(now, || Err(eyre!("device unavailable")))
                .is_err()
        );
        output
            .open_for_tone(now + Duration::from_secs(1), || -> Result<()> {
                panic!("must not spin on device failure")
            })
            .unwrap();
        assert!(!output.is_open());
        output
            .open_for_tone(now + OPEN_RETRY_BACKOFF, || Ok(()))
            .unwrap();
        assert!(output.is_open());
    }

    #[test]
    #[ignore = "opens the native default output and inspects pmset assertions"]
    #[cfg(target_os = "macos")]
    fn native_output_release_clears_the_idle_sleep_assertion() {
        fn assertion_for_this_process() -> bool {
            let output = Command::new("/usr/bin/pmset")
                .args(["-g", "assertions"])
                .output()
                .unwrap();
            let report = String::from_utf8_lossy(&output.stdout);
            let marker = format!("Created for PID: {}", std::process::id());
            report
                .split("PreventUserIdleSystemSleep")
                .skip(1)
                .any(|section| section.lines().take(3).any(|line| line.contains(&marker)))
        }

        let mut sink = DeviceSinkBuilder::open_default_sink().unwrap();
        sink.log_on_drop(false);
        thread::sleep(Duration::from_millis(500));
        assert!(
            assertion_for_this_process(),
            "an open output should hold the coreaudiod sleep assertion"
        );
        drop(sink);
        let released = (0..20).any(|_| {
            thread::sleep(Duration::from_millis(250));
            !assertion_for_this_process()
        });
        assert!(released, "releasing the output should clear the assertion");
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

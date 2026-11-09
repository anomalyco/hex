//! Labs › Keep recordings: hearing a saved dictation through the default
//! output device.
//!
//! Playback runs on a worker thread, because decoding a WAV and opening an
//! output device can take a while. One playback owns the output at a time:
//! starting another and Stop both advance [`GENERATION`], and the playback
//! they replaced goes quiet at its next check. The card shows Stop while a
//! [`Playback`] runs and Play again once its worker answers.

use std::fs::File;
use std::io::BufReader;
use std::num::{NonZeroU16, NonZeroU32};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::thread;
use std::time::{Duration, Instant};

use hound::WavReader;
use rodio::buffer::SamplesBuffer;
use rodio::{DeviceSinkBuilder, Player};

/// How often a playing clip checks whether it was replaced or stopped.
const CHECK_INTERVAL: Duration = Duration::from_millis(16);
/// How long the sink stays open after the clip ends, so the device plays
/// the frames the mixer still holds.
const DRAIN: Duration = Duration::from_millis(100);

/// The playback that owns the output. Advancing it stops that one.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// One saved recording started from its History card, and how it went.
pub struct Playback {
    /// The entry whose card started it.
    pub id: u64,
    worker: Option<Receiver<Result<(), &'static str>>>,
    /// Why it could not play, kept until the next start.
    error: Option<&'static str>,
}

impl Playback {
    /// Starts `path` playing on a worker, replacing whatever played before;
    /// `wake` asks the window to redraw when it ends or fails.
    pub fn start(id: u64, path: &Path, wake: impl Fn() + Send + 'static) -> Self {
        let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
        let path = path.to_path_buf();
        let (sender, receiver) = channel();
        let started = thread::Builder::new()
            .name("hex-history-playback".into())
            .spawn(move || {
                let result = play(&path, generation);
                let _ = sender.send(result);
                wake();
            });
        Self {
            id,
            worker: started.is_ok().then_some(receiver),
            error: started.err().map(|_| "Playback stopped."),
        }
    }

    /// A playback that never started, with its reason. It still takes the
    /// output from whatever played before, which goes quiet at its next
    /// check.
    pub fn failed(id: u64, error: &'static str) -> Self {
        GENERATION.fetch_add(1, Ordering::AcqRel);
        Self {
            id,
            worker: None,
            error: Some(error),
        }
    }

    /// A playback that never opened a device, for the pane's tests: it keeps
    /// its worker answer open, so the card shows Stop until `stop`.
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn test_playing(id: u64) -> Self {
        let (sender, receiver) = channel();
        std::mem::forget(sender);
        Self {
            id,
            worker: Some(receiver),
            error: None,
        }
    }

    /// Whether any playback is still running.
    pub fn playing(&self) -> bool {
        self.worker.is_some()
    }

    /// Whether entry `id` is the one playing.
    pub fn is_playing(&self, id: u64) -> bool {
        self.id == id && self.playing()
    }

    /// Why entry `id`'s playback failed, if it did.
    pub fn error(&self, id: u64) -> Option<&'static str> {
        self.error.filter(|_| self.id == id)
    }

    /// Stops the running playback; the card shows Play again at once.
    pub fn stop(&mut self) {
        if self.worker.take().is_some() {
            GENERATION.fetch_add(1, Ordering::AcqRel);
        }
    }

    /// Takes the worker's answer once it ends or fails; `true` when it did.
    pub fn poll(&mut self) -> bool {
        let Some(worker) = self.worker.as_ref() else {
            return false;
        };
        match worker.try_recv() {
            Ok(Ok(())) => self.worker = None,
            Ok(Err(error)) => {
                self.worker = None;
                self.error = Some(error);
            }
            Err(TryRecvError::Disconnected) => {
                self.worker = None;
                self.error = Some("Playback stopped.");
            }
            Err(TryRecvError::Empty) => return false,
        }
        true
    }
}

/// One recording's samples, at the format its WAV names.
struct Clip {
    samples: Vec<f32>,
    channels: NonZeroU16,
    sample_rate: NonZeroU32,
}

/// Decodes saved recordings the way `recordings::write` made them: 16-bit
/// mono PCM.
fn decode(file: File) -> Result<Clip, &'static str> {
    const UNREADABLE: &str = "The recording could not be read.";
    let reader = WavReader::new(BufReader::new(file)).map_err(|_| UNREADABLE)?;
    let spec = reader.spec();
    let samples = reader
        .into_samples::<i16>()
        .map(|sample| sample.map(|sample| f32::from(sample) / f32::from(i16::MAX)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| UNREADABLE)?;
    let channels = NonZeroU16::new(spec.channels).ok_or(UNREADABLE)?;
    let sample_rate = NonZeroU32::new(spec.sample_rate).ok_or(UNREADABLE)?;
    if samples.is_empty() {
        return Err(UNREADABLE);
    }
    Ok(Clip {
        samples,
        channels,
        sample_rate,
    })
}

/// Plays one recording to its end, or returns as soon as `generation` is
/// replaced. Blocks; only the worker calls it.
fn play(path: &Path, generation: u64) -> Result<(), &'static str> {
    let file = File::open(path).map_err(|_| "The recording is gone.")?;
    let clip = decode(file)?;
    if GENERATION.load(Ordering::Acquire) != generation {
        return Ok(());
    }
    let mut sink =
        DeviceSinkBuilder::open_default_sink().map_err(|_| "No audio output is available.")?;
    sink.log_on_drop(false);
    if GENERATION.load(Ordering::Acquire) != generation {
        return Ok(());
    }
    let player = Player::connect_new(sink.mixer());
    let seconds = clip.samples.len() as f64
        / (f64::from(clip.sample_rate.get()) * f64::from(clip.channels.get()));
    player.append(SamplesBuffer::new(
        clip.channels,
        clip.sample_rate,
        clip.samples,
    ));
    // A device that stops consuming would otherwise leave Stop showing
    // forever; give the clip its own length plus room to reach the device.
    let limit = Duration::from_secs_f64(seconds) + Duration::from_secs(5);
    let started = Instant::now();
    while !player.empty() {
        if GENERATION.load(Ordering::Acquire) != generation {
            player.stop();
            return Ok(());
        }
        if started.elapsed() > limit {
            break;
        }
        thread::sleep(CHECK_INTERVAL);
    }
    let drained = Instant::now() + DRAIN;
    while Instant::now() < drained {
        if GENERATION.load(Ordering::Acquire) != generation {
            break;
        }
        thread::sleep(CHECK_INTERVAL);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::recordings::{self, Recording};

    #[test]
    fn a_kept_recording_decodes_into_its_own_clip() {
        let directory = std::env::temp_dir().join(format!(
            "hex-playback-{}-{}",
            std::process::id(),
            crate::history::now_ms()
        ));
        recordings::write(
            &directory,
            1,
            &Recording::from_samples(vec![0.0, 0.5, -0.5, 1.0]),
        )
        .unwrap();
        let clip = decode(File::open(recordings::path(&directory, 1)).unwrap()).unwrap();
        let _ = std::fs::remove_dir_all(&directory);
        assert_eq!(clip.channels.get(), 1);
        assert_eq!(clip.sample_rate.get(), 16_000);
        assert_eq!(clip.samples.len(), 4);
        assert!(clip.samples[0].abs() < 0.001);
        assert!((clip.samples[1] - 0.5).abs() < 0.001);
        assert!((clip.samples[2] + 0.5).abs() < 0.001);
        assert!((clip.samples[3] - 1.0).abs() < 0.001);
    }

    #[test]
    fn a_worker_answer_returns_the_card_to_play_and_keeps_a_failure() {
        let (sender, receiver) = channel();
        let mut playback = Playback {
            id: 7,
            worker: Some(receiver),
            error: None,
        };
        assert!(playback.is_playing(7));
        sender.send(Ok(())).unwrap();
        assert!(playback.poll());
        assert!(!playback.playing(), "an answer returns the button to Play");
        assert!(!playback.poll(), "an answered playback polls once");

        let (sender, receiver) = channel();
        let mut playback = Playback {
            id: 8,
            worker: Some(receiver),
            error: None,
        };
        sender.send(Err("No audio output is available.")).unwrap();
        assert!(playback.poll());
        assert_eq!(
            playback.error(8),
            Some("No audio output is available."),
            "only the entry that failed shows its reason"
        );
        assert_eq!(playback.error(7), None);
    }
}

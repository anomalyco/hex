//! Where a capture's microphone audio comes from. Annotate and every
//! Screenshots recording that keeps the microphone dictation uses copy the
//! dictation timeline; a Screenshots recording can also keep no microphone,
//! or another input of its own (macOS), opened for that recording alone and
//! closed when it stops. Its own input never touches dictation's stream: a
//! device dictation already has open is copied from the timeline instead.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::SyncSender;

use color_eyre::Result;

use super::AudioTimeline;
use super::sink::{CaptureAudio, CaptureSinkClosed};
use crate::audio::CaptureInstant;
use crate::audio::input_level::{self, Peak};

/// Whose level the recording capsule shows: none (Annotate, or a recording
/// that keeps no microphone), dictation's input, or the recording's own.
static LEVEL: AtomicU8 = AtomicU8::new(NO_LEVEL);
const NO_LEVEL: u8 = 0;
const DICTATION_LEVEL: u8 = 1;
const OWN_LEVEL: u8 = 2;
/// The latest peak of a recording's own input.
static OWN_PEAK: Peak = Peak::new();

/// The recording's microphone level, 0 to 1 (0 while no audio arrives), or
/// `None` when its capsule shows none.
pub(crate) fn recording_level() -> Option<f32> {
    let peak = match LEVEL.load(Ordering::Acquire) {
        DICTATION_LEVEL => input_level::recent(),
        OWN_LEVEL => OWN_PEAK.recent(),
        _ => return None,
    };
    Some(peak.unwrap_or(0.0))
}

/// The capsule shows no level: the recording stopped.
pub(super) fn hide_level() {
    LEVEL.store(NO_LEVEL, Ordering::Release);
}

/// The microphone a capture keeps.
pub(super) enum Microphone {
    /// The dictation timeline's.
    Timeline,
    /// None: the sender is only held so the collector keeps running until the
    /// capture stops.
    Silent(SyncSender<CaptureAudio>),
    /// An input of the recording's own.
    #[cfg(target_os = "macos")]
    Own(own::OwnMicrophone),
}

/// What a Screenshots recording's `record_microphone` asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Timeline,
    Silent,
    Device(String),
}

impl Choice {
    /// `record_microphone` (`none`, `default`, or a device's name) against
    /// the device dictation has open: that one is copied from the timeline.
    pub(super) fn of(setting: &str, dictation_device: Option<&str>) -> Self {
        use crate::app_settings::{RECORD_MICROPHONE_DEFAULT, RECORD_MICROPHONE_NONE};
        let setting = setting.trim();
        if setting.eq_ignore_ascii_case(RECORD_MICROPHONE_NONE) {
            Self::Silent
        } else if setting.is_empty()
            || setting.eq_ignore_ascii_case(RECORD_MICROPHONE_DEFAULT)
            || dictation_device.is_some_and(|name| name == setting)
        {
            Self::Timeline
        } else {
            Self::Device(setting.to_owned())
        }
    }
}

impl Microphone {
    /// Starts copying `choice`'s audio into `sink` from `since`. An input of
    /// its own that is missing or won't open falls back to the timeline, so
    /// the recording still keeps a voice.
    pub(super) fn open(
        choice: Choice,
        audio: &impl AudioTimeline,
        sink: SyncSender<CaptureAudio>,
        since: CaptureInstant,
    ) -> Result<Self> {
        match choice {
            Choice::Silent => Ok(Self::Silent(sink)),
            Choice::Timeline => audio.open_sink(sink, since).map(|()| Self::Timeline),
            #[cfg(target_os = "macos")]
            Choice::Device(device) => {
                match own::OwnMicrophone::open(&device, sink.clone(), since) {
                    Ok(own) => Ok(Self::Own(own)),
                    Err(error) => {
                        tracing::warn!(%error, device, "recording with dictation's microphone instead");
                        audio.open_sink(sink, since).map(|()| Self::Timeline)
                    }
                }
            }
            #[cfg(not(target_os = "macos"))]
            Choice::Device(_) => audio.open_sink(sink, since).map(|()| Self::Timeline),
        }
    }

    /// Shows this microphone's level on the recording capsule while
    /// `shown`, or none.
    pub(super) fn show_level(&self, shown: bool) {
        let level = match self {
            _ if !shown => NO_LEVEL,
            Self::Timeline => DICTATION_LEVEL,
            Self::Silent(_) => NO_LEVEL,
            #[cfg(target_os = "macos")]
            Self::Own(_) => OWN_LEVEL,
        };
        LEVEL.store(level, Ordering::Release);
    }

    /// Stops copying once audio through `through` arrived (bounded wait).
    pub(super) fn close(
        self,
        audio: &impl AudioTimeline,
        through: CaptureInstant,
    ) -> Result<Option<CaptureSinkClosed>> {
        match self {
            Self::Timeline => audio.close_sink(through),
            Self::Silent(sink) => {
                drop(sink);
                Ok(None)
            }
            #[cfg(target_os = "macos")]
            Self::Own(own) => Ok(Some(own.close(through))),
        }
    }
}

#[cfg(target_os = "macos")]
mod own {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc::SyncSender;
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use color_eyre::Result;

    use super::super::sink::{CaptureAudio, CaptureSink, CaptureSinkClosed};
    use crate::audio::{AudioInput, AudioInputEvent, CaptureInstant};

    /// How long a closing input waits for audio through the stop.
    const CLOSE_WAIT: Duration = Duration::from_millis(300);
    /// How often a failed input is opened again.
    const REOPEN_EVERY: Duration = Duration::from_secs(1);
    /// [`OwnMicrophone::through`] while the recording goes on.
    const RUNNING: u64 = u64::MAX;

    /// An input opened for one recording, read on its own thread. A stream
    /// that fails is opened again; what it missed is a gap in the recording,
    /// never the end of it.
    pub(in super::super) struct OwnMicrophone {
        /// The stop's instant in nanoseconds once the recording stops.
        through: Arc<AtomicU64>,
        thread: JoinHandle<CaptureSinkClosed>,
    }

    impl OwnMicrophone {
        pub(in super::super) fn open(
            device: &str,
            sink: SyncSender<CaptureAudio>,
            since: CaptureInstant,
        ) -> Result<Self> {
            let input = open(device)?;
            tracing::info!(device, "a recording opened its own microphone");
            let through = Arc::new(AtomicU64::new(RUNNING));
            let stop = through.clone();
            let device = device.to_owned();
            let thread = std::thread::Builder::new()
                .name("capture-microphone".into())
                .spawn(move || read(device, input, CaptureSink::new(sink, since), &stop))?;
            Ok(Self { through, thread })
        }

        /// Stops reading once audio through `through` arrived, or after
        /// [`CLOSE_WAIT`], and closes the input.
        pub(in super::super) fn close(self, through: CaptureInstant) -> CaptureSinkClosed {
            self.through.store(through.as_nanos(), Ordering::Release);
            self.thread.join().unwrap_or(CaptureSinkClosed {
                pending_gap: None,
                accounted_through: through,
            })
        }
    }

    /// Opens `device` by its exact name, never another input in its place.
    fn open(device: &str) -> Result<AudioInput> {
        crate::audio::open_named_input(device)
    }

    fn read(
        device: String,
        input: AudioInput,
        mut sink: CaptureSink,
        through: &AtomicU64,
    ) -> CaptureSinkClosed {
        let mut input = Some(input);
        let mut reopened = Instant::now();
        let mut stopping: Option<(CaptureInstant, Instant)> = None;
        loop {
            if stopping.is_none() {
                let at = through.load(Ordering::Acquire);
                if at != RUNNING {
                    stopping = Some((CaptureInstant::from_nanos(at), Instant::now()));
                }
            }
            if let Some((_, asked)) = stopping
                && (input.is_none() || asked.elapsed() >= CLOSE_WAIT)
            {
                break;
            }
            let Some(open) = &input else {
                std::thread::sleep(Duration::from_millis(50));
                if reopened.elapsed() >= REOPEN_EVERY {
                    reopened = Instant::now();
                    match self::open(&device) {
                        Ok(again) => {
                            tracing::info!(device, "a recording's microphone opened again");
                            input = Some(again);
                        }
                        Err(error) => {
                            tracing::debug!(%error, "a recording's microphone is still gone");
                        }
                    }
                }
                continue;
            };
            match open.recv_timeout(Duration::from_millis(50)) {
                AudioInputEvent::Chunk {
                    samples,
                    captured_through,
                } => {
                    super::OWN_PEAK.record(&samples);
                    if !sink.offer(&samples, open.sample_rate, captured_through) {
                        break;
                    }
                    if stopping.is_some_and(|(at, _)| captured_through >= at) {
                        break;
                    }
                }
                AudioInputEvent::Timeout => {}
                AudioInputEvent::StreamFailed(error) => {
                    // What it misses is a gap: the next chunk lands after it.
                    tracing::warn!(error, device, "a recording's microphone failed");
                    input = None;
                    reopened = Instant::now();
                }
            }
        }
        sink.close()
    }
}

#[cfg(test)]
mod tests {
    use super::Choice;

    #[test]
    fn the_setting_picks_the_timeline_nothing_or_a_device() {
        assert_eq!(Choice::of("none", Some("Mic")), Choice::Silent);
        assert_eq!(Choice::of("default", Some("Mic")), Choice::Timeline);
        assert_eq!(Choice::of("", None), Choice::Timeline);
        // Dictation's own device is copied, never opened twice.
        assert_eq!(Choice::of("Mic", Some("Mic")), Choice::Timeline);
        assert_eq!(
            Choice::of("USB Mic", Some("Mic")),
            Choice::Device("USB Mic".into())
        );
    }
}

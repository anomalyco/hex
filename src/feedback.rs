use std::io::Cursor;
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use rodio::buffer::SamplesBuffer;
use rodio::cpal::BufferSize;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Source};

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

/// A request to the playback worker. The start tone of a shortcut press is
/// provisional: the press may still become a chord or short tap, which
/// retracts it before or while it sounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Request {
    Play(Tone),
    ProvisionalStart,
    RetractStart,
    Prepare,
}

static DICTATION_PLAYER: OnceLock<SyncSender<Request>> = OnceLock::new();
static LOADER_STARTED: AtomicBool = AtomicBool::new(false);
static ENABLED: AtomicBool = AtomicBool::new(true);
static VOLUME: AtomicU32 = AtomicU32::new(0.5_f32.to_bits());
/// 0 is the bundled classic set; `n` is the `n - 1`th procedural preset.
static SOUND_SET: AtomicUsize = AtomicUsize::new(0);

/// The id of the bundled recorded sounds, the default sound set.
pub const CLASSIC_SOUND_SET: &str = "classic";

/// Selects the sound set by id; an unknown id falls back to the classic set.
pub fn set_sound_set(id: &str) {
    let index = crate::sound_synth::presets()
        .iter()
        .position(|preset| preset.id == id)
        .map_or(0, |index| index + 1);
    SOUND_SET.store(index, Ordering::Relaxed);
}

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
    player: &OnceLock<SyncSender<Request>>,
    sender: SyncSender<Request>,
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
    let (sender, receiver) = mpsc::sync_channel::<Request>(8);
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
            Ok(SoundSet {
                start,
                stop,
                cancel,
            })
        })();
        let classic = match result {
            Ok(classic) => classic,
            Err(error) => {
                let _ = ready_sender.send(Err(error));
                LOADER_STARTED.store(false, Ordering::Release);
                return;
            }
        };
        let sets: Vec<SoundSet> = std::iter::once(classic)
            .chain(crate::sound_synth::presets().iter().map(SoundSet::render))
            .collect();
        let current = || &sets[SOUND_SET.load(Ordering::Relaxed).min(sets.len() - 1)];
        let mut output = FeedbackOutput::new();
        // The device opens for the first tone, not at startup, so an idle app
        // holds no output stream. Publish before any tone arrives.
        publish_player(&DICTATION_PLAYER, sender, ready_sender, Ok(()));
        let mut start_tone = ProvisionalStart::default();
        loop {
            let timeout = start_tone.wait(Instant::now(), OBSERVATION_INTERVAL);
            let request = match receiver.recv_timeout(timeout) {
                Ok(request) => Some(request),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            let received_at = Instant::now();
            let timed_out = request.is_none();
            let enabled = sounds_enabled();
            output.release_when_idle(enabled, received_at);
            if !enabled {
                start_tone.retract();
                continue;
            }
            let tone = match request {
                Some(Request::Play(tone)) => {
                    if tone == Tone::DictationStart {
                        start_tone.cancel_pending();
                    }
                    Some(tone)
                }
                Some(Request::ProvisionalStart) => {
                    start_tone.request(received_at);
                    // Opening during the chord grace keeps a cold device off
                    // the path to the audible tone.
                    output.prepare(received_at);
                    None
                }
                Some(Request::RetractStart) => {
                    start_tone.retract();
                    None
                }
                Some(Request::Prepare) => {
                    output.prepare(received_at);
                    None
                }
                None => None,
            };
            if let Some(tone) = tone {
                let set = current();
                let sound = match tone {
                    Tone::DictationStart => &set.start,
                    Tone::DictationStop => &set.stop,
                    Tone::Cancel => &set.cancel,
                    #[cfg(target_os = "macos")]
                    Tone::Wake | Tone::Sleep | Tone::Error => continue,
                };
                output.play(sound.clone(), tone);
            } else if timed_out && start_tone.is_due(Instant::now()) {
                // Only an empty queue reaches here, so a retraction sent while
                // the device opened always wins over the tone.
                let (sound, retracted) = Retractable::new(current().start.clone());
                if output.play(sound, Tone::DictationStart) {
                    start_tone.sounding(retracted);
                } else {
                    start_tone.retract();
                }
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
            enqueue(DICTATION_PLAYER.get(), Request::Play(tone));
        }
        #[cfg(target_os = "macos")]
        Tone::Wake | Tone::Sleep | Tone::Error => play_system_sound(tone),
    }
}

/// Plays the start tone for a shortcut press that may still become a chord or
/// short tap. It sounds after [`CHORD_GRACE`] unless [`retract_start`] arrives
/// first, so modifiers pressed together for another shortcut stay silent.
pub fn play_provisional_start() {
    if sounds_enabled() {
        enqueue(DICTATION_PLAYER.get(), Request::ProvisionalStart);
    }
}

/// Withdraws a provisional start tone: drops it before it sounds, or fades it
/// out over [`RETRACT_FADE`]. Has no effect on a tone that already finished.
pub fn retract_start() {
    enqueue(DICTATION_PLAYER.get(), Request::RetractStart);
}

/// Opens the output ahead of a tone expected soon, without playing anything.
pub fn prepare() {
    if sounds_enabled() {
        enqueue(DICTATION_PLAYER.get(), Request::Prepare);
    }
}

fn enqueue(player: Option<&SyncSender<Request>>, request: Request) {
    if let Some(player) = player {
        let _ = player.try_send(request);
    }
}

/// How long a provisional start tone waits before sounding. Modifiers pressed
/// together for another application's shortcut arrive within this window, so
/// those chords stay silent while a deliberate press still sounds at once.
const CHORD_GRACE: Duration = Duration::from_millis(40);
/// How quickly a retracted start tone fades out instead of clicking off.
const RETRACT_FADE: Duration = Duration::from_millis(20);
const OBSERVATION_INTERVAL: Duration = Duration::from_millis(250);
/// The output buffer, about 10 ms at 48 kHz. Rodio's default 50 ms buffer adds
/// that much again to every tone.
const OUTPUT_BUFFER_FRAMES: u32 = 512;

/// The worker's view of the provisional start tone: due while waiting out the
/// chord grace, then retractable while it sounds.
#[derive(Default)]
struct ProvisionalStart {
    due: Option<Instant>,
    sounding: Option<Arc<AtomicBool>>,
}

impl ProvisionalStart {
    fn request(&mut self, at: Instant) {
        self.retract();
        self.due = Some(at + CHORD_GRACE);
    }

    /// How long the worker may block before the pending tone is due.
    fn wait(&self, now: Instant, observation: Duration) -> Duration {
        self.due
            .map_or(observation, |due| due.saturating_duration_since(now))
            .min(observation)
    }

    fn is_due(&self, now: Instant) -> bool {
        self.due.is_some_and(|due| now >= due)
    }

    fn sounding(&mut self, retracted: Arc<AtomicBool>) {
        self.due = None;
        self.sounding = Some(retracted);
    }

    fn cancel_pending(&mut self) {
        self.due = None;
    }

    fn retract(&mut self) {
        self.due = None;
        if let Some(retracted) = self.sounding.take() {
            retracted.store(true, Ordering::Release);
        }
    }
}

/// A sound that fades out over [`RETRACT_FADE`] once its flag is set.
struct Retractable<S> {
    inner: S,
    retracted: Arc<AtomicBool>,
    gain: f32,
    step: f32,
    channel: u16,
}

impl<S: Source> Retractable<S> {
    fn new(inner: S) -> (Self, Arc<AtomicBool>) {
        let retracted = Arc::new(AtomicBool::new(false));
        let fade_frames = RETRACT_FADE.as_secs_f32() * inner.sample_rate().get() as f32;
        let sound = Self {
            inner,
            retracted: retracted.clone(),
            gain: 1.0,
            step: 1.0 / fade_frames.max(1.0),
            channel: 0,
        };
        (sound, retracted)
    }
}

impl<S: Source> Iterator for Retractable<S> {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        let channels = self.inner.channels().get();
        if self.channel == 0 && self.retracted.load(Ordering::Acquire) {
            if self.gain <= 0.0 {
                return None;
            }
            self.gain = (self.gain - self.step).max(0.0);
        }
        self.channel = (self.channel + 1) % channels;
        self.inner.next().map(|sample| sample * self.gain)
    }
}

impl<S: Source> Source for Retractable<S> {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

fn open_output() -> Result<MixerDeviceSink> {
    let mut sink = DeviceSinkBuilder::from_default_device()
        .and_then(|builder| {
            builder
                .with_buffer_size(BufferSize::Fixed(OUTPUT_BUFFER_FRAMES))
                .open_stream()
        })
        .or_else(|_| DeviceSinkBuilder::open_default_sink())
        .wrap_err("could not open the audio output")?;
    sink.log_on_drop(false);
    Ok(sink)
}

impl FeedbackOutput<MixerDeviceSink> {
    /// Opens the output and starts its idle grace without playing anything.
    fn prepare(&mut self, now: Instant) {
        let opening = Instant::now();
        let cold = self.sink.is_none();
        if let Err(error) = self.open_for_tone(now, open_output) {
            tracing::warn!(%error, "recording audio output unavailable; retrying");
        }
        if cold && self.sink.is_some() {
            tracing::debug!(
                open_ms = opening.elapsed().as_millis(),
                "opened feedback output"
            );
        }
        self.mark_playing(Instant::now(), Duration::ZERO);
    }

    /// Plays a sound, opening the output first if needed. Returns whether it
    /// reached the mixer.
    fn play(&mut self, sound: impl Source + Send + 'static, tone: Tone) -> bool {
        self.prepare(Instant::now());
        // Opening may have been slow; recheck the latest preference before
        // retaining the output or playing the queued sound.
        if !sounds_enabled() {
            self.release_when_idle(false, Instant::now());
            return false;
        }
        let Some(sink) = self.sink.as_ref() else {
            return false;
        };
        let duration = sound.total_duration().unwrap_or(Duration::ZERO);
        sink.mixer().add(sound.amplify(volume()));
        tracing::debug!(?tone, "playing feedback tone");
        self.mark_playing(Instant::now(), duration);
        true
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

/// One sound set's start, stop and cancel tones, ready for the mixer.
struct SoundSet {
    start: SamplesBuffer,
    stop: SamplesBuffer,
    cancel: SamplesBuffer,
}

impl SoundSet {
    /// Synthesizes a procedural preset's three cues.
    fn render(preset: &crate::sound_synth::Preset) -> Self {
        use crate::sound_synth::{Cue, SAMPLE_RATE, render};
        let buffer = |cue| {
            SamplesBuffer::new(
                std::num::NonZero::new(1).unwrap(),
                std::num::NonZero::new(SAMPLE_RATE).unwrap(),
                render(preset, cue),
            )
        };
        Self {
            start: buffer(Cue::Start),
            stop: buffer(Cue::Stop),
            cancel: buffer(Cue::Cancel),
        }
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

    fn leading_silence_ms(bytes: &'static [u8]) -> u128 {
        let sound = decode(bytes).unwrap();
        let channels = usize::from(sound.channels().get());
        let rate = sound.sample_rate().get() as usize;
        let samples = sound.collect::<Vec<f32>>();
        let first = samples
            .iter()
            .position(|sample| sample.abs() > 0.01)
            .unwrap_or(samples.len());
        (first / channels * 1_000 / rate) as u128
    }

    #[test]
    fn feedback_tones_sound_immediately() {
        for (name, bytes) in [
            (
                "start",
                include_bytes!("../resources/audio/startRecording.mp3").as_slice(),
            ),
            (
                "stop",
                include_bytes!("../resources/audio/stopRecording.mp3").as_slice(),
            ),
        ] {
            let silence = leading_silence_ms(bytes);
            eprintln!("{name} tone leading silence: {silence} ms");
            assert!(
                silence < 40,
                "{name} tone starts after {silence} ms of silence"
            );
        }
    }

    #[test]
    fn procedural_sound_sets_render_and_select_by_id() {
        for preset in crate::sound_synth::presets() {
            let set = SoundSet::render(preset);
            for sound in [set.start, set.stop, set.cancel] {
                let duration = sound.total_duration().unwrap();
                assert!(duration > Duration::ZERO && duration <= Duration::from_millis(250));
                let samples = sound.collect::<Vec<f32>>();
                let first = samples.iter().position(|sample| sample.abs() > 0.001);
                assert!(
                    first.is_some_and(|first| first < 48),
                    "{} starts late",
                    preset.id
                );
            }
        }
        set_sound_set("breath_ping");
        assert_eq!(SOUND_SET.load(Ordering::Relaxed), 1);
        set_sound_set("no-such-set");
        assert_eq!(SOUND_SET.load(Ordering::Relaxed), 0);
        set_sound_set(CLASSIC_SOUND_SET);
        assert_eq!(SOUND_SET.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn provisional_start_sounds_after_the_chord_grace_unless_retracted() {
        let now = Instant::now();
        let observation = Duration::from_millis(250);
        let mut start = ProvisionalStart::default();
        assert_eq!(start.wait(now, observation), observation);
        assert!(!start.is_due(now + Duration::from_secs(1)));

        // A press waits out the grace, then is due.
        start.request(now);
        assert_eq!(start.wait(now, observation), CHORD_GRACE);
        assert!(!start.is_due(now + CHORD_GRACE - Duration::from_millis(1)));
        assert!(start.is_due(now + CHORD_GRACE));
        assert_eq!(
            start.wait(now + CHORD_GRACE * 2, observation),
            Duration::ZERO
        );

        // A chord inside the grace never sounds.
        start.retract();
        assert!(!start.is_due(now + CHORD_GRACE));
        assert_eq!(start.wait(now, observation), observation);

        // A chord after the tone began fades that tone, and only once.
        start.request(now);
        let (_, sounding) = Retractable::new(SamplesBuffer::new(
            std::num::NonZero::new(1).unwrap(),
            std::num::NonZero::new(48_000).unwrap(),
            vec![1.0; 10],
        ));
        start.sounding(sounding.clone());
        assert!(!start.is_due(now + CHORD_GRACE));
        start.retract();
        assert!(sounding.load(Ordering::Acquire));

        // A later press supersedes an earlier pending one; a definitive start
        // tone cancels the provisional one instead of doubling it.
        start.request(now);
        start.request(now + Duration::from_millis(10));
        assert!(!start.is_due(now + CHORD_GRACE));
        start.cancel_pending();
        assert!(!start.is_due(now + Duration::from_secs(1)));
    }

    #[test]
    fn retracted_tone_fades_out_without_a_click_or_channel_skew() {
        let rate = 48_000_u32;
        let frames = rate as usize;
        let buffer = || {
            SamplesBuffer::new(
                std::num::NonZero::new(2).unwrap(),
                std::num::NonZero::new(rate).unwrap(),
                vec![1.0; frames * 2],
            )
        };
        let (untouched, _) = Retractable::new(buffer());
        assert_eq!(untouched.count(), frames * 2);

        let (mut sound, retracted) = Retractable::new(buffer());
        for _ in 0..101 {
            assert_eq!(sound.next(), Some(1.0));
        }
        // Retracting mid-frame finishes that frame at full gain first.
        retracted.store(true, Ordering::Release);
        assert_eq!(sound.next(), Some(1.0));
        let rest = sound.collect::<Vec<_>>();
        let fade_frames = (RETRACT_FADE.as_secs_f32() * rate as f32) as usize;
        assert_eq!(rest.len() % 2, 0, "the fade ends on a frame boundary");
        assert!(rest.len().abs_diff(fade_frames * 2) <= 4, "{}", rest.len());
        for frame in rest.chunks(2) {
            assert_eq!(frame[0], frame[1]);
        }
        assert!(rest.windows(2).all(|pair| pair[1] <= pair[0]));
        assert!(rest[0] < 1.0 && *rest.last().unwrap() < 0.01);
    }

    #[test]
    fn timed_out_admission_still_publishes_a_usable_player() {
        let player = OnceLock::new();
        let (sender, receiver) = mpsc::sync_channel(8);
        let (ready, waiting) = mpsc::sync_channel(1);
        assert!(admission_error(waiting.recv_timeout(Duration::ZERO)).is_err());
        drop(waiting);
        publish_player(&player, sender, ready, Ok(()));
        enqueue(player.get(), Request::Play(Tone::DictationStart));
        assert_eq!(
            receiver.try_recv().unwrap(),
            Request::Play(Tone::DictationStart)
        );
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

    /// Times the default output's tone path without recording it: from the
    /// request to the mixer's first pull of the tone, plus one output buffer
    /// until that sample is presented. `HEX_TONE_PATH=legacy` measures the
    /// previous path (rodio's default 50 ms buffer). Route the default output
    /// to a virtual device first so nothing sounds aloud.
    #[test]
    #[ignore = "opens the native default output; route it to a silent device or set HEX_TONE_MUTED"]
    fn native_start_tone_path_latency() {
        use std::sync::Mutex;

        struct Probe<S> {
            inner: S,
            pulled: Arc<Mutex<Option<Instant>>>,
        }
        impl<S: Source> Iterator for Probe<S> {
            type Item = rodio::Sample;
            fn next(&mut self) -> Option<Self::Item> {
                self.pulled.lock().unwrap().get_or_insert_with(Instant::now);
                self.inner.next()
            }
        }
        impl<S: Source> Source for Probe<S> {
            fn current_span_len(&self) -> Option<usize> {
                None
            }
            fn channels(&self) -> rodio::ChannelCount {
                self.inner.channels()
            }
            fn sample_rate(&self) -> rodio::SampleRate {
                self.inner.sample_rate()
            }
            fn total_duration(&self) -> Option<Duration> {
                self.inner.total_duration()
            }
        }

        let legacy = std::env::var("HEX_TONE_PATH").is_ok_and(|path| path == "legacy");
        // A muted probe times a real speaker path without sounding.
        let gain = if std::env::var_os("HEX_TONE_MUTED").is_some() {
            0.0
        } else {
            1.0
        };
        let device =
            rodio::cpal::traits::HostTrait::default_output_device(&rodio::cpal::default_host())
                .and_then(|device| rodio::DeviceTrait::description(&device).ok())
                .map(|description| description.name().to_owned());
        eprintln!("default output: {device:?}");
        let start = decode(include_bytes!("../resources/audio/startRecording.mp3")).unwrap();
        let open = || {
            if legacy {
                let mut sink = DeviceSinkBuilder::open_default_sink().unwrap();
                sink.log_on_drop(false);
                sink
            } else {
                open_output().unwrap()
            }
        };
        let measure = |sink: &mut Option<MixerDeviceSink>, label: &str| {
            let requested = Instant::now();
            let sink = sink.get_or_insert_with(open);
            let opened = requested.elapsed();
            let config = sink.config();
            let buffer = match config.buffer_size() {
                BufferSize::Fixed(frames) => Duration::from_secs_f64(
                    f64::from(*frames) / f64::from(config.sample_rate().get()),
                ),
                BufferSize::Default => Duration::ZERO,
            };
            let pulled = Arc::new(Mutex::new(None));
            sink.mixer().add(
                Probe {
                    inner: start.clone(),
                    pulled: pulled.clone(),
                }
                .amplify(gain),
            );
            while pulled.lock().unwrap().is_none() {
                thread::sleep(Duration::from_micros(200));
            }
            let pulled = pulled.lock().unwrap().unwrap();
            let latency = pulled - requested + buffer;
            eprintln!(
                "{label}: open {} ms, buffer {} ms, presented ~{} ms after request",
                opened.as_millis(),
                buffer.as_millis(),
                latency.as_millis()
            );
            thread::sleep(Duration::from_millis(1_200));
            latency
        };
        let mut sink = None;
        let cold = measure(&mut sink, "cold");
        let warm: Vec<_> = (0..5)
            .map(|_| measure(&mut sink, "warm").as_millis())
            .collect();
        eprintln!(
            "{} path: cold {} ms, warm {warm:?} ms",
            if legacy { "legacy" } else { "current" },
            cold.as_millis()
        );
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
        enqueue(Some(&sender), Request::ProvisionalStart);
        enqueue(Some(&sender), Request::RetractStart);
        enqueue(Some(&sender), Request::Play(Tone::Cancel));
        assert_eq!(receiver.try_recv().unwrap(), Request::ProvisionalStart);
        assert_eq!(receiver.try_recv().unwrap(), Request::RetractStart);
        assert!(receiver.try_recv().is_err());
        drop(receiver);
        enqueue(Some(&sender), Request::Play(Tone::Cancel));
        enqueue(None, Request::ProvisionalStart);
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

//! Where a shortcut finish's tail stands, and the poll a capture loop that
//! reads microphone chunks does while it waits for the tail's audio.

use std::time::Duration;
#[cfg(not(target_os = "macos"))]
use std::time::Instant;

use super::{
    CHUNK_GAP_TOLERANCE, PARAKEET_SAMPLE_RATE, Recording, STOP_TONE_MARGIN, TAIL_QUIET_FRAMES,
    TAIL_SPEECH_FRAMES, TAIL_SPEECH_GAP_FRAMES, VOICE_HISTORY_FRAMES,
};
use crate::audio::CaptureInstant;
use crate::dictation::DictationCapture;
#[cfg(not(target_os = "macos"))]
use crate::dictation::TAIL_DELIVERY_GRACE;
use crate::dictation::{MAX_RELEASE_TAIL, POST_RELEASE_TAIL};
use crate::transcription::pause_segmenter::FrameLevel;
use std::collections::VecDeque;

/// A release always receives fresh audio before an early finish.
pub(super) const MIN_RELEASE_TAIL: Duration = Duration::from_millis(40);
const QUIET_BEFORE_RELEASE: usize = 3; // 60 ms, longer than a syllable gap.

/// Where a shortcut finish's tail stands, from the audio recorded so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseTail {
    /// More audio is needed. `growing` once speech has carried past
    /// [`POST_RELEASE_TAIL`], so the wait is bounded by
    /// [`MAX_RELEASE_TAIL`] rather than the base tail.
    Waiting { growing: bool },
    /// Finish keeping this much audio after the release.
    Through(Duration),
}

impl ReleaseTail {
    /// The longest tail a wait in this state may still end with. Callers add
    /// [`crate::dictation::TAIL_DELIVERY_GRACE`] to bound how long they wait
    /// for the microphone stream to deliver it.
    pub fn bound(self) -> Duration {
        match self {
            Self::Waiting { growing: true } => MAX_RELEASE_TAIL,
            Self::Waiting { growing: false } => POST_RELEASE_TAIL,
            Self::Through(tail) => tail,
        }
    }
}

#[cfg(not(target_os = "macos"))]
#[derive(Clone, Copy)]
enum WaitState {
    Decided,
    Waiting { started: Instant, tone: Duration },
}

/// A shortcut release waiting for its tail, for capture loops that poll
/// between microphone chunks (Linux and Windows; the macOS audio owner
/// drains its tail directly).
#[cfg(not(target_os = "macos"))]
pub struct ReleaseWait {
    ended_at: CaptureInstant,
    state: WaitState,
}

#[cfg(not(target_os = "macos"))]
impl ReleaseWait {
    /// A release at `ended_at`. Only one that will transcribe waits; its
    /// stop tone (sounding from now) is never taken for speech.
    pub fn new(capture: &DictationCapture, ended_at: CaptureInstant) -> Self {
        let state = if capture.would_transcribe(ended_at) {
            WaitState::Waiting {
                started: Instant::now(),
                tone: crate::feedback::stop_tone_length(),
            }
        } else {
            WaitState::Decided
        };
        Self { ended_at, state }
    }

    pub fn ended_at(&self) -> CaptureInstant {
        self.ended_at
    }

    pub fn waits(&self) -> bool {
        matches!(self.state, WaitState::Waiting { .. })
    }

    /// Finishes with the audio already captured, as after a lost microphone.
    pub fn stop_waiting(&mut self) {
        self.state = WaitState::Decided;
    }

    /// The tail to finish with once it is decided, or once the wait for it
    /// is over (the base tail plus [`TAIL_DELIVERY_GRACE`], or the longest
    /// tail plus the grace while speech grows it); `None` while it is still
    /// coming.
    pub fn tail(&self, capture: &DictationCapture, now: Instant) -> Option<Duration> {
        let WaitState::Waiting { started, tone } = self.state else {
            return Some(POST_RELEASE_TAIL);
        };
        match capture.release_tail(self.ended_at, tone) {
            ReleaseTail::Through(tail) => Some(tail),
            waiting if now >= started + waiting.bound() + TAIL_DELIVERY_GRACE => {
                Some(waiting.bound())
            }
            ReleaseTail::Waiting { .. } => None,
        }
    }
}

/// Speech flags of a recording's 16 kHz audio, one per 20 ms frame, against
/// the meeting detector's adaptive noise floor. Only recent frames are kept.
#[derive(Default)]
pub(super) struct VoiceTrack {
    speech: crate::transcription::pause_segmenter::SpeechFrames,
    frame: Vec<f32>,
    /// Clip samples classified or waiting in `frame`.
    pub(super) consumed: usize,
    heard_speech: bool,
    flags: VecDeque<FrameLevel>,
    /// The frame index of `flags[0]`.
    first: usize,
}

impl VoiceTrack {
    pub(super) const FRAME: usize = crate::transcription::pause_segmenter::FRAME_SAMPLES;

    pub(super) fn extend(&mut self, mut samples: &[f32]) {
        self.consumed += samples.len();
        while !samples.is_empty() {
            let take = samples.len().min(Self::FRAME - self.frame.len());
            self.frame.extend_from_slice(&samples[..take]);
            samples = &samples[take..];
            if self.frame.len() == Self::FRAME {
                let mut level = self.speech.classify(&self.frame);
                // Invalid input is unknown, never evidence for an early stop.
                level.quiet &= self.frame.iter().all(|sample| sample.is_finite());
                self.heard_speech |= level.speech;
                self.flags.push_back(level);
                self.frame.clear();
                if self.flags.len() > VOICE_HISTORY_FRAMES {
                    self.flags.pop_front();
                    self.first += 1;
                }
            }
        }
    }

    /// Frames classified so far.
    fn frames(&self) -> usize {
        self.first + self.flags.len()
    }

    fn is_speech(&self, frame: usize) -> bool {
        frame
            .checked_sub(self.first)
            .and_then(|index| self.flags.get(index))
            .is_some_and(|level| level.speech)
    }

    /// Unknown or evicted frames are not quiet.
    fn is_quiet(&self, frame: usize) -> bool {
        frame
            .checked_sub(self.first)
            .and_then(|index| self.flags.get(index))
            .is_some_and(|level| level.quiet)
    }

    /// Whether speech runs up to frame `end` (exclusive): its last speech
    /// frame is within a syllable gap of `end`, and at least
    /// [`TAIL_SPEECH_FRAMES`] speech frames, none before `from`, lead up to
    /// it with no gap longer than a syllable's.
    fn talking_until(&self, end: usize, from: usize) -> bool {
        let mut speech = 0;
        let mut gap = 0;
        let mut frame = end;
        while frame > from.max(self.first) {
            frame -= 1;
            if self.is_speech(frame) {
                speech += 1;
                gap = 0;
                if speech >= TAIL_SPEECH_FRAMES {
                    return true;
                }
            } else {
                gap += 1;
                if gap > TAIL_SPEECH_GAP_FRAMES {
                    return false;
                }
            }
        }
        false
    }

    /// The first frame at or after `from`, before `end`, that ends
    /// [`TAIL_QUIET_FRAMES`] quiet frames in a row.
    fn quiet_end(&self, from: usize, end: usize) -> Option<usize> {
        let mut quiet = 0;
        for frame in from..end {
            if self.is_speech(frame) {
                quiet = 0;
            } else {
                quiet += 1;
                if quiet == TAIL_QUIET_FRAMES {
                    return Some(frame + 1);
                }
            }
        }
        None
    }
}

impl DictationCapture {
    /// The tail a shortcut finish at `ended_at` keeps, judged on the audio
    /// recorded so far: 40–200 ms after confirmed quiet, or, while speech carries on
    /// past it, until [`TAIL_QUIET_FRAMES`] of quiet or [`MAX_RELEASE_TAIL`].
    /// Only sustained speech grows it, never a key click, and never the
    /// stop tone of `tone` length that sounds from the release: speech must
    /// also be heard after the tone could have reached the microphone.
    pub fn release_tail(&self, ended_at: CaptureInstant, tone: Duration) -> ReleaseTail {
        let Some(recording) = &self.recording else {
            return ReleaseTail::Through(POST_RELEASE_TAIL);
        };
        let through = recording.recorded_through.unwrap_or(ended_at);
        if let Some(tail) = recording.quiet_release_tail(ended_at, through) {
            return ReleaseTail::Through(tail);
        }
        if through < ended_at + POST_RELEASE_TAIL {
            return ReleaseTail::Waiting { growing: false };
        }
        let voice = &recording.voice;
        let heard = voice.frames();
        let capped = through >= ended_at + MAX_RELEASE_TAIL;
        let base = recording.frame_at(ended_at + POST_RELEASE_TAIL).min(heard);
        if !voice.talking_until(base, 0) {
            return ReleaseTail::Through(POST_RELEASE_TAIL);
        }
        // What runs into the base tail's end may be the tone; speech must
        // carry on once it is over.
        let after_tone = if tone.is_zero() {
            0
        } else {
            recording.frame_at(ended_at + tone + STOP_TONE_MARGIN)
        };
        let decision = base.max(after_tone + TAIL_SPEECH_FRAMES);
        if decision > recording.frame_at(ended_at + MAX_RELEASE_TAIL) {
            return ReleaseTail::Through(POST_RELEASE_TAIL);
        }
        if heard < decision {
            return if capped {
                ReleaseTail::Through(POST_RELEASE_TAIL)
            } else {
                ReleaseTail::Waiting { growing: true }
            };
        }
        if !voice.talking_until(decision, after_tone) {
            return ReleaseTail::Through(POST_RELEASE_TAIL);
        }
        let tail = |frame| {
            recording
                .after(ended_at, frame)
                .clamp(POST_RELEASE_TAIL, MAX_RELEASE_TAIL)
        };
        match voice.quiet_end(decision, heard) {
            Some(end) => ReleaseTail::Through(tail(end)),
            None if capped => ReleaseTail::Through(MAX_RELEASE_TAIL),
            None => ReleaseTail::Waiting { growing: true },
        }
    }
}

impl Recording {
    /// Credit only complete quiet frames before key-up, then verify every
    /// frame through the shortened deadline. Renewed or ambiguous energy
    /// keeps the original tail, including its existing speech extension.
    fn quiet_release_tail(
        &self,
        ended_at: CaptureInstant,
        through: CaptureInstant,
    ) -> Option<Duration> {
        // A missing frame (including zero-filled capture gaps) is not
        // evidence of quiet. A tone or restored output in the fresh audio
        // also prevents shortening because every frame must be quiet.
        if !self.voice.heard_speech || self.max_chunk_gap > CHUNK_GAP_TOLERANCE {
            return None;
        }
        let release = self.frame_at(ended_at);
        let quiet = (self.voice.first..release)
            .rev()
            .take(10) // At most the 200 ms base tail.
            .take_while(|&frame| self.voice.is_quiet(frame))
            .count();
        if quiet < QUIET_BEFORE_RELEASE {
            return None;
        }
        let silence = Duration::from_secs_f64(
            (quiet * VoiceTrack::FRAME) as f64 / f64::from(PARAKEET_SAMPLE_RATE),
        );
        let tail = POST_RELEASE_TAIL
            .saturating_sub(silence)
            .max(MIN_RELEASE_TAIL);
        if through < ended_at + tail {
            return None;
        }
        // Include the frame containing the deadline, not just the ones
        // before it. This covers a partial release frame and resampler lag.
        let end = self.frame_at(ended_at + tail) + 1;
        if end > self.voice.frames() || !(release..end).all(|frame| self.voice.is_quiet(frame)) {
            return None;
        }
        Some(tail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet_recording() -> Recording {
        let mut recording = Recording::new(CaptureInstant::ZERO, PARAKEET_SAMPLE_RATE);
        let mut samples = vec![0.0; 1_600];
        samples.extend((0..8_000).map(|index| {
            0.3 * (2.0 * std::f32::consts::PI * 180.0 * index as f32 / 16_000.0).sin()
        }));
        samples.resize(17_600, 0.0);
        recording.push_through(
            &samples,
            CaptureInstant::ZERO + Duration::from_millis(1_100),
        );
        recording
    }

    #[test]
    fn missing_or_invalid_audio_is_never_silence_credit() {
        let release = CaptureInstant::ZERO + Duration::from_secs(1);
        let through = release + Duration::from_millis(100);
        let mut recording = quiet_recording();
        assert_eq!(
            recording.quiet_release_tail(release, through),
            Some(MIN_RELEASE_TAIL)
        );
        recording.max_chunk_gap = Duration::from_millis(100);
        assert_eq!(recording.quiet_release_tail(release, through), None);
        recording.max_chunk_gap = Duration::ZERO;
        // Timestamp delivery is not enough: resampled frames must exist.
        let future = through + Duration::from_millis(100);
        assert_eq!(
            recording.quiet_release_tail(future, future + MIN_RELEASE_TAIL),
            None
        );
        recording.push_through(&[f32::NAN; 320], through + Duration::from_millis(20));
        assert!(!recording.voice.is_quiet(recording.voice.frames() - 1));
        // The release frame itself is invalid, so it cannot be skipped.
        assert_eq!(
            recording.quiet_release_tail(through, through + Duration::from_millis(60)),
            None
        );
    }

    #[test]
    fn evicted_history_is_unknown_and_tracking_stays_bounded() {
        let mut recording = quiet_recording();
        recording.push_through(
            &[0.0; 64_000],
            CaptureInstant::ZERO + Duration::from_millis(5_100),
        );
        assert_eq!(recording.voice.flags.len(), VOICE_HISTORY_FRAMES);
        assert_eq!(
            recording.quiet_release_tail(
                CaptureInstant::ZERO + Duration::from_secs(1),
                CaptureInstant::ZERO + Duration::from_millis(5_100),
            ),
            None
        );
    }
}

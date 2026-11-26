//! Labs › Always listening: the open microphone starts a dictation when it
//! hears speech and finishes it after the chosen pause, with no shortcut.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::app_settings::AppSettings;
use crate::feedback::{self, Tone};
use crate::linux::input::ShortcutCommand;
use crate::transcription::pause_segmenter::SpeechFrames;

/// One speech flag per this much audio.
const FRAME: Duration = Duration::from_millis(20);
/// The newest frames speech is looked for in: 400 ms.
const WINDOW_FRAMES: usize = 20;
/// Speech frames in the window that mean someone is speaking: 160 ms, more
/// than a key click gives.
const SPEECH_FRAMES: usize = 8;
/// Audio a capture keeps from before its speech was heard: the window, and
/// room for a soft first syllable.
const LEAD: Duration = Duration::from_millis(700);
/// How long past one of Hex's own tones the microphone may still hear it
/// from the speakers.
const TONE_MARGIN: Duration = Duration::from_millis(300);
/// A start the state machine never answered (Settings is recording a
/// shortcut) is asked for again after this long.
const REQUEST_EXPIRY: Duration = Duration::from_secs(2);

/// Whether the microphone is released while idle: never while the lab
/// listens, since it hears nothing through a closed microphone.
pub(super) fn release_while_idle(settings: &AppSettings) -> bool {
    settings.release_microphone_while_idle && !settings.labs.always_listening
}

/// Whether someone is speaking, from the microphone's audio alone.
#[derive(Default)]
struct Detector {
    speech: SpeechFrames,
    frame: Vec<f32>,
    /// The newest [`WINDOW_FRAMES`] speech flags.
    recent: VecDeque<bool>,
    /// Frames since the last speech frame of someone speaking.
    quiet_frames: u32,
    /// Frames still ignored while one of Hex's own tones plays.
    deaf_frames: u32,
}

impl Detector {
    fn push(&mut self, mut samples: &[f32], sample_rate: u32) {
        let frame_samples = (sample_rate / 50) as usize;
        while !samples.is_empty() {
            let take = samples
                .len()
                .min(frame_samples.saturating_sub(self.frame.len()));
            self.frame.extend_from_slice(&samples[..take]);
            samples = &samples[take..];
            if self.frame.len() >= frame_samples {
                let speech = self.speech.is_speech(&self.frame) && self.deaf_frames == 0;
                self.frame.clear();
                self.deaf_frames = self.deaf_frames.saturating_sub(1);
                self.recent.push_back(speech);
                if self.recent.len() > WINDOW_FRAMES {
                    self.recent.pop_front();
                }
                if speech && self.speaking() {
                    self.quiet_frames = 0;
                } else {
                    self.quiet_frames = self.quiet_frames.saturating_add(1);
                }
            }
        }
    }

    fn speaking(&self) -> bool {
        self.recent.iter().filter(|speech| **speech).count() >= SPEECH_FRAMES
    }

    fn quiet_for(&self) -> Duration {
        FRAME * self.quiet_frames
    }

    fn deafen(&mut self, length: Duration) {
        self.deaf_frames = (length.as_millis() / FRAME.as_millis()) as u32;
    }
}

/// What the microphone's audio asks of the shortcut state machine.
#[derive(Default)]
pub(super) struct Listening {
    detector: Detector,
    /// When speech asked for a capture that has not started yet.
    requested: Option<Instant>,
    /// Speech started the capture in progress, so a pause finishes it.
    owned: bool,
    /// The dictation shortcut paused listening.
    paused: bool,
}

impl Listening {
    /// Hears one chunk of the microphone. `busy` is whether a capture
    /// records, finishes, or waits to start. The command it returns goes to
    /// the state machine as a `hex dictation` command would.
    pub(super) fn chunk(
        &mut self,
        samples: &[f32],
        sample_rate: u32,
        busy: bool,
    ) -> Option<ShortcutCommand> {
        let Some(pause) = crate::labs::always_listening() else {
            *self = Self::default();
            return None;
        };
        self.hear(samples, sample_rate, busy, pause)
    }

    fn hear(
        &mut self,
        samples: &[f32],
        sample_rate: u32,
        busy: bool,
        pause: Duration,
    ) -> Option<ShortcutCommand> {
        self.detector.push(samples, sample_rate);
        if self.owned {
            if busy && self.detector.quiet_for() < pause {
                return None;
            }
            // The capture ends here, or its shortcut already ended it: its
            // stop tone must not start the next one.
            self.owned = false;
            self.detector
                .deafen(feedback::stop_tone_length() + TONE_MARGIN);
            return busy.then_some(ShortcutCommand::DictationStop);
        }
        if self
            .requested
            .is_some_and(|at| at.elapsed() >= REQUEST_EXPIRY)
        {
            self.requested = None;
        }
        if busy || self.paused || self.requested.is_some() || !self.detector.speaking() {
            return None;
        }
        self.requested = Some(Instant::now());
        Some(ShortcutCommand::DictationStart)
    }

    /// A capture starts. When speech asked for it, this is the pre-roll that
    /// reaches back to before the speech was heard.
    pub(super) fn started(&mut self) -> Option<Duration> {
        let requested = self.requested.take()?;
        self.owned = true;
        Some(requested.elapsed() + LEAD)
    }

    /// The dictation shortcut, pressed while nothing records, pauses
    /// listening, and resumes it.
    pub(super) fn toggle(&mut self) {
        self.paused = !self.paused;
        tracing::info!(paused = self.paused, "always listening toggled");
        if self.paused {
            feedback::play(Tone::Cancel);
        } else {
            feedback::play(Tone::DictationStart);
            self.detector
                .deafen(feedback::start_tone_length() + TONE_MARGIN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;
    const PAUSE: Duration = Duration::from_secs(2);

    fn quiet(length: Duration) -> Vec<f32> {
        vec![0.0; (RATE as usize) * (length.as_millis() as usize) / 1_000]
    }

    fn voice(length: Duration) -> Vec<f32> {
        let mut samples = quiet(length);
        for (index, sample) in samples.iter_mut().enumerate() {
            *sample = 0.1 * (index as f32 * 0.05).sin();
        }
        samples
    }

    #[test]
    fn speech_starts_a_dictation_and_a_pause_finishes_it() {
        let mut listening = Listening::default();
        let hear = |listening: &mut Listening, samples: &[f32], busy: bool| {
            listening.hear(samples, RATE, busy, PAUSE)
        };
        assert_eq!(hear(&mut listening, &quiet(PAUSE), false), None);
        // A key click is too short to be someone speaking.
        assert_eq!(
            hear(&mut listening, &voice(Duration::from_millis(60)), false),
            None
        );
        assert_eq!(hear(&mut listening, &quiet(PAUSE), false), None);
        assert_eq!(
            hear(&mut listening, &voice(Duration::from_millis(200)), false),
            Some(ShortcutCommand::DictationStart)
        );
        // Asked once, however long the state machine takes to answer.
        assert_eq!(
            hear(&mut listening, &voice(Duration::from_millis(200)), false),
            None
        );
        assert!(listening.started().is_some_and(|pre_roll| pre_roll >= LEAD));

        // A pause shorter than the setting is part of the dictation.
        assert_eq!(
            hear(&mut listening, &quiet(Duration::from_millis(1_500)), true),
            None
        );
        assert_eq!(
            hear(&mut listening, &voice(Duration::from_millis(200)), true),
            None
        );
        assert_eq!(
            hear(&mut listening, &quiet(Duration::from_millis(1_900)), true),
            None
        );
        assert_eq!(
            hear(&mut listening, &quiet(Duration::from_millis(200)), true),
            Some(ShortcutCommand::DictationStop)
        );
        assert_eq!(hear(&mut listening, &quiet(PAUSE), true), None);
    }

    #[test]
    fn paused_listening_starts_nothing_until_it_resumes() {
        let mut listening = Listening {
            paused: true,
            ..Default::default()
        };
        let speech = voice(Duration::from_millis(200));
        assert_eq!(listening.hear(&quiet(PAUSE), RATE, false, PAUSE), None);
        assert_eq!(listening.hear(&speech, RATE, false, PAUSE), None);
        listening.paused = false;
        assert_eq!(
            listening.hear(&speech, RATE, false, PAUSE),
            Some(ShortcutCommand::DictationStart)
        );
    }

    #[test]
    fn a_deafened_detector_does_not_hear_a_tone() {
        let mut detector = Detector::default();
        detector.deafen(Duration::from_millis(600));
        detector.push(&voice(Duration::from_millis(500)), RATE);
        assert!(!detector.speaking());
        detector.push(&quiet(Duration::from_millis(500)), RATE);
        detector.push(&voice(Duration::from_millis(200)), RATE);
        assert!(detector.speaking());
    }

    #[test]
    fn the_lab_keeps_a_released_microphone_open() {
        let mut settings = AppSettings {
            release_microphone_while_idle: true,
            ..Default::default()
        };
        assert!(release_while_idle(&settings));
        settings.labs.always_listening = true;
        assert!(!release_while_idle(&settings));
    }
}

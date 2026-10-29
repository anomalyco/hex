//! Sound schedule for `"Prototypes/Onboarding/First run"`:
//!
//! - Auditionable typing key-sound candidates ([`KeySoundStyle`]: `Taptic tick`
//!   default, `Muted felt`, `Soft warm`, and `Silent typing`), never repeating
//!   an adjacent sample.
//! - Tactile shortcut keycap press and release (`⌥` down / up).
//! - Hex's real `#292` `DictationStart`, `DictationStop`, `DictationDone`, and
//!   `OnboardingSuccess` tones played through [`crate::feedback`], respecting
//!   the user's sound set and feedback volume (`volume == 0` is silent;
//!   `reduced_motion` never suppresses sound).
//! - Quiet motion landing cues: the hexagon mark, permission rows ticking to
//!   `Allowed`, the Accessibility helper detaching and docking beside System
//!   Settings, and `"Almost ready"`.
//! - Optional offline WAV export (`HEX_ONBOARDING_AUDIO_OUT`) so headless
//!   `--video` renders produce a frame-accurate 48 kHz soundtrack without ever
//!   opening an audio output device.

use std::path::Path;

use super::fixture::Take;
use super::hud_stage::DONE_DELAY;
use super::{permissions, typing};
use crate::feedback::{self, KeySoundStyle, OnboardingTone, SoundSet, Tone};

#[derive(Default)]
pub(crate) struct SfxTrack {
    pub(crate) style: KeySoundStyle,
    cues: Vec<(f64, Tone)>,
    fired: usize,
    almost_ready_scheduled: bool,
    flushed: bool,
}

impl SfxTrack {
    pub(crate) fn clear(&mut self) {
        self.cues.clear();
        self.fired = 0;
        self.almost_ready_scheduled = false;
        self.flushed = false;
    }

    pub(crate) fn has_pending(&self, now: f64) -> bool {
        self.cues
            .get(self.fired)
            .is_some_and(|&(at, _)| at <= now + 1.0)
    }

    /// Switches the active typing sound candidate (including any unfired
    /// strokes already queued) and plays a short 5-stroke typing cadence so
    /// the candidate can be heard immediately on click.
    pub(crate) fn set_style_and_preview(&mut self, style: KeySoundStyle, now: f64) {
        self.style = style;
        for (_, tone) in &mut self.cues[self.fired..] {
            *tone = match *tone {
                Tone::Onboarding(OnboardingTone::Key(_, v)) => {
                    Tone::Onboarding(OnboardingTone::Key(style, v))
                }
                Tone::Onboarding(OnboardingTone::KeySpace(_)) => {
                    Tone::Onboarding(OnboardingTone::KeySpace(style))
                }
                Tone::Onboarding(OnboardingTone::KeyBackspace(_)) => {
                    Tone::Onboarding(OnboardingTone::KeyBackspace(style))
                }
                other => other,
            };
        }
        if style == KeySoundStyle::Silent {
            return;
        }
        for (dt, cue) in [
            (0.005, OnboardingTone::Key(style, 0)),
            (0.082, OnboardingTone::Key(style, 2)),
            (0.152, OnboardingTone::Key(style, 4)),
            (0.228, OnboardingTone::Key(style, 1)),
            (0.312, OnboardingTone::KeySpace(style)),
        ] {
            self.push(now + dt, Tone::Onboarding(cue));
        }
    }

    fn push(&mut self, at: f64, tone: Tone) {
        self.cues.push((at, tone));
        let start = self.fired.min(self.cues.len());
        self.cues[start..]
            .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    }

    /// Initial `"Setting up Hex"` hexagon bloom.
    pub(crate) fn on_download_start(&mut self, now: f64) {
        self.push(now + 0.08, Tone::Onboarding(OnboardingTone::Hexagon));
    }

    /// `"Without Hex"`: hexagon settling into top pill + every typing stroke.
    pub(crate) fn on_without_hex(&mut self, now: f64, typing_from: f64, had_model: bool) {
        if !had_model {
            self.push(now + 0.12, Tone::Onboarding(OnboardingTone::Hexagon));
        }
        let style = self.style;
        let mut prev_variant: Option<u8> = None;
        for (i, stroke) in typing::all_strokes().iter().enumerate() {
            let at = typing_from + stroke.at;
            let tone = match stroke.key {
                "space" | "return" => Tone::Onboarding(OnboardingTone::KeySpace(style)),
                "delete" => Tone::Onboarding(OnboardingTone::KeyBackspace(style)),
                key => {
                    let hash = key
                        .bytes()
                        .fold(0usize, |acc, b| acc.wrapping_mul(31) + usize::from(b));
                    let mut v = ((i * 5 + hash) % 6) as u8;
                    if prev_variant == Some(v) {
                        v = (v + 1) % 6;
                    }
                    prev_variant = Some(v);
                    Tone::Onboarding(OnboardingTone::Key(style, v))
                }
            };
            self.push(at, tone);
        }
    }

    /// `"With Hex"` or scripted `"Your turn"` dictation: physical shortcut
    /// down, real `DictationStart`, then shortcut up, `DictationStop`, and
    /// `DictationDone`.
    pub(crate) fn on_voice_take(&mut self, start: f64, take: &Take) {
        self.push(start, Tone::Onboarding(OnboardingTone::ShortcutDown));
        self.push(start + 0.032, Tone::DictationStart);
        let release = start + take.release;
        self.push(release, Tone::Onboarding(OnboardingTone::ShortcutUp));
        self.push(release + 0.024, Tone::DictationStop);
        self.push(release + DONE_DELAY, Tone::DictationDone);
    }

    pub(crate) fn on_live_shortcut_down(&mut self, now: f64) {
        self.push(now, Tone::Onboarding(OnboardingTone::ShortcutDown));
        self.push(now + 0.028, Tone::DictationStart);
    }

    pub(crate) fn on_live_shortcut_up(&mut self, now: f64) {
        self.push(now, Tone::Onboarding(OnboardingTone::ShortcutUp));
        self.push(now + 0.020, Tone::DictationStop);
    }

    pub(crate) fn on_live_dictation_landed(&mut self, now: f64) {
        self.push(now, Tone::DictationDone);
    }

    /// Permissions choreography: Microphone tick, Accessibility helper panel
    /// escaping & docking, then Accessibility tick.
    pub(crate) fn on_permissions(&mut self, from: f64) {
        self.push(
            from + permissions::MIC_GRANT,
            Tone::Onboarding(OnboardingTone::PermissionTick),
        );
        self.push(
            from + permissions::AX_ESCAPE + 0.18,
            Tone::Onboarding(OnboardingTone::PanelEscape),
        );
        self.push(
            from + permissions::AX_GRANT,
            Tone::Onboarding(OnboardingTone::PermissionTick),
        );
    }

    /// `"Almost ready"` waiting screen (fires at most once per run).
    pub(crate) fn on_almost_ready(&mut self, now: f64) {
        if !self.almost_ready_scheduled {
            self.almost_ready_scheduled = true;
            self.push(now + 0.06, Tone::Onboarding(OnboardingTone::AlmostReady));
        }
    }

    /// `"Hex is ready"` finale: hexagon arrival + Hex's `#292` success motif.
    pub(crate) fn on_ready(&mut self, now: f64) {
        self.push(now, Tone::Onboarding(OnboardingTone::Hexagon));
        self.push(now + 0.12, Tone::OnboardingSuccess);
    }

    /// Interactive capsule tap (language or shortcut toggle).
    pub(crate) fn on_capsule_tap(&self) {
        feedback::play(Tone::Onboarding(OnboardingTone::ShortcutDown));
    }

    /// Advances playback to `now`, dispatching every newly-reached cue to
    /// [`feedback::play`] (no-op when headless or volume is 0).
    pub(crate) fn advance_to(&mut self, now: f64) {
        while let Some(&(at, tone)) = self.cues.get(self.fired) {
            if at > now + 1e-4 {
                break;
            }
            self.fired += 1;
            feedback::play(tone);
        }
    }

    pub(crate) fn fired_cues(&self) -> &[(f64, Tone)] {
        &self.cues[..self.fired]
    }

    /// Writes the fired soundtrack offline when `HEX_ONBOARDING_AUDIO_OUT` is set.
    pub(crate) fn flush_offline_if_requested(&mut self, fallback_secs: f64) {
        if self.flushed {
            return;
        }
        let Ok(path) = std::env::var("HEX_ONBOARDING_AUDIO_OUT") else {
            return;
        };
        if path.is_empty() || self.fired == 0 {
            return;
        }
        self.flushed = true;
        let duration_secs = std::env::var("HEX_ONBOARDING_AUDIO_SECS")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(fallback_secs)
            .max(fallback_secs);
        let gain = std::env::var("HEX_ONBOARDING_AUDIO_GAIN")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.88)
            .clamp(0.0, 1.0);
        if let Err(error) = feedback::mix_offline_wav(
            Path::new(&path),
            self.fired_cues(),
            SoundSet::Signature,
            gain,
            duration_secs,
        ) {
            tracing::warn!(%error, path, "could not write offline onboarding audio");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixture::ANSWER_JAY;
    use super::*;

    #[test]
    fn typing_clacks_never_repeat_an_adjacent_key_sample() {
        let mut track = SfxTrack::default();
        track.on_without_hex(0.0, 0.5, false);
        track.advance_to(20.0);

        let mut last_key: Option<u8> = None;
        let mut key_count = 0;
        let mut space_count = 0;
        let mut back_count = 0;
        for &(_, tone) in track.fired_cues() {
            match tone {
                Tone::Onboarding(OnboardingTone::Key(_, v)) => {
                    assert_ne!(last_key, Some(v), "adjacent key clacks repeated Key({v})");
                    last_key = Some(v);
                    key_count += 1;
                }
                Tone::Onboarding(OnboardingTone::KeySpace(_)) => {
                    last_key = None;
                    space_count += 1;
                }
                Tone::Onboarding(OnboardingTone::KeyBackspace(_)) => {
                    last_key = None;
                    back_count += 1;
                }
                _ => {}
            }
        }
        assert!(key_count >= 18, "{key_count}");
        assert!(space_count >= 4, "{space_count}");
        assert_eq!(back_count, 3);
    }

    #[test]
    fn voice_and_permission_cues_include_real_dictation_tones_and_motion_beats() {
        let mut track = SfxTrack::default();
        track.on_download_start(0.0);
        track.on_voice_take(10.0, &ANSWER_JAY);
        track.on_permissions(14.0);
        track.on_almost_ready(21.5);
        track.on_ready(28.0);
        track.advance_to(30.0);

        let tones: Vec<Tone> = track.fired_cues().iter().map(|&(_, t)| t).collect();
        assert!(tones.contains(&Tone::Onboarding(OnboardingTone::ShortcutDown)));
        assert!(tones.contains(&Tone::DictationStart));
        assert!(tones.contains(&Tone::Onboarding(OnboardingTone::ShortcutUp)));
        assert!(tones.contains(&Tone::DictationStop));
        assert!(tones.contains(&Tone::DictationDone));
        assert!(tones.contains(&Tone::Onboarding(OnboardingTone::PanelEscape)));
        assert!(tones.contains(&Tone::Onboarding(OnboardingTone::PermissionTick)));
        assert!(tones.contains(&Tone::Onboarding(OnboardingTone::AlmostReady)));
        assert!(tones.contains(&Tone::OnboardingSuccess));
    }
}

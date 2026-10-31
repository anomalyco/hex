//! Sound schedule for `"Prototypes/Onboarding/First run"`:
//!
//! - Auditionable typing key-sound candidates ([`KeySoundStyle`]: `Mechanical
//!   clack` default, `Taptic tick`, `Muted felt`, `Soft warm`, and `Silent
//!   typing`), never repeating an adjacent sample, each press at its own
//!   pitch and level.
//! - Tactile shortcut keycap press and release (`⌥` down / up).
//! - Hex's real `#292` `DictationStart`, `DictationStop`, and `DictationDone`
//!   tones and the ready logo's three rising notes, played through
//!   [`crate::feedback`], respecting
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
use super::hex_meter::BUILD_AT;
use super::hud_stage::DONE_DELAY;
use super::{permissions, scene_script, triumph, typing};
use crate::feedback::{
    self, CLACK_KEYS, KeySoundStyle, OnboardingTone, SoundSet, TRIUMPHS, Tapback, Tone,
};

/// The heartbeat's level, in percent, calm and in full panic.
const HEART_QUIET: f32 = 30.0;
const HEART_LOUD: f32 = 75.0;

/// The triumph candidate a render plays, `HEX_ONBOARDING_TRIUMPH=1..3`
/// (the first by default) until one is chosen.
fn triumph_take() -> u8 {
    std::env::var("HEX_ONBOARDING_TRIUMPH")
        .ok()
        .and_then(|v| v.parse::<u8>().ok())
        .filter(|n| (1..=TRIUMPHS).contains(n))
        .map_or(0, |n| n - 1)
}

/// Whether the heartbeat plays under the panic; `HEX_ONBOARDING_HEARTBEAT=off`
/// renders the comedy without it to compare.
fn heartbeat_on() -> bool {
    std::env::var("HEX_ONBOARDING_HEARTBEAT").map_or(true, |v| v != "off")
}

/// A fixed scramble of `seed`, so each press's pitch and level vary the same
/// way on every run.
fn scramble(seed: usize) -> usize {
    let mut x = seed.wrapping_mul(0x9E37_79B9).wrapping_add(0x7F4A_7C15);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^ (x >> 12)
}

/// The sound of the tapback drawn as `glyph`.
fn tapback(glyph: typing::Glyph) -> Tone {
    use typing::Glyph;
    Tone::Onboarding(OnboardingTone::Tapback(match glyph {
        Glyph::Heart => Tapback::Heart,
        Glyph::ThumbsUp => Tapback::ThumbsUp,
        Glyph::ThumbsDown => Tapback::ThumbsDown,
        Glyph::Haha => Tapback::Haha,
        Glyph::Bang => Tapback::Exclamation,
        Glyph::Question => Tapback::Question,
    }))
}

#[derive(Default)]
pub(crate) struct SfxTrack {
    pub(crate) style: KeySoundStyle,
    /// Each cue's time, tone, and percent of its level.
    cues: Vec<(f64, Tone, u8)>,
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
            .is_some_and(|&(at, ..)| at <= now + 1.0)
    }

    /// Switches the active typing sound candidate (including any unfired
    /// strokes already queued) and plays a short 5-stroke typing cadence so
    /// the candidate can be heard immediately on click.
    pub(crate) fn set_style_and_preview(&mut self, style: KeySoundStyle, now: f64) {
        self.style = style;
        for (_, tone, _) in &mut self.cues[self.fired..] {
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
        self.push_at(at, tone, 100);
    }

    fn push_at(&mut self, at: f64, tone: Tone, percent: u8) {
        self.cues.push((at, tone, percent));
        let start = self.fired.min(self.cues.len());
        self.cues[start..]
            .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    }

    /// Initial `"Setting up Hex"` hexagon bloom.
    pub(crate) fn on_download_start(&mut self, now: f64) {
        self.push(now + 0.08, Tone::Onboarding(OnboardingTone::Hexagon));
    }

    /// `"Without Hex"` waiting to start: the hexagon settles into the top
    /// pill.
    pub(crate) fn on_parked(&mut self, now: f64, had_model: bool) {
        if !had_model {
            self.push(now + 0.12, Tone::Onboarding(OnboardingTone::Hexagon));
        }
    }

    /// `"Without Hex"` from `from`: every keystroke, harder as panic builds
    /// (a space-bar thud under the tap), a soft send, Jay's messages, a
    /// deadpan tick on `"Read"`, and the selected set's error on removal.
    /// The keys are always the mechanical clack, whatever the reader picked
    /// for their own typing: this is someone else's keyboard.
    pub(crate) fn on_without_hex(&mut self, from: f64, scene: &typing::Scene) {
        let style = KeySoundStyle::Clack;
        let mut prev_variant: Option<u8> = None;
        let mut prev_at = None;
        for (i, stroke) in scene.strokes().iter().enumerate() {
            let at = from + stroke.at;
            // Autocorrect changes the draft without a key, and keys mashed
            // together (an elbow) land as one.
            if stroke.key.is_empty() || prev_at == Some(stroke.at) {
                continue;
            }
            prev_at = Some(stroke.at);
            // Calm typing sits under the chat sounds; panic brings it up,
            // and no two presses land quite alike.
            let wobble = (scramble(i) % 17) as f64 - 8.0;
            let level = (62.0 + 28.0 * stroke.panic + wobble)
                .round()
                .clamp(0.0, 100.0) as u8;
            let tone = match stroke.key {
                "space" | "return" => OnboardingTone::KeySpace(style),
                "delete" => OnboardingTone::KeyBackspace(style),
                key => {
                    let hash = key
                        .bytes()
                        .fold(0usize, |acc, b| acc.wrapping_mul(31) + usize::from(b));
                    let presses = if style == KeySoundStyle::Clack {
                        CLACK_KEYS
                    } else {
                        6
                    };
                    let mut v = ((i * 5 + hash) % usize::from(presses)) as u8;
                    if prev_variant == Some(v) {
                        v = (v + 1) % presses;
                    }
                    prev_variant = Some(v);
                    if style == KeySoundStyle::Clack {
                        v += CLACK_KEYS * (scramble(i + hash) % 3) as u8;
                    }
                    OnboardingTone::Key(style, v)
                }
            };
            self.push_at(at, Tone::Onboarding(tone), level);
            // The finger slipping onto Return lands with a heavy clunk.
            if stroke.key == "return" && scene.flash(stroke.at + 1e-3, "return") > 0.0 {
                self.push(
                    at + 0.004,
                    Tone::Onboarding(OnboardingTone::KeyBackspace(style)),
                );
            }
            if stroke.hard && matches!(tone, OnboardingTone::Key(..)) {
                self.push_at(
                    at + 0.004,
                    Tone::Onboarding(OnboardingTone::KeySpace(style)),
                    level,
                );
            }
        }
        // Lines from before the button were already there.
        for line in scene.lines().iter().filter(|l| l.at >= 0.0) {
            let at = from + line.at;
            match line.who {
                typing::Who::Them(_) | typing::Who::Status => {
                    self.push(at, Tone::Onboarding(OnboardingTone::Incoming))
                }
                typing::Who::You => self.push(at + 0.03, Tone::Onboarding(OnboardingTone::Sent)),
                typing::Who::System => self.push(at, Tone::Error),
            }
        }
        for at in scene.reads() {
            self.push(from + at, Tone::Onboarding(OnboardingTone::Read));
        }
        for tap in scene.all_taps() {
            self.push(from + tap.at, tapback(tap.glyph));
        }
        // The heartbeat rises under the clicks and stops dead at the removal.
        let beats = if heartbeat_on() {
            scene.heartbeats()
        } else {
            Vec::new()
        };
        for beat in beats {
            let tone = if beat.slow {
                OnboardingTone::HeartbeatSlow
            } else {
                OnboardingTone::Heartbeat
            };
            let level = (HEART_QUIET + (HEART_LOUD - HEART_QUIET) * beat.level).round() as u8;
            self.push_at(from + beat.at, Tone::Onboarding(tone), level);
        }
        for &(start, _) in scene.slows() {
            self.push(from + start, Tone::Onboarding(OnboardingTone::SlowDown));
        }
        for at in scene.slams() {
            self.push(from + at, Tone::Onboarding(OnboardingTone::Slam));
        }
    }

    /// The story winding back into `"With Hex"`.
    pub(crate) fn on_rewind(&mut self, now: f64) {
        self.push(now, Tone::Onboarding(OnboardingTone::Rewind));
    }

    /// `"With Hex"`'s payoff from the shortcut at `from`: the answer
    /// sending, each tapback and reply, and a triumphant sting under the
    /// first celebration, each with its own sound.
    pub(crate) fn on_hex_reply(&mut self, from: f64, beats: &triumph::HexBeats) {
        self.push(from + beats.send, Tone::Onboarding(OnboardingTone::Sent));
        for &(at, glyph) in &beats.taps {
            self.push(from + at, tapback(glyph));
        }
        for (at, said) in &beats.lines {
            let tone = match said {
                triumph::Said::You(_) => OnboardingTone::Sent,
                _ => OnboardingTone::Incoming,
            };
            self.push(from + at, Tone::Onboarding(tone));
        }
        if let Some(&(at, _)) = beats.effects.first() {
            self.push(
                from + at,
                Tone::Onboarding(OnboardingTone::Triumph(triumph_take())),
            );
        }
        for &(at, effect) in &beats.effects {
            let tone = match effect {
                scene_script::Effect::Confetti => OnboardingTone::Confetti,
                scene_script::Effect::Fireworks => OnboardingTone::Fireworks,
            };
            self.push(from + at, Tone::Onboarding(tone));
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

    /// The microphone step: a tick as macOS's prompt grants it.
    pub(crate) fn on_microphone(&mut self, from: f64) {
        self.push(
            from + permissions::MIC_GRANT,
            Tone::Onboarding(OnboardingTone::PermissionTick),
        );
    }

    /// The Accessibility step: the guide escaping Hex's card, then a tick
    /// as the switch goes on.
    pub(crate) fn on_accessibility(&mut self, from: f64) {
        let from = from - permissions::AX_SHIFT;
        self.push(
            from + permissions::AX_ESCAPE + 0.18,
            Tone::Onboarding(OnboardingTone::PanelEscape),
        );
        self.push(
            from + permissions::AX_GRANT,
            Tone::Onboarding(OnboardingTone::PermissionTick),
        );
    }

    /// `"With Hex"`'s score landing.
    pub(crate) fn on_score(&mut self, now: f64) {
        self.push(now + 0.05, Tone::Onboarding(OnboardingTone::AlmostReady));
    }

    /// `"Almost ready"` waiting screen (fires at most once per run).
    pub(crate) fn on_almost_ready(&mut self, now: f64) {
        if !self.almost_ready_scheduled {
            self.almost_ready_scheduled = true;
            self.push(now + 0.06, Tone::Onboarding(OnboardingTone::AlmostReady));
        }
    }

    /// `"Hex is ready"` finale: one rising note as each of the logo's
    /// hexagons lands, resolving on the last; Reduced Motion shows the logo
    /// built, with only the last.
    pub(crate) fn on_ready(&mut self, now: f64, reduced: bool) {
        if reduced {
            self.push(now, Tone::Onboarding(OnboardingTone::Build(2)));
            return;
        }
        for (i, at) in BUILD_AT.into_iter().enumerate() {
            self.push(now + at, Tone::Onboarding(OnboardingTone::Build(i as u8)));
        }
    }

    /// Interactive capsule tap (language or shortcut toggle).
    pub(crate) fn on_capsule_tap(&self) {
        feedback::play(Tone::Onboarding(OnboardingTone::ShortcutDown));
    }

    /// Advances playback to `now`, dispatching every newly-reached cue to
    /// [`feedback::play`] (no-op when headless or volume is 0).
    pub(crate) fn advance_to(&mut self, now: f64) {
        while let Some(&(at, tone, percent)) = self.cues.get(self.fired) {
            if at > now + 1e-4 {
                break;
            }
            self.fired += 1;
            feedback::play_at(tone, percent);
        }
    }

    pub(crate) fn fired_cues(&self) -> &[(f64, Tone, u8)] {
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
        let scene = typing::Scene::default();
        let mut track = SfxTrack::default();
        track.on_without_hex(0.5, &scene);
        track.advance_to(60.0);

        let mut last_key: Option<u8> = None;
        let mut key_count = 0;
        let mut back_count = 0;
        let mut sent = 0;
        let mut read = 0;
        for &(_, tone, _) in track.fired_cues() {
            match tone {
                Tone::Onboarding(OnboardingTone::Key(_, v)) => {
                    assert_ne!(last_key, Some(v), "adjacent key clacks repeated Key({v})");
                    last_key = Some(v);
                    key_count += 1;
                }
                Tone::Onboarding(OnboardingTone::KeyBackspace(_)) => {
                    last_key = None;
                    back_count += 1;
                }
                Tone::Onboarding(OnboardingTone::Sent) => sent += 1,
                Tone::Onboarding(OnboardingTone::Read) => read += 1,
                _ => {}
            }
        }
        assert!(key_count >= 18, "{key_count}");
        // Every delete, and the clunk under each slip onto Return.
        let deletes = scene.strokes().iter().filter(|s| s.key == "delete").count();
        let slips = scene
            .strokes()
            .iter()
            .filter(|s| s.key == "return" && scene.flash(s.at + 1e-3, "return") > 0.0)
            .count();
        assert_eq!(back_count, deletes + slips);
        assert_eq!(sent, scene.sent(scene.duration()));
        assert_eq!(read, scene.reads().count());
        assert!(track.fired_cues().iter().any(|&(_, t, _)| t == Tone::Error));
    }

    #[test]
    fn without_hex_clacks_whatever_key_sound_was_picked() {
        let scene = typing::Scene::default();
        let mut track = SfxTrack {
            style: KeySoundStyle::Silent,
            ..SfxTrack::default()
        };
        track.on_without_hex(0.0, &scene);
        track.advance_to(60.0);
        let styles: Vec<KeySoundStyle> = track
            .fired_cues()
            .iter()
            .filter_map(|&(_, tone, _)| match tone {
                Tone::Onboarding(
                    OnboardingTone::Key(style, _)
                    | OnboardingTone::KeySpace(style)
                    | OnboardingTone::KeyBackspace(style),
                ) => Some(style),
                _ => None,
            })
            .collect();
        assert!(!styles.is_empty());
        assert!(styles.iter().all(|&s| s == KeySoundStyle::Clack));
    }

    #[test]
    fn tapbacks_tick_and_the_heartbeat_stops_dead_at_the_removal() {
        let scene = typing::Scene::default();
        let mut track = SfxTrack::default();
        track.on_without_hex(0.0, &scene);
        track.advance_to(60.0);
        let count = |cue: OnboardingTone| {
            track
                .fired_cues()
                .iter()
                .filter(|&&(_, t, _)| t == Tone::Onboarding(cue))
                .count()
        };
        let taps = Tapback::ALL.map(|k| count(OnboardingTone::Tapback(k)));
        assert_eq!(taps.iter().sum::<usize>(), scene.all_taps().len());
        assert_eq!(count(OnboardingTone::Slam), scene.slams().count());
        let removal = scene.removal().unwrap();
        let hearts: Vec<&(f64, Tone, u8)> = track
            .fired_cues()
            .iter()
            .filter(|&&(_, t, _)| t == Tone::Onboarding(OnboardingTone::Heartbeat))
            .collect();
        assert!(hearts.len() >= 20);
        assert!(
            hearts
                .iter()
                .all(|&&(at, ..)| at + typing::BEAT_LEN <= removal)
        );
        assert!(
            hearts.first().unwrap().2 < hearts.last().unwrap().2,
            "it rises"
        );
    }

    #[test]
    fn panic_strokes_land_harder() {
        let scene = typing::Scene::default();
        let mut track = SfxTrack::default();
        track.on_without_hex(0.0, &scene);
        track.advance_to(60.0);
        let first = scene.strokes()[0].at;
        let last = scene
            .strokes()
            .iter()
            .rev()
            .find(|s| s.hard && s.key.len() == 1)
            .unwrap()
            .at;
        let thuds = |at: f64| {
            track
                .fired_cues()
                .iter()
                .filter(|&&(t, tone, _)| {
                    (t - at).abs() < 0.01
                        && matches!(tone, Tone::Onboarding(OnboardingTone::KeySpace(_)))
                })
                .count()
        };
        assert_eq!(thuds(first), 0);
        assert_eq!(thuds(last), 1);
    }

    #[test]
    fn voice_and_permission_cues_include_real_dictation_tones_and_motion_beats() {
        let mut track = SfxTrack::default();
        track.on_download_start(0.0);
        track.on_voice_take(10.0, &ANSWER_JAY);
        track.on_microphone(14.0);
        track.on_accessibility(17.0);
        track.on_almost_ready(21.5);
        track.on_ready(28.0, false);
        track.advance_to(30.0);

        let tones: Vec<Tone> = track.fired_cues().iter().map(|&(_, t, _)| t).collect();
        assert!(tones.contains(&Tone::Onboarding(OnboardingTone::ShortcutDown)));
        assert!(tones.contains(&Tone::DictationStart));
        assert!(tones.contains(&Tone::Onboarding(OnboardingTone::ShortcutUp)));
        assert!(tones.contains(&Tone::DictationStop));
        assert!(tones.contains(&Tone::DictationDone));
        assert!(tones.contains(&Tone::Onboarding(OnboardingTone::PanelEscape)));
        assert!(tones.contains(&Tone::Onboarding(OnboardingTone::PermissionTick)));
        assert!(tones.contains(&Tone::Onboarding(OnboardingTone::AlmostReady)));
        let built: Vec<i64> = track
            .fired_cues()
            .iter()
            .filter(|(_, t, _)| matches!(t, Tone::Onboarding(OnboardingTone::Build(_))))
            .map(|&(at, _, _)| ((at - 28.0) * 1000.0).round() as i64)
            .collect();
        assert_eq!(built, [0, 150, 300], "rising notes 150 ms apart");
    }

    #[test]
    fn reduce_motion_finishes_the_logo_on_one_note() {
        let mut track = SfxTrack::default();
        track.on_ready(1.0, true);
        track.advance_to(5.0);
        assert_eq!(
            track.fired_cues(),
            [(1.0, Tone::Onboarding(OnboardingTone::Build(2)), 100)]
        );
    }
}

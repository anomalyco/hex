//! The microphone row's level meter: three bars following the audio Hex
//! receives, polled only while General shows.

use std::path::Path;
use std::time::Duration;

use rune::{Color, El, Tx, el, row};

use super::super::controls::Paint;
use super::super::{Pane, Ui};

/// How often the microphone row follows the level while audio arrives.
const LEVEL_STEPS: f32 = 6.0;
/// A meter's next level for the latest `peak`: it rises at once and falls
/// over a few polls.
fn follow_level(level: f32, peak: f32) -> f32 {
    (peak * 3.0).clamp(0.0, 1.0).max(level - 0.12)
}
/// `level` in the steps a meter shows, so it redraws only when they change.
fn level_step(level: f32) -> u8 {
    (level * LEVEL_STEPS).round() as u8
}
/// `level` rounded to the step a meter shows.
fn shown_level(level: f32) -> f32 {
    f32::from(level_step(level)) / LEVEL_STEPS
}
const MIC_LEVEL: f32 = 14.0;
/// Three short bars, as tall as Hex hears it.
fn mic_bars(p: Paint, level: f32, tint: Color) -> El {
    const WEIGHTS: [f32; 3] = [0.6, 1.0, 0.75];
    let mut bars = row()
        .key("level")
        .size(MIC_LEVEL, MIC_LEVEL)
        .gap(2.0)
        .center();
    for (index, weight) in WEIGHTS.into_iter().enumerate() {
        // Uneven at rest too, so the bars never read as an ellipsis.
        let height = (2.0 + weight * 4.0 + level * weight * 8.0).round();
        bars = bars.child(
            el().key(format!("bar{index}"))
                .size(2.0, height)
                .radius(1.0)
                .fill(if level > 0.0 { p.text() } else { tint }),
        );
    }
    bars
}

const LEVEL_FRAME: Duration = Duration::from_millis(33);

/// What the microphone row's level shows: the audio Hex receives from the
/// microphone in use, or nothing while none arrives (released while idle,
/// failed, or not allowed), as flat, dimmed bars.
pub(in super::super) struct MicLevel {
    /// 0 to 1.
    level: f32,
    pub(in super::super) receiving: bool,
    /// The poll clock when the next level frame is due.
    frame_at: f64,
    /// Scenes: a recording played from virtual time zero instead of the
    /// microphone.
    recording: Option<(Vec<f32>, u32)>,
}

impl Default for MicLevel {
    /// A quiet room, as fixtures show it.
    fn default() -> Self {
        Self {
            level: 0.0,
            receiving: true,
            frame_at: 0.0,
            recording: None,
        }
    }
}

impl MicLevel {
    /// No audio arriving.
    pub(in super::super) fn closed() -> Self {
        Self {
            receiving: false,
            ..Self::default()
        }
    }

    /// Plays the mono WAV at `path` from virtual time zero.
    pub(in super::super) fn playing(path: &Path) -> Self {
        Self {
            recording: crate::audio::voice_filter::read_wav_mono_f32(path).ok(),
            ..Self::default()
        }
    }

    /// Takes the latest peak, `None` when no audio arrived lately; true
    /// when what shows changed.
    pub(in super::super) fn hear(&mut self, peak: Option<f32>) -> bool {
        let before = self.shown();
        match peak {
            Some(peak) => {
                self.receiving = true;
                self.level = follow_level(self.level, peak);
            }
            None => {
                self.receiving = false;
                self.level = 0.0;
            }
        }
        before != self.shown()
    }

    fn shown(&self) -> (bool, u8) {
        (self.receiving, level_step(self.level))
    }

    /// Whether the bars show a level, so the next frames can move them.
    pub(in super::super) fn moving(&self) -> bool {
        self.receiving && level_step(self.level) > 0
    }

    /// The recording's peak over the frame before `clock`, or `None` once
    /// it has played.
    fn recording_peak(&self, clock: f64) -> Option<Option<f32>> {
        let (samples, rate) = self.recording.as_ref()?;
        let end = (clock * f64::from(*rate)) as usize;
        let start = end.saturating_sub((LEVEL_FRAME.as_secs_f64() * f64::from(*rate)) as usize);
        Some((start < samples.len()).then(|| {
            samples[start..end.min(samples.len())]
                .iter()
                .fold(0.0_f32, |peak, s| peak.max(s.abs()))
        }))
    }

    pub(in super::super) fn bars(&self, p: Paint) -> El {
        let tint = if self.receiving {
            p.faint()
        } else {
            p.k.c.border_strong
        };
        mic_bars(p, shown_level(self.level), tint)
    }
}

/// Follows the microphone's level while General shows in a visible window
/// (a scene's recording also plays headless), waking at a UI rate only while
/// the bars show a level; the window's own tick notices audio rising again. It only reads the level the audio owner stores, so it
/// never opens a microphone, including one released while idle. True when
/// the row changed.
pub(in super::super) fn poll_level(ui: &mut Ui, clock: f64, tx: &mut Tx) -> bool {
    if ui.pane != Pane::General || ui.labs_open || ui.tour.is_some() {
        return false;
    }
    let visible = ui.window.as_ref().is_none_or(rune::AppHandle::is_visible);
    let (peak, playing) = match ui.mic_level.recording_peak(clock) {
        Some(peak) => (peak.or(Some(0.0)), peak.is_some()),
        None if ui.live() && visible => (crate::audio::input_level::recent(), true),
        // Other fixtures keep the level they set.
        _ => return false,
    };
    let level = &mut ui.mic_level;
    let changed = level.hear(peak);
    // Frames follow the bars only while they stand above rest (a level still
    // falling finishes, even once a recording has played) or a scene's
    // recording plays. A quiet room's noise shows nothing, so it waits for
    // the window's tick instead of waking the window 30 times a second.
    let recording = level.recording.is_some() && playing;
    if (level.moving() || recording) && clock >= level.frame_at {
        level.frame_at = clock + LEVEL_FRAME.as_secs_f64();
        tx.wake_after(LEVEL_FRAME);
    }
    changed
}

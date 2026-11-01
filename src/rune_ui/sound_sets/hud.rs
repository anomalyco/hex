//! The HUD the stage shows: Hex's own, told what the real one is told as
//! each sound plays, and stepped on its own clock from the start of the
//! playback, so a held moment always draws the same picture.
//!
//! It is `rune_hud`'s HUD, Dot scope: `dictation_indicator`'s model, drawn
//! by the window's compositor through [`rune_hud::Painter`] over an empty
//! element the view lays out ([`Preview`]).

use std::time::{Duration, Instant};

use rune::compose::Frame;
use rune::{El, el, row};

use super::super::Ui;
use crate::dictation::indicator::{DictationIndicatorEvent as Event, DotFrame, IndicatorModel};
use crate::feedback::{Audition, Cue};
use crate::rune_hud::dot_scope::{Tuning, demo_level};
use crate::rune_hud::hud_placement::Layout;
use crate::rune_hud::{Painter, Target, WINDOW};

const STEP: f32 = 1.0 / 60.0;
const JOB: u64 = 1;
/// The HUD's window, found by the compositor each frame.
pub(super) const WINDOW_KEY: &str = "sound-sets-hud";
/// The screen it sits on: it draws nowhere else.
pub(super) const SCREEN_KEY: &str = "sound-sets-screen";

/// What the HUD hears, as the real one's events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Beat {
    Start,
    Stop,
    Done,
    Fail,
    /// The end of a lone start or stop: the HUD leaves.
    Leave,
}

/// Each beat of `what` and when, in seconds from its first sound. A lone
/// sound begins as far into a dictation as its moment comes, so stop shows
/// the recording it ends.
fn beats(what: Audition) -> Vec<(f32, Beat)> {
    let lead: &[(f32, Beat)] = match what {
        Audition::Dictation | Audition::Cue(Cue::Start) => &[],
        Audition::Cue(Cue::Stop) => &[(-0.8, Beat::Start)],
        Audition::Cue(Cue::Done | Cue::Error) => &[(-1.2, Beat::Start), (-0.6, Beat::Stop)],
    };
    let mut beats = lead.to_vec();
    beats.extend(what.cues().iter().map(|&(cue, at)| {
        let beat = match cue {
            Cue::Start => Beat::Start,
            Cue::Stop => Beat::Stop,
            Cue::Done => Beat::Done,
            Cue::Error => Beat::Fail,
        };
        (at.as_secs_f32(), beat)
    }));
    if matches!(what, Audition::Cue(Cue::Start | Cue::Stop)) {
        beats.push((what.length().as_secs_f32(), Beat::Leave));
    }
    beats
}

pub(super) struct Hud {
    beats: Vec<(f32, Beat)>,
    next: usize,
    /// Seconds from the playback's first sound, from the first beat.
    time: f32,
    /// When the recording started, for the voice's level.
    voice: f32,
    tuning: Tuning,
    model: Box<IndicatorModel>,
    /// The model's clock at the first beat.
    clock: Instant,
    recording: bool,
    drawn: Option<DotFrame>,
}

impl Hud {
    pub(super) fn new(tuning: Tuning, what: Audition) -> Self {
        let beats = beats(what);
        let time = beats.first().map_or(0.0, |&(at, _)| at);
        let clock = Instant::now();
        Self {
            beats,
            next: 0,
            time,
            voice: time,
            tuning,
            model: Box::new(IndicatorModel::new(tuning, clock)),
            clock,
            recording: false,
            drawn: None,
        }
    }

    /// Steps to `seconds` from the playback's first sound.
    pub(super) fn advance_to(&mut self, seconds: f32) {
        while self.time + STEP <= seconds + 1e-4 {
            self.time += STEP;
            while let Some(&(at, beat)) = self.beats.get(self.next) {
                if at > self.time {
                    break;
                }
                self.next += 1;
                self.beat(beat);
            }
            self.step();
        }
    }

    /// The model's clock at the current time.
    fn now(&self) -> Instant {
        let start = self.beats.first().map_or(0.0, |&(at, _)| at);
        self.clock + Duration::from_secs_f32((self.time - start).max(0.0))
    }

    fn beat(&mut self, beat: Beat) {
        let now = self.now();
        self.recording = beat == Beat::Start;
        if beat == Beat::Start {
            self.voice = self.time;
        }
        let events: &[Event] = match beat {
            Beat::Start => &[Event::Started],
            Beat::Stop => &[
                Event::Submitted { job_id: JOB },
                Event::Transcribing { job_id: JOB },
            ],
            Beat::Done => &[Event::JobCompleted { job_id: JOB }],
            Beat::Fail => &[Event::JobFailed { job_id: JOB }],
            Beat::Leave => &[Event::Cancelled, Event::JobCancelled { job_id: JOB }],
        };
        for &event in events {
            self.model.handle(event, now, false);
        }
    }

    fn step(&mut self) {
        let now = self.now();
        if self.recording {
            let level = demo_level(self.time - self.voice);
            self.model.handle(
                Event::Meter {
                    average: 0.08 * level,
                    peak: 0.3 * level,
                },
                now,
                false,
            );
        }
        if self.model.active() {
            let drawn = self.model.step(now, STEP, false);
            self.drawn = self.model.active().then_some(drawn);
        }
    }

    /// Whether the HUD has left, or never came.
    pub(super) fn settled(&self) -> bool {
        self.drawn.is_none()
    }

    /// This frame, while the HUD shows.
    fn drawn(&self) -> Option<&DotFrame> {
        self.drawn.as_ref()
    }

    /// The window centred in a slot as wide as the screen, its height, and
    /// how far inside it the HUD's plate sits, top and bottom.
    pub(super) fn view(&self) -> (El, f32, f32) {
        let (width, height) = WINDOW;
        let plate = self.tuning.height;
        let slot = row()
            .key("hud-slot")
            .role("none")
            .w_full()
            .h(height)
            .center()
            .child(el().key(WINDOW_KEY).role("none").size(width, height));
        (slot, height, (height - plate) / 2.0)
    }
}

/// Draws the stage's HUD over the window's UI.
#[derive(Default)]
pub(crate) struct Preview {
    painter: Option<Painter>,
}

impl Preview {
    pub(crate) fn render(&mut self, f: &mut Frame<'_, Ui>) {
        let Some(hud) = &f.rt.state.sound_sets.hud else {
            return;
        };
        let (Some(drawn), Some((rect, opacity)), Some((screen, _))) = (
            hud.drawn(),
            f.rt.presented(WINDOW_KEY),
            f.rt.presented(SCREEN_KEY),
        ) else {
            return;
        };
        if opacity <= 0.001 || rect[2] <= 0.0 || rect[3] <= 0.0 {
            return;
        }
        let scale = f.rt.env.scale;
        let px = rect.map(|v| v * scale);
        let (target_w, target_h) = (f.size.0 as f32, f.size.1 as f32);
        let left = (screen[0] * scale).clamp(0.0, target_w);
        let top = (screen[1] * scale).clamp(0.0, target_h);
        let right = ((screen[0] + screen[2]) * scale).clamp(0.0, target_w);
        let bottom = ((screen[1] + screen[3]) * scale).clamp(0.0, target_h);
        if right - left < 1.0 || bottom - top < 1.0 {
            return;
        }
        if self
            .painter
            .as_ref()
            .is_none_or(|painter| painter.format != f.format)
        {
            self.painter = Some(Painter::new(f.device, f.format));
        }
        let painter = self.painter.as_ref().expect("created above");
        let mut frame = drawn.clone();
        frame.pose.opacity *= opacity;
        let target = Target {
            viewport: px,
            clip: [
                left as u32,
                top as u32,
                (right - left) as u32,
                (bottom - top) as u32,
            ],
            // The modal's scale included.
            scale: px[2] / WINDOW.0,
            dark: f.rt.env.system_dark,
        };
        let uniforms = painter.write(&frame, Layout::default(), target, |buffer, data| {
            f.write_buffer(buffer, 0, data)
        });
        painter.draw_over(f.encoder, f.target, target, &uniforms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settles_after(what: Audition) -> bool {
        let mut hud = Hud::new(Tuning::default(), what);
        hud.advance_to(what.length().as_secs_f32() + 1.5);
        hud.settled()
    }

    #[test]
    fn the_hud_shows_a_dictation_and_then_leaves() {
        let mut hud = Hud::new(Tuning::default(), Audition::Dictation);
        assert!(hud.settled(), "shows before the start sound");
        hud.advance_to(0.5);
        assert!(!hud.settled(), "hides while recording");
        hud.advance_to(1.8);
        assert!(!hud.settled(), "hides while transcribing");
        assert!(settles_after(Audition::Dictation), "stays");
    }

    #[test]
    fn a_lone_sound_shows_its_moment_and_leaves() {
        for cue in Cue::ALL {
            let what = Audition::Cue(cue);
            let mut hud = Hud::new(Tuning::default(), what);
            hud.advance_to(0.0);
            // Everything but a lone start begins mid-dictation, shown.
            assert_eq!(hud.settled(), cue == Cue::Start, "{cue:?}");
            assert!(settles_after(what), "{cue:?} stays");
        }
    }

    #[test]
    fn dot_scope_turns_coral_on_failure() {
        let mut hud = Hud::new(Tuning::default(), Audition::Cue(Cue::Error));
        hud.advance_to(0.3);
        let frame = hud.drawn().expect("Dot scope shows a failure");
        assert!(frame.pose.fail > 0.5);
    }
}

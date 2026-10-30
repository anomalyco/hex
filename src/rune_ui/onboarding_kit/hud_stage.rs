//! Hex's real recording indicator (`Dot scope` phosphor HUD) inside onboarding:
//!
//! Steps [`crate::dictation_indicator::IndicatorModel`] with the real listener
//! events (`Started`, `Meter`, `Submitted` + `Transcribing`, `JobCompleted`),
//! and paints the resulting [`DotFrame`] over the stage with
//! [`crate::rune_hud::Painter`] (`rune_hud.wgsl`), identical to `#292`'s Sounds
//! sheet and the real desktop HUD.

use std::time::{Duration, Instant};

use rune::{El, effect};

use super::fixture::Take;
use super::gpu_stage::{HUD_SLOT_MARK, set_dot_hud};
use crate::dictation_indicator::{DictationIndicatorEvent as Event, DotFrame, IndicatorModel};
use crate::dot_scope::{Tuning, demo_level};
use crate::rune_hud::WINDOW;

const STEP: f32 = 1.0 / 60.0;
/// Integrate all the way to the presentation clock, including between 60 Hz
/// samples. Equal substeps avoid a tiny remainder that the HUD's minimum dt
/// would round up; the simulation stays bounded without quantising the pose.
fn steps(duration: f32) -> (u32, f32) {
    let count = (duration / STEP).ceil() as u32;
    (
        count,
        if count == 0 {
            0.0
        } else {
            duration / count as f32
        },
    )
}
const JOB: u64 = 1;
/// Seconds after shortcut release (`Submitted` + `Transcribing`) when the final
/// streamed word lands and `JobCompleted` triggers Dot scope's Done wipe.
pub(crate) const DONE_DELAY: f64 = 0.14;

/// Real `Dot scope` `Processing::Flow` phosphor helix for `"Setting up Hex"` and
/// `"Almost ready"`.
pub(crate) fn flow_frame_at(now_secs: f64, reduce: bool) -> DotFrame {
    let clock = Instant::now();
    let mut model = IndicatorModel::new(Tuning::default(), clock);
    model.handle(Event::Started, clock, reduce);
    model.handle(Event::Submitted { job_id: JOB }, clock, reduce);
    model.handle(Event::Transcribing { job_id: JOB }, clock, reduce);

    // Bounded cyclic window (`0.65 s` warm-in + smooth continuous phase) so step
    // count stays tiny even at `t = 30 s`.
    let target = (0.45 + (now_secs.max(0.0) % 12.0)) as f32;
    let (count, dt) = steps(target);
    let mut drawn = model.step(clock, 0.0, reduce);
    for i in 1..=count {
        let time = i as f32 * dt;
        let at = clock + Duration::from_secs_f32(time);
        drawn = model.step(at, dt, reduce);
    }
    drawn
}

/// The resting meter the plate hums at while waiting for the user to hold `⌥`.
const IDLE_METER: Event = Event::Meter {
    average: 0.024,
    peak: 0.075,
};
/// After its first unfold, the resting plate replays this window so the idle
/// simulation stays bounded however long it waits.
const IDLE_LOOP: f64 = 6.0;
const IDLE_SETTLED: f64 = 0.55;

/// Where `idle` seconds of waiting land in the bounded idle simulation: the
/// whole first pass, so the plate unfolds when it mounts, then a loop that
/// starts once it has settled.
fn idle_time(idle: f64) -> f32 {
    let idle = idle.max(0.0);
    let t = if idle < IDLE_SETTLED + IDLE_LOOP {
        idle
    } else {
        IDLE_SETTLED + (idle - IDLE_SETTLED) % IDLE_LOOP
    };
    t as f32
}

/// Hex's real [`DotFrame`] for a resting plate that showed `idle` seconds
/// before the shortcut went down (or before now, while it still waits), then
/// `take` recording `elapsed` seconds after the press, on one model: the plate
/// unfolds when it mounts and keeps running through the press instead of
/// starting over. `None` before the press or once the Done collapse finishes.
pub(crate) fn voice_frame_at(
    idle: f64,
    take: Option<(&Take, f64)>,
    reduce: bool,
) -> Option<DotFrame> {
    if take.is_some_and(|(take, elapsed)| elapsed < 0.0 || elapsed > take.release + 1.2) {
        return None;
    }
    let clock = Instant::now();
    let mut model = IndicatorModel::new(Tuning::default(), clock);
    model.handle(Event::Started, clock, reduce);
    let mut drawn = model.step(clock, 0.0, reduce);

    let idle = idle_time(idle);
    let (count, dt) = steps(idle);
    for i in 1..=count {
        let time = i as f32 * dt;
        let at = clock + Duration::from_secs_f32(time);
        model.handle(IDLE_METER, at, reduce);
        drawn = model.step(at, dt, reduce);
    }
    let Some((take, elapsed)) = take else {
        return Some(drawn);
    };

    let pressed = idle;
    let target = elapsed as f32;
    let stop_at = take.release as f32;
    let done_at = (take.release + DONE_DELAY) as f32;
    let mut submitted = false;
    let mut completed = false;
    let (count, dt) = steps(target);
    for i in 1..=count {
        let since = i as f32 * dt;
        let now = clock + Duration::from_secs_f32(pressed + since);
        if !submitted && since >= stop_at {
            submitted = true;
            model.handle(Event::Submitted { job_id: JOB }, now, reduce);
            model.handle(Event::Transcribing { job_id: JOB }, now, reduce);
        }
        if !completed && since >= done_at {
            completed = true;
            model.handle(Event::JobCompleted { job_id: JOB }, now, reduce);
        }
        if !submitted {
            let (avg, pk) = take.meter(f64::from(since));
            let demo = demo_level(since);
            model.handle(
                Event::Meter {
                    average: (0.06 * demo + 0.11 * avg).min(0.35),
                    peak: (0.22 * demo + 0.28 * pk).min(0.85),
                },
                now,
                reduce,
            );
        }
        if !model.active() {
            return None;
        }
        drawn = model.step(now, dt, reduce);
    }
    model.active().then_some(drawn)
}

/// Computes Hex's real [`DotFrame`] at `elapsed` seconds after shortcut press
/// for `take`, returning `None` before press or once the Done collapse finishes.
#[cfg(test)]
pub(crate) fn frame_at(take: &Take, elapsed: f64, reduce: bool) -> Option<DotFrame> {
    voice_frame_at(0.0, Some((take, elapsed)), reduce)
}

fn slot_el(key: &str, centre: [f32; 2], scale: f32) -> El {
    let (w, h) = (WINDOW.0 * scale, WINDOW.1 * scale);
    effect(HUD_SLOT_MARK)
        .key(key.to_string())
        .role("none")
        .mark(HUD_SLOT_MARK)
        .absolute(centre[0] - w * 0.5, centre[1] - h * 0.5)
        .size(w, h)
}

/// The `"Your turn"` plate while a real dictation records and transcribes:
/// one model stepped each frame with the microphone's own level, as the
/// desktop HUD is, so silence rests and louder speech moves it more.
pub(crate) struct LiveHud {
    model: Option<IndicatorModel>,
    at: Instant,
    held: bool,
    released: Option<Instant>,
    frame: Option<DotFrame>,
}

/// How long the plate waits for a released dictation's text before it
/// finishes anyway.
const LAND_TIMEOUT: Duration = Duration::from_secs(6);

impl Default for LiveHud {
    fn default() -> Self {
        Self {
            model: None,
            at: Instant::now(),
            held: false,
            released: None,
            frame: None,
        }
    }
}

impl LiveHud {
    /// Steps to now: `held` while the dictation records, `peak` the
    /// microphone's latest peak (`None`: no audio), `landed` once its text
    /// arrived. True while the plate still shows.
    pub(crate) fn step(
        &mut self,
        held: bool,
        peak: Option<f32>,
        landed: bool,
        reduce: bool,
    ) -> bool {
        let now = Instant::now();
        if held && !self.held {
            let mut model = IndicatorModel::new(Tuning::default(), now);
            model.handle(Event::Started, now, reduce);
            self.model = Some(model);
            self.at = now;
        }
        let Some(model) = self.model.as_mut() else {
            self.held = held;
            self.frame = None;
            return false;
        };
        if !held && self.held {
            model.handle(Event::Submitted { job_id: JOB }, now, reduce);
            model.handle(Event::Transcribing { job_id: JOB }, now, reduce);
            self.released = Some(now);
        }
        let timed_out = self
            .released
            .is_some_and(|at| now.duration_since(at) >= LAND_TIMEOUT);
        if !held && self.released.is_some() && (landed || timed_out) {
            self.released = None;
            model.handle(Event::JobCompleted { job_id: JOB }, now, reduce);
        }
        self.held = held;
        if held {
            // The desktop HUD reads the chunk's RMS; speech runs at about a
            // third of its peak.
            let peak = peak.unwrap_or(0.0).clamp(0.0, 1.0);
            model.handle(
                Event::Meter {
                    average: peak * 0.3,
                    peak,
                },
                now,
                reduce,
            );
        }
        let dt = now.duration_since(self.at).as_secs_f32();
        self.at = now;
        let frame = model.step(now, dt, reduce);
        if model.active() {
            self.frame = Some(frame);
        } else {
            self.model = None;
            self.frame = None;
        }
        self.frame.is_some()
    }

    pub(crate) fn frame(&self) -> Option<&DotFrame> {
        self.frame.as_ref()
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Mounts `frame` as the plate.
pub(crate) fn frame_slot(key: &str, centre: [f32; 2], scale: f32, frame: &DotFrame) -> El {
    set_dot_hud(Some(frame.clone()));
    slot_el(key, centre, scale)
}

/// Mounts the real `Dot scope` `Processing::Flow` phosphor HUD centred in a
/// layout box of `(w, h)` for `"Setting up Hex"` and `"Almost ready"`.
pub(crate) fn flow_box(key: &str, scale: f32, now: f64, reduce: bool) -> El {
    set_dot_hud(Some(flow_frame_at(now, reduce)));
    let (w, h) = (WINDOW.0 * scale, 62.0 * scale);
    rune::el()
        .key(format!("{key}-wrap"))
        .size(w, h)
        .child(slot_el(key, [w * 0.5, h * 0.5], scale))
}

/// Mounts the real `Dot scope` phosphor HUD for `"With Hex"` or `"Your turn"`,
/// returning `(hud_element, hud_heat, voice_energy)`. The plate rests while
/// `waiting`, then records `take` from `take_from`, on one model that started
/// when the plate first showed (`Onboard::hud_from`).
pub(crate) fn voice_or_idle_slot(
    key: &str,
    centre: [f32; 2],
    scale: f32,
    waiting: bool,
    take: &Take,
    take_from: Option<f64>,
    s: &super::stories::Onboard,
) -> (Option<El>, f32, f32) {
    let now = s.now;
    let reduce = s.reduced.get();
    if let Some(from) = take_from {
        let elapsed = now - from;
        let idle = s.hud_from.map_or(0.0, |shown| from - shown);
        if let Some(frame) = voice_frame_at(idle, Some((take, elapsed)), reduce) {
            let heat = (frame.pose.opacity * if elapsed < take.release { 1.0 } else { 0.45 })
                .clamp(0.0, 1.0);
            let (avg, _) = take.meter(elapsed);
            set_dot_hud(Some(frame));
            return (Some(slot_el(key, centre, scale)), heat, avg);
        }
        set_dot_hud(None);
        return (None, 0.0, 0.0);
    }
    // A scene that opens mid-wait shows the plate already settled.
    let idle = s
        .hud_from
        .map_or(IDLE_SETTLED + now.rem_euclid(IDLE_LOOP), |shown| {
            now - shown
        });
    if waiting && let Some(frame) = voice_frame_at(idle, None, reduce) {
        let heat = 0.22 * frame.pose.opacity;
        set_dot_hud(Some(frame));
        return (Some(slot_el(key, centre, scale)), heat, 0.04);
    }
    set_dot_hud(None);
    (None, 0.0, 0.0)
}

#[cfg(test)]
mod tests {
    use super::super::fixture::ANSWER_JAY;
    use super::*;

    #[test]
    fn the_hud_advances_between_sixty_hertz_frames() {
        let a = voice_frame_at(0.25, None, false).unwrap();
        let b = voice_frame_at(0.25 + 1.0 / 120.0, None, false).unwrap();
        assert_ne!(a, b, "a 120 Hz frame must not repeat the previous pose");
        assert_ne!(
            flow_frame_at(0.25, false),
            flow_frame_at(0.25 + 1.0 / 120.0, false)
        );
    }

    #[test]
    fn dot_hud_appears_listens_wipes_done_and_leaves() {
        assert!(frame_at(&ANSWER_JAY, -0.1, false).is_none());
        let open = frame_at(&ANSWER_JAY, 0.25, false).expect("HUD open while speaking");
        assert!(open.pose.opacity > 0.5 && open.pose.live > 0.2);

        let wiping = frame_at(&ANSWER_JAY, ANSWER_JAY.release + DONE_DELAY + 0.08, false)
            .expect("HUD wiping Done right after completion");
        assert!(wiping.pose.opacity > 0.05);

        assert!(
            frame_at(&ANSWER_JAY, ANSWER_JAY.release + 1.0, false).is_none(),
            "HUD collapses away cleanly after Done"
        );

        let flow = flow_frame_at(0.9, false);
        assert!(flow.pose.opacity > 0.8);
    }
}

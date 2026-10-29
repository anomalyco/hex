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
    let mut time = 0.0_f32;
    let mut drawn = model.step(clock, 0.0, reduce);
    while time + STEP <= target + 1e-4 {
        time += STEP;
        let at = clock + Duration::from_secs_f32(time);
        drawn = model.step(at, STEP, reduce);
    }
    drawn
}

/// Calm resting `Dot scope` phosphor plate while waiting for the user to hold `⌥`
/// (`Step::YourTurn` and `Step::Try`).
pub(crate) fn idle_frame_at(now_secs: f64, reduce: bool) -> DotFrame {
    let clock = Instant::now();
    let mut model = IndicatorModel::new(Tuning::default(), clock);
    model.handle(Event::Started, clock, reduce);
    let target = (0.55 + (now_secs.max(0.0) % 6.0)) as f32;
    let mut time = 0.0_f32;
    let mut drawn = model.step(clock, 0.0, reduce);
    while time + STEP <= target + 1e-4 {
        time += STEP;
        let at = clock + Duration::from_secs_f32(time);
        model.handle(
            Event::Meter {
                average: 0.024,
                peak: 0.075,
            },
            at,
            reduce,
        );
        drawn = model.step(at, STEP, reduce);
    }
    drawn
}

/// Computes Hex's real [`DotFrame`] at `elapsed` seconds after shortcut press
/// for `take`, returning `None` before press or once the Done collapse finishes.
pub(crate) fn frame_at(take: &Take, elapsed: f64, reduce: bool) -> Option<DotFrame> {
    if elapsed < 0.0 || elapsed > take.release + 1.2 {
        return None;
    }
    let target = elapsed as f32;
    let clock = Instant::now();
    let mut model = IndicatorModel::new(Tuning::default(), clock);
    model.handle(Event::Started, clock, reduce);

    let stop_at = take.release as f32;
    let done_at = (take.release + DONE_DELAY) as f32;
    let mut submitted = false;
    let mut completed = false;
    let mut time = 0.0_f32;
    let mut drawn = Some(model.step(clock, 0.0, reduce));

    while time + STEP <= target + 1e-4 {
        time += STEP;
        let now = clock + Duration::from_secs_f32(time);
        if !submitted && time >= stop_at {
            submitted = true;
            model.handle(Event::Submitted { job_id: JOB }, now, reduce);
            model.handle(Event::Transcribing { job_id: JOB }, now, reduce);
        }
        if !completed && time >= done_at {
            completed = true;
            model.handle(Event::JobCompleted { job_id: JOB }, now, reduce);
        }
        if !submitted {
            let (avg, pk) = take.meter(f64::from(time));
            let demo = demo_level(time);
            model.handle(
                Event::Meter {
                    average: (0.06 * demo + 0.11 * avg).min(0.35),
                    peak: (0.22 * demo + 0.28 * pk).min(0.85),
                },
                now,
                reduce,
            );
        }
        if model.active() {
            let next = model.step(now, STEP, reduce);
            drawn = model.active().then_some(next);
        } else {
            drawn = None;
            break;
        }
    }
    drawn
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

/// Live `Dot scope` wave while the user physically holds the shortcut key on
/// interactive `"Your turn"`.
pub(crate) fn live_hold_slot(
    key: &str,
    centre: [f32; 2],
    scale: f32,
    held_secs: f64,
    reduce: bool,
) -> El {
    let clock = Instant::now();
    let mut model = IndicatorModel::new(Tuning::default(), clock);
    model.handle(Event::Started, clock, reduce);
    let target = (0.18 + (held_secs.max(0.0) % 10.0)) as f32;
    let mut time = 0.0_f32;
    let mut drawn = model.step(clock, 0.0, reduce);
    while time + STEP <= target + 1e-4 {
        time += STEP;
        let at = clock + Duration::from_secs_f32(time);
        let lvl = demo_level(time);
        model.handle(
            Event::Meter {
                average: (0.09 * lvl).min(0.32),
                peak: (0.34 * lvl).min(0.82),
            },
            at,
            reduce,
        );
        drawn = model.step(at, STEP, reduce);
    }
    set_dot_hud(Some(drawn));
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
/// returning `(hud_element, hud_heat, voice_energy)`.
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
        if let Some(frame) = frame_at(take, elapsed, reduce) {
            let heat = (frame.pose.opacity * if elapsed < take.release { 1.0 } else { 0.45 })
                .clamp(0.0, 1.0);
            let (avg, _) = take.meter(elapsed);
            set_dot_hud(Some(frame));
            return (Some(slot_el(key, centre, scale)), heat, avg);
        }
        set_dot_hud(None);
        return (None, 0.0, 0.0);
    }
    if waiting {
        let frame = idle_frame_at(now, reduce);
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

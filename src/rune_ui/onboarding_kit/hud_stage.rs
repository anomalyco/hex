//! Hex's real recording indicator (`Dot scope` phosphor HUD) inside onboarding:
//!
//! Steps [`crate::dictation::indicator::IndicatorModel`] with the real listener
//! events (`Started`, `Meter`, `Submitted` + `Transcribing`, `JobCompleted`),
//! and paints the resulting [`DotFrame`] over the stage with
//! [`crate::rune_hud::Painter`] (`rune_hud.wgsl`), identical to `#292`'s Sounds
//! sheet and the real desktop HUD.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use rune::{El, effect};

use super::fixture::Take;
use super::gpu_stage::{hud_mark, update_hud};
use crate::dictation::indicator::{DictationIndicatorEvent as Event, DotFrame, IndicatorModel};
use crate::rune_hud::WINDOW;
use crate::rune_hud::dot_scope::{Tuning, demo_level};

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

/// The download plate advances from the last presentation time. Rebuilding
/// its whole history in every view made a long download progressively costly.
#[derive(Default)]
pub(crate) struct FlowHud(RefCell<Option<Flow>>);

struct Flow {
    model: IndicatorModel,
    clock: Instant,
    at: f64,
    reduced: bool,
    frame: DotFrame,
}

impl FlowHud {
    pub(crate) fn frame(&self, now: f64, reduced: bool) -> DotFrame {
        let mut cached = self.0.borrow_mut();
        if cached
            .as_ref()
            .is_none_or(|flow| flow.at > now || flow.reduced != reduced)
        {
            let clock = Instant::now();
            let mut model = IndicatorModel::new(Tuning::default(), clock);
            model.handle(Event::Started, clock, reduced);
            model.handle(Event::Submitted { job_id: JOB }, clock, reduced);
            model.handle(Event::Transcribing { job_id: JOB }, clock, reduced);
            let frame = model.step(clock, 0.0, reduced);
            *cached = Some(Flow {
                model,
                clock,
                at: now - 0.45,
                reduced,
                frame,
            });
        }
        let flow = cached.as_mut().expect("created above");
        // A window returning after a long pause catches up once, bounded to
        // one second; ordinary frames advance exactly their clock delta.
        let delta = (now - flow.at).clamp(0.0, 1.0) as f32;
        let (count, dt) = steps(delta);
        for _ in 0..count {
            flow.clock += Duration::from_secs_f32(dt);
            flow.frame = flow.model.step(flow.clock, dt, reduced);
        }
        flow.at = now;
        flow.frame.clone()
    }
}

/// The resting meter the plate hums at while waiting for the user to hold `⌥`.
const IDLE_METER: Event = Event::Meter {
    average: 0.024,
    peak: 0.075,
};
/// After its first unfold, a plate that mounts mid-wait replays this window
/// so its first frame's simulation stays bounded however long it waited.
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
#[cfg(test)]
pub(crate) fn voice_frame_at(
    idle: f64,
    take: Option<(&Take, f64)>,
    reduce: bool,
) -> Option<DotFrame> {
    if outside(take) {
        return None;
    }
    Voice::replay(idle, take, reduce).drawn
}

fn outside(take: Option<(&Take, f64)>) -> bool {
    take.is_some_and(|(take, elapsed)| elapsed < 0.0 || elapsed > take.release + 1.2)
}

/// The `"With Hex"` and `"Your turn"` plates advance from the last
/// presentation time, as [`FlowHud`] does: replaying the whole wait and take
/// in every view kept the first run's window at about 40% of a core.
#[derive(Default)]
pub(crate) struct VoiceHud(RefCell<Option<(String, Voice)>>);

impl VoiceHud {
    /// [`voice_frame_at`]'s plate for `key`, stepped on from its last frame
    /// when it continues it; otherwise replayed.
    pub(crate) fn frame(
        &self,
        key: &str,
        idle: f64,
        take: Option<(&Take, f64)>,
        reduce: bool,
    ) -> Option<DotFrame> {
        if outside(take) {
            return None;
        }
        let mut cached = self.0.borrow_mut();
        match cached.as_mut() {
            Some((shown, voice)) if shown == key && voice.continues(idle, take, reduce) => {
                voice.advance(idle, take);
                voice.drawn.clone()
            }
            _ => {
                let voice = Voice::replay(idle, take, reduce);
                let drawn = voice.drawn.clone();
                *cached = Some((key.to_owned(), voice));
                drawn
            }
        }
    }
}

/// A window returning after a longer pause replays instead.
const CATCH_UP: f64 = 1.0;

struct Voice {
    model: IndicatorModel,
    clock: Instant,
    /// Model seconds since the plate mounted.
    at: f32,
    /// The caller's waiting seconds `at` stands for.
    idle: f64,
    take: Option<Taking>,
    reduce: bool,
    /// `None` once the Done collapse finished.
    drawn: Option<DotFrame>,
}

struct Taking {
    /// The caller's waiting seconds when the shortcut went down.
    press_idle: f64,
    /// Model seconds when the shortcut went down.
    pressed: f32,
    elapsed: f32,
    submitted: bool,
    completed: bool,
}

impl Voice {
    fn replay(idle: f64, take: Option<(&Take, f64)>, reduce: bool) -> Self {
        let clock = Instant::now();
        let mut model = IndicatorModel::new(Tuning::default(), clock);
        model.handle(Event::Started, clock, reduce);
        let drawn = Some(model.step(clock, 0.0, reduce));
        let mut voice = Self {
            model,
            clock,
            at: 0.0,
            idle,
            take: None,
            reduce,
            drawn,
        };
        voice.wait(idle_time(idle));
        if let Some((take, elapsed)) = take {
            voice.press(idle);
            voice.record(take, elapsed as f32);
        }
        voice
    }

    fn continues(&self, idle: f64, take: Option<(&Take, f64)>, reduce: bool) -> bool {
        let close = |from: f64, to: f64| to >= from && to - from <= CATCH_UP;
        reduce == self.reduce
            && match (&self.take, take) {
                (None, _) => close(self.idle, idle),
                (Some(taking), Some((_, elapsed))) => {
                    taking.press_idle == idle && close(f64::from(taking.elapsed), elapsed)
                }
                (Some(_), None) => false,
            }
    }

    fn advance(&mut self, idle: f64, take: Option<(&Take, f64)>) {
        if self.take.is_none() {
            self.wait((idle - self.idle) as f32);
            self.idle = idle;
        }
        if let Some((take, elapsed)) = take {
            if self.take.is_none() {
                self.press(idle);
            }
            self.record(take, elapsed as f32);
        }
    }

    /// Waits `seconds` more on the resting meter.
    fn wait(&mut self, seconds: f32) {
        let (count, dt) = steps(seconds);
        let from = self.at;
        for i in 1..=count {
            let at = self.clock + Duration::from_secs_f32(from + i as f32 * dt);
            self.model.handle(IDLE_METER, at, self.reduce);
            self.drawn = Some(self.model.step(at, dt, self.reduce));
        }
        self.at = from + seconds.max(0.0);
    }

    fn press(&mut self, idle: f64) {
        self.take = Some(Taking {
            press_idle: idle,
            pressed: self.at,
            elapsed: 0.0,
            submitted: false,
            completed: false,
        });
    }

    /// Records `take` on to `elapsed` seconds after the press.
    fn record(&mut self, take: &Take, elapsed: f32) {
        let Some(taking) = self.take.as_mut() else {
            return;
        };
        let reduce = self.reduce;
        let stop_at = take.release as f32;
        let done_at = (take.release + DONE_DELAY) as f32;
        let from = taking.elapsed;
        let (count, dt) = steps(elapsed - from);
        taking.elapsed = elapsed.max(from);
        if self.drawn.is_none() {
            return;
        }
        for i in 1..=count {
            let since = from + i as f32 * dt;
            let now = self.clock + Duration::from_secs_f32(taking.pressed + since);
            if !taking.submitted && since >= stop_at {
                taking.submitted = true;
                self.model.handle(Event::Submitted { job_id: JOB }, now, reduce);
                self.model.handle(Event::Transcribing { job_id: JOB }, now, reduce);
            }
            if !taking.completed && since >= done_at {
                taking.completed = true;
                self.model.handle(Event::JobCompleted { job_id: JOB }, now, reduce);
            }
            if !taking.submitted {
                let (avg, pk) = take.meter(f64::from(since));
                let demo = demo_level(since);
                self.model.handle(
                    Event::Meter {
                        average: (0.06 * demo + 0.11 * avg).min(0.35),
                        peak: (0.22 * demo + 0.28 * pk).min(0.85),
                    },
                    now,
                    reduce,
                );
            }
            if !self.model.active() {
                self.drawn = None;
                return;
            }
            self.drawn = Some(self.model.step(now, dt, reduce));
        }
        if !self.model.active() {
            self.drawn = None;
        }
    }
}

/// Computes Hex's real [`DotFrame`] at `elapsed` seconds after shortcut press
/// for `take`, returning `None` before press or once the Done collapse finishes.
#[cfg(test)]
pub(crate) fn frame_at(take: &Take, elapsed: f64, reduce: bool) -> Option<DotFrame> {
    voice_frame_at(0.0, Some((take, elapsed)), reduce)
}

fn slot_el(key: &str, centre: [f32; 2], scale: f32) -> El {
    let (w, h) = (WINDOW.0 * scale, WINDOW.1 * scale);
    let mark = hud_mark(key);
    effect(mark)
        .key(key.to_string())
        .role("none")
        .mark(mark)
        .absolute(centre[0] - w * 0.5, centre[1] - h * 0.5)
        .size(w, h)
}

/// The `"Your turn"` plate while a real dictation records and transcribes:
/// one model stepped each frame with the microphone's own level, as the
/// desktop HUD is, so silence rests and louder speech moves it more.
pub(crate) struct LiveHud {
    model: Option<IndicatorModel>,
    clock: Instant,
    at: Instant,
    held: bool,
    waiting: bool,
    finished: bool,
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
            clock: Instant::now(),
            at: Instant::now(),
            held: false,
            waiting: true,
            finished: false,
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
        seconds: f64,
        held: bool,
        peak: Option<f32>,
        landed: bool,
        reduce: bool,
    ) -> bool {
        let now = self.clock + Duration::from_secs_f64(seconds.max(0.0));
        if self.model.is_none() && (!self.finished || (held && !self.held)) {
            let mut model = IndicatorModel::new(Tuning::default(), now);
            model.handle(Event::Started, now, reduce);
            self.model = Some(model);
            self.at = now;
            self.finished = false;
            self.waiting = true;
        }
        let Some(model) = self.model.as_mut() else {
            self.held = held;
            self.frame = None;
            return false;
        };
        if held && !self.held {
            // The waiting plate is already recording a quiet meter. Started
            // resets Dot scope's scanner, so only restart after a previous take.
            if !self.waiting {
                model.handle(Event::Started, now, reduce);
            }
            self.waiting = false;
            self.released = None;
        }
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
        } else if self.waiting {
            model.handle(IDLE_METER, now, reduce);
        }
        self.held = held;
        let dt = now.saturating_duration_since(self.at).as_secs_f32();
        self.at = now;
        let frame = model.step(now, dt, reduce);
        if model.active() {
            self.frame = Some(frame);
        } else {
            self.model = None;
            self.frame = None;
            self.finished = true;
        }
        self.frame.is_some()
    }

    pub(crate) fn frame(&self) -> Option<&DotFrame> {
        self.frame.as_ref()
    }

    pub(crate) fn finished(&self) -> bool {
        self.finished
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Mounts `frame` as the plate.
pub(crate) fn frame_slot(key: &str, centre: [f32; 2], scale: f32, frame: &DotFrame) -> El {
    update_hud(hud_mark(key), frame.clone());
    slot_el(key, centre, scale)
}

/// Mounts the real `Dot scope` `Processing::Flow` phosphor HUD centred in a
/// layout box of `(w, h)` for `"Setting up Hex"` and `"Almost ready"`.
pub(crate) fn flow_box(key: &str, scale: f32, s: &super::stories::Onboard) -> El {
    update_hud(hud_mark(key), s.flow_hud.frame(s.now, s.reduced.get()));
    let (w, h) = (WINDOW.0 * scale, 62.0 * scale);
    rune::el()
        .key(format!("{key}-wrap"))
        .size(w, h)
        .transition(s.motion.roll(8.0))
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
        if let Some(frame) = s.voice_hud.frame(key, idle, Some((take, elapsed)), reduce) {
            let heat = (frame.pose.opacity * if elapsed < take.release { 1.0 } else { 0.45 })
                .clamp(0.0, 1.0);
            let (avg, _) = take.meter(elapsed);
            update_hud(hud_mark(key), frame);
            return (Some(slot_el(key, centre, scale)), heat, avg);
        }
        return (None, 0.0, 0.0);
    }
    // A scene that opens mid-wait shows the plate already settled.
    let idle = s
        .hud_from
        .map_or(IDLE_SETTLED + now.rem_euclid(IDLE_LOOP), |shown| {
            now - shown
        });
    if waiting && let Some(frame) = s.voice_hud.frame(key, idle, None, reduce) {
        let heat = 0.22 * frame.pose.opacity;
        update_hud(hud_mark(key), frame);
        return (Some(slot_el(key, centre, scale)), heat, 0.04);
    }
    (None, 0.0, 0.0)
}

#[cfg(test)]
mod tests {
    use super::super::fixture::ANSWER_JAY;
    use super::*;

    #[test]
    fn pressing_keeps_the_waiting_hud_open_and_completion_stays_gone() {
        let mut hud = LiveHud::default();
        for i in 0..120 {
            hud.step(i as f64 / 120.0, false, None, false, false);
        }
        let before = hud.frame().unwrap().pose;
        hud.step(1.0, true, Some(0.3), false, false);
        let pressed = hud.frame().unwrap().pose;
        assert!(
            pressed.opacity >= before.opacity - 0.01
                && pressed.scale >= before.scale - 0.01
                && pressed.width >= before.width - 0.1,
            "pressing must keep the waiting plate, not unfold a new one"
        );
        hud.step(1.1, false, None, false, false);
        hud.step(1.2, false, None, true, false);
        for i in 145..360 {
            hud.step(i as f64 / 120.0, false, None, false, false);
        }
        assert!(hud.finished());
        assert!(hud.frame().is_none());
        hud.step(3.0, true, Some(0.3), false, false);
        assert!(hud.frame().is_some(), "the next press starts a new plate");
    }

    #[test]
    fn the_hud_advances_between_sixty_hertz_frames() {
        let a = voice_frame_at(0.25, None, false).unwrap();
        let b = voice_frame_at(0.25 + 1.0 / 120.0, None, false).unwrap();
        assert_ne!(a, b, "a 120 Hz frame must not repeat the previous pose");
        let flow = FlowHud::default();
        assert_ne!(
            flow.frame(0.25, false),
            flow.frame(0.25 + 1.0 / 120.0, false)
        );
    }

    /// Stepped on from its last frame, the plate waits and records as a
    /// replay of its whole wait and take shows it within a frame of the same
    /// time, and its Done collapse ends on the same frame.
    #[test]
    fn the_plate_steps_on_as_a_replay_would() {
        const FRAME: f64 = 1.0 / 60.0;
        let hud = VoiceHud::default();
        let press = 0.5;
        let replay = |now: f64| {
            let take = (now >= press).then_some((&ANSWER_JAY, now - press));
            let idle = if take.is_some() { press } else { now };
            (idle, take)
        };
        let (mut stepped_end, mut replayed_end) = (None, None);
        for i in 0..=200 {
            let now = f64::from(i) * FRAME;
            let (idle, take) = replay(now);
            let stepped = hud.frame("plate", idle, take, false);
            if stepped.is_none() {
                stepped_end.get_or_insert(now);
            }
            if replayed_end.is_none()
                && now >= press + ANSWER_JAY.release
                && voice_frame_at(idle, take, false).is_none()
            {
                replayed_end = Some(now);
            }
            // The unfold, and the release onwards, move faster than a frame;
            // the collapse is checked by when it ends. Every fourth frame
            // keeps the replays affordable.
            let recording = i % 4 == 0 && (0.1..press + ANSWER_JAY.release).contains(&now);
            let (Some(a), true) = (stepped.map(|frame| frame.pose), recording) else {
                continue;
            };
            // Dot scope steps at 240 Hz and keeps the remainder: the nearest
            // replayed pose within a frame.
            let nearest = (-4..=4)
                .filter_map(|steps| {
                    let (idle, take) = replay(now + f64::from(steps) / 240.0);
                    voice_frame_at(idle, take, false)
                })
                .map(|b| {
                    let b = b.pose;
                    (a.opacity - b.opacity)
                        .abs()
                        .max((a.scale - b.scale).abs())
                        .max((a.width - b.width).abs() / 10.0)
                        .max((a.live - b.live).abs())
                })
                .fold(f32::INFINITY, f32::min);
            assert!(nearest < 0.02, "at {now}: {a:?} is {nearest} from any replay");
        }
        let (stepped_end, replayed_end) = (stepped_end.unwrap(), replayed_end.unwrap());
        assert!(
            (stepped_end - replayed_end).abs() <= FRAME + 1e-9,
            "the collapse ends at {stepped_end} stepped, {replayed_end} replayed"
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

        let flow = FlowHud::default().frame(0.9, false);
        assert!(flow.pose.opacity > 0.8);
    }
}

//! The dictation HUD's listening field, drawn into the recording header. The
//! capsule supplies the surface and timer; there is no HUD window.
use super::{Capsule, Kind, meeting};
use crate::dictation::indicator::{DictationIndicatorEvent as Event, DotFrame, IndicatorModel};
use crate::rune_hud::spring::{REDUCED_FADE, Spec, Spring};
use crate::rune_hud::{Painter, Target, dot_scope::Tuning};
use rune::compose::{Compositor, DeviceKey, Frame};
use std::time::{Duration, Instant};

const KEY: &str = "meeting-listening-field";
const WIDTH: f32 = 78.0;
const ATTACK: Spec = Spec::new(0.16, 0.0);
const RELEASE: Spec = Spec::new(0.55, 0.0);
/// Dot scope's default -60 dBFS noise floor, in the capture meter's RMS units.
const FLOOR: f32 = 0.001;

pub(super) fn slot() -> rune::El {
    rune::el()
        .key(KEY)
        .size(WIDTH, crate::rune_hud::dot_scope::PLATE_HEIGHT)
        .a11y_hidden()
}

pub(super) struct Listening {
    model: IndicatorModel,
    drawn: Option<DotFrame>,
    last: Instant,
    presence: Spring,
    reduced: bool,
}

impl Listening {
    #[cfg(test)]
    pub(super) fn probe(&self) -> (f32, f32) {
        let f = self.drawn.as_ref().unwrap();
        let mut mass = 0.;
        let mut moment = 0.;
        let mut peak: f32 = 0.;
        for (i, dot) in f.dots.iter().enumerate() {
            let light = dot[0] as f32 / 65535.;
            let y = (i / f.columns) as f32 - (f.rows / 2) as f32;
            peak = peak.max(light);
            mass += light;
            moment += light * (y * f.tuning.spacing).powi(2);
        }
        (peak, (moment / mass.max(1e-9)).sqrt())
    }
    pub(super) fn new(now: Instant) -> Self {
        // Same optical size, pitch, voice response and shader as dictation;
        // just fewer columns to leave room for the timer.
        let tuning = Tuning {
            width: WIDTH,
            columns: 31.0,
            ..Tuning::default()
        };
        let mut model = IndicatorModel::new(tuning, now);
        model.handle(Event::Started, now, true);
        Self {
            model,
            drawn: None,
            last: now,
            presence: Spring::at(0.0),
            reduced: false,
        }
    }

    /// After the last buffer fades to an unlit field, don't run the braid or redraw.
    pub(super) fn sample(&mut self, now: Instant, [mic, system]: [f32; 2]) -> bool {
        let dt = now.saturating_duration_since(self.last).as_secs_f32();
        if dt == 0. && self.drawn.is_some() {
            return false;
        }
        self.last = now;
        let rms = mic.max(system);
        if rms < FLOOR && self.presence.x == 0.0 && self.drawn.is_some() {
            return false;
        }
        let (target, spring) = if rms >= FLOOR {
            (1., ATTACK)
        } else if self.reduced {
            (0., REDUCED_FADE)
        } else {
            (0., RELEASE)
        };
        // Never travel away from newly heard speech, nor swell on a pause.
        // Keep the current height. Only discard momentum opposing the new target.
        if target != self.presence.target {
            self.presence.v = 0.;
        }
        self.presence.step_to(target, spring, dt);
        if (target - self.presence.x).abs() < 0.001 {
            self.presence.jump(target);
        }
        self.model.handle(
            Event::Meter {
                average: rms,
                peak: rms,
            },
            now,
            self.reduced,
        );
        let mut drawn = self.model.step(now, dt, self.reduced);
        settle_field(&mut drawn, self.presence.x, self.reduced);
        drawn.pose.opacity = 1.0;
        self.drawn = Some(drawn);
        true
    }
}

#[test]
fn a_pause_gathers_the_light_before_extinguishing_it() {
    let start = Instant::now();
    let mut wave = Listening::new(start);
    let at = |frame| start + Duration::from_secs_f64(f64::from(frame) / 60.);
    for i in 1..=60 {
        wave.sample(at(i), [0.1, 0.0]);
    }
    let (bright, tall) = wave.probe();
    assert!(bright > 0.9 && tall > 2.);
    for i in 61..=66 {
        wave.sample(at(i), [0.; 2]);
    }
    let (dim, narrow) = wave.probe();
    assert!((0.45..0.85).contains(&dim) && narrow < tall * 0.85 && narrow > 0.);
    for i in 67..=77 {
        wave.sample(at(i), [0.; 2]);
    }
    let (line, height) = wave.probe();
    assert!(
        line > 0.02 && line < dim && height < 0.05,
        "{line}, {height}"
    );
    for i in 78..=120 {
        wave.sample(at(i), [0.; 2]);
    }
    assert!(!wave.sample(at(3600), [0.; 2]));
    assert_eq!(wave.probe(), (0., 0.));
}

#[test]
fn either_track_can_interrupt_the_release_in_place() {
    let start = Instant::now();
    for (first, next) in [([0.1, 0.], [0., 0.1]), ([0., 0.1], [0.1, 0.])] {
        let mut wave = Listening::new(start);
        let at = |i: u64| start + Duration::from_millis(i * 20);
        for i in 1..=50 {
            wave.sample(at(i), first);
        }
        for i in 51..=55 {
            wave.sample(at(i), [0.; 2]);
        }
        let dip = wave.presence.x;
        assert!(dip > 0.5 && dip < 0.9);
        wave.sample(at(56), next);
        assert!(wave.presence.x > dip && wave.presence.x < dip + 0.12);
        for i in 57..=60 {
            wave.sample(at(i), next);
        }
        let (bright, height) = wave.probe();
        assert!(bright > 0.8 && height > 1.);
    }
}

#[test]
fn reduced_motion_fades_without_compressing_the_dot_rows() {
    let start = Instant::now();
    let mut wave = Listening::new(start);
    wave.reduced = true;
    for i in 1..=60 {
        wave.sample(start + Duration::from_millis(i * 16), [0.1, 0.]);
    }
    let mut field = wave.drawn.unwrap();
    let dots = field.dots.clone();
    settle_field(&mut field, 0.4, true);
    for (after, before) in field.dots.iter().zip(&dots) {
        assert_eq!(after[0], (f32::from(before[0]) * 0.4).round() as u16);
        assert_eq!(after[1], before[1]);
    }
}

/// Bring the light back to the centre, not the lattice: the dot pitch,
/// radius and stationary centreline still belong to the dictation painter.
fn settle_field(f: &mut DotFrame, presence: f32, reduced: bool) {
    if presence == 1. {
        return;
    }
    if presence == 0. {
        f.dots.fill([0; 2]);
        return;
    }
    let middle = f.rows / 2;
    // Sample progressively farther out in the source field as it contracts.
    // Work from the middle out so both source rows are still untouched.
    // Reduce Motion just cools the still braid, without squeezing it.
    let height = if reduced { 1. } else { presence };
    for row in (0..middle).rev().chain(middle..f.rows) {
        let y = middle as f32 + (row as f32 - middle as f32) / height;
        let lo = y.floor() as isize;
        let t = y - y.floor();
        for col in 0..f.columns {
            let sample = |r: isize| {
                if (0..f.rows as isize).contains(&r) {
                    f.dots[r as usize * f.columns + col].map(f32::from)
                } else {
                    [0.; 2]
                }
            };
            let a = sample(lo);
            let b = sample(lo + 1);
            let mix = |i: usize| a[i] + (b[i] - a[i]) * t;
            f.dots[row * f.columns + col] =
                [(mix(0) * presence).round() as u16, mix(1).round() as u16];
        }
    }
}

#[test]
fn either_track_lights_the_same_field_and_silence_parks_it() {
    let start = Instant::now();
    for levels in [[0.1, 0.0], [0.0, 0.1], [0.04, 0.1]] {
        let mut wave = Listening::new(start);
        for i in 1..=60 {
            wave.sample(start + Duration::from_secs_f32(i as f32 / 60.0), levels);
        }
        assert!(
            wave.drawn
                .as_ref()
                .unwrap()
                .dots
                .iter()
                .any(|dot| dot[0] > 1000)
        );
        for i in 61..=120 {
            wave.sample(start + Duration::from_secs_f32(i as f32 / 60.0), [0.0; 2]);
        }
        assert!(!wave.sample(start + Duration::from_secs(2), [0.0; 2]));
        assert!(
            wave.drawn
                .as_ref()
                .unwrap()
                .dots
                .iter()
                .all(|dot| dot[0] == 0)
        );
    }
}

pub(super) fn poll(c: &mut Capsule, _: f64, tx: &mut rune::Tx) -> bool {
    if !matches!(c.kind, Some(Kind::Recording(_))) {
        return false;
    }
    let Some(wave) = c.waveform.as_mut() else {
        return false;
    };
    let changed = wave.sample(Instant::now(), meeting::track_levels());
    tx.wake_after(Duration::from_millis(if wave.presence.x > 0.0 {
        16
    } else {
        50
    }));
    changed
}

#[derive(Default)]
pub(super) struct Ink(Option<(DeviceKey, Painter)>);

impl Compositor<Capsule> for Ink {
    fn render(&mut self, f: &mut Frame<'_, Capsule>) {
        f.draw_ui();
        let Some((rect, opacity)) = f.rt.presented(KEY) else {
            self.0 = None;
            return;
        };
        let dev = f.device_key();
        let scale = f.rt.env.scale;
        let Some(wave) = f.rt.state.waveform.as_mut() else {
            return;
        };
        wave.reduced = f.rt.reduced_motion;
        let Some(drawn) = wave.drawn.as_ref() else {
            return;
        };
        let px = rect.map(|v| v * scale);
        if opacity < 0.001
            || px[2] < 1.0
            || px[3] < 1.0
            || px[0] < 0.0
            || px[1] < 0.0
            || px[0] + px[2] > f.size.0 as f32
            || px[1] + px[3] > f.size.1 as f32
        {
            return;
        }
        let target = Target {
            viewport: px,
            clip: [0, 0, f.size.0, f.size.1],
            scale,
        };
        if self
            .0
            .as_ref()
            .is_none_or(|(key, p)| *key != dev || p.format != f.format)
        {
            self.0 = Some((dev, Painter::dots_only(f.device, f.format)));
        }
        let painter = &self.0.as_ref().unwrap().1;
        let uniforms = painter.write(drawn, Default::default(), target, |buf, bytes| {
            f.queue.write_buffer(buf, 0, bytes)
        });
        painter.draw_over(f.encoder, f.target, target, &uniforms);
    }
}

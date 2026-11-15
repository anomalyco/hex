//! The dictation HUD's listening field, drawn into the recording header. The
//! capsule supplies the surface, timer and recording dot; there is no HUD window.
use super::{Capsule, Kind, meeting};
use crate::dictation::indicator::{DictationIndicatorEvent as Event, DotFrame, IndicatorModel};
use crate::rune_hud::{Painter, Target, dot_scope::Tuning};
use rune::compose::{Compositor, DeviceKey, Frame};
use std::time::{Duration, Instant};

const KEY: &str = "meeting-listening-field";
const WIDTH: f32 = 78.0;
const RELEASE: f32 = 0.30;
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
    light: f32,
    reduced: bool,
}

impl Listening {
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
            light: 0.0,
            reduced: false,
        }
    }

    /// After the last buffer fades to an unlit field, don't run the braid or redraw.
    pub(super) fn sample(&mut self, now: Instant, [mic, system]: [f32; 2]) -> bool {
        let dt = now
            .saturating_duration_since(self.last)
            .as_secs_f32()
            .clamp(1.0 / 240.0, 0.05);
        self.last = now;
        let rms = mic.max(system);
        if rms < FLOOR && self.light == 0.0 && self.drawn.is_some() {
            return false;
        }
        self.light = if rms >= FLOOR {
            (self.light + dt / 0.08).min(1.0)
        } else {
            (self.light - dt / RELEASE).max(0.0)
        };
        self.model.handle(
            Event::Meter {
                average: rms,
                peak: rms,
            },
            now,
            self.reduced,
        );
        let mut drawn = self.model.step(now, dt, self.reduced);
        // Dictation breathes at rest. A meeting can be silent for hours:
        // leave the centreline, but extinguish the beam instead of simulating speech.
        for dot in &mut drawn.dots {
            dot[0] = (dot[0] as f32 * self.light).round() as u16;
        }
        drawn.pose.opacity = 1.0;
        self.drawn = Some(drawn);
        true
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
        for i in 61..=90 {
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
    tx.wake_after(Duration::from_millis(if wave.light > 0.0 {
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

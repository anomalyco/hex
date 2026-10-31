//! Labs › Words to cursor's overlay (macOS): a click-through Rune overlay
//! that flies a pasted dictation's words from the HUD to where they landed
//! in the focused app ([`crate::words_to_caret`] finds where). Each word
//! arcs out with a stagger, eases past its slot and springs back, glows,
//! and fades over the real text. Reduce Motion draws nothing.
//!
//! The overlay covers only the flight: the HUD, the landing rects, and room
//! for the arcs, placed for each flight and hidden once every word is gone.

use std::time::Duration;

use rune::{Cx, El, OverlayHandle, Tx, el};

use super::controls::Paint;
use super::onboarding_kit::flight::{Pose, flying};
use crate::accessibility::ScreenRect;
use crate::words_to_caret::{Launch, Tuning};

/// The word's weight in the air.
const WEIGHT: f32 = 560.0;
/// Room around the flight for the glow and the overshoot.
const MARGIN: f32 = 48.0;

/// One word in the air, in the overlay's points.
#[derive(Clone, Debug, PartialEq)]
struct Word {
    text: String,
    from: [f32; 2],
    to: [f32; 2],
    /// The font size it lands at, from its rect's height.
    land: f32,
    start: f64,
    /// Which side the arc bows to.
    side: f32,
}

#[derive(Default)]
pub(crate) struct Flights {
    words: Vec<Word>,
    tuning: Tuning,
    pending: Option<Launch>,
    size: [f32; 2],
    now: f64,
    overlay: Option<OverlayHandle<Flights>>,
    shown: bool,
}

impl Word {
    fn duration(&self, tuning: &Tuning) -> f64 {
        tuning.duration_ms / 1000.0
    }

    fn arrives(&self, tuning: &Tuning) -> f64 {
        self.start + self.duration(tuning)
    }

    fn gone(&self, tuning: &Tuning) -> f64 {
        self.arrives(tuning) + tuning.fade_ms / 1000.0
    }

    /// Where the word is `u` of the way through its flight (past 1 at the
    /// overshoot): a quadratic arc eased out with a little back.
    fn point(&self, u: f32, tuning: &Tuning) -> [f32; 2] {
        let c = tuning.overshoot.max(0.0);
        let x = u.clamp(0.0, 1.0) - 1.0;
        let s = 1.0 + (c + 1.0) * x * x * x + c * x * x;
        let [sx, sy] = self.from;
        let [gx, gy] = self.to;
        let (dx, dy) = (gx - sx, gy - sy);
        let bend = self.side * tuning.bend;
        let (cx, cy) = (sx + dx * 0.5 - dy * bend, sy + dy * 0.5 + dx * bend);
        // The ease runs past 1 on the far side of the slot, along the arc's
        // last tangent.
        let t = s.min(1.0);
        let over = (s - 1.0).max(0.0);
        let (a, b, c2) = ((1.0 - t) * (1.0 - t), 2.0 * (1.0 - t) * t, t * t);
        let (tx, ty) = (gx - cx, gy - cy);
        [
            a * sx + b * cx + c2 * gx + tx * over,
            a * sy + b * cy + c2 * gy + ty * over,
        ]
    }

    /// The landing spring's push along the arrival direction, `t` seconds in.
    fn spring(&self, t: f32, tuning: &Tuning) -> [f32; 2] {
        let w = std::f32::consts::TAU * tuning.land_hz.max(0.1);
        let zeta = tuning.land_damping.clamp(0.05, 0.95);
        let wd = w * (1.0 - zeta * zeta).sqrt();
        let k = tuning.land_push * (-zeta * w * t).exp() * (wd * t).sin();
        let a = self.point(0.98, tuning);
        let (dx, dy) = (self.to[0] - a[0], self.to[1] - a[1]);
        let d = dx.hypot(dy).max(1e-3);
        [dx / d * k, dy / d * k]
    }

    fn pose(&self, now: f64, tuning: &Tuning) -> Option<Pose> {
        let age = now - self.start;
        if age < 0.0 || now >= self.gone(tuning) {
            return None;
        }
        let dur = self.duration(tuning).max(1e-3);
        let sprite = tuning.launch_size.max(self.land);
        if age >= dur {
            let t = (age - dur) as f32;
            let fade = (t / (tuning.fade_ms.max(1.0) / 1000.0) as f32).clamp(0.0, 1.0);
            let [nx, ny] = self.spring(t, tuning);
            return Some(Pose {
                x: self.to[0] + nx,
                y: self.to[1] + ny,
                vx: 0.0,
                vy: 0.0,
                size: self.land,
                sprite,
                angle: 0.0,
                heat: 0.9 * tuning.glow.clamp(0.0, 1.0) * (1.0 - fade),
                alpha: (1.0 - fade) * (1.0 - fade),
            });
        }
        let u = (age / dur) as f32;
        let [x, y] = self.point(u, tuning);
        let step = 1.0 / 120.0;
        let [x0, y0] = self.point((u - step).max(0.0), tuning);
        let per_second = step * dur as f32;
        let (vx, vy) = ((x - x0) / per_second, (y - y0) / per_second);
        let e = smooth(0.15, 0.85, u);
        let mut angle = vy.atan2(vx);
        if angle > std::f32::consts::FRAC_PI_2 {
            angle -= std::f32::consts::PI;
        }
        if angle < -std::f32::consts::FRAC_PI_2 {
            angle += std::f32::consts::PI;
        }
        let grow = (u * 4.0).min(1.0);
        Some(Pose {
            x,
            y,
            vx,
            vy,
            size: lerp(tuning.launch_size * lerp(0.4, 1.0, grow), self.land, e),
            sprite,
            angle: lerp(angle * 0.2, 0.0, e),
            heat: lerp(0.35, 0.9, e) * tuning.glow.clamp(0.0, 1.0),
            alpha: grow.min(1.0),
        })
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The font size of text whose line is `height` points tall.
fn font_size(height: f64) -> f32 {
    (height as f32 / 1.25).clamp(9.0, 48.0)
}

/// The overlay's rect for a flight, in global points, and its words in the
/// overlay's points, launched `stagger` apart from `now`.
fn lay_out(launch: &Launch, now: f64) -> ([f32; 4], Vec<Word>) {
    let center = |rect: &ScreenRect| {
        let (x, y) = rect.center();
        [x as f32, y as f32]
    };
    let from = launch.from;
    let mut min = from;
    let mut max = from;
    let mut reach = 0.0_f32;
    for (_, rect) in &launch.words {
        let [x, y] = center(rect);
        min = [min[0].min(rect.x as f32), min[1].min(rect.y as f32)];
        max = [
            max[0].max((rect.x + rect.width) as f32),
            max[1].max((rect.y + rect.height) as f32),
        ];
        reach = reach.max((x - from[0]).hypot(y - from[1]));
    }
    // A quadratic arc bows half as far as its control point.
    let pad = MARGIN + reach * launch.tuning.bend.abs() * 0.5;
    let origin = [min[0] - pad, min[1] - pad];
    let rect = [
        origin[0],
        origin[1],
        max[0] - min[0] + pad * 2.0,
        max[1] - min[1] + pad * 2.0,
    ];
    let local = |[x, y]: [f32; 2]| [x - origin[0], y - origin[1]];
    let words = launch
        .words
        .iter()
        .enumerate()
        .map(|(index, (text, rect))| Word {
            text: text.clone(),
            from: local(from),
            to: local(center(rect)),
            land: font_size(rect.height),
            start: now + index as f64 * launch.tuning.stagger_ms / 1000.0,
            side: if index % 2 == 0 { -1.0 } else { 1.0 },
        })
        .collect();
    (rect, words)
}

fn poll(s: &mut Flights, now: f64, tx: &mut Tx) -> bool {
    s.now = now;
    let Some(overlay) = s.overlay.clone() else {
        return false;
    };
    if let Some(launch) = s.pending.take() {
        let (rect, words) = lay_out(&launch, now);
        s.words = words;
        s.tuning = launch.tuning;
        s.size = [rect[2], rect[3]];
        overlay.set_placement(Some(rune::OverlayPlacement::Rect {
            x: rect[0],
            y: rect[1],
            width: rect[2],
            height: rect[3],
        }));
        overlay.resize((rect[2], rect[3]));
        if !s.shown {
            s.shown = true;
            overlay.show();
        }
    }
    if s.words.is_empty() {
        return false;
    }
    if s.words.iter().all(|word| now >= word.gone(&s.tuning)) {
        s.words.clear();
        s.shown = false;
        overlay.hide();
        return true;
    }
    // Every word's pose is already exact for this instant.
    tx.instant();
    tx.wake_after(Duration::ZERO);
    true
}

/// Adds the overlay, hidden, and sends it each flight. Prewarmed only when
/// the lab is on at launch.
pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) {
    use rune::{DisplayTarget, OverlayInput, OverlayLevel, OverlayOptions, OverlaySpec};
    let spec = OverlaySpec::new("Hex words to cursor", (320.0, 240.0), view)
        .options(OverlayOptions {
            display: DisplayTarget::Primary,
            level: OverlayLevel::AboveStatus,
            input: OverlayInput::None,
            prewarm: crate::labs::words_to_caret(),
            ..Default::default()
        })
        .poll(poll);
    let overlay = app.overlay(spec, Flights::default());
    let handle = overlay.clone();
    overlay.update(move |s, _| s.overlay = Some(handle));
    crate::words_to_caret::on_launch(move |launch| {
        overlay.update(move |s, tx| {
            s.pending = Some(launch);
            tx.wake_after(Duration::ZERO);
        });
    });
}

fn view(s: &Flights, cx: &mut Cx<Flights>) -> El {
    let root = el().size(s.size[0].max(1.0), s.size[1].max(1.0));
    if s.words.is_empty() || cx.system_reduced_motion() {
        return root;
    }
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    s.words
        .iter()
        .enumerate()
        .fold(root, |root, (index, word)| {
            match word.pose(s.now, &s.tuning) {
                Some(pose) => root.child(flying(p, &format!("w{index}"), &word.text, WEIGHT, pose)),
                None => root,
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch() -> Launch {
        Launch {
            from: [700.0, 20.0],
            words: vec![
                (
                    "so".into(),
                    ScreenRect {
                        x: 200.0,
                        y: 500.0,
                        width: 14.0,
                        height: 16.0,
                    },
                ),
                (
                    "plan".into(),
                    ScreenRect {
                        x: 220.0,
                        y: 500.0,
                        width: 30.0,
                        height: 16.0,
                    },
                ),
            ],
            tuning: Tuning::default(),
        }
    }

    #[test]
    fn the_overlay_covers_the_hud_and_every_landing() {
        let (rect, words) = lay_out(&launch(), 10.0);
        let [x, y, w, h] = rect;
        assert!(x < 200.0 && y < 20.0);
        assert!(x + w > 700.0 && y + h > 516.0);
        assert_eq!(words[0].from, [700.0 - x, 20.0 - y]);
        assert_eq!(words[1].to, [235.0 - x, 508.0 - y]);
    }

    #[test]
    fn words_launch_in_turn_and_land_on_their_slots() {
        let tuning = Tuning::default();
        let (_, words) = lay_out(&launch(), 10.0);
        assert!(words[1].start > words[0].start);
        let first = words[0].pose(10.0, &tuning).unwrap();
        assert_eq!([first.x, first.y], words[0].from);
        let landed = words[0].pose(words[0].arrives(&tuning), &tuning).unwrap();
        assert_eq!([landed.x, landed.y], words[0].to);
        assert_eq!(landed.size, words[0].land);
        assert!(words[0].pose(words[0].gone(&tuning), &tuning).is_none());
        assert!(words[1].pose(10.0, &tuning).is_none());
    }

    #[test]
    fn a_word_overshoots_its_slot_then_settles() {
        let tuning = Tuning::default();
        let (_, words) = lay_out(&launch(), 0.0);
        let word = &words[0];
        let past = (1..100).any(|n| {
            let p = word.point(n as f32 / 100.0, &tuning);
            // It travels down and left; past the slot is lower.
            p[1] > word.to[1] + 0.5
        });
        assert!(past);
        assert_eq!(word.point(1.0, &tuning), word.to);
    }
}

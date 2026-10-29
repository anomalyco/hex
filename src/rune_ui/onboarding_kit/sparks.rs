//! Crisp landing sparks and impact rings for hot word arrivals, colored
//! strictly with OpenCode theme tokens (`rune_theme_opencode::ember`).

use rune::{El, LineCap, Shape, canvas};

use super::super::controls::Paint;
use super::flight::{Flight, ember_tone};

const DRAG: f32 = 4.6;
const GRAVITY: f32 = 360.0;
const STREAK: f32 = 0.65 / 60.0 * 1.4 * 0.5;
const RING_LIFE: f32 = 0.20;
const SPARKS_FROM: f32 = 0.40;
const RING_FROM: f32 = 0.42;

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn rnd(seed: usize, a: f32, b: f32) -> f32 {
    let x = ((seed as f32 * 12.9898 + 4.1).sin() * 43_758.545)
        .fract()
        .abs();
    lerp(a, b, x)
}

struct Spark {
    at: [f32; 2],
    v: [f32; 2],
    life: f32,
    heat: f32,
    size: f32,
}

fn sprayed(flight: &Flight, index: usize) -> Vec<Spark> {
    let i = flight.intensity();
    if i <= SPARKS_FROM {
        return Vec::new();
    }
    let [ux, uy] = flight.heading();
    let [x, y] = flight.to();
    let back = (-uy).atan2(-ux);
    let count = lerp(3.0, 10.0, i).round() as usize;
    (0..count)
        .map(|n| {
            let seed = index * 97 + n * 7;
            let graze = if n % 3 == 0 {
                if n % 2 == 0 { 1.15 } else { -1.15 }
            } else {
                rnd(seed, -0.7, 0.7)
            };
            let a = back + graze;
            let speed = rnd(seed + 1, 160.0, 560.0) * lerp(0.5, 1.0, i);
            Spark {
                at: [x + rnd(seed + 2, -2.5, 2.5), y + rnd(seed + 3, -2.5, 2.5)],
                v: [
                    a.cos() * speed + ux * 45.0,
                    a.sin() * speed - rnd(seed + 4, 20.0, 85.0),
                ],
                life: rnd(seed + 5, 0.12, 0.28),
                heat: rnd(seed + 6, 0.45, 0.92),
                size: rnd(seed + 7, 1.1, 1.8),
            }
        })
        .collect()
}

impl Spark {
    fn at(&self, t: f32) -> ([f32; 2], [f32; 2]) {
        let decay = (-DRAG * t).exp();
        let reach = (1.0 - decay) / DRAG;
        let terminal = GRAVITY / DRAG;
        let x = self.at[0] + self.v[0] * reach;
        let y = self.at[1] + terminal * t + (self.v[1] - terminal) * reach;
        let v = [self.v[0] * decay, terminal + (self.v[1] - terminal) * decay];
        ([x, y], v)
    }
}

pub(crate) fn impacts(
    p: Paint,
    key: &str,
    size: [f32; 2],
    flights: &[Flight],
    now: f64,
    reduced: bool,
) -> El {
    let mut shape = Shape::new();
    if !reduced {
        for (index, flight) in flights.iter().enumerate() {
            let t = (now - flight.arrives()) as f32;
            if !(0.0..=0.35).contains(&t) {
                continue;
            }
            shape = ring(shape, p, flight, t);
            for spark in sprayed(flight, index) {
                if t >= spark.life {
                    continue;
                }
                let left = 1.0 - t / spark.life;
                let ([x, y], [vx, vy]) = spark.at(t);
                let color = p.k.c.bg_base.mix(
                    ember_tone(p, spark.heat * left),
                    (left * 0.85).clamp(0.0, 1.0),
                );
                shape = shape.line(
                    [x - vx * STREAK, y - vy * STREAK],
                    [x + vx * STREAK + 0.1, y + vy * STREAK],
                    spark.size,
                    LineCap::Round,
                    color,
                );
            }
        }
    }
    canvas(shape).key(key.to_string()).size(size[0], size[1])
}

fn ring(shape: Shape, p: Paint, flight: &Flight, t: f32) -> Shape {
    let i = flight.intensity();
    if i <= RING_FROM || t >= RING_LIFE {
        return shape;
    }
    let u = t / RING_LIFE;
    let r = lerp(18.0, 44.0, i) * (1.0 - (1.0 - u).powi(3));
    let width = lerp(1.0, 1.6, i) * (1.0 - u * 0.65);
    let fade = (lerp(0.25, 0.55, i) * (1.0 - u) * (1.0 - u)).clamp(0.0, 1.0);
    if fade < 0.02 {
        return shape;
    }
    // Opaque mix avoids polyline segment-joint double blending.
    let color = p.k.c.bg_base.mix(ember_tone(p, 0.85 - 0.5 * u), fade);
    let [x, y] = flight.to();
    let points: Vec<[f32; 2]> = (0..36)
        .map(|n| {
            let a = n as f32 / 36.0 * std::f32::consts::TAU;
            [x + r * a.cos(), y + r * a.sin()]
        })
        .collect();
    shape.stroke_with(points, true, width, LineCap::Round, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calm_landings_throw_nothing_and_hard_ones_spray() {
        let calm = Flight::new([0.0, 0.0], [200.0, 100.0], 0.0, 0.12, 13.0, 0);
        assert!(sprayed(&calm, 0).is_empty());
        let hard = Flight::new([0.0, 0.0], [200.0, 100.0], 0.0, 0.7, 20.0, 0);
        let sparks = sprayed(&hard, 0);
        assert!(!sparks.is_empty());
        assert!(sparks.iter().all(|s| s.life >= 0.12 && s.life <= 0.28));
    }

    #[test]
    fn a_spark_slows_and_falls() {
        let spark = Spark {
            at: [0.0, 0.0],
            v: [400.0, -300.0],
            life: 0.25,
            heat: 0.8,
            size: 1.5,
        };
        let (_, early) = spark.at(0.01);
        let (_, late) = spark.at(0.24);
        assert!(late[0].abs() < early[0].abs());
        assert!(late[1] > early[1], "gravity pulls it down");
    }
}

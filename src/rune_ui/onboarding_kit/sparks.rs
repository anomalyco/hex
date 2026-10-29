//! What a hard landing throws off, from the site hero's `hitCard`
//! (`site/src/shout/fx.js`) and its renderer (`canvas2d.js`): past
//! intensity 0.4 a spray of sparks glances back off the card, and past 0.42
//! an impact ring opens where the word hit. Calm landings throw nothing.
//!
//! The site steps each spark per frame; here each is a function of the
//! clock, the drag and gravity solved in closed form.

use rune::{Color, El, LineCap, Shape, canvas};

use super::super::controls::Paint;
use super::flight::{Flight, ember, srgb};

/// `look.sparkDrag` and `look.sparkGravity`.
const DRAG: f32 = 4.2;
const GRAVITY: f32 = 380.0;
/// Half the streak a spark draws along its velocity: the shutter (0.75 of
/// a 60 Hz frame) times `sparkStretch` 1.6, halved.
const STREAK: f32 = 0.75 / 60.0 * 1.6 * 0.5;
const RING_LIFE: f32 = 0.22;
const SPARKS_FROM: f32 = 0.40;
const RING_FROM: f32 = 0.42;

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The site's `rnd(a, b)`, steady per spark.
fn rnd(seed: usize, a: f32, b: f32) -> f32 {
    let x = ((seed as f32 * 12.9898 + 4.1).sin() * 43_758.545)
        .fract()
        .abs();
    lerp(a, b, x)
}

/// One spark as it leaves the impact.
struct Spark {
    at: [f32; 2],
    v: [f32; 2],
    life: f32,
    heat: f32,
    size: f32,
}

/// The sparks `flight` throws when it lands; `index` seeds them.
fn sprayed(flight: &Flight, index: usize) -> Vec<Spark> {
    let i = flight.intensity();
    if i <= SPARKS_FROM {
        return Vec::new();
    }
    let [ux, uy] = flight.heading();
    let [x, y] = flight.to();
    let back = (-uy).atan2(-ux);
    let count = lerp(4.0, 16.0, i).round() as usize;
    (0..count)
        .map(|n| {
            let seed = index * 97 + n * 7;
            let graze = if n % 3 == 0 {
                if n % 2 == 0 { 1.25 } else { -1.25 }
            } else {
                rnd(seed, -0.75, 0.75)
            };
            let a = back + graze;
            let speed = rnd(seed + 1, 180.0, 720.0) * lerp(0.55, 1.15, i);
            Spark {
                at: [x + rnd(seed + 2, -3.0, 3.0), y + rnd(seed + 3, -3.0, 3.0)],
                v: [
                    a.cos() * speed + ux * 60.0,
                    a.sin() * speed - rnd(seed + 4, 20.0, 100.0),
                ],
                life: rnd(seed + 5, 0.14, 0.36),
                heat: rnd(seed + 6, 0.45, 0.95),
                size: rnd(seed + 7, 1.3, 2.3),
            }
        })
        .collect()
}

impl Spark {
    /// Where it is and how fast it's going `t` seconds out.
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

/// Every impact the landings in `flights` throw at `now`, drawn over a
/// stage `size` points across. Nothing under Reduce Motion.
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
            if t < 0.0 || t > 0.4 {
                continue;
            }
            shape = ring(shape, p, flight, t);
            for spark in sprayed(flight, index) {
                if t >= spark.life {
                    continue;
                }
                let left = 1.0 - t / spark.life;
                let ([x, y], [vx, vy]) = spark.at(t);
                let color = glow(p, ember(spark.heat * left * 1.1), left);
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

/// The ring opening where `flight` hit, `t` seconds after.
fn ring(shape: Shape, p: Paint, flight: &Flight, t: f32) -> Shape {
    let i = flight.intensity();
    if i <= RING_FROM || t >= RING_LIFE {
        return shape;
    }
    let u = t / RING_LIFE;
    let r = lerp(24.0, 68.0, i) * (1.0 - (1.0 - u).powi(3));
    let width = lerp(1.6, 2.8, i) * (1.0 - u * 0.7);
    let k = lerp(0.35, 0.75, i) * (1.0 - u) * (1.0 - u);
    let near_white = [1.0, 0.96, 0.9];
    let orange = [1.0, 0.48, 0.22];
    let c = |n: usize| lerp(near_white[n], orange[n], u);
    let color = glow(p, srgb(c(0), c(1), c(2), 1.0), k);
    let [x, y] = flight.to();
    let points: Vec<[f32; 2]> = (0..=40)
        .map(|n| {
            let a = n as f32 / 40.0 * std::f32::consts::TAU;
            [x + r * a.cos(), y + r * a.sin()]
        })
        .collect();
    shape.stroke(points, width, color)
}

/// The site adds its sparks (`lighter`); on a light page, where adding
/// light shows nothing, they're drawn a little stronger instead.
fn glow(p: Paint, color: Color, alpha: f32) -> Color {
    let strength = if p.k.dark { 1.0 } else { 0.85 };
    color.alpha((alpha * strength).clamp(0.0, 1.0))
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
        assert_eq!(sparks.len(), 12);
        assert!(sparks.iter().all(|s| s.life >= 0.14 && s.life <= 0.36));
    }

    #[test]
    fn a_spark_slows_and_falls() {
        let spark = Spark {
            at: [0.0, 0.0],
            v: [400.0, -300.0],
            life: 0.3,
            heat: 0.8,
            size: 2.0,
        };
        let (_, early) = spark.at(0.01);
        let (_, late) = spark.at(0.3);
        assert!(late[0].abs() < early[0].abs());
        assert!(late[1] > early[1], "gravity pulls it down");
    }
}

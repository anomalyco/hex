//! The finger's path in pitches (`SwipeDecoder.Gesture`), where it turned or paused
//! (`SwipeDecoder.evidence`), and how a word's keys account for that (`evidenceCost`).

use super::Parameters;
use super::geometry::{Point, distance, max, min, normalized, resample, sort_by_less, squared};

/// The finger's path in pitches without repeated points, resampled, its shape, and where it
/// turned or paused.
pub(crate) struct Gesture {
    pub raw: Vec<Point>,
    pub length: f64,
    pub points: Vec<Point>,
    pub shape: Vec<Point>,
    pub evidence: Vec<Evidence>,
    /// Whether the path's times showed where it paused: without them a key the word goes
    /// straight through can't have shown itself.
    pub timed: bool,
}

/// A point where the finger turned or slowed: a key it meant, most likely. `strength` is 0
/// to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Evidence {
    pub point: Point,
    pub strength: f64,
}

impl Gesture {
    pub fn new(
        path: &[(f64, f64)],
        times: Option<&[f64]>,
        pitch: Point,
        p: &Parameters,
    ) -> Option<Self> {
        let times = times.filter(|t| t.len() == path.len());
        let mut raw: Vec<Point> = Vec::new();
        let mut length = 0.0;
        let mut stamps: Vec<f64> = Vec::new();
        for (i, &(x, y)) in path.iter().enumerate() {
            let v = Point::new(x / pitch.x, y / pitch.y);
            if let Some(&last) = raw.last() {
                let step = distance(last, v);
                if !(step > 1e-4) {
                    continue;
                }
                length += step;
            }
            raw.push(v);
            // A still finger sends no points: the time it stayed goes to the next one's step.
            if let Some(times) = times {
                let last = stamps.last().copied().unwrap_or(times[i]);
                stamps.push(max(times[i], last));
            }
        }
        if raw.len() < 2 || !(length > 0.0) {
            return None;
        }
        let mut points = vec![Point::default(); p.samples];
        resample(&raw, length, &mut points);
        let shape = normalized(&points);
        let duration =
            stamps.last().copied().unwrap_or(0.0) - stamps.first().copied().unwrap_or(0.0);
        let timed = duration > 0.02;
        let evidence = evidence(&raw, timed.then_some(stamps.as_slice()), length, p);
        Some(Self {
            raw,
            length,
            points,
            shape,
            evidence,
            timed,
        })
    }
}

/// The path's turns and pauses, in order along it: the path is read every `evidence_step`
/// pitches; a point scores by how sharply the path turns there and how long the finger took
/// over it against the rest of the swipe; the strongest points at least `evidence_spacing`
/// apart, away from the ends, count.
fn evidence(raw: &[Point], times: Option<&[f64]>, length: f64, p: &Parameters) -> Vec<Evidence> {
    // Swift's `Int(_:)` truncates toward zero, as `as` does for these finite values.
    let count = (length / p.evidence_step) as usize + 1;
    if count < 5 {
        return Vec::new();
    }
    let mut points = vec![Point::default(); count];
    resample(raw, length, &mut points);
    let step = length / (count - 1) as f64;
    let mut score = vec![0.0; count];
    let k = 1.max((p.turn_window / step).round() as usize);
    if count > 2 * k {
        for i in k..(count - k) {
            let a = Point::new(points[i].x - points[i - k].x, points[i].y - points[i - k].y);
            let b = Point::new(points[i + k].x - points[i].x, points[i + k].y - points[i].y);
            let angle = (a.x * b.y - a.y * b.x).abs().atan2(a.x * b.x + a.y * b.y) * 180.0
                / std::f64::consts::PI;
            score[i] = min(
                1.0,
                max(0.0, (angle - p.turn_from) / (p.turn_full - p.turn_from)),
            );
        }
    }
    let guard = (p.evidence_guard / step).ceil() as usize;
    if count <= 2 * guard + 1 {
        return Vec::new();
    }
    if let Some(times) = times {
        // The time at each point read, from the raw points' times along the path.
        let mut at = vec![0.0; count];
        let mut segment = 0;
        let mut walked = 0.0;
        let mut span = distance(raw[0], raw[1]);
        for (i, slot) in at.iter_mut().enumerate() {
            let target = step * i as f64;
            while walked + span < target && segment + 2 < raw.len() {
                walked += span;
                segment += 1;
                span = distance(raw[segment], raw[segment + 1]);
            }
            let t = if span > 0.0 {
                min(1.0, max(0.0, (target - walked) / span))
            } else {
                0.0
            };
            *slot = times[segment] + t * (times[(segment + 1).min(raw.len() - 1)] - times[segment]);
        }
        let w = 1.max((p.dwell_window / step).round() as usize);
        let mut per_pitch = vec![0.0; count];
        for (i, slot) in per_pitch.iter_mut().enumerate() {
            let (lo, hi) = (i.saturating_sub(w), (count - 1).min(i + w));
            *slot = (at[hi] - at[lo]) / (step * (hi - lo) as f64);
        }
        let mut middle = per_pitch[guard..(count - guard)].to_vec();
        sort_by_less(&mut middle, |a, b| a < b);
        let median = middle[middle.len() / 2];
        if median > 0.0 {
            let (from, full) = (p.dwell_from.ln(), p.dwell_full.ln());
            for i in guard..(count - guard) {
                if per_pitch[i] > 0.0 {
                    let dwell = min(
                        1.0,
                        max(0.0, ((per_pitch[i] / median).ln() - from) / (full - from)),
                    );
                    score[i] = max(score[i], dwell);
                }
            }
        }
    }
    // The strongest first, each keeping its neighbours within the spacing out.
    let spacing = 1.max((p.evidence_spacing / step).round() as usize);
    let mut order: Vec<usize> = (guard..(count - guard))
        .filter(|&i| score[i] >= p.evidence_floor)
        .collect();
    sort_by_less(&mut order, |&a, &b| {
        if score[a] != score[b] {
            score[a] > score[b]
        } else {
            a < b
        }
    });
    let mut taken: Vec<usize> = Vec::new();
    for i in order {
        if taken.iter().all(|&t| t.abs_diff(i) >= spacing) {
            taken.push(i);
        }
    }
    taken.sort();
    taken
        .into_iter()
        .map(|i| Evidence {
            point: points[i],
            strength: score[i],
        })
        .collect()
}

/// Nats for how well a word's keys (in pitches) account for the path's turns and pauses, in
/// order: an edit distance where a key accounts for the turns or pauses near it (by how far
/// it lies from each), a turn or pause no key accounts for costs by its strength, and a key
/// with none costs `corner_key_cost` where the word turns, `straight_key_cost` where it goes
/// straight on (and the path's times would have shown a pause). The first and last keys cost
/// nothing without one. `rows` are two rows of `evidence.len() + 1`.
pub(crate) fn evidence_cost(
    keys: &[Point],
    gesture: &Gesture,
    p: &Parameters,
    rows: &mut (Vec<f64>, Vec<f64>),
) -> f64 {
    let evidence = &gesture.evidence;
    let n = evidence.len();
    let m = keys.len();
    if m < 2 {
        return 0.0;
    }
    let reach = p.evidence_reach * p.evidence_reach;
    let spread = 2.0 * p.evidence_sigma * p.evidence_sigma;
    let straight = (p.straight_angle * std::f64::consts::PI / 180.0).cos();
    let missing = |i: usize| -> f64 {
        if i == 0 || i >= m - 1 {
            return 0.0;
        }
        let a = Point::new(keys[i].x - keys[i - 1].x, keys[i].y - keys[i - 1].y);
        let b = Point::new(keys[i + 1].x - keys[i].x, keys[i + 1].y - keys[i].y);
        let (la, lb) = (
            (a.x * a.x + a.y * a.y).sqrt(),
            (b.x * b.x + b.y * b.y).sqrt(),
        );
        if !(la > 1e-6) || !(lb > 1e-6) {
            return 0.0;
        }
        let goes_straight = (a.x * b.x + a.y * b.y) / (la * lb) >= straight;
        if goes_straight {
            if gesture.timed {
                p.straight_key_cost
            } else {
                0.0
            }
        } else {
            p.corner_key_cost
        }
    };
    let accounts = |i: usize, j: usize| -> f64 {
        let d = squared(keys[i], evidence[j].point);
        if d <= reach {
            d / spread
        } else {
            f64::INFINITY
        }
    };
    let unexplained = |j: usize| p.unexplained_cost * evidence[j].strength;
    // Row i: keys 0...i placed; column j: the first j turns and pauses read.
    rows.0[0] = 0.0;
    for j in 0..n {
        rows.0[j + 1] = rows.0[j] + min(unexplained(j), accounts(0, j));
    }
    for i in 1..m {
        let skip = missing(i);
        rows.1[0] = rows.0[0] + skip;
        for j in 0..n {
            let near = accounts(i, j);
            let mut best = rows.0[j + 1] + skip;
            best = min(best, rows.0[j] + near);
            best = min(best, rows.1[j] + min(unexplained(j), near));
            rows.1[j + 1] = best;
        }
        std::mem::swap(&mut rows.0, &mut rows.1);
    }
    rows.0[n]
}

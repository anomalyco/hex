//! "Muffle" while dictating: other apps keep playing, but as if through a
//! wall, the way a club sounds from its bathroom. A 400 Hz low-pass with
//! a resonant thump as the door shuts, the level at 40 %, a small damped
//! room behind it, and a soft clip; opening the door sweeps the filter back
//! up over 200 ms. The macOS adapter
//! (`recording_environment/muffle.rs`) feeds it from a Core Audio process
//! tap; this module is the pure signal path, the same on every OS.
use std::f32::consts::{SQRT_2, TAU};

/// Where the wall sits: everything above this is what the door keeps out.
const CUTOFF_HZ: f32 = 400.0;
/// The filter is out of the way once it reaches this.
const OPEN_HZ: f32 = 20_000.0;
/// Other audio's level behind the door.
const GAIN: f32 = 0.4;
const BUTTERWORTH_Q: f32 = 1.0 / SQRT_2;
/// The door shuts with a resonant thump that settles in 150 ms.
const CLOSE_Q: f32 = 3.0;
const CLOSE_SECONDS: f32 = 0.15;
/// The level and the dry-to-muffled crossfade take 80 ms.
const CLOSE_FADE_SECONDS: f32 = 0.08;
/// Opening sweeps the cutoff back up, resonant again, over 200 ms.
const OPEN_Q: f32 = 4.0;
const OPEN_SECONDS: f32 = 0.2;
/// How front-loaded a sweep is: most of it in its first half.
const SWEEP_EASE: f32 = 4.0;
/// A small tiled room: four combs and two allpasses per channel, the right
/// ones 23 samples longer, as Freeverb spreads its stereo.
const COMBS: [usize; 4] = [1116, 1277, 1422, 1557];
const ALLPASSES: [usize; 2] = [556, 341];
const STEREO_SPREAD: usize = 23;
const REVERB_FEEDBACK: f32 = 0.72;
const REVERB_DAMP: f32 = 0.45;
const REVERB_LEVEL: f32 = 0.0875;
/// Above this the output bends instead of clipping.
const CLIP_KNEE: f32 = 0.9;
/// Filter coefficients move this often during a sweep.
const SWEEP_BLOCK: usize = 32;

/// The signal path for one output, every channel together.
pub(crate) struct Muffler {
    rate: f32,
    channels: Vec<Channel>,
    sweep: Option<Sweep>,
    /// The filter's state once the current sweep ends.
    rest: (f32, f32),
    /// Dry to muffled, 0 to 1, with its per-sample step and target.
    wet: Ramp,
    gain: Ramp,
}

impl Muffler {
    /// Starts with the door shutting: a dry output that muffles over
    /// [`CLOSE_SECONDS`].
    pub(crate) fn new(rate: f32, channels: usize) -> Self {
        let rate = rate.max(8_000.0);
        let channels = (0..channels.max(1))
            .map(|index| Channel::new(rate, if index % 2 == 1 { STEREO_SPREAD } else { 0 }))
            .collect();
        let mut muffler = Self {
            rate,
            channels,
            sweep: None,
            rest: (OPEN_HZ, BUTTERWORTH_Q),
            wet: Ramp::at(0.0),
            gain: Ramp::at(1.0),
        };
        muffler.close();
        muffler
    }

    fn close(&mut self) {
        self.sweep = Some(Sweep::new(
            self.rate,
            CLOSE_SECONDS,
            (CUTOFF_HZ, CLOSE_Q),
            (CUTOFF_HZ, BUTTERWORTH_Q),
        ));
        self.rest = (CUTOFF_HZ, BUTTERWORTH_Q);
        self.wet.go(1.0, CLOSE_FADE_SECONDS * self.rate);
        self.gain.go(GAIN, CLOSE_FADE_SECONDS * self.rate);
        for channel in &mut self.channels {
            channel.filter.reset();
        }
    }

    /// Opens the door: the filter sweeps back up and the level returns.
    pub(crate) fn open(&mut self) {
        if self.rest.0 >= OPEN_HZ {
            return;
        }
        let from = self.sweep.as_ref().map_or(self.rest, Sweep::current);
        self.sweep = Some(Sweep::new(
            self.rate,
            OPEN_SECONDS,
            (from.0, OPEN_Q),
            (OPEN_HZ, BUTTERWORTH_Q),
        ));
        self.rest = (OPEN_HZ, BUTTERWORTH_Q);
        self.wet.go(0.0, OPEN_SECONDS * self.rate);
        self.gain.go(1.0, OPEN_SECONDS * self.rate);
    }

    /// Whether the door is open again: the output is dry and at full
    /// level, so the tap can go.
    pub(crate) fn is_open(&self) -> bool {
        self.rest.0 >= OPEN_HZ && self.sweep.is_none() && self.wet.done() && self.gain.done()
    }

    /// Muffles `planes` in place, one slice per channel, all the same length.
    pub(crate) fn process(&mut self, planes: &mut [&mut [f32]]) {
        let frames = planes.iter().map(|plane| plane.len()).min().unwrap_or(0);
        let mut start = 0;
        while start < frames {
            let end = (start + SWEEP_BLOCK).min(frames);
            self.retune();
            let (wet, gain) = (self.wet.value, self.gain.value);
            for (channel, plane) in self.channels.iter_mut().zip(planes.iter_mut()) {
                for sample in &mut plane[start..end] {
                    *sample = channel.muffle(*sample, wet, gain);
                }
            }
            for _ in start..end {
                self.wet.step();
                self.gain.step();
            }
            if let Some(sweep) = &mut self.sweep {
                sweep.advance(end - start);
                if sweep.finished() {
                    self.sweep = None;
                }
            }
            start = end;
        }
    }

    fn retune(&mut self) {
        let (hz, q) = self.sweep.as_ref().map_or(self.rest, Sweep::current);
        for channel in &mut self.channels {
            channel.filter.set(hz, q, self.rate);
        }
    }
}

/// A value moving linearly to a target over a number of samples.
struct Ramp {
    value: f32,
    target: f32,
    step: f32,
}

impl Ramp {
    fn at(value: f32) -> Self {
        Self {
            value,
            target: value,
            step: 0.0,
        }
    }

    fn go(&mut self, target: f32, samples: f32) {
        self.target = target;
        self.step = (target - self.value) / samples.max(1.0);
    }

    fn step(&mut self) {
        if self.done() {
            return;
        }
        self.value += self.step;
        if (self.step > 0.0 && self.value >= self.target)
            || (self.step < 0.0 && self.value <= self.target)
        {
            self.value = self.target;
        }
    }

    fn done(&self) -> bool {
        self.value == self.target
    }
}

/// A move of the filter's cutoff and Q over time: the cutoff on a log
/// scale with a front-loaded ease, the Q straight.
struct Sweep {
    from: (f32, f32),
    to: (f32, f32),
    elapsed: usize,
    total: usize,
}

impl Sweep {
    fn new(rate: f32, seconds: f32, from: (f32, f32), to: (f32, f32)) -> Self {
        Self {
            from,
            to,
            elapsed: 0,
            total: ((seconds * rate) as usize).max(1),
        }
    }

    fn advance(&mut self, frames: usize) {
        self.elapsed = (self.elapsed + frames).min(self.total);
    }

    fn finished(&self) -> bool {
        self.elapsed >= self.total
    }

    fn current(&self) -> (f32, f32) {
        let t = self.elapsed as f32 / self.total as f32;
        let eased = (1.0 - (-SWEEP_EASE * t).exp()) / (1.0 - (-SWEEP_EASE).exp());
        let hz = self.from.0 * (self.to.0 / self.from.0).powf(eased);
        let q = self.from.1 + (self.to.1 - self.from.1) * t;
        (hz, q)
    }
}

struct Channel {
    filter: Lowpass,
    reverb: Reverb,
}

impl Channel {
    fn new(rate: f32, spread: usize) -> Self {
        Self {
            filter: Lowpass::default(),
            reverb: Reverb::new(rate, spread),
        }
    }

    #[inline]
    fn muffle(&mut self, dry: f32, wet: f32, gain: f32) -> f32 {
        let behind_the_wall = self.filter.process(dry) * gain;
        let room = self.reverb.process(behind_the_wall) * REVERB_LEVEL * wet;
        soft_clip(dry * (1.0 - wet) + behind_the_wall * wet + room)
    }
}

/// RBJ cookbook low-pass, transposed direct form II.
#[derive(Default)]
struct Lowpass {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Lowpass {
    fn set(&mut self, hz: f32, q: f32, rate: f32) {
        let w0 = TAU * hz.clamp(1.0, rate * 0.45) / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q.max(0.1));
        let a0 = 1.0 + alpha;
        self.b0 = (1.0 - cos) / 2.0 / a0;
        self.b1 = (1.0 - cos) / a0;
        self.b2 = self.b0;
        self.a1 = -2.0 * cos / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// Freeverb's shape, smaller: parallel damped combs into series allpasses.
struct Reverb {
    combs: Vec<Comb>,
    allpasses: Vec<Allpass>,
}

struct Comb {
    buffer: Vec<f32>,
    index: usize,
    store: f32,
}

struct Allpass {
    buffer: Vec<f32>,
    index: usize,
}

impl Reverb {
    fn new(rate: f32, spread: usize) -> Self {
        let scale = rate / 44_100.0;
        let length = |base: usize| (((base + spread) as f32) * scale).round().max(1.0) as usize;
        Self {
            combs: COMBS
                .iter()
                .map(|&base| Comb {
                    buffer: vec![0.0; length(base)],
                    index: 0,
                    store: 0.0,
                })
                .collect(),
            allpasses: ALLPASSES
                .iter()
                .map(|&base| Allpass {
                    buffer: vec![0.0; length(base)],
                    index: 0,
                })
                .collect(),
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let mut out = 0.0;
        for comb in &mut self.combs {
            let y = comb.buffer[comb.index];
            comb.store = y * (1.0 - REVERB_DAMP) + comb.store * REVERB_DAMP;
            comb.buffer[comb.index] = x + comb.store * REVERB_FEEDBACK;
            comb.index = (comb.index + 1) % comb.buffer.len();
            out += y;
        }
        for allpass in &mut self.allpasses {
            let delayed = allpass.buffer[allpass.index];
            allpass.buffer[allpass.index] = out + delayed * 0.5;
            allpass.index = (allpass.index + 1) % allpass.buffer.len();
            out = delayed - out;
        }
        out
    }
}

/// Straight up to the knee, then a tanh bend toward 1.
#[inline]
fn soft_clip(x: f32) -> f32 {
    let magnitude = x.abs();
    if magnitude <= CLIP_KNEE {
        return x;
    }
    let bent = CLIP_KNEE + (1.0 - CLIP_KNEE) * ((magnitude - CLIP_KNEE) / (1.0 - CLIP_KNEE)).tanh();
    bent.copysign(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn tone(hz: f32, seconds: f32) -> Vec<f32> {
        (0..(seconds * RATE) as usize)
            .map(|n| (TAU * hz * n as f32 / RATE).sin() * 0.5)
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|x| x * x).sum::<f32>() / samples.len().max(1) as f32).sqrt()
    }

    fn run(muffler: &mut Muffler, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.chunks_mut(512).zip(right.chunks_mut(512)) {
            muffler.process(&mut [l, r]);
        }
    }

    #[test]
    fn highs_vanish_and_lows_stay_once_the_door_is_shut() {
        let mut muffler = Muffler::new(RATE, 2);
        let mut cymbal = tone(4_000.0, 1.0);
        let mut bass = tone(80.0, 1.0);
        let dry = rms(&cymbal[24_000..]);
        run(&mut muffler, &mut cymbal, &mut bass);
        let settled = 24_000..;
        assert!(
            rms(&cymbal[settled.clone()]) < dry * 0.02,
            "4 kHz is behind the wall"
        );
        let bass_level = rms(&bass[settled]);
        assert!(
            (bass_level / dry - GAIN).abs() < 0.08,
            "80 Hz passes at the muffled level, got {}",
            bass_level / dry
        );
    }

    #[test]
    fn opening_restores_the_dry_signal() {
        let mut muffler = Muffler::new(RATE, 2);
        let mut left = tone(4_000.0, 0.5);
        let mut right = left.clone();
        run(&mut muffler, &mut left, &mut right);
        assert!(!muffler.is_open());
        muffler.open();
        let mut left = tone(4_000.0, 0.5);
        let mut right = left.clone();
        let dry = rms(&left[12_000..]);
        run(&mut muffler, &mut left, &mut right);
        assert!(muffler.is_open());
        let after = rms(&left[12_000..]);
        assert!((after / dry - 1.0).abs() < 0.02, "got {after} of {dry}");
        assert!(
            left[12_000..].iter().all(|x| x.abs() <= 1.0),
            "the room's tail never clips"
        );
    }

    #[test]
    fn silence_stays_silent_and_loud_input_bends_below_one() {
        let mut muffler = Muffler::new(RATE, 1);
        let mut quiet = vec![0.0; 4_096];
        muffler.process(&mut [&mut quiet]);
        assert!(quiet.iter().all(|x| *x == 0.0));
        let mut loud = vec![3.0; 4_096];
        let mut muffler = Muffler::new(RATE, 1);
        muffler.process(&mut [&mut loud]);
        assert!(loud.iter().all(|x| x.abs() <= 1.0 && x.is_finite()));
        assert_eq!(soft_clip(0.5), 0.5);
        assert!(soft_clip(1.0) < 1.0 && soft_clip(-1.0) > -1.0);
        assert!(soft_clip(10.0) <= 1.0 && soft_clip(-10.0) >= -1.0);
    }

    #[test]
    fn the_close_sweep_relaxes_from_the_thump_to_butterworth() {
        let mut sweep = Sweep::new(
            RATE,
            CLOSE_SECONDS,
            (CUTOFF_HZ, CLOSE_Q),
            (CUTOFF_HZ, BUTTERWORTH_Q),
        );
        assert_eq!(sweep.current(), (CUTOFF_HZ, CLOSE_Q));
        sweep.advance(usize::MAX / 2);
        assert!(sweep.finished());
        let (hz, q) = sweep.current();
        assert!((hz - CUTOFF_HZ).abs() < 0.01 && (q - BUTTERWORTH_Q).abs() < 1e-6);
        let mut open = Sweep::new(
            RATE,
            OPEN_SECONDS,
            (CUTOFF_HZ, OPEN_Q),
            (OPEN_HZ, BUTTERWORTH_Q),
        );
        open.advance(open.total / 2);
        let (midway, _) = open.current();
        assert!(
            midway > 5_000.0,
            "the sweep is front-loaded, got {midway} Hz at halfway"
        );
    }
}

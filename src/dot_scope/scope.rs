//! Dot scope's simulation: steps the voice, the braided traces, and the
//! phosphor at a fixed 240 Hz and yields one (brightness, freshness) pair
//! per dot.

use std::f32::consts::{PI, TAU};

use super::{AutoLevel, Processing, Tuning, fixed_level};
use crate::spring::{REDUCED_FADE, Spec, Spring};

const STEP: f32 = 1.0 / 240.0;
const HISTORY: usize = 512;
const SAMPLES_PER_COLUMN: usize = 3;
const SPEEDS: [f32; 4] = [0.8, 1.25, 1.85, 2.6];
const BASE: [f32; 4] = [0.55, 0.42, 0.3, 0.2];
const SPATIAL: [f32; 4] = [1.4, 2.4, 3.8, 5.6];
const START_WIDTH: f32 = 30.0;
const ENTER: f32 = 0.42;
const WIPE: f32 = 0.3;
const PASS_INSET: f32 = 9.0;
const PHOSPHOR_HEAD: f32 = 0.15;
const BREATH_PERIOD: f32 = 5.5;
const HIDDEN_SCALE: f32 = 0.9;
const HIDDEN_SOFTNESS: f32 = 3.0;
/// How much the resting helix swells while Flow and Pulse work.
const WORK_HELIX: f32 = 2.6;
/// Flow's bands sit this far apart, as a share of the processing width.
const FLOW_SPACING: f32 = 0.7;
/// The recording carrier's slowest turn, Hz.
const CARRIER_REST: f32 = 0.14;

/// HUD Lab's synthetic voice: phrases of syllables at about 4.5 a second
/// with breaths between, as a 0–1 level.
pub fn demo_level(time: f32) -> f32 {
    const PHRASE: f32 = 2.4;
    const SPOKEN: f32 = 1.7;
    let phrase = (time / PHRASE).floor();
    let t = time - phrase * PHRASE;
    if t > SPOKEN {
        return 0.04;
    }
    let shape = (PI * t / SPOKEN).sin().powf(0.4);
    let rate = TAU * (4.3 + 0.6 * (phrase * 1.7).sin());
    let syllable = 0.5 + 0.5 * (rate * t + phrase).sin();
    let stress = 0.75 + 0.25 * (TAU * 1.1 * t + 2.0 * phrase).sin();
    (0.08 + 0.82 * shape * syllable.powf(1.6) * stress).clamp(0.0, 1.0)
}

/// [`demo_level`] as a laptop microphone at talking distance hears it:
/// breaths near −57 dB and stressed syllables near −32 dB, dBFS.
pub fn demo_loudness(time: f32) -> f32 {
    -58.0 + 28.0 * demo_level(time)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Recording,
    Processing,
    Done,
    Failed,
}

/// What the HUD wants this frame.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub stage: Stage,
    pub visible: bool,
    /// The microphone's loudness, dBFS ([`super::loudness`]).
    pub loudness: f32,
    pub reduce_motion: bool,
}

/// The plate's pose and the field's global terms, for the shader.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub width: f32,
    pub height: f32,
    pub opacity: f32,
    pub scale: f32,
    pub softness: f32,
    pub cool: f32,
    pub fail: f32,
    pub glow: f32,
    pub grid: f32,
    pub live: f32,
    /// How far the plate has grown in, 0 hidden to 1 shown, past 1 as it
    /// bounces: what an island grows by.
    pub reveal: f32,
}

#[derive(Clone, Copy, Default)]
struct Voice {
    level: f32,
    reservoir: f32,
    energy: f32,
    kick: f32,
    kick_velocity: f32,
    /// The prototype's 16 bands, all the level: Hex meters no spectrum.
    band: f32,
}

impl Voice {
    fn step(&mut self, raw: f32, h: f32) {
        let previous = self.level;
        let time = if raw > self.level { 0.035 } else { 0.095 };
        self.level += (raw - self.level) * (1.0 - (-h / time).exp());
        let rise = ((self.level - previous) / h).max(0.0);
        self.reservoir += (1.75 * self.level * self.level - self.reservoir / 1.8) * h;
        self.energy = 1.0 - (-self.reservoir * 1.45).exp();
        let (w, z) = (TAU * 1.3, 0.22);
        self.kick_velocity += (-w * w * self.kick - 2.0 * z * w * self.kick_velocity
            + 15.0 * rise
            + 2.2 * self.level)
            * h;
        self.kick += self.kick_velocity * h;
        let time = if raw > self.band { 0.025 } else { 0.14 };
        self.band += (raw - self.band) * (1.0 - (-h / time).exp());
    }
}

struct Trace {
    envelope: f32,
    amplitude: f32,
    amplitude_velocity: f32,
    glow_fast: f32,
    glow_spring: f32,
    glow_spring_velocity: f32,
    glow_slow: f32,
    glow: f32,
    phases: [f32; 4],
    history: Vec<f32>,
    head: usize,
}

impl Trace {
    fn new(phases: [f32; 4]) -> Self {
        Self {
            envelope: 0.0,
            amplitude: 0.0,
            amplitude_velocity: 0.0,
            glow_fast: 0.0,
            glow_spring: 0.0,
            glow_spring_velocity: 0.0,
            glow_slow: 0.0,
            glow: 0.0,
            phases,
            history: vec![0.0; HISTORY * 5],
            head: 0,
        }
    }

    fn tick(&mut self, voice: &Voice, live: f32, speed: f32, tuning: &Tuning, h: f32) {
        let follow = |value: &mut f32, target: f32, rise: f32, fall: f32| {
            let time = if target > *value { rise } else { fall };
            *value += (target - *value) * (1.0 - (-h / time).exp());
        };
        follow(&mut self.envelope, voice.level, 0.022, tuning.release);
        let (w, z) = (TAU * tuning.response, 0.52);
        self.amplitude_velocity +=
            (-w * w * (self.amplitude - self.envelope) - 2.0 * z * w * self.amplitude_velocity) * h;
        self.amplitude += self.amplitude_velocity * h;
        follow(&mut self.glow_fast, voice.level, 0.045, 0.13);
        let (gw, gz) = (TAU * 3.6, 0.5);
        self.glow_spring_velocity += (-gw * gw * (self.glow_spring - self.glow_fast)
            - 2.0 * gz * gw * self.glow_spring_velocity)
            * h;
        self.glow_spring += self.glow_spring_velocity * h;
        follow(&mut self.glow_slow, voice.level, 0.15, 0.7);
        let swell = 0.8 * (self.glow_spring - 0.25).max(0.0) + 0.04 * voice.kick.max(-0.5);
        self.glow = (swell.max(0.0) + 0.42 * self.glow_slow + 0.2 * voice.energy).min(1.15) * live;
        let go = 0.6 + 1.1 * (self.envelope * 1.6).min(1.0);
        for (phase, speed_hz) in self.phases.iter_mut().zip(SPEEDS) {
            *phase = (*phase + h * TAU * speed_hz * go * speed) % TAU;
        }
        // With flat bands each group is the mean, so the prototype's
        // `g / (mean + 0.06)` emphasis is the same for every component.
        let emphasis = 0.35 + 0.65 * (voice.band / (voice.band + 0.06)).min(2.0);
        let offset = self.head * 5;
        let amplitude = self.amplitude.max(0.0) * live;
        self.history[offset] = amplitude;
        for (c, base) in BASE.into_iter().enumerate() {
            self.history[offset + 1 + c] = amplitude * base * emphasis;
        }
        self.head = (self.head + 1) % HISTORY;
    }

    fn past(&self, delay: f32, channel: usize) -> f32 {
        let d = (delay / STEP).min((HISTORY - 2) as f32);
        let i = d.floor() as usize;
        let k = d - i as f32;
        let a = (self.head + 2 * HISTORY - 1 - i) % HISTORY;
        let b = (a + HISTORY - 1) % HISTORY;
        self.history[a * 5 + channel] * (1.0 - k) + self.history[b * 5 + channel] * k
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn bump(d: f32, w: f32) -> f32 {
    (-(d / w) * (d / w)).exp()
}

/// From `start` moving at `speed` to rest at `end` as `t` goes 0 to 1 over
/// [`WIPE`]: a cubic Hermite, so done picks up the motion it interrupts.
fn hermite(start: f32, speed: f32, end: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * start + (t3 - 2.0 * t2 + t) * WIPE * speed + (3.0 * t2 - 2.0 * t3) * end
}

fn pass_position(p: f32) -> f32 {
    0.5 - 0.5 * (PI * p.clamp(0.0, 1.0)).cos()
}

/// Flow's bands or Pulse's rings, as light along the plate.
#[derive(Clone, Copy)]
enum Working {
    /// Bands `spacing` apart, one at `offset`.
    Flow { offset: f32, spacing: f32 },
    /// Rings leaving the middle every `period` at `speed`, `time` in.
    Pulse { time: f32, period: f32, speed: f32 },
}

impl Working {
    /// 0–1 light at `x`, and how far toward the plate's ends it fades.
    fn light(self, x: f32, half_width: f32) -> f32 {
        match self {
            Working::Flow { offset, spacing } => {
                let d = (x - offset + spacing / 2.0).rem_euclid(spacing) - spacing / 2.0;
                bump(d, 7.0)
            }
            Working::Pulse {
                time,
                period,
                speed,
            } => {
                let d = x.abs();
                let mut light = 0.0;
                let mut age = time.rem_euclid(period);
                while age * speed < half_width + 12.0 && age <= time + 1e-4 {
                    let radius = age * speed;
                    let fade = 1.0 - 0.6 * smoothstep(0.3 * half_width, half_width, radius);
                    light += bump(d - radius, 6.0) * fade;
                    age += period;
                }
                light.min(1.0)
            }
        }
    }
}

/// How the trace is drawn this instant: the scanner that unfolds it, the
/// processing beam or the working light, or the done wipe.
enum Envelope {
    Enter {
        reach: f32,
        head: f32,
        unfold: f32,
    },
    /// Sweep: the beam over the ironed trace.
    Process {
        iron: f32,
        front: f32,
        head: f32,
    },
    /// Flow and Pulse: light travels through the turning braid.
    Work {
        working: Working,
        head: f32,
        swell: f32,
        half_width: f32,
    },
    /// Sweep's done: the beam runs to the right end over the dark field.
    Wipe {
        front: f32,
        head: f32,
    },
    /// Flow's and Pulse's done: a front clears the braid behind it, from
    /// the left (Flow) or from the middle out (Pulse), while the light
    /// ahead of it fades.
    Clear {
        front: f32,
        head: f32,
        from_middle: bool,
        working: Working,
        glow: f32,
        swell: f32,
        half_width: f32,
    },
}

struct Scan {
    helix: f32,
    decay: f32,
    tail: f32,
    grid: f32,
    envelope: Envelope,
}

impl Scan {
    /// (amplitude, brightness, swell) at `x`.
    fn at(&self, x: f32) -> (f32, f32, f32) {
        match self.envelope {
            Envelope::Enter {
                reach,
                head,
                unfold,
            } => {
                let d = x.abs();
                let drawn =
                    unfold * (1.0 - smoothstep((reach - 14.0).max(0.0), reach.max(0.001), d));
                let amplitude = drawn
                    * (1.0 - smoothstep((reach - 22.0).max(0.0), (reach - 3.0).max(0.001), d));
                let beam = head * bump(d - reach, 6.0);
                let swell = 1.8 * unfold * (head / 2.8) * bump(d - (reach - 14.0).max(0.0), 14.0);
                (amplitude, amplitude + beam, swell)
            }
            Envelope::Process { iron, front, head } => {
                (iron, iron + head * bump(x - front, 5.0), 0.0)
            }
            Envelope::Work {
                working,
                head,
                swell,
                half_width,
            } => {
                let light = working.light(x, half_width);
                (1.0, 1.0 + head * light, swell * light)
            }
            Envelope::Wipe { front, head } => (0.0, head * bump(x - front, 5.0), 0.0),
            Envelope::Clear {
                front,
                head,
                from_middle,
                working,
                glow,
                swell,
                half_width,
            } => {
                let d = if from_middle { x.abs() } else { x };
                let keep = smoothstep(front - 12.0, front - 2.0, d);
                let light = working.light(x, half_width);
                (
                    keep,
                    keep * (1.0 + glow * light) + head * bump(d - front, 6.0),
                    keep * swell * light,
                )
            }
        }
    }
}

/// The Dot scope simulation.
pub struct Scope {
    tuning: Tuning,
    stage: Stage,
    elapsed: f32,
    front_at_done: f32,
    /// How fast the done front was moving when done arrived, pt/s.
    speed_at_done: f32,
    /// How long it had processed when done arrived, s.
    done_from: f32,
    /// Flow's band offset, pt, travelling right while it works.
    flow_offset: f32,
    /// How far the carrier has handed its turn over to Flow's, 0–1.
    flow_turn: f32,
    auto: AutoLevel,
    /// The level the voice follows, 0–1, before the gain.
    level: f32,
    voices: [Voice; 2],
    traces: [Trace; 2],
    live: f32,
    carrier: f32,
    time: f32,
    steps: u64,
    pending: f32,
    reduce_motion: bool,
    columns: usize,
    rows: usize,
    head: [Vec<f32>; 2],
    tail: [Vec<f32>; 2],
    dots: Vec<[f32; 2]>,
    ys: [Vec<f32>; 2],
    brightness: [Vec<f32>; 2],
    grid: f32,
    width: Spring,
    opacity: Spring,
    scale: Spring,
    softness: Spring,
    cool: Spring,
    fail: Spring,
}

impl Scope {
    pub fn new(tuning: Tuning) -> Self {
        let mut scope = Self {
            tuning: tuning.clamped(),
            stage: Stage::Recording,
            elapsed: 0.0,
            front_at_done: 0.0,
            speed_at_done: 0.0,
            done_from: 0.0,
            flow_offset: 0.0,
            flow_turn: 0.0,
            auto: AutoLevel::default(),
            level: 0.0,
            voices: [Voice::default(); 2],
            traces: [
                Trace::new([0.1, 1.7, 3.3, 4.6]),
                Trace::new([2.4, 0.6, 5.1, 1.9]),
            ],
            live: 0.0,
            carrier: 0.0,
            time: 0.0,
            steps: 0,
            pending: 0.0,
            reduce_motion: false,
            columns: 0,
            rows: 0,
            head: [Vec::new(), Vec::new()],
            tail: [Vec::new(), Vec::new()],
            dots: Vec::new(),
            ys: [Vec::new(), Vec::new()],
            brightness: [Vec::new(), Vec::new()],
            grid: 0.0,
            width: Spring::at(START_WIDTH),
            opacity: Spring::at(0.0),
            scale: Spring::at(HIDDEN_SCALE),
            softness: Spring::at(HIDDEN_SOFTNESS),
            cool: Spring::at(0.0),
            fail: Spring::at(0.0),
        };
        scope.resize();
        scope
    }

    /// Starts a recording from nothing: the scanner unfolds the trace again.
    /// The auto level remembers the speaker.
    pub fn restart(&mut self, reduce_motion: bool) {
        let auto = self.auto;
        *self = Self::new(self.tuning);
        self.auto = auto;
        if reduce_motion {
            self.scale.jump(1.0);
            self.softness.jump(0.0);
            self.width.jump(self.tuning.width);
        }
    }

    pub fn tune(&mut self, tuning: Tuning) {
        self.tuning = tuning.clamped();
        self.resize();
    }

    pub fn tuning(&self) -> Tuning {
        self.tuning
    }

    /// The 0–1 level the voice follows now, before the gain.
    pub fn level(&self) -> f32 {
        self.level
    }

    fn resize(&mut self) {
        let columns = self.tuning.columns as usize;
        let rows = self.tuning.rows as usize;
        if columns == self.columns && rows == self.rows {
            return;
        }
        self.columns = columns;
        self.rows = rows;
        let dots = columns * rows;
        let samples = (columns - 1) * SAMPLES_PER_COLUMN + 1;
        self.head = [vec![0.0; dots], vec![0.0; dots]];
        self.tail = [vec![0.0; dots], vec![0.0; dots]];
        self.dots = vec![[0.0; 2]; dots];
        self.ys = [vec![0.0; samples], vec![0.0; samples]];
        self.brightness = [vec![0.0; samples], vec![0.0; samples]];
    }

    pub fn columns(&self) -> usize {
        self.columns
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Each dot's (brightness, freshness), row by row.
    pub fn dots(&self) -> &[[f32; 2]] {
        &self.dots
    }

    pub fn pose(&self) -> Pose {
        Pose {
            width: self.width.x.max(self.tuning.height),
            height: self.tuning.height,
            opacity: self.opacity.x.clamp(0.0, 1.0),
            scale: self.scale.x.max(0.01),
            softness: self.softness.x.clamp(0.0, HIDDEN_SOFTNESS),
            cool: self.cool.x.clamp(0.0, 1.0),
            fail: self.fail.x.clamp(0.0, 1.0),
            glow: (self.traces[0].glow + self.traces[1].glow) / 2.0 * self.tuning.glow,
            grid: self.grid,
            live: self.live,
            reveal: ((self.scale.x - HIDDEN_SCALE) / (1.0 - HIDDEN_SCALE)).max(0.0),
        }
    }

    /// Whether the scope has faded out after being hidden.
    pub fn settled(&self) -> bool {
        self.opacity.target == 0.0 && self.opacity.x < 0.002
    }

    /// Advances by `dt` seconds in fixed 240 Hz steps.
    pub fn advance(&mut self, dt: f32, frame: Frame) {
        if frame.stage != self.stage {
            if frame.stage == Stage::Done {
                self.arm_done();
            }
            if frame.stage == Stage::Processing {
                self.flow_offset = -self.width.x / 2.0 - 4.0;
            }
            self.stage = frame.stage;
            self.elapsed = 0.0;
        }
        self.reduce_motion = frame.reduce_motion;
        self.pending += dt.clamp(0.0, 0.25);
        while self.pending >= STEP {
            self.pending -= STEP;
            self.step(frame);
        }
    }

    fn step(&mut self, frame: Frame) {
        let h = STEP;
        let tuning = self.tuning;
        let recording = self.stage == Stage::Recording && frame.visible;
        let level = if recording {
            self.level = if tuning.auto_level {
                self.auto.step(
                    frame.loudness,
                    h,
                    tuning.auto_release,
                    tuning.noise_floor,
                )
            } else {
                fixed_level(frame.loudness)
            };
            (self.level * tuning.gain).clamp(0.0, 1.0)
        } else {
            self.level = 0.0;
            0.0
        };
        self.voices[0].step(level, h);
        self.voices[1].step(level * 0.85, h);
        let live_target = if recording { 1.0 } else { 0.0 };
        self.live += (live_target - self.live) * (1.0 - (-h / 0.55).exp());
        let speed = if frame.reduce_motion {
            0.0
        } else {
            tuning.braid_speed
        };
        for (trace, voice) in self.traces.iter_mut().zip(&self.voices) {
            trace.tick(voice, self.live, speed, &tuning, h);
        }
        let energy = self.voices[0].energy.max(self.voices[1].energy);
        let working = matches!(self.stage, Stage::Processing | Stage::Done);
        let flowing = working && tuning.processing == Processing::Flow;
        self.flow_turn += (if flowing { 1.0 } else { 0.0 } - self.flow_turn) * (1.0 - (-h / 0.25).exp());
        if working {
            self.flow_offset += self.flow_speed() * h;
        }
        let rest = CARRIER_REST + 0.22 * energy;
        let turn = rest + (self.flow_turn_hz() - rest) * self.flow_turn;
        self.carrier = (self.carrier + h * TAU * turn * speed) % TAU;

        let width = match self.stage {
            Stage::Recording => tuning.width,
            Stage::Processing | Stage::Done => tuning.processing_width,
            Stage::Failed => self.width.target,
        };
        let cool = if matches!(self.stage, Stage::Processing | Stage::Done) {
            1.0
        } else {
            0.0
        };
        let fail = if self.stage == Stage::Failed {
            1.0
        } else {
            0.0
        };
        let shown = if frame.visible { 1.0 } else { 0.0 };
        let fade = Spec::new(tuning.transition, 0.0);
        let bouncy = Spec::new(tuning.transition, tuning.bounce);
        if frame.reduce_motion {
            self.width.jump(width);
            self.scale.jump(1.0);
            self.softness.jump(0.0);
            self.opacity.step_to(shown, REDUCED_FADE, h);
            self.cool.step_to(cool, REDUCED_FADE, h);
            self.fail.step_to(fail, REDUCED_FADE, h);
        } else {
            self.width.step_to(width, bouncy, h);
            self.opacity.step_to(shown, fade, h);
            self.scale
                .step_to(if frame.visible { 1.0 } else { HIDDEN_SCALE }, bouncy, h);
            self.softness
                .step_to(if frame.visible { 0.0 } else { HIDDEN_SOFTNESS }, fade, h);
            self.cool.step_to(cool, fade, h);
            self.fail.step_to(fail, fade, h);
        }

        self.elapsed += h;
        self.time += h;
        self.steps += 1;
        if self.steps.is_multiple_of(2) {
            self.beam(2.0 * h);
        }
    }

    /// One band or pulse per this many seconds.
    fn work_period(&self) -> f32 {
        2.0 * self.tuning.shimmer_pass.max(0.05)
    }

    fn flow_spacing(&self) -> f32 {
        FLOW_SPACING * self.tuning.processing_width
    }

    fn flow_speed(&self) -> f32 {
        self.flow_spacing() / self.work_period()
    }

    /// The carrier turn that moves the helix at Flow's band speed, Hz.
    fn flow_turn_hz(&self) -> f32 {
        self.flow_speed() * self.tuning.helix_cycles / self.width.x.max(1.0)
    }

    fn pulse_speed(&self) -> f32 {
        (self.tuning.processing_width / 2.0 + 12.0) / (1.4 * self.work_period())
    }

    fn working(&self, time: f32) -> Working {
        match self.tuning.processing {
            Processing::Pulse => Working::Pulse {
                time,
                period: self.work_period(),
                speed: self.pulse_speed(),
            },
            Processing::Sweep | Processing::Flow => Working::Flow {
                offset: self.flow_offset,
                spacing: self.flow_spacing(),
            },
        }
    }

    /// Where done's front starts and how fast it is already moving: the
    /// beam where it is (Sweep), the rightmost band (Flow), or the middle
    /// at the pulses' speed (Pulse).
    fn arm_done(&mut self) {
        let width = self.width.x;
        let processing = self.stage == Stage::Processing;
        match self.tuning.processing {
            Processing::Sweep => {
                let elapsed = if processing {
                    self.elapsed
                } else {
                    self.tuning.shimmer_pass / 2.0
                };
                self.front_at_done = self.process_front(elapsed, width);
                self.speed_at_done = 0.0;
            }
            Processing::Flow => {
                if !processing {
                    self.flow_offset = -width / 2.0 - 4.0;
                }
                let spacing = self.flow_spacing();
                let last = width / 2.0 - 4.0;
                let bands = ((last - self.flow_offset) / spacing).floor();
                self.front_at_done = self.flow_offset + bands.max(0.0) * spacing;
                self.speed_at_done = self.flow_speed();
            }
            Processing::Pulse => {
                self.front_at_done = 0.0;
                self.speed_at_done = 1.5 * self.pulse_speed();
            }
        }
        self.done_from = if processing { self.elapsed } else { 0.0 };
    }

    fn process_front(&self, elapsed: f32, width: f32) -> f32 {
        let pass = self.tuning.shimmer_pass.max(0.05);
        let position = 0.5 - 0.5 * (PI * (elapsed / pass + 0.5)).cos();
        -width / 2.0 + PASS_INSET + (width - 2.0 * PASS_INSET) * position
    }

    fn scan(&self, width: f32) -> Scan {
        let e = self.elapsed;
        let still = |helix: f32| Scan {
            helix,
            decay: 1.0,
            tail: 1.0,
            grid: 1.0,
            envelope: Envelope::Enter {
                reach: 1e4,
                head: 0.0,
                unfold: 1.0,
            },
        };
        match self.stage {
            Stage::Failed => still(1.0),
            _ if self.reduce_motion => still(1.0),
            Stage::Recording => Scan {
                helix: 1.0,
                decay: 1.0,
                tail: 1.0,
                grid: (e / 0.3).min(1.0),
                envelope: Envelope::Enter {
                    reach: (width / 2.0 + 12.0) * pass_position(e / ENTER),
                    head: 2.8 * (1.0 - smoothstep(0.08, ENTER, e)),
                    unfold: smoothstep(0.0, 0.1, e),
                },
            },
            Stage::Processing if self.tuning.processing != Processing::Sweep => {
                let enter = smoothstep(0.0, 0.35, e);
                Scan {
                    helix: 1.0 + (WORK_HELIX - 1.0) * enter,
                    decay: 1.0,
                    tail: 1.0,
                    grid: 1.0,
                    envelope: Envelope::Work {
                        working: self.working(e),
                        head: 2.5 * smoothstep(0.05, 0.3, e) * self.tuning.shimmer,
                        swell: 0.9 * enter,
                        half_width: width / 2.0,
                    },
                }
            }
            Stage::Done if self.tuning.processing != Processing::Sweep => {
                let target = width / 2.0 + 8.0;
                let front = hermite(self.front_at_done, self.speed_at_done, target, e / WIPE);
                let from_middle = self.tuning.processing == Processing::Pulse;
                Scan {
                    helix: WORK_HELIX,
                    decay: 1.0 - 0.6 * smoothstep(0.0, 0.12, e),
                    tail: 1.0 - smoothstep(0.0, 0.15, e),
                    grid: 1.0 - smoothstep(0.2, 0.4, e),
                    envelope: Envelope::Clear {
                        front,
                        head: 2.8 * smoothstep(0.0, 0.04, e) * (1.0 - smoothstep(0.22, 0.32, e)),
                        from_middle,
                        working: self.working(self.done_from + e),
                        glow: 2.5 * self.tuning.shimmer * (1.0 - smoothstep(0.0, 0.18, e)),
                        swell: 0.9,
                        half_width: width / 2.0,
                    },
                }
            }
            Stage::Processing => {
                let pass = self.tuning.shimmer_pass.max(0.05);
                let iron = 1.0 - smoothstep(0.0, 0.28, e);
                let head = (2.2 + (PI * (e / pass + 0.5)).sin().abs())
                    * smoothstep(0.0, 0.14, e)
                    * self.tuning.shimmer;
                Scan {
                    helix: iron,
                    decay: 1.0,
                    tail: 1.0,
                    grid: 1.0,
                    envelope: Envelope::Process {
                        iron,
                        front: self.process_front(e, width),
                        head,
                    },
                }
            }
            Stage::Done => {
                let target = width / 2.0 - 4.0;
                let start = self.front_at_done;
                Scan {
                    helix: 0.0,
                    decay: 1.0 - 0.75 * smoothstep(0.0, 0.08, e),
                    tail: 1.0 - smoothstep(0.0, 0.1, e),
                    grid: 1.0 - smoothstep(0.2, 0.4, e),
                    envelope: Envelope::Wipe {
                        front: start + (target - start) * pass_position(e / WIPE),
                        head: 2.8 * smoothstep(0.0, 0.035, e) * (1.0 - smoothstep(0.22, 0.32, e)),
                    },
                }
            }
        }
    }

    /// Draws both traces into the dot field and decays the phosphor.
    fn beam(&mut self, hb: f32) {
        let (columns, rows) = (self.columns, self.rows);
        let sub = SAMPLES_PER_COLUMN;
        let samples = (columns - 1) * sub + 1;
        let pitch = self.tuning.spacing;
        let width = self.width.x.max(self.tuning.height);
        let scan = self.scan(width);
        self.grid = scan.grid;
        let mid = (rows as f32 - 1.0) / 2.0;
        let half = mid - 0.55;
        let breath = if self.reduce_motion {
            0.5
        } else {
            0.5 + 0.5 * (self.time * TAU / BREATH_PERIOD).sin()
        };
        let rest = (0.55 + 0.35 * breath) * (0.4 + 0.6 * self.live) * scan.helix;
        let half_width = width / 2.0;
        for k in 0..2 {
            let trace = &self.traces[k];
            let sign = if k == 0 { 1.0 } else { -1.0 };
            let ys = &mut self.ys[k];
            let brightness = &mut self.brightness[k];
            for s in 0..samples {
                let x = (s as f32 / sub as f32 - (columns as f32 - 1.0) / 2.0) * pitch;
                if x.abs() > half_width {
                    ys[s] = mid;
                    brightness[s] = 0.0;
                    continue;
                }
                let (amplitude, bright, swell) = scan.at(x);
                let own = x.abs();
                let xn = (x + half_width) / width;
                let ends = smoothstep(-half_width + 3.0, -half_width + 17.0, x).min(smoothstep(
                    -half_width + 3.0,
                    -half_width + 17.0,
                    -x,
                ));
                let reach = 1.0 - 0.8 * smoothstep(14.0, half_width, own);
                let delay = own / self.tuning.history_speed;
                let a = trace.past(delay, 0);
                let mut spread = 0.0;
                for (c, (spatial, phase)) in SPATIAL.iter().zip(trace.phases).enumerate() {
                    spread += trace.past(delay, 1 + c) * (TAU * spatial * xn - phase).sin();
                }
                let helix = (rest + swell + half * 0.55 * a * reach)
                    * (TAU * self.tuning.helix_cycles * xn - self.carrier).sin();
                let y =
                    half * ((helix + half * 0.7 * spread * reach) / half).tanh() * ends * amplitude;
                ys[s] = mid - sign * y;
                let spatial = (0.3 + 0.7 * (a * 1.8).min(1.0)) * (0.45 + 0.55 * ends);
                brightness[s] = spatial * amplitude + 0.36 * (bright - amplitude).max(0.0);
            }
            for s in 0..samples {
                let slope =
                    (ys[(s + 1).min(samples - 1)] - ys[s.saturating_sub(1)]) * sub as f32 / 2.0;
                brightness[s] *= 1.0 - 0.55 + 0.55 / (1.0 + 0.22 * slope * slope).sqrt();
            }
        }

        let head_keep = (-hb / (PHOSPHOR_HEAD * scan.decay)).exp();
        let tail_keep = (-hb / (self.tuning.phosphor * scan.decay)).exp();
        let dx = 1.0 / sub as f32;
        for i in 0..columns {
            let s0 = i.saturating_sub(3) * sub;
            let s1 = ((i + 3) * sub).min(samples - 1);
            for j in 0..rows {
                let n = j * columns + i;
                let (mut now, mut total) = (0.0, 0.0);
                let mut lit = [0.0; 2];
                for (k, lit) in lit.iter_mut().enumerate() {
                    let ys = &self.ys[k];
                    let mut best = f32::MAX;
                    let mut nearest = i * sub;
                    for s in s0..s1 {
                        let ay = ys[s];
                        let dy = ys[s + 1] - ay;
                        let px = i as f32 - s as f32 * dx;
                        let py = j as f32 - ay;
                        let t = ((px * dx + py * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                        let ex = px - t * dx;
                        let ey = py - t * dy;
                        let d2 = ex * ex + ey * ey;
                        if d2 < best {
                            best = d2;
                            nearest = s;
                        }
                    }
                    let brightness = self.brightness[k][nearest];
                    let on_beam = (-best / 0.25).exp();
                    let instant =
                        (on_beam + 0.5 * (-best.sqrt() / 0.8).exp()).min(1.2) * brightness;
                    let core = on_beam * brightness;
                    let head = &mut self.head[k][n];
                    *head = (*head * head_keep).max(core);
                    let tail = &mut self.tail[k][n];
                    *tail = (*tail * tail_keep).max(core);
                    let light = instant.max(*head).max(0.55 * scan.tail * *tail);
                    *lit = light;
                    now += instant;
                    total += light;
                }
                let power = 1.0 - (1.0 - lit[0].min(1.0)) * (1.0 - lit[1].min(1.0));
                self.dots[n] = [power.clamp(0.0, 1.0), (now / total.max(1e-4)).min(1.0)];
            }
        }
    }
}

/// Dot scope's colours for one frame, sRGB.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub base: [f32; 3],
    pub core: [f32; 3],
    pub plate: [f32; 4],
    pub rim: [f32; 4],
    pub shadow: [f32; 4],
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

fn saturate(color: [f32; 3], amount: f32) -> [f32; 3] {
    let luma = 0.2126 * color[0] + 0.7152 * color[1] + 0.0722 * color[2];
    color.map(|c| (luma + (c - luma) * amount).clamp(0.0, 1.0))
}

/// Warm pearl while recording, cooling to agent blue while processing, and
/// coral on failure.
pub fn palette(dark: bool, pose: &Pose, saturation: f32) -> Palette {
    let (you, you_core, agent, agent_core, coral, coral_core) = if dark {
        (
            [0.86, 0.83, 0.77],
            [0.98, 0.96, 0.92],
            [0.70, 0.78, 0.90],
            [0.92, 0.95, 1.0],
            [1.0, 0.38, 0.31],
            [1.0, 0.66, 0.6],
        )
    } else {
        (
            [0.62, 0.53, 0.40],
            [0.47, 0.38, 0.26],
            [0.40, 0.47, 0.58],
            [0.24, 0.31, 0.43],
            [0.9, 0.27, 0.21],
            [0.7, 0.18, 0.13],
        )
    };
    let base = mix3(mix3(you, agent, pose.cool), coral, pose.fail);
    let core = mix3(mix3(you_core, agent_core, pose.cool), coral_core, pose.fail);
    let (plate, rim, shadow) = if dark {
        (
            [17.0 / 255.0, 17.0 / 255.0, 20.0 / 255.0, 0.94],
            [1.0, 1.0, 1.0, 0.11],
            [0.0, 0.0, 0.0, 0.5],
        )
    } else {
        (
            [252.0 / 255.0, 251.0 / 255.0, 249.0 / 255.0, 0.96],
            [0.0, 0.0, 0.0, 0.09],
            [20.0 / 255.0, 24.0 / 255.0, 40.0 / 255.0, 0.2],
        )
    };
    Palette {
        base: saturate(base, saturation),
        core: saturate(core, saturation),
        plate,
        rim,
        shadow,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(scope: &mut Scope, seconds: f32, frame: Frame) {
        let frames = (seconds * 60.0).round() as usize;
        for _ in 0..frames {
            scope.advance(1.0 / 60.0, frame);
        }
    }

    /// A frame at `level` through the prototype's fixed window.
    fn recording(level: f32) -> Frame {
        Frame {
            stage: Stage::Recording,
            visible: true,
            loudness: -40.0 + 30.0 * level,
            reduce_motion: false,
        }
    }

    /// The prototype's response: the fixed window and round 1's sweep.
    fn fixed() -> Tuning {
        Tuning {
            auto_level: false,
            processing: Processing::Sweep,
            ..Tuning::default()
        }
    }

    /// Mean distance of lit dots from the middle row, in rows.
    fn spread(scope: &Scope) -> f32 {
        let mid = (scope.rows() as f32 - 1.0) / 2.0;
        let (mut sum, mut weight) = (0.0, 0.0);
        for (n, [power, _]) in scope.dots().iter().enumerate() {
            let row = (n / scope.columns()) as f32;
            sum += power * (row - mid).abs();
            weight += power;
        }
        sum / weight.max(1e-6)
    }

    fn lit(scope: &Scope) -> f32 {
        scope.dots().iter().map(|[power, _]| power).sum()
    }

    #[test]
    fn silence_draws_a_resting_braid_and_the_voice_widens_it() {
        let mut quiet = Scope::new(fixed());
        run(&mut quiet, 1.5, recording(0.0));
        assert!(lit(&quiet) > 20.0, "the resting braid is visible");
        let mut loud = Scope::new(fixed());
        run(&mut loud, 1.5, recording(0.8));
        assert!(
            spread(&loud) > spread(&quiet) * 1.3,
            "voice {} vs silence {}",
            spread(&loud),
            spread(&quiet)
        );
        let pose = loud.pose();
        assert!((pose.width - 112.0).abs() < 1.0);
        assert!(pose.opacity > 0.99 && pose.glow > 0.2);
    }

    #[test]
    fn processing_irons_the_braid_flat_and_cools_it() {
        let mut scope = Scope::new(fixed());
        run(&mut scope, 1.0, recording(0.8));
        let voice = spread(&scope);
        run(
            &mut scope,
            1.0,
            Frame {
                stage: Stage::Processing,
                ..recording(0.0)
            },
        );
        assert!(spread(&scope) < voice * 0.6);
        let pose = scope.pose();
        assert!(pose.cool > 0.99);
        assert!((pose.width - 76.0).abs() < 1.0);
    }

    #[test]
    fn done_wipes_the_field_dark_and_hiding_settles() {
        let mut scope = Scope::new(fixed());
        run(&mut scope, 1.0, recording(0.5));
        let processing = Frame {
            stage: Stage::Processing,
            ..recording(0.0)
        };
        run(&mut scope, 0.6, processing);
        let done = Frame {
            stage: Stage::Done,
            ..processing
        };
        run(&mut scope, 0.5, done);
        assert!(lit(&scope) < 1.0, "lit {}", lit(&scope));
        assert!(!scope.settled());
        run(
            &mut scope,
            0.6,
            Frame {
                visible: false,
                ..done
            },
        );
        assert!(scope.settled());
    }

    fn processing() -> Frame {
        Frame {
            stage: Stage::Processing,
            ..recording(0.0)
        }
    }

    fn with(processing: Processing) -> Tuning {
        Tuning {
            processing,
            ..fixed()
        }
    }

    /// The working light along the plate, one sample a point.
    fn light(scope: &Scope) -> Vec<f32> {
        let scan = scope.scan(scope.width.x);
        let half = scope.width.x / 2.0;
        (0..=half as i32 * 2)
            .map(|i| {
                let (amplitude, bright, _) = scan.at(i as f32 - half);
                bright - amplitude
            })
            .collect()
    }

    /// How many points the light moved between `a` and `b` in `range`:
    /// the shift that best lines them up.
    fn shift(a: &[f32], b: &[f32], range: std::ops::Range<usize>) -> i32 {
        (-8..=8)
            .max_by(|&x, &y| {
                let score = |k: i32| -> f32 {
                    range
                        .clone()
                        .filter_map(|i| Some(a[i] * b.get(i.checked_add_signed(k as isize)?)?))
                        .sum()
                };
                score(x).total_cmp(&score(y))
            })
            .unwrap()
    }

    #[test]
    fn flow_turns_the_braid_with_light_flowing_rightward() {
        let mut scope = Scope::new(with(Processing::Flow));
        run(&mut scope, 1.0, recording(0.6));
        run(&mut scope, 1.2, processing());
        let mut sweep = Scope::new(fixed());
        run(&mut sweep, 1.0, recording(0.6));
        run(&mut sweep, 1.2, processing());
        assert!(
            spread(&scope) > spread(&sweep) * 1.4,
            "Flow keeps a braid: {} vs {}",
            spread(&scope),
            spread(&sweep)
        );
        let mut rightward = 0;
        for _ in 0..12 {
            let before = light(&scope);
            run(&mut scope, 0.1, processing());
            rightward += shift(&before, &light(&scope), 0..before.len()).signum();
        }
        assert!(rightward >= 10, "rightward {rightward} of 12");
        let helix = scope.flow_turn_hz() * scope.width.x / scope.tuning.helix_cycles;
        assert!((helix - scope.flow_speed()).abs() < 0.5, "the braid keeps pace");
        assert!(scope.pose().cool > 0.99);
    }

    #[test]
    fn pulse_carries_light_out_from_the_middle() {
        let mut scope = Scope::new(with(Processing::Pulse));
        run(&mut scope, 1.0, recording(0.6));
        run(&mut scope, 1.2, processing());
        let (mut left, mut right) = (0, 0);
        for _ in 0..12 {
            let before = light(&scope);
            let middle = before.len() / 2;
            run(&mut scope, 0.1, processing());
            let after = light(&scope);
            left += shift(&before, &after, 2..middle - 2).signum();
            right += shift(&before, &after, middle + 2..before.len() - 2).signum();
        }
        assert!(left <= -10 && right >= 10, "left {left} right {right}");
    }

    #[test]
    fn every_processing_look_settles_dark_on_done() {
        for look in Processing::ALL {
            for processed in [0.05, 1.37] {
                let mut scope = Scope::new(with(look));
                run(&mut scope, 1.0, recording(0.5));
                run(&mut scope, processed, processing());
                let done = Frame {
                    stage: Stage::Done,
                    ..processing()
                };
                run(&mut scope, 0.5, done);
                assert!(lit(&scope) < 1.0, "{look:?} after {processed}s: lit {}", lit(&scope));
                run(
                    &mut scope,
                    0.6,
                    Frame {
                        visible: false,
                        ..done
                    },
                );
                assert!(scope.settled(), "{look:?}");
            }
        }
    }

    #[test]
    fn done_picks_up_the_band_where_flow_left_it() {
        let mut scope = Scope::new(with(Processing::Flow));
        run(&mut scope, 0.5, recording(0.5));
        run(&mut scope, 1.0, processing());
        let speed = scope.flow_speed();
        scope.advance(
            1.0 / 240.0,
            Frame {
                stage: Stage::Done,
                ..processing()
            },
        );
        let start = scope.front_at_done;
        assert!(start <= scope.width.x / 2.0 - 4.0 && start > -scope.width.x / 2.0 - 5.0);
        let step = 0.004;
        let early = (hermite(start, speed, 50.0, step / WIPE) - start) / step;
        assert!((early - speed).abs() < 0.15 * speed + 3.0, "{early} vs {speed}");
        assert_eq!(hermite(start, speed, 50.0, 1.0), 50.0);
    }

    #[test]
    fn auto_level_lights_a_quiet_voice_the_fixed_window_misses() {
        let quiet = |time: f32| Frame {
            loudness: demo_loudness(time),
            ..recording(0.0)
        };
        let mut auto = Scope::new(Tuning::default());
        let mut fixed_window = Scope::new(fixed());
        let (mut a, mut f) = (0.0, 0.0);
        for i in 0..240 {
            let frame = quiet(i as f32 / 60.0);
            auto.advance(1.0 / 60.0, frame);
            fixed_window.advance(1.0 / 60.0, frame);
            if i >= 120 {
                a += spread(&auto);
                f += spread(&fixed_window);
            }
        }
        assert!(a > f * 1.25, "auto {a} vs fixed {f}");
        assert!(auto.level() >= 0.0 && auto.level() <= 1.0);
    }

    #[test]
    fn frame_rate_does_not_change_the_picture() {
        let mut at_60 = Scope::new(fixed());
        let mut at_120 = Scope::new(fixed());
        for _ in 0..90 {
            at_60.advance(1.0 / 60.0, recording(0.6));
            at_120.advance(1.0 / 120.0, recording(0.6));
            at_120.advance(1.0 / 120.0, recording(0.6));
        }
        assert_eq!(at_60.dots(), at_120.dots());
        assert_eq!(at_60.pose(), at_120.pose());
    }

    #[test]
    fn reduce_motion_holds_a_still_braid() {
        let mut scope = Scope::new(fixed());
        let still = Frame {
            reduce_motion: true,
            ..recording(0.0)
        };
        scope.restart(true);
        run(&mut scope, 3.0, still);
        let before = scope.dots().to_vec();
        run(&mut scope, 0.5, still);
        let drift = before
            .iter()
            .zip(scope.dots())
            .map(|(a, b)| (a[0] - b[0]).abs())
            .fold(0.0, f32::max);
        assert!(drift < 1e-3, "drift {drift}");
        assert_eq!(scope.pose().scale, 1.0);
        assert!(lit(&scope) > 20.0);
    }

    #[test]
    fn retuning_the_grid_resizes_the_field() {
        let mut scope = Scope::new(fixed());
        run(&mut scope, 0.2, recording(0.3));
        scope.tune(Tuning {
            columns: 64.0,
            rows: 15.0,
            ..Tuning::default()
        });
        run(&mut scope, 0.2, recording(0.3));
        assert_eq!(scope.dots().len(), crate::dot_scope::MAX_DOTS);
    }

    #[test]
    fn palette_cools_and_saturates() {
        let pose = Scope::new(fixed()).pose();
        let warm = palette(true, &pose, 1.0);
        assert_eq!(warm.base, [0.86, 0.83, 0.77]);
        let grey = palette(true, &pose, 0.0);
        assert!((grey.base[0] - grey.base[2]).abs() < 1e-6);
        let cool = palette(false, &Pose { cool: 1.0, ..pose }, 1.0);
        assert_eq!(cool.base, [0.40, 0.47, 0.58]);
    }
}

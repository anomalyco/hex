use serde::{Deserialize, Serialize};
use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VoiceFilterMode {
    Normal,
    #[default]
    Muffled,
    Quiet,
    Muted,
    Chipmunk,
    Robot,
    #[serde(alias = "walkie-talkie", alias = "walkie_talkie")]
    Walkie,
    Underwater,
    #[serde(alias = "far-away", alias = "far_away")]
    Far,
}

impl VoiceFilterMode {
    pub const ALL: [Self; 9] = [
        Self::Muffled,
        Self::Muted,
        Self::Quiet,
        Self::Normal,
        Self::Chipmunk,
        Self::Robot,
        Self::Walkie,
        Self::Underwater,
        Self::Far,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Muffled => "muffled",
            Self::Quiet => "quiet",
            Self::Muted => "muted",
            Self::Chipmunk => "chipmunk",
            Self::Robot => "robot",
            Self::Walkie => "walkie",
            Self::Underwater => "underwater",
            Self::Far => "far",
        }
    }

    pub fn from_str_loose(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "normal" | "passthrough" | "off" => Some(Self::Normal),
            "muffled" => Some(Self::Muffled),
            "quiet" | "quiet murmur" | "quiet_murmur" | "murmur" => Some(Self::Quiet),
            "muted" | "mute" => Some(Self::Muted),
            "chipmunk" => Some(Self::Chipmunk),
            "robot" => Some(Self::Robot),
            "walkie" | "walkie-talkie" | "walkie_talkie" => Some(Self::Walkie),
            "underwater" => Some(Self::Underwater),
            "far" | "far away" | "far-away" | "far_away" => Some(Self::Far),
            _ => None,
        }
    }

    pub fn preset(self) -> ModeSpec {
        match self {
            Self::Normal => ModeSpec {
                id: "normal".to_string(),
                name: "Normal".to_string(),
                fade_in_ms: 0.0,
                fade_out_ms: 0.0,
                squelch: None,
                stages: Vec::new(),
            },
            Self::Muffled => ModeSpec {
                id: "muffled".to_string(),
                name: "Muffled".to_string(),
                fade_in_ms: 180.0,
                fade_out_ms: 320.0,
                squelch: None,
                stages: vec![
                    StageSpec::Lowpass {
                        hz: 480.0,
                        q: default_q(),
                        poles: 4,
                    },
                    StageSpec::Highpass {
                        hz: 90.0,
                        q: default_q(),
                        poles: 2,
                    },
                    StageSpec::Reverb {
                        mix: 0.3,
                        room: 0.55,
                        damping: 0.6,
                        predelay_ms: 12.0,
                    },
                    StageSpec::Gain { db: -9.0 },
                ],
            },
            Self::Quiet => ModeSpec {
                id: "quiet".to_string(),
                name: "Quiet".to_string(),
                fade_in_ms: 150.0,
                fade_out_ms: 300.0,
                squelch: None,
                stages: vec![
                    StageSpec::Lowpass {
                        hz: 2200.0,
                        q: default_q(),
                        poles: 2,
                    },
                    StageSpec::Gain { db: -24.0 },
                ],
            },
            Self::Muted => ModeSpec {
                id: "muted".to_string(),
                name: "Muted".to_string(),
                fade_in_ms: 60.0,
                fade_out_ms: 200.0,
                squelch: None,
                stages: vec![
                    StageSpec::Mute,
                    StageSpec::Noise {
                        db: -68.0,
                        color: NoiseColor::Pink,
                    },
                ],
            },
            Self::Chipmunk => ModeSpec {
                id: "chipmunk".to_string(),
                name: "Chipmunk".to_string(),
                fade_in_ms: 80.0,
                fade_out_ms: 150.0,
                squelch: None,
                stages: vec![
                    StageSpec::Pitch {
                        semitones: 7.0,
                        window_ms: 40.0,
                    },
                    StageSpec::Highpass {
                        hz: 150.0,
                        q: default_q(),
                        poles: 2,
                    },
                    StageSpec::Gain { db: -4.0 },
                ],
            },
            Self::Robot => ModeSpec {
                id: "robot".to_string(),
                name: "Robot".to_string(),
                fade_in_ms: 80.0,
                fade_out_ms: 150.0,
                squelch: None,
                stages: vec![
                    StageSpec::Ringmod { hz: 70.0, mix: 1.0 },
                    StageSpec::Comb {
                        ms: 6.0,
                        feedback: 0.55,
                        mix: 0.6,
                    },
                    StageSpec::Drive { amount: 1.5 },
                ],
            },
            Self::Walkie => ModeSpec {
                id: "walkie".to_string(),
                name: "Walkie-talkie".to_string(),
                fade_in_ms: 20.0,
                fade_out_ms: 40.0,
                squelch: Some(SquelchSpec {
                    ms: 140.0,
                    db: -20.0,
                }),
                stages: vec![
                    StageSpec::Highpass {
                        hz: 500.0,
                        q: default_q(),
                        poles: 4,
                    },
                    StageSpec::Lowpass {
                        hz: 2800.0,
                        q: default_q(),
                        poles: 4,
                    },
                    StageSpec::Noise {
                        db: -46.0,
                        color: NoiseColor::White,
                    },
                    StageSpec::Drive { amount: 4.0 },
                    StageSpec::Gain { db: -6.0 },
                ],
            },
            Self::Underwater => ModeSpec {
                id: "underwater".to_string(),
                name: "Underwater".to_string(),
                fade_in_ms: 250.0,
                fade_out_ms: 350.0,
                squelch: None,
                stages: vec![
                    StageSpec::Wobble {
                        hz: 380.0,
                        octaves: 0.6,
                        rate_hz: 0.7,
                        q: 2.0,
                    },
                    StageSpec::Chorus {
                        delay_ms: 12.0,
                        depth_ms: 4.0,
                        rate_hz: 1.3,
                        mix: 0.6,
                    },
                    StageSpec::Reverb {
                        mix: 0.45,
                        room: 0.8,
                        damping: 0.7,
                        predelay_ms: 20.0,
                    },
                    StageSpec::Gain { db: -4.0 },
                ],
            },
            Self::Far => ModeSpec {
                id: "far".to_string(),
                name: "Far away".to_string(),
                fade_in_ms: 300.0,
                fade_out_ms: 400.0,
                squelch: None,
                stages: vec![
                    StageSpec::Highpass {
                        hz: 250.0,
                        q: default_q(),
                        poles: 2,
                    },
                    StageSpec::Lowpass {
                        hz: 3200.0,
                        q: default_q(),
                        poles: 2,
                    },
                    StageSpec::Reverb {
                        mix: 0.75,
                        room: 0.85,
                        damping: 0.4,
                        predelay_ms: 45.0,
                    },
                    StageSpec::Gain { db: -12.0 },
                ],
            },
        }
    }

    pub fn spec_with_options(self, comfort_noise: bool) -> ModeSpec {
        let mut spec = self.preset();
        if self == Self::Muted && !comfort_noise {
            for stage in &mut spec.stages {
                if let StageSpec::Noise { db, .. } = stage {
                    *db = -120.0;
                }
            }
        }
        spec
    }
}

#[allow(clippy::approx_constant)]
const fn default_q() -> f64 {
    0.7071
}

const fn default_poles() -> u8 {
    2
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NoiseColor {
    White,
    #[default]
    Pink,
    Brown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SquelchSpec {
    pub ms: f64,
    pub db: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StageSpec {
    Gain {
        db: f64,
    },
    Mute,
    Lowpass {
        hz: f64,
        #[serde(default = "default_q_sqrt")]
        q: f64,
        #[serde(default = "default_poles")]
        poles: u8,
    },
    Highpass {
        hz: f64,
        #[serde(default = "default_q_sqrt")]
        q: f64,
        #[serde(default = "default_poles")]
        poles: u8,
    },
    Noise {
        db: f64,
        #[serde(default)]
        color: NoiseColor,
    },
    Reverb {
        mix: f64,
        room: f64,
        damping: f64,
        #[serde(default, rename = "predelayMs", alias = "predelay_ms")]
        predelay_ms: f64,
    },
    Drive {
        amount: f64,
    },
    Ringmod {
        hz: f64,
        mix: f64,
    },
    Comb {
        ms: f64,
        feedback: f64,
        mix: f64,
    },
    Chorus {
        #[serde(rename = "delayMs", alias = "delay_ms")]
        delay_ms: f64,
        #[serde(rename = "depthMs", alias = "depth_ms")]
        depth_ms: f64,
        #[serde(rename = "rateHz", alias = "rate_hz")]
        rate_hz: f64,
        mix: f64,
    },
    Wobble {
        hz: f64,
        octaves: f64,
        #[serde(rename = "rateHz", alias = "rate_hz")]
        rate_hz: f64,
        q: f64,
    },
    Pitch {
        semitones: f64,
        #[serde(rename = "windowMs", alias = "window_ms")]
        window_ms: f64,
    },
}

const fn default_q_sqrt() -> f64 {
    FRAC_1_SQRT_2
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModeSpec {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "fadeInMs", alias = "fade_in_ms")]
    pub fade_in_ms: f64,
    #[serde(rename = "fadeOutMs", alias = "fade_out_ms")]
    pub fade_out_ms: f64,
    #[serde(default)]
    pub squelch: Option<SquelchSpec>,
    #[serde(default)]
    pub stages: Vec<StageSpec>,
}

impl ModeSpec {
    pub fn apply_params_json(&mut self, json_str: &str) -> Result<Option<f64>, String> {
        let value: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|err| format!("Invalid --params JSON: {err}"))?;
        let obj = value
            .as_object()
            .ok_or_else(|| "--params must be a JSON object".to_string())?;

        if let Some(fade_in) = obj
            .get("fadeInMs")
            .or_else(|| obj.get("fade_in_ms"))
            .and_then(|v| v.as_f64())
        {
            self.fade_in_ms = fade_in;
        }
        if let Some(fade_out) = obj
            .get("fadeOutMs")
            .or_else(|| obj.get("fade_out_ms"))
            .and_then(|v| v.as_f64())
        {
            self.fade_out_ms = fade_out;
        }
        if let Some(squelch_val) = obj.get("squelch") {
            if squelch_val.is_null() {
                self.squelch = None;
            } else {
                let sq: SquelchSpec = serde_json::from_value(squelch_val.clone())
                    .map_err(|err| format!("Invalid squelch in --params: {err}"))?;
                self.squelch = Some(sq);
            }
        }
        if let Some(stages_val) = obj.get("stages") {
            let stages: Vec<StageSpec> = serde_json::from_value(stages_val.clone())
                .map_err(|err| format!("Invalid stages in --params: {err}"))?;
            self.stages = stages;
        }
        if let Some(comfort) = obj
            .get("comfortNoise")
            .or_else(|| obj.get("comfort_noise"))
            .and_then(|v| v.as_bool())
        {
            for stage in &mut self.stages {
                if let StageSpec::Noise { db, .. } = stage {
                    *db = if comfort { -68.0 } else { -120.0 };
                }
            }
        }
        if let Some(room_tone_db) = obj
            .get("roomToneDb")
            .or_else(|| obj.get("room_tone_db"))
            .and_then(|v| v.as_f64())
        {
            for stage in &mut self.stages {
                if let StageSpec::Noise { db, .. } = stage {
                    *db = room_tone_db;
                }
            }
        }

        let strength = obj
            .get("strength")
            .and_then(|v| v.as_f64())
            .map(|s| s.clamp(0.0, 1.0));
        Ok(strength)
    }
}

#[inline]
fn db_to_gain(db: f64) -> f64 {
    10.0_f64.powf(db / 20.0)
}

// ── Noise ───────────────────────────────────────────────────────────────────

const NOISE_NORM_WHITE: f64 = 1.7320508;
const NOISE_NORM_PINK: f64 = 2.3194;
const NOISE_NORM_BROWN: f64 = 4.9715;

#[derive(Debug, Clone)]
struct Rng {
    s: u32,
}

impl Rng {
    fn new(seed: u32) -> Self {
        Self {
            s: if seed == 0 { 1 } else { seed },
        }
    }

    #[inline]
    fn next_unit(&mut self) -> f64 {
        let mut s = self.s;
        s ^= s.wrapping_shl(13);
        s ^= s >> 17;
        s ^= s.wrapping_shl(5);
        self.s = s;
        (f64::from(s) / 4_294_967_296.0) * 2.0 - 1.0
    }
}

#[derive(Debug, Clone)]
struct Noise {
    color: NoiseColor,
    rng: Rng,
    b0: f64,
    b1: f64,
    b2: f64,
}

impl Noise {
    fn new(color: NoiseColor, seed: u32) -> Self {
        Self {
            color,
            rng: Rng::new(seed),
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
        }
    }

    #[inline]
    fn next_sample(&mut self) -> f64 {
        let w = self.rng.next_unit();
        match self.color {
            NoiseColor::Pink => {
                self.b0 = 0.99765 * self.b0 + w * 0.099046;
                self.b1 = 0.963 * self.b1 + w * 0.2965164;
                self.b2 = 0.57 * self.b2 + w * 1.0526913;
                (self.b0 + self.b1 + self.b2 + w * 0.1848) * 0.25 * NOISE_NORM_PINK
            }
            NoiseColor::Brown => {
                self.b0 = (self.b0 + 0.02 * w) / 1.02;
                self.b0 * 3.5 * NOISE_NORM_BROWN
            }
            NoiseColor::White => w * NOISE_NORM_WHITE,
        }
    }
}

// ── Biquad (RBJ cookbook, transposed direct form II) ────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BiquadKind {
    Lowpass,
    Highpass,
}

#[derive(Debug, Clone)]
struct Biquad {
    kind: BiquadKind,
    sr: f64,
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn new(kind: BiquadKind, hz: f64, q: f64, sr: f64) -> Self {
        let mut bq = Self {
            kind,
            sr,
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        };
        bq.set(hz, q);
        bq
    }

    #[inline]
    fn set(&mut self, hz: f64, q: f64) {
        let f = hz.clamp(10.0, self.sr * 0.45);
        let w0 = (TAU * f) / self.sr;
        let cos = w0.cos();
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        let (b0, b1, b2) = match self.kind {
            BiquadKind::Lowpass => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0),
            BiquadKind::Highpass => ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0),
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = (-2.0 * cos) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

// ── Delay line with linear interpolation ────────────────────────────────────

#[derive(Debug, Clone)]
struct Delay {
    buf: Vec<f32>,
    w: usize,
}

impl Delay {
    fn new(max_samples: f64) -> Self {
        let len = (max_samples.ceil() as usize + 2).max(2);
        Self {
            buf: vec![0.0; len],
            w: 0,
        }
    }

    #[inline]
    fn push(&mut self, x: f64) {
        self.buf[self.w] = x as f32;
        self.w = (self.w + 1) % self.buf.len();
    }

    #[inline]
    fn read(&self, d: f64) -> f64 {
        let n = self.buf.len() as isize;
        let pos = (self.w as f64) - 1.0 - d;
        let i_floor = pos.floor();
        let frac = pos - i_floor;
        let i = i_floor as isize;
        let idx_a = ((i % n) + n) as usize % self.buf.len();
        let idx_b = (((i + 1) % n) + n) as usize % self.buf.len();
        let a = f64::from(self.buf[idx_a]);
        let b = f64::from(self.buf[idx_b]);
        a + (b - a) * frac
    }
}

// ── Freeverb (mono) ─────────────────────────────────────────────────────────

const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];

#[derive(Debug, Clone)]
struct CombLine {
    buf: Vec<f32>,
    i: usize,
    store: f64,
}

#[derive(Debug, Clone)]
struct AllpassLine {
    buf: Vec<f32>,
    i: usize,
}

#[derive(Debug, Clone)]
struct Freeverb {
    combs: Vec<CombLine>,
    aps: Vec<AllpassLine>,
    feedback: f64,
    damp: f64,
    pre: Delay,
    pre_samples: f64,
}

impl Freeverb {
    fn new(sr: f64, room: f64, damping: f64, predelay_ms: f64) -> Self {
        let k = sr / 44100.0;
        let combs = COMBS
            .iter()
            .map(|&n| {
                let len = ((n as f64) * k).round().max(1.0) as usize;
                CombLine {
                    buf: vec![0.0; len],
                    i: 0,
                    store: 0.0,
                }
            })
            .collect();
        let aps = ALLPASSES
            .iter()
            .map(|&n| {
                let len = ((n as f64) * k).round().max(1.0) as usize;
                AllpassLine {
                    buf: vec![0.0; len],
                    i: 0,
                }
            })
            .collect();
        Self {
            combs,
            aps,
            feedback: room * 0.28 + 0.7,
            damp: damping * 0.4,
            pre: Delay::new(sr * 0.25),
            pre_samples: (predelay_ms / 1000.0) * sr,
        }
    }

    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        self.pre.push(x);
        let input = self.pre.read(self.pre_samples) * 0.015;
        let mut out = 0.0;
        for c in &mut self.combs {
            let y = f64::from(c.buf[c.i]);
            c.store = y * (1.0 - self.damp) + c.store * self.damp;
            c.buf[c.i] = (input + c.store * self.feedback) as f32;
            c.i = (c.i + 1) % c.buf.len();
            out += y;
        }
        for a in &mut self.aps {
            let b = f64::from(a.buf[a.i]);
            a.buf[a.i] = (out + b * 0.5) as f32;
            a.i = (a.i + 1) % a.buf.len();
            out = b - out;
        }
        out * 3.0
    }
}

// ── Stages ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum StageRuntime {
    Bypass,
    Gain {
        g: f64,
    },
    Mute,
    Filter1 {
        bq: Biquad,
    },
    Filter2 {
        bq0: Biquad,
        bq1: Biquad,
    },
    Noise {
        noise: Noise,
        g: f64,
    },
    Reverb {
        reverb: Freeverb,
        mix: f64,
    },
    Drive {
        k: f64,
        norm: f64,
    },
    Ringmod {
        ph: f64,
        inc: f64,
        mix: f64,
    },
    Comb {
        d: Delay,
        n: f64,
        feedback: f64,
        mix: f64,
    },
    Chorus {
        d: Delay,
        ph: f64,
        inc: f64,
        base: f64,
        depth: f64,
        mix: f64,
    },
    Wobble {
        bq: Biquad,
        hz: f64,
        octaves: f64,
        q: f64,
        ph: f64,
        inc: f64,
        n: u64,
    },
    Pitch {
        d: Delay,
        w: f64,
        ph: f64,
        inc: f64,
    },
}

impl StageRuntime {
    fn from_spec(spec: &StageSpec, sr: f64, seed: u32) -> Self {
        match *spec {
            StageSpec::Gain { db } => Self::Gain { g: db_to_gain(db) },
            StageSpec::Mute => Self::Mute,
            StageSpec::Lowpass { hz, q, poles } => {
                Self::build_filter(BiquadKind::Lowpass, hz, q, poles, sr)
            }
            StageSpec::Highpass { hz, q, poles } => {
                Self::build_filter(BiquadKind::Highpass, hz, q, poles, sr)
            }
            StageSpec::Noise { db, color } => {
                if db <= -90.0 {
                    Self::Bypass
                } else {
                    Self::Noise {
                        noise: Noise::new(color, seed),
                        g: db_to_gain(db),
                    }
                }
            }
            StageSpec::Reverb {
                mix,
                room,
                damping,
                predelay_ms,
            } => Self::Reverb {
                reverb: Freeverb::new(sr, room, damping, predelay_ms),
                mix,
            },
            StageSpec::Drive { amount } => {
                let k = amount.max(0.01);
                let norm = 1.0 / k.tanh();
                Self::Drive { k, norm }
            }
            StageSpec::Ringmod { hz, mix } => Self::Ringmod {
                ph: 0.0,
                inc: hz / sr,
                mix,
            },
            StageSpec::Comb { ms, feedback, mix } => Self::Comb {
                d: Delay::new(sr * 0.1),
                n: (ms / 1000.0) * sr,
                feedback,
                mix,
            },
            StageSpec::Chorus {
                delay_ms,
                depth_ms,
                rate_hz,
                mix,
            } => Self::Chorus {
                d: Delay::new(sr * 0.1),
                ph: 0.0,
                inc: rate_hz / sr,
                base: (delay_ms / 1000.0) * sr,
                depth: (depth_ms / 1000.0) * sr,
                mix,
            },
            StageSpec::Wobble {
                hz,
                octaves,
                rate_hz,
                q,
            } => Self::Wobble {
                bq: Biquad::new(BiquadKind::Lowpass, hz, q, sr),
                hz,
                octaves,
                q,
                ph: 0.0,
                inc: rate_hz / sr,
                n: 0,
            },
            StageSpec::Pitch {
                semitones,
                window_ms,
            } => {
                let ratio = 2.0_f64.powf(semitones / 12.0);
                let w = ((window_ms / 1000.0) * sr).max(64.0);
                Self::Pitch {
                    d: Delay::new(w + 4.0),
                    w,
                    ph: 0.0,
                    inc: (1.0 - ratio) / w,
                }
            }
        }
    }

    fn build_filter(kind: BiquadKind, hz: f64, q: f64, poles: u8, sr: f64) -> Self {
        if poles == 4 {
            Self::Filter2 {
                bq0: Biquad::new(kind, hz, q, sr),
                bq1: Biquad::new(kind, hz, q, sr),
            }
        } else {
            Self::Filter1 {
                bq: Biquad::new(kind, hz, q, sr),
            }
        }
    }

    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        match self {
            Self::Bypass => x,
            Self::Gain { g } => x * *g,
            Self::Mute => 0.0,
            Self::Filter1 { bq } => bq.process(x),
            Self::Filter2 { bq0, bq1 } => {
                let y0 = bq0.process(x);
                bq1.process(y0)
            }
            Self::Noise { noise, g } => x + noise.next_sample() * *g,
            Self::Reverb { reverb, mix } => x * (1.0 - *mix) + reverb.process(x) * *mix,
            Self::Drive { k, norm } => (x * *k).tanh() * *norm,
            Self::Ringmod { ph, inc, mix } => {
                let y = x * (TAU * *ph).sin();
                *ph += *inc;
                *ph -= ph.floor();
                x * (1.0 - *mix) + y * *mix
            }
            Self::Comb {
                d,
                n,
                feedback,
                mix,
            } => {
                let y = x + *feedback * d.read(*n - 1.0);
                d.push(y);
                x * (1.0 - *mix) + y * *mix * (1.0 - *feedback)
            }
            Self::Chorus {
                d,
                ph,
                inc,
                base,
                depth,
                mix,
            } => {
                d.push(x);
                let y = d.read(*base + *depth * (TAU * *ph).sin());
                *ph += *inc;
                *ph -= ph.floor();
                x * (1.0 - *mix) + y * *mix
            }
            Self::Wobble {
                bq,
                hz,
                octaves,
                q,
                ph,
                inc,
                n,
            } => {
                if *n % 32 == 0 {
                    bq.set(*hz * 2.0_f64.powf(*octaves * (TAU * *ph).sin()), *q);
                }
                *n = n.wrapping_add(1);
                *ph += *inc;
                *ph -= ph.floor();
                bq.process(x)
            }
            Self::Pitch { d, w, ph, inc } => {
                d.push(x);
                let p2 = (*ph + 0.5) % 1.0;
                let w1 = 0.5 - 0.5 * (TAU * *ph).cos();
                let y = d.read(*ph * *w) * w1 + d.read(p2 * *w) * (1.0 - w1);
                *ph += *inc;
                *ph -= ph.floor();
                y
            }
        }
    }
}

// ── Engine ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct HexMicEngine {
    sr: f64,
    g: f64,
    dictating: bool,
    burst: usize,
    burst_noise: Noise,
    stages: Vec<StageRuntime>,
    inc: f64,
    dec: f64,
    squelch: Option<SquelchSpec>,
    strength: f64,
}

impl HexMicEngine {
    pub fn new(sr: f64, mode: &ModeSpec) -> Self {
        let mut engine = Self {
            sr,
            g: 0.0,
            dictating: false,
            burst: 0,
            burst_noise: Noise::new(NoiseColor::White, 0x2545_f491),
            stages: Vec::new(),
            inc: 1.0,
            dec: 1.0,
            squelch: None,
            strength: 1.0,
        };
        engine.set_mode(mode);
        engine
    }

    pub fn set_mode(&mut self, mode: &ModeSpec) {
        self.stages = mode
            .stages
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let seed = 0x9e37_79b9_u32.wrapping_add((i as u32).wrapping_mul(7919));
                StageRuntime::from_spec(s, self.sr, seed)
            })
            .collect();
        self.inc = 1.0 / ((mode.fade_in_ms / 1000.0) * self.sr).max(1.0);
        self.dec = 1.0 / ((mode.fade_out_ms / 1000.0) * self.sr).max(1.0);
        self.squelch = mode.squelch.clone();
    }

    pub fn set_strength(&mut self, strength: f64) {
        self.strength = strength.clamp(0.0, 1.0);
    }

    pub fn set_dictating(&mut self, on: bool) {
        if on == self.dictating {
            return;
        }
        self.dictating = on;
        if let Some(sq) = &self.squelch {
            self.burst = ((sq.ms / 1000.0) * self.sr).round().max(0.0) as usize;
        }
    }

    pub fn is_dictating(&self) -> bool {
        self.dictating
    }

    pub fn process_range(&mut self, input: &[f32], output: &mut [f32], from: usize, to: usize) {
        let end = to.min(input.len()).min(output.len());
        let start = from.min(end);
        let (sq_len, sq_gain) = match &self.squelch {
            Some(sq) => (
                ((sq.ms / 1000.0) * self.sr).round().max(1.0),
                db_to_gain(sq.db),
            ),
            None => (1.0, 0.0),
        };

        for i in start..end {
            let x = f64::from(input[i]);
            let mut wet = x;
            for stage in &mut self.stages {
                wet = stage.process(wet);
            }
            if self.dictating {
                self.g = (self.g + self.inc).min(1.0);
            } else {
                self.g = (self.g - self.dec).max(0.0);
            }
            let w = (0.5 - 0.5 * (PI * self.g).cos()) * self.strength;
            let mut y = x * (1.0 - w) + wet * w;
            if self.burst > 0 {
                y += self.burst_noise.next_sample()
                    * sq_gain
                    * ((self.burst as f64) / sq_len)
                    * self.strength;
                self.burst -= 1;
            }
            output[i] = y.clamp(-1.0, 1.0) as f32;
        }
    }

    pub fn process_in_place(&mut self, samples: &mut [f32]) {
        let (sq_len, sq_gain) = match &self.squelch {
            Some(sq) => (
                ((sq.ms / 1000.0) * self.sr).round().max(1.0),
                db_to_gain(sq.db),
            ),
            None => (1.0, 0.0),
        };

        for sample in samples {
            let x = f64::from(*sample);
            let mut wet = x;
            for stage in &mut self.stages {
                wet = stage.process(wet);
            }
            if self.dictating {
                self.g = (self.g + self.inc).min(1.0);
            } else {
                self.g = (self.g - self.dec).max(0.0);
            }
            let w = (0.5 - 0.5 * (PI * self.g).cos()) * self.strength;
            let mut y = x * (1.0 - w) + wet * w;
            if self.burst > 0 {
                y += self.burst_noise.next_sample()
                    * sq_gain
                    * ((self.burst as f64) / sq_len)
                    * self.strength;
                self.burst -= 1;
            }
            *sample = y.clamp(-1.0, 1.0) as f32;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DictationSpan {
    pub start_ms: f64,
    pub end_ms: f64,
}

impl DictationSpan {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let (start_s, end_s) = raw.trim().split_once('-').ok_or_else(|| {
            format!("Invalid dictation span '{raw}'; expected <start_ms>-<end_ms>")
        })?;
        let start_ms: f64 = start_s
            .trim()
            .parse()
            .map_err(|_| format!("Invalid dictation start_ms in '{raw}'"))?;
        let end_ms: f64 = end_s
            .trim()
            .parse()
            .map_err(|_| format!("Invalid dictation end_ms in '{raw}'"))?;
        if start_ms < 0.0 || end_ms < start_ms {
            return Err(format!(
                "Invalid dictation span '{raw}'; require 0 <= start_ms <= end_ms"
            ));
        }
        Ok(Self { start_ms, end_ms })
    }
}

pub fn render(
    input: &[f32],
    sr: f64,
    mode: &ModeSpec,
    spans: &[DictationSpan],
    strength: f64,
) -> Vec<f32> {
    let mut out = vec![0.0_f32; input.len()];
    let mut eng = HexMicEngine::new(sr, mode);
    eng.set_strength(strength);

    let mut edges: Vec<(usize, bool)> = Vec::with_capacity(spans.len() * 2);
    for s in spans {
        let start_idx = ((s.start_ms / 1000.0) * sr).round().max(0.0) as usize;
        let end_idx = ((s.end_ms / 1000.0) * sr).round().max(0.0) as usize;
        edges.push((start_idx, true));
        edges.push((end_idx, false));
    }
    edges.sort_by_key(|&(at, _)| at);

    let mut pos = 0;
    for (at, on) in edges {
        let end = at.max(pos).min(input.len());
        eng.process_range(input, &mut out, pos, end);
        pos = end;
        eng.set_dictating(on);
    }
    eng.process_range(input, &mut out, pos, input.len());
    out
}

// ── WAV I/O for CLI offline rendering ────────────────────────────────────────

pub fn read_wav_mono_f32(path: &Path) -> Result<(Vec<f32>, u32), String> {
    let bytes =
        fs::read(path).map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(format!("{} is not a valid RIFF WAVE file", path.display()));
    }

    let mut pos = 12usize;
    let mut format_tag = 0u16;
    let mut channels = 0u16;
    let mut sample_rate = 0u32;
    let mut bits_per_sample = 0u16;
    let mut data_slice: Option<&[u8]> = None;

    while pos + 8 <= bytes.len() {
        let chunk_id = &bytes[pos..pos + 4];
        let chunk_size = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let chunk_start = pos + 8;
        let chunk_end = chunk_start.saturating_add(chunk_size).min(bytes.len());

        if chunk_id == b"fmt " && chunk_size >= 16 {
            let fmt = &bytes[chunk_start..chunk_end];
            format_tag = u16::from_le_bytes([fmt[0], fmt[1]]);
            channels = u16::from_le_bytes([fmt[2], fmt[3]]);
            sample_rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
            bits_per_sample = u16::from_le_bytes([fmt[14], fmt[15]]);
            if format_tag == 0xFFFE && fmt.len() >= 40 {
                format_tag = u16::from_le_bytes([fmt[24], fmt[25]]);
            }
        } else if chunk_id == b"data" {
            data_slice = Some(&bytes[chunk_start..chunk_end]);
            break;
        }

        pos = chunk_start + chunk_size + (chunk_size & 1);
    }

    let data = data_slice.ok_or_else(|| format!("{} is missing a data chunk", path.display()))?;
    if channels == 0 || sample_rate == 0 {
        return Err(format!("{} has invalid WAV format header", path.display()));
    }

    let ch = usize::from(channels);
    let samples = match (format_tag, bits_per_sample) {
        (1, 16) => {
            let frame_bytes = 2 * ch;
            let mut out = Vec::with_capacity(data.len() / frame_bytes);
            for frame in data.chunks_exact(frame_bytes) {
                let mut sum = 0.0_f32;
                for c in 0..ch {
                    let s = i16::from_le_bytes([frame[c * 2], frame[c * 2 + 1]]);
                    sum += f32::from(s) / 32768.0;
                }
                out.push(sum / (ch as f32));
            }
            out
        }
        (1, 24) => {
            let frame_bytes = 3 * ch;
            let mut out = Vec::with_capacity(data.len() / frame_bytes);
            for frame in data.chunks_exact(frame_bytes) {
                let mut sum = 0.0_f32;
                for c in 0..ch {
                    let b0 = frame[c * 3] as i32;
                    let b1 = (frame[c * 3 + 1] as i32) << 8;
                    let b2 = ((frame[c * 3 + 2] as i8) as i32) << 16;
                    let s = b0 | b1 | b2;
                    sum += (s as f32) / 8_388_608.0;
                }
                out.push(sum / (ch as f32));
            }
            out
        }
        (1, 32) => {
            let frame_bytes = 4 * ch;
            let mut out = Vec::with_capacity(data.len() / frame_bytes);
            for frame in data.chunks_exact(frame_bytes) {
                let mut sum = 0.0_f32;
                for c in 0..ch {
                    let s = i32::from_le_bytes([
                        frame[c * 4],
                        frame[c * 4 + 1],
                        frame[c * 4 + 2],
                        frame[c * 4 + 3],
                    ]);
                    sum += (s as f32) / 2_147_483_648.0;
                }
                out.push(sum / (ch as f32));
            }
            out
        }
        (3, 32) => {
            let frame_bytes = 4 * ch;
            let mut out = Vec::with_capacity(data.len() / frame_bytes);
            for frame in data.chunks_exact(frame_bytes) {
                let mut sum = 0.0_f32;
                for c in 0..ch {
                    let s = f32::from_le_bytes([
                        frame[c * 4],
                        frame[c * 4 + 1],
                        frame[c * 4 + 2],
                        frame[c * 4 + 3],
                    ]);
                    sum += s;
                }
                out.push(sum / (ch as f32));
            }
            out
        }
        _ => {
            return Err(format!(
                "Unsupported WAV format (tag={format_tag}, bits={bits_per_sample}) in {}",
                path.display()
            ));
        }
    };

    Ok((samples, sample_rate))
}

pub fn write_wav_mono_f32(path: &Path, samples: &[f32], sample_rate: u32) -> Result<(), String> {
    let data_bytes = samples
        .len()
        .checked_mul(4)
        .ok_or_else(|| "WAV output too large".to_string())?;
    let riff_size = 36u32
        .checked_add(u32::try_from(data_bytes).map_err(|_| "WAV output too large".to_string())?)
        .ok_or_else(|| "WAV output too large".to_string())?;

    let mut buf = Vec::with_capacity(44 + data_bytes);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&riff_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    buf.extend_from_slice(&1u16.to_le_bytes()); // mono
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * 4;
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&4u16.to_le_bytes()); // block align
    buf.extend_from_slice(&32u16.to_le_bytes()); // bits per sample
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&(data_bytes as u32).to_le_bytes());
    for &sample in samples {
        buf.extend_from_slice(&sample.to_le_bytes());
    }

    fs::write(path, buf).map_err(|err| format!("Failed to write {}: {err}", path.display()))
}

pub fn render_wav_command(
    input_path: &Path,
    mode_name: &str,
    params_json: Option<&str>,
    raw_spans: &[String],
    strength_arg: f64,
    output_path: &Path,
) -> color_eyre::Result<()> {
    use color_eyre::eyre::eyre;
    let mode_enum = VoiceFilterMode::from_str_loose(mode_name)
        .ok_or_else(|| eyre!("Unknown voice-filter mode '{mode_name}'"))?;
    let mut spec = mode_enum.preset();
    let mut strength = strength_arg.clamp(0.0, 1.0);
    if let Some(raw_json) = params_json
        && let Some(s) = spec.apply_params_json(raw_json).map_err(|e| eyre!(e))?
    {
        strength = s;
    }
    let mut spans = Vec::with_capacity(raw_spans.len());
    for raw in raw_spans {
        spans.push(DictationSpan::parse(raw).map_err(|e| eyre!(e))?);
    }
    let (input, sr) = read_wav_mono_f32(input_path).map_err(|e| eyre!(e))?;
    let output = render(&input, f64::from(sr), &spec, &spans, strength);
    write_wav_mono_f32(output_path, &output, sr).map_err(|e| eyre!(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms_db(samples: &[f32]) -> f64 {
        if samples.is_empty() {
            return -120.0;
        }
        let sum_sq: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
        let rms = (sum_sq / (samples.len() as f64)).sqrt();
        if rms <= 0.0 {
            -120.0
        } else {
            20.0 * rms.log10()
        }
    }

    #[test]
    fn default_mode_is_muffled() {
        assert_eq!(VoiceFilterMode::default(), VoiceFilterMode::Muffled);
        assert_eq!(VoiceFilterMode::Muffled.id(), "muffled");
    }

    #[test]
    fn all_nine_modes_render_finite_and_expected_levels() {
        let sr = 48_000.0;
        let total_samples = 48_000 * 3; // 3 seconds
        // Synthetic speech-like harmonic signal around -18 dBFS
        let input: Vec<f32> = (0..total_samples)
            .map(|i| {
                let t = (i as f64) / sr;
                let s = 0.12 * (TAU * 180.0 * t).sin()
                    + 0.08 * (TAU * 540.0 * t).sin()
                    + 0.05 * (TAU * 1500.0 * t).sin();
                s as f32
            })
            .collect();

        let spans = [DictationSpan {
            start_ms: 500.0,
            end_ms: 2500.0,
        }];

        for mode in VoiceFilterMode::ALL {
            let out = render(&input, sr, &mode.preset(), &spans, 1.0);
            assert_eq!(out.len(), input.len());
            assert!(
                out.iter().all(|s| s.is_finite() && *s >= -1.0 && *s <= 1.0),
                "mode {} produced non-finite or out-of-range sample",
                mode.id()
            );

            // Passthrough outside dictation before span starts
            assert!(
                (out[100] - input[100]).abs() < 1e-6,
                "mode {} modified audio before dictation span",
                mode.id()
            );
        }

        // Verify Muffled attenuates high frequencies and level during dictation
        let muffled = render(&input, sr, &VoiceFilterMode::Muffled.preset(), &spans, 1.0);
        let in_rms = rms_db(&input[48_000..96_000]);
        let muffled_rms = rms_db(&muffled[48_000..96_000]);
        assert!(
            muffled_rms < in_rms - 8.0,
            "expected Muffled to attenuate by > 8 dB, got in={in_rms:.1} muffled={muffled_rms:.1}"
        );

        // Verify Muted produces ~ -68 dBFS comfort noise when enabled, and silence when disabled
        let muted_comfort = render(&input, sr, &VoiceFilterMode::Muted.preset(), &spans, 1.0);
        let muted_rms = rms_db(&muted_comfort[48_000..96_000]);
        assert!(
            (muted_rms - (-68.0)).abs() < 2.0,
            "expected Muted comfort noise near -68 dBFS, got {muted_rms:.1}"
        );

        let muted_silent = render(
            &input,
            sr,
            &VoiceFilterMode::Muted.spec_with_options(false),
            &spans,
            1.0,
        );
        let silent_rms = rms_db(&muted_silent[48_000..96_000]);
        assert!(
            silent_rms <= -110.0,
            "expected Muted without comfort noise to be silent, got {silent_rms:.1}"
        );
    }

    #[test]
    fn raised_cosine_fade_has_no_step_discontinuity() {
        let sr = 48_000.0;
        let input = vec![0.5_f32; 48_000];
        let spans = [DictationSpan {
            start_ms: 200.0,
            end_ms: 600.0,
        }];
        let out = render(
            &input,
            sr,
            &VoiceFilterMode::Muted.spec_with_options(false),
            &spans,
            1.0,
        );
        let edge = (0.2 * sr) as usize;
        let step_at_edge = (out[edge] - out[edge - 1]).abs();
        assert!(
            step_at_edge < 0.005,
            "expected smooth raised-cosine onset without click, got step {step_at_edge}"
        );
    }
}

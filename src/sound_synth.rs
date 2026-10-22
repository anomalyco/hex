//! Procedural feedback tones: each sound set is a small recipe (pitch, glide,
//! envelope, filtered-noise breath, low-pass and gentle saturation) rendered
//! deterministically into mono samples, with no audio files.
//!
//! `prototypes/sound-synth/synth.js` is the reference implementation used by
//! the tuning prototype. Keep the two step for step identical; the parity test
//! compares this module against renders exported from it.

use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};
use std::sync::OnceLock;

use serde::Deserialize;

pub const SAMPLE_RATE: u32 = 48_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cue {
    Start,
    Stop,
    Cancel,
}

impl Cue {
    #[cfg(test)]
    pub const ALL: [Cue; 3] = [Cue::Start, Cue::Stop, Cue::Cancel];

    fn index(self) -> u32 {
        match self {
            Cue::Start => 0,
            Cue::Stop => 1,
            Cue::Cancel => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Wave {
    #[default]
    Sine,
    Triangle,
}

/// A sound set's recipe. Field names match the prototype's "Copy preset" JSON.
/// Times are milliseconds, pitches hertz, intervals and glides semitones.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: String,
    pub name: String,
    /// The start ping's pitch.
    pub pitch: f64,
    /// How far below the ping the stop pong sits.
    pub interval: f64,
    pub attack: f64,
    pub decay: f64,
    /// Breath level relative to the tone, by RMS.
    pub breath: f64,
    /// Low-pass cutoff.
    pub brightness: f64,
    /// Loudness target in LUFS.
    pub volume: f64,
    pub wave: Wave,
    /// Overtones above the fundamental as `[ratio, amplitude, extra decay per second]`.
    pub partials: Vec<[f64; 3]>,
    pub start_glide: f64,
    pub stop_glide: f64,
    pub glide_ms: f64,
    /// The breath band's centre as a multiple of the tone's pitch.
    pub breath_center: f64,
    pub breath_sweep: f64,
    pub breath_q: f64,
    /// The breath's decay as a multiple of the tone's.
    pub breath_decay: f64,
    pub cancel_drop: f64,
    pub cancel_glide: f64,
    pub cancel_taps: u32,
    pub tap_gap: f64,
    /// Saturation drive; 1 or less leaves the tone linear.
    pub drive: f64,
    pub length: f64,
}

/// The compiled sound-set recipes, in the order Settings lists them.
pub fn presets() -> &'static [Preset] {
    static PRESETS: OnceLock<Vec<Preset>> = OnceLock::new();
    PRESETS.get_or_init(|| {
        serde_json::from_str(include_str!("../resources/sounds/presets.json"))
            .expect("resources/sounds/presets.json is a valid preset list")
    })
}

#[cfg(test)]
pub fn preset(id: &str) -> Option<&'static Preset> {
    presets().iter().find(|preset| preset.id == id)
}

fn semis(semitones: f64) -> f64 {
    2_f64.powf(semitones / 12.0)
}

fn biquad(x: &[f64], b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Vec<f64> {
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    x.iter()
        .map(|&sample| {
            let value = (b0 * sample + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2) / a0;
            x2 = x1;
            x1 = sample;
            y2 = y1;
            y1 = value;
            value
        })
        .collect()
}

fn low_pass(x: &[f64], cutoff: f64, sr: f64) -> Vec<f64> {
    let w = TAU * cutoff.min(sr * 0.45) / sr;
    let (c, alpha) = (w.cos(), w.sin() / (2.0 * FRAC_1_SQRT_2));
    biquad(
        x,
        (1.0 - c) / 2.0,
        1.0 - c,
        (1.0 - c) / 2.0,
        1.0 + alpha,
        -2.0 * c,
        1.0 - alpha,
    )
}

/// Raised-cosine attack, then an exponential decay with time constant `decay` ms.
fn envelope(i: usize, attack_frames: usize, decay: f64, sr: f64) -> f64 {
    if i < attack_frames {
        return 0.5 - 0.5 * (PI * i as f64 / attack_frames as f64).cos();
    }
    (-((i - attack_frames) as f64 / sr) / (decay / 1000.0)).exp()
}

fn frames(ms: f64, sr: f64) -> usize {
    (ms * sr / 1000.0).round() as usize
}

struct Voice {
    at: f64,
    f0: f64,
    glide: f64,
    gain: f64,
    decay: f64,
}

fn voices(preset: &Preset, cue: Cue) -> Vec<Voice> {
    let stop = preset.pitch * semis(-preset.interval);
    let voice = |f0, glide, gain, decay| Voice {
        at: 0.0,
        f0,
        glide,
        gain,
        decay,
    };
    match cue {
        Cue::Start => vec![voice(preset.pitch, preset.start_glide, 1.0, preset.decay)],
        Cue::Stop => vec![voice(stop, preset.stop_glide, 1.0, preset.decay)],
        Cue::Cancel => {
            let f0 = stop * semis(-preset.cancel_drop);
            if preset.cancel_taps < 2 {
                return vec![voice(f0, preset.cancel_glide, 1.0, preset.decay * 0.8)];
            }
            vec![
                voice(f0, preset.cancel_glide, 0.7, preset.decay * 0.66),
                Voice {
                    at: preset.tap_gap,
                    ..voice(
                        f0 * semis(-1.0),
                        preset.cancel_glide,
                        0.5,
                        preset.decay * 0.66,
                    )
                },
            ]
        }
    }
}

fn tone(out: &mut [f64], offset: usize, voice: &Voice, preset: &Preset, sr: f64) {
    let attack_frames = frames(preset.attack, sr).max(1);
    let ratio = semis(voice.glide);
    let glide = preset.glide_ms.max(1.0) / 1000.0;
    let partials: Vec<[f64; 3]> = std::iter::once([1.0, 1.0, 0.0])
        .chain(preset.partials.iter().copied())
        .collect();
    let mut phase = vec![0.0; partials.len()];
    for (i, sample) in out.iter_mut().skip(offset).enumerate() {
        let t = i as f64 / sr;
        let k = (t / glide).min(1.0);
        let f = voice.f0 * ratio.powf(k * (2.0 - k));
        let mut value = 0.0;
        for (j, [r, amp, extra]) in partials.iter().enumerate() {
            phase[j] += TAU * f * r / sr;
            let wave = if j == 0 && preset.wave == Wave::Triangle {
                (2.0 / PI) * phase[j].sin().asin()
            } else {
                phase[j].sin()
            };
            value += amp * wave * (-t * extra).exp();
        }
        *sample += voice.gain * value * envelope(i, attack_frames, voice.decay, sr);
    }
}

/// Band-passed noise whose centre glides from `fc0` to `fc1`, from a fixed LCG seed.
#[allow(clippy::too_many_arguments)]
fn breath(
    n: usize,
    fc0: f64,
    fc1: f64,
    q: f64,
    attack: f64,
    decay: f64,
    seed: u32,
    sr: f64,
) -> Vec<f64> {
    let attack_frames = frames(attack, sr).max(1);
    let mut state = seed;
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    (0..n)
        .map(|i| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let x = f64::from(state >> 8) / 16_777_216.0 * 2.0 - 1.0;
            let fc = fc0 * (fc1 / fc0).powf(i as f64 / n as f64);
            let w = TAU * fc.min(sr * 0.45) / sr;
            let alpha = w.sin() / (2.0 * q);
            let (a0, a1, a2) = (1.0 + alpha, -2.0 * w.cos(), 1.0 - alpha);
            let y = (alpha * x + 0.0 * x1 - alpha * x2 - a1 * y1 - a2 * y2) / a0;
            x2 = x1;
            x1 = x;
            y2 = y1;
            y1 = y;
            y * envelope(i, attack_frames, decay, sr)
        })
        .collect()
}

fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

/// BS.1770 K-weighted loudness of a clip in a 400 ms window (zero-padded), in LUFS.
#[cfg(test)]
pub fn loudness(x: &[f32], sample_rate: u32) -> f64 {
    let x: Vec<f64> = x.iter().map(|&v| f64::from(v)).collect();
    loudness_f64(&x, f64::from(sample_rate))
}

fn loudness_f64(x: &[f64], sr: f64) -> f64 {
    let n = x.len().max((0.4 * sr).round() as usize);
    let mut padded = x.to_vec();
    padded.resize(n, 0.0);
    let mut w = TAU * 1_681.974_450_955_532 / sr;
    let mut c = w.cos();
    let a = 10_f64.powf(3.99984385397 / 40.0);
    let sq = a.sqrt();
    let mut alpha = w.sin() / (2.0 * 0.7071752369554193);
    let shelf = biquad(
        &padded,
        a * (a + 1.0 + (a - 1.0) * c + 2.0 * sq * alpha),
        -2.0 * a * (a - 1.0 + (a + 1.0) * c),
        a * (a + 1.0 + (a - 1.0) * c - 2.0 * sq * alpha),
        a + 1.0 - (a - 1.0) * c + 2.0 * sq * alpha,
        2.0 * (a - 1.0 - (a + 1.0) * c),
        a + 1.0 - (a - 1.0) * c - 2.0 * sq * alpha,
    );
    w = TAU * 38.13547087613982 / sr;
    c = w.cos();
    alpha = w.sin() / (2.0 * 0.5003270373253953);
    let weighted = biquad(
        &shelf,
        (1.0 + c) / 2.0,
        -(1.0 + c),
        (1.0 + c) / 2.0,
        1.0 + alpha,
        -2.0 * c,
        1.0 - alpha,
    );
    let energy = weighted.iter().map(|v| v * v).sum::<f64>();
    -0.691 + 10.0 * (energy / n as f64).max(1e-20).log10()
}

/// Renders one cue of a preset as mono samples at [`SAMPLE_RATE`]. Deterministic.
pub fn render(preset: &Preset, cue: Cue) -> Vec<f32> {
    let sr = f64::from(SAMPLE_RATE);
    let n = frames(preset.length, sr);
    let voices = voices(preset, cue);
    let mut mix = vec![0.0; n];
    for voice in &voices {
        tone(&mut mix, frames(voice.at, sr), voice, preset, sr);
    }
    let fc0 = voices[0].f0 * preset.breath_center;
    let air = breath(
        n,
        fc0,
        fc0 * semis(preset.breath_sweep),
        preset.breath_q,
        preset.attack * 0.75,
        preset.decay * preset.breath_decay,
        0x2545_f491_u32.wrapping_add(cue.index() * 7919),
        sr,
    );
    let level = preset.breath * rms(&mix) / rms(&air).max(1e-12);
    for (sample, air) in mix.iter_mut().zip(&air) {
        *sample += air * level;
    }
    let mut out = low_pass(
        &low_pass(&mix, preset.brightness, sr),
        preset.brightness,
        sr,
    );
    let peak = out.iter().fold(0.0_f64, |peak, v| peak.max(v.abs()));
    let drive = preset.drive;
    let norm = if drive > 1.0 { drive.tanh() } else { 1.0 };
    for sample in &mut out {
        let v = *sample / peak.max(1e-12);
        *sample = if drive > 1.0 {
            (drive * v).tanh() / norm
        } else {
            v
        };
    }
    let fade = frames(10.0, sr);
    for i in 0..fade.min(n) {
        out[n - 1 - i] *= i as f64 / fade as f64;
    }
    let gain = 10_f64.powf((preset.volume - loudness_f64(&out, sr)) / 20.0);
    out.iter().map(|v| (v * gain) as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = SAMPLE_RATE as f64;

    /// When the sound first swings to half its loudest sample, as in the iOS cue tests.
    fn half_peak_onset_ms(samples: &[f32]) -> f64 {
        let peak = samples.iter().fold(0.0_f32, |p, v| p.max(v.abs()));
        let index = samples
            .iter()
            .position(|v| v.abs() >= 0.5 * peak)
            .unwrap_or(0);
        index as f64 * 1000.0 / SR
    }

    /// Power-weighted mean frequency over Hann-windowed 2048-sample frames.
    fn spectral_centroid(samples: &[f32]) -> f64 {
        const N: usize = 2048;
        let (mut num, mut den) = (0.0, 0.0);
        let mut start = 0;
        loop {
            let frame: Vec<f64> = (0..N)
                .map(|n| {
                    let v = samples.get(start + n).copied().unwrap_or(0.0) as f64;
                    v * (0.5 - 0.5 * (TAU * n as f64 / N as f64).cos())
                })
                .collect();
            for k in 1..N / 2 {
                let (mut re, mut im) = (0.0, 0.0);
                for (n, v) in frame.iter().enumerate() {
                    let angle = TAU * (k * n) as f64 / N as f64;
                    re += v * angle.cos();
                    im -= v * angle.sin();
                }
                let power = re * re + im * im;
                num += power * k as f64 * SR / N as f64;
                den += power;
            }
            start += N / 2;
            if start + N / 2 >= samples.len() {
                break;
            }
        }
        num / den
    }

    #[test]
    fn every_preset_renders_a_subtle_immediate_cue() {
        assert!(presets().len() >= 6);
        for preset in presets() {
            for cue in Cue::ALL {
                let samples = render(preset, cue);
                let label = format!("{} {cue:?}", preset.id);
                let length_ms = samples.len() as f64 * 1000.0 / SR;
                let onset = half_peak_onset_ms(&samples);
                let peak = samples.iter().fold(0.0_f32, |p, v| p.max(v.abs()));
                let lufs = loudness(&samples, SAMPLE_RATE);
                let centroid = spectral_centroid(&samples);
                eprintln!(
                    "{label}: {length_ms:.0} ms, onset {onset:.1} ms, peak {:.1} dBFS, {lufs:.1} LUFS, centroid {centroid:.0} Hz",
                    20.0 * f64::from(peak).log10()
                );
                assert!(onset <= 10.0, "{label} reaches half its peak at {onset} ms");
                assert!(length_ms <= 250.0, "{label} lasts {length_ms} ms");
                assert!(samples.iter().all(|v| v.is_finite()));
                assert!(peak < 0.5, "{label} peaks at {peak}, near clipping");
                assert!(
                    (lufs - preset.volume).abs() < 0.1,
                    "{label} measures {lufs} LUFS, not {}",
                    preset.volume
                );
                assert!(
                    (350.0..=1500.0).contains(&centroid),
                    "{label} centroid {centroid} Hz is outside the comfortable range"
                );
                assert!(
                    samples.last().is_some_and(|v| v.abs() < 1e-3),
                    "{label} ends without fading out"
                );
            }
        }
    }

    #[test]
    fn rendering_is_deterministic_and_pong_sits_below_ping() {
        let preset = preset("breath_ping").unwrap();
        assert_eq!(render(preset, Cue::Start), render(preset, Cue::Start));
        let start = voices(preset, Cue::Start)[0].f0;
        let stop = voices(preset, Cue::Stop)[0].f0;
        let cancel = voices(preset, Cue::Cancel)[0].f0;
        assert!(stop < start && cancel < stop);
    }

    fn reference(name: &str) -> Vec<f32> {
        let path = format!(
            "{}/tests/fixtures/sound_synth/{name}.f32",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read(&path)
            .unwrap_or_else(|error| panic!("{path}: {error}"))
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| f32::from_le_bytes(*bytes))
            .collect()
    }

    /// The prototype's JS synth and this module render the same samples.
    /// Regenerate the fixtures with `bun prototypes/sound-synth/render.ts`.
    #[test]
    fn matches_the_reference_js_renders() {
        let preset = preset("breath_ping").unwrap();
        for (cue, name) in [
            (Cue::Start, "breath_ping-start"),
            (Cue::Stop, "breath_ping-stop"),
            (Cue::Cancel, "breath_ping-cancel"),
        ] {
            let expected = reference(name);
            let actual = render(preset, cue);
            assert_eq!(actual.len(), expected.len(), "{name} length");
            let worst = actual
                .iter()
                .zip(&expected)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f32, f32::max);
            assert!(worst < 1e-6, "{name} differs from the JS render by {worst}");
        }

        #[derive(Deserialize)]
        struct Summary {
            frames: usize,
            energy: f64,
            peak: f64,
        }
        let summary: std::collections::HashMap<String, std::collections::HashMap<String, Summary>> =
            serde_json::from_str(include_str!("../tests/fixtures/sound_synth/summary.json"))
                .unwrap();
        for preset in presets() {
            for (cue, key) in [
                (Cue::Start, "start"),
                (Cue::Stop, "stop"),
                (Cue::Cancel, "cancel"),
            ] {
                let expected = &summary[&preset.id][key];
                let samples = render(preset, cue);
                let energy = samples.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
                let peak = samples
                    .iter()
                    .fold(0.0_f64, |p, v| p.max(f64::from(v.abs())));
                assert_eq!(samples.len(), expected.frames, "{} {key}", preset.id);
                assert!(
                    (energy - expected.energy).abs() <= expected.energy * 1e-5,
                    "{} {key} energy",
                    preset.id
                );
                assert!(
                    (peak - expected.peak).abs() < 1e-6,
                    "{} {key} peak",
                    preset.id
                );
            }
        }
    }
}

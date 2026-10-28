//! Dot scope (experiment `dot-scope`): the recording indicator as a field of
//! dots lit by a braided pair of oscilloscope traces that follow the voice.
//! Ported from the t422 prototype (look A), itself a port of the OpenCode iOS
//! dot scope (`DotScopeSimulation.swift`).
//!
//! [`Tuning`] is what HUD Lab edits; it is saved as `dot-scope.json` in Hex's
//! Application Support folder. [`Scope`] steps the simulation at a fixed
//! 240 Hz, so any frame rate draws the same picture, and yields one
//! brightness pair per dot for `dictation_indicator.metal` to draw.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
mod scope;
#[cfg(target_os = "macos")]
pub use scope::*;

/// The most dots [`Scope`] lights; the shader's dot buffer holds this many.
pub const MAX_COLUMNS: usize = 64;
pub const MAX_ROWS: usize = 15;
pub const MAX_DOTS: usize = MAX_COLUMNS * MAX_ROWS;
/// The plate height the HUD window is laid out around.
pub const PLATE_HEIGHT: f32 = 26.0;

/// Dot scope's live parameters. Every default is the prototype's value.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tuning {
    pub columns: f32,
    pub rows: f32,
    /// Dot pitch, pt.
    pub spacing: f32,
    /// Plate width while recording, pt.
    pub width: f32,
    /// Plate width while transcribing and processing, pt.
    pub processing_width: f32,
    pub height: f32,
    /// Multiplies how fast the braid's components and carrier travel.
    pub braid_speed: f32,
    /// Helix turns across the plate.
    pub helix_cycles: f32,
    /// Multiplies the microphone level.
    pub gain: f32,
    /// The amplitude spring's frequency, Hz.
    pub response: f32,
    /// How long the level takes to fall, s.
    pub release: f32,
    /// How fast the voice's history travels out from the centre, pt/s.
    pub history_speed: f32,
    /// The phosphor tail, s.
    pub phosphor: f32,
    /// Multiplies the glow behind the voice.
    pub glow: f32,
    /// Multiplies the colours' saturation.
    pub saturation: f32,
    /// One processing beam pass, s.
    pub shimmer_pass: f32,
    /// Multiplies the processing beam's brightness.
    pub shimmer: f32,
    /// Enter, exit, and morph duration, s.
    pub transition: f32,
    pub bounce: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            columns: 43.0,
            rows: 9.0,
            spacing: 2.6,
            width: 112.0,
            processing_width: 76.0,
            height: PLATE_HEIGHT,
            braid_speed: 1.0,
            helix_cycles: 1.6,
            gain: 1.0,
            response: 6.5,
            release: 0.15,
            history_speed: 150.0,
            phosphor: 0.6,
            glow: 1.0,
            saturation: 1.0,
            shimmer_pass: 0.55,
            shimmer: 1.0,
            transition: 0.3,
            bounce: 0.15,
        }
    }
}

/// One tunable parameter: its range and how HUD Lab labels it.
#[derive(Clone, Copy, Debug)]
pub struct Knob {
    pub key: &'static str,
    pub label: &'static str,
    pub min: f32,
    pub max: f32,
    pub whole: bool,
    pub unit: &'static str,
    index: usize,
}

const fn knob(
    index: usize,
    key: &'static str,
    label: &'static str,
    min: f32,
    max: f32,
    unit: &'static str,
) -> Knob {
    Knob {
        key,
        label,
        min,
        max,
        whole: false,
        unit,
        index,
    }
}

const fn whole(index: usize, key: &'static str, label: &'static str, min: f32, max: f32) -> Knob {
    Knob {
        whole: true,
        ..knob(index, key, label, min, max, "")
    }
}

pub const KNOBS: [Knob; 19] = [
    whole(0, "dot-columns", "Columns", 11.0, MAX_COLUMNS as f32),
    whole(1, "dot-rows", "Rows", 3.0, MAX_ROWS as f32),
    knob(2, "dot-spacing", "Dot spacing", 1.6, 4.0, " pt"),
    knob(3, "dot-width", "Recording width", 40.0, 150.0, " pt"),
    knob(
        4,
        "dot-processing-width",
        "Processing width",
        26.0,
        150.0,
        " pt",
    ),
    knob(5, "dot-height", "Height", 14.0, 40.0, " pt"),
    knob(6, "dot-braid-speed", "Braid speed", 0.0, 3.0, "x"),
    knob(7, "dot-helix", "Helix turns", 0.0, 4.0, ""),
    knob(8, "dot-gain", "Input gain", 0.25, 3.0, "x"),
    knob(9, "dot-response", "Amplitude response", 1.0, 16.0, " Hz"),
    knob(10, "dot-release", "Smoothing", 0.03, 0.6, " s"),
    knob(11, "dot-history", "History speed", 40.0, 400.0, " pt/s"),
    knob(12, "dot-phosphor", "Phosphor tail", 0.05, 1.5, " s"),
    knob(13, "dot-glow", "Glow", 0.0, 3.0, "x"),
    knob(14, "dot-saturation", "Colour", 0.0, 2.5, "x"),
    knob(15, "dot-shimmer-pass", "Shimmer pass", 0.2, 1.5, " s"),
    knob(16, "dot-shimmer", "Shimmer brightness", 0.0, 2.5, "x"),
    knob(17, "dot-transition", "Transition", 0.1, 0.8, " s"),
    knob(18, "dot-bounce", "Bounce", 0.0, 0.6, ""),
];

impl Knob {
    pub fn get(&self, tuning: &Tuning) -> f32 {
        let mut copy = *tuning;
        *copy.slot(self.index)
    }

    pub fn set(&self, tuning: &mut Tuning, value: f32) {
        let value = value.clamp(self.min, self.max);
        *tuning.slot(self.index) = if self.whole { value.round() } else { value };
    }

    /// The value as a 0–1 slider position.
    pub fn fraction(&self, tuning: &Tuning) -> f32 {
        ((self.get(tuning) - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }

    pub fn set_fraction(&self, tuning: &mut Tuning, fraction: f32) {
        self.set(
            tuning,
            self.min + fraction.clamp(0.0, 1.0) * (self.max - self.min),
        );
    }

    pub fn display(&self, tuning: &Tuning) -> String {
        let value = self.get(tuning);
        if self.whole {
            format!("{value:.0}")
        } else if self.max >= 100.0 {
            format!("{value:.0}{}", self.unit)
        } else {
            format!("{value:.2}{}", self.unit)
        }
    }
}

impl Tuning {
    fn slot(&mut self, index: usize) -> &mut f32 {
        match index {
            0 => &mut self.columns,
            1 => &mut self.rows,
            2 => &mut self.spacing,
            3 => &mut self.width,
            4 => &mut self.processing_width,
            5 => &mut self.height,
            6 => &mut self.braid_speed,
            7 => &mut self.helix_cycles,
            8 => &mut self.gain,
            9 => &mut self.response,
            10 => &mut self.release,
            11 => &mut self.history_speed,
            12 => &mut self.phosphor,
            13 => &mut self.glow,
            14 => &mut self.saturation,
            15 => &mut self.shimmer_pass,
            16 => &mut self.shimmer,
            17 => &mut self.transition,
            _ => &mut self.bounce,
        }
    }

    /// Every value inside its knob's range (a hand-edited file may not be).
    pub fn clamped(mut self) -> Self {
        for knob in &KNOBS {
            let value = knob.get(&self);
            let value = if value.is_finite() {
                value
            } else {
                knob.get(&Self::default())
            };
            knob.set(&mut self, value);
        }
        self
    }

    pub fn json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    fn file() -> color_eyre::Result<PathBuf> {
        Ok(crate::app_paths::support_dir()?.join("dot-scope.json"))
    }

    /// The saved tuning, or the defaults when there is none.
    pub fn load() -> Self {
        let Ok(path) = Self::file() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Self>(&text) {
                Ok(tuning) => tuning.clamped(),
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "ignoring an unreadable Dot scope tuning");
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    /// Saves the tuning atomically so a crash never leaves half a file.
    pub fn save(&self) -> color_eyre::Result<()> {
        let path = Self::file()?;
        let staging = path.with_extension("json.tmp");
        std::fs::write(&staging, self.json() + "\n")?;
        std::fs::rename(&staging, &path)?;
        Ok(())
    }
}

/// The prototype's level: RMS in dB, −40 dB silent through −10 dB full.
pub fn level_from_rms(rms: f32) -> f32 {
    ((20.0 * (rms.max(0.0) + 1e-6).log10() + 40.0) / 30.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_prototypes() {
        let t = Tuning::default();
        assert_eq!((t.columns, t.rows, t.spacing), (43.0, 9.0, 2.6));
        assert_eq!((t.width, t.processing_width, t.height), (112.0, 76.0, 26.0));
        assert_eq!((t.response, t.release, t.history_speed), (6.5, 0.15, 150.0));
        assert_eq!((t.phosphor, t.shimmer_pass), (0.6, 0.55));
        assert_eq!((t.transition, t.bounce), (0.3, 0.15));
        assert_eq!(t.clamped(), t, "every default sits inside its knob");
    }

    #[test]
    fn json_round_trips_and_fills_missing_values() {
        let tuning = Tuning {
            glow: 2.0,
            ..Tuning::default()
        };
        let back: Tuning = serde_json::from_str(&tuning.json()).unwrap();
        assert_eq!(back, tuning);
        let partial: Tuning = serde_json::from_str(r#"{"rows": 11}"#).unwrap();
        assert_eq!(partial.rows, 11.0);
        assert_eq!(partial.columns, 43.0);
        let wild: Tuning = serde_json::from_str(r#"{"columns": 900, "rows": 4.4}"#).unwrap();
        let wild = wild.clamped();
        assert_eq!((wild.columns, wild.rows), (MAX_COLUMNS as f32, 4.0));
    }

    #[test]
    fn knobs_round_trip_through_slider_fractions() {
        let mut tuning = Tuning::default();
        for knob in &KNOBS {
            knob.set_fraction(&mut tuning, 0.5);
            let back = knob.fraction(&tuning);
            let tolerance = if knob.whole {
                0.5 / (knob.max - knob.min)
            } else {
                1e-4
            };
            assert!((back - 0.5).abs() <= tolerance + 1e-4, "{}", knob.key);
        }
        let mut keys: Vec<_> = KNOBS.iter().map(|knob| knob.key).collect();
        keys.dedup();
        assert_eq!(keys.len(), KNOBS.len());
    }

    #[test]
    fn level_maps_minus_40_to_minus_10_db() {
        assert_eq!(level_from_rms(0.0), 0.0);
        assert!((level_from_rms(0.01) - 0.0).abs() < 1e-3);
        assert!((level_from_rms(0.1) - 2.0 / 3.0).abs() < 1e-3);
        assert_eq!(level_from_rms(0.5), 1.0);
    }
}

//! Dot scope (experiment `dot-scope`): the recording indicator as a field of
//! dots lit by a braided pair of oscilloscope traces that follow the voice.
//! Ported from the t422 prototype (look A), itself a port of the OpenCode iOS
//! dot scope (`DotScopeSimulation.swift`).
//!
//! [`Tuning`] is what HUD Lab edits; it is saved as `dot-scope.json` in Hex's
//! Application Support folder. [`Scope`] steps the simulation at a fixed
//! 240 Hz, so any frame rate draws the same picture, and yields one
//! brightness pair per dot for `rune_hud.wgsl` to draw.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

mod scope;
pub use scope::*;

/// The most dots [`Scope`] lights; the shader's dot buffer holds this many.
/// A wide field gets fewer rows ([`Tuning::clamped`]).
pub const MAX_COLUMNS: usize = 96;
pub const MAX_ROWS: usize = 15;
pub const MAX_DOTS: usize = 1024;
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
    /// Follows the speaker's recent peak instead of the prototype's fixed
    /// −40 to −10 dB window ([`AutoLevel`]).
    pub auto_level: bool,
    /// How long the auto level takes to forget a loud peak, s.
    pub auto_release: f32,
    /// Input below this never lights the scope, and the auto level never
    /// boosts the room up to full, dBFS.
    pub noise_floor: f32,
    /// How transcribing and processing look.
    pub processing: Processing,
    /// Where the HUD sits.
    pub placement: Placement,
    /// Draws From the notch's island on a screen without a notch, from a
    /// pretend one in the middle of the menu bar, for trying it on a
    /// flat display.
    pub pretend_notch: bool,
    /// How far From the notch's island reaches past the notch on each
    /// side, pt. The braid fills the island.
    pub island_spread: f32,
}

/// How Dot scope says "working" while a dictation transcribes and
/// processes. Every look settles into done the same way: the field clears
/// from the middle out as it collapses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Processing {
    /// Round 1: the braid irons flat and a beam sweeps back and forth.
    Sweep,
    /// The braid keeps turning and light flows through it left to right.
    #[default]
    Flow,
    /// Pulses leave the middle the way the voice did.
    Pulse,
}

impl Processing {
    pub const ALL: [Self; 3] = [Self::Sweep, Self::Flow, Self::Pulse];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sweep => "Sweep",
            Self::Flow => "Flow",
            Self::Pulse => "Pulse",
        }
    }
}

/// Where the HUD sits on the screen with the pointer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Placement {
    /// Under the menu bar (or above the Dock, with the indicator at the
    /// bottom), as before.
    #[default]
    Below,
    /// A small plate in the menu bar: centred on a flat screen, and just
    /// right of the notch on a screen with one.
    MenuBar,
    /// A black island that grows out of the notch; in the menu bar on a
    /// screen without one.
    Notch,
}

impl Placement {
    pub const ALL: [Self; 3] = [Self::Below, Self::MenuBar, Self::Notch];

    pub fn label(self) -> &'static str {
        match self {
            Self::Below => "Below the menu bar",
            Self::MenuBar => "In the menu bar",
            Self::Notch => "From the notch",
        }
    }
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
            auto_level: true,
            auto_release: 3.0,
            noise_floor: -60.0,
            processing: Processing::default(),
            placement: Placement::default(),
            pretend_notch: false,
            island_spread: 28.0,
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

pub const KNOBS: [Knob; 22] = [
    whole(0, "dot-columns", "Columns", 11.0, MAX_COLUMNS as f32),
    whole(1, "dot-rows", "Rows", 3.0, MAX_ROWS as f32),
    knob(2, "dot-spacing", "Dot spacing", 1.6, 4.0, " pt"),
    knob(3, "dot-width", "Recording width", 40.0, 280.0, " pt"),
    knob(
        4,
        "dot-processing-width",
        "Processing width",
        26.0,
        280.0,
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
    knob(
        19,
        "dot-auto-release",
        "Auto-level release",
        0.5,
        10.0,
        " s",
    ),
    knob(20, "dot-noise-floor", "Noise floor", -80.0, -30.0, " dB"),
    knob(
        21,
        "dot-island-spread",
        "Island past the notch",
        0.0,
        60.0,
        " pt",
    ),
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
        } else if self.max - self.min >= 40.0 {
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
            18 => &mut self.bounce,
            19 => &mut self.auto_release,
            20 => &mut self.noise_floor,
            _ => &mut self.island_spread,
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
        let rows = (MAX_DOTS / self.columns as usize).min(MAX_ROWS);
        self.rows = self.rows.min(rows as f32);
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

static LAB: std::sync::Mutex<Option<Tuning>> = std::sync::Mutex::new(None);

/// The tuning the HUD shows now: HUD Lab's latest, else the saved one.
/// The playground's HUD stories start from it and change it, so HUD Lab
/// and the stories edit one tuning.
pub fn lab() -> Tuning {
    *LAB.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_or_insert_with(Tuning::load)
}

pub fn set_lab(tuning: Tuning) {
    *LAB.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(tuning);
}

/// A meter's RMS as dBFS.
pub fn loudness(rms: f32) -> f32 {
    20.0 * (rms.max(0.0) + 1e-6).log10()
}

/// The prototype's level: −40 dB silent through −10 dB full. A laptop
/// microphone at talking distance peaks near −30 dB, so speech rarely
/// lit a third of it.
pub fn fixed_level(loudness: f32) -> f32 {
    ((loudness + 40.0) / 30.0).clamp(0.0, 1.0)
}

/// The quietest the speaker's peak may be thought to be, above the room.
const AUTO_MIN_SPAN: f32 = 16.0;
/// How far below the peak the level starts.
const AUTO_RANGE: f32 = 24.0;
/// How far above the room the level starts.
const AUTO_ROOM_MARGIN: f32 = 5.0;
const AUTO_ATTACK: f32 = 0.04;
const ROOM_FALL: f32 = 0.3;
const ROOM_RISE: f32 = 6.0;

/// A slow auto level: the loudest the speaker has recently been reads as
/// full and the room as silent, so a quiet microphone and a loud one move
/// the scope alike. The peak rises within 40 ms and falls back over
/// `release`; the room level falls to any quieter moment in 0.3 s and
/// creeps up over 6 s, so speech never becomes the room. The peak never
/// sits less than 16 dB above the room or `noise_floor`, so silence is
/// never boosted to full.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoLevel {
    peak: f32,
    room: f32,
}

impl Default for AutoLevel {
    /// A laptop microphone at talking distance: peaks near −30 dB over a
    /// room near −55 dB.
    fn default() -> Self {
        Self {
            peak: -30.0,
            room: -55.0,
        }
    }
}

impl AutoLevel {
    /// Steps `h` seconds at `loudness` dBFS and returns the 0–1 level.
    pub fn step(&mut self, loudness: f32, h: f32, release: f32, noise_floor: f32) -> f32 {
        let db = loudness.max(-120.0);
        let follow = |value: &mut f32, target: f32, time: f32| {
            *value += (target - *value) * (1.0 - (-h / time.max(1e-3)).exp());
        };
        let room_time = if db < self.room { ROOM_FALL } else { ROOM_RISE };
        follow(&mut self.room, db, room_time);
        let lowest = self.room.max(noise_floor) + AUTO_MIN_SPAN;
        if db > self.peak {
            follow(&mut self.peak, db, AUTO_ATTACK);
        } else {
            follow(&mut self.peak, lowest, release);
        }
        self.peak = self.peak.max(lowest);
        let low = (self.room.max(noise_floor) + AUTO_ROOM_MARGIN).max(self.peak - AUTO_RANGE);
        ((db - low) / (self.peak - low).max(1.0)).clamp(0.0, 1.0)
    }

    /// The speaker's recent peak and the room, dBFS.
    #[cfg(test)]
    pub fn levels(&self) -> (f32, f32) {
        (self.peak, self.room)
    }
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
    fn a_wide_field_gives_up_rows_to_stay_in_the_dot_buffer() {
        let wide = Tuning {
            columns: 96.0,
            rows: 15.0,
            ..Tuning::default()
        }
        .clamped();
        assert_eq!(wide.rows, 10.0);
        assert!((wide.columns * wide.rows) as usize <= MAX_DOTS);
        let narrow = Tuning {
            rows: 15.0,
            ..Tuning::default()
        }
        .clamped();
        assert_eq!(narrow.rows, 15.0);
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
    fn fixed_level_maps_minus_40_to_minus_10_db() {
        let level = |rms| fixed_level(loudness(rms));
        assert_eq!(level(0.0), 0.0);
        assert!((level(0.01) - 0.0).abs() < 1e-3);
        assert!((level(0.1) - 2.0 / 3.0).abs() < 1e-3);
        assert_eq!(level(0.5), 1.0);
    }

    /// Syllables peaking at `peak` dB over a room at `room` dB, 4 a second,
    /// for `seconds` at 240 Hz; returns the levels.
    fn speak(auto: &mut AutoLevel, peak: f32, room: f32, seconds: f32) -> Vec<f32> {
        let h = 1.0 / 240.0;
        (0..(seconds / h) as usize)
            .map(|i| {
                let t = i as f32 * h;
                let syllable = (std::f32::consts::TAU * 4.0 * t).sin().max(0.0);
                let db = room + (peak - room) * syllable.powf(0.5);
                auto.step(db, h, 3.0, -60.0)
            })
            .collect()
    }

    fn high(levels: &[f32]) -> f32 {
        let mut sorted = levels.to_vec();
        sorted.sort_by(f32::total_cmp);
        sorted[sorted.len() * 9 / 10]
    }

    #[test]
    fn auto_level_moves_a_quiet_microphone_as_far_as_a_loud_one() {
        let mut quiet = AutoLevel::default();
        speak(&mut quiet, -36.0, -58.0, 4.0);
        let quiet = high(&speak(&mut quiet, -36.0, -58.0, 2.0));
        let mut loud = AutoLevel::default();
        speak(&mut loud, -14.0, -50.0, 4.0);
        let loud = high(&speak(&mut loud, -14.0, -50.0, 2.0));
        assert!(quiet > 0.8 && loud > 0.8, "quiet {quiet} loud {loud}");
        assert!(fixed_level(-36.0) < 0.2, "the fixed window barely moved");
    }

    #[test]
    fn auto_level_never_boosts_the_room() {
        let mut auto = AutoLevel::default();
        speak(&mut auto, -30.0, -55.0, 2.0);
        let h = 1.0 / 240.0;
        let mut last = 1.0;
        for _ in 0..(20.0 / h) as usize {
            last = auto.step(-48.0, h, 3.0, -60.0);
        }
        assert!(last < 0.05, "a fan at −48 dB lit {last}");
        let (peak, room) = auto.levels();
        assert!(peak >= room + AUTO_MIN_SPAN - 0.01);
        let mut silent = AutoLevel::default();
        for _ in 0..(20.0 / h) as usize {
            last = silent.step(-90.0, h, 3.0, -60.0);
        }
        assert_eq!(last, 0.0);
        assert!(silent.levels().0 >= -60.0 + AUTO_MIN_SPAN - 0.01);
    }

    #[test]
    fn new_settings_fill_in_for_an_old_file() {
        let old: Tuning = serde_json::from_str(r#"{"gain": 2.0}"#).unwrap();
        assert!(old.auto_level);
        assert_eq!(old.processing, Processing::Flow);
        assert_eq!(old.placement, Placement::Below);
        let notch: Tuning =
            serde_json::from_str(r#"{"placement": "notch", "processing": "pulse"}"#).unwrap();
        assert_eq!(
            (notch.placement, notch.processing),
            (Placement::Notch, Processing::Pulse)
        );
        let bar = Tuning {
            placement: Placement::MenuBar,
            ..Tuning::default()
        };
        assert!(bar.json().contains("\"placement\": \"menu-bar\""));
    }
}

//! What a recording captures, chosen entirely by the host.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::frames::FrameTap;
use crate::input::HostInputReceiver;

/// The name the core gives its manifest. Every other file name comes from [`Names`].
pub const MANIFEST_NAME: &str = "capture.json";

/// Everything one recording does.
pub struct Plan {
    /// Displays, windows and regions, each recorded to its own MP4. The first source owns audio.
    pub sources: Vec<SourcePlan>,
    pub cursor: CursorDrawing,
    pub audio: AudioPlan,
    pub input: Input,
    /// Low-rate RGBA frames for the host while recording, such as for a contact sheet.
    pub frames: Option<FrameTap>,
    /// Windows and apps to keep out of display sources. The core excludes nothing on its own.
    pub exclude: Exclusions,
    /// Frames per second for every source.
    pub fps: u32,
    /// The bundle directory. It must exist; the core writes only the named files and the manifest.
    pub dir: PathBuf,
    pub names: Names,
}

/// One thing to record.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(into = "SourceJson", from = "SourceJson")]
pub enum Source {
    /// A display by its `CGDirectDisplayID`.
    Display(u32),
    /// A window by its `CGWindowID`.
    Window(u32),
    /// Part of a display: `rect` is in the display's own points from its top-left corner (the
    /// stream's `sourceRect`). It is trimmed to the display.
    Region { display: u32, rect: Rect },
}

/// `{"kind": "display", "id": 1}`, `{"kind": "region", "display": 1, "rect": {...}}`.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum SourceJson {
    Display { id: u32 },
    Window { id: u32 },
    Region { display: u32, rect: Rect },
}

impl From<Source> for SourceJson {
    fn from(source: Source) -> Self {
        match source {
            Source::Display(id) => Self::Display { id },
            Source::Window(id) => Self::Window { id },
            Source::Region { display, rect } => Self::Region { display, rect },
        }
    }
}

impl From<SourceJson> for Source {
    fn from(source: SourceJson) -> Self {
        match source {
            SourceJson::Display { id } => Self::Display(id),
            SourceJson::Window { id } => Self::Window(id),
            SourceJson::Region { display, rect } => Self::Region { display, rect },
        }
    }
}

/// A source and the size it records at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourcePlan {
    pub source: Source,
    /// The longest side of the video in pixels, keeping the aspect. `None` records at the
    /// source's own pixels; a source is never scaled above them.
    pub long_edge: Option<u32>,
}

impl From<Source> for SourcePlan {
    fn from(source: Source) -> Self {
        Self {
            source,
            long_edge: None,
        }
    }
}

/// A rectangle in points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub(crate) fn is_valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
    }

    /// The part of `self` inside `bounds`, if any.
    pub(crate) fn intersection(self, bounds: Self) -> Option<Self> {
        let x = self.x.max(bounds.x);
        let y = self.y.max(bounds.y);
        let right = (self.x + self.width).min(bounds.x + bounds.width);
        let bottom = (self.y + self.height).min(bounds.y + bounds.height);
        let rect = Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
        };
        rect.is_valid().then_some(rect)
    }
}

/// The encoded size of a source `points` large at `scale` pixels per point, with its long edge at
/// most `long_edge` pixels. Never above the source's own pixels; both sides even, as H.264 wants.
#[must_use]
pub(crate) fn output_size(
    points: (f64, f64),
    scale: f64,
    long_edge: Option<u32>,
) -> Option<(u32, u32)> {
    let (width, height) = points;
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return None;
    }
    let native = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let scale = long_edge.map_or(native, |edge| {
        (f64::from(edge) / width.max(height)).min(native)
    });
    let side = |points: f64| {
        let pixels = (points * scale).round();
        if !pixels.is_finite() || pixels > f64::from(u32::MAX) {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some(((pixels as u32) & !1).max(2))
    };
    Some((side(width)?, side(height)?))
}

/// Whether the cursor is drawn into the video.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorDrawing {
    /// Left out of the pixels; its positions can still be sampled into [`Names::cursor`].
    #[default]
    Hidden,
    /// Drawn into the pixels by `ScreenCaptureKit`.
    Baked,
}

#[derive(Clone, Debug, Default)]
pub struct AudioPlan {
    pub mic: Mic,
    /// The system's audio output, captured on the first source's stream.
    pub system: bool,
}

/// Where microphone audio comes from.
#[derive(Clone, Debug, Default)]
pub enum Mic {
    /// No microphone. `ScreenCaptureKit`'s microphone capture stays off.
    #[default]
    None,
    /// `ScreenCaptureKit` opens this Core Audio device (by unique ID) and records it as a track in
    /// the first source's MP4.
    Device(String),
    /// The host already owns the microphone and pushes PCM through
    /// [`Recording::external_audio`](crate::Recording::external_audio). The core never opens one.
    External(ExternalMic),
}

/// Microphone PCM the host pushes, written to [`Names::microphone`] as 16-bit WAV.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExternalMic {
    /// Mono frames per second of every chunk the host offers.
    pub sample_rate: u32,
    /// Chunks the sink holds before it starts recording drops as gaps.
    pub capacity: usize,
    /// How long after stop the core waits for audio the host captured before stop.
    pub tail_grace: Duration,
}

impl Default for ExternalMic {
    fn default() -> Self {
        Self {
            sample_rate: 16_000,
            capacity: 256,
            tail_grace: Duration::from_millis(250),
        }
    }
}

/// Where pointer and keyboard events come from.
#[derive(Default)]
pub enum Input {
    /// No input log and no click telemetry.
    #[default]
    None,
    /// The host forwards events from its own tap through a bounded queue.
    Host(HostInputReceiver),
    /// The core runs its own listen-only `CGEventTap` for mouse buttons. It needs Input Monitoring.
    Tap,
}

/// What display and region sources leave out. Window sources capture only their window, so
/// exclusions do not apply to them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Exclusions {
    /// Window IDs.
    pub windows: Vec<u32>,
    /// Process IDs of apps whose every window is left out.
    pub apps: Vec<i32>,
}

/// File names inside [`Plan::dir`], one per artifact the host wants. An artifact without a name
/// is not written, though its data still comes back in [`Finished`](crate::Finished).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Names {
    /// One MP4 per source, in source order.
    pub videos: Vec<String>,
    /// The WAV for [`Mic::External`].
    pub microphone: Option<String>,
    /// A separate AAC file of system audio.
    pub system_audio: Option<String>,
    /// Cursor positions and clicks per source, as JSON. Naming it turns on the pointer sampler.
    pub cursor: Option<String>,
    /// The input log, as JSON.
    pub input: Option<String>,
}

impl Names {
    pub(crate) fn validate(&self, sources: usize, plan_audio: &AudioPlan) -> Result<(), Error> {
        if self.videos.len() != sources {
            return Err(Error::InvalidNames(format!(
                "{} video names for {sources} sources",
                self.videos.len()
            )));
        }
        if matches!(plan_audio.mic, Mic::External(_)) && self.microphone.is_none() {
            return Err(Error::InvalidNames(
                "an external microphone needs a microphone file name".into(),
            ));
        }
        if self.system_audio.is_some() && !plan_audio.system {
            return Err(Error::InvalidNames(
                "a system audio file needs system audio in the plan".into(),
            ));
        }
        let mut seen = HashSet::new();
        for name in self.all() {
            if !is_plain_name(name) {
                return Err(Error::InvalidNames(format!(
                    "{name:?} is not a plain file name"
                )));
            }
            if name == MANIFEST_NAME {
                return Err(Error::InvalidNames(format!(
                    "{MANIFEST_NAME} is the manifest"
                )));
            }
            if !seen.insert(name) {
                return Err(Error::InvalidNames(format!("{name:?} is used twice")));
            }
        }
        Ok(())
    }

    pub(crate) fn all(&self) -> impl Iterator<Item = &str> {
        self.videos.iter().map(String::as_str).chain(
            [
                &self.microphone,
                &self.system_audio,
                &self.cursor,
                &self.input,
            ]
            .into_iter()
            .filter_map(Option::as_deref),
        )
    }
}

fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0'])
        && !name.starts_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(videos: &[&str]) -> Names {
        Names {
            videos: videos.iter().map(|name| (*name).to_owned()).collect(),
            ..Names::default()
        }
    }

    #[test]
    fn names_must_be_plain_distinct_and_one_per_source() {
        let audio = AudioPlan::default();
        assert!(names(&["recording.mp4"]).validate(1, &audio).is_ok());
        assert!(names(&["recording.mp4"]).validate(2, &audio).is_err());
        assert!(names(&["a.mp4", "a.mp4"]).validate(2, &audio).is_err());
        assert!(names(&["../a.mp4"]).validate(1, &audio).is_err());
        assert!(names(&[".hidden.mp4"]).validate(1, &audio).is_err());
        assert!(names(&[MANIFEST_NAME]).validate(1, &audio).is_err());
    }

    #[test]
    fn output_size_caps_the_long_edge_and_keeps_the_aspect() {
        // A 2x 1728×1117 display: 2560 px long edge, never above its 3456 px.
        assert_eq!(
            output_size((1_728.0, 1_117.0), 2.0, Some(2_560)),
            Some((2_560, 1_654))
        );
        assert_eq!(
            output_size((1_728.0, 1_117.0), 2.0, None),
            Some((3_456, 2_234))
        );
        // A 1x 1920×1080 display is not upscaled.
        assert_eq!(
            output_size((1_920.0, 1_080.0), 1.0, Some(2_560)),
            Some((1_920, 1_080))
        );
        // A 2x 480×320 region at 1280.
        assert_eq!(
            output_size((480.0, 320.0), 2.0, Some(1_280)),
            Some((960, 640))
        );
        // A tall window keeps its long side at the cap.
        assert_eq!(
            output_size((600.0, 1_600.0), 2.0, Some(1_280)),
            Some((480, 1_280))
        );
        assert_eq!(output_size((0.0, 100.0), 2.0, None), None);
        assert_eq!(output_size((f64::NAN, 100.0), 2.0, None), None);
    }

    #[test]
    fn regions_are_trimmed_to_their_display() {
        let display = Rect {
            x: 0.0,
            y: 0.0,
            width: 1_728.0,
            height: 1_117.0,
        };
        let rect = Rect {
            x: -100.0,
            y: 100.0,
            width: 300.0,
            height: 200.0,
        };
        assert_eq!(
            rect.intersection(display),
            Some(Rect {
                x: 0.0,
                y: 100.0,
                width: 200.0,
                height: 200.0
            })
        );
        let outside = Rect {
            x: 2_000.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        assert_eq!(outside.intersection(display), None);
    }

    #[test]
    fn source_json_is_tagged_by_kind() {
        let display = serde_json::to_value(Source::Display(1)).unwrap();
        assert_eq!(display, serde_json::json!({ "kind": "display", "id": 1 }));
        let region = Source::Region {
            display: 2,
            rect: Rect {
                x: 10.0,
                y: 20.0,
                width: 300.0,
                height: 200.0,
            },
        };
        let json = serde_json::to_value(region).unwrap();
        assert_eq!(json["kind"], "region");
        assert_eq!(json["display"], 2);
        assert_eq!(json["rect"]["width"], 300.0);
        assert_eq!(serde_json::from_value::<Source>(json).unwrap(), region);
    }

    #[test]
    fn audio_files_need_the_audio_they_hold() {
        let external = AudioPlan {
            mic: Mic::External(ExternalMic::default()),
            system: false,
        };
        assert!(names(&["recording.mp4"]).validate(1, &external).is_err());
        let mut with_wav = names(&["recording.mp4"]);
        with_wav.microphone = Some("audio.wav".into());
        assert!(with_wav.validate(1, &external).is_ok());

        let mut system_file = names(&["recording.mp4"]);
        system_file.system_audio = Some("system.m4a".into());
        assert!(system_file.validate(1, &AudioPlan::default()).is_err());
    }
}

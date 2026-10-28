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
    /// Displays and windows, each recorded to its own MP4. The first source owns audio.
    pub sources: Vec<Source>,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum Source {
    /// A display by its `CGDirectDisplayID`.
    Display(u32),
    /// A window by its `CGWindowID`.
    Window(u32),
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

/// What display sources leave out. Window sources capture only their window, so exclusions do
/// not apply to them.
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

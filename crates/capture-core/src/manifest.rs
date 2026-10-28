//! `capture.json`: the one file the core names. It says what the bundle holds and puts every file
//! on one clock.

use std::fs;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::plan::{MANIFEST_NAME, Source};
use crate::time::Timebase;

/// The manifest format. Readers refuse a newer major version.
pub const MANIFEST_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub clock: Clock,
    pub sources: Vec<SourceEntry>,
    pub files: Vec<FileEntry>,
    /// Spans of the microphone WAV that are silence because no audio arrived.
    pub audio_gaps: Vec<AudioGap>,
    pub missing_permissions: Vec<Permission>,
}

/// Every `*_ms` in the bundle counts from `anchor_host_time`, in Mach ticks of the host clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clock {
    pub anchor_host_time: u64,
    pub timebase: Timebase,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceEntry {
    pub source: Source,
    /// The source's video file.
    pub file: String,
    /// Where the video's first frame sits on the clock. Negative when it precedes the anchor.
    pub start_ms: i64,
    pub duration_ms: u64,
    /// Encoded size in pixels.
    pub width: u32,
    pub height: u32,
    /// The captured area in global top-left points when the recording started.
    pub frame: Frame,
    /// Whether the microphone ([`Mic::Device`](crate::Mic::Device)) and system audio are tracks in
    /// this video.
    pub audio_tracks: Vec<TrackKind>,
    /// Why this source's video is missing or incomplete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Microphone,
    SystemAudio,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub kind: FileKind,
    /// Where the file's time zero sits on the clock, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum FileKind {
    /// An H.264 MP4 of `sources[source]`.
    Video { source: usize },
    /// Mono 16-bit WAV of the host's microphone, sample 0 at the anchor.
    Microphone { sample_rate: u32 },
    /// AAC of the system's audio output.
    SystemAudio,
    /// Cursor positions and clicks per source.
    Cursor,
    /// The input log.
    Input,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioGap {
    pub start_ms: u64,
    pub end_ms: u64,
}

/// A permission the recording ran without, and what it lost.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// The core's own event tap could not start: no click telemetry.
    InputMonitoring,
}

impl Manifest {
    /// Reads `capture.json` from a bundle directory.
    ///
    /// # Errors
    ///
    /// Returns an error when the file is missing, malformed, or from a newer version.
    pub fn read(dir: &Path) -> std::io::Result<Self> {
        let bytes = fs::read(dir.join(MANIFEST_NAME))?;
        let manifest: Self = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
        if manifest.version > MANIFEST_VERSION {
            return Err(std::io::Error::other(format!(
                "capture manifest version {} is newer than {MANIFEST_VERSION}",
                manifest.version
            )));
        }
        Ok(manifest)
    }

    /// Writes `capture.json` atomically.
    pub(crate) fn write(&self, dir: &Path) -> std::io::Result<()> {
        write_json(dir, MANIFEST_NAME, self)
    }
}

/// Writes `value` as pretty JSON to `dir/name` through a sibling temporary file and a rename, so
/// a reader never sees a partial file.
pub(crate) fn write_json<T: Serialize>(dir: &Path, name: &str, value: &T) -> std::io::Result<()> {
    let staging = dir.join(format!(".{name}.tmp"));
    let mut bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    bytes.push(b'\n');
    let mut file = fs::File::create(&staging)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&staging, dir.join(name)).inspect_err(|_| {
        let _ = fs::remove_file(&staging);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        Manifest {
            version: MANIFEST_VERSION,
            clock: Clock {
                anchor_host_time: 123_456_789,
                timebase: Timebase {
                    numer: 125,
                    denom: 3,
                },
            },
            sources: vec![SourceEntry {
                source: Source::Display(1),
                file: "recording.mp4".into(),
                start_ms: -12,
                duration_ms: 4_500,
                width: 3_024,
                height: 1_964,
                frame: Frame {
                    x: 0.0,
                    y: 0.0,
                    width: 1_512.0,
                    height: 982.0,
                },
                audio_tracks: vec![],
                error: None,
            }],
            files: vec![
                FileEntry {
                    name: "recording.mp4".into(),
                    kind: FileKind::Video { source: 0 },
                    start_ms: Some(-12),
                },
                FileEntry {
                    name: "audio.wav".into(),
                    kind: FileKind::Microphone {
                        sample_rate: 16_000,
                    },
                    start_ms: Some(0),
                },
            ],
            audio_gaps: vec![AudioGap {
                start_ms: 1_000,
                end_ms: 1_250,
            }],
            missing_permissions: vec![Permission::InputMonitoring],
        }
    }

    #[test]
    fn manifest_round_trips_through_its_file() {
        let dir = crate::test_dir::tempdir().unwrap();
        let manifest = manifest();
        manifest.write(dir.path()).unwrap();
        assert_eq!(Manifest::read(dir.path()).unwrap(), manifest);
        let names = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![MANIFEST_NAME.to_owned()],
            "no staging file is left"
        );
    }

    #[test]
    fn manifest_json_shape_is_stable() {
        let json = serde_json::to_value(manifest()).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["clock"]["anchor_host_time"], 123_456_789);
        assert_eq!(json["sources"][0]["source"]["kind"], "display");
        assert_eq!(json["sources"][0]["source"]["id"], 1);
        assert!(json["sources"][0].get("error").is_none());
        assert_eq!(json["files"][0]["kind"]["type"], "video");
        assert_eq!(json["files"][1]["kind"]["sample_rate"], 16_000);
        assert_eq!(json["audio_gaps"][0]["end_ms"], 1_250);
        assert_eq!(json["missing_permissions"][0], "input_monitoring");
    }

    #[test]
    fn newer_manifests_are_refused() {
        let dir = crate::test_dir::tempdir().unwrap();
        let mut manifest = manifest();
        manifest.version = MANIFEST_VERSION + 1;
        manifest.write(dir.path()).unwrap();
        assert!(Manifest::read(dir.path()).is_err());
    }
}

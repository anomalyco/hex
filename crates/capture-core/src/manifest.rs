//! `capture.json`: the one file the core names. It says what the bundle holds and puts every file
//! on one clock.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::input::RecordedInput;
use crate::plan::{MANIFEST_NAME, Rect, Source};
use crate::session::SourceCursor;

/// The manifest format. Readers refuse a newer major version.
pub const MANIFEST_VERSION: u32 = 1;
/// The cursor and input sidecar format. Readers refuse a newer major version.
pub const SIDECAR_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    /// The clock every `*_ns` in the bundle is on.
    pub clock: Clock,
    /// The moment the recording started, just before its first source. The microphone WAV's
    /// sample 0 sits here.
    pub anchor_ns: u64,
    /// Wall-clock UTC timestamp at `anchor_ns` in RFC 3339 (`YYYY-MM-DDTHH:MM:SS.mmmZ`), for
    /// ordering takes across reboots. Absent in older v1 manifests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    pub sources: Vec<SourceEntry>,
    /// The files the recording left on disk. A source whose video failed is still in `sources`,
    /// with its `error`, but its video is not listed here.
    pub files: Vec<FileEntry>,
    /// Named moments recorded on `clock` while the take ran.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
    /// Spans of the microphone WAV that are silence because no audio arrived.
    pub audio_gaps: Vec<AudioGap>,
    pub missing_permissions: Vec<Permission>,
}

/// The clock bundle timestamps are on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Clock {
    /// The Mach host clock (`mach_absolute_time`) in nanoseconds: the clock of Core Audio host
    /// times, `CGEvent` timestamps and `ScreenCaptureKit` frame display times.
    #[default]
    MachHostNs,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceEntry {
    pub source: Source,
    /// Human-readable label captured when the source opened, such as an app and window title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The source's video file.
    pub file: String,
    /// When the video's first frame was shown.
    pub start_ns: u64,
    pub duration_ms: u64,
    /// Encoded size in pixels.
    pub width: u32,
    pub height: u32,
    /// Backing pixels per point when the source opened (`2.0` on a Retina display), even if
    /// [`SourcePlan::long_edge`](crate::SourcePlan::long_edge) scaled the encoded video down.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// The captured area in global top-left points when the recording started.
    pub frame: Rect,
    /// Which of the microphone ([`Mic::Device`](crate::Mic::Device)) and system audio this video
    /// contains. `ScreenCaptureKit` mixes both into the video's single AAC track.
    pub audio_tracks: Vec<TrackKind>,
    /// Why this source's video is missing or incomplete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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
    /// Where the file's time zero sits on the clock, for media files. Logs carry their own
    /// `host_ns` per entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_ns: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    /// On-screen window frames and app switches on `clock`.
    Windows,
    /// Word-timestamp transcript JSON sidecar by reference, optionally naming the audio file in
    /// `files` that its source word times index into.
    Transcript {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        audio_file: Option<String>,
    },
}

/// A named moment on the bundle's `clock`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub host_ns: u64,
    pub label: String,
}

/// The cursor sidecar written to [`Names::cursor`](crate::Names::cursor).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CursorBundle {
    pub version: u32,
    pub sources: Vec<SourceCursor>,
}

impl CursorBundle {
    /// Reads a cursor sidecar JSON file, refusing a newer major version.
    ///
    /// # Errors
    ///
    /// Returns an error when the file is missing, malformed, or from a newer version.
    pub fn read_file(path: &Path) -> std::io::Result<Self> {
        let bytes = fs::read(path)?;
        let bundle: Self = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
        if bundle.version > SIDECAR_VERSION {
            return Err(std::io::Error::other(format!(
                "cursor sidecar version {} is newer than {SIDECAR_VERSION}",
                bundle.version
            )));
        }
        Ok(bundle)
    }
}

/// The input sidecar written to [`Names::input`](crate::Names::input).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InputBundle {
    pub version: u32,
    pub clock: Clock,
    pub anchor_ns: u64,
    pub dropped: u64,
    pub events: Vec<RecordedInput>,
}

impl InputBundle {
    /// Reads an input sidecar JSON file, refusing a newer major version.
    ///
    /// # Errors
    ///
    /// Returns an error when the file is missing, malformed, or from a newer version.
    pub fn read_file(path: &Path) -> std::io::Result<Self> {
        let bytes = fs::read(path)?;
        let bundle: Self = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
        if bundle.version > SIDECAR_VERSION {
            return Err(std::io::Error::other(format!(
                "input sidecar version {} is newer than {SIDECAR_VERSION}",
                bundle.version
            )));
        }
        Ok(bundle)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioGap {
    pub start_ns: u64,
    pub end_ns: u64,
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
        Self::read_file(&dir.join(MANIFEST_NAME))
    }

    /// Reads a manifest saved under any name (see [`Names::manifest`](crate::Names::manifest)).
    ///
    /// # Errors
    ///
    /// Returns an error when the file is missing, malformed, or from a newer version.
    pub fn read_file(path: &Path) -> std::io::Result<Self> {
        let bytes = fs::read(path)?;
        let manifest: Self = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
        if manifest.version > MANIFEST_VERSION {
            return Err(std::io::Error::other(format!(
                "capture manifest version {} is newer than {MANIFEST_VERSION}",
                manifest.version
            )));
        }
        Ok(manifest)
    }

    /// Writes the manifest to `dir/name` atomically.
    pub(crate) fn write(&self, dir: &Path, name: &str) -> std::io::Result<()> {
        write_json(dir, name, self)
    }
}

/// Formats `time` as UTC RFC 3339 with millisecond precision (`YYYY-MM-DDTHH:MM:SS.mmmZ`).
pub(crate) fn format_utc_rfc3339_millis(time: SystemTime) -> Option<String> {
    let duration = time.duration_since(UNIX_EPOCH).ok()?;
    let total_secs = duration.as_secs();
    let millis = duration.subsec_millis();
    let days = i64::try_from(total_secs / 86_400).ok()?;
    let day_secs = total_secs % 86_400;
    let hour = day_secs / 3_600;
    let minute = (day_secs % 3_600) / 60;
    let second = day_secs % 60;

    // Howard Hinnant's civil-from-days shift (1970-01-01 -> 0000-03-01).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097).cast_unsigned();
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = i64::try_from(yoe).ok()? + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    if month <= 2 {
        year += 1;
    }
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z"
    ))
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
            clock: Clock::MachHostNs,
            anchor_ns: 5_000_000_000,
            started_at: Some("2026-10-30T18:05:12.345Z".into()),
            sources: vec![SourceEntry {
                source: Source::Display(1),
                label: None,
                file: "recording.mp4".into(),
                start_ns: 4_988_000_000,
                duration_ms: 4_500,
                width: 3_024,
                height: 1_964,
                scale: Some(2.0),
                frame: Rect {
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
                    start_ns: Some(4_988_000_000),
                },
                FileEntry {
                    name: "audio.wav".into(),
                    kind: FileKind::Microphone {
                        sample_rate: 16_000,
                    },
                    start_ns: Some(5_000_000_000),
                },
                FileEntry {
                    name: "windows.json".into(),
                    kind: FileKind::Windows,
                    start_ns: None,
                },
                FileEntry {
                    name: "transcript.json".into(),
                    kind: FileKind::Transcript {
                        audio_file: Some("audio.wav".into()),
                    },
                    start_ns: None,
                },
            ],
            markers: vec![Marker {
                host_ns: 5_500_000_000,
                label: "cut here".into(),
            }],
            audio_gaps: vec![AudioGap {
                start_ns: 6_000_000_000,
                end_ns: 6_250_000_000,
            }],
            missing_permissions: vec![Permission::InputMonitoring],
        }
    }

    #[test]
    fn manifest_round_trips_through_its_file() {
        let dir = crate::test_dir::tempdir().unwrap();
        let manifest = manifest();
        manifest.write(dir.path(), MANIFEST_NAME).unwrap();
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
        assert_eq!(json["clock"], "mach_host_ns");
        assert_eq!(json["anchor_ns"], 5_000_000_000_u64);
        assert_eq!(json["started_at"], "2026-10-30T18:05:12.345Z");
        assert_eq!(json["sources"][0]["start_ns"], 4_988_000_000_u64);
        assert_eq!(json["sources"][0]["source"]["kind"], "display");
        assert_eq!(json["sources"][0]["source"]["id"], 1);
        assert_eq!(json["sources"][0]["scale"], 2.0);
        assert!(json["sources"][0].get("label").is_none());
        assert!(json["sources"][0].get("error").is_none());
        assert_eq!(json["files"][0]["kind"]["type"], "video");
        assert_eq!(json["files"][1]["kind"]["sample_rate"], 16_000);
        assert_eq!(json["files"][2]["kind"]["type"], "windows");
        assert_eq!(json["files"][3]["kind"]["type"], "transcript");
        assert_eq!(json["files"][3]["kind"]["audio_file"], "audio.wav");
        assert_eq!(json["markers"][0]["host_ns"], 5_500_000_000_u64);
        assert_eq!(json["markers"][0]["label"], "cut here");
        assert_eq!(json["audio_gaps"][0]["end_ns"], 6_250_000_000_u64);
        assert_eq!(json["missing_permissions"][0], "input_monitoring");
    }

    #[test]
    fn initial_v1_manifests_without_optional_fields_still_read() {
        let legacy = r#"{
            "version": 1,
            "clock": "mach_host_ns",
            "anchor_ns": 81234000000000,
            "sources": [{
                "source": { "kind": "display", "id": 1 },
                "file": "recording.mp4",
                "start_ns": 81234041000000,
                "duration_ms": 5000,
                "width": 3024,
                "height": 1964,
                "frame": { "x": 0.0, "y": 0.0, "width": 1512.0, "height": 982.0 },
                "audio_tracks": []
            }],
            "files": [
                { "name": "recording.mp4", "kind": { "type": "video", "source": 0 }, "start_ns": 81234041000000 }
            ],
            "audio_gaps": [],
            "missing_permissions": []
        }"#;
        let dir = crate::test_dir::tempdir().unwrap();
        fs::write(dir.path().join(MANIFEST_NAME), legacy).unwrap();
        let parsed = Manifest::read(dir.path()).unwrap();
        assert_eq!(parsed.started_at, None);
        assert!(parsed.markers.is_empty());
        assert_eq!(parsed.sources[0].label, None);
        assert_eq!(parsed.sources[0].scale, None);

        let reserialized = serde_json::to_value(&parsed).unwrap();
        assert!(reserialized.get("started_at").is_none());
        assert!(reserialized.get("markers").is_none());
        assert!(reserialized["sources"][0].get("label").is_none());
        assert!(reserialized["sources"][0].get("scale").is_none());
    }

    #[test]
    fn utc_rfc3339_formatting_handles_epoch_and_leap_days() {
        assert_eq!(
            format_utc_rfc3339_millis(UNIX_EPOCH).as_deref(),
            Some("1970-01-01T00:00:00.000Z")
        );
        let leap_2000 = UNIX_EPOCH + std::time::Duration::from_millis(951_827_696_789);
        assert_eq!(
            format_utc_rfc3339_millis(leap_2000).as_deref(),
            Some("2000-02-29T12:34:56.789Z")
        );
        let leap_2024 = UNIX_EPOCH + std::time::Duration::from_millis(1_709_251_199_987);
        assert_eq!(
            format_utc_rfc3339_millis(leap_2024).as_deref(),
            Some("2024-02-29T23:59:59.987Z")
        );
    }

    #[test]
    fn newer_manifests_are_refused() {
        let dir = crate::test_dir::tempdir().unwrap();
        let mut manifest = manifest();
        manifest.version = MANIFEST_VERSION + 1;
        manifest.write(dir.path(), MANIFEST_NAME).unwrap();
        assert!(Manifest::read(dir.path()).is_err());
    }
}

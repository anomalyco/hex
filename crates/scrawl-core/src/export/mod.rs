//! `scrawl export`: a bundle's recording as one shareable `.mp4` (H.264 or HEVC, AAC audio).
//!
//! Rust decides everything about the picture and the helper (`helper/Export.swift`) renders it:
//!
//! - `crop`: the part of the recording kept (`--crop`, default the activity region);
//! - `camera`: with `--zoom`, a camera rectangle per frame from the bundle's events, so the
//!   helper only interpolates and draws;
//! - `ink`: the bundle's strokes, pins, laser and click ripples as a time-stamped vector plan,
//!   drawn in the recording's own clock;
//! - audio: `audio.wav` (or an iPhone movie's own track) trimmed to start where the video does.
//!
//! The helper reads the video with AVAssetReader, composites with Core Image, draws the
//! annotations with Core Graphics into the same pixel buffers, and hardware-encodes with
//! AVAssetWriter. Nothing is decoded into Rust.

pub mod camera;
pub mod crop;
pub mod ink;

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::autocrop::Rect;
use crate::model::{InteractionEvent, ScrawlBundle};

use self::camera::{CameraKey, Focus};
use self::crop::CropArg;

#[derive(Clone, Debug)]
pub struct Options {
    pub out: Option<PathBuf>,
    pub crop: CropArg,
    /// Long edge cap in pixels.
    pub size: u32,
    pub zoom: bool,
    pub ink: bool,
    pub clicks: bool,
    pub hevc: bool,
    pub fps: Option<u32>,
    /// Run the encode at utility priority (the app, so a recording in progress isn't hurt).
    pub background: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            out: None,
            crop: CropArg::Auto,
            size: crop::DEFAULT_LONG_EDGE,
            zoom: false,
            ink: true,
            clicks: true,
            hevc: false,
            fps: None,
            background: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Exported {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_ms: u64,
    pub crop: Rect,
    pub audio: bool,
    pub camera_moves: bool,
    pub bytes: u64,
}

/// Where `<home_dir>/Movies/Scrawl/<bundle>.mp4` goes for a bundle.
pub fn default_output_in(home_dir: &Path, bundle_dir: &Path) -> PathBuf {
    let name = bundle_dir
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("scrawl");
    home_dir.join("Movies/Scrawl").join(format!("{name}.mp4"))
}

/// Where `~/Movies/Scrawl/<bundle>.mp4` goes for a bundle.
pub fn default_output(bundle_dir: &Path) -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    default_output_in(&home, bundle_dir)
}

/// Serializable render plan passed to the platform video encoder (`Export.swift`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPlan {
    pub video: PathBuf,
    pub out: PathBuf,
    pub audio: Option<PathBuf>,
    pub audio_start_ms: u64,
    pub audio_length_ms: u64,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub codec: String,
    pub duration_ms: u64,
    pub crop: Rect,
    pub camera: Option<Vec<[f64; 5]>>,
    pub ink: ink::InkPlan,
}

/// Camera focuses for a bundle, on the video's clock: clicks, drags, drawn marks and laser
/// points. Activity before the video began is dropped.
pub fn focuses(bundle: &ScrawlBundle) -> Vec<Focus> {
    let offset = bundle.video_offset_ms;
    let mut out = Vec::new();
    let mut push = |t0: u64, t1: u64, rect: Rect| {
        if t1 >= offset {
            out.push(Focus {
                t0: t0.saturating_sub(offset),
                t1: t1.saturating_sub(offset),
                rect,
            });
        }
    };
    let bbox = |points: &mut dyn Iterator<Item = [f32; 2]>| -> Option<Rect> {
        points.fold(None, |acc: Option<Rect>, p| {
            Some(acc.map_or([p[0], p[1], p[0], p[1]], |r| {
                [
                    r[0].min(p[0]),
                    r[1].min(p[1]),
                    r[2].max(p[0]),
                    r[3].max(p[1]),
                ]
            }))
        })
    };
    for event in &bundle.events {
        match event {
            InteractionEvent::Click { t, x, y, .. } | InteractionEvent::Point { t, x, y, .. } => {
                push(*t, *t, [*x, *y, *x, *y]);
            }
            InteractionEvent::Drag {
                t,
                t1,
                x0,
                y0,
                x1,
                y1,
                path,
                ..
            } => {
                let mut points = [[*x0, *y0], [*x1, *y1]]
                    .into_iter()
                    .chain(path.iter().copied());
                if let Some(rect) = bbox(&mut points) {
                    push(*t, *t1, rect);
                }
            }
            _ => {}
        }
    }
    for stroke in &bundle.strokes {
        let mut points = stroke.normalized_points().into_iter();
        if let Some(rect) = bbox(&mut points) {
            push(stroke.t0, stroke.t1, rect);
        }
    }
    out
}

/// The camera path for `bundle` inside `crop`, as `[t, x0, y0, x1, y1]` rows.
pub fn camera_rows(
    bundle: &ScrawlBundle,
    crop: Rect,
    video_px: (u32, u32),
    duration_ms: u64,
) -> Vec<CameraKey> {
    let offset = bundle.video_offset_ms;
    let focuses = focuses(bundle);
    let cursor: Vec<(u64, f32, f32)> = bundle
        .cursor
        .iter()
        .filter(|c| c.0 >= offset)
        .map(|c| (c.0 - offset, c.1, c.2))
        .collect();
    camera::plan(&camera::Input {
        duration_ms,
        crop,
        aspect: video_px.0 as f32 / video_px.1.max(1) as f32,
        focuses: &focuses,
        cursor: &cursor,
    })
}

/// What `scrawl export` prints.
pub fn describe(exported: &Exported) -> String {
    format!(
        "{} · {}×{} @ {} fps · {} · {:.1} MB{}{}",
        exported.path.display(),
        exported.width,
        exported.height,
        exported.fps,
        crate::model::format_clock(exported.duration_ms),
        exported.bytes as f64 / 1_048_576.0,
        if exported.audio {
            " · audio"
        } else {
            " · no audio"
        },
        if exported.camera_moves {
            " · auto-zoom"
        } else {
            ""
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle() -> ScrawlBundle {
        serde_json::from_value(serde_json::json!({
            "version": 1, "id": "t", "createdAtMs": 0, "durationMs": 20_000,
            "target": {"mode": "display", "displayId": 0,
                "frame": {"x": 0.0, "y": 0.0, "width": 2560.0, "height": 1440.0, "scale": 2.0}},
            "transcript": {"source": "test", "text": "", "segments": []},
            "strokes": [{"t0": 4000, "t1": 4300, "fw": 2560, "fh": 1440,
                "pts": [2000, 3000, 0, 2600, 3400, 300]}],
            "marks": [], "keyframes": [], "contactSheet": "c.png", "videoOffsetMs": 100,
            "events": [
                {"kind": "click", "t": 50, "x": 0.1, "y": 0.1},
                {"kind": "click", "t": 2100, "x": 0.5, "y": 0.5},
                {"kind": "drag", "t": 8000, "t1": 8500, "x0": 0.7, "y0": 0.6, "x1": 0.75, "y1": 0.65},
                {"kind": "scroll", "t": 9000, "x": 0.1, "y": 0.1, "dx": 0.0, "dy": 3.0}
            ],
            "cursor": [[50, 0.1, 0.1], [2100, 0.5, 0.5]]
        }))
        .unwrap()
    }

    #[test]
    fn camera_focuses_are_on_the_video_clock_and_skip_what_came_before_it() {
        let f = focuses(&bundle());
        assert_eq!(
            f.len(),
            3,
            "click, drag, stroke; the early click and scroll don't count"
        );
        assert_eq!((f[0].t0, f[0].rect), (2_000, [0.5, 0.5, 0.5, 0.5]));
        assert_eq!((f[1].t0, f[1].t1), (7_900, 8_400));
        assert_eq!(f[1].rect, [0.7, 0.6, 0.75, 0.65]);
        assert_eq!(f[2].t0, 3_900);
        assert!((f[2].rect[0] - 0.2).abs() < 1e-6 && (f[2].rect[3] - 0.34).abs() < 1e-6);
    }

    #[test]
    fn the_camera_path_for_a_bundle_zooms_toward_its_activity() {
        let b = bundle();
        let keys = camera_rows(&b, [0.0, 0.0, 1.0, 1.0], (2560, 1440), 15_000);
        assert!(keys.iter().any(|k| k.rect[2] - k.rect[0] < 0.6), "zooms in");
        assert_eq!(keys.last().unwrap().rect, [0.0, 0.0, 1.0, 1.0], "ends wide");
    }

    #[test]
    fn default_output_lands_in_movies_scrawl() {
        let path = default_output(Path::new("/x/scrawl-17-9.scrawl"));
        assert!(
            path.ends_with("Movies/Scrawl/scrawl-17-9.mp4"),
            "{}",
            path.display()
        );
    }
}

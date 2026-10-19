//! `scrawl zoom`: a dense contact sheet around one moment of a bundle, for looking closely at an
//! animation or glitch the narration points at.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use crate::model::{
    InteractionEvent, Keyframe, KeyframeReason, RgbaFrame, ScrawlBundle, parse_clock_ms,
};

/// Box-filtered copy at most `max_w` wide.
pub fn downscale(frame: &RgbaFrame, max_w: u32) -> RgbaFrame {
    let f = frame.width.div_ceil(max_w.max(1)).max(1);
    if f == 1 {
        return frame.clone();
    }
    let (w, h) = (frame.width / f, frame.height / f);
    let mut rgba = vec![0_u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0_u32; 4];
            for dy in 0..f {
                let row = ((y * f + dy) * frame.width + x * f) as usize * 4;
                for px in frame.rgba[row..row + f as usize * 4].as_chunks::<4>().0 {
                    for (a, v) in acc.iter_mut().zip(px) {
                        *a += u32::from(*v);
                    }
                }
            }
            let i = ((y * w + x) * 4) as usize;
            for c in 0..4 {
                rgba[i + c] = (acc[c] / (f * f)) as u8;
            }
        }
    }
    RgbaFrame::new(frame.t_ms, w, h, rgba)
}

pub struct ZoomSheet {
    pub path: PathBuf,
    /// `(index, time ms)` of each cell, in order.
    pub cells: Vec<(usize, u64)>,
    pub from_ms: u64,
    pub to_ms: u64,
    /// The narration overlapping the window.
    pub said: String,
}

fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

/// Where `phrase` starts in the narration: the first run of transcript words matching it, else
/// the segment containing it.
pub fn find_phrase_ms(bundle: &ScrawlBundle, phrase: &str) -> Option<u64> {
    let needle: Vec<String> = normalize(phrase)
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    if needle.is_empty() {
        return None;
    }
    let words = &bundle.transcript.words;
    let tokens: Vec<String> = words
        .iter()
        .map(|w| normalize(&w.text).replace(' ', ""))
        .collect();
    let matched = (0..tokens.len()).find(|&i| {
        needle
            .iter()
            .enumerate()
            .all(|(j, n)| tokens.get(i + j).is_some_and(|t| t == n))
    });
    if let Some(i) = matched {
        return Some(words[i].t0);
    }
    let joined = needle.join(" ");
    bundle
        .transcript
        .segments
        .iter()
        .find(|s| {
            normalize(&s.text)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .contains(&joined)
        })
        .map(|s| s.t0)
}

/// Resolves `--at`: a time (`0:12`, `12.5`) or a phrase from the narration.
pub fn resolve_moment(bundle: &ScrawlBundle, at: &str) -> Result<u64> {
    parse_clock_ms(at)
        .or_else(|| find_phrase_ms(bundle, at))
        .with_context(|| format!("“{at}” is neither a time nor a phrase in the narration"))
}

/// The window around `at_ms`: a quarter before, the rest after, since what the narration points
/// at usually happens as or just after it is said. Clamped to the recording.
pub fn window(at_ms: u64, span_ms: u64, duration_ms: u64) -> (u64, u64) {
    let span = span_ms.clamp(200, duration_ms.max(200));
    let mut from = at_ms.saturating_sub(span / 4);
    let mut to = from + span;
    if to > duration_ms {
        to = duration_ms;
        from = to.saturating_sub(span);
    }
    (from, to)
}

/// Evenly spaced times across `[from, to]`, both ends included.
pub fn sample_times(from: u64, to: u64, count: usize) -> Vec<u64> {
    let count = count.clamp(1, 48);
    if count == 1 || to <= from {
        return vec![from];
    }
    (0..count)
        .map(|i| from + (to - from) * i as u64 / (count - 1) as u64)
        .collect()
}

/// How much of a gesture still under way a cell shows: the finger now and its recent path.
const TRAIL_MS: u64 = 350;

/// The interactions as they stood at `t`. A drag still under way is cut to its last
/// `TRAIL_MS`, so the cell shows where the finger is rather than the whole gesture to come
/// (its samples are evenly spaced in time, so a point's index gives its time).
fn events_at(events: &[InteractionEvent], t: u64) -> Vec<InteractionEvent> {
    events
        .iter()
        .map(|event| match event {
            InteractionEvent::Drag {
                t: t0,
                t1,
                path,
                label,
                button,
                ..
            } if *t0 <= t && t < *t1 && path.len() >= 2 => {
                let n = path.len() as u64;
                let at = |i: usize| t0 + (t1 - t0) * i as u64 / (n - 1);
                let done: Vec<usize> = (0..path.len()).filter(|&i| at(i) <= t).collect();
                let last = *done.last().unwrap_or(&0);
                let mut kept: Vec<[f32; 2]> = done
                    .into_iter()
                    .filter(|&i| at(i) + TRAIL_MS >= t)
                    .map(|i| path[i])
                    .collect();
                if kept.is_empty() {
                    kept.push(path[last]);
                }
                let [x0, y0] = kept[0];
                let [x1, y1] = kept[kept.len() - 1];
                InteractionEvent::Drag {
                    t: t.saturating_sub(TRAIL_MS).max(*t0),
                    t1: t,
                    x0,
                    y0,
                    x1,
                    y1,
                    label: label.clone(),
                    button: button.clone(),
                    path: kept,
                }
            }
            other => other.clone(),
        })
        .collect()
}

pub fn clock(ms: u64) -> String {
    format!("{}:{:02}.{:03}", ms / 60_000, ms / 1_000 % 60, ms % 1_000)
}

fn said_between(bundle: &ScrawlBundle, from: u64, to: u64) -> String {
    let words: Vec<&str> = bundle
        .transcript
        .words
        .iter()
        .filter(|w| w.t1 >= from && w.t0 <= to)
        .map(|w| w.text.as_str())
        .collect();
    if !words.is_empty() {
        return words.join(" ");
    }
    bundle
        .transcript
        .segments
        .iter()
        .filter(|s| s.t1 >= from && s.t0 <= to)
        .map(|s| s.text.trim())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The auto-crop of the keyframe nearest `t` (keyframe crops meet halfway between keyframes).
fn crop_at(bundle: &ScrawlBundle, t: u64) -> Option<crate::autocrop::Rect> {
    let info = bundle.autocrop.as_ref()?;
    let (k, _) = bundle
        .keyframes
        .iter()
        .enumerate()
        .min_by_key(|(_, kf)| kf.t.abs_diff(t))?;
    let crop = info.crops.get(k)?;
    (crop.kind != crate::autocrop::CropKind::Full).then(|| crop.rect(&bundle.target.frame))
}

/// `cropped`: in an auto-cropped bundle, crop each cell like the nearest keyframe.
/// `decode_frames(video_path, times_ms, video_offset_ms, max_edge)` supplies the decoded video frames.
pub fn zoom(
    dir: &Path,
    bundle: &ScrawlBundle,
    at: &str,
    span_ms: u64,
    count: usize,
    cropped: bool,
    mut decode_frames: impl FnMut(&Path, &[u64], u64, u32) -> Result<Vec<RgbaFrame>>,
) -> Result<ZoomSheet> {
    let video = bundle
        .video
        .as_ref()
        .map(|v| dir.join(v))
        .filter(|p| p.is_file())
        .with_context(|| "The bundle has no video to zoom into".to_owned())?;
    let at_ms = resolve_moment(bundle, at)?;
    ensure!(
        at_ms <= bundle.duration_ms,
        "{} is past the end ({})",
        clock(at_ms),
        clock(bundle.duration_ms)
    );
    let (from_ms, to_ms) = window(at_ms, span_ms, bundle.duration_ms);
    let times = sample_times(from_ms, to_ms, count);
    let crops: Vec<Option<crate::autocrop::Rect>> = times
        .iter()
        .map(|&t| crop_at(bundle, t).filter(|_| cropped))
        .collect();
    let mut frames = if crops.iter().any(Option::is_some) {
        // Decoded at full size a few at a time and cropped straight away, so a sheet of 2560 px
        // screens doesn't sit in memory.
        let mut out = Vec::with_capacity(times.len());
        for (chunk, rects) in times.chunks(6).zip(crops.chunks(6)) {
            let decoded = decode_frames(&video, chunk, bundle.video_offset_ms, 0)?;
            for (frame, rect) in decoded.into_iter().zip(rects) {
                let frame = match rect {
                    Some(r) => crate::autocrop::crop_frame(&frame, *r),
                    None => frame,
                };
                out.push(downscale(&frame, 1280));
            }
        }
        out
    } else {
        decode_frames(&video, &times, bundle.video_offset_ms, 900)?
    };
    ensure!(!frames.is_empty(), "No frames could be decoded");
    let mut cells = Vec::with_capacity(frames.len());
    for (i, frame) in frames.iter_mut().enumerate() {
        let t = frame.t_ms;
        let events = events_at(&bundle.events, t);
        let rect = crops
            .get(i)
            .copied()
            .flatten()
            .unwrap_or(crate::autocrop::FULL);
        // Labels are sized for the sheet's cell, not the decoded frame.
        let cell_width = if frame.height > frame.width {
            300.0
        } else {
            480.0
        };
        let label_scale = frame.width as f32 / cell_width;
        let keyframe = Keyframe {
            index: i + 1,
            t,
            reason: KeyframeReason::Speech,
            file: String::new(),
            event: String::new(),
            said: None,
            mark_id: None,
            cursor: None,
        };
        let view =
            crate::autocrop::crop_view(rect, &keyframe, &bundle.strokes, &bundle.marks, &events);
        crate::render::annotate_keyframe_in_place(
            frame,
            &view.keyframe,
            &view.strokes,
            &view.marks,
            &view.events,
        );
        crate::render::draw_time_label(frame, &clock(t), label_scale);
        cells.push((i + 1, t));
    }
    let sheet = crate::render::render_zoom_sheet(&frames);
    let out_dir = dir.join("zoom");
    std::fs::create_dir_all(&out_dir)?;
    let path = out_dir.join(format!("zoom-{from_ms}-{to_ms}.png"));
    crate::render::save_png(&sheet, &path)?;
    Ok(ZoomSheet {
        path,
        cells,
        from_ms,
        to_ms,
        said: said_between(bundle, from_ms, to_ms),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{TranscriptData, WordTiming};

    fn bundle_with_words(words: &[(&str, u64)]) -> ScrawlBundle {
        let mut bundle: ScrawlBundle = serde_json::from_value(serde_json::json!({
            "version": 1, "id": "t", "createdAtMs": 0, "durationMs": 25_000,
            "target": {"mode": "device", "displayId": 0,
                "frame": {"x": 0.0, "y": 0.0, "width": 10.0, "height": 10.0, "scale": 1.0}},
            "transcript": {"source": "test", "text": "", "segments": []},
            "strokes": [], "marks": [], "events": [], "keyframes": [], "contactSheet": "c.png"
        }))
        .unwrap();
        bundle.transcript = TranscriptData {
            source: "test".into(),
            words: words
                .iter()
                .map(|&(text, t0)| WordTiming {
                    t0,
                    t1: t0 + 200,
                    text: text.into(),
                })
                .collect(),
            ..Default::default()
        };
        bundle
    }

    #[test]
    fn phrases_and_times_resolve() {
        let bundle = bundle_with_words(&[
            ("but,", 9_000),
            ("the", 9_300),
            ("Width", 9_500),
            ("changes.", 9_800),
        ]);
        assert_eq!(resolve_moment(&bundle, "width changes").unwrap(), 9_500);
        assert_eq!(resolve_moment(&bundle, "0:12").unwrap(), 12_000);
        assert!(resolve_moment(&bundle, "tabular numbers").is_err());
    }

    #[test]
    fn window_leans_after_the_moment_and_stays_inside() {
        assert_eq!(window(10_000, 2_000, 25_000), (9_500, 11_500));
        assert_eq!(window(200, 2_000, 25_000), (0, 2_000));
        assert_eq!(window(24_800, 2_000, 25_000), (23_000, 25_000));
        assert_eq!(sample_times(1_000, 2_000, 3), vec![1_000, 1_500, 2_000]);
        assert_eq!(clock(61_234), "1:01.234");
    }
}

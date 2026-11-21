//! A Screenshots recording in the editor: a filmstrip under the picture
//! with a handle at each end of the span kept, the frame at the handle
//! being dragged shown above, and the words said inside the span under it.
//! The kept span saves as a movie, or goes to a session as a contact sheet
//! with its words and the movie's path. Nothing changes the recording.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::PathBuf;

use super::Shot;
use crate::app_settings::{ExportSize, RecordingQuality};

/// Frames along the filmstrip, read when the recording opens.
pub(super) const FRAMES: usize = 12;
/// Their long edge, in pixels.
pub(super) const FRAME_PX: u32 = 960;
/// The filmstrip's height.
pub(super) const STRIP_H: f32 = 44.0;
/// Play and the playhead's time, left of the filmstrip.
pub(super) const TRANSPORT_W: f32 = 108.0;
/// The shortest span a trim keeps.
const MIN_SPAN_MS: u64 = 500;
/// A playhead this close to the span's end starts over from its start.
const END_SLACK_MS: u64 = 100;
/// The most frames on a sent contact sheet.
const SHEET_FRAMES: usize = 4;
/// How much more a 60 fps encode takes than a 30 fps one: 1.27× on the
/// recording [`RecordingQuality`] was measured on.
const FPS_60_COST: f64 = 1.27;

/// A word said while recording, on the movie's clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Word {
    pub(crate) t0: u64,
    pub(crate) t1: u64,
    pub(crate) text: String,
}

/// An end of the span kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Handle {
    Start,
    End,
}

/// What a drag on the filmstrip moves: a handle, or the playhead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Grip {
    Handle(Handle),
    Playhead,
}

/// A recording open in the editor.
pub(crate) struct Clip {
    pub(crate) movie: PathBuf,
    pub(crate) duration_ms: u64,
    /// The filmstrip's frames and their times, in order.
    pub(crate) frames: Vec<(u64, Shot)>,
    pub(crate) words: Vec<Word>,
    /// The span kept, in ms.
    pub(crate) trim: [u64; 2],
    /// What a drag on the filmstrip holds.
    pub(crate) holding: Option<Grip>,
    /// The frame shown above the filmstrip.
    pub(crate) shown: usize,
    /// The playhead, in ms on the movie's clock.
    pub(crate) at_ms: u64,
    pub(crate) playing: bool,
    /// The animation seconds the playhead last moved while playing.
    ticked: Option<f64>,
    /// The movie itself, which plays over the frame once Play is pressed
    /// (`live`). Headless runs have none and step the filmstrip instead.
    pub(crate) player: Option<rune::Player>,
    pub(crate) live: bool,
    /// The movie's size on disk, and its picture's in pixels (zero when
    /// unknown), for the export's estimate.
    pub(crate) bytes: u64,
    pub(crate) pixels: [u32; 2],
}

impl Clip {
    pub(crate) fn new(
        movie: PathBuf,
        duration_ms: u64,
        frames: Vec<(u64, Shot)>,
        words: Vec<Word>,
    ) -> Self {
        Self {
            movie,
            duration_ms,
            frames,
            words,
            trim: [0, duration_ms],
            holding: None,
            shown: 0,
            at_ms: 0,
            playing: false,
            ticked: None,
            player: None,
            live: false,
            bytes: 0,
            pixels: [0, 0],
        }
    }

    /// The frame above the filmstrip.
    pub(crate) fn picture(&self) -> Option<&Shot> {
        self.frames.get(self.shown).map(|(_, shot)| shot)
    }

    /// About how many bytes the span kept takes exported at `size`,
    /// re-encoded at `quality` and `fps` when it's shrunk. As recorded,
    /// it's the movie's share; shrunk, that share scaled by the pixels
    /// left (an encode's size follows its pixels), and never more than the
    /// most `quality` was measured to take, since the recording may have
    /// been made finer.
    pub(crate) fn estimate(&self, size: ExportSize, quality: RecordingQuality, fps: u32) -> u64 {
        let [from, to] = self.trim;
        let kept = self.bytes as f64 * (to - from) as f64 / self.duration_ms.max(1) as f64;
        let ([w, h], Some(edge)) = (self.pixels, size.long_edge()) else {
            return kept.round() as u64;
        };
        if w == 0 || h == 0 {
            return kept.round() as u64;
        }
        let scale = (edge as f64 / w.max(h) as f64).min(1.0);
        let pixels = w as f64 * h as f64 * scale * scale;
        let rate = if fps > 45 { FPS_60_COST } else { 1.0 };
        let most = quality.bits_per_pixel_second() * pixels * rate * (to - from) as f64 / 8000.0;
        (kept * scale * scale).min(most).round() as u64
    }

    /// Whether the span kept is less than the whole recording.
    pub(crate) fn trimmed(&self) -> bool {
        self.trim != [0, self.duration_ms]
    }

    /// The words said inside the span kept, joined.
    pub(crate) fn words_in_trim(&self) -> String {
        let [from, to] = self.trim;
        self.words
            .iter()
            .filter(|word| word.t1 > from && word.t0 < to)
            .map(|word| word.text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// What a press at `ms` takes: the handle within `reach` ms of it,
    /// the nearer if both are, else the playhead.
    pub(crate) fn grip(&self, ms: u64, reach: u64) -> Grip {
        let [from, to] = self.trim;
        let (start, end) = (ms.abs_diff(from), ms.abs_diff(to));
        if start.min(end) > reach {
            Grip::Playhead
        } else if start <= end {
            Grip::Handle(Handle::Start)
        } else {
            Grip::Handle(Handle::End)
        }
    }

    /// Plays from the playhead, or from the span's start once it's at the
    /// end; or pauses.
    pub(crate) fn toggle_play(&mut self) {
        if self.playing {
            self.pause();
            return;
        }
        let [from, to] = self.trim;
        if self.at_ms < from || self.at_ms + END_SLACK_MS >= to {
            self.at_ms = from;
        }
        self.playing = true;
        self.ticked = None;
        if let Some(player) = &self.player {
            player.seek(self.at_ms as f64 / 1000.0);
            player.play();
            self.live = true;
        }
    }

    pub(crate) fn pause(&mut self) {
        self.playing = false;
        if let Some(player) = &self.player {
            player.pause();
        }
    }

    /// Moves the playhead with the movie, or by the time passed without
    /// one, and stops it at the span's end. Whether it moved.
    pub(crate) fn tick(&mut self, now: f64) -> bool {
        if !self.playing {
            return false;
        }
        let last = self.ticked.replace(now).unwrap_or(now);
        let [from, to] = self.trim;
        let at = match self
            .player
            .as_ref()
            .map(|player| (player.state(), player.time()))
        {
            Some((rune::PlayerState::Failed(error), _)) => {
                tracing::warn!(%error, "could not play the recording");
                self.live = false;
                self.pause();
                return true;
            }
            Some((rune::PlayerState::Ended, _)) => to,
            Some((_, seconds)) => (seconds * 1000.0).round() as u64,
            None => self.at_ms + ((now - last).max(0.0) * 1000.0).round() as u64,
        }
        .clamp(from, to);
        if at == to {
            self.pause();
        }
        let moved = at != self.at_ms;
        self.at_ms = at;
        self.shown = self.frame_at(at);
        moved
    }

    /// Puts the playhead at `ms` inside the span kept, paused, with the
    /// frame there shown.
    pub(crate) fn scrub(&mut self, ms: u64) {
        self.pause();
        self.at_ms = ms.clamp(self.trim[0], self.trim[1]);
        self.shown = self.frame_at(self.at_ms);
        self.live = false;
        if let Some(player) = &self.player {
            player.seek(self.at_ms as f64 / 1000.0);
        }
    }

    /// Moves `handle` to `ms`, keeping at least [`MIN_SPAN_MS`] between
    /// the two (or the whole recording, if it's shorter), and shows the
    /// frame there.
    pub(crate) fn drag(&mut self, handle: Handle, ms: u64) {
        let span = MIN_SPAN_MS.min(self.duration_ms);
        let [from, to] = &mut self.trim;
        match handle {
            Handle::Start => *from = ms.min(to.saturating_sub(span)),
            Handle::End => *to = ms.clamp(*from + span, self.duration_ms),
        }
        let at = match handle {
            Handle::Start => self.trim[0],
            Handle::End => self.trim[1],
        };
        self.scrub(at);
    }

    /// The filmstrip frame nearest `ms`.
    pub(crate) fn frame_at(&self, ms: u64) -> usize {
        self.frames
            .iter()
            .enumerate()
            .min_by_key(|(_, (t, _))| t.abs_diff(ms))
            .map_or(0, |(i, _)| i)
    }

    /// Up to [`SHEET_FRAMES`] frames spread over the span kept, for the
    /// contact sheet.
    pub(crate) fn sheet_frames(&self) -> Vec<&Shot> {
        let [from, to] = self.trim;
        let inside: Vec<&Shot> = self
            .frames
            .iter()
            .filter(|(t, _)| (from..=to).contains(t))
            .map(|(_, shot)| shot)
            .collect();
        if inside.is_empty() {
            return self.picture().into_iter().collect();
        }
        let n = inside.len().min(SHEET_FRAMES);
        (0..n).map(|i| inside[i * inside.len() / n]).collect()
    }
}

/// `ms` as `0:07` or `12:03`, rounded up: a length.
pub(crate) fn duration_label(ms: u64) -> String {
    clock(ms.div_ceil(1000))
}

/// The playhead at `ms` as `0:03`, the seconds already played.
pub(crate) fn position_label(ms: u64) -> String {
    clock(ms / 1000)
}

fn clock(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// `bytes` as a person reads a file's size: `820 KB`, `24 MB`, `1.3 GB`.
pub(crate) fn size_label(bytes: u64) -> String {
    const MB: f64 = 1_000_000.0;
    let mb = bytes as f64 / MB;
    if mb < 1.0 {
        format!("{} KB", (bytes as f64 / 1000.0).ceil().max(1.0))
    } else if mb < 1000.0 {
        format!("{} MB", mb.round())
    } else {
        format!("{:.1} GB", mb / 1000.0)
    }
}

/// `frames`, all one size, laid out as one, two side by side, or two by
/// two, as RGBA; none without a frame.
pub(super) fn sheet(frames: &[&Shot]) -> Option<(u32, u32, Vec<u8>)> {
    let first = frames.first()?;
    let [w, h] = first.size;
    let columns = if frames.len() == 1 { 1 } else { 2 };
    let rows = frames.len().div_ceil(columns) as u32;
    let (sw, sh) = (w * columns as u32, h * rows);
    let mut out = vec![0; (sw * sh * 4) as usize];
    for (i, frame) in frames.iter().enumerate() {
        if frame.size != [w, h] || frame.rgba.len() < (w * h * 4) as usize {
            continue;
        }
        let (cx, cy) = ((i % columns) as u32 * w, (i / columns) as u32 * h);
        for y in 0..h {
            let from = (y * w * 4) as usize;
            let to = (((cy + y) * sw + cx) * 4) as usize;
            out[to..to + (w * 4) as usize]
                .copy_from_slice(&frame.rgba[from..from + (w * 4) as usize]);
        }
    }
    Some((sw, sh, out))
}

/// The movie in the Annotate capture `bundle` a recording was written to,
/// and the words said over it on the movie's clock.
#[cfg(target_os = "macos")]
pub(crate) fn recorded(bundle: &std::path::Path) -> anyhow::Result<(PathBuf, Vec<Word>)> {
    let (_, loaded) = scrawl_capture::bundle::load_bundle_in(None, bundle)?;
    let movie = bundle.join(
        loaded
            .video
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("the recording has no movie"))?,
    );
    let offset = loaded.video_offset_ms;
    let words = loaded
        .transcript
        .words
        .into_iter()
        .map(|word| Word {
            t0: word.t0.saturating_sub(offset),
            t1: word.t1.saturating_sub(offset),
            text: word.text,
        })
        .collect();
    Ok((movie, words))
}

/// Reads the recording `movie`, with the `words` said over it: its length
/// and filmstrip. Takes a second or so; off the main thread.
#[cfg(target_os = "macos")]
pub(super) fn load(movie: PathBuf, words: Vec<Word>) -> anyhow::Result<Clip> {
    use scrawl_capture::helper::{self, FrameTimes};
    let probe = helper::probe(&movie)?;
    let duration_ms = probe.duration_ms.max(1);
    let bytes = std::fs::metadata(&movie).map_or(0, |meta| meta.len());
    let last = duration_ms.saturating_sub(100);
    let times: Vec<u64> = (0..FRAMES as u64)
        .map(|i| last * i / (FRAMES as u64 - 1))
        .collect();
    let frames = helper::frames(&movie, FrameTimes::At(&times), FRAME_PX)?
        .into_iter()
        .map(|frame| {
            let rgba = std::sync::Arc::new(frame.rgba);
            let image = rune::ImageCache::new(std::sync::Arc::new(|| {})).rgba(
                frame.width,
                frame.height,
                (*rgba).clone(),
            );
            image
                .wait()
                .map_err(|_| anyhow::anyhow!("could not decode a frame"))?;
            Ok((
                frame.t_ms,
                Shot {
                    image,
                    rgba,
                    size: [frame.width, frame.height],
                    scale: 1.0,
                },
            ))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    anyhow::ensure!(!frames.is_empty(), "the recording has no frames");
    let mut clip = Clip::new(movie, duration_ms, frames, words);
    clip.bytes = bytes;
    clip.pixels = [probe.width, probe.height];
    Ok(clip)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn load(_: PathBuf, _: Vec<Word>) -> anyhow::Result<Clip> {
    anyhow::bail!("recordings open on macOS only")
}

/// How an export shrinks the span kept: its picture's longest side, and
/// the quality and frames a second it's encoded at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Shrink {
    pub(crate) max_edge: u32,
    pub(crate) quality: f64,
    pub(crate) fps: u32,
}

impl Shrink {
    /// How `size` shrinks a recording, at Labs › Screenshots' quality and
    /// frame rate; none when `size` keeps it as recorded.
    pub(crate) fn of(
        size: ExportSize,
        settings: &crate::app_settings::ScreenshotSettings,
    ) -> Option<Self> {
        Some(Self {
            max_edge: size.long_edge()?,
            quality: f64::from(settings.quality.hundredths()) / 100.0,
            fps: settings.fps(),
        })
    }
}

/// The span kept of `movie` written to `out`: copied when it's the whole
/// recording, else cut without re-encoding, or re-encoded smaller as
/// `shrink` says, at utility priority, handing `progress` the share done.
#[cfg(target_os = "macos")]
pub(super) fn write_span(
    movie: &std::path::Path,
    trim: Option<[u64; 2]>,
    shrink: Option<Shrink>,
    out: &std::path::Path,
    progress: &mut dyn FnMut(f64),
) -> anyhow::Result<()> {
    use scrawl_capture::helper;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Written beside it under a hidden name and renamed once whole, so a
    // failed or interrupted export never leaves half a movie in the folder.
    let partial = partial_path(out);
    let written = match (shrink, trim) {
        (Some(how), span) => {
            let how = helper::Reencode {
                max_edge: how.max_edge,
                quality: how.quality,
                fps: how.fps,
                span,
            };
            helper::reencode(movie, &partial, how, progress).map(|_| ())
        }
        (None, Some([from, to])) => helper::trim(movie, &partial, from, to).map(|_| ()),
        (None, None) => std::fs::copy(movie, &partial)
            .map(|_| ())
            .map_err(Into::into),
    }
    .and_then(|()| std::fs::rename(&partial, out).map_err(Into::into));
    if written.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    written
}

/// Where `out` is written until it's whole: a hidden sibling that keeps its
/// extension, which tells the helper the container.
fn partial_path(out: &std::path::Path) -> std::path::PathBuf {
    let name = out
        .file_name()
        .map_or_else(|| "export".into(), |name| name.to_string_lossy());
    out.with_file_name(format!(".partial-{name}"))
}

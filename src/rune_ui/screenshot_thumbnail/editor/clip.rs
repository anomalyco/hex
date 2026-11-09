//! A Screenshots recording in the editor: a filmstrip under the picture
//! with a handle at each end of the span kept, the frame at the handle
//! being dragged shown above, and the words said inside the span under it.
//! The kept span saves as a movie, or goes to a session as a contact sheet
//! with its words and the movie's path. Nothing changes the recording.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::PathBuf;

use super::Shot;

/// Frames along the filmstrip, read when the recording opens.
pub(super) const FRAMES: usize = 12;
/// Their long edge, in pixels.
pub(super) const FRAME_PX: u32 = 960;
/// The filmstrip's height.
pub(super) const STRIP_H: f32 = 44.0;
/// The shortest span a trim keeps.
const MIN_SPAN_MS: u64 = 500;
/// The most frames on a sent contact sheet.
const SHEET_FRAMES: usize = 4;

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

/// A recording open in the editor.
pub(crate) struct Clip {
    pub(crate) movie: PathBuf,
    pub(crate) duration_ms: u64,
    /// The filmstrip's frames and their times, in order.
    pub(crate) frames: Vec<(u64, Shot)>,
    pub(crate) words: Vec<Word>,
    /// The span kept, in ms.
    pub(crate) trim: [u64; 2],
    /// The handle a drag holds.
    pub(crate) holding: Option<Handle>,
    /// The frame shown above the filmstrip.
    pub(crate) shown: usize,
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
        }
    }

    /// The frame above the filmstrip.
    pub(crate) fn picture(&self) -> Option<&Shot> {
        self.frames.get(self.shown).map(|(_, shot)| shot)
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

    /// The handle nearer `ms`.
    pub(crate) fn nearer(&self, ms: u64) -> Handle {
        let [from, to] = self.trim;
        if ms.abs_diff(from) <= ms.abs_diff(to) {
            Handle::Start
        } else {
            Handle::End
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
        self.shown = self.frame_at(match handle {
            Handle::Start => self.trim[0],
            Handle::End => self.trim[1],
        });
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

/// `ms` as `0:07` or `12:03`.
pub(crate) fn duration_label(ms: u64) -> String {
    let seconds = ms.div_ceil(1000);
    format!("{}:{:02}", seconds / 60, seconds % 60)
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
    let duration_ms = helper::probe(&movie)?.duration_ms.max(1);
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
    Ok(Clip::new(movie, duration_ms, frames, words))
}

#[cfg(not(target_os = "macos"))]
pub(super) fn load(_: PathBuf, _: Vec<Word>) -> anyhow::Result<Clip> {
    anyhow::bail!("recordings open on macOS only")
}

/// The span kept of `movie` written to `out`: copied when it's the whole
/// recording, else cut without re-encoding.
#[cfg(target_os = "macos")]
pub(super) fn write_span(
    movie: &std::path::Path,
    trim: Option<[u64; 2]>,
    out: &std::path::Path,
) -> anyhow::Result<()> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match trim {
        Some([from, to]) => scrawl_capture::helper::trim(movie, out, from, to).map(|_| ()),
        None => std::fs::copy(movie, out).map(|_| ()).map_err(Into::into),
    }
}

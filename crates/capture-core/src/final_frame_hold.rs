//! Holds a recording's final video frame until the end of its audio.
//!
//! `ScreenCaptureKit` only delivers frames when screen content changes, so a recording of a still
//! screen ends its video track at the last change while audio continues to the real stop time.
//! The hold lengthens the final sample in the video track's own sample table (`stts`), so every
//! reader agrees: `AVFoundation`, `FFmpeg`, and external players. A movie-level time-scale edit
//! would be stored as an edit-list media rate, which `FFmpeg` ignores. When B-frames put a frame
//! presented earlier last in decode order, one skipped picture repeating the final frame is
//! appended instead ([`skip_frame`]). The video ends where its media's presentation ends, mapped
//! through the edit list: `SCRecordingOutput` can write an edit list that runs past the media,
//! and readers show nothing there. Nothing is re-encoded and no existing sample moves. The
//! edit is applied to an APFS clone in the same directory, verified, and atomically renamed over
//! the original, so a failure always leaves the original recording intact.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::probe::{Probe, probe};
use crate::time::MediaTime;

mod h264;
mod skip_frame;

const CONTAINER_BOXES: [&[u8; 4]; 6] = [b"moov", b"trak", b"mdia", b"minf", b"stbl", b"edts"];
// A movie header is small; refuse anything implausible rather than reading it into memory.
const MAX_MOVIE_HEADER_BYTES: u64 = 256 * 1024 * 1024;

/// Outcome of holding the final video frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinalFrameHold {
    /// The video track already lasts until the target end; the file was not modified.
    Covered { video_end: MediaTime },
    /// The final video sample now lasts from `video_end` until `held_end`.
    Held {
        video_end: MediaTime,
        held_end: MediaTime,
    },
}

impl FinalFrameHold {
    /// Video end after the operation.
    #[must_use]
    pub const fn end(self) -> MediaTime {
        match self {
            Self::Covered { video_end } => video_end,
            Self::Held { held_end, .. } => held_end,
        }
    }
}

#[derive(Debug)]
pub enum FinalFrameHoldError {
    Io(io::Error),
    Malformed(&'static str),
    Unsupported(&'static str),
    Overflow,
    /// `AVFoundation` could not read the original or the edited file's timing.
    Probe(String),
    Verification(&'static str),
}

impl fmt::Display for FinalFrameHoldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "could not access the recording: {error}"),
            Self::Malformed(what) => write!(f, "malformed MP4 container: {what}"),
            Self::Unsupported(what) => write!(f, "unsupported MP4 layout: {what}"),
            Self::Overflow => f.write_str("held duration cannot be represented in the MP4 header"),
            Self::Probe(error) => write!(f, "could not verify the held recording: {error}"),
            Self::Verification(what) => write!(f, "held recording failed verification: {what}"),
        }
    }
}

impl std::error::Error for FinalFrameHoldError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for FinalFrameHoldError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Lengthens the final video sample of an MP4 so video lasts until the end of the latest audio
/// track, or until `recorded_end` when the file has no audio.
///
/// The file keeps its path and format. The edit is written to a sibling clone and renamed over
/// the original only after `AVFoundation` confirms the new video end and unchanged audio.
///
/// # Errors
///
/// Returns an error for unreadable, malformed, fragmented, or unsupported MP4 files, or when the
/// edited file fails verification. The original file is left unchanged on every error.
pub fn hold_final_video_frame(
    path: &Path,
    recorded_end: MediaTime,
) -> Result<FinalFrameHold, FinalFrameHoldError> {
    let layout = read_layout(path)?;
    let mut moov = parse_boxes(&layout.moov, false)?;
    let plan = plan_hold(&moov, recorded_end)?;
    let video_end = movie_time(plan.video_end, plan.movie_timescale)?;
    if plan.video_end >= plan.target_end {
        return Ok(FinalFrameHold::Covered { video_end });
    }
    let held_end = movie_time(plan.target_end, plan.movie_timescale)?;
    let untouched = other_tracks(&moov, plan.video_track)?;
    let appended = if plan.reordered {
        skip_frame::append_skip_frame(
            &mut moov,
            &plan,
            &mut File::open(path)?,
            layout.append_offset() + 8,
        )?
    } else {
        Vec::new()
    };
    apply_hold(&mut moov, &plan)?;
    let header = serialize_boxes(&moov)?;

    let before = probe(path).map_err(FinalFrameHoldError::Probe)?;
    let staging = staging_path(path)?;
    let result = write_staged(path, &staging, &layout, &appended, &header).and_then(|()| {
        verify(
            &before,
            &probe(&staging).map_err(FinalFrameHoldError::Probe)?,
            held_end,
        )?;
        let staged = parse_boxes(&read_layout(&staging)?.moov, false)?;
        if other_tracks(&staged, plan.video_track)? != untouched {
            return Err(FinalFrameHoldError::Verification("another track changed"));
        }
        fs::rename(&staging, path)?;
        if let Some(directory) = path.parent() {
            let _ = File::open(directory).and_then(|directory| directory.sync_all());
        }
        Ok(())
    });
    if result.is_err() {
        let _ = fs::remove_file(&staging);
    }
    result.map(|()| FinalFrameHold::Held {
        video_end,
        held_end,
    })
}

struct Layout {
    file_len: u64,
    moov_offset: u64,
    moov_is_last: bool,
    moov: Vec<u8>,
}

impl Layout {
    /// Where a box appended by [`write_staged`] begins.
    fn append_offset(&self) -> u64 {
        if self.moov_is_last {
            self.moov_offset
        } else {
            self.file_len
        }
    }
}

fn read_layout(path: &Path) -> Result<Layout, FinalFrameHoldError> {
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut offset = 0;
    let mut moov = None;
    while offset < file_len {
        file.seek(SeekFrom::Start(offset))?;
        let mut header = [0; 16];
        let available = usize::try_from((file_len - offset).min(16)).unwrap_or(16);
        if available < 8 {
            return Err(FinalFrameHoldError::Malformed("truncated top-level box"));
        }
        file.read_exact(&mut header[..available])?;
        let kind = [header[4], header[5], header[6], header[7]];
        let size = match u32::from_be_bytes([header[0], header[1], header[2], header[3]]) {
            // Anything written after a box that runs to the end of the file would be inside it.
            0 if &kind != b"moov" => {
                return Err(FinalFrameHoldError::Unsupported("open-ended box"));
            }
            0 => file_len - offset,
            1 if available == 16 => u64::from_be_bytes(header[8..16].try_into().unwrap_or([0; 8])),
            1 => return Err(FinalFrameHoldError::Malformed("truncated large box header")),
            size => u64::from(size),
        };
        if size < 8 || size > file_len - offset {
            return Err(FinalFrameHoldError::Malformed("top-level box size"));
        }
        match &kind {
            b"moof" => return Err(FinalFrameHoldError::Unsupported("fragmented movie")),
            b"moov" if moov.is_some() => {
                return Err(FinalFrameHoldError::Malformed("multiple movie headers"));
            }
            b"moov" => moov = Some((offset, size)),
            _ => {}
        }
        offset += size;
    }
    let (moov_offset, moov_size) =
        moov.ok_or(FinalFrameHoldError::Malformed("missing movie header"))?;
    if moov_size > MAX_MOVIE_HEADER_BYTES {
        return Err(FinalFrameHoldError::Unsupported("oversized movie header"));
    }
    let mut bytes = vec![0; usize::try_from(moov_size).map_err(|_| FinalFrameHoldError::Overflow)?];
    file.seek(SeekFrom::Start(moov_offset))?;
    file.read_exact(&mut bytes)?;
    Ok(Layout {
        file_len,
        moov_offset,
        moov_is_last: moov_offset + moov_size == file_len,
        moov: bytes,
    })
}

/// Writes `appended` boxes and the edited movie header into an APFS clone of the original. A
/// trailing header is replaced in place; a leading one becomes a `free` box so no sample offsets
/// move.
fn write_staged(
    path: &Path,
    staging: &Path,
    layout: &Layout,
    appended: &[u8],
    header: &[u8],
) -> Result<(), FinalFrameHoldError> {
    fs::copy(path, staging)?;
    let mut file = OpenOptions::new().write(true).open(staging)?;
    if layout.moov_is_last {
        file.seek(SeekFrom::Start(layout.moov_offset))?;
        file.write_all(appended)?;
        file.write_all(header)?;
        let written = u64::try_from(appended.len() + header.len())
            .map_err(|_| FinalFrameHoldError::Overflow)?;
        file.set_len(layout.moov_offset + written)?;
    } else {
        file.seek(SeekFrom::Start(layout.moov_offset + 4))?;
        file.write_all(b"free")?;
        file.seek(SeekFrom::Start(layout.file_len))?;
        file.write_all(appended)?;
        file.write_all(header)?;
    }
    file.sync_all()?;
    Ok(())
}

fn staging_path(path: &Path) -> Result<PathBuf, FinalFrameHoldError> {
    let name =
        path.file_name()
            .and_then(|name| name.to_str())
            .ok_or(FinalFrameHoldError::Malformed(
                "recording path has no file name",
            ))?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    // AVFoundation infers the container from the extension, so the staging name keeps it.
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("mp4");
    Ok(path.with_file_name(format!(
        ".{name}.hold-{}-{nanos}.{extension}",
        std::process::id()
    )))
}

fn verify(before: &Probe, after: &Probe, held_end: MediaTime) -> Result<(), FinalFrameHoldError> {
    if after
        .video_end()
        .ok_or(FinalFrameHoldError::Verification("missing video"))?
        != held_end
    {
        return Err(FinalFrameHoldError::Verification(
            "video does not end at the held time",
        ));
    }
    if before.audio() != after.audio() || before.tracks.len() != after.tracks.len() {
        return Err(FinalFrameHoldError::Verification(
            "audio or track set changed",
        ));
    }
    Ok(())
}

/// Serialized bytes of every track other than the video track, which the hold must not change.
fn other_tracks(moov: &[Mp4Box], video_track: usize) -> Result<Vec<Vec<u8>>, FinalFrameHoldError> {
    let moov = moov
        .first()
        .ok_or(FinalFrameHoldError::Malformed("movie header"))?;
    children(moov)
        .iter()
        .enumerate()
        .filter(|(index, item)| item.kind == *b"trak" && *index != video_track)
        .map(|(_, trak)| serialize_boxes(std::slice::from_ref(trak)))
        .collect()
}

fn movie_time(value: u64, timescale: u32) -> Result<MediaTime, FinalFrameHoldError> {
    MediaTime::new(
        i64::try_from(value).map_err(|_| FinalFrameHoldError::Overflow)?,
        i32::try_from(timescale).map_err(|_| FinalFrameHoldError::Overflow)?,
    )
    .ok_or(FinalFrameHoldError::Overflow)
}

struct Mp4Box {
    kind: [u8; 4],
    large: bool,
    body: Body,
}

enum Body {
    Leaf(Vec<u8>),
    Container(Vec<Mp4Box>),
}

fn parse_boxes(data: &[u8], nested: bool) -> Result<Vec<Mp4Box>, FinalFrameHoldError> {
    let mut boxes = Vec::new();
    let mut offset = 0;
    while offset < data.len() {
        let rest = &data[offset..];
        if rest.len() < 8 {
            return Err(FinalFrameHoldError::Malformed("truncated box"));
        }
        let kind = [rest[4], rest[5], rest[6], rest[7]];
        let (size, header_len, large) = match be_u32(rest, 0)? {
            0 => (rest.len(), 8, false),
            1 => (
                usize::try_from(be_u64(rest, 8)?).map_err(|_| FinalFrameHoldError::Overflow)?,
                16,
                true,
            ),
            size => (
                usize::try_from(size).map_err(|_| FinalFrameHoldError::Overflow)?,
                8,
                false,
            ),
        };
        if size < header_len || size > rest.len() {
            return Err(FinalFrameHoldError::Malformed("box size"));
        }
        let payload = &rest[header_len..size];
        let body = if CONTAINER_BOXES.contains(&&kind) {
            Body::Container(parse_boxes(payload, true)?)
        } else {
            Body::Leaf(payload.to_vec())
        };
        boxes.push(Mp4Box { kind, large, body });
        offset += size;
    }
    if !nested && boxes.len() != 1 {
        return Err(FinalFrameHoldError::Malformed("movie header"));
    }
    Ok(boxes)
}

fn serialize_boxes(boxes: &[Mp4Box]) -> Result<Vec<u8>, FinalFrameHoldError> {
    let mut out = Vec::new();
    for item in boxes {
        let payload = match &item.body {
            Body::Leaf(bytes) => bytes.clone(),
            Body::Container(children) => serialize_boxes(children)?,
        };
        let compact = u32::try_from(payload.len() + 8)
            .ok()
            .filter(|_| !item.large);
        if let Some(size) = compact {
            out.extend_from_slice(&size.to_be_bytes());
            out.extend_from_slice(&item.kind);
        } else {
            out.extend_from_slice(&1_u32.to_be_bytes());
            out.extend_from_slice(&item.kind);
            let size =
                u64::try_from(payload.len() + 16).map_err(|_| FinalFrameHoldError::Overflow)?;
            out.extend_from_slice(&size.to_be_bytes());
        }
        out.extend_from_slice(&payload);
    }
    Ok(out)
}

fn children(item: &Mp4Box) -> &[Mp4Box] {
    match &item.body {
        Body::Container(children) => children,
        Body::Leaf(_) => &[],
    }
}

fn child(item: &Mp4Box, kind: [u8; 4]) -> Option<&Mp4Box> {
    children(item).iter().find(|child| child.kind == kind)
}

fn leaf<'a>(item: &'a Mp4Box, path: &[&[u8; 4]]) -> Option<&'a [u8]> {
    let mut current = item;
    for kind in path {
        current = child(current, **kind)?;
    }
    match &current.body {
        Body::Leaf(bytes) => Some(bytes),
        Body::Container(_) => None,
    }
}

fn leaf_mut<'a>(item: &'a mut Mp4Box, path: &[&[u8; 4]]) -> Option<&'a mut Vec<u8>> {
    let mut current = item;
    for kind in path {
        let Body::Container(children) = &mut current.body else {
            return None;
        };
        current = children.iter_mut().find(|child| &child.kind == *kind)?;
    }
    match &mut current.body {
        Body::Leaf(bytes) => Some(bytes),
        Body::Container(_) => None,
    }
}

struct HoldPlan {
    movie_timescale: u32,
    video_track: usize,
    /// Where the video's frames really stop, in movie time: the end of the media's presentation,
    /// mapped through the edit list. An edit list may claim more.
    video_end: u64,
    target_end: u64,
    /// The movie duration after the hold: the target end or the longest other track.
    movie_end: u64,
    media_extension: u64,
    /// The final edit after the hold, when the track has an edit list.
    held_edit: Option<HeldEdit>,
    /// The final sample in decode order is not presented last.
    reordered: bool,
}

#[derive(Clone, Copy)]
struct HeldEdit {
    /// Movie-time duration that makes the edit end at the target end.
    duration: u64,
    /// Media time where the edit then ends.
    media_end: u64,
}

fn plan_hold(moov: &[Mp4Box], recorded_end: MediaTime) -> Result<HoldPlan, FinalFrameHoldError> {
    let moov = &moov[0];
    if moov.kind != *b"moov" {
        return Err(FinalFrameHoldError::Malformed("movie header"));
    }
    if child(moov, *b"mvex").is_some() {
        return Err(FinalFrameHoldError::Unsupported("fragmented movie"));
    }
    let movie_header =
        leaf(moov, &[b"mvhd"]).ok_or(FinalFrameHoldError::Malformed("missing mvhd"))?;
    let (movie_timescale, _) = timescale_and_duration(movie_header)?;
    if movie_timescale == 0 {
        return Err(FinalFrameHoldError::Malformed("movie timescale"));
    }

    let mut video = None;
    let mut audio_end = None::<u64>;
    let mut others_end = 0_u64;
    for (index, trak) in children(moov).iter().enumerate() {
        if trak.kind != *b"trak" {
            continue;
        }
        let handler = leaf(trak, &[b"mdia", b"hdlr"])
            .and_then(|hdlr| hdlr.get(8..12))
            .ok_or(FinalFrameHoldError::Malformed("missing hdlr"))?;
        let tkhd = leaf(trak, &[b"tkhd"]).ok_or(FinalFrameHoldError::Malformed("missing tkhd"))?;
        let duration = tkhd_duration(tkhd)?;
        match handler {
            b"vide" if video.is_some() => {
                return Err(FinalFrameHoldError::Unsupported("multiple video tracks"));
            }
            b"vide" => video = Some((index, duration)),
            b"soun" => audio_end = Some(audio_end.map_or(duration, |end| end.max(duration))),
            _ => {}
        }
        if handler != b"vide" {
            others_end = others_end.max(duration);
        }
    }
    let (video_track, track_duration) =
        video.ok_or(FinalFrameHoldError::Unsupported("no video"))?;
    let target_end = match audio_end {
        Some(end) => end,
        None => ceil_ratio(
            u64::try_from(recorded_end.value()).map_err(|_| FinalFrameHoldError::Overflow)?,
            u64::from(movie_timescale),
            u64::try_from(recorded_end.timescale()).map_err(|_| FinalFrameHoldError::Overflow)?,
        )?,
    };

    let trak = &children(moov)[video_track];
    let mdhd = leaf(trak, &[b"mdia", b"mdhd"]).ok_or(FinalFrameHoldError::Malformed("mdhd"))?;
    let (media_timescale, _) = timescale_and_duration(mdhd)?;
    if media_timescale == 0 {
        return Err(FinalFrameHoldError::Malformed("media timescale"));
    }
    let to_movie =
        |media: u64| ceil_ratio(media, u64::from(movie_timescale), media_timescale.into());
    let to_media =
        |movie: u64| ceil_ratio(movie, media_timescale.into(), u64::from(movie_timescale));
    let presentation = media_presentation(trak)?;
    let edits = leaf(trak, &[b"edts", b"elst"])
        .map(edit_entries)
        .transpose()?;
    // A final edit that is not plain playback is trusted as written; holding it is refused below.
    let final_edit = match edits.as_deref().map(<[Edit]>::split_last) {
        None => None,
        Some(None) => return Err(FinalFrameHoldError::Unsupported("empty edit list")),
        Some(Some((last, earlier))) => Some((
            last,
            earlier.iter().try_fold(0_u64, |start, edit| {
                start
                    .checked_add(edit.duration)
                    .ok_or(FinalFrameHoldError::Overflow)
            })?,
        )),
    };
    let plain = |edit: &Edit| edit.media_time >= 0 && edit.rate == 0x0001_0000;
    // An edit that runs past the media shows nothing there, so the media decides where it ends.
    let video_end = track_duration.min(match final_edit {
        None => to_movie(presentation.end)?,
        Some((last, start)) if plain(last) => {
            let shown = presentation
                .end
                .saturating_sub(last.media_time.cast_unsigned());
            start
                .checked_add(last.duration.min(to_movie(shown)?))
                .ok_or(FinalFrameHoldError::Overflow)?
        }
        Some(_) => track_duration,
    });
    if video_end >= target_end {
        return Ok(HoldPlan {
            movie_timescale,
            video_track,
            video_end,
            target_end,
            movie_end: target_end,
            media_extension: 0,
            held_edit: None,
            reordered: false,
        });
    }

    let held_edit = match final_edit {
        None => None,
        Some((last, _)) if !plain(last) => {
            return Err(FinalFrameHoldError::Unsupported(
                "final edit is not plain playback",
            ));
        }
        Some((last, start)) => {
            let duration = target_end
                .checked_sub(start)
                .ok_or(FinalFrameHoldError::Malformed("edit list"))?;
            Some(HeldEdit {
                duration,
                media_end: last
                    .media_time
                    .cast_unsigned()
                    .checked_add(to_media(duration)?)
                    .ok_or(FinalFrameHoldError::Overflow)?,
            })
        }
    };
    let media_end = match held_edit {
        Some(edit) => edit.media_end,
        None => to_media(target_end)?,
    };
    Ok(HoldPlan {
        movie_timescale,
        video_track,
        video_end,
        target_end,
        movie_end: target_end.max(others_end),
        media_extension: media_end.saturating_sub(presentation.end),
        held_edit,
        reordered: presentation.reordered,
    })
}

struct MediaPresentation {
    /// Media time where the final frame shown stops: its presentation time plus its duration.
    end: u64,
    /// The final sample in decode order is not the final frame shown, so lengthening it would
    /// hold the wrong frame.
    reordered: bool,
}

fn media_presentation(trak: &Mp4Box) -> Result<MediaPresentation, FinalFrameHoldError> {
    let stbl = [b"mdia", b"minf", b"stbl"];
    let stts = leaf(trak, &[stbl[0], stbl[1], stbl[2], b"stts"])
        .ok_or(FinalFrameHoldError::Malformed("missing stts"))?;
    let mut samples = Vec::new();
    let mut decode_time = 0_i64;
    for (count, delta) in time_to_sample_entries(stts)? {
        for _ in 0..count {
            samples.push((decode_time, i64::from(delta)));
            decode_time = decode_time
                .checked_add(i64::from(delta))
                .ok_or(FinalFrameHoldError::Overflow)?;
        }
    }
    if let Some(ctts) = leaf(trak, &[stbl[0], stbl[1], stbl[2], b"ctts"]) {
        let mut sample = 0;
        let entries = be_u32(ctts, 4)?;
        for entry in 0..usize::try_from(entries).map_err(|_| FinalFrameHoldError::Overflow)? {
            let count = be_u32(ctts, 8 + entry * 8)?;
            let offset = i64::from(be_u32(ctts, 12 + entry * 8)?.cast_signed());
            for _ in 0..count {
                let (time, _) = samples
                    .get_mut(sample)
                    .ok_or(FinalFrameHoldError::Malformed("ctts sample count"))?;
                *time = time
                    .checked_add(offset)
                    .ok_or(FinalFrameHoldError::Overflow)?;
                sample += 1;
            }
        }
    }
    let (last, earlier) = samples
        .split_last()
        .ok_or(FinalFrameHoldError::Malformed("empty video track"))?;
    let reordered = earlier.iter().any(|(time, _)| time >= &last.0);
    let (time, duration) = samples
        .iter()
        .copied()
        .max_by_key(|(time, _)| *time)
        .unwrap_or(*last);
    Ok(MediaPresentation {
        end: u64::try_from(
            time.checked_add(duration)
                .ok_or(FinalFrameHoldError::Overflow)?,
        )
        .map_err(|_| FinalFrameHoldError::Malformed("negative presentation time"))?,
        reordered,
    })
}

fn apply_hold(moov: &mut [Mp4Box], plan: &HoldPlan) -> Result<(), FinalFrameHoldError> {
    let moov = &mut moov[0];
    let movie_header = leaf_mut(moov, &[b"mvhd"]).ok_or(FinalFrameHoldError::Malformed("mvhd"))?;
    set_header_duration(movie_header, plan.movie_end)?;

    let Body::Container(tracks) = &mut moov.body else {
        return Err(FinalFrameHoldError::Malformed("movie header"));
    };
    let trak = &mut tracks[plan.video_track];
    let tkhd = leaf_mut(trak, &[b"tkhd"]).ok_or(FinalFrameHoldError::Malformed("tkhd"))?;
    set_tkhd_duration(tkhd, plan.target_end)?;
    if let (Some(elst), Some(edit)) = (leaf_mut(trak, &[b"edts", b"elst"]), plan.held_edit) {
        set_last_edit_duration(elst, edit.duration)?;
    }
    if plan.reordered {
        // The appended picture already carries the held media time.
        return Ok(());
    }
    let mdhd = leaf_mut(trak, &[b"mdia", b"mdhd"]).ok_or(FinalFrameHoldError::Malformed("mdhd"))?;
    let (_, media_duration) = timescale_and_duration(mdhd)?;
    set_header_duration(
        mdhd,
        media_duration
            .checked_add(plan.media_extension)
            .ok_or(FinalFrameHoldError::Overflow)?,
    )?;
    let stbl = [b"mdia", b"minf", b"stbl"];
    let stts = leaf_mut(trak, &[stbl[0], stbl[1], stbl[2], b"stts"])
        .ok_or(FinalFrameHoldError::Malformed("missing stts"))?;
    extend_last_sample(stts, plan.media_extension)?;
    if let Some(cslg) = leaf_mut(trak, &[stbl[0], stbl[1], stbl[2], b"cslg"]) {
        extend_composition_end(cslg, plan.media_extension)?;
    }
    Ok(())
}

fn extend_last_sample(stts: &mut Vec<u8>, extension: u64) -> Result<(), FinalFrameHoldError> {
    let mut entries = time_to_sample_entries(stts)?;
    let (count, delta) = entries
        .pop()
        .ok_or(FinalFrameHoldError::Malformed("empty stts"))?;
    let held =
        u32::try_from(u64::from(delta) + extension).map_err(|_| FinalFrameHoldError::Overflow)?;
    if count > 1 {
        entries.push((count - 1, delta));
    }
    entries.push((1, held));
    stts.truncate(4);
    let entry_count = u32::try_from(entries.len()).map_err(|_| FinalFrameHoldError::Overflow)?;
    stts.extend_from_slice(&entry_count.to_be_bytes());
    for (count, delta) in entries {
        stts.extend_from_slice(&count.to_be_bytes());
        stts.extend_from_slice(&delta.to_be_bytes());
    }
    Ok(())
}

fn time_to_sample_entries(stts: &[u8]) -> Result<Vec<(u32, u32)>, FinalFrameHoldError> {
    let entries = usize::try_from(be_u32(stts, 4)?).map_err(|_| FinalFrameHoldError::Overflow)?;
    (0..entries)
        .map(|entry| Ok((be_u32(stts, 8 + entry * 8)?, be_u32(stts, 12 + entry * 8)?)))
        .collect()
}

struct Edit {
    duration: u64,
    media_time: i64,
    rate: u32,
}

fn edit_entries(elst: &[u8]) -> Result<Vec<Edit>, FinalFrameHoldError> {
    let entry_len = if elst.first() == Some(&1) { 20 } else { 12 };
    let entries = usize::try_from(be_u32(elst, 4)?).map_err(|_| FinalFrameHoldError::Overflow)?;
    (0..entries)
        .map(|entry| {
            let offset = 8 + entry * entry_len;
            Ok(if entry_len == 20 {
                Edit {
                    duration: be_u64(elst, offset)?,
                    media_time: be_u64(elst, offset + 8)?.cast_signed(),
                    rate: be_u32(elst, offset + 16)?,
                }
            } else {
                Edit {
                    duration: u64::from(be_u32(elst, offset)?),
                    media_time: i64::from(be_u32(elst, offset + 4)?.cast_signed()),
                    rate: be_u32(elst, offset + 8)?,
                }
            })
        })
        .collect()
}

fn set_last_edit_duration(elst: &mut [u8], duration: u64) -> Result<(), FinalFrameHoldError> {
    let version = *elst.first().ok_or(FinalFrameHoldError::Malformed("elst"))?;
    let entries = usize::try_from(be_u32(elst, 4)?).map_err(|_| FinalFrameHoldError::Overflow)?;
    let entry_len = if version == 1 { 20 } else { 12 };
    let offset = 8 + entries
        .checked_sub(1)
        .ok_or(FinalFrameHoldError::Malformed("elst"))?
        * entry_len;
    if version == 1 {
        put(elst, offset, &duration.to_be_bytes())
    } else {
        let duration = u32::try_from(duration).map_err(|_| FinalFrameHoldError::Overflow)?;
        put(elst, offset, &duration.to_be_bytes())
    }
}

fn extend_composition_end(cslg: &mut [u8], extension: u64) -> Result<(), FinalFrameHoldError> {
    if cslg.first() == Some(&1) {
        let end = be_u64(cslg, 36)?
            .cast_signed()
            .checked_add(i64::try_from(extension).map_err(|_| FinalFrameHoldError::Overflow)?)
            .ok_or(FinalFrameHoldError::Overflow)?;
        put(cslg, 36, &end.to_be_bytes())
    } else {
        let end = i32::try_from(
            i64::from(be_u32(cslg, 20)?.cast_signed())
                + i64::try_from(extension).map_err(|_| FinalFrameHoldError::Overflow)?,
        )
        .map_err(|_| FinalFrameHoldError::Overflow)?;
        put(cslg, 20, &end.to_be_bytes())
    }
}

/// Reads `mvhd`/`mdhd` timescale and duration, which share a layout.
fn timescale_and_duration(header: &[u8]) -> Result<(u32, u64), FinalFrameHoldError> {
    if header.first() == Some(&1) {
        Ok((be_u32(header, 20)?, be_u64(header, 24)?))
    } else {
        Ok((be_u32(header, 12)?, u64::from(be_u32(header, 16)?)))
    }
}

fn set_header_duration(header: &mut [u8], duration: u64) -> Result<(), FinalFrameHoldError> {
    if header.first() == Some(&1) {
        put(header, 24, &duration.to_be_bytes())
    } else {
        let duration = u32::try_from(duration).map_err(|_| FinalFrameHoldError::Overflow)?;
        put(header, 16, &duration.to_be_bytes())
    }
}

fn tkhd_duration(tkhd: &[u8]) -> Result<u64, FinalFrameHoldError> {
    if tkhd.first() == Some(&1) {
        be_u64(tkhd, 28)
    } else {
        be_u32(tkhd, 20).map(u64::from)
    }
}

fn set_tkhd_duration(tkhd: &mut [u8], duration: u64) -> Result<(), FinalFrameHoldError> {
    if tkhd.first() == Some(&1) {
        put(tkhd, 28, &duration.to_be_bytes())
    } else {
        let duration = u32::try_from(duration).map_err(|_| FinalFrameHoldError::Overflow)?;
        put(tkhd, 20, &duration.to_be_bytes())
    }
}

fn ceil_ratio(value: u64, numerator: u64, denominator: u64) -> Result<u64, FinalFrameHoldError> {
    if denominator == 0 {
        return Err(FinalFrameHoldError::Malformed("timescale"));
    }
    let scaled = u128::from(value) * u128::from(numerator);
    u64::try_from(scaled.div_ceil(u128::from(denominator)))
        .map_err(|_| FinalFrameHoldError::Overflow)
}

fn be_u32(bytes: &[u8], offset: usize) -> Result<u32, FinalFrameHoldError> {
    bytes
        .get(offset..offset + 4)
        .and_then(|slice| slice.try_into().ok())
        .map(u32::from_be_bytes)
        .ok_or(FinalFrameHoldError::Malformed("truncated box field"))
}

fn be_u64(bytes: &[u8], offset: usize) -> Result<u64, FinalFrameHoldError> {
    bytes
        .get(offset..offset + 8)
        .and_then(|slice| slice.try_into().ok())
        .map(u64::from_be_bytes)
        .ok_or(FinalFrameHoldError::Malformed("truncated box field"))
}

fn put(bytes: &mut [u8], offset: usize, value: &[u8]) -> Result<(), FinalFrameHoldError> {
    bytes
        .get_mut(offset..offset + value.len())
        .ok_or(FinalFrameHoldError::Malformed("truncated box field"))?
        .copy_from_slice(value);
    Ok(())
}

#[cfg(test)]
mod tests;

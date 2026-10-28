//! Holds a reordered H.264 track by appending one picture that repeats its final frame.
//!
//! With B-frames, the sample decoded last is presented before the final frame, so lengthening it
//! would leave the wrong picture on screen (and `FFmpeg` would still end the video at the final
//! frame). Instead, a skipped P picture (see [`super::h264`]) is appended after every other
//! sample: it copies the frame presented last, is presented after it, and lasts until the held
//! end. The original samples, their data and every other track stay byte-identical; the new
//! picture's data goes in a new `mdat` box and its picture parameter set into the `avcC`.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use super::h264::{self, ParameterSets, SliceHeader};
use super::{
    Body, FinalFrameHoldError, HoldPlan, Mp4Box, be_u32, be_u64, leaf, put, set_header_duration,
    time_to_sample_entries, timescale_and_duration,
};

const STBL: [&[u8; 4]; 3] = [b"mdia", b"minf", b"stbl"];
/// Sample table boxes this edit keeps consistent. Any other box (compact sizes, padding,
/// degradation priorities, encryption) may describe samples per entry, so it is refused.
const KNOWN_SAMPLE_TABLE_BOXES: [&[u8; 4]; 12] = [
    b"stsd", b"stts", b"ctts", b"cslg", b"stss", b"sdtp", b"stsc", b"stsz", b"stco", b"co64",
    b"sgpd", b"sbgp",
];
// The trailing group of pictures is read into memory to parse its slice headers.
const MAX_TRAILING_BYTES: u64 = 256 * 1024 * 1024;
/// `sdtp` flags: depends on other samples, and no other sample depends on it.
const DISPOSABLE_DEPENDENT: u8 = 0b0001_1000;
const VISUAL_SAMPLE_ENTRY_LEN: usize = 86;

fn unsupported(what: &'static str) -> FinalFrameHoldError {
    FinalFrameHoldError::Unsupported(what)
}

fn malformed(what: &'static str) -> FinalFrameHoldError {
    FinalFrameHoldError::Malformed(what)
}

fn overflow<T>(_: T) -> FinalFrameHoldError {
    FinalFrameHoldError::Overflow
}

/// Appends the skipped picture to the video track's sample tables. `data_offset` is where its
/// sample data will sit in the edited file. Returns the new `mdat` box to write there.
pub(super) fn append_skip_frame(
    moov: &mut [Mp4Box],
    plan: &HoldPlan,
    file: &mut File,
    data_offset: u64,
) -> Result<Vec<u8>, FinalFrameHoldError> {
    let edit_media_end = plan
        .edit_media_end
        .ok_or_else(|| unsupported("reordered video without an edit list"))?;
    let trak = video_trak_mut(moov, plan.video_track)?;
    let stbl = stbl_mut(trak)?;
    if children(stbl)
        .iter()
        .any(|item| !KNOWN_SAMPLE_TABLE_BOXES.contains(&&item.kind))
    {
        return Err(unsupported("sample table box"));
    }
    let tables = SampleTables::read(stbl)?;
    let config = AvcConfig::parse(&tables.stsd)?;
    let parameters = ParameterSets {
        sps: config
            .sps
            .iter()
            .map(|nal| h264::parse_sps(nal))
            .collect::<Result<_, _>>()?,
        pps: config
            .pps
            .iter()
            .map(|nal| h264::parse_pps(nal))
            .collect::<Result<_, _>>()?,
    };

    let timing = Timing::read(&tables)?;
    let copied = timing.presented_last()?;
    let last_sync = tables.last_sync()?;
    if copied < last_sync {
        return Err(unsupported("final frame precedes the last sync sample"));
    }
    let offsets = tables.sample_offsets()?;
    let pictures = read_slice_headers(file, &tables, &offsets, last_sync, &config, &parameters)?;
    let sps_id = parameters
        .pps
        .iter()
        .find(|pps| pps.id == pictures[copied - last_sync].pps_id)
        .map(|pps| pps.sps_id)
        .ok_or_else(|| malformed("slice names a missing PPS"))?;
    if pictures.iter().any(|picture| {
        parameters
            .pps
            .iter()
            .any(|pps| pps.id == picture.pps_id && pps.sps_id != sps_id)
    }) {
        return Err(unsupported(
            "sequence parameter set changes in the final group",
        ));
    }
    let sps = parameters
        .sps
        .iter()
        .find(|sps| sps.id == sps_id)
        .ok_or_else(|| malformed("PPS names a missing SPS"))?;
    let skip = h264::plan_skip_picture(&pictures, copied - last_sync, sps)?;
    let pps_id = parameters.unused_pps_id()?;
    let pps = h264::skip_pps(pps_id, sps_id);
    let slice = h264::skip_slice(sps, pps_id, skip);

    let mut sample = Vec::with_capacity(config.length_size + slice.len());
    let slice_len = u64::try_from(slice.len()).map_err(overflow)?;
    if slice_len >> (8 * config.length_size) != 0 {
        return Err(FinalFrameHoldError::Overflow);
    }
    sample.extend_from_slice(&slice_len.to_be_bytes()[8 - config.length_size..]);
    sample.extend_from_slice(&slice);

    // The new picture decodes after every sample and is presented after the final frame, which
    // keeps the image it had until the new picture starts.
    let decode_time = timing.decode_end;
    let copied_duration = i64::from(timing.deltas[copied].max(1));
    let presentation = (timing.presentation[copied] + copied_duration).max(decode_time);
    let end = i64::try_from(edit_media_end).map_err(overflow)?;
    if presentation >= end {
        return Err(unsupported("hold is shorter than one frame"));
    }
    let duration = u32::try_from(end - presentation).map_err(overflow)?;
    let composition_offset = i32::try_from(presentation - decode_time).map_err(overflow)?;

    let new_stsd = config.with_pps(&tables.stsd, &pps)?;
    let sample_len = u32::try_from(sample.len()).map_err(overflow)?;
    let has_sdtp = child(stbl, *b"sdtp").is_some();
    replace_leaf(stbl, *b"stsd", new_stsd)?;
    append_time_entry(leaf_mut(stbl, *b"stts")?, duration.cast_signed())?;
    append_time_entry(leaf_mut(stbl, *b"ctts")?, composition_offset)?;
    append_size(leaf_mut(stbl, *b"stsz")?, sample_len)?;
    append_chunk(stbl, data_offset)?;
    if has_sdtp {
        let sdtp = leaf_mut(stbl, *b"sdtp")?;
        if sdtp.len() != 4 + timing.deltas.len() {
            return Err(malformed("sdtp sample count"));
        }
        sdtp.push(DISPOSABLE_DEPENDENT);
    }
    if let Ok(cslg) = leaf_mut(stbl, *b"cslg") {
        extend_composition(cslg, i64::from(composition_offset), end)?;
    }
    let media_duration = u64::try_from(decode_time + i64::from(duration)).map_err(overflow)?;
    let mdhd = super::leaf_mut(trak, &[b"mdia", b"mdhd"]).ok_or_else(|| malformed("mdhd"))?;
    let (_, old_duration) = timescale_and_duration(mdhd)?;
    set_header_duration(mdhd, old_duration.max(media_duration))?;

    let mut mdat = Vec::with_capacity(8 + sample.len());
    mdat.extend_from_slice(&(sample_len + 8).to_be_bytes());
    mdat.extend_from_slice(b"mdat");
    mdat.extend_from_slice(&sample);
    Ok(mdat)
}

fn read_slice_headers(
    file: &mut File,
    tables: &SampleTables,
    offsets: &[u64],
    first: usize,
    config: &AvcConfig,
    parameters: &ParameterSets,
) -> Result<Vec<SliceHeader>, FinalFrameHoldError> {
    let total: u64 = tables.sizes[first..]
        .iter()
        .map(|&size| u64::from(size))
        .sum();
    if total > MAX_TRAILING_BYTES {
        return Err(unsupported("final group of pictures is too large"));
    }
    let mut sample = Vec::new();
    (first..tables.sizes.len())
        .map(|index| {
            sample.resize(usize::try_from(tables.sizes[index]).map_err(overflow)?, 0);
            file.seek(SeekFrom::Start(offsets[index]))?;
            file.read_exact(&mut sample)?;
            h264::parse_slice_header(h264::first_slice(&sample, config.length_size)?, parameters)
        })
        .collect()
}

fn children(item: &Mp4Box) -> &[Mp4Box] {
    super::children(item)
}

fn child(item: &Mp4Box, kind: [u8; 4]) -> Option<&Mp4Box> {
    super::child(item, kind)
}

fn video_trak_mut(moov: &mut [Mp4Box], index: usize) -> Result<&mut Mp4Box, FinalFrameHoldError> {
    let Body::Container(tracks) = &mut moov
        .first_mut()
        .ok_or_else(|| malformed("movie header"))?
        .body
    else {
        return Err(malformed("movie header"));
    };
    tracks
        .get_mut(index)
        .ok_or_else(|| malformed("video track"))
}

fn stbl_mut(trak: &mut Mp4Box) -> Result<&mut Mp4Box, FinalFrameHoldError> {
    let mut current = trak;
    for kind in STBL {
        let Body::Container(children) = &mut current.body else {
            return Err(malformed("missing stbl"));
        };
        current = children
            .iter_mut()
            .find(|child| &child.kind == kind)
            .ok_or_else(|| malformed("missing stbl"))?;
    }
    Ok(current)
}

fn box_mut(stbl: &mut Mp4Box, kind: [u8; 4]) -> Option<&mut Mp4Box> {
    let Body::Container(children) = &mut stbl.body else {
        return None;
    };
    children.iter_mut().find(|child| child.kind == kind)
}

fn leaf_mut(stbl: &mut Mp4Box, kind: [u8; 4]) -> Result<&mut Vec<u8>, FinalFrameHoldError> {
    match box_mut(stbl, kind).map(|item| &mut item.body) {
        Some(Body::Leaf(bytes)) => Ok(bytes),
        _ => Err(malformed("missing sample table")),
    }
}

fn replace_leaf(
    stbl: &mut Mp4Box,
    kind: [u8; 4],
    bytes: Vec<u8>,
) -> Result<(), FinalFrameHoldError> {
    *leaf_mut(stbl, kind)? = bytes;
    Ok(())
}

/// The video track's sample tables, read before anything is edited.
struct SampleTables {
    stsd: Vec<u8>,
    stts: Vec<u8>,
    ctts: Vec<u8>,
    stss: Option<Vec<u8>>,
    stsc: Vec<u8>,
    chunk_offsets: Vec<u64>,
    sizes: Vec<u32>,
}

impl SampleTables {
    fn read(stbl: &Mp4Box) -> Result<Self, FinalFrameHoldError> {
        let table = |kind: &[u8; 4]| leaf(stbl, &[kind]).map(<[u8]>::to_vec);
        let required = |kind: &[u8; 4], what| table(kind).ok_or_else(|| malformed(what));
        let chunk_offsets = if let Some(stco) = leaf(stbl, &[b"stco"]) {
            (0..usize::try_from(be_u32(stco, 4)?).map_err(overflow)?)
                .map(|chunk| be_u32(stco, 8 + chunk * 4).map(u64::from))
                .collect::<Result<_, _>>()?
        } else {
            let co64 = leaf(stbl, &[b"co64"]).ok_or_else(|| malformed("missing stco"))?;
            (0..usize::try_from(be_u32(co64, 4)?).map_err(overflow)?)
                .map(|chunk| be_u64(co64, 8 + chunk * 8))
                .collect::<Result<_, _>>()?
        };
        let stsz = leaf(stbl, &[b"stsz"]).ok_or_else(|| malformed("missing stsz"))?;
        let (fixed, count) = (be_u32(stsz, 4)?, be_u32(stsz, 8)?);
        let count = usize::try_from(count).map_err(overflow)?;
        let sizes = if fixed == 0 {
            (0..count)
                .map(|sample| be_u32(stsz, 12 + sample * 4))
                .collect::<Result<_, _>>()?
        } else {
            vec![fixed; count]
        };
        Ok(Self {
            stsd: required(b"stsd", "missing stsd")?,
            stts: required(b"stts", "missing stts")?,
            ctts: required(b"ctts", "missing ctts")?,
            stss: table(b"stss"),
            stsc: required(b"stsc", "missing stsc")?,
            chunk_offsets,
            sizes,
        })
    }

    /// Index of the last sync sample. Without `stss` every sample is a sync sample, which a
    /// reordered stream cannot be.
    fn last_sync(&self) -> Result<usize, FinalFrameHoldError> {
        let stss = self
            .stss
            .as_deref()
            .ok_or_else(|| unsupported("every sample is a sync sample"))?;
        let entries = usize::try_from(be_u32(stss, 4)?).map_err(overflow)?;
        if entries == 0 {
            return Err(malformed("stss"));
        }
        let index = usize::try_from(be_u32(stss, 4 + entries * 4)?).map_err(overflow)?;
        if index == 0 || index > self.sizes.len() {
            return Err(malformed("stss"));
        }
        Ok(index - 1)
    }

    fn sample_offsets(&self) -> Result<Vec<u64>, FinalFrameHoldError> {
        let entries = usize::try_from(be_u32(&self.stsc, 4)?).map_err(overflow)?;
        let runs = (0..entries)
            .map(|entry| {
                Ok((
                    be_u32(&self.stsc, 8 + entry * 12)?,
                    be_u32(&self.stsc, 12 + entry * 12)?,
                ))
            })
            .collect::<Result<Vec<_>, FinalFrameHoldError>>()?;
        let mut offsets = Vec::with_capacity(self.sizes.len());
        let mut sizes = self.sizes.iter();
        for (chunk, &chunk_offset) in self.chunk_offsets.iter().enumerate() {
            let chunk = u32::try_from(chunk + 1).map_err(overflow)?;
            let samples = runs
                .iter()
                .rev()
                .find(|(first, _)| *first <= chunk)
                .map(|(_, samples)| *samples)
                .ok_or_else(|| malformed("stsc"))?;
            let mut offset = chunk_offset;
            for _ in 0..samples {
                let size = sizes.next().ok_or_else(|| malformed("stsc sample count"))?;
                offsets.push(offset);
                offset = offset
                    .checked_add(u64::from(*size))
                    .ok_or(FinalFrameHoldError::Overflow)?;
            }
        }
        if offsets.len() != self.sizes.len() {
            return Err(malformed("stsc sample count"));
        }
        Ok(offsets)
    }
}

/// Decode and presentation times of every sample, in media time.
struct Timing {
    deltas: Vec<u32>,
    presentation: Vec<i64>,
    decode_end: i64,
}

impl Timing {
    fn read(tables: &SampleTables) -> Result<Self, FinalFrameHoldError> {
        let mut deltas = Vec::with_capacity(tables.sizes.len());
        for (count, delta) in time_to_sample_entries(&tables.stts)? {
            for _ in 0..count {
                if deltas.len() == tables.sizes.len() {
                    return Err(malformed("stts sample count"));
                }
                deltas.push(delta);
            }
        }
        if deltas.len() != tables.sizes.len() {
            return Err(malformed("stts sample count"));
        }
        let mut presentation = Vec::with_capacity(deltas.len());
        let mut decode_time = 0_i64;
        for (count, offset) in time_to_sample_entries(&tables.ctts)? {
            for _ in 0..count {
                let delta = deltas
                    .get(presentation.len())
                    .ok_or_else(|| malformed("ctts sample count"))?;
                presentation.push(decode_time + i64::from(offset.cast_signed()));
                decode_time += i64::from(*delta);
            }
        }
        if presentation.len() != deltas.len() {
            return Err(malformed("ctts sample count"));
        }
        Ok(Self {
            deltas,
            presentation,
            decode_end: decode_time,
        })
    }

    /// The sample presented last.
    fn presented_last(&self) -> Result<usize, FinalFrameHoldError> {
        let last = self
            .presentation
            .iter()
            .enumerate()
            .max_by_key(|(_, time)| **time)
            .map(|(index, _)| index)
            .ok_or_else(|| malformed("empty video track"))?;
        let ties = self
            .presentation
            .iter()
            .filter(|time| **time == self.presentation[last])
            .count();
        if ties > 1 {
            return Err(malformed("two frames share a presentation time"));
        }
        Ok(last)
    }
}

/// The `avcC` box inside the single `avc1`/`avc3` sample entry.
struct AvcConfig {
    /// Byte range of the `avcC` box within `stsd`.
    avcc: std::ops::Range<usize>,
    length_size: usize,
    sps: Vec<Vec<u8>>,
    pps: Vec<Vec<u8>>,
    /// Offset of the PPS count byte, and of the end of the PPS list, within the `avcC` payload.
    pps_count_at: usize,
    pps_end: usize,
}

impl AvcConfig {
    fn parse(stsd: &[u8]) -> Result<Self, FinalFrameHoldError> {
        if be_u32(stsd, 4)? != 1 {
            return Err(unsupported("multiple sample descriptions"));
        }
        let entry_len = usize::try_from(be_u32(stsd, 8)?).map_err(overflow)?;
        let entry = stsd
            .get(8..8 + entry_len)
            .filter(|_| entry_len >= VISUAL_SAMPLE_ENTRY_LEN)
            .ok_or_else(|| malformed("stsd entry"))?;
        if !matches!(&entry[4..8], b"avc1" | b"avc3") {
            return Err(unsupported("video is not H.264"));
        }
        let mut offset = VISUAL_SAMPLE_ENTRY_LEN;
        while offset + 8 <= entry.len() {
            let size = usize::try_from(be_u32(entry, offset)?).map_err(overflow)?;
            if size < 8 || offset + size > entry.len() {
                return Err(malformed("sample entry box"));
            }
            if &entry[offset + 4..offset + 8] == b"avcC" {
                let start = 8 + offset;
                return Self::parse_payload(&entry[offset + 8..offset + size]).map(|config| Self {
                    avcc: start..start + size,
                    ..config
                });
            }
            offset += size;
        }
        Err(malformed("missing avcC"))
    }

    fn parse_payload(avcc: &[u8]) -> Result<Self, FinalFrameHoldError> {
        let byte = |at: usize| avcc.get(at).copied().ok_or_else(|| malformed("avcC"));
        let length_size = usize::from(byte(4)? & 3) + 1;
        if length_size == 3 {
            return Err(malformed("avcC NAL length size"));
        }
        let mut offset = 6;
        let sps = read_parameter_sets(avcc, &mut offset, usize::from(byte(5)? & 0x1f))?;
        let pps_count_at = offset;
        offset += 1;
        let pps = read_parameter_sets(avcc, &mut offset, usize::from(byte(pps_count_at)?))?;
        if sps.is_empty() || pps.is_empty() {
            return Err(malformed("avcC parameter sets"));
        }
        Ok(Self {
            avcc: 0..0,
            length_size,
            sps,
            pps,
            pps_count_at,
            pps_end: offset,
        })
    }

    /// `stsd` with `pps` added to the end of the `avcC` picture parameter set list.
    fn with_pps(&self, stsd: &[u8], pps: &[u8]) -> Result<Vec<u8>, FinalFrameHoldError> {
        let count = u8::try_from(self.pps.len() + 1)
            .map_err(|_| unsupported("too many picture parameter sets"))?;
        let len = u16::try_from(pps.len()).map_err(overflow)?;
        let payload = self.avcc.start + 8;
        let mut out = Vec::with_capacity(stsd.len() + pps.len() + 2);
        out.extend_from_slice(&stsd[..payload + self.pps_end]);
        out[payload + self.pps_count_at] = count;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(pps);
        out.extend_from_slice(&stsd[payload + self.pps_end..]);
        let growth = u32::try_from(pps.len() + 2).map_err(overflow)?;
        for size_at in [self.avcc.start, 8] {
            let size = be_u32(&out, size_at)?
                .checked_add(growth)
                .ok_or(FinalFrameHoldError::Overflow)?;
            put(&mut out, size_at, &size.to_be_bytes())?;
        }
        Ok(out)
    }
}

fn read_parameter_sets(
    avcc: &[u8],
    offset: &mut usize,
    count: usize,
) -> Result<Vec<Vec<u8>>, FinalFrameHoldError> {
    (0..count)
        .map(|_| {
            let len = avcc
                .get(*offset..*offset + 2)
                .map(|len| usize::from(u16::from_be_bytes([len[0], len[1]])))
                .ok_or_else(|| malformed("avcC parameter set"))?;
            let nal = avcc
                .get(*offset + 2..*offset + 2 + len)
                .ok_or_else(|| malformed("avcC parameter set"))?;
            *offset += 2 + len;
            Ok(nal.to_vec())
        })
        .collect()
}

/// Appends one sample's `stts` delta or `ctts` offset, extending the last run when it matches.
fn append_time_entry(table: &mut Vec<u8>, value: i32) -> Result<(), FinalFrameHoldError> {
    let entries = usize::try_from(be_u32(table, 4)?).map_err(overflow)?;
    if table.len() != 8 + entries * 8 {
        return Err(malformed("time table length"));
    }
    let value = value.cast_unsigned();
    if entries > 0 && be_u32(table, table.len() - 4)? == value {
        let count_at = table.len() - 8;
        let count = be_u32(table, count_at)?
            .checked_add(1)
            .ok_or(FinalFrameHoldError::Overflow)?;
        return put(table, count_at, &count.to_be_bytes());
    }
    let entries = u32::try_from(entries + 1).map_err(overflow)?;
    put(table, 4, &entries.to_be_bytes())?;
    table.extend_from_slice(&1_u32.to_be_bytes());
    table.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

fn append_size(stsz: &mut Vec<u8>, size: u32) -> Result<(), FinalFrameHoldError> {
    let (fixed, count) = (be_u32(stsz, 4)?, be_u32(stsz, 8)?);
    let entries = usize::try_from(count).map_err(overflow)?;
    if fixed != 0 && fixed != size {
        return Err(unsupported("constant sample size"));
    }
    if fixed == 0 {
        if stsz.len() != 12 + entries * 4 {
            return Err(malformed("stsz length"));
        }
        stsz.extend_from_slice(&size.to_be_bytes());
    }
    let count = count.checked_add(1).ok_or(FinalFrameHoldError::Overflow)?;
    put(stsz, 8, &count.to_be_bytes())
}

/// Adds a one-sample chunk at `offset`, switching `stco` to `co64` when the offset needs it.
fn append_chunk(stbl: &mut Mp4Box, offset: u64) -> Result<(), FinalFrameHoldError> {
    let stco = box_mut(stbl, *b"stco").is_some();
    let kind = if stco { *b"stco" } else { *b"co64" };
    let item = box_mut(stbl, kind).ok_or_else(|| malformed("missing stco"))?;
    let Body::Leaf(table) = &mut item.body else {
        return Err(malformed("stco"));
    };
    let chunks = be_u32(table, 4)?;
    let entries = usize::try_from(chunks).map_err(overflow)?;
    let width = if stco { 4 } else { 8 };
    if table.len() != 8 + entries * width {
        return Err(malformed("chunk offset table length"));
    }
    match u32::try_from(offset) {
        Ok(offset) if stco => table.extend_from_slice(&offset.to_be_bytes()),
        _ if stco => {
            let mut wide = table[..8].to_vec();
            for chunk in 0..entries {
                wide.extend_from_slice(&u64::from(be_u32(table, 8 + chunk * 4)?).to_be_bytes());
            }
            wide.extend_from_slice(&offset.to_be_bytes());
            *table = wide;
            item.kind = *b"co64";
        }
        _ => table.extend_from_slice(&offset.to_be_bytes()),
    }
    let Body::Leaf(table) = &mut item.body else {
        return Err(malformed("stco"));
    };
    let new_chunk = chunks.checked_add(1).ok_or(FinalFrameHoldError::Overflow)?;
    put(table, 4, &new_chunk.to_be_bytes())?;

    let stsc = leaf_mut(stbl, *b"stsc")?;
    let runs = usize::try_from(be_u32(stsc, 4)?).map_err(overflow)?;
    if runs == 0 || stsc.len() != 8 + runs * 12 {
        return Err(malformed("stsc length"));
    }
    let last = 8 + (runs - 1) * 12;
    if be_u32(stsc, last + 4)? != 1 {
        let description = be_u32(stsc, last + 8)?;
        stsc.extend_from_slice(&new_chunk.to_be_bytes());
        stsc.extend_from_slice(&1_u32.to_be_bytes());
        stsc.extend_from_slice(&description.to_be_bytes());
        let runs = u32::try_from(runs + 1).map_err(overflow)?;
        put(stsc, 4, &runs.to_be_bytes())?;
    }
    Ok(())
}

/// Widens `cslg` to cover the new sample's composition offset and end.
fn extend_composition(cslg: &mut [u8], offset: i64, end: i64) -> Result<(), FinalFrameHoldError> {
    let wide = cslg.first() == Some(&1);
    let read = |cslg: &[u8], field: usize| -> Result<i64, FinalFrameHoldError> {
        if wide {
            be_u64(cslg, 4 + field * 8).map(u64::cast_signed)
        } else {
            be_u32(cslg, 4 + field * 4).map(|value| i64::from(value.cast_signed()))
        }
    };
    let least = read(cslg, 1)?.min(offset);
    let greatest = read(cslg, 2)?.max(offset);
    let composition_end = read(cslg, 4)?.max(end);
    for (field, value) in [(1, least), (2, greatest), (4, composition_end)] {
        if wide {
            put(cslg, 4 + field * 8, &value.to_be_bytes())?;
        } else {
            let value = i32::try_from(value).map_err(overflow)?;
            put(cslg, 4 + field * 4, &value.to_be_bytes())?;
        }
    }
    Ok(())
}

//! Just enough H.264 (ITU-T H.264) to append one skipped picture after a reordered stream.
//!
//! The appended picture is a non-reference P picture whose every macroblock is `P_Skip` against
//! the final presented frame, so it decodes to an exact copy of that frame. It is entropy coded
//! with CAVLC through a picture parameter set of its own, because a CAVLC skip run is a single
//! Exp-Golomb code. Parsing reads only what that needs: the parameter sets, and the slice headers
//! from the last IDR picture on, to rebuild frame numbers, picture order counts and the
//! reference-picture set (clauses 7.3, 7.4 and 8.2).

use super::FinalFrameHoldError;

const NAL_SLICE: u8 = 1;
const NAL_IDR_SLICE: u8 = 5;
const NAL_PPS: u8 = 8;
const SLICE_P: u32 = 0;
const SLICE_B: u32 = 1;
const SLICE_I: u32 = 2;
const SLICE_SP: u32 = 3;
const SLICE_SI: u32 = 4;
const MAX_REFS: u32 = 32;

fn unsupported(what: &'static str) -> FinalFrameHoldError {
    FinalFrameHoldError::Unsupported(what)
}

fn malformed(what: &'static str) -> FinalFrameHoldError {
    FinalFrameHoldError::Malformed(what)
}

/// Reads bits, most significant first, from a raw byte sequence payload.
pub(super) struct BitReader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> BitReader<'a> {
    pub(super) const fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    pub(super) fn bit(&mut self) -> Result<bool, FinalFrameHoldError> {
        let byte = self
            .data
            .get(self.position / 8)
            .ok_or_else(|| malformed("truncated H.264 header"))?;
        let bit = byte >> (7 - self.position % 8) & 1;
        self.position += 1;
        Ok(bit == 1)
    }

    pub(super) fn bits(&mut self, count: u32) -> Result<u32, FinalFrameHoldError> {
        debug_assert!(count <= 32);
        let mut value = 0_u32;
        for _ in 0..count {
            value = value << 1 | u32::from(self.bit()?);
        }
        Ok(value)
    }

    /// An unsigned Exp-Golomb code, `ue(v)` (clause 9.1).
    pub(super) fn ue(&mut self) -> Result<u32, FinalFrameHoldError> {
        let mut leading_zeros = 0;
        while !self.bit()? {
            leading_zeros += 1;
            if leading_zeros > 31 {
                return Err(malformed("Exp-Golomb code"));
            }
        }
        let suffix = self.bits(leading_zeros)?;
        let value = (1_u64 << leading_zeros) - 1 + u64::from(suffix);
        u32::try_from(value).map_err(|_| malformed("Exp-Golomb code"))
    }

    /// A signed Exp-Golomb code, `se(v)` (clause 9.1.1).
    pub(super) fn se(&mut self) -> Result<i32, FinalFrameHoldError> {
        let code = i64::from(self.ue()?);
        let value = if code % 2 == 1 {
            (code + 1) / 2
        } else {
            -(code / 2)
        };
        i32::try_from(value).map_err(|_| malformed("Exp-Golomb code"))
    }

    fn flag(&mut self) -> Result<bool, FinalFrameHoldError> {
        self.bit()
    }
}

/// Writes bits, most significant first.
#[derive(Default)]
pub(super) struct BitWriter {
    bytes: Vec<u8>,
    bits: u32,
}

impl BitWriter {
    pub(super) fn bit(&mut self, bit: bool) {
        if self.bits % 8 == 0 {
            self.bytes.push(0);
        }
        if bit {
            if let Some(last) = self.bytes.last_mut() {
                *last |= 0x80 >> (self.bits % 8);
            }
        }
        self.bits += 1;
    }

    pub(super) fn bits(&mut self, value: u32, count: u32) {
        for shift in (0..count).rev() {
            self.bit(value >> shift & 1 == 1);
        }
    }

    pub(super) fn ue(&mut self, value: u32) {
        let code = u64::from(value) + 1;
        let length = 64 - code.leading_zeros();
        for _ in 1..length {
            self.bit(false);
        }
        for shift in (0..length).rev() {
            self.bit(code >> shift & 1 == 1);
        }
    }

    pub(super) fn se(&mut self, value: i32) {
        let magnitude = value.unsigned_abs();
        let code = if value > 0 {
            2 * magnitude - 1
        } else {
            2 * magnitude
        };
        self.ue(code);
    }

    /// `rbsp_trailing_bits()`: a stop bit, then zeros to the byte boundary.
    pub(super) fn finish(mut self) -> Vec<u8> {
        self.bit(true);
        self.bytes
    }
}

/// Removes emulation prevention bytes from a NAL unit payload (clause 7.4.1).
pub(super) fn unescape(payload: &[u8]) -> Vec<u8> {
    let mut rbsp = Vec::with_capacity(payload.len());
    let mut zeros = 0;
    for &byte in payload {
        if zeros >= 2 && byte == 3 {
            zeros = 0;
            continue;
        }
        zeros = if byte == 0 { zeros + 1 } else { 0 };
        rbsp.push(byte);
    }
    rbsp
}

/// Inserts emulation prevention bytes so no start code appears inside a NAL unit payload.
pub(super) fn escape(rbsp: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(rbsp.len() + rbsp.len() / 64 + 1);
    let mut zeros = 0;
    for &byte in rbsp {
        if zeros >= 2 && byte <= 3 {
            payload.push(3);
            zeros = 0;
        }
        zeros = if byte == 0 { zeros + 1 } else { 0 };
        payload.push(byte);
    }
    payload
}

/// The sequence parameter set fields the skipped picture and slice-header parsing depend on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Sps {
    pub(super) id: u32,
    pub(super) chroma_array_type: u32,
    pub(super) log2_max_frame_num: u32,
    pub(super) log2_max_poc_lsb: u32,
    pub(super) max_num_ref_frames: u32,
    pub(super) pic_size_in_mbs: u32,
}

/// Parses a sequence parameter set NAL unit, header byte included (clause 7.3.2.1.1).
pub(super) fn parse_sps(nal: &[u8]) -> Result<Sps, FinalFrameHoldError> {
    let rbsp = unescape(nal.get(1..).ok_or_else(|| malformed("empty SPS"))?);
    let mut reader = BitReader::new(&rbsp);
    let profile_idc = reader.bits(8)?;
    reader.bits(16)?;
    let id = reader.ue()?;
    let mut chroma_array_type = 1;
    if matches!(
        profile_idc,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        let chroma_format_idc = reader.ue()?;
        if chroma_format_idc == 3 && reader.flag()? {
            return Err(unsupported("separately coded colour planes"));
        }
        chroma_array_type = chroma_format_idc;
        reader.ue()?;
        reader.ue()?;
        reader.flag()?;
        if reader.flag()? {
            let lists = if chroma_format_idc == 3 { 12 } else { 8 };
            for list in 0..lists {
                if reader.flag()? {
                    skip_scaling_list(&mut reader, if list < 6 { 16 } else { 64 })?;
                }
            }
        }
    }
    let log2_max_frame_num = reader.ue()? + 4;
    let log2_max_poc_lsb = match reader.ue()? {
        0 => reader.ue()? + 4,
        1 => return Err(unsupported("picture order count type 1")),
        2 => return Err(unsupported("picture order count type 2")),
        _ => return Err(malformed("picture order count type")),
    };
    if log2_max_frame_num > 16 || log2_max_poc_lsb > 16 {
        return Err(malformed("SPS field range"));
    }
    let max_num_ref_frames = reader.ue()?;
    if max_num_ref_frames > MAX_REFS {
        return Err(malformed("SPS reference frame count"));
    }
    reader.flag()?;
    let width_in_mbs = reader.ue()?.checked_add(1);
    let height_in_map_units = reader.ue()?.checked_add(1);
    if !reader.flag()? {
        return Err(unsupported("interlaced video"));
    }
    let pic_size_in_mbs = width_in_mbs
        .zip(height_in_map_units)
        .and_then(|(width, height)| width.checked_mul(height))
        .ok_or(FinalFrameHoldError::Overflow)?;
    Ok(Sps {
        id,
        chroma_array_type,
        log2_max_frame_num,
        log2_max_poc_lsb,
        max_num_ref_frames,
        pic_size_in_mbs,
    })
}

fn skip_scaling_list(reader: &mut BitReader<'_>, size: usize) -> Result<(), FinalFrameHoldError> {
    let mut last_scale = 8_i32;
    let mut next_scale = 8_i32;
    for _ in 0..size {
        if next_scale != 0 {
            next_scale = (last_scale + reader.se()?).rem_euclid(256);
        }
        if next_scale != 0 {
            last_scale = next_scale;
        }
    }
    Ok(())
}

/// The picture parameter set fields a slice header's syntax depends on (clause 7.3.2.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Pps {
    pub(super) id: u32,
    pub(super) sps_id: u32,
    bottom_field_pic_order_in_frame_present: bool,
    num_ref_idx_default_active: [u32; 2],
    weighted_pred: bool,
    weighted_bipred_idc: u32,
    redundant_pic_cnt_present: bool,
}

pub(super) fn parse_pps(nal: &[u8]) -> Result<Pps, FinalFrameHoldError> {
    let rbsp = unescape(nal.get(1..).ok_or_else(|| malformed("empty PPS"))?);
    let mut reader = BitReader::new(&rbsp);
    let id = reader.ue()?;
    let sps_id = reader.ue()?;
    reader.flag()?;
    let bottom_field_pic_order_in_frame_present = reader.flag()?;
    if reader.ue()? != 0 {
        return Err(unsupported("slice groups"));
    }
    let num_ref_idx_default_active = [reader.ue()? + 1, reader.ue()? + 1];
    if num_ref_idx_default_active
        .iter()
        .any(|&count| count > MAX_REFS)
    {
        return Err(malformed("PPS reference index count"));
    }
    let weighted_pred = reader.flag()?;
    let weighted_bipred_idc = reader.bits(2)?;
    reader.se()?;
    reader.se()?;
    reader.se()?;
    reader.flag()?;
    reader.flag()?;
    let redundant_pic_cnt_present = reader.flag()?;
    Ok(Pps {
        id,
        sps_id,
        bottom_field_pic_order_in_frame_present,
        num_ref_idx_default_active,
        weighted_pred,
        weighted_bipred_idc,
        redundant_pic_cnt_present,
    })
}

/// The parsed parameter sets of one sample description.
pub(super) struct ParameterSets {
    pub(super) sps: Vec<Sps>,
    pub(super) pps: Vec<Pps>,
}

impl ParameterSets {
    fn pps(&self, id: u32) -> Result<&Pps, FinalFrameHoldError> {
        self.pps
            .iter()
            .find(|pps| pps.id == id)
            .ok_or_else(|| malformed("slice names a missing PPS"))
    }

    fn sps(&self, id: u32) -> Result<&Sps, FinalFrameHoldError> {
        self.sps
            .iter()
            .find(|sps| sps.id == id)
            .ok_or_else(|| malformed("PPS names a missing SPS"))
    }

    /// The lowest picture parameter set id no existing set uses.
    pub(super) fn unused_pps_id(&self) -> Result<u32, FinalFrameHoldError> {
        (0..=255)
            .find(|id| self.pps.iter().all(|pps| pps.id != *id))
            .ok_or_else(|| unsupported("no free picture parameter set id"))
    }
}

/// How a reference picture updates the set of reference pictures (clause 8.2.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Marking {
    NonReference,
    Idr {
        long_term: bool,
    },
    SlidingWindow,
    /// `memory_management_control_operation` commands; only operation 1 (unmark one short-term
    /// picture) is kept by value, any other operation is `None`.
    Adaptive(Vec<Option<u32>>),
}

/// The fields of a picture's first slice header that frame numbering and ordering depend on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SliceHeader {
    pub(super) idr: bool,
    pub(super) pps_id: u32,
    pub(super) frame_num: u32,
    pub(super) poc_lsb: u32,
    pub(super) delta_poc_bottom: i32,
    pub(super) marking: Marking,
}

/// Parses a slice header from a slice NAL unit, header byte included (clause 7.3.3).
pub(super) fn parse_slice_header(
    nal: &[u8],
    parameters: &ParameterSets,
) -> Result<SliceHeader, FinalFrameHoldError> {
    let header = *nal.first().ok_or_else(|| malformed("empty NAL unit"))?;
    let nal_ref_idc = header >> 5 & 3;
    let idr = header & 0x1f == NAL_IDR_SLICE;
    let rbsp = unescape(&nal[1..]);
    let mut reader = BitReader::new(&rbsp);
    reader.ue()?;
    let slice_type = reader.ue()?;
    if slice_type > 9 {
        return Err(malformed("slice type"));
    }
    let slice_type = slice_type % 5;
    let pps = parameters.pps(reader.ue()?)?;
    let sps = parameters.sps(pps.sps_id)?;
    let frame_num = reader.bits(sps.log2_max_frame_num)?;
    if idr {
        reader.ue()?;
    }
    let poc_lsb = reader.bits(sps.log2_max_poc_lsb)?;
    let delta_poc_bottom = if pps.bottom_field_pic_order_in_frame_present {
        reader.se()?
    } else {
        0
    };
    if pps.redundant_pic_cnt_present && reader.ue()? != 0 {
        return Err(unsupported("redundant pictures"));
    }
    if slice_type == SLICE_B {
        reader.flag()?;
    }
    let mut active = pps.num_ref_idx_default_active;
    if matches!(slice_type, SLICE_P | SLICE_SP | SLICE_B) && reader.flag()? {
        active[0] = reader.ue()? + 1;
        if slice_type == SLICE_B {
            active[1] = reader.ue()? + 1;
        }
        if active.iter().any(|&count| count > MAX_REFS) {
            return Err(malformed("slice reference index count"));
        }
    }
    if !matches!(slice_type, SLICE_I | SLICE_SI) {
        skip_ref_pic_list_modification(&mut reader)?;
        if slice_type == SLICE_B {
            skip_ref_pic_list_modification(&mut reader)?;
        }
    }
    if (pps.weighted_pred && matches!(slice_type, SLICE_P | SLICE_SP))
        || (pps.weighted_bipred_idc == 1 && slice_type == SLICE_B)
    {
        let lists = if slice_type == SLICE_B { 2 } else { 1 };
        skip_pred_weight_table(&mut reader, sps.chroma_array_type, &active[..lists])?;
    }
    let marking = if nal_ref_idc == 0 {
        Marking::NonReference
    } else if idr {
        reader.flag()?;
        Marking::Idr {
            long_term: reader.flag()?,
        }
    } else if reader.flag()? {
        let mut operations = Vec::new();
        loop {
            match reader.ue()? {
                0 => break,
                1 => operations.push(Some(reader.ue()?)),
                2 | 4 | 6 => {
                    reader.ue()?;
                    operations.push(None);
                }
                3 => {
                    reader.ue()?;
                    reader.ue()?;
                    operations.push(None);
                }
                5 => operations.push(None),
                _ => return Err(malformed("memory management operation")),
            }
            if operations.len() > 2 * MAX_REFS as usize + 2 {
                return Err(malformed("memory management operations"));
            }
        }
        Marking::Adaptive(operations)
    } else {
        Marking::SlidingWindow
    };
    Ok(SliceHeader {
        idr,
        pps_id: pps.id,
        frame_num,
        poc_lsb,
        delta_poc_bottom,
        marking,
    })
}

fn skip_ref_pic_list_modification(reader: &mut BitReader<'_>) -> Result<(), FinalFrameHoldError> {
    if !reader.flag()? {
        return Ok(());
    }
    for _ in 0..=MAX_REFS {
        match reader.ue()? {
            0..=2 => {
                reader.ue()?;
            }
            3 => return Ok(()),
            _ => return Err(malformed("reference list modification")),
        }
    }
    Err(malformed("reference list modification"))
}

fn skip_pred_weight_table(
    reader: &mut BitReader<'_>,
    chroma_array_type: u32,
    active: &[u32],
) -> Result<(), FinalFrameHoldError> {
    reader.ue()?;
    if chroma_array_type != 0 {
        reader.ue()?;
    }
    for &count in active {
        for _ in 0..count {
            if reader.flag()? {
                reader.se()?;
                reader.se()?;
            }
            if chroma_array_type != 0 && reader.flag()? {
                for _ in 0..4 {
                    reader.se()?;
                }
            }
        }
    }
    Ok(())
}

/// Finds the first slice NAL unit among a sample's length-prefixed NAL units.
pub(super) fn first_slice(sample: &[u8], length_size: usize) -> Result<&[u8], FinalFrameHoldError> {
    let mut offset = 0;
    while offset < sample.len() {
        let length = sample
            .get(offset..offset + length_size)
            .ok_or_else(|| malformed("truncated NAL unit length"))?
            .iter()
            .fold(0_usize, |length, &byte| length << 8 | usize::from(byte));
        offset += length_size;
        let end = offset
            .checked_add(length)
            .filter(|&end| end <= sample.len())
            .ok_or_else(|| malformed("truncated NAL unit"))?;
        let nal = &sample[offset..end];
        if let Some(&header) = nal.first() {
            if matches!(header & 0x1f, NAL_SLICE | NAL_IDR_SLICE) {
                return Ok(nal);
            }
        }
        offset = end;
    }
    Err(malformed("sample has no slice"))
}

/// The picture to append: its numbering and the reference it copies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SkipPicture {
    pub(super) frame_num: u32,
    pub(super) poc_lsb: u32,
    /// `abs_diff_pic_num_minus1` that moves the copied frame to the head of reference list 0.
    pub(super) abs_diff_pic_num_minus1: u32,
}

/// Replays reference marking and picture order counts from an IDR picture through the end of
/// the stream (`pictures`, in decode order), and plans a skipped picture that copies
/// `pictures[copied]` and is output after every other picture.
pub(super) fn plan_skip_picture(
    pictures: &[SliceHeader],
    copied: usize,
    sps: &Sps,
) -> Result<SkipPicture, FinalFrameHoldError> {
    let max_frame_num = 1_u32 << sps.log2_max_frame_num;
    let max_poc_lsb = 1_i64 << sps.log2_max_poc_lsb;
    let wrap = |frame_num: u32, current: u32| {
        if frame_num > current {
            i64::from(frame_num) - i64::from(max_frame_num)
        } else {
            i64::from(frame_num)
        }
    };
    if !pictures.first().is_some_and(|picture| picture.idr) {
        return Err(unsupported(
            "trailing pictures do not start at an IDR picture",
        ));
    }
    let mut references: Vec<u32> = Vec::new();
    let (mut prev_frame_num, mut prev_msb, mut prev_lsb) = (0_u32, 0_i64, 0_i64);
    let mut copied_poc = None;
    let mut max_other_poc = None::<i64>;
    for (index, header) in pictures.iter().enumerate() {
        if header.idr {
            if header.frame_num != 0 {
                return Err(malformed("IDR frame number"));
            }
            (prev_msb, prev_lsb) = (0, 0);
        } else if header.frame_num != (prev_frame_num + 1) % max_frame_num {
            return Err(unsupported("frame numbers out of sequence"));
        }
        let lsb = i64::from(header.poc_lsb);
        let msb = poc_msb(prev_msb, prev_lsb, lsb, max_poc_lsb);
        let top = msb + lsb;
        let poc = top.min(top + i64::from(header.delta_poc_bottom));
        if index == copied {
            copied_poc = Some(poc);
        } else {
            max_other_poc = Some(max_other_poc.map_or(poc, |max| max.max(poc)));
        }
        match &header.marking {
            Marking::NonReference => continue,
            Marking::Idr { long_term: true } => {
                return Err(unsupported("long-term reference pictures"));
            }
            Marking::Idr { long_term: false } => references.clear(),
            Marking::SlidingWindow => {
                if references.len() >= sps.max_num_ref_frames.max(1) as usize {
                    let oldest = references
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, frame_num)| wrap(**frame_num, header.frame_num))
                        .map(|(position, _)| position);
                    if let Some(oldest) = oldest {
                        references.remove(oldest);
                    }
                }
            }
            Marking::Adaptive(operations) => {
                for operation in operations {
                    let difference = operation.ok_or_else(|| {
                        unsupported("memory management operation other than unmarking")
                    })?;
                    let pic_num = i64::from(header.frame_num) - i64::from(difference) - 1;
                    let position = references
                        .iter()
                        .position(|&frame_num| wrap(frame_num, header.frame_num) == pic_num)
                        .ok_or_else(|| malformed("unmarks a missing reference picture"))?;
                    references.remove(position);
                }
            }
        }
        if references.len() >= sps.max_num_ref_frames.max(1) as usize {
            return Err(malformed("too many reference pictures"));
        }
        references.push(header.frame_num);
        (prev_frame_num, prev_msb, prev_lsb) = (header.frame_num, msb, lsb);
    }

    let copied_header = pictures
        .get(copied)
        .ok_or_else(|| malformed("copied picture"))?;
    let copied_poc = copied_poc.ok_or_else(|| malformed("copied picture"))?;
    if max_other_poc.is_some_and(|max| max >= copied_poc) {
        return Err(unsupported(
            "presentation order disagrees with picture order count",
        ));
    }
    if copied_header.marking == Marking::NonReference
        || !references.contains(&copied_header.frame_num)
    {
        return Err(unsupported("final frame is not a reference picture"));
    }
    let frame_num = (prev_frame_num + 1) % max_frame_num;
    let poc = copied_poc + 2;
    let lsb = poc.rem_euclid(max_poc_lsb);
    if poc_msb(prev_msb, prev_lsb, lsb, max_poc_lsb) + lsb != poc {
        return Err(unsupported("picture order count out of range"));
    }
    let difference = i64::from(frame_num) - wrap(copied_header.frame_num, frame_num);
    if difference < 1 || difference > i64::from(max_frame_num) {
        return Err(malformed("reference picture number"));
    }
    Ok(SkipPicture {
        frame_num,
        poc_lsb: u32::try_from(lsb).map_err(|_| FinalFrameHoldError::Overflow)?,
        abs_diff_pic_num_minus1: u32::try_from(difference - 1)
            .map_err(|_| FinalFrameHoldError::Overflow)?,
    })
}

/// `PicOrderCntMsb` for picture order count type 0 (clause 8.2.1.1).
fn poc_msb(prev_msb: i64, prev_lsb: i64, lsb: i64, max_lsb: i64) -> i64 {
    if lsb < prev_lsb && prev_lsb - lsb >= max_lsb / 2 {
        prev_msb + max_lsb
    } else if lsb > prev_lsb && lsb - prev_lsb > max_lsb / 2 {
        prev_msb - max_lsb
    } else {
        prev_msb
    }
}

/// A CAVLC picture parameter set for the skipped picture: one reference, no weighting, and
/// deblocking control present so the slice can turn the filter off.
pub(super) fn skip_pps(id: u32, sps_id: u32) -> Vec<u8> {
    let mut bits = BitWriter::default();
    bits.ue(id);
    bits.ue(sps_id);
    bits.bit(false); // entropy_coding_mode_flag: CAVLC
    bits.bit(false); // bottom_field_pic_order_in_frame_present_flag
    bits.ue(0); // num_slice_groups_minus1
    bits.ue(0); // num_ref_idx_l0_default_active_minus1
    bits.ue(0); // num_ref_idx_l1_default_active_minus1
    bits.bit(false); // weighted_pred_flag
    bits.bits(0, 2); // weighted_bipred_idc
    bits.se(0); // pic_init_qp_minus26
    bits.se(0); // pic_init_qs_minus26
    bits.se(0); // chroma_qp_index_offset
    bits.bit(true); // deblocking_filter_control_present_flag
    bits.bit(false); // constrained_intra_pred_flag
    bits.bit(false); // redundant_pic_cnt_present_flag
    nal(3, NAL_PPS, &bits.finish())
}

/// A non-reference P slice that skips every macroblock, copying the picture at the head of
/// reference list 0 with zero motion and no residual.
pub(super) fn skip_slice(sps: &Sps, pps_id: u32, picture: SkipPicture) -> Vec<u8> {
    let mut bits = BitWriter::default();
    bits.ue(0); // first_mb_in_slice
    bits.ue(SLICE_P + 5); // slice_type: every slice of the picture is P
    bits.ue(pps_id);
    bits.bits(picture.frame_num, sps.log2_max_frame_num);
    bits.bits(picture.poc_lsb, sps.log2_max_poc_lsb);
    bits.bit(false); // num_ref_idx_active_override_flag
    bits.bit(true); // ref_pic_list_modification_flag_l0
    bits.ue(0); // modification_of_pic_nums_idc: subtract from the predicted picture number
    bits.ue(picture.abs_diff_pic_num_minus1);
    bits.ue(3); // end of modifications
    bits.se(0); // slice_qp_delta
    bits.ue(1); // disable_deblocking_filter_idc
    bits.ue(sps.pic_size_in_mbs); // mb_skip_run: the whole picture
    nal(0, NAL_SLICE, &bits.finish())
}

fn nal(ref_idc: u8, kind: u8, rbsp: &[u8]) -> Vec<u8> {
    let mut nal = vec![ref_idc << 5 | kind];
    nal.extend(escape(rbsp));
    nal
}

#[cfg(test)]
mod tests;

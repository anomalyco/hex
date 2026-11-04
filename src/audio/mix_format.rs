//! The shared-mode mix format a WASAPI endpoint reports, read from its raw
//! `WAVEFORMATEX` bytes, and decoding of its frames to mono `f32`.

use std::fmt;

const WAVE_FORMAT_PCM: u16 = 0x0001;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
/// The bytes after `Data1` shared by the KSDATAFORMAT_SUBTYPE GUIDs.
const SUBTYPE_TAIL: [u8; 12] = [
    0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];

/// A `WAVEFORMATEXTENSIBLE` for 32-bit float audio, as raw bytes.
pub(super) fn float_format_bytes(channels: u16, sample_rate: u32, channel_mask: u32) -> Vec<u8> {
    let block_align = channels * 4;
    let mut bytes = Vec::with_capacity(40);
    bytes.extend(WAVE_FORMAT_EXTENSIBLE.to_le_bytes());
    bytes.extend(channels.to_le_bytes());
    bytes.extend(sample_rate.to_le_bytes());
    bytes.extend((sample_rate * u32::from(block_align)).to_le_bytes());
    bytes.extend(block_align.to_le_bytes());
    bytes.extend(32_u16.to_le_bytes());
    bytes.extend(22_u16.to_le_bytes());
    bytes.extend(32_u16.to_le_bytes());
    bytes.extend(channel_mask.to_le_bytes());
    bytes.extend(u32::from(WAVE_FORMAT_IEEE_FLOAT).to_le_bytes());
    bytes.extend(SUBTYPE_TAIL);
    bytes
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Sample {
    Float,
    Int,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MixFormat {
    pub tag: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub block_align: u16,
    pub bits_per_sample: u16,
    pub valid_bits: u16,
    pub channel_mask: u32,
    /// `None` when the format tag or subformat is not PCM or IEEE float.
    pub sample: Option<Sample>,
}

impl MixFormat {
    /// Reads a `WAVEFORMATEX`, and its `WAVEFORMATEXTENSIBLE` tail when the
    /// tag says so, from `bytes` (little-endian, `18 + cbSize` long).
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let u16_at = |at: usize| Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?));
        let u32_at = |at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
        let tag = u16_at(0)?;
        let bits_per_sample = u16_at(14)?;
        let mut format = Self {
            tag,
            channels: u16_at(2)?,
            sample_rate: u32_at(4)?,
            block_align: u16_at(12)?,
            bits_per_sample,
            valid_bits: bits_per_sample,
            channel_mask: 0,
            sample: match tag {
                WAVE_FORMAT_PCM => Some(Sample::Int),
                WAVE_FORMAT_IEEE_FLOAT => Some(Sample::Float),
                _ => None,
            },
        };
        if tag == WAVE_FORMAT_EXTENSIBLE && u16_at(16)? >= 22 {
            format.valid_bits = u16_at(18)?;
            format.channel_mask = u32_at(20)?;
            let subformat = bytes.get(24..40)?;
            format.sample = if subformat[4..] != SUBTYPE_TAIL {
                None
            } else {
                match u32_at(24)? {
                    1 => Some(Sample::Int),
                    3 => Some(Sample::Float),
                    _ => None,
                }
            };
        }
        Some(format)
    }

    /// Bytes per sample, when Hex can decode this format.
    fn sample_bytes(&self) -> Option<usize> {
        let bytes = usize::from(self.bits_per_sample / 8);
        let decodable = match self.sample? {
            Sample::Float => matches!(bytes, 4 | 8),
            Sample::Int => matches!(bytes, 2..=4),
        };
        (decodable
            && self.channels > 0
            && usize::from(self.block_align) == bytes * usize::from(self.channels))
        .then_some(bytes)
    }

    pub fn is_decodable(&self) -> bool {
        self.sample_bytes().is_some()
    }

    /// Averages each whole frame in `bytes` to one mono sample.
    pub fn decode_mono(&self, bytes: &[u8]) -> Vec<f32> {
        let Some(width) = self.sample_bytes() else {
            return Vec::new();
        };
        let channels = usize::from(self.channels);
        let decode = |sample: &[u8]| -> f32 {
            match (self.sample, width) {
                (Some(Sample::Float), 4) => f32::from_le_bytes(sample.try_into().unwrap_or([0; 4])),
                (Some(Sample::Float), _) => {
                    f64::from_le_bytes(sample.try_into().unwrap_or([0; 8])) as f32
                }
                (_, 2) => f32::from(i16::from_le_bytes([sample[0], sample[1]])) / 32_768.0,
                // Packed 24-bit and 32-bit containers keep the value in the
                // high bits, so both scale from a full 32-bit word.
                (_, 3) => {
                    i32::from_le_bytes([0, sample[0], sample[1], sample[2]]) as f32
                        / 2_147_483_648.0
                }
                _ => {
                    i32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]) as f32
                        / 2_147_483_648.0
                }
            }
        };
        bytes
            .chunks_exact(width * channels)
            .map(|frame| frame.chunks_exact(width).map(decode).sum::<f32>() / channels as f32)
            .collect()
    }
}

impl fmt::Display for MixFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sample = match self.sample {
            Some(Sample::Float) => "float",
            Some(Sample::Int) => "int",
            None => "unknown",
        };
        write!(
            f,
            "{} channels, {} Hz, {}-bit {sample} (tag {:#06x}, valid {} bits, block {}, mask {:#x})",
            self.channels,
            self.sample_rate,
            self.bits_per_sample,
            self.tag,
            self.valid_bits,
            self.block_align,
            self.channel_mask,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mix format the Intel Smart Sound microphone reported in #988.
    fn intel_smart_sound() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(0xFFFE_u16.to_le_bytes());
        bytes.extend(2_u16.to_le_bytes());
        bytes.extend(48_000_u32.to_le_bytes());
        bytes.extend(384_000_u32.to_le_bytes());
        bytes.extend(8_u16.to_le_bytes());
        bytes.extend(32_u16.to_le_bytes());
        bytes.extend(22_u16.to_le_bytes());
        bytes.extend(32_u16.to_le_bytes());
        bytes.extend(3_u32.to_le_bytes());
        bytes.extend(3_u32.to_le_bytes());
        bytes.extend(SUBTYPE_TAIL);
        bytes
    }

    #[test]
    fn reads_an_extensible_float_mix_format() {
        let format = MixFormat::parse(&intel_smart_sound()).unwrap();
        assert_eq!(
            format,
            MixFormat {
                tag: WAVE_FORMAT_EXTENSIBLE,
                channels: 2,
                sample_rate: 48_000,
                block_align: 8,
                bits_per_sample: 32,
                valid_bits: 32,
                channel_mask: 3,
                sample: Some(Sample::Float),
            }
        );
        assert!(format.is_decodable());
        assert_eq!(
            format.to_string(),
            "2 channels, 48000 Hz, 32-bit float (tag 0xfffe, valid 32 bits, block 8, mask 0x3)"
        );
    }

    #[test]
    fn builds_float_formats_byte_for_byte() {
        assert_eq!(float_format_bytes(2, 48_000, 3), intel_smart_sound());
        let mono = MixFormat::parse(&float_format_bytes(1, 16_000, 4)).unwrap();
        assert_eq!(
            mono.to_string(),
            "1 channels, 16000 Hz, 32-bit float (tag 0xfffe, valid 32 bits, block 4, mask 0x4)"
        );
        assert!(mono.is_decodable());
    }

    #[test]
    fn rejects_unknown_subformats_and_truncated_bytes() {
        let mut bytes = intel_smart_sound();
        bytes[24] = 0x92; // KSDATAFORMAT_SUBTYPE_DOLBY_AC3_SPDIF
        assert!(!MixFormat::parse(&bytes).unwrap().is_decodable());
        assert_eq!(MixFormat::parse(&bytes[..10]), None);
        assert_eq!(MixFormat::parse(&bytes[..30]), None);
    }

    #[test]
    fn averages_float_frames_to_mono() {
        let format = MixFormat::parse(&intel_smart_sound()).unwrap();
        let bytes: Vec<u8> = [0.5_f32, 0.25, -1.0, 1.0]
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        assert_eq!(format.decode_mono(&bytes), vec![0.375, 0.0]);
    }

    #[test]
    fn decodes_integer_mix_formats() {
        let pcm = |bits: u16, channels: u16| MixFormat {
            tag: WAVE_FORMAT_PCM,
            channels,
            sample_rate: 44_100,
            block_align: bits / 8 * channels,
            bits_per_sample: bits,
            valid_bits: bits,
            channel_mask: 0,
            sample: Some(Sample::Int),
        };
        let sixteen: Vec<u8> = [16_384_i16, -32_768]
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        assert_eq!(pcm(16, 1).decode_mono(&sixteen), vec![0.5, -1.0]);
        // 0x400000 is half of 24-bit full scale.
        assert_eq!(pcm(24, 1).decode_mono(&[0x00, 0x00, 0x40]), vec![0.5]);
        let thirty_two = (i32::MIN / 2).to_le_bytes();
        assert_eq!(pcm(32, 1).decode_mono(&thirty_two), vec![-0.5]);
        // A partial trailing frame is dropped, not misread.
        assert_eq!(pcm(16, 2).decode_mono(&sixteen[..3]), Vec::<f32>::new());
    }

    #[test]
    fn refuses_a_block_align_that_does_not_match_the_channels() {
        let mut format = MixFormat::parse(&intel_smart_sound()).unwrap();
        format.block_align = 6;
        assert!(!format.is_decodable());
        assert!(format.decode_mono(&[0; 12]).is_empty());
    }
}

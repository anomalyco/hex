//! Compressed and non-WAV audio through Symphonia: the container's audio
//! track decoded a packet at a time and averaged to mono at its own rate.
//! Opus, which Symphonia 0.5 has no decoder for, goes through `opuscule`
//! (as Rune's foreign-audio player decodes it).

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{
    CODEC_TYPE_AAC, CODEC_TYPE_NULL, CODEC_TYPE_OPUS, CodecParameters, DecoderOptions,
};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::PieceError;

/// Damaged packets skipped in a row before the file counts as damaged; a
/// player skips one, but a run of them is not audio worth transcribing.
const MAX_SKIPPED_PACKETS: usize = 16;

pub(super) struct Media {
    format: Box<dyn FormatReader>,
    track: u32,
    decoder: Decoder,
    /// The decoded rate, once the first packet decodes.
    rate: Option<u32>,
    /// Frames the container says the track holds, for the duration.
    n_frames: Option<u64>,
    /// Frames still to hand out when the container says where the audio
    /// ends (Ogg Opus, whose last granule counts the pre-skip).
    remaining: Option<u64>,
}

enum Decoder {
    Symphonia {
        decoder: Box<dyn symphonia::core::codecs::Decoder>,
        buffer: Option<SampleBuffer<f32>>,
    },
    Opus(Box<Opus>),
}

impl Media {
    /// Probes the container and picks its audio: the default track when it
    /// decodes, else the first that does. Video and other tracks are
    /// skipped.
    pub(super) fn open(file: File, path: &Path) -> Result<Self, PieceError> {
        let ogg = {
            use std::io::Read as _;
            let mut magic = [0_u8; 4];
            (&file).read_exact(&mut magic).is_ok() && &magic == b"OggS"
        };
        let file = {
            use std::io::Seek as _;
            let mut file = file;
            file.rewind().map_err(PieceError::Io)?;
            file
        };
        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
            hint.with_extension(extension);
        }
        let stream = MediaSourceStream::new(Box::new(file), Default::default());
        let options = FormatOptions {
            enable_gapless: true,
            ..Default::default()
        };
        let format = match symphonia::default::get_probe().format(
            &hint,
            stream,
            &options,
            &MetadataOptions::default(),
        ) {
            Ok(probed) => probed.format,
            Err(Error::IoError(error)) if error.kind() != std::io::ErrorKind::UnexpectedEof => {
                return Err(PieceError::Io(error));
            }
            Err(_) => return Err(PieceError::NotMedia),
        };
        let (track, params, decoder) = choose(format.as_ref())?;
        let remaining = match (&decoder, params.n_frames, ogg) {
            (Decoder::Opus(opus), Some(end), true) => Some(end.saturating_sub(opus.pre_skip)),
            _ => None,
        };
        Ok(Self {
            format,
            track,
            decoder,
            rate: None,
            n_frames: remaining.or(params.n_frames),
            remaining,
        })
    }

    /// The decoded sample rate; known once [`Self::read`] returned audio.
    pub(super) fn rate(&self) -> Option<u32> {
        self.rate
    }

    /// The track's length, when its container says.
    pub(super) fn duration_ms(&self) -> Option<u64> {
        let frames = self.n_frames?;
        let rate = u64::from(self.rate?);
        Some(frames * 1_000 / rate)
    }

    /// The next decoded block of mono samples, empty at the end. A damaged
    /// packet is skipped as a player would; a run of them, or a container
    /// that stops reading, is [`PieceError::Damaged`] (the caller fills in
    /// where).
    pub(super) fn read(&mut self) -> Result<Vec<f32>, PieceError> {
        let mut skipped = 0;
        loop {
            if self.remaining == Some(0) {
                return Ok(Vec::new());
            }
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(Error::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(Vec::new());
                }
                Err(_) => return Err(PieceError::Damaged { at_ms: 0 }),
            };
            if packet.track_id() != self.track {
                continue;
            }
            let decoded = match &mut self.decoder {
                Decoder::Symphonia { decoder, buffer } => match decoder.decode(&packet) {
                    Ok(audio) => {
                        let spec = *audio.spec();
                        let channels = spec.channels.count();
                        let frames = audio.frames();
                        if buffer
                            .as_ref()
                            .is_none_or(|buffer| buffer.capacity() < frames * channels)
                        {
                            *buffer = Some(SampleBuffer::new(audio.capacity() as u64, spec));
                        }
                        let buffer = buffer.as_mut().expect("the buffer was just made");
                        buffer.copy_interleaved_ref(audio);
                        Some((spec.rate, mono(buffer.samples(), channels)?))
                    }
                    Err(Error::DecodeError(_)) => None,
                    Err(_) => return Err(PieceError::Damaged { at_ms: 0 }),
                },
                Decoder::Opus(opus) => opus.decode(&packet.data)?.map(|samples| (48_000, samples)),
            };
            let Some((rate, mut samples)) = decoded else {
                skipped += 1;
                if skipped >= MAX_SKIPPED_PACKETS {
                    return Err(PieceError::Damaged { at_ms: 0 });
                }
                continue;
            };
            skipped = 0;
            match self.rate {
                None => self.rate = Some(rate),
                Some(known) if known != rate => {
                    return Err(PieceError::UnsupportedCodec(
                        "audio whose sample rate changes partway".into(),
                    ));
                }
                Some(_) => {}
            }
            if let Some(remaining) = &mut self.remaining {
                let keep = samples
                    .len()
                    .min(usize::try_from(*remaining).unwrap_or(usize::MAX));
                samples.truncate(keep);
                *remaining -= keep as u64;
            }
            if !samples.is_empty() {
                return Ok(samples);
            }
        }
    }
}

/// The track to decode and its decoder, or why there is none: no audio track
/// at all, or only audio in codecs this build can't decode.
fn choose(format: &dyn FormatReader) -> Result<(u32, CodecParameters, Decoder), PieceError> {
    let default = format.default_track().map(|track| track.id);
    let tracks = format.tracks();
    let ordered = tracks
        .iter()
        .filter(|track| Some(track.id) == default)
        .chain(tracks.iter().filter(|track| Some(track.id) != default));
    let mut undecodable = None;
    for track in ordered {
        let params = &track.codec_params;
        if params.codec == CODEC_TYPE_NULL {
            // A video or data track, or audio a container can't describe.
            if params.sample_rate.is_some() || params.channels.is_some() {
                undecodable.get_or_insert_with(|| "an unrecognized codec".to_owned());
            }
            continue;
        }
        let decoder = if params.codec == CODEC_TYPE_OPUS {
            Opus::new(params.extra_data.as_deref().unwrap_or_default())
                .map(|opus| Decoder::Opus(Box::new(opus)))
        } else {
            let params = &aac_config(params);
            symphonia::default::get_codecs()
                .make(params, &DecoderOptions::default())
                .map(|decoder| Decoder::Symphonia {
                    decoder,
                    buffer: None,
                })
                .map_err(|_| codec_name(params))
        };
        match decoder {
            Ok(decoder) => return Ok((track.id, params.clone(), decoder)),
            Err(name) => {
                undecodable.get_or_insert(name);
            }
        }
    }
    Err(match undecodable {
        Some(name) => PieceError::UnsupportedCodec(name),
        None => PieceError::NoAudioTrack,
    })
}

/// AAC's decoder takes the AudioSpecificConfig, but CAF's magic cookie (as
/// written by Apple's encoders) wraps it in an MPEG-4 ES descriptor; unwrap
/// it, leaving any other codec's parameters as they are.
fn aac_config(params: &CodecParameters) -> CodecParameters {
    let mut params = params.clone();
    if params.codec == CODEC_TYPE_AAC
        && let Some(config) = params.extra_data.as_deref().and_then(es_decoder_config)
    {
        params.extra_data = Some(config.into());
    }
    params
}

/// The DecoderSpecificInfo inside an ES_Descriptor (ISO/IEC 14496-1 §7.2.6.5),
/// or `None` when `data` isn't one.
fn es_decoder_config(data: &[u8]) -> Option<Vec<u8>> {
    /// One descriptor's tag and body, and what follows it.
    fn descriptor(data: &[u8]) -> Option<(u8, &[u8], &[u8])> {
        let (&tag, mut rest) = data.split_first()?;
        let mut length = 0_usize;
        for _ in 0..4 {
            let (&byte, after) = rest.split_first()?;
            rest = after;
            length = length << 7 | usize::from(byte & 0x7f);
            if byte & 0x80 == 0 {
                break;
            }
        }
        (rest.len() >= length).then(|| (tag, &rest[..length], &rest[length..]))
    }
    let (tag, es, _) = descriptor(data)?;
    if tag != 0x03 || es.len() < 3 {
        return None;
    }
    let flags = es[2];
    let mut body = &es[3..];
    if flags & 0x80 != 0 {
        body = body.get(2..)?;
    }
    if flags & 0x40 != 0 {
        let length = usize::from(*body.first()?);
        body = body.get(1 + length..)?;
    }
    if flags & 0x20 != 0 {
        body = body.get(2..)?;
    }
    while !body.is_empty() {
        let (tag, config, rest) = descriptor(body)?;
        if tag == 0x04 {
            // objectTypeIndication, streamType, bufferSizeDB, maxBitrate,
            // avgBitrate, then the nested descriptors.
            let mut nested = config.get(13..)?;
            while !nested.is_empty() {
                let (tag, info, rest) = descriptor(nested)?;
                if tag == 0x05 {
                    return Some(info.to_vec());
                }
                nested = rest;
            }
            return None;
        }
        body = rest;
    }
    None
}

fn codec_name(params: &CodecParameters) -> String {
    symphonia::default::get_codecs()
        .get_codec(params.codec)
        .map_or_else(
            || format!("codec {}", params.codec),
            |codec| codec.long_name.to_owned(),
        )
}

/// Interleaved samples averaged to mono, refusing a sample that is not a
/// number, as the WAV reader does.
fn mono(interleaved: &[f32], channels: usize) -> Result<Vec<f32>, PieceError> {
    let channels = channels.max(1);
    interleaved
        .chunks_exact(channels)
        .map(|frame| {
            let average =
                frame.iter().map(|&sample| f64::from(sample)).sum::<f64>() / channels as f64;
            let average = average as f32;
            if average.is_finite() {
                Ok(average)
            } else {
                Err(PieceError::Damaged { at_ms: 0 })
            }
        })
        .collect()
}

/// An Opus stream (RFC 7845's mapping family 0, mono or stereo) through
/// `opuscule`: its pre-skip dropped and output gain applied.
struct Opus {
    decoder: opuscule::Decoder,
    channels: usize,
    pre_skip: u64,
    /// Decoded frames still to drop for the pre-skip.
    skip: usize,
    gain: f32,
    buffer: Vec<f32>,
}

impl Opus {
    /// From the `OpusHead` identification header (RFC 7845 §5.1).
    fn new(head: &[u8]) -> Result<Self, String> {
        if head.len() < 19 || &head[..8] != b"OpusHead" {
            return Err("Opus without its OpusHead header".into());
        }
        let channels = usize::from(head[9]);
        if head[18] != 0 || !(1..=2).contains(&channels) {
            return Err(format!("Opus with {channels} channels"));
        }
        let pre_skip = u16::from_le_bytes([head[10], head[11]]);
        let gain_db = f32::from(i16::from_le_bytes([head[16], head[17]])) / 256.0;
        let layout = if channels == 1 {
            opuscule::Channels::Mono
        } else {
            opuscule::Channels::Stereo
        };
        Ok(Self {
            decoder: opuscule::Decoder::new(opuscule::SampleRate::Hz48000, layout),
            channels,
            pre_skip: u64::from(pre_skip),
            skip: usize::from(pre_skip),
            gain: 10_f32.powf(gain_db / 20.0),
            // 120 ms at 48 kHz, the longest packet.
            buffer: vec![0.0; 5_760 * channels],
        })
    }

    /// One packet as mono samples, or `None` for a damaged packet.
    fn decode(&mut self, packet: &[u8]) -> Result<Option<Vec<f32>>, PieceError> {
        let frames = match self.decoder.decode(Some(packet), &mut self.buffer, false) {
            Ok(frames) => frames,
            Err(opuscule::Error::InvalidPacket) => return Ok(None),
            Err(_) => return Err(PieceError::Damaged { at_ms: 0 }),
        };
        let start = self.skip.min(frames);
        self.skip -= start;
        let gain = self.gain;
        let samples: Vec<f32> = self.buffer[start * self.channels..frames * self.channels]
            .iter()
            .map(|sample| sample * gain)
            .collect();
        mono(&samples, self.channels).map(Some)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_caf_aac_cookie_unwraps_to_its_audio_specific_config() {
        // Apple's encoder's `kuki` for 22.05 kHz mono AAC-LC.
        let cookie = [
            0x03, 0x80, 0x80, 0x80, 0x22, 0x00, 0x00, 0x00, 0x04, 0x80, 0x80, 0x80, 0x14, 0x40,
            0x14, 0x00, 0x18, 0x00, 0x00, 0x00, 0xa5, 0xd8, 0x00, 0x00, 0x7d, 0x00, 0x05, 0x80,
            0x80, 0x80, 0x02, 0x13, 0x88, 0x06, 0x80, 0x80, 0x80, 0x01, 0x02,
        ];
        assert_eq!(super::es_decoder_config(&cookie), Some(vec![0x13, 0x88]));
        assert_eq!(super::es_decoder_config(&[0x13, 0x88]), None);
    }
}

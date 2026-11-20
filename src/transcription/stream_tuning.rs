//! The stream geometry Hex asks a Parakeet buffered model for: the env
//! overrides, else Labs › Faster live text, else the model's own default.
//!
//! A live stream's first words can only be decoded once the model has heard a
//! whole `chunk + right` lookahead, so a smaller tuple shows live text sooner
//! and re-encodes its `left | chunk | right` window more often per audio
//! second. The model's GGUF carries the menus every request must come from
//! (`stt.parakeet.encoder.att_chunk_{left,chunk,right}_choices`, in encoder
//! frames), and the native stream begin validates each field against them, so
//! nothing here is hard-coded to one model's menu.

use std::path::Path;

use transcribe_cpp::{ParakeetBufferedStreamOptions, StreamExtension, StreamOptions};

use crate::transcription::gguf_header;

/// The chunk Labs › Faster live text aims for, in encoder frames: 7 frames of
/// 80 ms is 560 ms, the balanced tuple of every Parakeet buffered menu Hex
/// ships (Unified English `(70, 7, 7)`: 1.12 s of lookahead against the
/// default's 2.08 s, at about 1.6 times the encoder work).
const FASTER_TARGET_FRAMES: i32 = 7;
/// The shortest chunk the lab accepts. A one-frame chunk re-encodes its whole
/// 5.6 s left context every 80 ms, which costs several times more for the
/// same words, so a menu without a chunk of 2 to 7 frames keeps the default.
const FASTER_MIN_FRAMES: i32 = 2;
/// The encoder frame every shipped Parakeet streaming model uses. A file
/// whose header Hex cannot read still asks for the lab's tuple in these
/// frames; the native stream begin validates it against the model's real
/// menu, and the default geometry takes over when it does not fit.
const SHIPPED_FRAME_MS: i32 = 80;

/// A buffered Parakeet model's own chunk menus, read from its GGUF header.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct StreamMenus {
    /// Milliseconds per encoder frame, divided as the native resolver does
    /// (`subsampling_factor * hop_length * 1000 / sample_rate`).
    frame_ms: i32,
    chunk_frames: Vec<i32>,
    right_frames: Vec<i32>,
}

impl StreamMenus {
    /// The menus the GGUF at `path` carries, or `None` for a model without
    /// them (a cache-aware Parakeet such as Nemotron, or a file Hex can't
    /// read), which keeps the model's default stream geometry.
    pub(crate) fn read(path: &Path) -> Option<Self> {
        gguf_header::read(path)
            .ok()
            .and_then(|header| Self::from_header(&header))
    }

    /// The menus `header` carries, as [`Self::read`] reads them.
    fn from_header(header: &gguf_header::GgufHeader) -> Option<Self> {
        let scalar = |key: &str| header.u32s.get(key).copied().map(i64::from);
        let subsampling = scalar("stt.parakeet.encoder.subsampling_factor")?;
        let hop = scalar("stt.frontend.hop_length")?;
        let sample_rate = scalar("stt.frontend.sample_rate")?.max(1);
        let frame_ms = i32::try_from(subsampling * hop * 1_000 / sample_rate).ok()?;
        let menu = |key: &str| {
            header
                .int32_arrays
                .get(key)
                .cloned()
                .filter(|frames| !frames.is_empty())
        };
        Some(Self {
            frame_ms: (frame_ms > 0).then_some(frame_ms)?,
            chunk_frames: menu("stt.parakeet.encoder.att_chunk_chunk_choices")?,
            right_frames: menu("stt.parakeet.encoder.att_chunk_right_choices")?,
        })
    }

    /// Labs › Faster live text's `(chunk, right)` in milliseconds, or `None`
    /// when this model's menu has no chunk between [`FASTER_MIN_FRAMES`] and
    /// [`FASTER_TARGET_FRAMES`].
    fn faster(&self) -> Option<(i32, i32)> {
        let (chunk, right) = faster_frames(&self.chunk_frames, &self.right_frames)?;
        Some((chunk * self.frame_ms, right * self.frame_ms))
    }
}

/// The selection rule: the largest chunk menu entry from
/// [`FASTER_MIN_FRAMES`] to [`FASTER_TARGET_FRAMES`] frames, and the largest
/// right entry from 0 to [`FASTER_TARGET_FRAMES`]. Both must be in the model's
/// own menu, because the native stream begin rejects a tuple outside it.
fn faster_frames(chunk_choices: &[i32], right_choices: &[i32]) -> Option<(i32, i32)> {
    let chunk = chunk_choices
        .iter()
        .copied()
        .filter(|frames| (FASTER_MIN_FRAMES..=FASTER_TARGET_FRAMES).contains(frames))
        .max()?;
    let right = right_choices
        .iter()
        .copied()
        .filter(|frames| (0..=FASTER_TARGET_FRAMES).contains(frames))
        .max()?;
    Some((chunk, right))
}

/// What one process asks a buffered stream for. Two readings that differ
/// describe different geometry, so a cached probe is refreshed when they do
/// not match; see `OfflineGgufSession::refresh_stream`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct StreamTuning {
    /// `HEX_STREAM_CHUNK_MS`, when it parses.
    chunk_ms: Option<i32>,
    /// `HEX_STREAM_RIGHT_MS`, when it parses.
    right_ms: Option<i32>,
    /// Labs › Faster live text, read now.
    faster: bool,
}

impl StreamTuning {
    /// The tuning this process holds now: the env overrides as written, and
    /// the Labs toggle, read each time so a change applies to the next
    /// capture without a restart.
    pub(crate) fn current() -> Self {
        let read = |name: &str| {
            std::env::var(name)
                .ok()
                .and_then(|value| value.parse::<i32>().ok())
        };
        Self {
            chunk_ms: read("HEX_STREAM_CHUNK_MS"),
            right_ms: read("HEX_STREAM_RIGHT_MS"),
            faster: crate::labs::faster_live_text(),
        }
    }

    /// The buffered-stream extension this tuning asks for, or `None` for the
    /// model's own default geometry. `HEX_STREAM_CHUNK_MS` and
    /// `HEX_STREAM_RIGHT_MS` take precedence over the Labs toggle, and a
    /// chunk alone keeps setting the right context too, as it always has.
    fn extension(self, menus: Option<&StreamMenus>) -> Option<ParakeetBufferedStreamOptions> {
        let (chunk_ms, right_ms) = if self.chunk_ms.is_some() || self.right_ms.is_some() {
            (self.chunk_ms, self.right_ms.or(self.chunk_ms))
        } else if self.faster {
            match menus {
                Some(menus) => {
                    let (chunk_ms, right_ms) = menus.faster()?;
                    (Some(chunk_ms), Some(right_ms))
                }
                None => {
                    let ms = FASTER_TARGET_FRAMES * SHIPPED_FRAME_MS;
                    (Some(ms), Some(ms))
                }
            }
        } else {
            return None;
        };
        Some(ParakeetBufferedStreamOptions {
            left_ms: None,
            chunk_ms,
            right_ms,
        })
    }

    /// The stream options this tuning asks for.
    pub(crate) fn options(self, menus: Option<&StreamMenus>) -> StreamOptions {
        StreamOptions {
            family: self.extension(menus).map(StreamExtension::ParakeetBuffered),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::gguf_header::tests::{GgufParts, gguf_with, read_header};

    /// Unified English's published menus, as its GGUF carries them.
    fn unified() -> StreamMenus {
        StreamMenus {
            frame_ms: 80,
            chunk_frames: vec![1, 2, 7, 13],
            right_frames: vec![0, 1, 2, 3, 4, 7, 13],
        }
    }

    #[test]
    fn faster_live_text_picks_the_balanced_tuple_from_unified_englishs_menu() {
        assert_eq!(
            faster_frames(&[1, 2, 7, 13], &[0, 1, 2, 3, 4, 7, 13]),
            Some((7, 7))
        );
        assert_eq!(unified().faster(), Some((560, 560)));
    }

    #[test]
    fn a_menu_without_seven_frames_picks_the_largest_chunk_below_it() {
        // A fine-tune whose chunk menu stops at 2 and right menu at 4.
        assert_eq!(faster_frames(&[1, 2, 13], &[0, 1, 2, 4, 13]), Some((2, 4)));
        let menus = StreamMenus {
            frame_ms: 80,
            chunk_frames: vec![1, 2, 13],
            right_frames: vec![0, 1, 2, 4, 13],
        };
        assert_eq!(menus.faster(), Some((160, 320)));
    }

    #[test]
    fn a_menu_of_only_one_frame_chunks_keeps_the_default() {
        assert_eq!(faster_frames(&[1, 13], &[0, 13]), None);
        let menus = StreamMenus {
            frame_ms: 80,
            chunk_frames: vec![1, 13],
            right_frames: vec![0, 13],
        };
        assert_eq!(menus.faster(), None);
    }

    #[test]
    fn the_frame_size_and_menus_come_from_the_gguf_header() {
        let header = read_header(gguf_with(&GgufParts {
            u32s: &[
                ("stt.parakeet.encoder.subsampling_factor", 8),
                ("stt.frontend.hop_length", 160),
                ("stt.frontend.sample_rate", 16_000),
            ],
            int32_arrays: &[
                (
                    "stt.parakeet.encoder.att_chunk_chunk_choices",
                    &[1, 2, 7, 13],
                ),
                (
                    "stt.parakeet.encoder.att_chunk_right_choices",
                    &[0, 1, 2, 3, 4, 7, 13],
                ),
            ],
            ..GgufParts::default()
        }));
        assert_eq!(StreamMenus::from_header(&header), Some(unified()));
    }

    #[test]
    fn a_model_without_chunk_menus_keeps_the_default() {
        let header = read_header(gguf_with(&GgufParts {
            u32s: &[("stt.parakeet.encoder.subsampling_factor", 8)],
            ..GgufParts::default()
        }));
        assert_eq!(StreamMenus::from_header(&header), None);
    }

    #[test]
    fn the_env_overrides_win_over_the_lab_and_keep_right_following_chunk() {
        let menus = unified();
        let geometry = |tuning: StreamTuning| {
            tuning
                .extension(Some(&menus))
                .map(|options| (options.chunk_ms, options.right_ms))
        };
        // A chunk alone still sets the right context, as it always has.
        assert_eq!(
            geometry(StreamTuning {
                chunk_ms: Some(160),
                right_ms: None,
                faster: true,
            }),
            Some((Some(160), Some(160)))
        );
        assert_eq!(
            geometry(StreamTuning {
                chunk_ms: Some(560),
                right_ms: Some(320),
                faster: false,
            }),
            Some((Some(560), Some(320)))
        );
        assert_eq!(
            geometry(StreamTuning {
                faster: true,
                ..StreamTuning::default()
            }),
            Some((Some(560), Some(560)))
        );
        assert_eq!(geometry(StreamTuning::default()), None);
    }

    #[test]
    fn an_unreadable_menu_still_asks_for_the_labs_tuple_in_shipped_frames() {
        let geometry = StreamTuning {
            faster: true,
            ..StreamTuning::default()
        }
        .extension(None)
        .map(|options| (options.chunk_ms, options.right_ms));
        assert_eq!(geometry, Some((Some(560), Some(560))));
    }
}

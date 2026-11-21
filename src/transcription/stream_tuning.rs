//! The stream geometry Hex asks a Parakeet buffered model for: the env
//! overrides, else Labs › Faster live text, else the model's own default.
//!
//! A live stream's first words can only be decoded once the model has heard a
//! whole `chunk + right` lookahead, so a smaller tuple shows live text sooner,
//! and the final text comes from that same stream, so it uses the same tuple.
//! The model's GGUF carries the menus every request must come from
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
/// menu, and the default geometry takes care of a model that has none.
const SHIPPED_FRAME_MS: i32 = 80;

/// What a model's GGUF says about its buffered-stream geometry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum StreamMenus {
    /// A buffered model's own chunk and right menus, in encoder frames.
    Buffered {
        /// Milliseconds per encoder frame, divided as the native resolver
        /// does (`subsampling_factor * hop_length * 1000 / sample_rate`).
        frame_ms: i32,
        chunk_frames: Vec<i32>,
        right_frames: Vec<i32>,
    },
    /// The header was read and carries no buffered menus: a cache-aware
    /// Parakeet such as Nemotron, which takes no buffered extension at all.
    NotBuffered,
}

impl StreamMenus {
    /// Reads the menus from the GGUF at `path`; `None` when its header cannot
    /// be read, which leaves the lab's request to the native probe.
    pub(crate) fn read(path: &Path) -> Option<Self> {
        gguf_header::read(path)
            .ok()
            .map(|header| Self::from_header(&header))
    }

    /// The menus `header` carries, as [`Self::read`] reads them.
    fn from_header(header: &gguf_header::GgufHeader) -> Self {
        Self::buffered(header).unwrap_or(Self::NotBuffered)
    }

    /// The buffered menus, when the header has all of them.
    fn buffered(header: &gguf_header::GgufHeader) -> Option<Self> {
        let scalar = |key: &str| header.u32s.get(key).copied().map(i64::from);
        let subsampling = scalar("stt.parakeet.encoder.subsampling_factor")?;
        let hop = scalar("stt.frontend.hop_length")?;
        let sample_rate = scalar("stt.frontend.sample_rate")?.max(1);
        let frame_ms = subsampling.checked_mul(hop)?.checked_mul(1_000)? / sample_rate;
        let frame_ms = i32::try_from(frame_ms).ok().filter(|ms| *ms > 0)?;
        let menu = |key: &str| {
            header
                .int32_arrays
                .get(key)
                .cloned()
                .filter(|frames| !frames.is_empty())
        };
        Some(Self::Buffered {
            frame_ms,
            chunk_frames: menu("stt.parakeet.encoder.att_chunk_chunk_choices")?,
            right_frames: menu("stt.parakeet.encoder.att_chunk_right_choices")?,
        })
    }

    /// Labs › Faster live text's `(chunk, right)` in milliseconds: `None`
    /// for a model with no buffered menu, or one whose menu has no chunk
    /// between [`FASTER_MIN_FRAMES`] and [`FASTER_TARGET_FRAMES`].
    fn faster(&self) -> Option<(i32, i32)> {
        let Self::Buffered {
            frame_ms,
            chunk_frames,
            right_frames,
        } = self
        else {
            return None;
        };
        let (chunk, right) = faster_frames(chunk_frames, right_frames)?;
        Some((chunk * frame_ms, right * frame_ms))
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
/// not match; see [`StreamCache`].
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
    /// the Labs toggle, read each time so a change applies without a restart.
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

    /// The tuning a model uses: only a Parakeet buffered model has geometry
    /// to tune, so every other architecture keeps the default (no extension)
    /// and is never re-probed.
    pub(crate) fn for_model(parakeet: bool, current: Self) -> Self {
        if parakeet { current } else { Self::default() }
    }

    /// Whether this tuning must read the model's menus: only the lab's tuple
    /// comes from them, and an env override wins over it.
    pub(crate) fn needs_menus(self) -> bool {
        self.faster && self.chunk_ms.is_none() && self.right_ms.is_none()
    }

    /// The buffered-stream extension this tuning asks for, or `None` for the
    /// model's own default geometry. `HEX_STREAM_CHUNK_MS` and
    /// `HEX_STREAM_RIGHT_MS` take precedence over the Labs toggle; a chunk
    /// alone keeps setting the right context too, as it always has.
    fn extension(self, menus: Option<&StreamMenus>) -> Option<ParakeetBufferedStreamOptions> {
        let (chunk_ms, right_ms) = if self.chunk_ms.is_some() || self.right_ms.is_some() {
            (self.chunk_ms, self.right_ms.or(self.chunk_ms))
        } else if self.faster {
            match menus {
                Some(menus @ StreamMenus::Buffered { .. }) => {
                    let (chunk_ms, right_ms) = menus.faster()?;
                    (Some(chunk_ms), Some(right_ms))
                }
                // A model with no buffered menu takes no buffered extension.
                Some(StreamMenus::NotBuffered) => return None,
                // The header could not be read: ask in shipped frames and
                // let the native probe validate it against the real menu.
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

/// What a model streams with, and the tuning its geometry belongs to.
///
/// Only a probe that ran past the device and capability gates may start a
/// stream, so `tuning` stays `None` until one has. A failed re-probe keeps
/// the working geometry, and remembers the tuning that failed so the next
/// capture does not pay for the same probe again.
#[derive(Debug, Default)]
pub(crate) struct StreamCache {
    stream: Option<StreamOptions>,
    tuning: Option<StreamTuning>,
    rejected: Option<StreamTuning>,
}

impl StreamCache {
    /// The geometry this model streams with, `None` when it cannot stream.
    pub(crate) fn stream(&self) -> Option<&StreamOptions> {
        self.stream.as_ref()
    }

    /// Whether `tuning` asks for a probe: one already ran, neither it nor the
    /// tuning that failed is this one.
    pub(crate) fn wants(&self, tuning: StreamTuning) -> bool {
        self.tuning.is_some() && self.tuning != Some(tuning) && self.rejected != Some(tuning)
    }

    /// Records a probe. A geometry replaces the cached one; a failure keeps
    /// the working geometry and the tuning it belongs to, and only remembers
    /// that `tuning` failed. Returns whether the geometry changed.
    pub(crate) fn record(&mut self, tuning: StreamTuning, probed: Option<StreamOptions>) -> bool {
        match probed {
            Some(stream) => {
                self.stream = Some(stream);
                self.tuning = Some(tuning);
                self.rejected = None;
                true
            }
            None => {
                self.rejected = Some(tuning);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::gguf_header::tests::{GgufParts, gguf_with, read_header};

    /// Unified English's published menus, as its GGUF carries them.
    fn unified() -> StreamMenus {
        StreamMenus::Buffered {
            frame_ms: 80,
            chunk_frames: vec![1, 2, 7, 13],
            right_frames: vec![0, 1, 2, 3, 4, 7, 13],
        }
    }

    fn lab() -> StreamTuning {
        StreamTuning {
            faster: true,
            ..StreamTuning::default()
        }
    }

    fn env() -> StreamTuning {
        StreamTuning {
            chunk_ms: Some(160),
            ..StreamTuning::default()
        }
    }

    fn geometry(tuning: StreamTuning, menus: Option<&StreamMenus>) -> StreamOptions {
        tuning.options(menus)
    }

    #[test]
    fn faster_live_text_picks_the_balanced_tuple_from_unified_englishs_menu() {
        assert_eq!(
            faster_frames(&[1, 2, 7, 13], &[0, 1, 2, 3, 4, 7, 13]),
            Some((7, 7))
        );
        let menus = unified();
        let StreamMenus::Buffered { .. } = &menus else {
            panic!("Unified English has buffered menus");
        };
        assert_eq!(menus.faster(), Some((560, 560)));
    }

    #[test]
    fn a_menu_without_seven_frames_picks_the_largest_chunk_below_it() {
        // A fine-tune whose chunk menu stops at 2 and right menu at 4.
        assert_eq!(faster_frames(&[1, 2, 13], &[0, 1, 2, 4, 13]), Some((2, 4)));
        let menus = StreamMenus::Buffered {
            frame_ms: 80,
            chunk_frames: vec![1, 2, 13],
            right_frames: vec![0, 1, 2, 4, 13],
        };
        assert_eq!(menus.faster(), Some((160, 320)));
    }

    #[test]
    fn a_menu_of_only_one_frame_chunks_keeps_the_default() {
        assert_eq!(faster_frames(&[1, 13], &[0, 13]), None);
        let menus = StreamMenus::Buffered {
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
        assert_eq!(StreamMenus::from_header(&header), unified());
    }

    #[test]
    fn a_read_header_without_chunk_menus_is_a_cache_aware_model() {
        let header = read_header(gguf_with(&GgufParts {
            u32s: &[("stt.parakeet.encoder.subsampling_factor", 8)],
            ..GgufParts::default()
        }));
        assert_eq!(StreamMenus::from_header(&header), StreamMenus::NotBuffered);
        // No buffered extension, and no blind request either: the header was
        // read and has no menus to fit one to.
        assert!(
            geometry(lab(), Some(&StreamMenus::NotBuffered))
                .family
                .is_none()
        );
    }

    #[test]
    fn an_unreadable_header_still_asks_for_the_labs_tuple_in_shipped_frames() {
        let requested = geometry(lab(), None);
        assert_eq!(
            requested.family.and_then(|family| match family {
                StreamExtension::ParakeetBuffered(options) => {
                    Some((options.chunk_ms, options.right_ms))
                }
                _ => None,
            }),
            Some((Some(560), Some(560)))
        );
    }

    #[test]
    fn the_env_overrides_win_over_the_lab_and_keep_right_following_chunk() {
        let menus = unified();
        let wire = |tuning: StreamTuning| {
            tuning
                .extension(Some(&menus))
                .map(|options| (options.chunk_ms, options.right_ms))
        };
        // A chunk alone still sets the right context, as it always has.
        assert_eq!(wire(env()), Some((Some(160), Some(160))));
        assert_eq!(
            wire(StreamTuning {
                chunk_ms: Some(560),
                right_ms: Some(320),
                faster: false,
            }),
            Some((Some(560), Some(320)))
        );
        // A right alone keeps the model's default chunk.
        assert_eq!(
            wire(StreamTuning {
                right_ms: Some(320),
                ..StreamTuning::default()
            }),
            Some((None, Some(320)))
        );
        assert_eq!(wire(lab()), Some((Some(560), Some(560))));
        assert_eq!(wire(StreamTuning::default()), None);
    }

    #[test]
    fn only_the_labs_tuple_without_an_env_override_reads_the_menus() {
        assert!(lab().needs_menus());
        assert!(!env().needs_menus());
        assert!(!StreamTuning::default().needs_menus());
        assert!(
            !StreamTuning {
                faster: true,
                right_ms: Some(320),
                ..StreamTuning::default()
            }
            .needs_menus()
        );
    }

    #[test]
    fn a_non_parakeet_model_keeps_the_default_tuning() {
        assert_eq!(
            StreamTuning::for_model(false, lab()),
            StreamTuning::default()
        );
        assert_eq!(
            StreamTuning::for_model(false, env()),
            StreamTuning::default()
        );
        assert_eq!(StreamTuning::for_model(true, lab()), lab());
    }

    #[test]
    fn nothing_probes_before_the_gated_probe() {
        let cache = StreamCache::default();
        assert_eq!(cache.stream(), None);
        assert!(!cache.wants(lab()));
        assert!(!cache.wants(StreamTuning::default()));
    }

    #[test]
    fn the_same_tuning_does_not_probe_again() {
        let mut cache = StreamCache::default();
        assert!(cache.record(StreamTuning::default(), Some(StreamOptions::default())));
        assert!(!cache.wants(StreamTuning::default()));
        assert!(cache.wants(lab()));
    }

    #[test]
    fn a_failed_probe_keeps_the_working_geometry_and_is_not_retried() {
        let mut cache = StreamCache::default();
        let working = StreamOptions::default();
        assert!(cache.record(StreamTuning::default(), Some(working.clone())));
        // The lab's geometry cannot stream here: nothing changes.
        assert!(!cache.record(lab(), None));
        assert_eq!(cache.stream(), Some(&working));
        assert!(!cache.wants(lab()), "the same failed tuning is not retried");
        assert!(
            !cache.wants(StreamTuning::default()),
            "the working one is kept"
        );
        // A different choice does ask again.
        assert!(cache.wants(env()));
    }

    #[test]
    fn a_later_probe_replaces_the_geometry() {
        let mut cache = StreamCache::default();
        cache.record(StreamTuning::default(), Some(StreamOptions::default()));
        let faster = StreamOptions {
            family: Some(StreamExtension::ParakeetBuffered(
                ParakeetBufferedStreamOptions {
                    left_ms: None,
                    chunk_ms: Some(560),
                    right_ms: Some(560),
                },
            )),
            ..Default::default()
        };
        assert!(cache.record(lab(), Some(faster.clone())));
        assert_eq!(cache.stream(), Some(&faster));
        assert!(!cache.wants(lab()));
    }
}

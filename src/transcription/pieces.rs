//! Saved audio read as pieces of at most 30 s: streaming decode, downmix,
//! 16 kHz resampling, and the quiet-point cut the GGUF adapters use for a
//! long clip, so `hex transcribe` and a dropped file hold about one 30 s piece
//! instead of the whole file. [`Pieces`] reads a WAV; another format's
//! decoder feeds its mono blocks through [`PieceBuffer`] the same way.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::Path;

use hound::{Sample, SampleFormat, WavReader, WavSpec, WavWriter};

use crate::dictation::StreamingResampler;
use crate::transcription::gguf_session::quiet_cut;
use crate::transcription::service::{AudioError, WavFormat, validate_wav_input, wav_format};

/// 30 s at 16 kHz: the longest piece. A clip that short can run on the
/// running Hex's dictation engine's loaded copy when it holds the model,
/// instead of its service loading a second copy.
pub(crate) const PIECE_SAMPLES: usize = 30 * 16_000;
/// Frames decoded per read: 85 ms at 48 kHz, so a piece is cut near its end
/// without holding much more than one piece.
pub(crate) const BLOCK_FRAMES: usize = 4_096;

/// Why saved audio could not be read into pieces.
#[derive(Debug)]
pub(crate) enum PieceError {
    /// The file could not be opened or read at all.
    Io(std::io::Error),
    /// The file is not audio this build decodes: either it is not that
    /// format at all, or its rate, channels, or sample format is unsupported.
    Audio(AudioError),
    /// The file holds no audio at all.
    Empty,
    /// The header was accepted but the samples could not all be read: the
    /// file is truncated or corrupt.
    Incomplete,
    /// The file could not be read after the up-front read accepted it: it
    /// changed, or could no longer be read, while Hex was transcribing it.
    Changed { at_ms: u64 },
}

/// One saved file's 16 kHz samples, one piece at a time.
pub(crate) trait PieceSource {
    /// The next piece of at most [`PIECE_SAMPLES`] samples, or `None` at the
    /// end of the file.
    fn next_piece(&mut self) -> Result<Option<Vec<f32>>, PieceError>;

    /// How many 16 kHz samples it has decoded so far, for progress.
    fn position(&self) -> usize;
}

/// The 16 kHz samples of one saved WAV, one piece at a time.
pub(crate) struct Pieces {
    reader: WavReader<BufReader<File>>,
    format: WavFormat,
    channels: usize,
    bits: u16,
    /// `None` once the input ended and its tail was flushed.
    resampler: Option<StreamingResampler>,
    buffer: PieceBuffer,
    ready: VecDeque<Vec<f32>>,
    done: bool,
    /// 16 kHz samples handed to `buffer` so far, for where a read failed.
    emitted: usize,
}

impl std::fmt::Debug for Pieces {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Pieces")
            .field("format", &self.format)
            .field("channels", &self.channels)
            .field("bits", &self.bits)
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}

impl Pieces {
    /// Opens `path` and checks its header and format, so a file Hex cannot
    /// read is refused before any model is loaded or Hex is asked.
    pub(crate) fn open(path: &Path) -> Result<Self, PieceError> {
        let file = File::open(path).map_err(PieceError::Io)?;
        let reader = WavReader::new(BufReader::new(file))
            .map_err(|_| PieceError::Audio(AudioError::Invalid))?;
        let spec = reader.spec();
        validate_wav_input(&spec).map_err(PieceError::Audio)?;
        let format = wav_format(&spec).map_err(PieceError::Audio)?;
        if reader.duration() == 0 {
            return Err(PieceError::Empty);
        }
        let resampler = StreamingResampler::new(spec.sample_rate)
            .map_err(|_| PieceError::Audio(AudioError::Unsupported))?;
        Ok(Self {
            reader,
            format,
            channels: usize::from(spec.channels),
            bits: spec.bits_per_sample,
            resampler: Some(resampler),
            buffer: PieceBuffer::default(),
            ready: VecDeque::new(),
            done: false,
            emitted: 0,
        })
    }

    /// How long the whole file is at 16 kHz, for progress.
    pub(crate) fn total_samples(&self) -> usize {
        let rate = usize::try_from(self.reader.spec().sample_rate).unwrap_or(16_000);
        let frames = usize::try_from(self.reader.duration()).unwrap_or(0);
        frames.saturating_mul(16_000) / rate.max(1)
    }

    /// Reads every sample once without keeping any, so a truncated or
    /// corrupt file is refused before any transcription rather than after
    /// minutes of it. Only `prepare` calls this: it costs one decode pass.
    pub(crate) fn validate(&mut self) -> Result<(), PieceError> {
        loop {
            match self.read_source() {
                Ok(block) if block.is_empty() => return Ok(()),
                Ok(_) => {}
                Err(_) => return Err(PieceError::Incomplete),
            }
        }
    }

    /// The next piece of at most [`PIECE_SAMPLES`] samples, or `None` at the
    /// end of the file.
    pub(crate) fn next(&mut self) -> Result<Option<Vec<f32>>, PieceError> {
        loop {
            if let Some(piece) = self.ready.pop_front() {
                return Ok(Some(piece));
            }
            if self.done {
                return Ok(self.buffer.take_rest());
            }
            self.fill()?;
        }
    }

    /// Decodes one block and queues the pieces it completed. The end of the
    /// file flushes the resampler's tail and then the last piece.
    fn fill(&mut self) -> Result<(), PieceError> {
        let block = self.read_block()?;
        if block.is_empty() {
            self.done = true;
            let tail = self
                .resampler
                .take()
                .map(StreamingResampler::finish)
                .unwrap_or_default();
            self.ready.extend(self.buffer.push(&tail));
            return Ok(());
        }
        let resampled = match self.resampler.as_mut() {
            Some(resampler) => resampler.push(&block),
            None => block,
        };
        self.emitted += resampled.len();
        self.ready.extend(self.buffer.push(&resampled));
        Ok(())
    }

    /// One block of mono source samples, before resampling.
    fn read_source(&mut self) -> Result<Vec<f32>, PieceError> {
        read_frames(
            &mut self.reader,
            self.channels,
            BLOCK_FRAMES,
            self.format,
            self.bits,
        )
    }

    fn read_block(&mut self) -> Result<Vec<f32>, PieceError> {
        // `validate` read the whole file before this run started, so a failure
        // here means it changed underneath.
        self.read_source().map_err(|_| PieceError::Changed {
            at_ms: self.position_ms(),
        })
    }

    /// How far into the file the pieces have got, in milliseconds.
    fn position_ms(&self) -> u64 {
        self.emitted as u64 * 1_000 / 16_000
    }
}

impl PieceSource for Pieces {
    fn next_piece(&mut self) -> Result<Option<Vec<f32>>, PieceError> {
        self.next()
    }

    fn position(&self) -> usize {
        self.buffer.position()
    }
}

/// The next piece that may hold speech, skipping pieces of only silence as a
/// long GGUF clip's silent chunks are skipped: models invent text for silence
/// (#1194) or run to their generation cap (#968).
///
/// The check runs only when the speech detector is already installed: a
/// one-shot run must not start its background model download.
pub(crate) fn next_speechful<P: PieceSource>(
    pieces: &mut P,
) -> Result<Option<Vec<f32>>, PieceError> {
    let detector = crate::transcription::models::speech_detector_installed();
    next_speechful_with(pieces, detector)
}

/// [`next_speechful`] with the detector's availability passed in, so a test
/// needs no installed model.
pub(crate) fn next_speechful_with<P: PieceSource>(
    pieces: &mut P,
    detector: bool,
) -> Result<Option<Vec<f32>>, PieceError> {
    loop {
        match pieces.next_piece()? {
            Some(samples) if !keeps_piece(&samples, detector) => continue,
            piece => return Ok(piece),
        }
    }
}

/// Whether a piece goes to the model: only silence is skipped, and only when
/// the detector is installed and hears none in it.
fn keeps_piece(samples: &[f32], detector: bool) -> bool {
    !detector || crate::transcription::gguf_session::chunk_may_hold_speech(samples)
}

/// 16 kHz samples waiting to be cut, never more than one piece plus the block
/// that completed it.
#[derive(Default)]
pub(crate) struct PieceBuffer {
    samples: Vec<f32>,
    /// Every 16 kHz sample pushed so far, for progress.
    decoded: usize,
}

impl PieceBuffer {
    /// Appends `samples` and returns every piece they completed, each cut
    /// through the quietest 100 ms near its 30 s end as
    /// [`crate::transcription::gguf_session::chunk_ranges`] cuts a long clip.
    pub(crate) fn push(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        self.samples.extend_from_slice(samples);
        self.decoded += samples.len();
        let mut pieces = Vec::new();
        while self.samples.len() > PIECE_SAMPLES {
            let rest = self
                .samples
                .split_off(quiet_cut(&self.samples, PIECE_SAMPLES));
            pieces.push(std::mem::replace(&mut self.samples, rest));
        }
        pieces
    }

    /// The samples after the last piece, as the final piece, unless there are
    /// none.
    pub(crate) fn take_rest(&mut self) -> Option<Vec<f32>> {
        (!self.samples.is_empty()).then(|| std::mem::take(&mut self.samples))
    }

    /// How many 16 kHz samples have been pushed so far.
    pub(crate) fn position(&self) -> usize {
        self.decoded
    }
}

/// `samples` as a 16 kHz mono 32-bit float WAV: what the local API takes for
/// one piece, with no sample converted or dropped.
pub(crate) fn wav_bytes(samples: &[f32]) -> Result<Vec<u8>, PieceError> {
    let mut bytes = Cursor::new(Vec::new());
    {
        let spec = WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let mut writer = WavWriter::new(&mut bytes, spec).map_err(broken)?;
        for sample in samples {
            writer.write_sample(*sample).map_err(broken)?;
        }
        writer.finalize().map_err(broken)?;
    }
    Ok(bytes.into_inner())
}

fn broken(_: hound::Error) -> PieceError {
    PieceError::Audio(AudioError::Invalid)
}

/// Up to `frames` mono samples, averaging `channels` and converting them as
/// the file's format says, exactly as [`crate::transcription::service::AudioClip`]
/// converts a whole file. The last read may be short; a broken one is
/// `Invalid`.
fn read_frames(
    reader: &mut WavReader<BufReader<File>>,
    channels: usize,
    frames: usize,
    format: WavFormat,
    bits: u16,
) -> Result<Vec<f32>, PieceError> {
    let scale = 2_f32.powi(i32::from(bits) - 1);
    match format {
        WavFormat::F32 => read_mono(reader, channels, frames, |sample: f32| sample),
        WavFormat::I8 => read_mono(reader, channels, frames, |sample: i8| {
            f32::from(sample) / 128.0
        }),
        WavFormat::I16 => read_mono(reader, channels, frames, move |sample: i16| {
            f32::from(sample) / scale
        }),
        WavFormat::I32 => read_mono(reader, channels, frames, move |sample: i32| {
            sample as f32 / scale
        }),
    }
}

fn read_mono<S: Sample>(
    reader: &mut WavReader<BufReader<File>>,
    channels: usize,
    frames: usize,
    convert: impl Fn(S) -> f32,
) -> Result<Vec<f32>, PieceError> {
    let mut mono = Vec::with_capacity(frames);
    let mut samples = reader.samples::<S>();
    for _ in 0..frames {
        // A whole frame or nothing: hound rejects a data chunk that is not a
        // multiple of the channels, so a missing sample is a broken file.
        let Some(first) = samples.next() else {
            return Ok(mono);
        };
        let first = convert(first.map_err(broken)?);
        if !first.is_finite() {
            return Err(PieceError::Audio(AudioError::Invalid));
        }
        let mut sum = f64::from(first);
        for _ in 1..channels {
            let Some(sample) = samples.next() else {
                return Err(PieceError::Audio(AudioError::Invalid));
            };
            let sample = convert(sample.map_err(broken)?);
            if !sample.is_finite() {
                return Err(PieceError::Audio(AudioError::Invalid));
            }
            sum += f64::from(sample);
        }
        let average = (sum / channels as f64) as f32;
        if !average.is_finite() {
            return Err(PieceError::Audio(AudioError::Invalid));
        }
        mono.push(average);
    }
    Ok(mono)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::service::AudioClip;

    /// A WAV of `seconds` with one second of silence every five, so a cut has
    /// somewhere quiet to land.
    fn paced(sample_rate: u32, seconds: usize) -> Vec<f32> {
        (0..sample_rate as usize * seconds)
            .map(|sample| {
                if sample / sample_rate as usize % 5 == 4 {
                    0.0
                } else {
                    0.25
                }
            })
            .collect()
    }

    fn write(path: &Path, spec: WavSpec, samples: &[f32]) {
        let mut writer = WavWriter::create(path, spec).unwrap();
        for sample in samples {
            match spec.sample_format {
                SampleFormat::Float => writer.write_sample(*sample).unwrap(),
                SampleFormat::Int if spec.bits_per_sample <= 8 => {
                    writer.write_sample((sample * 127.0) as i8).unwrap();
                }
                SampleFormat::Int if spec.bits_per_sample <= 16 => {
                    writer.write_sample((sample * 32_767.0) as i16).unwrap();
                }
                SampleFormat::Int if spec.bits_per_sample <= 24 => {
                    writer.write_sample((sample * 8_388_607.0) as i32).unwrap();
                }
                SampleFormat::Int => {
                    writer
                        .write_sample((sample * 2_147_483_647.0) as i32)
                        .unwrap();
                }
            }
        }
        writer.finalize().unwrap();
    }

    /// A varying signal, so each sample format's quantization is exercised.
    fn wave(sample_rate: u32, seconds: f32) -> Vec<f32> {
        (0..(sample_rate as f32 * seconds) as usize)
            .map(|index| (index as f32 * 0.01).sin() * 0.6)
            .collect()
    }

    /// A temp WAV path unique to this module: tests in other modules run in
    /// parallel in one process and share `TMPDIR`.
    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "hex-transcribe-pieces-{name}-{}.wav",
            std::process::id()
        ))
    }

    fn collect(path: &Path) -> Vec<Vec<f32>> {
        let mut pieces = Pieces::open(path).unwrap();
        let mut collected = Vec::new();
        while let Some(piece) = pieces.next().unwrap() {
            collected.push(piece);
        }
        collected
    }

    /// A 16 kHz mono WAV of `seconds` of silence.
    fn silent_wav(name: &str, seconds: f32) -> std::path::PathBuf {
        let path = temp(name);
        write(
            &path,
            WavSpec {
                channels: 1,
                sample_rate: 16_000,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            },
            &vec![0.0; (16_000.0 * seconds) as usize],
        );
        path
    }

    #[test]
    fn pieces_cover_every_sample_exactly_once_in_order() {
        let samples = paced(16_000, 95);
        let mut buffer = PieceBuffer::default();
        let mut pieces = Vec::new();
        for block in samples.chunks(777) {
            pieces.extend(buffer.push(block));
        }
        pieces.extend(buffer.take_rest());

        assert!(pieces.len() > 1, "95 s is more than one piece");
        assert!(
            pieces.iter().all(|piece| !piece.is_empty()),
            "no piece is empty"
        );
        assert!(
            pieces.iter().all(|piece| piece.len() <= PIECE_SAMPLES),
            "every piece is at most 30 s"
        );
        assert_eq!(
            pieces.concat(),
            samples,
            "every sample appears once, in order"
        );
        assert_eq!(buffer.position(), samples.len());
    }

    #[test]
    fn a_piece_of_exactly_thirty_seconds_is_not_cut() {
        let samples = vec![0.25_f32; PIECE_SAMPLES];
        let mut buffer = PieceBuffer::default();
        assert!(buffer.push(&samples).is_empty());
        assert_eq!(buffer.take_rest().unwrap().len(), PIECE_SAMPLES);
    }

    #[test]
    fn a_48_khz_file_is_read_in_pieces_of_at_most_thirty_seconds() {
        let path = temp("48k");
        let spec = WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        write(&path, spec, &paced(48_000, 70));
        let pieces = collect(&path);
        let whole = AudioClip::decode_wav(BufReader::new(File::open(&path).unwrap())).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(pieces.len() > 1, "70 s is more than one piece");
        assert!(
            pieces.iter().all(|piece| piece.len() <= PIECE_SAMPLES),
            "every piece is at most 30 s of 16 kHz audio"
        );
        assert_eq!(
            pieces.iter().map(Vec::len).sum::<usize>(),
            70 * 16_000,
            "the whole 48 kHz file is resampled, not truncated"
        );
        let streamed = pieces.concat();
        assert_eq!(streamed.len(), whole.samples.len());
        assert!(
            streamed
                .iter()
                .zip(&whole.samples)
                .all(|(streamed, whole)| (streamed - whole).abs() <= 1e-5),
            "every piece's samples match the whole-file decoder"
        );
    }

    #[test]
    fn every_supported_sample_format_matches_the_whole_file_decoder() {
        for (name, bits, sample_format) in [
            ("i8", 8, SampleFormat::Int),
            ("i16", 16, SampleFormat::Int),
            ("i24", 24, SampleFormat::Int),
            ("i32", 32, SampleFormat::Int),
            ("f32", 32, SampleFormat::Float),
        ] {
            let path = temp(name);
            let spec = WavSpec {
                channels: 1,
                sample_rate: 48_000,
                bits_per_sample: bits,
                sample_format,
            };
            write(&path, spec, &wave(48_000, 2.7));
            let pieces = collect(&path);
            let whole = AudioClip::decode_wav(BufReader::new(File::open(&path).unwrap())).unwrap();
            let _ = std::fs::remove_file(&path);

            assert_eq!(whole.duration_ms, 2_700, "{name}");
            assert!(
                pieces.iter().all(|piece| piece.len() <= PIECE_SAMPLES),
                "{name}: every piece is at most 30 s"
            );
            assert_eq!(pieces.concat(), whole.samples, "{name}: same samples");
        }
    }

    #[test]
    fn streamed_samples_match_the_whole_file_decoder() {
        let path = temp("match");
        let spec = WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let left = paced(48_000, 7);
        let mut stereo = Vec::with_capacity(left.len() * 2);
        for (index, sample) in left.iter().enumerate() {
            stereo.push(*sample);
            stereo.push(if index % 3 == 0 { -*sample } else { *sample });
        }
        write(&path, spec, &stereo);
        let pieces = collect(&path);
        let whole = AudioClip::decode_wav(BufReader::new(File::open(&path).unwrap())).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(pieces.concat(), whole.samples);
        assert_eq!(whole.duration_ms, 7_000);
    }

    #[test]
    fn a_file_with_no_frames_is_refused() {
        let path = temp("empty");
        write(
            &path,
            WavSpec {
                channels: 1,
                sample_rate: 16_000,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            },
            &[],
        );
        let error = Pieces::open(&path).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert!(matches!(error, PieceError::Empty), "{error:?}");
    }

    #[test]
    fn a_truncated_file_is_refused_by_the_up_front_read() {
        let path = temp("truncated");
        write(
            &path,
            WavSpec {
                channels: 1,
                sample_rate: 16_000,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            },
            &paced(16_000, 3),
        );
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();

        // The header still promises the whole recording, so opening succeeds;
        // the up-front read is what finds the missing audio.
        let mut pieces = Pieces::open(&path).unwrap();
        let error = pieces.validate().unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert!(matches!(error, PieceError::Incomplete), "{error:?}");
    }

    #[test]
    fn a_file_that_changes_mid_read_reports_where_it_stopped() {
        let path = temp("changed");
        write(
            &path,
            WavSpec {
                channels: 1,
                sample_rate: 16_000,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            },
            &paced(16_000, 40),
        );
        let mut pieces = Pieces::open(&path).unwrap();
        assert!(pieces.next().unwrap().is_some(), "the first piece reads");

        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() / 3]).unwrap();
        let error = loop {
            match pieces.next() {
                Ok(Some(_)) => continue,
                Ok(None) => panic!("expected the shortened file to fail"),
                Err(error) => break error,
            }
        };
        let _ = std::fs::remove_file(&path);
        assert!(
            matches!(error, PieceError::Changed { at_ms } if (29_000..31_000).contains(&at_ms)),
            "{error:?}"
        );
    }

    #[test]
    fn a_file_that_is_not_wav_is_invalid() {
        let path = temp("not-wav");
        std::fs::write(&path, b"this is not a wav").unwrap();
        let error = Pieces::open(&path).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert!(
            matches!(error, PieceError::Audio(AudioError::Invalid)),
            "{error:?}"
        );
    }

    #[test]
    fn a_missing_file_keeps_its_own_error() {
        let error = Pieces::open(Path::new("/nonexistent/hex-transcribe.wav")).unwrap_err();
        assert!(matches!(error, PieceError::Io(_)), "{error:?}");
    }

    #[test]
    fn a_piece_is_a_wav_the_whole_file_decoder_reads_back() {
        let samples = paced(16_000, 1);
        let bytes = wav_bytes(&samples).unwrap();
        let clip = AudioClip::decode_wav(Cursor::new(bytes)).unwrap();
        assert_eq!(clip.duration_ms, 1_000);
        assert_eq!(clip.samples, samples);
    }

    #[test]
    fn a_piece_is_kept_without_the_detector_and_it_is_not_consulted() {
        let path = silent_wav("silent", 1.0);
        let mut pieces = Pieces::open(&path).unwrap();
        {
            // The override says "no speech": a kept piece proves the check did
            // not run, so the detector cannot have started its download.
            let _silent = crate::transcription::speech_detector::override_for_test(Some(false));
            assert!(next_speechful_with(&mut pieces, false).unwrap().is_some());
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_installed_detector_skips_a_piece_it_hears_nothing_in() {
        let path = silent_wav("kept", 1.0);
        {
            let mut pieces = Pieces::open(&path).unwrap();
            let _silent = crate::transcription::speech_detector::override_for_test(Some(false));
            assert!(next_speechful_with(&mut pieces, true).unwrap().is_none());
        }
        {
            let mut pieces = Pieces::open(&path).unwrap();
            let _speech = crate::transcription::speech_detector::override_for_test(Some(true));
            assert!(next_speechful_with(&mut pieces, true).unwrap().is_some());
        }
        let _ = std::fs::remove_file(&path);
    }
}

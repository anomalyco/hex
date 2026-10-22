use std::io::{Read, Seek};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use hound::{Sample, SampleFormat, WavReader};

use crate::transcription::{Transcriber, WarmTranscriber};
use crate::transcription_models::TranscriptionSelection;

const COMMAND_CAPACITY: usize = 1;
const MIN_SAMPLE_RATE: u32 = 8_000;
const MAX_SAMPLE_RATE: u32 = 192_000;
const MAX_CHANNELS: u16 = 8;
const MAX_NORMALIZED_AUDIO_BYTES: usize = 64 * 1024 * 1024;
const MAX_NORMALIZED_SAMPLES: usize = MAX_NORMALIZED_AUDIO_BYTES / size_of::<f32>();
/// Pause before asking again while dictation holds the shared engine.
const SHARED_RETRY: Duration = Duration::from_millis(50);
/// How long host audio waits out a dictation, such as a locked one, before
/// using a copy of the service's own.
const SHARED_WAIT: Duration = Duration::from_secs(20);
/// Longer host audio would hold a dictation released during it behind it on
/// the shared engine, so it runs on a copy of the service's own.
const SHARED_MAX_SAMPLES: usize = 30 * 16_000;

pub struct TranscriptionService {
    commands: Option<SyncSender<Command>>,
    worker: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub struct TranscriptionServiceHandle {
    commands: SyncSender<Command>,
    audio_busy: Arc<AtomicBool>,
}

#[derive(Debug)]
pub struct AudioClip {
    samples: Vec<f32>,
    pub duration_ms: u64,
}

pub struct TranscriptionJob {
    canceled: Arc<AtomicBool>,
    result: Receiver<std::result::Result<String, TranscriptionServiceError>>,
}

pub struct AudioAdmission(Arc<AtomicBool>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioError {
    Invalid,
    Unsupported,
    ResourceExhausted,
}

#[derive(Debug)]
pub enum TranscriptionServiceError {
    QueueFull,
    Unavailable,
    Cancelled,
    Model(String),
    Inference(String),
}

enum Command {
    Prepare {
        selection: TranscriptionSelection,
        canceled: Arc<AtomicBool>,
        response: SyncSender<std::result::Result<(), TranscriptionServiceError>>,
    },
    Transcribe {
        selection: TranscriptionSelection,
        clip: AudioClip,
        _admission: AudioAdmission,
        canceled: Arc<AtomicBool>,
        response: SyncSender<std::result::Result<String, TranscriptionServiceError>>,
    },
}

impl TranscriptionService {
    pub fn start() -> Result<(Self, TranscriptionServiceHandle)> {
        let (commands, receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
        let audio_busy = Arc::new(AtomicBool::new(false));
        let worker = thread::Builder::new()
            .name("transcription-service".into())
            .spawn(move || run(receiver))?;
        Ok((
            Self {
                commands: Some(commands.clone()),
                worker: Some(worker),
            },
            TranscriptionServiceHandle {
                commands,
                audio_busy,
            },
        ))
    }

    pub fn shutdown(&mut self) {
        self.commands.take();
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            tracing::error!("transcription service worker panicked during shutdown");
        }
    }
}

impl Drop for TranscriptionService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl TranscriptionServiceHandle {
    pub fn admit_audio(&self) -> Option<AudioAdmission> {
        self.audio_busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| AudioAdmission(self.audio_busy.clone()))
    }

    pub fn prepare(
        &self,
        selection: TranscriptionSelection,
        canceled: Arc<AtomicBool>,
    ) -> std::result::Result<(), TranscriptionServiceError> {
        let (response, result) = mpsc::sync_channel(1);
        self.submit(Command::Prepare {
            selection,
            canceled,
            response,
        })?;
        result
            .recv()
            .unwrap_or(Err(TranscriptionServiceError::Unavailable))
    }

    pub fn transcribe(
        &self,
        selection: TranscriptionSelection,
        clip: AudioClip,
        admission: AudioAdmission,
    ) -> std::result::Result<TranscriptionJob, TranscriptionServiceError> {
        let (response, result) = mpsc::sync_channel(1);
        let canceled = Arc::new(AtomicBool::new(false));
        self.submit(Command::Transcribe {
            selection,
            clip,
            _admission: admission,
            canceled: canceled.clone(),
            response,
        })?;
        Ok(TranscriptionJob { canceled, result })
    }

    fn submit(&self, command: Command) -> std::result::Result<(), TranscriptionServiceError> {
        match self.commands.try_send(command) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(TranscriptionServiceError::QueueFull),
            Err(TrySendError::Disconnected(_)) => Err(TranscriptionServiceError::Unavailable),
        }
    }
}

impl Drop for AudioAdmission {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl TranscriptionJob {
    pub fn wait_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<
        Option<std::result::Result<String, TranscriptionServiceError>>,
        TranscriptionServiceError,
    > {
        match self.result.recv_timeout(timeout) {
            Ok(result) => Ok(Some(result)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(TranscriptionServiceError::Unavailable),
        }
    }

    pub fn cancel(&self) {
        self.canceled.store(true, Ordering::Release);
    }
}

impl Drop for TranscriptionJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl AudioClip {
    pub fn decode_wav<R: Read + Seek>(reader: R) -> std::result::Result<Self, AudioError> {
        let mut reader = WavReader::new(reader).map_err(|_| AudioError::Invalid)?;
        let spec = reader.spec();
        if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&spec.sample_rate)
            || !(1..=MAX_CHANNELS).contains(&spec.channels)
        {
            return Err(AudioError::Unsupported);
        }
        let frames =
            usize::try_from(reader.duration()).map_err(|_| AudioError::ResourceExhausted)?;
        let normalized_samples = frames
            .checked_mul(16_000)
            .and_then(|samples| samples.checked_add(spec.sample_rate as usize - 1))
            .map(|samples| samples / spec.sample_rate as usize)
            .ok_or(AudioError::ResourceExhausted)?;
        if frames == 0
            || frames > MAX_NORMALIZED_SAMPLES
            || normalized_samples > MAX_NORMALIZED_SAMPLES
        {
            return Err(AudioError::ResourceExhausted);
        }
        let channels = usize::from(spec.channels);
        let int_scale = 2_f32.powi(i32::from(spec.bits_per_sample) - 1);
        let mono = match (spec.sample_format, spec.bits_per_sample) {
            (SampleFormat::Float, 32) => {
                decode_mono::<_, f32, _>(&mut reader, channels, frames, |sample| sample)?
            }
            (SampleFormat::Int, 1..=8) => {
                decode_mono::<_, i8, _>(&mut reader, channels, frames, |sample| {
                    f32::from(sample) / 128.0
                })?
            }
            (SampleFormat::Int, 9..=16) => {
                decode_mono::<_, i16, _>(&mut reader, channels, frames, |sample| {
                    f32::from(sample) / int_scale
                })?
            }
            (SampleFormat::Int, 17..=32) => {
                decode_mono::<_, i32, _>(&mut reader, channels, frames, |sample| {
                    sample as f32 / int_scale
                })?
            }
            _ => return Err(AudioError::Unsupported),
        };
        let duration_ms = mono.len() as u64 * 1_000 / u64::from(spec.sample_rate);
        let samples = crate::dictation::try_resample_for_parakeet(&mono, spec.sample_rate)
            .map_err(|_| AudioError::Unsupported)?;
        if samples.len() > MAX_NORMALIZED_SAMPLES {
            return Err(AudioError::ResourceExhausted);
        }
        Ok(Self {
            samples,
            duration_ms,
        })
    }
}

fn decode_mono<R, S, F>(
    reader: &mut WavReader<R>,
    channels: usize,
    frames: usize,
    convert: F,
) -> std::result::Result<Vec<f32>, AudioError>
where
    R: Read,
    S: Sample,
    F: Fn(S) -> f32,
{
    let mut mono = Vec::new();
    mono.try_reserve_exact(frames)
        .map_err(|_| AudioError::ResourceExhausted)?;
    let mut samples = reader.samples::<S>();
    for _ in 0..frames {
        let mut sum = 0.0_f64;
        for _ in 0..channels {
            let sample = samples
                .next()
                .ok_or(AudioError::Invalid)?
                .map_err(|_| AudioError::Invalid)?;
            let sample = convert(sample);
            if !sample.is_finite() {
                return Err(AudioError::Invalid);
            }
            sum += f64::from(sample);
        }
        let average = (sum / channels as f64) as f32;
        if !average.is_finite() {
            return Err(AudioError::Invalid);
        }
        mono.push(average);
    }
    Ok(mono)
}

fn run(commands: Receiver<Command>) {
    crate::parakeet::prioritize_inference_thread();
    let mut active = WarmTranscriber::default();
    while let Ok(command) = commands.recv() {
        match command {
            Command::Prepare {
                selection,
                canceled,
                response,
            } => {
                let _ = response.send(prepare(&mut active, &selection, &canceled));
            }
            Command::Transcribe {
                selection,
                clip,
                _admission,
                canceled,
                response,
            } => {
                let result = transcribe(&mut active, &selection, clip, &canceled);
                let _ = response.send(result);
            }
        }
    }
}

fn check_canceled(canceled: &AtomicBool) -> std::result::Result<(), TranscriptionServiceError> {
    if canceled.load(Ordering::Acquire) {
        Err(TranscriptionServiceError::Cancelled)
    } else {
        Ok(())
    }
}

fn prepare(
    active: &mut WarmTranscriber,
    selection: &TranscriptionSelection,
    canceled: &Arc<AtomicBool>,
) -> std::result::Result<(), TranscriptionServiceError> {
    if with_shared_model(selection, canceled, |_| ())?.is_some() {
        *active = WarmTranscriber::default();
        return Ok(());
    }
    check_canceled(canceled)?;
    load(active, selection).map(drop)
}

fn load<'a>(
    active: &'a mut WarmTranscriber,
    selection: &TranscriptionSelection,
) -> std::result::Result<&'a mut Transcriber, TranscriptionServiceError> {
    active
        .activate(selection)
        .map_err(|error| TranscriptionServiceError::Model(error.to_string()))
}

/// Runs `work` on the dictation engine's loaded model when it is the GGUF
/// model `selection` asks for, so the app never holds a second copy for host
/// audio. `None` when there is no engine (a direct-child service) or it holds
/// another model, or when dictation stays busy for `SHARED_WAIT`. Like
/// meeting chunks, it starts only while no dictation is capturing or
/// transcribing.
fn with_shared_model<R: Send + 'static>(
    selection: &TranscriptionSelection,
    canceled: &Arc<AtomicBool>,
    work: impl FnOnce(&mut Transcriber) -> R + Send + Clone + 'static,
) -> std::result::Result<Option<R>, TranscriptionServiceError> {
    enum Shared<R> {
        Busy,
        Canceled,
        Other,
        Ran(R),
    }
    let Some(engine) = crate::transcription_engine::TranscriptionEngine::shared() else {
        return Ok(None);
    };
    let started = Instant::now();
    loop {
        check_canceled(canceled)?;
        if started.elapsed() >= SHARED_WAIT {
            return Ok(None);
        }
        if crate::parakeet::dictation_busy() {
            thread::sleep(SHARED_RETRY);
            continue;
        }
        let selection = selection.clone();
        let canceled = canceled.clone();
        let work = work.clone();
        let ran = engine.background(move |model| {
            if crate::parakeet::dictation_busy() {
                return Shared::Busy;
            }
            if canceled.load(Ordering::Acquire) {
                return Shared::Canceled;
            }
            match model.active() {
                Some(transcriber @ Transcriber::Gguf(_))
                    if transcriber.matches_selection(&selection) =>
                {
                    Shared::Ran(work(transcriber))
                }
                _ => Shared::Other,
            }
        });
        match ran {
            Ok(Some(Shared::Ran(result))) => return Ok(Some(result)),
            Ok(Some(Shared::Canceled)) => return Err(TranscriptionServiceError::Cancelled),
            Ok(Some(Shared::Other)) | Err(_) => return Ok(None),
            Ok(None | Some(Shared::Busy)) => thread::sleep(SHARED_RETRY),
        }
    }
}

fn transcribe(
    active: &mut WarmTranscriber,
    selection: &TranscriptionSelection,
    clip: AudioClip,
    canceled: &Arc<AtomicBool>,
) -> std::result::Result<String, TranscriptionServiceError> {
    check_canceled(canceled)?;
    let mut samples = clip.samples;
    let engine_has_model = if samples.len() <= SHARED_MAX_SAMPLES {
        let audio = Arc::new(samples);
        let shared = {
            let audio = audio.clone();
            with_shared_model(selection, canceled, move |model| {
                model.transcribe(audio.as_ref().clone())
            })?
        };
        if let Some(transcript) = shared {
            *active = WarmTranscriber::default();
            check_canceled(canceled)?;
            return transcript
                .map_err(|error| TranscriptionServiceError::Inference(error.to_string()));
        }
        samples = Arc::unwrap_or_clone(audio);
        false
    } else {
        with_shared_model(selection, canceled, |_| ())?.is_some()
    };
    let model = load(active, selection)?;
    check_canceled(canceled)?;
    let transcript = model.transcribe(samples);
    // A long clip's copy is only temporary when the engine holds the model.
    if engine_has_model {
        *active = WarmTranscriber::default();
    }
    let transcript =
        transcript.map_err(|error| TranscriptionServiceError::Inference(error.to_string()))?;
    check_canceled(canceled)?;
    Ok(transcript)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use hound::{WavSpec, WavWriter};

    use super::*;

    #[test]
    fn wav_decode_downmixes_and_resamples_without_a_duration_limit() {
        let mut wav = Cursor::new(Vec::new());
        {
            let mut writer = WavWriter::new(
                &mut wav,
                WavSpec {
                    channels: 2,
                    sample_rate: 48_000,
                    bits_per_sample: 16,
                    sample_format: SampleFormat::Int,
                },
            )
            .unwrap();
            for _ in 0..61 * 48_000 {
                writer.write_sample(16_384_i16).unwrap();
                writer.write_sample(-16_384_i16).unwrap();
            }
            writer.finalize().unwrap();
        }
        wav.set_position(0);

        let clip = AudioClip::decode_wav(wav).unwrap();

        assert_eq!(clip.duration_ms, 61_000);
        assert_eq!(clip.samples.len(), 61 * 16_000);
        assert!(clip.samples.iter().all(|sample| sample.abs() < 0.001));
    }

    #[test]
    fn invalid_wav_is_rejected() {
        assert_eq!(
            AudioClip::decode_wav(Cursor::new(b"not a wav".to_vec())).unwrap_err(),
            AudioError::Invalid
        );
    }

    #[test]
    fn hostile_wav_metadata_is_rejected_before_audio_allocation() {
        let mut wav = wav_bytes(WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        });
        let data = wav.windows(4).position(|bytes| bytes == b"data").unwrap();
        let declared_bytes = u32::try_from((MAX_NORMALIZED_SAMPLES + 1) * 2).unwrap();
        wav[data + 4..data + 8].copy_from_slice(&declared_bytes.to_le_bytes());

        assert_eq!(
            AudioClip::decode_wav(Cursor::new(wav)).unwrap_err(),
            AudioError::ResourceExhausted
        );
    }

    #[test]
    fn unsupported_sample_rates_are_rejected_before_resampling() {
        let wav = wav_bytes(WavSpec {
            channels: 1,
            sample_rate: 1,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        });

        assert_eq!(
            AudioClip::decode_wav(Cursor::new(wav)).unwrap_err(),
            AudioError::Unsupported
        );
    }

    #[test]
    fn float_downmix_accumulates_large_finite_channels_without_overflow() {
        let mut wav = Cursor::new(Vec::new());
        {
            let mut writer = WavWriter::new(
                &mut wav,
                WavSpec {
                    channels: 8,
                    sample_rate: 16_000,
                    bits_per_sample: 32,
                    sample_format: SampleFormat::Float,
                },
            )
            .unwrap();
            for _ in 0..8 {
                writer.write_sample(f32::MAX).unwrap();
            }
            writer.finalize().unwrap();
        }
        wav.set_position(0);

        let clip = AudioClip::decode_wav(wav).unwrap();
        assert!(clip.samples.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn audio_admission_is_exclusive() {
        let (_service, handle) = TranscriptionService::start().unwrap();
        let admission = handle.admit_audio().unwrap();
        assert!(handle.admit_audio().is_none());
        drop(admission);
        assert!(handle.admit_audio().is_some());
    }

    #[test]
    fn dropping_a_pending_job_requests_cancellation() {
        let canceled = Arc::new(AtomicBool::new(false));
        let (_sender, result) = mpsc::sync_channel(1);
        let job = TranscriptionJob {
            canceled: canceled.clone(),
            result,
        };

        drop(job);

        assert!(canceled.load(Ordering::Acquire));
    }

    fn wav_bytes(spec: WavSpec) -> Vec<u8> {
        let mut wav = Cursor::new(Vec::new());
        {
            let mut writer = WavWriter::new(&mut wav, spec).unwrap();
            writer.write_sample(0_i16).unwrap();
            writer.finalize().unwrap();
        }
        wav.into_inner()
    }
}

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::Instant;

use color_eyre::eyre::{Result, WrapErr, eyre};
use transcribe_cpp::{Backend, Model, ModelOptions, RunOptions, TimestampKind};

use crate::app_settings::TranscriptionDevice;
use crate::transcription::gguf_session::{OfflineGgufSession, output_truncated};
use crate::transcription::gpu_health;
use crate::transcription::models::{
    TranscriptionModelId, TranscriptionSelection, download_with_progress, model_path, validate,
};
use crate::transcription::{pad_gguf_samples, requires_closing_mark, trailing_silence_samples};

/// A word or segment's start and end (ms from the clip's start) and text.
pub type TimedSpan = (i64, i64, String);

pub struct LocalTranscriber {
    session: OfflineGgufSession,
    selection: TranscriptionSelection,
    options: RunOptions,
    device_label: String,
    /// Where it runs, for Settings and `hex status`.
    backend: crate::transcription::SpeechBackend,
    max_audio_samples: Option<usize>,
    /// The finest timestamps the model gives, for a capture's or a
    /// meeting's word times.
    finest_timestamps: TimestampKind,
    /// The GPU it runs on; `None` on the CPU.
    gpu: Option<transcribe_cpp::Device>,
    /// The device setting it loaded for.
    preference: TranscriptionDevice,
    /// A warm run of [`PREWARM_SAMPLES`] once loaded, in milliseconds.
    warm_ms: u128,
    /// It loaded on the CPU because the GPU ran out of memory, as it does
    /// when another copy of a model already fills it (#1170).
    gpu_full: bool,
    /// The GPU out-of-memory fallback it took mid-run, for the dictation
    /// engine to publish in Settings (#1984). Not saved.
    out_of_memory: Option<gpu_health::Failure>,
}

/// 1.5 s of silence: what loading runs to warm a model and time it.
const PREWARM_SAMPLES: usize = 24_000;

/// An integrated GPU slower than this on a warm 1.5 s run (a fifth of real
/// time) may lose to the CPU, so Auto times the CPU too (#484). Healthy
/// GPUs run it in tens of milliseconds; the UHD 620 and Iris Plus 655 that
/// missed streaming deadlines ran a 35 s clip at about 0.4× real time.
const SLOW_WARM_MS: u128 = 300;

/// Settings › Speech engine (`transcription_device`), for every later load.
static DEVICE: AtomicU8 = AtomicU8::new(0);

pub(crate) fn set_device(device: TranscriptionDevice) {
    DEVICE.store(device as u8, Ordering::Relaxed);
}

pub(crate) fn device() -> TranscriptionDevice {
    match DEVICE.load(Ordering::Relaxed) {
        1 => TranscriptionDevice::Gpu,
        2 => TranscriptionDevice::Cpu,
        _ => TranscriptionDevice::Auto,
    }
}

pub fn install(selection: &TranscriptionSelection, canceled: &AtomicBool) -> Result<()> {
    let model = validate(selection)?;
    let downloaded = AtomicU64::new(0);
    println!("Installing {} ({})...", model.name, model.size_label());
    let path = download_with_progress(&model, canceled, &downloaded)?;
    println!(
        "Installed {} bytes at {}",
        downloaded.load(Ordering::Relaxed),
        path.display()
    );
    Ok(())
}

/// This launch registered the CPU alone: Speech engine was CPU when
/// [`init_backends`] ran, or someone chose GPU after the crash marker was
/// cleared (`retry_gpu_start`), so the GPU needs a restart.
static CPU_CHOSEN: AtomicBool = AtomicBool::new(false);

/// Registers transcribe.cpp's backends once per process and returns the
/// backend models load on: every GPU (`Auto`), or only the CPU when Speech
/// engine is CPU, so no GPU driver loads (#1253), or on Windows after this
/// build's launches crashed twice while ggml started a GPU (#662). A GPU
/// driver crash can't be caught, so a marker file spans that work.
pub fn init_backends() -> Result<Backend> {
    static BACKEND: std::sync::OnceLock<Backend> = std::sync::OnceLock::new();
    if let Some(backend) = BACKEND.get() {
        return Ok(*backend);
    }
    transcribe_cpp::init_logging();
    let cpu_only = || {
        transcribe_cpp::init_backends_with(
            None::<&Path>,
            transcribe_cpp::BackendMask::for_backend(Backend::Cpu),
        )
        .wrap_err("could not initialize the CPU transcription backend")
    };
    // A person who chose CPU is never a crash fallback, and needs no marker.
    if device() == TranscriptionDevice::Cpu {
        cpu_only()?;
        CPU_CHOSEN.store(true, Ordering::Relaxed);
        return Ok(*BACKEND.get_or_init(|| Backend::Cpu));
    }
    #[cfg(target_os = "windows")]
    if gpu_guard::crashed_before() {
        gpu_health::set_crash_fallback();
        tracing::warn!(
            marker = %gpu_guard::marker().map(|path| path.display().to_string()).unwrap_or_default(),
            "this build crashed starting the GPU for transcription twice; transcribing on the CPU (Settings › Speech engine › Auto or GPU tries it again)"
        );
        cpu_only()?;
        return Ok(*BACKEND.get_or_init(|| Backend::Cpu));
    }
    let init = || {
        transcribe_cpp::init_backends_default()?;
        // Enumerating the devices makes sure every backend started now.
        drop(transcribe_cpp::devices());
        Ok::<_, transcribe_cpp::Error>(())
    };
    #[cfg(target_os = "windows")]
    let init = || gpu_guard::guarded(init);
    init().wrap_err("could not initialize transcription backends")?;
    Ok(*BACKEND.get_or_init(|| Backend::Auto))
}

/// The GPU needs a restart: this launch registered the CPU alone, and the
/// Speech engine now asks for the GPU (left CPU, or a person chose GPU after
/// a crash marker cleared it).
pub(crate) fn gpu_after_restart() -> bool {
    CPU_CHOSEN.load(Ordering::Relaxed) && device() != TranscriptionDevice::Cpu
}

/// How many GPU starts this build may crash before it stays on the CPU: one
/// automatic retry after a crash, and the CPU until an update or Settings ›
/// Speech engine › GPU after a second one.
#[cfg(any(target_os = "windows", test))]
const MAX_GPU_START_ATTEMPTS: u32 = 2;

/// What the `gpu-start.pending` marker says this launch should do about the
/// GPU.
#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GpuStart {
    /// Start the GPU; `crashed` GPU starts already crashed under this build.
    Attempt { crashed: u32 },
    /// This build already crashed starting the GPU as often as it may: keep
    /// the CPU.
    CrashedTooOften,
}

/// A `gpu-start.pending` marker: the build it belongs to, and how many GPU
/// starts crashed under that build.
#[cfg(any(target_os = "windows", test))]
struct GpuStartMarker {
    version: String,
    crashed: u32,
}

#[cfg(any(target_os = "windows", test))]
impl GpuStartMarker {
    /// Reads `<version> <crashed>`. A bare `<version>` is the first format,
    /// written before attempts were counted, and counts as one crashed start
    /// so the launch after it still gets a retry. Text without a readable
    /// count counts as one attempt too, never as a reason to give up.
    fn parse(text: &str) -> Self {
        let text = text.trim();
        let (version, crashed) = match text.split_once(char::is_whitespace) {
            Some((version, crashed)) => (version, crashed.trim().parse().unwrap_or(1)),
            None => (text, 1),
        };
        Self {
            version: version.to_owned(),
            crashed,
        }
    }
}

/// This launch's GPU start, from the marker at `path`. A marker from another
/// build is deleted, so an update tries the GPU again.
#[cfg(any(target_os = "windows", test))]
fn gpu_start_at(path: &Path) -> GpuStart {
    let Ok(text) = std::fs::read_to_string(path) else {
        return GpuStart::Attempt { crashed: 0 };
    };
    let marker = GpuStartMarker::parse(&text);
    if marker.version != crate::VERSION {
        let _ = std::fs::remove_file(path);
        return GpuStart::Attempt { crashed: 0 };
    }
    if marker.crashed >= MAX_GPU_START_ATTEMPTS {
        GpuStart::CrashedTooOften
    } else {
        GpuStart::Attempt {
            crashed: marker.crashed,
        }
    }
}

/// Runs `work` with `<version> <crashed + 1>` in `path`, so a crash inside it
/// is recorded before it can happen; a normal return removes the marker.
/// Every scope in one launch passes the same `crashed`, so an earlier scope
/// that succeeded does not restart this launch's count.
#[cfg(any(target_os = "windows", test))]
fn guarded_at<T>(path: &Path, crashed: u32, work: impl FnOnce() -> T) -> T {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(error) = std::fs::write(
        path,
        format!("{} {}", crate::VERSION, crashed.saturating_add(1)),
    ) {
        tracing::warn!(%error, path = %path.display(), "could not write the GPU start marker");
    }
    let out = work();
    let _ = std::fs::remove_file(path);
    out
}

/// The marker a Windows launch keeps while ggml starts a GPU (`init_backends`,
/// a model's load and prewarm). Left behind, that launch crashed in a GPU
/// driver; it holds the build's version and the crashed starts so far, so
/// this build retries once and then stays on the CPU until an update or
/// Settings › GPU asks for the GPU again.
#[cfg(target_os = "windows")]
mod gpu_guard {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// The crashed starts this launch began with, read once, so every guarded
    /// scope records this launch's attempt rather than a fresh first one.
    static CRASHED: AtomicU32 = AtomicU32::new(0);

    pub(super) fn marker() -> Option<PathBuf> {
        crate::app_paths::support_dir()
            .ok()
            .map(|dir| dir.join("gpu-start.pending"))
    }

    pub(super) fn crashed_before() -> bool {
        let Some(path) = marker() else {
            return false;
        };
        match super::gpu_start_at(&path) {
            super::GpuStart::Attempt { crashed } => {
                CRASHED.store(crashed, Ordering::Relaxed);
                false
            }
            super::GpuStart::CrashedTooOften => true,
        }
    }

    pub(super) fn guarded<T>(work: impl FnOnce() -> T) -> T {
        match marker() {
            Some(path) => super::guarded_at(&path, CRASHED.load(Ordering::Relaxed), work),
            None => work(),
        }
    }

    /// A person chose GPU in Settings: forget the crashes, so the next GPU
    /// start is attempted again.
    pub(super) fn clear() {
        if let Some(path) = marker() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Re-picking Auto or GPU in Settings after this launch registered the CPU
/// alone asks for the GPU again: the next launch gets a fresh attempt (#662),
/// and Settings says the GPU turns on at restart. The crash marker is cleared
/// too, so a launch that chose CPU after the crashes cannot leave it for the
/// next one. A launch already on the GPU changes nothing, and keeps a
/// write-ahead marker a load in progress depends on: no guarded GPU scope
/// runs when only the CPU registered.
#[cfg(target_os = "windows")]
pub(crate) fn retry_gpu_start() {
    if !(gpu_health::crash_fallback() || CPU_CHOSEN.load(Ordering::Relaxed)) {
        return;
    }
    CPU_CHOSEN.store(true, Ordering::Relaxed);
    gpu_guard::clear();
}

pub fn devices() -> Result<Vec<String>> {
    init_backends()?;
    Ok(transcribe_cpp::devices()
        .into_iter()
        .map(|device| {
            format!(
                "{}\t{}\t{} MB\t{}",
                device
                    .index
                    .map_or_else(|| "-".into(), |index| index.to_string()),
                device.kind,
                device.memory_total / (1024 * 1024),
                if device.description.is_empty() {
                    device.name
                } else {
                    device.description
                }
            )
        })
        .collect())
}

/// Initializes transcribe.cpp's backends, which creates its Vulkan instance,
/// on the calling thread. The Linux process calls this before its listener
/// or window can start: two Vulkan instances created at once in one process
/// (ggml's here, wgpu's for the window) crash inside the Vulkan loader.
/// Later model loads reuse the instance.
#[cfg(target_os = "linux")]
pub fn initialize_backends() {
    if let Err(error) = init_backends() {
        tracing::warn!(%error, "could not initialize transcription backends");
    }
}

pub fn transcribe_wav_with_selection(
    path: &Path,
    selection: &TranscriptionSelection,
) -> Result<String> {
    let samples = read_wav_for_parakeet(path)?;
    LocalTranscriber::load(selection)?.transcribe(&samples)
}

pub fn read_wav_for_parakeet(path: &Path) -> Result<Vec<f32>> {
    let file =
        std::fs::File::open(path).wrap_err_with(|| format!("could not open {}", path.display()))?;
    read_wav(std::io::BufReader::new(file))
}

fn read_wav(reader: impl std::io::Read + std::io::Seek) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::new(reader)?;
    let spec = reader.spec();
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int if spec.bits_per_sample <= 16 => reader
            .samples::<i16>()
            .map(|sample| sample.map(|sample| sample as f32 / i16::MAX as f32))
            .collect::<std::result::Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => reader
            .samples::<i32>()
            .map(|sample| sample.map(|sample| sample as f32 / i32::MAX as f32))
            .collect::<std::result::Result<Vec<_>, _>>()?,
    };
    let channels = usize::from(spec.channels);
    let mono = samples
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect::<Vec<_>>();
    let mut samples = crate::dictation::resample_for_parakeet(&mono, spec.sample_rate);
    crate::dictation::pad_for_parakeet(&mut samples);
    Ok(samples)
}

impl LocalTranscriber {
    /// Loads and prewarms `selection` where Settings › Speech engine says:
    /// on Auto a GPU unless it failed before under its current driver
    /// ([`gpu_health`], #856), on GPU the best GPU, on CPU the CPU. A GPU load
    /// or prewarm that fails retries on the CPU, and when the CPU works the
    /// GPU is remembered as failed.
    pub fn load(selection: &TranscriptionSelection) -> Result<Self> {
        Self::load_cancellable(selection, &AtomicBool::new(false))
    }

    pub(crate) fn load_cancellable(
        selection: &TranscriptionSelection,
        canceled: &AtomicBool,
    ) -> Result<Self> {
        let loaded = Self::load_where(selection, canceled)?;
        gpu_health::set_auto_on_cpu(
            loaded.preference == TranscriptionDevice::Auto && loaded.gpu.is_none(),
        );
        Ok(loaded)
    }

    fn load_where(selection: &TranscriptionSelection, canceled: &AtomicBool) -> Result<Self> {
        check_canceled(canceled)?;
        let backend = init_backends()?;
        let preference = device();
        if backend == Backend::Cpu || preference == TranscriptionDevice::Cpu {
            return Self::load_on(selection, Backend::Cpu, None, preference, canceled);
        }
        let gpus = gpu_health::gpus();
        let (backend, target) = if preference == TranscriptionDevice::Gpu
            || !gpus.iter().any(gpu_health::failed)
        {
            (Backend::Auto, None)
        } else {
            let healthy = gpus
                .iter()
                .find(|gpu| !gpu_health::failed(gpu) && gpu_health::backend(gpu).is_some());
            match healthy.and_then(|gpu| Some((gpu_health::backend(gpu)?, gpu.clone()))) {
                Some((backend, gpu)) => (backend, Some(gpu)),
                None => {
                    tracing::info!(
                        "this GPU failed or ran slower than the CPU before, under its current driver; transcribing on the CPU (Settings › Speech engine › GPU tries it again)"
                    );
                    return Self::load_on(selection, Backend::Cpu, None, preference, canceled);
                }
            }
        };
        // The GPU this load runs on: the one chosen, else the one Auto picks.
        let expected = target.clone().or_else(|| gpus.into_iter().next());
        let on_gpu = || Self::load_on(selection, backend, target, preference, canceled);
        #[cfg(target_os = "windows")]
        let on_gpu = || gpu_guard::guarded(on_gpu);
        match on_gpu() {
            Ok(loaded) => {
                if preference == TranscriptionDevice::Gpu
                    && let Some(gpu) = &loaded.gpu
                {
                    gpu_health::forget(gpu);
                }
                if preference == TranscriptionDevice::Auto && loaded.slow_igpu() {
                    check_canceled(canceled)?;
                    return Ok(loaded.or_faster_cpu(canceled));
                }
                Ok(loaded)
            }
            Err(error) => {
                check_canceled(canceled)?;
                tracing::warn!(
                    error = format!("{error:#}"),
                    "could not load the transcription model on the GPU; loading it on the CPU"
                );
                let mut loaded =
                    Self::load_on(selection, Backend::Cpu, None, preference, canceled)?;
                // The CPU loads the same model, so the GPU failed, unless it
                // only ran out of memory (another copy can fill it, #1165).
                if let Some(gpu) = &expected {
                    if gpu_health::out_of_memory(&error) {
                        tracing::info!(
                            "the GPU ran out of memory; not remembering it as failed, Auto tries it again next load"
                        );
                        loaded.gpu_full = true;
                    } else {
                        gpu_health::record(gpu, &format!("{error:#}"));
                    }
                }
                Ok(loaded)
            }
        }
    }

    fn load_on(
        selection: &TranscriptionSelection,
        backend: Backend,
        target: Option<transcribe_cpp::Device>,
        preference: TranscriptionDevice,
        canceled: &AtomicBool,
    ) -> Result<Self> {
        check_canceled(canceled)?;
        let definition = validate(selection)?;
        if !crate::transcription::models::is_installed(&definition, &selection.language) {
            return Err(eyre!(
                "{} is not installed; run `cargo run -- model install`",
                definition.name
            ));
        }
        let path = model_path(&definition)?;
        check_canceled(canceled)?;
        let model = Model::load_with(
            &path,
            &ModelOptions {
                backend,
                device: target,
                ..Default::default()
            },
        )
        .wrap_err_with(|| format!("could not load transcription model from {}", path.display()))?;
        check_canceled(canceled)?;
        let device = model.device()?;
        let variant = model.variant();
        let architecture = model.arch();
        let capabilities = model.capabilities()?;
        let language = crate::transcription::gguf_session::validate_gguf_artifact(
            &path,
            &definition,
            &architecture,
            &variant,
            selection,
            capabilities.supports_language_detect,
        )?;
        let options = RunOptions {
            timestamps: if capabilities.max_timestamp_kind == TimestampKind::None {
                TimestampKind::None
            } else {
                TimestampKind::Segment
            },
            language,
            ..Default::default()
        };
        let device_label = format!("{} ({})", device.description, device.kind);
        let backend = crate::transcription::SpeechBackend {
            kind: device.kind.clone(),
            device: device.description.clone(),
            crash_fallback: gpu_health::crash_fallback(),
        };
        tracing::info!(
            backend = device.kind,
            device = device.description,
            key = gpu_health::device_key(&device),
            driver = gpu_health::driver(),
            variant,
            "loaded local transcription model"
        );
        let session = OfflineGgufSession::new(model)?;
        let mut transcriber = Self {
            session,
            selection: selection.clone(),
            options,
            device_label,
            backend,
            max_audio_samples: crate::transcription::models::max_audio_chunk_samples(
                &architecture,
                capabilities.max_audio_ms,
            ),
            finest_timestamps: match capabilities.max_timestamp_kind {
                TimestampKind::Word | TimestampKind::Token => TimestampKind::Word,
                kind => kind,
            },
            gpu: matches!(
                device.device_type,
                transcribe_cpp::DeviceType::Gpu | transcribe_cpp::DeviceType::Igpu
            )
            .then(|| device.clone()),
            preference,
            warm_ms: 0,
            gpu_full: false,
            out_of_memory: None,
        };
        check_canceled(canceled)?;
        let started = Instant::now();
        transcriber.warm_on_silence()?;
        tracing::info!(
            prewarm_ms = started.elapsed().as_millis(),
            "prewarmed local transcription model"
        );
        check_canceled(canceled)?;
        transcriber
            .session
            .probe_stream(&transcriber.options, &path);
        check_canceled(canceled)?;
        // Warm now, so this times the device rather than its first run's
        // shader compiles.
        let started = Instant::now();
        transcriber.warm_on_silence()?;
        transcriber.warm_ms = started.elapsed().as_millis();
        Ok(transcriber)
    }

    /// Runs [`PREWARM_SAMPLES`] of silence through the model, without the
    /// vocabulary prompt: with it, a decoder such as Qwen3-ASR reads a long
    /// term list back to its output cap on every warm-up (#1568). A decode
    /// that still runs to that cap ran the whole graph on the device, so
    /// that is a warm model, not a failed device.
    fn warm_on_silence(&mut self) -> Result<()> {
        let run = |this: &mut Self| {
            this.session
                .warm(&vec![0.0; PREWARM_SAMPLES], &this.options)
                .map(drop)
                .wrap_err("transcription failed")
        };
        let result = match run(self) {
            // A GPU that fails mid-run reloads on the CPU, as a transcript
            // does; the CPU model is warmed by its own load.
            Err(error) if self.gpu_failed(&error) => self.recover_on_cpu(&error),
            result => result,
        };
        match result {
            Err(error) if output_truncated(&error) => {
                tracing::info!(
                    error = format!("{error:#}"),
                    "the warm-up run on silence hit the output limit; the model is warm"
                );
                Ok(())
            }
            result => result,
        }
    }

    pub(crate) fn stream_options(&self) -> Option<&transcribe_cpp::StreamOptions> {
        self.session.stream_options()
    }

    /// Re-probes the live stream when Labs › Faster live text or an env
    /// override changed, so the next capture streams with it.
    pub(crate) fn refresh_stream(&mut self) {
        self.session.refresh_stream(&self.options);
    }

    /// Silence a live stream of this model hears before it finalizes, the
    /// same trailing silence its whole-clip runs get.
    pub(crate) fn stream_trailing_silence(&self) -> usize {
        trailing_silence_samples(Some(self.model_id()))
    }

    /// Whether a live stream of this model must end its text with closing
    /// sentence punctuation before it is trusted, as Unified English's does
    /// (issue 1933).
    pub(crate) fn stream_requires_closing_mark(&self) -> bool {
        requires_closing_mark(Some(self.model_id()))
    }

    /// Whether this model streams with Parakeet's buffered chunk menus, whose
    /// first-word timestamp lags its audio.
    pub(crate) fn streams_buffered_parakeet(&mut self) -> bool {
        self.session.streams_buffered_parakeet()
    }

    /// The model this transcriber was loaded for.
    pub(crate) fn model_id(&self) -> TranscriptionModelId {
        self.selection.model
    }

    pub(crate) fn with_stream<R>(
        &mut self,
        stream_options: &transcribe_cpp::StreamOptions,
        cancel: &transcribe_cpp::CancelToken,
        drive: impl FnOnce(&mut transcribe_cpp::Stream<'_>) -> R,
    ) -> transcribe_cpp::Result<R> {
        self.session
            .with_stream(&self.options, stream_options, cancel, drive)
    }

    /// The selection this model was loaded for, so the transcription engine
    /// can say which model it holds.
    pub(crate) fn gguf_selection(&self) -> Option<&TranscriptionSelection> {
        Some(&self.selection)
    }

    pub(crate) fn matches_selection(&self, selection: &TranscriptionSelection) -> bool {
        &self.selection == selection
    }

    /// The out-of-memory fallback this model took, for the dictation engine
    /// to publish in Settings (#1984). `None` on a GPU.
    pub(crate) fn out_of_memory(&self) -> Option<&gpu_health::Failure> {
        self.out_of_memory.as_ref()
    }

    /// Brings an idle model's weights back before audio arrives, as loading
    /// does; a model that ran recently is already warm.
    pub(crate) fn prewarm(&mut self) -> Result<()> {
        if self.session.ran_recently() {
            return Ok(());
        }
        self.warm_on_silence()
            .wrap_err("could not prewarm transcription model")
    }

    /// Transcribes a complete 16 kHz clip, padded as a dictation's clip is:
    /// the minimum duration, then Unified English's trailing silence.
    pub(crate) fn transcribe_clip(&mut self, mut samples: Vec<f32>) -> Result<String> {
        pad_gguf_samples(&mut samples, Some(self.model_id()));
        self.transcribe(&samples)
    }

    /// [`Self::transcribe_clip`] with the model's own segment and word times,
    /// chunked and padded exactly as the untimed run chunks and pads it, so
    /// the text matches and each chunk's times sit on the clip's timeline.
    pub(crate) fn transcribe_timed(
        &mut self,
        mut samples: Vec<f32>,
    ) -> Result<crate::transcription::transcript::Transcript> {
        pad_gguf_samples(&mut samples, Some(self.model_id()));
        let options = RunOptions {
            timestamps: self.finest_timestamps,
            ..self.options.clone()
        };
        match self
            .session
            .transcribe_chunked_timed(&samples, &options, self.max_audio_samples)
        {
            Err(error) if self.gpu_failed(&error) => {
                self.recover_on_cpu(&error)?;
                self.session
                    .transcribe_chunked_timed(&samples, &options, self.max_audio_samples)
            }
            result => result,
        }
    }

    /// The finest native timings this model gives, as a timed request needs
    /// them. Never the level a caller asked for.
    pub(crate) fn timestamps(&self) -> crate::transcription::transcript::TimestampLevel {
        crate::transcription::gguf_session::timestamp_level(self.finest_timestamps)
    }

    pub fn device_label(&self) -> &str {
        &self.device_label
    }

    /// Where this model runs, for `hex status` and Settings.
    pub(crate) fn backend(&self) -> &crate::transcription::SpeechBackend {
        &self.backend
    }

    /// Text and word times (ms from the clip's start) for a capture, in
    /// chunks the model takes whole. A model without word times gives its
    /// segments instead.
    pub fn transcribe_words(&mut self, samples: &[f32]) -> Result<(String, Vec<TimedSpan>)> {
        let transcript = self.timed(samples)?;
        let spans = if transcript.words.is_empty() {
            transcript.segments
        } else {
            transcript.words
        };
        let words = spans
            .into_iter()
            .map(|word| (word.start_ms, word.end_ms, word.text))
            .collect();
        Ok((transcript.text, words))
    }

    /// One meeting chunk with segment and word timings, padded as dictation
    /// pads a clip.
    pub(crate) fn transcribe_segments(
        &mut self,
        mut samples: Vec<f32>,
    ) -> Result<crate::transcription::transcript::Transcript> {
        crate::dictation::pad_for_parakeet(&mut samples);
        self.timed(&samples)
    }

    /// Text, segments, and words, in chunks the model takes whole.
    fn timed(&mut self, samples: &[f32]) -> Result<crate::transcription::transcript::Transcript> {
        match self.timed_here(samples) {
            Err(error) if self.gpu_failed(&error) => {
                self.recover_on_cpu(&error)?;
                self.timed_here(samples)
            }
            result => result,
        }
    }

    /// Whether this model runs on an integrated GPU slow enough that the
    /// CPU may be faster.
    fn slow_igpu(&self) -> bool {
        self.warm_ms > SLOW_WARM_MS
            && self
                .gpu
                .as_ref()
                .is_some_and(|gpu| gpu.device_type == transcribe_cpp::DeviceType::Igpu)
    }

    /// This model, or the same one on the CPU when the CPU runs it faster;
    /// then the GPU is remembered as slow, so later loads go straight to the
    /// CPU (#484).
    fn or_faster_cpu(self, canceled: &AtomicBool) -> Self {
        let cpu = match Self::load_on(
            &self.selection,
            Backend::Cpu,
            None,
            self.preference,
            canceled,
        ) {
            Ok(cpu) => cpu,
            Err(error) => {
                tracing::warn!(
                    error = format!("{error:#}"),
                    "could not time the model on the CPU; keeping the GPU"
                );
                return self;
            }
        };
        tracing::info!(
            gpu_ms = self.warm_ms,
            cpu_ms = cpu.warm_ms,
            "timed a warm 1.5 s run on the integrated GPU and the CPU"
        );
        if cpu.warm_ms >= self.warm_ms {
            return self;
        }
        if let Some(gpu) = &self.gpu {
            gpu_health::record_slow(
                gpu,
                &format!(
                    "a warm 1.5 s run took {} ms on the GPU and {} ms on the CPU",
                    self.warm_ms, cpu.warm_ms
                ),
            );
        }
        cpu
    }

    /// Whether `error` came from the GPU this model runs on failing.
    fn gpu_failed(&self, error: &color_eyre::eyre::Report) -> bool {
        self.gpu.is_some() && gpu_health::device_error(error)
    }

    /// Reloads this model on the CPU after its GPU failed mid-run (a hang
    /// or reset comes back as an error, #856), remembering the GPU once the
    /// CPU works. An out-of-memory failure is carried by the CPU model for
    /// the dictation engine to show in Settings (#1984).
    fn recover_on_cpu(&mut self, error: &color_eyre::eyre::Report) -> Result<()> {
        tracing::warn!(
            error = format!("{error:#}"),
            "the GPU failed while transcribing; reloading the model on the CPU"
        );
        let gpu = self.gpu.take();
        let mut cpu = Self::load_on(
            &self.selection,
            Backend::Cpu,
            None,
            self.preference,
            &AtomicBool::new(false),
        )?;
        if let Some(gpu) = &gpu {
            if gpu_health::out_of_memory(error) {
                cpu.out_of_memory = Some(gpu_health::out_of_memory_failure(
                    gpu,
                    &format!("{error:#}"),
                ));
            } else {
                gpu_health::record(gpu, &format!("{error:#}"));
            }
        }
        gpu_health::set_auto_on_cpu(self.preference == TranscriptionDevice::Auto);
        *self = cpu;
        Ok(())
    }

    fn timed_here(
        &mut self,
        samples: &[f32],
    ) -> Result<crate::transcription::transcript::Transcript> {
        use crate::transcription::gguf_session::{chunk_ranges, sample_offset_ms};
        use crate::transcription::transcript::TranscriptSegment;
        const CHUNK: usize = 30 * 16_000;
        let chunk = self.max_audio_samples.map_or(CHUNK, |max| max.min(CHUNK));
        let options = RunOptions {
            timestamps: self.finest_timestamps,
            ..self.options.clone()
        };
        let span = |offset: i64, t0: i64, t1: i64, text: String| {
            let text = text.trim().to_owned();
            (!text.is_empty()).then(|| TranscriptSegment {
                start_ms: t0.max(0) + offset,
                end_ms: t1.max(t0) + offset,
                text,
            })
        };
        let mut timed = crate::transcription::transcript::Transcript::default();
        let mut texts = Vec::new();
        for range in chunk_ranges(samples, chunk) {
            let offset = sample_offset_ms(range.start);
            let transcript = self
                .session
                .run(&samples[range], &options)
                .wrap_err("transcription failed")?;
            let text = transcript.text.trim();
            if !text.is_empty() {
                texts.push(text.to_owned());
            }
            timed.segments.extend(
                transcript
                    .segments
                    .into_iter()
                    .filter_map(|s| span(offset, s.t0_ms, s.t1_ms, s.text)),
            );
            timed.words.extend(
                transcript
                    .words
                    .into_iter()
                    .filter_map(|w| span(offset, w.t0_ms, w.t1_ms, w.text)),
            );
        }
        timed.text = texts.join(" ");
        Ok(timed)
    }

    pub fn transcribe(&mut self, samples: &[f32]) -> Result<String> {
        let run = |this: &mut Self| {
            this.session
                .transcribe_chunked(samples, &this.options, this.max_audio_samples)
        };
        match run(self) {
            Err(error) if self.gpu_failed(&error) => {
                self.recover_on_cpu(&error)?;
                run(self)
            }
            result => result,
        }
    }
}

/// The local API's own model, loaded for the selection a host asks for
/// when the dictation engine holds another one.
#[derive(Default)]
pub(crate) struct WarmLocal {
    active: Option<LocalTranscriber>,
}

impl WarmLocal {
    /// The loaded model, without switching or loading one.
    pub(crate) fn active(&mut self) -> Option<&mut LocalTranscriber> {
        self.active.as_mut()
    }

    pub(crate) fn activate(
        &mut self,
        selection: &TranscriptionSelection,
    ) -> Result<&mut LocalTranscriber> {
        if !self
            .active
            .as_ref()
            .is_some_and(|model| model.matches_selection(selection))
        {
            self.active = None;
            self.active = Some(LocalTranscriber::load(selection)?);
        }
        Ok(self
            .active
            .as_mut()
            .expect("activated transcriber must be available"))
    }
}

fn check_canceled(canceled: &AtomicBool) -> Result<()> {
    if canceled.load(Ordering::Acquire) {
        return Err(eyre!("model load was canceled"));
    }
    Ok(())
}

/// A model loading on a worker thread.
type Loading = std::sync::mpsc::Receiver<std::result::Result<LocalTranscriber, String>>;
/// An in-place engine reload confirming whether it committed.
type Reloading = std::sync::mpsc::Receiver<std::result::Result<(), String>>;

fn spawn_model_load(selection: TranscriptionSelection, cancel: Arc<AtomicBool>) -> Result<Loading> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("hex-model-swap".into())
        .spawn(move || {
            let _ = sender.send(
                LocalTranscriber::load_cancellable(&selection, &cancel)
                    .map_err(|error| format!("{error:#}")),
            );
        })?;
    Ok(receiver)
}

enum SwapState {
    /// Running `(loaded, device)` with no swap in flight.
    Idle,
    /// Loading `wanted` beside the running model on one worker thread.
    /// A newer `want` sets `cancel` and records the newest target in `queued`
    /// so two loads never race for VRAM at the same time.
    Loading {
        wanted: TranscriptionSelection,
        device: TranscriptionDevice,
        cancel: Arc<AtomicBool>,
        receiver: Loading,
        queued: Option<(TranscriptionSelection, TranscriptionDevice)>,
    },
    /// Loaded beside the running model and waiting for the listener to go
    /// idle so it can swap in.
    Ready {
        selection: TranscriptionSelection,
        transcriber: Box<LocalTranscriber>,
    },
    /// Loading beside the running model found the GPU full: waiting for the
    /// listener to go idle so the engine can drop its model and reload
    /// `selection` in place (#1170).
    NeedsReload {
        selection: TranscriptionSelection,
        device: TranscriptionDevice,
    },
    /// The engine is reloading `wanted` in place; `loaded` advances only once
    /// `receiver` confirms `Ok(())`.
    Reloading {
        wanted: TranscriptionSelection,
        device: TranscriptionDevice,
        receiver: Reloading,
        queued: Option<(TranscriptionSelection, TranscriptionDevice)>,
    },
}

/// A running listener's model: the one it transcribes with, and the one it
/// changes to. A new selection loads on one worker thread while dictation goes
/// on with the old model; the listener swaps it in once nothing records or
/// waits on the old one. A model the host already prepared skips the load.
pub(crate) struct ModelSwap {
    loaded: TranscriptionSelection,
    /// The device setting the running model loaded for.
    device: TranscriptionDevice,
    state: SwapState,
}

impl ModelSwap {
    pub(crate) fn new(loaded: &TranscriptionSelection) -> Self {
        Self {
            loaded: super::custom_models::runnable(loaded),
            device: device(),
            state: SwapState::Idle,
        }
    }

    /// The selection and device setting the listener will transcribe with
    /// once swaps settle.
    fn target(&self) -> (&TranscriptionSelection, TranscriptionDevice) {
        match &self.state {
            SwapState::Idle => (&self.loaded, self.device),
            SwapState::Loading {
                wanted,
                device,
                cancel,
                queued,
                ..
            } => match queued {
                Some((selection, device)) => (selection, *device),
                None if cancel.load(Ordering::Acquire) => (&self.loaded, self.device),
                None => (wanted, *device),
            },
            SwapState::Ready {
                selection,
                transcriber,
            } => (selection, transcriber.preference),
            SwapState::NeedsReload { selection, device } => (selection, *device),
            SwapState::Reloading {
                wanted,
                device,
                queued,
                ..
            } => match queued {
                Some((selection, device)) => (selection, *device),
                None => (wanted, *device),
            },
        }
    }

    /// A model the host loaded for `selection` (Linux prepares models
    /// beside the listener; Windows lets the listener load them).
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub(crate) fn offer(
        &mut self,
        selection: &TranscriptionSelection,
        transcriber: LocalTranscriber,
    ) {
        if let SwapState::Loading { cancel, .. } = &self.state {
            cancel.store(true, Ordering::Release);
        }
        self.place(selection.clone(), transcriber);
    }

    /// Keeps a loaded model to swap in, unless it fell back to the CPU only
    /// because the running model fills the GPU: then it is dropped, and the
    /// engine reloads the selection once its own model is gone.
    fn place(&mut self, selection: TranscriptionSelection, transcriber: LocalTranscriber) {
        if transcriber.gpu_full {
            tracing::info!(
                "the GPU has no room for the new model beside the running one; loading it in its place"
            );
            let device = transcriber.preference;
            drop(transcriber);
            self.state = SwapState::NeedsReload { selection, device };
        } else {
            self.state = SwapState::Ready {
                selection,
                transcriber: Box::new(transcriber),
            };
        }
    }

    /// Settings now select `selection`; loads it unless it is already the
    /// target.
    pub(crate) fn want(&mut self, selection: &TranscriptionSelection) {
        self.want_with(selection, spawn_model_load);
    }

    fn want_with(
        &mut self,
        selection: &TranscriptionSelection,
        spawn: impl FnOnce(TranscriptionSelection, Arc<AtomicBool>) -> Result<Loading>,
    ) {
        // An import that can't run stays saved; the listener runs the
        // default model meanwhile.
        let selection = super::custom_models::runnable(selection);
        let next_device = device();
        if (&selection, next_device) == self.target() {
            return;
        }
        match &mut self.state {
            SwapState::Loading { cancel, queued, .. } => {
                // Never overlap two background loads: cancel the stale one and
                // start the newest target once it exits.
                cancel.store(true, Ordering::Release);
                *queued = if selection == self.loaded && next_device == self.device {
                    None
                } else {
                    Some((selection, next_device))
                };
                return;
            }
            SwapState::Reloading {
                wanted,
                device,
                queued,
                ..
            } => {
                // Wait for the engine's in-place reload to settle before
                // starting another load beside it.
                *queued = if selection == *wanted && next_device == *device {
                    None
                } else {
                    Some((selection, next_device))
                };
                return;
            }
            SwapState::Idle | SwapState::Ready { .. } | SwapState::NeedsReload { .. } => {}
        }
        if selection == self.loaded && next_device == self.device {
            self.state = SwapState::Idle;
            return;
        }
        self.start_load_with(selection, next_device, spawn);
    }

    fn start_load_with(
        &mut self,
        selection: TranscriptionSelection,
        next_device: TranscriptionDevice,
        spawn: impl FnOnce(TranscriptionSelection, Arc<AtomicBool>) -> Result<Loading>,
    ) {
        let cancel = Arc::new(AtomicBool::new(false));
        match spawn(selection.clone(), cancel.clone()) {
            Ok(receiver) => {
                self.state = SwapState::Loading {
                    wanted: selection,
                    device: next_device,
                    cancel,
                    receiver,
                    queued: None,
                };
            }
            Err(error) => {
                tracing::warn!(%error, "could not load the new transcription model");
                self.state = SwapState::Idle;
            }
        }
    }

    /// Collects a finished load or in-place engine reload; a failure keeps the
    /// current model and reports it unless superseded.
    pub(crate) fn poll(&mut self) -> Option<SwapFailed> {
        self.poll_with(spawn_model_load)
    }

    fn poll_with(
        &mut self,
        spawn: impl FnOnce(TranscriptionSelection, Arc<AtomicBool>) -> Result<Loading>,
    ) -> Option<SwapFailed> {
        match &self.state {
            SwapState::Loading { receiver, .. } => {
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(std::sync::mpsc::TryRecvError::Empty) => return None,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        Err("the model loader stopped unexpectedly".into())
                    }
                };
                let SwapState::Loading {
                    wanted,
                    cancel,
                    queued,
                    ..
                } = std::mem::replace(&mut self.state, SwapState::Idle)
                else {
                    unreachable!();
                };
                if cancel.load(Ordering::Acquire) {
                    drop(result);
                    if let Some((next_sel, next_dev)) = queued
                        && (next_sel != self.loaded || next_dev != self.device)
                    {
                        self.start_load_with(next_sel, next_dev, spawn);
                    }
                    return None;
                }
                match result {
                    Ok(transcriber) => {
                        self.place(wanted, transcriber);
                        None
                    }
                    Err(error) => Some(SwapFailed {
                        wanted,
                        kept: self.loaded.clone(),
                        error: format!("could not switch the transcription model: {error}"),
                    }),
                }
            }
            SwapState::Reloading { receiver, .. } => {
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(std::sync::mpsc::TryRecvError::Empty) => return None,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        Err("the transcription engine stopped".into())
                    }
                };
                let SwapState::Reloading {
                    wanted,
                    device,
                    queued,
                    ..
                } = std::mem::replace(&mut self.state, SwapState::Idle)
                else {
                    unreachable!();
                };
                match result {
                    Ok(()) => {
                        self.loaded = wanted;
                        self.device = device;
                        if let Some((next_sel, next_dev)) = queued
                            && (next_sel != self.loaded || next_dev != self.device)
                        {
                            self.start_load_with(next_sel, next_dev, spawn);
                        }
                        None
                    }
                    Err(error) => {
                        if let Some((next_sel, next_dev)) = queued {
                            if next_sel != self.loaded || next_dev != self.device {
                                self.start_load_with(next_sel, next_dev, spawn);
                            }
                            None
                        } else {
                            Some(SwapFailed {
                                wanted,
                                kept: self.loaded.clone(),
                                error: format!("could not switch the transcription model: {error}"),
                            })
                        }
                    }
                }
            }
            SwapState::Idle | SwapState::Ready { .. } | SwapState::NeedsReload { .. } => None,
        }
    }

    /// The model to swap in now, if one is ready.
    pub(crate) fn take_ready(&mut self) -> Option<LocalTranscriber> {
        if !matches!(self.state, SwapState::Ready { .. }) {
            return None;
        }
        let SwapState::Ready {
            selection,
            transcriber,
        } = std::mem::replace(&mut self.state, SwapState::Idle)
        else {
            unreachable!();
        };
        self.loaded = selection;
        self.device = transcriber.preference;
        Some(*transcriber)
    }

    /// Starts an in-place engine reload when the GPU had no room for two
    /// copies ([`TranscriptionEngine::reload`]). `loaded` advances only once
    /// the returned receiver confirms `Ok(())` in [`Self::poll`].
    ///
    /// [`TranscriptionEngine::reload`]: crate::transcription::engine::TranscriptionEngine::reload
    pub(crate) fn start_reload(
        &mut self,
        reload: impl FnOnce(TranscriptionSelection) -> Result<Reloading>,
    ) {
        if !matches!(self.state, SwapState::NeedsReload { .. }) {
            return;
        }
        let SwapState::NeedsReload { selection, device } =
            std::mem::replace(&mut self.state, SwapState::Idle)
        else {
            unreachable!();
        };
        match reload(selection.clone()) {
            Ok(receiver) => {
                self.state = SwapState::Reloading {
                    wanted: selection,
                    device,
                    receiver,
                    queued: None,
                };
            }
            Err(error) => {
                tracing::warn!(%error, "could not replace the transcription model");
            }
        }
    }
}

/// A model change that failed to load; the listener still runs `kept`.
/// Windows hands it to the host, which puts settings back on `kept`; Linux
/// only reports the error.
#[derive(Debug)]
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) struct SwapFailed {
    pub(crate) wanted: TranscriptionSelection,
    pub(crate) kept: TranscriptionSelection,
    pub(crate) error: String,
}

#[cfg(test)]
mod model_swap_tests {
    use super::*;
    use crate::transcription::models::TranscriptionModelId;

    fn lang(code: &str) -> TranscriptionSelection {
        TranscriptionSelection {
            model: TranscriptionModelId::default(),
            language: code.into(),
        }
    }

    #[test]
    fn an_unchanged_selection_loads_nothing() {
        let selection = lang("en");
        let mut swap = ModelSwap::new(&selection);
        swap.want(&selection);
        assert!(matches!(swap.state, SwapState::Idle));
        assert!(swap.take_ready().is_none());
        assert!(swap.poll().is_none());
    }

    #[test]
    fn a_model_without_room_on_the_gpu_reloads_in_place_and_commits_loaded_on_ok() {
        let old = lang("en");
        let new = lang("de");
        let mut swap = ModelSwap::new(&old);
        swap.state = SwapState::NeedsReload {
            selection: new.clone(),
            device: device(),
        };
        // Already the target: nothing loads beside the running model.
        swap.want(&new);
        assert!(matches!(swap.state, SwapState::NeedsReload { .. }));

        let (tx, rx) = std::sync::mpsc::channel();
        let mut started = Vec::new();
        swap.start_reload(|sel| {
            started.push(sel);
            Ok(rx)
        });
        assert_eq!(started.len(), 1);
        assert_eq!(started[0], new);
        // `loaded` has NOT advanced yet while the engine reload is in flight.
        assert_eq!(swap.loaded, old);
        assert!(matches!(swap.state, SwapState::Reloading { .. }));
        // Repeated want(&new) while reloading does nothing.
        swap.want(&new);
        assert!(swap.poll().is_none());

        // Engine confirms commit -> `loaded` advances and state returns to Idle.
        tx.send(Ok(())).unwrap();
        assert!(swap.poll().is_none());
        assert_eq!(swap.loaded, new);
        assert!(matches!(swap.state, SwapState::Idle));
        swap.want(&new);
        assert!(matches!(swap.state, SwapState::Idle));
    }

    #[test]
    fn a_failed_in_place_reload_does_not_advance_loaded_and_reports_failure() {
        let old = lang("en");
        let new = lang("de");
        let mut swap = ModelSwap::new(&old);
        swap.state = SwapState::NeedsReload {
            selection: new.clone(),
            device: device(),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        swap.start_reload(|_| Ok(rx));
        tx.send(Err("out of memory".into())).unwrap();

        let failed = swap.poll().expect("reload failure must be reported");
        assert_eq!(failed.wanted, new);
        assert_eq!(failed.kept, old);
        assert!(failed.error.contains("out of memory"));
        assert_eq!(swap.loaded, old, "failed reload must not advance loaded");
        assert!(matches!(swap.state, SwapState::Idle));
    }

    #[test]
    fn overlapping_wants_cancel_the_stale_load_and_start_the_newest_after_it_exits() {
        let a = lang("en");
        let b = lang("de");
        let c = lang("fr");
        let mut swap = ModelSwap::new(&a);

        let mut spawns: Vec<(TranscriptionSelection, Arc<AtomicBool>)> = Vec::new();
        let (tx_b, rx_b) = std::sync::mpsc::channel();
        swap.want_with(&b, |sel, cancel| {
            spawns.push((sel, cancel));
            Ok(rx_b)
        });
        assert_eq!(spawns.len(), 1);
        assert_eq!(spawns[0].0, b);
        assert!(!spawns[0].1.load(Ordering::Acquire));

        // Second want while B is still loading must NOT spawn a second thread;
        // it cancels B and queues C.
        swap.want_with(&c, |_, _| panic!("must not overlap two loads"));
        assert!(
            spawns[0].1.load(Ordering::Acquire),
            "stale load is canceled"
        );
        assert_eq!(swap.target().0, &c);

        // When B finishes (even with an error), its result is fenced off and C
        // starts cleanly in its place.
        tx_b.send(Err("canceled".into())).unwrap();
        let (tx_c, rx_c) = std::sync::mpsc::channel();
        let failed = swap.poll_with(|sel, cancel| {
            spawns.push((sel, cancel));
            Ok(rx_c)
        });
        assert!(failed.is_none(), "canceled load must not emit SwapFailed");
        assert_eq!(spawns.len(), 2);
        assert_eq!(spawns[1].0, c);
        assert!(!spawns[1].1.load(Ordering::Acquire));

        // Canceling C back to A leaves no queued target and returns to Idle.
        swap.want_with(&a, |_, _| panic!("reverting to loaded spawns nothing"));
        assert!(spawns[1].1.load(Ordering::Acquire));
        assert_eq!(swap.target().0, &a);
        tx_c.send(Err("canceled".into())).unwrap();
        assert!(
            swap.poll_with(|_, _| panic!("no queued load when back on loaded"))
                .is_none()
        );
        assert!(matches!(swap.state, SwapState::Idle));
    }
}

/// The Windows GPU-start marker's rules, which are platform-neutral so every
/// Linux test build checks them too (#662).
#[cfg(test)]
mod gpu_start_tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn marker_path(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("hex-gpu-start-{name}-{unique}.pending"))
    }

    fn marker(text: &str) -> String {
        format!("{} {text}", crate::VERSION)
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn a_marker_reads_its_version_and_crashed_starts() {
        let bare = GpuStartMarker::parse("1.2.3");
        assert_eq!((bare.version.as_str(), bare.crashed), ("1.2.3", 1));
        let counted = GpuStartMarker::parse(" 1.2.3 2\n");
        assert_eq!((counted.version.as_str(), counted.crashed), ("1.2.3", 2));
        let unreadable = GpuStartMarker::parse("1.2.3 two");
        assert_eq!(
            (unreadable.version.as_str(), unreadable.crashed),
            ("1.2.3", 1)
        );
        let empty = GpuStartMarker::parse(" ");
        assert_eq!((empty.version.as_str(), empty.crashed), ("", 1));
    }

    #[test]
    fn no_marker_starts_the_gpu() {
        let path = marker_path("absent");
        assert_eq!(gpu_start_at(&path), GpuStart::Attempt { crashed: 0 });
    }

    #[test]
    fn a_marker_from_another_build_is_forgotten_and_the_gpu_tried() {
        let path = marker_path("other-build");
        std::fs::write(&path, "0.0.1 5").unwrap();
        assert_eq!(gpu_start_at(&path), GpuStart::Attempt { crashed: 0 });
        assert!(!path.exists(), "the old build's marker is deleted");
    }

    #[test]
    fn the_first_formats_bare_version_gets_one_retry() {
        // Written before attempts were counted: one crashed start, so this
        // launch tries the GPU and records attempt 2.
        let path = marker_path("bare");
        std::fs::write(&path, crate::VERSION).unwrap();
        assert_eq!(gpu_start_at(&path), GpuStart::Attempt { crashed: 1 });
        guarded_at(&path, 1, || {
            assert_eq!(read(&path), marker("2"));
        });
        assert!(!path.exists(), "a normal start removes the marker");
    }

    #[test]
    fn two_crashed_starts_keep_the_cpu() {
        let path = marker_path("twice");
        std::fs::write(&path, marker("2")).unwrap();
        assert_eq!(gpu_start_at(&path), GpuStart::CrashedTooOften);
        assert!(
            path.exists(),
            "the marker stays for an update or Settings › GPU"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_unreadable_count_still_gets_a_retry() {
        let path = marker_path("unreadable");
        std::fs::write(&path, marker("two")).unwrap();
        assert_eq!(gpu_start_at(&path), GpuStart::Attempt { crashed: 1 });
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn every_guarded_scope_records_this_launchs_attempt() {
        // The first scope succeeded and removed the marker; the second scope
        // in the same launch still writes attempt 2, not a fresh attempt 1.
        let path = marker_path("scopes");
        guarded_at(&path, 1, || {});
        assert!(!path.exists());
        guarded_at(&path, 1, || {
            assert_eq!(read(&path), marker("2"));
        });
        assert!(!path.exists());
    }
}

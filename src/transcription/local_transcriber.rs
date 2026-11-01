use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use color_eyre::eyre::{Result, WrapErr, eyre};
use transcribe_cpp::{Backend, Model, ModelOptions, RunOptions, TimestampKind};

use crate::transcription::gguf_session::OfflineGgufSession;
use crate::transcription::models::{
    TranscriptionSelection, download_with_progress, model_path, validate,
};

/// A word or segment's start and end (ms from the clip's start) and text.
pub type TimedSpan = (i64, i64, String);

pub struct LocalTranscriber {
    session: OfflineGgufSession,
    selection: TranscriptionSelection,
    options: RunOptions,
    device_label: String,
    max_audio_samples: Option<usize>,
    stream: Option<transcribe_cpp::StreamOptions>,
    /// The finest timestamps the model gives, for a capture's or a
    /// meeting's word times.
    finest_timestamps: TimestampKind,
}

pub fn install(selection: &TranscriptionSelection, canceled: &AtomicBool) -> Result<()> {
    let model = validate(selection)?;
    let downloaded = AtomicU64::new(0);
    println!("Installing {} ({})...", model.name, model.size_label());
    let path = download_with_progress(model, canceled, &downloaded)?;
    println!(
        "Installed {} bytes at {}",
        downloaded.load(Ordering::Relaxed),
        path.display()
    );
    Ok(())
}

/// Registers transcribe.cpp's backends once per process and returns the
/// backend models load on: every GPU (`Auto`), or on Windows only the CPU
/// after a launch of this build crashed while ggml started a GPU (#662). A
/// GPU driver crash can't be caught, so a marker file spans that work.
pub fn init_backends() -> Result<Backend> {
    static BACKEND: std::sync::OnceLock<Backend> = std::sync::OnceLock::new();
    if let Some(backend) = BACKEND.get() {
        return Ok(*backend);
    }
    transcribe_cpp::init_logging();
    #[cfg(target_os = "windows")]
    if gpu_guard::crashed_before() {
        tracing::warn!(
            marker = %gpu_guard::marker().map(|path| path.display().to_string()).unwrap_or_default(),
            "a previous launch of this Hex crashed while starting the GPU for transcription; transcribing on the CPU until Hex updates or the marker is deleted"
        );
        transcribe_cpp::init_backends_with(
            None::<&Path>,
            transcribe_cpp::BackendMask::for_backend(Backend::Cpu),
        )
        .wrap_err("could not initialize the CPU transcription backend")?;
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

/// The marker a Windows launch keeps while ggml starts a GPU (`init_backends`,
/// a model's load and prewarm). Left behind, that launch crashed in a GPU
/// driver; it holds the build's version, so an update tries the GPU again.
#[cfg(target_os = "windows")]
mod gpu_guard {
    use std::path::PathBuf;

    pub(super) fn marker() -> Option<PathBuf> {
        crate::app_paths::support_dir()
            .ok()
            .map(|dir| dir.join("gpu-start.pending"))
    }

    pub(super) fn crashed_before() -> bool {
        let Some(path) = marker() else {
            return false;
        };
        match std::fs::read_to_string(&path) {
            Ok(version) if version == crate::VERSION => true,
            Ok(_) => {
                let _ = std::fs::remove_file(&path);
                false
            }
            Err(_) => false,
        }
    }

    pub(super) fn guarded<T>(work: impl FnOnce() -> T) -> T {
        let path = marker();
        if let Some(path) = &path {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(error) = std::fs::write(path, crate::VERSION) {
                tracing::warn!(%error, "could not write the GPU start marker");
            }
        }
        let out = work();
        if let Some(path) = &path {
            let _ = std::fs::remove_file(path);
        }
        out
    }
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
    let mut reader = hound::WavReader::open(path)
        .wrap_err_with(|| format!("could not open {}", path.display()))?;
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
    /// Loads and prewarms `selection` on the GPU when one starts, else on the
    /// CPU: a GPU load or prewarm that fails retries on the CPU.
    pub fn load(selection: &TranscriptionSelection) -> Result<Self> {
        let backend = init_backends()?;
        let on_gpu = || Self::load_on(selection, backend);
        #[cfg(target_os = "windows")]
        let on_gpu = || {
            if backend == Backend::Auto {
                gpu_guard::guarded(on_gpu)
            } else {
                on_gpu()
            }
        };
        match on_gpu() {
            Err(error) if backend != Backend::Cpu => {
                tracing::warn!(%error, "could not load the transcription model on the GPU; loading it on the CPU");
                Self::load_on(selection, Backend::Cpu)
            }
            loaded => loaded,
        }
    }

    fn load_on(selection: &TranscriptionSelection, backend: Backend) -> Result<Self> {
        let definition = validate(selection)?;
        if !crate::transcription::models::is_installed(definition, &selection.language) {
            return Err(eyre!(
                "{} is not installed; run `cargo run -- model install`",
                definition.name
            ));
        }
        let path = model_path(definition)?;
        let model = Model::load_with(
            &path,
            &ModelOptions {
                backend,
                ..Default::default()
            },
        )
        .wrap_err_with(|| format!("could not load transcription model from {}", path.display()))?;
        let device = model.device()?;
        let variant = model.variant();
        let architecture = model.arch();
        let capabilities = model.capabilities()?;
        let language = crate::transcription::gguf_session::validate_gguf_artifact(
            &path,
            definition,
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
        tracing::info!(
            backend = device.kind,
            device = device.description,
            variant,
            "loaded local transcription model"
        );
        let session = OfflineGgufSession::new(model)?;
        let mut transcriber = Self {
            session,
            selection: selection.clone(),
            options,
            device_label,
            max_audio_samples: crate::transcription::models::max_audio_chunk_samples(
                &architecture,
                capabilities.max_audio_ms,
            ),
            stream: None,
            finest_timestamps: match capabilities.max_timestamp_kind {
                TimestampKind::Word | TimestampKind::Token => TimestampKind::Word,
                kind => kind,
            },
        };
        let started = Instant::now();
        transcriber.transcribe(&vec![0.0; 24_000])?;
        tracing::info!(
            prewarm_ms = started.elapsed().as_millis(),
            "prewarmed local transcription model"
        );
        transcriber.stream = transcriber
            .session
            .probe_stream(&transcriber.options, &architecture);
        Ok(transcriber)
    }

    pub(crate) fn stream_options(&self) -> Option<&transcribe_cpp::StreamOptions> {
        self.stream.as_ref()
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

    /// Brings an idle model's weights back before audio arrives, as loading
    /// does; a model that ran recently is already warm.
    pub(crate) fn prewarm(&mut self) -> Result<()> {
        if self.session.ran_recently() {
            return Ok(());
        }
        self.transcribe(&vec![0.0; 24_000])
            .wrap_err("could not prewarm transcription model")
            .map(drop)
    }

    /// Transcribes a complete 16 kHz clip, padded as a dictation's clip is.
    pub(crate) fn transcribe_clip(&mut self, mut samples: Vec<f32>) -> Result<String> {
        crate::dictation::pad_for_parakeet(&mut samples);
        self.transcribe(&samples)
    }

    pub fn device_label(&self) -> &str {
        &self.device_label
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
        for (index, part) in samples.chunks(chunk.max(1)).enumerate() {
            let offset = (index * chunk / 16) as i64;
            let transcript = self
                .session
                .run(part, &options)
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
        self.session
            .transcribe_chunked(samples, &self.options, self.max_audio_samples)
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

/// A running listener's model: the one it transcribes with, and the one it
/// changes to. A new selection loads on a worker thread while dictation goes
/// on with the old model; the listener swaps it in once nothing records or
/// waits on the old one. A model the host already prepared skips the load.
pub(crate) struct ModelSwap {
    loaded: TranscriptionSelection,
    loading: Option<(
        TranscriptionSelection,
        std::sync::mpsc::Receiver<std::result::Result<LocalTranscriber, String>>,
    )>,
    ready: Option<(TranscriptionSelection, LocalTranscriber)>,
}

impl ModelSwap {
    pub(crate) fn new(loaded: &TranscriptionSelection) -> Self {
        Self {
            loaded: super::custom_models::runnable(loaded),
            loading: None,
            ready: None,
        }
    }

    /// The selection the listener will transcribe with once swaps settle.
    fn target(&self) -> &TranscriptionSelection {
        self.ready
            .as_ref()
            .map(|(selection, _)| selection)
            .or(self.loading.as_ref().map(|(selection, _)| selection))
            .unwrap_or(&self.loaded)
    }

    /// A model the host loaded for `selection` (Linux prepares models
    /// beside the listener; Windows lets the listener load them).
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub(crate) fn offer(
        &mut self,
        selection: &TranscriptionSelection,
        transcriber: LocalTranscriber,
    ) {
        self.loading = None;
        self.ready = Some((selection.clone(), transcriber));
    }

    /// Settings now select `selection`; loads it unless it is already the
    /// target.
    pub(crate) fn want(&mut self, selection: &TranscriptionSelection) {
        // An import that can't run stays saved; the listener runs the
        // default model meanwhile.
        let selection = super::custom_models::runnable(selection);
        if &selection == self.target() {
            return;
        }
        self.ready = None;
        if selection == self.loaded {
            self.loading = None;
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let load = selection.clone();
        let spawned = std::thread::Builder::new()
            .name("hex-model-swap".into())
            .spawn(move || {
                let _ = sender
                    .send(LocalTranscriber::load(&load).map_err(|error| format!("{error:#}")));
            });
        match spawned {
            Ok(_) => self.loading = Some((selection, receiver)),
            Err(error) => tracing::warn!(%error, "could not load the new transcription model"),
        }
    }

    /// Collects a finished load; a failure keeps the current model.
    pub(crate) fn poll(&mut self) -> Option<SwapFailed> {
        let (_, receiver) = self.loading.as_ref()?;
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("the model loader stopped unexpectedly".into())
            }
        };
        let (selection, _) = self.loading.take()?;
        match result {
            Ok(transcriber) => {
                self.ready = Some((selection, transcriber));
                None
            }
            Err(error) => Some(SwapFailed {
                wanted: selection,
                kept: self.loaded.clone(),
                error: format!("could not switch the transcription model: {error}"),
            }),
        }
    }

    /// The model to swap in now, if one is ready.
    pub(crate) fn take_ready(&mut self) -> Option<LocalTranscriber> {
        let (selection, transcriber) = self.ready.take()?;
        self.loaded = selection;
        Some(transcriber)
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

    #[test]
    fn an_unchanged_selection_loads_nothing() {
        let selection = TranscriptionSelection {
            model: TranscriptionModelId::default(),
            ..Default::default()
        };
        let mut swap = ModelSwap::new(&selection);
        swap.want(&selection);
        assert!(swap.loading.is_none());
        assert!(swap.take_ready().is_none());
        assert!(swap.poll().is_none());
    }
}

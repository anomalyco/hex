use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use color_eyre::eyre::{Result, WrapErr, eyre};
use transcribe_cpp::{Backend, Model, ModelOptions, RunOptions, Session, TimestampKind};

use crate::transcription_models::{
    ModelDefinition, TranscriptionModelId, TranscriptionSelection, definition,
    download_with_progress, model_path, validate,
};

pub struct LinuxTranscriber {
    session: Session,
    options: RunOptions,
    device_label: String,
    max_audio_samples: Option<usize>,
}

pub fn default_model() -> &'static ModelDefinition {
    definition(TranscriptionModelId::ParakeetV2)
}

pub fn install_default(canceled: &AtomicBool) -> Result<()> {
    let model = default_model();
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

pub fn devices() -> Result<Vec<String>> {
    transcribe_cpp::init_logging();
    transcribe_cpp::init_backends_default()
        .wrap_err("could not initialize transcription backends")?;
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

pub fn transcribe_wav(path: &Path) -> Result<String> {
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
    LinuxTranscriber::load_default()?.transcribe(&samples)
}

impl LinuxTranscriber {
    pub fn load_default() -> Result<Self> {
        Self::load(&TranscriptionSelection::default())
    }

    pub fn load(selection: &TranscriptionSelection) -> Result<Self> {
        transcribe_cpp::init_logging();
        transcribe_cpp::init_backends_default()
            .wrap_err("could not initialize transcription backends")?;
        let definition = validate(selection)?;
        if !crate::transcription_models::is_installed(definition, &selection.language) {
            return Err(eyre!(
                "{} is not installed; run `cargo run -- model install`",
                definition.name
            ));
        }
        let path = model_path(definition)?;
        let model = Model::load_with(
            &path,
            &ModelOptions {
                backend: Backend::Auto,
                gpu_device: 0,
            },
        )
        .wrap_err_with(|| format!("could not load transcription model from {}", path.display()))?;
        let device = model.device()?;
        let variant = model.variant();
        let architecture = model.arch();
        let crate::transcription_models::ModelRuntime::Gguf(artifact) = definition.runtime else {
            return Err(eyre!(
                "{} is not a GGUF transcription model",
                definition.name
            ));
        };
        if architecture != artifact.architecture || variant != artifact.variant {
            return Err(eyre!(
                "{} contains {architecture}/{variant}, expected {}/{}",
                path.display(),
                artifact.architecture,
                artifact.variant
            ));
        }
        let capabilities = model.capabilities();
        let language = definition
            .accepts_language_hint
            .then(|| definition.runtime_language(&selection.language).to_string());
        let device_label = format!("{} ({})", device.description, device.kind);
        tracing::info!(
            backend = device.kind,
            device = device.description,
            variant,
            "loaded Linux transcription model"
        );
        let session = model.session()?;
        let mut transcriber = Self {
            session,
            options: RunOptions {
                timestamps: if capabilities.max_timestamp_kind == TimestampKind::None {
                    TimestampKind::None
                } else {
                    TimestampKind::Segment
                },
                language,
                ..Default::default()
            },
            device_label,
            max_audio_samples: (capabilities.max_audio_ms > 0)
                .then_some(capabilities.max_audio_ms as usize * 16),
        };
        let started = Instant::now();
        transcriber.transcribe(&vec![0.0; 24_000])?;
        tracing::info!(
            prewarm_ms = started.elapsed().as_millis(),
            "prewarmed Linux transcription model"
        );
        Ok(transcriber)
    }

    pub fn device_label(&self) -> &str {
        &self.device_label
    }

    pub fn transcribe(&mut self, samples: &[f32]) -> Result<String> {
        let Some(max_samples) = self.max_audio_samples else {
            return self.run(samples);
        };
        let mut output = Vec::new();
        for samples in samples.chunks(max_samples) {
            let text = self.run(samples)?;
            if !text.trim().is_empty() {
                output.push(text);
            }
        }
        Ok(output.join(" "))
    }

    fn run(&mut self, samples: &[f32]) -> Result<String> {
        self.session
            .run(samples, &self.options)
            .map(|transcript| transcript.text)
            .wrap_err("transcription failed")
    }
}

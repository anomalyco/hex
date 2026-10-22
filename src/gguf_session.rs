//! The `transcribe-cpp` session owner shared by the macOS, Linux, and Windows
//! GGUF transcribers. It recreates the native session around every run (and
//! every live stream) so retained scheduler scratch never outlives the
//! inference that sized it.

use std::path::Path;

use color_eyre::eyre::{Result, eyre};

use crate::transcription_models::{
    AUTO_LANGUAGE, ModelDefinition, ModelRuntime, TranscriptionSelection,
};

/// 8 s of audio reaches every stream window shape a dictation can use (the
/// left context grows to 5.6 s), so the GPU driver compiles those kernels
/// during launch. With a 1 s probe, a new build's first dictation compiled
/// them in its finish step and took 3-4 s on an RTX 5070 Ti.
const STREAM_PREWARM_SAMPLES: usize = 8 * 16_000;

pub(crate) fn validate_gguf_artifact(
    path: &Path,
    definition: &ModelDefinition,
    architecture: &str,
    variant: &str,
    selection: &TranscriptionSelection,
    supports_language_detect: bool,
) -> Result<Option<String>> {
    let ModelRuntime::Gguf(artifact) = definition.runtime else {
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
    if selection.language == AUTO_LANGUAGE && !supports_language_detect {
        return Err(eyre!(
            "{} does not advertise automatic language detection",
            definition.name
        ));
    }
    Ok(definition
        .runtime_language_hint(&selection.language)
        .map(str::to_string))
}

// On GPU devices only the per-token decoder runs on CPU; extra spin-waiting ggml
// workers (default up to 8) stall it under load, so keep it on the calling thread.
fn session_options(device_type: transcribe_cpp::DeviceType) -> transcribe_cpp::SessionOptions {
    match device_type {
        transcribe_cpp::DeviceType::Gpu | transcribe_cpp::DeviceType::Igpu => {
            transcribe_cpp::SessionOptions {
                n_threads: 1,
                ..Default::default()
            }
        }
        _ => transcribe_cpp::SessionOptions::default(),
    }
}

pub(crate) struct OfflineGgufSession {
    model: transcribe_cpp::Model,
    session: Option<transcribe_cpp::Session>,
    options: transcribe_cpp::SessionOptions,
}

impl OfflineGgufSession {
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn set_threads_for_test(&mut self, n_threads: i32) -> transcribe_cpp::Result<()> {
        self.options.n_threads = n_threads;
        self.session = None;
        self.session = Some(self.model.session_with(&self.options)?);
        Ok(())
    }

    pub(crate) fn new(model: transcribe_cpp::Model) -> transcribe_cpp::Result<Self> {
        let options = session_options(model.device()?.device_type);
        let session = model.session_with(&options)?;
        Ok(Self {
            model,
            session: Some(session),
            options,
        })
    }

    /// Streams only on a GPU (Metal, Vulkan): a CPU stream re-encodes each
    /// overlapping window and would cost several times the offline compute.
    /// The probe also prewarms the stream graph (see
    /// [`STREAM_PREWARM_SAMPLES`]). Without an extension
    /// Parakeet buffers with its largest left, chunk, and right context
    /// (Unified English: 5.6 s, 1.04 s, 1.04 s), its most accurate and least
    /// compute-intensive setting. `HEX_STREAMING=0` keeps the model offline
    /// and `HEX_STREAMING=1` streams on any device (diagnostics and CI);
    /// `HEX_STREAM_CHUNK_MS` overrides the chunk and right context.
    pub(crate) fn probe_stream(
        &mut self,
        options: &transcribe_cpp::RunOptions,
        architecture: &str,
    ) -> Option<transcribe_cpp::StreamOptions> {
        use transcribe_cpp::DeviceType;
        let forced = match std::env::var("HEX_STREAMING").as_deref() {
            Ok("0") => return None,
            Ok("1") => true,
            _ => false,
        };
        if !self.model.capabilities().ok()?.supports_streaming {
            return None;
        }
        let device_type = self.model.device().ok()?.device_type;
        if !forced && !matches!(device_type, DeviceType::Gpu | DeviceType::Igpu) {
            return None;
        }
        let chunk_ms = std::env::var("HEX_STREAM_CHUNK_MS")
            .ok()
            .and_then(|value| value.parse::<i32>().ok());
        let stream_options = transcribe_cpp::StreamOptions {
            family: (architecture == "parakeet" && chunk_ms.is_some()).then_some(
                transcribe_cpp::StreamExtension::ParakeetBuffered(
                    transcribe_cpp::ParakeetBufferedStreamOptions {
                        left_ms: None,
                        chunk_ms,
                        right_ms: chunk_ms,
                    },
                ),
            ),
            ..Default::default()
        };
        let started = std::time::Instant::now();
        let probe = self.with_stream(
            options,
            &stream_options,
            &transcribe_cpp::CancelToken::new(),
            |stream| {
                stream.feed(&vec![0.0; STREAM_PREWARM_SAMPLES])?;
                stream.finalize().map(|_| ())
            },
        );
        match probe {
            Ok(Ok(())) => {
                tracing::info!(
                    ?device_type,
                    chunk_ms,
                    prewarm_ms = started.elapsed().as_millis(),
                    "streaming transcription is available"
                );
                Some(stream_options)
            }
            Ok(Err(error)) | Err(error) => {
                tracing::warn!(%error, "model advertises streaming but could not stream");
                None
            }
        }
    }

    /// Runs `drive` against one live stream on this session, then rotates the
    /// session like [`Self::run`] so the stream's buffered audio and scratch
    /// are released. `cancel` stays installed only for the stream's lifetime.
    pub(crate) fn with_stream<R>(
        &mut self,
        options: &transcribe_cpp::RunOptions,
        stream_options: &transcribe_cpp::StreamOptions,
        cancel: &transcribe_cpp::CancelToken,
        drive: impl FnOnce(&mut transcribe_cpp::Stream<'_>) -> R,
    ) -> transcribe_cpp::Result<R> {
        let mut session = match self.session.take() {
            Some(session) => session,
            None => self.model.session_with(&self.options)?,
        };
        session.set_cancel_token(cancel);
        let result = session
            .stream(options, stream_options)
            .map(|mut stream| drive(&mut stream));
        drop(session);
        self.session = self.model.session_with(&self.options).ok();
        result
    }

    pub(crate) fn run(
        &mut self,
        samples: &[f32],
        options: &transcribe_cpp::RunOptions,
    ) -> transcribe_cpp::Result<transcribe_cpp::Transcript> {
        // transcribe-cpp 0.1.x retains input-sized scheduler buffers for the
        // session lifetime. Drop each used session before creating its
        // successor so a long inference cannot pin that high-water mark or
        // overlap two sessions' persistent decoder state.
        let mut session = match self.session.take() {
            Some(session) => session,
            None => self.model.session_with(&self.options)?,
        };
        let result = session.run(samples, options);
        drop(session);
        match self.model.session_with(&self.options) {
            Ok(session) => {
                self.session = Some(session);
                result
            }
            Err(recovery_error) => match result {
                Ok(_) => Err(recovery_error),
                Err(run_error) => Err(run_error),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_sessions_bound_decoder_threads_and_cpu_sessions_keep_library_default() {
        use transcribe_cpp::DeviceType;
        assert_eq!(session_options(DeviceType::Gpu).n_threads, 1);
        assert_eq!(session_options(DeviceType::Igpu).n_threads, 1);
        assert_eq!(session_options(DeviceType::Cpu).n_threads, 0);
        assert_eq!(session_options(DeviceType::Accel).n_threads, 0);
    }
}

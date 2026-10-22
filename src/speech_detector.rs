//! Silero voice activity detection, used to keep audio without speech away
//! from the transcription models, which tend to invent text for it.
//!
//! The detector is optional. While its file is missing it downloads in the
//! background, and every caller falls back to its level-only silence check.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use color_eyre::eyre::Result;
use transcribe_cpp::{Backend, Model, ModelOptions, VadOptions, VadSession};

use crate::transcription_models;

/// Speech shorter than this is ignored. Silero's default of 250 ms drops a
/// short "Ok" or "Ciao"; 160 ms keeps those and still rejects clicks, keys
/// and room noise.
const MIN_SPEECH_MS: i32 = 160;

#[cfg_attr(test, allow(dead_code))]
enum State {
    Unloaded,
    Loaded(VadSession),
    Failed,
}

#[cfg_attr(test, allow(dead_code))]
static STATE: Mutex<State> = Mutex::new(State::Unloaded);
static DOWNLOAD_STARTED: AtomicBool = AtomicBool::new(false);
#[cfg_attr(test, allow(dead_code))]
static RUN_FAILURE_LOGGED: AtomicBool = AtomicBool::new(false);

/// Whether 16 kHz mono `samples` contain speech, or `None` when the detector
/// is not available and the caller should keep its current behavior.
pub(crate) fn contains_speech(samples: &[f32]) -> Option<bool> {
    #[cfg(test)]
    {
        let _ = samples;
        OVERRIDE.with(|verdict| verdict.get())
    }
    #[cfg(not(test))]
    {
        let mut state = lock_state();
        let session = loaded_session(&mut state)?;
        match detect(session, samples) {
            Ok(speech) => Some(speech),
            Err(error) => {
                if !RUN_FAILURE_LOGGED.swap(true, Ordering::Relaxed) {
                    tracing::warn!(%error, "speech detection failed");
                }
                None
            }
        }
    }
}

/// Loads the detector in the background so the first dictation does not pay
/// for it.
pub(crate) fn prewarm() {
    #[cfg(not(test))]
    {
        let _ = std::thread::Builder::new()
            .name("hex-speech-detector".into())
            .spawn(|| {
                let _ = loaded_session(&mut lock_state());
            });
    }
}

#[cfg_attr(test, allow(dead_code))]
fn lock_state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|error| error.into_inner())
}

/// The loaded session, loading it on first use. A missing detector starts one
/// download per process and stays unloaded, so it loads once the file lands.
#[cfg_attr(test, allow(dead_code))]
fn loaded_session(state: &mut State) -> Option<&mut VadSession> {
    if matches!(state, State::Unloaded) {
        if !transcription_models::speech_detector_installed() {
            start_download();
            return None;
        }
        *state = match transcription_models::speech_detector_path().and_then(|path| load(&path)) {
            Ok(session) => State::Loaded(session),
            Err(error) => {
                tracing::warn!(%error, "could not load the speech detector");
                State::Failed
            }
        };
    }
    match state {
        State::Loaded(session) => Some(session),
        State::Unloaded | State::Failed => None,
    }
}

#[cfg_attr(test, allow(dead_code))]
fn start_download() {
    if DOWNLOAD_STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("hex-speech-detector-download".into())
        .spawn(|| {
            if let Err(error) =
                transcription_models::download_speech_detector(&AtomicBool::new(false))
            {
                tracing::warn!(%error, "could not download the speech detector");
            }
        });
}

/// Loads the detector on the CPU: it loads in tens of milliseconds there,
/// while a first Metal start can take seconds and would hold up dictation.
fn load(path: &Path) -> Result<VadSession> {
    let model = Model::load_with(
        path,
        &ModelOptions {
            backend: Backend::Cpu,
            device: None,
        },
    )?;
    Ok(model.vad_session()?)
}

fn detect(session: &mut VadSession, samples: &[f32]) -> Result<bool> {
    let options = VadOptions {
        min_speech_ms: MIN_SPEECH_MS,
        ..VadOptions::default()
    };
    Ok(!session.run(samples, &options)?.segments.is_empty())
}

#[cfg(test)]
thread_local! {
    static OVERRIDE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Makes `contains_speech` answer `verdict` on this thread until the guard
/// drops. Tests never load the real detector.
#[cfg(test)]
pub(crate) fn override_for_test(verdict: Option<bool>) -> OverrideGuard {
    OverrideGuard(OVERRIDE.with(|current| current.replace(verdict)))
}

#[cfg(test)]
pub(crate) struct OverrideGuard(Option<bool>);

#[cfg(test)]
impl Drop for OverrideGuard {
    fn drop(&mut self) {
        OVERRIDE.with(|current| current.set(self.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_decides_and_restores() {
        {
            let _guard = override_for_test(Some(false));
            assert_eq!(contains_speech(&[0.0; 16_000]), Some(false));
        }
        assert_eq!(contains_speech(&[0.0; 16_000]), None);
    }

    #[test]
    fn a_corrupt_detector_file_is_not_loaded() {
        let path = std::env::temp_dir().join(format!(
            "hex-corrupt-speech-detector-{}.gguf",
            std::process::id()
        ));
        std::fs::write(&path, b"not a gguf file").unwrap();
        let loaded = load(&path);
        std::fs::remove_file(&path).unwrap();
        assert!(loaded.is_err());
    }

    #[test]
    #[ignore = "requires HEX_VAD_MODEL (Silero VAD GGUF)"]
    fn real_detector_rejects_silence_and_noise() {
        let mut session = load(Path::new(&std::env::var("HEX_VAD_MODEL").unwrap())).unwrap();
        assert!(!detect(&mut session, &[0.0; 32_000]).unwrap());
        // -60 dBFS white noise from a fixed linear congruential generator.
        let mut seed = 0x2545_f491_u32;
        let noise: Vec<f32> = (0..32_000)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.001 * 3f32.sqrt()
            })
            .collect();
        assert!(!detect(&mut session, &noise).unwrap());
    }

    #[test]
    #[ignore = "requires HEX_VAD_MODEL and HEX_VAD_SPEECH_WAV (16 kHz mono float speech)"]
    fn real_detector_hears_speech() {
        let mut session = load(Path::new(&std::env::var("HEX_VAD_MODEL").unwrap())).unwrap();
        let mut reader =
            hound::WavReader::open(std::env::var("HEX_VAD_SPEECH_WAV").unwrap()).unwrap();
        assert_eq!(reader.spec().sample_rate, 16_000);
        let speech: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).collect();
        assert!(detect(&mut session, &speech).unwrap());
    }
}

use color_eyre::eyre::Result;
use serde::Deserialize;

use crate::apple_speech::AppleSpeech;
use crate::parakeet::Parakeet;
use crate::transcription_models::{ModelRuntime, TranscriptionSelection, validate};

pub enum Transcriber {
    Gguf(Box<Parakeet>),
    AppleSpeech(AppleSpeech),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

pub struct Transcript {
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
}

impl Transcriber {
    pub fn load() -> Result<Self> {
        let (_, selection) = crate::app_settings::transcription_selection();
        Self::load_selection(&selection)
    }

    pub fn load_selection(selection: &TranscriptionSelection) -> Result<Self> {
        match validate(selection)?.runtime {
            ModelRuntime::Gguf(_) => Parakeet::load_selection(selection)
                .map(Box::new)
                .map(Self::Gguf),
            ModelRuntime::AppleSpeech => AppleSpeech::load(selection).map(Self::AppleSpeech),
        }
    }

    pub fn matches_selection(&self, selection: &TranscriptionSelection) -> bool {
        match self {
            Self::Gguf(model) => model.matches_selection(selection),
            Self::AppleSpeech(model) => model.matches_selection(selection),
        }
    }

    pub fn prepare_samples(&self, mut samples: Vec<f32>) -> Vec<f32> {
        if matches!(self, Self::Gguf(_)) {
            crate::dictation::pad_for_parakeet(&mut samples);
        }
        samples
    }

    pub fn transcribe(&mut self, samples: &[f32]) -> Result<String> {
        match self {
            Self::Gguf(model) => model.transcribe(samples),
            Self::AppleSpeech(model) => model.transcribe(samples).map(|result| result.text),
        }
    }

    pub fn transcribe_segments(&mut self, samples: &[f32]) -> Result<Transcript> {
        match self {
            Self::Gguf(model) => model.transcribe_segments(samples).map(|result| Transcript {
                text: result.text,
                segments: result
                    .segments
                    .into_iter()
                    .map(|segment| TranscriptSegment {
                        start_ms: segment.t0_ms,
                        end_ms: segment.t1_ms,
                        text: segment.text,
                    })
                    .collect(),
            }),
            Self::AppleSpeech(model) => model.transcribe(samples),
        }
    }
}

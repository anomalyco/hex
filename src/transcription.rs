use color_eyre::eyre::Result;
use serde::Deserialize;

use crate::apple_speech::AppleSpeech;
use crate::parakeet::Parakeet;
use crate::transcription_models::{
    ModelRuntime, TranscriptionModelId, TranscriptionSelection, validate,
};

const UNIFIED_ENGLISH_TRAILING_SILENCE_SAMPLES: usize = 3_200;

pub enum Transcriber {
    Gguf(Box<Parakeet>),
    AppleSpeech(AppleSpeech),
}

#[derive(Default)]
pub struct WarmTranscriber {
    active: Option<Transcriber>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
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
        if let Self::Gguf(model) = self {
            prepare_gguf_samples(&mut samples, model.model_id());
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

fn prepare_gguf_samples(samples: &mut Vec<f32>, model: Option<TranscriptionModelId>) {
    crate::dictation::pad_for_parakeet(samples);
    if model == Some(TranscriptionModelId::ParakeetUnifiedEnglish) {
        samples.resize(
            samples.len() + UNIFIED_ENGLISH_TRAILING_SILENCE_SAMPLES,
            0.0,
        );
    }
}

impl WarmTranscriber {
    pub fn load() -> Result<Self> {
        Ok(Self {
            active: Some(Transcriber::load()?),
        })
    }

    pub fn activate(&mut self, selection: &TranscriptionSelection) -> Result<&mut Transcriber> {
        if !self
            .active
            .as_ref()
            .is_some_and(|model| model.matches_selection(selection))
        {
            let candidate = Transcriber::load_selection(selection)?;
            self.active = Some(candidate);
        }
        Ok(self
            .active
            .as_mut()
            .expect("activated transcriber must be available"))
    }
}

#[cfg(test)]
mod tests {
    use super::{Transcript, UNIFIED_ENGLISH_TRAILING_SILENCE_SAMPLES, prepare_gguf_samples};
    use crate::transcription_models::TranscriptionModelId;

    #[test]
    fn apple_bridge_transcript_uses_the_canonical_wire_shape() {
        let transcript: Transcript = serde_json::from_str(
            r#"{"text":"hello","segments":[{"startMs":10,"endMs":40,"text":"hello"}]}"#,
        )
        .unwrap();

        assert_eq!(transcript.text, "hello");
        assert_eq!(transcript.segments.len(), 1);
        assert_eq!(transcript.segments[0].start_ms, 10);
        assert_eq!(transcript.segments[0].end_ms, 40);
    }

    #[test]
    fn unified_english_gets_trailing_silence_for_final_token_context() {
        let mut samples = vec![0.5; 32_000];

        prepare_gguf_samples(
            &mut samples,
            Some(TranscriptionModelId::ParakeetUnifiedEnglish),
        );

        assert_eq!(
            samples.len(),
            32_000 + UNIFIED_ENGLISH_TRAILING_SILENCE_SAMPLES
        );
        assert!(samples[..32_000].iter().all(|sample| *sample == 0.5));
        assert!(samples[32_000..].iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn other_gguf_models_do_not_get_trailing_silence() {
        let mut samples = vec![0.5; 32_000];

        prepare_gguf_samples(&mut samples, Some(TranscriptionModelId::ParakeetV2));

        assert_eq!(samples.len(), 32_000);
    }
}

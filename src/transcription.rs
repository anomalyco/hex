use color_eyre::eyre::Result;
use serde::Deserialize;

use crate::apple_speech::AppleSpeech;
use crate::dictation::DictationProtocol;
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

    /// Transcribe normalized 16 kHz audio. Backend padding is applied once here,
    /// before GGUF chunking or any voice-control retranscription.
    pub fn transcribe(&mut self, samples: Vec<f32>) -> Result<String> {
        self.transcribe_with_protocol(samples, None)
    }

    pub fn transcribe_voice(
        &mut self,
        samples: Vec<f32>,
        protocol: &DictationProtocol,
    ) -> Result<String> {
        self.transcribe_with_protocol(samples, Some(protocol))
    }

    fn transcribe_with_protocol(
        &mut self,
        mut samples: Vec<f32>,
        protocol: Option<&DictationProtocol>,
    ) -> Result<String> {
        match self {
            Self::Gguf(model) => {
                prepare_gguf_samples(&mut samples, model.model_id());
                match protocol {
                    Some(protocol) => model.transcribe_voice(&samples, protocol),
                    None => model.transcribe(&samples),
                }
            }
            Self::AppleSpeech(model) => model.transcribe(&samples).map(|result| result.text),
        }
    }

    /// Transcribe one normalized segment with the same input preparation as
    /// plain dictation. The caller retains ownership of segment offsets.
    pub fn transcribe_segments(&mut self, mut samples: Vec<f32>) -> Result<Transcript> {
        match self {
            Self::Gguf(model) => {
                prepare_gguf_samples(&mut samples, model.model_id());
                model
                    .transcribe_segments(&samples)
                    .map(|result| Transcript {
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
                    })
            }
            Self::AppleSpeech(model) => model.transcribe(&samples),
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

    #[test]
    fn short_unified_input_is_minimum_padded_before_trailing_context() {
        let mut samples = vec![0.5; 1_600];
        prepare_gguf_samples(
            &mut samples,
            Some(TranscriptionModelId::ParakeetUnifiedEnglish),
        );

        // The 200 ms context is additional to the 1.5 s minimum, not part of it.
        assert_eq!(
            samples.len(),
            24_000 + UNIFIED_ENGLISH_TRAILING_SILENCE_SAMPLES
        );
        assert!(samples[..1_600].iter().all(|sample| *sample == 0.5));
        assert!(samples[1_600..].iter().all(|sample| *sample == 0.0));
    }
}

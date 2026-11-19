//! What a catalog model's details view shows beyond its card: the credit
//! its licence asks for, links to its source and licence, the notice its
//! publisher supplied, measured resource use, and benchmarks the publisher
//! measured (never Hex). Only models that need more than the card have one.

use super::models::TranscriptionModelId;

pub struct ModelDetails {
    /// The credit, word for word as the licence's attribution.
    pub attribution: &'static str,
    pub source_url: &'static str,
    pub license_url: &'static str,
    /// The NOTICE the publisher ships with the file.
    pub notice: &'static str,
    /// Peak memory while transcribing and where it was measured; `None`
    /// when Hex hasn't measured it.
    pub peak_memory: Option<(&'static str, &'static str)>,
    /// Published error rates per language, in percent.
    pub published: &'static [(&'static str, f64)],
    /// How the published figures were made, and what is unknown about them.
    pub published_conditions: &'static str,
}

const PARAKEET_ULTRA: ModelDetails = ModelDetails {
    attribution: "Parakeet Ultra by Moondream, based on NVIDIA Parakeet TDT 0.6B v3. \
                  GGUF conversion and quantization by handy-computer. Creative Commons \
                  Attribution 4.0 International (CC BY 4.0).",
    source_url: "https://huggingface.co/handy-computer/parakeet-ultra-gguf/blob/\
                 a1539be1b849ea7463b88343e5277b373d0c4ea9/README.md",
    license_url: "https://creativecommons.org/licenses/by/4.0/legalcode.en#s3a",
    notice: "Modifications: handy-computer converted \
             https://huggingface.co/moondream/parakeet-ultra at commit 73175eb to GGUF for \
             transcribe.cpp (https://github.com/handy-computer/transcribe.cpp). Conversion \
             repackages the weights, renaming and re-laying-out tensors; the quantized files \
             (Q8_0, Q6_K, Q5_K_M, Q4_K_M) also store them at reduced numeric precision. The \
             weights were not retrained or fine-tuned.",
    // transcribe-cli at 7045d1c, five runs of a 10.6 s FLEURS clip.
    peak_memory: Some(("~0.7 GB GPU · ~0.2 GB RAM", "Radeon 8060S, Vulkan")),
    // transcribe.cpp 7045d1c's catalog/parakeet-ultra.json.
    published: &[
        ("en", 4.26),
        ("de", 4.58),
        ("fr", 4.66),
        ("es", 3.06),
        ("it", 2.34),
        ("pt", 4.16),
        ("ru", 5.63),
        ("uk", 5.71),
        ("hr", 11.3),
        ("sl", 18.98),
        ("lv", 18.04),
        ("lt", 17.06),
        ("et", 14.44),
        ("fi", 11.14),
        ("sv", 12.83),
        ("da", 15.98),
        ("nl", 6.7),
        ("pl", 6.16),
        ("cs", 11.04),
        ("sk", 7.83),
        ("hu", 13.09),
        ("ro", 10.12),
        ("bg", 11.04),
        ("el", 32.25),
        ("mt", 17.01),
    ],
    published_conditions: "FLEURS test · Q8_0 · CUDA · batch 8 · each language's hint · \
                           no timestamps · transcribe.cpp 021aa17d · the benchmark file's \
                           digest, dataset revision, and scorer versions unknown",
};

const ORUKEET: ModelDetails = ModelDetails {
    attribution: "Orukeet by Oruk AI, based on NVIDIA Parakeet TDT 0.6B v3. GGUF \
                  conversion and quantization by handy-computer. Creative Commons \
                  Attribution-ShareAlike 4.0 International (CC BY-SA 4.0).",
    source_url: "https://huggingface.co/handy-computer/orukeet-gguf/blob/\
                 c5511a3850fce1cbbfa5d94ef815966e1d99275f/README.md",
    license_url: "https://creativecommons.org/licenses/by-sa/4.0/legalcode.en#s3a",
    notice: "Orukeet is an adaptation of NVIDIA Parakeet TDT 0.6B v3. NVIDIA retains \
             copyright in its model and upstream work. The base weights are distributed \
             under CC BY 4.0 (https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3). Oruk \
             AI's changes comprise multilingual/accent continuation training, parameter \
             blending, fitted and frozen Gabor-kernel replacement and recovery, native \
             and ONNX export, application integration and evaluation. Model stages \
             (https://github.com/Oruk-AI/orukeet/blob/main/release/model-stages.json) \
             identifies r3 as the source of every current Orukeet download and records its \
             ancestry. The r3 NeMo, ONNX INT8, Q8 GGUF and F16 GGUF weights, and their \
             fitted Gabor kernels, are designated CC BY-SA 4.0. Modifications: \
             handy-computer converted https://huggingface.co/oruk/orukeet at commit b59c13a \
             to GGUF for transcribe.cpp (https://github.com/handy-computer/transcribe.cpp). \
             Conversion repackages the weights, renaming and re-laying-out tensors; the \
             quantized files (Q8_0, Q6_K, Q5_K_M, Q4_K_M) also store them at reduced numeric \
             precision. The weights were not retrained or fine-tuned.",
    peak_memory: None,
    // transcribe.cpp 2778ae4's catalog/orukeet.json, Q8_0 FLEURS test records.
    published: &[
        ("bg", 11.14),
        ("hr", 11.26),
        ("cs", 10.22),
        ("da", 16.58),
        ("nl", 6.93),
        ("en", 4.43),
        ("et", 15.16),
        ("fi", 11.88),
        ("fr", 6.17),
        ("de", 5.64),
        ("el", 33.12),
        ("hu", 13.53),
        ("it", 3.11),
        ("lv", 19.53),
        ("lt", 17.72),
        ("mt", 17.18),
        ("pl", 7.35),
        ("pt", 4.56),
        ("ro", 10.86),
        ("ru", 6.7),
        ("sk", 8.99),
        ("sl", 24.0),
        ("es", 3.75),
        ("sv", 13.26),
        ("uk", 6.53),
    ],
    published_conditions: "FLEURS test · Q8_0 · CUDA · batch 8 · each language's hint · \
                           no timestamps · transcribe.cpp 37273409 · the benchmark file's \
                           digest, dataset revision, and scorer versions unknown",
};

const FLUID_SPEECH_PRO: ModelDetails = ModelDetails {
    attribution: "Fluid Speech Pro by FluidVoice, based on NVIDIA Parakeet TDT 0.6B v3 and \
                  Moondream Parakeet Ultra. GGUF conversion and quantization by TalkMate. \
                  Creative Commons Attribution 4.0 International (CC BY 4.0).",
    source_url: "https://huggingface.co/TalkMate/fluid-speech-pro-gguf/blob/\
                 d59dc79f7e04715c5b0ba07f01106f71ebea97dc/README.md",
    license_url: "https://creativecommons.org/licenses/by/4.0/legalcode.en#s3a",
    notice: "Fluid Speech Pro by FluidVoice \
             (https://huggingface.co/FluidVoice-Labs/fluid-speech-pro), CC BY 4.0. Built on \
             NVIDIA Parakeet TDT 0.6B v3 and Moondream Parakeet Ultra, both CC BY 4.0. \
             Converted to a transcribe.cpp GGUF (Q8_0) by TalkMate, in the layout of Handy's \
             Parakeet Ultra GGUF (https://huggingface.co/handy-computer/parakeet-ultra-gguf, \
             CC BY 4.0).",
    peak_memory: None,
    // The card's FLEURS test table: 24 languages, all of its languages but
    // English, for which it publishes no FLEURS figure.
    published: &[
        ("bg", 10.19),
        ("hr", 9.73),
        ("cs", 8.83),
        ("da", 14.57),
        ("nl", 5.51),
        ("et", 9.51),
        ("fi", 8.82),
        ("fr", 4.65),
        ("de", 3.52),
        ("el", 17.1),
        ("hu", 10.65),
        ("it", 1.91),
        ("lv", 16.39),
        ("lt", 16.21),
        ("mt", 15.05),
        ("pl", 5.7),
        ("pt", 3.57),
        ("ro", 9.28),
        ("ru", 4.35),
        ("sk", 7.2),
        ("sl", 17.97),
        ("es", 2.66),
        ("sv", 11.1),
        ("uk", 4.77),
    ],
    published_conditions: "The card's FLEURS test figures (greedy decoding) for the reference \
                           NeMo model, not the Q8_0 file Hex downloads. The Q8_0 card reports \
                           only 10.2% WER on EdAcc's 600 accented-English clips against the \
                           .nemo's 10.1%. The card publishes no FLEURS English figure. Dataset \
                           revisions, normalization, and scorer versions unknown",
};

pub fn details(id: TranscriptionModelId) -> Option<&'static ModelDetails> {
    match id {
        TranscriptionModelId::ParakeetUltra => Some(&PARAKEET_ULTRA),
        TranscriptionModelId::Orukeet => Some(&ORUKEET),
        TranscriptionModelId::FluidSpeechPro => Some(&FLUID_SPEECH_PRO),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parakeet_ultras_published_scores_cover_its_languages_once() {
        let ultra = details(TranscriptionModelId::ParakeetUltra).unwrap();
        let model = super::super::models::definition(TranscriptionModelId::ParakeetUltra);
        let mut scored: Vec<_> = ultra.published.iter().map(|(code, _)| *code).collect();
        let mut languages = model.languages.to_vec();
        scored.sort_unstable();
        languages.sort_unstable();
        assert_eq!(scored, languages);
        assert!(ultra.attribution.ends_with("(CC BY 4.0)."));
        assert!(details(TranscriptionModelId::ParakeetV3).is_none());
    }

    #[test]
    fn orukeets_details_credit_its_base_and_cover_its_languages_once() {
        let orukeet = details(TranscriptionModelId::Orukeet).unwrap();
        let model = super::super::models::definition(TranscriptionModelId::Orukeet);
        let mut scored: Vec<_> = orukeet.published.iter().map(|(code, _)| *code).collect();
        let mut languages = model.languages.to_vec();
        scored.sort_unstable();
        languages.sort_unstable();
        assert_eq!(scored, languages);
        assert!(
            orukeet
                .attribution
                .starts_with("Orukeet by Oruk AI, based on NVIDIA Parakeet TDT 0.6B v3.")
        );
        assert!(orukeet.attribution.ends_with("(CC BY-SA 4.0)."));
        assert!(
            orukeet
                .source_url
                .ends_with("c5511a3850fce1cbbfa5d94ef815966e1d99275f/README.md")
        );
        assert!(orukeet.license_url.ends_with("#s3a"));
        assert!(
            orukeet
                .notice
                .contains("The weights were not retrained or fine-tuned.")
        );
        assert!(orukeet.peak_memory.is_none());
    }

    #[test]
    fn fluid_speech_pros_details_credit_its_bases_and_cover_its_languages_once() {
        let fluid = details(TranscriptionModelId::FluidSpeechPro).unwrap();
        let model = super::super::models::definition(TranscriptionModelId::FluidSpeechPro);
        let mut scored: Vec<_> = fluid.published.iter().map(|(code, _)| *code).collect();
        // The card publishes FLEURS figures for every language but English.
        let mut languages: Vec<_> = model
            .languages
            .iter()
            .copied()
            .filter(|code| *code != "en")
            .collect();
        scored.sort_unstable();
        languages.sort_unstable();
        assert_eq!(scored, languages);
        assert!(
            fluid.published.iter().all(|(code, _)| *code != "en"),
            "the card publishes no FLEURS English figure"
        );
        assert!(fluid.attribution.starts_with(
            "Fluid Speech Pro by FluidVoice, based on NVIDIA Parakeet TDT 0.6B v3 and \
             Moondream Parakeet Ultra."
        ));
        assert!(fluid.attribution.ends_with("(CC BY 4.0)."));
        assert!(
            fluid
                .source_url
                .ends_with("d59dc79f7e04715c5b0ba07f01106f71ebea97dc/README.md")
        );
        assert!(fluid.license_url.ends_with("#s3a"));
        assert!(
            fluid
                .notice
                .contains("Converted to a transcribe.cpp GGUF (Q8_0) by TalkMate")
        );
        assert!(fluid.peak_memory.is_none());
    }
}

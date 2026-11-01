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

pub fn details(id: TranscriptionModelId) -> Option<&'static ModelDetails> {
    match id {
        TranscriptionModelId::ParakeetUltra => Some(&PARAKEET_ULTRA),
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
}

//! The catalog's pinned model files and the language lists its entries share.

use super::models::GgufArtifact;

pub(super) const PARAKEET_V3_LANGUAGES: &[&str] = &[
    "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it", "lv", "lt", "mt",
    "pl", "pt", "ro", "ru", "sk", "sl", "es", "sv", "uk",
];
pub(super) const QWEN_LANGUAGES: &[&str] = &[
    "zh", "en", "yue", "ar", "de", "fr", "es", "pt", "id", "it", "ko", "ru", "th", "vi", "ja",
    "tr", "hi", "ms", "nl", "sv", "da", "fi", "pl", "cs", "fil", "fa", "el", "ro", "hu", "mk",
];
pub(super) const COHERE_LANGUAGES: &[&str] = &[
    "en", "fr", "de", "es", "it", "pt", "nl", "pl", "el", "ar", "ja", "zh", "vi", "ko",
];

pub(super) const PARAKEET_V2_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "parakeet-tdt-0.6b-v2-Q8_0.gguf",
    revision: "07cee0616125a08ef619729bb47f40ef747e4bc4",
    repository: "handy-computer/parakeet-tdt-0.6b-v2-gguf",
    bytes: 729_574_912,
    sha256: "f0d0e99cebb6d3b83f1f7069b82b5d3c2e39a54545b0da039cb4bafd9c4e5caa",
    architecture: "parakeet",
    variant: "tdt-0.6b-v2",
};
pub(super) const PARAKEET_UNIFIED_ENGLISH_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "parakeet-unified-en-0.6b-Q8_0.gguf",
    revision: "7e948f21b7bdbac698d3318db9d350f1096f3b6c",
    repository: "handy-computer/parakeet-unified-en-0.6b-gguf",
    bytes: 731_357_568,
    sha256: "4b50b6dd862bf6e346929aaf4f5eaacec003bfa3f56462d6c874b41ef2f38795",
    architecture: "parakeet",
    variant: "unified-en-0.6b",
};
pub(super) const PARAKEET_V3_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "parakeet-tdt-0.6b-v3-Q8_0.gguf",
    revision: "85ac09ea12fc4b1112fa76810059364bc6adc9de",
    repository: "handy-computer/parakeet-tdt-0.6b-v3-gguf",
    bytes: 739_508_576,
    sha256: "5859f77944efcd8eafa23a6350731960b2b55b2203df51f319665c807d802cc7",
    architecture: "parakeet",
    variant: "tdt-0.6b-v3",
};
pub(super) const WHISPER_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "whisper-large-v3-turbo-Q8_0.gguf",
    revision: "d222c9f621c1128299248f2ded4d8a1820519780",
    repository: "handy-computer/whisper-large-v3-turbo-gguf",
    bytes: 886_381_824,
    sha256: "d5e65f2b0828802ae2c231673d31982cebe3a778c95d9494a9f3efee6bd17448",
    architecture: "whisper",
    variant: "whisper-large-v3-turbo",
};
pub(super) const QWEN_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "Qwen3-ASR-0.6B-Q8_0.gguf",
    revision: "e4e16599b900eb0cb36e524514756bb92eb092b7",
    repository: "handy-computer/Qwen3-ASR-0.6B-gguf",
    bytes: 850_423_456,
    sha256: "f081b2d5e23bd669d92cc331d722a8a0681943b8e6f34b48996fd5c319b5acd8",
    architecture: "qwen3_asr",
    variant: "qwen3-asr-0.6b",
};
// Mohammed Aly's Qwen3-ASR 1.7B fine-tune for Egyptian Arabic and
// Arabic/English code-switching (Apache-2.0), from mohammedaly22/QwenCleo-ASR
// at 50f6b901. Converted with transcribe.cpp 7045d1c's Qwen3-ASR converter,
// with the fine-tune's attribution in `general.*`, and quantized to Q8_0;
// rebuilding it that way reproduces this file byte for byte.
pub(super) const QWENCLEO_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "QwenCleo-ASR-1.7B-Q8_0.gguf",
    revision: "08c66976a0496e1e3f1a88965eea123631e634fa",
    repository: "reomar15/QwenCleo-ASR-transcribe-cpp-GGUF",
    bytes: 2_185_030_816,
    sha256: "43bae9d5d7a26684b4a0faf1abc6e872b5825cc9147dbb7200da9ff4c5d12f87",
    architecture: "qwen3_asr",
    variant: "qwen3-asr-1.7b",
};
// ivrit.ai's Hebrew fine-tune of Whisper large-v3-turbo (Apache-2.0), from
// ivrit-ai/whisper-large-v3-turbo at f33172a8. Converted with transcribe.cpp
// 7045d1c's Whisper converter, with the variant's display name added
// (`ivrit.ai Whisper Large v3 Turbo`; author and repository come from the
// source), and quantized to Q8_0; rebuilding it that way reproduces this
// file byte for byte.
pub(super) const IVRIT_WHISPER_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "ivrit-whisper-large-v3-turbo-Q8_0.gguf",
    revision: "a12befaff80add995ac6a8fe10cbb7f38438c7b5",
    repository: "adolan/ivrit-whisper-large-v3-turbo-gguf",
    bytes: 886_381_536,
    sha256: "23756afa3d3c52cd3cb55e259c8373373eba6d19c3858a67531ba2772b7f272f",
    architecture: "whisper",
    variant: "ivrit-whisper-large-v3-turbo",
};
pub(super) const SENSEVOICE_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "SenseVoiceSmall-Q8_0.gguf",
    revision: "4a08b8e900b38a977e32eb08d5d0697d6e72ba04",
    repository: "handy-computer/SenseVoiceSmall-gguf",
    bytes: 252_684_608,
    sha256: "6c759ee4c9748c9b3f7a5a60ca74f0f7e685fb9d45d1378fce7cfd62f59adf29",
    architecture: "sensevoice",
    variant: "sensevoice-small",
};
pub(super) const COHERE_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "cohere-transcribe-03-2026-Q8_0.gguf",
    revision: "dfa4adebb64f3076b7b6b90b721275cc069cb421",
    repository: "handy-computer/cohere-transcribe-03-2026-gguf",
    bytes: 2_410_655_232,
    sha256: "931916663432fd895423a4291a8400221802b288967ca2d435fc5e3141c9e71e",
    architecture: "cohere_asr",
    variant: "cohere-transcribe-03-2026",
};
// Conway Research's Parakeet TDT v3 fine-tunes (CC-BY-4.0), converted to
// Q8_0 with transcribe.cpp's Parakeet converter; each repository holds the
// recipe.
pub(super) const UNDERDOG_BARK_3_PRO_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "underdog-bark-3.0-pro-Q8_0.gguf",
    revision: "5c10586263806666c0a8b4905a3346a7c679803e",
    repository: "joshthomasdev/underdog-bark-3.0-pro-gguf",
    bytes: 739_508_480,
    sha256: "472d151cc1546bd30629c4c25df3a8e555e8990845843ddafe76800c640e9865",
    architecture: "parakeet",
    variant: "tdt-0.6b-bark-3.0-pro",
};
pub(super) const UNDERDOG_BARK_3_CLEAN_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "underdog-bark-3.0-clean-Q8_0.gguf",
    revision: "868701fd53d62c59d759ed1d79e095118f3fa7b9",
    repository: "joshthomasdev/underdog-bark-3.0-clean-gguf",
    bytes: 739_508_480,
    sha256: "42ec2f70a3d14842e322391baf5186b2829f271cc7e396aed25580135ae46fae",
    architecture: "parakeet",
    variant: "tdt-0.6b-bark-3.0-clean",
};
// Moondream's Parakeet Ultra, a post-trained Parakeet TDT 0.6B v3
// (CC-BY-4.0), converted to GGUF and quantized to Q8_0 by handy-computer
// (430 F32, 225 Q8_0, and 48 F16 tensors). Behind Labs › Parakeet Ultra.
pub(super) const PARAKEET_ULTRA_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "parakeet-ultra-Q8_0.gguf",
    revision: "a1539be1b849ea7463b88343e5277b373d0c4ea9",
    repository: "handy-computer/parakeet-ultra-gguf",
    bytes: 740_363_168,
    sha256: "007a59761e9258779f189df396b50d12b83bb5b79e66df4b955c230d2f2a0a59",
    architecture: "parakeet",
    variant: "tdt-0.6b-ultra",
};
// NVIDIA's Nemotron 3.5 ASR Streaming 0.6B (OpenMDW-1.1), a cache-aware
// FastConformer with a language-prompted RNN-T. Hex converted
// nvidia/nemotron-3.5-asr-streaming-0.6b at 24b151a (the .nemo,
// 210214ed…) with transcribe.cpp 7045d1c's Parakeet converter and quantized
// it to Q8_0: all 653 tensors match this file byte for byte, and the
// metadata differs only by two capability flags the newer converter writes
// as false (translate, diarization). The pinned file is handy-computer's,
// which ships the licence and notice beside it.
pub(super) const NEMOTRON_3_5_ASR_ARTIFACT: GgufArtifact = GgufArtifact {
    filename: "nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf",
    revision: "53ce288f69326b415aeef030cfcb02a08a8096dd",
    repository: "handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf",
    bytes: 751_094_240,
    sha256: "b94545b313b3223fda7b2857a52681da813935c2127643d1e9ff0c23d988089c",
    architecture: "parakeet",
    variant: "nemotron-3.5-asr-streaming-0.6b",
};
/// The languages Nemotron 3.5 transcribes that Hex names, as the locales
/// its language prompt takes (`runtime_language`). Its card also lists
/// Norwegian Bokmål, which Hex doesn't offer yet.
pub(super) const NEMOTRON_3_5_LOCALES: &[(&str, &str)] = &[
    ("en", "en-US"),
    ("es", "es-ES"),
    ("fr", "fr-FR"),
    ("de", "de-DE"),
    ("it", "it-IT"),
    ("pt", "pt-BR"),
    ("nl", "nl-NL"),
    ("tr", "tr-TR"),
    ("ru", "ru-RU"),
    ("uk", "uk-UA"),
    ("pl", "pl-PL"),
    ("cs", "cs-CZ"),
    ("sk", "sk-SK"),
    ("bg", "bg-BG"),
    ("hr", "hr-HR"),
    ("ro", "ro-RO"),
    ("hu", "hu-HU"),
    ("sv", "sv-SE"),
    ("da", "da-DK"),
    ("fi", "fi-FI"),
    ("et", "et-EE"),
    ("ar", "ar-AR"),
    ("hi", "hi-IN"),
    ("ja", "ja-JP"),
    ("ko", "ko-KR"),
    ("zh", "zh-CN"),
    ("vi", "vi-VN"),
];
pub(super) const NEMOTRON_3_5_LANGUAGES: &[&str] = &[
    "en", "es", "fr", "de", "it", "pt", "nl", "tr", "ru", "uk", "pl", "cs", "sk", "bg", "hr", "ro",
    "hu", "sv", "da", "fi", "et", "ar", "hi", "ja", "ko", "zh", "vi",
];

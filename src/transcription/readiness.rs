//! One readiness check and inventory projection for speech models: installed,
//! verified, ready, needs-check, and source. Onboarding, Settings, the local
//! API, the control CLI, and every listener decide with it, so none of them
//! calls a model ready while another refuses it.

use std::borrow::Cow;

use super::found_models;
use super::models::{
    ModelDefinition, TranscriptionModelId, VocabularyUse, all_models, is_installed, is_verified,
    model_path,
};

/// One readiness snapshot for a speech model: whether its files are here,
/// whether its checksum/asset is verified, whether it can run right now, and
/// whether Enable / `model use` still needs to check it first.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModelReadiness {
    pub installed: bool,
    pub verified: bool,
    pub ready: bool,
    pub needs_check: bool,
}

/// Evaluates `model`'s readiness for `language`.
pub fn readiness(model: &ModelDefinition, language: &str) -> ModelReadiness {
    let installed = is_installed(model, language);
    let verified = if model.runtime.is_gguf() {
        is_verified(model)
    } else {
        installed
    };
    let ready = installed && verified;
    ModelReadiness {
        installed,
        verified,
        ready,
        needs_check: installed && !ready,
    }
}

/// Whether `model` can run for `language`: installed, and for a GGUF file
/// checked against its pinned artifact. A file another app downloaded
/// becomes ready once a full check records it (found_models::adopt_catalog).
pub fn is_ready(model: &ModelDefinition, language: &str) -> bool {
    readiness(model, language).ready
}

/// Whether a model's file is here but not ready yet: Enable can verify and
/// adopt it with the existing installed-only preparation. A model with no
/// file is not a check (it stays "Model required").
pub fn needs_check(model: &ModelDefinition, language: &str) -> bool {
    readiness(model, language).needs_check
}

/// How an architecture feeds the shared vocabulary during transcription.
pub fn vocabulary_use_for_architecture(architecture: &str) -> VocabularyUse {
    match architecture {
        "parakeet" => VocabularyUse::Bias,
        "whisper" | "qwen3_asr" => VocabularyUse::Prompt,
        _ => VocabularyUse::None,
    }
}

/// Wire label for [`VocabularyUse`] in `hex model list`.
pub fn vocabulary_use_label(use_kind: VocabularyUse) -> &'static str {
    match use_kind {
        VocabularyUse::Prompt => "prompt",
        VocabularyUse::Bias => "bias",
        VocabularyUse::None => "none",
    }
}

/// Where `model`'s file is or comes from, as `hex model list` and the local
/// API say it: `found: owner/name@abcdef1`, `imported`, `hex`, or `download`.
fn model_source_with(model: &ModelDefinition, installed: bool) -> String {
    let here = model_path(model)
        .ok()
        .filter(|_| installed)
        .and_then(|path| found_models::describe(&path));
    match (model.id.is_custom(), here) {
        (true, Some(origin)) => format!("found: {origin} (not tested by Hex)"),
        (false, Some(origin)) => format!("found: {origin}"),
        (true, None) => "imported".into(),
        (false, None) if installed => "hex".into(),
        (false, None) => "download".into(),
    }
}

/// Shared projection of one available catalog or imported model for
/// `GET /models` and `hex model list`. Each surface serializes its own wire
/// schema from these values so they cannot drift.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogModelInfo {
    pub id: TranscriptionModelId,
    pub id_str: String,
    pub name: Cow<'static, str>,
    pub readiness: ModelReadiness,
    pub managed: bool,
    pub download_bytes: Option<u64>,
    pub size: String,
    pub languages: Vec<&'static str>,
    pub supports_language_detection: bool,
    pub streams: bool,
    pub vocabulary: VocabularyUse,
    pub source: String,
}

/// Every available catalog or imported model projected for `language`.
pub fn catalog_models(language: &str) -> Vec<CatalogModelInfo> {
    all_models()
        .into_iter()
        .filter(|model| model.available())
        .map(|model| {
            let status = readiness(&model, language);
            let managed = !model.runtime.is_gguf();
            CatalogModelInfo {
                id: model.id,
                id_str: model.id.as_str().to_string(),
                name: model.name.clone(),
                readiness: status,
                managed,
                download_bytes: model.download_bytes(),
                size: model.size_label(),
                languages: model.presentation_languages(),
                supports_language_detection: model.supports_language_detection,
                streams: model.streams,
                vocabulary: model.vocabulary_use(),
                source: model_source_with(&model, status.installed),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::models::{
        GgufArtifact, MODELS, ModelRuntime, TranscriptionModelId, set_test_models_dir,
    };

    #[test]
    fn readiness_distinguishes_missing_unverified_and_verified_gguf() {
        const ARTIFACT: GgufArtifact = GgufArtifact {
            filename: Cow::Borrowed("fixture.gguf"),
            stored_as: None,
            revision: "rev",
            repository: "example/fixture",
            bytes: 4,
            sha256: Cow::Borrowed(
                "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
            ),
            architecture: "parakeet",
            variant: Cow::Borrowed("fixture"),
        };
        let dir = std::env::temp_dir().join(format!(
            "hex-readiness-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        set_test_models_dir(Some(dir.clone()));

        let model = ModelDefinition {
            id: TranscriptionModelId::ParakeetV3,
            name: Cow::Borrowed("Fixture"),
            realtime: "100x",
            word_error_rate: None,
            streams: false,
            runtime: ModelRuntime::Gguf(Cow::Borrowed(&ARTIFACT)),
            languages: Cow::Borrowed(&["en"]),
            accepts_language_hint: false,
            supports_language_detection: false,
        };

        assert_eq!(
            readiness(&model, "en"),
            ModelReadiness {
                installed: false,
                verified: false,
                ready: false,
                needs_check: false,
            }
        );

        std::fs::write(dir.join("fixture.gguf"), b"test").unwrap();
        assert_eq!(
            readiness(&model, "en"),
            ModelReadiness {
                installed: true,
                verified: false,
                ready: false,
                needs_check: true,
            }
        );

        crate::transcription::models::verify_installed(
            &model,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            readiness(&model, "en"),
            ModelReadiness {
                installed: true,
                verified: true,
                ready: true,
                needs_check: false,
            }
        );

        set_test_models_dir(None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn catalog_models_projects_every_available_model_with_presentation_languages() {
        let inventory = catalog_models("en");
        let expected_ids: Vec<_> = MODELS
            .iter()
            .filter(|model| model.available())
            .map(|model| model.id)
            .collect();
        for id in expected_ids {
            let entry = inventory
                .iter()
                .find(|model| model.id == id)
                .expect("available catalog model missing from inventory");
            let def = crate::transcription::models::definition(id);
            assert_eq!(entry.languages, def.presentation_languages());
            assert_eq!(entry.vocabulary, def.vocabulary_use());
        }
    }
}

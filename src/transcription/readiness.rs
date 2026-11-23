//! One readiness check for a speech model: installed, and for a GGUF file
//! checked against its pinned artifact. Onboarding, Settings, and every
//! listener decide with it, so none of them calls a model ready while another
//! refuses it.

use super::models::{ModelDefinition, ModelRuntime, is_installed, is_verified};

/// Whether `model` can run for `language`: installed, and for a GGUF file
/// checked against its pinned artifact. A file another app downloaded
/// becomes ready once a full check records it (found_models::adopt_catalog).
pub fn is_ready(model: &ModelDefinition, language: &str) -> bool {
    is_installed(model, language)
        && (!matches!(model.runtime, ModelRuntime::Gguf(_)) || is_verified(model))
}

/// Whether a model's file is here but not ready yet: Enable can verify and
/// adopt it with the existing installed-only preparation. A model with no
/// file is not a check (it stays "Model required").
pub fn needs_check(model: &ModelDefinition, language: &str) -> bool {
    is_installed(model, language) && !is_ready(model, language)
}

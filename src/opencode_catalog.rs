//! The OpenCode model catalog as the window shows it: loading it off the UI
//! thread, the notice when OpenCode is missing or failing, and how a model
//! key is named, searched, and offered thinking variants.

use std::sync::mpsc::{Receiver, sync_channel};
use std::thread;
use std::time::Duration;

use crate::desktop_i18n::tr;
use crate::dictation_processor::ModelCatalog;

/// How often a missing `opencode2` install is looked for again.
pub(crate) const OPENCODE_INSTALL_RETRY_INTERVAL: Duration = Duration::from_secs(60);
const MAX_OPENCODE_ERROR_BYTES: usize = 4 * 1024;
const OPENCODE_BETA_DOCS_URL: &str = "https://v2.opencode.ai/";

pub(crate) enum ModelCatalogState {
    Loading,
    Loaded(ModelCatalog),
    Missing,
    Failed(String),
}

pub(crate) struct OpenCodeUnavailableCopy {
    pub(crate) title: &'static str,
    pub(crate) description: &'static str,
    pub(crate) error: Option<String>,
    pub(crate) can_retry: bool,
    pub(crate) retry_label: &'static str,
    pub(crate) can_open_setup: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum OpenCodeFeature {
    VoiceAction,
    Transformation,
}

pub(crate) fn opencode_unavailable_copy(
    state: &ModelCatalogState,
    feature: OpenCodeFeature,
) -> Option<OpenCodeUnavailableCopy> {
    match state {
        ModelCatalogState::Loading => Some(OpenCodeUnavailableCopy {
            title: tr("Checking for OpenCode"),
            description: match feature {
                OpenCodeFeature::VoiceAction => {
                    tr("Voice Action will be available after Hex connects to OpenCode.")
                }
                OpenCodeFeature::Transformation => {
                    tr("OpenCode transformation will be available after Hex connects to OpenCode.")
                }
            },
            error: None,
            can_retry: false,
            retry_label: tr("Retry"),
            can_open_setup: false,
        }),
        ModelCatalogState::Missing => Some(OpenCodeUnavailableCopy {
            title: match feature {
                OpenCodeFeature::VoiceAction => tr("Voice Action requires OpenCode"),
                OpenCodeFeature::Transformation => tr("Transformation requires OpenCode"),
            },
            description: match feature {
                OpenCodeFeature::VoiceAction => tr(
                    "Install and configure OpenCode to turn spoken instructions into paste-ready text.",
                ),
                OpenCodeFeature::Transformation => {
                    tr("Install and configure OpenCode to transform dictated text.")
                }
            },
            error: None,
            can_retry: true,
            retry_label: tr("Check again"),
            can_open_setup: true,
        }),
        ModelCatalogState::Failed(error) => Some(OpenCodeUnavailableCopy {
            title: tr("OpenCode is unavailable"),
            description: tr("Hex could not load the OpenCode model catalog."),
            error: Some(error.clone()),
            can_retry: true,
            retry_label: tr("Retry"),
            can_open_setup: false,
        }),
        ModelCatalogState::Loaded(_) => None,
    }
}

fn bounded_opencode_error(error: &str) -> String {
    if error.len() <= MAX_OPENCODE_ERROR_BYTES {
        return error.into();
    }
    let mut end = MAX_OPENCODE_ERROR_BYTES;
    while !error.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &error[..end])
}

pub(crate) fn start_model_catalog_load() -> Receiver<ModelCatalogState> {
    let (sender, receiver) = sync_channel(1);
    thread::spawn(move || {
        let state = if crate::dictation_processor::opencode_installed() {
            match crate::dictation_processor::load_model_catalog() {
                Ok(catalog) => ModelCatalogState::Loaded(catalog),
                Err(error) => {
                    let error = bounded_opencode_error(&error.to_string());
                    tracing::warn!(%error, "could not load OpenCode model catalog");
                    ModelCatalogState::Failed(error)
                }
            }
        } else {
            ModelCatalogState::Missing
        };
        let _ = sender.send(state);
    });
    receiver
}

pub(crate) fn open_opencode_beta_docs() {
    if let Err(error) = crate::commands::execute(crate::commands::Action::OpenUrl(
        OPENCODE_BETA_DOCS_URL.into(),
    )) {
        tracing::error!(%error, "could not open the OpenCode beta documentation");
    }
}

pub(crate) struct ModelPresentation {
    pub(crate) name: String,
    pub(crate) provider: String,
    #[cfg(test)]
    key: String,
    pub(crate) is_default: bool,
}

pub(crate) fn filtered_model_keys(catalog: &ModelCatalog, query: &str) -> Vec<Option<String>> {
    let mut choices = Vec::new();
    if query.is_empty() && catalog.default_name.is_some() {
        choices.push(None);
    }
    choices.extend(
        catalog
            .models
            .iter()
            .filter(|choice| {
                query.is_empty()
                    || choice.key.to_lowercase().contains(query)
                    || choice.name.to_lowercase().contains(query)
                    || choice.provider.to_lowercase().contains(query)
            })
            .take(8)
            .map(|choice| Some(choice.key.clone())),
    );
    choices
}

pub(crate) fn model_presentation(
    catalog: &ModelCatalogState,
    selected_key: Option<&str>,
) -> Option<ModelPresentation> {
    let ModelCatalogState::Loaded(catalog) = catalog else {
        return selected_key.map(fallback_model_presentation);
    };
    let is_default = selected_key.is_none();
    let key = selected_key.or(catalog.default_key.as_deref())?;
    let choice = catalog.models.iter().find(|choice| choice.key == key);
    let name = choice
        .map(|choice| choice.name.clone())
        .or_else(|| is_default.then(|| catalog.default_name.clone()).flatten())
        .unwrap_or_else(|| model_id(key).to_owned());
    let provider = choice
        .map(|choice| provider_label(&choice.provider))
        .unwrap_or_else(|| {
            provider_label(key.split_once('/').map_or(key, |(provider, _)| provider))
        });
    Some(ModelPresentation {
        name,
        provider,
        #[cfg(test)]
        key: key.to_owned(),
        is_default,
    })
}

pub(crate) fn model_variants(catalog: &ModelCatalog, selected_key: Option<&str>) -> Vec<String> {
    selected_key
        .or(catalog.default_key.as_deref())
        .and_then(|key| catalog.models.iter().find(|choice| choice.key == key))
        .map_or_else(Vec::new, |choice| choice.variants.clone())
}

fn fallback_model_presentation(key: &str) -> ModelPresentation {
    let provider = key.split_once('/').map_or(key, |(provider, _)| provider);
    ModelPresentation {
        name: model_id(key).to_owned(),
        provider: provider_label(provider),
        #[cfg(test)]
        key: key.to_owned(),
        is_default: false,
    }
}

fn model_id(key: &str) -> &str {
    key.split_once('/').map_or(key, |(_, model)| model)
}

fn provider_label(provider: &str) -> String {
    if provider.eq_ignore_ascii_case("opencode") {
        return "OpenCode".into();
    }
    provider
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(characters).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictation_processor::ModelChoice;

    fn example_choice(variants: Vec<String>) -> ModelChoice {
        ModelChoice {
            key: "opencode/example-rewrite".into(),
            name: "Example Rewrite".into(),
            provider: "opencode".into(),
            variants,
        }
    }

    #[test]
    fn selected_model_presents_provider_name_and_resolved_key() {
        let catalog = ModelCatalogState::Loaded(ModelCatalog {
            models: vec![example_choice(Vec::new())],
            default_key: Some("opencode/example-rewrite".into()),
            default_name: Some("Example Rewrite".into()),
            default_variant: None,
        });

        let selected = model_presentation(&catalog, Some("opencode/example-rewrite")).unwrap();
        assert_eq!(selected.name, "Example Rewrite");
        assert_eq!(selected.provider, "OpenCode");
        assert_eq!(selected.key, "opencode/example-rewrite");
        assert!(!selected.is_default);

        let default = model_presentation(&catalog, None).unwrap();
        assert!(default.is_default);
    }

    #[test]
    fn model_search_finds_provider_models_with_nested_ids() {
        let catalog = ModelCatalog {
            models: vec![ModelChoice {
                key: "console-groq/openai/gpt-oss-120b".into(),
                name: "GPT OSS 120B".into(),
                provider: "console-groq".into(),
                variants: Vec::new(),
            }],
            default_key: None,
            default_name: None,
            default_variant: None,
        };

        assert_eq!(
            filtered_model_keys(&catalog, "gpt oss"),
            [Some("console-groq/openai/gpt-oss-120b".into())]
        );
    }

    #[test]
    fn opencode_failure_copy_preserves_the_actionable_error() {
        let state = ModelCatalogState::Failed(
            "OpenCode /api/model failed: config.providers was invalid".into(),
        );

        let copy = opencode_unavailable_copy(&state, OpenCodeFeature::VoiceAction).unwrap();

        assert_eq!(copy.title, "OpenCode is unavailable");
        assert_eq!(
            copy.description,
            "Hex could not load the OpenCode model catalog."
        );
        assert_eq!(
            copy.error.as_deref(),
            Some("OpenCode /api/model failed: config.providers was invalid")
        );
        assert!(copy.can_retry);
        assert!(!copy.can_open_setup);
    }

    #[test]
    fn opencode_missing_copy_offers_setup_without_an_error() {
        let copy =
            opencode_unavailable_copy(&ModelCatalogState::Missing, OpenCodeFeature::VoiceAction)
                .unwrap();

        assert_eq!(copy.title, "Voice Action requires OpenCode");
        assert_eq!(copy.error, None);
        assert!(copy.can_retry);
        assert!(copy.can_open_setup);
    }

    #[test]
    fn opencode_transformation_copy_describes_modes_without_changing_actions() {
        let loading =
            opencode_unavailable_copy(&ModelCatalogState::Loading, OpenCodeFeature::Transformation)
                .unwrap();
        assert_eq!(loading.title, "Checking for OpenCode");
        assert_eq!(
            loading.description,
            "OpenCode transformation will be available after Hex connects to OpenCode."
        );
        assert!(!loading.can_retry);
        assert!(!loading.can_open_setup);

        let missing =
            opencode_unavailable_copy(&ModelCatalogState::Missing, OpenCodeFeature::Transformation)
                .unwrap();
        assert_eq!(missing.title, "Transformation requires OpenCode");
        assert_eq!(
            missing.description,
            "Install and configure OpenCode to transform dictated text."
        );
        assert_eq!(missing.error, None);
        assert!(missing.can_retry);
        assert_eq!(missing.retry_label, "Check again");
        assert!(missing.can_open_setup);

        let failed = ModelCatalogState::Failed("catalog failure".into());
        let error = opencode_unavailable_copy(&failed, OpenCodeFeature::Transformation).unwrap();
        assert_eq!(error.title, "OpenCode is unavailable");
        assert_eq!(error.error.as_deref(), Some("catalog failure"));
        assert!(error.can_retry);
        assert!(!error.can_open_setup);
        let loaded = ModelCatalogState::Loaded(ModelCatalog {
            models: Vec::new(),
            default_key: None,
            default_name: None,
            default_variant: None,
        });
        for feature in [
            OpenCodeFeature::VoiceAction,
            OpenCodeFeature::Transformation,
        ] {
            assert!(opencode_unavailable_copy(&loaded, feature).is_none());
        }
    }

    #[test]
    fn opencode_errors_are_bounded_without_splitting_utf8() {
        let error = "é".repeat(MAX_OPENCODE_ERROR_BYTES);
        let bounded = bounded_opencode_error(&error);

        assert!(bounded.ends_with('…'));
        assert!(bounded.is_char_boundary(bounded.len()));
        assert!(bounded.len() <= MAX_OPENCODE_ERROR_BYTES + '…'.len_utf8());
    }

    #[test]
    fn default_model_exposes_its_thinking_variants() {
        let catalog = ModelCatalog {
            models: vec![example_choice(vec!["low".into(), "high".into()])],
            default_key: Some("opencode/example-rewrite".into()),
            default_name: Some("Example Rewrite".into()),
            default_variant: None,
        };

        assert_eq!(model_variants(&catalog, None), ["low", "high"]);
    }
}

//! The OpenCode model catalog as the window shows it: loading it off the UI
//! thread, the notice when OpenCode isn't running or fails, and how a model
//! key is named, searched, and offered thinking variants.

use std::sync::mpsc::{Receiver, sync_channel};
use std::thread;
use std::time::Duration;

use crate::desktop::i18n::tr;
use crate::dictation::processor::ModelCatalog;

/// How often a stopped OpenCode service is looked for again while a pane
/// that needs it shows.
pub(crate) const OPENCODE_RETRY_INTERVAL: Duration = Duration::from_secs(60);
const MAX_OPENCODE_ERROR_BYTES: usize = 4 * 1024;
const OPENCODE_DOCS_URL: &str = "https://v2.opencode.ai/";

pub(crate) enum ModelCatalogState {
    Loading,
    Loaded(ModelCatalog),
    /// No OpenCode service answers; Hex never starts one.
    NotRunning,
    Failed(String),
}

pub(crate) struct OpenCodeUnavailableCopy {
    /// Why, in a few words, for beside a switch the user can't turn on.
    pub(crate) reason: &'static str,
    pub(crate) title: &'static str,
    pub(crate) description: &'static str,
    pub(crate) error: Option<String>,
    pub(crate) can_retry: bool,
    pub(crate) retry_label: &'static str,
    pub(crate) can_open_setup: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum OpenCodeFeature {
    Transformation,
}

pub(crate) fn opencode_unavailable_copy(
    state: &ModelCatalogState,
    feature: OpenCodeFeature,
) -> Option<OpenCodeUnavailableCopy> {
    let OpenCodeFeature::Transformation = feature;
    match state {
        ModelCatalogState::Loading => Some(OpenCodeUnavailableCopy {
            reason: tr("Checking for OpenCode"),
            title: tr("Checking for OpenCode"),
            description: tr(
                "OpenCode transformation will be available after Hex connects to OpenCode.",
            ),
            error: None,
            can_retry: false,
            retry_label: tr("Retry"),
            can_open_setup: false,
        }),
        ModelCatalogState::NotRunning => Some(OpenCodeUnavailableCopy {
            reason: tr("OpenCode isn't running"),
            title: tr("OpenCode isn't running"),
            description: tr("Install or start OpenCode to transform dictated text."),
            error: None,
            can_retry: true,
            retry_label: tr("Check again"),
            can_open_setup: true,
        }),
        ModelCatalogState::Failed(error) => Some(OpenCodeUnavailableCopy {
            reason: tr("OpenCode is unavailable"),
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
        let state = match crate::dictation::processor::load_model_catalog() {
            Ok(Some(catalog)) => ModelCatalogState::Loaded(catalog),
            Ok(None) => ModelCatalogState::NotRunning,
            Err(error) => {
                let error = bounded_opencode_error(&error.to_string());
                tracing::warn!(%error, "could not load OpenCode model catalog");
                ModelCatalogState::Failed(error)
            }
        };
        let _ = sender.send(state);
    });
    receiver
}

pub(crate) fn open_url(url: &str) -> color_eyre::eyre::Result<()> {
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(url).status()?;
    #[cfg(target_os = "linux")]
    let status = std::process::Command::new("xdg-open").arg(url).status()?;
    #[cfg(target_os = "windows")]
    let status = crate::background_process::command("rundll32")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .status()?;
    if !status.success() {
        return Err(color_eyre::eyre::eyre!("open failed for {url}: {status}"));
    }
    Ok(())
}

pub(crate) fn open_opencode_docs() {
    if let Err(error) = open_url(OPENCODE_DOCS_URL) {
        tracing::error!(%error, "could not open the OpenCode documentation");
    }
}

pub(crate) struct ModelPresentation {
    pub(crate) name: String,
    pub(crate) provider: String,
    #[cfg(test)]
    key: String,
    pub(crate) is_default: bool,
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

pub(crate) fn provider_label(provider: &str) -> String {
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
    use crate::dictation::processor::ModelChoice;

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
    fn opencode_failure_copy_preserves_the_actionable_error() {
        let state = ModelCatalogState::Failed(
            "OpenCode /api/model failed: config.providers was invalid".into(),
        );

        let copy = opencode_unavailable_copy(&state, OpenCodeFeature::Transformation).unwrap();

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

        let missing = opencode_unavailable_copy(
            &ModelCatalogState::NotRunning,
            OpenCodeFeature::Transformation,
        )
        .unwrap();
        assert_eq!(missing.title, "OpenCode isn't running");
        assert_eq!(
            missing.description,
            "Install or start OpenCode to transform dictated text."
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
        assert!(opencode_unavailable_copy(&loaded, OpenCodeFeature::Transformation).is_none());
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

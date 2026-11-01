use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};

use crate::app_settings::DictationProcessingSettings;
use crate::dictation::processor::{Profile, Profiles};
use crate::dictation::text_replacements::ReplacementSet;

static DICTATION_PROFILES: OnceLock<RwLock<Profiles>> = OnceLock::new();
static READS_BROWSER: AtomicBool = AtomicBool::new(false);

/// Whether anything the user set up looks at the page: a mode chosen by
/// website, an OpenCode rewrite (its prompt names the site), or a script
/// transformation (it receives the page address). Otherwise Hex never asks a
/// browser for its page, so macOS never shows an Automation prompt for it.
pub fn reads_browser() -> bool {
    READS_BROWSER.load(Ordering::Acquire)
}

fn uses_browser(settings: &DictationProcessingSettings) -> bool {
    settings
        .modes
        .iter()
        .any(|mode| !mode.browser_hosts.is_empty())
        || std::iter::once(&settings.default_mode)
            .chain(&settings.modes)
            .any(|mode| {
                mode.post_processing.enabled
                    || mode
                        .transformations
                        .iter()
                        .any(|id| !crate::personal_commands::is_builtin_transformation(id))
            })
}

pub fn dictation_profiles() -> Profiles {
    dictation_profiles_lock()
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

pub fn update_dictation_profiles(settings: &DictationProcessingSettings) {
    let profiles = build_dictation_profiles(settings);
    READS_BROWSER.store(uses_browser(settings), Ordering::Release);
    *dictation_profiles_lock()
        .write()
        .unwrap_or_else(|error| error.into_inner()) = profiles;
}

fn dictation_profiles_lock() -> &'static RwLock<Profiles> {
    DICTATION_PROFILES.get_or_init(|| {
        RwLock::new(build_dictation_profiles(
            &DictationProcessingSettings::default(),
        ))
    })
}

fn build_dictation_profiles(settings: &DictationProcessingSettings) -> Profiles {
    let global = &settings.default_mode;
    let mut profiles = Profiles::new(
        Profile::configured(
            "Global",
            ReplacementSet::default(),
            global.transformations.clone(),
            &global.post_processing,
        )
        .vocabulary(global.vocabulary.clone()),
    )
    .global_replacements(ReplacementSet::new(&global.replacements));
    for mode in &settings.modes {
        let profile = Profile::from_mode(mode);
        for application in &mode.applications {
            profiles = profiles.application(application.clone(), profile.clone());
        }
        for host in &mode.browser_hosts {
            profiles = profiles.browser_host(host.clone(), profile.clone());
        }
    }
    profiles
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictation::context::ContextSnapshot;

    #[test]
    fn configured_mode_profiles_match_application_context() {
        let settings = DictationProcessingSettings {
            modes: vec![crate::app_settings::DictationMode {
                applications: vec!["Slack".into()],
                transformations: vec!["lowercase".into()],
                ..crate::app_settings::DictationMode::default()
            }],
            ..DictationProcessingSettings::default()
        };
        let context = ContextSnapshot {
            application: Some("Slack".into()),
            ..ContextSnapshot::default()
        };

        assert!(build_dictation_profiles(&settings).processes(&context));
    }

    #[test]
    fn global_corrections_run_before_an_app_modes_own() {
        use crate::app_settings::DictationMode;
        use crate::dictation::text_replacements::TextReplacement;
        let pair = |phrase: &str, output: &str| TextReplacement {
            matched_phrase: phrase.into(),
            output: output.into(),
        };
        let mut settings = DictationProcessingSettings {
            modes: vec![DictationMode {
                applications: vec!["Slack".into()],
                replacements: vec![pair("OpenCode", "OpenCode :rocket:")],
                ..DictationMode::default()
            }],
            ..DictationProcessingSettings::default()
        };
        settings.default_mode.replacements = vec![pair("open code", "OpenCode")];
        let profiles = build_dictation_profiles(&settings);
        let process = |application: &str| {
            let context = ContextSnapshot {
                application: Some(application.into()),
                ..ContextSnapshot::default()
            };
            profiles
                .process_cancellable("I like open code", &context, &AtomicBool::new(false))
                .text
        };

        assert_eq!(process("Slack"), "I like OpenCode :rocket:");
        assert_eq!(process("Notes"), "I like OpenCode");
    }

    #[test]
    fn an_app_mode_has_its_own_vocabulary() {
        use crate::app_settings::DictationMode;
        use crate::dictation::vocabulary::VocabularyEntry;
        let term = |term: &str| VocabularyEntry {
            term: term.into(),
            sounds_like: Vec::new(),
        };
        let settings = DictationProcessingSettings {
            modes: vec![DictationMode {
                applications: vec!["Zed".into()],
                vocabulary: vec![term("useEffect")],
                ..DictationMode::default()
            }],
            ..DictationProcessingSettings::default()
        };
        let profiles = build_dictation_profiles(&settings);
        let in_app = |application: &str| {
            profiles.vocabulary_for(&ContextSnapshot {
                application: Some(application.into()),
                ..ContextSnapshot::default()
            })
        };

        assert_eq!(in_app("Zed"), [term("useEffect")]);
        assert!(in_app("Notes").is_empty());
    }

    #[test]
    fn browsers_are_asked_only_when_something_uses_the_page() {
        use crate::app_settings::{DictationMode, DictationPostProcessing};
        let with = |mode: DictationMode| DictationProcessingSettings {
            modes: vec![mode],
            ..DictationProcessingSettings::default()
        };

        assert!(!uses_browser(&DictationProcessingSettings::default()));
        assert!(!uses_browser(&with(DictationMode {
            applications: vec!["Slack".into()],
            transformations: vec!["lowercase".into()],
            ..DictationMode::default()
        })));
        assert!(uses_browser(&with(DictationMode {
            browser_hosts: vec!["x.com".into()],
            ..DictationMode::default()
        })));
        assert!(uses_browser(&with(DictationMode {
            applications: vec!["Slack".into()],
            transformations: vec!["my-script".into()],
            ..DictationMode::default()
        })));
        assert!(uses_browser(&with(DictationMode {
            applications: vec!["Slack".into()],
            post_processing: DictationPostProcessing {
                enabled: true,
                ..DictationPostProcessing::default()
            },
            ..DictationMode::default()
        })));
    }
}

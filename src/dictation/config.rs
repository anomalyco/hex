use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};

use crate::app_settings::DictationProcessingSettings;
use crate::dictation::processor::{Profile, Profiles};

static DICTATION_PROFILES: OnceLock<RwLock<Profiles>> = OnceLock::new();
static READS_BROWSER: AtomicBool = AtomicBool::new(false);
static READS_WINDOW_TITLE: AtomicBool = AtomicBool::new(false);

/// Whether anything the user set up looks at the page: a mode chosen by
/// website, an OpenCode rewrite (its prompt names the site), or a script
/// transformation (it receives the page address). Otherwise Hex never asks a
/// browser for its page, so macOS never shows an Automation prompt for it.
pub fn reads_browser() -> bool {
    READS_BROWSER.load(Ordering::Acquire)
}

/// Whether a script transformation is set up, the only reader of the front
/// window's title. Otherwise Hex never reads it.
pub fn reads_window_title() -> bool {
    READS_WINDOW_TITLE.load(Ordering::Acquire)
}

fn uses_scripts(settings: &DictationProcessingSettings) -> bool {
    std::iter::once(&settings.default_mode)
        .chain(&settings.modes)
        .flat_map(|mode| &mode.transformations)
        .any(|id| !crate::personal_commands::is_builtin_transformation(id))
}

fn uses_browser(settings: &DictationProcessingSettings) -> bool {
    uses_scripts(settings)
        || std::iter::once(&settings.default_mode)
            .chain(&settings.modes)
            .any(|mode| !mode.browser_hosts.is_empty() || mode.post_processing.enabled)
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
    READS_WINDOW_TITLE.store(uses_scripts(settings), Ordering::Release);
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
    let mut profiles = Profiles::new(Profile::from_mode(&settings.default_mode));
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
        let script = with(DictationMode {
            applications: vec!["Slack".into()],
            transformations: vec!["my-script".into()],
            ..DictationMode::default()
        });
        assert!(uses_browser(&script));
        assert!(uses_scripts(&script));
        let rewrite = with(DictationMode {
            applications: vec!["Slack".into()],
            post_processing: DictationPostProcessing {
                enabled: true,
                ..DictationPostProcessing::default()
            },
            ..DictationMode::default()
        });
        assert!(uses_browser(&rewrite));
        assert!(!uses_scripts(&rewrite));
    }
}

use std::sync::{OnceLock, RwLock};

use crate::app_settings::DictationProcessingSettings;
use crate::dictation_processor::{Profile, Profiles};

static DICTATION_PROFILES: OnceLock<RwLock<Profiles>> = OnceLock::new();

pub fn dictation_profiles() -> Profiles {
    dictation_profiles_lock()
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

pub fn update_dictation_profiles(settings: &DictationProcessingSettings) {
    let profiles = build_dictation_profiles(
        settings,
        crate::experiments::enabled(crate::experiments::Experiment::Modes),
    );
    *dictation_profiles_lock()
        .write()
        .unwrap_or_else(|error| error.into_inner()) = profiles;
}

fn dictation_profiles_lock() -> &'static RwLock<Profiles> {
    DICTATION_PROFILES.get_or_init(|| {
        RwLock::new(build_dictation_profiles(
            &DictationProcessingSettings::default(),
            true,
        ))
    })
}

/// With the `modes` experiment off only the global replacements run.
fn build_dictation_profiles(settings: &DictationProcessingSettings, modes_on: bool) -> Profiles {
    if !modes_on {
        return Profiles::new(Profile::global_replacements(&settings.default_mode));
    }
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
    use crate::context::ContextSnapshot;

    #[test]
    fn without_the_modes_experiment_the_profile_is_global_replacements_only() {
        let mut settings = DictationProcessingSettings::default();
        settings.default_mode.transformations = vec!["lowercase".into()];
        settings.modes = vec![crate::app_settings::DictationMode {
            applications: vec!["Slack".into()],
            transformations: vec!["lowercase".into()],
            ..crate::app_settings::DictationMode::default()
        }];
        let context = ContextSnapshot {
            application: Some("Slack".into()),
            ..ContextSnapshot::default()
        };

        assert!(build_dictation_profiles(&settings, true).processes(&context));
        assert!(!build_dictation_profiles(&settings, false).processes(&context));
    }
}

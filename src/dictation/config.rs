use std::sync::{OnceLock, RwLock};

use crate::app_settings::DictationProcessingSettings;
use crate::dictation::processor::{Profile, Profiles};

static DICTATION_PROFILES: OnceLock<RwLock<Profiles>> = OnceLock::new();

pub fn dictation_profiles() -> Profiles {
    dictation_profiles_lock()
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

pub fn update_dictation_profiles(settings: &DictationProcessingSettings) {
    let profiles = build_dictation_profiles(settings);
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
}

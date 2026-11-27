//! The support and write contract shared by settings descriptions and writes.

use super::{ControlError, Key, managed, registry::needs};
use crate::labs::{Feature, Os};
use crate::policy::Policy;

pub(super) struct Environment<'a> {
    pub os: Os,
    pub dev_build: bool,
    pub policy: Policy,
    pub capability: &'a dyn Fn(&str) -> bool,
    pub feature_on_os: &'a dyn Fn(Feature) -> bool,
}

impl Environment<'static> {
    pub fn current() -> Self {
        Self {
            os: Os::CURRENT,
            dev_build: crate::dev_build::is_dev_build(),
            policy: Policy::current(),
            capability: &crate::rune_ui::capability,
            feature_on_os: &Feature::on_this_os,
        }
    }
}

pub(super) struct SettingAccess {
    pub supported: bool,
    pub writable: bool,
    pub refusal: Option<ControlError>,
}

pub(super) fn setting_access(key: &Key, env: &Environment<'_>) -> SettingAccess {
    let unsupported = unsupported(key.key, env);
    let supported = unsupported.is_none();
    let refusal = key.read_only.map(|why| {
        ControlError::new(
            "read-only",
            format!("`{}` can't be set here: {why}", key.key),
        )
    });
    let refusal = refusal
        .or_else(|| {
            env.policy
                .setting_lock(key.key)
                .map(|lock| managed(key.key, lock))
        })
        .or(unsupported);
    SettingAccess {
        supported,
        writable: refusal.is_none(),
        refusal,
    }
}

pub(super) fn check_write(key: &Key, env: &Environment<'_>) -> Result<(), ControlError> {
    match setting_access(key, env).refusal {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn unsupported(name: &str, env: &Environment<'_>) -> Option<ControlError> {
    let why = if let Some(feature) = Feature::for_settings_key(name)
        && !feature.audience().reaches(env.dev_build)
    {
        "belongs to a Labs feature that only dev builds have".to_owned()
    } else if let Some(feature) = Feature::for_settings_key(name)
        && (name.starts_with("labs.") || name == "capture.replay_hotkey")
        && !(env.feature_on_os)(feature)
    {
        "belongs to a Labs feature this OS doesn't have".to_owned()
    } else if env.os != Os::Macos && name.starts_with("screenshots.") {
        "belongs to Labs › Screenshots, which only macOS has".to_owned()
    } else if env.os != Os::Macos && name == "extra_dictation_hotkeys" {
        "belongs to Labs › More dictation shortcuts, which only macOS has".to_owned()
    } else {
        let capability = needs(name).filter(|name| !(env.capability)(name))?;
        format!("does nothing on this OS: it needs {capability}, which Hex has only elsewhere")
    };
    Some(ControlError::new("unsupported", format!("`{name}` {why}")))
}

#[cfg(test)]
mod tests {
    use super::super::{AppSettings, describe, keys};
    use super::*;
    use crate::policy::Lock;

    fn capability(os: Os, name: &str) -> bool {
        match name {
            "calendar" | "dock_icon" => os == Os::Macos,
            "meetings" | "keyboard_model_switching" => true,
            "fun" => os != Os::Windows,
            "typed_text" => os == Os::Linux,
            "app_model_switching" => os != Os::Linux,
            "listener_control" => os != Os::Macos,
            "share_shortcut" => os == Os::Windows,
            _ => panic!("add a capability fixture for {name}"),
        }
    }

    #[test]
    fn every_key_describes_the_same_support_and_write_contract_in_every_environment() {
        let json = serde_json::to_value(AppSettings::default()).unwrap();
        for os in [Os::Macos, Os::Windows, Os::Linux] {
            for dev_build in [false, true] {
                for session_features in [false, true] {
                    for policy in [
                        Policy::default(),
                        Policy::default()
                            .with(Lock::Labs)
                            .with(Lock::Meetings)
                            .with(Lock::Annotate),
                    ] {
                        let capabilities = |name: &str| {
                            capability(os, name)
                                && (name != "keyboard_model_switching" || session_features)
                        };
                        let features = |feature: Feature| {
                            feature.oses().contains(&os)
                                && (!matches!(
                                    feature,
                                    Feature::AnnotateReplay | Feature::KeyboardModelSwitching
                                ) || session_features)
                        };
                        let env = Environment {
                            os,
                            dev_build,
                            policy,
                            capability: &capabilities,
                            feature_on_os: &features,
                        };
                        for key in keys() {
                            let described = describe(key, &json, &env);
                            let write = check_write(key, &env);
                            assert_eq!(
                                described.writable,
                                write.is_ok(),
                                "{}: {os:?}, dev={dev_build}",
                                key.key
                            );
                            let unlocked = Environment {
                                policy: Policy::default(),
                                ..env
                            };
                            assert_eq!(
                                described.available,
                                describe(key, &json, &unlocked).available,
                                "locks cannot change support: {}",
                                key.key
                            );
                            if !described.available {
                                assert!(!described.writable, "{}", key.key);
                            }
                            if key.read_only.is_some() {
                                assert_eq!(write.unwrap_err().code, "read-only");
                            }
                        }
                        for name in ["screenshots.hotkey", "extra_dictation_hotkeys"] {
                            let key = keys().find(|key| key.key == name).unwrap();
                            assert_eq!(
                                describe(key, &json, &env).available,
                                os == Os::Macos,
                                "{name}"
                            );
                        }
                        for key in keys().filter(|key| {
                            key.key.starts_with("labs.") || key.key == "capture.replay_hotkey"
                        }) {
                            if let Some(feature) = Feature::for_settings_key(key.key)
                                && (!features(feature) || !feature.audience().reaches(dev_build))
                            {
                                assert!(!describe(key, &json, &env).available, "{}", key.key);
                            }
                        }
                        let key = keys().find(|key| key.key == "meetings.enabled").unwrap();
                        let access = setting_access(key, &env);
                        assert!(access.supported);
                        assert_eq!(access.writable, policy.setting_lock(key.key).is_none());
                    }
                }
            }
        }
    }
}

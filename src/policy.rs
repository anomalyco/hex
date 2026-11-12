//! Feature locks an organization sets through managed preferences (macOS
//! configuration profiles) in the `anomaly.hex` domain: `DisableMeetings`,
//! `DisableAnnotate`, `DisableOpenCodeProcessing`, `DisableTransforms`, and
//! `DisableLabs`, plus `RestrictTransformsToLoopback`, which keeps
//! transformations on but holds their `fetch` to this computer and runs no
//! programs. Only a value the profile forces counts, so a key a person
//! wrote with `defaults write` locks nothing. Other OSes have no locks.
//!
//! The policy is read once per process. A locked feature reads as off
//! everywhere without its saved setting changing: `AppSettings` turns its
//! switches off as they load and writes the saved values back on save
//! ([`Policy::enforce`], [`Policy::keep_saved`]); work that follows a list
//! rather than a switch (mode rewrites, transformations, Annotate's sends,
//! the transformation host) checks [`Policy::locks`] where it starts.

use std::sync::OnceLock;

use crate::app_settings::{AppSettings, MeetingSummaries};

/// One managed lock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lock {
    /// No new meeting recording or meeting processing; existing meetings stay
    /// readable (`hex meetings`, `hex mcp`).
    Meetings,
    Annotate,
    /// No OpenCode generation: mode rewrites, meeting notes, and Annotate's
    /// sends to OpenCode sessions.
    OpenCodeProcessing,
    /// No transformations at all, built in or from `hex.config.ts`.
    Transforms,
    /// Every Labs feature off.
    Labs,
    /// Transformations stay on, but `fetch` reaches only loopback addresses
    /// (after DNS, on every redirect) and no program runs
    /// (`personal_commands::script`).
    LoopbackTransforms,
}

impl Lock {
    pub const ALL: [Self; 6] = [
        Self::Meetings,
        Self::Annotate,
        Self::OpenCodeProcessing,
        Self::Transforms,
        Self::Labs,
        Self::LoopbackTransforms,
    ];

    /// Its key in the `anomaly.hex` managed preferences.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Meetings => "DisableMeetings",
            Self::Annotate => "DisableAnnotate",
            Self::OpenCodeProcessing => "DisableOpenCodeProcessing",
            Self::Transforms => "DisableTransforms",
            Self::Labs => "DisableLabs",
            Self::LoopbackTransforms => "RestrictTransformsToLoopback",
        }
    }

    const fn bit(self) -> u8 {
        1 << self as u8
    }

    /// What a refusal says.
    pub const fn refusal(self) -> &'static str {
        match self {
            Self::Meetings => "Meetings are managed by your organization",
            Self::Annotate => "Annotate is managed by your organization",
            Self::OpenCodeProcessing => "OpenCode processing is managed by your organization",
            Self::Transforms => "Transformations are managed by your organization",
            Self::Labs => "Labs features are managed by your organization",
            Self::LoopbackTransforms => {
                "Transformations may only reach this computer, by your organization's policy"
            }
        }
    }
}

/// The locks in force.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Policy(u8);

impl Policy {
    /// This process's policy, read once.
    pub fn current() -> Self {
        static CURRENT: OnceLock<Policy> = OnceLock::new();
        *CURRENT.get_or_init(|| {
            let policy = Self::from_forced(platform::forced);
            if policy != Self::default() {
                tracing::info!(
                    locks = ?Lock::ALL.into_iter().filter(|lock| policy.locks(*lock)).map(Lock::key).collect::<Vec<_>>(),
                    "features managed by your organization"
                );
            }
            policy
        })
    }

    /// The locks whose key `forced` reports forced on.
    fn from_forced(forced: impl Fn(&str) -> bool) -> Self {
        Self(
            Lock::ALL
                .into_iter()
                .filter(|lock| forced(lock.key()))
                .fold(0, |bits, lock| bits | lock.bit()),
        )
    }

    #[cfg(test)]
    pub fn with(self, lock: Lock) -> Self {
        Self(self.0 | lock.bit())
    }

    pub fn locks(self, lock: Lock) -> bool {
        self.0 & lock.bit() != 0
    }

    /// Whether any lock is in force.
    pub fn any(self) -> bool {
        self.0 != 0
    }

    /// The lock on a Labs feature: Labs, Meetings for Meetings, Annotate for
    /// Annotate replay, or OpenCode processing for a voice prompt.
    pub fn labs_lock(self, feature: crate::labs::Feature) -> Option<Lock> {
        if feature == crate::labs::Feature::Meetings && self.locks(Lock::Meetings) {
            return Some(Lock::Meetings);
        }
        if feature == crate::labs::Feature::AnnotateReplay && self.locks(Lock::Annotate) {
            return Some(Lock::Annotate);
        }
        if feature == crate::labs::Feature::VoicePrompt && self.locks(Lock::OpenCodeProcessing) {
            return Some(Lock::OpenCodeProcessing);
        }
        self.locks(Lock::Labs).then_some(Lock::Labs)
    }

    /// The lock on `hex settings` key `name`, if any.
    pub fn setting_lock(self, name: &str) -> Option<Lock> {
        if let Some(feature) = crate::labs::Feature::for_settings_key(name) {
            return self.labs_lock(feature);
        }
        let lock = match name {
            "capture.enabled" => Lock::Annotate,
            _ => return None,
        };
        self.locks(lock).then_some(lock)
    }

    /// The switches `lock` holds off in `settings`.
    fn switches(settings: &mut AppSettings, lock: Lock) -> Vec<&mut bool> {
        match lock {
            Lock::Meetings => vec![&mut settings.meetings.enabled],
            #[cfg(hex_capture)]
            Lock::Annotate => vec![
                &mut settings.capture.enabled,
                &mut settings.labs.annotate_replay,
            ],
            #[cfg(not(hex_capture))]
            Lock::Annotate => Vec::new(),
            // `meetings.summaries` is a choice, not a switch; see `enforce`.
            // A voice prompt is one of the OpenCode requests the lock stops.
            Lock::OpenCodeProcessing => vec![&mut settings.labs.voice_prompt],
            Lock::Transforms | Lock::LoopbackTransforms => Vec::new(),
            Lock::Labs => crate::labs::Feature::switches(settings),
        }
    }

    /// Turns every locked switch off in memory.
    pub fn enforce(self, settings: &mut AppSettings) {
        for lock in Lock::ALL.into_iter().filter(|lock| self.locks(*lock)) {
            for switch in Self::switches(settings, lock) {
                *switch = false;
            }
        }
        if self.locks(Lock::OpenCodeProcessing)
            && settings.meetings.summaries == MeetingSummaries::OpenCode
        {
            settings.meetings.summaries = MeetingSummaries::Off;
        }
    }

    /// Puts the saved values of the locked switches back into `settings`
    /// before it is written, so a lock never changes the file.
    pub fn keep_saved(self, settings: &mut AppSettings, saved: &AppSettings) {
        let mut saved = saved.clone();
        for lock in Lock::ALL.into_iter().filter(|lock| self.locks(*lock)) {
            for (switch, value) in Self::switches(settings, lock)
                .into_iter()
                .zip(Self::switches(&mut saved, lock))
            {
                *switch = *value;
            }
        }
        if self.locks(Lock::OpenCodeProcessing)
            && settings.meetings.summaries == MeetingSummaries::Off
            && saved.meetings.summaries == MeetingSummaries::OpenCode
        {
            settings.meetings.summaries = MeetingSummaries::OpenCode;
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2_core_foundation::{
        CFPreferencesAppValueIsForced, CFPreferencesGetAppBooleanValue, CFString,
    };

    const DOMAIN: &str = "anomaly.hex";

    /// Whether a configuration profile forces `key` to true.
    pub fn forced(key: &str) -> bool {
        let key = CFString::from_str(key);
        let domain = CFString::from_str(DOMAIN);
        if !CFPreferencesAppValueIsForced(&key, &domain) {
            return false;
        }
        // A null "exists" pointer is allowed; a missing or non-Boolean
        // value reads false.
        unsafe { CFPreferencesGetAppBooleanValue(&key, &domain, std::ptr::null_mut()) }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn forced(_key: &str) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_forced_keys_lock() {
        let policy = Policy::from_forced(|key| key == "DisableMeetings" || key == "DisableLabs");
        assert!(policy.locks(Lock::Meetings));
        assert!(policy.locks(Lock::Labs));
        assert!(!policy.locks(Lock::Annotate));
        assert!(!policy.locks(Lock::OpenCodeProcessing));
        assert!(!policy.locks(Lock::Transforms));
        assert!(!Policy::from_forced(|_| false).any());
        let keys: Vec<_> = Lock::ALL.map(Lock::key).into();
        assert_eq!(
            keys,
            [
                "DisableMeetings",
                "DisableAnnotate",
                "DisableOpenCodeProcessing",
                "DisableTransforms",
                "DisableLabs",
                "RestrictTransformsToLoopback"
            ]
        );
    }

    #[test]
    fn a_voice_prompt_is_locked_by_opencode_processing() {
        let feature = crate::labs::Feature::VoicePrompt;
        assert_eq!(Policy::default().labs_lock(feature), None);
        let opencode = Policy::default().with(Lock::OpenCodeProcessing);
        assert_eq!(
            opencode.labs_lock(feature),
            Some(Lock::OpenCodeProcessing),
            "the card shows it managed"
        );
        assert_eq!(
            opencode.setting_lock("labs.voice_prompt"),
            Some(Lock::OpenCodeProcessing)
        );
        assert_eq!(
            opencode.setting_lock("labs.voice_prompt_phrase"),
            Some(Lock::OpenCodeProcessing)
        );
        let labs = Policy::default().with(Lock::Labs);
        assert_eq!(labs.labs_lock(feature), Some(Lock::Labs));
    }

    #[test]
    fn the_opencode_lock_turns_only_the_voice_prompt_off_and_keeps_it_saved() {
        let mut saved = AppSettings::default();
        saved.labs.voice_prompt = true;
        saved.labs.live_text = true;
        let policy = Policy::default().with(Lock::OpenCodeProcessing);
        let mut loaded = saved.clone();
        policy.enforce(&mut loaded);
        assert!(!loaded.labs.voice_prompt, "the locked lab reads off");
        assert!(loaded.labs.live_text, "another lab is untouched");

        let mut written = loaded.clone();
        policy.keep_saved(&mut written, &saved);
        assert!(
            written.labs.voice_prompt,
            "the saved switch is written back"
        );
        assert!(written.labs.live_text);
    }

    #[test]
    fn locked_switches_read_off_and_save_as_they_were() {
        let mut saved = AppSettings::default();
        saved.meetings.enabled = true;
        saved.meetings.summaries = MeetingSummaries::OpenCode;
        saved.labs.live_text = true;
        saved.labs.voice_prompt = true;
        saved.pause_browser_media = true;
        let policy = Policy::default()
            .with(Lock::Meetings)
            .with(Lock::OpenCodeProcessing)
            .with(Lock::Labs);
        let mut loaded = saved.clone();
        policy.enforce(&mut loaded);
        assert!(!loaded.meetings.enabled);
        assert_eq!(loaded.meetings.summaries, MeetingSummaries::Off);
        assert!(!loaded.labs.live_text);
        assert!(!loaded.labs.voice_prompt);
        assert!(!loaded.pause_browser_media);
        for feature in crate::labs::Feature::ALL {
            assert!(!feature.on(&loaded), "{} reads on", feature.name());
        }

        // An unrelated edit saves with the user's own values kept.
        loaded.sound_effects = !saved.sound_effects;
        let mut written = loaded.clone();
        policy.keep_saved(&mut written, &saved);
        assert!(written.meetings.enabled);
        assert_eq!(written.meetings.summaries, MeetingSummaries::OpenCode);
        assert!(written.labs.live_text);
        assert!(written.labs.voice_prompt);
        assert!(written.pause_browser_media);
        assert_eq!(written.sound_effects, loaded.sound_effects);
    }

    #[test]
    fn the_opencode_lock_leaves_other_summary_choices_alone() {
        let policy = Policy::default().with(Lock::OpenCodeProcessing);
        let mut saved = AppSettings::default();
        saved.meetings.summaries = MeetingSummaries::OpenCode;
        let mut loaded = saved.clone();
        policy.enforce(&mut loaded);
        // Choosing On this Mac while locked is the user's own change.
        loaded.meetings.summaries = MeetingSummaries::Mac;
        let mut written = loaded.clone();
        policy.keep_saved(&mut written, &saved);
        assert_eq!(written.meetings.summaries, MeetingSummaries::Mac);
    }

    #[test]
    fn no_policy_changes_nothing() {
        let mut settings = AppSettings::default();
        settings.meetings.enabled = true;
        let before = settings.clone();
        Policy::default().enforce(&mut settings);
        assert_eq!(settings, before);
    }

    #[test]
    fn settings_keys_name_their_lock() {
        let none = Policy::default();
        assert_eq!(none.setting_lock("meetings.enabled"), None);
        let meetings = Policy::default().with(Lock::Meetings);
        assert_eq!(
            meetings.setting_lock("meetings.enabled"),
            Some(Lock::Meetings)
        );
        assert_eq!(meetings.setting_lock("labs.live_text"), None);
        let labs = Policy::default().with(Lock::Labs);
        assert_eq!(labs.setting_lock("labs.live_text"), Some(Lock::Labs));
        assert_eq!(labs.setting_lock("meetings.enabled"), Some(Lock::Labs));
        assert_eq!(labs.setting_lock("pause_browser_media"), Some(Lock::Labs));
        assert_eq!(labs.setting_lock("sound_effects"), None);
        // A choice that is only partly locked: `hex settings` refuses
        // `opencode` itself.
        let opencode = Policy::default().with(Lock::OpenCodeProcessing);
        assert_eq!(opencode.setting_lock("meetings.summaries"), None);
        let annotate = Policy::default().with(Lock::Annotate);
        assert_eq!(
            annotate.setting_lock("capture.enabled"),
            Some(Lock::Annotate)
        );
    }
}

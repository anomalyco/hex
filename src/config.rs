use std::sync::{OnceLock, RwLock};

use crate::app_settings::DictationProcessingSettings;
use crate::commands::{
    Action, Command, CommandConfig, ConfiguredCommand, ContextPredicate, Count, Digit, Direction,
};
use crate::dictation::{CAPTAINS_LOG_START_PHRASES, DICTATION_START_PHRASES};
use crate::dictation_processor::{Profile, Profiles};
use crate::keyboard::{Key, Modifiers};

pub const INPUT_DEVICES: &[&str] = &["Universal Audio Thunderbolt", "Studio Display Microphone"];
static DICTATION_PROFILES: OnceLock<RwLock<Profiles>> = OnceLock::new();

pub fn dictation_profiles() -> Profiles {
    DICTATION_PROFILES
        .get_or_init(|| {
            RwLock::new(build_dictation_profiles(
                &DictationProcessingSettings::default(),
            ))
        })
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

pub fn update_dictation_profiles(settings: &DictationProcessingSettings) {
    let profiles = build_dictation_profiles(settings);
    if let Some(runtime) = DICTATION_PROFILES.get() {
        *runtime.write().unwrap_or_else(|error| error.into_inner()) = profiles;
    } else {
        let _ = DICTATION_PROFILES.set(RwLock::new(profiles));
    }
}

fn build_dictation_profiles(settings: &DictationProcessingSettings) -> Profiles {
    let mut profiles = Profiles::new(mode_profile(&settings.default_mode));
    for mode in &settings.modes {
        let profile = mode_profile(mode);
        for application in &mode.applications {
            profiles = match &profile {
                Some(profile) => profiles.application(application.clone(), profile.clone()),
                None => profiles.application_raw(application.clone()),
            };
        }
        for host in &mode.browser_hosts {
            profiles = match &profile {
                Some(profile) => profiles.browser_host(host.clone(), profile.clone()),
                None => profiles.browser_host_raw(host.clone()),
            };
        }
    }
    profiles
}

fn mode_profile(mode: &crate::app_settings::DictationMode) -> Option<Profile> {
    let processing = &mode.post_processing;
    if !processing.enabled {
        return None;
    }
    let name = if mode.name.trim().is_empty() {
        "Untitled mode"
    } else {
        &mode.name
    };
    let mut profile = Profile::new(name, &processing.prompt);
    if let Some((provider, model)) = processing
        .model
        .as_deref()
        .and_then(|model| model.split_once('/'))
    {
        profile = profile.model(provider, model);
        if let Some(variant) = processing
            .variant
            .as_deref()
            .filter(|variant| !variant.is_empty())
        {
            profile = profile.variant(variant);
        }
    }
    Some(profile.deadline(std::time::Duration::from_secs(
        processing.deadline_seconds.max(1),
    )))
}

/// The compiled personal configuration. This stays ordinary Rust code so it
/// can grow functions and typed processors without becoming a new language.
pub fn voice_control() -> CommandConfig {
    voice_control_for(crate::DEVELOPER_FEATURES_ENABLED)
}

fn voice_control_for(meetings_enabled: bool) -> CommandConfig {
    let commands = CommandConfig::new()
        .wake_with(["voice control", "wake up", "start voice control"])
        .sleep_with(["go to sleep", "stop voice control"])
        .command(
            Command::new("app.open.brave", "Open application")
                .phrases(["open brave", "launch brave"])
                .action(|()| Action::OpenApplication("Brave Browser")),
        )
        .command(
            Command::new("app.open.slack", "Open application")
                .phrases(["open slack", "launch slack"])
                .action(|()| Action::OpenApplication("Slack")),
        )
        .command(
            Command::new("website.open.x", "Open website")
                .phrases([
                    "go to x",
                    "go to twitter",
                    "go to x dot com",
                    "open x",
                    "open twitter",
                    "open x dot com",
                ])
                .action(|()| Action::OpenUrl("https://x.com")),
        )
        .command(
            Command::new("website.open.training", "Open website")
                .phrases(["go to training", "open training"])
                .action(|()| Action::OpenUrl("https://hub.kitlangton.dev/training")),
        )
        .command(
            Command::new("website.open.meditation", "Open website")
                .phrases(["go to meditation", "open meditation"])
                .action(|()| Action::OpenUrl("https://hub.kitlangton.dev/meditation")),
        )
        .command(
            Command::new("dictation.start", "Start voice dictation")
                .phrases(DICTATION_START_PHRASES.iter().copied())
                .action(|()| Action::StartDictation),
        )
        .command(
            Command::new("captains-log.start", "Record a journal entry")
                .phrases(CAPTAINS_LOG_START_PHRASES.iter().copied())
                .action(|()| Action::StartCaptainsLog),
        );
    let commands = if meetings_enabled {
        commands
            .command(
                Command::new("meeting.start", "Start meeting recording")
                    .phrases([
                        "start meeting",
                        "start a meeting",
                        "record meeting",
                        "record this meeting",
                    ])
                    .action(|()| Action::StartMeeting),
            )
            .command(
                Command::new("meeting.stop", "Stop meeting recording")
                    .phrases(["stop meeting", "stop the meeting", "stop recording"])
                    .action(|()| Action::StopMeeting),
            )
    } else {
        commands
    };
    commands
        .command(
            Command::new("shortcut.command-number", "Use keyboard shortcut")
                .spoken(("command", Digit))
                .spoken(("key", "command", Digit))
                .spoken(("terminal", Digit))
                .spoken(("tab", Digit))
                .action(|digit| Action::Keystroke {
                    key: Key::Character(digit.as_char()),
                    modifiers: Modifiers::COMMAND,
                }),
        )
        .command(directional_command(
            "edit.move",
            "Move the cursor",
            "go",
            Modifiers::NONE,
        ))
        .command(directional_command(
            "edit.select",
            "Extend the selection",
            "select",
            Modifiers::SHIFT,
        ))
        .command(
            Command::new("x.home", "Navigate active tab")
                .spoken(("go", "home"))
                .spoken(("go", "to", "home"))
                .when(ContextPredicate::browser_host("x.com"))
                .action(|()| Action::NavigateBrowser("https://x.com/home")),
        )
        .command(shortcut_command(
            "shortcut.home",
            ["key home"],
            Key::Home,
            Modifiers::NONE,
        ))
        .command(shortcut_command(
            "shortcut.end",
            ["key end"],
            Key::End,
            Modifiers::NONE,
        ))
        .command(shortcut_command(
            "shortcut.command-up",
            ["key command up"],
            Key::Up,
            Modifiers::COMMAND,
        ))
        .command(shortcut_command(
            "shortcut.command-down",
            ["key command down"],
            Key::Down,
            Modifiers::COMMAND,
        ))
        .command(shortcut_command(
            "shortcut.command-left",
            ["key command left"],
            Key::Left,
            Modifiers::COMMAND,
        ))
        .command(shortcut_command(
            "shortcut.command-right",
            ["key command right"],
            Key::Right,
            Modifiers::COMMAND,
        ))
        .command(
            Command::new("x.notifications", "Navigate active tab")
                .phrases(["go to notifications", "open notifications"])
                .when(ContextPredicate::browser_host("x.com"))
                .action(|()| Action::NavigateBrowser("https://x.com/notifications")),
        )
        .command(
            Command::new("x.chat", "Navigate active tab")
                .phrases(["go to chat", "open chat", "go to messages"])
                .when(ContextPredicate::browser_host("x.com"))
                .action(|()| Action::NavigateBrowser("https://x.com/messages")),
        )
        .command(
            Command::new("slack.threads", "Use application shortcut")
                .phrases(["go to threads", "open threads"])
                .when(ContextPredicate::application("Slack"))
                .action(|()| Action::Keystroke {
                    key: Key::Character('t'),
                    modifiers: Modifiers::COMMAND.with(Modifiers::SHIFT),
                }),
        )
        .command(
            Command::new("slack.next-unread", "Use application shortcut")
                .phrases(["next unread", "go to next unread"])
                .when(ContextPredicate::application("Slack"))
                .action(|()| Action::Keystroke {
                    key: Key::Down,
                    modifiers: Modifiers::OPTION.with(Modifiers::SHIFT),
                }),
        )
        .command(
            Command::new("slack.previous-unread", "Use application shortcut")
                .phrases(["previous unread", "go to previous unread"])
                .when(ContextPredicate::application("Slack"))
                .action(|()| Action::Keystroke {
                    key: Key::Up,
                    modifiers: Modifiers::OPTION.with(Modifiers::SHIFT),
                }),
        )
        .command(
            Command::new("slack.channel.console", "Open application destination")
                .phrases(["go to console", "open console", "go to console channel"])
                .when(ContextPredicate::application("Slack"))
                .action(|()| Action::QuickSwitch("console")),
        )
}

fn directional_command(
    id: &'static str,
    description: &'static str,
    verb: &'static str,
    modifiers: Modifiers,
) -> ConfiguredCommand {
    Command::new(id, description)
        .spoken((verb, Direction, Count.optional()))
        .action(move |(direction, count)| Action::RepeatedKeystroke {
            key: direction.key(),
            modifiers,
            count: count.map_or(1, |count| count.get()),
        })
}

fn shortcut_command<const N: usize>(
    id: &'static str,
    phrases: [&'static str; N],
    key: Key,
    modifiers: Modifiers,
) -> ConfiguredCommand {
    Command::new(id, "Use keyboard shortcut")
        .phrases(phrases)
        .action(move |()| Action::Keystroke { key, modifiers })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_configuration_excludes_meeting_commands() {
        let catalog = voice_control_for(false).catalog();

        assert!(!catalog.is_empty());
        assert!(
            !catalog
                .iter()
                .any(|command| command.id.starts_with("meeting."))
        );
    }
    use crate::commands::{Decision, Mode};
    use crate::context::ContextSnapshot;

    #[test]
    fn meeting_recording_can_be_controlled_by_voice() {
        let commands = voice_control_for(true);
        let context = ContextSnapshot::default();

        assert!(matches!(
            commands.resolve(Mode::Listening, "Start a meeting.", &context),
            Decision::Execute {
                id: "meeting.start",
                ..
            }
        ));
        assert!(matches!(
            commands.resolve(Mode::Listening, "Stop meeting.", &context),
            Decision::Execute {
                id: "meeting.stop",
                ..
            }
        ));
    }

    #[test]
    fn command_digit_slot_covers_zero_through_nine() {
        let commands = voice_control();
        let context = ContextSnapshot::default();

        for (heard, expected) in [
            ("command zero", '0'),
            ("command one", '1'),
            ("terminal five", '5'),
            ("tab nine", '9'),
        ] {
            assert!(matches!(
                commands.resolve(Mode::Listening, heard, &context),
                Decision::Execute {
                    id: "shortcut.command-number",
                    action: Action::Keystroke {
                        key: Key::Character(actual),
                        ..
                    },
                } if actual == expected
            ));
        }

        assert!(matches!(
            commands.resolve(Mode::Listening, "command eleven", &context),
            Decision::Ignore
        ));
    }

    #[test]
    fn movement_commands_capture_direction_and_optional_count() {
        let commands = voice_control();
        let context = ContextSnapshot::default();

        assert!(matches!(
            commands.resolve(Mode::Listening, "go left", &context),
            Decision::Execute {
                id: "edit.move",
                action: Action::RepeatedKeystroke {
                    key: Key::Left,
                    count: 1,
                    ..
                },
            }
        ));
        assert!(matches!(
            commands.resolve(Mode::Listening, "select down five", &context),
            Decision::Execute {
                id: "edit.select",
                action: Action::RepeatedKeystroke {
                    key: Key::Down,
                    count: 5,
                    ..
                },
            }
        ));
    }

    #[test]
    fn typed_literal_command_respects_browser_context() {
        let commands = voice_control();
        let no_context = ContextSnapshot::default();
        let x = ContextSnapshot {
            application: Some("Brave Browser".into()),
            browser_url: Some("https://x.com/home".parse().unwrap()),
            window_title: None,
            selected_text: None,
            input_revision: None,
        };

        assert!(matches!(
            commands.resolve(Mode::Listening, "go home", &no_context),
            Decision::Ignore
        ));
        assert!(matches!(
            commands.resolve(Mode::Listening, "go home", &x),
            Decision::Execute { id: "x.home", .. }
        ));
    }
}

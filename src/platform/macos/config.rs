use std::sync::{OnceLock, RwLock};

use crate::app_settings::DictationProcessingSettings;
use crate::commands::{Action, Command, CommandConfig, ConfiguredCommand, Count, Digit, Direction};
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
            profiles = profiles.application(application.clone(), profile.clone());
        }
        for host in &mode.browser_hosts {
            profiles = profiles.browser_host(host.clone(), profile.clone());
        }
    }
    profiles
}

fn mode_profile(mode: &crate::app_settings::DictationMode) -> Profile {
    let processing = &mode.post_processing;
    let name = if mode.name.trim().is_empty() {
        "Untitled mode"
    } else {
        &mode.name
    };
    Profile::configured(
        name,
        crate::text_replacements::ReplacementSet::new(&mode.replacements),
        mode.transformations.clone(),
        processing,
    )
}

/// Native system commands and typed captures that cannot yet be expressed by
/// the TypeScript command grammar.
pub fn voice_control() -> CommandConfig {
    voice_control_for(crate::DEVELOPER_FEATURES_ENABLED)
}

fn voice_control_for(meetings_enabled: bool) -> CommandConfig {
    let commands = CommandConfig::new()
        .wake_with(["voice control", "wake up", "start voice control"])
        .sleep_with(["go to sleep", "stop voice control"]);
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
                    .protected()
                    .action(|()| Action::StartMeeting),
            )
            .command(
                Command::new("meeting.stop", "Stop meeting recording")
                    .phrases(["stop meeting", "stop the meeting", "stop recording"])
                    .protected()
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
    fn captains_log_is_not_a_command() {
        assert!(matches!(
            voice_control().resolve(
                Mode::Listening,
                "captain's log",
                &ContextSnapshot::default()
            ),
            Decision::Ignore
        ));
    }

    #[test]
    fn ordinary_literal_commands_are_not_compiled() {
        let catalog = voice_control().catalog();

        for id in [
            "app.open.brave",
            "app.open.slack",
            "website.open.training",
            "website.open.meditation",
            "shortcut.home",
            "x.notifications",
            "slack.next-unread",
            "slack.channel.console",
        ] {
            assert!(!catalog.iter().any(|command| command.id == id));
        }
    }

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
}

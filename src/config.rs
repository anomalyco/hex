use crate::commands::{CommandConfig, ContextualCommand, Target};
use crate::dictation::{CAPTAINS_LOG_START_PHRASES, DICTATION_START_PHRASES};
use crate::keyboard::{Key, Modifiers};

pub const INPUT_DEVICES: &[&str] = &["Universal Audio Thunderbolt", "Studio Display Microphone"];

/// The compiled personal configuration. This stays ordinary Rust code so it
/// can grow functions and typed processors without becoming a new language.
pub fn voice_control() -> CommandConfig {
    CommandConfig::new()
        .wake_with(["voice control", "wake up", "start voice control"])
        .sleep_with(["go to sleep", "stop voice control"])
        .target(Target::application(
            "app.open.brave",
            "Brave Browser",
            ["brave"],
        ))
        .target(Target::application("app.open.slack", "Slack", ["slack"]))
        .target(Target::website(
            "website.open.x",
            "https://x.com",
            ["x", "twitter", "x dot com"],
        ))
        .target(Target::website(
            "website.open.training",
            "https://hub.kitlangton.dev/training",
            ["training"],
        ))
        .target(Target::website(
            "website.open.meditation",
            "https://hub.kitlangton.dev/meditation",
            ["meditation"],
        ))
        .contextual(ContextualCommand::dictation_start(
            "dictation.start",
            DICTATION_START_PHRASES.iter().copied(),
        ))
        .contextual(ContextualCommand::captains_log_start(
            "captains-log.start",
            CAPTAINS_LOG_START_PHRASES.iter().copied(),
        ))
        .contextual(ContextualCommand::meeting_start(
            "meeting.start",
            [
                "start meeting",
                "start a meeting",
                "record meeting",
                "record this meeting",
            ],
        ))
        .contextual(ContextualCommand::meeting_stop(
            "meeting.stop",
            ["stop meeting", "stop the meeting", "stop recording"],
        ))
        .contextual(ContextualCommand::global_shortcut(
            "shortcut.command-one",
            Key::Character('1'),
            Modifiers::COMMAND,
            ["command one", "key command one", "terminal one", "tab one"],
        ))
        .contextual(ContextualCommand::global_shortcut(
            "shortcut.command-two",
            Key::Character('2'),
            Modifiers::COMMAND,
            ["command two", "key command two", "terminal two", "tab two"],
        ))
        .contextual(ContextualCommand::global_shortcut(
            "shortcut.command-three",
            Key::Character('3'),
            Modifiers::COMMAND,
            [
                "command three",
                "key command three",
                "terminal three",
                "tab three",
            ],
        ))
        .contextual(ContextualCommand::global_shortcut(
            "shortcut.command-four",
            Key::Character('4'),
            Modifiers::COMMAND,
            [
                "command four",
                "key command four",
                "terminal four",
                "tab four",
            ],
        ))
        .contextual(ContextualCommand::global_shortcut(
            "shortcut.home",
            Key::Home,
            Modifiers::NONE,
            ["key home"],
        ))
        .contextual(ContextualCommand::global_shortcut(
            "shortcut.end",
            Key::End,
            Modifiers::NONE,
            ["key end"],
        ))
        .contextual(ContextualCommand::global_shortcut(
            "shortcut.command-up",
            Key::Up,
            Modifiers::COMMAND,
            ["key command up"],
        ))
        .contextual(ContextualCommand::global_shortcut(
            "shortcut.command-down",
            Key::Down,
            Modifiers::COMMAND,
            ["key command down"],
        ))
        .contextual(ContextualCommand::global_shortcut(
            "shortcut.command-left",
            Key::Left,
            Modifiers::COMMAND,
            ["key command left"],
        ))
        .contextual(ContextualCommand::global_shortcut(
            "shortcut.command-right",
            Key::Right,
            Modifiers::COMMAND,
            ["key command right"],
        ))
        .contextual(ContextualCommand::browser_navigation(
            "x.notifications",
            "x.com",
            "https://x.com/notifications",
            ["go to notifications", "open notifications"],
        ))
        .contextual(ContextualCommand::browser_navigation(
            "x.chat",
            "x.com",
            "https://x.com/messages",
            ["go to chat", "open chat", "go to messages"],
        ))
        .contextual(ContextualCommand::browser_navigation(
            "x.home",
            "x.com",
            "https://x.com/home",
            ["go home", "go to home"],
        ))
        .contextual(ContextualCommand::application_shortcut(
            "slack.threads",
            "Slack",
            Key::Character('t'),
            Modifiers::COMMAND.with(Modifiers::SHIFT),
            ["go to threads", "open threads"],
        ))
        .contextual(ContextualCommand::application_shortcut(
            "slack.next-unread",
            "Slack",
            Key::Down,
            Modifiers::OPTION.with(Modifiers::SHIFT),
            ["next unread", "go to next unread"],
        ))
        .contextual(ContextualCommand::application_shortcut(
            "slack.previous-unread",
            "Slack",
            Key::Up,
            Modifiers::OPTION.with(Modifiers::SHIFT),
            ["previous unread", "go to previous unread"],
        ))
        .contextual(ContextualCommand::application_quick_switch(
            "slack.channel.console",
            "Slack",
            "console",
            ["go to console", "open console", "go to console channel"],
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{Decision, Mode};
    use crate::context::ContextSnapshot;

    #[test]
    fn meeting_recording_can_be_controlled_by_voice() {
        let commands = voice_control();
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
}

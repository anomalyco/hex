use std::process::{Command as ProcessCommand, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;

use color_eyre::eyre::{Result, WrapErr, eyre};

use crate::command_grammar::normalize;
#[allow(unused_imports)]
pub use crate::command_grammar::{
    CapturedCount, CapturedDigit, CapturedDirection, Command, CommandBuilder, ConfiguredCommand,
    Count, Digit, Direction, OptionalCount, PatternSpec, TypedPattern,
};
pub use crate::context::ContextSelector as ContextPredicate;
use crate::context::ContextSnapshot;
use crate::keyboard::{Key, Modifiers};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Sleeping,
    Listening,
}

#[derive(Clone, Copy, Debug)]
pub enum Action {
    StartDictation,
    StartCaptainsLog,
    StartMeeting,
    StopMeeting,
    OpenApplication(&'static str),
    OpenUrl(&'static str),
    NavigateBrowser(&'static str),
    Keystroke {
        key: Key,
        modifiers: Modifiers,
    },
    RepeatedKeystroke {
        key: Key,
        modifiers: Modifiers,
        count: u8,
    },
    QuickSwitch(&'static str),
}

pub struct CommandConfig {
    wake_phrases: Vec<&'static str>,
    sleep_phrases: Vec<&'static str>,
    commands: Vec<ConfiguredCommand>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandScope {
    Sleeping,
    Global,
    Application(String),
    Browser(String),
}

pub struct CommandInfo {
    pub scope: CommandScope,
    pub phrase: String,
    pub aliases: Vec<String>,
    pub description: &'static str,
    pub id: &'static str,
}

pub enum Decision {
    Ignore,
    Wake,
    Sleep,
    Execute { id: &'static str, action: Action },
}

struct ActionRequest {
    id: &'static str,
    action: Action,
    heard: String,
    context: String,
}

pub struct ActionOutcome {
    pub id: &'static str,
    pub heard: String,
    pub context: String,
    pub result: Result<(), String>,
}

pub struct ActionExecutor {
    requests: SyncSender<ActionRequest>,
    outcomes: Receiver<ActionOutcome>,
}

impl ActionExecutor {
    pub fn start() -> Self {
        let (requests, request_receiver) = mpsc::sync_channel::<ActionRequest>(1);
        let (outcome_sender, outcomes) = mpsc::channel();
        thread::spawn(move || {
            while let Ok(request) = request_receiver.recv() {
                let outcome = ActionOutcome {
                    id: request.id,
                    heard: request.heard,
                    context: request.context,
                    result: execute(request.action).map_err(|error| error.to_string()),
                };
                if outcome_sender.send(outcome).is_err() {
                    break;
                }
            }
        });
        Self { requests, outcomes }
    }

    pub fn submit(
        &self,
        id: &'static str,
        action: Action,
        heard: &str,
        context: String,
    ) -> Result<(), &'static str> {
        self.requests
            .try_send(ActionRequest {
                id,
                action,
                heard: heard.into(),
                context,
            })
            .map_err(|error| match error {
                TrySendError::Full(_) => "command execution queue is full",
                TrySendError::Disconnected(_) => "command executor is unavailable",
            })
    }

    pub fn try_recv(&self) -> Option<ActionOutcome> {
        self.outcomes.try_recv().ok()
    }
}

impl CommandConfig {
    pub fn new() -> Self {
        Self {
            wake_phrases: Vec::new(),
            sleep_phrases: Vec::new(),
            commands: Vec::new(),
        }
    }

    pub fn wake_with(mut self, phrases: impl IntoIterator<Item = &'static str>) -> Self {
        self.wake_phrases.extend(phrases);
        self
    }

    pub fn sleep_with(mut self, phrases: impl IntoIterator<Item = &'static str>) -> Self {
        self.sleep_phrases.extend(phrases);
        self
    }

    pub fn command(mut self, command: ConfiguredCommand) -> Self {
        assert!(
            !self
                .commands
                .iter()
                .any(|existing| existing.id() == command.id()),
            "duplicate command id: {}",
            command.id()
        );
        for existing in &self.commands {
            assert!(
                !existing.overlaps(&command),
                "command patterns overlap: {} and {}",
                existing.id(),
                command.id()
            );
        }
        self.commands.push(command);
        self
    }

    pub fn resolve(&self, mode: Mode, heard: &str, context: &ContextSnapshot) -> Decision {
        let heard = normalize(heard);
        if mode == Mode::Sleeping {
            return if matches_phrase(&heard, &self.wake_phrases) {
                Decision::Wake
            } else {
                Decision::Ignore
            };
        }
        if matches_phrase(&heard, &self.sleep_phrases) {
            return Decision::Sleep;
        }

        let words = heard.split_whitespace().collect::<Vec<_>>();
        if let Some((command, action)) = self.commands.iter().find_map(|command| {
            command
                .match_words(&words, context)
                .map(|action| (command, action))
        }) {
            return Decision::Execute {
                id: command.id(),
                action,
            };
        }

        Decision::Ignore
    }

    pub fn catalog(&self) -> Vec<CommandInfo> {
        let mut commands = Vec::new();
        if let Some((canonical, aliases)) = self.wake_phrases.split_first() {
            commands.push(CommandInfo {
                scope: CommandScope::Sleeping,
                phrase: (*canonical).into(),
                aliases: aliases.iter().map(|phrase| (*phrase).into()).collect(),
                description: "Wake voice control",
                id: "mode.wake",
            });
        }
        if let Some((canonical, aliases)) = self.sleep_phrases.split_first() {
            commands.push(CommandInfo {
                scope: CommandScope::Global,
                phrase: (*canonical).into(),
                aliases: aliases.iter().map(|phrase| (*phrase).into()).collect(),
                description: "Put voice control to sleep",
                id: "mode.sleep",
            });
        }
        commands.extend(self.commands.iter().map(ConfiguredCommand::catalog));
        commands
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn available_catalog(&self, mode: Mode, context: &ContextSnapshot) -> Vec<CommandInfo> {
        self.catalog()
            .into_iter()
            .filter(|command| match mode {
                Mode::Sleeping => command.scope == CommandScope::Sleeping,
                Mode::Listening => match command.scope {
                    CommandScope::Global => true,
                    CommandScope::Application(_) | CommandScope::Browser(_) => {
                        self.commands.iter().any(|configured| {
                            configured.id() == command.id && configured.matches_context(context)
                        })
                    }
                    CommandScope::Sleeping => false,
                },
            })
            .collect()
    }
}

pub fn execute(action: Action) -> Result<()> {
    let mut command = ProcessCommand::new("/usr/bin/open");
    match action {
        Action::StartDictation
        | Action::StartCaptainsLog
        | Action::StartMeeting
        | Action::StopMeeting => {
            return Err(eyre!("interactive actions require the HEX app"));
        }
        Action::OpenApplication(application) => {
            command.args(["-a", application]);
        }
        Action::OpenUrl(url) => {
            command.arg(url);
        }
        Action::NavigateBrowser(url) => {
            return navigate_brave(url);
        }
        Action::Keystroke { key, modifiers } => {
            return application_keystroke(key, modifiers);
        }
        Action::RepeatedKeystroke {
            key,
            modifiers,
            count,
        } => {
            return crate::keyboard::post_repeated_shortcut(key, modifiers, count);
        }
        Action::QuickSwitch(query) => {
            return application_quick_switch(query);
        }
    }
    checked_status(&mut command, "macOS open")
}

fn application_quick_switch(query: &str) -> Result<()> {
    let script = r#"
on run argv
tell application "System Events"
    keystroke "k" using command down
    delay 0.15
    keystroke (item 1 of argv)
    delay 0.2
    key code 36
end tell
end run
"#;
    checked_status(
        ProcessCommand::new("/usr/bin/osascript").args(["-e", script, "--", query]),
        "application quick switch",
    )
}

fn application_keystroke(key: Key, modifiers: Modifiers) -> Result<()> {
    crate::keyboard::post_shortcut(key, modifiers)
}

fn checked_status(command: &mut ProcessCommand, operation: &str) -> Result<()> {
    let status = command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .wrap_err_with(|| format!("could not invoke {operation}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(eyre!("{operation} exited with {status}"))
    }
}

fn navigate_brave(url: &str) -> Result<()> {
    let script = r#"
on run argv
    tell application "Brave Browser"
        set URL of active tab of front window to item 1 of argv
    end tell
end run
"#;
    checked_status(
        ProcessCommand::new("/usr/bin/osascript").args(["-e", script, "--", url]),
        "Brave navigation",
    )
}

fn matches_phrase(heard: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|phrase| normalize(phrase) == heard)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_context() -> ContextSnapshot {
        ContextSnapshot::default()
    }

    fn config() -> CommandConfig {
        CommandConfig::new()
            .wake_with(["voice control"])
            .sleep_with(["go to sleep"])
            .command(
                Command::new("app.open.slack", "Open application")
                    .phrases(["open slack", "launch slack"])
                    .action(|()| Action::OpenApplication("Slack")),
            )
    }

    fn digit_config() -> CommandConfig {
        CommandConfig::new().command(
            Command::new("shortcut.command-number", "Use keyboard shortcut")
                .spoken(("command", Digit))
                .spoken(("key", "command", Digit))
                .action(|digit| Action::Keystroke {
                    key: Key::Character(digit.as_char()),
                    modifiers: Modifiers::COMMAND,
                }),
        )
    }

    #[test]
    fn typed_digit_slot_accepts_words_and_digits_from_zero_through_nine() {
        for heard in ["command zero", "command 5", "key command nine"] {
            assert!(matches!(
                digit_config().resolve(Mode::Listening, heard, &no_context()),
                Decision::Execute {
                    id: "shortcut.command-number",
                    action: Action::Keystroke { .. },
                }
            ));
        }
    }

    #[test]
    fn typed_digit_slot_rejects_numbers_outside_a_single_digit() {
        for heard in ["command ten", "command 11", "command negative one"] {
            assert!(matches!(
                digit_config().resolve(Mode::Listening, heard, &no_context()),
                Decision::Ignore
            ));
        }
    }

    #[test]
    fn typed_command_catalog_is_derived_from_its_patterns() {
        let catalog = digit_config().catalog();

        assert_eq!(catalog[0].phrase, "command <digit>");
        assert_eq!(catalog[0].aliases, ["key command <digit>"]);
    }

    #[test]
    fn direction_and_optional_count_produce_typed_captures() {
        let commands = CommandConfig::new().command(
            Command::new("edit.move", "Move the cursor")
                .spoken(("GO", Direction, Count.optional()))
                .action(|(direction, count)| Action::RepeatedKeystroke {
                    key: direction.key(),
                    modifiers: Modifiers::NONE,
                    count: count.map_or(1, CapturedCount::get),
                }),
        );

        assert!(matches!(
            commands.resolve(Mode::Listening, "go left", &no_context()),
            Decision::Execute {
                action: Action::RepeatedKeystroke {
                    key: Key::Left,
                    count: 1,
                    ..
                },
                ..
            }
        ));
        assert!(matches!(
            commands.resolve(Mode::Listening, "go down five", &no_context()),
            Decision::Execute {
                action: Action::RepeatedKeystroke {
                    key: Key::Down,
                    count: 5,
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    #[should_panic(expected = "command patterns overlap: digit and count")]
    fn overlapping_typed_patterns_are_rejected_at_configuration_time() {
        let _ = CommandConfig::new()
            .command(
                Command::new("digit", "Digit")
                    .spoken(("choose", Digit))
                    .action(|_| Action::StartMeeting),
            )
            .command(
                Command::new("count", "Count")
                    .spoken(("choose", Count))
                    .action(|_| Action::StopMeeting),
            );
    }

    #[test]
    #[should_panic(expected = "command patterns overlap: digit and literal")]
    fn literal_that_a_slot_accepts_is_rejected_as_an_overlap() {
        let _ = CommandConfig::new()
            .command(
                Command::new("digit", "Digit")
                    .spoken(("choose", Digit))
                    .action(|_| Action::StartMeeting),
            )
            .command(
                Command::new("literal", "Literal")
                    .spoken(("choose", "1"))
                    .action(|()| Action::StopMeeting),
            );
    }

    #[test]
    fn browser_and_application_scopes_are_conservatively_overlapping() {
        assert!(
            ContextPredicate::browser_host("x.com")
                .overlaps(&ContextPredicate::application("Brave Browser"))
        );
        assert!(
            !ContextPredicate::browser_host("x.com")
                .overlaps(&ContextPredicate::browser_host("example.com"))
        );
    }

    #[test]
    fn sleeping_only_accepts_standalone_wake_phrase() {
        assert!(matches!(
            config().resolve(Mode::Sleeping, "Voice control.", &no_context()),
            Decision::Wake
        ));
        assert!(matches!(
            config().resolve(Mode::Sleeping, "Voice control, open Slack", &no_context()),
            Decision::Ignore
        ));
        assert!(matches!(
            config().resolve(Mode::Sleeping, "Open Slack", &no_context()),
            Decision::Ignore
        ));
    }

    #[test]
    fn listening_resolves_configured_target() {
        assert!(matches!(
            config().resolve(Mode::Listening, "Open Slack.", &no_context()),
            Decision::Execute {
                id: "app.open.slack",
                ..
            }
        ));
    }

    #[test]
    fn command_numbers_match_words_or_digits() {
        let config = CommandConfig::new().command(
            Command::new("shortcut.command-one", "Use keyboard shortcut")
                .phrases(["command one"])
                .action(|()| Action::Keystroke {
                    key: Key::Character('1'),
                    modifiers: Modifiers::COMMAND,
                }),
        );
        assert!(matches!(
            config.resolve(Mode::Listening, "Command 1.", &no_context()),
            Decision::Execute {
                id: "shortcut.command-one",
                ..
            }
        ));
    }

    #[test]
    fn sleep_phrase_must_be_the_whole_utterance() {
        assert!(matches!(
            config().resolve(Mode::Listening, "Go to sleep.", &no_context()),
            Decision::Sleep
        ));
        assert!(matches!(
            config().resolve(Mode::Listening, "I should go to sleep", &no_context()),
            Decision::Ignore
        ));
    }

    #[test]
    fn catalog_is_derived_from_resolvable_commands() {
        let config = config();
        let catalog = config.catalog();
        assert_eq!(catalog.len(), 3);
        assert!(catalog.iter().any(|command| command.phrase == "open slack"));
    }

    #[test]
    fn contextual_command_requires_matching_foreground_browser() {
        let config = CommandConfig::new().command(
            Command::new("x.chat", "Navigate active tab")
                .phrases(["go to chat"])
                .when(ContextPredicate::browser_host("x.com"))
                .action(|()| Action::NavigateBrowser("https://x.com/messages")),
        );
        assert!(matches!(
            config.resolve(Mode::Listening, "go to chat", &no_context()),
            Decision::Ignore
        ));
        let x = ContextSnapshot {
            application: Some("Brave Browser".into()),
            browser_url: Some(url::Url::parse("https://x.com/home").unwrap()),
            window_title: None,
            selected_text: None,
            input_revision: None,
        };
        assert!(matches!(
            config.resolve(Mode::Listening, "go to chat", &x),
            Decision::Execute { id: "x.chat", .. }
        ));
    }

    #[test]
    fn application_command_requires_matching_foreground_application() {
        let config = CommandConfig::new().command(
            Command::new("slack.channel.console", "Open application destination")
                .phrases(["go to console"])
                .when(ContextPredicate::application("Slack"))
                .action(|()| Action::QuickSwitch("console")),
        );
        assert!(matches!(
            config.resolve(Mode::Listening, "go to console", &no_context()),
            Decision::Ignore
        ));
        let slack = ContextSnapshot {
            application: Some("Slack".into()),
            browser_url: None,
            window_title: None,
            selected_text: None,
            input_revision: None,
        };
        assert!(matches!(
            config.resolve(Mode::Listening, "go to console", &slack),
            Decision::Execute {
                id: "slack.channel.console",
                ..
            }
        ));
    }

    #[test]
    fn available_catalog_only_includes_current_context() {
        let config = CommandConfig::new()
            .wake_with(["voice control"])
            .sleep_with(["go to sleep"])
            .command(
                Command::new("website.open.training", "Open website")
                    .phrases(["go to training", "open training"])
                    .action(|()| Action::OpenUrl("https://hub.kitlangton.dev/training")),
            )
            .command(
                Command::new("slack.threads", "Use application shortcut")
                    .phrases(["go to threads"])
                    .when(ContextPredicate::application("Slack"))
                    .action(|()| Action::Keystroke {
                        key: Key::Character('t'),
                        modifiers: Modifiers::COMMAND.with(Modifiers::SHIFT),
                    }),
            );

        let no_context = config.available_catalog(Mode::Listening, &no_context());
        assert!(no_context.iter().any(|command| {
            command.phrase == "go to training"
                && command.aliases.iter().any(|alias| alias == "open training")
        }));
        assert!(
            !no_context
                .iter()
                .any(|command| command.phrase == "go to threads")
        );

        let slack = ContextSnapshot {
            application: Some("Slack".into()),
            browser_url: None,
            window_title: None,
            selected_text: None,
            input_revision: None,
        };
        let slack_commands = config.available_catalog(Mode::Listening, &slack);
        assert!(
            slack_commands
                .iter()
                .any(|command| command.phrase == "go to threads")
        );

        let sleeping = config.available_catalog(Mode::Sleeping, &slack);
        assert_eq!(sleeping.len(), 1);
        assert_eq!(sleeping[0].phrase, "voice control");
    }
}

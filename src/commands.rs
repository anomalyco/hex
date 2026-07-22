use std::process::{Command as ProcessCommand, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;
use std::{error, fmt};

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

#[derive(Clone, Debug)]
pub enum Action {
    StartDictation,
    StartCaptainsLog,
    StartMeeting,
    StopMeeting,
    InvokeHandler {
        generation: u64,
        command_id: String,
    },
    OpenApplication(String),
    OpenUrl(String),
    #[allow(dead_code)]
    OpenPath(String),
    NavigateBrowser(String),
    #[allow(dead_code)]
    TypeText(String),
    Keystroke {
        key: Key,
        modifiers: Modifiers,
    },
    RepeatedKeystroke {
        key: Key,
        modifiers: Modifiers,
        count: u8,
    },
    QuickSwitch(String),
}

#[derive(Clone)]
pub struct CommandConfig {
    wake_phrases: Vec<String>,
    sleep_phrases: Vec<String>,
    commands: Vec<ConfiguredCommand>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandError {
    DuplicateId {
        id: String,
    },
    #[allow(dead_code)]
    MissingPhrase {
        id: String,
    },
    #[allow(dead_code)]
    OverlappingAliases {
        id: String,
    },
    OverlappingPatterns {
        first: String,
        second: String,
    },
    ReservedId {
        id: String,
    },
    ReservedPhrase {
        phrase: String,
    },
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId { id } => write!(formatter, "duplicate command id: {id}"),
            Self::MissingPhrase { id } => write!(formatter, "command must have a phrase: {id}"),
            Self::OverlappingAliases { id } => {
                write!(formatter, "command aliases overlap: {id}")
            }
            Self::OverlappingPatterns { first, second } => {
                write!(formatter, "command patterns overlap: {first} and {second}")
            }
            Self::ReservedId { id } => write!(formatter, "reserved command id: {id}"),
            Self::ReservedPhrase { phrase } => {
                write!(formatter, "reserved command phrase: {phrase}")
            }
        }
    }
}

impl error::Error for CommandError {}

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
    pub description: String,
    pub id: String,
    pub group: Option<String>,
}

pub enum Decision<'a> {
    Ignore,
    Wake,
    Sleep,
    Execute { id: &'a str, action: Action },
}

struct ActionRequest {
    id: String,
    action: Action,
    heard: String,
    context: String,
}

pub struct ActionOutcome {
    pub id: String,
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
        id: impl Into<String>,
        action: Action,
        heard: &str,
        context: String,
    ) -> Result<(), &'static str> {
        self.requests
            .try_send(ActionRequest {
                id: id.into(),
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

    pub fn wake_with(mut self, phrases: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.wake_phrases
            .extend(phrases.into_iter().map(Into::into));
        self
    }

    pub fn sleep_with(mut self, phrases: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.sleep_phrases
            .extend(phrases.into_iter().map(Into::into));
        self
    }

    pub fn command(mut self, command: ConfiguredCommand) -> Self {
        self.try_command(command)
            .unwrap_or_else(|error| panic!("{error}"));
        self
    }

    pub fn try_command(&mut self, command: ConfiguredCommand) -> Result<(), CommandError> {
        if self
            .commands
            .iter()
            .any(|existing| existing.id() == command.id())
        {
            return Err(CommandError::DuplicateId {
                id: command.id().into(),
            });
        }
        for existing in &self.commands {
            if existing.conflicts_with(&command) {
                return Err(CommandError::OverlappingPatterns {
                    first: existing.id().into(),
                    second: command.id().into(),
                });
            }
        }
        self.commands.push(command);
        Ok(())
    }

    pub fn validate_personal_identity(
        &self,
        id: &str,
        phrases: &[String],
    ) -> Result<(), CommandError> {
        if matches!(id, "mode.wake" | "mode.sleep") {
            return Err(CommandError::ReservedId { id: id.into() });
        }
        for phrase in phrases {
            let phrase = normalize(phrase);
            if matches_phrase(&phrase, &self.wake_phrases)
                || matches_phrase(&phrase, &self.sleep_phrases)
            {
                return Err(CommandError::ReservedPhrase { phrase });
            }
        }
        Ok(())
    }

    pub fn resolve<'a>(
        &'a self,
        mode: Mode,
        heard: &str,
        context: &ContextSnapshot,
    ) -> Decision<'a> {
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
        if let Some((command, action)) = self
            .commands
            .iter()
            .filter_map(|command| {
                command
                    .match_words(&words, context)
                    .map(|action| (command, action))
            })
            .max_by_key(|(command, _)| command.specificity())
        {
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
                phrase: canonical.clone(),
                aliases: aliases.to_vec(),
                description: "Wake voice control".into(),
                id: "mode.wake".into(),
                group: None,
            });
        }
        if let Some((canonical, aliases)) = self.sleep_phrases.split_first() {
            commands.push(CommandInfo {
                scope: CommandScope::Global,
                phrase: canonical.clone(),
                aliases: aliases.to_vec(),
                description: "Put voice control to sleep".into(),
                id: "mode.sleep".into(),
                group: None,
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
        | Action::StopMeeting
        | Action::InvokeHandler { .. } => {
            return Err(eyre!("interactive actions require the HEX app"));
        }
        Action::OpenApplication(application) => {
            command.arg("-a").arg(application);
        }
        Action::OpenUrl(url) => {
            command.arg(url);
        }
        Action::OpenPath(path) => {
            command.arg(path);
        }
        Action::NavigateBrowser(url) => {
            return navigate_brave(&url);
        }
        Action::TypeText(text) => {
            return crate::keyboard::type_text(&text);
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
            return application_quick_switch(&query);
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

fn matches_phrase(heard: &str, phrases: &[String]) -> bool {
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
                    .action(|()| Action::OpenApplication("Slack".into())),
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
    fn context_coexistence_distinguishes_disjoint_values() {
        assert!(
            ContextPredicate::browser_host("x.com")
                .can_coexist_with(&ContextPredicate::application("Brave Browser"))
        );
        assert!(
            !ContextPredicate::browser_host("x.com")
                .can_coexist_with(&ContextPredicate::browser_host("example.com"))
        );
    }

    #[test]
    fn more_specific_commands_specialize_global_commands() {
        let config = CommandConfig::new()
            .command(
                Command::new("global", "Global")
                    .phrases(["open it"])
                    .action(|()| Action::OpenUrl("https://example.com".into())),
            )
            .command(
                Command::new("application", "Application")
                    .phrases(["open it"])
                    .when(ContextPredicate::application("Brave Browser"))
                    .action(|()| Action::TypeText("application".into())),
            )
            .command(
                Command::new("browser", "Browser")
                    .phrases(["open it"])
                    .when(ContextPredicate::browser_host("x.com"))
                    .action(|()| Action::TypeText("browser".into())),
            );
        let browser = ContextSnapshot {
            application: Some("Brave Browser".into()),
            browser_url: Some(url::Url::parse("https://x.com/home").unwrap()),
            ..ContextSnapshot::default()
        };

        assert!(matches!(
            config.resolve(Mode::Listening, "open it", &browser),
            Decision::Execute { id: "browser", .. }
        ));
        let application = ContextSnapshot {
            application: Some("Brave Browser".into()),
            ..ContextSnapshot::default()
        };
        assert!(matches!(
            config.resolve(Mode::Listening, "open it", &application),
            Decision::Execute {
                id: "application",
                ..
            }
        ));
        assert!(matches!(
            config.resolve(Mode::Listening, "open it", &no_context()),
            Decision::Execute { id: "global", .. }
        ));
    }

    #[test]
    fn runtime_insertion_rejects_equal_specificity_ambiguity() {
        let mut config = CommandConfig::new().command(
            ConfiguredCommand::literal(
                "first".to_string(),
                "First".to_string(),
                vec!["run it".to_string()],
                ContextPredicate::application("Slack"),
                Action::TypeText("first".into()),
            )
            .unwrap(),
        );
        let error = config
            .try_command(
                ConfiguredCommand::literal(
                    "second".to_string(),
                    "Second".to_string(),
                    vec!["run it".to_string()],
                    ContextPredicate::application("Slack"),
                    Action::TypeText("second".into()),
                )
                .unwrap(),
            )
            .unwrap_err();

        assert_eq!(
            error,
            CommandError::OverlappingPatterns {
                first: "first".into(),
                second: "second".into(),
            }
        );
    }

    #[test]
    fn runtime_literal_constructor_rejects_empty_phrases() {
        let result = ConfiguredCommand::literal(
            "empty",
            "Empty",
            ["  ...  "],
            ContextPredicate::Always,
            Action::TypeText("ignored".into()),
        );

        assert!(matches!(result, Err(CommandError::MissingPhrase { id }) if id == "empty"));
    }

    #[test]
    fn protected_commands_cannot_be_specialized() {
        let mut config = CommandConfig::new().command(
            Command::new("protected", "Protected")
                .phrases(["start dictation"])
                .protected()
                .action(|()| Action::StartDictation),
        );

        let error = config
            .try_command(
                ConfiguredCommand::literal(
                    "personal",
                    "Personal",
                    ["start dictation"],
                    ContextPredicate::application("Slack"),
                    Action::TypeText("override".into()),
                )
                .unwrap(),
            )
            .unwrap_err();

        assert!(matches!(error, CommandError::OverlappingPatterns { .. }));
    }

    #[test]
    fn dynamic_literal_commands_and_configs_are_cloneable() {
        let command = ConfiguredCommand::literal(
            String::from("personal.open"),
            String::from("Open a personal path"),
            vec![String::from("open project")],
            ContextPredicate::Always,
            Action::OpenPath(String::from("/tmp/project")),
        )
        .unwrap();
        let config = CommandConfig::new().command(command).clone();

        assert!(matches!(
            config.resolve(Mode::Listening, "open project", &no_context()),
            Decision::Execute {
                id: "personal.open",
                action: Action::OpenPath(path),
            } if path == "/tmp/project"
        ));
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
                .action(|()| Action::NavigateBrowser("https://x.com/messages".into())),
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
                .action(|()| Action::QuickSwitch("console".into())),
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
                    .action(|()| Action::OpenUrl("https://hub.kitlangton.dev/training".into())),
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

use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;

use color_eyre::eyre::{Result, WrapErr, eyre};

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
    OpenApplication(&'static str),
    OpenUrl(&'static str),
    NavigateBrowser(&'static str),
    Keystroke { key: Key, modifiers: Modifiers },
    QuickSwitch(&'static str),
}

#[derive(Clone, Debug)]
pub struct Target {
    id: &'static str,
    spoken: Vec<&'static str>,
    action: Action,
}

impl Target {
    pub fn application(
        id: &'static str,
        application: &'static str,
        spoken: impl IntoIterator<Item = &'static str>,
    ) -> Self {
        Self {
            id,
            spoken: spoken.into_iter().collect(),
            action: Action::OpenApplication(application),
        }
    }

    pub fn website(
        id: &'static str,
        url: &'static str,
        spoken: impl IntoIterator<Item = &'static str>,
    ) -> Self {
        Self {
            id,
            spoken: spoken.into_iter().collect(),
            action: Action::OpenUrl(url),
        }
    }

    fn verbs(&self) -> &'static [&'static str] {
        match self.action {
            Action::StartDictation | Action::StartCaptainsLog => &[],
            Action::OpenApplication(_) => &["open", "launch"],
            Action::OpenUrl(_) => &["go to", "open"],
            Action::NavigateBrowser(_) => &[],
            Action::Keystroke { .. } => &[],
            Action::QuickSwitch(_) => &[],
        }
    }

    fn description(&self) -> &'static str {
        match self.action {
            Action::StartDictation => "Start voice dictation",
            Action::StartCaptainsLog => "Record a journal entry",
            Action::OpenApplication(_) => "Open application",
            Action::OpenUrl(_) => "Open website",
            Action::NavigateBrowser(_) => "Navigate active tab",
            Action::Keystroke { .. } => "Use application shortcut",
            Action::QuickSwitch(_) => "Open application destination",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ContextPredicate {
    Always,
    BrowserHost(&'static str),
    Application(&'static str),
}

impl ContextPredicate {
    fn matches(self, context: &ContextSnapshot) -> bool {
        match self {
            Self::Always => true,
            Self::BrowserHost(host) => context.browser_host_is(host),
            Self::Application(application) => context.application_is(application),
        }
    }

    fn scope(self) -> CommandScope {
        match self {
            Self::Always => CommandScope::Global,
            Self::BrowserHost(host) => CommandScope::Browser(host),
            Self::Application(application) => CommandScope::Application(application),
        }
    }
}

pub struct ContextualCommand {
    id: &'static str,
    phrases: Vec<&'static str>,
    description: &'static str,
    context: ContextPredicate,
    action: Action,
}

impl ContextualCommand {
    pub fn dictation_start(
        id: &'static str,
        phrases: impl IntoIterator<Item = &'static str>,
    ) -> Self {
        Self {
            id,
            phrases: phrases.into_iter().collect(),
            description: "Start voice dictation",
            context: ContextPredicate::Always,
            action: Action::StartDictation,
        }
    }

    pub fn captains_log_start(
        id: &'static str,
        phrases: impl IntoIterator<Item = &'static str>,
    ) -> Self {
        Self {
            id,
            phrases: phrases.into_iter().collect(),
            description: "Record a journal entry",
            context: ContextPredicate::Always,
            action: Action::StartCaptainsLog,
        }
    }

    pub fn global_shortcut(
        id: &'static str,
        key: Key,
        modifiers: Modifiers,
        phrases: impl IntoIterator<Item = &'static str>,
    ) -> Self {
        Self {
            id,
            phrases: phrases.into_iter().collect(),
            description: "Use keyboard shortcut",
            context: ContextPredicate::Always,
            action: Action::Keystroke { key, modifiers },
        }
    }

    pub fn browser_navigation(
        id: &'static str,
        host: &'static str,
        url: &'static str,
        phrases: impl IntoIterator<Item = &'static str>,
    ) -> Self {
        Self {
            id,
            phrases: phrases.into_iter().collect(),
            description: "Navigate active tab",
            context: ContextPredicate::BrowserHost(host),
            action: Action::NavigateBrowser(url),
        }
    }

    pub fn application_shortcut(
        id: &'static str,
        application: &'static str,
        key: Key,
        modifiers: Modifiers,
        phrases: impl IntoIterator<Item = &'static str>,
    ) -> Self {
        Self {
            id,
            phrases: phrases.into_iter().collect(),
            description: "Use application shortcut",
            context: ContextPredicate::Application(application),
            action: Action::Keystroke { key, modifiers },
        }
    }

    pub fn application_quick_switch(
        id: &'static str,
        application: &'static str,
        query: &'static str,
        phrases: impl IntoIterator<Item = &'static str>,
    ) -> Self {
        Self {
            id,
            phrases: phrases.into_iter().collect(),
            description: "Open application destination",
            context: ContextPredicate::Application(application),
            action: Action::QuickSwitch(query),
        }
    }
}

pub struct CommandConfig {
    wake_phrases: Vec<&'static str>,
    sleep_phrases: Vec<&'static str>,
    targets: Vec<Target>,
    contextual: Vec<ContextualCommand>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandScope {
    Sleeping,
    Global,
    Application(&'static str),
    Browser(&'static str),
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
            targets: Vec::new(),
            contextual: Vec::new(),
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

    pub fn target(mut self, target: Target) -> Self {
        self.targets.push(target);
        self
    }

    pub fn contextual(mut self, command: ContextualCommand) -> Self {
        self.contextual.push(command);
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

        if let Some(command) = self.contextual.iter().find(|command| {
            command.context.matches(context)
                && command
                    .phrases
                    .iter()
                    .any(|phrase| normalize(phrase) == heard)
        }) {
            return Decision::Execute {
                id: command.id,
                action: command.action,
            };
        }

        self.targets
            .iter()
            .find(|candidate| {
                candidate.verbs().iter().any(|verb| {
                    heard
                        .strip_prefix(&format!("{verb} "))
                        .is_some_and(|heard| {
                            candidate
                                .spoken
                                .iter()
                                .any(|spoken| normalize(spoken) == heard)
                        })
                })
            })
            .map_or(Decision::Ignore, |target| Decision::Execute {
                id: target.id,
                action: target.action,
            })
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
        commands.extend(self.targets.iter().map(|target| {
            let canonical = target.spoken[0];
            let verb = target.verbs()[0];
            let mut aliases: Vec<_> = target
                .spoken
                .iter()
                .skip(1)
                .map(|spoken| format!("{verb} {spoken}"))
                .collect();
            aliases.extend(target.verbs().iter().skip(1).flat_map(|verb| {
                target
                    .spoken
                    .iter()
                    .map(move |spoken| format!("{verb} {spoken}"))
            }));
            CommandInfo {
                scope: CommandScope::Global,
                phrase: format!("{verb} {canonical}"),
                aliases,
                description: target.description(),
                id: target.id,
            }
        }));
        commands.extend(self.contextual.iter().map(|command| {
            let (canonical, aliases) = command.phrases.split_first().expect("command has a phrase");
            CommandInfo {
                scope: command.context.scope(),
                phrase: (*canonical).into(),
                aliases: aliases.iter().map(|phrase| (*phrase).into()).collect(),
                description: command.description,
                id: command.id,
            }
        }));
        commands
    }

    pub fn available_catalog(&self, mode: Mode, context: &ContextSnapshot) -> Vec<CommandInfo> {
        self.catalog()
            .into_iter()
            .filter(|command| match mode {
                Mode::Sleeping => command.scope == CommandScope::Sleeping,
                Mode::Listening => match command.scope {
                    CommandScope::Global => true,
                    CommandScope::Application(_) | CommandScope::Browser(_) => {
                        self.contextual.iter().any(|contextual| {
                            contextual.id == command.id && contextual.context.matches(context)
                        })
                    }
                    CommandScope::Sleeping => false,
                },
            })
            .collect()
    }
}

pub fn execute(action: Action) -> Result<()> {
    let mut command = Command::new("/usr/bin/open");
    match action {
        Action::StartDictation | Action::StartCaptainsLog => {
            return Err(eyre!("dictation actions require the recognition loop"));
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
        Action::QuickSwitch(query) => {
            return application_quick_switch(query);
        }
    }
    let status = command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .wrap_err("could not invoke macOS open")?;
    if status.success() {
        Ok(())
    } else {
        Err(eyre!("macOS open exited with {status}"))
    }
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
        Command::new("/usr/bin/osascript").args(["-e", script, "--", query]),
        "application quick switch",
    )
}

fn application_keystroke(key: Key, modifiers: Modifiers) -> Result<()> {
    crate::keyboard::post_shortcut(key, modifiers)
}

fn checked_status(command: &mut Command, operation: &str) -> Result<()> {
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
    let status = Command::new("/usr/bin/osascript")
        .args(["-e", script, "--", url])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .wrap_err("could not navigate Brave")?;
    if status.success() {
        Ok(())
    } else {
        Err(eyre!("Brave navigation exited with {status}"))
    }
}

fn matches_phrase(heard: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|phrase| normalize(phrase) == heard)
}

fn normalize(text: &str) -> String {
    text.trim()
        .trim_matches(|character: char| character.is_ascii_punctuation())
        .split_whitespace()
        .map(|word| match word.to_ascii_lowercase().as_str() {
            "zero" => "0".to_string(),
            "one" => "1".to_string(),
            "two" => "2".to_string(),
            "three" => "3".to_string(),
            "four" => "4".to_string(),
            "five" => "5".to_string(),
            "six" => "6".to_string(),
            "seven" => "7".to_string(),
            "eight" => "8".to_string(),
            "nine" => "9".to_string(),
            _ => word.to_ascii_lowercase(),
        })
        .collect::<Vec<_>>()
        .join(" ")
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
            .target(Target::application("app.open.slack", "Slack", ["slack"]))
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
        let config = CommandConfig::new().contextual(ContextualCommand::global_shortcut(
            "shortcut.command-one",
            Key::Character('1'),
            Modifiers::COMMAND,
            ["command one"],
        ));
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
        let config = CommandConfig::new().contextual(ContextualCommand::browser_navigation(
            "x.chat",
            "x.com",
            "https://x.com/messages",
            ["go to chat"],
        ));
        assert!(matches!(
            config.resolve(Mode::Listening, "go to chat", &no_context()),
            Decision::Ignore
        ));
        let x = ContextSnapshot {
            application: Some("Brave Browser".into()),
            browser_url: Some(url::Url::parse("https://x.com/home").unwrap()),
            window_title: None,
        };
        assert!(matches!(
            config.resolve(Mode::Listening, "go to chat", &x),
            Decision::Execute { id: "x.chat", .. }
        ));
    }

    #[test]
    fn application_command_requires_matching_foreground_application() {
        let config = CommandConfig::new().contextual(ContextualCommand::application_quick_switch(
            "slack.channel.console",
            "Slack",
            "console",
            ["go to console"],
        ));
        assert!(matches!(
            config.resolve(Mode::Listening, "go to console", &no_context()),
            Decision::Ignore
        ));
        let slack = ContextSnapshot {
            application: Some("Slack".into()),
            browser_url: None,
            window_title: None,
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
            .target(Target::website(
                "website.open.training",
                "https://hub.kitlangton.dev/training",
                ["training"],
            ))
            .contextual(ContextualCommand::application_shortcut(
                "slack.threads",
                "Slack",
                Key::Character('t'),
                Modifiers::COMMAND.with(Modifiers::SHIFT),
                ["go to threads"],
            ));

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

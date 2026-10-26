//! The macOS host of `hex` control commands ([`crate::control`]). The main
//! thread answers what it owns (the window, the menu bar's meeting state,
//! settings saves) and hands commands that wait on the listener or the
//! window to a short-lived thread, so a slow answer never stalls the run loop.

use std::sync::mpsc::{self, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use crate::app_settings::AppSettings;
use crate::control::{
    self, CaptureReport, CaptureSnapshot, CaptureState, ControlCall, ControlCommand, ControlError,
    ControlReply, ControlResponse, FeatureReport, ListenerReport, MeetingReport, ModelReport, Pane,
    PermissionReport, StatusReport, WindowReport, settings,
};
use crate::desktop_host::{DesktopAction, DesktopUpdateStatus};
use crate::meeting::MeetingRequest;
use crate::meeting_watcher::{ControllerEvent, MeetingMenuState};
use crate::onboarding::PermissionState;
use crate::recognition::{CommandModelStatus, RecognitionControl};
use crate::rune_ui::{Destination, Ui};

use super::{Pump, Request, RuntimeHandle, show, window_visible};

/// How long a command waits for the listener's answer.
const LISTENER_TIMEOUT: Duration = Duration::from_secs(3);
const WINDOW_TIMEOUT: Duration = Duration::from_secs(1);
/// How long `meetings record|stop` and `model use` wait to confirm, inside
/// the local API's own control deadline.
const CONFIRM_WAIT: Duration = Duration::from_secs(8);
/// Lets the local API write the reply before Hex quits.
const QUIT_GRACE: Duration = Duration::from_millis(250);

/// A `meetings record|stop` waiting for the controller to confirm.
pub(super) struct MeetingWaiter {
    stop: bool,
    reply: SyncSender<ControlResponse>,
    deadline: Instant,
}

impl Pump {
    pub(super) fn control(&mut self, call: ControlCall) {
        let ControlCall { request, reply } = call;
        let answer = match control::check_version(&request) {
            Ok(()) => self.answer(request.command, &reply),
            Err(error) => Some(error.into()),
        };
        if let Some(answer) = answer {
            let _ = reply.send(answer.into());
        }
    }

    /// Answers now, or returns `None` once a thread or waiter owns `reply`.
    fn answer(
        &mut self,
        command: ControlCommand,
        reply: &SyncSender<ControlResponse>,
    ) -> Option<ControlReply> {
        let runtime = self.runtime.clone();
        match command {
            ControlCommand::Status => {
                let meeting = meeting_report(&self.meeting_state);
                let window = self.window.clone();
                background(reply, move || status(&runtime, &window, meeting));
                None
            }
            ControlCommand::Quit { force } | ControlCommand::Restart { force } => {
                let restart = matches!(command, ControlCommand::Restart { .. });
                let meeting = meeting_report(&self.meeting_state);
                let reply = reply.clone();
                spawn(move || {
                    let capture = capture_snapshot(&runtime);
                    let answer = control::check_quit(&meeting, capture, force).and_then(|()| {
                        if restart && !crate::onboarding::spawn_relaunch() {
                            return Err(ControlError::new(
                                "unsupported",
                                "only an installed Hex.app can restart itself; quit it and open \
                                 it again",
                            ));
                        }
                        Ok(())
                    });
                    let quitting = answer.is_ok();
                    let _ = reply.send(
                        match answer {
                            Ok(()) if restart => ControlReply::done("Restarting Hex"),
                            Ok(()) => ControlReply::done("Quitting Hex"),
                            Err(error) => error.into(),
                        }
                        .into(),
                    );
                    if quitting {
                        thread::sleep(QUIT_GRACE);
                        let _ = runtime.send(Request::CancelModel);
                        super::quit();
                    }
                });
                None
            }
            ControlCommand::Dictation { action } | ControlCommand::VoiceAction { action } => {
                let voice_action = matches!(command, ControlCommand::VoiceAction { .. });
                background(reply, move || {
                    listener_ready(&runtime)?;
                    ask(&runtime, |reply| RecognitionControl::Capture {
                        voice_action,
                        action,
                        reply,
                    })
                });
                None
            }
            #[cfg(hex_capture)]
            ControlCommand::ScrawlCapture { action } => {
                background(reply, move || {
                    listener_ready(&runtime)?;
                    ask(&runtime, |reply| RecognitionControl::ScrawlCapture {
                        action,
                        reply,
                    })
                });
                None
            }
            #[cfg(not(hex_capture))]
            ControlCommand::ScrawlCapture { .. } => Some(ControlReply::error(
                "unsupported",
                "this build has no capture engine: it is a build option (`--features capture`), off in public builds",
            )),
            ControlCommand::PasteLast => {
                background(reply, move || {
                    listener_ready(&runtime)?;
                    ask(&runtime, |reply| RecognitionControl::PasteLast {
                        reply: Some(reply),
                    })
                });
                None
            }
            ControlCommand::MeetingRecord { title } => self.meeting(false, title, reply),
            ControlCommand::MeetingStop => self.meeting(true, None, reply),
            ControlCommand::MeetingCopyLast => Some(
                crate::meeting::latest_transcript_markdown()
                    .and_then(|markdown| {
                        arboard::Clipboard::new()?.set_text(markdown)?;
                        Ok(())
                    })
                    .map(|()| ControlReply::done("Copied the last meeting transcript"))
                    .unwrap_or_else(|error| {
                        ControlReply::error("failed", format!("couldn't copy it: {error:#}"))
                    }),
            ),
            ControlCommand::SettingsGet { key } => Some(reply_with(load().and_then(|settings| {
                Ok(ControlReply::Settings {
                    settings: match key {
                        Some(key) => settings::get(&settings, &key)?,
                        None => settings::list(&settings),
                    },
                })
            }))),
            ControlCommand::SettingsSet {
                key,
                value,
                confirm,
            } => Some(reply_with(self.set_setting(&key, &value, confirm))),
            ControlCommand::CommandsRetry => Some(reply_with(retry_commands(&runtime))),
            ControlCommand::ModelList => {
                Some(reply_with(load().map(|settings| ControlReply::Models {
                    models: control::model_infos(&settings.transcription),
                })))
            }
            ControlCommand::ModelUse { model, language } => {
                let selection = match load().and_then(|settings| {
                    control::model_selection(&settings, &model, language.as_deref())
                }) {
                    Ok(selection) => selection,
                    Err(error) => return Some(error.into()),
                };
                let definition = crate::transcription_models::definition(selection.model);
                if !crate::transcription_models::is_installed(definition, &selection.language) {
                    return Some(ControlReply::error(
                        "not-installed",
                        format!(
                            "{} isn't installed; `hex model install {}` downloads it ({})",
                            definition.name,
                            selection.model.as_str(),
                            definition.size_label()
                        ),
                    ));
                }
                background(reply, move || use_model(&runtime, selection));
                None
            }
            ControlCommand::ModelInstall { model, language } => {
                let selection = match load().and_then(|settings| {
                    control::model_selection(&settings, &model, language.as_deref())
                }) {
                    Ok(selection) => selection,
                    Err(error) => return Some(error.into()),
                };
                let definition = crate::transcription_models::definition(selection.model);
                if crate::transcription_models::is_installed(definition, &selection.language) {
                    background(reply, move || use_model(&runtime, selection));
                    return None;
                }
                Some(reply_with(
                    runtime
                        .send(Request::ChooseModel {
                            model: selection.model,
                            language: selection.language,
                        })
                        .map_err(|error| ControlError::new("unavailable", error.to_string()))
                        .map(|()| {
                            ControlReply::done(format!(
                                "Downloading {} ({}); Hex switches to it once it's ready. \
                                 `hex status` shows progress, `hex model cancel` stops it",
                                definition.name,
                                definition.size_label()
                            ))
                        }),
                ))
            }
            ControlCommand::ModelCancel => {
                let preparing = runtime
                    .published()
                    .and_then(|published| published.preparation.model);
                Some(match preparing {
                    None => ControlReply::unchanged("No model is downloading or loading"),
                    Some(model) => {
                        let _ = runtime.send(Request::CancelModel);
                        ControlReply::done(format!(
                            "Stopped preparing {}",
                            crate::transcription_models::definition(model).name
                        ))
                    }
                })
            }
            ControlCommand::WindowShow { pane } => Some(reply_with(self.show_window(pane))),
            ControlCommand::WindowHide => Some(if window_visible() {
                self.window.close();
                ControlReply::done("Closed the window")
            } else {
                ControlReply::unchanged("The window isn't open")
            }),
            ControlCommand::HistoryClear => Some(reply_with(clear_history(&runtime))),
            ControlCommand::LaunchAtLogin { enabled } => {
                background(reply, move || launch_at_login(enabled));
                None
            }
            ControlCommand::CheckForUpdates => Some(match crate::macos_rune::update_status() {
                DesktopUpdateStatus::Managed => {
                    ControlReply::error("unsupported", "updates are managed by your organization")
                }
                DesktopUpdateStatus::Unavailable | DesktopUpdateStatus::SourceBuild => {
                    ControlReply::error("unavailable", "this Hex build doesn't update itself")
                }
                _ => match crate::macos_rune::update_action(&DesktopAction::CheckForUpdates) {
                    Ok(()) => ControlReply::done("Checking for updates; Hex shows what it finds"),
                    Err(error) => ControlReply::error("unavailable", format!("{error:#}")),
                },
            }),
            ControlCommand::VocabularyList => Some(reply_with(
                control::check_vocabulary()
                    .and_then(|()| load())
                    .map(|settings| ControlReply::Vocabulary {
                        entries: settings.vocabulary,
                    }),
            )),
            ControlCommand::VocabularyAdd { .. } | ControlCommand::VocabularyRemove { .. } => {
                Some(reply_with(self.edit_vocabulary(&command)))
            }
        }
    }

    /// Saves a vocabulary edit as the window would, then lets the runtime and
    /// an open window adopt it.
    fn edit_vocabulary(&mut self, command: &ControlCommand) -> Result<ControlReply, ControlError> {
        control::check_vocabulary()?;
        let mut settings = load()?;
        let reply = control::edit_vocabulary(&mut settings.vocabulary, command)?;
        if matches!(reply, ControlReply::Done { changed: true, .. }) {
            settings.save().map_err(|error| {
                ControlError::new("internal", format!("couldn't save: {error:#}"))
            })?;
            let _ = self.runtime.send(Request::ReloadSettings);
        }
        Ok(reply)
    }

    fn meeting(
        &mut self,
        stop: bool,
        title: Option<String>,
        reply: &SyncSender<ControlResponse>,
    ) -> Option<ControlReply> {
        if !crate::app_settings::meetings_enabled() {
            return Some(ControlReply::error(
                "feature-off",
                "Meetings are off; turn them on with `hex settings set meetings.enabled on`",
            ));
        }
        let request = match (&self.meeting_state, stop) {
            (MeetingMenuState::Recording { title, .. }, false) => {
                return Some(ControlReply::unchanged(format!(
                    "Already recording “{title}”"
                )));
            }
            (MeetingMenuState::Starting, false) => {
                return Some(ControlReply::unchanged(
                    "A meeting recording is already starting",
                ));
            }
            (MeetingMenuState::Transcribing, false) => {
                return Some(ControlReply::error(
                    "busy",
                    "the last meeting is still transcribing; try again when it finishes",
                ));
            }
            (MeetingMenuState::Idle, false) => MeetingRequest::Start {
                title: title.filter(|title| !title.trim().is_empty()),
            },
            (MeetingMenuState::Idle, true) => {
                return Some(ControlReply::unchanged("No meeting is recording"));
            }
            (MeetingMenuState::Transcribing, true) => {
                return Some(ControlReply::unchanged(
                    "The meeting is already transcribing",
                ));
            }
            (MeetingMenuState::Starting | MeetingMenuState::Recording { .. }, true) => {
                MeetingRequest::Stop
            }
        };
        if let Err(error) = self.runtime.meeting(request) {
            return Some(ControlReply::error("unavailable", error));
        }
        self.meeting_waiters.push(MeetingWaiter {
            stop,
            reply: reply.clone(),
            deadline: Instant::now() + CONFIRM_WAIT,
        });
        None
    }

    /// Confirms waiting `meetings record|stop` commands from the
    /// controller's events, or once they wait too long.
    pub(super) fn settle_meeting_waiters(&mut self, event: Option<&ControllerEvent>) {
        let now = Instant::now();
        self.meeting_waiters.retain(|waiter| {
            let answer = match (event, waiter.stop) {
                (Some(ControllerEvent::RecordingStarted { title, .. }), false) => {
                    ControlReply::done(format!(
                        "Recording “{title}”; `hex meetings stop` stops and transcribes it"
                    ))
                }
                (Some(ControllerEvent::Transcribing | ControllerEvent::Finished(_)), true) => {
                    ControlReply::done("Stopped recording; transcribing the meeting")
                }
                (Some(ControllerEvent::Failed { title, error }), _) => {
                    ControlReply::error("failed", format!("{title}: {error}"))
                }
                _ if now >= waiter.deadline => ControlReply::done(if waiter.stop {
                    "Asked the meeting to stop; `hex status` shows when it's transcribing"
                } else {
                    "Hex is starting the recording; `hex status` shows when it's recording"
                }),
                _ => return true,
            };
            let _ = waiter.reply.send(answer.into());
            false
        });
    }

    /// Saves one setting as the window would, then lets the runtime, the
    /// listener, and an open window adopt it.
    fn set_setting(
        &mut self,
        key: &str,
        value: &str,
        confirm: bool,
    ) -> Result<ControlReply, ControlError> {
        let before = load()?;
        let rules = settings::ShortcutRules {
            key_named: |name| crate::rune_ui::hotkey_key_named(name).map_err(str::to_owned),
            rule: |slot, binding| {
                crate::rune_ui::shortcut_rule(slot, binding).map_err(str::to_owned)
            },
        };
        let change = settings::apply(&before, key, value, confirm, &rules)?;
        if !change.changed(&before) {
            return Ok(ControlReply::unchanged(format!("{key} is already {value}")));
        }
        change
            .settings
            .save()
            .map_err(|error| ControlError::new("internal", format!("couldn't save: {error:#}")))?;
        let _ = self.runtime.send(Request::ReloadSettings);
        for effect in change.effects {
            match effect {
                settings::Effect::HistoryRetention(retention) => {
                    if let Some(history) = self.runtime.history()
                        && let Err(error) = history.set_retention(retention)
                    {
                        tracing::warn!(%error, "could not apply history retention");
                    }
                }
                // An open window keeps the Dock icon; closing it applies this.
                settings::Effect::DockIcon if !window_visible() => {
                    crate::app_settings::set_dock_icon_visible(
                        crate::app_settings::dock_icon_visible(
                            change.settings.show_dock_icon,
                            crate::status_item::installed(),
                        ),
                    );
                }
                settings::Effect::DockIcon => {}
            }
        }
        Ok(ControlReply::done(change.message))
    }

    fn show_window(&mut self, pane: Option<Pane>) -> Result<ControlReply, ControlError> {
        let destination = match pane {
            None | Some(Pane::Settings) => Destination::Settings,
            Some(Pane::Models) => Destination::Models,
            Some(Pane::Modes) => Destination::Modes,
            Some(Pane::VoiceAction) => Destination::VoiceAction,
            Some(Pane::History) => Destination::History,
            Some(Pane::Commands) => Destination::Commands,
            // While Meetings is off the pane shows its opt-in.
            Some(Pane::Meetings) => Destination::Meetings(None),
        };
        show(&self.window, destination);
        Ok(ControlReply::done("Opened the window"))
    }
}

fn reply_with(result: Result<ControlReply, ControlError>) -> ControlReply {
    result.unwrap_or_else(ControlReply::from)
}

fn spawn(work: impl FnOnce() + Send + 'static) {
    if let Err(error) = thread::Builder::new()
        .name("hex-control".into())
        .spawn(work)
    {
        tracing::error!(%error, "could not start a control thread");
    }
}

/// Answers `reply` from a thread; a dropped reply reads as `control-unavailable`.
fn background(
    reply: &SyncSender<ControlResponse>,
    work: impl FnOnce() -> Result<ControlReply, ControlError> + Send + 'static,
) {
    let reply = reply.clone();
    spawn(move || {
        let _ = reply.send(reply_with(work()).into());
    });
}

fn load() -> Result<AppSettings, ControlError> {
    AppSettings::load().map_err(|error| {
        ControlError::new("internal", format!("couldn't read settings: {error:#}"))
    })
}

fn listener_ready(runtime: &RuntimeHandle) -> Result<(), ControlError> {
    if runtime
        .published()
        .is_some_and(|published| published.listener.running)
    {
        return Ok(());
    }
    Err(ControlError::new(
        "not-ready",
        "Hex is waiting for setup (permissions or the dictation model); run `hex status`",
    ))
}

/// Sends the listener a control and waits for its answer.
fn ask<T: Send + 'static>(
    runtime: &RuntimeHandle,
    build: impl FnOnce(SyncSender<T>) -> RecognitionControl,
) -> Result<T, ControlError> {
    let (sender, receiver) = mpsc::sync_channel(1);
    runtime
        .recognition(build(sender))
        .map_err(|error| ControlError::new("busy", error))?;
    receiver.recv_timeout(LISTENER_TIMEOUT).map_err(|_| {
        ControlError::new(
            "unavailable",
            "the listener didn't answer in time; check `hex status`",
        )
    })
}

/// What records now; a listener still waiting for setup records nothing.
fn capture_snapshot(runtime: &RuntimeHandle) -> Result<CaptureSnapshot, ControlError> {
    if listener_ready(runtime).is_err() {
        return Ok(CaptureSnapshot::default());
    }
    ask(runtime, |reply| RecognitionControl::CaptureStatus { reply })
}

fn retry_commands(runtime: &RuntimeHandle) -> Result<ControlReply, ControlError> {
    if !crate::app_settings::commands_enabled() {
        return Err(ControlError::new(
            "feature-off",
            "voice commands are off; turn them on with `hex commands enable`",
        ));
    }
    if crate::recognition::command_model_status() != CommandModelStatus::Failed {
        return Ok(ControlReply::unchanged("The command model hasn't failed"));
    }
    runtime
        .recognition(RecognitionControl::RetryCommands)
        .map_err(|error| ControlError::new("busy", error))?;
    Ok(ControlReply::done("Loading the command model again"))
}

fn clear_history(runtime: &RuntimeHandle) -> Result<ControlReply, ControlError> {
    let history = runtime
        .history()
        .ok_or_else(|| ControlError::new("unavailable", "history couldn't be opened"))?;
    let count = history.search("").len();
    if count == 0 {
        return Ok(ControlReply::unchanged("History is already empty"));
    }
    history
        .clear()
        .map_err(|error| ControlError::new("failed", format!("couldn't clear history: {error}")))?;
    Ok(ControlReply::done(format!(
        "Cleared {count} dictation{}",
        if count == 1 { "" } else { "s" }
    )))
}

/// The signed app's `SMAppService` registration; an open Settings window
/// picks the change up on its next status poll.
fn launch_at_login(enabled: Option<bool>) -> Result<ControlReply, ControlError> {
    use crate::login_item::LoginItemStatus;

    let describe = |status| match status {
        LoginItemStatus::Enabled => "on",
        LoginItemStatus::Disabled => "off",
        LoginItemStatus::RequiresApproval => {
            "waiting for approval in System Settings › General › Login Items"
        }
    };
    let before = crate::login_item::set_now(None)
        .map_err(|error| ControlError::new("failed", format!("{error:#}")))?;
    let Some(enabled) = enabled else {
        return Ok(ControlReply::unchanged(format!(
            "Launch at login is {}",
            describe(before)
        )));
    };
    if (before != LoginItemStatus::Disabled) == enabled {
        return Ok(ControlReply::unchanged(format!(
            "Launch at login is already {}",
            describe(before)
        )));
    }
    let bundled = std::env::current_exe().is_ok_and(|executable| {
        executable
            .ancestors()
            .nth(3)
            .is_some_and(|path| path.extension().is_some_and(|extension| extension == "app"))
    });
    if !bundled {
        return Err(ControlError::new(
            "unsupported",
            "only an installed Hex.app can launch at login",
        ));
    }
    let after = crate::login_item::set_now(Some(enabled)).map_err(|error| {
        ControlError::new(
            "failed",
            format!("couldn't change launch at login: {error:#}"),
        )
    })?;
    Ok(ControlReply::done(format!(
        "Launch at login is {}",
        describe(after)
    )))
}

/// What `hex model use` asks the runtime: the installed-only path, which
/// verifies the model on disk and never downloads or repairs it.
fn use_request(selection: crate::transcription_models::TranscriptionSelection) -> Request {
    Request::UseInstalledSelection(selection)
}

/// Switches to an installed model and waits until it's loaded.
fn use_model(
    runtime: &RuntimeHandle,
    selection: crate::transcription_models::TranscriptionSelection,
) -> Result<ControlReply, ControlError> {
    let name = crate::transcription_models::definition(selection.model).name;
    let matches = |settings: &AppSettings| {
        settings.transcription.model == selection.model
            && settings.transcription.language == selection.language
    };
    if load().is_ok_and(|settings| matches(&settings)) {
        return Ok(ControlReply::unchanged(format!(
            "{name} ({}) is already in use",
            selection.language
        )));
    }
    runtime
        .send(use_request(selection.clone()))
        .map_err(|error| ControlError::new("unavailable", error.to_string()))?;
    let started = Instant::now();
    let mut seen_preparing = false;
    while started.elapsed() < CONFIRM_WAIT {
        thread::sleep(Duration::from_millis(100));
        let Some(published) = runtime.published() else {
            continue;
        };
        let preparing = published.preparation.model.is_some();
        seen_preparing |= preparing;
        if preparing {
            continue;
        }
        if matches(&published.settings) {
            return Ok(ControlReply::done(format!(
                "Switched to {name} ({})",
                selection.language
            )));
        }
        if let Some(error) = &published.preparation.error
            && (seen_preparing || started.elapsed() > Duration::from_secs(1))
        {
            return Err(ControlError::new(
                "failed",
                format!("couldn't switch to {name}: {error}"),
            ));
        }
    }
    Ok(ControlReply::done(format!(
        "Loading {name}; `hex status` shows when it's ready"
    )))
}

fn meeting_report(state: &MeetingMenuState) -> MeetingReport {
    match state {
        MeetingMenuState::Idle => MeetingReport {
            state: "idle".into(),
            ..Default::default()
        },
        MeetingMenuState::Starting => MeetingReport {
            state: "starting".into(),
            ..Default::default()
        },
        MeetingMenuState::Recording { started, title, .. } => MeetingReport {
            state: "recording".into(),
            title: Some(title.clone()),
            elapsed_seconds: Some(started.elapsed().as_secs()),
        },
        MeetingMenuState::Transcribing => MeetingReport {
            state: "transcribing".into(),
            ..Default::default()
        },
    }
}

/// The pane the window shows, asked on the UI thread.
fn current_pane(window: &rune::AppHandle<Ui>) -> Option<String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    window.update(move |ui, _| {
        let _ = sender.send(crate::rune_ui::current_pane(ui));
    });
    receiver.recv_timeout(WINDOW_TIMEOUT).ok().map(String::from)
}

fn permission(state: PermissionState) -> String {
    match state {
        PermissionState::Ready => "ready",
        PermissionState::NeedsRequest => "needs-request",
        PermissionState::NeedsSettings => "needs-settings",
        PermissionState::NeedsRestart => "needs-restart",
    }
    .into()
}

fn status(
    runtime: &RuntimeHandle,
    window: &rune::AppHandle<Ui>,
    meeting: MeetingReport,
) -> Result<ControlReply, ControlError> {
    let published = runtime
        .published()
        .ok_or_else(|| ControlError::new("not-ready", "Hex is still starting; try again"))?;
    let capture = match capture_snapshot(runtime) {
        Ok(snapshot) => CaptureReport::from(snapshot),
        Err(_) => CaptureReport {
            state: CaptureState::Unknown,
            ..Default::default()
        },
    };
    let open = window.is_visible();
    let settings = &published.settings;
    let selection = &settings.transcription;
    let definition = crate::transcription_models::definition(selection.model);
    let setup = crate::onboarding::status(selection);
    Ok(ControlReply::Status(Box::new(StatusReport {
        app_version: env!("CARGO_PKG_VERSION").into(),
        pid: std::process::id(),
        memory_footprint_bytes: control::memory_footprint(),
        listener: ListenerReport {
            running: published.listener.running,
            status: published.listener.status.clone(),
        },
        capture,
        meeting,
        model: ModelReport {
            id: selection.model.as_str().into(),
            name: definition.name.into(),
            language: selection.language.clone(),
            installed: crate::transcription_models::is_installed(definition, &selection.language),
            preparing: published
                .preparation
                .model
                .map(|model| model.as_str().into()),
            downloaded_bytes: published.preparation.downloaded_bytes,
            error: published.preparation.error.clone(),
        },
        features: FeatureReport {
            commands: settings.commands_enabled,
            command_model: match crate::recognition::command_model_status() {
                CommandModelStatus::Disabled => "disabled",
                CommandModelStatus::Loading => "loading",
                CommandModelStatus::Ready => "ready",
                CommandModelStatus::Failed => "failed",
            }
            .into(),
            voice_action: settings.voice_action.enabled,
            meetings: crate::app_settings::meetings_enabled(),
            release_microphone_while_idle: settings.release_microphone_while_idle,
            history_retention: serde_json::to_value(settings.history_retention)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default(),
        },
        permissions: PermissionReport {
            ready: setup.ready(),
            microphone: permission(setup.microphone),
            input_monitoring: permission(setup.input_monitoring),
            accessibility: permission(setup.accessibility),
            transcription_model: setup.transcription_model,
        },
        window: WindowReport {
            open,
            pane: open.then(|| current_pane(window)).flatten(),
        },
        update: match published.update_status {
            DesktopUpdateStatus::Checking | DesktopUpdateStatus::Downloading => "checking",
            DesktopUpdateStatus::Available
            | DesktopUpdateStatus::ReadyToRestart
            | DesktopUpdateStatus::PackageUpdate { .. } => "available",
            DesktopUpdateStatus::Unavailable
            | DesktopUpdateStatus::Managed
            | DesktopUpdateStatus::Failed => "unavailable",
            DesktopUpdateStatus::Current
            | DesktopUpdateStatus::SourceBuild
            | DesktopUpdateStatus::PackageManager => "current",
        }
        .into(),
        experiments: crate::experiments::active_names(),
    })))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    use crate::transcription_models::{TranscriptionModelId, TranscriptionSelection};
    use crate::transcription_preparation::TranscriptionPreparation;

    #[test]
    fn model_use_prepares_the_exact_selection_installed_only() {
        let (sender, prepared) = mpsc::channel();
        let preparation = TranscriptionPreparation::with_prepare(Arc::new(
            move |selection: &TranscriptionSelection,
                  installed_only: bool,
                  _: &AtomicBool,
                  _: &AtomicU64| {
                let _ = sender.send((selection.clone(), installed_only));
                Ok(())
            },
        ));
        let mut runtime = super::super::Runtime {
            settings: Default::default(),
            preparation,
            recognition_start: None,
        };
        let selection = TranscriptionSelection {
            model: TranscriptionModelId::WhisperLargeV3Turbo,
            language: "de".into(),
            recognition_hints: "Kit Langton".into(),
        };
        runtime.handle(super::use_request(selection.clone()));
        // installed_only: verify what's on disk, never download or repair.
        assert_eq!(
            prepared.recv_timeout(Duration::from_secs(3)).unwrap(),
            (selection, true)
        );
    }
}

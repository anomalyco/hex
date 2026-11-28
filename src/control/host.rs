//! The Linux and Windows host of `hex` control commands. Each OS supplies a
//! [`Host`], its thin adapter to the running process (the listener's
//! queue, the window, model preparation, the updater); [`answer`] carries
//! out every command the same way on both, with the rules the window and
//! the macOS host apply. A command the OS can't carry out answers
//! `unsupported` here, at runtime, instead of being left out of the CLI.

use std::time::{Duration, Instant};

use super::{
    CaptureAction, CaptureReport, CaptureSnapshot, CaptureState, ControlCommand, ControlError,
    ControlReply, ControlRequest, ControlResponse, FeatureReport, ListenerReport, MeetingReport,
    ModelReport, Pane, PermissionReport, PrintReply, StatusReport, WindowReport, settings,
};
use crate::app_settings::AppSettings;
use crate::desktop::host::{
    DesktopListenerSnapshot, DesktopTranscriptionSnapshot, DesktopUpdateStatus,
};
use crate::transcription::models::TranscriptionSelection;

/// How long `model use` waits to confirm, inside the transport's deadline.
const CONFIRM_WAIT: Duration = Duration::from_secs(8);

/// What the running Hex published last.
pub(crate) struct HostState {
    pub(crate) settings: AppSettings,
    pub(crate) listener: DesktopListenerSnapshot,
    pub(crate) transcription: DesktopTranscriptionSnapshot,
    pub(crate) update: DesktopUpdateStatus,
    pub(crate) window_open: bool,
}

/// One OS's adapter. Every method returns promptly; [`answer`] does the
/// waiting on the transport's worker thread.
pub(crate) trait Host {
    fn state(&self) -> Result<HostState, ControlError>;
    /// Queries the listener owner; a missing answer is never an idle capture.
    fn capture(&self) -> Result<CaptureSnapshot, ControlError>;
    /// Hands a dictation command to the listener's own queue, as its
    /// shortcut would; `print` rides along with a printing stop, and `raw`
    /// with a start that skips processing.
    fn dictation(
        &self,
        action: CaptureAction,
        print: Option<PrintReply>,
        raw: bool,
    ) -> Result<(), ControlError>;
    fn paste_last(&self) -> Result<(), ControlError>;
    /// Runs the focused field's text through its Output steps and replaces
    /// it, where this OS has Labs › Transform typed text.
    fn typed_text(&self) -> Result<(), ControlError> {
        Err(ControlError::new(
            "unsupported",
            "Transform typed text is Linux only",
        ))
    }
    /// Saves `settings` as the window would and lets the runtime, the
    /// listener, and an open window adopt them.
    fn save(&self, base: &AppSettings, settings: AppSettings) -> Result<(), ControlError>;
    fn history(&self) -> Option<crate::history::History>;
    fn show_window(&self, pane: Option<Pane>) -> Result<(), ControlError>;
    /// Closes the window; false when it wasn't open.
    fn hide_window(&self) -> bool;
    fn onboarding_reset(&self) -> ControlReply;
    /// Prepares `selection` (downloading it when missing) and switches to
    /// it once it is ready.
    fn choose_model(&self, selection: TranscriptionSelection) -> Result<(), ControlError>;
    /// Labs › Model switch shortcut from `hex model switch`: swaps the
    /// dictation model with the saved other one, through the same
    /// installed-only preparation as a picker choice.
    fn model_switch(&self) -> Result<(), ControlError>;
    fn cancel_model(&self);
    fn check_for_updates(&self) -> Result<(), ControlError>;
    /// Launch at login, after setting it when `enabled` is given; `None`
    /// when this OS has no login item.
    fn launch_at_login(&self, enabled: Option<bool>) -> Option<Result<bool, String>>;
    /// Quits Hex (and, for `restart`, reopens it) once this reply is out.
    fn quit(&self, restart: bool) -> Result<(), ControlError>;
    fn shortcut_rules(&self) -> settings::ShortcutRules;
    /// Annotate's commands, where this OS has a capture engine.
    fn annotate(&self, _command: ControlCommand) -> ControlReply {
        unsupported("Annotate isn't available on this OS yet")
    }
}

pub(crate) fn unsupported(message: impl Into<String>) -> ControlReply {
    ControlReply::error("unsupported", message)
}

/// Answers the local API's `POST /control` calls until it stops, each on
/// a thread of its own: a printing stop waits up to a minute.
pub(crate) fn serve(
    calls: std::sync::mpsc::Receiver<super::ControlCall>,
    host: std::sync::Arc<dyn Host + Send + Sync>,
) {
    let spawned = std::thread::Builder::new()
        .name("hex-control".into())
        .spawn(move || {
            for super::ControlCall { request, reply } in calls {
                let host = host.clone();
                let answered = std::thread::Builder::new()
                    .name("hex-control-call".into())
                    .spawn(move || {
                        let _ = reply.send(answer(host.as_ref(), request));
                    });
                if let Err(error) = answered {
                    tracing::warn!(%error, "could not answer a control command");
                }
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "`hex` control commands will not reach this Hex");
    }
}

/// Answers one request on the transport's thread.
pub(crate) fn answer(host: &dyn Host, request: ControlRequest) -> ControlResponse {
    match super::check_version(&request) {
        Ok(()) => command(host, request.command),
        Err(error) => error.into(),
    }
    .into()
}

/// `hex model switch`: the same installed-only preparation as a picker
/// choice, refused while Hex records. A failed preparation keeps the running
/// model and the saved selection.
fn switch_model(host: &dyn Host) -> Result<ControlReply, ControlError> {
    use crate::transcription::other_model::{self, SwitchError};
    let state = host.state()?;
    let capture = host.capture()?;
    if capture.recording.is_some() || capture.pending_jobs > 0 {
        return Err(ControlError::new(
            "busy",
            "Hex is dictating; try again once it finishes",
        ));
    }
    let plan = match other_model::planned(&state.settings).map_err(|error| match error {
        SwitchError::NotInstalled(message) => ControlError::new("not-installed", message),
        other => ControlError::new("unavailable", other.message()),
    })? {
        Some(plan) => plan,
        None => {
            return Ok(ControlReply::unchanged(format!(
                "Already using {} · {}",
                crate::transcription::models::definition(state.settings.transcription.model).name,
                crate::transcription::models::language_name(&state.settings.transcription.language)
            )));
        }
    };
    other_model::installed(&plan).map_err(|error| match error {
        SwitchError::NotInstalled(message) => ControlError::new("not-installed", message),
        other => ControlError::new("unavailable", other.message()),
    })?;
    host.model_switch()?;
    Ok(ControlReply::done(format!(
        "Switching to {} · {}; Hex keeps the current model if it fails",
        crate::transcription::models::definition(plan.to.model).name,
        crate::transcription::models::language_name(&plan.to.language)
    )))
}

fn command(host: &dyn Host, command: ControlCommand) -> ControlReply {
    let reply =
        |result: Result<ControlReply, ControlError>| result.unwrap_or_else(ControlReply::from);
    match command {
        ControlCommand::Status => reply(status(host)),
        ControlCommand::Quit { force } | ControlCommand::Restart { force } => {
            let restart = matches!(command, ControlCommand::Restart { .. });
            reply(
                super::check_quit(&meeting_report(), host.capture(), force)
                    .and_then(|()| host.quit(restart))
                    .map(|()| {
                        ControlReply::done(if restart {
                            "Restarting Hex"
                        } else {
                            "Quitting Hex"
                        })
                    }),
            )
        }
        ControlCommand::Dictation {
            action: CaptureAction::Start,
            print: true,
            ..
        } => ControlReply::error(
            "unsupported",
            "`hex dictation start --print` is macOS only; `hex dictation stop --print` \
             still prints here",
        ),
        ControlCommand::Dictation {
            action,
            print: true,
            raw,
        } => reply(super::check_flags(action, true, raw).and_then(|action| {
            let (print, printed) = PrintReply::channel();
            host.dictation(action, Some(print), false)?;
            Ok(super::await_printed(&printed, super::PRINT_WAIT))
        })),
        ControlCommand::Dictation { action, raw, .. } => {
            reply(super::check_flags(action, false, raw).and_then(|action| {
                host.dictation(action, None, raw)
                    .map(|()| ControlReply::done(dictation_message(action)))
            }))
        }
        ControlCommand::PasteLast => reply(
            host.paste_last()
                .map(|()| ControlReply::done("Pasting the last dictation")),
        ),
        ControlCommand::TypedText => reply(
            host.typed_text()
                .map(|()| ControlReply::done("Transforming the focused text")),
        ),
        ControlCommand::ModelSwitch => reply(switch_model(host)),
        ControlCommand::MeetingRecord { title } => meeting(false, title),
        ControlCommand::MeetingStop => meeting(true, None),
        ControlCommand::MeetingCopyLast => match crate::meeting::runtime::copy_last_transcript() {
            Ok(()) => ControlReply::done("Copied the last meeting transcript"),
            Err(error) => ControlReply::error("failed", format!("couldn't copy it: {error}")),
        },
        ControlCommand::MeetingDelete { id } => super::meeting_deleted(crate::meeting::delete(&id)),
        ControlCommand::MeetingRename { id, title } => {
            super::meeting_renamed(crate::meeting::rename_finished(&id, &title))
        }
        ControlCommand::SettingsGet { key } => reply(load().and_then(|settings| {
            Ok(ControlReply::Settings {
                settings: match key {
                    Some(key) => settings::get(&settings, &key)?,
                    None => settings::list(&settings),
                },
            })
        })),
        ControlCommand::SettingsSet { key, value } => reply(set_setting(host, &key, &value)),
        ControlCommand::SettingsApply { values } => reply(apply_settings(host, &values)),
        ControlCommand::ModelList => reply(load().map(|settings| ControlReply::Models {
            models: super::model_infos(&settings.transcription),
        })),
        ControlCommand::ModelUse { model, language } => {
            reply(use_model(host, &model, language.as_deref(), false))
        }
        ControlCommand::ModelInstall { model, language } => {
            reply(use_model(host, &model, language.as_deref(), true))
        }
        ControlCommand::ModelCancel => {
            reply(
                host.state()
                    .map(|state| match state.transcription.preparing {
                        None => ControlReply::unchanged("No model is downloading or loading"),
                        Some(model) => {
                            host.cancel_model();
                            ControlReply::done(format!(
                                "Stopped preparing {}",
                                crate::transcription::models::definition(model).name
                            ))
                        }
                    }),
            )
        }
        ControlCommand::WindowShow { pane } => reply(
            host.show_window(pane)
                .map(|()| ControlReply::done("Opened the window")),
        ),
        ControlCommand::WindowHide => {
            if host.hide_window() {
                ControlReply::done("Closed the window")
            } else {
                ControlReply::unchanged("The window isn't open")
            }
        }
        ControlCommand::WindowStory { .. } => {
            unsupported("playground story links are macOS-only; open the playground in the window")
        }
        ControlCommand::OnboardingReset => host.onboarding_reset(),
        ControlCommand::HistoryClear => reply(clear_history(host)),
        ControlCommand::CheckForUpdates => reply(host.state().and_then(|state| {
            match state.update {
                DesktopUpdateStatus::Managed => Err(ControlError::new(
                    "unsupported",
                    "updates are managed by your organization",
                )),
                DesktopUpdateStatus::Unavailable
                | DesktopUpdateStatus::SourceBuild
                | DesktopUpdateStatus::PackageManager => Err(ControlError::new(
                    "unavailable",
                    "this Hex build doesn't update itself",
                )),
                _ => host
                    .check_for_updates()
                    .map(|()| ControlReply::done("Checking for updates; Hex shows what it finds")),
            }
        })),
        ControlCommand::LaunchAtLogin { enabled } => launch_at_login(host, enabled),
        ControlCommand::VocabularyList => reply(load().map(|settings| ControlReply::Vocabulary {
            entries: settings.vocabulary,
        })),
        ControlCommand::VocabularyAdd { .. } | ControlCommand::VocabularyRemove { .. } => {
            reply(load().and_then(|mut settings| {
                let base = settings.clone();
                let reply = super::edit_vocabulary(&mut settings.vocabulary, &command)?;
                if matches!(reply, ControlReply::Done { changed: true, .. }) {
                    host.save(&base, settings)?;
                }
                Ok(reply)
            }))
        }
        ControlCommand::Screenshot { .. } => reply(Err(ControlError::new(
            "unsupported",
            "screenshots are macOS only",
        ))),
        command @ (ControlCommand::ScrawlCapture { .. }
        | ControlCommand::ScrawlCaptureList { .. }
        | ControlCommand::ScrawlCaptureShow { .. }
        | ControlCommand::ScrawlCaptureSend { .. }
        | ControlCommand::ScrawlCaptureUndo
        | ControlCommand::ScrawlCaptureRoute { .. }) => host.annotate(command),
    }
}

fn dictation_message(action: CaptureAction) -> &'static str {
    match action {
        CaptureAction::Start => "Starting dictation",
        CaptureAction::Stop => "Stopping dictation",
        CaptureAction::Toggle => "Toggling dictation",
        CaptureAction::Cancel => "Cancelling dictation",
    }
}

/// What the meeting recorder is doing (`meeting_runtime`).
fn meeting_report() -> MeetingReport {
    super::meetings::report(&crate::meeting::runtime::state())
}

/// `hex meetings record|stop`, as the tray's items do; waits a moment for
/// the recorder to confirm.
fn meeting(stop: bool, title: Option<String>) -> ControlReply {
    use crate::meeting::watcher::MeetingMenuState;
    if !crate::app_settings::meetings_enabled() {
        return ControlReply::error("feature-off", crate::app_settings::meetings_off_reason());
    }
    let request = match super::meetings::plan(&crate::meeting::runtime::state(), stop, title) {
        Ok(request) => request,
        Err(reply) => return reply,
    };
    if let Err(error) = crate::meeting::runtime::request(request) {
        return ControlReply::error("unavailable", error);
    }
    let started = Instant::now();
    while started.elapsed() < CONFIRM_WAIT {
        std::thread::sleep(Duration::from_millis(100));
        match (crate::meeting::runtime::state(), stop) {
            (MeetingMenuState::Recording { title, .. }, false) => {
                return ControlReply::done(format!(
                    "Recording “{title}”; `hex meetings stop` stops and transcribes it"
                ));
            }
            (MeetingMenuState::Transcribing | MeetingMenuState::Idle, true) => {
                return ControlReply::done("Stopped recording; transcribing the meeting");
            }
            _ => {}
        }
    }
    ControlReply::done(if stop {
        "Asked the meeting to stop; `hex status` shows when it's transcribing"
    } else {
        "Hex is starting the recording; `hex status` shows when it's recording"
    })
}

fn load() -> Result<AppSettings, ControlError> {
    AppSettings::load().map_err(|error| {
        ControlError::new("internal", format!("couldn't read settings: {error:#}"))
    })
}

/// Saves one setting as the window would, then carries out its effects.
fn set_setting(host: &dyn Host, key: &str, value: &str) -> Result<ControlReply, ControlError> {
    if let Some(error) = settings::unavailable(key) {
        return Err(error);
    }
    let before = load()?;
    let change = settings::apply(&before, key, value, &host.shortcut_rules())?;
    commit(host, &before, change, || {
        format!("{key} is already {value}")
    })
}

/// Saves every setting in `values` at once, as the window would.
fn apply_settings(
    host: &dyn Host,
    values: &serde_json::Map<String, serde_json::Value>,
) -> Result<ControlReply, ControlError> {
    let before = load()?;
    let change = settings::apply_all(&before, values, &host.shortcut_rules())?;
    commit(host, &before, change, || {
        "Every setting already has its value".into()
    })
}

/// Saves a validated change and carries out its effects.
fn commit(
    host: &dyn Host,
    before: &AppSettings,
    change: settings::Change,
    unchanged: impl FnOnce() -> String,
) -> Result<ControlReply, ControlError> {
    if !change.changed(before) {
        return Ok(ControlReply::unchanged(unchanged()));
    }
    host.save(before, change.settings)?;
    for effect in change.effects {
        match effect {
            settings::Effect::DockIcon => {}
            settings::Effect::CheckForUpdates => {
                let _ = host.check_for_updates();
            }
        }
    }
    Ok(ControlReply::done(change.message))
}

/// `hex model use` (installed only) and `hex model install` (downloads
/// first). Both switch once the model is ready.
fn use_model(
    host: &dyn Host,
    model: &str,
    language: Option<&str>,
    install: bool,
) -> Result<ControlReply, ControlError> {
    let selection = super::model_selection(&load()?, model, language)?;
    let definition = crate::transcription::models::definition(selection.model);
    let status = crate::transcription::readiness::readiness(&definition, &selection.language);
    let state = host.state()?;
    if same_model(&state.settings.transcription, &selection) && status.ready {
        return Ok(ControlReply::unchanged(format!(
            "{} ({}) is already in use",
            definition.name, selection.language
        )));
    }
    if !status.installed && !install {
        return Err(ControlError::new(
            "not-installed",
            format!(
                "{} isn't installed; `hex model install {}` downloads it ({})",
                definition.name,
                selection.model.as_str(),
                definition.size_label()
            ),
        ));
    }
    host.choose_model(selection.clone())?;
    if !status.installed {
        return Ok(ControlReply::done(format!(
            "Downloading {} ({}); Hex switches to it once it's ready. \
             `hex status` shows progress, `hex model cancel` stops it",
            definition.name,
            definition.size_label()
        )));
    }
    wait_for_model(host, &selection)
}

fn same_model(current: &TranscriptionSelection, wanted: &TranscriptionSelection) -> bool {
    current.model == wanted.model && current.language == wanted.language
}

fn wait_for_model(
    host: &dyn Host,
    selection: &TranscriptionSelection,
) -> Result<ControlReply, ControlError> {
    let name = crate::transcription::models::definition(selection.model)
        .name
        .to_string();
    // What the listener runs: the saved selection, or a lost import's fallback.
    let matches = |settings: &crate::app_settings::AppSettings| {
        same_model(&settings.transcription, selection)
            || same_model(
                &crate::transcription::custom_models::runnable(&settings.transcription),
                selection,
            )
    };
    let ready = || {
        crate::transcription::models::validate(selection)
            .is_ok_and(|model| crate::transcription::models::is_ready(&model, &selection.language))
    };
    let started = Instant::now();
    let mut seen_preparing = false;
    while started.elapsed() < CONFIRM_WAIT {
        std::thread::sleep(Duration::from_millis(100));
        let state = host.state()?;
        // A check that failed reports its reason, never a switch.
        if let Some(error) = &state.transcription.error
            && (seen_preparing || started.elapsed() > Duration::from_secs(1))
        {
            return Err(ControlError::new(
                "failed",
                format!("couldn't switch to {name}: {error}"),
            ));
        }
        let preparing = state.transcription.preparing.is_some();
        seen_preparing |= preparing;
        if preparing {
            continue;
        }
        // Only a model Hex can actually run counts as switched: a saved
        // file may still be waiting for its check.
        if ready() && matches(&state.settings) {
            return Ok(ControlReply::done(format!(
                "Switched to {name} ({})",
                selection.language
            )));
        }
    }
    Ok(ControlReply::done(format!(
        "Loading {name}; `hex status` shows when it's ready"
    )))
}

fn clear_history(host: &dyn Host) -> Result<ControlReply, ControlError> {
    let history = host
        .history()
        .ok_or_else(|| ControlError::new("unavailable", "history couldn't be opened"))?;
    let count = history.search("").len();
    let rollup = history.has_usage();
    if count == 0 && !rollup {
        return Ok(ControlReply::unchanged("History is already empty"));
    }
    history
        .clear()
        .map_err(|error| ControlError::new("failed", format!("couldn't clear history: {error}")))?;
    if count == 0 {
        return Ok(ControlReply::done("Cleared history stats"));
    }
    Ok(ControlReply::done(format!(
        "Cleared {count} dictation{}",
        if count == 1 { "" } else { "s" }
    )))
}

fn launch_at_login(host: &dyn Host, enabled: Option<bool>) -> ControlReply {
    let on = |enabled: bool| if enabled { "on" } else { "off" };
    let before = match host.launch_at_login(None) {
        None => return unsupported("Launch at login isn't available on this OS"),
        Some(Err(error)) => return ControlReply::error("failed", error),
        Some(Ok(before)) => before,
    };
    match enabled {
        None => ControlReply::unchanged(format!("Launch at login is {}", on(before))),
        Some(enabled) if enabled == before => {
            ControlReply::unchanged(format!("Launch at login is already {}", on(before)))
        }
        Some(enabled) => match host.launch_at_login(Some(enabled)) {
            Some(Ok(after)) => ControlReply::done(format!("Launch at login is {}", on(after))),
            Some(Err(error)) => ControlReply::error(
                "failed",
                format!("couldn't change launch at login: {error}"),
            ),
            None => unsupported("Launch at login isn't available on this OS"),
        },
    }
}

fn status(host: &dyn Host) -> Result<ControlReply, ControlError> {
    let state = host.state()?;
    let settings = &state.settings;
    let ready = crate::desktop::onboarding::model_ready(&settings.transcription);
    let capture = if state.listener.running {
        match host.capture() {
            Ok(snapshot) => CaptureReport::from(snapshot),
            Err(_) => CaptureReport {
                state: CaptureState::Unknown,
                ..Default::default()
            },
        }
    } else {
        CaptureReport {
            state: CaptureState::Idle,
            ..Default::default()
        }
    };
    Ok(ControlReply::Status(Box::new(StatusReport {
        app_version: crate::VERSION.into(),
        pid: std::process::id(),
        memory_footprint_bytes: super::memory_footprint(),
        listener: ListenerReport {
            running: state.listener.running,
            status: match state
                .listener
                .notice
                .as_ref()
                .or(state.listener.warning.as_ref())
            {
                Some(warning) => format!("{} ({warning})", state.listener.status),
                None => state.listener.status.to_string(),
            },
            shortcuts: match &state.listener.shortcuts {
                crate::desktop::host::DesktopShortcuts::Portal(granted) => granted
                    .iter()
                    .map(|shortcut| super::GrantedShortcutReport {
                        name: shortcut.name.clone(),
                        trigger: shortcut.trigger.clone(),
                    })
                    .collect(),
                _ => Vec::new(),
            },
        },
        capture,
        meeting: meeting_report(),
        model: ModelReport {
            preparing: state
                .transcription
                .preparing
                .map(|model| model.as_str().into()),
            downloaded_bytes: state.transcription.downloaded_bytes,
            error: state.transcription.error.clone(),
            backend: state.transcription.backend.clone(),
            ..ModelReport::saved(&settings.transcription)
        },
        features: FeatureReport {
            meetings: crate::app_settings::meetings_enabled(),
            release_microphone_while_idle: settings.release_microphone_while_idle,
            history_retention: serde_json::to_value(settings.history_retention)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default(),
        },
        // Linux and Windows gate dictation on no privacy permission.
        permissions: PermissionReport {
            ready,
            microphone: "ready".into(),
            accessibility: "ready".into(),
            transcription_model: ready,
        },
        window: WindowReport {
            open: state.window_open,
            pane: None,
        },
        update: super::update_label(&state.update).into(),
        labs: crate::labs::active_names(settings),
        capabilities: crate::rune_ui::capabilities(),
    })))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct FakeHost {
        recording: bool,
        service: bool,
        pending_jobs: usize,
        unresponsive: bool,
        quits: Mutex<Vec<bool>>,
        dictations: Mutex<Vec<CaptureAction>>,
        history: Option<crate::history::History>,
        backend: Option<crate::transcription::SpeechBackend>,
    }

    impl Host for FakeHost {
        fn state(&self) -> Result<HostState, ControlError> {
            Ok(HostState {
                settings: AppSettings::default(),
                listener: DesktopListenerSnapshot {
                    running: true,
                    warming: false,
                    status: crate::desktop::host::ListenerStatus::Enabled,
                    warning: None,
                    notice: None,
                    last_failure: None,
                    shortcuts: Default::default(),
                    shortcuts_configurable: false,
                },
                transcription: DesktopTranscriptionSnapshot {
                    downloaded_bytes: 0,
                    error: None,
                    preparation_stage: None,
                    selection: Default::default(),
                    preparing: None,
                    backend: self.backend.clone(),
                },
                update: DesktopUpdateStatus::Current,
                window_open: false,
            })
        }
        fn capture(&self) -> Result<CaptureSnapshot, ControlError> {
            if self.unresponsive {
                return Err(ControlError::new("unavailable", "listener didn't answer"));
            }
            Ok(CaptureSnapshot {
                recording: if self.service {
                    Some(super::super::CaptureKind::Service)
                } else {
                    self.recording
                        .then_some(super::super::CaptureKind::Dictation)
                },
                pending_jobs: self.pending_jobs,
                ..Default::default()
            })
        }
        fn dictation(
            &self,
            action: CaptureAction,
            print: Option<PrintReply>,
            _: bool,
        ) -> Result<(), ControlError> {
            self.dictations.lock().unwrap().push(action);
            if let Some(print) = print {
                print.send(super::super::PrintedTranscript::Text("Ship it.".into()));
            }
            Ok(())
        }
        fn paste_last(&self) -> Result<(), ControlError> {
            Ok(())
        }
        fn save(&self, _: &AppSettings, _: AppSettings) -> Result<(), ControlError> {
            Ok(())
        }
        fn history(&self) -> Option<crate::history::History> {
            self.history.clone()
        }
        fn show_window(&self, _: Option<Pane>) -> Result<(), ControlError> {
            Ok(())
        }
        fn hide_window(&self) -> bool {
            false
        }
        fn onboarding_reset(&self) -> ControlReply {
            ControlReply::done("Opened onboarding")
        }
        fn choose_model(&self, _: TranscriptionSelection) -> Result<(), ControlError> {
            Ok(())
        }
        fn model_switch(&self) -> Result<(), ControlError> {
            Ok(())
        }
        fn cancel_model(&self) {}
        fn check_for_updates(&self) -> Result<(), ControlError> {
            Ok(())
        }
        fn launch_at_login(&self, _: Option<bool>) -> Option<Result<bool, String>> {
            None
        }
        fn quit(&self, restart: bool) -> Result<(), ControlError> {
            self.quits.lock().unwrap().push(restart);
            Ok(())
        }
        fn shortcut_rules(&self) -> settings::ShortcutRules {
            settings::ShortcutRules {
                key_named: |name| Ok(crate::app_settings::HotkeyKey::named(name)),
                rule: |_, _| Ok(()),
            }
        }
    }

    fn ask(host: &FakeHost, command: ControlCommand) -> ControlReply {
        answer(host, ControlRequest::new(command)).reply
    }

    fn code(reply: &ControlReply) -> &str {
        match reply {
            ControlReply::Error { code, .. } => code,
            _ => "ok",
        }
    }

    #[test]
    fn clearing_history_with_only_a_usage_rollup_still_deletes_it() {
        let directory =
            std::env::temp_dir().join(format!("hex-control-clear-usage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("history.json");
        let history = crate::history::History::new(crate::history::HistoryStore::open(
            path.clone(),
            crate::history::HistoryRetention::Week,
            crate::history::now_ms(),
        ));
        crate::history::usage::count(
            &crate::history::usage::path(&path),
            crate::history::now_ms(),
            4,
            1_000,
        )
        .unwrap();
        assert!(history.has_usage());

        let host = FakeHost {
            history: Some(history.clone()),
            ..Default::default()
        };
        let reply = ask(&host, ControlCommand::HistoryClear);
        assert!(matches!(reply, ControlReply::Done { changed: true, .. }));
        assert!(!history.has_usage());
        assert!(history.search("").is_empty());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn quitting_waits_for_a_recording_unless_forced() {
        let host = FakeHost {
            recording: true,
            ..Default::default()
        };
        assert_eq!(
            code(&ask(&host, ControlCommand::Quit { force: false })),
            "busy"
        );
        assert!(host.quits.lock().unwrap().is_empty());
        assert_eq!(
            code(&ask(&host, ControlCommand::Restart { force: true })),
            "ok"
        );
        assert_eq!(*host.quits.lock().unwrap(), [true]);
    }

    /// A switch never disturbs a recording or an accepted dictation job.
    #[test]
    fn model_switch_is_busy_while_dictation_is_pending() {
        for host in [
            FakeHost {
                recording: true,
                ..Default::default()
            },
            FakeHost {
                pending_jobs: 1,
                ..Default::default()
            },
        ] {
            assert_eq!(
                code(&ask(&host, ControlCommand::ModelSwitch)),
                "busy",
                "recording or a pending job refuses the switch"
            );
        }
    }

    #[test]
    fn quit_and_status_use_the_owner_even_without_dictating_log_events() {
        for (host, expected) in [
            (
                FakeHost {
                    service: true,
                    ..Default::default()
                },
                CaptureState::Recording,
            ),
            (
                FakeHost {
                    pending_jobs: 2,
                    ..Default::default()
                },
                CaptureState::Transcribing,
            ),
            (
                FakeHost {
                    unresponsive: true,
                    ..Default::default()
                },
                CaptureState::Unknown,
            ),
        ] {
            assert_eq!(
                code(&ask(&host, ControlCommand::Quit { force: false })),
                "busy"
            );
            assert!(host.quits.lock().unwrap().is_empty());
            let ControlReply::Status(status) = ask(&host, ControlCommand::Status) else {
                panic!("expected status");
            };
            assert_eq!(status.capture.state, expected);
            assert_eq!(status.capture.pending_jobs, host.pending_jobs);
            assert_eq!(
                code(&ask(&host, ControlCommand::Quit { force: true })),
                "ok"
            );
        }
    }

    #[test]
    fn status_reports_the_published_speech_backend() {
        let host = FakeHost {
            backend: Some(crate::transcription::SpeechBackend {
                kind: "vulkan".into(),
                device: "AMD Radeon RX 9060 XT".into(),
                crash_fallback: false,
            }),
            ..Default::default()
        };
        let ControlReply::Status(status) = ask(&host, ControlCommand::Status) else {
            panic!("expected status");
        };
        assert_eq!(status.model.backend, host.backend);
    }

    #[test]
    fn a_printing_stop_answers_with_the_transcript() {
        let host = FakeHost::default();
        let stop = |print| ControlCommand::Dictation {
            action: CaptureAction::Stop,
            print,
            raw: false,
        };
        assert_eq!(
            ask(&host, stop(true)),
            ControlReply::Transcript {
                text: "Ship it.".into()
            }
        );
        assert_eq!(code(&ask(&host, stop(false))), "ok");
        let start_print = ControlCommand::Dictation {
            action: CaptureAction::Start,
            print: true,
            raw: false,
        };
        assert_eq!(code(&ask(&host, start_print)), "unsupported");
        assert_eq!(
            *host.dictations.lock().unwrap(),
            [CaptureAction::Stop, CaptureAction::Stop]
        );
    }

    #[test]
    fn what_this_os_cannot_do_is_unsupported_at_runtime() {
        let host = FakeHost::default();
        for command in [
            ControlCommand::LaunchAtLogin { enabled: None },
            ControlCommand::TypedText,
            ControlCommand::ScrawlCaptureUndo,
            ControlCommand::WindowStory {
                story: "HUD".into(),
                variant: None,
                play: false,
            },
        ] {
            assert_eq!(
                code(&ask(&host, command.clone())),
                "unsupported",
                "{command:?}"
            );
        }
        assert!(matches!(
            ask(&host, ControlCommand::Status),
            ControlReply::Status(report) if report.listener.running && !report.capabilities.is_empty()
        ));
    }
}

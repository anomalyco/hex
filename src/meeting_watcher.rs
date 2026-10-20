use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};
use gpui::{
    App, Application, Bounds, ClipboardItem, Context, IntoElement, KeyBinding, Menu, MenuItem,
    Render, SystemMenuType, Timer, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions, div, point, prelude::*, px, rgb, size,
};

use crate::app_settings::AppSettings;
use crate::app_window::AppWindow;
use crate::context::ContextSnapshot;
use crate::design_tokens::*;
use crate::desktop_ui::{ButtonKind, button, status_dot, truncated_line};
use crate::dictation_indicator::DictationIndicatorEvent;
use crate::dictation_indicator::{self, DictationIndicatorUi};
use crate::meeting::{self, MeetingRequest};
use crate::meeting_detection::{
    CallPresence, MeetingCandidate, MeetingDetector, MeetingSource, candidate_from_microphone,
};
use crate::microphone_activity::active_microphone_applications;
use crate::transcription_preparation::TranscriptionPreparation;
use crate::{config, recognition};

const PANEL_WIDTH: f32 = 420.0;
const PANEL_HEIGHT: f32 = 64.0;
const WINDOW_MARGIN: f32 = 12.0;
const OFFER_TIMEOUT: Duration = Duration::from_secs(20);
const READY_TIMEOUT: Duration = Duration::from_secs(15);

enum ControllerCommand {
    Record(MeetingCandidate),
    Dismiss,
    Stop,
    Summarize(String),
}

enum ControllerEvent {
    Offer(MeetingCandidate),
    Starting,
    RecordingStarted { title: String },
    Retitled { title: String },
    Transcribing,
    CallEnded { app: String },
    Finished(Box<meeting::MeetingManifest>),
    Failed { title: String, error: String },
    NotesStarted,
    NotesFinished(Box<meeting::MeetingManifest>),
}

struct ActiveRecording {
    title: String,
    application: Option<String>,
    app_label: String,
    presence: CallPresence,
    stop: Arc<AtomicBool>,
    updates: Receiver<meeting::RecordingUpdate>,
    reported_started: bool,
    worker: JoinHandle<Result<meeting::MeetingManifest>>,
}

/// The meeting state mirrored into the menu bar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MeetingMenuState {
    Idle,
    Starting,
    Recording { started: Instant, title: String },
    Transcribing,
}

struct MeetingUi {
    commands: SyncSender<ControllerCommand>,
    app_window: Rc<RefCell<Option<WindowHandle<AppWindow>>>>,
    event_path: PathBuf,
    meeting_requests: SyncSender<MeetingRequest>,
    indicator: crate::dictation_indicator::DictationIndicatorSender,
    recognition_start: Option<SyncSender<()>>,
    history: Option<crate::history::History>,
    recognition_controls: SyncSender<crate::recognition::RecognitionControl>,
    status_actions: Option<Receiver<crate::status_item::StatusItemAction>>,
    transcription_preparation: TranscriptionPreparation,
    model_menu_settings: AppSettings,
}

impl MeetingUi {
    fn open_or_focus(&self, cx: &mut App) -> gpui::Result<WindowHandle<AppWindow>> {
        crate::app_window::open_or_focus(
            &self.app_window,
            self.event_path.clone(),
            config::voice_control(),
            self.meeting_requests.clone(),
            self.indicator.clone(),
            self.recognition_start.clone(),
            self.history.clone(),
            cx,
        )
    }

    /// Replaces a pending offer with the app window and applies a meeting
    /// state change to it. A failed open is logged; the update is still
    /// attempted against any already-open window.
    fn show_meeting_window(
        &self,
        offer: Option<WindowHandle<MeetingPanel>>,
        open_failure: &'static str,
        cx: &mut App,
        update: impl FnOnce(&mut AppWindow, &mut Context<AppWindow>),
    ) {
        close_window(offer, cx);
        if let Err(error) = self.open_or_focus(cx) {
            tracing::error!(%error, "{open_failure}");
        }
        update_app_window(&self.app_window, cx, update);
    }
}

struct RuntimeWorkers {
    local_api: Option<crate::local_api::LocalApi>,
    recognition: Option<thread::JoinHandle<()>>,
    controller: Option<thread::JoinHandle<()>>,
}

impl RuntimeWorkers {
    fn join(mut self) -> Result<()> {
        if let Some(mut local_api) = self.local_api.take() {
            local_api.shutdown();
        }
        if let Some(worker) = self.recognition {
            worker
                .join()
                .map_err(|_| eyre!("recognition worker panicked during shutdown"))?;
        }
        if let Some(controller) = self.controller {
            controller
                .join()
                .map_err(|_| eyre!("meeting controller panicked during shutdown"))?;
        }
        Ok(())
    }

    fn join_and_log(self) {
        if let Err(error) = self.join() {
            tracing::error!(%error, "runtime worker shutdown failed");
        }
    }
}

pub struct ListenerConfig {
    pub project_root: PathBuf,
    pub event_path: PathBuf,
    pub device: Option<String>,
}

/// What the desktop process hosts for its lifetime.
pub enum Launch {
    /// The production app with local dictation.
    App(ListenerConfig),
    /// The dictation HUD driven by a synthetic capture loop, without recognition.
    DictationHudPreview,
    /// Developer meeting detection; `offer_preview` shows an offer immediately.
    #[cfg(debug_assertions)]
    MeetingWatch { offer_preview: bool },
    /// One isolated, deterministic UI preview without app services.
    Shell(crate::app_window::AppWindowPreview),
    /// One meeting capsule without app services or detection.
    MeetingPanelPreview(PanelPreview),
}

#[derive(Clone, Copy, Debug)]
pub enum PanelPreview {
    Offer,
    CallEnded,
    Ready,
}

fn run_panel_preview(preview: PanelPreview) -> Result<()> {
    let kind = match preview {
        PanelPreview::Offer => PanelKind::Offer(MeetingCandidate {
            key: "preview".into(),
            title: "Zoom meeting".into(),
            source: MeetingSource::Zoom,
            application: None,
            named: false,
        }),
        PanelPreview::CallEnded => PanelKind::CallEnded { app: "Zoom".into() },
        PanelPreview::Ready => {
            let mut manifest: meeting::MeetingManifest =
                serde_json::from_value(serde_json::json!({
                    "schema_version": 1,
                    "id": "preview",
                    "title": "Zoom call — 2:30 PM",
                    "status": "complete",
                    "created_at_ms": crate::events::now_ms(),
                    "ended_at_ms": null,
                    "duration_ms": 1_934_000,
                    "system": {"file": "system.wav", "samples": 0, "dropped_packets": 0},
                    "microphone": {"file": "microphone.wav", "samples": 0, "dropped_packets": 0},
                    "error": null
                }))?;
            manifest.export_path = Some(PathBuf::from(
                "~/Documents/Hex Meetings/2026-10-15 1430 Zoom call — 2-30 PM.md",
            ));
            manifest.title = meeting::default_title(None, manifest.created_at_ms);
            manifest.notes = Some(meeting::MeetingNotes {
                title: "Q4 launch plan and docs owners".into(),
                summary: "The team set the Q4 launch for November 12. Docs owners are still open."
                    .into(),
                ..Default::default()
            });
            PanelKind::Ready(Box::new(manifest))
        }
    };
    let (commands, _receiver) = mpsc::sync_channel(8);
    Application::new().run(move |cx| {
        cx.bind_keys([KeyBinding::new(
            "cmd-q",
            crate::app_window::QuitApplication,
            None,
        )]);
        cx.on_action(|_: &crate::app_window::QuitApplication, cx| cx.quit());
        let deadline = matches!(preview, PanelPreview::Offer).then_some(OFFER_TIMEOUT * 30);
        if let Err(error) = open_panel(kind, commands, deadline, cx) {
            tracing::error!(%error, "could not open meeting panel preview");
            cx.quit();
        }
    });
    Ok(())
}

fn should_open_app_on_launch(
    show_dock_icon: bool,
    setup_ready: bool,
    onboarding_completed: bool,
    status_item_available: bool,
) -> bool {
    show_dock_icon || !setup_ready || !onboarding_completed || !status_item_available
}

pub fn run(shutdown: &'static AtomicBool, launch: Launch) -> Result<()> {
    if objc2::MainThreadMarker::new().is_none() {
        return Err(eyre!("desktop startup requires the main thread"));
    }
    let (listener, dictation_preview, preview, shell_preview) = match launch {
        Launch::MeetingPanelPreview(preview) => return run_panel_preview(preview),
        Launch::App(listener) => (Some(listener), false, false, None),
        Launch::DictationHudPreview => (None, true, false, None),
        #[cfg(debug_assertions)]
        Launch::MeetingWatch { offer_preview } => (None, false, offer_preview, None),
        Launch::Shell(shell_preview) => (None, false, false, Some(shell_preview)),
    };
    shutdown.store(false, Ordering::Relaxed);
    crate::keyboard::initialize_layout()?;
    let settings = if shell_preview.is_some() {
        AppSettings::default()
    } else {
        AppSettings::load()?
    };
    let show_dock_icon = settings.show_dock_icon;
    let setup_ready = crate::onboarding::status(&settings.transcription).ready();
    let onboarding_completed = shell_preview.is_some() || crate::onboarding::completion_recorded();
    let history = if listener.is_some() {
        match crate::history::History::open_default(settings.history_retention) {
            Ok(history) => Some(history),
            Err(error) => {
                tracing::warn!(%error, "dictation history is unavailable");
                None
            }
        }
    } else {
        None
    };
    let app_event_path = listener
        .as_ref()
        .map(|listener| listener.event_path.clone());
    let event_log = app_event_path
        .as_ref()
        .map(|path| crate::events::EventLog::create(path))
        .transpose()?;
    let (developer_sender, developer_receiver) = mpsc::sync_channel(8);
    let (recognition_control_sender, recognition_control_receiver) = mpsc::sync_channel(8);
    let local_api = event_log
        .as_ref()
        .map(|events| {
            crate::local_api::LocalApi::start(
                events.clone(),
                crate::local_api::LocalApiOptions {
                    developer_control: crate::DEVELOPER_FEATURES_ENABLED
                        .then(|| developer_sender.clone()),
                    dictation_control: Some(recognition_control_sender.clone()),
                },
            )
        })
        .transpose()?;
    let meeting_project_root = listener
        .as_ref()
        .map(|listener| listener.project_root.clone())
        .unwrap_or(std::env::current_dir()?);
    let shell_event_path = app_event_path
        .clone()
        .unwrap_or_else(|| meeting_project_root.join("logs/live.ndjson"));
    let (event_sender, event_receiver) = mpsc::sync_channel(32);
    let (command_sender, command_receiver) = mpsc::sync_channel(8);
    let (meeting_request_sender, meeting_request_receiver) = mpsc::sync_channel(8);
    let (indicator_sender, indicator_receiver) = dictation_indicator::channel();
    if dictation_preview {
        let preview_sender = indicator_sender.clone();
        thread::spawn(move || {
            loop {
                preview_sender.send(DictationIndicatorEvent::Started);
                thread::sleep(Duration::from_millis(450));
                let started = Instant::now();
                while started.elapsed() < Duration::from_secs(5) {
                    let wave = (started.elapsed().as_secs_f32() * 5.0).sin() * 0.5 + 0.5;
                    preview_sender.send(DictationIndicatorEvent::Meter {
                        average: 0.025 + wave * 0.09,
                        peak: 0.15 + wave * 0.55,
                    });
                    thread::sleep(Duration::from_millis(20));
                }
                preview_sender.send(DictationIndicatorEvent::Submitted { job_id: 0 });
                preview_sender.send(DictationIndicatorEvent::Transcribing { job_id: 0 });
                thread::sleep(Duration::from_millis(700));
                preview_sender.send(DictationIndicatorEvent::Started);
                thread::sleep(Duration::from_millis(500));
                preview_sender.send(DictationIndicatorEvent::Processing { job_id: 0 });
                thread::sleep(Duration::from_millis(800));
                preview_sender.send(DictationIndicatorEvent::Submitted { job_id: 1 });
                preview_sender.send(DictationIndicatorEvent::Transcribing { job_id: 1 });
                thread::sleep(Duration::from_millis(700));
                preview_sender.send(DictationIndicatorEvent::JobCompleted { job_id: 0 });
                thread::sleep(Duration::from_millis(700));
                preview_sender.send(DictationIndicatorEvent::Processing { job_id: 1 });
                thread::sleep(Duration::from_secs(1));
                preview_sender.send(DictationIndicatorEvent::JobCompleted { job_id: 1 });
                thread::sleep(Duration::from_secs(1));
            }
        });
    }
    // The controller idles without inspecting microphone activity until the
    // user opts in to meetings (always available in developer builds).
    let controller_worker = (shell_preview.is_none() && !dictation_preview).then(|| {
        let controller_events = event_sender.clone();
        thread::spawn(move || {
            if preview {
                let _ = controller_events.send(ControllerEvent::Offer(MeetingCandidate {
                    key: "preview".into(),
                    title: "Design sync".into(),
                    source: MeetingSource::Zoom,
                    application: None,
                    named: true,
                }));
            }
            if let Err(error) = controller_loop(
                shutdown,
                controller_events.clone(),
                command_receiver,
                meeting_request_receiver,
                meeting_project_root,
            ) {
                tracing::error!(%error, "meeting controller stopped");
                let _ = controller_events.send(ControllerEvent::Failed {
                    title: "Meeting detection stopped".into(),
                    error: error.to_string(),
                });
            }
        })
    });

    let recognition_meeting_requests = meeting_request_sender.clone();
    let (recognition_start_sender, recognition_start_receiver) = if listener.is_some() {
        let (sender, receiver) = mpsc::sync_channel(1);
        (Some(sender), Some(receiver))
    } else {
        (None, None)
    };
    let recognition_worker = listener.map(|listener| {
        let recognition_events = event_log.clone().expect("listener event log must exist");
        let failure_indicator = indicator_sender.clone();
        let recognition_indicator = indicator_sender.clone();
        let meeting_requests = Some(recognition_meeting_requests.clone());
        let recognition_history = history.clone();
        let start = recognition_start_receiver.expect("listener start receiver must exist");
        thread::spawn(move || {
            if !setup_ready {
                loop {
                    if shutdown.load(Ordering::Relaxed) {
                        return;
                    }
                    match start.recv_timeout(Duration::from_millis(100)) {
                        Ok(()) => break,
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            }
            if let Err(error) = recognition::listen(
                &listener.project_root,
                recognition_events,
                listener.device.as_deref(),
                config::voice_control(),
                shutdown,
                Some(recognition_indicator),
                meeting_requests,
                recognition_history,
                Some(recognition_control_receiver),
            ) {
                tracing::error!(%error, "desktop recognition stopped");
                if format!("{error:#}").contains("event tap") {
                    crate::onboarding::note_listener_failure();
                }
                failure_indicator.send(crate::dictation_indicator::DictationIndicatorEvent::Failed);
            }
        })
    });
    let runtime_workers = Rc::new(RefCell::new(Some(RuntimeWorkers {
        local_api,
        recognition: recognition_worker,
        controller: controller_worker,
    })));

    let app_window = Rc::new(RefCell::new(None::<WindowHandle<AppWindow>>));
    let reopen_window = app_window.clone();
    let reopen_event_path = shell_event_path.clone();
    let reopen_meeting_requests = meeting_request_sender.clone();
    let reopen_recognition_start = recognition_start_sender.clone();
    let reopen_preview = shell_preview.clone();
    let reopen_history = history.clone();
    let reopen_indicator = indicator_sender.clone();
    let indicator_enabled = shell_preview.as_ref().is_none_or(|preview| {
        matches!(
            preview.pane,
            crate::developer_control::DeveloperPane::HudLab
        )
    });
    let application = Application::new();
    application.on_reopen(move |cx| {
        let result = match reopen_preview.clone() {
            Some(preview) => crate::app_window::open_preview(
                &reopen_window,
                reopen_event_path.clone(),
                config::voice_control(),
                reopen_meeting_requests.clone(),
                reopen_indicator.clone(),
                preview,
                cx,
            ),
            None => crate::app_window::open_or_focus(
                &reopen_window,
                reopen_event_path.clone(),
                config::voice_control(),
                reopen_meeting_requests.clone(),
                reopen_indicator.clone(),
                reopen_recognition_start.clone(),
                reopen_history.clone(),
                cx,
            ),
        };
        if let Err(error) = result {
            tracing::error!(%error, "could not open Hex");
        }
    });
    let application_runtime_workers = runtime_workers.clone();
    application.run(move |cx| {
        let transcription_preparation = cx.default_global::<TranscriptionPreparation>().clone();
        let close_app_window = app_window.clone();
        cx.bind_keys([
            KeyBinding::new("cmd-w", crate::app_window::CloseWindow, None),
            KeyBinding::new("cmd-q", crate::app_window::QuitApplication, None),
            KeyBinding::new("cmd-h", crate::app_window::HideApplication, None),
            KeyBinding::new("cmd-m", crate::app_window::MinimizeWindow, None),
            KeyBinding::new("ctrl-cmd-f", crate::app_window::ToggleFullscreen, None),
            KeyBinding::new("cmd-,", crate::app_window::ShowSettings, None),
            KeyBinding::new("cmd-1", crate::app_window::ShowSettings, None),
            KeyBinding::new("cmd-2", crate::app_window::ShowModes, None),
            KeyBinding::new("cmd-3", crate::app_window::ShowCommands, None),
            KeyBinding::new("cmd-4", crate::app_window::ShowVoiceAction, None),
            KeyBinding::new("cmd-5", crate::app_window::ShowHistory, None),
        ]);
        cx.bind_keys(crate::text_input::key_bindings());
        cx.on_action(move |_: &crate::app_window::CloseWindow, cx| {
            close_window(close_app_window.borrow_mut().take(), cx)
        });
        cx.on_action(|_: &crate::app_window::QuitApplication, cx| cx.quit());
        cx.on_action(|_: &crate::app_window::CheckForUpdates, _| {
            crate::sparkle::check_for_updates()
        });
        let retry_commands = recognition_control_sender.clone();
        cx.on_action(move |_: &crate::app_window::RetryCommands, _| {
            if let Err(error) =
                retry_commands.try_send(crate::recognition::RecognitionControl::RetryCommands)
            {
                tracing::warn!(%error, "could not request command model retry");
            }
        });
        cx.on_action(|_: &crate::app_window::HideApplication, _| {
            crate::app_settings::hide_application()
        });
        let mut application_items = Vec::new();
        if !crate::sparkle::updates_managed() {
            application_items.extend([
                MenuItem::action("Check for Updates…", crate::app_window::CheckForUpdates),
                MenuItem::separator(),
            ]);
        }
        application_items.extend([
            MenuItem::action("Settings…", crate::app_window::ShowSettings),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Hide Hex", crate::app_window::HideApplication),
            MenuItem::separator(),
            MenuItem::action("Quit Hex", crate::app_window::QuitApplication),
        ]);
        cx.set_menus(vec![
            Menu {
                name: "Hex".into(),
                items: application_items,
            },
            Menu {
                name: "File".into(),
                items: vec![MenuItem::action(
                    "Close Window",
                    crate::app_window::CloseWindow,
                )],
            },
            Menu {
                name: "Window".into(),
                items: vec![
                    MenuItem::action("Minimize", crate::app_window::MinimizeWindow),
                    MenuItem::action("Enter Full Screen", crate::app_window::ToggleFullscreen),
                ],
            },
        ]);
        if shell_preview.is_none() {
            crate::sparkle::start();
        }
        let status_actions = if shell_preview.is_none() {
            match crate::status_item::install() {
                Ok(actions) => Some(actions),
                Err(error) => {
                    tracing::error!(%error, "could not install Hex status item");
                    None
                }
            }
        } else {
            None
        };
        crate::status_item::update_transcription(&settings, &transcription_preparation.status());
        crate::app_settings::set_dock_icon_visible(crate::app_settings::dock_icon_visible(
            show_dock_icon,
            status_actions.is_some(),
        ));
        let show_app_on_launch = should_open_app_on_launch(
            show_dock_icon,
            setup_ready,
            onboarding_completed,
            status_actions.is_some(),
        );
        let open_result = match shell_preview.clone() {
            Some(preview) => Some(crate::app_window::open_preview(
                &app_window,
                shell_event_path.clone(),
                config::voice_control(),
                meeting_request_sender.clone(),
                indicator_sender.clone(),
                preview,
                cx,
            )),
            None if show_app_on_launch => app_event_path.map(|event_path| {
                crate::app_window::open_or_focus(
                    &app_window,
                    event_path,
                    config::voice_control(),
                    meeting_request_sender.clone(),
                    indicator_sender.clone(),
                    recognition_start_sender.clone(),
                    history.clone(),
                    cx,
                )
            }),
            None => None,
        };
        if let Some(Err(error)) = open_result {
            tracing::error!(%error, "could not open Hex");
        }
        let quit_shutdown = shutdown;
        let quit_workers = application_runtime_workers.clone();
        let quit_preparation = transcription_preparation.clone();
        cx.on_app_quit(move |_| {
            quit_preparation.cancel();
            quit_shutdown.store(true, Ordering::Relaxed);
            if let Some(workers) = quit_workers.borrow_mut().take() {
                workers.join_and_log();
            }
            async {}
        })
        .detach();

        let ui_history = history.clone();
        cx.spawn(async move |cx| {
            drive_ui(
                event_receiver,
                indicator_receiver,
                developer_receiver,
                MeetingUi {
                    commands: command_sender,
                    app_window: app_window.clone(),
                    event_path: shell_event_path.clone(),
                    meeting_requests: meeting_request_sender.clone(),
                    indicator: indicator_sender.clone(),
                    recognition_start: recognition_start_sender.clone(),
                    history: ui_history.clone(),
                    recognition_controls: recognition_control_sender.clone(),
                    status_actions,
                    transcription_preparation,
                    model_menu_settings: settings,
                },
                shutdown,
                indicator_enabled,
                cx,
            )
            .await;
        })
        .detach();
    });
    shutdown.store(true, Ordering::Relaxed);
    if let Some(workers) = runtime_workers.borrow_mut().take() {
        workers.join()?
    }
    Ok(())
}

async fn drive_ui(
    events: Receiver<ControllerEvent>,
    indicator_events: Receiver<crate::dictation_indicator::DictationIndicatorEvent>,
    developer_calls: Receiver<crate::developer_control::DeveloperCall>,
    mut ui: MeetingUi,
    shutdown: &AtomicBool,
    indicator_enabled: bool,
    cx: &mut gpui::AsyncApp,
) {
    let mut offer_window: Option<WindowHandle<MeetingPanel>> = None;
    let mut dictation_indicator = indicator_enabled.then(DictationIndicatorUi::new);
    let mut model_menu_refresh_at = Instant::now();
    let mut meeting_state = MeetingMenuState::Idle;
    let mut meeting_menu_refresh_at = Instant::now();

    loop {
        if shutdown.load(Ordering::Relaxed) {
            ui.transcription_preparation.cancel();
            let _ = cx.update(|cx| cx.quit());
            return;
        }
        while let Ok(event) = indicator_events.try_recv() {
            if let Some(dictation_indicator) = &mut dictation_indicator
                && let Err(error) = cx.update(|cx| dictation_indicator.handle(event, cx))
            {
                tracing::error!(%error, "could not update dictation indicator");
                return;
            }
        }
        if let Some(dictation_indicator) = &mut dictation_indicator {
            let _ = cx.update(|cx| dictation_indicator.follow_pointer(cx));
        }
        while let Ok(call) = developer_calls.try_recv() {
            let reply = cx
                .update(|cx| apply_developer_command(call.command, &ui, cx))
                .unwrap_or_else(|error| {
                    crate::developer_control::DeveloperReply::error(
                        "developer-ui-failed",
                        error.to_string(),
                    )
                });
            let _ = call.reply.send(reply);
        }
        while let Some(action) = ui
            .status_actions
            .as_ref()
            .and_then(|actions| actions.try_recv().ok())
        {
            let result = cx.update(|cx| match action {
                crate::status_item::StatusItemAction::OpenSettings
                | crate::status_item::StatusItemAction::OpenModels => match ui.open_or_focus(cx) {
                    Ok(handle) => {
                        let _ = handle.update(cx, |window, _, cx| {
                            if action == crate::status_item::StatusItemAction::OpenModels {
                                window.show_transcription_models(cx);
                            } else {
                                window.show_settings(cx);
                            }
                        });
                    }
                    Err(error) => {
                        tracing::error!(%error, "could not open Hex from status item");
                    }
                },
                crate::status_item::StatusItemAction::SelectModel(model) => {
                    let (_, current) = crate::app_settings::transcription_selection();
                    ui.model_menu_settings.transcription = current;
                    let selection = crate::app_window::menu_transcription_selection(
                        &ui.app_window,
                        model,
                        &ui.model_menu_settings,
                        cx,
                    );
                    ui.transcription_preparation.start(selection, true);
                    model_menu_refresh_at = Instant::now();
                }
                crate::status_item::StatusItemAction::CancelModelPreparation => {
                    ui.transcription_preparation.cancel();
                    model_menu_refresh_at = Instant::now();
                }
                crate::status_item::StatusItemAction::PasteLast => {
                    let _ = ui
                        .recognition_controls
                        .try_send(crate::recognition::RecognitionControl::PasteLast);
                }
                crate::status_item::StatusItemAction::CheckForUpdates => {
                    crate::sparkle::check_for_updates();
                }
                crate::status_item::StatusItemAction::StartMeeting => {
                    if let Err(error) = ui.meeting_requests.try_send(MeetingRequest::Start) {
                        tracing::warn!(%error, "could not start meeting from status item");
                    }
                }
                crate::status_item::StatusItemAction::StopMeeting => {
                    if let Err(error) = ui.commands.try_send(ControllerCommand::Stop) {
                        tracing::warn!(%error, "could not stop meeting from status item");
                    }
                }
                crate::status_item::StatusItemAction::OpenMeetings => match ui.open_or_focus(cx) {
                    Ok(handle) => {
                        let _ = handle.update(cx, |window, _, cx| window.show_meetings(cx));
                    }
                    Err(error) => tracing::error!(%error, "could not open Meetings"),
                },
                crate::status_item::StatusItemAction::Quit => {
                    ui.transcription_preparation.cancel();
                    cx.quit();
                }
            });
            if let Err(error) = result {
                tracing::error!(%error, "could not handle Hex status item action");
                return;
            }
        }
        // Delivered menu cancellations/new choices win over an uncommitted completion.
        if let Some(result) = ui.transcription_preparation.poll() {
            let result = result.and_then(|selection| {
                cx.update(|cx| {
                    crate::app_window::commit_transcription_selection(&ui.app_window, selection, cx)
                })
                .map_err(|error| error.to_string())?
            });
            match result {
                Ok(settings) => {
                    ui.model_menu_settings = settings;
                    ui.transcription_preparation.set_error(None);
                }
                Err(error) => {
                    tracing::warn!(%error, "could not select transcription model");
                    ui.transcription_preparation.set_error(Some(error));
                }
            }
            model_menu_refresh_at = Instant::now();
        }
        if ui.status_actions.is_some() && Instant::now() >= model_menu_refresh_at {
            let status = ui.transcription_preparation.status();
            let _ = cx.update(|cx| {
                crate::app_window::refresh_transcription_menu(
                    &ui.app_window,
                    &ui.model_menu_settings,
                    &status,
                    cx,
                );
            });
            model_menu_refresh_at = Instant::now() + Duration::from_secs(1);
        }
        while let Ok(event) = events.try_recv() {
            let result = cx.update(|cx| match event {
                ControllerEvent::Offer(candidate) => {
                    close_window(offer_window.take(), cx);
                    offer_window = open_panel(
                        PanelKind::Offer(candidate),
                        ui.commands.clone(),
                        Some(OFFER_TIMEOUT),
                        cx,
                    )
                    .ok();
                }
                ControllerEvent::Starting => {
                    close_window(offer_window.take(), cx);
                    meeting_state = MeetingMenuState::Starting;
                    update_app_window(&ui.app_window, cx, |window, cx| window.meeting_starting(cx));
                }
                ControllerEvent::RecordingStarted { title } => {
                    meeting_state = MeetingMenuState::Recording {
                        started: Instant::now(),
                        title,
                    };
                    update_app_window(&ui.app_window, cx, |window, cx| window.meeting_started(cx));
                }
                ControllerEvent::Retitled { title } => {
                    if let MeetingMenuState::Recording { title: shown, .. } = &mut meeting_state {
                        *shown = title;
                    }
                    update_app_window(&ui.app_window, cx, |window, cx| {
                        window.meeting_updated_notes(cx);
                    });
                }
                ControllerEvent::CallEnded { app } => {
                    close_window(offer_window.take(), cx);
                    offer_window =
                        open_panel(PanelKind::CallEnded { app }, ui.commands.clone(), None, cx)
                            .ok();
                }
                ControllerEvent::Transcribing => {
                    close_window(offer_window.take(), cx);
                    meeting_state = MeetingMenuState::Transcribing;
                    update_app_window(&ui.app_window, cx, |window, cx| {
                        window.meeting_updated(cx);
                    });
                }
                ControllerEvent::Finished(manifest) => {
                    meeting_state = MeetingMenuState::Idle;
                    update_app_window(&ui.app_window, cx, |window, cx| {
                        window.meeting_finished(cx);
                    });
                    close_window(offer_window.take(), cx);
                    offer_window = open_panel(
                        PanelKind::Ready(manifest),
                        ui.commands.clone(),
                        Some(READY_TIMEOUT),
                        cx,
                    )
                    .ok();
                }
                ControllerEvent::NotesStarted => {
                    update_app_window(&ui.app_window, cx, |window, cx| {
                        window.meeting_updated_notes(cx);
                    });
                }
                ControllerEvent::NotesFinished(manifest) => {
                    if let Some(panel) = offer_window {
                        let _ = panel.update(cx, |panel, _, cx| {
                            if let PanelKind::Ready(ready) = &mut panel.kind
                                && ready.id == manifest.id
                            {
                                *ready = manifest.clone();
                                cx.notify();
                            }
                        });
                    }
                    update_app_window(&ui.app_window, cx, |window, cx| {
                        window.meeting_updated_notes(cx);
                    });
                }
                ControllerEvent::Failed { title, error } => {
                    meeting_state = MeetingMenuState::Idle;
                    ui.show_meeting_window(
                        offer_window.take(),
                        "could not open Hex for meeting failure",
                        cx,
                        |window, cx| window.meeting_failed(title, error, cx),
                    )
                }
            });
            if let Err(error) = result {
                tracing::error!(%error, "could not update meeting UI");
                return;
            }
            meeting_menu_refresh_at = Instant::now();
        }
        if ui.status_actions.is_some() && Instant::now() >= meeting_menu_refresh_at {
            let enabled = crate::app_settings::meetings_enabled();
            let _ = cx.update(|_| crate::status_item::update_meeting(&meeting_state, enabled));
            meeting_menu_refresh_at = Instant::now() + Duration::from_millis(500);
        }
        Timer::after(Duration::from_millis(16)).await;
    }
}

fn apply_developer_command(
    command: crate::developer_control::DeveloperCommand,
    ui: &MeetingUi,
    cx: &mut App,
) -> crate::developer_control::DeveloperReply {
    use crate::developer_control::{DeveloperCommand, DeveloperHudState, DeveloperReply};

    match command {
        DeveloperCommand::Status => {
            let pane = ui.app_window.borrow().as_ref().copied().and_then(|handle| {
                handle
                    .update(cx, |window, _, _| window.developer_pane())
                    .ok()
            });
            DeveloperReply::State {
                window_open: pane.is_some(),
                pane,
                commands_enabled: crate::app_settings::commands_enabled(),
            }
        }
        DeveloperCommand::Hud { state } => {
            ui.indicator.send(DictationIndicatorEvent::Reset);
            match state {
                DeveloperHudState::Reset => {}
                DeveloperHudState::Recording => {
                    ui.indicator.send(DictationIndicatorEvent::Started);
                    ui.indicator.send(DictationIndicatorEvent::Meter {
                        average: 0.08,
                        peak: 0.42,
                    });
                }
                DeveloperHudState::Transcribing => {
                    ui.indicator
                        .send(DictationIndicatorEvent::Submitted { job_id: 0 });
                    ui.indicator
                        .send(DictationIndicatorEvent::Transcribing { job_id: 0 });
                }
                DeveloperHudState::Processing => {
                    ui.indicator
                        .send(DictationIndicatorEvent::Submitted { job_id: 0 });
                    ui.indicator
                        .send(DictationIndicatorEvent::Processing { job_id: 0 });
                }
            }
            DeveloperReply::Ok
        }
        DeveloperCommand::ShowPane { pane } => {
            let handle = match ui.open_or_focus(cx) {
                Ok(handle) => handle,
                Err(error) => {
                    return DeveloperReply::error("open-window-failed", error.to_string());
                }
            };
            match handle.update(cx, |window, _, cx| window.developer_select_pane(pane, cx)) {
                Ok(()) => DeveloperReply::Ok,
                Err(error) => DeveloperReply::error("show-pane-failed", error.to_string()),
            }
        }
        DeveloperCommand::SetCommandsEnabled { enabled } => {
            let handle = ui.app_window.borrow().as_ref().copied();
            if let Some(handle) = handle
                && let Ok(result) = handle.update(cx, |window, _, cx| {
                    window.developer_set_commands_enabled(enabled, cx)
                })
            {
                return match result {
                    Ok(()) => DeveloperReply::Ok,
                    Err(error) => DeveloperReply::error("settings-save-failed", error),
                };
            }
            let mut settings = match AppSettings::load() {
                Ok(settings) => settings,
                Err(error) => {
                    return DeveloperReply::error("settings-load-failed", error.to_string());
                }
            };
            if let Err(error) = settings.set_commands_enabled(enabled) {
                return DeveloperReply::error("settings-conflict", error.to_string());
            }
            match settings.save() {
                Ok(()) => DeveloperReply::Ok,
                Err(error) => DeveloperReply::error("settings-save-failed", error.to_string()),
            }
        }
    }
}

fn controller_loop(
    shutdown: &AtomicBool,
    events: SyncSender<ControllerEvent>,
    commands: Receiver<ControllerCommand>,
    meeting_requests: Receiver<MeetingRequest>,
    project_root: PathBuf,
) -> Result<()> {
    let mut detector = MeetingDetector::default();
    let mut recording: Option<ActiveRecording> = None;
    let mut observed_microphone_apps = Vec::new();
    tracing::info!("GPUI meeting watcher started");

    while !shutdown.load(Ordering::Relaxed) {
        let enabled = crate::app_settings::meetings_enabled();
        let requested_commands = meeting_requests
            .try_iter()
            .take(8)
            .map(|request| match request {
                MeetingRequest::Start => ControllerCommand::Record(MeetingCandidate::manual()),
                MeetingRequest::Stop => ControllerCommand::Stop,
                MeetingRequest::Summarize(id) => ControllerCommand::Summarize(id),
            });
        for command in commands.try_iter().take(8).chain(requested_commands) {
            match command {
                ControllerCommand::Record(_) if !enabled => {
                    tracing::warn!("ignored a meeting recording request while meetings are off");
                }
                ControllerCommand::Record(candidate) if recording.is_none() => {
                    let stop = Arc::new(AtomicBool::new(false));
                    let worker_stop = stop.clone();
                    let (update_sender, updates) = mpsc::sync_channel(4);
                    let start = candidate.recording_start();
                    let worker_project_root = project_root.clone();
                    let worker = thread::spawn(move || {
                        meeting::record(
                            start,
                            &worker_stop,
                            &worker_project_root,
                            Some(update_sender),
                        )
                    });
                    let _ = events.send(ControllerEvent::Starting);
                    recording = Some(ActiveRecording {
                        title: candidate.title,
                        application: candidate.application,
                        app_label: candidate.source.label().into(),
                        presence: CallPresence::default(),
                        stop,
                        updates,
                        reported_started: false,
                        worker,
                    });
                }
                ControllerCommand::Stop => {
                    if let Some(active) = &recording
                        && !active.stop.swap(true, Ordering::Relaxed)
                    {
                        let _ = events.send(ControllerEvent::Transcribing);
                    }
                }
                ControllerCommand::Summarize(id) => start_notes(id, &events),
                ControllerCommand::Dismiss | ControllerCommand::Record(_) => {}
            }
        }

        if let Some(active) = &mut recording {
            for update in active.updates.try_iter() {
                match update {
                    meeting::RecordingUpdate::Started { title } => {
                        active.reported_started = true;
                        active.title = title.clone();
                        if !active.stop.load(Ordering::Relaxed) {
                            let _ = events.send(ControllerEvent::RecordingStarted { title });
                        }
                    }
                    meeting::RecordingUpdate::Retitled { title } => {
                        active.title = title.clone();
                        let _ = events.send(ControllerEvent::Retitled { title });
                    }
                }
            }
        }

        if recording
            .as_ref()
            .is_some_and(|active| active.worker.is_finished())
        {
            let active = recording
                .take()
                .ok_or_else(|| eyre!("recording disappeared"))?;
            let event = match active.worker.join() {
                Ok(Ok(manifest)) => {
                    if manifest.status == meeting::MeetingStatus::Complete
                        && meeting::notes_wanted()
                    {
                        start_notes(manifest.id.clone(), &events);
                    }
                    ControllerEvent::Finished(Box::new(manifest))
                }
                Ok(Err(error)) => ControllerEvent::Failed {
                    title: active.title,
                    error: error.to_string(),
                },
                Err(_) => ControllerEvent::Failed {
                    title: active.title,
                    error: "meeting recorder panicked".into(),
                },
            };
            let _ = events.send(event);
        }

        if let Some(active) = &mut recording
            && active.reported_started
            && !active.stop.load(Ordering::Relaxed)
            && let Some(application) = active.application.as_deref()
        {
            // A failed query must not end the controller while it owns a recording.
            let present = match active_microphone_applications() {
                Ok(applications) => applications
                    .iter()
                    .any(|candidate| candidate.canonical_bundle_id() == Some(application)),
                Err(error) => {
                    tracing::warn!(%error, "could not inspect microphone activity during a meeting");
                    true
                }
            };
            if active.presence.observe(present) {
                tracing::info!(application, "meeting application released the microphone");
                let _ = events.send(ControllerEvent::CallEnded {
                    app: active.app_label.clone(),
                });
            }
        }

        if recording.is_none() && enabled {
            let microphone_apps = match active_microphone_applications() {
                Ok(applications) => applications,
                Err(error) => {
                    tracing::warn!(%error, "could not inspect microphone activity");
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };
            let observed = microphone_apps
                .iter()
                .map(|application| (application.pid, application.bundle_id.clone()))
                .collect::<Vec<_>>();
            if observed != observed_microphone_apps {
                let applications = microphone_apps
                    .iter()
                    .map(|application| {
                        format!(
                            "{} ({}, pid {})",
                            application.name, application.bundle_id, application.pid
                        )
                    })
                    .collect::<Vec<_>>();
                tracing::info!(?applications, "microphone application activity changed");
                observed_microphone_apps = observed;
            }
            let application = microphone_apps
                .into_iter()
                .filter(|application| application.is_supported_meeting_app())
                .min_by_key(|application| application.detection_priority());
            let context = if application
                .as_ref()
                .is_some_and(|application| application.is_browser())
            {
                ContextSnapshot::capture().unwrap_or_default()
            } else {
                ContextSnapshot::default()
            };
            let candidate = application
                .and_then(|application| candidate_from_microphone(&application, &context));
            if let Some(candidate) = detector.observe(candidate) {
                tracing::info!(
                    source = candidate.source.label(),
                    title = candidate.title,
                    "meeting recording offer triggered"
                );
                let _ = events.send(ControllerEvent::Offer(candidate));
            }
        }
        thread::sleep(Duration::from_millis(500));
    }

    if let Some(active) = recording {
        active.stop.store(true, Ordering::Relaxed);
        active
            .worker
            .join()
            .map_err(|_| eyre!("meeting recorder panicked"))??;
    }
    Ok(())
}

/// Writes notes on a background thread; the UI refreshes from the manifest
/// whether generation succeeds or fails.
fn start_notes(id: String, events: &SyncSender<ControllerEvent>) {
    let finished = events.clone();
    let reload_id = id.clone();
    if meeting::spawn_summary(id, move |result| {
        let manifest = match result {
            Ok(manifest) => Some(manifest),
            Err(_) => meeting::list()
                .ok()
                .and_then(|meetings| meetings.into_iter().find(|item| item.id == reload_id)),
        };
        if let Some(manifest) = manifest {
            let _ = finished.send(ControllerEvent::NotesFinished(Box::new(manifest)));
        }
    }) {
        let _ = events.send(ControllerEvent::NotesStarted);
    }
}

fn panel_options(cx: &App) -> WindowOptions {
    let window_size = size(px(PANEL_WIDTH), px(PANEL_HEIGHT));
    let (display_id, bounds) = cx.primary_display().map_or_else(
        || (None, Bounds::centered(None, window_size, cx)),
        |display| {
            // Below the menu bar, beside the status items like a notification.
            let origin = display.bounds().top_right()
                - point(px(PANEL_WIDTH + WINDOW_MARGIN), px(-(WINDOW_MARGIN + 28.0)));
            (
                Some(display.id()),
                Bounds {
                    origin,
                    size: window_size,
                },
            )
        },
    );
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        display_id,
        titlebar: None,
        window_background: WindowBackgroundAppearance::Transparent,
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    }
}

/// Opens a focus-free capsule. A timed panel closes itself; an expired offer
/// is simply dismissed, and nothing starts or stops on a timeout.
fn open_panel(
    kind: PanelKind,
    commands: SyncSender<ControllerCommand>,
    timeout: Option<Duration>,
    cx: &mut App,
) -> gpui::Result<WindowHandle<MeetingPanel>> {
    let handle = cx.open_window(panel_options(cx), |_, cx| {
        cx.new(|_| MeetingPanel {
            kind,
            commands,
            copied: false,
        })
    })?;
    if let Some(timeout) = timeout {
        cx.spawn(async move |cx| {
            Timer::after(timeout).await;
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        })
        .detach();
    }
    Ok(handle)
}

fn close_window<T: Render + 'static>(window: Option<WindowHandle<T>>, cx: &mut App) {
    if let Some(window) = window {
        window
            .update(cx, |_, window, _| window.remove_window())
            .ok();
    }
}

fn update_app_window(
    app_window: &Rc<RefCell<Option<WindowHandle<AppWindow>>>>,
    cx: &mut App,
    update: impl FnOnce(&mut AppWindow, &mut Context<AppWindow>),
) {
    if let Some(window) = app_window.borrow().as_ref().copied() {
        window.update(cx, |window, _, cx| update(window, cx)).ok();
    }
}

enum PanelKind {
    Offer(MeetingCandidate),
    CallEnded { app: String },
    Ready(Box<meeting::MeetingManifest>),
}

struct MeetingPanel {
    kind: PanelKind,
    commands: SyncSender<ControllerCommand>,
    copied: bool,
}

impl Render for MeetingPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (dot, title, subtitle, actions) = match &self.kind {
            PanelKind::Offer(candidate) => {
                let dismiss = self.commands.clone();
                let record = self.commands.clone();
                let candidate = candidate.clone();
                let is_preview = candidate.key == "preview";
                let title = if candidate.named {
                    candidate.title.clone()
                } else {
                    format!("{} call detected", candidate.source.label())
                };
                (
                    TEXT_SECONDARY,
                    title,
                    "Record locally? Tell participants.".to_string(),
                    div()
                        .flex()
                        .gap_2()
                        .child(capsule_button(
                            ButtonKind::Secondary,
                            "not-now",
                            "Not now",
                            move |window, _| {
                                let _ = dismiss.try_send(ControllerCommand::Dismiss);
                                window.remove_window();
                            },
                        ))
                        .child(capsule_button(
                            ButtonKind::Primary,
                            "record",
                            "Record",
                            move |window, _| {
                                let command = if is_preview {
                                    ControllerCommand::Dismiss
                                } else {
                                    ControllerCommand::Record(candidate.clone())
                                };
                                if record.try_send(command).is_ok() {
                                    window.remove_window();
                                }
                            },
                        )),
                )
            }
            PanelKind::CallEnded { app } => {
                let stop = self.commands.clone();
                (
                    DANGER,
                    format!("{app} left the call"),
                    "Hex is still recording.".into(),
                    div()
                        .flex()
                        .gap_2()
                        .child(capsule_button(
                            ButtonKind::Secondary,
                            "keep-recording",
                            "Keep recording",
                            |window, _| {
                                window.remove_window();
                            },
                        ))
                        .child(capsule_button(
                            ButtonKind::Primary,
                            "stop-meeting",
                            "Stop & transcribe",
                            move |window, _| {
                                if stop.try_send(ControllerCommand::Stop).is_ok() {
                                    window.remove_window();
                                }
                            },
                        )),
                )
            }
            PanelKind::Ready(manifest) => {
                let path = manifest.export_path.clone();
                let reveal_path = path.clone();
                let copy_path = path.clone();
                let entity = cx.entity();
                let summary = manifest
                    .notes
                    .as_ref()
                    .map(|notes| notes.summary.trim().to_string())
                    .filter(|summary| !summary.is_empty());
                let subtitle = match (&path, &manifest.export_error) {
                    _ if meeting::notes_in_progress(&manifest.id) => {
                        "Summarizing with Apple Intelligence…".into()
                    }
                    _ if summary.is_some() => summary.unwrap_or_default(),
                    (Some(path), _) => path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    (None, Some(error)) => format!("Markdown not saved: {error}"),
                    (None, None) => "Saved in Meetings".into(),
                };
                let title = meeting::display_title(manifest);
                let title = if title == manifest.title && manifest.notes.is_none() {
                    "Meeting transcript ready".into()
                } else {
                    format!("Ready · {title}")
                };
                (
                    SUCCESS,
                    title,
                    subtitle,
                    div()
                        .flex()
                        .gap_2()
                        .when_some(copy_path, |actions, path| {
                            let label = if self.copied { "Copied" } else { "Copy" };
                            actions.child(capsule_button(
                                ButtonKind::Secondary,
                                "copy-transcript",
                                label,
                                move |_, cx| match std::fs::read_to_string(&path) {
                                    Ok(markdown) => {
                                        cx.write_to_clipboard(ClipboardItem::new_string(markdown));
                                        entity.update(cx, |panel, cx| {
                                            panel.copied = true;
                                            cx.notify();
                                        });
                                    }
                                    Err(error) => {
                                        tracing::warn!(%error, "could not copy meeting transcript")
                                    }
                                },
                            ))
                        })
                        .when_some(reveal_path, |actions, path| {
                            actions.child(capsule_button(
                                ButtonKind::Primary,
                                "reveal-transcript",
                                "Reveal",
                                move |window, _| {
                                    meeting::reveal(&path);
                                    window.remove_window();
                                },
                            ))
                        }),
                )
            }
        };
        div().size_full().p_1().child(
            div()
                .size_full()
                .flex()
                .items_center()
                .gap_3()
                .pl_4()
                .pr_2()
                .bg(rgb(CANVAS))
                .border_1()
                .border_color(rgb(LINE_STRONG))
                .rounded_full()
                .shadow_lg()
                .overflow_hidden()
                .child(status_dot(dot).size_2())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            truncated_line(BODY, title)
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(TEXT_PRIMARY)),
                        )
                        .child(truncated_line(CAPTION, subtitle).text_color(rgb(TEXT_TERTIARY))),
                )
                .child(actions.flex_none()),
        )
    }
}

/// Capsule buttons are the shared buttons in the capsule's pill shape.
fn capsule_button(
    kind: ButtonKind,
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    button(kind, label)
        .id(id)
        .rounded_full()
        .on_click(move |_, window, cx| on_click(window, cx))
}

#[cfg(debug_assertions)]
pub fn probe() -> Result<()> {
    for application in active_microphone_applications()? {
        println!(
            "{}\t{}\t{}\t{}",
            application.pid,
            if application.is_supported_meeting_app() {
                "meeting"
            } else {
                "other"
            },
            application.bundle_id,
            application.name
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dockless_startup_stays_quiet_only_when_setup_and_menu_bar_are_ready() {
        for setup_ready in [false, true] {
            for onboarding_completed in [false, true] {
                for status_item_available in [false, true] {
                    assert_eq!(
                        should_open_app_on_launch(
                            false,
                            setup_ready,
                            onboarding_completed,
                            status_item_available,
                        ),
                        !(setup_ready && onboarding_completed && status_item_available),
                        "setup_ready={setup_ready}, onboarding_completed={onboarding_completed}, status_item_available={status_item_available}",
                    );
                }
            }
        }
    }

    #[test]
    fn dock_visible_startup_always_opens_the_app() {
        for setup_ready in [false, true] {
            for onboarding_completed in [false, true] {
                for status_item_available in [false, true] {
                    assert!(should_open_app_on_launch(
                        true,
                        setup_ready,
                        onboarding_completed,
                        status_item_available,
                    ));
                }
            }
        }
    }
}

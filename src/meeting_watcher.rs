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

use crate::app_settings::{AppSettings, MeetingIndicatorCorner};
use crate::app_window::AppWindow;
use crate::context::ContextSnapshot;
use crate::design_tokens::*;
use crate::desktop_ui::{
    ButtonKind, Icon, Tone, button, icon_button, small_button, status_dot, tag, truncated_line,
};
use crate::dictation_indicator::DictationIndicatorEvent;
use crate::dictation_indicator::{self, DictationIndicatorUi};
use crate::meeting::{self, MeetingRequest};
use crate::meeting_detection::{
    CallPresence, MeetingCandidate, MeetingDetector, MeetingSource, candidate_from_microphone,
};
use crate::microphone_activity::active_microphone_applications;
use crate::motion::WindowMotion;
use crate::transcription_preparation::TranscriptionPreparation;
use crate::{config, recognition};

const PANEL_WIDTH: f32 = MEETING_PANEL_WIDTH;
const PANEL_HEIGHT: f32 = MEETING_PANEL_HEIGHT;
const WINDOW_MARGIN: f32 = 12.0;
const OFFER_TIMEOUT: Duration = Duration::from_secs(20);
const READY_TIMEOUT: Duration = Duration::from_secs(15);
const COPIED_FEEDBACK_DURATION: Duration = Duration::from_millis(1_500);
const RECORDING_TRANSCRIPT_POLL: Duration = Duration::from_millis(400);

enum ControllerCommand {
    Record(MeetingCandidate),
    Dismiss,
    Stop,
    Open(String),
    Summarize(String),
}

enum ControllerEvent {
    Offer(MeetingCandidate),
    Starting,
    RecordingStarted { id: String, title: String },
    Retitled { title: String },
    OpenMeeting(String),
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
    /// The offer with its "Never offer" confirmation open.
    OfferMore,
    /// The offer, dismissed after three seconds the way "Not now" closes it.
    OfferDismissed,
    Recording,
    RecordingExpanded,
    RecordingCopied,
    CallEnded,
    Ready,
}

fn run_panel_preview(preview: PanelPreview) -> Result<()> {
    let kind = match preview {
        PanelPreview::Offer | PanelPreview::OfferMore | PanelPreview::OfferDismissed => {
            PanelKind::Offer(MeetingCandidate {
                key: "preview".into(),
                title: "Browser call".into(),
                source: MeetingSource::BrowserCall,
                application: Some("com.brave.Browser".into()),
                named: false,
                site: Some("meet.jit.si".into()),
                application_name: Some("Brave Browser".into()),
            })
        }
        PanelPreview::Recording
        | PanelPreview::RecordingExpanded
        | PanelPreview::RecordingCopied => {
            let mut state = RecordingPanelState::preview(
                matches!(
                    preview,
                    PanelPreview::RecordingExpanded | PanelPreview::RecordingCopied
                ),
                matches!(preview, PanelPreview::RecordingCopied),
            );
            if matches!(preview, PanelPreview::RecordingCopied) {
                state.pending_turns = 0;
            }
            PanelKind::Recording(Box::new(state))
        }
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
    let (commands, receiver) = mpsc::sync_channel(8);
    Application::new().run(move |cx| {
        cx.bind_keys([KeyBinding::new(
            "cmd-q",
            crate::app_window::QuitApplication,
            None,
        )]);
        cx.on_action(|_: &crate::app_window::QuitApplication, cx| cx.quit());
        // Open and Reveal take the same cross-thread path to the app window as
        // the live capsules, so the preview exercises opening it from a panel.
        cx.spawn(async move |cx| {
            let app_window = Rc::new(RefCell::new(None));
            let (meeting_requests, _meeting_receiver) = mpsc::sync_channel(8);
            let (indicator, _indicator_receiver) = dictation_indicator::channel();
            loop {
                while let Ok(command) = receiver.try_recv() {
                    if !matches!(command, ControllerCommand::Open(_)) {
                        continue;
                    }
                    let result = cx.update(|cx| {
                        crate::app_window::open_preview(
                            &app_window,
                            std::env::temp_dir().join("hex-panel-preview.ndjson"),
                            config::voice_control(),
                            meeting_requests.clone(),
                            indicator.clone(),
                            crate::app_window::AppWindowPreview::pane(
                                crate::developer_control::DeveloperPane::Meetings,
                            ),
                            cx,
                        )
                    });
                    if let Ok(Err(error)) = result {
                        tracing::error!(%error, "could not open Hex from the panel preview");
                    }
                }
                Timer::after(Duration::from_millis(16)).await;
            }
        })
        .detach();
        let deadline = match preview {
            PanelPreview::Offer | PanelPreview::OfferMore => Some(OFFER_TIMEOUT * 30),
            PanelPreview::OfferDismissed => Some(Duration::from_secs(3)),
            _ => None,
        };
        match open_panel(kind, commands, deadline, cx) {
            Ok(handle) if matches!(preview, PanelPreview::OfferMore) => {
                let _ = handle.update(cx, |panel, _, cx| {
                    panel.more_open = true;
                    cx.notify();
                });
            }
            Ok(_) => {}
            Err(error) => {
                tracing::error!(%error, "could not open meeting panel preview");
                cx.quit();
            }
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
        let fast_hud = std::env::var("HEX_PREVIEW_CHOREOGRAPHY").ok().as_deref() == Some("hud");
        thread::spawn(move || {
            if fast_hud {
                thread::sleep(Duration::from_millis(120));
                preview_sender.send(DictationIndicatorEvent::Started);
                let started = Instant::now();
                while started.elapsed() < Duration::from_millis(1350) {
                    let wave = (started.elapsed().as_secs_f32() * 7.5).sin() * 0.5 + 0.5;
                    preview_sender.send(DictationIndicatorEvent::Meter {
                        average: 0.03 + wave * 0.09,
                        peak: 0.18 + wave * 0.58,
                    });
                    thread::sleep(Duration::from_millis(20));
                }
                preview_sender.send(DictationIndicatorEvent::Submitted { job_id: 0 });
                preview_sender.send(DictationIndicatorEvent::Transcribing { job_id: 0 });
                thread::sleep(Duration::from_millis(750));
                preview_sender.send(DictationIndicatorEvent::Processing { job_id: 0 });
                thread::sleep(Duration::from_millis(750));
                preview_sender.send(DictationIndicatorEvent::JobCompleted { job_id: 0 });
                thread::sleep(Duration::from_millis(650));
                preview_sender.send(DictationIndicatorEvent::Started);
                let second = Instant::now();
                while second.elapsed() < Duration::from_millis(800) {
                    let wave = (second.elapsed().as_secs_f32() * 8.0).sin() * 0.5 + 0.5;
                    preview_sender.send(DictationIndicatorEvent::Meter {
                        average: 0.03 + wave * 0.08,
                        peak: 0.16 + wave * 0.52,
                    });
                    thread::sleep(Duration::from_millis(20));
                }
                return;
            }
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
    let controller_worker = (!shell_preview.is_some() && !dictation_preview).then(|| {
        let controller_events = event_sender.clone();
        thread::spawn(move || {
            if preview {
                let _ = controller_events.send(ControllerEvent::Offer(MeetingCandidate {
                    key: "preview".into(),
                    title: "Design sync".into(),
                    source: MeetingSource::Zoom,
                    application: None,
                    named: true,
                    site: None,
                    application_name: None,
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
                    app_window: {
                        SETTINGS_WINDOW.with(|slot| *slot.borrow_mut() = Some(app_window.clone()));
                        app_window.clone()
                    },
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
    let mut recording_window: Option<WindowHandle<MeetingPanel>> = None;
    let mut active_recording: Option<(String, Instant, String)> = None;
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
                crate::status_item::StatusItemAction::CopyLastMeetingTranscript => {
                    match meeting::latest_transcript_markdown() {
                        Ok(markdown) => cx.write_to_clipboard(ClipboardItem::new_string(markdown)),
                        Err(error) => {
                            tracing::warn!(%error, "could not copy last meeting transcript")
                        }
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
                    match open_panel(
                        PanelKind::Offer(candidate.clone()),
                        ui.commands.clone(),
                        Some(OFFER_TIMEOUT),
                        cx,
                    ) {
                        Ok(window) => {
                            tracing::info!(
                                source = candidate.source.label(),
                                title = %candidate.title,
                                application = ?candidate.application,
                                "meeting recording offer shown"
                            );
                            offer_window = Some(window);
                        }
                        Err(error) => {
                            tracing::error!(%error, "could not open meeting offer capsule");
                        }
                    }
                }
                ControllerEvent::Starting => {
                    close_window(offer_window.take(), cx);
                    close_window(recording_window.take(), cx);
                    active_recording = None;
                    meeting_state = MeetingMenuState::Starting;
                    update_app_window(&ui.app_window, cx, |window, cx| window.meeting_starting(cx));
                }
                ControllerEvent::RecordingStarted { id, title } => {
                    let started = Instant::now();
                    active_recording = Some((id.clone(), started, title.clone()));
                    meeting_state = MeetingMenuState::Recording {
                        started,
                        title: title.clone(),
                    };
                    close_window(recording_window.take(), cx);
                    if crate::app_settings::meeting_indicator_enabled() {
                        recording_window = open_panel(
                            PanelKind::Recording(Box::new(RecordingPanelState::new(
                                id, title, started,
                            ))),
                            ui.commands.clone(),
                            None,
                            cx,
                        )
                        .ok();
                    }
                    update_app_window(&ui.app_window, cx, |window, cx| window.meeting_started(cx));
                }
                ControllerEvent::Retitled { title } => {
                    if let MeetingMenuState::Recording { title: shown, .. } = &mut meeting_state {
                        *shown = title.clone();
                    }
                    if let Some((_, _, active_title)) = &mut active_recording {
                        *active_title = title.clone();
                    }
                    if let Some(panel) = recording_window {
                        let _ = panel.update(cx, |panel, _, cx| {
                            if let PanelKind::Recording(state) = &mut panel.kind {
                                state.title = title.clone();
                                cx.notify();
                            }
                        });
                    }
                    update_app_window(&ui.app_window, cx, |window, cx| {
                        window.meeting_updated_notes(cx);
                    });
                }
                ControllerEvent::OpenMeeting(id) => match ui.open_or_focus(cx) {
                    Ok(handle) => {
                        let _ = handle.update(cx, |window, _, cx| window.open_meeting(id, cx));
                    }
                    Err(error) => tracing::error!(%error, "could not open Meetings from capsule"),
                },
                ControllerEvent::CallEnded { app } => {
                    close_window(offer_window.take(), cx);
                    close_window(recording_window.take(), cx);
                    offer_window =
                        open_panel(PanelKind::CallEnded { app }, ui.commands.clone(), None, cx)
                            .ok();
                }
                ControllerEvent::Transcribing => {
                    active_recording = None;
                    close_window(offer_window.take(), cx);
                    close_window(recording_window.take(), cx);
                    meeting_state = MeetingMenuState::Transcribing;
                    update_app_window(&ui.app_window, cx, |window, cx| {
                        window.meeting_updated(cx);
                    });
                }
                ControllerEvent::Finished(manifest) => {
                    active_recording = None;
                    meeting_state = MeetingMenuState::Idle;
                    update_app_window(&ui.app_window, cx, |window, cx| {
                        window.meeting_finished(cx);
                    });
                    close_window(offer_window.take(), cx);
                    close_window(recording_window.take(), cx);
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
                    active_recording = None;
                    close_window(recording_window.take(), cx);
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
        if active_recording.is_some() {
            let _ = cx.update(|cx| {
                if offer_window
                    .as_ref()
                    .is_some_and(|handle| handle.update(cx, |_, _, _| ()).is_err())
                {
                    offer_window = None;
                }
                let indicator_wanted =
                    crate::app_settings::meeting_indicator_enabled() && offer_window.is_none();
                if !indicator_wanted {
                    close_window(recording_window.take(), cx);
                } else if recording_window.is_none()
                    && let Some((id, started, title)) = &active_recording
                {
                    recording_window = open_panel(
                        PanelKind::Recording(Box::new(RecordingPanelState::new(
                            id.clone(),
                            title.clone(),
                            *started,
                        ))),
                        ui.commands.clone(),
                        None,
                        cx,
                    )
                    .ok();
                }
                if let Some(handle) = recording_window
                    && handle
                        .update(cx, |panel, window, cx| panel.tick_recording(window, cx))
                        .is_err()
                {
                    recording_window = None;
                }
            });
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
                ControllerCommand::Open(id) => {
                    let _ = events.send(ControllerEvent::OpenMeeting(id));
                }
                ControllerCommand::Summarize(id) => start_notes(id, &events),
                ControllerCommand::Dismiss | ControllerCommand::Record(_) => {}
            }
        }

        if let Some(active) = &mut recording {
            for update in active.updates.try_iter() {
                match update {
                    meeting::RecordingUpdate::Started { id, title } => {
                        active.reported_started = true;
                        active.title = title.clone();
                        if !active.stop.load(Ordering::Relaxed) {
                            let _ = events.send(ControllerEvent::RecordingStarted { id, title });
                        }
                    }
                    meeting::RecordingUpdate::Retitled { title } => {
                        active.title = title.clone();
                        let _ = events.send(ControllerEvent::Retitled { title });
                    }
                    meeting::RecordingUpdate::Transcribing => {
                        if !active.stop.swap(true, Ordering::Relaxed) {
                            let _ = events.send(ControllerEvent::Transcribing);
                        }
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
            let context = match application.as_ref().and_then(|application| {
                application
                    .is_browser()
                    .then(|| application.canonical_bundle_id())
                    .flatten()
                    .filter(|bundle_id| !detector.is_application_snoozed(bundle_id))
            }) {
                Some(bundle_id) => {
                    ContextSnapshot::capture_for_meeting(bundle_id).unwrap_or_default()
                }
                None => ContextSnapshot::default(),
            };
            let silenced = crate::app_settings::meeting_never_offer_keys();
            let candidate = application
                .and_then(|application| candidate_from_microphone(&application, &context))
                .filter(|candidate| !candidate.is_silenced(&silenced));
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

fn recording_panel_options(corner: MeetingIndicatorCorner, width: f32, cx: &App) -> WindowOptions {
    let window_size = size(px(width), px(RECORDING_PANEL_HEIGHT));
    let (display_id, bounds) = cx.primary_display().map_or_else(
        || (None, Bounds::centered(None, window_size, cx)),
        |display| {
            let bounds = display.bounds();
            let x = if corner.is_right() {
                bounds.right() - px(width + WINDOW_MARGIN)
            } else {
                bounds.left() + px(WINDOW_MARGIN)
            };
            let y = if corner.is_top() {
                bounds.top() + px(WINDOW_MARGIN + 28.0)
            } else {
                bounds.bottom() - px(RECORDING_PANEL_HEIGHT + WINDOW_MARGIN + 24.0)
            };
            (
                Some(display.id()),
                Bounds {
                    origin: point(x, y),
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

/// Turns a GPUI pop-up into a borderless, shadowless, fully transparent
/// non-activating panel. GPUI creates frameless windows as titled panels with
/// a near-clear background, so AppKit draws a rectangular window shadow around
/// the full window bounds, beneath the narrower animated capsule.
fn prepare_capsule_window(window: &Window) -> Option<objc2::rc::Retained<objc2_app_kit::NSWindow>> {
    use objc2_app_kit::{NSColor, NSView, NSWindowCollectionBehavior, NSWindowStyleMask};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // SAFETY: GPUI's AppKit handle points at the live content view of this
    // window, and this runs on the main thread inside a window callback.
    let view: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    let ns_window = view.window()?;
    ns_window.setStyleMask(
        NSWindowStyleMask::Borderless
            | NSWindowStyleMask::NonactivatingPanel
            | NSWindowStyleMask::FullSizeContentView,
    );
    ns_window.setOpaque(false);
    ns_window.setBackgroundColor(Some(&NSColor::clearColor()));
    ns_window.setHasShadow(false);
    ns_window.setHidesOnDeactivate(false);
    ns_window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    Some(ns_window)
}

fn appkit_corner_origin(
    corner: MeetingIndicatorCorner,
    screen_frame: objc2_foundation::NSRect,
    width: f64,
    height: f64,
) -> objc2_foundation::NSPoint {
    let margin = f64::from(WINDOW_MARGIN);
    let x = if corner.is_right() {
        screen_frame.origin.x + screen_frame.size.width - width - margin
    } else {
        screen_frame.origin.x + margin
    };
    let y = if corner.is_top() {
        screen_frame.origin.y + screen_frame.size.height - height - margin - 28.0
    } else {
        screen_frame.origin.y + margin + 24.0
    };
    objc2_foundation::NSPoint::new(x, y)
}

/// Opens a focus-free capsule. A timed panel closes itself; an expired offer
/// is simply dismissed, and nothing starts or stops on a timeout.
fn open_panel(
    mut kind: PanelKind,
    commands: SyncSender<ControllerCommand>,
    timeout: Option<Duration>,
    cx: &mut App,
) -> gpui::Result<WindowHandle<MeetingPanel>> {
    let options = match &kind {
        // Previews use the production window: full width, with the pill
        // inside it, so the transparent margin is visible in captures.
        PanelKind::Recording(state) => {
            recording_panel_options(state.corner, RECORDING_PANEL_WIDTH, cx)
        }
        PanelKind::Offer(_) | PanelKind::CallEnded { .. } | PanelKind::Ready(_) => {
            panel_options(cx)
        }
    };
    let handle = cx.open_window(options, |window, cx| {
        let ns_window = prepare_capsule_window(window);
        if let PanelKind::Recording(state) = &mut kind {
            state.ns_window = ns_window;
            let target = if state.hovered || state.force_expanded {
                RECORDING_PANEL_WIDTH
            } else {
                RECORDING_COMPACT_WIDTH
            };
            let _ = window.spring("recording-capsule-width", target, SPRING_INSTANT);
        }
        cx.new(|_| MeetingPanel {
            kind,
            commands,
            copied: false,
            more_open: false,
        })
    })?;
    if let Some(timeout) = timeout {
        cx.spawn(async move |cx| {
            Timer::after(timeout).await;
            let _ = handle.update(cx, |panel, window, _| {
                if let PanelKind::Offer(candidate) = &panel.kind {
                    tracing::info!(
                        source = candidate.source.label(),
                        title = %candidate.title,
                        "meeting recording offer timed out"
                    );
                }
                window.remove_window();
            });
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecordingCopyKind {
    New,
    All,
}

#[derive(Clone, Copy, Debug)]
struct RecordingDrag {
    start_mouse: objc2_foundation::NSPoint,
    start_origin: objc2_foundation::NSPoint,
}

struct RecordingPanelState {
    id: String,
    title: String,
    started: Instant,
    corner: MeetingIndicatorCorner,
    hovered: bool,
    force_expanded: bool,
    preview: bool,
    pending_turns: usize,
    has_transcript: bool,
    copied: Option<(RecordingCopyKind, Instant)>,
    last_refresh_at: Instant,
    last_second: u64,
    transcript_reader: meeting::TranscriptReader,
    drag: Option<RecordingDrag>,
    ns_window: Option<objc2::rc::Retained<objc2_app_kit::NSWindow>>,
}

impl RecordingPanelState {
    fn new(id: String, title: String, started: Instant) -> Self {
        let mut transcript_reader = meeting::TranscriptReader::default();
        let pending_turns = meeting::pending_delta_turns_for_id(&id).unwrap_or(0);
        let has_transcript = pending_turns > 0
            || meeting::live_transcript_turns(&id, &mut transcript_reader)
                .is_ok_and(|turns| !turns.is_empty());
        Self {
            id,
            title,
            started,
            corner: crate::app_settings::meeting_indicator_corner(),
            hovered: false,
            force_expanded: false,
            preview: false,
            pending_turns,
            has_transcript,
            copied: None,
            last_refresh_at: Instant::now(),
            last_second: started.elapsed().as_secs(),
            transcript_reader,
            drag: None,
            ns_window: None,
        }
    }

    fn preview(expanded: bool, copied: bool) -> Self {
        let started = Instant::now() - Duration::from_secs(258);
        Self {
            id: "preview-live".into(),
            title: "Q4 launch sync".into(),
            started,
            corner: MeetingIndicatorCorner::TopRight,
            hovered: expanded,
            force_expanded: expanded,
            preview: true,
            pending_turns: if copied { 0 } else { 3 },
            has_transcript: true,
            copied: copied.then(|| (RecordingCopyKind::New, Instant::now())),
            last_refresh_at: Instant::now(),
            last_second: started.elapsed().as_secs(),
            transcript_reader: meeting::TranscriptReader::default(),
            drag: None,
            ns_window: None,
        }
    }

    fn refresh_counts(&mut self) -> bool {
        if self.preview {
            return false;
        }
        let next_pending = meeting::pending_delta_turns_for_id(&self.id).unwrap_or(0);
        let next_has_transcript = next_pending > 0
            || meeting::live_transcript_turns(&self.id, &mut self.transcript_reader)
                .is_ok_and(|turns| !turns.is_empty());
        let changed =
            self.pending_turns != next_pending || self.has_transcript != next_has_transcript;
        self.pending_turns = next_pending;
        self.has_transcript = next_has_transcript;
        self.last_refresh_at = Instant::now();
        changed
    }
}

enum PanelKind {
    Offer(MeetingCandidate),
    Recording(Box<RecordingPanelState>),
    CallEnded { app: String },
    Ready(Box<meeting::MeetingManifest>),
}

struct MeetingPanel {
    kind: PanelKind,
    commands: SyncSender<ControllerCommand>,
    copied: bool,
    /// The offer shows its "Never offer" confirmation in place of its actions.
    more_open: bool,
}

type SharedAppWindow = Rc<RefCell<Option<WindowHandle<AppWindow>>>>;

thread_local! {
    /// The Settings window, so a "Never offer" choice updates its live copy
    /// of the settings instead of being overwritten by its next save.
    static SETTINGS_WINDOW: RefCell<Option<SharedAppWindow>> = const { RefCell::new(None) };
}

fn never_offer(entry: crate::app_settings::NeverOfferMeeting, cx: &mut App) {
    let window = SETTINGS_WINDOW.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|window| window.borrow().as_ref().copied())
    });
    let routed = window.is_some_and(|window| {
        let entry = entry.clone();
        window
            .update(cx, |window, _, cx| window.never_offer_meeting(entry, cx))
            .is_ok()
    });
    if !routed {
        crate::app_settings::save_never_offer_meeting(entry);
    }
}

impl MeetingPanel {
    fn begin_drag(&mut self, cx: &mut Context<Self>) {
        let PanelKind::Recording(state) = &mut self.kind else {
            return;
        };
        if state.preview {
            return;
        }
        if let Some(ns_window) = &state.ns_window {
            let mouse = objc2_app_kit::NSEvent::mouseLocation();
            state.drag = Some(RecordingDrag {
                start_mouse: mouse,
                start_origin: ns_window.frame().origin,
            });
            state.hovered = true;
            cx.notify();
        }
    }

    fn copy_new_turns(&mut self, cx: &mut Context<Self>) {
        let PanelKind::Recording(state) = &mut self.kind else {
            return;
        };
        if state.pending_turns == 0 {
            return;
        }
        if state.preview {
            cx.write_to_clipboard(ClipboardItem::new_string(
                "You: Let's ship the floating recording indicator today.\n\nThem: Sounds good."
                    .into(),
            ));
            state.pending_turns = 0;
            state.copied = Some((RecordingCopyKind::New, Instant::now()));
            cx.notify();
            return;
        }
        let id = state.id.clone();
        match meeting::consume_delta_for_id(&id, |text| {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
            Ok(())
        }) {
            Ok(_) => {
                state.copied = Some((RecordingCopyKind::New, Instant::now()));
                let _ = state.refresh_counts();
                cx.notify();
            }
            Err(error) => {
                tracing::warn!(%error, "could not copy new meeting transcript turns");
            }
        }
    }

    fn copy_all_turns(&mut self, cx: &mut Context<Self>) {
        let PanelKind::Recording(state) = &mut self.kind else {
            return;
        };
        if !state.has_transcript {
            return;
        }
        if state.preview {
            cx.write_to_clipboard(ClipboardItem::new_string(
                "Them: Kickoff is at 2:30.\n\nYou: Let's ship the floating recording indicator today."
                    .into(),
            ));
            state.copied = Some((RecordingCopyKind::All, Instant::now()));
            cx.notify();
            return;
        }
        match meeting::live_transcript_turns(&state.id, &mut state.transcript_reader) {
            Ok(turns) if !turns.is_empty() => {
                let markdown = meeting::transcript_markdown(&turns);
                cx.write_to_clipboard(ClipboardItem::new_string(markdown));
                state.copied = Some((RecordingCopyKind::All, Instant::now()));
                cx.notify();
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, "could not copy full live meeting transcript");
            }
        }
    }

    fn tick_recording(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let PanelKind::Recording(state) = &mut self.kind else {
            return;
        };
        let mut dirty = false;
        if let Some((_, copied_at)) = state.copied
            && copied_at.elapsed() >= COPIED_FEEDBACK_DURATION
        {
            state.copied = None;
            dirty = true;
        }
        if state.last_refresh_at.elapsed() >= RECORDING_TRANSCRIPT_POLL && state.refresh_counts() {
            dirty = true;
        }
        let elapsed_secs = state.started.elapsed().as_secs();
        if elapsed_secs != state.last_second {
            state.last_second = elapsed_secs;
            dirty = true;
        }
        if !state.preview && !crate::motion::reduced_motion() {
            dirty = true;
        }
        if !state.preview
            && let Some(ns_window) = state.ns_window.clone()
        {
            let frame = ns_window.frame();
            let mouse = objc2_app_kit::NSEvent::mouseLocation();
            let pressed = objc2_app_kit::NSEvent::pressedMouseButtons();
            let left_down = (pressed & 1) != 0;
            if let Some(drag) = state.drag {
                if left_down {
                    let next = objc2_foundation::NSPoint::new(
                        drag.start_origin.x + (mouse.x - drag.start_mouse.x),
                        drag.start_origin.y + (mouse.y - drag.start_mouse.y),
                    );
                    ns_window.setFrameOrigin(next);
                    ns_window.setIgnoresMouseEvents(false);
                } else {
                    state.drag = None;
                    if let Some(mtm) = objc2::MainThreadMarker::new()
                        && let Some(screen) = ns_window
                            .screen()
                            .or_else(|| objc2_app_kit::NSScreen::mainScreen(mtm))
                    {
                        let screen_frame = screen.frame();
                        let center_x = frame.origin.x + frame.size.width * 0.5;
                        let center_y = frame.origin.y + frame.size.height * 0.5;
                        let right =
                            center_x >= screen_frame.origin.x + screen_frame.size.width * 0.5;
                        let top =
                            center_y >= screen_frame.origin.y + screen_frame.size.height * 0.5;
                        let corner = match (top, right) {
                            (true, true) => MeetingIndicatorCorner::TopRight,
                            (true, false) => MeetingIndicatorCorner::TopLeft,
                            (false, true) => MeetingIndicatorCorner::BottomRight,
                            (false, false) => MeetingIndicatorCorner::BottomLeft,
                        };
                        let snapped = appkit_corner_origin(
                            corner,
                            screen_frame,
                            frame.size.width,
                            frame.size.height,
                        );
                        ns_window.setFrameOrigin(snapped);
                        if state.corner != corner {
                            state.corner = corner;
                            crate::app_settings::save_meeting_indicator_corner(corner);
                        }
                    }
                    dirty = true;
                }
            }
            if state.drag.is_none() {
                let expanded = state.hovered || state.force_expanded;
                let (min_x, max_x) = if expanded {
                    (frame.origin.x, frame.origin.x + frame.size.width)
                } else if state.corner.is_right() {
                    (
                        frame.origin.x + (frame.size.width - f64::from(RECORDING_COMPACT_WIDTH)),
                        frame.origin.x + frame.size.width,
                    )
                } else {
                    (
                        frame.origin.x,
                        frame.origin.x + f64::from(RECORDING_COMPACT_WIDTH),
                    )
                };
                let inside = mouse.x >= min_x
                    && mouse.x <= max_x
                    && mouse.y >= frame.origin.y
                    && mouse.y <= frame.origin.y + frame.size.height;
                ns_window.setIgnoresMouseEvents(!inside);
                if !state.force_expanded && state.hovered != inside {
                    state.hovered = inside;
                    dirty = true;
                }
            }
        }
        if dirty {
            cx.notify();
        }
    }

    fn render_recording(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let PanelKind::Recording(state) = &self.kind else {
            unreachable!("render_recording requires PanelKind::Recording");
        };
        let expanded = state.hovered || state.force_expanded || state.drag.is_some();
        let target_width = if expanded {
            RECORDING_PANEL_WIDTH
        } else {
            RECORDING_COMPACT_WIDTH
        };
        let animated_width = window.spring("recording-capsule-width", target_width, SPRING_SNAP);
        let pill_width = if state.preview && !state.force_expanded {
            RECORDING_COMPACT_WIDTH - 8.0
        } else {
            (animated_width - 8.0).max(RECORDING_COMPACT_WIDTH - 8.0)
        };
        let pulse = if state.preview || crate::motion::reduced_motion() {
            1.0
        } else {
            0.725 + 0.275 * (state.started.elapsed().as_secs_f32() * std::f32::consts::PI).cos()
        };
        let elapsed = meeting::format_duration(state.started.elapsed().as_millis() as u64);
        let title = state.title.clone();
        let pending_turns = state.pending_turns;
        let has_transcript = state.has_transcript;
        let copied_new = matches!(state.copied, Some((RecordingCopyKind::New, _)));
        let copied_all = matches!(state.copied, Some((RecordingCopyKind::All, _)));
        let copy_new_enabled = pending_turns > 0 || copied_new;
        let copy_all_enabled = has_transcript || copied_all;
        let copy_new_label = if copied_new {
            "Copied".to_string()
        } else if pending_turns > 0 {
            format!("Copy new ({pending_turns})")
        } else {
            "Copy new".to_string()
        };
        let copy_all_label = if copied_all { "Copied" } else { "Copy all" };
        let open_id = state.id.clone();
        let open_commands = self.commands.clone();
        let stop_commands = self.commands.clone();
        let align_right = state.corner.is_right();

        div()
            .size_full()
            .p_1()
            .flex()
            .when(align_right, |row| row.justify_end())
            .when(!align_right, |row| row.justify_start())
            .child(
                div()
                    .id("recording-capsule")
                    .h_full()
                    .w(px(pill_width))
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .pl_3p5()
                    .pr_2()
                    .bg(rgb(CANVAS))
                    .border_1()
                    .border_color(rgb(LINE_STRONG))
                    .rounded_full()
                    .shadow_sm()
                    .overflow_hidden()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|panel, _, _, cx| panel.begin_drag(cx)),
                    )
                    .child(status_dot(DANGER).size_2().opacity(pulse))
                    .child(
                        div()
                            .flex_none()
                            .text_caption()
                            .text_monospace()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(rgb(TEXT_SECONDARY))
                            .child(elapsed),
                    )
                    .child(
                        div().flex_1().min_w_0().child(
                            truncated_line(FOOTNOTE, title)
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(TEXT_PRIMARY)),
                        ),
                    )
                    .when(!expanded && pending_turns > 0, |pill| {
                        pill.child(tag(Tone::Accent, format!("+{pending_turns}")))
                    })
                    .when(expanded, |pill| {
                        pill.child(
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    small_button(ButtonKind::Secondary, copy_new_label)
                                        .id("recording-copy-new")
                                        .w(px(RECORDING_COPY_NEW_WIDTH))
                                        .justify_center()
                                        .rounded_full()
                                        .when(!copy_new_enabled, |btn| {
                                            btn.opacity(DISABLED_OPACITY).cursor_default()
                                        })
                                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .on_click(cx.listener(|panel, _, _, cx| {
                                            panel.copy_new_turns(cx);
                                        })),
                                )
                                .child(
                                    small_button(ButtonKind::Secondary, copy_all_label)
                                        .id("recording-copy-all")
                                        .w(px(RECORDING_COPY_ALL_WIDTH))
                                        .justify_center()
                                        .rounded_full()
                                        .when(!copy_all_enabled, |btn| {
                                            btn.opacity(DISABLED_OPACITY).cursor_default()
                                        })
                                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .on_click(cx.listener(|panel, _, _, cx| {
                                            panel.copy_all_turns(cx);
                                        })),
                                )
                                .child(
                                    small_button(ButtonKind::Secondary, "Open")
                                        .id("recording-open")
                                        .rounded_full()
                                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .on_click(move |_, _, _| {
                                            let _ = open_commands
                                                .try_send(ControllerCommand::Open(open_id.clone()));
                                        }),
                                )
                                .child(
                                    small_button(ButtonKind::Secondary, "Stop")
                                        .id("recording-stop")
                                        .rounded_full()
                                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .on_click(move |_, window, _| {
                                            if stop_commands
                                                .try_send(ControllerCommand::Stop)
                                                .is_ok()
                                            {
                                                window.remove_window();
                                            }
                                        }),
                                ),
                        )
                    }),
            )
    }
}

impl Render for MeetingPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if matches!(self.kind, PanelKind::Recording(_)) {
            return self.render_recording(window, cx).into_any_element();
        }
        let (dot, title, subtitle, actions) = match &self.kind {
            PanelKind::Recording(_) => unreachable!("handled above"),
            PanelKind::Offer(candidate) if self.more_open => {
                let target = candidate.never_offer_target();
                let label = target
                    .as_ref()
                    .map_or_else(String::new, |target| target.label.clone());
                let entity = cx.entity();
                let dismiss = self.commands.clone();
                (
                    TEXT_SECONDARY,
                    format!("Never offer for {label}?"),
                    "You can undo this in Settings › Meetings.".to_string(),
                    div()
                        .flex()
                        .gap_2()
                        .child(capsule_button(
                            ButtonKind::Secondary,
                            "never-offer-cancel",
                            "Cancel",
                            move |_, cx| {
                                entity.update(cx, |panel, cx| {
                                    panel.more_open = false;
                                    cx.notify();
                                });
                            },
                        ))
                        .child(capsule_button(
                            ButtonKind::Primary,
                            "never-offer",
                            "Never offer",
                            move |window, cx| {
                                if let Some(target) = target.clone() {
                                    tracing::info!(key = %target.key, "meeting offers silenced");
                                    never_offer(target, cx);
                                }
                                let _ = dismiss.try_send(ControllerCommand::Dismiss);
                                window.remove_window();
                            },
                        )),
                )
            }
            PanelKind::Offer(candidate) => {
                let dismiss = self.commands.clone();
                let record = self.commands.clone();
                let candidate = candidate.clone();
                let dismissed_candidate = candidate.clone();
                let is_preview = candidate.key == "preview";
                let can_silence = candidate.never_offer_target().is_some();
                let entity = cx.entity();
                (
                    TEXT_SECONDARY,
                    candidate.offer_title(),
                    "Record locally? Tell participants.".to_string(),
                    div()
                        .flex()
                        .gap_2()
                        .when(can_silence, |actions| {
                            actions.child(
                                icon_button(ButtonKind::Ghost, Icon::Ellipsis)
                                    .id("offer-more")
                                    .rounded_full()
                                    .on_click(move |_, _, cx| {
                                        entity.update(cx, |panel, cx| {
                                            panel.more_open = true;
                                            cx.notify();
                                        });
                                    }),
                            )
                        })
                        .child(capsule_button(
                            ButtonKind::Secondary,
                            "not-now",
                            "Not now",
                            move |window, _| {
                                tracing::info!(
                                    source = dismissed_candidate.source.label(),
                                    title = %dismissed_candidate.title,
                                    "meeting recording offer dismissed"
                                );
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
                let copy_manifest = (**manifest).clone();
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
                let label = if self.copied { "Copied" } else { "Copy" };
                (
                    SUCCESS,
                    title,
                    subtitle,
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            button(ButtonKind::Secondary, label)
                                .id("copy-transcript")
                                .w(px(READY_COPY_WIDTH))
                                .justify_center()
                                .rounded_full()
                                .on_click(move |_, _, cx| {
                                    let markdown = if copy_manifest.id == "preview" {
                                        Ok("Them: The team set the Q4 launch for November 12.\n\nYou: I'll follow up on docs owners today.".into())
                                    } else {
                                        meeting::transcript_markdown_for_manifest(&copy_manifest)
                                    };
                                    match markdown {
                                        Ok(markdown) => {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                markdown,
                                            ));
                                            entity.update(cx, |panel, cx| {
                                                panel.copied = true;
                                                cx.notify();
                                            });
                                        }
                                        Err(error) => {
                                            tracing::warn!(%error, "could not copy meeting transcript")
                                        }
                                    }
                                }),
                        )
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
        div()
            .size_full()
            .p_1()
            .child(
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
                    .shadow_sm()
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
                            .child(
                                truncated_line(CAPTION, subtitle).text_color(rgb(TEXT_TERTIARY)),
                            ),
                    )
                    .child(actions.flex_none()),
            )
            .into_any_element()
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

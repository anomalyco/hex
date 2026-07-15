use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};
use gpui::{
    App, Application, Bounds, Context, IntoElement, Render, Rgba, Timer, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, div, point,
    prelude::*, px, rgb, size,
};

use crate::app_settings::AppSettings;
use crate::app_window::AppWindow;
use crate::context::ContextSnapshot;
use crate::dictation_indicator::DictationIndicatorEvent;
use crate::dictation_indicator::{self, DictationIndicatorUi};
use crate::meeting::{self, MeetingRequest};
use crate::meeting_detection::{
    MeetingCandidate, MeetingDetector, MeetingSource, candidate_from_microphone,
};
use crate::microphone_activity::active_microphone_applications;
use crate::{config, recognition};

const WINDOW_WIDTH: f32 = 344.0;
const WINDOW_HEIGHT: f32 = 144.0;
const WINDOW_MARGIN: f32 = 18.0;

enum ControllerCommand {
    Record(MeetingCandidate),
    Dismiss,
    Stop,
}

enum ControllerEvent {
    Offer(MeetingCandidate),
    Starting,
    RecordingStarted,
    Transcribing,
    Finished,
    Failed { title: String, error: String },
}

struct ActiveRecording {
    title: String,
    stop: Arc<AtomicBool>,
    started: Receiver<()>,
    reported_started: bool,
    worker: JoinHandle<Result<meeting::MeetingManifest>>,
}

struct MeetingUi {
    commands: SyncSender<ControllerCommand>,
    app_window: Rc<RefCell<Option<WindowHandle<AppWindow>>>>,
    event_path: PathBuf,
    meeting_requests: SyncSender<MeetingRequest>,
}

pub struct ListenerConfig {
    pub project_root: PathBuf,
    pub event_path: PathBuf,
    pub device: Option<String>,
}

pub fn run(
    shutdown: &'static AtomicBool,
    preview: bool,
    listener: Option<ListenerConfig>,
    dictation_preview: bool,
) -> Result<()> {
    shutdown.store(false, Ordering::Relaxed);
    crate::keyboard::initialize_layout()?;
    let settings = AppSettings::load()?;
    crate::feedback::set_enabled(settings.sound_effects);
    let app_event_path = listener
        .as_ref()
        .map(|listener| listener.event_path.clone());
    let meeting_project_root = listener
        .as_ref()
        .map(|listener| listener.project_root.clone())
        .unwrap_or(std::env::current_dir()?);
    let shell_event_path = app_event_path
        .clone()
        .unwrap_or_else(|| meeting_project_root.join("logs/live.ndjson"));
    let show_app_on_launch = app_event_path.is_some() && !dictation_preview;
    let (event_sender, event_receiver) = mpsc::sync_channel(32);
    let (command_sender, command_receiver) = mpsc::sync_channel(8);
    let (meeting_request_sender, meeting_request_receiver) = mpsc::sync_channel(8);
    let (indicator_sender, indicator_receiver) = dictation_indicator::channel();
    if dictation_preview {
        let preview_sender = indicator_sender.clone();
        thread::spawn(move || {
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
            preview_sender.send(DictationIndicatorEvent::Transcribing);
            thread::sleep(Duration::from_secs(2));
            preview_sender.send(DictationIndicatorEvent::Completed);
        });
    }
    let controller_events = event_sender.clone();
    let controller_worker = thread::spawn(move || {
        if preview {
            let _ = controller_events.send(ControllerEvent::Offer(MeetingCandidate {
                key: "preview".into(),
                title: "Design sync".into(),
                source: MeetingSource::Zoom,
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
    });

    let recognition_meeting_requests = meeting_request_sender.clone();
    let recognition_worker = listener.map(|listener| {
        let failure_indicator = indicator_sender.clone();
        let meeting_requests = recognition_meeting_requests.clone();
        thread::spawn(move || {
            if let Err(error) = recognition::listen(
                &listener.project_root,
                &listener.event_path,
                listener.device.as_deref(),
                config::voice_control(),
                shutdown,
                Some(indicator_sender),
                Some(meeting_requests),
            ) {
                tracing::error!(%error, "desktop recognition stopped");
                failure_indicator.send(crate::dictation_indicator::DictationIndicatorEvent::Failed);
            }
        })
    });

    let app_window = Rc::new(RefCell::new(None::<WindowHandle<AppWindow>>));
    let reopen_window = app_window.clone();
    let reopen_event_path = shell_event_path.clone();
    let reopen_meeting_requests = meeting_request_sender.clone();
    let application = Application::new();
    application.on_reopen(move |cx| {
        if let Err(error) = crate::app_window::open_or_focus(
            &reopen_window,
            reopen_event_path.clone(),
            config::voice_control(),
            reopen_meeting_requests.clone(),
            cx,
        ) {
            tracing::error!(%error, "could not open HEX");
        }
    });
    application.run(move |cx| {
        if show_app_on_launch
            && let Some(event_path) = app_event_path
            && let Err(error) = crate::app_window::open_or_focus(
                &app_window,
                event_path,
                config::voice_control(),
                meeting_request_sender.clone(),
                cx,
            )
        {
            tracing::error!(%error, "could not open HEX");
        }
        let quit_shutdown = shutdown;
        cx.on_app_quit(move |_| {
            quit_shutdown.store(true, Ordering::Relaxed);
            async {}
        })
        .detach();

        cx.spawn(async move |cx| {
            drive_ui(
                event_receiver,
                indicator_receiver,
                MeetingUi {
                    commands: command_sender,
                    app_window: app_window.clone(),
                    event_path: shell_event_path.clone(),
                    meeting_requests: meeting_request_sender.clone(),
                },
                shutdown,
                cx,
            )
            .await;
        })
        .detach();
    });
    shutdown.store(true, Ordering::Relaxed);
    if let Some(worker) = recognition_worker {
        worker
            .join()
            .map_err(|_| eyre!("recognition worker panicked during shutdown"))?;
    }
    controller_worker
        .join()
        .map_err(|_| eyre!("meeting controller panicked during shutdown"))?;
    Ok(())
}

async fn drive_ui(
    events: Receiver<ControllerEvent>,
    indicator_events: Receiver<crate::dictation_indicator::DictationIndicatorEvent>,
    ui: MeetingUi,
    shutdown: &AtomicBool,
    cx: &mut gpui::AsyncApp,
) {
    let mut offer_window: Option<WindowHandle<MeetingOffer>> = None;
    let mut dictation_indicator = DictationIndicatorUi::new();

    loop {
        if shutdown.load(Ordering::Relaxed) {
            let _ = cx.update(|cx| cx.quit());
            return;
        }
        while let Ok(event) = indicator_events.try_recv() {
            if let Err(error) = cx.update(|cx| dictation_indicator.handle(event, cx)) {
                tracing::error!(%error, "could not update dictation indicator");
                return;
            }
        }
        let _ = cx.update(|cx| dictation_indicator.follow_pointer(cx));
        while let Ok(event) = events.try_recv() {
            let result = cx.update(|cx| match event {
                ControllerEvent::Offer(candidate) => {
                    close_window(offer_window.take(), cx);
                    offer_window = open_offer(candidate, ui.commands.clone(), cx).ok();
                }
                ControllerEvent::Starting => {
                    close_window(offer_window.take(), cx);
                    if let Err(error) = crate::app_window::open_or_focus(
                        &ui.app_window,
                        ui.event_path.clone(),
                        config::voice_control(),
                        ui.meeting_requests.clone(),
                        cx,
                    ) {
                        tracing::error!(%error, "could not open HEX for starting meeting");
                    }
                    update_app_window(&ui.app_window, cx, |window, cx| {
                        window.meeting_starting(cx);
                    });
                }
                ControllerEvent::RecordingStarted => {
                    close_window(offer_window.take(), cx);
                    if let Err(error) = crate::app_window::open_or_focus(
                        &ui.app_window,
                        ui.event_path.clone(),
                        config::voice_control(),
                        ui.meeting_requests.clone(),
                        cx,
                    ) {
                        tracing::error!(%error, "could not open HEX for active meeting");
                    }
                    update_app_window(&ui.app_window, cx, |window, cx| {
                        window.meeting_started(cx);
                    });
                }
                ControllerEvent::Transcribing => {
                    update_app_window(&ui.app_window, cx, |window, cx| {
                        window.meeting_updated(cx);
                    });
                }
                ControllerEvent::Finished => {
                    update_app_window(&ui.app_window, cx, |window, cx| {
                        window.meeting_finished(cx);
                    });
                }
                ControllerEvent::Failed { title, error } => {
                    close_window(offer_window.take(), cx);
                    if let Err(open_error) = crate::app_window::open_or_focus(
                        &ui.app_window,
                        ui.event_path.clone(),
                        config::voice_control(),
                        ui.meeting_requests.clone(),
                        cx,
                    ) {
                        tracing::error!(%open_error, "could not open HEX for meeting failure");
                    }
                    update_app_window(&ui.app_window, cx, |window, cx| {
                        window.meeting_failed(title, error, cx);
                    });
                }
            });
            if let Err(error) = result {
                tracing::error!(%error, "could not update meeting UI");
                return;
            }
        }
        Timer::after(Duration::from_millis(16)).await;
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
        let requested_commands = meeting_requests
            .try_iter()
            .take(8)
            .map(|request| match request {
                MeetingRequest::Start => ControllerCommand::Record(MeetingCandidate {
                    key: "manual".into(),
                    title: "Meeting".into(),
                    source: MeetingSource::Manual,
                }),
                MeetingRequest::Stop => ControllerCommand::Stop,
            });
        for command in commands.try_iter().take(8).chain(requested_commands) {
            match command {
                ControllerCommand::Record(candidate) if recording.is_none() => {
                    let stop = Arc::new(AtomicBool::new(false));
                    let worker_stop = stop.clone();
                    let (started_sender, started) = mpsc::sync_channel(1);
                    let title = candidate.title;
                    let worker_title = title.clone();
                    let worker_project_root = project_root.clone();
                    let worker = thread::spawn(move || {
                        meeting::record(
                            Some(worker_title),
                            &worker_stop,
                            &worker_project_root,
                            Some(started_sender),
                        )
                    });
                    let _ = events.send(ControllerEvent::Starting);
                    recording = Some(ActiveRecording {
                        title,
                        stop,
                        started,
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
                ControllerCommand::Dismiss | ControllerCommand::Record(_) => {}
            }
        }

        if let Some(active) = &mut recording
            && !active.reported_started
            && active.started.try_recv().is_ok()
        {
            active.reported_started = true;
            if !active.stop.load(Ordering::Relaxed) {
                let _ = events.send(ControllerEvent::RecordingStarted);
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
                Ok(Ok(_manifest)) => ControllerEvent::Finished,
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

        if recording.is_none() {
            let microphone_apps = active_microphone_applications()?;
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

fn popup_options(cx: &App) -> WindowOptions {
    let window_size = size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT));
    let (display_id, bounds) = cx.primary_display().map_or_else(
        || (None, Bounds::centered(None, window_size, cx)),
        |display| {
            let origin = display.bounds().top_right()
                - point(px(WINDOW_WIDTH + WINDOW_MARGIN), px(-WINDOW_MARGIN));
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

fn open_offer(
    candidate: MeetingCandidate,
    commands: SyncSender<ControllerCommand>,
    cx: &mut App,
) -> gpui::Result<WindowHandle<MeetingOffer>> {
    cx.open_window(popup_options(cx), |_, cx| {
        cx.new(|_| MeetingOffer {
            candidate,
            commands,
        })
    })
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

struct MeetingOffer {
    candidate: MeetingCandidate,
    commands: SyncSender<ControllerCommand>,
}

impl Render for MeetingOffer {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let dismiss = self.commands.clone();
        let record = self.commands.clone();
        let candidate = self.candidate.clone();
        let is_preview = candidate.key == "preview";
        panel()
            .child(header("MEETING DETECTED"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(18.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(0xf1f1f1))
                            .child(self.candidate.title.clone()),
                    )
                    .child(div().text_sm().text_color(rgb(0x8f8f8f)).child(format!(
                        "{} · Tell participants before recording",
                        self.candidate.source.label()
                    ))),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .items_end()
                    .justify_end()
                    .gap_2()
                    .child(secondary_button("not-now", "Not Now", move |window| {
                        if dismiss.try_send(ControllerCommand::Dismiss).is_ok() {
                            window.remove_window();
                        }
                    }))
                    .child(primary_button("record", "Record Locally", move |window| {
                        let command = if is_preview {
                            ControllerCommand::Dismiss
                        } else {
                            ControllerCommand::Record(candidate.clone())
                        };
                        if record.try_send(command).is_ok() {
                            window.remove_window();
                        }
                    })),
            )
    }
}

fn panel() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .size_full()
        .gap_2()
        .p_3()
        .bg(rgb(0x121212))
        .border_1()
        .border_color(rgb(0x303030))
        .rounded_lg()
        .shadow_lg()
}

fn header(label: &'static str) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(div().size(px(6.0)).rounded_full().bg(rgb(0xd4d4d4)))
        .child(
            div()
                .text_size(px(10.0))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(0x9a9a9a))
                .child(label),
        )
}

fn primary_button(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&mut Window) + 'static,
) -> impl IntoElement {
    button(id, label, rgb(0xe8e8e8), rgb(0x111111), on_click)
}

fn secondary_button(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&mut Window) + 'static,
) -> impl IntoElement {
    button(id, label, rgb(0x242424), rgb(0xbdbdbd), on_click)
}

fn button(
    id: &'static str,
    label: &'static str,
    background: Rgba,
    foreground: Rgba,
    on_click: impl Fn(&mut Window) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(32.0))
        .px_3()
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .bg(background)
        .text_sm()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(foreground)
        .cursor_pointer()
        .hover(|style| style.opacity(0.86))
        .active(|style| style.opacity(0.8))
        .child(label)
        .on_click(move |_, window, _| on_click(window))
}

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

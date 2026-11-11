//! Hex on Windows: one tray process per login session.
//!
//! - The main thread runs Rune's event loop and the window; the Rune tray
//!   runs on its own thread.
//! - The runtime thread owns `WindowsDesktopHost` (the listener, OpenCode,
//!   transformations, updates) and model preparation, and keeps running while
//!   the window is hidden. The window reaches it through [`RuntimeHandle`].
//! - The named event from `windows_app` keeps one app per session: a later
//!   launch signals it and the running app shows its window.
//!
//! Rune's `App` keeps the loop and the state after the window closes and gives
//! the window and its GPU surface back (`keep_running` with
//! `release_gpu_when_closed`), starts without a window for a login launch
//! (`windowless`), and opens or quits from other threads through its
//! `AppHandle`. Opening again makes the window it closed where that one was and
//! at its size (`keep_window_frame`).

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use color_eyre::Result;
use color_eyre::eyre::eyre;
use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWLP_HINSTANCE, GetSystemMetrics, GetWindowLongPtrW, ICON_BIG, ICON_SMALL, IMAGE_ICON,
    LR_DEFAULTCOLOR, LoadImageW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW,
    WM_NCDESTROY, WM_SETICON, WM_SETTINGCHANGE,
};

use crate::app_settings::AppSettings;
use crate::control::{CaptureAction, ControlError, ControlReply, Pane};
use crate::desktop::host::{
    DesktopAction, DesktopListenerSnapshot, DesktopShortcut, DesktopTranscriptionSnapshot,
    DesktopUpdateStatus,
};
use crate::desktop::tray_icon;
use crate::desktop::tray_menu::{TrayCommand, TrayView};
use crate::history::History;
use crate::rune_ui::Ui;
use crate::transcription::models::TranscriptionModelId;
use crate::windows::app::WindowsDesktopHost;

/// How often the runtime refreshes the listener while nothing is asked.
const RUNTIME_TICK: Duration = Duration::from_millis(250);

/// What the window asks the runtime, in the Linux service's vocabulary so
/// the shared panes send the same requests on both platforms.
#[derive(Clone, Debug)]
pub(crate) enum Request {
    Desktop(DesktopAction),
    /// Handled in the window (`rune_ui::host`): Windows polls the keyboard.
    CaptureShortcutFor(#[allow(dead_code)] ShortcutTarget),
    CancelShortcut,
    /// Linux's service registers recorded shortcuts; on Windows the window
    /// saves them, so the runtime ignores this.
    #[allow(dead_code)]
    SetShortcut {
        target: ShortcutTarget,
        shortcut: DesktopShortcut,
    },
    ChooseModel {
        model: TranscriptionModelId,
        language: String,
    },
    CancelModel,
    /// The window saved `settings.json`; adopt it.
    ReloadSettings,
    /// The tray's Enable / Disable Hex.
    ToggleListening,
    /// The tray's Paste last dictation, into the app its menu covered.
    PasteLast,
    /// The window's Quit Hex: stop the listener, then end the app.
    Quit,
}

/// A shortcut the listener registers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ShortcutTarget {
    #[default]
    Dictation,
    PasteLast,
    /// Annotate (the `capture` experiment).
    Capture,
    /// Dictate without processing.
    RawDictation,
    /// Labs › Terminal paste shortcut: dictates like dictation and always
    /// pastes with Ctrl+Shift+V.
    TerminalPaste,
    /// Labs › Second dictation shortcut: another key that dictates exactly
    /// like the dictation shortcut.
    SecondDictation,
    /// Toggle mode's cancel shortcut.
    Cancel,
}

/// What the runtime last published for the window.
#[derive(Clone, PartialEq)]
pub(crate) struct Published {
    /// What the listener last reported doing, from the event log.
    pub(crate) activity: Option<crate::desktop::events::VoiceState>,
    pub(crate) listener: DesktopListenerSnapshot,
    pub(crate) operation_error: Option<String>,
    pub(crate) transcription: DesktopTranscriptionSnapshot,
    pub(crate) update_status: DesktopUpdateStatus,
    pub(crate) update_notes: Option<crate::desktop::host::UpdateNotes>,
    pub(crate) update_blocked: Option<&'static str>,
    pub(crate) settings: AppSettings,
    /// The running listener's state, for the tray.
    pub(crate) state: Option<crate::desktop::events::VoiceState>,
}

type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Shared {
    published: Mutex<Option<Arc<Published>>>,
    wakers: Mutex<Vec<Wake>>,
    /// Where `hex` dictation commands go while a listener runs.
    listener: Mutex<Option<mpsc::Sender<crate::windows::dictation::ListenerCommand>>>,
}

/// The window's connection to the runtime thread.
#[derive(Clone)]
pub(crate) struct RuntimeHandle {
    requests: Sender<Request>,
    shared: Arc<Shared>,
    history: Option<History>,
}

impl RuntimeHandle {
    pub(crate) fn send(&self, request: Request) -> Result<()> {
        self.requests
            .send(request)
            .map_err(|_| eyre!("Hex's runtime stopped"))
    }

    pub(crate) fn published(&self) -> Option<Arc<Published>> {
        lock(&self.shared.published).clone()
    }

    pub(crate) fn history(&self) -> Option<History> {
        self.history.clone()
    }

    /// Wakes `wake` after every publication.
    pub(crate) fn subscribe(&self, wake: Wake) {
        lock(&self.shared.wakers).push(wake);
    }
}

static RUNTIME: OnceLock<RuntimeHandle> = OnceLock::new();

/// The running app's runtime; `None` outside `open` (headless snapshots).
pub(crate) fn runtime() -> Option<RuntimeHandle> {
    RUNTIME.get().cloned()
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

pub fn open(event_path: PathBuf, shutdown: &'static AtomicBool, start_hidden: bool) -> Result<()> {
    if let Err(error) = crate::windows::login_item::follow_current_version() {
        tracing::warn!(%error, "could not point Launch at login at this version of Hex");
    }
    let Some(instance) = crate::windows::app::AppInstance::claim(start_hidden)? else {
        return Ok(());
    };
    name_ui_thread();
    crate::labs::log_launch();
    shutdown.store(false, Ordering::Relaxed);
    let launch = AppSettings::load_for_launch();
    let settings_error = launch
        .note
        .map(crate::app_settings::LaunchNote::into_message);
    let settings = launch.settings;
    let history = History::open_default(settings.history_retention)
        .inspect_err(|error| tracing::warn!(%error, "could not open retained history"))
        .ok();
    crate::windows::gpu_fallback::watch();
    let mut app = crate::rune_ui::app(Default::default())
        .args(crate::rune_ui::window_args())
        .on_quit(quit_hook)
        .on_window(|window| {
            WINDOW.with(|current| *current.borrow_mut() = Some(window.clone()));
            let _ = window.with_raw_handle(|handle| {
                if let rune::raw_window_handle::RawWindowHandle::Win32(handle) = handle.as_raw() {
                    let hwnd = handle.hwnd.get() as HWND;
                    apply_window_icon(hwnd);
                    blend_title_bar(hwnd);
                }
            });
        });
    let window = app.handle();
    // Adding the tray icon waits for Explorer, so it runs on its own thread
    // while the overlays install.
    let adding_tray = {
        let window = window.clone();
        std::thread::Builder::new()
            .name("hex-tray-setup".into())
            .spawn(move || create_tray(window))
    };
    let hud = crate::rune_hud::install(&mut app);
    crate::rune_ui::live_text::install(&mut app, crate::rune_hud::live_text_place);
    // The capture capsule, picker, and ink.
    let _capture_overlays =
        crate::rune_ui::capture_capsule::feed::CaptureOverlays::install(&mut app);
    let meetings = crate::meeting::runtime::Meetings::install(&mut app, shutdown)
        .inspect_err(|error| tracing::error!(%error, "meetings could not start"))
        .ok();
    let added_tray = match adding_tray {
        Ok(thread) => thread
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
        Err(_) => create_tray(window.clone()),
    };
    let tray = match added_tray {
        Ok(tray) => {
            // Closing gives the window and its swapchain back, so a Hex sitting
            // in the tray holds no Direct3D window and never passes for a game
            // to capture tools such as NVIDIA's Instant Replay (#1308). Opening
            // it again makes both, where the window was and at its size.
            app = if start_hidden {
                app.windowless().release_gpu_when_closed()
            } else {
                app.keep_running().release_gpu_when_closed()
            };
            Some(Arc::new(tray))
        }
        // Without a tray, closing the window quits: Hex must not run on
        // with no way back to it.
        Err(error) => {
            tracing::warn!(%error, "Windows system tray is unavailable; closing Hex will quit");
            None
        }
    };
    let (requests, receiver) = mpsc::channel();
    let shared = Arc::new(Shared::default());
    RUNTIME
        .set(RuntimeHandle {
            requests: requests.clone(),
            shared: shared.clone(),
            history: history.clone(),
        })
        .map_err(|_| eyre!("the Hex runtime already started"))?;
    let (control_calls, control_receiver) = mpsc::sync_channel(8);
    let (dictation_controls, dictations) = mpsc::sync_channel(8);
    let processing = crate::dictation::modes::ProcessingState::from_settings(&settings);
    let local_api = crate::local_api::LocalApi::start_for_app(
        &event_path,
        control_calls,
        Some(dictation_controls),
        Some(crate::local_api::Processing::installed(
            crate::local_api::processing::dictation_processor(processing.clone()),
        )),
    );
    let quit = window.clone();
    let runtime_tray = tray.clone();
    let runtime_shared = shared.clone();
    let runtime = std::thread::Builder::new()
        .name("hex-runtime".into())
        .spawn(move || {
            run_runtime(
                event_path,
                settings,
                settings_error,
                history,
                receiver,
                &runtime_shared,
                shutdown,
                hud,
                runtime_tray.as_deref(),
                dictations.into(),
                processing,
            );
            quit.quit();
        })?;
    crate::control::host::serve(
        control_receiver,
        Arc::new(WindowsControl {
            runtime: runtime_handle(),
            shared,
            window: Mutex::new(window.clone()),
        }),
    );
    let relaunch = window.clone();
    instance.forward_launches(move || {
        relaunch.open();
        true
    });
    // Returns on Quit, or when the window closes without a tray.
    app.run();
    crate::windows::gpu_fallback::disarm();
    shutdown.store(true, Ordering::Relaxed);
    drop(requests);
    let _ = runtime.join();
    // A recording stops, keeping its audio for the next launch.
    if let Some(meetings) = meetings {
        meetings.join();
    }
    drop(local_api);
    drop(tray);
    Ok(())
}

/// Hex is ending: Rune runs this before the loop ends, including from Windows'
/// `WM_QUERYENDSESSION` and `WM_ENDSESSION` (also for a windowless launch),
/// which may end the process before `app.run()` returns. Clear the GPU draw
/// marker now, so a logoff or shutdown isn't read as a crash while drawing.
/// If another app vetoes the shutdown, the marker is already gone and this
/// launch carries on without it.
fn quit_hook(_: &mut Ui, _: &mut rune::Tx) {
    crate::windows::gpu_fallback::disarm();
}

/// Names the main thread, which runs Rune's event loop and draws, so
/// profilers and debuggers tell it from Hex's other threads (which the
/// standard library names when they start).
fn name_ui_thread() {
    use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadDescription};
    let name: Vec<u16> = "hex-ui".encode_utf16().chain([0]).collect();
    // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives the
    // call, and the pseudo handle names the calling thread.
    unsafe { SetThreadDescription(GetCurrentThread(), name.as_ptr()) };
}

// ---------------------------------------------------------------------------
// Runtime thread

#[allow(clippy::too_many_arguments)]
fn run_runtime(
    event_path: PathBuf,
    settings: AppSettings,
    settings_error: Option<String>,
    history: Option<History>,
    requests: Receiver<Request>,
    shared: &Shared,
    shutdown: &AtomicBool,
    indicator: crate::rune_hud::HudSender,
    tray: Option<&rune::Tray>,
    dictations: crate::dictation::service::DictationQueue,
    processing: crate::dictation::modes::ProcessingState,
) {
    let mut tray_view = None;
    let mut tray_meetings = None;
    indicator.set_position(settings.indicator_position);
    let mut host = WindowsDesktopHost::new(
        event_path,
        settings,
        settings_error,
        history,
        indicator,
        dictations,
        processing,
    );
    loop {
        let mut next = match requests.recv_timeout(RUNTIME_TICK) {
            Ok(request) => Some(request),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let mut quit = false;
        while let Some(request) = next {
            quit |= host.handle(request);
            next = requests.try_recv().ok();
        }
        quit |= host.update_restarted();
        if shutdown.load(Ordering::Relaxed) || quit {
            break;
        }
        host.refresh();
        let published = Arc::new(host.publish());
        let changed = {
            let mut current = lock(&shared.published);
            let changed = current.as_deref() != Some(&published);
            if changed {
                *current = Some(published.clone());
            }
            changed
        };
        *lock(&shared.listener) = host.listener_commands();
        if changed {
            for wake in lock(&shared.wakers).iter() {
                wake();
            }
        }
        let meetings = crate::meeting::runtime::tray_state();
        if let Some(tray) = tray
            && (changed || tray_view.is_none() || tray_meetings != meetings)
        {
            tray_meetings = meetings;
            let view = tray_view_for(&published);
            if tray_view.as_ref() != Some(&view) {
                tray.set_tooltip(view.tooltip());
                tray.set_menu(view.menu());
                tray_view = Some(view);
            }
        }
    }
    host.stop();
}

fn runtime_handle() -> RuntimeHandle {
    RUNTIME
        .get()
        .cloned()
        .expect("the runtime is set before control starts")
}

/// The Windows adapter `hex` control commands from the local API run
/// through ([`crate::control::host`]).
struct WindowsControl {
    runtime: RuntimeHandle,
    shared: Arc<Shared>,
    window: Mutex<rune::AppHandle<Ui>>,
}

/// Lets the local API write the reply before Hex quits.
const QUIT_GRACE: Duration = Duration::from_millis(250);

impl WindowsControl {
    fn window(&self) -> rune::AppHandle<Ui> {
        lock(&self.window).clone()
    }

    fn send(&self, request: Request) -> std::result::Result<(), ControlError> {
        self.runtime
            .send(request)
            .map_err(|error| ControlError::new("unavailable", format!("{error:#}")))
    }

    fn listener(
        &self,
        command: crate::windows::dictation::ListenerCommand,
    ) -> std::result::Result<(), ControlError> {
        let listener = lock(&self.shared.listener).clone().ok_or_else(|| {
            ControlError::new(
                "not-ready",
                "Hex is not listening; turn it on in the tray or the window",
            )
        })?;
        listener
            .send(command)
            .map_err(|_| ControlError::new("busy", "Hex's listener stopped; try again"))?;
        crate::windows::dictation::wake();
        Ok(())
    }
}

impl crate::control::host::Host for WindowsControl {
    fn state(&self) -> std::result::Result<crate::control::host::HostState, ControlError> {
        let published = self
            .runtime
            .published()
            .ok_or_else(|| ControlError::new("not-ready", "Hex is starting; try again"))?;
        Ok(crate::control::host::HostState {
            settings: published.settings.clone(),
            listener: published.listener.clone(),
            transcription: published.transcription.clone(),
            update: published.update_status.clone(),
            capture: crate::control::host::capture_from_state(published.activity),
            window_open: self.window().is_visible(),
        })
    }

    fn dictation(
        &self,
        action: CaptureAction,
        print: Option<crate::control::PrintReply>,
        raw: bool,
    ) -> std::result::Result<(), ControlError> {
        use crate::windows::dictation::ListenerCommand;
        self.listener(match print {
            Some(print) => ListenerCommand::StopAndPrint(print),
            None => ListenerCommand::Dictation { action, raw },
        })
    }

    fn paste_last(&self) -> std::result::Result<(), ControlError> {
        self.listener(crate::windows::dictation::ListenerCommand::PasteLast)
    }

    fn save(&self, settings: AppSettings) -> std::result::Result<(), ControlError> {
        settings
            .save()
            .map_err(|error| ControlError::new("internal", format!("couldn't save: {error:#}")))?;
        self.send(Request::ReloadSettings)
    }

    fn history(&self) -> Option<History> {
        self.runtime.history()
    }

    fn show_window(&self, pane: Option<Pane>) -> std::result::Result<(), ControlError> {
        let window = self.window();
        window.open();
        window.update(move |ui, tx| {
            crate::rune_ui::show(ui, crate::rune_ui::destination(pane), tx);
        });
        Ok(())
    }

    fn hide_window(&self) -> bool {
        let window = self.window();
        let open = window.is_visible();
        if open {
            window.close();
        }
        open
    }

    fn onboarding_reset(&self) -> ControlReply {
        let (sender, receiver) = mpsc::sync_channel(1);
        let window = self.window();
        window.open();
        window.update(move |ui, tx| {
            let _ = sender.send(crate::rune_ui::restart_onboarding(ui, tx));
        });
        match receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => ControlReply::done("Opened onboarding"),
            Ok(Err(why)) => ControlReply::error("unsupported", why),
            Err(_) => ControlReply::error("unavailable", "the window didn't answer; try again"),
        }
    }

    fn choose_model(
        &self,
        selection: crate::transcription::models::TranscriptionSelection,
    ) -> std::result::Result<(), ControlError> {
        self.send(Request::ChooseModel {
            model: selection.model,
            language: selection.language,
        })
    }

    fn cancel_model(&self) {
        let _ = self.runtime.send(Request::CancelModel);
    }

    fn check_for_updates(&self) -> std::result::Result<(), ControlError> {
        self.send(Request::Desktop(DesktopAction::CheckForUpdates))
    }

    fn launch_at_login(&self, enabled: Option<bool>) -> Option<std::result::Result<bool, String>> {
        let format = |error: color_eyre::Report| format!("{error:#}");
        Some(match enabled {
            None => crate::windows::login_item::is_enabled().map_err(format),
            Some(enabled) => crate::windows::login_item::set_enabled(enabled)
                .and_then(|()| crate::windows::login_item::is_enabled())
                .map_err(format),
        })
    }

    /// `hex restart` opens Hex again once this one has gone.
    fn quit(&self, _restart: bool) -> std::result::Result<(), ControlError> {
        let runtime = self.runtime.clone();
        std::thread::spawn(move || {
            std::thread::sleep(QUIT_GRACE);
            let _ = runtime.send(Request::Quit);
        });
        Ok(())
    }

    fn shortcut_rules(&self) -> crate::control::settings::ShortcutRules {
        crate::rune_ui::shortcut_rules()
    }

    /// `hex annotate`: recording commands wait for the listener's capture
    /// host; the captures folder is read here.
    fn annotate(&self, command: crate::control::ControlCommand) -> ControlReply {
        use crate::capture_host::{self as capture, CaptureCommand, CaptureControl};
        use crate::control::ControlCommand as C;
        let result = match command {
            C::ScrawlCapture { action } => {
                let (sender, receiver) = mpsc::sync_channel(1);
                capture::send_to_listener(CaptureControl::Reply(action, sender))
                    .map_err(|error| ControlError::new("not-ready", format!("{error:#}")))
                    .and_then(|()| {
                        receiver.recv_timeout(Duration::from_secs(5)).map_err(|_| {
                            ControlError::new("unavailable", "the listener didn't answer")
                        })
                    })
            }
            C::ScrawlCaptureList { limit } => capture::list_captures(limit),
            C::ScrawlCaptureShow { capture } => capture::show_capture(&capture),
            C::ScrawlCaptureSend {
                capture,
                to,
                always,
            } => capture::send_capture(&capture, to, always),
            C::ScrawlCaptureUndo => {
                capture::send_to_listener(CaptureControl::Command(CaptureCommand::Undo))
                    .map(|()| ControlReply::done("Asked Hex to take the newest send back"))
                    .map_err(|error| ControlError::new("not-ready", format!("{error:#}")))
            }
            C::ScrawlCaptureRoute { capture, ask_jev } => {
                capture::explain_capture(&capture, ask_jev)
            }
            _ => unreachable!("only Annotate's commands"),
        };
        result.unwrap_or_else(ControlReply::from)
    }
}

// ---------------------------------------------------------------------------
// The tray and the window icon

/// What the tray shows for what the runtime published.
fn tray_view_for(published: &Published) -> TrayView {
    TrayView {
        meetings: crate::meeting::runtime::tray_state(),
        ..TrayView::new(
            published.listener.running,
            published.state,
            &published.update_status,
            crate::desktop::tray_menu::model_menu_entries(
                &published.settings,
                published.transcription.preparing.is_some(),
                crate::transcription::models::is_installed,
            ),
        )
    }
}

/// Gives the window the executable's embedded icon (resource 1, see
/// build.rs) for the taskbar, Alt-Tab, and the caption, so they match the
/// exe in Explorer and Task Manager. Windows picks the frame that fits each
/// size and DPI.
fn apply_window_icon(hwnd: HWND) {
    const APP_ICON: u16 = 1;
    unsafe {
        let module = GetWindowLongPtrW(hwnd, GWLP_HINSTANCE) as HINSTANCE;
        for (kind, width, height) in [
            (ICON_SMALL, SM_CXSMICON, SM_CYSMICON),
            (ICON_BIG, SM_CXICON, SM_CYICON),
        ] {
            let icon = LoadImageW(
                module,
                APP_ICON as usize as *const u16,
                IMAGE_ICON,
                GetSystemMetrics(width),
                GetSystemMetrics(height),
                LR_DEFAULTCOLOR,
            );
            if !icon.is_null() {
                SendMessageW(hwnd, WM_SETICON, kind as usize, icon as isize);
            }
        }
    }
}

thread_local! {
    /// Rune's handle to the settings window, which outlives each native window
    /// it makes: closing destroys the native one (`release_gpu_when_closed`) and
    /// the next open makes another from this handle. UI-thread only, like the
    /// native window it names.
    static WINDOW: std::cell::RefCell<Option<rune::WindowHandle>> = const { std::cell::RefCell::new(None) };
}

/// Closing destroys the native window, so the next open would make a new one at
/// the app's own size and where the OS puts it. Keep the frame the closing
/// window had instead, so one the user moved or resized comes back as they left
/// it: Rune keeps a position on the window's options and a resize request's
/// size on the app, and both reach the window it creates next. A maximized,
/// minimized, or full-screen frame is not the size and place to reopen at, so
/// that one is left alone.
pub(crate) fn keep_window_frame(tx: &mut rune::Tx) {
    let Some(placement) = WINDOW.with(|window| {
        let window = window.borrow();
        let handle = window.as_ref()?;
        let placement = handle.placement()?;
        if placement.scale <= 0.0
            || placement.maximized
            || placement.minimized
            || placement.full_screen
        {
            return None;
        }
        // Outer position in physical desktop pixels, the space Rune's window
        // options take; it reaches the next window this handle creates.
        handle.set_position(placement.position);
        Some(placement)
    }) else {
        return;
    };
    let (width, height) = placement.content_size;
    let scale = placement.scale as f32;
    tx.resize_window((width as f32 / scale, height as f32 / scale));
}

/// The settings window Rune created last, for [`match_title_bar`].
static TITLED_WINDOW: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
/// What the last frame asked of the caption, or `None` before the first
/// frame names a theme.
static CAPTION: Mutex<Option<Caption>> = Mutex::new(None);

/// A caption theme: the window background's COLORREF (`0x00bbggrr`), whether
/// the window is dark, and the OS theme when it was applied.
///
/// The OS theme is part of the key because winit repaints the native frame
/// from it on every `WM_SETTINGCHANGE` (`dark_mode.rs`'s `try_theme`), not
/// only when the OS theme changed, so [`keep_caption`] writes a forced theme
/// back after each one. When the OS theme does change, the key changes too,
/// and the next frame repaints the caption for the new pair.
#[derive(Clone, Copy, PartialEq)]
struct Caption {
    color: u32,
    dark: bool,
    system_dark: bool,
}

/// Identifies Hex's caption subclass among any others on the window.
const CAPTION_SUBCLASS: usize = 1;

/// Blends the native title bar into the window: DWM paints the caption the
/// window's background color once a frame has named it, and draws its text
/// and buttons in the window's light or dark mode. The caption keeps its
/// icon and title, the frame, its resize edges, and the caption buttons.
fn blend_title_bar(hwnd: HWND) {
    TITLED_WINDOW.store(hwnd, Ordering::Relaxed);
    // SAFETY: `hwnd` is the settings window Rune just created, and the proc
    // outlives it. Installing the same proc and id on the same window again
    // replaces the reference data instead of being refused, which is harmless:
    // it carries none, and re-applying the caption is idempotent besides.
    unsafe {
        SetWindowSubclass(hwnd, Some(keep_caption), CAPTION_SUBCLASS, 0);
    }
    let caption = *lock(&CAPTION);
    if let Some(caption) = caption {
        set_title_bar(hwnd, caption);
    }
}

/// Puts a forced caption back after a settings broadcast, and drops this
/// subclass when the window goes away.
///
/// winit handles `WM_SETTINGCHANGE` by repainting the frame from the OS theme
/// (`dark_mode.rs`'s `try_theme`) on every broadcast, not only when the theme
/// changed, and sends `ThemeChanged` only for a real change. An unrelated
/// broadcast (an accent colour, region, or environment change) therefore
/// leaves a forced appearance overwritten: the view's cached theme did not
/// change, so it does not re-apply it. Running after the next proc, which is
/// winit's, gives the last word back to the theme the view last resolved. The
/// caption is written only while Appearance forces a theme different from the
/// OS (`dark` against `system_dark`); when it follows the OS, or is off, the
/// frame winit just painted is already the one asked for, and a real OS theme
/// change repaints it from the next frame, whose key changed.
unsafe extern "system" fn keep_caption(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass: usize,
    _data: usize,
) -> LRESULT {
    if message == WM_NCDESTROY {
        // SAFETY: `keep_caption` was installed under `CAPTION_SUBCLASS` on
        // `hwnd`, which still exists while its window procedure handles the
        // destroy notification, so this removes exactly this proc's entry
        // before the chain forwards it. comctl32 also frees the chain with the
        // window; removing it first just does that while the handle is usable.
        unsafe {
            RemoveWindowSubclass(hwnd, Some(keep_caption), CAPTION_SUBCLASS);
        }
    }
    // SAFETY: Windows passes this window's message and parameters, and the
    // next proc in the chain is the one winit installed for it.
    let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    if message == WM_SETTINGCHANGE {
        // Copy the theme out before DWM is asked: a guard taken in an `if`'s
        // condition would be held until that statement ends.
        let caption = *lock(&CAPTION);
        if let Some(caption) = caption
            && caption.dark != caption.system_dark
        {
            set_title_bar(hwnd, caption);
        }
    }
    result
}

/// Paints the settings window's caption `background` and light/dark mode, so
/// the title bar reads as part of the window. Called every frame with the
/// current theme; DWM is asked only when the caption's theme changes (the
/// window's theme, or the OS theme winit last copied onto the frame).
/// Windows 10 has no caption colour, so DWM keeps its own caption background
/// (light or dark with the same flag; before build 19041 it ignores attribute
/// 20 and keeps the OS caption), and its text and buttons follow `dark`.
pub(crate) fn match_title_bar(background: rune::Color, dark: bool, system_dark: bool) {
    let [r, g, b, _] = background.premul_srgb(1.0);
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u32;
    let caption = Caption {
        color: byte(r) | byte(g) << 8 | byte(b) << 16,
        dark,
        system_dark,
    };
    {
        let mut last = lock(&CAPTION);
        if *last == Some(caption) {
            return;
        }
        *last = Some(caption);
    }
    let hwnd = TITLED_WINDOW.load(Ordering::Relaxed);
    if !hwnd.is_null() {
        set_title_bar(hwnd, caption);
    }
}

fn set_title_bar(hwnd: HWND, caption: Caption) {
    // `DWMWA_USE_IMMERSIVE_DARK_MODE` takes a Win32 `BOOL`, which is an `i32`.
    let dark = i32::from(caption.dark);
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            (&raw const dark).cast(),
            size_of::<i32>() as u32,
        );
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR as u32,
            (&raw const caption.color).cast(),
            size_of::<u32>() as u32,
        );
    }
}

/// Whether the taskbar uses the light theme (Windows 11 light mode). Any
/// failure to read it means dark, Windows' default taskbar.
fn taskbar_is_light() -> bool {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_value::<u32, _>("SystemUsesLightTheme"))
        .is_ok_and(|light| light != 0)
}

fn create_tray(window: rune::AppHandle<Ui>) -> Result<rune::Tray> {
    let icon = rune::TrayIcon::rgba(
        tray_icon::SIZE,
        tray_icon::SIZE,
        tray_icon::pixels(taskbar_is_light()),
    );
    let view = TrayView::default();
    let spec = rune::TraySpec::new("hex", "Hex", icon)
        .tooltip(view.tooltip())
        .menu(view.menu());
    let tray = rune::Tray::new(spec, move |event| match event {
        // Left click shows the window; right click opens the menu.
        rune::TrayEvent::Activate { .. } => window.open(),
        rune::TrayEvent::Menu(id) => match TrayCommand::from_id(&id) {
            Some(TrayCommand::Show) => window.open(),
            Some(TrayCommand::Quit) => window.quit(),
            Some(
                command @ (TrayCommand::RecordMeeting
                | TrayCommand::StopMeeting
                | TrayCommand::CopyLastMeeting),
            ) => {
                use crate::meeting::MeetingRequest;
                let result = match command {
                    TrayCommand::RecordMeeting => {
                        crate::meeting::runtime::request(MeetingRequest::Start { title: None })
                    }
                    TrayCommand::StopMeeting => {
                        crate::meeting::runtime::request(MeetingRequest::Stop)
                    }
                    _ => crate::meeting::runtime::copy_last_transcript(),
                };
                if let Err(error) = result {
                    tracing::warn!(%error, ?command, "the tray action failed");
                }
            }
            Some(command) => {
                let Some(runtime) = runtime() else { return };
                let published = runtime.published();
                let request = match command {
                    TrayCommand::PasteLast => Request::PasteLast,
                    TrayCommand::ToggleListening => Request::ToggleListening,
                    TrayCommand::UseModel(model) => {
                        let Some(published) = published else { return };
                        Request::ChooseModel {
                            model,
                            language: published.settings.transcription_for_model(model).language,
                        }
                    }
                    TrayCommand::Update => {
                        let Some(action) = published.and_then(|published| {
                            crate::desktop::tray_menu::update_action(&published.update_status)
                        }) else {
                            return;
                        };
                        Request::Desktop(action)
                    }
                    TrayCommand::Show
                    | TrayCommand::Quit
                    | TrayCommand::RecordMeeting
                    | TrayCommand::StopMeeting
                    | TrayCommand::CopyLastMeeting => return,
                };
                let _ = runtime.send(request);
            }
            None => {}
        },
        _ => {}
    })?;
    Ok(tray)
}

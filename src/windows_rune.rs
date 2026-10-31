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
//! Rune's `App` keeps the loop, the state, and the hidden window alive
//! after the window closes (`keep_running`), starts without a window for a
//! login launch (`windowless`), and opens or quits from other threads
//! through its `AppHandle`.

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use color_eyre::Result;
use color_eyre::eyre::eyre;
use windows_sys::Win32::Foundation::{HINSTANCE, HWND};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_CAPTION_COLOR, DwmSetWindowAttribute};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWLP_HINSTANCE, GetSystemMetrics, GetWindowLongPtrW, ICON_BIG, ICON_SMALL, IMAGE_ICON,
    LR_DEFAULTCOLOR, LoadImageW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW,
    WM_SETICON,
};

use crate::app_settings::AppSettings;
use crate::control::{CaptureAction, ControlError, ControlReply, Pane};
use crate::desktop_host::{
    DesktopAction, DesktopListenerSnapshot, DesktopShortcut, DesktopTranscriptionSnapshot,
    DesktopUpdateStatus,
};
use crate::desktop_i18n::tr_fill;
use crate::history::History;
use crate::rune_ui::Ui;
use crate::transcription_models::TranscriptionModelId;
use crate::tray_menu::{TrayCommand, TrayView};
use crate::windows_app::WindowsDesktopHost;

/// The tray icon: the white Hex mark as raw 32×32 RGBA, recolored black for
/// a light taskbar ([`tray_icon_pixels`]).
const TRAY_ICON_SIZE: u32 = 32;
const TRAY_ICON_RGBA: &[u8] = include_bytes!("../resources/windows/tray-32.rgba");
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
}

/// What the runtime last published for the window.
#[derive(Clone, PartialEq)]
pub(crate) struct Published {
    /// What the listener last reported doing, from the event log.
    pub(crate) activity: Option<crate::events::VoiceState>,
    pub(crate) listener: DesktopListenerSnapshot,
    pub(crate) operation_error: Option<String>,
    pub(crate) transcription: DesktopTranscriptionSnapshot,
    pub(crate) update_status: DesktopUpdateStatus,
    pub(crate) settings: AppSettings,
    /// The running listener's state, for the tray.
    pub(crate) state: Option<crate::events::VoiceState>,
}

type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Shared {
    published: Mutex<Option<Arc<Published>>>,
    wakers: Mutex<Vec<Wake>>,
    /// Where `hex` dictation commands go while a listener runs.
    listener: Mutex<Option<mpsc::Sender<crate::windows_dictation::ListenerCommand>>>,
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
    let Some(instance) = crate::windows_app::AppInstance::claim(start_hidden)? else {
        return Ok(());
    };
    crate::labs::log_launch();
    shutdown.store(false, Ordering::Relaxed);
    let (settings, settings_error) = match AppSettings::load() {
        Ok(settings) => (settings, None),
        Err(error) => (
            AppSettings::default(),
            Some(tr_fill(
                "Could not load settings: {}",
                &format!("{error:#}"),
            )),
        ),
    };
    let history = History::open_default(settings.history_retention)
        .inspect_err(|error| tracing::warn!(%error, "could not open retained history"))
        .ok();
    let mut app = crate::rune_ui::app(Default::default())
        .args(Vec::new())
        .on_window(|window| {
            let _ = window.with_raw_handle(|handle| {
                if let rune::raw_window_handle::RawWindowHandle::Win32(handle) = handle.as_raw() {
                    let hwnd = handle.hwnd.get() as HWND;
                    apply_window_icon(hwnd);
                    blend_title_bar(hwnd);
                }
            });
        });
    let window = app.handle();
    let tray = match create_tray(window.clone()) {
        Ok(tray) => {
            app = if start_hidden {
                app.windowless()
            } else {
                app.keep_running()
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
    let hud = crate::rune_hud::install(&mut app);
    crate::rune_ui::live_text::install(&mut app, crate::rune_hud::live_text_place);
    // The capture capsule, picker, and ink, in dev builds only.
    let _capture_overlays = crate::dev_build::is_dev_build()
        .then(|| crate::rune_ui::capture_capsule::feed::CaptureOverlays::install(&mut app));
    let meetings = crate::meeting_runtime::Meetings::install(&mut app, shutdown)
        .inspect_err(|error| tracing::error!(%error, "meetings could not start"))
        .ok();
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
    let local_api = crate::local_api::LocalApi::start_for_app(&event_path, control_calls);
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
) {
    let mut tray_view = None;
    let mut tray_meetings = None;
    indicator.set_position(settings.indicator_position);
    let mut host =
        WindowsDesktopHost::new(event_path, settings, settings_error, history, indicator);
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
        let meetings = crate::meeting_runtime::tray_state();
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
        command: crate::windows_dictation::ListenerCommand,
    ) -> std::result::Result<(), ControlError> {
        let listener = lock(&self.shared.listener).clone().ok_or_else(|| {
            ControlError::new(
                "not-ready",
                "Hex is not listening; turn it on in the tray or the window",
            )
        })?;
        listener
            .send(command)
            .map_err(|_| ControlError::new("busy", "Hex's listener stopped; try again"))
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
    ) -> std::result::Result<(), ControlError> {
        use crate::windows_dictation::ListenerCommand;
        self.listener(match print {
            Some(print) => ListenerCommand::StopAndPrint(print),
            None => ListenerCommand::Dictation(action),
        })
    }

    fn paste_last(&self) -> std::result::Result<(), ControlError> {
        self.listener(crate::windows_dictation::ListenerCommand::PasteLast)
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
        selection: crate::transcription_models::TranscriptionSelection,
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
            None => crate::windows_login_item::is_enabled().map_err(format),
            Some(enabled) => crate::windows_login_item::set_enabled(enabled)
                .and_then(|()| crate::windows_login_item::is_enabled())
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
        meetings: crate::meeting_runtime::tray_state(),
        ..TrayView::new(
            published.listener.running,
            published.state,
            &published.update_status,
            crate::tray_menu::model_menu_entries(
                &published.settings,
                published.transcription.preparing.is_some(),
                crate::transcription_models::is_installed,
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

/// The settings window Rune created last, for [`match_title_bar`].
static TITLED_WINDOW: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
/// The caption's COLORREF (`0x00bbggrr`), or [`NO_CAPTION_COLOR`] before the
/// first frame names one.
static CAPTION_COLOR: AtomicU32 = AtomicU32::new(NO_CAPTION_COLOR);
const NO_CAPTION_COLOR: u32 = u32::MAX;

/// Blends the native title bar into the window: DWM paints the caption the
/// window's background color once a frame has named it. The caption keeps
/// its icon and title, the frame, its resize edges, and the caption buttons.
fn blend_title_bar(hwnd: HWND) {
    TITLED_WINDOW.store(hwnd, Ordering::Relaxed);
    let color = CAPTION_COLOR.load(Ordering::Relaxed);
    if color != NO_CAPTION_COLOR {
        set_caption_color(hwnd, color);
    }
}

/// Paints the settings window's caption `background`, so the title bar reads
/// as part of the window. Called every frame with the current theme's
/// background; DWM is asked only when the color changes (the system theme
/// switched). Windows 10 has no caption color and keeps its own.
pub(crate) fn match_title_bar(background: rune::Color) {
    let [r, g, b, _] = background.premul_srgb(1.0);
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u32;
    let color = byte(r) | byte(g) << 8 | byte(b) << 16;
    if CAPTION_COLOR.swap(color, Ordering::Relaxed) == color {
        return;
    }
    let hwnd = TITLED_WINDOW.load(Ordering::Relaxed);
    if !hwnd.is_null() {
        set_caption_color(hwnd, color);
    }
}

fn set_caption_color(hwnd: HWND, color: u32) {
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR as u32,
            (&raw const color).cast(),
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

/// The tray mark for the taskbar's theme: the white mark on a dark taskbar,
/// the same shape in black on a light one, where white would vanish.
fn tray_icon_pixels(light_taskbar: bool) -> Vec<u8> {
    let mut pixels = TRAY_ICON_RGBA.to_vec();
    if light_taskbar {
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel[..3].fill(0);
        }
    }
    pixels
}

fn create_tray(window: rune::AppHandle<Ui>) -> Result<rune::Tray> {
    let icon = rune::TrayIcon::rgba(
        TRAY_ICON_SIZE,
        TRAY_ICON_SIZE,
        tray_icon_pixels(taskbar_is_light()),
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
                        crate::meeting_runtime::request(MeetingRequest::Start { title: None })
                    }
                    TrayCommand::StopMeeting => {
                        crate::meeting_runtime::request(MeetingRequest::Stop)
                    }
                    _ => crate::meeting_runtime::copy_last_transcript(),
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
                            crate::tray_menu::update_action(&published.update_status)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_taskbar_gets_a_dark_mark_with_the_same_shape() {
        let dark = tray_icon_pixels(false);
        let light = tray_icon_pixels(true);
        assert_eq!(dark, TRAY_ICON_RGBA);
        assert_eq!(light.len(), (TRAY_ICON_SIZE * TRAY_ICON_SIZE * 4) as usize);
        for (shown, mark) in light.as_chunks::<4>().0.iter().zip(dark.as_chunks::<4>().0) {
            assert_eq!(&shown[..3], &[0, 0, 0]);
            assert_eq!(shown[3], mark[3]);
        }
    }
}

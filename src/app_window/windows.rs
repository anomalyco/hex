//! Windows parts of the app window: the custom caption over a full-height
//! sidebar, the Enabled/Disabled listener card, software updates, and
//! shortcut capture through the keyboard hook. The tray, the HUD window,
//! and the listener lifecycle stay in `windows_app`.

use std::cell::RefCell;

use super::*;
use crate::windows_app::WindowsDesktopHost;
use crate::windows_input::WindowsHotkey;
use crate::windows_input::{ChordCapture, ChordPoll};
use crate::windows_updater::UpdateCheck;

pub(super) const CAPABILITIES: Capabilities = Capabilities {
    meetings: false,
    commands: false,
    hud_lab: false,
    fun: false,
    modes: true,
    voice_action: true,
    history: true,
    permissions: false,
    dock_icon: false,
    launch_at_login: true,
    listener_control: true,
    paste_last: true,
    recording_audio: true,
    release_microphone: true,
    double_tap_only: true,
    modifier_sides: false,
    indicator_position: true,
    interface_language: true,
    software_updates: true,
    terminal_paste: false,
    practice_dictation: false,
};

/// Windows records shortcuts by polling the keyboard state: a modifier-only
/// chord such as Ctrl+Win never reaches the window as a key event.
pub(super) fn records_key_events() -> bool {
    false
}

/// How the Modes pane describes website rules; `None` hides them where
/// the listener can't read the browser's address.
pub(super) const WEB_PAGES_DESCRIPTION: Option<&str> =
    Some("Switch when a domain is active in your browser");

/// Whether the listener knows which application is frontmost.
pub(super) fn focused_application_known() -> bool {
    true
}

/// The Modes application picker's button for an app it doesn't list.
pub(super) const CHOOSE_APPLICATION_FILE: &str = "Choose an .exe…";

pub(super) const MANUAL_UPDATE_CHECK: bool = true;

/// What the Windows lifecycle hands the window.
pub(super) struct Args {
    pub(super) host: Rc<RefCell<WindowsDesktopHost>>,
    /// Enable on launch waited for first-run setup to finish.
    pub(super) listen_after_setup: bool,
}

pub(super) struct Platform {
    host: Rc<RefCell<WindowsDesktopHost>>,
    listen_after_setup: bool,
    /// What the window last showed of the host, to redraw only on change.
    shown: HostView,
    restart_scheduled: bool,
}

#[derive(PartialEq)]
struct HostView {
    running: bool,
    status: String,
    error: Option<String>,
    last_failure: Option<String>,
    update: String,
}

impl Platform {
    pub(super) fn new(
        args: Args,
        _: Option<&AppWindowPreview>,
        _: &AppSettings,
        _: &StatusSnapshot,
        _: &mut Context<AppWindow>,
    ) -> Self {
        let shown = host_view(&args.host.borrow());
        Self {
            host: args.host,
            listen_after_setup: args.listen_after_setup,
            shown,
            restart_scheduled: false,
        }
    }
}

fn host_view(host: &WindowsDesktopHost) -> HostView {
    let listener = host.listener_state();
    HostView {
        running: listener.running,
        status: listener.status,
        error: listener.error,
        last_failure: listener.last_failure,
        update: format!("{:?}", host.update_state()),
    }
}

/// Opens the shared window for the Windows lifecycle.
pub fn open(
    event_path: PathBuf,
    host: Rc<RefCell<WindowsDesktopHost>>,
    listen_after_setup: bool,
    cx: &mut App,
) -> gpui::Result<WindowHandle<AppWindow>> {
    let history = host.borrow().history();
    open_new(
        event_path,
        history,
        None,
        Args {
            host,
            listen_after_setup,
        },
        cx,
    )
}

pub(super) fn window_options(options: WindowOptions) -> WindowOptions {
    WindowOptions {
        app_id: Some("com.kitlangton.Hex".into()),
        ..options
    }
}

pub(super) fn frame(frame: Div) -> Div {
    crate::windows_ui::ui_font(frame)
}

/// The sidebar runs under the caption row; its half holds the brand.
pub(super) fn sidebar(sidebar: Div) -> Div {
    sidebar
        .pt_0()
        .child(crate::windows_ui::caption_brand())
        .child(div().h_2().flex_none())
}

/// Empty sidebar space moves the window like the caption, with snapping.
/// Only macOS asks for Accessibility; the tour names it there.
pub(super) fn accessibility_title() -> &'static str {
    tr("Accessibility")
}

pub(super) fn window_drag_region(region: Div) -> Div {
    region
        .occlude()
        .window_control_area(gpui::WindowControlArea::Drag)
}

pub(super) fn caption(window: &mut Window) -> Option<AnyElement> {
    Some(crate::windows_ui::caption_controls(window).into_any_element())
}

/// Suspends the keyboard hook while Settings records a shortcut.
pub(super) fn set_hotkey_capture_active(active: bool) {
    crate::windows_input::set_capture_inhibited(active);
}

pub(super) fn hotkey_modifiers(modifiers: GpuiModifiers) -> HotkeyModifiers {
    let held = |down: bool| down.then_some(ModifierSide::Either);
    HotkeyModifiers {
        control: held(modifiers.control),
        option: held(modifiers.alt),
        shift: held(modifiers.shift),
        command: held(modifiers.platform),
        function: false,
    }
}

pub(super) fn hotkey_key(key: &str) -> std::result::Result<HotkeyKey, &'static str> {
    crate::windows_input::virtual_key(key)
        .map(|_| HotkeyKey::named(key))
        .map_err(|_| tr("Unsupported key"))
}

/// The keyboard hook cannot tell a single modifier from ordinary typing,
/// and Paste Last and Voice Action fire on a key press.
pub(super) fn validate_hotkey(
    kind: HotkeyKind,
    binding: &HotkeyBinding,
) -> std::result::Result<(), &'static str> {
    if kind != HotkeyKind::Dictation && binding.key.is_none() {
        return Err(tr("Add a key"));
    }
    WindowsHotkey::from(binding)
        .validate()
        .map_err(|_| tr("Use two modifiers or add a key"))
}

impl AppWindow {
    /// Models are prepared in this window.
    pub(super) fn platform_prepare_transcription(&mut self, _: &TranscriptionSelection) -> bool {
        false
    }

    pub(super) fn platform_cancel_transcription(&mut self) -> bool {
        false
    }

    pub(super) fn platform_preparation_status(&self) -> Option<PreparationStatus> {
        None
    }

    /// Shortcuts are saved with the window's settings.
    pub(super) fn platform_save_hotkey(&mut self, _: HotkeyKind, _: &HotkeyBinding) -> bool {
        false
    }

    pub(super) fn platform_capture_cancelled(&mut self) {}

    pub(super) fn poll_platform(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        // Windows has no application coordinator: the window commits the
        // model it prepared, and the host restarts the listener with it.
        if !self.preview
            && let Some(result) = self.transcription_preparation.poll()
        {
            let result =
                result.and_then(|selection| self.apply_transcription_selection(selection, cx));
            if let Err(error) = result {
                self.transcription_preparation.set_error(Some(error));
            }
            self.poll_transcription_download();
            changed = true;
        }
        let shown = host_view(&self.platform.host.borrow());
        if shown != self.platform.shown {
            self.platform.shown = shown;
            changed = true;
        }
        changed
    }

    pub(super) fn platform_opened(&mut self, _: Option<&AppWindowPreview>) {}

    pub(super) fn platform_refresh(&mut self) {}

    pub(super) fn platform_select_pane(&mut self, _: Pane) {}

    pub(super) fn render_platform_pane(
        &mut self,
        _: Pane,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<AnyElement> {
        None
    }

    pub(super) fn platform_settings_saved(&mut self) {
        self.platform
            .host
            .borrow_mut()
            .apply_settings(&self.settings);
    }

    pub(super) fn platform_closed(&mut self) {}

    pub(super) fn platform_settings_notice(&mut self, _: &mut Context<Self>) -> Option<AnyElement> {
        None
    }

    /// Settings records every shortcut this platform registers.
    pub(super) fn platform_shortcut_rows(&self) -> bool {
        true
    }

    pub(super) fn platform_settings_section(
        &mut self,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        Vec::new()
    }

    pub(super) fn platform_model_notice(
        &mut self,
        _: &mut Context<Self>,
    ) -> Option<(&'static str, &'static str, AnyElement)> {
        None
    }

    pub(super) fn platform_tour_permission(&mut self, _: crate::onboarding::Effect) {}

    pub(super) fn platform_relaunch(&mut self) -> bool {
        false
    }

    pub(super) fn platform_awaits_setup(&self) -> bool {
        self.platform.listen_after_setup
    }

    pub(super) fn platform_setup_ready(&mut self) {
        if std::mem::take(&mut self.platform.listen_after_setup) {
            self.platform.host.borrow_mut().start();
        }
    }

    pub(super) fn platform_poll_setup(&mut self) -> bool {
        false
    }

    pub(super) fn personal_commands_changed(&mut self) {}

    pub(super) fn platform_update_button(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Stateful<Div>> {
        matches!(
            self.platform.host.borrow().update_state(),
            UpdateCheck::Available { .. }
        )
        .then(|| {
            sidebar_update_button().on_click(cx.listener(|this, _, _, cx| {
                this.install_update(cx);
            }))
        })
    }

    /// Polls the keyboard state until the user presses a chord or Escape.
    pub(super) fn platform_capture_started(&mut self, cx: &mut Context<Self>) {
        if self.preview {
            return;
        }
        let mut capture = ChordCapture::new();
        cx.spawn(async move |this, cx| {
            loop {
                Timer::after(Duration::from_millis(16)).await;
                // Keys typed into another application never rebind a
                // shortcut.
                let poll = if crate::windows_input::app_is_foreground() {
                    capture.poll()
                } else {
                    ChordPoll::Pending
                };
                let captured = matches!(poll, ChordPoll::Captured(_));
                let listening = this.update(cx, |this, cx| {
                    if !this.hotkey_capture.is_listening() {
                        return false;
                    }
                    match poll {
                        ChordPoll::Pending => {}
                        ChordPoll::Cancelled => this.cancel_hotkey_capture(cx),
                        ChordPoll::Captured(hotkey) => {
                            this.save_hotkey_binding(HotkeyBinding::from(&hotkey), cx);
                        }
                    }
                    this.hotkey_capture.is_listening()
                });
                if !matches!(listening, Ok(true)) {
                    break;
                }
                if captured {
                    // A rejected chord: record the next one from scratch.
                    capture = ChordCapture::new();
                }
            }
        })
        .detach();
    }

    pub(super) fn platform_listener(&self) -> Option<ListenerView> {
        let shown = &self.platform.shown;
        Some(ListenerView {
            running: shown.running,
            status: shown.status.clone(),
            last_failure: shown.last_failure.clone(),
            error: shown.error.clone(),
        })
    }

    pub(super) fn set_listening(&mut self, enabled: bool) {
        let mut host = self.platform.host.borrow_mut();
        if enabled {
            host.start();
        } else {
            host.stop();
        }
        self.platform.shown = host_view(&host);
    }

    pub(super) fn platform_software_update(&self) -> Option<SoftwareUpdate> {
        Some(match self.platform.host.borrow().update_state() {
            UpdateCheck::Checking => SoftwareUpdate::Checking,
            UpdateCheck::Current => SoftwareUpdate::Current,
            UpdateCheck::Failed => SoftwareUpdate::Failed,
            UpdateCheck::Available { version, .. } => SoftwareUpdate::Available {
                version: Some(version),
            },
            UpdateCheck::ReadyToRestart { version, .. } => SoftwareUpdate::ReadyToRestart {
                version: Some(version),
            },
        })
    }

    pub(super) fn check_for_updates(&mut self) {
        self.platform.host.borrow_mut().check_for_updates();
    }

    pub(super) fn install_update(&mut self, cx: &mut Context<Self>) {
        let state = self.platform.host.borrow().update_state();
        match state {
            UpdateCheck::Available { url, .. } => cx.open_url(&url),
            UpdateCheck::ReadyToRestart { .. } => {
                // One relaunch only; a second click while the quit drains
                // would start a second instance.
                if self.platform.restart_scheduled {
                    return;
                }
                let restarted = self.platform.host.borrow_mut().restart_into_update();
                if restarted.is_ok() {
                    self.platform.restart_scheduled = true;
                    cx.quit();
                }
            }
            UpdateCheck::Checking | UpdateCheck::Current | UpdateCheck::Failed => {}
        }
    }
}

//! The Linux window's host: the dictation host in this process
//! (`linux_app::HostHandle`), which owns the listener, shortcuts, models,
//! History, and updates. A headless render without one reads the running
//! Hex's `hex status` instead.

use std::sync::Arc;

use color_eyre::Result;
use color_eyre::eyre::eyre;

use super::{HostState, ServiceView};
use crate::app_settings::{AppSettings, HotkeyBinding};
use crate::desktop_i18n::tr;
use crate::linux_app::HostHandle;
pub(super) use crate::linux_app::{HostRequest as Request, ShortcutTarget};
use crate::linux_input::LinuxHotkey;

/// The host registers recorded shortcuts (`SetShortcut`) and saves them.
pub(super) const HOST_SAVES_SHORTCUTS: bool = true;
/// Settings records shortcuts from the window's key events.
pub(super) const RECORDS_KEY_EVENTS: bool = true;
/// X11 and portal shortcuts paste with Ctrl+V; terminals want Ctrl+Shift+V.
pub(super) const TERMINAL_PASTE: bool = true;
/// Linux has no tray, so the window offers Quit Hex.
pub(super) const QUIT_IN_WINDOW: bool = true;

/// The host the window's app runs beside; none for a headless `--snap` of
/// another process's `hex status`.
pub(crate) type Connect = Option<HostHandle>;

pub(super) enum Backend {
    Host(HostHandle),
    /// `--snap --scene live` from a separate process.
    Status(Option<Box<crate::linux_service::Status>>, Option<String>),
}

impl Backend {
    pub(super) fn connect(host: Connect, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let Some(host) = host else {
            let mut backend = Self::Status(None, None);
            backend.refresh();
            return backend;
        };
        host.set_waker(wake);
        Self::Host(host)
    }

    pub(super) fn send(&mut self, request: Request) -> Result<()> {
        match self {
            Self::Host(host) => host.request(request),
            Self::Status(..) => Err(eyre!("This window only shows `hex status`")),
        }
    }

    /// Saves through the host, which adopts the settings at once.
    pub(super) fn save(&mut self, settings: &AppSettings) -> Result<()> {
        match self {
            Self::Host(host) => host.save_settings(settings.clone()),
            Self::Status(..) => Err(eyre!("This window only shows `hex status`")),
        }
    }

    /// The host drains its work, then asks the app to quit.
    pub(super) fn quit(&mut self) {
        if let Self::Host(host) = self {
            host.quit();
        }
    }

    pub(super) fn closed(&mut self) {
        if let Self::Host(host) = self {
            host.window_closed();
        }
    }

    pub(super) fn refresh(&mut self) {
        match self {
            Self::Host(_) => {}
            Self::Status(status, error) => {
                match crate::linux_service::request(crate::linux_service::Request::Status) {
                    Ok(state) => (*status, *error) = (Some(Box::new(state)), None),
                    Err(failure) => (*status, *error) = (None, Some(failure.to_string())),
                }
            }
        }
    }

    pub(super) fn error(&self) -> Option<String> {
        match self {
            Self::Host(_) => None,
            Self::Status(_, error) => error.clone(),
        }
    }

    pub(super) fn state(&self) -> Option<HostState> {
        let (desktop, settings, editing) = match self {
            Self::Host(host) => {
                let snapshot = host.snapshot();
                (snapshot.desktop, snapshot.settings, snapshot.editing)
            }
            Self::Status(status, _) => {
                let status = *status.clone()?;
                (status.desktop, status.settings, status.editing)
            }
        };
        Some(HostState {
            view: ServiceView {
                listener: desktop.listener,
                operation_error: desktop.operation_error,
                transcription: desktop.transcription,
                update_status: desktop.update_status,
                editing,
            },
            settings,
        })
    }

    /// Linux records shortcuts from the window's key events.
    pub(super) fn take_chord(&mut self) -> Option<super::settings::Chord> {
        None
    }
}

/// The host's History store: one store per process.
pub(super) fn history(
    connect: &Connect,
    _: crate::history::HistoryRetention,
) -> Option<crate::history::History> {
    connect.as_ref()?.history()
}

pub(super) fn validate_hotkey(
    _: ShortcutTarget,
    binding: &HotkeyBinding,
) -> Result<(), &'static str> {
    LinuxHotkey::from(binding)
        .validate()
        .map_err(|_| tr("Add a key Linux can register"))
}

/// Wayland never says which application is focused.
pub(super) fn focused_application_known() -> bool {
    !crate::linux_session::LinuxSession::detect().is_wayland()
}

/// Linux has no Launch at login row; autostart is the desktop entry.
pub(super) fn launch_at_login() -> Option<Result<bool, String>> {
    None
}

pub(super) fn set_launch_at_login(_: bool) -> Result<bool, String> {
    Err("Launch at login is not available on Linux".into())
}

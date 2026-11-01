//! The desktop's paste permission on Wayland desktops without a virtual
//! keyboard (GNOME, KDE): Hex sends its paste shortcut through the
//! RemoteDesktop portal, whose dialog asks once (#932). Onboarding and
//! Settings explain it and ask before a paste would, so the dialog never
//! shows unexplained.

use std::sync::{Mutex, PoisonError};

use super::Ui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PasteAccess {
    /// Not asked yet.
    Ask,
    /// The desktop's dialog is open.
    Asking,
    Allowed,
    /// Refused or failed: pastes stay on the clipboard.
    Declined,
    /// Onboarding moved on without it; the desktop asks at the first paste.
    Later,
}

impl PasteAccess {
    /// Onboarding can move on.
    pub(super) fn settled(self) -> bool {
        matches!(self, Self::Allowed | Self::Later)
    }
}

/// The desktop's answer, from the thread that asked.
static ANSWER: Mutex<Option<bool>> = Mutex::new(None);

/// Where this session pastes through the portal; `None` everywhere else.
#[cfg(target_os = "linux")]
pub(super) fn detect() -> Option<PasteAccess> {
    crate::linux::paste::pastes_through_portal().then(|| {
        if crate::linux::remote_desktop::RemoteDesktop::granted() {
            PasteAccess::Allowed
        } else {
            PasteAccess::Ask
        }
    })
}

/// Allow: opens the desktop's dialog. A fixture only shows it asking.
pub(super) fn ask(ui: &mut Ui) {
    if ui.platform.paste == Some(PasteAccess::Asking) {
        return;
    }
    ui.platform.paste = Some(PasteAccess::Asking);
    if ui.is_fixture() {
        return;
    }
    #[cfg(target_os = "linux")]
    {
        let waker = ui.modes.waker.clone();
        std::thread::spawn(move || {
            let allowed = crate::linux::remote_desktop::RemoteDesktop::ask()
                .inspect_err(|error| {
                    tracing::warn!(error = format!("{error:#}"), "paste permission not granted");
                })
                .is_ok();
            *ANSWER.lock().unwrap_or_else(PoisonError::into_inner) = Some(allowed);
            if let Some(waker) = waker {
                waker();
            }
        });
    }
}

/// Onboarding's Continue after a refusal.
pub(super) fn later(ui: &mut Ui) {
    ui.platform.paste = Some(PasteAccess::Later);
}

/// Adopts the desktop's answer; true when it arrived.
pub(super) fn poll(ui: &mut Ui) -> bool {
    let Some(allowed) = ANSWER.lock().unwrap_or_else(PoisonError::into_inner).take() else {
        return false;
    };
    ui.platform.paste = Some(if allowed {
        PasteAccess::Allowed
    } else {
        PasteAccess::Declined
    });
    true
}

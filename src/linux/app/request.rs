//! What a Linux window asks of its host: settings reloads, shortcut capture,
//! model choices, and the macOS-only consent and recording requests the
//! shared window type carries (Linux accepts and ignores those).

use crate::desktop::host::{DesktopAction, DesktopShortcut};
use crate::transcription::models::TranscriptionModelId;

use super::ShortcutTarget;

/// What the window asks of the host. Settings edits go through
/// [`super::HostHandle::save_settings`].
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum HostRequest {
    Desktop(DesktopAction),
    ReloadSettings,
    /// Stops listening while the window records a shortcut.
    CaptureShortcutFor(ShortcutTarget),
    CancelShortcut,
    /// The chord the window recorded, for the host to register and save.
    SetShortcut {
        target: ShortcutTarget,
        shortcut: DesktopShortcut,
    },
    ChooseModel {
        model: TranscriptionModelId,
        language: String,
    },
    CancelModel,
    /// Labs › Model switch shortcut: swaps the dictation model with the saved
    /// other one, through the same installed-only preparation as a picker
    /// choice.
    SwapModel,
    /// The window stopped or cancelled a consented recording; macOS only.
    RecordControl {
        #[allow(dead_code)]
        id: u64,
        #[allow(dead_code)]
        stop: bool,
    },
    /// The window withdrew a client's grant; macOS only.
    RevokePermission {
        id: u64,
    },
    RevokeRecordGrant {
        #[allow(dead_code)]
        id: u64,
    },
}

//! What a Linux window asks of its host: settings reloads, shortcut capture,
//! model choices, and the consented recordings' Stop, Cancel, and Revoke.

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
    /// The window stopped (`stop`) or cancelled a consented recording.
    RecordControl {
        id: u64,
        stop: bool,
    },
    /// The window withdrew a saved permission (Labs › Browser access).
    RevokePermission {
        id: u64,
    },
    /// The window withdrew a client's grant to start recordings.
    RevokeRecordGrant {
        id: u64,
    },
}

//! What the window and the menu bar ask the macOS runtime, and the shortcut
//! targets they can record, in the Linux service's vocabulary so the shared
//! panes send the same requests on every platform.

use crate::desktop::host::{DesktopAction, DesktopShortcut};
use crate::transcription::models::{TranscriptionModelId, TranscriptionSelection};

/// What the window asks the runtime, in the Linux service's vocabulary so
/// the shared panes send the same requests on every platform.
#[derive(Clone, Debug)]
pub(crate) enum Request {
    /// Update actions go to the updater from the window (`rune_ui::host`).
    Desktop(#[allow(dead_code)] DesktopAction),
    /// Handled in the window (`rune_ui::host`): an AppKit event monitor.
    CaptureShortcutFor(#[allow(dead_code)] ShortcutTarget),
    CancelShortcut,
    /// Linux's service registers recorded shortcuts; on macOS the window
    /// saves them and the listener follows `settings.json`.
    #[allow(dead_code)]
    SetShortcut {
        target: ShortcutTarget,
        shortcut: DesktopShortcut,
    },
    ChooseModel {
        model: TranscriptionModelId,
        language: String,
    },
    /// The menu bar's model list: only installed models, never a download.
    SelectInstalledModel(TranscriptionModelId),
    /// `hex model use`: this exact selection, installed only, never a download.
    UseInstalledSelection(TranscriptionSelection),
    /// Snapshot from the main-thread TIS observer, not a shortcut event.
    InputSourceChanged(Option<String>),
    CancelModel,
    /// The window saved `settings.json`; adopt it.
    ReloadSettings,
}

/// A shortcut the listener registers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ShortcutTarget {
    #[default]
    Dictation,
    PasteLast,
    #[cfg(hex_capture)]
    Capture,
    /// Labs › Annotate 15 s replay shortcut.
    #[cfg(hex_capture)]
    CaptureReplay,
    RawDictation,
    /// Labs › Second dictation shortcut.
    SecondDictation,
    /// Labs › More dictation shortcuts (macOS): row `index`.
    ExtraDictation(usize),
    /// Toggle mode's cancel shortcut.
    Cancel,
    /// Labs › Screenshots.
    Screenshot,
    /// Labs › Screenshots' record shortcut.
    ScreenshotRecord,
}

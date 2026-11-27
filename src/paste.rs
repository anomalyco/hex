//! Clipboard insertion and generation-safe clipboard restoration.

pub(crate) mod clipboard_restore;

#[cfg(all(target_os = "macos", hex_capture))]
pub(crate) mod browse;
#[cfg(all(target_os = "macos", hex_capture))]
pub(crate) mod capture;
#[cfg(all(target_os = "macos", hex_capture))]
pub(crate) mod gesture;
#[cfg(all(target_os = "macos", hex_capture))]
pub(crate) mod recent;
#[cfg(target_os = "macos")]
pub(crate) mod return_to_app;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(all(target_os = "macos", hex_capture, test))]
use macos::ClipboardFlavor;
#[cfg(all(target_os = "macos", hex_capture))]
use macos::{
    CLIPBOARD_MANAGER_MARKERS, ClipboardRestore, ClipboardSnapshot, capture_clipboard,
    restore_clipboard, write_items,
};
#[cfg(target_os = "macos")]
pub use macos::{PasteMode, Paster};
#[cfg(target_os = "macos")]
pub(crate) use macos::{note_paste, paste_generation};

/// Whether a paste must be refused because macOS 27.3 split posting key events
/// out of Accessibility and this process has not been granted it. Below 27.3
/// the split does not exist, and a stale preflight never blocks a paste.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) const fn keystroke_refused(split: bool, allowed: bool) -> bool {
    split && !allowed
}

/// What a refused paste says: the posting grant is missing, so nothing can
/// land until Hex is added to System & App Protection Override. It is also
/// the HUD notice's English source and its i18n key.
pub(crate) const POST_EVENT_REFUSED: &str = "macOS isn't letting Hex paste. Add Hex in System Settings > Privacy & Security > \
     System & App Protection Override, then restart Hex.";

/// Whether `message` is that refusal: a failure the HUD explains even with
/// Labs › Failure notices off, because nothing else says why the words never
/// landed.
pub(crate) fn refused_by_macos(message: &str) -> bool {
    message.contains(POST_EVENT_REFUSED)
}

/// What a Windows paste says when `SendInput` rejects its chord: an elevated
/// window or security software blocking Hex's simulated input (#2975). The
/// transcript stays on the clipboard. It is also the HUD notice's English
/// source and its i18n key.
pub(crate) const WINDOWS_PASTE_BLOCKED: &str = "Windows blocked Hex's paste. Your words are on the clipboard: \
     press Ctrl+V. Security software or an app running as administrator can cause this.";

/// Whether `message` is that Windows refusal, which, like the macOS one, the
/// HUD explains even with Labs › Failure notices off.
pub(crate) fn blocked_by_windows(message: &str) -> bool {
    message.contains(WINDOWS_PASTE_BLOCKED)
}

/// Refuses a paste that would post a keystroke when macOS 27.3 has not
/// granted posting, before anything touches the clipboard. Every paste path
/// calls it: dictation, Paste last dictation, and captures.
#[cfg(target_os = "macos")]
pub(crate) fn ensure_keystroke_access() -> color_eyre::Result<()> {
    macos::ensure_keystroke_access()
}

#[cfg(test)]
mod tests {
    use super::keystroke_refused;

    #[test]
    fn a_split_paste_needs_the_post_event_grant() {
        assert!(keystroke_refused(true, false));
        assert!(!keystroke_refused(true, true));
        assert!(
            !keystroke_refused(false, false),
            "below 27.3 the Accessibility grant path is unchanged"
        );
        assert!(!keystroke_refused(false, true));
    }
}

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

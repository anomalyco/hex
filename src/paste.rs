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

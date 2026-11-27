//! Putting a Rune overlay's panel back on every Space (macOS). Rune makes
//! each overlay panel join every Space, but an activation-policy change (the
//! Dock icon shown or hidden) can tie it to one Space while AppKit still
//! reports the old mask (#1725), and Rune repairs that only as a panel shows.
//! The dictation HUD ([`super::macos`]) and the screenshot cards
//! (`rune_ui::screenshot_thumbnail::spaces`) both repair their panels with
//! these; [`super::rehome`] decides when.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSWindow, NSWindowCollectionBehavior};

/// The collection behaviors Rune gives an overlay's panel when it makes it
/// (`rune`'s `platform/macos.rs`): on every Space, ordered with it, out of the
/// window cycle, and over another application's full-screen Space too (#75).
fn intended_behavior() -> NSWindowCollectionBehavior {
    NSWindowCollectionBehavior::CanJoinAllSpaces
        | NSWindowCollectionBehavior::Stationary
        | NSWindowCollectionBehavior::IgnoresCycle
        | NSWindowCollectionBehavior::FullScreenAuxiliary
        // macOS 13+; earlier versions ignore the bit.
        | NSWindowCollectionBehavior::CanJoinAllApplications
}

/// Clears the panel's behaviors and sets the intended ones again. A read-back
/// of a broken mask can equal the intended one, and AppKit then ignores the
/// write, so the clear makes it a change again.
pub(crate) fn apply_behavior(panel: &NSWindow) {
    panel.setCollectionBehavior(NSWindowCollectionBehavior::Default);
    panel.setCollectionBehavior(intended_behavior());
}

/// The behaviors a re-home orders the panel in with: the intended ones with
/// `MoveToActiveSpace` in place of `CanJoinAllSpaces` (AppKit takes only one
/// of the two). Writing the intended mask back does not move a panel the
/// window server has tied to one Space, notably another application's
/// full-screen Space while Hex is a Regular (Dock) app (#2875); ordering it in
/// with this one should put it on the active Space, as AppKit documents for
/// `MoveToActiveSpace` (not yet observed fixing #2875 on hardware).
fn pull_behavior() -> NSWindowCollectionBehavior {
    let all_spaces = NSWindowCollectionBehavior::CanJoinAllSpaces.0;
    NSWindowCollectionBehavior(
        (intended_behavior().0 & !all_spaces) | NSWindowCollectionBehavior::MoveToActiveSpace.0,
    )
}

/// Orders the panel out, orders it back in regardless with
/// [`pull_behavior`] so it should land on whichever Space is active, then
/// writes the intended behaviors so it joins every Space from there. That
/// write has no clear first, unlike [`apply_behavior`]: the mask is
/// [`pull_behavior`] at that point, so the write is a change AppKit applies,
/// and a clear would leave the shown panel without `FullScreenAuxiliary` on
/// another application's full-screen Space, which could move it off again.
/// This reorders the panel but leaves Rune's presence state and alpha alone.
pub(crate) fn rehome_panel(panel: &NSWindow) {
    panel.orderOut(None);
    panel.setCollectionBehavior(pull_behavior());
    panel.orderFrontRegardless();
    panel.setCollectionBehavior(intended_behavior());
}

/// The app's window numbered `number`, if it has one.
pub(crate) fn find_panel(mtm: MainThreadMarker, number: u32) -> Option<Retained<NSWindow>> {
    NSApplication::sharedApplication(mtm)
        .windows()
        .iter()
        .find(|window| window.windowNumber() == number as isize)
}

//! Whether the window opens when Hex starts. macOS tells a login-item launch
//! apart in the open-application Apple event, so Launch at login starts Hex
//! quietly in the menu bar even with the Dock icon shown (#1321).

use std::sync::atomic::{AtomicBool, Ordering};

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager};

use super::MenuTarget;

const OPEN_APPLICATION: u32 = u32::from_be_bytes(*b"oapp");
const PROP_DATA: u32 = u32::from_be_bytes(*b"prdt");
const LAUNCHED_AS_LOGIN_ITEM: u32 = u32::from_be_bytes(*b"lgit");

/// What the window does when Hex starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaunchWindow {
    /// Setup needs the user, or the menu bar can't bring the window back.
    Open,
    /// The Dock icon is shown: open, unless macOS started Hex at login.
    OpenUnlessLoginItem,
    /// Menu-bar only.
    Quiet,
}

impl LaunchWindow {
    pub(crate) fn decide(
        show_dock_icon: bool,
        setup_ready: bool,
        onboarding_completed: bool,
        status_item_available: bool,
    ) -> Self {
        if !setup_ready || !onboarding_completed || !status_item_available {
            Self::Open
        } else if show_dock_icon {
            Self::OpenUnlessLoginItem
        } else {
            Self::Quiet
        }
    }

    pub(crate) fn opens(self, launched_as_login_item: bool) -> bool {
        match self {
            Self::Open => true,
            Self::OpenUnlessLoginItem => !launched_as_login_item,
            Self::Quiet => false,
        }
    }
}

/// Set by a login-item launch until Hex's first activation, which macOS
/// sends with the launch and must not open the window (see
/// [`super::activation`]).
static LOGIN_LAUNCH_ACTIVATION: AtomicBool = AtomicBool::new(false);

/// Calls the target's `applicationDidFinishLaunching:` once AppKit has
/// started. Register before the loop runs.
pub(super) fn observe_launch(target: &MenuTarget) {
    // SAFETY: the target outlives the observer (kept in LAUNCH, then PUMP,
    // for the app's life), and the selector matches its method.
    unsafe {
        let _: () = msg_send![
            &*objc2_foundation::NSNotificationCenter::defaultCenter(),
            addObserver: target,
            selector: sel!(applicationDidFinishLaunching:),
            name: objc2_app_kit::NSApplicationDidFinishLaunchingNotification,
            object: std::ptr::null::<AnyObject>(),
        ];
    }
}

/// Whether macOS started Hex as a login item. Valid only while AppKit
/// finishes launching, inside the open-application Apple event.
pub(super) fn launched_as_login_item() -> bool {
    let Some(event) = NSAppleEventManager::sharedAppleEventManager().currentAppleEvent() else {
        return false;
    };
    // SAFETY: `eventID` and `paramDescriptorForKeyword:` take and return
    // four-character codes and an optional descriptor, as declared here.
    let (event_id, data): (u32, Option<Retained<NSAppleEventDescriptor>>) = unsafe {
        (
            msg_send![&*event, eventID],
            msg_send![&*event, paramDescriptorForKeyword: PROP_DATA],
        )
    };
    let login_item = event_id == OPEN_APPLICATION
        && data.is_some_and(|data| data.enumCodeValue() == LAUNCHED_AS_LOGIN_ITEM);
    if login_item {
        LOGIN_LAUNCH_ACTIVATION.store(true, Ordering::Relaxed);
    }
    login_item
}

/// Whether this activation is the one a login-item launch brings; true once.
pub(super) fn take_login_launch_activation() -> bool {
    LOGIN_LAUNCH_ACTIVATION.swap(false, Ordering::Relaxed)
}

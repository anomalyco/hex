//! Whether the user demonstrably cannot dictate: the display is asleep, the
//! session is locked, or the Mac is asleep. The dictation owner releases the
//! warm microphone for as long as any of these holds, because an open input
//! stream keeps coreaudiod's PreventUserIdleSystemSleep assertion alive, and
//! an idle Mac would then never sleep (#1953). Only macOS reports these;
//! elsewhere every reason stays clear.

use std::sync::atomic::{AtomicBool, Ordering};

/// One reason the person cannot be dictating, held while it lasts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) enum Reason {
    DisplayAsleep,
    SystemAsleep,
    SessionLocked,
}

/// The reasons held right now. Each is tracked on its own, so a screen that
/// sleeps while the session is locked clears only once both are over.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Reasons {
    display_asleep: bool,
    system_asleep: bool,
    session_locked: bool,
}

impl Reasons {
    /// Whether the warm microphone is released now: any reason holds.
    pub(crate) fn released(self) -> bool {
        self.display_asleep || self.system_asleep || self.session_locked
    }
}

static DISPLAY_ASLEEP: AtomicBool = AtomicBool::new(false);
static SYSTEM_ASLEEP: AtomicBool = AtomicBool::new(false);
static SESSION_LOCKED: AtomicBool = AtomicBool::new(false);

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn slot(reason: Reason) -> &'static AtomicBool {
    match reason {
        Reason::DisplayAsleep => &DISPLAY_ASLEEP,
        Reason::SystemAsleep => &SYSTEM_ASLEEP,
        Reason::SessionLocked => &SESSION_LOCKED,
    }
}

fn held() -> Reasons {
    Reasons {
        display_asleep: DISPLAY_ASLEEP.load(Ordering::Acquire),
        system_asleep: SYSTEM_ASLEEP.load(Ordering::Acquire),
        session_locked: SESSION_LOCKED.load(Ordering::Acquire),
    }
}

/// Whether the person demonstrably cannot dictate, so a warm microphone is
/// released even with `release_microphone_while_idle` off.
pub fn released() -> bool {
    held().released()
}

/// Holds or clears one reason for the app's life, logging each transition
/// once. The observers below only store: the listener reads [`released`] on
/// its own thread every pass.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn hold(reason: Reason, held: bool) {
    if slot(reason).swap(held, Ordering::AcqRel) == held {
        return;
    }
    tracing::info!(
        ?reason,
        held,
        released = released(),
        "dictation availability changed"
    );
}

/// Starts watching the display, the session, and system sleep. Call once on
/// the app's main thread, whose run loop delivers the notifications; nothing
/// does where no OS reports them.
pub fn start() {
    #[cfg(target_os = "macos")]
    macos::start();
}

#[cfg(target_os = "macos")]
mod macos {
    //! The two sources macOS reports through: NSWorkspace for the display and
    //! the system, and the distributed notification center for the session.

    use std::ffi::c_void;
    use std::ptr::NonNull;
    use std::sync::Once;

    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceScreensDidSleepNotification,
        NSWorkspaceScreensDidWakeNotification, NSWorkspaceWillSleepNotification,
    };
    use objc2_core_foundation::{CFBoolean, CFDictionary, CFRetained, CFString, CFType};
    use objc2_foundation::{NSNotification, NSNotificationName};

    use super::{Reason, hold};

    /// Locked and unlocked, as the session posts them.
    const SCREEN_LOCKED: &str = "com.apple.screenIsLocked";
    const SCREEN_UNLOCKED: &str = "com.apple.screenIsUnlocked";
    /// The session dictionary's lock state: a CFBoolean while locked.
    const SESSION_LOCKED_KEY: &str = "CGSSessionScreenIsLocked";
    /// `DeliverImmediately`, including while Hex is in the background; what
    /// `keyboard_input_sources` uses for its distributed observer.
    const DELIVER_IMMEDIATELY: isize = 4;

    type Ref = *const c_void;
    type Callback = unsafe extern "C" fn(Ref, *mut c_void, Ref, Ref, Ref);

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFNotificationCenterGetDistributedCenter() -> Ref;
        fn CFNotificationCenterAddObserver(
            center: Ref,
            observer: Ref,
            callback: Callback,
            name: Ref,
            object: Ref,
            suspension_behavior: isize,
        );
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        /// The main display's ID, and whether it is asleep. The screens' sleep
        /// notification only fires on change, so launch reads these.
        fn CGMainDisplayID() -> u32;
        fn CGDisplayIsAsleep(display: u32) -> u32;
        /// A +1 dictionary of this GUI session's properties, or null when
        /// there is none; `SESSION_LOCKED_KEY`'s value is a CFBoolean.
        fn CGSessionCopyCurrentDictionary() -> *mut CFDictionary;
    }

    /// Registers the workspace and session observers once, for the app's
    /// life; the app's main thread has the run loop that delivers them.
    pub(super) fn start() {
        let Some(_mtm) = MainThreadMarker::new() else {
            tracing::warn!(
                "display sleep and the locked session can only be watched from the main thread"
            );
            return;
        };
        static STARTED: Once = Once::new();
        STARTED.call_once(|| {
            observe_workspace();
            observe_session();
            // The observers only report changes: a Mac already asleep or
            // locked when Hex launched posted those before it listened.
            seed();
        });
    }

    /// Reads the display and lock state a launch starts in.
    fn seed() {
        // SAFETY: a plain CoreGraphics read of the main display's state.
        hold(Reason::DisplayAsleep, unsafe {
            CGDisplayIsAsleep(CGMainDisplayID()) != 0
        });
        // SAFETY: the session dictionary follows the Copy rule, so this
        // reference owns it and releases it below; a null return means no GUI
        // session to ask, and the lock value is a CFBoolean when present.
        let locked =
            NonNull::new(unsafe { CGSessionCopyCurrentDictionary() }).is_some_and(|session| {
                let session = unsafe { CFRetained::from_raw(session) };
                let key = CFString::from_static_str(SESSION_LOCKED_KEY);
                // SAFETY: the dictionary lives while it is read here, and its
                // lock value is a CFBoolean when present.
                unsafe {
                    let value = session.value((&*key as *const CFString).cast());
                    (value as *const CFType)
                        .as_ref()
                        .and_then(CFType::downcast_ref::<CFBoolean>)
                        .is_some_and(CFBoolean::value)
                }
            });
        hold(Reason::SessionLocked, locked);
    }

    fn observe_workspace() {
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        // SAFETY: reading the notification names, which AppKit owns forever.
        let listeners = unsafe {
            [
                (
                    NSWorkspaceScreensDidSleepNotification,
                    Reason::DisplayAsleep,
                    true,
                ),
                (
                    NSWorkspaceScreensDidWakeNotification,
                    Reason::DisplayAsleep,
                    false,
                ),
                (NSWorkspaceWillSleepNotification, Reason::SystemAsleep, true),
                (NSWorkspaceDidWakeNotification, Reason::SystemAsleep, false),
            ]
        };
        for (name, reason, held) in listeners {
            observe(&center, name, reason, held);
        }
    }

    /// Calls [`hold`] once per change of `name`, on the thread the workspace
    /// posts it from (the main thread).
    fn observe(
        center: &objc2_foundation::NSNotificationCenter,
        name: &NSNotificationName,
        reason: Reason,
        held: bool,
    ) {
        let block = RcBlock::new(move |_: NonNull<NSNotification>| hold(reason, held));
        // SAFETY: the workspace's center keeps the observer until it is
        // removed, and it never is; the block only stores an atomic.
        let observer = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
        };
        // The center holds its own reference; this one lives as long as the
        // process, like the registration.
        std::mem::forget(observer);
    }

    fn observe_session() {
        // SAFETY: the process-wide distributed center, read only.
        let center = unsafe { CFNotificationCenterGetDistributedCenter() };
        if center.is_null() {
            tracing::warn!("no distributed notification center; the locked session is unwatched");
            return;
        }
        unsafe extern "C" fn on_locked(_: Ref, _: *mut c_void, _: Ref, _: Ref, _: Ref) {
            hold(Reason::SessionLocked, true);
        }
        unsafe extern "C" fn on_unlocked(_: Ref, _: *mut c_void, _: Ref, _: Ref, _: Ref) {
            hold(Reason::SessionLocked, false);
        }
        let listeners: [(&str, Callback); 2] =
            [(SCREEN_LOCKED, on_locked), (SCREEN_UNLOCKED, on_unlocked)];
        for (name, callback) in listeners {
            let name = CFString::from_static_str(name);
            let name: Ref = (&*name as *const CFString).cast();
            // SAFETY: the center copies the name, and a null observer passes
            // no client pointer to the callback.
            unsafe {
                CFNotificationCenterAddObserver(
                    center,
                    std::ptr::null(),
                    callback,
                    name,
                    std::ptr::null(),
                    DELIVER_IMMEDIATELY,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each reason releases the warm microphone on its own, and one ending
    /// never clears the others.
    #[test]
    fn any_held_reason_alone_releases_the_microphone() {
        assert!(!Reasons::default().released());
        for reasons in [
            Reasons {
                display_asleep: true,
                ..Default::default()
            },
            Reasons {
                system_asleep: true,
                ..Default::default()
            },
            Reasons {
                session_locked: true,
                ..Default::default()
            },
        ] {
            assert!(reasons.released());
        }
        assert!(
            Reasons {
                display_asleep: true,
                session_locked: true,
                ..Default::default()
            }
            .released()
        );
    }

    /// The reasons the observers hold are independent: a screen that wakes
    /// while the session is still locked keeps the microphone released.
    #[test]
    fn each_reason_holds_and_clears_on_its_own() {
        for reason in [
            Reason::DisplayAsleep,
            Reason::SystemAsleep,
            Reason::SessionLocked,
        ] {
            hold(reason, true);
        }
        assert!(released());
        hold(Reason::DisplayAsleep, false);
        assert!(released(), "a locked session still releases the microphone");
        hold(Reason::SystemAsleep, false);
        assert!(released(), "a locked session still releases the microphone");
        hold(Reason::SessionLocked, false);
        assert!(!released());
    }
}

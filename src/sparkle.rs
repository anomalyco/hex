use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};

use color_eyre::eyre::{Result, WrapErr, eyre};
use libloading::Library;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, Bool, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, msg_send};
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSError, NSInteger, NSObject};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum UpdateStatus {
    Unavailable,
    Idle,
    Checking,
    UpToDate,
    UpdateAvailable,
}

static UPDATE_STATUS: AtomicU8 = AtomicU8::new(UpdateStatus::Unavailable as u8);

objc2::extern_protocol!(
    /// # Safety
    /// Implementations must match Sparkle's Objective-C callback signatures and
    /// must not retain borrowed callback arguments without taking ownership.
    #[allow(clippy::missing_safety_doc)] // objc2's trait expansion hides these docs from Clippy.
    unsafe trait SPUUpdaterDelegate: NSObjectProtocol {}
);

objc2::define_class!(
    #[unsafe(super = NSObject)]
    #[name = "HexSparkleUpdaterDelegate"]
    #[ivars = &'static AtomicU8]
    struct SparkleDelegate;

    unsafe impl NSObjectProtocol for SparkleDelegate {}

    // Sparkle 2.9.4's SPUUpdaterDelegate.h: object arguments are borrowed,
    // SPUUpdateCheck is NSInteger, and only the completion error is nullable.
    unsafe impl SPUUpdaterDelegate for SparkleDelegate {
        #[unsafe(method(updater:didFindValidUpdate:))]
        fn did_find_update(&self, _updater: &AnyObject, _item: &AnyObject) {
            self.ivars()
                .store(UpdateStatus::UpdateAvailable as u8, Ordering::Release);
        }

        #[unsafe(method(updaterDidNotFindUpdate:error:))]
        fn did_not_find_update(&self, _updater: &AnyObject, _error: &NSError) {
            self.ivars()
                .store(UpdateStatus::UpToDate as u8, Ordering::Release);
        }

        #[unsafe(method(updater:didFinishUpdateCycleForUpdateCheck:error:))]
        fn did_finish_update_cycle(
            &self,
            _updater: &AnyObject,
            _update_check: NSInteger,
            _error: Option<&NSError>,
        ) {
            // No update is also an error (SUNoUpdateError). Keep that confirmation,
            // and keep a found update available when installation is deferred.
            clear_status(self.ivars(), UpdateStatus::Checking);
        }
    }
);

impl SparkleDelegate {
    fn new(status: &'static AtomicU8) -> Retained<Self> {
        let this = Self::alloc().set_ivars(status);
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    static UPDATER: RefCell<Option<SparkleUpdater>> = const { RefCell::new(None) };
}

struct SparkleUpdater {
    controller: Retained<AnyObject>,
    // Both the controller and updater hold their delegate weakly.
    _delegate: Retained<SparkleDelegate>,
    _framework: Library,
}

pub fn start() {
    if let Err(error) = start_inner() {
        tracing::error!(%error, "could not start Sparkle updater");
    }
}

fn start_inner() -> Result<()> {
    MainThreadMarker::new().ok_or_else(|| eyre!("Sparkle must start on the main thread"))?;
    if UPDATER.with(|updater| updater.borrow().is_some()) {
        return Ok(());
    }
    let Some(framework_path) = framework_path()? else {
        tracing::debug!("Sparkle is unavailable outside the packaged app");
        return Ok(());
    };

    // Loading the framework registers its Objective-C classes without making
    // CLI and test binaries depend on an adjacent Sparkle.framework.
    let framework = unsafe { Library::new(&framework_path) }
        .wrap_err_with(|| format!("could not load {}", framework_path.display()))?;
    // define_class! needs the framework's protocol registered before allocation.
    AnyProtocol::get(c"SPUUpdaterDelegate")
        .ok_or_else(|| eyre!("Sparkle updater delegate protocol is unavailable"))?;
    let class = AnyClass::get(c"SPUStandardUpdaterController")
        .ok_or_else(|| eyre!("Sparkle updater class is unavailable"))?;
    let delegate = SparkleDelegate::new(&UPDATE_STATUS);
    let controller: Retained<AnyObject> = unsafe {
        let allocated: *mut AnyObject = msg_send![class, alloc];
        let controller: *mut AnyObject = msg_send![
            allocated,
            initWithStartingUpdater: Bool::NO,
            updaterDelegate: ProtocolObject::<dyn SPUUpdaterDelegate>::from_ref(&*delegate),
            userDriverDelegate: std::ptr::null_mut::<AnyObject>()
        ];
        Retained::from_raw(controller)
            .ok_or_else(|| eyre!("Sparkle updater initialization returned null"))?
    };
    unsafe {
        let updater: *mut AnyObject = msg_send![&*controller, updater];
        let updater = updater
            .as_ref()
            .ok_or_else(|| eyre!("Sparkle controller returned no updater"))?;
        let mut error: Option<Retained<NSError>> = None;
        let started: Bool = msg_send![updater, startUpdater: &mut error];
        if !started.as_bool() {
            return Err(eyre!("Sparkle updater failed to start: {error:?}"));
        }
        UPDATE_STATUS.store(UpdateStatus::Idle as u8, Ordering::Release);
        let automatically_checks: Bool = msg_send![updater, automaticallyChecksForUpdates];
        if automatically_checks.as_bool() {
            let _: () = msg_send![updater, checkForUpdatesInBackground];
        }
    }
    UPDATER.with(|updater| {
        *updater.borrow_mut() = Some(SparkleUpdater {
            controller,
            _delegate: delegate,
            _framework: framework,
        });
    });
    Ok(())
}

pub fn check_for_updates() {
    let Some(mtm) = MainThreadMarker::new() else {
        tracing::warn!("Sparkle update checks must run on the main thread");
        return;
    };
    UPDATER.with(|updater| {
        let updater = updater.borrow();
        let Some(updater) = updater.as_ref() else {
            UPDATE_STATUS.store(UpdateStatus::Unavailable as u8, Ordering::Release);
            tracing::warn!("Sparkle updater is not available");
            return;
        };
        unsafe {
            let native_updater: *mut AnyObject = msg_send![&*updater.controller, updater];
            let can_check: Bool = msg_send![native_updater, canCheckForUpdates];
            let session_in_progress: Bool = msg_send![native_updater, sessionInProgress];
            begin_check(
                &UPDATE_STATUS,
                can_check.as_bool(),
                session_in_progress.as_bool(),
            );
            #[allow(deprecated)]
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
            let _: () =
                msg_send![&*updater.controller, checkForUpdates: std::ptr::null_mut::<AnyObject>()];
            let user_driver: *mut AnyObject = msg_send![&*updater.controller, userDriver];
            let _: () = msg_send![user_driver, showUpdateInFocus];
        }
    });
}

pub fn status() -> UpdateStatus {
    match UPDATE_STATUS.load(Ordering::Acquire) {
        value if value == UpdateStatus::Idle as u8 => UpdateStatus::Idle,
        value if value == UpdateStatus::Checking as u8 => UpdateStatus::Checking,
        value if value == UpdateStatus::UpToDate as u8 => UpdateStatus::UpToDate,
        value if value == UpdateStatus::UpdateAvailable as u8 => UpdateStatus::UpdateAvailable,
        _ => UpdateStatus::Unavailable,
    }
}

pub fn clear_confirmation() {
    clear_status(&UPDATE_STATUS, UpdateStatus::UpToDate);
}

fn begin_check(status: &AtomicU8, can_check: bool, session_in_progress: bool) {
    // An existing session may only be focused, and resuming a deferred install
    // need not emit didFindValidUpdate again. Neither should erase its result.
    if can_check && !session_in_progress {
        let _ = status.fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            (value == UpdateStatus::Idle as u8 || value == UpdateStatus::UpToDate as u8)
                .then_some(UpdateStatus::Checking as u8)
        });
    }
}

fn clear_status(status: &AtomicU8, expected: UpdateStatus) {
    let _ = status.compare_exchange(
        expected as u8,
        UpdateStatus::Idle as u8,
        Ordering::AcqRel,
        Ordering::Acquire,
    );
}

fn framework_path() -> Result<Option<PathBuf>> {
    let executable = std::env::current_exe()?;
    let Some(macos_dir) = executable.parent() else {
        return Ok(None);
    };
    let path = macos_dir.join("../Frameworks/Sparkle.framework/Versions/B/Sparkle");
    Ok(path.exists().then_some(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::rc::autoreleasepool;
    use objc2::runtime::ProtocolBuilder;
    use objc2::{ClassType, ProtocolType, sel};
    use objc2_foundation::NSString;

    #[test]
    fn status_transitions_preserve_results_and_unavailable() {
        for initial in [
            UpdateStatus::Unavailable,
            UpdateStatus::Idle,
            UpdateStatus::Checking,
            UpdateStatus::UpToDate,
            UpdateStatus::UpdateAvailable,
        ] {
            for (can_check, session_in_progress) in
                [(false, false), (false, true), (true, false), (true, true)]
            {
                let status = AtomicU8::new(initial as u8);
                begin_check(&status, can_check, session_in_progress);
                let expected = if can_check
                    && !session_in_progress
                    && matches!(initial, UpdateStatus::Idle | UpdateStatus::UpToDate)
                {
                    UpdateStatus::Checking
                } else {
                    initial
                };
                assert_eq!(status.load(Ordering::Acquire), expected as u8);
            }

            for cleared in [UpdateStatus::Checking, UpdateStatus::UpToDate] {
                let status = AtomicU8::new(initial as u8);
                clear_status(&status, cleared);
                let expected = if initial == cleared {
                    UpdateStatus::Idle
                } else {
                    initial
                };
                assert_eq!(status.load(Ordering::Acquire), expected as u8);
            }
        }
    }

    #[test]
    fn native_delegate_completion_allows_retry_and_preserves_results() {
        // Exercise objc_msgSend without loading Sparkle, starting an updater,
        // touching preferences, or showing UI. objc2 needs protocol metadata.
        let protocol = AnyProtocol::get(c"SPUUpdaterDelegate").unwrap_or_else(|| {
            let mut protocol = ProtocolBuilder::new(c"SPUUpdaterDelegate").unwrap();
            protocol.add_protocol(<dyn NSObjectProtocol>::protocol().unwrap());
            protocol.add_method_description::<(&AnyObject, &AnyObject), ()>(
                sel!(updater:didFindValidUpdate:),
                false,
            );
            protocol.add_method_description::<(&AnyObject, &NSError), ()>(
                sel!(updaterDidNotFindUpdate:error:),
                false,
            );
            protocol.add_method_description::<(&AnyObject, NSInteger, Option<&NSError>), ()>(
                sel!(updater:didFinishUpdateCycleForUpdateCheck:error:),
                false,
            );
            protocol.register()
        });

        autoreleasepool(|_| {
            static STATUS: AtomicU8 = AtomicU8::new(UpdateStatus::Idle as u8);
            let delegate = SparkleDelegate::new(&STATUS);
            assert!(SparkleDelegate::class().conforms_to(protocol));
            let completion = SparkleDelegate::class()
                .instance_method(sel!(updater:didFinishUpdateCycleForUpdateCheck:error:))
                .unwrap();
            assert_eq!(&*completion.return_type(), c"v");
            assert_eq!(completion.arguments_count(), 5);
            // The third explicit argument is nullable id; SPUUpdateCheck is
            // signed, pointer-width NSInteger, not a C int or unsigned enum.
            for (index, encoding) in [c"@", c":", c"@", c"q", c"@"].into_iter().enumerate() {
                assert_eq!(&*completion.argument_type(index).unwrap(), encoding);
            }

            let object = NSObject::new();
            let failure = unsafe {
                NSError::errorWithDomain_code_userInfo(
                    &NSString::from_str("NSURLErrorDomain"),
                    -1009,
                    None,
                )
            };
            // Both an offline failure and a cancellation (nil error) finish
            // Checking. A subsequent click must be able to start again.
            for error in [Some(&*failure), None] {
                begin_check(&STATUS, true, false);
                assert_eq!(STATUS.load(Ordering::Acquire), UpdateStatus::Checking as u8);
                unsafe {
                    let _: () = msg_send![
                        &*delegate,
                        updater: &*object,
                        didFinishUpdateCycleForUpdateCheck: 0 as NSInteger,
                        error: error
                    ];
                }
                assert_eq!(STATUS.load(Ordering::Acquire), UpdateStatus::Idle as u8);
            }

            let no_update = unsafe {
                NSError::errorWithDomain_code_userInfo(
                    &NSString::from_str("SUSparkleErrorDomain"),
                    1001,
                    None,
                )
            };
            begin_check(&STATUS, true, false);
            unsafe {
                let _: () = msg_send![
                    &*delegate, updaterDidNotFindUpdate: &*object, error: &*no_update
                ];
                let _: () = msg_send![
                    &*delegate,
                    updater: &*object,
                    didFinishUpdateCycleForUpdateCheck: 0 as NSInteger,
                    error: Some(&*no_update)
                ];
            }
            assert_eq!(STATUS.load(Ordering::Acquire), UpdateStatus::UpToDate as u8);
            clear_status(&STATUS, UpdateStatus::UpToDate);
            assert_eq!(STATUS.load(Ordering::Acquire), UpdateStatus::Idle as u8);

            unsafe {
                let _: () = msg_send![
                    &*delegate, updater: &*object, didFindValidUpdate: &*object
                ];
            }
            // Dismissal, resuming without another found callback, and a failed
            // installation must leave the known update accessible for retry.
            for error in [None, Some(&*failure)] {
                begin_check(&STATUS, true, false);
                assert_eq!(
                    STATUS.load(Ordering::Acquire),
                    UpdateStatus::UpdateAvailable as u8
                );
                unsafe {
                    let _: () = msg_send![
                        &*delegate,
                        updater: &*object,
                        didFinishUpdateCycleForUpdateCheck: 1 as NSInteger,
                        error: error
                    ];
                }
                assert_eq!(
                    STATUS.load(Ordering::Acquire),
                    UpdateStatus::UpdateAvailable as u8
                );
            }
        });
    }
}

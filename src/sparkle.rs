use std::cell::RefCell;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU8, Ordering};

use block2::RcBlock;
use color_eyre::eyre::{Result, WrapErr, eyre};
use libloading::Library;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, NSObjectProtocol, ProtocolObject};
use objc2::{MainThreadMarker, msg_send};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSString};

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

thread_local! {
    static UPDATER: RefCell<Option<SparkleUpdater>> = const { RefCell::new(None) };
}

struct SparkleUpdater {
    controller: Retained<AnyObject>,
    observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    _framework: Library,
}

impl Drop for SparkleUpdater {
    fn drop(&mut self) {
        let center = NSNotificationCenter::defaultCenter();
        for observer in &self.observers {
            unsafe { center.removeObserver(observer.as_ref()) };
        }
    }
}

pub fn start() {
    if let Err(error) = start_inner() {
        tracing::error!(%error, "could not start Sparkle updater");
    }
}

fn start_inner() -> Result<()> {
    MainThreadMarker::new().ok_or_else(|| eyre!("Sparkle must start on the main thread"))?;
    let Some(framework_path) = framework_path()? else {
        tracing::debug!("Sparkle is unavailable outside the packaged app");
        return Ok(());
    };

    // Loading the framework registers its Objective-C classes without making
    // CLI and test binaries depend on an adjacent Sparkle.framework.
    let framework = unsafe { Library::new(&framework_path) }
        .wrap_err_with(|| format!("could not load {}", framework_path.display()))?;
    let center = NSNotificationCenter::defaultCenter();
    let observers = [
        (
            "SUUpdaterDidFindValidUpdateNotification",
            UpdateStatus::UpdateAvailable,
        ),
        (
            "SUUpdaterDidNotFindUpdateNotification",
            UpdateStatus::UpToDate,
        ),
    ]
    .into_iter()
    .map(|(name, status)| {
        let name = NSString::from_str(name);
        let block = RcBlock::new(move |_: NonNull<NSNotification>| {
            UPDATE_STATUS.store(status as u8, Ordering::Release);
        });
        unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(&name), None, None, &block)
        }
    })
    .collect();
    let class = AnyClass::get(c"SPUStandardUpdaterController")
        .ok_or_else(|| eyre!("Sparkle updater class is unavailable"))?;
    let controller: Retained<AnyObject> = unsafe {
        let allocated: *mut AnyObject = msg_send![class, alloc];
        let controller: *mut AnyObject = msg_send![
            allocated,
            initWithStartingUpdater: Bool::YES,
            updaterDelegate: std::ptr::null_mut::<AnyObject>(),
            userDriverDelegate: std::ptr::null_mut::<AnyObject>()
        ];
        Retained::from_raw(controller)
            .ok_or_else(|| eyre!("Sparkle updater initialization returned null"))?
    };
    unsafe {
        let updater: *mut AnyObject = msg_send![&*controller, updater];
        let automatically_checks: Bool = msg_send![updater, automaticallyChecksForUpdates];
        if automatically_checks.as_bool() {
            let _: () = msg_send![updater, checkForUpdatesInBackground];
        }
    }
    UPDATER.with(|updater| {
        *updater.borrow_mut() = Some(SparkleUpdater {
            controller,
            observers,
            _framework: framework,
        });
    });
    UPDATE_STATUS.store(UpdateStatus::Idle as u8, Ordering::Release);
    Ok(())
}

pub fn check_for_updates() {
    UPDATER.with(|updater| {
        let updater = updater.borrow();
        let Some(updater) = updater.as_ref() else {
            UPDATE_STATUS.store(UpdateStatus::Unavailable as u8, Ordering::Release);
            tracing::warn!("Sparkle updater is not available");
            return;
        };
        UPDATE_STATUS.store(UpdateStatus::Checking as u8, Ordering::Release);
        unsafe {
            let _: () =
                msg_send![&*updater.controller, checkForUpdates: std::ptr::null_mut::<AnyObject>()];
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
    let _ = UPDATE_STATUS.compare_exchange(
        UpdateStatus::UpToDate as u8,
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

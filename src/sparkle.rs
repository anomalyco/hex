use std::cell::RefCell;
use std::path::PathBuf;

use color_eyre::eyre::{Result, WrapErr, eyre};
use libloading::Library;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::{MainThreadMarker, msg_send};

thread_local! {
    static UPDATER: RefCell<Option<SparkleUpdater>> = const { RefCell::new(None) };
}

struct SparkleUpdater {
    controller: Retained<AnyObject>,
    _framework: Library,
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
    UPDATER.with(|updater| {
        *updater.borrow_mut() = Some(SparkleUpdater {
            controller,
            _framework: framework,
        });
    });
    Ok(())
}

pub fn check_for_updates() {
    UPDATER.with(|updater| {
        let updater = updater.borrow();
        let Some(updater) = updater.as_ref() else {
            tracing::warn!("Sparkle updater is not available");
            return;
        };
        unsafe {
            let _: () =
                msg_send![&*updater.controller, checkForUpdates: std::ptr::null_mut::<AnyObject>()];
        }
    });
}

fn framework_path() -> Result<Option<PathBuf>> {
    let executable = std::env::current_exe()?;
    let Some(macos_dir) = executable.parent() else {
        return Ok(None);
    };
    let path = macos_dir.join("../Frameworks/Sparkle.framework/Versions/B/Sparkle");
    Ok(path.exists().then_some(path))
}

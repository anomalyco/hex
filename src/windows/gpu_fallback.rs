//! Which GPU Hex draws on. Rune takes the lowest-power adapter that drives a
//! display, which keeps a hybrid laptop's discrete GPU asleep. A broken
//! integrated GPU driver can crash Hex before its first frame (#662), and a
//! driver crash can't be caught, so the app marks its launch from when Rune
//! starts its first GPU device until a frame reaches the screen or it starts
//! to quit (a clean quit, or the Windows logoff or shutdown Rune's quit hook
//! hears before the process may end). A tray launch that never draws makes no
//! device and leaves no marker. A marker this build left behind means that
//! launch crashed while drawing: this one draws on the high-performance GPU,
//! and so does every launch of this build. An update tries the integrated GPU
//! again.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// This launch draws on the high-performance GPU.
static FALLBACK: AtomicBool = AtomicBool::new(false);
/// The marker is gone, or this launch never wrote one.
static SETTLED: AtomicBool = AtomicBool::new(false);
/// Rune started this launch's first GPU device.
static ARMED: AtomicBool = AtomicBool::new(false);

fn marker() -> Option<PathBuf> {
    crate::app_paths::support_dir()
        .ok()
        .map(|dir| dir.join("gpu-draw.pending"))
}

/// First thing in `main`, before any thread starts: after a crash this
/// build's marker records, Rune picks the high-performance adapter
/// (`WGPU_POWER_PREF`), unless the user chose one.
pub(crate) fn choose() {
    let Some(path) = marker() else {
        return;
    };
    match std::fs::read_to_string(&path) {
        Ok(version) if version == crate::VERSION => {}
        Ok(_) => {
            let _ = std::fs::remove_file(&path);
            return;
        }
        Err(_) => return,
    }
    if std::env::var_os("WGPU_POWER_PREF").is_some()
        || std::env::var_os("WGPU_ADAPTER_NAME").is_some()
    {
        return;
    }
    // SAFETY: `main`'s first statements, before any other thread can read
    // the environment.
    unsafe { std::env::set_var("WGPU_POWER_PREF", "high") };
    FALLBACK.store(true, Ordering::Relaxed);
}

/// The app's launch: says why it draws on the high-performance GPU, or
/// marks its drawing as started when Rune starts its first GPU device and
/// clears the mark at its first frame.
pub(crate) fn watch() {
    if FALLBACK.load(Ordering::Relaxed) {
        SETTLED.store(true, Ordering::Relaxed);
        tracing::warn!(
            marker = %marker().map(|path| path.display().to_string()).unwrap_or_default(),
            "a previous launch of this Hex crashed before drawing; drawing on the high-performance GPU until Hex updates or the marker is deleted"
        );
        return;
    }
    rune::on_gpu(|event| match event {
        rune::GpuEvent::Starting => arm(),
        rune::GpuEvent::Drew => disarm(),
    });
}

/// Marks this launch's drawing as started, once: a device made again later
/// (after the last surface released it) already drew in this launch.
fn arm() {
    if ARMED.swap(true, Ordering::Relaxed) || SETTLED.load(Ordering::Relaxed) {
        return;
    }
    let Some(path) = marker() else {
        SETTLED.store(true, Ordering::Relaxed);
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(error) = std::fs::write(&path, crate::VERSION) {
        tracing::warn!(%error, "could not write the GPU draw marker");
        SETTLED.store(true, Ordering::Relaxed);
    }
}

/// A frame reached the screen, or Hex quit cleanly: this launch's GPU works.
pub(crate) fn disarm() {
    if SETTLED.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Some(path) = marker() {
        let _ = std::fs::remove_file(path);
    }
}

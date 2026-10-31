//! How the macOS app process ends.
//!
//! `exit` (what `-[NSApplication terminate:]` and returning from `main` call)
//! runs C++ static destructors and `atexit` handlers while threads Hex does
//! not join are still running: CoreAudio's IO threads or ggml's residency-set
//! keep-alive. Those destructors tear down native globals under them; ggml's
//! Metal device list aborts in `ggml_metal_rsets_free` unless every Metal
//! buffer was already freed. So Hex drains its own work first, then ends with
//! `_exit`, which runs none of them. Nothing Hex keeps relies on them: the
//! drain flushes the event log, and settings, history, and meetings are saved
//! atomically when they change. A meeting recording at quit keeps its audio
//! and finishes its transcript at the next launch.
//!
//! The drain runs on the main thread, so a worker that never stops would hang
//! quit, logout, and updates. After [`DEADLINE`] the process ends anyway.
//! Dropping the transcription engine cancels its job and normally returns at
//! once; only a wedged native run reaches either bound, and `_exit` is what
//! makes leaving it running safe.

use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// How long quitting waits for the drain before ending the process anyway.
const DEADLINE: Duration = Duration::from_secs(5);

static QUITTING: AtomicBool = AtomicBool::new(false);

/// Whether the process has begun quitting. Work that would outlast the
/// drain, such as loading a model, checks this and stops instead.
pub(crate) fn quitting() -> bool {
    QUITTING.load(Ordering::Acquire)
}

/// Marks the process as quitting without ending it: the Linux and Windows
/// apps, which end by returning from `main` once their workers stop.
#[cfg(not(target_os = "macos"))]
pub(crate) fn begin_quitting() {
    QUITTING.store(true, Ordering::Release);
}

#[cfg(test)]
pub(crate) fn begin_quitting_for_test() {
    QUITTING.store(true, Ordering::Release);
}

/// Work that must finish before the process ends, run once by whichever
/// quit path gets there first.
pub(crate) struct Drain(Mutex<Option<Box<dyn FnOnce() + Send>>>);

impl Drain {
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(None))
    }

    pub(crate) fn set(&self, work: impl FnOnce() + Send + 'static) {
        *self.lock() = Some(Box::new(work));
    }

    /// Runs the drain if nothing has yet; reports whether this call ran it.
    pub(crate) fn run(&self) -> bool {
        let work = self.lock().take();
        let ran = work.is_some();
        if let Some(work) = work {
            work();
        }
        ran
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Box<dyn FnOnce() + Send>>> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }
}

/// Drains, then ends the process without running static destructors.
pub(crate) fn finish(drain: &Drain) -> ! {
    finish_within(drain, DEADLINE)
}

fn finish_within(drain: &Drain, deadline: Duration) -> ! {
    QUITTING.store(true, Ordering::Release);
    let _ = std::thread::Builder::new()
        .name("hex-exit-deadline".into())
        .spawn(move || {
            std::thread::sleep(deadline);
            tracing::warn!("Hex stopped before its workers finished");
            end_process();
        });
    drain.run();
    tracing::info!("Hex stopped");
    end_process()
}

fn end_process() -> ! {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    // SAFETY: `_exit` ends the process immediately; nothing runs after it.
    unsafe { libc::_exit(0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn a_drain_runs_once_whichever_path_comes_first() {
        let drain = Drain::new();
        let runs = Arc::new(AtomicUsize::new(0));
        let counted = runs.clone();
        drain.set(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        });
        assert!(drain.run());
        assert!(!drain.run());
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    const CHILD: &str = "HEX_PROCESS_EXIT_CHILD";

    /// The child half of `finishing_stops_the_listener_and_skips_static_destructors`.
    #[test]
    fn finishing_child() {
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        extern "C" fn static_destructor() {
            // What ggml's Metal teardown does with a live buffer.
            std::process::abort();
        }
        /// A native session the listener frees when it stops.
        struct Session;
        impl Drop for Session {
            fn drop(&mut self) {
                print!("freed ");
            }
        }
        static DRAIN: Drain = Drain::new();
        static STOP: AtomicBool = AtomicBool::new(false);
        // SAFETY: registers a plain C function with no captured state.
        unsafe { libc::atexit(static_destructor) };
        let listener = std::thread::spawn(|| {
            let _session = Session;
            while !STOP.load(Ordering::SeqCst) {
                std::thread::yield_now();
            }
        });
        DRAIN.set(move || {
            STOP.store(true, Ordering::SeqCst);
            listener.join().unwrap();
            print!("drained");
        });
        finish(&DRAIN);
    }

    /// The child half of `a_stuck_drain_still_ends_the_process`.
    #[test]
    fn stuck_child() {
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        static DRAIN: Drain = Drain::new();
        DRAIN.set(|| {
            loop {
                std::thread::park();
            }
        });
        finish_within(&DRAIN, Duration::from_millis(50));
    }

    fn run_child(name: &str) -> std::process::Output {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("desktop::process_exit::tests::{name}"),
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap()
    }

    #[test]
    fn finishing_stops_the_listener_and_skips_static_destructors() {
        let output = run_child("finishing_child");
        assert!(
            output.status.success(),
            "the child ran an atexit handler: {:?}",
            output.status
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("freed drained"),
            "the listener did not stop and free its session before the process ended"
        );
    }

    #[test]
    fn a_stuck_drain_still_ends_the_process() {
        assert!(run_child("stuck_child").status.success());
    }
}

//! The priority background transcription runs at (meeting chunks, notes,
//! and chapters), so it never competes with dictation for the CPU.

/// Runs `work` at background priority on the dictation inference thread,
/// then restores its priority.
pub(crate) fn at_background_priority<R>(work: impl FnOnce() -> R) -> R {
    os::at_background_priority(work)
}

/// Runs the calling thread at background priority for good.
pub(crate) fn deprioritize_background_thread() {
    os::lower_for_good();
}

#[cfg(target_os = "macos")]
mod os {
    pub(super) use crate::parakeet::{
        at_background_priority, deprioritize_background_thread as lower_for_good,
    };
}

#[cfg(target_os = "linux")]
mod os {
    /// An unprivileged thread cannot raise its priority again, so the
    /// engine runs background work at its own priority.
    pub(super) fn at_background_priority<R>(work: impl FnOnce() -> R) -> R {
        work()
    }

    /// Nice 10 for this thread.
    pub(super) fn lower_for_good() {
        // SAFETY: `gettid` has no arguments; `setpriority` with
        // `PRIO_PROCESS` and a thread id changes only that thread.
        let status = unsafe {
            let thread = libc::syscall(libc::SYS_gettid) as libc::id_t;
            libc::setpriority(libc::PRIO_PROCESS, thread, 10)
        };
        if status != 0 {
            tracing::debug!("could not lower background transcription priority");
        }
    }
}

#[cfg(target_os = "windows")]
mod os {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, GetThreadPriority, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };

    pub(super) fn lower_for_good() {
        lower();
    }

    pub(super) fn at_background_priority<R>(work: impl FnOnce() -> R) -> R {
        let previous = lower();
        let result = work();
        // SAFETY: The pseudo-handle names the calling thread only.
        unsafe { SetThreadPriority(GetCurrentThread(), previous) };
        result
    }

    /// Lowers the calling thread, returning its priority before.
    fn lower() -> i32 {
        // SAFETY: The pseudo-handle names the calling thread only.
        unsafe {
            let thread = GetCurrentThread();
            let previous = GetThreadPriority(thread);
            SetThreadPriority(thread, THREAD_PRIORITY_BELOW_NORMAL);
            previous
        }
    }
}

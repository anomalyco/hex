//! Whether this process is recording a meeting, for the quits and updates that
//! must wait for its audio to be finalised.
//!
//! The recorder holds one [`Recording`] for its whole run, so a quit or a
//! restart to update can see it from any thread without asking the controller.

use std::sync::atomic::{AtomicUsize, Ordering};

static RECORDING: AtomicUsize = AtomicUsize::new(0);

/// Whether a meeting is recording right now.
pub fn recording() -> bool {
    RECORDING.load(Ordering::Acquire) > 0
}

/// Marks this process as recording until dropped. Overlapping recorders (a
/// CLI recording beside the app's) each hold their own.
pub(super) struct Recording(());

impl Recording {
    pub(super) fn start() -> Self {
        RECORDING.fetch_add(1, Ordering::AcqRel);
        Self(())
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        RECORDING.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_holds_until_every_recorder_drops_it() {
        // No assertion about idleness: other tests may record at the same
        // time. Each guard's own hold is what this checks.
        let first = Recording::start();
        assert!(recording());
        let second = Recording::start();
        drop(first);
        assert!(recording(), "another recorder still holds it");
        drop(second);
    }
}

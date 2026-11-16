//! Whether this process is recording a meeting, for the quits and updates that
//! must wait for its audio to be finalised.
//!
//! The recorder holds one [`Recording`] for its whole run, so a quit or a
//! restart to update can see it from any thread without asking the controller.

use std::sync::atomic::{AtomicBool, Ordering};

static RECORDING: AtomicBool = AtomicBool::new(false);

/// Whether a meeting is recording right now.
pub fn recording() -> bool {
    RECORDING.load(Ordering::Acquire)
}

/// Marks this process as recording until dropped.
pub(super) struct Recording(());

impl Recording {
    pub(super) fn start() -> Self {
        RECORDING.store(true, Ordering::Release);
        Self(())
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        RECORDING.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_holds_until_the_recorder_drops_it() {
        assert!(!recording());
        let held = Recording::start();
        assert!(recording());
        drop(held);
        assert!(!recording());
    }
}

//! Labs › Meeting finished event: tells one local API subscriber
//! (`GET /meetings/events`) a meeting's id once its final transcript and
//! Markdown are written. Only the id leaves this module: no title, path,
//! transcript text, or audio. With the toggle off, or with nobody
//! subscribed, nothing is queued or kept.

use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::time::Duration;

/// Events waiting for a slow subscriber; more are dropped and logged.
const BACKLOG: usize = 16;

/// The one subscriber slot.
pub(crate) struct Hub {
    subscriber: Mutex<Option<SyncSender<String>>>,
}

static HUB: Hub = Hub::new();

/// The process's hub, which the meeting recorder publishes to.
pub(crate) fn global() -> &'static Hub {
    &HUB
}

/// Publishes a finished meeting while Labs › Meeting finished event is on.
pub(crate) fn publish(id: &str) {
    HUB.publish(crate::labs::meeting_finished_event(), id);
}

impl Hub {
    pub(crate) const fn new() -> Self {
        Self {
            subscriber: Mutex::new(None),
        }
    }

    /// Takes the one subscriber slot; `None` while another subscriber holds it.
    pub(crate) fn subscribe(&'static self) -> Option<Subscription> {
        let mut slot = self
            .subscriber
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if slot.is_some() {
            return None;
        }
        let (sender, receiver) = mpsc::sync_channel(BACKLOG);
        *slot = Some(sender);
        Some(Subscription {
            hub: self,
            receiver,
        })
    }

    /// Hands `id` to the subscriber, if `enabled` and someone listens.
    pub(crate) fn publish(&self, enabled: bool, id: &str) {
        if !enabled {
            return;
        }
        let mut slot = self
            .subscriber
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(sender) = slot.as_ref() else {
            return;
        };
        match sender.try_send(id.to_owned()) {
            Ok(()) => tracing::info!(meeting = id, "sent meeting finished event"),
            Err(TrySendError::Full(_)) => {
                tracing::warn!(
                    meeting = id,
                    "meeting finished event dropped: subscriber is behind"
                );
            }
            Err(TrySendError::Disconnected(_)) => *slot = None,
        }
    }
}

/// The subscriber's end; dropping it frees the slot.
pub(crate) struct Subscription {
    hub: &'static Hub,
    receiver: Receiver<String>,
}

impl Subscription {
    /// The next finished meeting's id, waiting at most `timeout`.
    pub(crate) fn next(&self, timeout: Duration) -> Result<String, RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        *self
            .hub
            .subscriber
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finished_meeting_reaches_the_one_subscriber_as_its_id() {
        static TEST_HUB: Hub = Hub::new();
        let subscription = TEST_HUB.subscribe().unwrap();
        assert!(TEST_HUB.subscribe().is_none(), "one subscriber at a time");
        TEST_HUB.publish(true, "1700000000000-42");
        assert_eq!(
            subscription.next(Duration::from_secs(1)).unwrap(),
            "1700000000000-42"
        );
        assert_eq!(
            subscription.next(Duration::from_millis(10)),
            Err(RecvTimeoutError::Timeout),
            "published once"
        );
        drop(subscription);
        assert!(TEST_HUB.subscribe().is_some(), "dropping frees the slot");
    }

    #[test]
    fn nothing_is_sent_or_kept_while_the_toggle_is_off() {
        static TEST_HUB: Hub = Hub::new();
        TEST_HUB.publish(true, "before-anyone-listened");
        let subscription = TEST_HUB.subscribe().unwrap();
        TEST_HUB.publish(false, "while-off");
        assert_eq!(
            subscription.next(Duration::from_millis(10)),
            Err(RecvTimeoutError::Timeout)
        );
    }
}

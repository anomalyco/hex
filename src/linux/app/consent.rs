//! The Linux app's side of consent: Hex's permission window answers the
//! local API's asks through one desk, and the main window stops, cancels,
//! and revokes the recordings and grants they allowed.

use std::sync::Arc;
use std::sync::mpsc::SyncSender;

use super::{HostHandle, HostRequest};
use crate::desktop::consent::Desk;
use crate::local_api::{Answer, AppConsent, RecordSessions};

/// Hex's permission window answers `desk`'s asks; the main window stops,
/// cancels, and revokes `record`'s recordings and grants.
#[derive(Clone)]
pub(crate) struct Consent {
    pub(crate) desk: Arc<Desk>,
    pub(crate) record: Arc<RecordSessions>,
}

impl Consent {
    /// The desk and the local API's side of it, recording through `control`.
    pub(super) fn start(
        control: SyncSender<crate::dictation::service::DictationControl>,
    ) -> (Self, AppConsent) {
        let desk = Arc::new(Desk::default());
        let api = AppConsent::new(desk.clone(), control);
        let record = api.record.clone();
        (Self { desk, record }, api)
    }
}

impl HostHandle {
    pub(super) fn set_consent(&self, consent: Consent) {
        let _ = self.0.consent.set(consent);
    }

    pub(crate) fn consent(&self) -> Option<&Consent> {
        self.0.consent.get()
    }

    /// The window's Stop, Cancel, and Revoke for consented recordings.
    pub(super) fn record_request(&self, request: HostRequest) {
        let Some(record) = self.consent().map(|consent| consent.record.clone()) else {
            return;
        };
        // Stop transcribes and Cancel waits on the capture loop, so neither
        // holds the window's thread or the host.
        let _ = std::thread::Builder::new()
            .name("hex-consent-request".into())
            .spawn(move || match request {
                HostRequest::RecordControl { id, stop: true } => _ = record.stop(id),
                HostRequest::RecordControl { id, stop: false } => record.cancel(id),
                HostRequest::RevokePermission { id } => _ = record.revoke_permission(id),
                HostRequest::RevokeRecordGrant { id } => _ = record.revoke(id),
                HostRequest::Website { origin, action } => _ = record.website(&origin, action),
                _ => {}
            });
    }

    /// The window's app failed, and its permission window with it: an ask
    /// answers unavailable at once rather than waiting out its minute unseen.
    pub(super) fn consent_window_gone(&self) {
        if let Some(consent) = self.consent() {
            refuse_every_ask(&consent.desk);
        }
    }
}

/// Answers each ask `desk` starts unavailable, at once.
fn refuse_every_ask(desk: &Arc<Desk>) {
    let weak = Arc::downgrade(desk);
    desk.watch(move |prompt| {
        if let (Some(prompt), Some(desk)) = (prompt, weak.upgrade()) {
            // Off the desk's watcher lock, which answering takes again.
            std::thread::spawn(move || desk.answer(prompt.id, Answer::Unavailable));
        }
    });
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::local_api::{ConsentGate, Intent, Prompt, Recipient};

    #[test]
    fn without_a_window_an_ask_is_unavailable_at_once() {
        let desk = Arc::new(Desk::default());
        refuse_every_ask(&desk);
        let prompt = Prompt {
            id: 0,
            intent: Intent::Record,
            recipient: Recipient::Web {
                origin: "https://notes.example".into(),
                client_name: None,
            },
        };
        assert_eq!(
            desk.ask(&prompt, Duration::from_secs(30)),
            Answer::Unavailable
        );
    }
}

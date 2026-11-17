//! The check-in lease of a consented recording: a recording runs for as long
//! as its client checks in, and stops itself once the check-ins stop.

use std::sync::{Arc, Weak};
use std::thread;
use std::time::{Duration, Instant};

use super::super::http::HttpResponse;
use super::{Channel, Phase, Sessions, Started, lock, phase_response};

impl Sessions {
    /// Watches recording `id` while it records: it stops the recording once
    /// the client's lease lapses, whether or not the client comes back, and
    /// until then keeps the capture loop's own owner lease alive, so only the
    /// client's check-ins decide how long it runs.
    pub(super) fn watch(self: &Arc<Self>, id: u64) {
        let weak: Weak<Self> = Arc::downgrade(self);
        // Renew the capture loop well inside its own lease.
        let tick = (self.timings.lease / 3).max(Duration::from_millis(1));
        thread::Builder::new()
            .name("record-lease".into())
            .spawn(move || {
                loop {
                    let Some(sessions) = weak.upgrade() else {
                        return;
                    };
                    let Some((deadline, started)) = sessions.recording_lease(id) else {
                        return;
                    };
                    let now = Instant::now();
                    if deadline <= now {
                        sessions.lease_expired(id);
                        return;
                    }
                    if let Err(error) = sessions.recording.renew(&started) {
                        tracing::warn!(%error, "a consented recording ended in the capture loop");
                        sessions.cancel(id);
                        return;
                    }
                    drop(sessions);
                    thread::sleep(tick.min(deadline - now));
                }
            })
            .ok();
    }

    /// The live recording `id`'s lease deadline and capture, while it is
    /// still recording.
    fn recording_lease(&self, id: u64) -> Option<(Instant, Started)> {
        let state = lock(&self.state);
        let session = state.sessions.get(state.active.as_ref()?)?;
        if session.id != id || session.phase != Phase::Recording || session.finishing {
            return None;
        }
        Some((session.deadline, session.started.clone()?))
    }

    /// `POST /record/heartbeat`: the client is still there, so its recording
    /// keeps going for another lease. A lapsed lease has already stopped
    /// the recording, and no check-in starts it again.
    pub(in crate::local_api) fn heartbeat(self: &Arc<Self>, token: &str, channel: &Channel) -> HttpResponse {
        let now = Instant::now();
        let mut state = lock(&self.state);
        let Some(session) = state.sessions.get_mut(token) else {
            return HttpResponse::code(404, "record-not-found");
        };
        if session.channel != *channel {
            return HttpResponse::code(404, "record-not-found");
        }
        if session.phase != Phase::Recording {
            return phase_response(session.phase);
        }
        if session.deadline <= now {
            let id = session.id;
            drop(state);
            self.lease_expired(id);
            let state = lock(&self.state);
            return state
                .sessions
                .get(token)
                .map_or(HttpResponse::code(404, "record-not-found"), |session| {
                    phase_response(session.phase)
                });
        }
        session.deadline = now + self.timings.lease;
        HttpResponse::empty(204)
    }
}

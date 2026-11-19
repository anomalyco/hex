//! What Hex's window does to consented recordings: the live card's view,
//! Stop and Cancel, revoking grants and saved permissions, and the grace
//! after a stop that discards an uncollected transcript.

use super::*;

impl Sessions {
    /// The panel Hex's window shows while a recording is live.
    pub(crate) fn view(&self) -> Option<View> {
        let state = lock(&self.state);
        let session = state.sessions.get(state.active.as_ref()?)?;
        if !matches!(session.phase, Phase::Recording | Phase::Stopped) {
            return None;
        }
        let recipient = state
            .grants
            .values()
            .find(|grant| grant.id == session.grant)
            .map(|grant| grant.recipient.clone())?;
        Some(View {
            id: session.id,
            recipient,
            phase: session.phase,
            elapsed_ms: session.began.elapsed().as_millis() as u64,
            remaining_ms: match session.phase {
                Phase::Stopped => remaining(session.deadline),
                _ => 0,
            },
        })
    }

    /// Every paired client, in pairing order.
    pub(crate) fn grants(&self) -> Vec<GrantView> {
        let state = lock(&self.state);
        let mut grants: Vec<GrantView> = state
            .grants
            .values()
            .map(|grant| GrantView {
                id: grant.id,
                recipient: grant.recipient.clone(),
                durable: grant.durable,
            })
            .collect();
        grants.sort_by_key(|grant| grant.id);
        grants
    }

    /// Revokes pairing `id` (the window's Revoke). A recording it is running
    /// stops and is discarded, and every saved Always Allow permission for
    /// the same verified recipient is removed too. False when no pairing has
    /// that id.
    pub(crate) fn revoke(&self, id: u64) -> bool {
        let mut state = lock(&self.state);
        let Some((token, grant)) = state.grants.iter().find(|(_, grant)| grant.id == id) else {
            return false;
        };
        let token = token.clone();
        if !self.forget_record_permission(&grant.recipient) {
            return false;
        }
        self.revoke_locked(&mut state, &token)
    }

    pub(crate) fn permissions(&self) -> Vec<crate::local_api::PermissionView> {
        self.permissions.list()
    }

    pub(crate) fn revoke_permission(&self, id: u64) -> bool {
        let mut state = lock(&self.state);
        let Some(removed) = self.permissions.revoke(id) else {
            return false;
        };
        if removed.capability == crate::local_api::PermissionCapability::MicrophoneRecording {
            let key = recipient_key(&removed.recipient);
            state
                .codes
                .retain(|_, code| recipient_key(&code.recipient) != key);
            let token = state
                .grants
                .iter()
                .find(|(_, grant)| recipient_key(&grant.recipient) == key)
                .map(|(token, _)| token.clone());
            if let Some(token) = token {
                self.revoke_locked(&mut state, &token);
            }
        }
        true
    }

    pub(super) fn forget_record_permission(&self, recipient: &Recipient) -> bool {
        self.permissions
            .permission(Intent::Record, recipient)
            .is_none_or(|id| self.permissions.revoke(id).is_some())
    }

    /// Forgets `token` and everything it authorized. The caller holds the
    /// state lock.
    pub(super) fn revoke_locked(&self, state: &mut State, token: &str) -> bool {
        let Some(grant) = state.grants.remove(token) else {
            return false;
        };
        // A code issued before the revoke must not pair the same recipient
        // again without asking.
        let key = recipient_key(&grant.recipient);
        state
            .codes
            .retain(|_, code| recipient_key(&code.recipient) != key);
        let running = state
            .sessions
            .iter()
            .find(|(_, session)| session.grant == grant.id && session.phase.live())
            .map(|(token, session)| (token.clone(), session.started.clone()));
        if let Some((token, started)) = running {
            if let Some(started) = started {
                self.recording.cancel(&started);
            }
            if let Some(session) = state.sessions.get_mut(&token) {
                session.phase = Phase::Cancelled;
                session.transcript = None;
            }
            if state.active.as_deref() == Some(&token) {
                state.active = None;
            }
        }
        true
    }

    /// Stops recording `id` (the window's Stop) and keeps its transcript for
    /// the client's `finish` for the grace, as a lapsed lease does. False
    /// when it is not the live recording.
    pub(crate) fn stop(self: &Arc<Self>, id: u64) -> bool {
        let stopped = self.stop_recording(id);
        if stopped {
            self.expire_after_grace(id);
        }
        stopped
    }

    pub(super) fn stop_recording(&self, id: u64) -> bool {
        let started = {
            let mut state = lock(&self.state);
            let Some(token) = state.active.clone() else {
                return false;
            };
            let Some(session) = state.sessions.get_mut(&token) else {
                return false;
            };
            if session.id != id || session.phase != Phase::Recording || session.finishing {
                return false;
            }
            let Some(started) = session.started.clone() else {
                return false;
            };
            session.finishing = true;
            started
        };
        // Transcription can take a moment; nothing else holds the lock.
        let result = self.recording.finish(&started);
        let mut state = lock(&self.state);
        let Some(token) = state.active.clone() else {
            return false;
        };
        let Some(session) = state.sessions.get_mut(&token) else {
            return false;
        };
        if session.id != id || session.phase != Phase::Recording {
            return false;
        }
        session.finishing = false;
        match result {
            Ok(transcript) => {
                session.transcript = Some(transcript);
                session.phase = Phase::Stopped;
                session.deadline = Instant::now() + self.timings.grace;
                true
            }
            Err(error) => {
                tracing::warn!(%error, "could not stop a consented recording");
                let grant = session.grant;
                session.phase = Phase::Cancelled;
                state.active = None;
                retire_once_grant(&mut state, grant);
                false
            }
        }
    }

    /// Discards recording `id` (the window's Cancel).
    pub(crate) fn cancel(&self, id: u64) {
        let mut state = lock(&self.state);
        let Some(token) = state.active.clone() else {
            return;
        };
        let grant = {
            let Some(session) = state.sessions.get_mut(&token) else {
                return;
            };
            if session.id != id || !session.phase.live() {
                return;
            }
            if let Some(started) = &session.started {
                self.recording.cancel(started);
            }
            session.phase = Phase::Cancelled;
            session.transcript = None;
            session.grant
        };
        state.active = None;
        retire_once_grant(&mut state, grant);
    }

    /// The client stopped checking in: stop the recording, then discard it
    /// if the client never collects the transcript.
    pub(super) fn lease_expired(self: &Arc<Self>, id: u64) {
        if self
            .view()
            .is_some_and(|view| view.id == id && view.phase == Phase::Recording)
        {
            self.stop_recording(id);
        }
        self.expire_after_grace(id);
    }

    /// Discards stopped recording `id` once the grace runs out without a
    /// `finish`.
    pub(super) fn expire_after_grace(self: &Arc<Self>, id: u64) {
        let weak: Weak<Self> = Arc::downgrade(self);
        let grace = self.timings.grace;
        thread::Builder::new()
            .name("record-grace".into())
            .spawn(move || {
                thread::sleep(grace);
                if let Some(sessions) = weak.upgrade() {
                    sessions.expire(id);
                }
            })
            .ok();
    }

    pub(super) fn expire(&self, id: u64) {
        let mut state = lock(&self.state);
        let Some(token) = state.active.clone() else {
            return;
        };
        let grant = {
            let Some(session) = state.sessions.get_mut(&token) else {
                return;
            };
            if session.id != id || !matches!(session.phase, Phase::Stopped | Phase::Recording) {
                return;
            }
            if session.phase == Phase::Recording
                && let Some(started) = &session.started
            {
                self.recording.cancel(started);
            }
            session.phase = Phase::Expired;
            session.grant
        };
        state.active = None;
        retire_once_grant(&mut state, grant);
    }
}

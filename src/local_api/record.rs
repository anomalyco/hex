//! Consented recording for local clients: one consent, then recordings.
//!
//! A client asks Hex's window once. The person sees the caller's verified
//! identity and chooses Deny, Allow Once, or Always Allow for *starting
//! recordings*; an allowed ask issues a revocable grant token. The grant
//! never starts a recording by itself: each `POST /record/start` opens a
//! separate session that records for as long as the client keeps checking
//! in (`POST /record/heartbeat`, the same 10 s lease `/dictations` and
//! `/streams` use), stops itself when the check-ins stop, and ends with one
//! transcript (`finish`) or nothing (`cancel`).
//!
//! Allow Once mints a grant for exactly one recording session; the grant is
//! retired when that session finishes, is cancelled, or expires. Always Allow
//! saves the capability for the verified recipient in [`Permissions`] and
//! mints a grant that may open repeated sessions. The saved permission
//! survives a restart, so after Hex restarts the client pairs again and Hex
//! answers from the saved permission without a dialog. Grant tokens
//! themselves never survive a restart.
//!
//! The recording itself is Hex's existing programmatic dictation (the same
//! path `/dictations` drives), so the microphone, pre-roll, and model choice
//! are Hex's own. What a client gets is narrow: a grant token that can only
//! open a session, and a session token that starts, observes, finishes, or
//! cancels *that one recording*. Raw audio, levels, partials, History,
//! paste, and every other local API route stay out of reach
//! (`X-Hex-Record-Grant` and `X-Hex-Record-Token` are checked before the
//! bearer token and only for `/record/*`).
//!
//! A client reaches the consent in one of two ways, and the grant is bound
//! to which:
//!
//! - a browser page over the loopback HTTP API, by its `Origin` (a website
//!   Labs › Connected apps allows, see [`super::access`]);
//! - a native process over the owner-only Unix socket, by the audit token
//!   `Security.framework` resolves from the peer, never by a name or path the
//!   process claims.
//!
//! Pairings live in this process only and are revocable from Hex's window or
//! by the client. Only Always Allow's verified capability and scope are
//! written to disk (see `docs/connect.md`).

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::access::Access;
use super::consent::{Answer, ConsentGate, Intent, Prompt, Recipient, Verification};
use super::http::HttpResponse;
use super::permissions::Permissions;
use super::read_body_bounded;

mod lease;

pub(super) const PAIR_PATH: &str = "/record/pair";
pub(super) const TOKEN_PATH: &str = "/record/token";
pub(super) const START_PATH: &str = "/record/start";
pub(super) const STATUS_PATH: &str = "/record/status";
pub(super) const FINISH_PATH: &str = "/record/finish";
pub(super) const CANCEL_PATH: &str = "/record/cancel";
pub(super) const REVOKE_PATH: &str = "/record/revoke";
pub(super) const HEARTBEAT_PATH: &str = "/record/heartbeat";

/// How long a recording runs past its client's last check-in; it then stops
/// itself. The same lease as Hex's other programmatic dictations. There is
/// no limit on how long a recording that keeps checking in may run.
pub(crate) const LEASE: Duration = crate::dictation::service::LEASE;
/// A stopped recording waits this long for the client's `finish`.
pub(super) const FINISH_GRACE: Duration = Duration::from_secs(30);
/// How long an allowed pairing's code lives.
pub(super) const CODE_LIFETIME: Duration = Duration::from_secs(120);
/// How long the person has to answer the window's dialog.
pub(super) const PROMPT_WAIT: Duration = Duration::from_secs(60);
/// How long a denied recipient waits before it may ask again.
pub(super) const DENY_COOLDOWN: Duration = Duration::from_secs(30);
pub(super) const MAX_BODY_BYTES: usize = 8 * 1024;
const BODY_DEADLINE: Duration = Duration::from_secs(5);

/// The verified identity of one native process, from its peer audit token.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct PeerId(pub(crate) [u32; 8]);

/// What a code-signing check reported about a native client.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Identity {
    /// The signing identifier (`com.example.app` for a signed bundle).
    pub(crate) identifier: String,
    /// Verified code-directory hash, including for ad-hoc signatures.
    pub(crate) cdhash: String,
    /// The Team ID, when the signature has one.
    pub(crate) team: Option<String>,
}

/// How a request reached Hex, and what a grant is bound to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Channel {
    /// A browser page's verified `Origin` over loopback HTTP.
    Web(String),
    /// A native process over Hex's Unix socket: its audit token, and the
    /// signing identity `Security.framework` resolved for that token. Only
    /// macOS has the socket: elsewhere native clients have no channel.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Native { peer: PeerId, identity: Identity },
}

/// One recording Hex's capture loop is producing.
#[derive(Clone, Debug)]
pub(crate) struct Started {
    pub(crate) id: u64,
    /// The internal owner token the capture loop knows this recording by.
    pub(crate) owner: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Transcript {
    pub(crate) text: String,
    pub(crate) duration_ms: u64,
}

/// Hex's recording path. The app's implementation is the programmatic
/// dictation the `/dictations` route already uses; tests use one that never
/// opens a microphone.
pub(crate) trait Recording: Send + Sync {
    fn start(&self) -> Result<Started, String>;
    /// Keeps the capture loop's own owner lease alive while Hex holds the
    /// recording. An error means the recording is gone (microphone lost).
    fn renew(&self, started: &Started) -> Result<(), String>;
    /// Stops the recording and returns its transcript.
    fn finish(&self, started: &Started) -> Result<Transcript, String>;
    fn cancel(&self, started: &Started);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Phase {
    Recording,
    /// Stopped (by the client, Hex's window, or a lapsed lease); the transcript
    /// waits for `finish`.
    Stopped,
    Finished,
    Cancelled,
    Expired,
}

impl Phase {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Stopped => "stopped",
            Self::Finished => "finished",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }

    fn live(self) -> bool {
        matches!(self, Self::Recording | Self::Stopped)
    }
}

/// What Hex's window shows while a client's recording is live: who it is
/// for, how long it has recorded, and that it can be stopped or cancelled.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct View {
    pub(crate) id: u64,
    pub(crate) recipient: Recipient,
    pub(crate) phase: Phase,
    /// How long the recording has run (it keeps running while the client
    /// checks in).
    pub(crate) elapsed_ms: u64,
    /// While stopped, how long the client has left to collect the transcript.
    pub(crate) remaining_ms: u64,
}

/// One paired client, as Hex's window lists it for revocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GrantView {
    pub(crate) id: u64,
    pub(crate) recipient: Recipient,
    /// True for Always Allow, false for the one session Allow Once gave.
    pub(crate) durable: bool,
}

struct Code {
    challenge: [u8; 32],
    channel: Channel,
    recipient: Recipient,
    durable: bool,
    expires: Instant,
}

struct Grant {
    id: u64,
    channel: Channel,
    recipient: Recipient,
    durable: bool,
}

struct Session {
    id: u64,
    grant: u64,
    channel: Channel,
    phase: Phase,
    started: Option<Started>,
    transcript: Option<Transcript>,
    /// A stop/finish is transcribing outside the lock; a second one waits.
    finishing: bool,
    /// When the current phase runs out: the check-in lease while
    /// recording, the finish grace once stopped.
    deadline: Instant,
    /// When the recording started.
    began: Instant,
}

#[derive(Default)]
struct State {
    codes: HashMap<String, Code>,
    /// Grant tokens, keyed by the secret the client holds.
    grants: HashMap<String, Grant>,
    /// Session tokens, keyed by the secret the client holds.
    sessions: HashMap<String, Session>,
    /// The one live session's token, if any.
    active: Option<String>,
    /// A start is opening the microphone outside the lock.
    starting: bool,
    denied: HashMap<String, Instant>,
    next_prompt: u64,
    prompting: bool,
    next_id: u64,
}

/// How long each part of a session lasts, injectable for tests.
#[derive(Clone, Copy)]
pub(crate) struct Timings {
    pub(super) code: Duration,
    /// How long a recording runs past its last check-in.
    pub(super) lease: Duration,
    pub(super) grace: Duration,
    pub(super) prompt: Duration,
    pub(super) cooldown: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            code: CODE_LIFETIME,
            lease: LEASE,
            grace: FINISH_GRACE,
            prompt: PROMPT_WAIT,
            cooldown: DENY_COOLDOWN,
        }
    }
}

/// Every pairing and recording session the running app serves.
pub(crate) struct Sessions {
    access: Access,
    gate: Option<Arc<dyn ConsentGate>>,
    recording: Arc<dyn Recording>,
    permissions: Arc<Permissions>,
    timings: Timings,
    state: Mutex<State>,
}

impl Sessions {
    pub(crate) fn new(
        access: Access,
        gate: Option<Arc<dyn ConsentGate>>,
        recording: Arc<dyn Recording>,
        permissions: Arc<Permissions>,
    ) -> Self {
        Self {
            access,
            gate,
            recording,
            permissions,
            timings: Timings::default(),
            state: Mutex::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_timings(
        access: Access,
        gate: Option<Arc<dyn ConsentGate>>,
        recording: Arc<dyn Recording>,
        permissions: Arc<Permissions>,
        timings: Timings,
    ) -> Self {
        Self {
            access,
            gate,
            recording,
            permissions,
            timings,
            state: Mutex::default(),
        }
    }

    /// Labs › Connected apps' switches, for the routes' CORS.
    pub(super) fn access(&self) -> Access {
        self.access
    }

    /// This token-bearing request's channel, when it names one Hex accepts:
    /// a browser request needs an `Origin` Connected apps allows. (A native
    /// request's channel is its verified peer.)
    pub(super) fn web_channel(&self, origin: Option<&str>) -> Option<Channel> {
        origin
            .filter(|origin| self.access.website_allowed(&self.permissions, origin))
            .map(|origin| Channel::Web(origin.to_owned()))
    }

    /// Labs › Connected apps' website lists, as the window shows them.
    pub(crate) fn websites(&self) -> super::permissions::Websites {
        self.permissions.websites()
    }

    /// The window moved `origin` between the website lists; whatever that
    /// website no longer may do ends at once.
    pub(crate) fn website(&self, origin: &str, action: super::permissions::WebsiteAction) -> bool {
        let saved = self.permissions.website_action(origin, action);
        self.reconcile();
        saved
    }

    /// Ends what Labs › Connected apps no longer allows: a pending ask from a
    /// recipient it no longer admits is withdrawn (answered unavailable), and
    /// a live recording for one is cancelled. Saved grants stay, unusable
    /// until the lab allows them again.
    pub(crate) fn reconcile(&self) {
        let (access, permissions) = (self.access, self.permissions.clone());
        if let Some(gate) = &self.gate {
            gate.withdraw(&|prompt: &Prompt| access.admits(&permissions, &prompt.recipient));
        }
        let live = {
            let state = lock(&self.state);
            state.active.as_ref().and_then(|token| {
                let session = state.sessions.get(token)?;
                let grant = state
                    .grants
                    .values()
                    .find(|grant| grant.id == session.grant)?;
                (session.phase.live() && !access.admits(&permissions, &grant.recipient))
                    .then_some(session.id)
            })
        };
        if let Some(id) = live {
            tracing::info!(
                "Labs › Connected apps no longer allows a live recording; cancelling it"
            );
            self.cancel(id);
        }
    }

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

    pub(crate) fn permissions(&self) -> Vec<super::PermissionView> {
        self.permissions.list()
    }

    pub(crate) fn revoke_permission(&self, id: u64) -> bool {
        let mut state = lock(&self.state);
        let Some(removed) = self.permissions.revoke(id) else {
            return false;
        };
        if removed.capability == super::PermissionCapability::MicrophoneRecording {
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

    fn forget_record_permission(&self, recipient: &Recipient) -> bool {
        self.permissions
            .permission(Intent::Record, recipient)
            .is_none_or(|id| self.permissions.revoke(id).is_some())
    }

    /// Forgets `token` and everything it authorized. The caller holds the
    /// state lock.
    fn revoke_locked(&self, state: &mut State, token: &str) -> bool {
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
    /// the client's `finish`. False when it is not the live recording.
    pub(crate) fn stop(&self, id: u64) -> bool {
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
    fn lease_expired(self: &Arc<Self>, id: u64) {
        if self
            .view()
            .is_some_and(|view| view.id == id && view.phase == Phase::Recording)
        {
            self.stop(id);
        }
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

    fn expire(&self, id: u64) {
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

    /// `POST /record/pair`: the window asks once whether this caller may
    /// start recordings. Pairing starts nothing. An Always Allow permission
    /// answers from disk without a dialog.
    pub(super) fn pair(&self, channel: Channel, body: &[u8]) -> HttpResponse {
        let Ok(request) = serde_json::from_slice::<PairRequest>(body) else {
            return HttpResponse::code(400, "invalid-request");
        };
        if request.code_challenge_method.as_deref().unwrap_or("S256") != "S256" {
            return HttpResponse::code(400, "invalid-request");
        }
        let Some(challenge) =
            decode_base64url(&request.code_challenge).filter(|challenge| challenge.len() == 32)
        else {
            return HttpResponse::code(400, "invalid-request");
        };
        if !self.access.lab() {
            return HttpResponse::code(403, "record-unavailable");
        }
        let Some(recipient) = self.recipient_for(&channel, request.client_name.as_deref()) else {
            return HttpResponse::code(403, "identity-unverified");
        };
        let Some(gate) = self.gate.clone() else {
            return HttpResponse::code(403, "record-unavailable");
        };
        let key = recipient_key(&recipient);
        let now = Instant::now();
        let mut state = lock(&self.state);
        prune(&mut state, now);
        if state
            .grants
            .values()
            .any(|grant| recipient_key(&grant.recipient) == key)
        {
            return HttpResponse::code(409, "already-paired");
        }
        // Always Allow from an earlier run is the person's answer; it neither
        // needs a dialog nor depends on this run's grant map.
        let saved = match self.permissions.authorize_saved(
            Intent::Record,
            &recipient,
            request.permission_proof.as_deref(),
        ) {
            Ok(saved) => saved,
            Err(()) => return HttpResponse::code(403, "permission-proof-required"),
        };
        if saved.is_some() {
            return self.issue_pairing(
                &mut state,
                challenge,
                channel,
                recipient,
                true,
                None,
                request.state,
            );
        }
        if state
            .denied
            .get(&key)
            .is_some_and(|denied| *denied + self.timings.cooldown > now)
        {
            return HttpResponse::code(429, "consent-cooldown");
        }
        if state.prompting {
            return HttpResponse::code(409, "consent-busy");
        }
        state.next_prompt += 1;
        let prompt = Prompt {
            id: state.next_prompt,
            intent: Intent::Record,
            recipient: recipient.clone(),
        };
        state.prompting = true;
        drop(state);

        let answer = gate.ask(&prompt, self.timings.prompt);

        let mut state = lock(&self.state);
        state.prompting = false;
        match answer {
            Answer::Once => self.issue_pairing(
                &mut state,
                challenge,
                channel,
                recipient,
                false,
                None,
                request.state,
            ),
            Answer::Always => {
                // Always Allow is only real if it is written down; an
                // unwritable permission must not quietly act like Once.
                let (_, proof) = match self.permissions.remember(Intent::Record, &recipient) {
                    Ok(saved) => saved,
                    Err(error) => {
                        tracing::warn!(%error, "could not save an Always Allow permission");
                        return HttpResponse::code(500, "permission-not-saved");
                    }
                };
                self.issue_pairing(
                    &mut state,
                    challenge,
                    channel,
                    recipient,
                    true,
                    proof,
                    request.state,
                )
            }
            Answer::Deny => {
                state.denied.insert(key, Instant::now());
                HttpResponse::code(403, "consent-denied")
            }
            Answer::Timeout => HttpResponse::code(403, "consent-timeout"),
            Answer::Unavailable => HttpResponse::code(403, "record-unavailable"),
        }
    }

    /// One allowed ask's pairing code, carrying whether its grant should last
    /// past one recording session.
    #[allow(clippy::too_many_arguments)]
    fn issue_pairing(
        &self,
        state: &mut State,
        challenge: Vec<u8>,
        channel: Channel,
        recipient: Recipient,
        durable: bool,
        proof: Option<String>,
        client_state: Option<String>,
    ) -> HttpResponse {
        let code = match random_token("hexp_") {
            Ok(code) => code,
            Err(error) => {
                tracing::error!(%error, "could not generate a pairing code");
                return HttpResponse::code(500, "token-generation-failed");
            }
        };
        state.codes.insert(
            code.clone(),
            Code {
                challenge: challenge.try_into().expect("32 bytes were checked"),
                channel,
                recipient,
                durable,
                expires: Instant::now() + self.timings.code,
            },
        );
        HttpResponse::json(
            200,
            &serde_json::json!({
                "code": code,
                "expiresInMs": self.timings.code.as_millis(),
                "permissionProof": proof,
                "scope": "record:pair",
                "duration": if durable { "always" } else { "once" },
                "state": client_state,
            }),
        )
    }

    /// `POST /record/token`: spends the pairing code and mints one revocable
    /// grant token. No recording starts.
    pub(super) fn exchange(&self, channel: Channel, body: &[u8]) -> HttpResponse {
        let Ok(request) = serde_json::from_slice::<TokenRequest>(body) else {
            return HttpResponse::code(400, "invalid-request");
        };
        if !valid_verifier(&request.code_verifier) {
            return HttpResponse::code(400, "invalid-request");
        }
        if !self.access.lab() {
            return HttpResponse::code(403, "record-unavailable");
        }
        let challenge = sha256(request.code_verifier.as_bytes());
        let now = Instant::now();
        let mut state = lock(&self.state);
        prune(&mut state, now);
        let Some(code) = state.codes.remove(&request.code) else {
            return HttpResponse::code(403, "invalid-grant");
        };
        if code.expires <= now || code.channel != channel || code.challenge != challenge {
            return HttpResponse::code(403, "invalid-grant");
        }
        if state
            .grants
            .values()
            .any(|grant| recipient_key(&grant.recipient) == recipient_key(&code.recipient))
        {
            return HttpResponse::code(409, "already-paired");
        }
        state.next_id += 1;
        let id = state.next_id;
        let token = match random_token("hexg_") {
            Ok(token) => token,
            Err(error) => {
                tracing::error!(%error, "could not generate a record grant token");
                return HttpResponse::code(500, "token-generation-failed");
            }
        };
        state.grants.insert(
            token.clone(),
            Grant {
                id,
                channel: code.channel,
                recipient: code.recipient,
                durable: code.durable,
            },
        );
        HttpResponse::json(
            200,
            &serde_json::json!({
                "grantId": id,
                "grantToken": token,
                "scope": "record:any",
                "duration": if code.durable { "always" } else { "once" },
            }),
        )
    }

    /// `POST /record/start`: opens one recording session for a paired client.
    pub(super) fn start(self: &Arc<Self>, token: &str, channel: &Channel) -> HttpResponse {
        let now = Instant::now();
        let grant_id;
        {
            let mut state = lock(&self.state);
            prune(&mut state, now);
            let Some(grant) = state.grants.get(token) else {
                return HttpResponse::code(401, "invalid-grant");
            };
            let bound = grant.channel.clone();
            let id = grant.id;
            if bound != *channel {
                return HttpResponse::code(401, "invalid-grant");
            }
            if state.starting || state.active.is_some() {
                return HttpResponse::code(409, "session-busy");
            }
            state.starting = true;
            grant_id = id;
        }
        // Opening the microphone is the capture loop's work; the lock is
        // free while it happens.
        let started = self.recording.start();
        let mut state = lock(&self.state);
        state.starting = false;
        let started = match started {
            Ok(started) => started,
            Err(error) => {
                tracing::warn!(%error, "could not start a consented recording");
                return HttpResponse::code(503, "capture-unavailable");
            }
        };
        let Some(grant) = state
            .grants
            .values()
            .find(|grant| grant.id == grant_id)
            .map(|grant| grant.channel.clone())
        else {
            // Revoked while the microphone opened.
            self.recording.cancel(&started);
            return HttpResponse::code(401, "invalid-grant");
        };
        state.next_id += 1;
        let id = state.next_id;
        let session_token = match random_token("hexs_") {
            Ok(token) => token,
            Err(error) => {
                tracing::error!(%error, "could not generate a record session token");
                self.recording.cancel(&started);
                return HttpResponse::code(500, "token-generation-failed");
            }
        };
        state.sessions.insert(
            session_token.clone(),
            Session {
                id,
                grant: grant_id,
                channel: grant,
                phase: Phase::Recording,
                started: Some(started),
                transcript: None,
                finishing: false,
                deadline: now + self.timings.lease,
                began: now,
            },
        );
        state.active = Some(session_token.clone());
        let lease = self.timings.lease;
        drop(state);
        self.watch(id);
        HttpResponse::json(
            201,
            &serde_json::json!({
                "id": id,
                "sessionToken": session_token,
                "state": Phase::Recording.name(),
                "leaseMs": lease.as_millis(),
            }),
        )
    }

    /// `GET /record/status`.
    pub(super) fn status(self: &Arc<Self>, token: &str, channel: &Channel) -> HttpResponse {
        let now = Instant::now();
        let mut state = lock(&self.state);
        let Some(session) = state.sessions.get_mut(token) else {
            return HttpResponse::code(404, "record-not-found");
        };
        if session.channel != *channel {
            return HttpResponse::code(404, "record-not-found");
        }
        if session.phase.live() && session.deadline <= now {
            // A lapsed lease stops the recording; a passed grace discards it.
            let (id, recording) = (session.id, session.phase == Phase::Recording);
            drop(state);
            if recording {
                self.lease_expired(id);
            } else {
                self.expire(id);
            }
            state = lock(&self.state);
        }
        let Some(session) = state.sessions.get(token) else {
            return HttpResponse::code(404, "record-not-found");
        };
        HttpResponse::json(
            200,
            &serde_json::json!({
                "id": session.id,
                "state": session.phase.name(),
                "elapsedMs": session.began.elapsed().as_millis() as u64,
                "remainingMs": remaining(session.deadline),
                "leaseMs": self.timings.lease.as_millis(),
            }),
        )
    }

    /// `POST /record/finish`: stops a live recording (if needed) and answers
    /// with its transcript, once.
    pub(super) fn finish(&self, token: &str, channel: &Channel) -> HttpResponse {
        enum Step {
            Finish(Started),
            Take,
        }
        let step = {
            let mut state = lock(&self.state);
            let Some(session) = state.sessions.get_mut(token) else {
                return HttpResponse::code(404, "record-not-found");
            };
            if session.channel != *channel {
                return HttpResponse::code(404, "record-not-found");
            }
            match session.phase {
                Phase::Recording => {
                    let Some(started) = session.started.clone() else {
                        return HttpResponse::code(409, "not-started");
                    };
                    if session.finishing {
                        return HttpResponse::code(409, "finish-in-progress");
                    }
                    session.finishing = true;
                    Step::Finish(started)
                }
                Phase::Stopped => Step::Take,
                phase => return phase_response(phase),
            }
        };
        let started = match step {
            Step::Take => return self.take_transcript(token),
            Step::Finish(started) => started,
        };
        // Finish outside the lock, so status, the window's Cancel, and the
        // lease keep working while the engine transcribes.
        let result = self.recording.finish(&started);
        let mut state = lock(&self.state);
        let Some(session) = state.sessions.get_mut(token) else {
            return HttpResponse::code(404, "record-not-found");
        };
        if session.phase != Phase::Recording {
            // Cancelled (or otherwise ended) while it transcribed.
            return phase_response(session.phase);
        }
        session.finishing = false;
        match result {
            Ok(transcript) => {
                session.transcript = Some(transcript);
                session.phase = Phase::Stopped;
                session.deadline = Instant::now() + self.timings.grace;
            }
            Err(error) => {
                tracing::warn!(%error, "could not finish a consented recording");
                return HttpResponse::code(500, "transcription-failed");
            }
        }
        drop(state);
        self.take_transcript(token)
    }

    /// Hands over a stopped recording's transcript, once.
    fn take_transcript(&self, token: &str) -> HttpResponse {
        let mut state = lock(&self.state);
        let (id, grant, transcript) = {
            let Some(session) = state.sessions.get_mut(token) else {
                return HttpResponse::code(404, "record-not-found");
            };
            if session.phase != Phase::Stopped {
                return phase_response(session.phase);
            }
            let Some(transcript) = session.transcript.take() else {
                return HttpResponse::code(500, "transcription-failed");
            };
            session.phase = Phase::Finished;
            (session.id, session.grant, transcript)
        };
        if state.active.as_deref() == Some(token) {
            state.active = None;
        }
        retire_once_grant(&mut state, grant);
        HttpResponse::json(
            200,
            &serde_json::json!({
                "id": id,
                "transcript": transcript.text,
                "durationMs": transcript.duration_ms,
            }),
        )
    }

    /// `POST /record/cancel`: stops and discards whatever this session owns.
    pub(super) fn cancel_token(&self, token: &str, channel: &Channel) -> HttpResponse {
        let mut state = lock(&self.state);
        let grant = {
            let Some(session) = state.sessions.get_mut(token) else {
                return HttpResponse::code(404, "record-not-found");
            };
            if session.channel != *channel {
                return HttpResponse::code(404, "record-not-found");
            }
            if !session.phase.live() {
                return phase_response(session.phase);
            }
            if let Some(started) = &session.started {
                self.recording.cancel(started);
            }
            session.phase = Phase::Cancelled;
            session.transcript = None;
            session.grant
        };
        if state.active.as_deref() == Some(token) {
            state.active = None;
        }
        retire_once_grant(&mut state, grant);
        HttpResponse::empty(204)
    }

    /// `POST /record/revoke`: the paired client gives its grant back. The
    /// saved Always Allow permission for its verified recipient goes too.
    pub(super) fn revoke_token(&self, token: &str, channel: &Channel) -> HttpResponse {
        let mut state = lock(&self.state);
        let recipient = match state.grants.get(token) {
            Some(grant) if grant.channel == *channel => grant.recipient.clone(),
            _ => return HttpResponse::empty(401),
        };
        if !self.forget_record_permission(&recipient) {
            return HttpResponse::code(500, "revocation-not-saved");
        }
        self.revoke_locked(&mut state, token);
        HttpResponse::empty(204)
    }

    fn recipient_for(&self, channel: &Channel, client_name: Option<&str>) -> Option<Recipient> {
        match channel {
            Channel::Web(origin) => Some(Recipient::Web {
                origin: origin.clone(),
                client_name: client_name
                    .filter(|name| plain_name(name))
                    .map(str::to_owned),
                icon: None,
            }),
            Channel::Native { identity, .. } => Some(Recipient::Native {
                identifier: identity.identifier.clone(),
                cdhash: identity.cdhash.clone(),
                verification: Verification::MacSignature {
                    team: identity.team.clone(),
                },
                name: None,
                icon: None,
            }),
        }
    }
}

fn prune(state: &mut State, now: Instant) {
    state.codes.retain(|_, code| code.expires > now);
    state.sessions.retain(|_, session| {
        session.phase.live() || session.deadline + Duration::from_secs(300) > now
    });
    state
        .denied
        .retain(|_, denied| *denied + DENY_COOLDOWN > now);
}

/// Allow Once is exactly one recording session: when that session ends, its
/// grant goes, so the next recording asks again. Always Allow's grant stays.
fn retire_once_grant(state: &mut State, grant_id: u64) {
    let token = state
        .grants
        .iter()
        .find(|(_, grant)| grant.id == grant_id && !grant.durable)
        .map(|(token, _)| token.clone());
    if let Some(token) = token {
        state.grants.remove(&token);
    }
}

fn phase_response(phase: Phase) -> HttpResponse {
    HttpResponse::code(
        409,
        match phase {
            Phase::Recording => "already-recording",
            Phase::Stopped => "stopped",
            Phase::Finished => "session-finished",
            Phase::Cancelled => "session-cancelled",
            Phase::Expired => "session-expired",
        },
    )
}

fn remaining(deadline: Instant) -> u64 {
    deadline
        .saturating_duration_since(Instant::now())
        .as_millis() as u64
}

/// A recipient's grant identity; its name and icon are not part of it.
fn recipient_key(recipient: &Recipient) -> String {
    match recipient {
        Recipient::Web { origin, .. } => format!("web:{origin}"),
        Recipient::Native {
            identifier,
            cdhash,
            verification,
            ..
        } => {
            let verified = match verification {
                Verification::MacSignature { team } => team.clone().unwrap_or_default(),
                other => format!("{other:?}"),
            };
            format!("native:{identifier}:{cdhash}:{verified}")
        }
    }
}

/// Where the app's native recording socket lives.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn native_socket_path() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        super::native::socket_path()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// The app's sessions: its capture loop, the window's gate, saved Always
/// Allow permissions, and Labs › Connected apps, whose changes end at once
/// what it no longer allows.
pub(crate) fn app_sessions(
    gate: Arc<dyn ConsentGate>,
    control: super::dictation::Control,
    permissions: Arc<Permissions>,
) -> Arc<Sessions> {
    let sessions = Arc::new(Sessions::new(
        Access::app(),
        Some(gate),
        Arc::new(super::dictation::DictationRecording::new(control)),
        permissions,
    ));
    let weak = Arc::downgrade(&sessions);
    // Settings apply on whichever thread saved them, the window's included;
    // cancelling a recording waits on the capture loop, so it runs apart.
    crate::labs::watch_connected_apps(move || {
        if let Some(sessions) = weak.upgrade() {
            let _ = thread::Builder::new()
                .name("hex-connected-apps".into())
                .spawn(move || sessions.reconcile());
        }
    });
    sessions
}

/// One request that belongs to consented recording, or a refusal for a path
/// that announced itself as one.
pub(super) enum Request {
    Refused(HttpResponse),
    Pair(Call),
    Session {
        action: Action,
        token: String,
        channel: Channel,
    },
    /// A paired client giving its grant back.
    Revoke {
        token: String,
        channel: Channel,
    },
}

impl Request {
    pub(super) fn respond(
        self,
        context: &super::HttpContext,
        stream: &mut (impl std::io::Read + std::io::Write),
        shutdown: &AtomicBool,
    ) -> HttpResponse {
        match self {
            Self::Refused(response) => response,
            Self::Pair(call) => match context.record.as_ref() {
                Some(sessions) => call.respond(sessions, stream, shutdown),
                None => HttpResponse::code(403, "record-unavailable"),
            },
            // Labs › Connected apps off keeps grants and sessions but lets
            // none of them be used.
            Self::Session {
                action,
                token,
                channel,
            } => match context.record.as_ref() {
                Some(sessions) if sessions.access.lab() => {
                    action.respond(sessions, &token, &channel)
                }
                Some(_) => HttpResponse::empty(401),
                None => HttpResponse::code(403, "record-unavailable"),
            },
            Self::Revoke { token, channel } => match context.record.as_ref() {
                Some(sessions) if sessions.access.lab() => sessions.revoke_token(&token, &channel),
                Some(_) => HttpResponse::empty(401),
                None => HttpResponse::code(403, "record-unavailable"),
            },
        }
    }
}

/// What a request that reached the record check is.
pub(super) enum Routed {
    /// Not a record request: the local API's own routing continues.
    Pass(super::HttpRequest),
    Record(Request),
}

/// Routes a browser page's `/record/*` request, or passes it on when this is
/// not one. It runs before the bearer token: `/record/pair` and
/// `/record/token` are the page's own, and a grant or session token names
/// only its own actions.
pub(super) fn route(request: super::HttpRequest, context: &super::HttpContext) -> Routed {
    let path = request.path.split('?').next().unwrap_or("/").to_owned();
    if request.method == "POST"
        && let Some(endpoint) = Endpoint::of(&path)
    {
        let Some(sessions) = context.record.as_ref() else {
            return Routed::Record(Request::Refused(HttpResponse::code(
                403,
                "record-unavailable",
            )));
        };
        // Only a pairing ask lists a website that tried; a token exchange
        // can't come from one that never asked.
        let channel = match sessions.access.admit_website(
            &sessions.permissions,
            request.origin.as_deref(),
            endpoint == Endpoint::Pair,
        ) {
            Ok(origin) => Channel::Web(origin),
            Err(response) => return Routed::Record(Request::Refused(response)),
        };
        return Routed::Record(match Call::admit(endpoint, request, channel) {
            Ok(call) => Request::Pair(call),
            Err(response) => Request::Refused(response),
        });
    }
    let Some(record) = context.record.as_ref() else {
        return Routed::Pass(request);
    };
    if let Some(grant) = request.record_grant.clone() {
        let Some(channel) = record.web_channel(request.origin.as_deref()) else {
            return Routed::Record(Request::Refused(HttpResponse::empty(401)));
        };
        if !bodyless(&request) {
            return Routed::Record(Request::Refused(HttpResponse::code(400, "invalid-request")));
        }
        if request.method == "POST" && path == START_PATH {
            return Routed::Record(Request::Session {
                action: Action::Start,
                token: grant,
                channel,
            });
        }
        if request.method != "POST" || path != REVOKE_PATH {
            return Routed::Record(Request::Refused(HttpResponse::empty(401)));
        }
        return Routed::Record(Request::Revoke {
            token: grant,
            channel,
        });
    }
    let Some(token) = request.record_token.clone() else {
        return Routed::Pass(request);
    };
    let Some(channel) = record.web_channel(request.origin.as_deref()) else {
        return Routed::Record(Request::Refused(HttpResponse::empty(401)));
    };
    let Some(action) = action(&request.method, &path) else {
        return Routed::Record(Request::Refused(HttpResponse::empty(401)));
    };
    if !bodyless(&request) {
        return Routed::Record(Request::Refused(HttpResponse::code(400, "invalid-request")));
    }
    Routed::Record(Request::Session {
        action,
        token,
        channel,
    })
}

/// One `/record/pair` or `/record/token` request, before its body is read.
pub(super) struct Call {
    endpoint: Endpoint,
    channel: Channel,
    body: Vec<u8>,
    content_length: usize,
}

impl Call {
    pub(super) fn admit(
        endpoint: Endpoint,
        request: super::HttpRequest,
        channel: Channel,
    ) -> Result<Self, HttpResponse> {
        if !request.has_content_type("application/json") {
            return Err(HttpResponse::code(415, "unsupported-content-type"));
        }
        let Some(content_length) = request.content_length else {
            return Err(HttpResponse::code(411, "length-required"));
        };
        if content_length > MAX_BODY_BYTES {
            return Err(HttpResponse::code(413, "request-too-large"));
        }
        Ok(Self {
            endpoint,
            channel,
            body: request.body_prefix,
            content_length,
        })
    }

    pub(super) fn respond(
        self,
        sessions: &Sessions,
        stream: &mut (impl std::io::Read + std::io::Write),
        shutdown: &AtomicBool,
    ) -> HttpResponse {
        let mut body = self.body;
        if let Err(error) = read_body_bounded(
            stream,
            self.content_length,
            &mut body,
            shutdown,
            MAX_BODY_BYTES,
            BODY_DEADLINE,
        ) {
            return super::http::body_read_response(error);
        }
        match self.endpoint {
            Endpoint::Pair => sessions.pair(self.channel, &body),
            Endpoint::Token => sessions.exchange(self.channel, &body),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Endpoint {
    Pair,
    Token,
}

impl Endpoint {
    pub(super) fn of(path: &str) -> Option<Self> {
        match path {
            PAIR_PATH => Some(Self::Pair),
            TOKEN_PATH => Some(Self::Token),
            _ => None,
        }
    }
}

/// Which session action a token-bearing request asks for, if any.
pub(super) fn action(method: &str, path: &str) -> Option<Action> {
    match (method, path) {
        ("POST", START_PATH) => Some(Action::Start),
        ("GET", STATUS_PATH) => Some(Action::Status),
        ("POST", FINISH_PATH) => Some(Action::Finish),
        ("POST", CANCEL_PATH) => Some(Action::Cancel),
        ("POST", HEARTBEAT_PATH) => Some(Action::Heartbeat),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Action {
    Start,
    Status,
    Finish,
    Cancel,
    Heartbeat,
}

impl Action {
    pub(super) fn respond(
        self,
        sessions: &Arc<Sessions>,
        token: &str,
        channel: &Channel,
    ) -> HttpResponse {
        match self {
            Self::Start => sessions.start(token, channel),
            Self::Status => sessions.status(token, channel),
            Self::Finish => sessions.finish(token, channel),
            Self::Cancel => sessions.cancel_token(token, channel),
            Self::Heartbeat => sessions.heartbeat(token, channel),
        }
    }
}

/// A session or revoke request must not carry a body: it takes no input
/// beyond its token, so a body of any kind is refused rather than ignored.
pub(super) fn bodyless(request: &super::HttpRequest) -> bool {
    request.content_length.unwrap_or(0) == 0 && request.body_prefix.is_empty()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairRequest {
    permission_proof: Option<String>,
    code_challenge: String,
    #[serde(default)]
    code_challenge_method: Option<String>,
    #[serde(default)]
    client_name: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenRequest {
    code: String,
    #[serde(alias = "code_verifier")]
    code_verifier: String,
}

fn plain_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name.chars().count() <= 40
        && name.chars().all(|character| {
            !character.is_control() && !('\u{202a}'..='\u{202e}').contains(&character)
        })
        && !name.contains("  ")
}

fn valid_verifier(verifier: &str) -> bool {
    (43..=128).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes).into()
}

pub(super) fn random_token(prefix: &str) -> Result<String, getrandom::Error> {
    use std::fmt::Write as _;
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random)?;
    let mut token = String::with_capacity(prefix.len() + 64);
    token.push_str(prefix);
    for byte in random {
        write!(token, "{byte:02x}").expect("writing to a string cannot fail");
    }
    Ok(token)
}

pub(super) fn decode_base64url(text: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(text.len() * 3 / 4);
    let mut buffer = 0_u32;
    let mut bits = 0_u32;
    for character in text.bytes() {
        let value = match character {
            b'A'..=b'Z' => character - b'A',
            b'a'..=b'z' => character - b'a' + 26,
            b'0'..=b'9' => character - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buffer >> bits) as u8);
        }
    }
    (bits < 6 && buffer & ((1 << bits) - 1) == 0).then_some(bytes)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_matches_rfc7636s_example() {
        assert_eq!(
            decode_base64url("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM").unwrap(),
            sha256(b"dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")
        );
        assert!(decode_base64url("a").is_none());
        assert!(decode_base64url("ab==").is_none());
    }

    #[test]
    fn a_verifier_is_checked_against_rfc7636s_grammar() {
        assert!(valid_verifier(&"a".repeat(43)));
        assert!(valid_verifier(&"a".repeat(128)));
        assert!(!valid_verifier(&"a".repeat(42)));
        assert!(!valid_verifier(
            "has space and that is not allowed at all.."
        ));
    }
}

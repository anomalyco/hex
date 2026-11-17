//! Consented recording for local clients: one pairing, then repeated
//! one-off recordings.
//!
//! A client pairs with Hex's window once. The person sees the caller's
//! verified identity and allows it to *start recordings*; the pairing issues
//! a revocable grant token. The grant never starts a recording by itself:
//! each `POST /record/start` opens a separate short-lived session that stops
//! itself at the lease and ends with one transcript (`finish`) or nothing
//! (`cancel`).
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
//! - a browser page over the loopback HTTP API, by its `Origin` (a listed
//!   Labs origin, checked by CORS like every browser request);
//! - a native process over the owner-only Unix socket, by the audit token
//!   `Security.framework` resolves from the peer, never by a name or path the
//!   process claims.
//!
//! Pairings live in this process only. They are revocable from Hex's window
//! and end when Hex quits; nothing about a client's identity is written to
//! disk (see `docs/connect.md`).

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::consent::{Answer, ConsentGate, Intent, Prompt, Recipient};
use super::http::HttpResponse;
use super::read_body_bounded;

pub(super) const PAIR_PATH: &str = "/record/pair";
pub(super) const TOKEN_PATH: &str = "/record/token";
pub(super) const START_PATH: &str = "/record/start";
pub(super) const STATUS_PATH: &str = "/record/status";
pub(super) const FINISH_PATH: &str = "/record/finish";
pub(super) const CANCEL_PATH: &str = "/record/cancel";
pub(super) const REVOKE_PATH: &str = "/record/revoke";

/// One recording can last this long and no longer; it stops itself.
pub(crate) const MAX_RECORDING: Duration = Duration::from_secs(120);
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
    /// The Team ID, when the signature has one.
    pub(crate) team: Option<String>,
}

/// How a request reached Hex, and what a grant is bound to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Channel {
    /// A browser page's verified `Origin` over loopback HTTP.
    Web(String),
    /// A native process over Hex's Unix socket: its audit token, and the
    /// signing identity `Security.framework` resolved for that token.
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
    /// Stops the recording and returns its transcript.
    fn finish(&self, started: &Started) -> Result<Transcript, String>;
    fn cancel(&self, started: &Started);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Phase {
    Recording,
    /// Stopped (by the client, Hex's window, or the lease); the transcript
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
/// for, how long the lease has left, and that it can be stopped or
/// cancelled.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct View {
    pub(crate) id: u64,
    pub(crate) recipient: Recipient,
    pub(crate) phase: Phase,
    pub(crate) remaining_ms: u64,
    pub(crate) max_recording_ms: u64,
}

/// One paired client, as Hex's window lists it for revocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GrantView {
    pub(crate) id: u64,
    pub(crate) recipient: Recipient,
}

struct Code {
    challenge: [u8; 32],
    channel: Channel,
    recipient: Recipient,
    expires: Instant,
}

struct Grant {
    id: u64,
    channel: Channel,
    recipient: Recipient,
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
    /// When the current phase's lease runs out.
    deadline: Instant,
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
    pub(super) recording: Duration,
    pub(super) grace: Duration,
    pub(super) prompt: Duration,
    pub(super) cooldown: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            code: CODE_LIFETIME,
            recording: MAX_RECORDING,
            grace: FINISH_GRACE,
            prompt: PROMPT_WAIT,
            cooldown: DENY_COOLDOWN,
        }
    }
}

/// Every pairing and recording session the running app serves.
pub(crate) struct Sessions {
    origins: fn() -> Vec<String>,
    enabled: fn() -> bool,
    gate: Option<Arc<dyn ConsentGate>>,
    recording: Arc<dyn Recording>,
    timings: Timings,
    state: Mutex<State>,
}

impl Sessions {
    pub(crate) fn new(
        origins: fn() -> Vec<String>,
        enabled: fn() -> bool,
        gate: Option<Arc<dyn ConsentGate>>,
        recording: Arc<dyn Recording>,
    ) -> Self {
        Self {
            origins,
            enabled,
            gate,
            recording,
            timings: Timings::default(),
            state: Mutex::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_timings(
        origins: fn() -> Vec<String>,
        enabled: fn() -> bool,
        gate: Option<Arc<dyn ConsentGate>>,
        recording: Arc<dyn Recording>,
        timings: Timings,
    ) -> Self {
        Self {
            origins,
            enabled,
            gate,
            recording,
            timings,
            state: Mutex::default(),
        }
    }

    /// Whether a browser page may pair or spend a grant: its origin is
    /// canonical and listed.
    pub(super) fn origin_allowed(&self, origin: &str) -> bool {
        let canonical = crate::app_settings::normalize_browser_origins(&[origin.to_owned()]);
        canonical.len() == 1
            && canonical[0] == origin
            && (self.origins)().iter().any(|allowed| allowed == origin)
    }

    /// This request's channel, when it names one Hex accepts. A browser
    /// request needs a listed `Origin`; a native request's channel is its
    /// verified peer.
    pub(super) fn web_channel(&self, origin: Option<&str>) -> Option<Channel> {
        origin
            .filter(|origin| self.origin_allowed(origin))
            .map(|origin| Channel::Web(origin.to_owned()))
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
            remaining_ms: remaining(session.deadline),
            max_recording_ms: self.timings.recording.as_millis() as u64,
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
            })
            .collect();
        grants.sort_by_key(|grant| grant.id);
        grants
    }

    /// Revokes pairing `id` (the window's Revoke). A recording it is running
    /// stops and is discarded. False when no pairing has that id.
    pub(crate) fn revoke(&self, id: u64) -> bool {
        let mut state = lock(&self.state);
        let Some(token) = state
            .grants
            .iter()
            .find(|(_, grant)| grant.id == id)
            .map(|(token, _)| token.clone())
        else {
            return false;
        };
        self.revoke_locked(&mut state, &token);
        true
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
                session.phase = Phase::Cancelled;
                state.active = None;
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
        state.active = None;
    }

    /// The recording's lease passed: stop it, then discard it if the client
    /// never collected the transcript.
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
        state.active = None;
    }

    /// `POST /record/pair`: the window asks once whether this caller may
    /// start recordings. Pairing starts nothing.
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
        if !(self.enabled)() {
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
            Answer::Allow => {
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
                        expires: Instant::now() + self.timings.code,
                    },
                );
                HttpResponse::json(
                    200,
                    &serde_json::json!({
                        "code": code,
                        "expiresInMs": self.timings.code.as_millis(),
                        "scope": "record:pair",
                        "state": request.state,
                    }),
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

    /// `POST /record/token`: spends the pairing code and mints one revocable
    /// grant token. No recording starts.
    pub(super) fn exchange(&self, channel: Channel, body: &[u8]) -> HttpResponse {
        let Ok(request) = serde_json::from_slice::<TokenRequest>(body) else {
            return HttpResponse::code(400, "invalid-request");
        };
        if !valid_verifier(&request.code_verifier) {
            return HttpResponse::code(400, "invalid-request");
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
            },
        );
        HttpResponse::json(
            200,
            &serde_json::json!({
                "grantId": id,
                "grantToken": token,
                "scope": "record:any",
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
                deadline: now + self.timings.recording,
            },
        );
        state.active = Some(session_token.clone());
        let lease = self.timings.recording;
        drop(state);
        // The lease stops the recording on its own, whether or not the
        // client comes back.
        let weak: Weak<Self> = Arc::downgrade(self);
        thread::Builder::new()
            .name("record-lease".into())
            .spawn(move || {
                thread::sleep(lease);
                if let Some(sessions) = weak.upgrade() {
                    sessions.lease_expired(id);
                }
            })
            .ok();
        HttpResponse::json(
            201,
            &serde_json::json!({
                "id": id,
                "sessionToken": session_token,
                "state": Phase::Recording.name(),
                "maxRecordingMs": lease.as_millis(),
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
            let id = session.id;
            drop(state);
            self.lease_expired(id);
            return self.status(token, channel);
        }
        HttpResponse::json(
            200,
            &serde_json::json!({
                "id": session.id,
                "state": session.phase.name(),
                "remainingMs": remaining(session.deadline),
                "maxRecordingMs": self.timings.recording.as_millis(),
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
        let id = session.id;
        if state.active.as_deref() == Some(token) {
            state.active = None;
        }
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
        if state.active.as_deref() == Some(token) {
            state.active = None;
        }
        HttpResponse::empty(204)
    }

    /// `POST /record/revoke`: the paired client gives its grant back.
    pub(super) fn revoke_token(&self, token: &str, channel: &Channel) -> HttpResponse {
        let mut state = lock(&self.state);
        match state.grants.get(token) {
            Some(grant) if grant.channel == *channel => {}
            _ => return HttpResponse::empty(401),
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
            }),
            Channel::Native { identity, .. } => Some(Recipient::Native {
                identifier: identity.identifier.clone(),
                team: identity.team.clone(),
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

fn recipient_key(recipient: &Recipient) -> String {
    match recipient {
        Recipient::Web { origin, .. } => format!("web:{origin}"),
        Recipient::Native { identifier, team } => {
            format!(
                "native:{identifier}:{}",
                team.as_deref().unwrap_or_default()
            )
        }
    }
}

/// Where the app's native recording socket lives.
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

/// The longest a consented recording can run, for the window's copy.
pub(crate) fn max_recording() -> Duration {
    MAX_RECORDING
}

/// The app's sessions: its capture loop, the window's gate, and the Labs
/// switch that turns both recording and browser access on.
pub(crate) fn app_sessions(
    gate: Arc<dyn ConsentGate>,
    control: super::dictation::Control,
) -> Arc<Sessions> {
    Arc::new(Sessions::new(
        crate::labs::api_browser_origins,
        crate::labs::api_browser_access,
        Some(gate),
        Arc::new(super::dictation::DictationRecording::new(control)),
    ))
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
            Self::Session {
                action,
                token,
                channel,
            } => match context.record.as_ref() {
                Some(sessions) => action.respond(sessions, &token, &channel),
                None => HttpResponse::code(403, "record-unavailable"),
            },
            Self::Revoke { token, channel } => match context.record.as_ref() {
                Some(sessions) => sessions.revoke_token(&token, &channel),
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
        let Some(channel) = sessions.web_channel(request.origin.as_deref()) else {
            return Routed::Record(Request::Refused(HttpResponse::code(
                403,
                "origin-not-allowed",
            )));
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
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Action {
    Start,
    Status,
    Finish,
    Cancel,
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

fn random_token(prefix: &str) -> Result<String, getrandom::Error> {
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

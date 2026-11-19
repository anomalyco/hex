//! Browser connect: a web page asks Hex to transcribe audio, and Hex's own
//! window asks the person.
//!
//! The page never sees the local API's bearer token. It starts a small
//! OAuth-shaped exchange instead:
//!
//! 1. `POST /connect/authorize` with a PKCE `codeChallenge` (S256) and the
//!    page's `Origin`. Hex shows a native dialog naming that origin; Allow
//!    Once answers with a one-use `code`, Deny answers `403 consent-denied`.
//! 2. `POST /connect/token` with the `code` and the `codeVerifier` mints a
//!    one-use `audioToken`, valid only for plain `POST /transcriptions` from
//!    the same origin.
//! 3. `POST /transcriptions` with `X-Hex-Connect-Token` spends the token,
//!    whatever the audio turns out to be.
//!
//! Codes and tokens expire (two minutes each), and one code, one token, and
//! one transcription are all the grant is worth. The token reaches nothing
//! else: not `/control`, `/dictations`, `/streams`, model preparation, or the
//! developer control. Only a website Labs › Connected apps allows may ask
//! (see [`super::access`]): any other is refused before the dialog, and a
//! Deny starts a short cooldown so a page cannot reopen the dialog in a loop.
//!
//! Always Allow saves the capability for this exact `Origin` in
//! [`Permissions`]. A later ask from the same origin skips the dialog and
//! mints a fresh one-operation code; the saved permission survives a restart
//! until the person revokes it in the Connected apps pane.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::access::Access;
use super::consent::{Answer, ConsentGate, Intent, Prompt, Recipient};
use super::http::HttpResponse;
use super::permissions::Permissions;
use super::{HttpRequest, read_body_bounded};

pub(super) const AUTHORIZE_PATH: &str = "/connect/authorize";
pub(super) const TOKEN_PATH: &str = "/connect/token";
/// How long the person has to answer the dialog.
pub(super) const PROMPT_WAIT: Duration = Duration::from_secs(60);
/// How long an allowed grant's code lives, and then its audio token.
pub(super) const GRANT_LIFETIME: Duration = Duration::from_secs(120);
/// How long an origin that was denied waits before it may prompt again.
pub(super) const DENY_COOLDOWN: Duration = Duration::from_secs(30);
/// The connect bodies are two short JSON objects; an ask may also carry the
/// page's favicon (at most 64 KiB, base64 in a `data:` URL).
pub(super) const MAX_BODY_BYTES: usize = 96 * 1024;
const BODY_DEADLINE: Duration = Duration::from_secs(5);

struct Grant {
    permission: Option<u64>,
    challenge: [u8; 32],
    origin: String,
    expires: Instant,
}

struct Audio {
    permission: Option<u64>,
    origin: String,
    expires: Instant,
}

#[derive(Default)]
struct State {
    /// Codes from allowed prompts, by code.
    grants: HashMap<String, Grant>,
    /// Minted audio tokens, by token.
    audio: HashMap<String, Audio>,
    next_prompt: u64,
    /// One dialog at a time: another page waits for this one to close.
    prompting: bool,
}

/// The running app's browser-connect gate: Labs › Connected apps, the
/// window's consent gate, saved Always Allow permissions (and the lab's
/// website lists beside them), and the one-use grants between them.
pub(crate) struct Connect {
    access: Access,
    gate: Option<Arc<dyn ConsentGate>>,
    permissions: Arc<Permissions>,
    code_lifetime: Duration,
    token_lifetime: Duration,
    prompt_wait: Duration,
    cooldown: Duration,
    state: Mutex<State>,
}

impl Connect {
    pub(crate) fn new(
        access: Access,
        gate: Option<Arc<dyn ConsentGate>>,
        permissions: Arc<Permissions>,
    ) -> Self {
        Self {
            access,
            gate,
            permissions,
            code_lifetime: GRANT_LIFETIME,
            token_lifetime: GRANT_LIFETIME,
            prompt_wait: PROMPT_WAIT,
            cooldown: DENY_COOLDOWN,
            state: Mutex::default(),
        }
    }

    /// A gate with its own timings, so route tests need not wait minutes or
    /// touch the process's Labs state.
    #[cfg(test)]
    pub(crate) fn with_timings(
        access: Access,
        gate: Option<Arc<dyn ConsentGate>>,
        permissions: Arc<Permissions>,
        code_lifetime: Duration,
        token_lifetime: Duration,
        prompt_wait: Duration,
        cooldown: Duration,
    ) -> Self {
        Self {
            access,
            gate,
            permissions,
            code_lifetime,
            token_lifetime,
            prompt_wait,
            cooldown,
            state: Mutex::default(),
        }
    }

    /// Labs › Connected apps' switches, for the routes' CORS.
    pub(super) fn access(&self) -> Access {
        self.access
    }

    /// Whether this page may still use what it was given: Connected apps
    /// allows its exact origin.
    pub(super) fn origin_allowed(&self, origin: &str) -> bool {
        self.access.website_allowed(&self.permissions, origin)
    }

    /// Spends `token` for one transcription from `origin`. The token is gone
    /// whatever the answer, so a page gets exactly one try.
    pub(super) fn take_audio(&self, token: &str, origin: Option<&str>) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let Some(audio) = state.audio.remove(token) else {
            return false;
        };
        let now = Instant::now();
        audio
            .permission
            .is_none_or(|id| self.permissions.current(id))
            && audio.expires > now
            && origin == Some(audio.origin.as_str())
            && self.origin_allowed(&audio.origin)
    }

    /// Reads one connect request's body and answers it.
    pub(super) fn respond(
        &self,
        call: Call,
        stream: &mut std::net::TcpStream,
        shutdown: &std::sync::atomic::AtomicBool,
    ) -> HttpResponse {
        let mut body = call.body;
        if let Err(error) = read_body_bounded(
            stream,
            call.content_length,
            &mut body,
            shutdown,
            MAX_BODY_BYTES,
            BODY_DEADLINE,
        ) {
            return super::http::body_read_response(error);
        }
        match call.endpoint {
            Endpoint::Authorize => self.authorize(&call.origin, &body),
            Endpoint::Token => self.exchange(&call.origin, &body),
        }
    }

    fn authorize(&self, origin: &str, body: &[u8]) -> HttpResponse {
        let Ok(request) = serde_json::from_slice::<AuthorizeRequest>(body) else {
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
        let client_name = request.client_name.filter(|name| plain_name(name));
        let Some(gate) = self.gate.clone() else {
            return HttpResponse::code(403, "connect-unavailable");
        };
        // The page's favicon, decoded here or ignored; Hex never fetches it.
        let icon = request.icon.as_deref().and_then(super::icon::from_data_url);
        let recipient = Recipient::Web {
            origin: origin.to_owned(),
            client_name,
            icon,
        };
        // The pane shows what it last asked with, even while it's offline.
        self.permissions.keep_display(&recipient);
        // Always Allow is the person's earlier answer for this exact origin,
        // so this ask needs no dialog. A saved permission never carries the
        // page's claimed name, only the `Origin` Hex verified.
        let saved = match self.permissions.authorize_saved(
            Intent::Transcribe,
            &recipient,
            request.permission_proof.as_deref(),
        ) {
            Ok(saved) => saved,
            Err(()) => return HttpResponse::code(403, "permission-proof-required"),
        };
        // One cooldown per recipient across both asks.
        let cooled = super::record::recipient_key(&recipient);
        let answer = if saved.is_some() {
            Answer::Once
        } else {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if self.permissions.cooling(&cooled, self.cooldown) {
                return HttpResponse::code(429, "consent-cooldown");
            }
            if state.prompting {
                return HttpResponse::code(409, "consent-busy");
            }
            state.next_prompt += 1;
            let prompt = Prompt {
                id: state.next_prompt,
                intent: Intent::Transcribe,
                recipient: recipient.clone(),
            };
            state.prompting = true;
            drop(state);

            let answer = gate.ask(&prompt, self.prompt_wait);

            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.prompting = false;
            answer
        };

        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        match answer {
            Answer::Once => {
                self.issue_code(&mut state, challenge, origin, request.state, saved, None)
            }
            Answer::Always => {
                // Save before issuing anything: a permission Hex cannot write
                // must never behave like Once.
                let (id, proof) = match self.permissions.remember(Intent::Transcribe, &recipient) {
                    Ok(id) => id,
                    Err(error) => {
                        tracing::warn!(%error, "could not save an Always Allow permission");
                        return HttpResponse::code(500, "permission-not-saved");
                    }
                };
                self.issue_code(
                    &mut state,
                    challenge,
                    origin,
                    request.state,
                    Some(id),
                    proof,
                )
            }
            Answer::Deny => {
                self.permissions.refused(&cooled);
                HttpResponse::code(403, "consent-denied")
            }
            Answer::Timeout => {
                self.permissions.refused(&cooled);
                HttpResponse::code(403, "consent-timeout")
            }
            Answer::Unavailable => HttpResponse::code(403, "connect-unavailable"),
        }
    }

    /// One allowed ask's code: one exchange, then one transcription.
    fn issue_code(
        &self,
        state: &mut State,
        challenge: Vec<u8>,
        origin: &str,
        client_state: Option<String>,
        permission: Option<u64>,
        proof: Option<String>,
    ) -> HttpResponse {
        state
            .grants
            .retain(|_, grant| grant.expires > Instant::now());
        let code = random_token("hexc_");
        state.grants.insert(
            code.clone(),
            Grant {
                permission,
                challenge: challenge.try_into().expect("32 bytes were checked"),
                origin: origin.to_owned(),
                expires: Instant::now() + self.code_lifetime,
            },
        );
        HttpResponse::json(
            200,
            &serde_json::json!({
                "code": code,
                "permissionProof": proof,
                "expiresInMs": self.code_lifetime.as_millis(),
                "origin": origin,
                "scope": "transcription:transcribe",
                "state": client_state,
            }),
        )
    }

    fn exchange(&self, origin: &str, body: &[u8]) -> HttpResponse {
        let Ok(request) = serde_json::from_slice::<TokenRequest>(body) else {
            return HttpResponse::code(400, "invalid-request");
        };
        if !valid_verifier(&request.code_verifier) {
            return HttpResponse::code(400, "invalid-request");
        }
        let challenge = sha256(request.code_verifier.as_bytes());
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let now = Instant::now();
        // A code is spent by its first exchange, right or wrong, so a page
        // cannot guess a verifier against the same consent.
        let Some(grant) = state.grants.remove(&request.code) else {
            return HttpResponse::code(403, "invalid-grant");
        };
        if grant
            .permission
            .is_some_and(|id| !self.permissions.current(id))
            || grant.expires <= now
            || grant.origin != origin
            || !equal(&grant.challenge, &challenge)
        {
            return HttpResponse::code(403, "invalid-grant");
        }
        state.audio.retain(|_, audio| audio.expires > now);
        let token = random_token("hexa_");
        state.audio.insert(
            token.clone(),
            Audio {
                permission: grant.permission,
                origin: origin.to_owned(),
                expires: now + self.token_lifetime,
            },
        );
        HttpResponse::json(
            200,
            &serde_json::json!({
                "audioToken": token,
                "expiresInMs": self.token_lifetime.as_millis(),
                "origin": origin,
                "scope": "transcription:transcribe",
            }),
        )
    }
}

/// What a connect-shaped request means to the local API's router.
pub(super) enum Intercept {
    /// Answer the page's connect body on the worker.
    Connect(Call),
    /// A minted token spends itself here, and the router continues with the
    /// request as a scoped transcription.
    Audio(HttpRequest),
    /// Answer without routing further (no window, a bad token, a wrong
    /// origin, or a wrong content type).
    Refuse(HttpResponse),
    /// Not a connect request: route it as it always was.
    Pass(HttpRequest),
}

/// Checks for the connect routes and the `X-Hex-Connect-Token` header before
/// the bearer token, so a page can ask without ever holding it.
pub(super) fn intercept(request: HttpRequest, connect: Option<&Connect>) -> Intercept {
    let path = request.path.split('?').next().unwrap_or("/");
    if request.method == "POST"
        && let Some(endpoint) = Endpoint::of(path)
    {
        let Some(connect) = connect else {
            return Intercept::Refuse(HttpResponse::code(403, "connect-unavailable"));
        };
        return match Call::admit(connect, endpoint, request) {
            Ok(call) => Intercept::Connect(call),
            Err(response) => Intercept::Refuse(response),
        };
    }
    let Some(token) = request.connect_token.clone() else {
        return Intercept::Pass(request);
    };
    // The grant covers one plain transcription from its origin, and nothing
    // else: any other route or method answers exactly as a wrong bearer
    // token does.
    let scoped = request.method == "POST"
        && path == "/transcriptions"
        && connect.is_some_and(|connect| connect.take_audio(&token, request.origin.as_deref()));
    if scoped {
        Intercept::Audio(request)
    } else {
        Intercept::Refuse(HttpResponse::empty(401))
    }
}

/// Refuses a connect transcription that asks for more than a plain transcript:
/// `process=global` and `timestamps=…` are the bearer-token API's options,
/// not this grant's scope.
pub(super) fn connect_scope_refusal(path: &str) -> Option<HttpResponse> {
    (query_has(path, "process") || query_has(path, "timestamps"))
        .then(|| HttpResponse::code(403, "connect-scope"))
}

/// Whether the URL's query carries `name` as a key, whatever its value.
fn query_has(path: &str, name: &str) -> bool {
    path.split_once('?').is_some_and(|(_, query)| {
        query
            .split('&')
            .any(|pair| pair.split('=').next() == Some(name))
    })
}

/// Which connect route a request asks for.
#[derive(Clone, Copy, Debug)]
pub(super) enum Endpoint {
    Authorize,
    Token,
}

impl Endpoint {
    pub(super) fn of(path: &str) -> Option<Self> {
        match path {
            AUTHORIZE_PATH => Some(Self::Authorize),
            TOKEN_PATH => Some(Self::Token),
            _ => None,
        }
    }
}

/// One admitted connect request, before its body is read.
pub(super) struct Call {
    endpoint: Endpoint,
    origin: String,
    content_length: usize,
    body: Vec<u8>,
}

impl Call {
    /// Checks everything that needs no body: the page's origin, the JSON
    /// content type, and a declared, bounded length.
    pub(super) fn admit(
        connect: &Connect,
        endpoint: Endpoint,
        request: HttpRequest,
    ) -> Result<Self, HttpResponse> {
        // Only an ask lists a website that tried; a token exchange can't come
        // from one that never asked.
        let origin = connect.access.admit_website(
            &connect.permissions,
            request.origin.as_deref(),
            matches!(endpoint, Endpoint::Authorize),
        )?;
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
            origin,
            content_length,
            body: request.body_prefix,
        })
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthorizeRequest {
    permission_proof: Option<String>,
    code_challenge: String,
    #[serde(default)]
    code_challenge_method: Option<String>,
    #[serde(default)]
    client_name: Option<String>,
    /// The page's favicon as a `data:` URL (PNG or ICO). Display only.
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenRequest {
    code: String,
    #[serde(alias = "code_verifier")]
    code_verifier: String,
}

/// A page's self-description may be shown in Hex's dialog, so only a short
/// plain line passes: no controls, no bidi overrides, no run of spaces.
fn plain_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name.chars().count() <= 40
        && name.chars().all(|character| {
            !character.is_control() && !('\u{202a}'..='\u{202e}').contains(&character)
        })
        && !name.contains("  ")
}

/// RFC 7636's `code_verifier`: 43 to 128 unreserved characters.
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

/// Compares two digests without stopping at the first differing byte.
fn equal(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |mismatch, (a, b)| mismatch | (a ^ b))
            == 0
}

/// `prefix` plus 32 random bytes as hex, like the API token.
fn random_token(prefix: &str) -> String {
    use std::fmt::Write as _;
    let mut random = [0_u8; 32];
    // `getrandom` failing is not something a request can recover from; a
    // zero token would still be one-use, but keep the failure visible.
    if let Err(error) = getrandom::fill(&mut random) {
        tracing::error!(%error, "could not generate a connect token");
        random = [0xFF; 32];
    }
    let mut token = String::with_capacity(prefix.len() + 64);
    token.push_str(prefix);
    for byte in random {
        write!(token, "{byte:02x}").expect("writing to a string cannot fail");
    }
    token
}

/// Base64url without padding, as PKCE's S256 challenge is written.
fn decode_base64url(text: &str) -> Option<Vec<u8>> {
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
    // Leftover bits must be zero, and no trailing character may encode more
    // than a byte's worth.
    (bits < 6 && buffer & ((1 << bits) - 1) == 0).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_matches_rfc7636s_example() {
        // The RFC's S256 example: verifier ...Kx4Z, challenge ...2Y8.
        assert_eq!(
            decode_base64url("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM").unwrap(),
            sha256(b"dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")
        );
        assert_eq!(decode_base64url("aQ").unwrap(), vec![0x69]);
        assert!(decode_base64url("a").is_none());
        assert!(decode_base64url("ab==").is_none());
        assert!(decode_base64url("a+b/").is_none());
    }

    #[test]
    fn a_page_name_is_shown_only_when_plain_and_short() {
        assert!(plain_name("Field Notes"));
        assert!(!plain_name(""));
        assert!(!plain_name("two  spaces"));
        assert!(!plain_name("line\nbreak"));
        assert!(!plain_name("right\u{202e}to left"));
        assert!(!plain_name(&"long".repeat(11)));
    }

    #[test]
    fn a_verifier_is_checked_against_rfc7636s_grammar() {
        assert!(valid_verifier(&"a".repeat(43)));
        assert!(valid_verifier(&"a".repeat(128)));
        assert!(!valid_verifier(&"a".repeat(42)));
        assert!(!valid_verifier(&"a".repeat(129)));
        assert!(!valid_verifier(
            "has space and that is not allowed at all.."
        ));
    }

    #[test]
    fn digests_compare_by_content_only() {
        assert!(equal(&[1, 2, 3], &[1, 2, 3]));
        assert!(!equal(&[1, 2, 3], &[1, 2, 4]));
        assert!(!equal(&[1, 2], &[1, 2, 3]));
    }
}

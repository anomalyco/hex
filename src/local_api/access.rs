//! Labs › Connected apps as the consent routes read it: who may ask Hex's
//! permission window, and which browser requests those routes answer CORS
//! for.
//!
//! The lab lets any local app ask (a native one is named by its code
//! signature). Websites ask only with the lab's Allow websites to use Hex on,
//! and only once the person allowed their exact origin in the lab's list.
//! Being allowed to ask grants nothing: access still needs Allow Once or
//! Always Allow in the window. A website that is neither allowed nor blocked
//! gets no window: its first consent or pairing request is answered
//! `403 website-not-allowed` and its origin is listed (origin and time only)
//! under Asking to connect in the Connected apps pane, where the person may allow or
//! block it. A blocked website answers `403 website-blocked` and is never
//! listed again.
//!
//! With websites on, the consent, pairing, record, and connect-grant routes
//! answer CORS for any `http(s)` origin, so an unlisted page can read why it
//! was refused. No other route answers a browser, `*` is never sent, and
//! neither are credentials.

use std::sync::Arc;

use super::consent::{ConsentGate, Recipient};
use super::http::HttpResponse;
use super::origin::{CorsOrigin, canonical};
use super::permissions::{Permissions, WebsiteStatus};

/// The lab's two switches, read on every request so turning either off acts
/// at once.
#[derive(Clone, Copy)]
pub(crate) struct Access {
    lab: fn() -> bool,
    websites: fn() -> bool,
}

impl Access {
    /// The running app's Labs › Connected apps.
    pub(crate) fn app() -> Self {
        Self {
            lab: crate::labs::connected_apps,
            websites: crate::labs::connected_apps_websites,
        }
    }

    /// Switches under a test's control, so route tests need no Labs state.
    #[cfg(test)]
    pub(crate) fn gated(lab: fn() -> bool, websites: fn() -> bool) -> Self {
        Self { lab, websites }
    }

    /// The lab is on: apps may ask.
    pub(crate) fn lab(self) -> bool {
        (self.lab)()
    }

    /// The lab and its Allow websites to use Hex are both on.
    pub(crate) fn websites(self) -> bool {
        (self.lab)() && (self.websites)()
    }

    /// Whether a consent or pairing ask from `origin` may reach the window.
    /// An origin the person has not decided on is listed when `record` (a
    /// consent or pairing ask), and refused either way.
    pub(super) fn admit_website(
        self,
        permissions: &Permissions,
        origin: Option<&str>,
        record: bool,
    ) -> Result<String, HttpResponse> {
        // The lab or its websites switch off answers as the routes did before
        // either existed, and records nothing.
        let refused = || HttpResponse::code(403, "origin-not-allowed");
        if !self.websites() {
            return Err(refused());
        }
        let Some(origin) = origin.filter(|origin| canonical(origin)) else {
            return Err(refused());
        };
        match permissions.website(origin) {
            WebsiteStatus::Allowed => Ok(origin.to_owned()),
            WebsiteStatus::Blocked => Err(HttpResponse::code(403, "website-blocked")),
            WebsiteStatus::Unknown => {
                if record {
                    permissions.website_tried(origin);
                }
                Err(HttpResponse::code(403, "website-not-allowed"))
            }
        }
    }

    /// Whether a website's token-bearing request (a grant, a session, an
    /// upload) may be served: the same rule, without listing anything.
    pub(super) fn website_allowed(self, permissions: &Permissions, origin: &str) -> bool {
        self.websites()
            && canonical(origin)
            && permissions.website(origin) == WebsiteStatus::Allowed
    }

    /// Whether `recipient` may still ask, or use what it was given.
    pub(super) fn admits(self, permissions: &Permissions, recipient: &Recipient) -> bool {
        match recipient {
            Recipient::Web { origin, .. } => self.website_allowed(permissions, origin),
            Recipient::Native { .. } => self.lab(),
        }
    }

    /// The request's origin when Connected apps answers CORS for it: the
    /// websites switch is on, the origin is an `http(s)` origin, and the
    /// route is one of the consent, pairing, record, or connect-grant
    /// routes. A `/transcriptions` request counts only as a connect grant's
    /// upload (with its token and without the bearer token); its preflight
    /// names no `Authorization` header (see [`HttpResponse::consent_preflight`]).
    pub(super) fn cors(self, request: &super::HttpRequest) -> Option<CorsOrigin> {
        if !self.websites() {
            return None;
        }
        let origin = request
            .origin
            .as_deref()
            .filter(|origin| canonical(origin))?;
        let path = request.path.split('?').next().unwrap_or("/");
        let routed = match path {
            super::connect::AUTHORIZE_PATH
            | super::connect::TOKEN_PATH
            | super::record::PAIR_PATH
            | super::record::TOKEN_PATH
            | super::record::START_PATH
            | super::record::STATUS_PATH
            | super::record::FINISH_PATH
            | super::record::CANCEL_PATH
            | super::record::REVOKE_PATH
            | super::record::HEARTBEAT_PATH => true,
            "/transcriptions" => {
                request.method == "OPTIONS"
                    || (request.connect_token.is_some() && request.authorization.is_none())
            }
            _ => false,
        };
        routed.then(|| CorsOrigin {
            origin: origin.to_owned(),
            private_network: request.private_network,
        })
    }
}

/// The Linux and Windows apps' two asks, browser connect and consented
/// recording, answered in the window through `gate`; native clients ask over
/// the native channel `LocalApi::start_for_app` opens.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(crate) struct AppConsent {
    pub(super) connect: super::Connect,
    /// The window's Stop, Cancel, and Revoke reach these.
    pub(crate) record: Arc<super::RecordSessions>,
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
impl AppConsent {
    pub(crate) fn new(gate: Arc<dyn ConsentGate>, control: super::dictation::Control) -> Self {
        let permissions = Permissions::app();
        Self {
            record: super::record::app_sessions(gate.clone(), control, permissions.clone()),
            connect: super::Connect::new(Access::app(), Some(gate), permissions),
        }
    }
}

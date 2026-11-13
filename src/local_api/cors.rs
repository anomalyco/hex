//! Browser access to the local API (Labs › Browser access to the local API):
//! the request origins this API answers cross-origin browser requests for.
//!
//! A browser sends an unauthenticated `OPTIONS` preflight before a page's
//! `fetch` can carry its bearer token, so a preflight for an allowed origin
//! is answered without authentication; every other request still needs the
//! token exactly as before, and its response (a `401` included) carries the
//! same `Access-Control-Allow-Origin`. Nothing here sends
//! `Access-Control-Allow-Credentials`, and `*` is never allowed.

use super::HttpRequest;

/// An allowed request origin, and whether the browser asked for
/// private-network access (a page on the public internet reaching this
/// loopback API).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CorsOrigin {
    pub(super) origin: String,
    pub(super) private_network: bool,
}

/// How this local API learns its allowed browser origins, read for each
/// request so turning the lab off takes effect at once.
#[derive(Clone)]
pub(crate) struct Cors {
    origins: fn() -> Vec<String>,
}

impl Default for Cors {
    fn default() -> Self {
        Self {
            origins: crate::labs::api_browser_origins,
        }
    }
}

impl Cors {
    /// A gate under a test's control, so route tests need no global Labs
    /// state.
    #[cfg(test)]
    pub(super) fn gated(origins: fn() -> Vec<String>) -> Self {
        Self { origins }
    }

    /// The request's `Origin` when the lab allows it, with the
    /// private-network request it carried. No lab, no `Origin`, or an origin
    /// that isn't listed leaves the response exactly as it was.
    pub(super) fn allowed(&self, request: &HttpRequest) -> Option<CorsOrigin> {
        let origin = request.origin.as_deref()?;
        (self.origins)()
            .iter()
            .any(|allowed| allowed == origin)
            .then(|| CorsOrigin {
                origin: origin.to_owned(),
                private_network: request.private_network,
            })
    }
}

//! Labs › Connected apps' routes, served before the bearer token: a consent
//! route's CORS preflight, browser connect, and one consented recording.

use super::origin::CorsOrigin;
use super::{HttpContext, HttpRequest, HttpResponse, RequestAction, connect, record};

/// The origin a consent route answers CORS for. Labs › Connected apps
/// answers any website on its own routes only; no other route answers a
/// browser.
pub(super) fn cors(request: &HttpRequest, context: &HttpContext) -> Option<CorsOrigin> {
    context.connected.and_then(|access| access.cors(request))
}

/// A consent route's CORS preflight, answered before authentication: a
/// browser's preflight carries no credentials, so it cannot hold the bearer
/// token.
pub(super) fn preflight(request: &HttpRequest, cors: Option<&CorsOrigin>) -> Option<RequestAction> {
    let cors = cors.filter(|_| request.method == "OPTIONS")?;
    Some(RequestAction::Respond(HttpResponse::consent_preflight(
        cors.private_network,
    )))
}

/// A consent route's action, or a request left for the bearer-token routes.
pub(super) enum Intercepted {
    Action(RequestAction),
    Pass(HttpRequest),
}

/// Browser connect, then one consented recording: an allowed page asks
/// without the bearer token, and a session token reaches only the record
/// action it names.
pub(super) fn intercept(request: HttpRequest, context: &HttpContext) -> Intercepted {
    use Intercepted::{Action, Pass};
    let request = match connect::intercept(request, context.connect.as_ref()) {
        connect::Intercept::Refuse(response) => return Action(RequestAction::Respond(response)),
        connect::Intercept::Connect(call) => return Action(RequestAction::Connect(call)),
        connect::Intercept::Audio(request) => {
            return Action(super::transcription_action(
                request,
                &context.transcription,
                &context.processing,
                &context.timestamps,
                true,
            ));
        }
        connect::Intercept::Pass(request) => request,
    };
    match record::route(request, context) {
        record::Routed::Record(request) => Action(RequestAction::Record(request)),
        record::Routed::Pass(request) => Pass(request),
    }
}

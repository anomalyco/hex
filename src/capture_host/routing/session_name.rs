//! Session names for the capsule and the bundle: a title, never a raw `ses…` id.

use super::*;

/// What the capsule calls a session whose title it never learned.
const SESSION: &str = "session";

/// Whether `name` is a raw session id rather than a title.
fn is_session_id(name: &str) -> bool {
    name.trim().starts_with("ses_")
}

/// `name` when it is a real name: never the raw `ses…` id the router falls
/// back to when a session's title wasn't read.
fn titled(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty() && !is_session_id(name)).then(|| name.to_owned())
}

/// The name the capsule shows for `session`: the router's name when that is
/// one, else the title from this capture's sessions, else the title OpenCode
/// knows it by, else "session". What the capsule shows is never a raw
/// `ses…` id (Kit, t1311: a rule that named a session by id put one in the
/// pill).
pub(super) fn session_name(session: &str, name: &str, candidates: &[smart::Candidate]) -> String {
    session_name_by(session, name, candidates, server_title)
}

/// [`session_name`] with the server lookup passed in.
pub(super) fn session_name_by(
    session: &str,
    name: &str,
    candidates: &[smart::Candidate],
    lookup: impl FnOnce(&str) -> Option<String>,
) -> String {
    titled(name)
        .or_else(|| {
            candidates
                .iter()
                .find(|candidate| candidate.id == session)
                .and_then(|candidate| titled(&candidate.title))
        })
        .or_else(|| lookup(session).and_then(|title| titled(&title)))
        .unwrap_or_else(|| SESSION.to_owned())
}

/// A session's title from OpenCode, for a route that had only its id (the
/// early local read, or a saved rule). One bounded request; `None` when it
/// can't answer (tests never ask).
fn server_title(session: &str) -> Option<String> {
    if cfg!(test) {
        return None;
    }
    let mut api = routing::within(routing::Hooks::default().api, routing::SESSIONS_BUDGET);
    scrawl_capture::opencode::get_session(&mut api, session)
        .ok()
        .map(|session| session.title)
}

/// Names every session an answer lists, and a send's own destination, so
/// the capsule and the bundle read titles: a rule, or a list read without a
/// title, would otherwise show the session's `ses…` id.
pub(super) fn name_answer(
    answer: &mut RouteAnswer,
    send: Option<&mut Destination>,
    candidates: &[smart::Candidate],
) {
    let fix =
        |dest: &mut RouteDest| dest.name = session_name(&dest.session, &dest.name, candidates);
    match answer {
        RouteAnswer::Looking | RouteAnswer::Unreachable { .. } => {}
        RouteAnswer::Rule {
            to,
            suggested,
            recent,
            ..
        } => {
            fix(to);
            suggested.iter_mut().chain(recent.iter_mut()).for_each(fix);
        }
        RouteAnswer::Guess {
            suggested, recent, ..
        } => suggested.iter_mut().chain(recent.iter_mut()).for_each(fix),
        RouteAnswer::Nothing { recent } | RouteAnswer::NoAnswer { recent } => {
            recent.iter_mut().for_each(fix);
        }
        // Its folder's name is already a name; a session found later is named
        // by its own answer.
        RouteAnswer::ResolvingRule { .. } => {}
    }
    if let Some(send) = send {
        send.title = session_name(&send.session, &send.title, candidates);
    }
}

//! Labs › Connected apps over the routes: its two switches, the website
//! lists, and its CORS.

use std::sync::atomic::AtomicBool;

use super::*;

/// The API with `access` in front of both consent asks, `gate` answering.
fn start(
    access: Access,
    gate: Arc<dyn ConsentGate>,
    allowed: &[String],
) -> (LocalApi, Arc<record::Sessions>, Arc<Permissions>) {
    let directory = temp_directory();
    let store = allowing(
        Permissions::new(Some(directory.join("permissions.json"))),
        allowed,
    );
    let sessions = Arc::new(record::Sessions::with_timings(
        access,
        Some(gate.clone()),
        FakeRecording::with_text("words"),
        store.clone(),
        record_timings(),
    ));
    let api = start_unpublished(
        EventLog::create(&directory.join("events.ndjson")).unwrap(),
        LocalApiOptions {
            connect: Some(Connect::new(access, Some(gate), store.clone())),
            record: Some(sessions.clone()),
            stream_engine: Some(crate::transcription::stream::tests::fake_source()),
            ..LocalApiOptions::default()
        },
    )
    .unwrap();
    (api, sessions, store)
}

fn off() -> bool {
    false
}

fn ask_body() -> String {
    format!(r#"{{"codeChallenge":"{CHALLENGE}"}}"#)
}

fn token_body() -> String {
    format!(r#"{{"code":"hexc_none","codeVerifier":"{VERIFIER}"}}"#)
}

/// Every consent route's answer to `origin` while the switches refuse it,
/// and whether any carried CORS.
fn every_route(port: u16, origin: &str) -> Vec<(String, String)> {
    let mut answers = Vec::new();
    for path in [connect::AUTHORIZE_PATH, record::PAIR_PATH] {
        answers.push((
            path.to_owned(),
            connect_post(port, origin, path, &ask_body()),
        ));
    }
    for path in [connect::TOKEN_PATH, record::TOKEN_PATH] {
        answers.push((
            path.to_owned(),
            connect_post(port, origin, path, &token_body()),
        ));
    }
    for path in [record::START_PATH, record::REVOKE_PATH] {
        let response = record_request(port, "POST", path, origin, Some("hexg_x"), None, None);
        answers.push((path.to_owned(), response));
    }
    for (method, path) in [
        ("GET", record::STATUS_PATH),
        ("POST", record::HEARTBEAT_PATH),
        ("POST", record::FINISH_PATH),
        ("POST", record::CANCEL_PATH),
    ] {
        let response = record_request(port, method, path, origin, None, Some("hexs_x"), None);
        answers.push((path.to_owned(), response));
    }
    answers.push((
        "/transcriptions".to_owned(),
        connect_post_with(
            port,
            origin,
            "/transcriptions",
            Some("hexa_x"),
            "audio/wav",
            "",
        ),
    ));
    answers
}

fn options(port: u16, origin: &str, path: &str) -> String {
    raw_request(
        port,
        &format!(
            "OPTIONS {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: {origin}\r\nAccess-Control-Request-Method: POST\r\nConnection: close\r\n\r\n"
        ),
    )
}

fn cors(response: &str) -> bool {
    response
        .to_ascii_lowercase()
        .contains("access-control-allow-origin")
}

const ASKING: &str = "https://app.example";
const UNLISTED: &str = "https://unlisted.example";

#[test]
fn off_every_consent_route_answers_as_it_did_without_the_lab() {
    let gate = ScriptedGate::new(Answer::Always);
    let (api, sessions, store) = start(Access::gated(off, on), gate.clone(), &app_origin());
    let port = api.document.port;
    for origin in [ASKING, UNLISTED] {
        for (path, response) in every_route(port, origin) {
            let asks = path == connect::AUTHORIZE_PATH
                || path == record::PAIR_PATH
                || path == connect::TOKEN_PATH
                || path == record::TOKEN_PATH;
            if asks {
                assert!(response.starts_with("HTTP/1.1 403"), "{path}: {response}");
                assert_eq!(body_code(&response), "origin-not-allowed", "{path}");
            } else {
                assert!(response.starts_with("HTTP/1.1 401"), "{path}: {response}");
            }
            assert!(!cors(&response), "{path}: {response}");
            assert!(!cors(&options(port, origin, &path)), "{path}");
        }
    }
    // A native app can't pair either, and nothing was listed or asked.
    let native = record::Channel::Native {
        peer: record::PeerId([1; 8]),
        identity: record::Identity {
            identifier: "com.example.recorder".into(),
            cdhash: "fixture-digest".into(),
            verification: crate::local_api::Verification::MacSignature { team: None },
            name: None,
            icon: None,
        },
    };
    let paired = sessions.pair(native, ask_body().as_bytes());
    assert_eq!(paired.status, 403);
    assert_eq!(gate.asks(), 0);
    assert!(store.websites().tried.is_empty());
}

#[test]
fn turning_the_lab_off_keeps_grants_but_lets_none_be_used() {
    static LAB: AtomicBool = AtomicBool::new(true);
    fn lab() -> bool {
        LAB.load(Ordering::SeqCst)
    }
    let gate = ScriptedGate::new(Answer::Always);
    let (api, sessions, _) = start(Access::gated(lab, on), gate, &app_origin());
    let port = api.document.port;
    let code = record_pair(port, ASKING);
    let (grant, _) = record_grant(port, ASKING, &code);
    let audio = authorize(port, ASKING);

    LAB.store(false, Ordering::SeqCst);
    let start = record_request(
        port,
        "POST",
        record::START_PATH,
        ASKING,
        Some(&grant),
        None,
        None,
    );
    assert!(start.starts_with("HTTP/1.1 401"), "{start}");
    let exchanged = exchange(port, ASKING, &audio);
    assert_eq!(body_code(&exchanged), "origin-not-allowed");
    assert_eq!(sessions.grants().len(), 1, "the grant is kept");
    assert_eq!(
        sessions.permissions().len(),
        2,
        "both saved permissions are kept"
    );

    LAB.store(true, Ordering::SeqCst);
    let (session, _, state) = record_start(port, ASKING, &grant);
    assert_eq!(state, "recording");
    let cancelled = record_request(
        port,
        "POST",
        record::CANCEL_PATH,
        ASKING,
        None,
        Some(&session),
        None,
    );
    assert!(cancelled.starts_with("HTTP/1.1 204"), "{cancelled}");
}

#[test]
fn websites_off_no_website_asks_is_listed_or_gets_cors() {
    let gate = ScriptedGate::new(Answer::Always);
    let (api, sessions, store) = start(Access::gated(on, off), gate.clone(), &app_origin());
    let port = api.document.port;
    for origin in [ASKING, UNLISTED] {
        for (path, response) in every_route(port, origin) {
            assert!(!cors(&response), "{path}: {response}");
            assert!(!cors(&options(port, origin, &path)), "{path}");
        }
    }
    assert_eq!(gate.asks(), 0);
    assert!(store.websites().tried.is_empty(), "nothing is recorded");
    // Apps still ask.
    let native = record::Channel::Native {
        peer: record::PeerId([2; 8]),
        identity: record::Identity {
            identifier: "com.example.recorder".into(),
            cdhash: "fixture-digest".into(),
            verification: crate::local_api::Verification::MacSignature { team: None },
            name: None,
            icon: None,
        },
    };
    assert_eq!(sessions.pair(native, ask_body().as_bytes()).status, 200);
    assert_eq!(gate.asks(), 1);
}

#[test]
fn an_unlisted_website_is_listed_once_and_never_asked() {
    let gate = ScriptedGate::new(Answer::Always);
    let (api, _, store) = start(connected(), gate.clone(), &app_origin());
    let port = api.document.port;
    for path in [
        connect::AUTHORIZE_PATH,
        record::PAIR_PATH,
        connect::AUTHORIZE_PATH,
    ] {
        let response = connect_post(port, UNLISTED, path, &ask_body());
        assert!(response.starts_with("HTTP/1.1 403"), "{response}");
        assert_eq!(body_code(&response), "website-not-allowed");
        assert!(response.contains(&format!("Access-Control-Allow-Origin: {UNLISTED}")));
    }
    // A token exchange from another unknown site is refused but not listed.
    let token = connect_post(
        port,
        "https://other.example",
        connect::TOKEN_PATH,
        &token_body(),
    );
    assert_eq!(body_code(&token), "website-not-allowed");
    assert_eq!(gate.asks(), 0);
    let tried = store.websites().tried;
    assert_eq!(tried.len(), 1, "{tried:?}");
    assert_eq!(tried[0].origin, UNLISTED);
    assert!(store.websites().allowed == app_origin());
}

#[test]
fn an_allowed_website_asks_on_its_next_request_and_still_needs_an_answer() {
    let gate = ScriptedGate::new(Answer::Deny);
    let (api, sessions, store) = start(connected(), gate.clone(), &[]);
    let port = api.document.port;
    let first = connect_post(port, UNLISTED, connect::AUTHORIZE_PATH, &ask_body());
    assert_eq!(body_code(&first), "website-not-allowed");
    assert!(sessions.website(UNLISTED, WebsiteAction::Allow));
    let websites = store.websites();
    assert!(websites.tried.is_empty());
    assert_eq!(websites.allowed, [UNLISTED]);
    assert_eq!(gate.asks(), 0, "Hex never asks after the fact");

    // Allowed to ask is not allowed: the window still decides, and Deny
    // grants nothing.
    let denied = connect_post(port, UNLISTED, connect::AUTHORIZE_PATH, &ask_body());
    assert_eq!(body_code(&denied), "consent-denied");
    let paired = connect_post(port, UNLISTED, record::PAIR_PATH, &ask_body());
    assert_eq!(body_code(&paired), "consent-denied");
    assert_eq!(gate.asks(), 2);
    assert!(sessions.permissions().is_empty());
}

#[test]
fn a_blocked_website_is_refused_and_never_listed_until_unblocked() {
    let gate = ScriptedGate::new(Answer::Always);
    let (api, sessions, store) = start(connected(), gate.clone(), &[]);
    let port = api.document.port;
    connect_post(port, UNLISTED, record::PAIR_PATH, &ask_body());
    assert!(sessions.website(UNLISTED, WebsiteAction::Block));
    for path in [connect::AUTHORIZE_PATH, record::PAIR_PATH] {
        let response = connect_post(port, UNLISTED, path, &ask_body());
        assert_eq!(body_code(&response), "website-blocked", "{path}");
    }
    let websites = store.websites();
    assert!(
        websites.tried.is_empty(),
        "a blocked site is never listed again"
    );
    assert_eq!(websites.blocked, [UNLISTED]);
    assert_eq!(gate.asks(), 0);

    // Unblocked, it is an unknown website again.
    assert!(sessions.website(UNLISTED, WebsiteAction::Forget));
    let again = connect_post(port, UNLISTED, connect::AUTHORIZE_PATH, &ask_body());
    assert_eq!(body_code(&again), "website-not-allowed");
    assert_eq!(store.websites().tried.len(), 1);
    assert!(store.websites().blocked.is_empty());
}

#[test]
fn consent_cors_answers_only_its_own_routes() {
    let gate = ScriptedGate::new(Answer::Always);
    let (api, _, _) = start(connected(), gate, &[]);
    let port = api.document.port;
    for path in [
        connect::AUTHORIZE_PATH,
        connect::TOKEN_PATH,
        record::PAIR_PATH,
        record::TOKEN_PATH,
        record::START_PATH,
        record::STATUS_PATH,
        record::HEARTBEAT_PATH,
        record::FINISH_PATH,
        record::CANCEL_PATH,
        record::REVOKE_PATH,
        "/transcriptions",
    ] {
        let answered = options(port, UNLISTED, path);
        assert!(answered.starts_with("HTTP/1.1 204"), "{path}: {answered}");
        assert!(answered.contains(&format!("Access-Control-Allow-Origin: {UNLISTED}\r\n")));
        assert!(answered.contains(
            "Access-Control-Allow-Headers: Content-Type, X-Hex-Connect-Token, X-Hex-Record-Grant, X-Hex-Record-Token\r\n"
        ));
        assert!(!answered.contains("Authorization"), "{path}: {answered}");
        assert!(!answered.contains("Allow-Credentials"));
        assert!(!answered.contains("Allow-Origin: *"));
    }
    for path in [
        "/control",
        "/dictations",
        "/streams",
        "/health",
        "/models",
        "/dev/control",
    ] {
        let refused = options(port, UNLISTED, path);
        assert!(!cors(&refused), "{path}: {refused}");
    }
    // An upload with the bearer token is the token API's, not a grant's.
    let bearer = raw_request(
        port,
        &format!(
            "POST /transcriptions HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: {UNLISTED}\r\nAuthorization: Bearer nope\r\nX-Hex-Connect-Token: hexa_x\r\nContent-Type: audio/wav\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ),
    );
    assert!(!cors(&bearer), "{bearer}");
    // Not an http(s) origin.
    assert!(!cors(&options(port, "null", connect::AUTHORIZE_PATH)));
}

#[test]
fn the_revoke_list_covers_websites_and_native_apps() {
    let gate = ScriptedGate::new(Answer::Always);
    let (_api, sessions, _) = start(connected(), gate, &app_origin());
    let native = record::Channel::Native {
        peer: record::PeerId([3; 8]),
        identity: record::Identity {
            identifier: "com.example.recorder".into(),
            cdhash: "fixture-digest".into(),
            verification: crate::local_api::Verification::MacSignature {
                team: Some("ABCDE12345".into()),
            },
            name: None,
            icon: None,
        },
    };
    for channel in [record::Channel::Web(ASKING.into()), native] {
        assert_eq!(sessions.pair(channel, ask_body().as_bytes()).status, 200);
    }
    let listed = sessions.permissions();
    assert_eq!(listed.len(), 2);
    assert!(
        listed
            .iter()
            .any(|view| matches!(view.recipient, Recipient::Web { .. }))
    );
    assert!(
        listed
            .iter()
            .any(|view| matches!(view.recipient, Recipient::Native { .. }))
    );
    for view in listed {
        assert!(sessions.revoke_permission(view.id));
    }
    assert!(sessions.permissions().is_empty());
}

#[test]
fn turning_off_withdraws_the_ask_and_cancels_the_recording() {
    static LAB: AtomicBool = AtomicBool::new(true);
    fn lab() -> bool {
        LAB.load(Ordering::SeqCst)
    }
    // The window's own desk: an ask waits until something ends it.
    let desk = Arc::new(crate::desktop::consent::Desk::default());
    let recording = FakeRecording::with_text("words");
    let store = Permissions::new(Some(temp_directory().join("permissions.json")));
    let sessions = Arc::new(record::Sessions::with_timings(
        Access::gated(lab, on),
        Some(desk.clone() as Arc<dyn ConsentGate>),
        recording.clone(),
        store,
        record_timings(),
    ));
    let native = record::Channel::Native {
        peer: record::PeerId([4; 8]),
        identity: record::Identity {
            identifier: "com.example.recorder".into(),
            cdhash: "fixture-digest".into(),
            verification: crate::local_api::Verification::MacSignature { team: None },
            name: None,
            icon: None,
        },
    };
    let asking = {
        let (sessions, native) = (sessions.clone(), native.clone());
        std::thread::spawn(move || sessions.pair(native, ask_body().as_bytes()))
    };
    wait_for(|| desk.prompt().is_some());
    // Still allowed: nothing ends.
    sessions.reconcile();
    assert!(desk.prompt().is_some());
    LAB.store(false, Ordering::SeqCst);
    sessions.reconcile();
    let answered = asking.join().unwrap();
    assert_eq!(answered.status, 403);
    assert!(desk.prompt().is_none());

    // A live recording ends too.
    LAB.store(true, Ordering::SeqCst);
    let pairing = {
        let (sessions, native) = (sessions.clone(), native.clone());
        std::thread::spawn(move || sessions.pair(native, ask_body().as_bytes()))
    };
    wait_for(|| desk.prompt().is_some());
    let id = desk.prompt().unwrap().id;
    desk.answer(id, Answer::Once);
    let code: String = serde_json::from_slice::<serde_json::Value>(&pairing.join().unwrap().body)
        .unwrap()["code"]
        .as_str()
        .unwrap()
        .to_owned();
    let minted: serde_json::Value = serde_json::from_slice(
        &sessions
            .exchange(
                native.clone(),
                format!(r#"{{"code":"{code}","codeVerifier":"{VERIFIER}"}}"#).as_bytes(),
            )
            .body,
    )
    .unwrap();
    let grant = minted["grantToken"].as_str().unwrap().to_owned();
    assert_eq!(sessions.start(&grant, &native).status, 201);
    assert!(sessions.view().is_some());
    LAB.store(false, Ordering::SeqCst);
    sessions.reconcile();
    assert_eq!(recording.cancels(), 1);
    assert!(sessions.view().is_none());
}

/// A window already showing the other route's ask answers busy on both, and
/// busy starts no cooldown: the page may ask again once it closes.
#[test]
fn an_ask_while_the_window_shows_another_is_busy() {
    let gate = ScriptedGate::new(Answer::Busy);
    let (api, _, _) = start(connected(), gate.clone(), &app_origin());
    let port = api.document.port;
    for path in [
        connect::AUTHORIZE_PATH,
        record::PAIR_PATH,
        connect::AUTHORIZE_PATH,
    ] {
        let response = connect_post(port, ASKING, path, &ask_body());
        assert!(response.starts_with("HTTP/1.1 409"), "{path}: {response}");
        assert_eq!(body_code(&response), "consent-busy");
    }
    assert_eq!(gate.asks(), 3);
}

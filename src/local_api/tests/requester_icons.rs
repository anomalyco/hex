//! Requester icons over the routes: a page's favicon reaches the window and
//! is kept for the Connected apps pane, an invalid one is ignored, and
//! neither name nor icon changes who a saved permission matches.

use super::*;
use crate::local_api::icon::tests::{data_url, png};

const PAGE: &str = "https://notes.example";

fn start(
    gate: Arc<dyn ConsentGate>,
    directory: &Path,
) -> (LocalApi, Arc<record::Sessions>, Arc<Permissions>) {
    let store = allowing(
        Permissions::new(Some(directory.join("permissions.json"))),
        &[PAGE.to_owned()],
    );
    let sessions = Arc::new(record::Sessions::with_timings(
        connected(),
        Some(gate.clone()),
        FakeRecording::with_text("words"),
        store.clone(),
        record_timings(),
    ));
    let api = start_unpublished(
        EventLog::create(&directory.join("events.ndjson")).unwrap(),
        LocalApiOptions {
            connect: Some(Connect::new(connected(), Some(gate), store.clone())),
            record: Some(sessions.clone()),
            stream_engine: Some(crate::transcription::stream::tests::fake_source()),
            ..LocalApiOptions::default()
        },
    )
    .unwrap();
    (api, sessions, store)
}

fn ask(icon: &str) -> String {
    serde_json::json!({ "codeChallenge": CHALLENGE, "icon": icon }).to_string()
}

fn red() -> String {
    data_url("image/png", &png(64, 64, [220, 40, 40, 255]))
}

#[test]
fn a_pages_favicon_reaches_the_window_on_both_asks() {
    let gate = ScriptedGate::new(Answer::Deny);
    let (api, _, _) = start(gate.clone(), &temp_directory());
    let port = api.document.port;
    for path in [connect::AUTHORIZE_PATH, record::PAIR_PATH] {
        let answer = connect_post(port, PAGE, path, &ask(&red()));
        assert_eq!(body_code(&answer), "consent-denied", "{path}");
        let prompt = gate.last_prompt().unwrap();
        let icon = prompt.recipient.icon().expect(path);
        assert_eq!(
            (icon.size(), &icon.pixels()[..4]),
            (64, &[220, 40, 40, 255][..])
        );
        // The address stays the identity.
        assert_eq!(prompt.recipient.display_name(), PAGE);
    }
}

#[test]
fn an_invalid_or_remote_favicon_is_ignored_and_the_ask_still_shows() {
    let gate = ScriptedGate::new(Answer::Timeout);
    let (api, _, _) = start(gate.clone(), &temp_directory());
    let port = api.document.port;
    for icon in [
        "https://notes.example/favicon.ico".to_owned(),
        data_url("image/png", b"not a png"),
        data_url("image/svg+xml", b"<svg/>"),
    ] {
        let answer = connect_post(port, PAGE, connect::AUTHORIZE_PATH, &ask(&icon));
        assert_eq!(body_code(&answer), "consent-timeout");
        assert_eq!(gate.last_prompt().unwrap().recipient.icon(), None);
    }
    // Past the body's bound, the request itself is refused.
    let refused = raw_request(
        port,
        &format!(
            "POST {} HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: {PAGE}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            connect::AUTHORIZE_PATH,
            connect::MAX_BODY_BYTES + 1
        ),
    );
    assert!(refused.starts_with("HTTP/1.1 413"), "{refused}");
}

#[test]
fn always_allow_keeps_the_icon_across_a_restart_and_matching_ignores_it() {
    let directory = temp_directory();
    let gate = ScriptedGate::new(Answer::Always);
    let (api, _, store) = start(gate.clone(), &directory);
    let port = api.document.port;
    let allowed = consent_json(&connect_post(
        port,
        PAGE,
        connect::AUTHORIZE_PATH,
        &ask(&red()),
    ));
    let proof = allowed["permissionProof"].as_str().unwrap().to_owned();

    let reloaded = Permissions::new(Some(directory.join("permissions.json")));
    let saved = reloaded.list();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].recipient.icon().map(|icon| icon.size()), Some(64));
    assert_eq!(
        reloaded.websites().icon(PAGE).map(|icon| icon.size()),
        Some(64),
        "the allowed website keeps its favicon too"
    );

    // Another icon, or none, still matches the saved permission (no
    // dialog), and the newest icon is kept.
    let blue = data_url("image/png", &png(32, 32, [30, 60, 220, 255]));
    let again = serde_json::json!({
        "codeChallenge": CHALLENGE, "icon": blue, "permissionProof": proof,
    });
    let answer = connect_post(port, PAGE, connect::AUTHORIZE_PATH, &again.to_string());
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    assert_eq!(gate.asks(), 1);
    let kept = store.list();
    assert_eq!(kept[0].recipient.icon().map(|icon| icon.size()), Some(32));
}

#[test]
fn forgetting_a_website_forgets_its_favicon() {
    let directory = temp_directory();
    let gate = ScriptedGate::new(Answer::Deny);
    let (api, _, store) = start(gate, &directory);
    connect_post(
        api.document.port,
        PAGE,
        connect::AUTHORIZE_PATH,
        &ask(&red()),
    );
    assert!(store.websites().icon(PAGE).is_some());
    assert!(store.website_action(PAGE, WebsiteAction::Block));
    assert!(
        store.websites().icon(PAGE).is_some(),
        "a blocked site keeps it"
    );
    assert!(store.website_action(PAGE, WebsiteAction::Forget));
    assert!(store.websites().icon(PAGE).is_none());
    let reloaded = Permissions::new(Some(directory.join("permissions.json")));
    assert!(reloaded.websites().icons.is_empty());
}

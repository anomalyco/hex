//! The Slack adapter on hand-written trees, shaped like the ones Slack's
//! huddle page gives: nobody here is a real person, and no id is a real
//! Slack id.

use super::*;
use crate::meeting::speakers::evidence::Evidence;
use crate::meeting::speakers::reader::PollState;
use crate::meeting::speakers::registry::{CallApp, adapter_for};

const OVERLAY: &str = "p-huddle_peer_tile__overlay";
const MIC_OVERLAY: &str = "p-huddle_peer_tile__mic_overlay";

fn node(role: &str, classes: &[&str], children: Vec<Node>) -> Node {
    Node {
        role: Some(role.into()),
        dom_classes: classes.iter().map(|class| (*class).into()).collect(),
        children,
        ..Default::default()
    }
}

fn text(value: &str) -> Node {
    Node {
        value: Some(value.into()),
        ..node("AXStaticText", &[], Vec::new())
    }
}

/// The two overlays of a tile, each marked while its person speaks. The
/// name overlay holds `shown` when Slack draws a name there.
fn overlays(shown: Option<&str>, speaking: bool) -> Vec<Node> {
    let marked = |part: &'static str| {
        let mut classes = vec![OVERLAY, part];
        classes.extend(speaking.then_some(SPEAKING));
        classes
    };
    vec![
        node(
            "AXGroup",
            &marked(MIC_OVERLAY),
            vec![node("AXImage", &[], Vec::new())],
        ),
        node(
            "AXGroup",
            &marked(NAME_OVERLAY),
            shown.map(text).into_iter().collect(),
        ),
    ]
}

/// A remote person's tile, on the connection `peer`.
fn tile(peer: &str, user: &str, shown: &str, speaking: bool) -> Node {
    Node {
        dom_id: Some(format!("{TILE_ID}{peer}_{user}")),
        title: Some(format!("View {shown}'s profile")),
        ..node("AXCell", &[TILE], overlays(Some(shown), speaking))
    }
}

/// The user's own tile, which has no name in its overlay.
fn own_tile(user: &str, shown: &str, speaking: bool) -> Node {
    Node {
        dom_id: Some(format!("{OWN_TILE_ID}{user}")),
        title: Some(format!("View {shown}'s profile")),
        ..node("AXCell", &[TILE], overlays(None, speaking))
    }
}

/// A Slack window with `content` somewhere inside its page.
fn window(content: Vec<Node>) -> Node {
    let page = node(
        "AXWebArea",
        &[],
        vec![
            node("AXGroup", &["p-client_container"], vec![text("general")]),
            node("AXGroup", &["p-huddle_grid"], content),
        ],
    );
    node("AXWindow", &[], vec![node("AXGroup", &[], vec![page])])
}

fn snapshot(windows: Vec<Node>) -> Snapshot {
    Snapshot {
        windows,
        ..Default::default()
    }
}

fn id(shown: &str) -> AppParticipantId {
    AppParticipantId::parse(shown).unwrap()
}

fn participant(user: &str, name: &str, is_local: bool) -> ObservedParticipant {
    ObservedParticipant {
        id: id(user),
        name: SpeakerName::parse(name),
        is_local,
    }
}

/// Carol's own view of a huddle with Alice and Bob.
fn a_huddle(alice_speaking: bool, bob_speaking: bool, carol_speaking: bool) -> Node {
    window(vec![
        own_tile("U0001SELF", "Carol", carol_speaking),
        tile("peer-1", "U0002ALICE", "Alice", alice_speaking),
        tile("peer-2", "U0003BOB", "Bob", bob_speaking),
    ])
}

fn everyone() -> Vec<ObservedParticipant> {
    vec![
        participant("U0001SELF", "Carol", true),
        participant("U0002ALICE", "Alice", false),
        participant("U0003BOB", "Bob", false),
    ]
}

fn floor(observation: &Observation) -> Option<AppParticipantId> {
    match observation {
        Observation::Active { floor, .. } => floor.clone(),
        other => panic!("no huddle was read: {other:?}"),
    }
}

fn local_speaking(observation: &Observation) -> bool {
    match observation {
        Observation::Active { local_speaking, .. } => *local_speaking,
        other => panic!("no huddle was read: {other:?}"),
    }
}

fn roster(observation: Observation) -> Vec<ObservedParticipant> {
    match observation {
        Observation::Active { roster, .. } => roster,
        other => panic!("no huddle was read: {other:?}"),
    }
}

#[test]
fn a_huddle_is_read_as_everyone_on_it_and_the_one_person_speaking() {
    let seen = observe(&snapshot(vec![a_huddle(true, false, false)]));
    assert_eq!(
        seen,
        Observation::Active {
            roster: everyone(),
            floor: Some(id("U0002ALICE")),
            local_speaking: false,
        }
    );
    // The user's own tile takes the floor when nobody remote is speaking.
    let own = observe(&snapshot(vec![a_huddle(false, false, true)]));
    assert_eq!(floor(&own), Some(id("U0001SELF")));
    assert!(local_speaking(&own));
}

#[test]
fn nobody_holds_the_floor_when_nobody_or_several_remote_people_are_marked() {
    let quiet = observe(&snapshot(vec![a_huddle(false, false, false)]));
    assert_eq!(floor(&quiet), None);
    assert!(!local_speaking(&quiet));
    assert_eq!(roster(quiet), everyone());

    for (alice, bob, carol) in [(true, true, false), (true, true, true)] {
        let overlap = observe(&snapshot(vec![a_huddle(alice, bob, carol)]));
        assert_eq!(floor(&overlap), None, "{alice} {bob} {carol}");
        assert_eq!(local_speaking(&overlap), carol, "{alice} {bob} {carol}");
        assert_eq!(roster(overlap), everyone());
    }
}

#[test]
fn the_sole_remote_speaker_holds_the_floor_when_the_user_speaks_too() {
    // Local + 1 remote: that remote participant is the only remote speaker on
    // system audio, while `local_speaking` keeps the user's tile visible.
    let alice_and_carol = observe(&snapshot(vec![a_huddle(true, false, true)]));
    assert_eq!(
        alice_and_carol,
        Observation::Active {
            roster: everyone(),
            floor: Some(id("U0002ALICE")),
            local_speaking: true,
        }
    );
    let bob_and_carol = observe(&snapshot(vec![a_huddle(false, true, true)]));
    assert_eq!(floor(&bob_and_carol), Some(id("U0003BOB")));
    assert!(local_speaking(&bob_and_carol));

    // Duplicate tiles of the user (one without the `self_` prefix marked, one
    // with it unmarked) still merge `is_local` before partitioning speakers.
    let merged = observe(&snapshot(vec![window(vec![
        tile("peer-3", "U0001SELF", "Carol", true),
        own_tile("U0001SELF", "Carol", false),
        tile("peer-1", "U0002ALICE", "Alice", true),
    ])]));
    assert_eq!(floor(&merged), Some(id("U0002ALICE")));
    assert!(local_speaking(&merged));

    // An un-ID'd marked tile still clears `floor` even when the user's own
    // tile is marked too.
    let unknown = Node {
        dom_id: None,
        ..tile("peer-9", "U0009ANON", "Anon", true)
    };
    let with_unknown = observe(&snapshot(vec![window(vec![
        own_tile("U0001SELF", "Carol", true),
        tile("peer-1", "U0002ALICE", "Alice", true),
        unknown,
    ])]));
    assert_eq!(floor(&with_unknown), None);
    assert!(local_speaking(&with_unknown));

    // Through the reader: simultaneous local + remote and local + two remotes
    // save `local_speaking: true` without ever writing a Slack id.
    let mut reader = PollState::continuing(&[]);
    let mut saved = Vec::new();
    for (at_ms, alice, bob, carol) in [
        (0, false, false, true),
        (500, true, false, true),
        (1_000, true, true, true),
        (1_500, true, false, false),
    ] {
        let seen = Observation::of(&snapshot(vec![a_huddle(alice, bob, carol)]), &ADAPTER);
        saved.extend(reader.observe(at_ms, seen).evidence);
    }
    let floor_lines: Vec<String> = saved
        .iter()
        .filter(|line| matches!(line, Evidence::Floor { .. }))
        .map(|line| serde_json::to_string(line).unwrap())
        .collect();
    assert_eq!(
        floor_lines,
        [
            r#"{"kind":"floor","at_ms":0,"holder":"p1","local_speaking":true}"#,
            r#"{"kind":"floor","at_ms":500,"holder":"p2","local_speaking":true}"#,
            r#"{"kind":"floor","at_ms":1000,"holder":null,"local_speaking":true}"#,
            r#"{"kind":"floor","at_ms":1500,"holder":"p2"}"#,
        ]
    );
}

#[test]
fn the_mark_counts_on_either_overlay_and_only_inside_a_tile() {
    for part in [MIC_OVERLAY, NAME_OVERLAY] {
        let mut alice = tile("peer-1", "U0002ALICE", "Alice", false);
        let overlay = alice
            .children
            .iter_mut()
            .find(|overlay| overlay.dom_classes.iter().any(|class| class == part))
            .unwrap();
        overlay.dom_classes.push(SPEAKING.into());
        let seen = observe(&snapshot(vec![window(vec![
            own_tile("U0001SELF", "Carol", false),
            alice,
        ])]));
        assert_eq!(floor(&seen), Some(id("U0002ALICE")), "{part}");
    }

    // The same class on something that is no part of a tile marks nobody.
    let stray = node("AXGroup", &[OVERLAY, SPEAKING], Vec::new());
    let seen = observe(&snapshot(vec![window(vec![
        stray,
        tile("peer-1", "U0002ALICE", "Alice", false),
    ])]));
    assert_eq!(floor(&seen), None);
}

#[test]
fn someone_who_reconnects_is_still_the_same_person() {
    let before = roster(observe(&snapshot(vec![window(vec![tile(
        "peer-1",
        "U0002ALICE",
        "Alice",
        false,
    )])])));
    let after = roster(observe(&snapshot(vec![window(vec![tile(
        "peer-7",
        "U0002ALICE",
        "Alice",
        false,
    )])])));
    assert_eq!(before, after);

    // And two tiles of one person at one look are one person, who speaks
    // if either tile is marked and is the user if either tile is theirs.
    let seen = observe(&snapshot(vec![window(vec![
        tile("peer-1", "U0002ALICE", "Alice", false),
        tile("peer-2", "U0002ALICE", "Alice", true),
        tile("peer-3", "U0001SELF", "Carol", false),
        own_tile("U0001SELF", "Carol", false),
    ])]));
    assert_eq!(floor(&seen), Some(id("U0002ALICE")));
    assert_eq!(
        roster(seen),
        [
            participant("U0002ALICE", "Alice", false),
            participant("U0001SELF", "Carol", true),
        ]
    );
}

#[test]
fn a_tile_is_told_apart_by_whatever_its_id_gives() {
    let with_id = |dom_id: Option<&str>, speaking: bool| Node {
        dom_id: dom_id.map(str::to_owned),
        ..tile("peer-1", "U0002ALICE", "Alice", speaking)
    };
    // An id of another shape is taken whole.
    for (dom_id, whole) in [
        ("some-other-tile", "some-other-tile"),
        ("huddle-grid-gridcell-peer", "huddle-grid-gridcell-peer"),
        ("huddle-grid-gridcell-peer_", "huddle-grid-gridcell-peer_"),
        ("huddle-grid-gridcell-a_b_U0002ALICE", "U0002ALICE"),
    ] {
        let seen = observe(&snapshot(vec![window(vec![with_id(Some(dom_id), true)])]));
        assert_eq!(floor(&seen), Some(id(whole)), "{dom_id}");
        assert_eq!(roster(seen), [participant(whole, "Alice", false)]);
    }

    // A tile with no id is nobody Hex can tell apart. It is left out, and
    // while it is marked nobody else holds the floor either.
    for blank in [None, Some(""), Some("  ")] {
        let seen = observe(&snapshot(vec![window(vec![
            with_id(blank, true),
            tile("peer-2", "U0003BOB", "Bob", true),
        ])]));
        assert_eq!(floor(&seen), None, "{blank:?}");
        assert_eq!(roster(seen), [participant("U0003BOB", "Bob", false)]);

        let unmarked = observe(&snapshot(vec![window(vec![
            with_id(blank, false),
            tile("peer-2", "U0003BOB", "Bob", true),
        ])]));
        assert_eq!(floor(&unmarked), Some(id("U0003BOB")), "{blank:?}");
    }
}

#[test]
fn a_name_is_the_overlays_text_or_else_the_one_in_the_tiles_label() {
    let named = |tile: Node| {
        let people = roster(observe(&snapshot(vec![window(vec![tile])])));
        people[0].name.as_ref().map(|name| name.as_str().to_owned())
    };
    let bare = |title: Option<&str>, description: Option<&str>| Node {
        title: title.map(str::to_owned),
        description: description.map(str::to_owned),
        ..own_tile("U0001SELF", "Carol", false)
    };

    // The overlay's text wins over the label.
    let relabelled = Node {
        title: Some("View Somebody Else's profile".into()),
        ..tile("peer-1", "U0002ALICE", "Alice", false)
    };
    assert_eq!(named(relabelled).as_deref(), Some("Alice"));

    // The user's own tile has only its label, as a title or a description,
    // with either apostrophe.
    assert_eq!(
        named(own_tile("U0001SELF", "Carol", false)).as_deref(),
        Some("Carol")
    );
    assert_eq!(
        named(bare(None, Some("View Carol\u{2019}s profile"))).as_deref(),
        Some("Carol")
    );
    assert_eq!(
        named(bare(Some("Huddle tile"), Some("View Carol's profile"))).as_deref(),
        Some("Carol")
    );

    // A label of any other shape is not a name, and neither is no label.
    for label in [
        "Carol",
        "View profile",
        "Carol's profile",
        "View 's profile",
        "",
    ] {
        assert_eq!(named(bare(Some(label), None)), None, "{label}");
    }
    assert_eq!(named(bare(None, None)), None);

    // An overlay with nothing usable in it falls back to the label, and a
    // name is cleaned like any other.
    let mut blank = tile("peer-1", "U0002ALICE", "Alice", false);
    blank.children[1].children = vec![text("  "), text("You")];
    assert_eq!(named(blank).as_deref(), Some("Alice"));
    let mut hostile = tile("peer-1", "U0002ALICE", "Alice", false);
    hostile.children[1].children = vec![text(" **Alice**\n<b>A.</b> ")];
    assert_eq!(named(hostile).as_deref(), Some("Alice bA./b"));

    // Someone with no usable name anywhere is still on the huddle.
    let mut nameless = bare(None, None);
    nameless.children[1].children = vec![text("Them")];
    let people = roster(observe(&snapshot(vec![window(vec![nameless])])));
    assert_eq!(people, [participant("U0001SELF", "", true)]);
}

#[test]
fn only_a_grid_cell_with_the_tile_class_is_a_tile() {
    let alice = tile("peer-1", "U0002ALICE", "Alice", true);
    // A tile's own parts carry classes that only start like the tile's.
    let not_tiles = vec![
        Node {
            role: Some("AXGroup".into()),
            ..alice.clone()
        },
        Node {
            dom_classes: vec![NAME_OVERLAY.into(), "p-huddle_peer_tile--large".into()],
            ..alice.clone()
        },
        Node {
            dom_classes: Vec::new(),
            ..alice.clone()
        },
    ];
    assert_eq!(
        observe(&snapshot(vec![window(not_tiles)])),
        Observation::NoCallWindow
    );
    // With other classes beside the tile's, it is a tile.
    let styled = Node {
        dom_classes: vec![
            "c-grid_cell".into(),
            TILE.into(),
            "p-huddle_peer_tile--large".into(),
        ],
        ..alice
    };
    let seen = observe(&snapshot(vec![window(vec![styled])]));
    assert_eq!(floor(&seen), Some(id("U0002ALICE")));
}

#[test]
fn slack_with_no_huddle_on_screen_has_no_call_window() {
    let channel = window(vec![text("general"), node("AXButton", &[], Vec::new())]);
    for windows in [
        vec![channel.clone()],
        vec![channel.clone(), channel.clone()],
        vec![node("AXWindow", &[], Vec::new())],
    ] {
        assert_eq!(observe(&snapshot(windows)), Observation::NoCallWindow);
    }
}

#[test]
fn the_huddle_is_read_wherever_its_one_window_is_and_however_deep() {
    let channel = window(vec![text("general")]);
    // The compact view: a few tiles deep inside the main window's sidebar.
    let compact = window(vec![node(
        "AXGroup",
        &["p-flexpane"],
        vec![node(
            "AXGroup",
            &["p-huddle_sidebar"],
            vec![
                own_tile("U0001SELF", "Carol", false),
                tile("peer-1", "U0002ALICE", "Alice", true),
            ],
        )],
    )]);
    for windows in [
        vec![compact.clone()],
        vec![channel.clone(), compact.clone()],
        vec![compact, channel],
    ] {
        let seen = observe(&snapshot(windows));
        assert_eq!(floor(&seen), Some(id("U0002ALICE")));
        assert_eq!(roster(seen).len(), 2);
    }
}

#[test]
fn two_windows_with_tiles_are_not_believed() {
    let seen = observe(&snapshot(vec![
        a_huddle(true, false, false),
        window(vec![text("general")]),
        a_huddle(true, false, false),
    ]));
    assert_eq!(seen, Observation::Untrusted(Untrusted::TwoWindows));
}

#[test]
fn slack_is_read_by_this_adapter_under_its_bundle_id() {
    let adapter = adapter_for(CallApp::Bundle("com.tinyspeck.slackmacgap")).unwrap();
    let huddle = snapshot(vec![a_huddle(false, true, false)]);
    assert_eq!(adapter.observe(&huddle), observe(&huddle));
    assert_eq!(floor(&adapter.observe(&huddle)), Some(id("U0003BOB")));
    // Through the seam every adapter is asked through, as the reader asks.
    assert_eq!(Observation::of(&huddle, adapter), observe(&huddle));
    assert!(adapter_for(CallApp::Site("com.tinyspeck.slackmacgap")).is_none());
}

#[test]
fn what_the_reader_saves_of_a_huddle_holds_names_and_never_a_slack_id() {
    let mut reader = PollState::continuing(&[]);
    let mut saved = Vec::new();
    for (at_ms, alice, bob) in [(0, false, false), (500, true, false), (1_000, false, true)] {
        let seen = Observation::of(&snapshot(vec![a_huddle(alice, bob, false)]), &ADAPTER);
        // Nothing printed of a look holds a name or an id either.
        let printed = format!("{seen:?}");
        assert!(
            !printed.contains("U000") && !printed.contains("Alice"),
            "{printed}"
        );
        saved.extend(reader.observe(at_ms, seen).evidence);
    }
    let lines = saved
        .iter()
        .map(|line| serde_json::to_string(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        lines,
        [
            r#"{"kind":"participant","key":"p1","name":"Carol","is_local":true,"at_ms":0}"#,
            r#"{"kind":"participant","key":"p2","name":"Alice","is_local":false,"at_ms":0}"#,
            r#"{"kind":"participant","key":"p3","name":"Bob","is_local":false,"at_ms":0}"#,
            r#"{"kind":"floor","at_ms":0,"holder":null}"#,
            r#"{"kind":"floor","at_ms":500,"holder":"p2"}"#,
            r#"{"kind":"floor","at_ms":1000,"holder":"p3"}"#,
        ]
    );
    assert!(
        saved
            .iter()
            .all(|line| !matches!(line, Evidence::Untrusted { .. }))
    );
}

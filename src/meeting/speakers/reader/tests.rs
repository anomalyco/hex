//! The reader against a call app invented for these tests, read from
//! snapshots written by hand: no app, no wait and no disk but one test's
//! own folder.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex};

use color_eyre::eyre::eyre;

use super::*;
use crate::accessibility::tree::{Cut, Node};
use crate::meeting::speakers::evidence;

// MARK: The invented call app

/// The app's ids for four people, shaped like a real app's. None of them
/// may reach a file or a log.
const KIT: &str = "U0AAAAAAA01";
const PRIYA: &str = "U0BBBBBBB02";
const MARCUS: &str = "U0CCCCCCC03";
const SAM: &str = "U0DDDDDDD04";
const APP_IDS: [&str; 4] = [KIT, PRIYA, MARCUS, SAM];

/// One person's tile in the invented app's call window.
#[derive(Clone, Copy)]
struct Tile {
    id: &'static str,
    shown: &'static str,
    is_local: bool,
    speaking: bool,
}

fn tile(id: &'static str, shown: &'static str) -> Tile {
    Tile {
        id,
        shown,
        is_local: false,
        speaking: false,
    }
}

impl Tile {
    fn local(self) -> Self {
        Self {
            is_local: true,
            ..self
        }
    }

    fn speaking(self) -> Self {
        Self {
            speaking: true,
            ..self
        }
    }

    fn node(self) -> Node {
        let mut dom_classes = vec!["tile".to_owned()];
        if self.is_local {
            dom_classes.push("tile--self".into());
        }
        if self.speaking {
            dom_classes.push("tile--speaking".into());
        }
        Node {
            role: Some("AXCell".into()),
            dom_id: Some(format!("tile-{}", self.id)),
            description: Some(self.shown.into()),
            dom_classes,
            ..Default::default()
        }
    }
}

fn window(title: &str, children: Vec<Node>) -> Node {
    Node {
        role: Some("AXWindow".into()),
        title: Some(title.into()),
        children: vec![Node {
            role: Some("AXWebArea".into()),
            children,
            ..Default::default()
        }],
        ..Default::default()
    }
}

/// The app's call window: a page holding a grid of tiles.
fn call_window(tiles: &[Tile]) -> Node {
    let grid = Node {
        role: Some("AXGroup".into()),
        dom_classes: vec!["grid".into()],
        children: tiles.iter().map(|tile| tile.node()).collect(),
        ..Default::default()
    };
    window("Call", vec![grid])
}

fn showing(windows: Vec<Node>) -> Snapshot {
    Snapshot {
        windows,
        ..Default::default()
    }
}

/// The app with its chat window and a call under way.
fn call(tiles: &[Tile]) -> Snapshot {
    showing(vec![window("Chat", Vec::new()), call_window(tiles)])
}

/// The app with no call: its chat window alone.
fn no_call() -> Snapshot {
    showing(vec![window("Chat", Vec::new())])
}

/// The call window minimised: its page is there and draws nothing.
fn hidden_call() -> Snapshot {
    showing(vec![window("Call", Vec::new())])
}

/// A call popped out into a second window beside the first.
fn two_calls(first: &[Tile], second: &[Tile]) -> Snapshot {
    showing(vec![call_window(first), call_window(second)])
}

/// A read of a call that ran out of nodes part of the way through its tiles.
fn cut_short(tiles: &[Tile]) -> Snapshot {
    Snapshot {
        cut_short: vec![Cut::Nodes],
        ..call(tiles)
    }
}

/// The invented app's adapter, written as a real one will be: a pure
/// function of the snapshot. The call window is the one titled `Call`, a
/// tile's DOM id ends in the app's id for the person, its description is
/// their name, and its classes mark the user and the speaker.
fn invented_app(snapshot: &Snapshot) -> Observation {
    let mut calls = snapshot
        .windows
        .iter()
        .filter(|window| window.title.as_deref() == Some("Call"));
    let Some(call) = calls.next() else {
        return Observation::NoCallWindow;
    };
    if calls.next().is_some() {
        return Observation::Untrusted(Untrusted::TwoWindows);
    }
    let page = call.children.first();
    let grid = page.and_then(|page| page.children.iter().find(|node| is(node, "grid")));
    let Some(grid) = grid else {
        return Observation::Untrusted(Untrusted::WindowHidden);
    };
    let mut roster = Vec::new();
    let mut floor = None;
    for tile in &grid.children {
        let id = tile
            .dom_id
            .as_deref()
            .and_then(|id| id.strip_prefix("tile-"));
        let Some(id) = id.and_then(AppParticipantId::parse) else {
            continue;
        };
        if is(tile, "tile--speaking") {
            floor = Some(id.clone());
        }
        roster.push(ObservedParticipant {
            id,
            name: tile.description.as_deref().and_then(SpeakerName::parse),
            is_local: is(tile, "tile--self"),
        });
    }
    Observation::Active {
        roster,
        floor,
        local_speaking: false,
    }
}

fn is(node: &Node, class: &str) -> bool {
    node.dom_classes.iter().any(|known| known == class)
}

// MARK: A recording that is not one

/// A recording's clock that moves only when the reader waits on it or a
/// read of the app takes time, and stops at a set moment.
struct Recording<'a> {
    now_ms: &'a Cell<u64>,
    stops_at_ms: u64,
}

impl CaptureClock for Recording<'_> {
    fn now_ms(&self) -> u64 {
        self.now_ms.get()
    }

    fn wait(&mut self, ms: u64) -> Wait {
        let woken_ms = self.now_ms.get() + ms;
        if woken_ms >= self.stops_at_ms {
            self.now_ms.set(self.stops_at_ms);
            Wait::Stopped
        } else {
            self.now_ms.set(woken_ms);
            Wait::Elapsed
        }
    }
}

/// What a reader did over a recording.
struct Read {
    saved: Vec<Evidence>,
    tally: Tally,
    /// When each read of the app started.
    looked_at_ms: Vec<u64>,
}

/// Runs a reader over a recording of `stops_at_ms`. Each look reads the
/// next of `looks`, a snapshot and how long the app took to give it, and
/// `save` is the sink. A look past the end of `looks` fails the test.
fn read_into(
    state: PollState,
    looks: Vec<(Snapshot, u64)>,
    stops_at_ms: u64,
    mut save: impl FnMut(&[Evidence]) -> Result<()>,
) -> (Tally, Vec<u64>) {
    let now_ms = Cell::new(0);
    let mut looks = VecDeque::from(looks);
    let mut looked_at_ms = Vec::new();
    let tally = run(
        state,
        &invented_app,
        || {
            looked_at_ms.push(now_ms.get());
            let (snapshot, took_ms) = looks.pop_front().expect("no look after the last");
            now_ms.set(now_ms.get() + took_ms);
            snapshot
        },
        &mut Recording {
            now_ms: &now_ms,
            stops_at_ms,
        },
        &mut save,
    );
    assert!(looks.is_empty(), "{} looks were never taken", looks.len());
    (tally, looked_at_ms)
}

/// A new meeting read at the reader's own pace, by an app that answers at
/// once, for exactly as long as there are snapshots.
fn read(snapshots: Vec<Snapshot>) -> Read {
    let stops_at_ms = snapshots.len() as u64 * POLL_MS;
    let looks = snapshots
        .into_iter()
        .map(|snapshot| (snapshot, 0))
        .collect();
    let mut saved = Vec::new();
    let (tally, looked_at_ms) =
        read_into(PollState::continuing(&[]), looks, stops_at_ms, |lines| {
            saved.extend_from_slice(lines);
            Ok(())
        });
    Read {
        saved,
        tally,
        looked_at_ms,
    }
}

fn key(number: u32) -> ParticipantKey {
    ParticipantKey::numbered(number)
}

fn person(number: u32, name: &str, is_local: bool, at_ms: u64) -> Evidence {
    Evidence::Participant(Participant {
        key: key(number),
        name: SpeakerName::parse(name),
        is_local,
        at_ms,
    })
}

fn floor(at_ms: u64, holder: Option<u32>) -> Evidence {
    Evidence::Floor {
        at_ms,
        holder: holder.map(key),
        local_speaking: false,
    }
}

fn untrusted(at_ms: u64, reason: Untrusted) -> Evidence {
    Evidence::Untrusted { at_ms, reason }
}

fn folder(name: &str) -> std::path::PathBuf {
    let folder =
        std::env::temp_dir().join(format!("hex-speaker-reader-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

// MARK: Who is on the call

#[test]
fn people_are_written_once_under_keys_in_the_order_they_are_first_seen() {
    let kit = tile(KIT, "Kit").local();
    let priya = tile(PRIYA, "Priya Nair");
    let marcus = tile(MARCUS, "Marcus Webb");
    let read = read(vec![
        call(&[kit, priya]),
        call(&[kit, priya]),
        // Marcus joins, and the app draws him first.
        call(&[marcus, kit, priya]),
        // Priya leaves and comes back: she is who she was.
        call(&[marcus, kit]),
        call(&[kit, priya, marcus]),
    ]);
    assert_eq!(
        read.saved,
        [
            person(1, "Kit", true, 0),
            person(2, "Priya Nair", false, 0),
            floor(0, None),
            floor(500, None),
            person(3, "Marcus Webb", false, 1_000),
            floor(1_000, None),
            floor(1_500, None),
            floor(2_000, None),
        ]
    );
    assert_eq!(
        read.tally,
        Tally {
            floor: 5,
            people: 3,
            ..Default::default()
        }
    );
}

#[test]
fn someone_the_app_comes_to_show_differently_is_written_again_under_their_key() {
    let kit = tile(KIT, "Kit").local();
    let read = read(vec![
        // Sam's tile is drawn before the app has his name.
        call(&[kit, tile(PRIYA, "Priya"), tile(SAM, "")]),
        call(&[kit, tile(PRIYA, "Priya"), tile(SAM, "Sam Ortiz")]),
        // Priya renames herself, to a name Hex cleans...
        call(&[kit, tile(PRIYA, "**Priya Nair**"), tile(SAM, "Sam Ortiz")]),
        // ...and the same name drawn another way is no change at all.
        call(&[kit, tile(PRIYA, " Priya  Nair "), tile(SAM, "Sam Ortiz")]),
        // A name Hex cannot use is no name.
        call(&[kit, tile(PRIYA, "You"), tile(SAM, "Sam Ortiz")]),
        // The app works out late which tile is the user's.
        call(&[kit, tile(PRIYA, "You"), tile(SAM, "Sam Ortiz").local()]),
    ]);
    assert_eq!(
        read.saved,
        [
            person(1, "Kit", true, 0),
            person(2, "Priya", false, 0),
            person(3, "", false, 0),
            floor(0, None),
            person(3, "Sam Ortiz", false, 500),
            floor(500, None),
            person(2, "Priya Nair", false, 1_000),
            floor(1_000, None),
            floor(1_500, None),
            person(2, "", false, 2_000),
            floor(2_000, None),
            person(3, "Sam Ortiz", true, 2_500),
            floor(2_500, None),
        ]
    );
    assert_eq!(read.tally.people, 3);
}

#[test]
fn two_tiles_of_one_person_are_one_person_and_the_first_stands() {
    let kit = tile(KIT, "Kit").local();
    // Priya and the screen she is sharing, under her one id.
    let sharing = [
        kit,
        tile(PRIYA, "Priya Nair"),
        tile(PRIYA, "Priya's screen"),
    ];
    let read = read(vec![call(&sharing), call(&sharing)]);
    assert_eq!(
        read.saved,
        [
            person(1, "Kit", true, 0),
            person(2, "Priya Nair", false, 0),
            floor(0, None),
            floor(500, None),
        ]
    );
}

#[test]
fn a_reader_starting_again_never_gives_out_a_key_the_meeting_has() {
    let before = [
        person(1, "Kit", true, 0),
        person(2, "Priya Nair", false, 0),
        floor(0, Some(2)),
    ];
    let mut state = PollState::continuing(&before);
    // It has forgotten which app id was whose, so it cannot say Priya is
    // p2. It can make sure nobody else ever is.
    let step = state.observe(
        60_000,
        invented_app(&call(&[
            tile(PRIYA, "Priya Nair").speaking(),
            tile(MARCUS, "Marcus Webb"),
        ])),
    );
    assert_eq!(
        step.evidence,
        [
            person(3, "Priya Nair", false, 60_000),
            person(4, "Marcus Webb", false, 60_000),
            floor(60_000, Some(3)),
        ]
    );
}

// MARK: Who holds the floor

#[test]
fn the_floor_is_written_at_every_look_as_it_passes_from_one_to_another() {
    let kit = tile(KIT, "Kit").local();
    let priya = tile(PRIYA, "Priya Nair");
    let marcus = tile(MARCUS, "Marcus Webb");
    let read = read(vec![
        call(&[kit, priya.speaking(), marcus]),
        call(&[kit, priya.speaking(), marcus]),
        call(&[kit, priya, marcus.speaking()]),
        call(&[kit, priya, marcus]),
        call(&[kit.speaking(), priya, marcus]),
    ]);
    assert_eq!(
        read.saved[3..],
        [
            floor(0, Some(2)),
            floor(500, Some(2)),
            floor(1_000, Some(3)),
            floor(1_500, None),
            floor(2_000, Some(1)),
        ]
    );
    assert_eq!(read.looked_at_ms, [0, 500, 1_000, 1_500, 2_000]);
}

#[test]
fn someone_who_left_the_roster_can_still_hold_the_floor() {
    let priya = AppParticipantId::parse(PRIYA).unwrap();
    let mut state = PollState::continuing(&[]);
    state.observe(0, invented_app(&call(&[tile(PRIYA, "Priya Nair")])));
    // The app shows only who fits on screen, and marks her all the same.
    let step = state.observe(
        500,
        Observation::Active {
            roster: Vec::new(),
            floor: Some(priya),
            local_speaking: false,
        },
    );
    assert_eq!(step.evidence, [floor(500, Some(1))]);
}

#[test]
fn a_mark_on_someone_the_app_never_showed_writes_nothing_of_the_floor() {
    let mut state = PollState::continuing(&[]);
    let step = state.observe(
        0,
        Observation::Active {
            roster: vec![ObservedParticipant {
                id: AppParticipantId::parse(KIT).unwrap(),
                name: SpeakerName::parse("Kit"),
                is_local: true,
            }],
            floor: AppParticipantId::parse(MARCUS),
            local_speaking: false,
        },
    );
    // Kit is still on the call. Who spoke is not known, and no key is
    // made up for a person nobody saw.
    assert_eq!(step.evidence, [person(1, "Kit", true, 0)]);
    assert_eq!(
        step.change,
        Some(Change {
            now: Sight::UnknownHolder,
            before: None,
        })
    );
    assert_eq!(state.tally().unknown_holder, 1);
    assert_eq!(state.tally().people, 1);
}

// MARK: Looks that cannot be trusted

#[test]
fn a_snapshot_cut_short_is_an_untrusted_look_and_names_nobody_new() {
    let kit = tile(KIT, "Kit").local();
    let priya = tile(PRIYA, "Priya Nair");
    let marcus = tile(MARCUS, "Marcus Webb");
    let read = read(vec![
        call(&[kit, priya.speaking()]),
        // Marcus has joined, and the read stopped before his tile: what it
        // did reach, Priya still marked, is not believed.
        cut_short(&[kit, priya.speaking()]),
        call(&[kit, priya, marcus.speaking()]),
    ]);
    assert_eq!(
        read.saved,
        [
            person(1, "Kit", true, 0),
            person(2, "Priya Nair", false, 0),
            floor(0, Some(2)),
            untrusted(500, Untrusted::CutShort),
            person(3, "Marcus Webb", false, 1_000),
            floor(1_000, Some(3)),
        ]
    );
    assert_eq!(read.tally.cut_short, 1);
}

#[test]
fn two_call_windows_are_an_untrusted_look() {
    let kit = tile(KIT, "Kit").local();
    let priya = tile(PRIYA, "Priya Nair");
    let marcus = tile(MARCUS, "Marcus Webb");
    let read = read(vec![
        call(&[kit, priya.speaking()]),
        two_calls(&[kit, priya.speaking()], &[kit, marcus.speaking()]),
        call(&[kit, priya.speaking()]),
    ]);
    assert_eq!(
        read.saved,
        [
            person(1, "Kit", true, 0),
            person(2, "Priya Nair", false, 0),
            floor(0, Some(2)),
            // Marcus, seen only in a look that is not believed, is nobody.
            untrusted(500, Untrusted::TwoWindows),
            floor(1_000, Some(2)),
        ]
    );
    assert_eq!((read.tally.two_windows, read.tally.people), (1, 2));
}

#[test]
fn a_hidden_call_window_is_an_untrusted_look_for_as_long_as_it_is_hidden() {
    let kit = tile(KIT, "Kit").local();
    let priya = tile(PRIYA, "Priya Nair");
    let read = read(vec![
        call(&[kit, priya.speaking()]),
        hidden_call(),
        hidden_call(),
        hidden_call(),
        call(&[kit, priya]),
    ]);
    assert_eq!(
        read.saved[2..],
        [
            floor(0, Some(2)),
            untrusted(500, Untrusted::WindowHidden),
            untrusted(1_000, Untrusted::WindowHidden),
            untrusted(1_500, Untrusted::WindowHidden),
            floor(2_000, None),
        ]
    );
    assert_eq!(
        read.tally,
        Tally {
            floor: 2,
            window_hidden: 3,
            people: 2,
            ..Default::default()
        }
    );
    assert_eq!((read.tally.looks(), read.tally.untrusted()), (5, 3));
}

#[test]
fn a_run_of_looks_that_come_to_the_same_is_one_change() {
    let mut state = PollState::continuing(&[]);
    let seen = [tile(KIT, "Kit").local(), tile(PRIYA, "Priya Nair")];
    let looks = [
        call(&seen),
        call(&seen),
        hidden_call(),
        hidden_call(),
        hidden_call(),
        cut_short(&seen),
        no_call(),
        call(&seen),
    ];
    let changes = looks.iter().enumerate().filter_map(|(look, snapshot)| {
        let observation = Observation::of(snapshot, &invented_app);
        state.observe(look as u64 * POLL_MS, observation).change
    });
    let hidden = Sight::Untrusted(Untrusted::WindowHidden);
    let cut = Sight::Untrusted(Untrusted::CutShort);
    assert_eq!(
        changes
            .map(|change| (change.now, change.before))
            .collect::<Vec<_>>(),
        [
            (Sight::Floor, None),
            (hidden, Some((Sight::Floor, 2))),
            (cut, Some((hidden, 3))),
            (Sight::NoCallWindow, Some((cut, 1))),
            (Sight::Floor, Some((Sight::NoCallWindow, 1))),
        ]
    );
}

#[test]
fn an_app_with_no_call_window_writes_nothing_and_makes_no_file() {
    let read = read(vec![no_call(), no_call(), showing(Vec::new())]);
    assert_eq!(read.saved, []);
    assert_eq!(
        read.tally,
        Tally {
            no_call_window: 3,
            ..Default::default()
        }
    );

    let folder = folder("no-call");
    let looks = vec![(no_call(), 0), (no_call(), 0)];
    read_into(PollState::continuing(&[]), looks, 2 * POLL_MS, |lines| {
        evidence::append(&folder, lines)
    });
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
    std::fs::remove_dir_all(folder).unwrap();
}

// MARK: What reaches the file and the log

/// A whole call, with everything in it that could go wrong.
fn eventful_call() -> Vec<Snapshot> {
    let kit = tile(KIT, "Kit").local();
    let priya = tile(PRIYA, "Priya Nair");
    let marcus = tile(MARCUS, "Marcus Webb");
    vec![
        no_call(),
        call(&[kit, priya.speaking()]),
        call(&[kit, priya.speaking(), marcus]),
        cut_short(&[kit, priya, marcus.speaking()]),
        call(&[kit, priya, marcus.speaking()]),
        hidden_call(),
        two_calls(&[kit, priya], &[kit, tile(SAM, "Sam Ortiz").speaking()]),
        call(&[kit.speaking(), tile(PRIYA, "Priya N."), marcus]),
    ]
}

#[test]
fn the_file_holds_keys_and_names_and_never_an_app_id() {
    let folder = folder("keys");
    let snapshots = eventful_call();
    let stops_at_ms = snapshots.len() as u64 * POLL_MS;
    let looks = snapshots
        .into_iter()
        .map(|snapshot| (snapshot, 0))
        .collect();
    read_into(PollState::continuing(&[]), looks, stops_at_ms, |lines| {
        evidence::append(&folder, lines)
    });

    let file = std::fs::read_to_string(folder.join("speakers.ndjson")).unwrap();
    assert_eq!(
        file,
        concat!(
            r#"{"kind":"participant","key":"p1","name":"Kit","is_local":true,"at_ms":500}"#,
            "\n",
            r#"{"kind":"participant","key":"p2","name":"Priya Nair","is_local":false,"at_ms":500}"#,
            "\n",
            r#"{"kind":"floor","at_ms":500,"holder":"p2"}"#,
            "\n",
            r#"{"kind":"participant","key":"p3","name":"Marcus Webb","is_local":false,"at_ms":1000}"#,
            "\n",
            r#"{"kind":"floor","at_ms":1000,"holder":"p2"}"#,
            "\n",
            r#"{"kind":"untrusted","at_ms":1500,"reason":"cut_short"}"#,
            "\n",
            r#"{"kind":"floor","at_ms":2000,"holder":"p3"}"#,
            "\n",
            r#"{"kind":"untrusted","at_ms":2500,"reason":"window_hidden"}"#,
            "\n",
            r#"{"kind":"untrusted","at_ms":3000,"reason":"two_windows"}"#,
            "\n",
            r#"{"kind":"participant","key":"p2","name":"Priya N.","is_local":false,"at_ms":3500}"#,
            "\n",
            r#"{"kind":"floor","at_ms":3500,"holder":"p1"}"#,
            "\n",
        )
    );
    for id in APP_IDS {
        assert!(!file.contains(id), "{id} reached the file");
        // Nor any piece of one: no line holds the ids' common shape.
        assert!(!file.contains(&id[..3]), "part of {id} reached the file");
    }
    // And it reads back as the evidence the resolver takes.
    assert_eq!(evidence::read(&folder).unwrap().len(), 11);
    std::fs::remove_dir_all(folder).unwrap();
}

/// A fixed-seed generator: the same calls on every run.
struct Dice(u64);

impl Dice {
    fn roll(&mut self, sides: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % sides
    }

    /// One look at a call of up to six people who come, go, rename
    /// themselves and appear twice, through an app that is sometimes
    /// hidden, doubled or cut short, and sometimes marks a stranger.
    fn look(&mut self) -> Observation {
        match self.roll(12) {
            0 => return Observation::NoCallWindow,
            1 => return Observation::Untrusted(Untrusted::WindowHidden),
            2 => return Observation::Untrusted(Untrusted::TwoWindows),
            3 => return Observation::Untrusted(Untrusted::CutShort),
            _ => {}
        }
        let person = |dice: &mut Self| {
            let who = dice.roll(6);
            ObservedParticipant {
                id: AppParticipantId::parse(&format!("U0APPID{who:04}")).unwrap(),
                name: SpeakerName::parse(["Ada", "Ben", "", "You"][dice.roll(4) as usize]),
                is_local: who == 0,
            }
        };
        let roster: Vec<_> = (0..self.roll(8)).map(|_| person(self)).collect();
        let floor = match self.roll(4) {
            0 => None,
            // Anyone at all, in the roster or not, ever seen or not.
            _ => Some(person(self).id),
        };
        Observation::Active {
            roster,
            floor,
            local_speaking: self.roll(2) == 1,
        }
    }
}

#[test]
fn whatever_an_app_shows_every_key_is_introduced_before_it_is_used_and_no_id_is_written() {
    let mut dice = Dice(0x5EED_0005);
    for _ in 0..50 {
        let mut state = PollState::continuing(&[]);
        let mut saved = Vec::new();
        for look in 0..200 {
            saved.extend(state.observe(look * POLL_MS, dice.look()).evidence);
        }

        let mut introduced = Vec::new();
        let mut looks_written = 0;
        for line in &saved {
            match line {
                Evidence::Participant(participant) => {
                    // Keys run p1, p2, ... with none skipped.
                    if !introduced.contains(&participant.key) {
                        assert_eq!(participant.key, key(introduced.len() as u32 + 1));
                        introduced.push(participant.key);
                    }
                }
                Evidence::Floor { holder, .. } => {
                    looks_written += 1;
                    assert!(holder.is_none_or(|holder| introduced.contains(&holder)));
                }
                Evidence::Untrusted { .. } => looks_written += 1,
                Evidence::Correction(_) => panic!("a reader corrects nothing"),
            }
        }
        let tally = state.tally();
        assert_eq!(tally.looks(), 200);
        assert_eq!(looks_written, tally.floor + tally.untrusted());
        assert_eq!(introduced.len() as u32, tally.people);
        assert!(tally.people <= 6, "one key a person");

        let written: String = saved
            .iter()
            .map(|line| serde_json::to_string(line).unwrap())
            .collect();
        assert!(!written.contains("APPID"), "an app id was written");
        assert!(!format!("{saved:?}").contains("APPID"));
    }
}

/// Gathers what is logged on this thread while `work` runs, at every
/// level.
fn logged(work: impl FnOnce()) -> String {
    #[derive(Clone)]
    struct Lines(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Lines {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let lines = Lines(Arc::default());
    let writer = lines.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, work);
    String::from_utf8(lines.0.lock().unwrap().clone()).unwrap()
}

#[test]
fn the_log_holds_counts_and_reasons_and_never_a_name_or_an_app_id() {
    let snapshots = eventful_call();
    let stops_at_ms = snapshots.len() as u64 * POLL_MS;
    let looks = snapshots
        .into_iter()
        .map(|snapshot| (snapshot, 0))
        .collect();
    let saves = Cell::new(0);
    let log = logged(|| {
        read_into(PollState::continuing(&[]), looks, stops_at_ms, |_| {
            // The disk fills part of the way through the call.
            saves.set(saves.get() + 1);
            if saves.get() > 2 {
                return Err(eyre!(io::Error::from(io::ErrorKind::StorageFull)));
            }
            Ok(())
        });
    });

    // Every kind of line the reader logs was logged: each change in what
    // it saw, the failure to save, and the count at the end.
    for expected in [
        "the speaker reader sees something else now=NoCallWindow before=None",
        "now=Floor before=Some((NoCallWindow, 1))",
        "now=Untrusted(CutShort) before=Some((Floor, 2))",
        "now=Untrusted(WindowHidden) before=Some((Floor, 1))",
        "now=Untrusted(TwoWindows) before=Some((Untrusted(WindowHidden), 1))",
        "could not save what the speaker reader saw",
        "the speaker reader stopped looks=8 floor=4 no_call_window=1 cut_short=1 \
         window_hidden=1 two_windows=1 failed_health_check=0 unknown_holder=0 people=3 unsaved=11",
    ] {
        assert!(log.contains(expected), "{expected:?} is not in:\n{log}");
    }
    assert_eq!(log.matches("could not save").count(), 1, "{log}");

    for private in APP_IDS.into_iter().chain([
        "Kit", "Priya", "Nair", "Marcus", "Webb", "Sam", "Ortiz", "U0",
    ]) {
        assert!(!log.contains(private), "{private:?} was logged:\n{log}");
    }
}

#[test]
fn nothing_the_reader_can_print_holds_a_name_or_an_app_id() {
    let mut state = PollState::continuing(&[]);
    let seen = invented_app(&call(&[
        tile(KIT, "Kit").local(),
        tile(PRIYA, "Priya Nair").speaking(),
    ]));
    let step = state.observe(0, seen.clone());
    let printed = format!("{seen:?} {step:?} {:?}", state.tally());
    for private in [KIT, PRIYA, "Kit", "Priya", "Nair"] {
        assert!(!printed.contains(private), "{private:?} in {printed}");
    }
}

// MARK: Saving, pace and stopping

#[test]
fn lines_that_were_not_saved_are_counted_and_everyone_is_written_again() {
    let kit = tile(KIT, "Kit").local();
    let priya = tile(PRIYA, "Priya Nair");
    let snapshots = vec![
        call(&[kit, priya.speaking()]),
        call(&[kit, priya.speaking()]),
        call(&[kit, priya]),
        call(&[kit, priya]),
    ];
    let looks = snapshots
        .into_iter()
        .map(|snapshot| (snapshot, 0))
        .collect();
    let saved = RefCell::new(Vec::new());
    let mut saves = 0;
    let (tally, _) = read_into(PollState::continuing(&[]), looks, 4 * POLL_MS, |lines| {
        saves += 1;
        // The first two writes fail: the one that introduced everybody,
        // and the look after it.
        if saves <= 2 {
            return Err(eyre!("the disk is full"));
        }
        saved.borrow_mut().extend_from_slice(lines);
        Ok(())
    });
    // The floor of the two lost looks is gone, and that is all: the first
    // lines to be saved bring everyone with them, under the keys they had.
    assert_eq!(
        saved.into_inner(),
        [
            person(1, "Kit", true, 1_000),
            person(2, "Priya Nair", false, 1_000),
            floor(1_000, None),
            floor(1_500, None),
        ]
    );
    assert_eq!(
        tally,
        Tally {
            floor: 4,
            people: 2,
            // Two people and a floor, then two people and a floor again.
            unsaved: 6,
            ..Default::default()
        }
    );
}

#[test]
fn a_slow_look_is_followed_by_a_rest_and_one_look_not_by_the_ones_it_missed() {
    let seen = [tile(KIT, "Kit").local(), tile(PRIYA, "Priya Nair")];
    let looks = vec![
        // At once: the next look comes a whole pace after this one began.
        (call(&seen), 0),
        // Within the pace: the next still comes a pace after this began.
        (call(&seen), 200),
        // Past it: three looks were due meanwhile. One is taken, after a
        // rest.
        (call(&seen), 1_600),
        // Most of the pace: the rest is kept whole, and the next look
        // comes a little late.
        (call(&seen), 400),
        (call(&seen), 0),
    ];
    let (tally, looked_at_ms) = read_into(PollState::continuing(&[]), looks, 3_800, |_| Ok(()));
    assert_eq!(REST_MS, 250);
    assert_eq!(looked_at_ms, [0, 500, 1_000, 2_850, 3_500]);
    assert_eq!(tally.looks(), 5);
}

#[test]
fn stopping_ends_the_reader_with_nothing_read_or_written_afterwards() {
    let kit = tile(KIT, "Kit").local();
    let priya = tile(PRIYA, "Priya Nair");
    // The recording stops in the wait after the third look. `read_into`
    // fails the test if the app is read again.
    let looks = vec![
        (call(&[kit, priya]), 0),
        (call(&[kit, priya.speaking()]), 0),
        (call(&[kit, priya.speaking()]), 0),
    ];
    let mut saved = Vec::new();
    let (tally, looked_at_ms) = read_into(PollState::continuing(&[]), looks, 1_200, |lines| {
        saved.extend_from_slice(lines);
        Ok(())
    });
    assert_eq!(looked_at_ms, [0, 500, 1_000]);
    assert_eq!(saved.last(), Some(&floor(1_000, Some(2))));
    assert_eq!(tally.looks(), 3);

    // A recording that stops while the app is being read keeps that look:
    // it was made inside the recording.
    let (tally, looked_at_ms) = read_into(
        PollState::continuing(&[]),
        vec![(call(&[kit, priya]), 900)],
        600,
        |lines| {
            assert_eq!(lines.len(), 3);
            Ok(())
        },
    );
    assert_eq!((tally.looks(), looked_at_ms), (1, vec![0]));

    // And a reader started on a recording that has already stopped never
    // looks at the app at all.
    let (tally, looked_at_ms) = read_into(PollState::continuing(&[]), Vec::new(), 0, |_| {
        panic!("nothing is saved")
    });
    assert_eq!((tally, looked_at_ms), (Tally::default(), Vec::new()));
}

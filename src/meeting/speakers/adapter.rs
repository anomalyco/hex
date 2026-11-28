//! What a call app shows, as Hex takes it.
//!
//! Every call app draws its call differently, and each gets an adapter: one
//! pure function from a snapshot of the app's windows to an
//! [`Observation`], which says who is on the call and who holds the floor,
//! or why that cannot be told. The snapshot is plain data
//! ([`accessibility::tree`](crate::accessibility::tree)), so an adapter
//! touches no app, no clock and no file, and is tested on trees written by
//! hand. Reading another app is one adapter and one line in
//! [`registry`](super::registry).
//!
//! An adapter tells people apart by whatever the app does, most often a
//! user id inside a DOM id. That is an [`AppParticipantId`], and it is the
//! one thing here that must never be saved or logged. It cannot be: it does
//! not serialize, it prints without its text, and nothing reads the text
//! back out of it. The [`reader`](super::reader) gives each one a key of
//! the meeting's own (`p1`, `p2`) and forgets it when the recording ends.
//!
//! macOS only: the snapshot is read by Hex's Accessibility client, and
//! Windows and Linux Hex have none (UI Automation, AT-SPI) yet.

use std::fmt;

use super::SpeakerName;
use super::evidence::Untrusted;
use crate::accessibility::tree::Snapshot;

/// A call app's own id for someone on a call. It tells two tiles of one
/// person from two people, and that is all it can do: two of them can be
/// compared, and nothing else.
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct AppParticipantId(String);

impl AppParticipantId {
    /// The id an app shows for someone. `None` when there is nothing in
    /// it: a blank id would make one person of everyone it was read for.
    pub fn parse(shown: &str) -> Option<Self> {
        let id = shown.trim();
        (!id.is_empty()).then(|| Self(id.to_owned()))
    }
}

impl fmt::Debug for AppParticipantId {
    /// Without the text, so that no log line or panic message can carry it.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AppParticipantId(..)")
    }
}

/// Someone a call app showed on the call at one look.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedParticipant {
    pub id: AppParticipantId,
    /// What the app calls them, cleaned. Nobody when it shows nothing Hex
    /// can use as a name.
    pub name: Option<SpeakerName>,
    /// The person using Hex.
    pub is_local: bool,
}

/// What one look at a call app's windows showed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Observation {
    /// The call, read whole.
    Active {
        /// Everyone the app shows on the call. Two tiles of one person are
        /// one person: the first of them stands.
        roster: Vec<ObservedParticipant>,
        /// The one person the app marks as speaking, or nobody. They are
        /// in the roster, now or at an earlier look: a mark on someone the
        /// app never showed says nothing about the floor.
        ///
        /// When an app marks active tiles independently (as Slack does),
        /// the user's own tile does not compete with a lone remote speaker
        /// because the microphone is its own audio track: the sole remote
        /// speaker holds `floor`, while `local_speaking` records the user's
        /// own tile.
        floor: Option<AppParticipantId>,
        /// Whether the user's own tile is marked as speaking at this look,
        /// even while a remote speaker holds `floor` or several remote
        /// tiles are marked at once. Lets calibration follow the user's
        /// tile without blocking remote attribution.
        local_speaking: bool,
    },
    /// None of the app's windows is a call.
    NoCallWindow,
    /// A call window is there, and what it shows cannot be relied on.
    Untrusted(Untrusted),
}

impl Observation {
    /// What `adapter` makes of one snapshot, under the two rules that hold
    /// for every app and so are no adapter's to remember. A snapshot a
    /// budget cut short may be missing a tile, so no adapter is shown it.
    /// And an app with no window has no call window.
    pub fn of(snapshot: &Snapshot, adapter: &dyn CallAppAdapter) -> Self {
        if !snapshot.cut_short.is_empty() {
            Self::Untrusted(Untrusted::CutShort)
        } else if snapshot.windows.is_empty() {
            Self::NoCallWindow
        } else {
            adapter.observe(snapshot)
        }
    }
}

/// Reads one call app's windows. The same snapshot always gets the same
/// answer and nothing is touched, so one adapter serves every recording.
pub trait CallAppAdapter: Sync {
    /// What the app's windows show. Asked only through
    /// [`Observation::of`]: of a snapshot read whole, with a window in it.
    fn observe(&self, snapshot: &Snapshot) -> Observation;
}

/// An adapter is most often just a function.
impl<F: Fn(&Snapshot) -> Observation + Sync> CallAppAdapter for F {
    fn observe(&self, snapshot: &Snapshot) -> Observation {
        self(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessibility::tree::{Cut, Node};

    fn window() -> Node {
        Node {
            role: Some("AXWindow".into()),
            ..Default::default()
        }
    }

    fn nobody_speaking(_: &Snapshot) -> Observation {
        Observation::Active {
            roster: Vec::new(),
            floor: None,
            local_speaking: false,
        }
    }

    fn never_asked(_: &Snapshot) -> Observation {
        panic!("no adapter is shown this snapshot")
    }

    #[test]
    fn a_snapshot_read_whole_is_the_adapters_to_read() {
        let snapshot = Snapshot {
            windows: vec![window()],
            ..Default::default()
        };
        assert_eq!(
            Observation::of(&snapshot, &nobody_speaking),
            nobody_speaking(&snapshot)
        );
    }

    #[test]
    fn a_snapshot_cut_short_is_never_shown_to_an_adapter() {
        for cut in [Cut::Nodes, Cut::Depth, Cut::Deadline] {
            // Whatever was read before the budget ran out, even nothing.
            for windows in [vec![window()], Vec::new()] {
                let snapshot = Snapshot {
                    windows,
                    cut_short: vec![cut],
                    ..Default::default()
                };
                assert_eq!(
                    Observation::of(&snapshot, &never_asked),
                    Observation::Untrusted(Untrusted::CutShort)
                );
            }
        }
    }

    #[test]
    fn an_app_with_no_window_has_no_call_window() {
        assert_eq!(
            Observation::of(&Snapshot::default(), &never_asked),
            Observation::NoCallWindow
        );
    }

    #[test]
    fn an_app_id_is_never_blank_and_never_printed() {
        let id = AppParticipantId::parse(" U024BE7LH ").unwrap();
        assert_eq!(id, AppParticipantId::parse("U024BE7LH").unwrap());
        assert_ne!(id, AppParticipantId::parse("U024BE7LJ").unwrap());
        for blank in ["", "  ", "\n"] {
            assert_eq!(AppParticipantId::parse(blank), None, "{blank:?}");
        }

        let seen = Observation::Active {
            roster: vec![ObservedParticipant {
                id: id.clone(),
                name: SpeakerName::parse("Priya Nair"),
                is_local: false,
            }],
            floor: Some(id),
            local_speaking: false,
        };
        let printed = format!("{seen:?}");
        assert!(!printed.contains("U024BE7LH") && !printed.contains("Priya"));
    }
}

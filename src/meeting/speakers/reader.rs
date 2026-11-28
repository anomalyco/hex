//! The reader: looks at a call app while a meeting records, and writes
//! down what it sees.
//!
//! It is two things. [`PollState`] is the rule, and it is pure: one look
//! in, the evidence that look adds out. It is where a call app's own id for
//! a person stops. Each id gets a key of the meeting's own (`p1`, `p2`, in
//! the order people are first seen), the key is what is written, and the id
//! stays in memory until the recording ends. [`run`] is the loop around
//! it: look, write, rest, until the recording stops. The clock, the read of
//! the app and the file are handed to it, so the whole life of a reader
//! runs in a test with no app, no wait and no disk.
//!
//! What it writes is what it saw and nothing it worked out. Someone on the
//! call is written when first seen, and again only when what the app says
//! of them changes. Every look that can be trusted writes who held the
//! floor, and one that cannot writes why not. A look that finds no call
//! window writes nothing: the hole it leaves already says nobody was seen.
//!
//! It logs how many looks came to what, and nothing else. A name or an app
//! id has no way into a log line: everything logged is a [`Sight`] or a
//! count.
//!
//! macOS only, as the [`adapter`](super::adapter) is.

use std::collections::{HashMap, HashSet};

use color_eyre::eyre::Result;

use super::SpeakerName;
use super::adapter::{AppParticipantId, CallAppAdapter, Observation, ObservedParticipant};
use super::evidence::{Evidence, Participant, ParticipantKey, Untrusted};
use super::floor::POLL_MS;
use crate::accessibility::tree::Snapshot;

/// The least the reader rests between two looks, however long the first
/// took: an app slow to answer is still left a pause between reads.
const REST_MS: u64 = POLL_MS / 2;

/// What one look came to. This and counts are all the reader ever logs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Sight {
    /// The call, read whole: who held the floor was written.
    Floor,
    /// None of the app's windows was a call. Nothing was written.
    NoCallWindow,
    /// A call window that could not be relied on. Why was written.
    Untrusted(Untrusted),
    /// The app marked someone it had never shown as speaking. Nothing was
    /// written of the floor: the mark has nobody to belong to.
    UnknownHolder,
}

/// What the reader saw changed from one look to the next.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Change {
    pub now: Sight,
    /// What it had been seeing, and for how many looks running. Nothing
    /// at the first look.
    pub before: Option<(Sight, u32)>,
}

/// What one look adds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Step {
    /// The lines to save, in order.
    pub evidence: Vec<Evidence>,
    /// Set when this look came to something other than the last one did.
    pub change: Option<Change>,
}

/// How many looks came to what over a reader's life, and how many people
/// it met.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Tally {
    pub floor: u32,
    pub no_call_window: u32,
    pub cut_short: u32,
    pub window_hidden: u32,
    pub two_windows: u32,
    pub failed_health_check: u32,
    pub unknown_holder: u32,
    pub people: u32,
    /// Lines that could not be saved.
    pub unsaved: usize,
}

impl Tally {
    pub fn looks(&self) -> u32 {
        self.floor + self.no_call_window + self.untrusted() + self.unknown_holder
    }

    pub fn untrusted(&self) -> u32 {
        self.cut_short + self.window_hidden + self.two_windows + self.failed_health_check
    }

    fn count(&mut self, sight: Sight) {
        *match sight {
            Sight::Floor => &mut self.floor,
            Sight::NoCallWindow => &mut self.no_call_window,
            Sight::Untrusted(Untrusted::CutShort) => &mut self.cut_short,
            Sight::Untrusted(Untrusted::WindowHidden) => &mut self.window_hidden,
            Sight::Untrusted(Untrusted::TwoWindows) => &mut self.two_windows,
            Sight::Untrusted(Untrusted::FailedHealthCheck) => &mut self.failed_health_check,
            Sight::UnknownHolder => &mut self.unknown_holder,
        } += 1;
    }
}

/// Someone the reader has met.
struct Known {
    key: ParticipantKey,
    /// What the evidence last said of them: their name, and whether they
    /// are the user. Nothing while a line for them is still owed.
    written: Option<(Option<SpeakerName>, bool)>,
}

/// One reader's memory of one recording: who it has met and under which
/// key, and what its looks have come to.
pub struct PollState {
    /// The key the next person met takes.
    next: ParticipantKey,
    people: HashMap<AppParticipantId, Known>,
    /// What the last look came to, and how many looks running have.
    sight: Option<(Sight, u32)>,
    tally: Tally,
}

impl PollState {
    /// A reader starting on a meeting that already holds `evidence`, which
    /// is none for a new one. It remembers nobody, so everyone it meets is
    /// keyed after everyone the evidence holds: a key never comes to mean
    /// two people.
    pub fn continuing(evidence: &[Evidence]) -> Self {
        Self {
            next: ParticipantKey::next(evidence),
            people: HashMap::new(),
            sight: None,
            tally: Tally::default(),
        }
    }

    /// Takes one look, made `at_ms` into the recording, and says what it
    /// adds to the evidence.
    pub fn observe(&mut self, at_ms: u64, observation: Observation) -> Step {
        let mut evidence = Vec::new();
        let sight = match observation {
            Observation::NoCallWindow => Sight::NoCallWindow,
            Observation::Untrusted(reason) => {
                evidence.push(Evidence::Untrusted { at_ms, reason });
                Sight::Untrusted(reason)
            }
            Observation::Active {
                roster,
                floor,
                local_speaking,
            } => {
                evidence.extend(self.meet(at_ms, roster));
                match floor.map(|holder| self.people.get(&holder)) {
                    Some(None) => Sight::UnknownHolder,
                    holder => {
                        let holder = holder.flatten().map(|known| known.key);
                        evidence.push(Evidence::Floor {
                            at_ms,
                            holder,
                            local_speaking,
                        });
                        Sight::Floor
                    }
                }
            }
        };
        self.tally.count(sight);
        let change = match &mut self.sight {
            Some((last, looks)) if *last == sight => {
                *looks += 1;
                None
            }
            last => {
                let before = last.replace((sight, 1));
                Some(Change { now: sight, before })
            }
        };
        Step { evidence, change }
    }

    /// A line for each person in `roster` the evidence has not met, or has
    /// met as someone the app no longer shows them to be.
    fn meet(&mut self, at_ms: u64, roster: Vec<ObservedParticipant>) -> Vec<Evidence> {
        let mut lines = Vec::new();
        let mut met = HashSet::new();
        for ObservedParticipant { id, name, is_local } in roster {
            let known = self.people.entry(id).or_insert_with(|| {
                let key = self.next;
                self.next = key.following();
                self.tally.people += 1;
                Known { key, written: None }
            });
            // A second tile of someone already read at this look.
            if !met.insert(known.key) {
                continue;
            }
            let shown = (name, is_local);
            if known.written.as_ref() != Some(&shown) {
                lines.push(Evidence::Participant(Participant {
                    key: known.key,
                    name: shown.0.clone(),
                    is_local,
                    at_ms,
                }));
                known.written = Some(shown);
            }
        }
        lines
    }

    /// Notes that `lines` handed out by [`Self::observe`] were not saved.
    /// Any of them may have been the one that introduced someone, so
    /// everyone is written again at their next look: a floor line never
    /// waits on a person the file has not met.
    pub fn unsaved(&mut self, lines: usize) {
        self.tally.unsaved += lines;
        for known in self.people.values_mut() {
            known.written = None;
        }
    }

    pub fn tally(&self) -> Tally {
        self.tally
    }
}

/// The recording the reader runs beside: its clock, and its end.
pub trait CaptureClock {
    /// Milliseconds since the recording started, as the transcript's lines
    /// count them.
    fn now_ms(&self) -> u64;

    /// Waits `ms`, or until the recording stops when that comes first.
    fn wait(&mut self, ms: u64) -> Wait;
}

/// How a wait ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Wait {
    Elapsed,
    /// The recording stopped, before the wait or during it.
    Stopped,
}

/// Where the evidence goes: the end of the meeting's own file.
pub trait EvidenceSink {
    /// Saves `evidence`, in order, after everything saved before. Lines it
    /// fails to save are lost, and the reader carries on: losing a look at
    /// the floor can only take a name away. Its error is logged, so it
    /// says what went wrong and nothing of what was being saved.
    fn append(&mut self, evidence: &[Evidence]) -> Result<()>;
}

impl<F: FnMut(&[Evidence]) -> Result<()>> EvidenceSink for F {
    fn append(&mut self, evidence: &[Evidence]) -> Result<()> {
        self(evidence)
    }
}

/// Reads a call app for as long as a recording runs: takes a snapshot with
/// `snapshot`, has `adapter` read it, and saves what that adds to `sink`,
/// every [`POLL_MS`] by `clock`, until `clock` says the recording stopped.
/// Returns what its looks came to.
///
/// A look is stamped with the moment it started. A look that overran is
/// followed by a short rest and then the next one, never by the ones it
/// missed. Once the recording has stopped nothing more is read or written,
/// and a reader started on a stopped recording never looks at all.
pub fn run(
    mut state: PollState,
    adapter: &dyn CallAppAdapter,
    mut snapshot: impl FnMut() -> Snapshot,
    clock: &mut impl CaptureClock,
    sink: &mut impl EvidenceSink,
) -> Tally {
    let mut rest_ms = 0;
    let mut saving = true;
    while clock.wait(rest_ms) == Wait::Elapsed {
        let at_ms = clock.now_ms();
        let step = state.observe(at_ms, Observation::of(&snapshot(), adapter));
        if let Some(Change { now, before }) = step.change {
            tracing::debug!(?now, ?before, "the speaker reader sees something else");
        }
        if !step.evidence.is_empty() {
            match sink.append(&step.evidence) {
                Ok(()) => saving = true,
                Err(error) => {
                    state.unsaved(step.evidence.len());
                    // Once for a run of failures, not twice a second.
                    if saving {
                        tracing::warn!(%error, "could not save what the speaker reader saw");
                    }
                    saving = false;
                }
            }
        }
        let due_ms = at_ms.saturating_add(POLL_MS);
        rest_ms = due_ms.saturating_sub(clock.now_ms()).max(REST_MS);
    }
    let tally = state.tally();
    tracing::info!(
        looks = tally.looks(),
        floor = tally.floor,
        no_call_window = tally.no_call_window,
        cut_short = tally.cut_short,
        window_hidden = tally.window_hidden,
        two_windows = tally.two_windows,
        failed_health_check = tally.failed_health_check,
        unknown_holder = tally.unknown_holder,
        people = tally.people,
        unsaved = tally.unsaved,
        "the speaker reader stopped"
    );
    tally
}

#[cfg(test)]
mod tests;

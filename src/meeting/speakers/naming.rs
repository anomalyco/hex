//! Naming a meeting's transcript from the evidence in its folder.
//!
//! This is where the pieces meet. The lines give when each side spoke, the
//! evidence gives who the call app showed speaking, the user's own tile is
//! checked against the user's own microphone ([`calibrate`]), each stretch
//! of remote speech is given to one person or to nobody ([`resolve`]), and
//! the lines take their names ([`name_lines`]).
//!
//! It runs at two moments. While a meeting records, [`LiveNames`] names
//! each in-call transcript as it is written, and hands the speaker table to
//! the recorder, which owns the manifest. When the recording is over,
//! [`name_transcript`] names the final transcript. Both work the whole
//! meeting out again from the evidence every time, so what a line was
//! called a moment ago is never an input, and both keep the ids the meeting
//! has already given ([`Named::keeping_ids`]), so a reader that catches the
//! table and the lines between their two writes sees `Them`, never the
//! wrong name.
//!
//! Names start with the first remote line. Until the user's own tile has
//! lit five times there is too little to judge the call by, and the floor
//! is placed on the assumed timing ([`Calibration::Unproven`]); after
//! that, on what this call measured, or not at all if the tile and the
//! microphone disagree.
//!
//! Nothing is named unless Labs › Speaker names is on and the meeting has
//! evidence: every other transcript comes back as it went in.
//!
//! What is logged is counts and reasons ([`Summary`]), never a name.

use std::mem::discriminant;
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::Speakers;
use super::calibrate::{Calibration, calibrate};
use super::evidence::{self, Evidence, Interval};
use super::floor::local_floor;
use super::names::{Named, name_lines};
use super::resolve::{Attribution, Said, Strictness, Why, WhyNot, resolve};
use crate::meeting::{MeetingManifest, MeetingSource, TranscriptEntry};

/// Microphone lines closer than this are one stretch of the user talking,
/// as remote words closer than this are one span to the resolver.
const SPEECH_GAP_MS: u64 = 300;

/// What naming one transcript came to: counts and reasons, and nothing of
/// who anyone is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Summary {
    pub calibration: Calibration,
    /// Lines heard on the remote track, and how many of them took a name.
    pub remote_lines: usize,
    pub named_lines: usize,
    /// People who ended up on a line.
    pub speakers: usize,
    /// Stretches of remote speech given to someone, by why.
    pub held_floor: usize,
    pub only_remote_participant: usize,
    pub corrected: usize,
    /// Stretches given to nobody, by why not.
    pub unsure: usize,
    pub unhealthy: usize,
    pub no_evidence: usize,
    pub untrusted: usize,
    pub nobody_held_floor: usize,
    pub local_held_floor: usize,
    pub floor_changed: usize,
    pub mixed: usize,
}

impl Summary {
    fn of(calibration: Calibration, attributions: &[Attribution], named: &Named) -> Self {
        let remote = |entry: &&TranscriptEntry| entry.source == MeetingSource::System;
        let mut summary = Self {
            calibration,
            remote_lines: named.entries.iter().filter(remote).count(),
            named_lines: named
                .entries
                .iter()
                .filter(|entry| entry.speaker.is_some())
                .count(),
            speakers: named.speakers.0.len(),
            held_floor: 0,
            only_remote_participant: 0,
            corrected: 0,
            unsure: 0,
            unhealthy: 0,
            no_evidence: 0,
            untrusted: 0,
            nobody_held_floor: 0,
            local_held_floor: 0,
            floor_changed: 0,
            mixed: 0,
        };
        for attribution in attributions {
            *match attribution.said {
                Said::By(_, Why::HeldFloor) => &mut summary.held_floor,
                Said::By(_, Why::OnlyRemoteParticipant) => &mut summary.only_remote_participant,
                Said::By(_, Why::Corrected) => &mut summary.corrected,
                Said::ByNobody(WhyNot::Corrected) => &mut summary.unsure,
                Said::ByNobody(WhyNot::Unhealthy) => &mut summary.unhealthy,
                Said::ByNobody(WhyNot::NoEvidence) => &mut summary.no_evidence,
                Said::ByNobody(WhyNot::Untrusted(_)) => &mut summary.untrusted,
                Said::ByNobody(WhyNot::NobodyHeldFloor) => &mut summary.nobody_held_floor,
                Said::ByNobody(WhyNot::LocalHeldFloor) => &mut summary.local_held_floor,
                Said::ByNobody(WhyNot::FloorChanged) => &mut summary.floor_changed,
                Said::ByNobody(WhyNot::Mixed) => &mut summary.mixed,
            } += 1;
        }
        summary
    }

    fn log(&self, meeting: &str, pass: &'static str) {
        tracing::info!(
            meeting,
            pass,
            calibration = ?self.calibration,
            remote_lines = self.remote_lines,
            named_lines = self.named_lines,
            speakers = self.speakers,
            held_floor = self.held_floor,
            only_remote_participant = self.only_remote_participant,
            corrected = self.corrected,
            unsure = self.unsure,
            unhealthy = self.unhealthy,
            no_evidence = self.no_evidence,
            untrusted = self.untrusted,
            nobody_held_floor = self.nobody_held_floor,
            local_held_floor = self.local_held_floor,
            floor_changed = self.floor_changed,
            mixed = self.mixed,
            "meeting speakers named"
        );
    }
}

/// Names `entries`, a whole transcript on the recording's clock, from
/// `evidence`. A pure function of the two.
fn derive(entries: &[TranscriptEntry], evidence: &[Evidence]) -> (Named, Summary) {
    let from = |source: MeetingSource| entries.iter().filter(move |entry| entry.source == source);
    let calibration = calibrate(
        &local_floor(evidence),
        &speech(from(MeetingSource::Microphone)),
    );
    let words = from(MeetingSource::System)
        .flat_map(words)
        .collect::<Vec<_>>();
    let attributions = resolve(&words, evidence, calibration, Strictness::default());
    let named = name_lines(entries, &attributions, evidence);
    let summary = Summary::of(calibration, &attributions, &named);
    (named, summary)
}

/// When each word of a line was said, as nearly as the line can tell. A
/// line keeps its own start and end and not its words', so its time is
/// shared among its words by their length. A line has no long pause inside
/// it, so a word is placed within a few hundred milliseconds of where it
/// was said, which is finer than the half second between two looks at the
/// floor.
fn words(line: &TranscriptEntry) -> Vec<Interval> {
    let lengths = line
        .text
        .split_whitespace()
        .map(|word| word.chars().count() as u64 + 1)
        .collect::<Vec<_>>();
    let whole: u64 = lengths.iter().sum();
    let (start_ms, end_ms) = (line.start_ms, line.end_ms.max(line.start_ms));
    if whole == 0 {
        return vec![Interval::new(start_ms, end_ms)];
    }
    let at = |said: u64| start_ms + (end_ms - start_ms) * said / whole;
    let mut said = 0;
    lengths
        .into_iter()
        .map(|length| {
            let word = Interval::new(at(said), at(said + length));
            said += length;
            word
        })
        .collect()
}

/// When the user talked: the microphone's lines, with lines that run on
/// from one another joined into one stretch.
fn speech<'a>(lines: impl Iterator<Item = &'a TranscriptEntry>) -> Vec<Interval> {
    let mut lines = lines
        .map(|line| Interval::new(line.start_ms, line.end_ms))
        .collect::<Vec<_>>();
    lines.sort_by_key(|line| (line.start_ms(), line.end_ms()));
    let mut stretches: Vec<Interval> = Vec::new();
    for line in lines {
        match stretches.last_mut() {
            Some(last) if line.start_ms() < last.end_ms() + SPEECH_GAP_MS => {
                *last = Interval::new(last.start_ms(), last.end_ms().max(line.end_ms()));
            }
            _ => stretches.push(line),
        }
    }
    stretches
}

/// Names `entries` from the evidence in `directory`, under the ids
/// `known` has given. With `lab_on` unset, or no evidence, nobody is named.
fn named(
    directory: &Path,
    entries: Vec<TranscriptEntry>,
    known: &mut Speakers,
    lab_on: bool,
) -> (Named, Option<Summary>) {
    let evidence = if lab_on {
        evidence::read(directory).unwrap_or_else(|error| {
            tracing::warn!(%error, "could not read what the speaker reader saw");
            Vec::new()
        })
    } else {
        Vec::new()
    };
    if evidence.is_empty() {
        let speakers = Speakers::default();
        return (Named { speakers, entries }, None);
    }
    let (named, summary) = derive(&entries, &evidence);
    (named.keeping_ids(known), Some(summary))
}

/// Names a meeting's final transcript and sets the manifest's speaker
/// table to match it. The caller publishes both.
pub fn name_transcript(
    directory: &Path,
    manifest: &mut MeetingManifest,
    entries: Vec<TranscriptEntry>,
) -> Vec<TranscriptEntry> {
    name_transcript_when(
        directory,
        manifest,
        entries,
        crate::labs::meeting_speaker_names(),
    )
}

fn name_transcript_when(
    directory: &Path,
    manifest: &mut MeetingManifest,
    entries: Vec<TranscriptEntry>,
    lab_on: bool,
) -> Vec<TranscriptEntry> {
    // The table the recording saved: the ids its lines already carry.
    let mut known = manifest.speakers.clone();
    let (named, summary) = named(directory, entries, &mut known, lab_on);
    if let Some(summary) = summary {
        summary.log(&manifest.id, "final");
    }
    manifest.speakers = named.speakers;
    named.entries
}

/// The speaker table of a recording under way, on its way from the
/// transcript worker, which works it out, to the recorder, which saves it
/// in the manifest. Only the latest table waits.
#[derive(Clone, Default)]
pub struct LiveTable(Arc<Mutex<Option<Speakers>>>);

impl LiveTable {
    fn put(&self, speakers: Speakers) {
        *self.0.lock().unwrap_or_else(|error| error.into_inner()) = Some(speakers);
    }

    /// The table to save, when it changed since the last one taken.
    pub fn take(&self) -> Option<Speakers> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }
}

/// Names a recording's in-call transcript each time it is written. By
/// default its table goes to nobody.
#[derive(Default)]
pub struct LiveNames {
    table: LiveTable,
    /// Everyone this recording has given an id, on a line now or not.
    known: Speakers,
    /// The table last handed to the recorder.
    handed: Speakers,
    /// What the call had shown of its speaking mark, last time.
    calibration: Option<Calibration>,
}

impl LiveNames {
    pub fn handing_to(table: LiveTable) -> Self {
        Self {
            table,
            ..Self::default()
        }
    }

    /// Names `entries`, the whole transcript so far, from what the reader
    /// has saved in `directory` so far.
    pub fn name(
        &mut self,
        directory: &Path,
        entries: Vec<TranscriptEntry>,
    ) -> Vec<TranscriptEntry> {
        self.name_when(directory, entries, crate::labs::meeting_speaker_names())
    }

    fn name_when(
        &mut self,
        directory: &Path,
        entries: Vec<TranscriptEntry>,
        lab_on: bool,
    ) -> Vec<TranscriptEntry> {
        let (named, summary) = named(directory, entries, &mut self.known, lab_on);
        // Once for each change in how the call is judged, not at every
        // transcript.
        if let Some(summary) = summary
            && self.calibration.map(|last| discriminant(&last))
                != Some(discriminant(&summary.calibration))
        {
            self.calibration = Some(summary.calibration);
            let meeting = directory.file_name().unwrap_or_default().to_string_lossy();
            summary.log(&meeting, "live");
        }
        if named.speakers != self.handed {
            self.handed.clone_from(&named.speakers);
            self.table.put(named.speakers);
        }
        named.entries
    }
}

#[cfg(test)]
mod tests;

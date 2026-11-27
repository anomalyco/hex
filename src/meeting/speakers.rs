//! Who said a transcript line, and what a reader sees it called.
//!
//! A meeting keeps a small table of the remote speakers it could name, and a
//! transcript entry may point at one of them. [`Labels::of`] is the one
//! place that turns an entry into the word shown beside its text: the
//! speaker's name when the table knows the entry's speaker, and otherwise
//! `You`, `Them` or `Speaker`, exactly as before speakers existed.
//!
//! Names are untrusted. Other people on a call choose them, and they end up
//! in Markdown, in prompts and in what agents read. So the only way to hold
//! a name is a [`SpeakerName`], which exists only once the text has been
//! cleaned, whether it was typed, seen in a call app or read from disk. It
//! never reaches a log: a [`SpeakerName`] prints without its text.
//!
//! The file formats, the shape of an id and the cleaning are this module's
//! business. Everything else holds a [`SpeakerId`] or a [`SpeakerName`]
//! and asks [`Labels`].
//!
//! A speaker comes from one place: what a call app showed while the
//! meeting recorded (Labs › Speaker names; macOS, Slack huddles). A reader
//! beside the recording saves what it sees ([`SpeakerReader`]), and the
//! names are worked out from that, while the meeting records
//! ([`LiveNames`]) and again when it is over ([`name_transcript`]). With
//! the lab off, or in any other call, every table is empty and every line
//! is called what it was always called.

use std::fmt;
use std::num::NonZeroU32;

use serde::{Deserialize, Deserializer, Serialize};

use super::{MeetingManifest, MeetingSource, TranscriptEntry};

// Naming speakers from what a call app shows.
//
// The reader and what it reads through are macOS only: they take the
// snapshot Hex's Accessibility client makes, and Windows and Linux Hex have
// no such client yet. The rules that name a transcript from saved evidence
// are every OS's.
#[cfg(target_os = "macos")]
mod adapter;
mod calibrate;
mod evidence;
mod floor;
mod names;
mod naming;
#[cfg(target_os = "macos")]
mod reader;
#[cfg(target_os = "macos")]
mod registry;
mod resolve;
#[cfg(target_os = "macos")]
mod slack;
#[cfg(target_os = "macos")]
mod watch;

pub(super) use naming::{LiveNames, LiveTable, name_transcript};
#[cfg(target_os = "macos")]
pub(super) use watch::SpeakerReader;

/// The longest name the table keeps, in characters.
const NAME_CHARS: usize = 64;

/// The characters that give Markdown and markup their structure. A name
/// sits inside `**…**` headings, `<v …>` subtitle tags and prompt lines.
const MARKUP: [char; 7] = ['*', '`', '[', ']', '<', '>', '\\'];

/// The table of a meeting nobody has been named in.
static NOBODY: Speakers = Speakers(Vec::new());

/// A speaker's id inside one meeting: short and opaque (`s1`, `s2`), minted
/// by that meeting's table and meaningless outside it. A call app's own user
/// id is never one: text of any other shape does not read as an id at all.
#[derive(Clone, Eq, Hash, PartialEq, Serialize)]
#[serde(into = "String")]
pub struct SpeakerId(NonZeroU32);

impl SpeakerId {
    fn numbered(number: u32) -> Option<Self> {
        NonZeroU32::new(number).map(Self)
    }

    /// Reads an id as it is saved: `s` and a number, written the one way
    /// Hex writes it.
    fn parse(saved: &str) -> Option<Self> {
        let id = Self::numbered(saved.strip_prefix('s')?.parse().ok()?)?;
        (id.to_string() == saved).then_some(id)
    }

    /// A saved line's `speaker`. Anything that is not an id Hex could have
    /// minted reads as no speaker, so the line is called by its track and
    /// the transcript still opens.
    pub(super) fn saved<'de, D: Deserializer<'de>>(saved: D) -> Result<Option<Self>, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Saved {
            Text(String),
            Other(serde::de::IgnoredAny),
        }
        Ok(match Saved::deserialize(saved)? {
            Saved::Text(text) => Self::parse(&text),
            Saved::Other(_) => None,
        })
    }
}

impl fmt::Display for SpeakerId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "s{}", self.0)
    }
}

impl fmt::Debug for SpeakerId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self}")
    }
}

impl From<SpeakerId> for String {
    fn from(id: SpeakerId) -> Self {
        id.to_string()
    }
}

/// A speaker's display name, cleaned: one line, single spaces, nothing
/// invisible, no [`MARKUP`], at most [`NAME_CHARS`] characters, and never
/// one of Hex's own labels, so that nobody on a call can make their words
/// read as `You`. Holding one is the proof; nothing downstream checks again.
#[derive(Clone, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct SpeakerName(String);

impl SpeakerName {
    /// Cleans text someone else chose. `None` when nothing usable is left.
    pub fn parse(shown: &str) -> Option<Self> {
        let visible = shown
            .chars()
            .map(|character| {
                if character.is_whitespace() {
                    ' '
                } else {
                    character
                }
            })
            .filter(|character| !character.is_control() && !is_invisible(*character))
            .filter(|character| !MARKUP.contains(character))
            .collect::<String>();
        let spaced = visible.split_whitespace().collect::<Vec<_>>().join(" ");
        let name = spaced.chars().take(NAME_CHARS).collect::<String>();
        let name = name.trim_end();
        let own = ["You", "Them", "Speaker", "Notes"];
        (!name.is_empty() && !own.iter().any(|label| name.eq_ignore_ascii_case(label)))
            .then(|| Self(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SpeakerName {
    /// Without the text, so that no log line or panic message can carry it.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SpeakerName(..)")
    }
}

impl TryFrom<String> for SpeakerName {
    type Error = &'static str;

    /// A saved name is cleaned like any other, and is no name when nothing
    /// usable is left.
    fn try_from(saved: String) -> Result<Self, Self::Error> {
        Self::parse(&saved).ok_or("not a usable name")
    }
}

impl From<SpeakerName> for String {
    fn from(name: SpeakerName) -> Self {
        name.0
    }
}

/// Where a speaker's name came from.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NameSource {
    /// What the call app showed for the participant.
    CallApp,
    /// The calendar event's guest list.
    Calendar,
    /// Typed or chosen by the user.
    User,
}

/// One remote speaker a meeting could name.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Speaker {
    id: SpeakerId,
    name: SpeakerName,
    from: NameSource,
}

/// A meeting's speaker table. No two speakers share an id.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(from = "Vec<serde_json::Value>")]
pub struct Speakers(Vec<Speaker>);

impl Speakers {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Names a new speaker and returns the id their lines carry: the
    /// lowest number nobody in this meeting holds.
    pub fn add(&mut self, name: SpeakerName, from: NameSource) -> SpeakerId {
        let id = (1..)
            .filter_map(SpeakerId::numbered)
            .find(|id| self.name(id).is_none())
            .expect("a meeting has fewer speakers than there are ids");
        self.0.push(Speaker {
            id: id.clone(),
            name,
            from,
        });
        id
    }

    /// The name `id` goes by in this meeting, if the table knows it.
    pub fn name(&self, id: &SpeakerId) -> Option<&SpeakerName> {
        self.0
            .iter()
            .find(|speaker| speaker.id == *id)
            .map(|speaker| &speaker.name)
    }

    /// Every name, in the order the speakers were added.
    fn names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|speaker| speaker.name.as_str())
    }
}

impl From<Vec<serde_json::Value>> for Speakers {
    /// A table read from disk is parsed row by row into the same types as
    /// one built here: the file may have been edited, or written by
    /// something else. A row whose id, name or origin does not parse, or
    /// whose id is taken, is dropped, and its lines fall back to `Them`;
    /// the manifest still opens.
    fn from(saved: Vec<serde_json::Value>) -> Self {
        #[derive(Deserialize)]
        struct Saved {
            id: String,
            name: SpeakerName,
            from: NameSource,
        }
        let mut speakers = Self::default();
        for row in saved {
            let Ok(row) = serde_json::from_value::<Saved>(row) else {
                continue;
            };
            let Some(id) = SpeakerId::parse(&row.id).filter(|id| speakers.name(id).is_none())
            else {
                continue;
            };
            let (name, from) = (row.name, row.from);
            speakers.0.push(Speaker { id, name, from });
        }
        speakers
    }
}

/// How a recording's tracks are called when nobody is named: a call has
/// `You` and `Them`, and an imported audio file has one track, `Speaker`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tracks {
    Call,
    Imported,
}

/// Characters that draw nothing but can hide or reorder text: zero-width
/// spaces, direction marks and overrides, and the Unicode tag block. The
/// joiners that emoji and some scripts need are kept.
fn is_invisible(character: char) -> bool {
    matches!(
        character,
        '\u{061C}'
            | '\u{200B}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2069}'
            | '\u{FEFF}'
            | '\u{E0000}'..='\u{E007F}'
    )
}

/// What one transcript line is called. It has no `Debug`: a label may be a
/// person's name, and names stay out of logs.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum Label<'a> {
    /// The local microphone.
    You,
    /// Remote speech nobody has been named for.
    Them,
    /// The single track of an imported audio file.
    Speaker,
    /// A remote speaker, by a name from the meeting's table.
    Named(&'a SpeakerName),
}

impl<'a> Label<'a> {
    /// The rule. Remote speech takes the name of its speaker when there is
    /// one; everything else is called what its track is called, and an
    /// imported file's one track is `Speaker`. The microphone is always the
    /// local user, whatever a line claims.
    pub fn of(source: MeetingSource, name: Option<&'a SpeakerName>, tracks: Tracks) -> Self {
        match (source, name, tracks) {
            (MeetingSource::System, Some(name), _) => Self::Named(name),
            (_, _, Tracks::Imported) => Self::Speaker,
            (MeetingSource::Microphone, _, Tracks::Call) => Self::You,
            (MeetingSource::System, None, Tracks::Call) => Self::Them,
        }
    }

    /// The track a rendered label, such as a Markdown turn heading, was
    /// heard on: `You` is the microphone, and everything else, a name
    /// included, is remote speech.
    #[cfg(any(debug_assertions, test))]
    pub fn heard_on(rendered: &str) -> MeetingSource {
        if rendered == Self::You.as_str() {
            MeetingSource::Microphone
        } else {
            MeetingSource::System
        }
    }

    pub fn as_str(self) -> &'a str {
        match self {
            Self::You => "You",
            Self::Them => "Them",
            Self::Speaker => "Speaker",
            Self::Named(name) => name.as_str(),
        }
    }

    /// The speaker's name, when the line has one.
    pub fn name(self) -> Option<&'a SpeakerName> {
        match self {
            Self::Named(name) => Some(name),
            Self::You | Self::Them | Self::Speaker => None,
        }
    }
}

impl fmt::Display for Label<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What the lines of one meeting are called. Everything that prints a
/// transcript asks here.
#[derive(Clone, Copy, Debug)]
pub struct Labels<'a> {
    speakers: &'a Speakers,
    tracks: Tracks,
}

impl<'a> Labels<'a> {
    /// `You` and `Them` by track, with the names `speakers` knows, even for
    /// an imported file: what prompts, subtitles and the meeting folder's own
    /// `transcript.md` have always said.
    pub fn by_track(speakers: &'a Speakers) -> Self {
        Self {
            speakers,
            tracks: Tracks::Call,
        }
    }

    /// An imported audio file's labels: its one track is `Speaker`.
    pub fn imported(speakers: &'a Speakers) -> Self {
        Self {
            speakers,
            tracks: Tracks::Imported,
        }
    }

    pub fn of(self, entry: &TranscriptEntry) -> Label<'a> {
        let name = entry.speaker.as_ref().and_then(|id| self.speakers.name(id));
        Label::of(entry.source, name, self.tracks)
    }

    /// What a track's lines are called when nobody is named for them.
    pub fn track(self, source: MeetingSource) -> Label<'a> {
        Label::of(source, None, self.tracks)
    }

    /// Every label a reader of this meeting can meet: the tracks', then each
    /// named speaker.
    pub fn everyone(self) -> impl Iterator<Item = &'a str> {
        let tracks: &[Label<'a>] = match self.tracks {
            Tracks::Imported => &[Label::Speaker],
            Tracks::Call => &[Label::You, Label::Them],
        };
        let tracks = tracks.iter().map(|label| label.as_str());
        tracks.chain(self.speakers.names())
    }
}

impl Default for Labels<'static> {
    /// By track, with nobody named: a recording still under way, and the
    /// chapter prompts, where a name would tell the model nothing about
    /// where a topic changes.
    fn default() -> Self {
        Self::by_track(&NOBODY)
    }
}

impl MeetingManifest {
    /// What this meeting's lines are called where the user reads them: its
    /// speakers by name, and `Speaker` for an imported file's one track.
    pub fn labels(&self) -> Labels<'_> {
        if self.imported {
            Labels::imported(&self.speakers)
        } else {
            Labels::by_track(&self.speakers)
        }
    }
}

#[cfg(test)]
mod resolve_tests;
#[cfg(test)]
mod tests;

//! Who said a transcript line, and what a reader sees it called.
//!
//! A meeting keeps a small table of the remote speakers it could name, and a
//! transcript entry may point at one of them. [`Labels::of`] is the one
//! place that turns an entry into the word shown beside its text: the
//! speaker's name when the table knows the entry's speaker, and otherwise
//! `You`, `Them` or `Speaker`, exactly as before speakers existed.
//!
//! Names are untrusted. Other people on a call choose them, and they end up
//! in Markdown, in prompts and in what agents read. So a name is cleaned on
//! its way into the table (when it is added, and again when a table is read
//! from disk), and it never reaches a log: [`Speaker`] prints without it.
//!
//! Nothing produces a speaker yet. Until something does, every table is
//! empty and every line is called what it was always called.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::{MeetingManifest, MeetingSource, TranscriptEntry};

/// The longest name the table keeps, in characters.
const NAME_CHARS: usize = 64;

/// The characters that give Markdown and markup their structure. A name
/// sits inside `**…**` headings, `<v …>` subtitle tags and prompt lines.
const MARKUP: [char; 7] = ['*', '`', '[', ']', '<', '>', '\\'];

/// The table of a meeting nobody has been named in.
static NOBODY: Speakers = Speakers(Vec::new());

/// A speaker's id inside one meeting: short and opaque (`s1`, `s2`), minted
/// by that meeting's table and meaningless outside it. A call app's own user
/// id is never one, and never reaches the disk.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SpeakerId(String);

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
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct Speaker {
    id: SpeakerId,
    /// Untrusted text from someone else on the call, already cleaned.
    name: String,
    from: NameSource,
}

impl fmt::Debug for Speaker {
    /// Without the name, so that no log line or panic message can carry one.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Speaker")
            .field("id", &self.id)
            .field("from", &self.from)
            .finish_non_exhaustive()
    }
}

/// A meeting's speaker table. Every name in it has been cleaned, and no two
/// speakers share an id.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(from = "Vec<Speaker>")]
pub struct Speakers(Vec<Speaker>);

impl Speakers {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Names a new speaker and returns the id their lines carry, or `None`
    /// when cleaning leaves nothing usable of `name`.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "nothing names a speaker until the resolver is wired in"
        )
    )]
    pub fn add(&mut self, name: &str, from: NameSource) -> Option<SpeakerId> {
        let id = (1_usize..)
            .map(|number| SpeakerId(format!("s{number}")))
            .find(|id| self.name(id).is_none())?;
        self.insert(id.clone(), name, from).then_some(id)
    }

    /// The name `id` goes by in this meeting, if the table knows it.
    pub fn name(&self, id: &SpeakerId) -> Option<&str> {
        self.0
            .iter()
            .find(|speaker| speaker.id == *id)
            .map(|speaker| speaker.name.as_str())
    }

    /// Every name, in the order the speakers were added.
    fn names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|speaker| speaker.name.as_str())
    }

    fn insert(&mut self, id: SpeakerId, name: &str, from: NameSource) -> bool {
        let Some(name) = clean_name(name).filter(|_| self.name(&id).is_none()) else {
            return false;
        };
        self.0.push(Speaker { id, name, from });
        true
    }
}

impl From<Vec<Speaker>> for Speakers {
    /// A table read from disk is held to the same rules as one built here:
    /// the file may have been edited, or written by something else. A row
    /// that breaks them is dropped, and its lines fall back to `Them`.
    fn from(stored: Vec<Speaker>) -> Self {
        let mut speakers = Self::default();
        for speaker in stored {
            speakers.insert(speaker.id, &speaker.name, speaker.from);
        }
        speakers
    }
}

/// A display name made safe to show and to hand to a language model: one
/// line, single spaces, nothing invisible, no [`MARKUP`], at most
/// [`NAME_CHARS`] characters. `None` when nothing is left, or when what is
/// left is one of Hex's own labels, so that nobody on a call can make their
/// words read as `You`.
fn clean_name(name: &str) -> Option<String> {
    let visible = name
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
        .then(|| name.to_owned())
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
    Named(&'a str),
}

impl<'a> Label<'a> {
    /// The rule. Remote speech takes the name of its speaker when there is
    /// one; everything else is called what its track is called, and an
    /// imported file's one track is `Speaker`. The microphone is always the
    /// local user, whatever a line claims.
    pub fn of(source: MeetingSource, name: Option<&'a str>, imported: bool) -> Self {
        match (source, name) {
            (MeetingSource::System, Some(name)) => Self::Named(name),
            _ if imported => Self::Speaker,
            (MeetingSource::Microphone, _) => Self::You,
            (MeetingSource::System, None) => Self::Them,
        }
    }

    /// Reads back a label Hex rendered, such as a Markdown turn heading.
    #[cfg(any(debug_assertions, test))]
    pub fn parse(text: &'a str) -> Self {
        match text {
            "You" => Self::You,
            "Them" => Self::Them,
            "Speaker" => Self::Speaker,
            name => Self::Named(name),
        }
    }

    pub fn as_str(self) -> &'a str {
        match self {
            Self::You => "You",
            Self::Them => "Them",
            Self::Speaker => "Speaker",
            Self::Named(name) => name,
        }
    }

    /// The speaker's name, when the line has one.
    pub fn name(self) -> Option<&'a str> {
        match self {
            Self::Named(name) => Some(name),
            Self::You | Self::Them | Self::Speaker => None,
        }
    }

    /// The track a line with this label was heard on.
    #[cfg(any(debug_assertions, test))]
    pub fn source(self) -> MeetingSource {
        match self {
            Self::You => MeetingSource::Microphone,
            Self::Them | Self::Speaker | Self::Named(_) => MeetingSource::System,
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
    imported: bool,
}

impl<'a> Labels<'a> {
    /// `You` and `Them` by track, with the names `speakers` knows, even for
    /// an imported file: what prompts, subtitles and the meeting folder's own
    /// `transcript.md` have always said.
    pub fn by_track(speakers: &'a Speakers) -> Self {
        Self {
            speakers,
            imported: false,
        }
    }

    /// An imported audio file's labels: its one track is `Speaker`.
    pub fn imported(speakers: &'a Speakers) -> Self {
        Self {
            speakers,
            imported: true,
        }
    }

    pub fn of(self, entry: &TranscriptEntry) -> Label<'a> {
        let name = entry.speaker.as_ref().and_then(|id| self.speakers.name(id));
        Label::of(entry.source, name, self.imported)
    }

    /// What a track's lines are called when nobody is named for them.
    pub fn track(self, source: MeetingSource) -> Label<'a> {
        Label::of(source, None, self.imported)
    }

    /// Every label a reader of this meeting can meet: the tracks', then each
    /// named speaker.
    pub fn everyone(self) -> impl Iterator<Item = &'a str> {
        let tracks: &[Label<'a>] = if self.imported {
            &[Label::Speaker]
        } else {
            &[Label::You, Label::Them]
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
mod tests;

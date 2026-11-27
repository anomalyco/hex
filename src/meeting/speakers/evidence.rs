//! What Hex saw of who was speaking, kept beside the meeting.
//!
//! A call app shows who is on the call and marks who is speaking. A reader
//! (which nothing starts yet) notes what it sees while a meeting records:
//! who is there, and at each look who holds the floor. Those notes
//! are the evidence, and names are worked out from them afterwards, so a
//! better rule or a correction from the user renames a meeting without
//! recording it again.
//!
//! Every time is in milliseconds from the start of the recording, on the
//! same clock as the transcript's lines.
//!
//! The evidence is the one record of who spoke. A meeting's speaker table
//! and the speaker on each transcript line are worked out from it, whole,
//! every time, and are never edited by hand; what the user corrects is the
//! evidence.
//!
//! Where it is kept is this module's business: [`read`], [`append`] and
//! [`correct`] take a meeting's folder, and nothing else knows the file's
//! name, its lines, or what a key looks like. Today that is one JSON object
//! a line in a file readable by its owner only, which goes to the Trash
//! with the folder. A line this build cannot read is skipped and kept:
//! losing a look at the floor can only take a name away.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::num::NonZeroU32;
use std::path::Path;

use color_eyre::eyre::Result;
use serde::{Deserialize, Deserializer, Serialize};

use super::SpeakerName;
use crate::durable::sync_directory;
use crate::meeting::set_owner_only;

/// The evidence file in a meeting's folder.
const FILE: &str = "speakers.ndjson";

/// A stretch of the recording. It never ends before it starts: one made
/// the wrong way round, here or on disk, is turned round.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(from = "Saved")]
pub struct Interval {
    start_ms: u64,
    end_ms: u64,
}

/// An interval as it is saved, before it is known to run forwards.
#[derive(Deserialize)]
struct Saved {
    start_ms: u64,
    end_ms: u64,
}

impl From<Saved> for Interval {
    fn from(saved: Saved) -> Self {
        Self::new(saved.start_ms, saved.end_ms)
    }
}

impl Interval {
    pub fn new(one_end_ms: u64, other_end_ms: u64) -> Self {
        Self {
            start_ms: one_end_ms.min(other_end_ms),
            end_ms: one_end_ms.max(other_end_ms),
        }
    }

    pub fn start_ms(self) -> u64 {
        self.start_ms
    }

    pub fn end_ms(self) -> u64 {
        self.end_ms
    }

    /// The moment a word is judged at.
    pub fn middle_ms(self) -> u64 {
        self.start_ms + (self.end_ms - self.start_ms) / 2
    }

    pub fn contains(self, ms: u64) -> bool {
        (self.start_ms..=self.end_ms).contains(&ms)
    }
}

/// Someone on the call, inside one meeting's evidence: short and opaque
/// (`p1`, `p2`), numbered in the order the reader first saw them. A call
/// app's own user id is never a key: text of any other shape does not read
/// as one, so a line carrying it is not evidence.
#[derive(Clone, Copy, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct ParticipantKey(NonZeroU32);

impl ParticipantKey {
    /// The key for someone `evidence` has not met: one past the highest it
    /// holds.
    #[cfg_attr(
        not(target_os = "macos"),
        allow(dead_code, reason = "the reader is macOS only")
    )]
    pub fn next(evidence: &[Evidence]) -> Self {
        let highest = evidence.iter().filter_map(|line| match line {
            Evidence::Participant(participant) => Some(participant.key),
            _ => None,
        });
        highest.max().map_or(Self(NonZeroU32::MIN), Self::following)
    }

    /// The key after this one, for the next person met.
    #[cfg_attr(
        not(target_os = "macos"),
        allow(dead_code, reason = "the reader is macOS only")
    )]
    pub fn following(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    #[cfg(test)]
    pub(super) fn numbered(number: u32) -> Self {
        Self(NonZeroU32::new(number).unwrap())
    }
}

impl fmt::Display for ParticipantKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "p{}", self.0)
    }
}

impl fmt::Debug for ParticipantKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self}")
    }
}

impl TryFrom<String> for ParticipantKey {
    type Error = &'static str;

    fn try_from(saved: String) -> Result<Self, Self::Error> {
        let number = saved
            .strip_prefix('p')
            .and_then(|number| number.parse().ok());
        number
            .map(Self)
            .filter(|key| key.to_string() == saved)
            .ok_or("a participant key is p and a number")
    }
}

impl From<ParticipantKey> for String {
    fn from(key: ParticipantKey) -> Self {
        key.to_string()
    }
}

/// Someone the call app showed on the call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Participant {
    pub key: ParticipantKey,
    /// What the call app called them, cleaned. Nobody when it showed
    /// nothing Hex can use as a name; they still count as someone on the
    /// call, and their lines stay `Them`.
    #[serde(deserialize_with = "shown_name")]
    pub name: Option<SpeakerName>,
    /// The person using Hex. Their words are on the microphone track, so
    /// they never name a remote line; their tile is what the reader is
    /// checked against.
    pub is_local: bool,
    /// When the reader first saw them as this line has them. Someone the
    /// call app comes to show under another name gets another line.
    pub at_ms: u64,
}

/// A participant's saved name: cleaned like any other, and nobody when
/// nothing usable is left, so the participant is kept either way.
fn shown_name<'de, D: Deserializer<'de>>(saved: D) -> Result<Option<SpeakerName>, D::Error> {
    let saved = Option::<String>::deserialize(saved)?;
    Ok(saved.as_deref().and_then(SpeakerName::parse))
}

/// Why a look at the call window says nothing about who held the floor.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Untrusted {
    /// The look ran out of time or nodes before it saw every tile.
    CutShort,
    /// The call window was hidden or minimised, so what it showed was stale.
    WindowHidden,
    /// Two windows could have been the call, and they disagreed.
    TwoWindows,
    /// The user's own tile was not tracking the user's own microphone.
    FailedHealthCheck,
}

/// Something the user said about who spoke. It outranks everything Hex saw.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "correction", rename_all = "snake_case")]
pub enum Correction {
    /// The remote words in this stretch were this person's, or, with
    /// nobody, are not to be named ("Not sure").
    Said {
        during: Interval,
        by: Option<ParticipantKey>,
    },
    /// This person goes by this name in this meeting.
    Named {
        key: ParticipantKey,
        name: SpeakerName,
    },
}

impl Correction {
    /// Whether this correction leaves nothing of an `earlier` one standing.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "nothing records a correction yet")
    )]
    fn replaces(&self, earlier: &Self) -> bool {
        match (self, earlier) {
            (
                Self::Said { during, .. },
                Self::Said {
                    during: earlier, ..
                },
            ) => during.start_ms <= earlier.start_ms && earlier.end_ms <= during.end_ms,
            (Self::Named { key, .. }, Self::Named { key: earlier, .. }) => key == earlier,
            _ => false,
        }
    }
}

/// One line of the evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Evidence {
    /// The call app showed this person on the call.
    Participant(Participant),
    /// One look at the call window that could be trusted: who it marked as
    /// speaking, or nobody.
    Floor {
        at_ms: u64,
        holder: Option<ParticipantKey>,
    },
    /// One look that could not be trusted. It counts as no look at all.
    Untrusted {
        at_ms: u64,
        reason: Untrusted,
    },
    Correction(Correction),
}

/// A meeting's evidence, oldest line first; empty when the meeting has
/// none. A line cut short by a crash, or written by a build that knew more
/// kinds of line, is left out.
pub fn read(directory: &Path) -> Result<Vec<Evidence>> {
    Ok(complete_lines(&stored(directory)?)
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect())
}

/// Adds what the reader saw to the end of the file, creating it for its
/// owner only.
#[cfg_attr(
    not(target_os = "macos"),
    allow(dead_code, reason = "the reader is macOS only")
)]
pub fn append(directory: &Path, evidence: &[Evidence]) -> Result<()> {
    let path = directory.join(FILE);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(&path)?;
    let mut lines = Vec::new();
    match file.metadata()?.len() {
        0 => set_owner_only(&path, false)?,
        _ => {
            // A crash can leave half a line. Close it, so that it spoils
            // only itself.
            let mut last = [0_u8];
            file.seek(SeekFrom::End(-1))?;
            file.read_exact(&mut last)?;
            if last != *b"\n" {
                lines.push(b'\n');
            }
        }
    }
    for line in evidence {
        serde_json::to_writer(&mut lines, line)?;
        lines.push(b'\n');
    }
    file.write_all(&lines)?;
    Ok(())
}

/// Records a correction from the user, dropping the earlier corrections it
/// replaces. The file is rewritten whole under a temporary name and renamed
/// into place, so a crash leaves either the old evidence or the new. Lines
/// this build cannot read are carried over untouched.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "nothing records a correction yet")
)]
pub fn correct(directory: &Path, correction: Correction) -> Result<()> {
    let existing = stored(directory)?;
    let temporary = directory.join(format!("{FILE}.tmp"));
    let mut file = BufWriter::new(File::create(&temporary)?);
    set_owner_only(&temporary, false)?;
    for line in complete_lines(&existing) {
        if let Ok(Evidence::Correction(earlier)) = serde_json::from_slice(line)
            && correction.replaces(&earlier)
        {
            continue;
        }
        file.write_all(line)?;
        file.write_all(b"\n")?;
    }
    serde_json::to_writer(&mut file, &Evidence::Correction(correction))?;
    file.write_all(b"\n")?;
    file.flush()?;
    file.get_ref().sync_all()?;
    fs::rename(&temporary, directory.join(FILE))?;
    Ok(sync_directory(directory)?)
}

fn stored(directory: &Path) -> Result<Vec<u8>> {
    match fs::read(directory.join(FILE)) {
        Ok(contents) => Ok(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

/// The lines that end in a newline, without the empty ones.
fn complete_lines(contents: &[u8]) -> impl Iterator<Item = &[u8]> {
    let complete = contents
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(&[][..], |newline| &contents[..newline]);
    complete
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "hex-speaker-evidence-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn key(number: u32) -> ParticipantKey {
        ParticipantKey::numbered(number)
    }

    fn person(number: u32, name: &str, is_local: bool) -> Evidence {
        Evidence::Participant(Participant {
            key: key(number),
            name: SpeakerName::parse(name),
            is_local,
            at_ms: 0,
        })
    }

    fn said(start_ms: u64, end_ms: u64, by: Option<u32>) -> Correction {
        Correction::Said {
            during: Interval::new(start_ms, end_ms),
            by: by.map(key),
        }
    }

    fn a_call() -> Vec<Evidence> {
        vec![
            person(1, "Kit", true),
            person(2, "Priya Nair", false),
            Evidence::Floor {
                at_ms: 500,
                holder: Some(key(2)),
            },
            Evidence::Floor {
                at_ms: 1_000,
                holder: None,
            },
            Evidence::Untrusted {
                at_ms: 1_500,
                reason: Untrusted::WindowHidden,
            },
            Evidence::Correction(said(0, 900, None)),
        ]
    }

    #[test]
    fn evidence_is_one_plain_json_object_a_line() {
        let folder = folder("lines");
        append(&folder, &a_call()).unwrap();
        assert_eq!(
            fs::read_to_string(folder.join(FILE)).unwrap(),
            concat!(
                r#"{"kind":"participant","key":"p1","name":"Kit","is_local":true,"at_ms":0}"#,
                "\n",
                r#"{"kind":"participant","key":"p2","name":"Priya Nair","is_local":false,"at_ms":0}"#,
                "\n",
                r#"{"kind":"floor","at_ms":500,"holder":"p2"}"#,
                "\n",
                r#"{"kind":"floor","at_ms":1000,"holder":null}"#,
                "\n",
                r#"{"kind":"untrusted","at_ms":1500,"reason":"window_hidden"}"#,
                "\n",
                r#"{"kind":"correction","correction":"said","during":{"start_ms":0,"end_ms":900},"by":null}"#,
                "\n",
            )
        );
        assert_eq!(read(&folder).unwrap(), a_call());
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_meeting_with_no_evidence_reads_as_empty() {
        let folder = folder("none");
        assert_eq!(read(&folder).unwrap(), []);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn appending_adds_to_the_end_and_keeps_what_was_there() {
        let folder = folder("append");
        let call = a_call();
        append(&folder, &call[..2]).unwrap();
        append(&folder, &call[2..]).unwrap();
        append(&folder, &[]).unwrap();
        assert_eq!(read(&folder).unwrap(), call);
        fs::remove_dir_all(folder).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_readable_by_its_owner_only_before_and_after_a_correction() {
        use std::os::unix::fs::PermissionsExt;
        let folder = folder("private");
        let mode = || {
            let permissions = fs::metadata(folder.join(FILE)).unwrap().permissions();
            permissions.mode() & 0o777
        };
        append(&folder, &a_call()).unwrap();
        assert_eq!(mode(), 0o600);
        correct(&folder, said(0, 500, Some(2))).unwrap();
        assert_eq!(mode(), 0o600);
        assert!(!folder.join(format!("{FILE}.tmp")).exists());
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_line_cut_short_by_a_crash_spoils_only_itself() {
        let folder = folder("torn");
        let call = a_call();
        append(&folder, &call[..3]).unwrap();
        let mut file = OpenOptions::new()
            .append(true)
            .open(folder.join(FILE))
            .unwrap();
        file.write_all(br#"{"kind":"floor","at_"#).unwrap();
        drop(file);
        assert_eq!(read(&folder).unwrap(), call[..3]);

        append(&folder, &call[3..]).unwrap();
        assert_eq!(read(&folder).unwrap(), call);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_line_from_a_newer_build_is_skipped_and_survives_a_correction() {
        let folder = folder("newer");
        let newer = r#"{"kind":"captions","at_ms":700,"text":"hello"}"#;
        fs::write(
            folder.join(FILE),
            format!("{newer}\n{{\"kind\":\"floor\",\"at_ms\":500,\"holder\":null}}\n"),
        )
        .unwrap();
        let floor = Evidence::Floor {
            at_ms: 500,
            holder: None,
        };
        assert_eq!(read(&folder).unwrap(), std::slice::from_ref(&floor));

        let correction = said(0, 900, Some(2));
        correct(&folder, correction.clone()).unwrap();
        assert_eq!(
            read(&folder).unwrap(),
            [floor, Evidence::Correction(correction)]
        );
        let kept = fs::read_to_string(folder.join(FILE)).unwrap();
        assert!(kept.starts_with(newer), "{kept}");
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_correction_replaces_the_ones_it_covers_and_keeps_the_rest() {
        let folder = folder("correct");
        let renamed = |name: &str| Correction::Named {
            key: key(2),
            name: SpeakerName::parse(name).unwrap(),
        };
        correct(&folder, said(1_000, 2_000, Some(2))).unwrap();
        correct(&folder, said(5_000, 6_000, Some(3))).unwrap();
        correct(&folder, renamed("Priya")).unwrap();
        // Wider than the first, so the first is gone; the second stands.
        correct(&folder, said(500, 2_500, None)).unwrap();
        correct(&folder, renamed("Priya N.")).unwrap();
        assert_eq!(
            read(&folder).unwrap(),
            [
                said(5_000, 6_000, Some(3)),
                said(500, 2_500, None),
                renamed("Priya N."),
            ]
            .map(Evidence::Correction)
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_key_is_p_and_a_number_and_nothing_else() {
        let read = |key: &str| serde_json::from_str::<ParticipantKey>(&format!("\"{key}\""));
        assert_eq!(read("p12").unwrap(), key(12));
        assert_eq!(serde_json::to_string(&key(12)).unwrap(), r#""p12""#);
        for other in [
            "U024BE7LH",
            "p",
            "p0",
            "p01",
            "P1",
            "p1x",
            "p+1",
            "p99999999999",
            "",
        ] {
            assert!(read(other).is_err(), "{other}");
        }
        // So a line carrying a call app's own id is not evidence at all.
        let line = r#"{"kind":"floor","at_ms":500,"holder":"U024BE7LH"}"#;
        assert!(serde_json::from_str::<Evidence>(line).is_err());
    }

    #[test]
    fn keys_are_handed_out_in_the_order_people_are_first_seen() {
        assert_eq!(ParticipantKey::next(&[]), key(1));
        assert_eq!(key(1).following(), key(2));
        let call = [person(1, "Kit", true), person(4, "Priya Nair", false)];
        assert_eq!(ParticipantKey::next(&call), key(5));
    }

    #[test]
    fn a_saved_name_is_cleaned_coming_in_and_never_printed() {
        let participant = |name: &str| {
            let line = format!(
                r#"{{"kind":"participant","key":"p2","name":{name},"is_local":false,"at_ms":0}}"#
            );
            serde_json::from_str::<Evidence>(&line).unwrap()
        };
        let hostile = serde_json::to_string("  Priya\n**Nair**\u{202E} ").unwrap();
        let stored = participant(&hostile);
        assert_eq!(stored, person(2, "Priya Nair", false));
        assert!(!format!("{stored:?}").contains("Priya"));

        // Someone the call app gave no usable name is still someone on the
        // call: with them there, nobody else is the only remote person.
        for unusable in [r#""You""#, r#""  ""#, "null"] {
            assert_eq!(participant(unusable), person(2, "", false), "{unusable}");
        }

        // A rename to nothing usable is no correction at all.
        let rename = |name: &str| {
            format!(r#"{{"kind":"correction","correction":"named","key":"p2","name":"{name}"}}"#)
        };
        assert!(serde_json::from_str::<Evidence>(&rename("Them")).is_err());
        let renamed: Evidence = serde_json::from_str(&rename("Priya")).unwrap();
        assert!(!format!("{renamed:?}").contains("Priya"));
    }

    #[test]
    fn an_interval_runs_forwards_however_it_was_given() {
        let word = Interval::new(1_000, 1_400);
        assert_eq!(Interval::new(1_400, 1_000), word);
        assert_eq!((word.start_ms(), word.end_ms()), (1_000, 1_400));
        assert_eq!(word.middle_ms(), 1_200);
        assert!(word.contains(1_000) && word.contains(1_400) && !word.contains(1_401));
        let saved = r#"{"start_ms":1400,"end_ms":1000}"#;
        assert_eq!(serde_json::from_str::<Interval>(saved).unwrap(), word);
    }
}

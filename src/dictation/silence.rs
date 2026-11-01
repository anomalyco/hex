//! A dictation discarded for silence is never discarded silently (#782): the
//! error tone plays, a short note shows beside the HUD, the log has the
//! measured level against the thresholds ([`super::DictationClip::silence`]),
//! and Settings' status keeps a line about it until a dictation goes through.

use std::sync::Mutex;
use std::time::Duration;

use crate::desktop::i18n::tr;
use crate::transcription::engine::Partial;

/// How long the note stays beside the HUD.
const NOTE: Duration = Duration::from_millis(2500);

/// Why a capture held no speech worth transcribing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Silence {
    /// Its level stayed below the silence threshold.
    TooQuiet,
    /// It was loud enough, but the speech detector heard no speech in it.
    NoSpeech,
}

impl Silence {
    /// The short note shown beside the HUD and in Settings' status.
    pub(crate) fn note(self) -> &'static str {
        match self {
            Self::TooQuiet => tr("too quiet, check your mic level"),
            Self::NoSpeech => tr("no speech heard, check your mic level"),
        }
    }
}

/// The last dictation discarded for silence, until one goes through.
static LAST: Mutex<Option<Silence>> = Mutex::new(None);

/// Plays the error tone, shows the note beside the HUD, and keeps it for
/// Settings' status.
pub(crate) fn announce(silence: Silence) {
    crate::feedback::play(crate::feedback::Tone::Error);
    *LAST.lock().unwrap_or_else(|error| error.into_inner()) = Some(silence);
    let note = super::live_text::Publisher::new();
    note.show(&Partial {
        stable: silence.note().to_string(),
        tentative: String::new(),
    });
    // Dropping the publisher clears the note, unless a newer recording's
    // text replaced it meanwhile.
    let _ = std::thread::Builder::new()
        .name("hex-silence-note".into())
        .spawn(move || {
            std::thread::sleep(NOTE);
            drop(note);
        });
}

/// A dictation went through: Settings stops showing the discard.
pub(crate) fn clear() {
    *LAST.lock().unwrap_or_else(|error| error.into_inner()) = None;
}

/// The last dictation discarded for silence, if none has gone through since.
pub(crate) fn last() -> Option<Silence> {
    *LAST.lock().unwrap_or_else(|error| error.into_inner())
}

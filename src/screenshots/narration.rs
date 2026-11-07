//! Labs › Screenshots: talking over a shot. A dictation that starts while
//! the editor or the thumbnail shows talks over that shot: its words go to
//! the shot instead of being pasted. In the editor they show under the shot
//! and go with it to a session; over the card, they open the shot in the
//! editor. The dictation itself is the shortcut's own, on the loaded model
//! and through the same processing; only where its text lands changes.
//!
//! The listener calls [`started`], [`claim`], and [`ended`] from its own
//! thread at the dictation's start, finish, and discard or cancel.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::control::{CaptureAction, PrintReply, PrintedTranscript};
use crate::rune_ui::screenshot_thumbnail::{self as card, editor};
use editor::{Listen, Talk};

/// The shot a dictation talks over.
#[derive(Clone, Debug, PartialEq)]
enum Over {
    Editor,
    Card(PathBuf),
}

impl Over {
    /// The shot that shows now, the editor's before the card's.
    fn showing() -> Option<Self> {
        if editor::is_open() {
            Some(Self::Editor)
        } else {
            card::showing().map(Self::Card)
        }
    }

    fn tell(&self, listen: Listen) {
        match self {
            Self::Editor => editor::listen(listen),
            Self::Card(_) => card::listen(listen),
        }
    }

    /// The dictation's words, or none when it had none or failed.
    fn hear(self, words: Option<String>) {
        match (self, words) {
            (Self::Editor, words) => editor::heard(words),
            (Self::Card(path), Some(words)) => card::heard(path, words),
            (Self::Card(_), None) => card::listen(Listen::Idle),
        }
    }
}

/// The shot the dictation now recording talks over.
static OVER: Mutex<Option<Over>> = Mutex::new(None);

fn over() -> MutexGuard<'static, Option<Over>> {
    OVER.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A dictation started: with the Lab on, it talks over the shot that shows,
/// if one does.
pub(crate) fn started() {
    let now = crate::labs::screenshots().then(Over::showing).flatten();
    if let Some(now) = &now {
        now.tell(Listen::Recording);
    }
    if let Some(before) = std::mem::replace(&mut *over(), now.clone())
        && Some(&before) != now.as_ref()
    {
        before.tell(Listen::Idle);
    }
}

/// The dictation was discarded or cancelled: nothing lands.
pub(crate) fn ended() {
    if let Some(before) = over().take() {
        before.tell(Listen::Idle);
    }
}

/// The dictation finished: the reply that carries its words to the shot it
/// talks over, or `None` to paste them as usual. A dropped reply (a short,
/// silent, cancelled, or failed clip) lands nothing.
pub(crate) fn claim() -> Option<PrintReply> {
    let over = over().take()?;
    over.tell(Listen::Transcribing);
    let (reply, printed) = PrintReply::channel();
    let spawned = std::thread::Builder::new()
        .name("hex-screenshot-narration".into())
        .spawn(move || {
            let words = match printed.recv_timeout(crate::control::PRINT_WAIT) {
                Ok(PrintedTranscript::Text(words)) => Some(words),
                Ok(PrintedTranscript::Failed(error)) => {
                    tracing::warn!(%error, "the words over the screenshot failed");
                    None
                }
                _ => None,
            };
            over.hear(words);
        });
    match spawned {
        Ok(_) => Some(reply),
        Err(error) => {
            tracing::warn!(%error, "could not wait for the words over the screenshot");
            None
        }
    }
}

/// The editor's microphone and Escape, through the listener as `hex
/// dictation` goes, so the shortcut and the chip finish each other's.
pub(crate) fn talk(talk: Talk) {
    let Some(runtime) = crate::macos_rune::runtime() else {
        return;
    };
    let action = match talk {
        Talk::Toggle => CaptureAction::Toggle,
        Talk::Cancel => CaptureAction::Cancel,
    };
    // The listener answers a reply nobody waits for.
    let (reply, _) = std::sync::mpsc::sync_channel(1);
    if let Err(error) = runtime.recognition(crate::recognition::RecognitionControl::Capture {
        action,
        print: None,
        raw: false,
        reply,
    }) {
        tracing::warn!(%error, "could not reach the listener for the screenshot");
    }
}

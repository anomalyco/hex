//! Labs › Screenshots: talking over a shot. A dictation that starts while a
//! shot's editor is open talks over that shot: its words show under the shot
//! and go with it to a session instead of being pasted. The corner card that
//! shows a fresh shot never takes words: a dictation while it shows pastes as
//! usual, so the shortcut behaves the way it did before the Lab.
//!
//! The dictation itself is the shortcut's own, on the loaded model and
//! through the same processing; only where its text lands changes.
//!
//! The listener calls [`started`], [`claim`], and [`ended`] from its own
//! thread at the dictation's start, finish, and discard or cancel.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::control::{CaptureAction, PrintReply, PrintedTranscript};
use crate::rune_ui::screenshot_thumbnail::editor;
use editor::{Listen, Talk};

/// Whether the dictation now recording talks over the open shot.
static OVER: AtomicBool = AtomicBool::new(false);

/// A dictation started: with the Lab on and a shot's editor open, it talks
/// over that shot.
pub(crate) fn started() {
    let talking = crate::labs::screenshots() && editor::is_open();
    let before = OVER.swap(talking, Ordering::SeqCst);
    if talking {
        editor::listen(Listen::Recording);
    } else if before {
        editor::listen(Listen::Idle);
    }
}

/// The dictation was discarded or cancelled: nothing lands.
pub(crate) fn ended() {
    if OVER.swap(false, Ordering::SeqCst) {
        editor::listen(Listen::Idle);
    }
}

/// The dictation finished: the reply that carries its words to the open
/// shot, or `None` to paste them as usual. A dropped reply (a short, silent,
/// cancelled, or failed clip) lands nothing.
pub(crate) fn claim() -> Option<PrintReply> {
    if !OVER.swap(false, Ordering::SeqCst) {
        return None;
    }
    editor::listen(Listen::Transcribing);
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
            editor::heard(words);
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

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

use std::sync::{Arc, Mutex};

use crate::control::{CaptureAction, PrintReply, PrintedTranscript};
use crate::rune_ui::screenshot_thumbnail::editor;
use editor::{Listen, Talk};

/// Whether the dictation now recording talks over the open shot.
static OVER: Mutex<Option<Arc<editor::Narration>>> = Mutex::new(None);

/// A dictation started: with the Lab on and a shot's editor open, it talks
/// over that shot.
pub(crate) fn started() {
    let request = crate::labs::screenshots().then(editor::narration).flatten();
    *OVER.lock().unwrap_or_else(|e| e.into_inner()) = request.clone();
    if let Some(request) = request {
        editor::listen(request, Listen::Recording);
    }
}

/// The dictation was discarded or cancelled: nothing lands.
pub(crate) fn ended() {
    if let Some(request) = OVER.lock().unwrap_or_else(|e| e.into_inner()).take() {
        editor::listen(request, Listen::Idle);
    }
}

/// The dictation finished: the reply that carries its words to the open
/// shot, or `None` to paste them as usual. A dropped reply (a short, silent,
/// cancelled, or failed clip) lands nothing.
pub(crate) fn claim() -> Option<PrintReply> {
    let request = OVER.lock().unwrap_or_else(|e| e.into_inner()).take()?;
    if !editor::can_claim(&request) {
        return None;
    }
    editor::listen(request.clone(), Listen::Transcribing);
    let (reply, printed) = PrintReply::channel();
    let receiving = request.clone();
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
            editor::heard(receiving, words);
        });
    match spawned {
        Ok(_) => Some(reply),
        Err(error) => {
            tracing::warn!(%error, "could not wait for the words over the screenshot");
            editor::listen(request, Listen::Idle);
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

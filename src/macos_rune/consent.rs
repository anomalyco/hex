//! The macOS app's side of browser connect: the window shows one web page's
//! ask, and the local API's worker waits for the answer.
//!
//! The local API thread blocks in [`Desk::ask`] until the person presses a
//! button in the window, or a minute passes. The runtime thread reads
//! [`Desk::prompt`] into the state it publishes, and the window answers with
//! [`Desk::answer`]; nothing about the page's request touches the microphone,
//! Accessibility, or any other desktop authority.

use std::sync::Mutex;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use crate::local_api::{Answer, ConsentGate, Prompt};

/// What the window shows, and where its answer goes.
struct Pending {
    prompt: Prompt,
    answer: Sender<Answer>,
}

#[derive(Default)]
struct State {
    pending: Option<Pending>,
    next_id: u64,
}

/// The one dialog the app can show. A page's ask is published from here, so
/// every window and menu reads the same pending ask.
#[derive(Default)]
pub(crate) struct Desk {
    state: Mutex<State>,
}

impl Desk {
    /// The ask the window should draw, if one is waiting.
    pub(crate) fn prompt(&self) -> Option<Prompt> {
        lock(&self.state)
            .pending
            .as_ref()
            .map(|pending| pending.prompt.clone())
    }

    /// Answers the ask `id`, if it is still the pending one. Escape and the
    /// scrim answer Deny the same way the buttons do.
    pub(crate) fn answer(&self, id: u64, answer: Answer) -> bool {
        let mut state = lock(&self.state);
        let Some(pending) = state.pending.as_ref() else {
            return false;
        };
        if pending.prompt.id != id {
            return false;
        }
        let pending = state.pending.take().expect("checked just above");
        pending.answer.send(answer).is_ok()
    }
}

impl ConsentGate for Desk {
    fn ask(&self, prompt: &Prompt, wait: Duration) -> Answer {
        let (answer, answered) = mpsc::channel();
        let id = {
            let mut state = lock(&self.state);
            if state.pending.is_some() {
                // The local API allows one ask at a time; a second gate call
                // is a bug, not a second dialog.
                return Answer::Unavailable;
            }
            state.next_id += 1;
            let id = state.next_id;
            state.pending = Some(Pending {
                prompt: Prompt {
                    id,
                    intent: prompt.intent,
                    recipient: prompt.recipient.clone(),
                },
                answer,
            });
            id
        };
        let answer = match answered.recv_timeout(wait) {
            Ok(answer) => answer,
            Err(_) => Answer::Timeout,
        };
        // A timeout leaves the dialog unanswerable, so it goes away; a button
        // that answered first already cleared it.
        let mut state = lock(&self.state);
        if state
            .pending
            .as_ref()
            .is_some_and(|pending| pending.prompt.id == id)
        {
            state.pending = None;
        }
        answer
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    fn prompt() -> Prompt {
        Prompt {
            id: 0,
            intent: crate::local_api::Intent::Transcribe,
            recipient: crate::local_api::Recipient::Web {
                origin: "https://notes.example".into(),
                client_name: Some("Field Notes".into()),
            },
        }
    }

    #[test]
    fn an_allowed_ask_waits_for_the_windows_answer() {
        let desk = Arc::new(Desk::default());
        let asking = {
            let desk = desk.clone();
            thread::spawn(move || desk.ask(&prompt(), Duration::from_secs(5)))
        };
        let shown = loop {
            if let Some(shown) = desk.prompt() {
                break shown;
            }
            thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(shown.recipient.label(), "https://notes.example");
        assert!(desk.answer(shown.id, Answer::Once));
        assert_eq!(asking.join().unwrap(), Answer::Once);
        assert!(desk.prompt().is_none());
    }

    #[test]
    fn a_denied_ask_reports_deny_and_a_stale_answer_is_ignored() {
        let desk = Arc::new(Desk::default());
        let asking = {
            let desk = desk.clone();
            thread::spawn(move || desk.ask(&prompt(), Duration::from_secs(5)))
        };
        let shown = loop {
            if let Some(shown) = desk.prompt() {
                break shown;
            }
            thread::sleep(Duration::from_millis(5));
        };
        assert!(!desk.answer(shown.id + 1, Answer::Once));
        assert!(desk.answer(shown.id, Answer::Deny));
        assert_eq!(asking.join().unwrap(), Answer::Deny);
    }

    #[test]
    fn an_unanswered_ask_times_out_and_leaves_nothing_pending() {
        let desk = Desk::default();
        assert_eq!(
            desk.ask(&prompt(), Duration::from_millis(1)),
            Answer::Timeout
        );
        assert!(desk.prompt().is_none());
    }
}

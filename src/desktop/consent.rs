//! The app's side of consent on every OS: Hex's permission window shows one
//! ask, and the local API's worker waits for the answer.
//!
//! The local API thread blocks in [`Desk::ask`] until the person presses a
//! button in the permission window, or a minute passes. The window learns of
//! each ask, and of its end, from [`Desk::watch`], and answers with
//! [`Desk::answer`]; nothing about the request touches the microphone,
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

/// Told the ask to show when one starts, and `None` when it ends (answered
/// or timed out).
type Watcher = Box<dyn Fn(Option<Prompt>) + Send + Sync>;

/// The one dialog the app can show, at most one ask at a time.
#[derive(Default)]
pub(crate) struct Desk {
    state: Mutex<State>,
    watcher: Mutex<Option<Watcher>>,
}

impl Desk {
    /// Hands each ask to `watcher` as it starts, and `None` as it ends.
    pub(crate) fn watch(&self, watcher: impl Fn(Option<Prompt>) + Send + Sync + 'static) {
        *lock(&self.watcher) = Some(Box::new(watcher));
    }

    /// Tells the watcher, outside the state lock, so it may answer at once.
    fn tell(&self, prompt: Option<Prompt>) {
        if let Some(watcher) = lock(&self.watcher).as_ref() {
            watcher(prompt);
        }
    }

    /// The ask the window should draw, if one is waiting.
    #[cfg(test)]
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
        drop(state);
        self.tell(None);
        pending.answer.send(answer).is_ok()
    }
}

impl ConsentGate for Desk {
    fn ask(&self, prompt: &Prompt, wait: Duration) -> Answer {
        let (answer, answered) = mpsc::channel();
        let id = {
            let mut state = lock(&self.state);
            if state.pending.is_some() {
                // One dialog at a time across every route that asks.
                return Answer::Busy;
            }
            state.next_id += 1;
            let id = state.next_id;
            let shown = Prompt {
                id,
                intent: prompt.intent,
                recipient: prompt.recipient.clone(),
            };
            state.pending = Some(Pending {
                prompt: shown.clone(),
                answer,
            });
            drop(state);
            self.tell(Some(shown));
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
            drop(state);
            self.tell(None);
        }
        answer
    }

    fn withdraw(&self, keep: &dyn Fn(&Prompt) -> bool) {
        let mut state = lock(&self.state);
        let Some(pending) = state.pending.take_if(|pending| !keep(&pending.prompt)) else {
            return;
        };
        drop(state);
        self.tell(None);
        let _ = pending.answer.send(Answer::Unavailable);
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
                icon: None,
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

    /// The permission window hears each ask start and end: shown, then
    /// hidden on an answer or a timeout.
    #[test]
    fn the_watcher_hears_each_ask_start_and_end() {
        let desk = Arc::new(Desk::default());
        let heard = Arc::new(Mutex::new(Vec::new()));
        {
            let heard = heard.clone();
            desk.watch(move |prompt| heard.lock().unwrap().push(prompt.map(|p| p.id)));
        }
        assert_eq!(
            desk.ask(&prompt(), Duration::from_millis(1)),
            Answer::Timeout
        );
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
        assert!(desk.answer(shown.id, Answer::Deny));
        assert_eq!(asking.join().unwrap(), Answer::Deny);
        assert_eq!(*heard.lock().unwrap(), [Some(1), None, Some(2), None]);
    }

    /// Labs › Connected apps turned off ends the shown ask at once; an ask
    /// it still allows stays.
    #[test]
    fn a_withdrawn_ask_answers_unavailable_and_hides() {
        let desk = Arc::new(Desk::default());
        let asking = {
            let desk = desk.clone();
            thread::spawn(move || desk.ask(&prompt(), Duration::from_secs(5)))
        };
        while desk.prompt().is_none() {
            thread::sleep(Duration::from_millis(5));
        }
        desk.withdraw(&|_| true);
        assert!(desk.prompt().is_some());
        desk.withdraw(&|_| false);
        assert_eq!(asking.join().unwrap(), Answer::Unavailable);
        assert!(desk.prompt().is_none());
    }

    #[test]
    fn a_second_ask_while_one_shows_is_busy() {
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
        assert_eq!(desk.ask(&prompt(), Duration::from_secs(5)), Answer::Busy);
        assert_eq!(desk.prompt(), Some(shown.clone()), "the first stays");
        assert!(desk.answer(shown.id, Answer::Once));
        assert_eq!(asking.join().unwrap(), Answer::Once);
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

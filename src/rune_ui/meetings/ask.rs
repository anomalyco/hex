//! Labs › Ask meetings: the question field under a meeting's notes, the
//! worker that answers it, and the answer shown below it. The answer is never
//! saved: changing the meeting, asking again, or turning the lab off drops the
//! one on screen. One question runs at a time: Apple Intelligence can't be
//! interrupted, so a second Enter waits rather than stacking a second call
//! beside the first.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::thread::JoinHandle;

use rune::{El, TextEdit, Waker, col, text};
use rune_kit::TextInput;

use super::super::Ui;
use super::super::controls::{DESCRIPTION, Paint};
use super::super::lists::section_label;
use super::Meeting;
use crate::app_settings::MeetingSummaries;
use crate::desktop::i18n::tr;

/// One question's worker: the thread answering it and the meeting it was
/// asked about, so an answer that arrives after the meeting changed is
/// dropped rather than shown for another meeting.
struct Worker {
    meeting: String,
    receiver: Receiver<Result<String, String>>,
    thread: JoinHandle<()>,
}

/// One meeting's question at a time: the field's text, the worker behind it,
/// and what it brought back.
pub(super) struct Ask {
    pub(super) question: TextEdit,
    /// The meeting the shown answer or error belongs to.
    target: Option<String>,
    answer: Option<String>,
    error: Option<String>,
    worker: Option<Worker>,
    /// Set when the shown question is given up, so OpenCode stops. Apple
    /// Intelligence ignores it and runs to its deadline; its answer is then
    /// dropped because the meeting no longer matches.
    cancelled: Option<Arc<AtomicBool>>,
}

impl Default for Ask {
    fn default() -> Self {
        Self {
            question: TextEdit::new(String::new()),
            target: None,
            answer: None,
            error: None,
            worker: None,
            cancelled: None,
        }
    }
}

impl Drop for Ask {
    /// The window going away gives up the question it was waiting on.
    fn drop(&mut self) {
        self.stop();
    }
}

impl Ask {
    /// Whether a question's thread is still alive. Apple Intelligence can't be
    /// interrupted, so the card waits for it before asking another.
    pub(super) fn running(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(|worker| !worker.thread.is_finished())
    }

    /// Starts answering `question` about `meeting`, or says why it can't.
    /// `wake` redraws the window when the answer arrives.
    pub(super) fn submit(
        &mut self,
        meeting: &str,
        question: &str,
        summaries: MeetingSummaries,
        wake: Option<Waker>,
    ) {
        // Take an answer that already arrived first, so a new question
        // replaces it instead of losing it with the old worker.
        let _ = self.poll();
        if self.running() {
            // One call at a time: Apple Intelligence cannot be cancelled, so a
            // second question would run beside the first for up to a minute.
            // Asking about another meeting says why; asking again about this
            // one keeps what shows, "Thinking…" included.
            if self
                .worker
                .as_ref()
                .is_some_and(|worker| worker.meeting != meeting)
            {
                self.target = Some(meeting.to_owned());
                self.answer = None;
                self.error = Some(tr("Wait for the last question to finish").to_owned());
            }
            return;
        }
        self.stop();
        self.target = Some(meeting.to_owned());
        self.answer = None;
        self.error = None;
        let question = question.trim();
        if question.is_empty() {
            return;
        }
        if summaries == MeetingSummaries::Off {
            self.error =
                Some(tr("Choose a writer in Meetings settings to ask about a meeting").to_owned());
            return;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&cancelled);
        let (sender, receiver) = channel();
        let id = meeting.to_owned();
        let asked = question.to_owned();
        let started = std::thread::Builder::new()
            .name("hex-meeting-ask".into())
            .spawn(move || {
                crate::transcription::background_priority::deprioritize_background_thread();
                let result = crate::meeting::ask::answer(&id, &asked, &stopping)
                    .map_err(|error| format!("{error:#}"));
                let _ = sender.send(result);
                if let Some(waker) = &wake {
                    waker();
                }
            });
        match started {
            Ok(thread) => {
                self.worker = Some(Worker {
                    meeting: meeting.to_owned(),
                    receiver,
                    thread,
                });
                self.cancelled = Some(cancelled);
            }
            // A thread Hex can't start is this question's error line, not a
            // reason to take the pane down.
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    /// Takes the answer a worker sent, if one did; true when something
    /// changed on screen.
    pub(super) fn poll(&mut self) -> bool {
        let received = match &self.worker {
            Some(worker) => worker.receiver.try_recv(),
            None => return false,
        };
        match received {
            Ok(result) => {
                let Some(worker) = self.worker.take() else {
                    return false;
                };
                self.cancelled = None;
                if self.target.as_deref() == Some(worker.meeting.as_str()) {
                    match result {
                        Ok(answer) => self.answer = Some(answer),
                        Err(error) => self.error = Some(error),
                    }
                }
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                let Some(worker) = self.worker.take() else {
                    return false;
                };
                self.cancelled = None;
                if self.target.as_deref() == Some(worker.meeting.as_str()) {
                    self.error = Some(tr("The question could not be answered.").to_owned());
                }
                true
            }
        }
    }

    /// Gives up what the card shows and gives up the question; true when
    /// something changed. The worker stays until its thread exits, so the
    /// next question never runs beside an Apple Intelligence call.
    pub(super) fn reset(&mut self) -> bool {
        let shown = self.target.is_some() || self.answer.is_some() || self.error.is_some();
        if let Some(cancelled) = self.cancelled.take() {
            cancelled.store(true, Ordering::Release);
        }
        self.target = None;
        self.answer = None;
        self.error = None;
        if self.question.text.is_empty() {
            return shown;
        }
        self.question = TextEdit::new(String::new());
        true
    }

    /// Cancels the request and forgets its worker. Only called when no thread
    /// is alive, so anything it drops has already finished.
    fn stop(&mut self) {
        if let Some(cancelled) = self.cancelled.take() {
            cancelled.store(true, Ordering::Release);
        }
        self.worker = None;
    }

    fn answer_for(&self, meeting: &str) -> Option<&str> {
        self.target.as_deref().filter(|target| *target == meeting)?;
        self.answer.as_deref()
    }

    fn error_for(&self, meeting: &str) -> Option<&str> {
        self.target.as_deref().filter(|target| *target == meeting)?;
        self.error.as_deref()
    }

    /// Whether this meeting's question is still being answered.
    fn running_for(&self, meeting: &str) -> bool {
        self.running()
            && self
                .worker
                .as_ref()
                .is_some_and(|worker| worker.meeting == meeting)
    }
}

/// The question field and what it brought back, on a meeting with turns
/// while Labs › Ask meetings is on.
pub(super) fn body(ui: &Ui, p: Paint, meeting: &Meeting) -> Option<El> {
    if !crate::labs::ask_meetings() || meeting.turns().is_empty() {
        return None;
    }
    let ask = &ui.meetings.ask;
    let id = meeting.id.clone();
    let mut block = col()
        .key("ask")
        .gap(8.0)
        .child(section_label(p, "ask-label", tr("Question")))
        .child(
            TextInput::new(&ask.question)
                .label(tr("Ask about this meeting"))
                .placeholder(tr("Ask about this meeting"))
                .width(None)
                .on_edit(|ui: &mut Ui| &mut ui.meetings.ask.question)
                .on_submit(move |ui: &mut Ui, _| submit(ui, &id))
                .build(p.k)
                .key("ask-field"),
        );
    if let Some(error) = ask.error_for(&meeting.id) {
        block = block.child(
            text(error.to_owned())
                .key("ask-error")
                .font(DESCRIPTION)
                .color(p.danger())
                .wrap(),
        );
    } else if ask.running_for(&meeting.id) {
        block = block.child(
            text(tr("Thinking…"))
                .key("ask-thinking")
                .font(DESCRIPTION)
                .color(p.faint()),
        );
    } else if let Some(answer) = ask.answer_for(&meeting.id) {
        block = block
            .child(section_label(p, "ask-answer-label", tr("Answer")))
            .child(
                text(answer.to_owned())
                    .key("ask-answer")
                    .font(p.k.ty.body)
                    .color(p.text())
                    .wrap(),
            );
    }
    Some(block)
}

/// Asks the question the field holds. Off the UI thread, one question at a
/// time; nothing runs while the lab is off.
fn submit(ui: &mut Ui, meeting: &str) {
    if !crate::labs::ask_meetings() {
        return;
    }
    let question = ui.meetings.ask.question.text.trim().to_owned();
    let summaries = crate::app_settings::meeting_summaries();
    let wake = ui.modes.waker.clone();
    ui.meetings.ask.submit(meeting, &question, summaries, wake);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Sender;

    /// A worker whose thread blocks until its gate drops, standing in for an
    /// Apple Intelligence call that cannot be interrupted.
    fn blocked_worker(meeting: &str) -> (Worker, Sender<()>) {
        let (gate, wait) = channel::<()>();
        let (sender, receiver) = channel();
        let thread = std::thread::spawn(move || {
            let _ = wait.recv();
            let _ = sender.send(Err("stopped".to_owned()));
        });
        (
            Worker {
                meeting: meeting.into(),
                receiver,
                thread,
            },
            gate,
        )
    }

    /// Lets the blocked worker's thread run to its end.
    fn wait_for(ask: &Ask) {
        while ask
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.thread.is_finished())
        {
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_second_question_waits_for_the_running_one() {
        let mut ask = Ask::default();
        ask.target = Some("m".into());
        ask.answer = Some("Sam owns it.".into());
        let (worker, gate) = blocked_worker("m");
        ask.worker = Some(worker);
        assert!(ask.running());

        ask.submit("m", "and the contract?", MeetingSummaries::OpenCode, None);
        assert!(ask.running(), "the first question still runs");
        assert_eq!(
            ask.worker.as_ref().map(|worker| worker.meeting.as_str()),
            Some("m"),
            "no second worker starts"
        );
        assert_eq!(ask.answer_for("m"), Some("Sam owns it."), "nothing clears");

        drop(gate);
        wait_for(&ask);
        assert!(!ask.running(), "the worker finished");
    }

    #[test]
    fn another_meeting_is_told_to_wait_and_the_old_answer_is_dropped() {
        let mut ask = Ask::default();
        ask.target = Some("a".into());
        let (worker, gate) = blocked_worker("a");
        ask.worker = Some(worker);

        ask.submit("b", "who owns it?", MeetingSummaries::OpenCode, None);
        assert!(ask.error_for("b").is_some(), "it says to wait");
        assert_eq!(
            ask.worker.as_ref().map(|worker| worker.meeting.as_str()),
            Some("a"),
            "the running call is left alone"
        );

        drop(gate);
        wait_for(&ask);
        assert!(ask.poll(), "the late answer is taken");
        assert_eq!(ask.answer_for("b"), None, "it was not about b");
        assert!(ask.error_for("b").is_some(), "the wait line stays");
    }

    #[test]
    fn a_writerless_question_says_so_and_starts_nothing() {
        let mut ask = Ask::default();
        ask.submit("m", "who owns the contract?", MeetingSummaries::Off, None);
        assert!(!ask.running());
        assert!(ask.error_for("m").is_some(), "it says which setting");
        assert_eq!(ask.answer_for("m"), None);
    }

    #[test]
    fn an_empty_question_asks_nothing() {
        let mut ask = Ask::default();
        ask.submit("m", "   ", MeetingSummaries::OpenCode, None);
        assert!(!ask.running());
        assert_eq!(ask.error_for("m"), None);
        assert_eq!(ask.answer_for("m"), None);
    }

    #[test]
    fn the_shown_question_belongs_to_one_meeting_only() {
        let mut ask = Ask::default();
        ask.submit("m", "hi", MeetingSummaries::Off, None);
        assert!(ask.error_for("m").is_some());
        assert_eq!(ask.error_for("other"), None);

        assert!(ask.reset(), "giving it up changes the card");
        assert_eq!(ask.error_for("m"), None);
        assert!(ask.question.text.is_empty());
        assert!(!ask.reset(), "nothing left to give up");
    }
}

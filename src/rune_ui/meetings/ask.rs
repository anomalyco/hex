//! Labs › Ask meetings: the question field under a meeting's notes, the
//! worker that answers it, and the answer shown below it. Answers are shown,
//! never saved: changing the meeting, asking again, or turning the lab off
//! drops the one on screen and stops its request.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use rune::{El, TextEdit, Waker, col, text};
use rune_kit::TextInput;

use super::super::Ui;
use super::super::controls::{DESCRIPTION, Paint};
use super::super::lists::section_label;
use super::Meeting;
use crate::app_settings::MeetingSummaries;
use crate::desktop::i18n::tr;

/// One meeting's question at a time: the field's text, the worker behind it,
/// and what it brought back.
pub(super) struct Ask {
    pub(super) question: TextEdit,
    /// The meeting the shown answer or error belongs to.
    target: Option<String>,
    answer: Option<String>,
    error: Option<String>,
    worker: Option<Receiver<Result<String, String>>>,
    /// Set when the shown question is given up, so a running request stops.
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
    /// Whether a question is being answered now.
    pub(super) fn running(&self) -> bool {
        self.worker.is_some()
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
            Ok(_) => {
                self.worker = Some(receiver);
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
        let Some(worker) = &self.worker else {
            return false;
        };
        match worker.try_recv() {
            Ok(result) => {
                self.worker = None;
                self.cancelled = None;
                match result {
                    Ok(answer) => self.answer = Some(answer),
                    Err(error) => self.error = Some(error),
                }
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.worker = None;
                self.cancelled = None;
                self.error = Some(tr("The question could not be answered.").to_owned());
                true
            }
        }
    }

    /// Gives up the shown question and what it brought back; true when
    /// something changed.
    pub(super) fn reset(&mut self) -> bool {
        let shown = self.target.is_some()
            || self.answer.is_some()
            || self.error.is_some()
            || self.worker.is_some();
        self.stop();
        self.target = None;
        self.answer = None;
        self.error = None;
        if self.question.text.is_empty() {
            return shown;
        }
        self.question = TextEdit::new(String::new());
        true
    }

    /// Stops a running request, dropping whatever it still sends.
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
        .child(section_label(p, "ask-label", tr("Ask")))
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
    if ask.running() {
        block = block.child(
            text(tr("Thinking…"))
                .key("ask-thinking")
                .font(DESCRIPTION)
                .color(p.faint()),
        );
    } else if let Some(error) = ask.error_for(&meeting.id) {
        block = block.child(
            text(error.to_owned())
                .key("ask-error")
                .font(DESCRIPTION)
                .color(p.danger())
                .wrap(),
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

    #[test]
    fn a_question_with_no_writer_says_so_and_starts_nothing() {
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

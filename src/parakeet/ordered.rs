use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};

use super::{DictationJobId, OutputJob, PasteKind, TranscriptionTarget, WorkerEvent, WorkerState};

#[derive(Default)]
pub(super) struct OrderedOutputs {
    pub(super) waiting: BTreeMap<u64, OutputJob>,
    skipped: BTreeSet<u64>,
    next_sequence: u64,
}

impl OrderedOutputs {
    pub(super) fn push(&mut self, job: OutputJob) -> Vec<OutputJob> {
        if let OutputJob::Skipped { sequence } = job {
            return self.skip(sequence);
        }
        let sequence = job.sequence();
        if sequence < self.next_sequence {
            return Vec::new();
        }
        self.skipped.remove(&sequence);
        self.waiting.insert(sequence, job);
        self.drain_ready()
    }

    pub(super) fn skip(&mut self, sequence: u64) -> Vec<OutputJob> {
        if sequence < self.next_sequence || self.waiting.contains_key(&sequence) {
            return Vec::new();
        }
        self.skipped.insert(sequence);
        self.drain_ready()
    }

    fn drain_ready(&mut self) -> Vec<OutputJob> {
        let mut ready = Vec::new();
        loop {
            if let Some(job) = self.waiting.remove(&self.next_sequence) {
                ready.push(job);
                self.next_sequence += 1;
            } else if self.skipped.remove(&self.next_sequence) {
                self.next_sequence += 1;
            } else {
                break;
            }
        }
        ready
    }
}

#[derive(Clone, Copy)]
pub(super) enum TicketKind {
    Dictation {
        job_id: DictationJobId,
        target: TranscriptionTarget,
    },
    Paste(PasteKind),
}

/// RAII reservation for one sequence number in [`OrderedOutputs`] and its
/// admission slot in [`WorkerState`].
///
/// Every allocated sequence carries a ticket from submission through
/// inference, processing, and output. Dropping an unsettled ticket — because a
/// worker stage panicked, hit a closed channel, or dropped a job — frees its
/// admission slot, records the sequence as skipped in `WorkerState`, notifies
/// the output worker non-blockingly, and emits a terminal failure event so
/// `next_sequence` and `WorkerState` can never wedge forever.
pub(super) struct OutputTicket {
    sequence: u64,
    kind: TicketKind,
    state: Arc<Mutex<WorkerState>>,
    output: SyncSender<OutputJob>,
    events: mpsc::Sender<WorkerEvent>,
    settled: bool,
}

impl OutputTicket {
    pub(super) fn new(
        sequence: u64,
        kind: TicketKind,
        state: Arc<Mutex<WorkerState>>,
        output: SyncSender<OutputJob>,
        events: mpsc::Sender<WorkerEvent>,
    ) -> Self {
        Self {
            sequence,
            kind,
            state,
            output,
            events,
            settled: false,
        }
    }

    #[cfg(test)]
    pub(super) fn for_test(sequence: u64) -> Self {
        let (output, _rx) = mpsc::sync_channel(1);
        let (events, _erx) = mpsc::channel();
        Self {
            sequence,
            kind: TicketKind::Paste(PasteKind::LastTranscript),
            state: Arc::new(Mutex::new(WorkerState::default())),
            output,
            events,
            settled: true,
        }
    }

    pub(super) fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Cancels a ticket whose admission slot was never committed (for example,
    /// when `try_send` rejected the job).
    pub(super) fn disarm(mut self) {
        self.settled = true;
    }

    /// Completes a ticket normally, freeing its admission slot in
    /// [`WorkerState`].
    pub(super) fn settle(mut self) {
        self.settled = true;
        self.release_slot(false);
    }

    fn release_slot(&self, record_skip: bool) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        match self.kind {
            TicketKind::Dictation { job_id, .. } => {
                state.jobs.remove(&job_id);
            }
            TicketKind::Paste(_) => {
                state.pending_pastes = state.pending_pastes.saturating_sub(1);
            }
        }
        if record_skip {
            state.skipped.insert(self.sequence);
        }
    }
}

impl Drop for OutputTicket {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        self.settled = true;
        self.release_slot(true);
        tracing::error!(
            sequence = self.sequence,
            "dictation output ticket dropped before completion; skipping its sequence"
        );
        // Non-blocking: if the channel is full, the output worker already has
        // queued work and will drain `state.skipped` before pushing the next
        // job. If the channel is empty, this wakes it immediately.
        let _ = self.output.try_send(OutputJob::Skipped {
            sequence: self.sequence,
        });
        let event = match self.kind {
            TicketKind::Dictation { job_id, target } => WorkerEvent::Completed {
                job_id,
                target,
                result: Err("dictation stage aborted unexpectedly".into()),
                processing: None,
            },
            TicketKind::Paste(kind) => WorkerEvent::Pasted {
                kind,
                result: Err("paste stage aborted unexpectedly".into()),
            },
        };
        let _ = self.events.send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_a_ticket_skips_its_sequence_and_unblocks_later_jobs() {
        let state = Arc::new(Mutex::new(WorkerState {
            next_sequence: 2,
            jobs: BTreeSet::from([DictationJobId(0), DictationJobId(1)]),
            pending_pastes: 0,
            skipped: BTreeSet::new(),
            return_to: None,
            started_by_shortcut: false,
        }));
        let (output_tx, output_rx) = mpsc::sync_channel(4);
        let (event_tx, event_rx) = mpsc::channel();

        let ticket0 = OutputTicket::new(
            0,
            TicketKind::Dictation {
                job_id: DictationJobId(0),
                target: TranscriptionTarget::Paste,
            },
            state.clone(),
            output_tx.clone(),
            event_tx.clone(),
        );
        let ticket1 = OutputTicket::new(
            1,
            TicketKind::Dictation {
                job_id: DictationJobId(1),
                target: TranscriptionTarget::Paste,
            },
            state.clone(),
            output_tx,
            event_tx,
        );

        let mut ordered = OrderedOutputs::default();
        // Sequence 1 arrives before sequence 0 and waits.
        assert!(
            ordered
                .push(OutputJob::Paste {
                    ticket: ticket1,
                    kind: PasteKind::LastTranscript,
                })
                .is_empty()
        );
        assert_eq!(ordered.waiting.len(), 1);

        // Simulate a panicking worker stage dropping ticket 0 without settling.
        drop(ticket0);

        // Admission slot was freed and failure event emitted.
        assert!(!state.lock().unwrap().jobs.contains(&DictationJobId(0)));
        assert!(state.lock().unwrap().skipped.contains(&0));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            WorkerEvent::Completed {
                job_id,
                result: Err(_),
                ..
            } if job_id == DictationJobId(0)
        ));

        // The output worker drains `state.skipped` and receives the Skipped
        // sentinel, immediately releasing waiting sequence 1.
        let skipped = std::mem::take(&mut state.lock().unwrap().skipped);
        let mut ready = Vec::new();
        for seq in skipped {
            ready.extend(ordered.skip(seq));
        }
        ready.extend(ordered.push(output_rx.try_recv().unwrap()));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].sequence(), 1);
        if let OutputJob::Paste { ticket, .. } = ready.pop().unwrap() {
            ticket.settle();
        }
        assert!(state.lock().unwrap().jobs.is_empty());
        assert_eq!(ordered.next_sequence, 2);
    }

    #[test]
    fn dropping_a_ticket_never_deadlocks_when_the_output_channel_is_full() {
        let state = Arc::new(Mutex::new(WorkerState {
            next_sequence: 2,
            jobs: BTreeSet::from([DictationJobId(0)]),
            pending_pastes: 1,
            skipped: BTreeSet::new(),
            return_to: None,
            started_by_shortcut: false,
        }));
        let (output_tx, output_rx) = mpsc::sync_channel(1);
        let (event_tx, _event_rx) = mpsc::channel();
        let ticket0 = OutputTicket::new(
            0,
            TicketKind::Dictation {
                job_id: DictationJobId(0),
                target: TranscriptionTarget::Paste,
            },
            state.clone(),
            output_tx.clone(),
            event_tx.clone(),
        );
        let ticket1 = OutputTicket::new(
            1,
            TicketKind::Paste(PasteKind::LastTranscript),
            state.clone(),
            output_tx.clone(),
            event_tx,
        );
        // Fill the 1-slot output channel with job 1 before dropping ticket 0.
        output_tx
            .try_send(OutputJob::Paste {
                ticket: ticket1,
                kind: PasteKind::LastTranscript,
            })
            .unwrap();
        drop(ticket0);

        // Even though `try_send(Skipped)` saw Full, `state.skipped` recorded 0
        // so the output worker still releases job 1 when it reads the channel.
        let mut ordered = OrderedOutputs::default();
        let job = output_rx.try_recv().unwrap();
        let skipped = std::mem::take(&mut state.lock().unwrap().skipped);
        let mut ready = Vec::new();
        for seq in skipped {
            ready.extend(ordered.skip(seq));
        }
        ready.extend(ordered.push(job));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].sequence(), 1);
        if let OutputJob::Paste { ticket, .. } = ready.pop().unwrap() {
            ticket.settle();
        }
        assert!(state.lock().unwrap().jobs.is_empty());
        assert_eq!(state.lock().unwrap().pending_pastes, 0);
    }
}

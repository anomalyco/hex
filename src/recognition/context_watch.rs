//! The listener's foreground context: the monitor's latest snapshot, the
//! page refresh a shortcut's start asks for, and the finish that waits for
//! it briefly instead of using the previous dictation's page.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use color_eyre::Result;

use crate::desktop::events::{EventLog, VoiceEvent, now_ms};
use crate::dictation::context::{ContextMonitor, ContextSnapshot};
use crate::suppression::InputActivity;

/// How long a finished shortcut waits for the page its start asked to
/// refresh before handing the worker a context with no page.
const REFRESH_WAIT: Duration = Duration::from_secs(1);
/// How often that wait rechecks the completed ticket: a worker stores it
/// just after queueing the snapshot, so a delivered page can arrive a moment
/// before `refreshed_since` sees it.
const REFRESH_POLL: Duration = Duration::from_millis(10);

pub(super) struct ContextWatch {
    monitor: ContextMonitor,
    snapshot: ContextSnapshot,
    activity: InputActivity,
    /// The refresh ticket for the capture now recording, until it finishes
    /// or ends.
    refresh: Option<u64>,
}

impl ContextWatch {
    pub(super) fn start(activity: InputActivity) -> Self {
        Self {
            monitor: ContextMonitor::start(),
            snapshot: ContextSnapshot::default(),
            activity,
            refresh: None,
        }
    }

    /// Drains every published context into the loop's own snapshot.
    pub(super) fn drain(&mut self, events: &mut EventLog) -> Result<()> {
        while let Ok(next) = self.monitor.updates.try_recv() {
            self.apply(next, events)?;
        }
        Ok(())
    }

    fn apply(&mut self, next: ContextSnapshot, events: &mut EventLog) -> Result<()> {
        if self.snapshot != next {
            self.activity.invalidate();
        }
        let switched_app = self.snapshot.application != next.application;
        self.snapshot = next;
        crate::dictation::vocabulary::focus(&self.snapshot);
        if switched_app {
            events.emit(&VoiceEvent::Context {
                timestamp_ms: now_ms(),
                application: self.snapshot.application.clone(),
            })?;
        }
        Ok(())
    }

    /// A shortcut's capture starts: ask for a page read for its finish.
    pub(super) fn begin(&mut self) {
        self.refresh = Some(self.monitor.request_refresh());
    }

    /// The capture ended without transcribing: no finish waits for its page.
    pub(super) fn cancel(&mut self) {
        self.refresh = None;
    }

    /// The context to hand a finished shortcut: the refreshed page when it
    /// arrived in time, else the same context without a page rather than the
    /// page of a previous dictation. A finish whose start never asked for a
    /// refresh (a control start) returns the loop's context unchanged.
    pub(super) fn finish(&mut self, events: &mut EventLog) -> Result<ContextSnapshot> {
        let Some(ticket) = self.refresh.take() else {
            return Ok(self.snapshot.clone());
        };
        let deadline = Instant::now() + REFRESH_WAIT;
        loop {
            let refreshed = self.monitor.refreshed_since(ticket);
            // A completed refresh has its snapshot in the slot or already
            // delivered, so this drain takes it or a newer one.
            self.drain(events)?;
            if refreshed {
                return Ok(self.snapshot.clone());
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Ok(self.without_page());
            };
            match self
                .monitor
                .updates
                .recv_timeout(remaining.min(REFRESH_POLL))
            {
                Ok(next) => self.apply(next, events)?,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Ok(self.without_page());
                }
            }
        }
    }

    fn without_page(&self) -> ContextSnapshot {
        ContextSnapshot {
            browser_url: None,
            ..self.snapshot.clone()
        }
    }
}

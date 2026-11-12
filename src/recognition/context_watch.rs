//! The listener's foreground context: the monitor's latest snapshot, the
//! page refresh a capture asks for once it is real, and the finish that waits
//! for it briefly instead of using the previous dictation's page.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::Result;

use crate::desktop::events::{EventLog, VoiceEvent, now_ms};
use crate::dictation::context::{CaptureRefresh, ContextMonitor, ContextSnapshot};
use crate::suppression::InputActivity;

/// How long a finished shortcut waits for the page its capture asked to
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
    /// The one refresh this capture owes, if any.
    capture: CaptureRefresh,
    /// Labs › Screen vocabulary's one read, for this capture.
    screen: ScreenVocabulary,
}

/// One capture's screen vocabulary: a background read of the focused window,
/// started as the capture becomes real and taken only if it finished in time.
#[derive(Default)]
struct ScreenVocabulary {
    read: Option<mpsc::Receiver<Vec<String>>>,
}

impl ScreenVocabulary {
    /// Starts this capture's one read, if Labs › Screen vocabulary is on.
    /// Off, nothing reads Accessibility at all.
    fn start(&mut self) {
        if self.read.is_some() || !crate::labs::screen_vocabulary() {
            return;
        }
        let (sender, read) = mpsc::channel();
        self.read = Some(read);
        let spawned = thread::Builder::new()
            .name("screen-vocabulary".into())
            .spawn(move || {
                if let Some(terms) = crate::dictation::screen_vocabulary::from_focused_window() {
                    let _ = sender.send(terms);
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not start the screen vocabulary reader");
            self.read = None;
        }
    }

    /// This capture's terms, if the read finished before its run.
    fn take(&mut self) -> Vec<String> {
        self.read
            .take()
            .and_then(|read| read.try_recv().ok())
            .unwrap_or_default()
    }

    /// The capture ended without transcribing, or a new one started: a late
    /// read is discarded instead of reaching another dictation.
    fn cancel(&mut self) {
        self.read = None;
    }
}

impl ContextWatch {
    pub(super) fn start(activity: InputActivity) -> Self {
        Self {
            monitor: ContextMonitor::start(),
            snapshot: ContextSnapshot::default(),
            activity,
            capture: CaptureRefresh::default(),
            screen: ScreenVocabulary::default(),
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

    /// A shortcut's press started a capture. It may still be a provisional
    /// press a chord discards, so nothing reads the page or the screen yet.
    pub(super) fn begin(&mut self) {
        self.screen.cancel();
        self.capture.begin();
    }

    /// The capture became real: a control start (no hold to judge), a hold
    /// past its threshold, or a released microphone finally opening. Ask for
    /// its page and its screen's vocabulary now, once per capture.
    pub(super) fn intend(&mut self) {
        self.screen.start();
        let monitor = &self.monitor;
        self.capture.intend(|| monitor.request_refresh());
    }

    /// The capture ended without transcribing: no finish waits for its page,
    /// and a late screen vocabulary read is discarded.
    pub(super) fn cancel(&mut self) {
        self.screen.cancel();
        self.capture.cancel();
    }

    /// The context to hand a finished capture: the refreshed page when it
    /// arrived in time, else the same context without a page rather than the
    /// page of a previous dictation. A capture that finished before it was
    /// real asks for its page here, and a finish with no capture at all (the
    /// loop's own context) returns it unchanged.
    pub(super) fn finish(&mut self, events: &mut EventLog) -> Result<ContextSnapshot> {
        let mut snapshot = self.finish_snapshot(events)?;
        let terms = self.screen.take();
        if !terms.is_empty() {
            snapshot.screen_terms = terms;
        }
        Ok(snapshot)
    }

    /// The context itself: the refreshed page when it arrived in time, else
    /// the same context without a page rather than the page of a previous
    /// dictation.
    fn finish_snapshot(&mut self, events: &mut EventLog) -> Result<ContextSnapshot> {
        let monitor = &self.monitor;
        let Some(ticket) = self.capture.finish(|| monitor.request_refresh()) else {
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

    /// Hands a capture the same context with no page, and focuses the
    /// vocabulary there too: the previous dictation's page must not name this
    /// one's mode.
    fn without_page(&self) -> ContextSnapshot {
        let snapshot = ContextSnapshot {
            browser_url: None,
            ..self.snapshot.clone()
        };
        crate::dictation::vocabulary::focus(&snapshot);
        snapshot
    }
}

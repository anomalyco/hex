//! The reader, running: one thread beside one recording.
//!
//! [`SpeakerReader::start`] is the gate. With Labs › Speaker names off, or
//! a call in an app Hex has no adapter for, there is no thread: nothing is
//! read through Accessibility and no app is asked for its web tree. With a
//! reader started, turning the lab off ends it at its next look.
//!
//! The thread is the [`reader`](super::reader) loop with the real things
//! handed to it: the recording's capture clock, counted from the first
//! audio as the transcript's lines are; the call app's windows, each look
//! bounded so that a slow app costs one look and never a pile of them; and
//! the meeting's own evidence file. It shares nothing with audio capture,
//! and dropping the [`SpeakerReader`] stops it and waits for it.
//!
//! A recording started by hand names no call app. Its reader waits for an
//! app Hex can read to take the microphone, so a recording started before
//! the huddle is joined is still read.
//!
//! macOS only, as the [`adapter`](super::adapter) is.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::adapter::CallAppAdapter;
use super::evidence::{self, Evidence};
use super::reader::{self, CaptureClock, PollState, Wait};
use super::registry::{CallApp, adapter_for};
use crate::accessibility::tree::{Budgets, Snapshot};
use crate::accessibility::watch::AppWindows;
use crate::meeting::NO_AUDIO_YET;

/// The most one look may read. A huddle of twenty people reads whole in
/// well under a tenth of this time; a look that runs out is not believed.
const LOOK: Budgets = Budgets {
    max_nodes: 4_000,
    max_depth: 80,
    deadline: Duration::from_millis(400),
};

/// How often a recording that named no call app looks for one.
const CALL_APP_WAIT_MS: u64 = 5_000;
/// How often a reader started before the first audio checks for it.
const FIRST_AUDIO_WAIT_MS: u64 = 50;

/// The speaker reader of one recording. Dropping it stops the reader and
/// waits for its thread.
pub struct SpeakerReader {
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl SpeakerReader {
    /// Starts reading the call app of a recording into `directory`, when
    /// Labs › Speaker names is on and Hex can read the app. `application`
    /// is the call's canonical bundle id, or nothing for a recording that
    /// knows of no call app. `capture` is the recording's clock, and
    /// `first_audio` when on it the first audio arrived, in microseconds
    /// ([`NO_AUDIO_YET`] until it has): where the transcript's lines count
    /// from.
    pub fn start(
        directory: &Path,
        application: Option<&str>,
        capture: Instant,
        first_audio: Arc<AtomicU64>,
    ) -> Option<Self> {
        if !crate::labs::meeting_speaker_names() {
            return None;
        }
        let call = match application {
            Some(bundle_id) => Some(readable(bundle_id)?),
            None => None,
        };
        let clock = |stopped| Clock {
            capture,
            first_audio,
            stopped,
            reading: crate::labs::meeting_speaker_names,
        };
        let directory = directory.to_path_buf();
        Self::spawn(clock, move |mut clock| {
            let Some((bundle_id, adapter)) = call.or_else(|| call_on_the_microphone(&mut clock))
            else {
                return;
            };
            let mut windows = AppWindows::of_web_app(bundle_id, LOOK);
            read(&directory, adapter, || windows.snapshot(), &mut clock);
        })
    }

    fn spawn(
        clock: impl FnOnce(Receiver<()>) -> Clock,
        reader: impl FnOnce(Clock) + Send + 'static,
    ) -> Option<Self> {
        let (stop, stopped) = mpsc::channel();
        let clock = clock(stopped);
        let thread = thread::Builder::new()
            .name("meeting-speakers".into())
            .spawn(move || reader(clock))
            .map_err(|error| tracing::warn!(%error, "could not start the speaker reader"))
            .ok()?;
        Some(Self {
            stop: Some(stop),
            thread: Some(thread),
        })
    }
}

impl Drop for SpeakerReader {
    fn drop(&mut self) {
        // The reader is between looks or in one; a look is bounded.
        drop(self.stop.take());
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("the speaker reader panicked");
        }
    }
}

/// The call app with this bundle id, when Hex has an adapter for it.
fn readable(bundle_id: &str) -> Option<(&'static str, &'static dyn CallAppAdapter)> {
    let bundle_id = crate::microphone_activity::canonical_bundle_id_for(bundle_id, None)?;
    Some((bundle_id, adapter_for(CallApp::Bundle(bundle_id))?))
}

/// Waits for a call app Hex can read to hold the microphone. Nothing when
/// the recording stops first.
fn call_on_the_microphone(
    clock: &mut Clock,
) -> Option<(&'static str, &'static dyn CallAppAdapter)> {
    loop {
        let applications =
            crate::microphone_activity::active_microphone_applications().unwrap_or_default();
        let call = applications
            .iter()
            .filter_map(|application| application.canonical_bundle_id())
            .find_map(readable);
        if call.is_some() {
            return call;
        }
        if clock.pause(CALL_APP_WAIT_MS) == Wait::Stopped {
            return None;
        }
    }
}

/// Runs the reader on one call app until `clock` stops, saving what it
/// sees to the evidence in `directory`.
fn read(
    directory: &Path,
    adapter: &dyn CallAppAdapter,
    snapshot: impl FnMut() -> Snapshot,
    clock: &mut Clock,
) {
    let saved = evidence::read(directory).unwrap_or_default();
    let mut sink = |lines: &[Evidence]| evidence::append(directory, lines);
    reader::run(
        PollState::continuing(&saved),
        adapter,
        snapshot,
        clock,
        &mut sink,
    );
}

/// A recording's clock, and its end.
struct Clock {
    capture: Instant,
    first_audio: Arc<AtomicU64>,
    /// Disconnects when the recording drops its [`SpeakerReader`].
    stopped: Receiver<()>,
    /// Whether the reader may still read: the lab's switch.
    reading: fn() -> bool,
}

impl Clock {
    fn first_audio_us(&self) -> Option<u64> {
        let first = self.first_audio.load(Ordering::Relaxed);
        (first != NO_AUDIO_YET).then_some(first)
    }

    /// Waits `ms`, or less when the recording stops.
    fn pause(&self, ms: u64) -> Wait {
        match self.stopped.recv_timeout(Duration::from_millis(ms)) {
            Err(RecvTimeoutError::Timeout) if (self.reading)() => Wait::Elapsed,
            _ => Wait::Stopped,
        }
    }
}

impl CaptureClock for Clock {
    fn now_ms(&self) -> u64 {
        let now_us = self.capture.elapsed().as_micros() as u64;
        now_us.saturating_sub(self.first_audio_us().unwrap_or(now_us)) / 1_000
    }

    /// No look is made before the first audio: the transcript has no line
    /// that early, and a look has no time to be stamped with.
    fn wait(&mut self, ms: u64) -> Wait {
        let mut wait = self.pause(ms);
        while wait == Wait::Elapsed && self.first_audio_us().is_none() {
            wait = self.pause(FIRST_AUDIO_WAIT_MS);
        }
        wait
    }
}

#[cfg(test)]
mod tests;

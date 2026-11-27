//! Capture snapshot feed and [`Outcomes`] lifecycle tracking.
//!
//! Each in-flight capture moves monotonically through [`UnfinishedPhase`]:
//! `Recording` -> `Finishing` (or `Discarded`) -> settled and removed.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use scrawl_capture::capture::Stage;

use super::{CaptureRoute, End, TargetKind, Written};
use crate::audio::CaptureInstant;
use crate::control::ScrawlCaptureOutcome;

/// Every capture Hex knows about, as one value. A view (the capture
/// capsule, the menu bar, `hex capture`) reads it whole instead of reaching
/// into the host, and [`watch`] says when it changed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CaptureSnapshot {
    /// Bumped on every change, across hosts, for the life of the process.
    pub generation: u64,
    pub recording: Option<RecordingCapture>,
    /// Stopped captures still being written or cancelled, oldest first.
    pub finishing: Vec<FinishingCapture>,
    /// The most recent capture that finished, failed, or was cancelled.
    pub last: Option<SettledCapture>,
    /// The cabled iPhone's name, when one was found at the last look
    /// ([`super::CaptureCommand::FindPhone`]): the capsule's iPhone chip.
    pub phone: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordingCapture {
    pub bundle: PathBuf,
    /// When it started, on the clock [`CaptureInstant::now`] reads.
    pub since: CaptureInstant,
    /// The engine started recording the screen; until then it is starting.
    pub screen: bool,
    pub target: TargetKind,
    /// The capture recorded a click, scroll, or ink stroke: the person got
    /// to work, so the capsule's switch chips fold.
    pub worked: bool,
    /// A Screenshots recording: once stopped, the screenshot card shows it
    /// and the capsule folds.
    pub clip: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinishingCapture {
    pub bundle: PathBuf,
    /// The engine's latest stage: `Stopping` until it reports another.
    pub stage: Stage,
    pub cancelling: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettledCapture {
    pub bundle: PathBuf,
    /// `Done`, `Failed`, or `Cancelled`.
    pub stage: Stage,
    /// A written capture's words, missing spans of audio, and why it has
    /// no transcript, if it has none.
    pub words: Option<usize>,
    pub gaps: Option<usize>,
    /// Where a written capture's audio stops, when Hex had to stop keeping it.
    pub audio_stopped_at_ms: Option<u64>,
    pub missing_transcript: Option<String>,
    /// Why a capture failed.
    pub error: Option<String>,
    /// Why a written capture's summary didn't reach the clipboard; `None`
    /// once it did (or for a capture that isn't copied).
    pub copy_error: Option<String>,
    /// Where a written capture goes, while captures are sent to OpenCode
    /// (`capture.routing`'s `autoSend`).
    pub route: Option<CaptureRoute>,
}

/// How a capture ended when settled into [`Outcomes`].
#[expect(
    clippy::large_enum_variant,
    reason = "passed directly into Outcomes::settle and never stored"
)]
pub(super) enum CaptureEnd {
    Written {
        audio: Written,
        copy_error: Option<String>,
        route: Option<CaptureRoute>,
    },
    Failed(String),
    Cancelled,
}

impl SettledCapture {
    fn from_end(bundle: PathBuf, end: CaptureEnd) -> Self {
        match end {
            CaptureEnd::Written {
                audio,
                copy_error,
                route,
            } => Self {
                bundle,
                stage: Stage::Done,
                words: Some(audio.words),
                gaps: Some(audio.gaps),
                audio_stopped_at_ms: audio.audio_stopped_at_ms,
                missing_transcript: audio.missing_transcript,
                error: None,
                copy_error,
                route,
            },
            CaptureEnd::Failed(error) => Self {
                bundle,
                stage: Stage::Failed,
                words: None,
                gaps: None,
                audio_stopped_at_ms: None,
                missing_transcript: None,
                error: Some(error),
                copy_error: None,
                route: None,
            },
            CaptureEnd::Cancelled => Self {
                bundle,
                stage: Stage::Cancelled,
                words: None,
                gaps: None,
                audio_stopped_at_ms: None,
                missing_transcript: None,
                error: None,
                copy_error: None,
                route: None,
            },
        }
    }

    pub(super) fn outcome(&self) -> ScrawlCaptureOutcome {
        ScrawlCaptureOutcome {
            bundle: self.bundle.clone(),
            stage: self.stage.label().into(),
            words: self.words,
            gaps: self.gaps,
            audio_stopped_at_ms: self.audio_stopped_at_ms,
            missing_transcript: self.missing_transcript.clone(),
        }
    }
}

/// The process's capture snapshot and those watching it.
pub(super) struct Feed {
    snapshot: Mutex<CaptureSnapshot>,
    pub(super) watchers: Mutex<Vec<SyncSender<()>>>,
}

impl Feed {
    pub(super) const fn new() -> Self {
        Self {
            snapshot: Mutex::new(CaptureSnapshot {
                generation: 0,
                recording: None,
                finishing: Vec::new(),
                last: None,
                phone: None,
            }),
            watchers: Mutex::new(Vec::new()),
        }
    }

    fn publish(&self, mut next: CaptureSnapshot) {
        {
            let mut snapshot = self
                .snapshot
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            next.generation = snapshot.generation;
            if *snapshot == next {
                return;
            }
            next.generation += 1;
            *snapshot = next;
        }
        // A full wake-up is already pending; a gone watcher stops watching.
        self.watchers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|wake| !matches!(wake.try_send(()), Err(TrySendError::Disconnected(()))));
    }

    pub(super) fn snapshot(&self) -> CaptureSnapshot {
        self.snapshot
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub(super) fn watch(&'static self) -> CaptureWatch {
        let (wake, woken) = mpsc::sync_channel(1);
        let _ = wake.try_send(());
        self.watchers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(wake);
        CaptureWatch { feed: self, woken }
    }
}

pub(super) static FEED: Feed = Feed::new();

/// The current capture snapshot.
#[cfg(target_os = "macos")]
pub fn snapshot() -> CaptureSnapshot {
    FEED.snapshot()
}

/// Watches the capture snapshot from any thread. Changes coalesce: a watcher
/// always gets the latest snapshot, never a stale one, but may skip the ones
/// between.
pub fn watch() -> CaptureWatch {
    FEED.watch()
}

pub struct CaptureWatch {
    feed: &'static Feed,
    woken: Receiver<()>,
}

impl CaptureWatch {
    /// The snapshot if it changed since the last call (the first call always
    /// returns it), without blocking.
    #[cfg(test)]
    pub(super) fn changed(&self) -> Option<CaptureSnapshot> {
        let mut woke = false;
        while self.woken.try_recv().is_ok() {
            woke = true;
        }
        woke.then(|| self.feed.snapshot())
    }

    /// Waits up to `timeout` for a change.
    pub fn wait(&self, timeout: Duration) -> Option<CaptureSnapshot> {
        self.woken.recv_timeout(timeout).ok()?;
        while self.woken.try_recv().is_ok() {}
        Some(self.feed.snapshot())
    }
}

/// The host's captures from start to outcome; every change publishes a
/// snapshot to the feed.
pub(super) struct Outcomes {
    /// Every capture from its start until its outcome is settled: starting,
    /// recording, waiting to be written, writing, or cancelling. Each is
    /// settled exactly once, by its collector or by the finisher.
    pub(super) unfinished: Vec<Unfinished>,
    pub(super) last: Option<SettledCapture>,
    phone: Option<String>,
    /// The iPhone mirror ended on its own; the listener's next
    /// [`super::CaptureHost::reap`] finishes the capture.
    pub(super) phone_ended: bool,
    pub(super) feed: &'static Feed,
    /// A switch is between its discard and its start: views see neither
    /// half alone ([`super::CaptureCommand::Switch`]).
    pub(super) held: bool,
}

pub(super) struct Unfinished {
    bundle: PathBuf,
    /// A Screenshots recording ([`super::CaptureCommand::Clip`]).
    clip: bool,
    phase: UnfinishedPhase,
}

enum UnfinishedPhase {
    Recording {
        since: CaptureInstant,
        screen: bool,
        target: TargetKind,
        worked: bool,
    },
    Finishing {
        stage: Stage,
        cancelling: bool,
    },
    /// Dropped for another target: views never see it again, it does not count
    /// against [`super::MAX_FINISHING`], and it settles no outcome.
    Discarded,
}

impl Default for Outcomes {
    fn default() -> Self {
        Self::on(&FEED)
    }
}

impl Outcomes {
    pub(super) fn on(feed: &'static Feed) -> Self {
        Self {
            unfinished: Vec::new(),
            last: None,
            phone: None,
            phone_ended: false,
            feed,
            held: false,
        }
    }

    pub(super) fn idle(&self) -> bool {
        self.unfinished.is_empty()
    }

    /// Stopped captures still being written or cancelled, which count against
    /// [`super::MAX_FINISHING`]. Excludes both the actively recording capture
    /// and any discarded capture being cleaned up.
    pub(super) fn finishing_count(&self) -> usize {
        self.unfinished
            .iter()
            .filter(|unfinished| matches!(unfinished.phase, UnfinishedPhase::Finishing { .. }))
            .count()
    }

    /// Bundles of stopped captures still being written or cancelled, for
    /// status reports.
    pub(super) fn finishing_bundles(&self, exclude_live: Option<&Path>) -> Vec<PathBuf> {
        self.unfinished
            .iter()
            .filter(|unfinished| {
                !matches!(unfinished.phase, UnfinishedPhase::Discarded)
                    && exclude_live.is_none_or(|live| live != unfinished.bundle)
            })
            .map(|unfinished| unfinished.bundle.clone())
            .collect()
    }

    pub(super) fn start(
        &mut self,
        bundle: PathBuf,
        since: CaptureInstant,
        target: TargetKind,
        clip: bool,
    ) {
        self.unfinished.push(Unfinished {
            bundle,
            clip,
            phase: UnfinishedPhase::Recording {
                since,
                screen: false,
                target,
                worked: false,
            },
        });
        self.publish();
    }

    /// Whether `bundle` is a Screenshots recording still unsettled.
    pub(super) fn is_clip(&self, bundle: &Path) -> bool {
        self.unfinished
            .iter()
            .any(|unfinished| unfinished.bundle == bundle && unfinished.clip)
    }

    pub(super) fn stopped(&mut self, bundle: &Path, end: End) {
        if let Some(unfinished) = self.find(bundle) {
            unfinished.phase = match end {
                End::Finish => UnfinishedPhase::Finishing {
                    stage: Stage::Stopping,
                    cancelling: false,
                },
                End::Cancel => UnfinishedPhase::Finishing {
                    stage: Stage::Stopping,
                    cancelling: true,
                },
                End::Discard => UnfinishedPhase::Discarded,
            };
            self.publish();
        }
    }

    pub(super) fn worked(&mut self, bundle: &Path) {
        if let Some(unfinished) = self.find(bundle)
            && let UnfinishedPhase::Recording { worked, .. } = &mut unfinished.phase
            && !*worked
        {
            *worked = true;
            self.publish();
        }
    }

    /// The engine's progress, which arrives late and out of band: it moves a
    /// capture on but never settles one (its collector or the finisher does).
    pub(super) fn progress(&mut self, bundle: &Path, stage: Stage) {
        let Some(unfinished) = self.find(bundle) else {
            return;
        };
        match (stage, &mut unfinished.phase) {
            (Stage::Recording, UnfinishedPhase::Recording { screen, .. }) => *screen = true,
            (
                Stage::Stopping | Stage::Transcribing | Stage::Rendering | Stage::Writing,
                UnfinishedPhase::Finishing {
                    stage: current,
                    cancelling: false,
                },
            ) => *current = stage,
            _ => return,
        }
        self.publish();
    }

    pub(super) fn settle(&mut self, bundle: PathBuf, end: CaptureEnd) {
        let discarded = self
            .find(&bundle)
            .is_some_and(|unfinished| matches!(unfinished.phase, UnfinishedPhase::Discarded));
        self.unfinished
            .retain(|unfinished| unfinished.bundle != bundle);
        if !discarded {
            self.last = Some(SettledCapture::from_end(bundle, end));
        }
        self.publish();
    }

    /// A switch discarded `bundle` and then could not start: its outcome,
    /// since the discard settles none.
    pub(super) fn fail_switch(&mut self, bundle: PathBuf, error: String) {
        self.last = Some(SettledCapture::from_end(bundle, CaptureEnd::Failed(error)));
        self.publish();
    }

    pub(super) fn phone(&mut self, phone: Option<String>) {
        if self.phone != phone {
            self.phone = phone;
            self.publish();
        }
    }

    /// A written capture's route changed; only the newest capture's shows.
    pub(super) fn routed(&mut self, bundle: &Path, route: CaptureRoute) {
        if let Some(last) = self.last.as_mut()
            && last.bundle == bundle
            && last.route.as_ref() != Some(&route)
        {
            last.route = Some(route);
            self.publish();
        }
    }

    /// A capture whose initial copy failed has now reached the clipboard.
    pub(super) fn copied(&mut self, bundle: &Path) {
        if let Some(last) = self.last.as_mut()
            && last.bundle == bundle
            && last.copy_error.take().is_some()
        {
            self.publish();
        }
    }

    fn find(&mut self, bundle: &Path) -> Option<&mut Unfinished> {
        self.unfinished
            .iter_mut()
            .find(|unfinished| unfinished.bundle == bundle)
    }

    pub(super) fn snapshot(&self) -> CaptureSnapshot {
        let mut snapshot = CaptureSnapshot {
            last: self.last.clone(),
            phone: self.phone.clone(),
            ..CaptureSnapshot::default()
        };
        for unfinished in &self.unfinished {
            match unfinished.phase {
                UnfinishedPhase::Recording {
                    since,
                    screen,
                    target,
                    worked,
                } => {
                    snapshot.recording = Some(RecordingCapture {
                        bundle: unfinished.bundle.clone(),
                        since,
                        screen,
                        target,
                        worked,
                        clip: unfinished.clip,
                    });
                }
                UnfinishedPhase::Finishing { stage, cancelling } => {
                    snapshot.finishing.push(FinishingCapture {
                        bundle: unfinished.bundle.clone(),
                        stage,
                        cancelling,
                    });
                }
                UnfinishedPhase::Discarded => {}
            }
        }
        snapshot
    }

    fn publish(&self) {
        if !self.held {
            self.feed.publish(self.snapshot());
        }
    }

    /// Ends a hold, publishing what changed during it.
    pub(super) fn release(&mut self) {
        self.held = false;
        self.publish();
    }
}

/// The host that last started, for engine progress, which arrives on the
/// process-wide `capture-events` thread.
pub(super) static LIVE_OUTCOMES: Mutex<Weak<Mutex<Outcomes>>> = Mutex::new(Weak::new());

pub(super) type SharedOutcomes = Arc<Mutex<Outcomes>>;

pub(super) fn lock(outcomes: &Mutex<Outcomes>) -> std::sync::MutexGuard<'_, Outcomes> {
    outcomes.lock().unwrap_or_else(|error| error.into_inner())
}

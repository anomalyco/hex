//! Track warnings that outlive the recording's live state, and the digital
//! silence policy that raises them.

use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::SyncSender;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{MeetingManifest, RecordingUpdate};

/// A problem with one captured track that the user should know about.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackWarning {
    /// The microphone stream delivered only exact zeros, as a call app
    /// holding the device with voice processing does (#1518).
    DigitalSilence,
}

/// Exact-zero samples that mean the microphone stream is digitally silent
/// rather than a quiet room.
const SILENCE_TIMEOUT: Duration = Duration::from_secs(3);
/// How many times one recording reopens a digitally silent microphone before
/// keeping the stream open and warning instead.
const SILENCE_REOPENS: u8 = 2;

/// The microphone's warning, shared by each reopened source with the recorder
/// that persists the final one.
///
/// The live warning clears when audio returns, so the capsule and menu bar
/// stop saying the microphone is silent. The sticky flag keeps that it was
/// silent at some point, which is what the finished meeting records.
#[derive(Clone)]
pub(super) struct TrackWarnings {
    live: Arc<Mutex<Option<TrackWarning>>>,
    sticky: Arc<AtomicBool>,
    updates: Option<SyncSender<RecordingUpdate>>,
}

impl TrackWarnings {
    pub(super) fn new(updates: Option<SyncSender<RecordingUpdate>>) -> Self {
        Self {
            live: Arc::default(),
            sticky: Arc::default(),
            updates,
        }
    }

    /// The warning the controller shows right now.
    pub(super) fn live(&self) -> Option<TrackWarning> {
        *self.live.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// The warning for the final manifest: the microphone was digitally
    /// silent at some point during the recording.
    pub(super) fn persisted(&self) -> Option<TrackWarning> {
        self.sticky
            .load(Ordering::Relaxed)
            .then_some(TrackWarning::DigitalSilence)
    }

    /// Shows the warning and keeps it for the manifest; false when the
    /// controller's queue was full and a later chunk should try again.
    pub(super) fn warn(&self) -> bool {
        self.sticky.store(true, Ordering::Relaxed);
        self.set(Some(TrackWarning::DigitalSilence))
    }

    /// Clears the live warning, keeping the sticky one; false when the
    /// controller's queue was full and a later chunk should try again.
    pub(super) fn clear(&self) -> bool {
        self.set(None)
    }

    /// Commits `warning` once the controller has it, so a dropped update is
    /// retried by the next chunk instead of leaving the UI stale.
    fn set(&self, warning: Option<TrackWarning>) -> bool {
        let mut live = self.live.lock().unwrap_or_else(|error| error.into_inner());
        if *live == warning {
            return true;
        }
        if let Some(updates) = &self.updates
            && updates
                .try_send(RecordingUpdate::SourceWarning { warning })
                .is_err()
        {
            return false;
        }
        *live = warning;
        true
    }
}

/// What one microphone chunk means for its stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SilenceAction {
    /// The chunk changes nothing.
    Nothing,
    /// The stream is digitally silent but the call app does not report
    /// recording from this input; logged once per episode, never acted on.
    Gated,
    /// The stream is digitally silent and this recording has a reopen left.
    Reopen,
    /// The stream is digitally silent and every reopen is spent: warn.
    Warn,
    /// Audible audio ended a digitally silent episode: clear the warning.
    Clear,
}

/// Counts consecutive exact-zero samples from the microphone so a stream
/// that keeps delivering digital silence while its chunks keep arriving, as
/// a call app holding the device with voice processing does, is noticed.
#[derive(Clone, Debug)]
struct SilenceWatch {
    rate: u32,
    zeros: u64,
}

impl SilenceWatch {
    fn new(rate: u32) -> Self {
        Self { rate, zeros: 0 }
    }

    /// Starts a fresh count for a newly opened input device or sample rate.
    fn reset(&mut self, rate: u32) {
        self.rate = rate;
        self.zeros = 0;
    }

    /// Counts `samples`, then reports whether the stream is digitally silent.
    fn observe(&mut self, samples: &[f32]) -> bool {
        for &sample in samples {
            if sample == 0.0 {
                self.zeros += 1;
            } else {
                self.zeros = 0;
            }
        }
        self.is_silent()
    }

    fn is_silent(&self) -> bool {
        self.rate > 0
            && self.zeros >= u64::from(self.rate).saturating_mul(SILENCE_TIMEOUT.as_secs())
    }
}

/// The per-chunk digital silence decision, kept by each opened source but
/// sharing one reopen budget and one sticky warning per recording.
pub(super) struct SilencePolicy {
    watch: SilenceWatch,
    reopens: Arc<AtomicU8>,
    /// The live warning is showing; cached so no chunk locks the mutex.
    warning: bool,
    /// This silent episode was already reported as gated by the call app.
    gated: bool,
}

impl SilencePolicy {
    /// `warning` is whether the live warning is already showing, which a
    /// source reopened after a supervisor restart inherits.
    pub(super) fn new(rate: u32, reopens: Arc<AtomicU8>, warning: bool) -> Self {
        Self {
            watch: SilenceWatch::new(rate),
            reopens,
            warning,
            gated: false,
        }
    }

    /// Starts a fresh zero count for a newly opened device, keeping the
    /// warning until audible audio clears it.
    pub(super) fn reset(&mut self, rate: u32) {
        self.watch.reset(rate);
    }

    /// Decides one chunk. The reopen and warn decisions are taken only while
    /// the call app records from this input, so a microphone a call app
    /// leaves digitally silent between user speech is not mistaken for a
    /// broken stream; a manual recording never reopens or warns.
    pub(super) fn observe(
        &mut self,
        samples: &[f32],
        call_app_holds_device: bool,
    ) -> SilenceAction {
        let silent = self.watch.observe(samples);
        if !silent {
            self.gated = false;
            let audible = samples.iter().any(|&sample| sample != 0.0);
            return if audible && self.warning {
                SilenceAction::Clear
            } else {
                SilenceAction::Nothing
            };
        }
        if !call_app_holds_device {
            if self.gated {
                return SilenceAction::Nothing;
            }
            self.gated = true;
            return SilenceAction::Gated;
        }
        if self.warning {
            return SilenceAction::Nothing;
        }
        if claim_silence_reopen(&self.reopens) {
            SilenceAction::Reopen
        } else {
            SilenceAction::Warn
        }
    }

    /// Records that `action` reached the controller, so a warning whose
    /// update was dropped is offered again by the next chunk.
    pub(super) fn applied(&mut self, action: SilenceAction) {
        match action {
            SilenceAction::Warn => self.warning = true,
            SilenceAction::Clear => self.warning = false,
            SilenceAction::Nothing | SilenceAction::Gated | SilenceAction::Reopen => {}
        }
    }
}

/// Claims one of the recording's silence reopens; false once they are spent.
fn claim_silence_reopen(reopens: &AtomicU8) -> bool {
    reopens
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
            (used < SILENCE_REOPENS).then_some(used + 1)
        })
        .is_ok()
}

/// Keeps `manifest.microphone.warning` on the recording's sticky flag,
/// writing it as soon as it is set so a crash while recording keeps it.
pub(super) fn persist_warning(
    directory: &Path,
    manifest: &mut MeetingManifest,
    warnings: &TrackWarnings,
) {
    let warning = warnings.persisted();
    if manifest.microphone.warning == warning {
        return;
    }
    let previous = manifest.microphone.warning;
    manifest.microphone.warning = warning;
    if let Err(error) = super::write_manifest(directory, manifest) {
        manifest.microphone.warning = previous;
        tracing::warn!(%error, meeting = manifest.id, "could not record the microphone warning");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_silence_budget_is_shared_across_reopened_streams() {
        let reopens = Arc::new(AtomicU8::new(0));
        let mut first = SilencePolicy::new(1_000, reopens.clone(), false);
        assert_eq!(first.observe(&[0.0; 3_000], true), SilenceAction::Reopen);
        let mut second = SilencePolicy::new(1_000, reopens.clone(), false);
        assert_eq!(second.observe(&[0.0; 3_000], true), SilenceAction::Reopen);
        let mut third = SilencePolicy::new(1_000, reopens.clone(), false);
        assert_eq!(third.observe(&[0.0; 3_000], true), SilenceAction::Warn);
        // The warning is offered again until the controller has it.
        assert_eq!(third.observe(&[0.0; 100], true), SilenceAction::Warn);
        third.applied(SilenceAction::Warn);
        assert_eq!(third.observe(&[0.0; 100], true), SilenceAction::Nothing);
    }

    #[test]
    fn audible_audio_clears_a_warning_a_reopened_stream_inherited() {
        let warnings = TrackWarnings::new(None);
        assert!(warnings.warn());
        let mut policy = SilencePolicy::new(
            1_000,
            Arc::new(AtomicU8::new(SILENCE_REOPENS)),
            warnings.live().is_some(),
        );
        assert_eq!(
            policy.observe(&[0.0, 0.25], true),
            SilenceAction::Clear,
            "a fresh stream must clear a warning it did not raise"
        );
        policy.applied(SilenceAction::Clear);
        assert_eq!(policy.observe(&[0.0, 0.25], true), SilenceAction::Nothing);
    }

    #[test]
    fn a_microphone_the_call_app_does_not_hold_is_never_reopened_or_warned() {
        let mut policy = SilencePolicy::new(1_000, Arc::new(AtomicU8::new(0)), false);
        // Reported once per episode for field logs, never reopened or warned.
        assert_eq!(policy.observe(&[0.0; 3_000], false), SilenceAction::Gated);
        assert_eq!(policy.observe(&[0.0; 3_000], false), SilenceAction::Nothing);
        // Audio returning starts a new episode, so a later one is reported.
        assert_eq!(policy.observe(&[0.25], false), SilenceAction::Nothing);
        assert_eq!(policy.observe(&[0.0; 3_000], false), SilenceAction::Gated);
    }

    #[test]
    fn a_sticky_warning_survives_its_live_one_clearing() {
        let warnings = TrackWarnings::new(None);
        assert!(warnings.warn());
        assert_eq!(warnings.live(), Some(TrackWarning::DigitalSilence));
        assert!(warnings.clear());
        assert_eq!(warnings.live(), None);
        assert_eq!(warnings.persisted(), Some(TrackWarning::DigitalSilence));
    }

    #[test]
    fn a_dropped_warning_update_is_not_committed_and_is_retried() {
        let (updates, receiver) = std::sync::mpsc::sync_channel(1);
        updates.try_send(RecordingUpdate::Transcribing).unwrap();
        let warnings = TrackWarnings::new(Some(updates));
        assert!(!warnings.warn(), "the full queue drops the update");
        assert_eq!(warnings.live(), None, "the live value waits for the UI");
        assert_eq!(receiver.try_recv().unwrap(), RecordingUpdate::Transcribing);
        assert!(warnings.warn(), "the next chunk retries it");
        assert_eq!(warnings.live(), Some(TrackWarning::DigitalSilence));
        assert!(matches!(
            receiver.try_recv().unwrap(),
            RecordingUpdate::SourceWarning {
                warning: Some(TrackWarning::DigitalSilence)
            }
        ));
    }

    #[test]
    fn a_warning_is_written_to_the_manifest_as_soon_as_it_is_raised() {
        let directory = std::env::temp_dir().join(format!(
            "hex-meeting-warning-{}-{}",
            std::process::id(),
            super::super::now_ms()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let mut manifest: MeetingManifest = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "id": "1760000000000-1",
            "title": "Old meeting",
            "status": "recording",
            "created_at_ms": 1_760_000_000_000_u64,
            "system": {"file": "system.wav", "samples": 0, "dropped_packets": 0},
            "microphone": {"file": "microphone.wav", "samples": 0, "dropped_packets": 0},
            "error": null,
        }))
        .unwrap();
        super::super::write_manifest(&directory, &manifest).unwrap();

        // The live warning clears, but the manifest keeps that it was silent.
        let warnings = TrackWarnings::new(None);
        assert!(warnings.warn());
        assert!(warnings.clear());
        persist_warning(&directory, &mut manifest, &warnings);
        assert_eq!(
            super::super::read_manifest(&directory)
                .unwrap()
                .microphone
                .warning,
            Some(TrackWarning::DigitalSilence)
        );
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_device_switch_restarts_the_zero_count_without_clearing_the_warning() {
        let warnings = TrackWarnings::new(None);
        assert!(warnings.warn());
        let mut policy = SilencePolicy::new(
            1_000,
            Arc::new(AtomicU8::new(SILENCE_REOPENS)),
            warnings.live().is_some(),
        );
        policy.reset(2_000);
        assert_eq!(policy.observe(&[0.0; 1_000], true), SilenceAction::Nothing);
        assert_eq!(policy.observe(&[0.0; 5_000], true), SilenceAction::Nothing);
        assert_eq!(policy.observe(&[0.25], true), SilenceAction::Clear);
    }
}

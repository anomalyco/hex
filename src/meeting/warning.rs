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
/// How many release-triggered renegotiating retries one silent episode gets
/// after its warning, reset when audible audio clears it.
const SILENCE_RETRIES: u8 = 3;

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
    /// The stream is digitally silent and is reopened: one of the recording's
    /// quick reopens, or a warned recording's release-triggered retry.
    Reopen,
    /// The stream is digitally silent and every quick reopen is spent: warn.
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

/// One recording's shared silence recovery: its quick reopen budget, the
/// release-triggered retry count, whether another app released the input
/// during the current silent episode, and whether a decided reopen still has
/// to renegotiate the device's rate. All survive the supervisor's reopens.
#[derive(Default)]
pub(super) struct SilenceRecovery {
    reopens: AtomicU8,
    retries: AtomicU8,
    /// Another application released the input while the stream was silent,
    /// even before the warning showed; a warned stream spends it as a retry.
    released: AtomicBool,
    /// A silence reopen was decided, but its open deferred the rate change
    /// because dictation was active; a still-silent stream finishes it once
    /// dictation is idle.
    renegotiation: AtomicBool,
}

impl SilenceRecovery {
    /// Whether the recording still has a quick reopen left, without
    /// spending it.
    fn has_reopen(&self) -> bool {
        self.reopens.load(Ordering::Relaxed) < SILENCE_REOPENS
    }

    /// Claims one of the recording's quick reopens; false once they are
    /// spent, which is when a warning is raised instead.
    fn claim_reopen(&self) -> bool {
        self.reopens
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                (used < SILENCE_REOPENS).then_some(used + 1)
            })
            .is_ok()
    }

    /// Claims one release-triggered renegotiating retry; false once this
    /// silent episode's are spent.
    fn claim_retry(&self) -> bool {
        self.retries
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                (used < SILENCE_RETRIES).then_some(used + 1)
            })
            .is_ok()
    }

    /// Keeps that an application released the input, even before the
    /// warning, so a warned stream still retries on it. Capture notes it
    /// where it is detected, so a failing source cannot drop the evidence.
    pub(super) fn note_release(&self) {
        self.released.store(true, Ordering::Relaxed);
    }

    /// Whether a release is waiting for a warned stream to spend it.
    fn has_release(&self) -> bool {
        self.released.load(Ordering::Relaxed)
    }

    /// Takes the pending release, if any, for this chunk's retry.
    fn take_release(&self) -> bool {
        self.released.swap(false, Ordering::Relaxed)
    }

    /// Forgets a release that belongs to an input this stream no longer
    /// records from.
    pub(super) fn forget_release(&self) {
        self.released.store(false, Ordering::Relaxed);
    }

    /// Marks that the next open of this recording's microphone must
    /// renegotiate its sample rate.
    pub(super) fn request_renegotiation(&self) {
        self.renegotiation.store(true, Ordering::Relaxed);
    }

    /// Whether a decided reopen still has to renegotiate the device's rate.
    pub(super) fn renegotiation_pending(&self) -> bool {
        self.renegotiation.load(Ordering::Relaxed)
    }

    /// Takes the pending renegotiation for an open to attempt now. It is
    /// one-shot like dictation's: taking it spends it even if the open
    /// fails, so a supervisor retry cannot flip the device's rate again.
    /// Dictation defers it instead, leaving it pending for a later open.
    pub(super) fn take_renegotiation(&self, dictation_busy: bool) -> bool {
        if dictation_busy {
            return false;
        }
        self.renegotiation.swap(false, Ordering::Relaxed)
    }

    /// Clears a pending renegotiation, such as one left on an input this
    /// stream no longer records from.
    pub(super) fn clear_renegotiation(&self) {
        self.renegotiation.store(false, Ordering::Relaxed);
    }

    /// Audible audio ended the silent episode: the next one gets its retries
    /// back, and no stale release or pending renegotiation carries over. The
    /// quick reopen budget stays spent for the recording, because each
    /// reopen changes the whole device's nominal sample rate.
    fn recovered(&self) {
        self.retries.store(0, Ordering::Relaxed);
        self.released.store(false, Ordering::Relaxed);
        self.renegotiation.store(false, Ordering::Relaxed);
    }
}

/// The per-chunk digital silence decision, kept by each opened source but
/// sharing one reopen budget, retry count, and sticky warning per recording.
pub(super) struct SilencePolicy {
    watch: SilenceWatch,
    recovery: Arc<SilenceRecovery>,
    /// The live warning is showing; cached so no chunk locks the mutex.
    warning: bool,
    /// This silent episode was already reported as gated by the call app.
    gated: bool,
    /// The reopen this chunk decided was a release-triggered retry, for its
    /// log.
    retrying: bool,
}

impl SilencePolicy {
    /// `warning` is whether the live warning is already showing, which a
    /// source reopened after a supervisor restart inherits.
    pub(super) fn new(rate: u32, recovery: Arc<SilenceRecovery>, warning: bool) -> Self {
        Self {
            watch: SilenceWatch::new(rate),
            recovery,
            warning,
            gated: false,
            retrying: false,
        }
    }

    /// Starts a fresh zero count for a newly opened device, keeping the
    /// warning until audible audio clears it.
    pub(super) fn reset(&mut self, rate: u32) {
        self.watch.reset(rate);
    }

    /// Decides one chunk. The quick reopens and the warning are decided only
    /// while the call app records from this input, so a microphone a call app
    /// leaves digitally silent between user speech is not mistaken for a
    /// broken stream; a manual recording never reopens or warns. Capture
    /// records a release of the input in the shared recovery where it is
    /// detected, even before the warning; after the warning it runs a
    /// renegotiating retry, at most [`SILENCE_RETRIES`] times per episode.
    /// A decided reopen whose rate change was deferred for dictation reopens
    /// a still-silent stream once dictation is idle, without spending a
    /// reopen or retry. Both kinds of reopen wait while dictation is active,
    /// so a rate change never interrupts dictation; `dictation_active` is
    /// called only on those paths.
    pub(super) fn observe(
        &mut self,
        samples: &[f32],
        call_app_holds_device: bool,
        dictation_active: impl Fn() -> bool,
    ) -> SilenceAction {
        self.retrying = false;
        let silent = self.watch.observe(samples);
        if !silent {
            self.gated = false;
            let audible = samples.iter().any(|&sample| sample != 0.0);
            if audible {
                self.recovery.recovered();
                return if self.warning {
                    SilenceAction::Clear
                } else {
                    SilenceAction::Nothing
                };
            }
        }
        if !silent {
            return SilenceAction::Nothing;
        }
        if self.recovery.renegotiation_pending() {
            if dictation_active() {
                return SilenceAction::Nothing;
            }
            // This completes a reopen already decided; it spends neither
            // the quick budget nor a retry.
            return SilenceAction::Reopen;
        }
        if self.warning {
            // Taking the release before claiming the retry spends it even
            // when the cap is spent, so it cannot fire a later retry.
            if self.recovery.has_release()
                && !dictation_active()
                && self.recovery.take_release()
                && self.recovery.claim_retry()
            {
                self.retrying = true;
                return SilenceAction::Reopen;
            }
            return SilenceAction::Nothing;
        }
        if !call_app_holds_device {
            if self.gated {
                return SilenceAction::Nothing;
            }
            self.gated = true;
            return SilenceAction::Gated;
        }
        // The warning changes nothing on the device, so it is raised even
        // while dictation is active; only a quick reopen waits.
        if !self.recovery.has_reopen() {
            return SilenceAction::Warn;
        }
        if dictation_active() {
            return SilenceAction::Nothing;
        }
        if self.recovery.claim_reopen() {
            SilenceAction::Reopen
        } else {
            SilenceAction::Warn
        }
    }

    /// Forgets a release that belongs to an input this stream no longer
    /// records from.
    pub(super) fn forget_release(&self) {
        self.recovery.forget_release();
    }

    /// Keeps a release capture detected, for a retry once the warning shows.
    pub(super) fn note_release(&self) {
        self.recovery.note_release();
    }

    /// Marks that the next open of this recording's microphone must
    /// renegotiate its sample rate.
    pub(super) fn request_renegotiation(&self) {
        self.recovery.request_renegotiation();
    }

    /// Forgets a renegotiation that belongs to an input this stream no
    /// longer records from.
    pub(super) fn clear_renegotiation(&self) {
        self.recovery.clear_renegotiation();
    }

    /// Whether the reopen the last chunk decided was a release-triggered
    /// retry, for its log.
    pub(super) fn retrying(&self) -> bool {
        self.retrying
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

    /// One recording's silence recovery with both quick reopens spent, so its
    /// next silent chunk warns.
    fn spent_recovery() -> Arc<SilenceRecovery> {
        let recovery = Arc::new(SilenceRecovery::default());
        recovery.reopens.store(SILENCE_REOPENS, Ordering::Relaxed);
        recovery
    }

    /// A spent recording whose warning is showing.
    fn warned_policy() -> (Arc<SilenceRecovery>, SilencePolicy) {
        let recovery = spent_recovery();
        let mut policy = SilencePolicy::new(1_000, recovery.clone(), false);
        assert_eq!(
            policy.observe(&[0.0; 3_000], true, || false),
            SilenceAction::Warn
        );
        policy.applied(SilenceAction::Warn);
        (recovery, policy)
    }

    #[test]
    fn a_silence_budget_is_shared_across_reopened_streams() {
        let recovery = Arc::new(SilenceRecovery::default());
        let mut first = SilencePolicy::new(1_000, recovery.clone(), false);
        assert_eq!(
            first.observe(&[0.0; 3_000], true, || false),
            SilenceAction::Reopen
        );
        let mut second = SilencePolicy::new(1_000, recovery.clone(), false);
        assert_eq!(
            second.observe(&[0.0; 3_000], true, || false),
            SilenceAction::Reopen
        );
        let mut third = SilencePolicy::new(1_000, recovery.clone(), false);
        assert_eq!(
            third.observe(&[0.0; 3_000], true, || false),
            SilenceAction::Warn
        );
        // The warning is offered again until the controller has it.
        assert_eq!(
            third.observe(&[0.0; 100], true, || false),
            SilenceAction::Warn
        );
        third.applied(SilenceAction::Warn);
        assert_eq!(
            third.observe(&[0.0; 100], true, || false),
            SilenceAction::Nothing
        );
    }

    #[test]
    fn a_warned_microphone_never_retries_without_a_release() {
        let (_, mut policy) = warned_policy();
        // Silence alone never renegotiates, however long it lasts and
        // whether or not the call app still reports the input.
        for call_app_holds_device in [true; 10].into_iter().chain([false; 10]) {
            assert_eq!(
                policy.observe(&[0.0; 100], call_app_holds_device, || false),
                SilenceAction::Nothing
            );
            assert!(!policy.retrying());
        }
    }

    #[test]
    fn a_release_triggers_a_warned_retry() {
        let (recovery, mut policy) = warned_policy();
        // Capture notes another app letting go of the opened input; that is
        // the change that can make a renegotiation succeed.
        recovery.note_release();
        assert_eq!(
            policy.observe(&[0.0; 100], true, || false),
            SilenceAction::Reopen
        );
        assert!(policy.retrying(), "logged as a retry, not a quick reopen");
        policy.applied(SilenceAction::Reopen);
    }

    #[test]
    fn a_warned_microphone_retries_after_the_call_app_lets_go() {
        let (recovery, mut policy) = warned_policy();
        recovery.note_release();
        assert_eq!(
            policy.observe(&[0.0; 100], false, || false),
            SilenceAction::Reopen
        );
        assert!(policy.retrying());
    }

    #[test]
    fn a_silent_episode_gets_three_release_triggered_retries() {
        let (recovery, mut policy) = warned_policy();
        for _ in 0..SILENCE_RETRIES {
            recovery.note_release();
            assert_eq!(
                policy.observe(&[0.0; 100], true, || false),
                SilenceAction::Reopen
            );
            policy.applied(SilenceAction::Reopen);
        }
        recovery.note_release();
        assert_eq!(
            policy.observe(&[0.0; 100], true, || false),
            SilenceAction::Nothing,
            "the episode's retries are spent"
        );
        // The cap is shared, so a stream the supervisor reopened gets none.
        let mut replacement = SilencePolicy::new(1_000, recovery.clone(), true);
        recovery.note_release();
        assert_eq!(
            replacement.observe(&[0.0; 3_000], true, || false),
            SilenceAction::Nothing
        );
    }

    #[test]
    fn a_rebuilt_policy_keeps_the_silent_episodes_retries() {
        let (recovery, mut first) = warned_policy();
        recovery.note_release();
        assert_eq!(
            first.observe(&[0.0; 100], true, || false),
            SilenceAction::Reopen
        );
        first.applied(SilenceAction::Reopen);
        // The supervisor reopened the stream; the replacement inherits the
        // warning, and needs its own release before another retry.
        let mut second = SilencePolicy::new(1_000, recovery.clone(), true);
        assert_eq!(
            second.observe(&[0.0; 3_000], true, || false),
            SilenceAction::Nothing
        );
        recovery.note_release();
        assert_eq!(
            second.observe(&[0.0; 100], true, || false),
            SilenceAction::Reopen
        );
        assert!(second.retrying());
    }

    #[test]
    fn a_release_noted_before_a_failed_source_still_retries_after_its_reopen() {
        let (recovery, warned) = warned_policy();
        // Capture records the release where it sees it, then the stream
        // fails before another chunk carries it; the shared state keeps it.
        recovery.note_release();
        drop(warned);
        let mut reopened = SilencePolicy::new(1_000, recovery, true);
        assert_eq!(
            reopened.observe(&[0.0; 3_000], true, || false),
            SilenceAction::Reopen
        );
        assert!(reopened.retrying());
    }

    #[test]
    fn a_reopen_waits_while_dictation_is_active() {
        let mut policy = SilencePolicy::new(1_000, Arc::new(SilenceRecovery::default()), false);
        assert_eq!(
            policy.observe(&[0.0; 3_000], true, || true),
            SilenceAction::Nothing,
            "a quick reopen must not renegotiate under a dictation"
        );
        assert_eq!(
            policy.observe(&[0.0; 100], true, || false),
            SilenceAction::Reopen
        );
    }

    #[test]
    fn a_spent_recording_warns_even_while_dictation_is_active() {
        let mut policy = SilencePolicy::new(1_000, spent_recovery(), false);
        assert_eq!(
            policy.observe(&[0.0; 3_000], true, || true),
            SilenceAction::Warn,
            "the warning changes nothing on the device, so dictation does not defer it"
        );
    }

    #[test]
    fn a_release_triggered_retry_waits_while_dictation_is_active() {
        let (recovery, mut policy) = warned_policy();
        recovery.note_release();
        assert_eq!(
            policy.observe(&[0.0; 100], true, || true),
            SilenceAction::Nothing
        );
        // The release is kept until dictation is idle.
        assert_eq!(
            policy.observe(&[0.0; 100], true, || false),
            SilenceAction::Reopen
        );
        assert!(policy.retrying());
    }

    #[test]
    fn a_release_seen_before_the_warning_retries_once_it_shows() {
        let recovery = spent_recovery();
        recovery.note_release();
        let mut policy = SilencePolicy::new(1_000, recovery, false);
        // The release was noted while the stream was silent but before the
        // warning: this chunk warns, and the release is kept.
        assert_eq!(
            policy.observe(&[0.0; 3_000], true, || false),
            SilenceAction::Warn
        );
        policy.applied(SilenceAction::Warn);
        assert_eq!(
            policy.observe(&[0.0; 100], true, || false),
            SilenceAction::Reopen
        );
        assert!(policy.retrying());
    }

    #[test]
    fn a_release_seen_before_a_reopen_still_retries_after_it() {
        let recovery = spent_recovery();
        recovery.note_release();
        let mut first = SilencePolicy::new(1_000, recovery.clone(), false);
        assert_eq!(
            first.observe(&[0.0; 3_000], true, || false),
            SilenceAction::Warn
        );
        first.applied(SilenceAction::Warn);
        drop(first);
        // The supervisor reopened the stream before the warning's next chunk.
        let mut second = SilencePolicy::new(1_000, recovery, true);
        assert_eq!(
            second.observe(&[0.0; 3_000], true, || false),
            SilenceAction::Reopen
        );
        assert!(second.retrying());
    }

    #[test]
    fn audible_audio_resets_the_episodes_retries() {
        let (recovery, mut warned) = warned_policy();
        for _ in 0..SILENCE_RETRIES {
            recovery.note_release();
            assert_eq!(
                warned.observe(&[0.0; 100], true, || false),
                SilenceAction::Reopen
            );
        }
        recovery.note_release();
        assert_eq!(
            warned.observe(&[0.0; 100], true, || false),
            SilenceAction::Nothing
        );

        // A reopened stream that hears audio clears the warning and the retry
        // cap, but the recording's two quick reopens stay spent.
        let mut replacement = SilencePolicy::new(1_000, recovery.clone(), true);
        assert_eq!(
            replacement.observe(&[0.25], true, || false),
            SilenceAction::Clear
        );
        replacement.applied(SilenceAction::Clear);
        // The next silent episode warns at once, then gets its own retries.
        assert_eq!(
            replacement.observe(&[0.0; 3_000], true, || false),
            SilenceAction::Warn
        );
        replacement.applied(SilenceAction::Warn);
        recovery.note_release();
        assert_eq!(
            replacement.observe(&[0.0; 100], true, || false),
            SilenceAction::Reopen
        );
        assert!(replacement.retrying());
    }

    #[test]
    fn a_pending_renegotiation_reopens_once_dictation_is_idle() {
        let recovery = Arc::new(SilenceRecovery::default());
        recovery.request_renegotiation();
        let mut policy = SilencePolicy::new(1_000, recovery.clone(), false);
        // Silent but dictation is active: wait, without warning or spending
        // the quick reopen that is still available.
        assert_eq!(
            policy.observe(&[0.0; 3_000], true, || true),
            SilenceAction::Nothing
        );
        assert_eq!(recovery.reopens.load(Ordering::Relaxed), 0);
        assert_eq!(recovery.retries.load(Ordering::Relaxed), 0);
        // Idle: finish the reopen already decided.
        assert_eq!(
            policy.observe(&[0.0; 100], true, || false),
            SilenceAction::Reopen
        );
        assert!(!policy.retrying());
        assert_eq!(recovery.reopens.load(Ordering::Relaxed), 0);
        assert_eq!(recovery.retries.load(Ordering::Relaxed), 0);
        // The renegotiating open takes it when it is attempted; a later
        // silent chunk then spends a normal quick reopen.
        assert!(recovery.take_renegotiation(false));
        assert_eq!(
            policy.observe(&[0.0; 100], true, || false),
            SilenceAction::Reopen
        );
        assert_eq!(recovery.reopens.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_renegotiation_is_taken_once_and_dictation_defers_it() {
        let recovery = SilenceRecovery::default();
        assert!(!recovery.take_renegotiation(false), "nothing was pending");
        recovery.request_renegotiation();
        assert!(!recovery.take_renegotiation(true), "dictation defers it");
        assert!(
            recovery.renegotiation_pending(),
            "a deferred renegotiation stays pending"
        );
        assert!(recovery.take_renegotiation(false), "idle takes it");
        assert!(!recovery.renegotiation_pending());
        assert!(
            !recovery.take_renegotiation(false),
            "taking it spends it, so a failed open's retry is plain"
        );
    }

    #[test]
    fn audible_audio_clears_a_pending_renegotiation() {
        let (recovery, mut warned) = warned_policy();
        recovery.request_renegotiation();
        assert_eq!(
            warned.observe(&[0.25], true, || false),
            SilenceAction::Clear
        );
        assert!(!recovery.renegotiation_pending());
    }

    #[test]
    fn audible_audio_clears_a_warning_a_reopened_stream_inherited() {
        let warnings = TrackWarnings::new(None);
        assert!(warnings.warn());
        let mut policy = SilencePolicy::new(1_000, spent_recovery(), warnings.live().is_some());
        assert_eq!(
            policy.observe(&[0.0, 0.25], true, || false),
            SilenceAction::Clear,
            "a fresh stream must clear a warning it did not raise"
        );
        policy.applied(SilenceAction::Clear);
        assert_eq!(
            policy.observe(&[0.0, 0.25], true, || false),
            SilenceAction::Nothing
        );
    }

    #[test]
    fn a_microphone_the_call_app_does_not_hold_is_never_reopened_or_warned() {
        let mut policy = SilencePolicy::new(1_000, Arc::new(SilenceRecovery::default()), false);
        // Reported once per episode for field logs, never reopened or warned.
        assert_eq!(
            policy.observe(&[0.0; 3_000], false, || false),
            SilenceAction::Gated
        );
        assert_eq!(
            policy.observe(&[0.0; 3_000], false, || false),
            SilenceAction::Nothing
        );
        // Audio returning starts a new episode, so a later one is reported.
        assert_eq!(
            policy.observe(&[0.25], false, || false),
            SilenceAction::Nothing
        );
        assert_eq!(
            policy.observe(&[0.0; 3_000], false, || false),
            SilenceAction::Gated
        );
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
        let mut policy = SilencePolicy::new(1_000, spent_recovery(), warnings.live().is_some());
        policy.reset(2_000);
        assert_eq!(
            policy.observe(&[0.0; 1_000], true, || false),
            SilenceAction::Nothing
        );
        assert_eq!(
            policy.observe(&[0.0; 5_000], true, || false),
            SilenceAction::Nothing
        );
        assert_eq!(
            policy.observe(&[0.25], true, || false),
            SilenceAction::Clear
        );
    }
}

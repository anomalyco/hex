//! The listener's microphone: the one stream Hex keeps open, reopened after a
//! failure or a device change with `recovery`'s backoff, and closed while it
//! is not needed.

use std::sync::Arc;
#[cfg(test)]
use std::sync::mpsc::Sender;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};

use super::CaptureInstant;
use super::interruption;
use super::open::{InputOpenResult, OpenRequest, SelectedFallback, open_configured_input};
use super::recovery::{MICROPHONE_RETRY_INITIAL, MICROPHONE_RETRY_MAX, MicrophoneRecovery};
use super::routes::input_route_generation;
use super::stream::{
    AudioInput, AudioInputEvent, Opened, STREAM_PROVEN_UPTIME, Selection, VoicePath,
};

type Opener = Arc<dyn Fn(&OpenRequest) -> Result<Opened> + Send + Sync>;

/// Configuration-change reopens inside this window count toward a loop.
const VOICE_PROCESSING_RECONFIGURE_WINDOW: Duration = Duration::from_secs(30);
/// More than this many reopens in the window means the engine keeps
/// reconfiguring, so voice processing is remembered as failed.
const VOICE_PROCESSING_RECONFIGURE_LIMIT: usize = 3;

/// Whether the installed stream has delivered chunks across
/// [`STREAM_PROVEN_UPTIME`] without stopping. A stream that fails before
/// reaching `Proven` never proved steady delivery, so its reopen backs off
/// instead of repeating the stall timeout forever (#1681).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum StreamDelivery {
    #[default]
    Unobserved,
    Observing {
        since: Instant,
    },
    Proven,
}

impl StreamDelivery {
    /// Records a delivered chunk at `now`. Returns `true` on the transition
    /// that proves steady delivery.
    fn observe(&mut self, now: Instant) -> bool {
        match *self {
            Self::Proven => false,
            Self::Unobserved => {
                *self = Self::Observing { since: now };
                false
            }
            Self::Observing { since } => {
                if now.saturating_duration_since(since) >= STREAM_PROVEN_UPTIME {
                    *self = Self::Proven;
                    true
                } else {
                    false
                }
            }
        }
    }

    fn is_proven(self) -> bool {
        matches!(self, Self::Proven)
    }
}

/// Why no microphone opens, reported at most once per outage.
#[derive(Default)]
enum Outage {
    #[default]
    None,
    Unreported(String),
    Reported,
}

impl Outage {
    fn record(&mut self, error: String) {
        if matches!(self, Self::None) {
            *self = Self::Unreported(error);
        }
    }

    fn take(&mut self) -> Option<String> {
        if matches!(self, Self::Unreported(_)) {
            match std::mem::replace(self, Self::Reported) {
                Self::Unreported(error) => Some(error),
                _ => unreachable!(),
            }
        } else {
            None
        }
    }

    fn clear(&mut self) {
        *self = Self::None;
    }
}

/// Labs › Voice isolation policy for dictation's stream.
enum VoiceIsolation {
    Off,
    /// Enabled; tries Apple's voice processing and tracks recent engine
    /// configuration changes to distinguish one route change from a loop.
    Active {
        reconfigures: Vec<Instant>,
    },
    /// Voice processing failed on this microphone: stay on the raw stream
    /// until the lab is toggled or the microphone changes.
    Fallback,
}

impl VoiceIsolation {
    fn from_enabled(enabled: bool) -> Self {
        if enabled {
            Self::Active {
                reconfigures: Vec::new(),
            }
        } else {
            Self::Off
        }
    }

    fn enabled(&self) -> bool {
        !matches!(self, Self::Off)
    }

    fn is_fallback(&self) -> bool {
        matches!(self, Self::Fallback)
    }

    fn wanted_path(&self) -> VoicePath {
        match self {
            Self::Off => VoicePath::Raw,
            Self::Active { .. } => VoicePath::VoiceProcessing,
            Self::Fallback => VoicePath::VoiceProcessingFallback,
        }
    }

    fn set_enabled(&mut self, enabled: bool) {
        if enabled != self.enabled() {
            *self = Self::from_enabled(enabled);
        }
    }

    fn reset_fallback(&mut self) {
        if self.enabled() {
            *self = Self::Active {
                reconfigures: Vec::new(),
            };
        }
    }

    fn mark_fallback(&mut self, reason: &str) {
        if !matches!(self, Self::Active { .. }) {
            return;
        }
        *self = Self::Fallback;
        tracing::warn!(
            %reason,
            "voice processing failed; using normal capture until Labs › Voice isolation is toggled or the microphone changes"
        );
    }

    fn note_reconfigure(&mut self, now: Instant, device: &str) {
        let Self::Active { reconfigures } = self else {
            return;
        };
        reconfigures
            .retain(|at| now.saturating_duration_since(*at) <= VOICE_PROCESSING_RECONFIGURE_WINDOW);
        reconfigures.push(now);
        if reconfigures.len() > VOICE_PROCESSING_RECONFIGURE_LIMIT {
            let reason = format!("{device} reconfigured repeatedly");
            self.mark_fallback(&reason);
        }
    }
}

pub struct RecoveringAudioInput {
    input: Option<AudioInput>,
    delivery: StreamDelivery,
    device_override: Option<String>,
    selected_device: Option<String>,
    voice_isolation: VoiceIsolation,
    /// This replacement is a silence recovery, which opens raw to renegotiate
    /// the device's clock; its install must not remember a voice-processing
    /// failure.
    voice_isolation_skipped: bool,
    /// The resolved microphone last installed, kept while the stream is
    /// closed so a reopen on another device can be told apart.
    resolved_device: Option<(String, Option<String>)>,
    active_revision: u64,
    recovery: MicrophoneRecovery,
    replacement: Option<Receiver<InputOpenResult>>,
    /// The [`input_route_generation`] the open input was resolved at.
    route_generation: u64,
    /// The wait before looking again for a missing saved microphone.
    selection_retry: Duration,
    /// The OS reports devices arriving, so an unlisted saved microphone's
    /// return arrives as a route change and needs no timed look.
    reports_new_devices: bool,
    opener: Opener,
    route_generation_source: fn() -> u64,
    outage: Outage,
    /// The retry error last logged, so a microphone that stays gone (a closed
    /// lid, an unplugged headset) logs once, not on every retry.
    retry_warning: Option<String>,
}

pub enum RecoveringAudioInputEvent {
    Chunk {
        samples: Vec<f32>,
        captured_through: CaptureInstant,
    },
    Timeout,
    Interrupted,
    Reopened,
    OpenFailed(String),
}

impl RecoveringAudioInput {
    pub fn open(
        device_override: Option<&str>,
        selection_revision: u64,
        selected_device: Option<&str>,
    ) -> Self {
        let mut input = Self::closed(device_override, selection_revision, selected_device);
        let opened = open_configured_input(&OpenRequest {
            device_override: device_override.map(str::to_owned),
            selected_device: selected_device.map(str::to_owned),
            fallback: SelectedFallback::Immediate,
            keep: None,
            avoids_bluetooth: true,
            call_input: None,
            renegotiate_rate: false,
            voice_processing: input.voice_isolation.enabled(),
            voice_processing_fallback: false,
        })
        .and_then(|opened| match opened {
            Opened::Input(input) => Ok(*input),
            Opened::Unchanged { .. } => Err(eyre!("nothing was open to keep")),
        });
        input.finish_initial_open(opened, Instant::now());
        input
    }

    fn finish_initial_open(&mut self, opened: Result<AudioInput>, now: Instant) {
        match opened {
            Ok(opened) => {
                self.install(opened);
                self.after_resolving(now);
            }
            Err(error) => {
                let error = format!("{error:#}");
                tracing::warn!(%error, "could not open dictation microphone at startup; retrying");
                self.voice_isolation.mark_fallback(&error);
                self.outage.record(error);
                self.recovery.request_stream_recovery(now);
                self.recovery.failed(now);
            }
        }
    }

    pub fn closed(
        device_override: Option<&str>,
        selection_revision: u64,
        selected_device: Option<&str>,
    ) -> Self {
        Self {
            input: None,
            delivery: StreamDelivery::Unobserved,
            device_override: device_override.map(str::to_owned),
            selected_device: selected_device.map(str::to_owned),
            voice_isolation: VoiceIsolation::from_enabled(crate::labs::voice_isolation()),
            voice_isolation_skipped: false,
            resolved_device: None,
            active_revision: selection_revision,
            recovery: MicrophoneRecovery::default(),
            replacement: None,
            route_generation: input_route_generation(),
            selection_retry: MICROPHONE_RETRY_INITIAL,
            reports_new_devices: cfg!(target_os = "macos"),
            opener: Arc::new(open_configured_input),
            route_generation_source: input_route_generation,
            outage: Outage::None,
            retry_warning: None,
        }
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn take_failure(&mut self) -> Option<String> {
        self.outage.take()
    }

    /// An input whose opens and route changes come from the test.
    #[cfg(test)]
    fn with_opener_for_test(
        selected_device: Option<&str>,
        route_generation: fn() -> u64,
        opener: impl Fn(&OpenRequest) -> Result<Opened> + Send + Sync + 'static,
    ) -> Self {
        let mut input = Self::closed(None, 0, selected_device);
        input.route_generation_source = route_generation;
        input.route_generation = route_generation();
        // A test's opens follow its own requests, never the saved setting.
        input.voice_isolation = VoiceIsolation::Off;
        input.voice_isolation_skipped = false;
        input.resolved_device = None;
        input.opener = Arc::new(opener);
        input
    }

    /// An input that opens into a stream `opener` makes, without naming the
    /// request: for a test in another module that needs each open to finish
    /// with a stream of the test's choosing.
    #[cfg(test)]
    pub(crate) fn with_stream_opener_for_test(
        selected_device: Option<&str>,
        opener: impl Fn() -> Result<Opened> + Send + Sync + 'static,
    ) -> Self {
        Self::with_opener_for_test(selected_device, input_route_generation, move |_| opener())
    }

    /// An input whose startup open failed, without touching CoreAudio.
    #[cfg(test)]
    pub fn failed_for_test(device_override: &str) -> Self {
        let mut input = Self::closed(Some(device_override), 0, None);
        input.finish_initial_open(Err(eyre!("{device_override} not found")), Instant::now());
        input
    }

    #[cfg(test)]
    pub fn pending_for_test() -> (Self, Sender<InputOpenResult>) {
        let mut input = Self::closed(
            Some("Hex nonexistent microphone for pending open test"),
            0,
            None,
        );
        let (sender, receiver) = mpsc::channel();
        input.replacement = Some(receiver);
        (input, sender)
    }

    pub fn request_open(&mut self) {
        if self.input.is_none() && self.replacement.is_none() && !self.is_recovering() {
            self.start_replacement();
        }
    }

    pub fn request_recovery(&mut self) {
        self.recovery.request_stream_recovery(Instant::now());
    }

    /// Labs › Voice isolation: while `enabled`, the next open goes through
    /// Apple's voice processing, and a stream of the other kind is replaced
    /// once `capture_idle`. A capture in progress keeps its stream. Toggling
    /// the lab forgets a remembered failure, so it is tried again.
    pub fn request_voice_isolation(&mut self, enabled: bool, capture_idle: bool) {
        self.voice_isolation.set_enabled(enabled);
        if !capture_idle || self.replacement.is_some() || self.recovery.blocks_audio() {
            return;
        }
        let mismatched = self
            .input
            .as_ref()
            .is_some_and(|input| !self.open_input_matches(input));
        if mismatched {
            self.start_replacement();
        }
    }

    /// Whether `input` already runs on the path Hex wants now.
    fn open_input_matches(&self, input: &AudioInput) -> bool {
        input.voice_path == self.voice_isolation.wanted_path()
    }

    /// A finished dictation was digital silence (#1122): reopen the stream and
    /// make that open renegotiate the device's sample rate first, which clears
    /// a device left on a stale clock (#1575).
    pub fn request_silence_recovery(&mut self) {
        self.recovery.request_silence_recovery(Instant::now());
    }

    pub fn is_recovering(&self) -> bool {
        self.recovery.blocks_audio()
    }

    /// Whether the installed stream has stopped delivering audio before the
    /// stall watchdog's next poll reports it.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn is_stalled(&self) -> bool {
        self.input
            .as_ref()
            .is_some_and(|input| interruption::stalled(input.stall_after, &input.last_delivery))
    }

    /// A dictation press asks for the microphone now instead of waiting out a
    /// failed stream's backoff. The grown backoff stays, so pressing again
    /// cannot restore the fast reopen loop; the press itself still cannot
    /// capture until the stream opens.
    pub fn retry_now(&mut self) {
        if self.replacement.is_none() {
            self.recovery.retry_now(Instant::now());
        }
    }

    /// Makes `input` the listener's microphone and the only stream feeding
    /// Hex Microphone, stopping the previous stream first.
    fn install(&mut self, input: AudioInput) {
        self.install_after_open(input, false);
    }

    /// Installs a stream an open produced. `voice_isolation_skipped` marks the
    /// raw silence-recovery open, whose raw stream is deliberate and not a
    /// voice-processing failure to remember.
    fn install_after_open(&mut self, input: AudioInput, voice_isolation_skipped: bool) {
        let device_changed = self.resolved_device.as_ref().is_some_and(|(name, uid)| {
            match (uid.as_deref(), input.uid.as_deref()) {
                (Some(previous), Some(next)) => previous != next,
                _ => name != &input.device_name,
            }
        });
        self.resolved_device = Some((input.device_name.clone(), input.uid.clone()));
        let had_fallback = self.voice_isolation.is_fallback();
        if device_changed {
            // A different resolved microphone, even one the saved setting
            // didn't name (a stand-in, the OS default, or a route change),
            // gets a fresh try at voice processing.
            self.voice_isolation.reset_fallback();
        }
        let fell_back = input.voice_path.fell_back();
        // A raw open made because the old fallback skipped voice processing
        // says nothing about the new device; the next idle pass tries it.
        if !voice_isolation_skipped && fell_back && !(device_changed && had_fallback) {
            let reason = format!(
                "{} did not open through voice processing",
                input.device_name
            );
            self.voice_isolation.mark_fallback(&reason);
        }
        self.retire_input();
        input.feed_hex_microphone(true);
        self.input = Some(input);
        self.outage.clear();
        self.retry_warning = None;
    }

    /// Stops the open stream and drops it, so no stream of Hex's is left on
    /// the device.
    fn retire_input(&mut self) {
        self.delivery = StreamDelivery::Unobserved;
        if let Some(previous) = self.input.take() {
            previous.feed_hex_microphone(false);
            drop(previous);
        }
    }

    pub fn close(&mut self) {
        if let Some(previous) = self.input.take() {
            previous.feed_hex_microphone(false);
        }
        self.delivery = StreamDelivery::Unobserved;
        self.replacement = None;
        self.voice_isolation_skipped = false;
        self.recovery.recovered();
        self.outage.clear();
        self.retry_warning = None;
    }

    pub fn is_open(&self) -> bool {
        self.input.is_some()
    }

    pub fn is_opening(&self) -> bool {
        self.replacement.is_some()
    }

    pub fn is_active(&self) -> bool {
        self.is_open() || self.is_opening() || self.is_recovering()
    }

    pub fn cancel_open(&mut self) {
        // Cancelling a capture must not cancel the warm microphone's recovery.
        if self.is_recovering() {
            return;
        }
        self.replacement = None;
        self.voice_isolation_skipped = false;
        if self.input.is_none() {
            self.recovery.recovered();
        }
    }

    pub fn request_selection(&mut self, revision: u64, selected_device: Option<&str>) {
        if self.device_override.is_some() || revision == self.active_revision {
            return;
        }
        self.selected_device = selected_device.map(str::to_owned);
        self.voice_isolation.reset_fallback();
        if self.input.is_none() && !self.is_recovering() {
            let was_opening = self.replacement.take().is_some();
            self.voice_isolation_skipped = false;
            self.active_revision = revision;
            self.recovery.recovered();
            if was_opening {
                self.start_replacement();
            }
            return;
        }
        self.recovery
            .request_selection_change(revision, Instant::now());
    }

    pub fn recv_timeout(
        &mut self,
        timeout: Duration,
        capture_idle: bool,
    ) -> RecoveringAudioInputEvent {
        let now = Instant::now();
        if let Some(((opened_revision, result), skipped)) = self.poll_replacement() {
            let target_revision = self
                .recovery
                .target_revision
                .unwrap_or(self.active_revision);
            if opened_revision != target_revision {
                self.recovery.next_attempt = Some(now);
                return RecoveringAudioInputEvent::Timeout;
            }
            match result {
                Ok(Opened::Unchanged { selection }) => {
                    let Some(input) = self.input.as_mut() else {
                        // Nothing was open to keep; open a fresh input.
                        self.recovery.request_stream_recovery(now);
                        return RecoveringAudioInputEvent::Timeout;
                    };
                    // A stream that failed while this open was in flight is
                    // dead; open it again rather than keeping it.
                    if self.recovery.blocks_audio() {
                        self.recovery.next_attempt = Some(now);
                        return RecoveringAudioInputEvent::Timeout;
                    }
                    input.selection = selection;
                    self.active_revision = opened_revision;
                    // The stream itself kept running, so a failed stream's
                    // backoff it still owes is kept too.
                    self.recovery.reopened();
                    self.after_resolving(now);
                }
                Ok(Opened::Input(replacement))
                    if capture_idle || self.input.is_none() || self.recovery.blocks_audio() =>
                {
                    self.install_after_open(*replacement, skipped);
                    self.active_revision = opened_revision;
                    // A failed stream keeps the backoff it grew until this
                    // one delivers steadily.
                    self.recovery.reopened();
                    self.after_resolving(now);
                    let reopened = self.input.as_ref().expect("just installed");
                    tracing::info!(
                        device = %reopened.device_name,
                        uid = reopened.uid.as_deref().unwrap_or("none"),
                        sample_rate = reopened.sample_rate,
                        "dictation microphone reopened"
                    );
                    return RecoveringAudioInputEvent::Reopened;
                }
                Ok(Opened::Input(_)) => {
                    self.recovery.failed(now);
                }
                Err(error) => {
                    if !skipped {
                        self.voice_isolation.mark_fallback(&error);
                    }
                    if self.input.is_none() && !self.is_recovering() {
                        self.recovery.recovered();
                        return RecoveringAudioInputEvent::OpenFailed(error);
                    }
                    self.recovery.failed(now);
                    if self.retry_warning.as_ref() != Some(&error) {
                        tracing::warn!(
                            %error,
                            "could not reopen dictation microphone; retrying"
                        );
                        self.retry_warning = Some(error.clone());
                    }
                    if self.recovery.blocks_audio() {
                        self.outage.record(error);
                    }
                }
            }
        }
        // A change while an open is in flight is seen once it finishes. A
        // failed stream with nothing open is resolved again as well: a lid
        // opening must reach the built-in microphone at once.
        if self.device_override.is_none()
            && self.replacement.is_none()
            && (self.input.is_some() || self.recovery.blocks_audio())
        {
            let generation = (self.route_generation_source)();
            if generation != self.route_generation {
                self.route_generation = generation;
                self.recovery.request_route_check(now);
            }
        }
        // A failed stream delivers nothing, so a press waiting for it never
        // holds the microphone dead: its replacement starts even though the
        // press keeps the capture from counting as idle.
        if (capture_idle || self.input.is_none() || self.recovery.blocks_audio())
            && self.recovery.should_attempt(now)
            && self.replacement.is_none()
        {
            self.start_replacement();
        }
        if self.recovery.blocks_audio() {
            std::thread::sleep(timeout);
            return RecoveringAudioInputEvent::Timeout;
        }
        let Some(input) = self.input.as_ref() else {
            std::thread::sleep(timeout);
            return RecoveringAudioInputEvent::Timeout;
        };
        match input.recv_timeout(timeout) {
            AudioInputEvent::Chunk {
                samples,
                captured_through,
            } => {
                self.note_delivery(Instant::now());
                RecoveringAudioInputEvent::Chunk {
                    samples,
                    captured_through,
                }
            }
            AudioInputEvent::Timeout => RecoveringAudioInputEvent::Timeout,
            AudioInputEvent::StreamReconfigured => {
                let now = Instant::now();
                let device = input.device_name.clone();
                tracing::info!(
                    device = %device,
                    "voice processing engine reconfigured; reopening the microphone"
                );
                self.voice_isolation.note_reconfigure(now, &device);
                self.recovery.request_stream_recovery(now);
                RecoveringAudioInputEvent::Interrupted
            }
            AudioInputEvent::StreamFailed(error) => {
                let failed_at = Instant::now();
                let voice_processing_failed = input.voice_path.active();
                let device = input.device_name.clone();
                tracing::warn!(%error, device = %device, "microphone stream stopped; reopening it");
                if voice_processing_failed {
                    let reason =
                        format!("{device} stopped delivering audio through voice processing");
                    self.voice_isolation.mark_fallback(&reason);
                }
                if self.delivery.is_proven() {
                    self.recovery.request_stream_recovery(failed_at);
                } else {
                    // Release the dead stream before the wait starts: Hex
                    // holds no microphone while it backs off.
                    self.retire_input();
                    self.recovery.back_off_stream_recovery(failed_at);
                }
                RecoveringAudioInputEvent::Interrupted
            }
        }
    }

    /// Records that the installed stream delivered a chunk. A stream that has
    /// delivered across [`STREAM_PROVEN_UPTIME`] has proven it works: the
    /// failed streams before it no longer count against it.
    fn note_delivery(&mut self, now: Instant) {
        if self.delivery.observe(now) {
            self.recovery.stream_proved();
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.input
            .as_ref()
            .expect("microphone sample rate requested while closed")
            .sample_rate
    }

    pub fn device_name(&self) -> &str {
        &self
            .input
            .as_ref()
            .expect("microphone name requested while closed")
            .device_name
    }

    fn start_replacement(&mut self) {
        let revision = self
            .recovery
            .target_revision
            .unwrap_or(self.active_revision);
        // A failed stream is replaced even by the same device; a route,
        // selection, or voice-isolation change keeps the open device only
        // when it already runs on the path Hex wants now.
        let keep = self
            .input
            .as_ref()
            .filter(|input| !self.recovery.blocks_audio() && self.open_input_matches(input))
            .map(|input| input.device_name.clone());
        // A failed or silent stream lets go of the device before its
        // replacement opens, so the replacement starts cold, as at launch. A
        // live stream stays until its replacement is ready to install.
        if self.recovery.blocks_audio() {
            self.retire_input();
        }
        let renegotiate_rate = self.recovery.take_renegotiation();
        // A silence recovery must change the device's rate before its stream
        // opens; voice processing builds its own engine and cannot, so that
        // one open stays raw and the next idle pass restores it.
        self.voice_isolation_skipped = self.voice_isolation.enabled() && renegotiate_rate;
        let request = OpenRequest {
            device_override: self.device_override.clone(),
            selected_device: self.selected_device.clone(),
            fallback: SelectedFallback::AfterRetries,
            keep,
            avoids_bluetooth: true,
            call_input: None,
            renegotiate_rate,
            voice_processing: self.voice_isolation.enabled(),
            voice_processing_fallback: self.voice_isolation.is_fallback() || renegotiate_rate,
        };
        self.route_generation = (self.route_generation_source)();
        let opener = self.opener.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = opener(&request).map_err(|error| format!("{error:#}"));
            let _ = sender.send((revision, result));
        });
        self.replacement = Some(receiver);
    }

    /// While the open input stands in for a saved microphone that would not
    /// open, or that is not listed where the OS doesn't report new devices,
    /// looks for it again with bounded backoff. A device change looks at once.
    fn after_resolving(&mut self, now: Instant) {
        let look_again = match self.input.as_ref().map(|input| input.selection) {
            Some(Selection::Unopenable) => true,
            Some(Selection::Unlisted | Selection::LidClosed) => !self.reports_new_devices,
            Some(Selection::InUse) | None => false,
        };
        if look_again {
            self.recovery
                .request_route_check(now + self.selection_retry);
            self.selection_retry = self
                .selection_retry
                .saturating_mul(2)
                .min(MICROPHONE_RETRY_MAX);
        } else {
            self.selection_retry = MICROPHONE_RETRY_INITIAL;
        }
    }

    fn poll_replacement(&mut self) -> Option<(InputOpenResult, bool)> {
        let skipped = self.voice_isolation_skipped;
        let result = match self.replacement.as_ref()?.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => (
                self.active_revision,
                Err("microphone replacement worker stopped".into()),
            ),
        };
        self.replacement = None;
        self.voice_isolation_skipped = false;
        Some((result, skipped))
    }
}

#[cfg(test)]
mod tests;

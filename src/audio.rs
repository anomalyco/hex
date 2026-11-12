#[cfg(target_os = "macos")]
pub(crate) mod browser_media;
mod call_input;
mod fixture;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) mod hex_microphone;
pub(crate) mod idle_sleep;
mod input_format;
pub(crate) mod input_level;
pub(crate) mod instant;
#[cfg(target_os = "macos")]
mod lid;
pub(crate) mod linger;
#[cfg(any(target_os = "windows", test))]
mod mix_format;
pub(crate) mod output_ducking;
pub(crate) mod output_route;
mod overloads;
#[cfg(target_os = "macos")]
pub(crate) mod recording_environment;
mod recovery;
mod transport;
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub(crate) mod voice_filter;
#[cfg(target_os = "windows")]
mod wasapi_capture;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use recovery::{MICROPHONE_RETRY_INITIAL, MICROPHONE_RETRY_MAX, MicrophoneRecovery};

static AUDIO_SERVICE_RESTART_GENERATION: AtomicU64 = AtomicU64::new(0);
static INPUT_ROUTE_GENERATION: AtomicU64 = AtomicU64::new(0);
static AVOID_BLUETOOTH_MICROPHONE: AtomicBool = AtomicBool::new(true);

/// Whether `HEX_AUDIO_FIXTURE` names a WAV file that stands in for every
/// microphone (benchmarks and tests; see `audio/fixture.rs`).
pub fn uses_fixture() -> bool {
    fixture::path().is_some()
}

/// Whether System default passes over a Bluetooth default input, whose
/// microphone drops the headset into low-quality call audio. A change
/// resolves the input again once capture is idle.
pub fn set_avoid_bluetooth_microphone(avoid: bool) {
    if AVOID_BLUETOOTH_MICROPHONE.swap(avoid, Ordering::AcqRel) != avoid {
        INPUT_ROUTE_GENERATION.fetch_add(1, Ordering::AcqRel);
    }
}

fn avoid_bluetooth_microphone() -> bool {
    AVOID_BLUETOOTH_MICROPHONE.load(Ordering::Acquire)
}

/// How long ago a microphone stream last reported an overload it continued
/// through, for diagnosing captures.
pub fn since_last_stream_glitch() -> Option<Duration> {
    overloads::since_last()
}

pub fn notify_audio_service_restarted() {
    AUDIO_SERVICE_RESTART_GENERATION.fetch_add(1, Ordering::AcqRel);
}

pub fn audio_service_restart_generation() -> u64 {
    AUDIO_SERVICE_RESTART_GENERATION.load(Ordering::Acquire)
}

/// Advances when the OS's default input or its list of inputs changes, or
/// a MacBook's lid opens or closes, so an input following the system
/// default, or standing in for a missing saved microphone, resolves again.
/// macOS reports these through CoreAudio listeners and an IOKit interest
/// notification; Windows reports a default change through its stream.
pub fn input_route_generation() -> u64 {
    INPUT_ROUTE_GENERATION.load(Ordering::Acquire)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn notify_input_route_changed() {
    INPUT_ROUTE_GENERATION.fetch_add(1, Ordering::AcqRel);
}

static OUTPUT_ROUTE_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Advances when the OS's default output or its list of devices changes, or
/// the audio service restarts, so a warm feedback output opened on an older
/// route reopens on the current default before its next tone. Only macOS
/// reports these, through the same CoreAudio listeners as the input route.
pub fn output_route_generation() -> u64 {
    OUTPUT_ROUTE_GENERATION
        .load(Ordering::Acquire)
        .wrapping_add(audio_service_restart_generation())
}

#[cfg(target_os = "macos")]
fn notify_output_route_changed() {
    OUTPUT_ROUTE_GENERATION.fetch_add(1, Ordering::AcqRel);
}

/// Registers the CoreAudio listeners behind [`audio_service_restart_generation`]
/// [`input_route_generation`], and [`output_route_generation`]. Their callbacks run on CoreAudio's
/// notification thread, never the audio callback, and only bump a counter.
#[cfg(target_os = "macos")]
pub fn watch_input_devices() {
    use std::ffi::c_void;
    use std::sync::Once;

    #[repr(C)]
    struct AudioObjectPropertyAddress {
        m_selector: u32,
        m_scope: u32,
        m_element: u32,
    }

    type PropertyListenerProc =
        unsafe extern "C" fn(u32, u32, *const AudioObjectPropertyAddress, *mut c_void) -> i32;

    unsafe extern "C" {
        fn AudioObjectAddPropertyListener(
            in_object_id: u32,
            in_address: *const AudioObjectPropertyAddress,
            in_listener: PropertyListenerProc,
            in_client_data: *mut c_void,
        ) -> i32;
    }

    unsafe extern "C" fn on_service_restarted(
        _in_object_id: u32,
        _in_number_addresses: u32,
        _in_addresses: *const AudioObjectPropertyAddress,
        _in_client_data: *mut c_void,
    ) -> i32 {
        tracing::warn!("CoreAudio daemon (coreaudiod) restarted; recovering microphone stream");
        notify_audio_service_restarted();
        0
    }

    unsafe extern "C" fn on_inputs_changed(
        _in_object_id: u32,
        _in_number_addresses: u32,
        _in_addresses: *const AudioObjectPropertyAddress,
        _in_client_data: *mut c_void,
    ) -> i32 {
        notify_input_route_changed();
        0
    }

    unsafe extern "C" fn on_outputs_changed(
        _in_object_id: u32,
        _in_number_addresses: u32,
        _in_addresses: *const AudioObjectPropertyAddress,
        _in_client_data: *mut c_void,
    ) -> i32 {
        notify_output_route_changed();
        0
    }

    unsafe extern "C" fn on_devices_changed(
        _in_object_id: u32,
        _in_number_addresses: u32,
        _in_addresses: *const AudioObjectPropertyAddress,
        _in_client_data: *mut c_void,
    ) -> i32 {
        notify_input_route_changed();
        notify_output_route_changed();
        0
    }

    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        const SYSTEM_OBJECT: u32 = 1;
        const SERVICE_RESTARTED_SELECTOR: u32 = u32::from_be_bytes(*b"srst");
        const DEFAULT_INPUT_SELECTOR: u32 = u32::from_be_bytes(*b"dIn ");
        const DEFAULT_OUTPUT_SELECTOR: u32 = u32::from_be_bytes(*b"dOut");
        const DEVICES_SELECTOR: u32 = u32::from_be_bytes(*b"dev#");
        const GLOBAL_SCOPE: u32 = u32::from_be_bytes(*b"glob");
        let listeners: [(u32, PropertyListenerProc); 4] = [
            (SERVICE_RESTARTED_SELECTOR, on_service_restarted),
            (DEFAULT_INPUT_SELECTOR, on_inputs_changed),
            (DEFAULT_OUTPUT_SELECTOR, on_outputs_changed),
            (DEVICES_SELECTOR, on_devices_changed),
        ];
        for (selector, listener) in listeners {
            let addr = AudioObjectPropertyAddress {
                m_selector: selector,
                m_scope: GLOBAL_SCOPE,
                m_element: 0,
            };
            let status = unsafe {
                AudioObjectAddPropertyListener(SYSTEM_OBJECT, &addr, listener, std::ptr::null_mut())
            };
            if status != 0 {
                tracing::warn!(status, "could not watch CoreAudio input devices");
            }
        }
        // Closing the lid switches the built-in microphone off without
        // changing CoreAudio's device list or default input.
        lid::watch(notify_input_route_changed);
    });
}

use color_eyre::eyre::{Result, WrapErr, eyre};
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{Device, SizedSample, Stream, StreamConfig};
use transport::{Transport, transport};

#[cfg(all(test, target_os = "macos"))]
use instant::scale_mach_ticks;
pub use instant::{CaptureInstant, duration_for_frames};

pub struct AudioInput {
    // Tests can deliver PCM through channels without opening a native microphone.
    _stream: Option<input_format::InputStream>,
    chunks: Receiver<(Vec<f32>, CaptureInstant)>,
    pub sample_rate: u32,
    pub device_name: String,
    /// The device's stable OS identifier, when it has one, which is what a
    /// call app's reported inputs are compared by.
    pub uid: Option<String>,
    /// Whether this input stands in for the saved microphone until it can
    /// be opened again.
    pub selection: Selection,
    stream_errors: Receiver<String>,
    watch_service_restarts: bool,
    restart_generation: u64,
    hex_microphone: crate::audio::hex_microphone::CaptureFeedSwitch,
    /// How long the stream may go without delivering audio before it counts
    /// as failed; `None` for test inputs fed by hand.
    stall_after: Option<Duration>,
    /// When the stream opened or last delivered a chunk.
    last_delivery: Mutex<Instant>,
    /// Whether this stream's first chunk has reported how often it delivers
    /// audio.
    reported_period: AtomicBool,
}

/// A running stream delivers audio every few milliseconds, even silence. One
/// that has delivered nothing for this long has stopped without reporting an
/// error (seen after a burst of overruns, #1206) and is reopened.
pub(crate) const STREAM_STALL_TIMEOUT: Duration = Duration::from_secs(4);

/// A stream that has delivered chunks for this long since its first one has
/// proven that it works: its failure reopens it at once. One that fails
/// sooner never proved steady delivery, so its reopen backs off instead of
/// looping every stall timeout (#1681).
const STREAM_PROVEN_UPTIME: Duration = STREAM_STALL_TIMEOUT.saturating_mul(2);

pub enum AudioInputEvent {
    Chunk {
        samples: Vec<f32>,
        captured_through: CaptureInstant,
    },
    Timeout,
    StreamFailed(String),
}

/// Whether an input stands in for the saved microphone, and why.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Selection {
    /// Nothing is saved, or the input is the saved microphone.
    #[default]
    InUse,
    /// The saved microphone is not listed.
    Unlisted,
    /// The saved microphone is listed but would not open.
    Unopenable,
    /// The saved microphone is built in and off while the lid is closed.
    LidClosed,
}

/// What resolving and opening the configured input found.
// One is made per stream open, so the Windows stream's size doesn't matter.
#[allow(clippy::large_enum_variant)]
pub enum Opened {
    Input(Box<AudioInput>),
    /// The input resolved to the device already open, which was kept.
    Unchanged {
        selection: Selection,
    },
}

type InputOpenResult = (u64, Result<Opened, String>);

/// What an open resolves: `--device`, the saved microphone, and the open
/// device's name, which is kept rather than reopened when it resolves again.
struct OpenRequest {
    device_override: Option<String>,
    selected_device: Option<String>,
    fallback: SelectedFallback,
    keep: Option<String>,
    /// Whether System default passes over a Bluetooth default when the
    /// setting asks. Dictation does; a meeting records the input the call
    /// is using, so the headset a call app holds is never swapped out.
    avoids_bluetooth: bool,
    /// The UID of the input a meeting's call app records from, tried first.
    call_input: Option<String>,
    /// Whether the open makes the device renegotiate its sample rate first: a
    /// silence recovery's device can stay on a stale clock (#1575).
    renegotiate_rate: bool,
}

impl OpenRequest {
    /// The route this open resolves against.
    fn route(&self, current: Route) -> Route {
        Route {
            avoid_bluetooth: current.avoid_bluetooth && self.avoids_bluetooth,
            ..current
        }
    }
}

type Opener = Arc<dyn Fn(&OpenRequest) -> Result<Opened> + Send + Sync>;

pub struct RecoveringAudioInput {
    input: Option<AudioInput>,
    /// When the installed stream first delivered a chunk, and whether it has
    /// since delivered for [`STREAM_PROVEN_UPTIME`] without stopping. A stream
    /// that fails before that never proved steady delivery, so its reopen
    /// backs off instead of repeating the stall timeout forever (#1681).
    first_delivery: Option<Instant>,
    proven: bool,
    device_override: Option<String>,
    selected_device: Option<String>,
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
    outage: Option<Option<String>>, // why no input opens; `Some(None)` once taken
    /// The retry error last logged, so a microphone that stays gone (a closed
    /// lid, an unplugged headset) logs once, not on every retry.
    retry_warning: Option<String>,
}

/// Whether a listener's capture is idle, so [`RecoveringAudioInput`] may
/// switch devices. A pending start or a release's tail still needs the
/// open stream: replacing it would discard the capture.
#[cfg(any(target_os = "linux", target_os = "windows", test))]
pub fn capture_idle(recording: bool, start_pending: bool, finishing: bool) -> bool {
    !recording && !start_pending && !finishing
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

/// An on-demand open retries the selected microphone briefly before falling
/// back, because a device that has idled for minutes can fail its first open.
const SELECTED_MICROPHONE_ATTEMPTS: usize = 3;
const SELECTED_MICROPHONE_RETRY_DELAY: Duration = Duration::from_millis(100);

/// How an open treats a saved microphone that is listed but cannot be
/// opened. Either way the system default stands in, and the saved
/// microphone is tried again with backoff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectedFallback {
    /// Startup and meetings fall back at once.
    Immediate,
    /// Other opens retry briefly first.
    AfterRetries,
}

impl AudioInput {
    #[cfg(test)]
    pub fn channel_for_test() -> (Self, Sender<(Vec<f32>, CaptureInstant)>) {
        let (sender, chunks) = mpsc::channel();
        let (_, stream_errors) = mpsc::channel();
        (
            Self {
                _stream: None,
                chunks,
                stream_errors,
                sample_rate: 48_000,
                device_name: "Test microphone".into(),
                uid: None,
                selection: Selection::InUse,
                watch_service_restarts: false,
                restart_generation: audio_service_restart_generation(),
                hex_microphone: crate::audio::hex_microphone::capture_feed(48_000).1,
                stall_after: None,
                last_delivery: Mutex::new(Instant::now()),
                reported_period: AtomicBool::new(false),
            },
            sender,
        )
    }

    #[cfg(test)]
    fn watch_stalls_for_test(&mut self, after: Duration) {
        self.stall_after = Some(after);
        *self
            .last_delivery
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Instant::now();
    }

    #[cfg(test)]
    pub fn watch_service_restarts_for_test(&mut self) {
        self.watch_service_restarts = true;
        self.restart_generation = audio_service_restart_generation();
    }

    /// The OS's default input, or a real one standing in when the default is
    /// Hex Microphone or missing.
    #[cfg(target_os = "windows")]
    pub fn open_system_default() -> Result<Self> {
        match open_configured_input(&OpenRequest {
            device_override: None,
            selected_device: None,
            fallback: SelectedFallback::Immediate,
            keep: None,
            avoids_bluetooth: true,
            call_input: None,
            renegotiate_rate: false,
        })? {
            Opened::Input(input) => Ok(*input),
            Opened::Unchanged { .. } => unreachable!("nothing to keep"),
        }
    }

    #[cfg(target_os = "windows")]
    pub fn open_matching(query: &str) -> Result<Self> {
        let host = cpal::default_host();
        let (devices, names) = input_devices_of(&host)?;
        let query_lower = query.to_lowercase();
        let index = names
            .iter()
            .position(|name| name.to_lowercase().contains(&query_lower))
            .ok_or_else(|| eyre!("microphone is unavailable: {query}"))?;
        Self::open_device(devices[index].clone(), names[index].clone(), false, false)
    }

    /// Opens `device`. `follows_default` marks cpal's default-input handle,
    /// whose Windows stream reports a default change as invalidation while
    /// it keeps running on the old device. `renegotiate_rate` makes the
    /// device change its sample rate before the stream opens (#1575).
    fn open_device(
        device: Device,
        device_name: String,
        follows_default: bool,
        renegotiate_rate: bool,
    ) -> Result<Self> {
        #[cfg(target_os = "macos")]
        watch_input_devices();
        let uid = device.id().ok().map(|id| id.id().to_owned());
        let restart_generation = audio_service_restart_generation();
        // Taken before the stream runs, so CoreAudio's real-time thread reads
        // an initialized clock.
        overloads::start_clock();
        let started = input_format::start_stream_with_fallback(
            &device,
            &device_name,
            follows_default.then(|| device_name.clone()),
            renegotiate_rate,
        )?;

        Ok(Self {
            _stream: Some(started.stream),
            chunks: started.chunks,
            sample_rate: started.sample_rate,
            device_name,
            uid,
            selection: Selection::InUse,
            stream_errors: started.stream_errors,
            watch_service_restarts: true,
            restart_generation,
            hex_microphone: started.hex_microphone,
            stall_after: Some(STREAM_STALL_TIMEOUT),
            last_delivery: Mutex::new(Instant::now()),
            reported_period: AtomicBool::new(false),
        })
    }

    /// Whether this stream's audio feeds Hex Microphone. Only the dictation
    /// listener's current input does; a meeting's second stream on the same
    /// microphone must not.
    pub fn feed_hex_microphone(&self, on: bool) {
        self.hex_microphone.set(on);
    }

    pub fn recv_timeout(&self, timeout: Duration) -> AudioInputEvent {
        // The one place a stream's overloads reach the log, whoever reported
        // them: the error callback only counts them, and this poll writes the
        // count.
        overloads::report();
        if self.watch_service_restarts
            && audio_service_restart_generation() != self.restart_generation
        {
            return AudioInputEvent::StreamFailed("CoreAudio service restarted".into());
        }
        // A stream failure reported before or during the wait outranks any
        // chunk delivered alongside it.
        if let Ok(error) = self.stream_errors.try_recv() {
            return AudioInputEvent::StreamFailed(error);
        }
        let received = self.chunks.recv_timeout(timeout);
        if self.watch_service_restarts
            && audio_service_restart_generation() != self.restart_generation
        {
            return AudioInputEvent::StreamFailed("CoreAudio service restarted".into());
        }
        if let Ok(error) = self.stream_errors.try_recv() {
            return AudioInputEvent::StreamFailed(error);
        }
        match received {
            Ok((samples, captured_through)) => {
                self.report_period(samples.len());
                if self.stall_after.is_some() {
                    *self
                        .last_delivery
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = Instant::now();
                }
                AudioInputEvent::Chunk {
                    samples,
                    captured_through,
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                let stalled = self.stall_after.is_some_and(|after| {
                    self.last_delivery
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .elapsed()
                        >= after
                });
                if stalled {
                    AudioInputEvent::StreamFailed("microphone stopped delivering audio".into())
                } else {
                    AudioInputEvent::Timeout
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                AudioInputEvent::StreamFailed("microphone audio channel disconnected".into())
            }
        }
    }

    /// Reports how much audio the stream's callbacks deliver, once per open.
    /// cpal exposes no readback of the period it picks for `BufferSize::Default`
    /// and overloads arrive sooner on a smaller one, so the first chunk says
    /// what the device actually does. A fixture input's chunks are its WAV
    /// file's, not a device's, so it says nothing.
    fn report_period(&self, frames: usize) {
        let first = !self.reported_period.swap(true, Ordering::Relaxed);
        #[cfg(target_os = "macos")]
        if first && !uses_fixture() {
            tracing::info!(
                device = %self.device_name,
                sample_rate = self.sample_rate,
                frames,
                period_us = duration_for_frames(frames, self.sample_rate).as_micros() as u64,
                "microphone stream opened"
            );
        }
        // Only macOS logs the period; every platform keeps the same open state.
        #[cfg(not(target_os = "macos"))]
        let _ = (frames, first);
    }
}

impl Drop for AudioInput {
    fn drop(&mut self) {
        // Stop the stream before the count is taken, so a closing stream's last
        // burst is written here instead of beside the next open's lines.
        self._stream = None;
        overloads::flush();
    }
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
                tracing::warn!(%error, "could not open dictation microphone at startup; retrying");
                self.outage.get_or_insert(Some(error.to_string()));
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
            first_delivery: None,
            proven: false,
            device_override: device_override.map(str::to_owned),
            selected_device: selected_device.map(str::to_owned),
            active_revision: selection_revision,
            recovery: MicrophoneRecovery::default(),
            replacement: None,
            route_generation: input_route_generation(),
            selection_retry: MICROPHONE_RETRY_INITIAL,
            reports_new_devices: cfg!(target_os = "macos"),
            opener: Arc::new(open_configured_input),
            route_generation_source: input_route_generation,
            outage: None,
            retry_warning: None,
        }
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn take_failure(&mut self) -> Option<String> {
        self.outage.as_mut().and_then(Option::take)
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
        input.opener = Arc::new(opener);
        input
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

    /// A finished dictation was digital silence (#1122): reopen the stream and
    /// make that open renegotiate the device's sample rate first, which clears
    /// a device left on a stale clock (#1575).
    pub fn request_silence_recovery(&mut self) {
        self.recovery.request_silence_recovery(Instant::now());
    }

    pub fn is_recovering(&self) -> bool {
        self.recovery.blocks_audio()
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
        self.retire_input();
        input.feed_hex_microphone(true);
        self.input = Some(input);
        self.outage = None;
        self.retry_warning = None;
    }

    /// Stops the open stream and drops it, so no stream of Hex's is left on
    /// the device.
    fn retire_input(&mut self) {
        self.first_delivery = None;
        self.proven = false;
        if let Some(previous) = self.input.take() {
            previous.feed_hex_microphone(false);
            drop(previous);
        }
    }

    pub fn close(&mut self) {
        if let Some(previous) = self.input.take() {
            previous.feed_hex_microphone(false);
        }
        self.first_delivery = None;
        self.proven = false;
        self.replacement = None;
        self.recovery.recovered();
        self.outage = None;
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
        if self.input.is_none() {
            self.recovery.recovered();
        }
    }

    pub fn request_selection(&mut self, revision: u64, selected_device: Option<&str>) {
        if self.device_override.is_some() || revision == self.active_revision {
            return;
        }
        self.selected_device = selected_device.map(str::to_owned);
        if self.input.is_none() && !self.is_recovering() {
            let was_opening = self.replacement.take().is_some();
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
        if let Some((opened_revision, result)) = self.poll_replacement() {
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
                Ok(Opened::Input(replacement)) if capture_idle || self.input.is_none() => {
                    self.install(*replacement);
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
                        self.outage.get_or_insert(Some(error));
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
        if (capture_idle || self.input.is_none())
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
            AudioInputEvent::StreamFailed(error) => {
                let failed_at = Instant::now();
                tracing::warn!(%error, device = %input.device_name, "microphone stream stopped; reopening it");
                if self.proven {
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
        if self.proven {
            return;
        }
        let first = *self.first_delivery.get_or_insert(now);
        if now.saturating_duration_since(first) >= STREAM_PROVEN_UPTIME {
            self.proven = true;
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
        // A failed stream is replaced even by the same device; a route or
        // selection change keeps the open device when it resolves again.
        let keep = self
            .input
            .as_ref()
            .filter(|_| !self.recovery.blocks_audio())
            .map(|input| input.device_name.clone());
        // A failed or silent stream lets go of the device before its
        // replacement opens, so the replacement starts cold, as at launch. A
        // live stream stays until its replacement is ready to install.
        if self.recovery.blocks_audio() {
            self.retire_input();
        }
        let request = OpenRequest {
            device_override: self.device_override.clone(),
            selected_device: self.selected_device.clone(),
            fallback: SelectedFallback::AfterRetries,
            keep,
            avoids_bluetooth: true,
            call_input: None,
            renegotiate_rate: self.recovery.take_renegotiation(),
        };
        self.route_generation = (self.route_generation_source)();
        let opener = self.opener.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = opener(&request).map_err(|error| error.to_string());
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

    fn poll_replacement(&mut self) -> Option<InputOpenResult> {
        let result = match self.replacement.as_ref()?.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => (
                self.active_revision,
                Err("microphone replacement worker stopped".into()),
            ),
        };
        self.replacement = None;
        Some(result)
    }
}

/// Which input an open resolves to, before anything opens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InputTarget {
    /// `--device` or the saved microphone, by its index in the list.
    Named(usize),
    /// The OS's default input.
    SystemDefault,
    /// A real input standing in for a default that is Hex Microphone, the
    /// built-in microphone with the lid closed, a passed-over Bluetooth
    /// headset, or missing: the built-in microphone while the lid is open,
    /// else the first usable one listed, never a virtual input or an
    /// iPhone's Continuity microphone.
    StandIn(usize),
}

/// The state of the machine an input resolves against.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Route {
    /// A MacBook's lid is closed, so its built-in microphone is off.
    lid_closed: bool,
    /// System default passes over a Bluetooth default while another real
    /// input is listed.
    avoid_bluetooth: bool,
}

pub(crate) const LID_CLOSED_ERROR: &str = "the built-in microphone is off while the lid is closed";
pub(crate) const NO_MICROPHONE_ERROR: &str = "no microphone is available";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InputPlan {
    target: InputTarget,
    selection: Selection,
}

/// Resolves the input Hex captures from: `--device` exactly as named, else
/// the saved microphone while it is listed, else the system default. Hex
/// never captures from its own virtual microphone, so a default that is Hex
/// Microphone (or none) falls back to a real input. With the lid closed, the
/// built-in microphone is switched off in hardware though still listed, so
/// it is passed over the same way. With `avoid_bluetooth`, a Bluetooth
/// default is passed over for the built-in microphone, else the first other
/// real input, and kept only when nothing else is usable. Saved microphones
/// and `--device` are never passed over for being Bluetooth. A virtual or
/// aggregate input, or an iPhone's Continuity microphone, never stands in,
/// though such a default, saved microphone, or `--device` is used as named.
fn resolve_input(
    device_override: Option<&str>,
    selected: Option<&str>,
    names: &[String],
    default: Option<&str>,
    transport: impl Fn(usize) -> Transport,
    route: Route,
) -> std::result::Result<InputPlan, String> {
    let is_hex = crate::audio::hex_microphone::is_hex_virtual_device_name;
    let position = |name: &str| names.iter().position(|listed| listed == name);
    let is_built_in = |index: usize| transport(index) == Transport::BuiltIn;
    let avoided = |index: usize| route.avoid_bluetooth && transport(index) == Transport::Bluetooth;
    let off = |index: usize| route.lid_closed && is_built_in(index);
    if let Some(name) = device_override {
        if is_hex(name) {
            return Err(format!(
                "Hex cannot capture from its own virtual microphone: {name}"
            ));
        }
        return position(name)
            .map(|index| InputPlan {
                target: InputTarget::Named(index),
                selection: Selection::InUse,
            })
            .ok_or_else(|| format!("microphone is unavailable: {name}"));
    }
    let selection = match selected.filter(|name| !is_hex(name)) {
        Some(name) => match position(name) {
            Some(index) if off(index) => Selection::LidClosed,
            Some(index) => {
                return Ok(InputPlan {
                    target: InputTarget::Named(index),
                    selection: Selection::InUse,
                });
            }
            None => Selection::Unlisted,
        },
        None => Selection::InUse,
    };
    // ALSA names a default even with no devices listed.
    let default_usable = !names.is_empty()
        && default.is_some_and(|name| !is_hex(name) && !position(name).is_some_and(off));
    let default_avoided = default.and_then(position).is_some_and(avoided);
    if default_usable && !default_avoided {
        return Ok(InputPlan {
            target: InputTarget::SystemDefault,
            selection,
        });
    }
    // A virtual device or an iPhone's Continuity microphone is used only as
    // the default or by name, never as a stand-in: a virtual one can record
    // silence while its app is idle, and the iPhone prompts and can fail.
    let usable = || {
        (0..names.len()).filter(|&index| {
            !is_hex(&names[index])
                && !off(index)
                && !matches!(transport(index), Transport::Virtual | Transport::Continuity)
        })
    };
    usable()
        .find(|&index| is_built_in(index))
        .or_else(|| usable().find(|&index| !avoided(index)))
        .map(InputTarget::StandIn)
        // A Bluetooth default beats recording nothing.
        .or_else(|| default_usable.then_some(InputTarget::SystemDefault))
        .or_else(|| usable().next().map(InputTarget::StandIn))
        .map(|target| InputPlan { target, selection })
        .ok_or_else(|| {
            if (0..names.len()).any(off) {
                LID_CLOSED_ERROR.into()
            } else {
                NO_MICROPHONE_ERROR.into()
            }
        })
}

fn current_route() -> Route {
    Route {
        lid_closed: lid_closed(),
        avoid_bluetooth: avoid_bluetooth_microphone(),
    }
}

/// Whether a MacBook's lid is closed, which switches its built-in microphone
/// off in hardware while CoreAudio still lists it, often as the default
/// input. A microphone in the headphone jack stays on. Read from
/// `AppleClamshellState` on IOKit's power root domain; Macs without a lid,
/// and other OSes, read open.
#[cfg(not(target_os = "macos"))]
fn lid_closed() -> bool {
    false
}

#[cfg(target_os = "macos")]
use lid::closed as lid_closed;

/// The input devices whose names can be read, with those names. A device
/// without a readable name, such as CoreAudio's placeholder when no input
/// exists, cannot be selected or opened, so it is left out.
fn input_devices_of(host: &cpal::Host) -> Result<(Vec<Device>, Vec<String>)> {
    Ok(host
        .input_devices()
        .wrap_err("could not enumerate input devices")?
        .filter_map(|device| device_name(&device).map(|name| (device, name)))
        .unzip())
}

/// The system default input with its name, or `None` when there is none or
/// its name cannot be read.
fn default_input_of(host: &cpal::Host) -> Option<(Device, String)> {
    let device = host.default_input_device()?;
    let name = device_name(&device)?;
    Some((device, name))
}

/// The device's name. cpal's `Display` for `Device` returns `fmt::Error`
/// when the name cannot be read, which makes `to_string` panic, so read the
/// description directly.
fn device_name(device: &Device) -> Option<String> {
    description_name(device.description())
}

fn description_name<E: std::fmt::Display>(
    description: std::result::Result<cpal::DeviceDescription, E>,
) -> Option<String> {
    match description {
        Ok(description) => Some(description.name().to_owned()),
        Err(error) => {
            tracing::debug!(%error, "skipping an input device whose name cannot be read");
            None
        }
    }
}

/// Resolves and opens the input, or keeps the open one when it resolves to
/// the same device. A listed saved microphone that will not open falls back
/// to the system default.
fn open_configured_input(request: &OpenRequest) -> Result<Opened> {
    if let Some(path) = fixture::path() {
        if request.keep.as_deref() == Some(fixture::NAME) {
            return Ok(Opened::Unchanged {
                selection: Selection::InUse,
            });
        }
        return Ok(Opened::Input(Box::new(fixture::open(path)?)));
    }
    let host = cpal::default_host();
    let (devices, names) = input_devices_of(&host)?;
    let (default, default_name) = default_input_of(&host).unzip();
    let transports = devices.iter().map(transport).collect::<Vec<_>>();
    let route = request.route(current_route());
    let resolve = |selected: Option<&str>| {
        resolve_input(
            request.device_override.as_deref(),
            selected,
            &names,
            default_name.as_deref(),
            |index| transports[index],
            route,
        )
        .map_err(|error| eyre!(error))
    };
    // Keeping the open device returns before logging, so a periodic look
    // for a missing microphone logs only when the input changes.
    let open = |plan: InputPlan| -> Result<Opened> {
        let (device, name, follows_default) = match plan.target {
            InputTarget::Named(index) | InputTarget::StandIn(index) => {
                (devices[index].clone(), names[index].clone(), false)
            }
            InputTarget::SystemDefault => (
                default.clone().expect("resolved default"),
                default_name.clone().expect("resolved default"),
                true,
            ),
        };
        if request.keep.as_deref() == Some(name.as_str()) {
            return Ok(Opened::Unchanged {
                selection: plan.selection,
            });
        }
        if let InputTarget::StandIn(_) = plan.target {
            tracing::warn!(
                default = default_name.as_deref().unwrap_or("none"),
                using = %name,
                lid_closed = route.lid_closed,
                avoid_bluetooth = route.avoid_bluetooth,
                "the system default input is Hex Microphone, off with the lid closed, Bluetooth, or missing; using another microphone"
            );
        }
        match plan.selection {
            Selection::InUse => {}
            Selection::Unlisted => tracing::info!(
                selected = request.selected_device.as_deref(),
                "the saved microphone is not connected; using the system default"
            ),
            Selection::Unopenable => tracing::warn!(
                selected = request.selected_device.as_deref(),
                "the saved microphone would not open; using the system default"
            ),
            Selection::LidClosed => tracing::info!(
                selected = request.selected_device.as_deref(),
                "the saved microphone is off while the lid is closed; using the system default"
            ),
        }
        let mut input =
            AudioInput::open_device(device, name, follows_default, request.renegotiate_rate)?;
        input.selection = plan.selection;
        // The ALSA default cannot name another PipeWire source, so Linux
        // moves the opened stream off a Bluetooth default instead.
        #[cfg(target_os = "linux")]
        if follows_default && route.avoid_bluetooth {
            crate::linux::input_route::move_streams_off_bluetooth();
        }
        Ok(Opened::Input(Box::new(input)))
    };
    if let Some(index) =
        (request.call_input.as_deref()).and_then(|uid| call_input::listed_at(uid, &devices, &names))
    {
        let target = InputTarget::Named(index);
        match open(InputPlan {
            target,
            selection: Selection::InUse,
        }) {
            Ok(opened) => return Ok(opened),
            Err(error) => tracing::warn!(%error, "the input the call uses would not open"),
        }
    }
    let plan = resolve(request.selected_device.as_deref())?;
    let (Some(selected), InputTarget::Named(_), None) = (
        request.selected_device.as_deref(),
        plan.target,
        request.device_override.as_deref(),
    ) else {
        return open(plan);
    };
    open_selected(
        selected,
        request.fallback,
        || open(plan),
        || {
            let mut fallback = resolve(None)?;
            fallback.selection = Selection::Unopenable;
            open(fallback)
        },
        std::thread::sleep,
    )
}

/// Opens the microphone a meeting records: the input its call app records
/// from (`call_input`, a device UID) when that opens, else one resolved like
/// dictation's but keeping a Bluetooth default a call app on a headset uses;
/// or keeps the open one named `keep` when it resolves to that again.
/// `renegotiate_rate` marks a silence recovery's open, like dictation's.
pub fn open_meeting_input(
    selected_device: Option<&str>,
    call_input: Option<&str>,
    keep: Option<&str>,
    renegotiate_rate: bool,
) -> Result<Opened> {
    open_configured_input(&OpenRequest {
        device_override: None,
        selected_device: selected_device.map(str::to_owned),
        fallback: SelectedFallback::Immediate,
        keep: keep.map(str::to_owned),
        avoids_bluetooth: false,
        call_input: call_input.map(str::to_owned),
        renegotiate_rate,
    })
}

fn open_selected<T>(
    device: &str,
    fallback: SelectedFallback,
    mut open_selected: impl FnMut() -> Result<T>,
    open_default: impl FnOnce() -> Result<T>,
    mut sleep: impl FnMut(Duration),
) -> Result<T> {
    let attempts = if fallback == SelectedFallback::AfterRetries {
        SELECTED_MICROPHONE_ATTEMPTS
    } else {
        1
    };
    let mut attempt = 1;
    let error = loop {
        match open_selected() {
            Ok(input) => return Ok(input),
            Err(error) if attempt >= attempts => break error,
            Err(error) => {
                tracing::debug!(%error, device, attempt, "could not open the selected microphone; retrying");
                attempt += 1;
                sleep(SELECTED_MICROPHONE_RETRY_DELAY);
            }
        }
    };
    tracing::debug!(%error, device, "the saved microphone would not open");
    open_default()
}

/// The inputs Settings offers, without Hex Microphone, and what System
/// default currently resolves to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputDevices {
    pub names: Vec<String>,
    /// The input System default captures from. Linux's ALSA default has no
    /// useful name, so it is `None` there.
    pub system_default: Option<String>,
    /// The OS default is Hex Microphone, so `system_default` stands in.
    pub default_is_hex_microphone: bool,
    /// The internal microphone, listed but off because the lid is closed. A
    /// headphone-jack microphone stays usable and is never named here.
    pub off_with_lid_closed: Option<String>,
    /// The OS default is `off_with_lid_closed`, so `system_default` stands in.
    pub default_is_off_with_lid_closed: bool,
    /// The OS default, a Bluetooth headset that `system_default` stands in
    /// for so the headset keeps its audio quality.
    pub bluetooth_default_passed_over: Option<String>,
}

pub fn input_devices() -> Result<InputDevices> {
    #[cfg(target_os = "macos")]
    watch_input_devices();
    let host = cpal::default_host();
    let (devices, listed) = input_devices_of(&host)?;
    let default = default_input_of(&host).map(|(_, name)| name);
    let default_is_hex_microphone = default
        .as_deref()
        .is_some_and(crate::audio::hex_microphone::is_hex_virtual_device_name);
    let transports = devices.iter().map(transport).collect::<Vec<_>>();
    let route = current_route();
    let off_with_lid_closed = (0..devices.len())
        .find(|&index| route.lid_closed && transports[index] == Transport::BuiltIn)
        .map(|index| listed[index].clone());
    let default_is_off_with_lid_closed =
        off_with_lid_closed.is_some() && default == off_with_lid_closed;
    let plan = resolve_input(
        None,
        None,
        &listed,
        default.as_deref(),
        |index| transports[index],
        route,
    )
    .ok();
    let bluetooth_default_passed_over = default
        .clone()
        .filter(|name| {
            listed
                .iter()
                .position(|listed| listed == name)
                .is_some_and(|index| transports[index] == Transport::Bluetooth)
        })
        .filter(|_| matches!(plan.map(|plan| plan.target), Some(InputTarget::StandIn(_))));
    let system_default = plan
        .and_then(|plan| match plan.target {
            InputTarget::SystemDefault => default.clone(),
            InputTarget::Named(index) | InputTarget::StandIn(index) => Some(listed[index].clone()),
        })
        .filter(|_| cfg!(not(target_os = "linux")));
    // Linux names the sound server's sources instead, and only when it
    // passes over a Bluetooth default.
    #[cfg(target_os = "linux")]
    let (system_default, bluetooth_default_passed_over) = match route
        .avoid_bluetooth
        .then(crate::linux::input_route::bluetooth_stand_in)
        .flatten()
    {
        Some(stand_in) => (Some(stand_in.input), Some(stand_in.bluetooth)),
        None => (system_default, bluetooth_default_passed_over),
    };
    let mut names = listed
        .into_iter()
        .filter(|name| !crate::audio::hex_microphone::is_hex_virtual_device_name(name))
        .collect::<Vec<_>>();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    Ok(InputDevices {
        names,
        system_default,
        default_is_hex_microphone,
        off_with_lid_closed,
        default_is_off_with_lid_closed,
        bluetooth_default_passed_over,
    })
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub fn input_device_names() -> Result<Vec<String>> {
    input_devices().map(|devices| devices.names)
}

fn mono<T>(samples: &[T], channels: usize, convert: impl Fn(&T) -> f32) -> Vec<f32> {
    samples
        .chunks(channels)
        .map(|frame| frame.iter().map(&convert).sum::<f32>() / channels as f32)
        .collect()
}

/// Where a stream reports errors to the audio owner.
#[derive(Clone)]
struct StreamErrorReporter {
    sender: SyncSender<String>,
    /// The device's name when the stream was opened on cpal's default-input
    /// handle.
    default_name: Option<String>,
}

impl StreamErrorReporter {
    fn report(&self, error: cpal::Error) {
        // WASAPI reports a default-input change as invalidation, yet the
        // stream keeps capturing the old device. Hex switches once capture is
        // idle rather than failing a recording.
        #[cfg(target_os = "windows")]
        if error.kind() == cpal::ErrorKind::StreamInvalidated
            && let Some(name) = &self.default_name
            && cpal::default_host()
                .default_input_device()
                .and_then(|device| device_name(&device))
                .is_some_and(|current| current != *name)
        {
            tracing::info!(from = %name, "the default microphone changed");
            notify_input_route_changed();
            return;
        }
        #[cfg(not(target_os = "windows"))]
        let _ = &self.default_name;
        report_stream_error(&self.sender, error);
    }
}

fn report_stream_error(sender: &SyncSender<String>, error: cpal::Error) {
    match error.kind() {
        // An overload or a refused real-time promotion is a glitch on a stream
        // that keeps running, common while a cold device starts. Some hosts
        // report it from the thread that runs the stream, where a log line or a
        // lock blocks an already-late cycle, so only the counters move here and
        // the audio owner writes the burst. Real sample gaps still surface as
        // capture discontinuities.
        cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied => overloads::record(),
        _ => {
            tracing::error!(%error, "microphone stream failed");
            let _ = sender.try_send(error.to_string());
        }
    }
}

fn build_stream<T: SizedSample>(
    device: &Device,
    config: &StreamConfig,
    channels: usize,
    sender: Sender<(Vec<f32>, CaptureInstant)>,
    reporter: StreamErrorReporter,
    mut feed: crate::audio::hex_microphone::CaptureFeed,
    convert: impl Fn(T) -> f32 + Send + 'static,
) -> Result<Stream> {
    let sample_rate = config.sample_rate;
    device
        .build_input_stream(
            *config,
            move |data: &[T], info| {
                let captured_through = captured_through(info, data.len() / channels, sample_rate);
                let mono_samples = mono(data, channels, |sample| convert(*sample));
                feed.push(&mono_samples);
                let _ = sender.send((mono_samples, captured_through));
            },
            move |error| reporter.report(error),
            None,
        )
        .wrap_err_with(|| format!("could not open {} microphone stream", T::FORMAT))
}

fn captured_through(
    info: &cpal::InputCallbackInfo,
    frames: usize,
    sample_rate: u32,
) -> CaptureInstant {
    let captured_at = CaptureInstant::from_stream(info.timestamp().capture);
    let duration = duration_for_frames(frames, sample_rate);
    captured_at.checked_add(duration).unwrap_or(captured_at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    const OPEN: Route = Route {
        lid_closed: false,
        avoid_bluetooth: false,
    };
    const CLOSED: Route = Route {
        lid_closed: true,
        avoid_bluetooth: false,
    };

    fn other(_: usize) -> Transport {
        Transport::Other
    }

    fn built_in_at(at: usize) -> impl Fn(usize) -> Transport {
        move |index| {
            if index == at {
                Transport::BuiltIn
            } else {
                Transport::Other
            }
        }
    }

    #[test]
    fn a_headphone_jack_microphone_is_used_with_the_lid_closed() {
        use InputTarget::{Named, StandIn, SystemDefault};
        let names = listed(&["MacBook Pro Microphone", "External Microphone"]);
        // The jack input reads as Other, so only the internal one is off.
        let transport = built_in_at(0);
        let found = |target, selection| Ok(InputPlan { target, selection });
        assert_eq!(
            resolve_input(
                None,
                None,
                &names,
                Some("External Microphone"),
                &transport,
                CLOSED
            ),
            found(SystemDefault, Selection::InUse)
        );
        assert_eq!(
            resolve_input(
                None,
                Some("External Microphone"),
                &names,
                Some("MacBook Pro Microphone"),
                &transport,
                CLOSED
            ),
            found(Named(1), Selection::InUse)
        );
        assert_eq!(
            resolve_input(
                None,
                None,
                &names,
                Some("MacBook Pro Microphone"),
                &transport,
                CLOSED
            ),
            found(StandIn(1), Selection::InUse)
        );
    }

    #[test]
    fn system_default_passes_over_a_bluetooth_default() {
        use InputTarget::{Named, StandIn, SystemDefault};
        use Transport::{Bluetooth, BuiltIn, Other};
        let names = listed(&[
            "Hex Microphone",
            "AirPods Pro",
            "MacBook Pro Microphone",
            "USB Mic",
        ]);
        let transports = [Other, Bluetooth, BuiltIn, Other];
        let transport = |index: usize| transports[index];
        let avoid = Route {
            lid_closed: false,
            avoid_bluetooth: true,
        };
        let found = |target, selection| Ok(InputPlan { target, selection });
        let plan = |selected, default, route| {
            resolve_input(None, selected, &names, default, transport, route)
        };

        // A Bluetooth default stands in with the built-in microphone.
        assert_eq!(
            plan(None, Some("AirPods Pro"), avoid),
            found(StandIn(2), Selection::InUse)
        );
        // A missing saved microphone falls back the same way.
        assert_eq!(
            plan(Some("Gone"), Some("AirPods Pro"), avoid),
            found(StandIn(2), Selection::Unlisted)
        );
        // A saved Bluetooth microphone is honoured, and so is `--device`.
        assert_eq!(
            plan(Some("AirPods Pro"), Some("USB Mic"), avoid),
            found(Named(1), Selection::InUse)
        );
        assert_eq!(
            resolve_input(
                Some("AirPods Pro"),
                None,
                &names,
                Some("AirPods Pro"),
                transport,
                avoid
            ),
            found(Named(1), Selection::InUse)
        );
        // Other defaults are followed.
        assert_eq!(
            plan(None, Some("USB Mic"), avoid),
            found(SystemDefault, Selection::InUse)
        );
        // With the setting off, System default follows the Bluetooth default.
        assert_eq!(
            plan(None, Some("AirPods Pro"), OPEN),
            found(SystemDefault, Selection::InUse)
        );
        // With the lid closed, the next real input stands in.
        let closed = Route {
            lid_closed: true,
            ..avoid
        };
        assert_eq!(
            plan(None, Some("AirPods Pro"), closed),
            found(StandIn(3), Selection::InUse)
        );
        // A Hex Microphone default prefers a wired input to Bluetooth too.
        let no_built_in = listed(&["Hex Microphone", "AirPods Pro", "USB Mic"]);
        let no_built_in_transports = [Other, Bluetooth, Other];
        assert_eq!(
            resolve_input(
                None,
                None,
                &no_built_in,
                Some("Hex Microphone"),
                |index| no_built_in_transports[index],
                avoid
            ),
            found(StandIn(2), Selection::InUse)
        );
    }

    #[test]
    fn a_meeting_keeps_the_bluetooth_default_a_call_uses() {
        use Transport::{Bluetooth, BuiltIn};
        let names = listed(&["AirPods Pro", "MacBook Pro Microphone"]);
        let transport = |index: usize| [Bluetooth, BuiltIn][index];
        let current = Route {
            lid_closed: false,
            avoid_bluetooth: true,
        };
        let request = |avoids_bluetooth| OpenRequest {
            device_override: None,
            selected_device: None,
            fallback: SelectedFallback::Immediate,
            keep: None,
            avoids_bluetooth,
            call_input: None,
            renegotiate_rate: false,
        };
        let target = |request: OpenRequest| {
            resolve_input(
                None,
                None,
                &names,
                Some("AirPods Pro"),
                transport,
                request.route(current),
            )
            .map(|plan| plan.target)
        };
        // Dictation passes over the headset; a meeting records it.
        assert_eq!(target(request(true)), Ok(InputTarget::StandIn(1)));
        assert_eq!(target(request(false)), Ok(InputTarget::SystemDefault));
    }

    #[test]
    fn a_bluetooth_default_is_kept_when_nothing_else_is_usable() {
        use InputTarget::SystemDefault;
        use Transport::{Bluetooth, BuiltIn, Other};
        let avoid = Route {
            lid_closed: false,
            avoid_bluetooth: true,
        };
        let found = |target| {
            Ok(InputPlan {
                target,
                selection: Selection::InUse,
            })
        };

        let only_bluetooth = listed(&["Hex Microphone", "AirPods Pro", "Beats"]);
        let bluetooth = |index: usize| [Other, Bluetooth, Bluetooth][index];
        assert_eq!(
            resolve_input(
                None,
                None,
                &only_bluetooth,
                Some("AirPods Pro"),
                bluetooth,
                avoid
            ),
            found(SystemDefault)
        );
        // With the lid closed the built-in microphone is no alternative.
        let lid = listed(&["AirPods Pro", "MacBook Pro Microphone"]);
        let lid_transports = |index: usize| [Bluetooth, BuiltIn][index];
        assert_eq!(
            resolve_input(
                None,
                None,
                &lid,
                Some("AirPods Pro"),
                lid_transports,
                Route {
                    lid_closed: true,
                    avoid_bluetooth: true,
                }
            ),
            found(SystemDefault)
        );
    }

    #[test]
    fn a_virtual_input_never_stands_in() {
        use InputTarget::{Named, StandIn, SystemDefault};
        use Transport::{Bluetooth, BuiltIn, Other, Virtual};
        let found = |target, selection| Ok(InputPlan { target, selection });
        let avoid = Route {
            lid_closed: false,
            avoid_bluetooth: true,
        };

        // Lid closed, only the built-in microphone and Teams: say why.
        let lid = listed(&["MacBook Pro Microphone", "Microsoft Teams Audio"]);
        let lid_transports = |index: usize| [BuiltIn, Virtual][index];
        assert_eq!(
            resolve_input(
                None,
                None,
                &lid,
                Some("MacBook Pro Microphone"),
                lid_transports,
                CLOSED
            ),
            Err(LID_CLOSED_ERROR.into())
        );
        // A real input still stands in ahead of it.
        let with_usb = listed(&["MacBook Pro Microphone", "Microsoft Teams Audio", "USB Mic"]);
        let usb_transports = |index: usize| [BuiltIn, Virtual, Other][index];
        assert_eq!(
            resolve_input(
                None,
                None,
                &with_usb,
                Some("MacBook Pro Microphone"),
                usb_transports,
                CLOSED
            ),
            found(StandIn(2), Selection::InUse)
        );
        // A Bluetooth default is kept rather than passed over for Teams.
        let headset = listed(&["Microsoft Teams Audio", "AirPods Pro"]);
        let headset_transports = |index: usize| [Virtual, Bluetooth][index];
        assert_eq!(
            resolve_input(
                None,
                None,
                &headset,
                Some("AirPods Pro"),
                headset_transports,
                avoid
            ),
            found(SystemDefault, Selection::InUse)
        );
        // Only Hex Microphone and a virtual input: nothing to record from.
        let virtual_only = listed(&["Hex Microphone", "Microsoft Teams Audio"]);
        let virtual_transports = |index: usize| [Virtual, Virtual][index];
        assert_eq!(
            resolve_input(
                None,
                None,
                &virtual_only,
                Some("Hex Microphone"),
                virtual_transports,
                OPEN
            ),
            Err(NO_MICROPHONE_ERROR.into())
        );
        // A virtual default, saved microphone, or `--device` is used as named.
        assert_eq!(
            resolve_input(
                None,
                None,
                &headset,
                Some("Microsoft Teams Audio"),
                headset_transports,
                OPEN
            ),
            found(SystemDefault, Selection::InUse)
        );
        assert_eq!(
            resolve_input(
                None,
                Some("Microsoft Teams Audio"),
                &headset,
                Some("AirPods Pro"),
                headset_transports,
                OPEN
            ),
            found(Named(0), Selection::InUse)
        );
        assert_eq!(
            resolve_input(
                Some("Microsoft Teams Audio"),
                None,
                &headset,
                Some("AirPods Pro"),
                headset_transports,
                OPEN
            ),
            found(Named(0), Selection::InUse)
        );
    }

    #[test]
    fn an_iphone_microphone_never_stands_in() {
        use InputTarget::{Named, SystemDefault};
        use Transport::{Bluetooth, BuiltIn, Continuity};
        let found = |target, selection| Ok(InputPlan { target, selection });

        // Lid closed, only the built-in microphone and the iPhone: say why.
        let lid = listed(&["MacBook Pro Microphone", "iPhone Microphone"]);
        let lid_transports = |index: usize| [BuiltIn, Continuity][index];
        assert_eq!(
            resolve_input(
                None,
                None,
                &lid,
                Some("MacBook Pro Microphone"),
                lid_transports,
                CLOSED
            ),
            Err(LID_CLOSED_ERROR.into())
        );
        // A Bluetooth default is kept rather than passed over for the iPhone.
        let headset = listed(&["iPhone Microphone", "AirPods Pro"]);
        let headset_transports = |index: usize| [Continuity, Bluetooth][index];
        let avoid = Route {
            lid_closed: false,
            avoid_bluetooth: true,
        };
        assert_eq!(
            resolve_input(
                None,
                None,
                &headset,
                Some("AirPods Pro"),
                headset_transports,
                avoid
            ),
            found(SystemDefault, Selection::InUse)
        );
        // An iPhone default, saved microphone, or `--device` is used as named.
        assert_eq!(
            resolve_input(
                None,
                None,
                &headset,
                Some("iPhone Microphone"),
                headset_transports,
                OPEN
            ),
            found(SystemDefault, Selection::InUse)
        );
        assert_eq!(
            resolve_input(
                None,
                Some("iPhone Microphone"),
                &headset,
                Some("AirPods Pro"),
                headset_transports,
                OPEN
            ),
            found(Named(0), Selection::InUse)
        );
        assert_eq!(
            resolve_input(
                Some("iPhone Microphone"),
                None,
                &headset,
                Some("AirPods Pro"),
                headset_transports,
                OPEN
            ),
            found(Named(0), Selection::InUse)
        );
    }

    #[test]
    fn a_microphone_that_cannot_open_is_reported_once_per_outage() {
        let mut input = RecoveringAudioInput::failed_for_test("Gone microphone");
        let failure = input
            .take_failure()
            .expect("the startup failure is reported");
        assert!(failure.contains("Gone microphone"), "{failure}");
        assert_eq!(input.take_failure(), None);
        // Retries within the same outage stay quiet.
        input.outage.get_or_insert(Some("still gone".into()));
        assert_eq!(input.take_failure(), None);
        // Closing ends the outage; the next one is reported again.
        input.close();
        input.outage.get_or_insert(Some("gone again".into()));
        assert_eq!(input.take_failure().as_deref(), Some("gone again"));
    }

    #[test]
    fn an_unreadable_device_name_is_skipped_instead_of_panicking() {
        let unreadable: std::result::Result<cpal::DeviceDescription, cpal::Error> =
            Err(cpal::ErrorKind::DeviceNotAvailable.into());
        assert_eq!(description_name(unreadable), None);
        let readable: std::result::Result<_, cpal::Error> = Ok(
            cpal::DeviceDescriptionBuilder::new("MacBook Pro Microphone")
                .manufacturer("Apple Inc.")
                .build(),
        );
        assert_eq!(
            description_name(readable).as_deref(),
            Some("MacBook Pro Microphone")
        );
    }

    #[test]
    fn inputs_resolve_to_the_selection_then_the_system_default() {
        use InputTarget::{Named, StandIn, SystemDefault};
        let names = listed(&[
            "Hex Microphone",
            "ALEX PODS",
            "MacBook Pro Microphone",
            "USB Mic",
        ]);
        let built_in = built_in_at(2);
        let plan =
            |selected, default| resolve_input(None, selected, &names, default, &built_in, OPEN);
        let found = |target, selection| Ok(InputPlan { target, selection });

        // A listed saved microphone wins over the default.
        assert_eq!(
            plan(Some("USB Mic"), Some("ALEX PODS")),
            found(Named(3), Selection::InUse)
        );
        // System default follows the OS default, even a Bluetooth headset.
        assert_eq!(
            plan(None, Some("ALEX PODS")),
            found(SystemDefault, Selection::InUse)
        );
        // A missing saved microphone stands in with the default.
        assert_eq!(
            plan(Some("Studio Display Microphone"), Some("ALEX PODS")),
            found(SystemDefault, Selection::Unlisted)
        );
        // A Hex Microphone default never loops back: the built-in mic stands in.
        assert_eq!(
            plan(None, Some("Hex Microphone")),
            found(StandIn(2), Selection::InUse)
        );
        assert_eq!(
            plan(Some("Gone"), Some("Hex Microphone")),
            found(StandIn(2), Selection::Unlisted)
        );
        // Without a built-in mic or any default, the first real input stands in.
        let no_built_in = resolve_input(None, None, &names, Some("Hex Microphone"), other, OPEN);
        assert_eq!(no_built_in, found(StandIn(1), Selection::InUse));
        assert_eq!(plan(None, None), found(StandIn(2), Selection::InUse));
        // A saved Hex Microphone is never captured from.
        assert_eq!(
            plan(Some("Hex Microphone"), Some("ALEX PODS")),
            found(SystemDefault, Selection::InUse)
        );
        // No real inputs at all.
        assert_eq!(
            resolve_input(
                None,
                None,
                &listed(&["Hex Microphone"]),
                Some("Hex Microphone"),
                &built_in,
                OPEN
            ),
            Err(NO_MICROPHONE_ERROR.into())
        );
        assert_eq!(
            resolve_input(None, Some("USB Mic"), &[], None, &built_in, OPEN),
            Err(NO_MICROPHONE_ERROR.into())
        );
        assert_eq!(
            resolve_input(
                None,
                None,
                &[],
                Some("Default Audio Device"),
                &built_in,
                OPEN
            ),
            Err(NO_MICROPHONE_ERROR.into())
        );
    }

    #[test]
    fn a_closed_lid_passes_over_the_built_in_microphone() {
        use InputTarget::{Named, StandIn, SystemDefault};
        let names = listed(&[
            "Hex Microphone",
            "MacBook Pro Microphone",
            "Studio Display Microphone",
            "USB Mic",
        ]);
        let built_in = built_in_at(1);
        let closed =
            |selected, default| resolve_input(None, selected, &names, default, &built_in, CLOSED);
        let found = |target, selection| Ok(InputPlan { target, selection });

        // A built-in default stands in with the first other real input.
        assert_eq!(
            closed(None, Some("MacBook Pro Microphone")),
            found(StandIn(2), Selection::InUse)
        );
        // So does a Hex Microphone or missing default: never the built-in.
        assert_eq!(
            closed(None, Some("Hex Microphone")),
            found(StandIn(2), Selection::InUse)
        );
        assert_eq!(closed(None, None), found(StandIn(2), Selection::InUse));
        // Another default is still followed.
        assert_eq!(
            closed(None, Some("USB Mic")),
            found(SystemDefault, Selection::InUse)
        );
        // A saved built-in microphone stands aside for the default.
        assert_eq!(
            closed(Some("MacBook Pro Microphone"), Some("USB Mic")),
            found(SystemDefault, Selection::LidClosed)
        );
        assert_eq!(
            closed(
                Some("MacBook Pro Microphone"),
                Some("MacBook Pro Microphone")
            ),
            found(StandIn(2), Selection::LidClosed)
        );
        // Other saved microphones are unaffected.
        assert_eq!(
            closed(Some("USB Mic"), Some("MacBook Pro Microphone")),
            found(Named(3), Selection::InUse)
        );
        // With the lid open the built-in microphone is used as before.
        assert_eq!(
            resolve_input(
                None,
                None,
                &names,
                Some("MacBook Pro Microphone"),
                &built_in,
                OPEN
            ),
            found(SystemDefault, Selection::InUse)
        );
        // With nothing else to use, say why rather than record silence.
        let only_built_in = listed(&["Hex Microphone", "MacBook Pro Microphone"]);
        for default in [Some("MacBook Pro Microphone"), Some("Hex Microphone"), None] {
            assert_eq!(
                resolve_input(None, None, &only_built_in, default, &built_in, CLOSED),
                Err(LID_CLOSED_ERROR.into())
            );
        }
        assert_eq!(
            resolve_input(
                None,
                Some("MacBook Pro Microphone"),
                &only_built_in,
                Some("MacBook Pro Microphone"),
                &built_in,
                CLOSED
            ),
            Err(LID_CLOSED_ERROR.into())
        );
    }

    #[test]
    fn device_override_wins_and_never_falls_back() {
        let names = listed(&["Hex Microphone", "ALEX PODS", "USB Mic"]);
        let plan = |device_override| {
            resolve_input(
                device_override,
                Some("ALEX PODS"),
                &names,
                Some("ALEX PODS"),
                other,
                OPEN,
            )
        };

        assert_eq!(
            plan(Some("USB Mic")),
            Ok(InputPlan {
                target: InputTarget::Named(2),
                selection: Selection::InUse,
            })
        );
        assert_eq!(
            plan(Some("Missing Mic")),
            Err("microphone is unavailable: Missing Mic".into())
        );
        assert!(
            plan(Some("Hex Microphone"))
                .unwrap_err()
                .contains("its own virtual microphone")
        );
    }

    static TEST_ROUTE_GENERATION: AtomicU64 = AtomicU64::new(0);

    fn test_route_generation() -> u64 {
        TEST_ROUTE_GENERATION.load(Ordering::Acquire)
    }

    /// A route generation only this test changes, so another test's route
    /// change cannot open the stream it is about to fail.
    static DEAD_STREAM_ROUTE_GENERATION: AtomicU64 = AtomicU64::new(0);

    fn dead_stream_route_generation() -> u64 {
        DEAD_STREAM_ROUTE_GENERATION.load(Ordering::Acquire)
    }

    /// Polls until any open the input starts has finished, returning the
    /// last event.
    fn settle(input: &mut RecoveringAudioInput, capture_idle: bool) -> RecoveringAudioInputEvent {
        let mut event = input.recv_timeout(Duration::ZERO, capture_idle);
        let deadline = Instant::now() + Duration::from_secs(5);
        while input.is_opening() && Instant::now() < deadline {
            event = input.recv_timeout(Duration::from_millis(1), capture_idle);
        }
        event
    }

    fn named_input(name: &str) -> AudioInput {
        let (mut input, samples) = AudioInput::channel_for_test();
        // A dropped sender reads as a failed stream.
        std::mem::forget(samples);
        input.device_name = name.into();
        input
    }

    #[test]
    fn a_default_change_switches_only_while_capture_is_idle() {
        let default = Arc::new(Mutex::new("ALEX PODS".to_owned()));
        let opener_default = default.clone();
        let mut input = RecoveringAudioInput::with_opener_for_test(
            None,
            test_route_generation,
            move |request| {
                let name = opener_default.lock().unwrap().clone();
                if request.keep.as_deref() == Some(name.as_str()) {
                    return Ok(Opened::Unchanged {
                        selection: Selection::InUse,
                    });
                }
                Ok(Opened::Input(Box::new(named_input(&name))))
            },
        );
        input.finish_initial_open(Ok(named_input("ALEX PODS")), Instant::now());

        // The OS default changes during a recording: nothing opens yet.
        *default.lock().unwrap() = "MacBook Pro Microphone".into();
        TEST_ROUTE_GENERATION.fetch_add(1, Ordering::AcqRel);
        assert!(matches!(
            settle(&mut input, false),
            RecoveringAudioInputEvent::Timeout
        ));
        assert!(!input.is_opening());
        assert_eq!(input.device_name(), "ALEX PODS");

        // Once capture is idle, the new default replaces the old one.
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Reopened
        ));
        assert_eq!(input.device_name(), "MacBook Pro Microphone");

        // A device change that leaves the default alone keeps the stream.
        TEST_ROUTE_GENERATION.fetch_add(1, Ordering::AcqRel);
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Timeout
        ));
        assert!(input.recovery.reason.is_none());
        assert_eq!(input.device_name(), "MacBook Pro Microphone");
    }

    /// An input saved as "USB Mic" that opens once `connected` is set and
    /// until then stands in with "ALEX PODS" for the given reason.
    fn standing_in(
        missing: Selection,
        reports_new_devices: bool,
        route_generation: fn() -> u64,
    ) -> (
        RecoveringAudioInput,
        Arc<std::sync::atomic::AtomicBool>,
        Instant,
    ) {
        let connected = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let opener_connected = connected.clone();
        let mut input = RecoveringAudioInput::with_opener_for_test(
            Some("USB Mic"),
            route_generation,
            move |request| {
                let (name, selection) = if opener_connected.load(Ordering::Acquire) {
                    ("USB Mic", Selection::InUse)
                } else {
                    ("ALEX PODS", missing)
                };
                if request.keep.as_deref() == Some(name) {
                    return Ok(Opened::Unchanged { selection });
                }
                let mut opened = named_input(name);
                opened.selection = selection;
                Ok(Opened::Input(Box::new(opened)))
            },
        );
        input.reports_new_devices = reports_new_devices;
        let mut stand_in = named_input("ALEX PODS");
        stand_in.selection = missing;
        let started = Instant::now();
        input.finish_initial_open(Ok(stand_in), started);
        (input, connected, started)
    }

    #[test]
    fn a_missing_selection_uses_the_default_until_it_returns() {
        let (mut input, connected, started) = standing_in(Selection::Unlisted, false, || 0);
        assert_eq!(
            input.recovery.next_attempt,
            Some(started + MICROPHONE_RETRY_INITIAL)
        );

        // Still missing: the default keeps running and the next look backs off.
        input.recovery.next_attempt = Some(Instant::now());
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Timeout
        ));
        assert_eq!(input.device_name(), "ALEX PODS");
        assert_eq!(input.selection_retry, MICROPHONE_RETRY_INITIAL * 4);

        connected.store(true, Ordering::Release);
        input.recovery.next_attempt = Some(Instant::now());
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Reopened
        ));
        assert_eq!(input.device_name(), "USB Mic");
        assert_eq!(input.input.as_ref().unwrap().selection, Selection::InUse);
        assert!(input.recovery.next_attempt.is_none());
        assert_eq!(input.selected_device.as_deref(), Some("USB Mic"));
    }

    static DEVICE_ARRIVALS: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn where_the_os_reports_new_devices_only_a_failed_open_is_retried() {
        let arrivals = || DEVICE_ARRIVALS.load(Ordering::Acquire);

        // An unlisted microphone waits for the OS to report a device.
        let (mut input, connected, _) = standing_in(Selection::Unlisted, true, arrivals);
        assert!(input.recovery.next_attempt.is_none());
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Timeout
        ));
        assert!(!input.is_opening());
        connected.store(true, Ordering::Release);
        DEVICE_ARRIVALS.fetch_add(1, Ordering::AcqRel);
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Reopened
        ));
        assert_eq!(input.device_name(), "USB Mic");

        // A listed microphone that would not open is looked at again.
        let (input, _, started) = standing_in(Selection::Unopenable, true, arrivals);
        assert_eq!(
            input.recovery.next_attempt,
            Some(started + MICROPHONE_RETRY_INITIAL)
        );
    }

    #[test]
    fn only_an_idle_capture_lets_the_microphone_switch() {
        assert!(capture_idle(false, false, false));
        // Recording, a press waiting for its stream, or a release's tail.
        assert!(!capture_idle(true, false, false));
        assert!(!capture_idle(false, true, false));
        assert!(!capture_idle(false, false, true));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mach_event_ticks_are_converted_to_nanoseconds() {
        assert_eq!(scale_mach_ticks(72_000_000, 125, 3), 3_000_000_000);
        assert_eq!(scale_mach_ticks(3_000_000_000, 1, 1), 3_000_000_000);
    }

    #[test]
    fn stream_failures_are_visible_to_the_audio_owner() {
        let (sender, errors) = mpsc::sync_channel(1);

        report_stream_error(
            &sender,
            cpal::Error::with_message(
                cpal::ErrorKind::StreamInvalidated,
                "Device sample rate changed",
            ),
        );

        assert_eq!(errors.try_recv().unwrap(), "Device sample rate changed");
    }

    #[test]
    fn overload_glitches_do_not_interrupt_the_stream() {
        let (sender, errors) = mpsc::sync_channel(1);

        report_stream_error(&sender, cpal::ErrorKind::Xrun.into());
        report_stream_error(&sender, cpal::ErrorKind::RealtimeDenied.into());
        assert!(errors.try_recv().is_err());
        assert!(since_last_stream_glitch().is_some());

        report_stream_error(&sender, cpal::ErrorKind::DeviceNotAvailable.into());
        assert!(errors.try_recv().is_ok());
    }

    #[test]
    fn cold_open_retries_the_selected_microphone_before_falling_back() {
        let open = |fallback, failures: usize| {
            let mut attempts = 0;
            let mut sleeps = Vec::new();
            let result = open_selected(
                "USB microphone",
                fallback,
                || {
                    attempts += 1;
                    if attempts > failures {
                        Ok("selected")
                    } else {
                        Err(eyre!("device is waking"))
                    }
                },
                || Ok("automatic"),
                |delay| sleeps.push(delay),
            )
            .map_err(|error| error.to_string());
            (result, attempts, sleeps.len())
        };

        assert_eq!(
            open(SelectedFallback::AfterRetries, 2),
            (Ok("selected"), 3, 2)
        );
        assert_eq!(
            open(SelectedFallback::AfterRetries, 3),
            (Ok("automatic"), 3, 2)
        );
        assert_eq!(
            open(SelectedFallback::Immediate, 1),
            (Ok("automatic"), 1, 0)
        );
    }

    #[test]
    fn closed_input_applies_selection_without_opening() {
        let mut input = RecoveringAudioInput::closed(None, 3, Some("Old microphone"));

        input.request_selection(4, Some("Next microphone"));

        assert!(!input.is_open());
        assert_eq!(input.active_revision, 4);
        assert_eq!(input.selected_device.as_deref(), Some("Next microphone"));
        assert!(input.replacement.is_none());
    }

    #[test]
    fn failed_startup_retry_keeps_backoff_without_an_open_stream() {
        let mut input = RecoveringAudioInput::closed(None, 3, Some("Missing microphone"));
        let started = Instant::now();
        input.finish_initial_open(Err(eyre!("device unavailable")), started);
        assert!(input.is_recovering());
        assert!(
            !input
                .recovery
                .should_attempt(started + Duration::from_millis(249))
        );
        assert!(
            input
                .recovery
                .should_attempt(started + Duration::from_millis(250))
        );
        let (sender, receiver) = mpsc::channel();
        input.replacement = Some(receiver);
        sender.send((3, Err("device unavailable".into()))).unwrap();

        let before = Instant::now();
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Timeout
        ));
        let after = Instant::now();
        assert!(!input.is_open());
        assert!(!input.is_opening());
        assert!(input.is_recovering());
        let next_attempt = input.recovery.next_attempt.unwrap();
        assert!(next_attempt >= before + Duration::from_millis(500));
        assert!(next_attempt <= after + Duration::from_millis(500));

        input.cancel_open();
        input.request_open();
        assert!(input.is_recovering());
        assert!(!input.is_opening());
        assert_eq!(input.recovery.next_attempt, Some(next_attempt));

        input.request_selection(4, Some("Next microphone"));
        assert!(input.is_recovering());
        assert_eq!(input.recovery.target_revision, Some(4));
        assert_eq!(input.active_revision, 3);
        assert_eq!(input.selected_device.as_deref(), Some("Next microphone"));
        assert!(input.recovery.should_attempt(Instant::now()));
    }

    #[test]
    fn only_the_listeners_current_input_feeds_hex_microphone() {
        let (meeting, _) = AudioInput::channel_for_test();
        let (first, _) = AudioInput::channel_for_test();
        let (replacement, _) = AudioInput::channel_for_test();
        let first_feed = first.hex_microphone.clone();
        let replacement_feed = replacement.hex_microphone.clone();

        let mut input = RecoveringAudioInput::closed(None, 3, Some("Test microphone"));
        input.finish_initial_open(Ok(first), Instant::now());
        assert!(first_feed.is_on());
        assert!(!meeting.hex_microphone.is_on());

        let (sender, receiver) = mpsc::channel();
        input.replacement = Some(receiver);
        sender
            .send((3, Ok(Opened::Input(Box::new(replacement)))))
            .unwrap();
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Reopened
        ));
        assert!(!first_feed.is_on());
        assert!(replacement_feed.is_on());

        input.close();
        assert!(!replacement_feed.is_on());
    }

    #[test]
    fn startup_retry_recovers_metadata_and_audio_without_restarting() {
        let mut input = RecoveringAudioInput::closed(None, 3, Some("Test microphone"));
        input.finish_initial_open(Err(eyre!("device unavailable")), Instant::now());
        let (sender, receiver) = mpsc::channel();
        input.replacement = Some(receiver);
        let (opened, samples) = AudioInput::channel_for_test();
        sender
            .send((3, Ok(Opened::Input(Box::new(opened)))))
            .unwrap();

        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Reopened
        ));
        assert!(!input.is_recovering());
        assert!(!input.is_opening());
        assert!(input.recovery.next_attempt.is_none());
        assert_eq!(input.sample_rate(), 48_000);
        assert_eq!(input.device_name(), "Test microphone");
        let at = CaptureInstant::from_nanos(60_000_000_000);
        samples.send((vec![0.25; 480], at)).unwrap();
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Chunk { samples, captured_through }
                if samples == vec![0.25; 480] && captured_through == at
        ));
    }

    #[test]
    fn startup_recovery_opens_even_when_a_capture_is_pending() {
        let mut input = RecoveringAudioInput::closed(
            Some("Hex nonexistent microphone for pending recovery test"),
            3,
            None,
        );
        input.request_recovery();
        input.request_selection(4, Some("Ignored selection under CLI override"));
        assert_eq!(input.active_revision, 3);
        assert!(input.selected_device.is_none());

        assert!(matches!(
            input.recv_timeout(Duration::ZERO, false),
            RecoveringAudioInputEvent::Timeout
        ));
        assert!(input.is_opening());
        input.cancel_open();
        assert!(input.is_opening());
        input.close();
        assert!(!input.is_opening());
        assert!(!input.is_recovering());
        assert!(input.recovery.next_attempt.is_none());
    }

    #[test]
    fn failed_on_demand_open_does_not_retry_while_idle() {
        let mut input = RecoveringAudioInput::closed(None, 3, None);
        let (sender, receiver) = mpsc::channel();
        input.replacement = Some(receiver);
        sender.send((3, Err("device unavailable".into()))).unwrap();

        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::OpenFailed(error) if error == "device unavailable"
        ));
        assert!(!input.is_recovering());
        assert!(!input.is_opening());
        assert!(input.recovery.next_attempt.is_none());
    }

    #[test]
    fn microphone_recovery_retries_with_bounded_backoff_and_resets() {
        let started = Instant::now();
        let mut recovery = MicrophoneRecovery::default();

        recovery.request_selection_change(1, started);
        assert!(!recovery.blocks_audio());
        assert!(recovery.should_attempt(started));

        recovery.failed(started);
        recovery.request_selection_change(1, started + Duration::from_millis(100));
        assert!(!recovery.should_attempt(started + Duration::from_millis(249)));
        assert!(recovery.should_attempt(started + Duration::from_millis(250)));

        let mut now = started + Duration::from_millis(250);
        for expected_delay in [500, 1_000, 2_000, 4_000, 5_000, 5_000] {
            recovery.failed(now);
            assert!(!recovery.should_attempt(now + Duration::from_millis(expected_delay - 1)));
            now += Duration::from_millis(expected_delay);
            assert!(recovery.should_attempt(now));
        }

        recovery.recovered();
        assert!(!recovery.blocks_audio());
        assert!(!recovery.should_attempt(now));
        recovery.request_stream_recovery(now);
        assert!(recovery.blocks_audio());
        recovery.failed(now);
        recovery.request_stream_recovery(now + Duration::from_millis(100));
        recovery.request_selection_change(2, now + Duration::from_millis(100));
        assert!(recovery.blocks_audio());
        assert!(recovery.should_attempt(now + Duration::from_millis(100)));
    }

    #[test]
    fn audio_service_restart_interrupts_stream_and_recovers() {
        let mut input = RecoveringAudioInput::closed(None, 1, Some("Test microphone"));
        let (mut opened, _samples) = AudioInput::channel_for_test();
        opened.watch_service_restarts_for_test();
        input.finish_initial_open(Ok(opened), Instant::now());
        assert!(input.is_open());

        notify_audio_service_restarted();
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Interrupted
        ));
        assert!(input.is_recovering());

        let (sender, receiver) = mpsc::channel();
        input.replacement = Some(receiver);
        let (reopened, samples_after) = AudioInput::channel_for_test();
        sender
            .send((1, Ok(Opened::Input(Box::new(reopened)))))
            .unwrap();

        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Reopened
        ));
        assert!(!input.is_recovering());
        let at = CaptureInstant::from_nanos(10_000_000_000);
        samples_after.send((vec![0.5; 240], at)).unwrap();
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Chunk { samples, captured_through }
                if samples == vec![0.5; 240] && captured_through == at
        ));
    }

    #[test]
    fn a_stream_that_stops_delivering_audio_is_reopened() {
        let stall = Duration::from_millis(400);
        let mut input = RecoveringAudioInput::closed(None, 1, Some("Test microphone"));
        let (mut opened, samples) = AudioInput::channel_for_test();
        opened.watch_stalls_for_test(stall);
        input.finish_initial_open(Ok(opened), Instant::now());

        // Delivering audio keeps the stream alive.
        let at = CaptureInstant::from_nanos(10_000_000_000);
        std::thread::sleep(stall / 4);
        samples.send((vec![0.5; 240], at)).unwrap();
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Chunk { .. }
        ));
        std::thread::sleep(stall / 4);
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Timeout
        ));

        // A quiet stream with no error counts as failed and recovers.
        std::thread::sleep(stall);
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Interrupted
        ));
        assert!(input.is_recovering());
    }

    /// What the opener saw of the previous stream's feed switch when it ran.
    fn recording_opener(
        seen: &Arc<Mutex<Option<bool>>>,
        switch: &hex_microphone::CaptureFeedSwitch,
    ) -> impl Fn(&OpenRequest) -> Result<Opened> + Send + Sync + 'static {
        let seen = seen.clone();
        let switch = switch.clone();
        move |_| {
            *seen.lock().unwrap() = Some(switch.is_on());
            Ok(Opened::Input(Box::new(named_input(
                "Replacement microphone",
            ))))
        }
    }

    #[test]
    fn a_dead_stream_is_stopped_before_its_replacement_opens() {
        // A failed stream lets go of the device before its replacement opens.
        let (failed, samples) = AudioInput::channel_for_test();
        let failed_switch = failed.hex_microphone.clone();
        let seen = Arc::new(Mutex::new(None));
        let mut input = RecoveringAudioInput::with_opener_for_test(
            None,
            dead_stream_route_generation,
            recording_opener(&seen, &failed_switch),
        );
        input.finish_initial_open(Ok(failed), Instant::now());
        drop(samples);
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Interrupted
        ));
        // A stream that never delivered waits out its first backoff; let the
        // wait pass.
        input.recovery.next_attempt = Some(Instant::now());
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Reopened
        ));
        assert_eq!(*seen.lock().unwrap(), Some(false));
        assert!(!failed_switch.is_on());

        // A route change still keeps the old stream until the new one installs.
        let (old, samples) = AudioInput::channel_for_test();
        let old_switch = old.hex_microphone.clone();
        let seen = Arc::new(Mutex::new(None));
        let mut routed = RecoveringAudioInput::with_opener_for_test(
            None,
            dead_stream_route_generation,
            recording_opener(&seen, &old_switch),
        );
        routed.finish_initial_open(Ok(old), Instant::now());
        std::mem::forget(samples);
        DEAD_STREAM_ROUTE_GENERATION.fetch_add(1, Ordering::AcqRel);
        assert!(matches!(
            settle(&mut routed, true),
            RecoveringAudioInputEvent::Reopened
        ));
        assert_eq!(*seen.lock().unwrap(), Some(true));
        assert!(!old_switch.is_on());
    }

    /// A route generation only the silence test changes, so another test's
    /// route change cannot reopen its microphone.
    static SILENCE_ROUTE_GENERATION: AtomicU64 = AtomicU64::new(0);

    fn silence_route_generation() -> u64 {
        SILENCE_ROUTE_GENERATION.load(Ordering::Acquire)
    }

    #[test]
    fn a_silent_stream_reopens_with_a_rate_renegotiation() {
        let renegotiations = Arc::new(Mutex::new(Vec::new()));
        let seen = renegotiations.clone();
        let mut input = RecoveringAudioInput::with_opener_for_test(
            None,
            silence_route_generation,
            move |request| {
                seen.lock().unwrap().push(request.renegotiate_rate);
                Ok(Opened::Input(Box::new(named_input("Test microphone"))))
            },
        );
        input.finish_initial_open(Ok(named_input("Test microphone")), Instant::now());

        // A dictation of digital silence reopens the microphone, and that open
        // renegotiates the device's sample rate first (#1575).
        input.request_silence_recovery();
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Reopened
        ));
        // A stream failure reopens the same microphone without it.
        input.request_recovery();
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Reopened
        ));
        // Only the silence open renegotiated; the next open took nothing.
        assert_eq!(*renegotiations.lock().unwrap(), vec![true, false]);
    }

    /// A route generation only the stall tests change, so another test's
    /// route change cannot end a stalled stream's wait.
    static STALLED_ROUTE_GENERATION: AtomicU64 = AtomicU64::new(0);

    fn stalled_route_generation() -> u64 {
        STALLED_ROUTE_GENERATION.load(Ordering::Acquire)
    }

    /// A stream whose sender is gone: polling it reads as a failed stream,
    /// with no device behind it.
    fn stalling_input() -> AudioInput {
        let (mut input, samples) = AudioInput::channel_for_test();
        drop(samples);
        input.device_name = "Stalling microphone".into();
        input
    }

    /// A stream's chunk sender, kept so a test can stop the stream.
    type ChunkSender = Sender<(Vec<f32>, CaptureInstant)>;

    /// Opens a live stream and keeps its sender, so a test can stop it.
    fn opener_keeping_sender(
        sender: &Arc<Mutex<Option<ChunkSender>>>,
    ) -> impl Fn(&OpenRequest) -> Result<Opened> + Send + Sync + 'static {
        let sender = sender.clone();
        move |_| {
            let (mut input, samples) = AudioInput::channel_for_test();
            input.device_name = "Reopened microphone".into();
            *sender.lock().unwrap() = Some(samples);
            Ok(Opened::Input(Box::new(input)))
        }
    }

    #[test]
    fn a_stream_that_keeps_stalling_backs_off_to_a_minute() {
        let mut input = RecoveringAudioInput::with_opener_for_test(
            None,
            || 0,
            |_| Ok(Opened::Input(Box::new(stalling_input()))),
        );
        input.finish_initial_open(Ok(stalling_input()), Instant::now());

        let mut delays = Vec::new();
        for _ in 0..10 {
            assert!(matches!(
                input.recv_timeout(Duration::ZERO, true),
                RecoveringAudioInputEvent::Interrupted
            ));
            delays.push(input.recovery.delay);
            // Let the wait pass without sleeping; the reopen keeps it.
            input.recovery.next_attempt = Some(Instant::now());
            assert!(matches!(
                settle(&mut input, true),
                RecoveringAudioInputEvent::Reopened
            ));
        }
        assert_eq!(
            delays,
            [
                500, 1_000, 2_000, 4_000, 8_000, 16_000, 32_000, 60_000, 60_000, 60_000
            ]
            .map(Duration::from_millis)
        );

        // The last wait is a full minute, not the next stall timeout.
        let failed_at = Instant::now();
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Interrupted
        ));
        assert!(input.recovery.next_attempt.unwrap() >= failed_at + Duration::from_secs(60));
    }

    #[test]
    fn a_reopened_stream_keeps_the_stall_backoff_its_predecessors_grew() {
        let fails = Arc::new(AtomicBool::new(false));
        let opener_fails = fails.clone();
        let mut input = RecoveringAudioInput::with_opener_for_test(
            None,
            || 0,
            move |_| {
                if opener_fails.load(Ordering::Acquire) {
                    return Err(eyre!("microphone is busy"));
                }
                Ok(Opened::Input(Box::new(stalling_input())))
            },
        );
        input.finish_initial_open(Ok(stalling_input()), Instant::now());

        // Six streams that never deliver take the wait to 16 s.
        for expected in [500, 1_000, 2_000, 4_000, 8_000, 16_000] {
            assert!(matches!(
                input.recv_timeout(Duration::ZERO, true),
                RecoveringAudioInputEvent::Interrupted
            ));
            assert_eq!(input.recovery.delay, Duration::from_millis(expected));
            input.recovery.next_attempt = Some(Instant::now());
            assert!(matches!(
                settle(&mut input, true),
                RecoveringAudioInputEvent::Reopened
            ));
            assert_eq!(input.recovery.delay, Duration::from_millis(expected));
            assert!(input.recovery.next_attempt.is_none());
        }

        // The reopened stream stops too, and its failure waits the grown 16 s.
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Interrupted
        ));
        assert_eq!(input.recovery.delay, Duration::from_secs(32));
        assert!(!input.recovery.should_attempt(Instant::now()));

        // Let that wait pass into a failed open: it waits the grown 32 s and
        // keeps growing to the minute, not back to the five-second open cap.
        fails.store(true, Ordering::Release);
        input.recovery.next_attempt = Some(Instant::now());
        let before = Instant::now();
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Timeout
        ));
        assert!(input.is_opening());
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Timeout
        ));
        assert!(input.recovery.next_attempt.unwrap() >= before + Duration::from_secs(32));
        assert_eq!(input.recovery.delay, Duration::from_secs(60));
    }

    #[test]
    fn a_stream_that_delivers_steadily_reopens_at_once_and_forgets_the_backoff() {
        let sender = Arc::new(Mutex::new(None));
        let mut input =
            RecoveringAudioInput::with_opener_for_test(None, || 0, opener_keeping_sender(&sender));
        input.finish_initial_open(Ok(stalling_input()), Instant::now());
        // Two streams that never deliver take the wait to 1 s.
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Interrupted
        ));
        input.recovery.next_attempt = Some(Instant::now());
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Reopened
        ));
        // The reopened stream stops before it delivers anything either.
        drop(sender.lock().unwrap().take());
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Interrupted
        ));
        input.recovery.next_attempt = Some(Instant::now());
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Reopened
        ));
        assert_eq!(input.recovery.delay, Duration::from_secs(1));

        // Delivering across the proof window forgets that wait.
        input.first_delivery = Some(Instant::now() - STREAM_PROVEN_UPTIME);
        let at = CaptureInstant::from_nanos(30_000_000_000);
        sender
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .send((vec![0.5; 240], at))
            .unwrap();
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Chunk { .. }
        ));
        assert_eq!(input.recovery.delay, MICROPHONE_RETRY_INITIAL);

        // Its failure reopens at once, as a working stream always has, with
        // the backoff it no longer needs.
        drop(sender.lock().unwrap().take());
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Interrupted
        ));
        assert!(input.recovery.should_attempt(Instant::now()));
        assert_eq!(input.recovery.delay, MICROPHONE_RETRY_INITIAL);
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Reopened
        ));
    }

    #[test]
    fn a_stream_that_stops_before_proving_itself_backs_off() {
        let sender = Arc::new(Mutex::new(None));
        let mut input =
            RecoveringAudioInput::with_opener_for_test(None, || 0, opener_keeping_sender(&sender));
        input.finish_initial_open(Ok(stalling_input()), Instant::now());
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Interrupted
        ));
        input.recovery.next_attempt = Some(Instant::now());
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Reopened
        ));

        // One chunk is not proof: the stream stopped before delivering across
        // the proof window, so its next reopen backs off instead of looping.
        let at = CaptureInstant::from_nanos(1_000_000_000);
        sender
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .send((vec![0.5; 240], at))
            .unwrap();
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Chunk { .. }
        ));
        assert!(!input.proven);
        let failed_at = Instant::now();
        drop(sender.lock().unwrap().take());
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Interrupted
        ));
        assert!(!input.recovery.should_attempt(Instant::now()));
        assert!(input.recovery.next_attempt.unwrap() >= failed_at + Duration::from_millis(500));
        assert_eq!(input.recovery.delay, Duration::from_secs(1));
    }

    #[test]
    fn a_route_change_ends_a_stall_wait_without_resetting_its_backoff() {
        let connected = Arc::new(AtomicBool::new(false));
        let opener_connected = connected.clone();
        let mut input =
            RecoveringAudioInput::with_opener_for_test(None, stalled_route_generation, move |_| {
                let opened = if opener_connected.load(Ordering::Acquire) {
                    named_input("Next microphone")
                } else {
                    stalling_input()
                };
                Ok(Opened::Input(Box::new(opened)))
            });
        input.finish_initial_open(Ok(stalling_input()), Instant::now());
        for _ in 0..2 {
            assert!(matches!(
                input.recv_timeout(Duration::ZERO, true),
                RecoveringAudioInputEvent::Interrupted
            ));
            input.recovery.next_attempt = Some(Instant::now());
            assert!(matches!(
                settle(&mut input, true),
                RecoveringAudioInputEvent::Reopened
            ));
        }

        // The next failed stream waits out the grown backoff.
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Interrupted
        ));
        assert_eq!(input.recovery.delay, Duration::from_secs(2));
        assert!(!input.recovery.should_attempt(Instant::now()));
        // The dead stream was released when its wait began.
        assert!(!input.is_open());

        // The lid opening resolves the input at once, not in two seconds,
        // and keeps the wait the stalled streams grew.
        connected.store(true, Ordering::Release);
        STALLED_ROUTE_GENERATION.fetch_add(1, Ordering::AcqRel);
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Timeout
        ));
        assert!(input.is_opening());
        assert_eq!(input.recovery.delay, Duration::from_secs(2));
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Reopened
        ));
        assert_eq!(input.device_name(), "Next microphone");
        assert_eq!(input.recovery.delay, Duration::from_secs(2));
    }

    #[test]
    fn a_dictation_press_retries_a_stalled_microphone_at_once() {
        let mut input = RecoveringAudioInput::with_opener_for_test(
            None,
            || 0,
            |_| Ok(Opened::Input(Box::new(stalling_input()))),
        );
        input.finish_initial_open(Ok(stalling_input()), Instant::now());
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Interrupted
        ));
        assert!(!input.recovery.should_attempt(Instant::now()));
        let delay = input.recovery.delay;

        input.retry_now();
        assert!(input.recovery.should_attempt(Instant::now()));
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Timeout
        ));
        assert!(input.is_opening());
        // The press opens the microphone now; it does not start the backoff
        // over for the streams that keep stalling.
        assert_eq!(input.recovery.delay, delay);
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Reopened
        ));
        assert_eq!(input.recovery.delay, delay);
    }
}

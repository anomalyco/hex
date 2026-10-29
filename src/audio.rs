use std::ops::Add;
#[cfg(target_os = "macos")]
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static AUDIO_SERVICE_RESTART_GENERATION: AtomicU64 = AtomicU64::new(0);
static INPUT_ROUTE_GENERATION: AtomicU64 = AtomicU64::new(0);
static STREAM_GLITCHED_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// How long ago a microphone stream last reported an overload it continued
/// through, for diagnosing captures.
pub fn since_last_stream_glitch() -> Option<Duration> {
    STREAM_GLITCHED_AT
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .map(|at| at.elapsed())
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

/// Registers the CoreAudio listeners behind [`audio_service_restart_generation`]
/// and [`input_route_generation`]. Their callbacks run on CoreAudio's
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

    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        const SYSTEM_OBJECT: u32 = 1;
        const SERVICE_RESTARTED_SELECTOR: u32 = u32::from_be_bytes(*b"srst");
        const DEFAULT_INPUT_SELECTOR: u32 = u32::from_be_bytes(*b"dIn ");
        const DEVICES_SELECTOR: u32 = u32::from_be_bytes(*b"dev#");
        const GLOBAL_SCOPE: u32 = u32::from_be_bytes(*b"glob");
        let listeners: [(u32, PropertyListenerProc); 3] = [
            (SERVICE_RESTARTED_SELECTOR, on_service_restarted),
            (DEFAULT_INPUT_SELECTOR, on_inputs_changed),
            (DEVICES_SELECTOR, on_inputs_changed),
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
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, SizedSample, Stream, StreamConfig};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CaptureInstant(u64);

impl CaptureInstant {
    pub const ZERO: Self = Self(0);

    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    #[cfg(target_os = "macos")]
    pub fn now() -> Self {
        unsafe extern "C" {
            fn mach_absolute_time() -> u64;
        }
        Self::from_mach_ticks(unsafe { mach_absolute_time() })
    }

    /// The monotonic clock. Linux's microphone streams count from when they
    /// opened, so the capture timeline anchors them to this.
    #[cfg(target_os = "linux")]
    pub fn now() -> Self {
        let mut now = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `now` is a valid timespec to fill.
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) };
        Self(now.tv_sec as u64 * 1_000_000_000 + now.tv_nsec as u64)
    }

    #[cfg(target_os = "macos")]
    pub fn from_mach_ticks(ticks: u64) -> Self {
        let Some((numerator, denominator)) = mach_timebase() else {
            return Self::ZERO;
        };
        Self(scale_mach_ticks(ticks, numerator, denominator))
    }

    pub fn from_stream(instant: cpal::StreamInstant) -> Self {
        Self(u64::try_from(instant.as_nanos()).unwrap_or(u64::MAX))
    }

    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        let nanos = u64::try_from(duration.as_nanos()).ok()?;
        self.0.checked_add(nanos).map(Self)
    }

    pub fn checked_sub(self, duration: Duration) -> Option<Self> {
        let nanos = u64::try_from(duration.as_nanos()).ok()?;
        self.0.checked_sub(nanos).map(Self)
    }

    pub fn checked_duration_since(self, earlier: Self) -> Option<Duration> {
        self.0.checked_sub(earlier.0).map(Duration::from_nanos)
    }

    pub fn duration_since(self, earlier: Self) -> Duration {
        Duration::from_nanos(self.0.saturating_sub(earlier.0))
    }

    pub fn sub_frames(self, frames: usize, sample_rate: u32) -> Self {
        self.checked_sub(duration_for_frames(frames, sample_rate))
            .unwrap_or(self)
    }
}

pub fn duration_for_frames(frames: usize, sample_rate: u32) -> Duration {
    let nanos = frames as u128 * 1_000_000_000 / u128::from(sample_rate);
    Duration::from_nanos(nanos as u64)
}

#[cfg(target_os = "macos")]
fn scale_mach_ticks(ticks: u64, numerator: u32, denominator: u32) -> u64 {
    let nanos = u128::from(ticks) * u128::from(numerator) / u128::from(denominator);
    u64::try_from(nanos).unwrap_or(u64::MAX)
}

#[cfg(target_os = "macos")]
fn mach_timebase() -> Option<(u32, u32)> {
    #[repr(C)]
    struct MachTimebaseInfo {
        numer: u32,
        denom: u32,
    }

    unsafe extern "C" {
        fn mach_timebase_info(info: *mut MachTimebaseInfo) -> i32;
    }

    static TIMEBASE: OnceLock<Option<(u32, u32)>> = OnceLock::new();
    *TIMEBASE.get_or_init(|| {
        let mut info = MachTimebaseInfo { numer: 0, denom: 0 };
        let status = unsafe { mach_timebase_info(&mut info) };
        (status == 0 && info.denom != 0).then_some((info.numer, info.denom))
    })
}

impl Add<Duration> for CaptureInstant {
    type Output = Self;

    fn add(self, duration: Duration) -> Self::Output {
        self.checked_add(duration)
            .expect("capture instant overflowed")
    }
}

pub struct AudioInput {
    // Tests can deliver PCM through channels without opening a native microphone.
    _stream: Option<Stream>,
    chunks: Receiver<(Vec<f32>, CaptureInstant)>,
    pub sample_rate: u32,
    pub device_name: String,
    /// Whether this input stands in for the saved microphone until it can
    /// be opened again.
    pub selection: Selection,
    stream_errors: Receiver<String>,
    watch_service_restarts: bool,
    restart_generation: u64,
    hex_microphone: crate::hex_microphone::CaptureFeedSwitch,
}

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
}

type Opener = Arc<dyn Fn(&OpenRequest) -> Result<Opened> + Send + Sync>;

pub struct RecoveringAudioInput {
    input: Option<AudioInput>,
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

const MICROPHONE_RETRY_INITIAL: Duration = Duration::from_millis(250);
const MICROPHONE_RETRY_MAX: Duration = Duration::from_secs(5);
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MicrophoneRecoveryReason {
    /// The default input or the list of inputs changed, or a missing saved
    /// microphone is due another look. The open input keeps running.
    RouteChanged,
    SelectionChanged,
    StreamFailed,
}

struct MicrophoneRecovery {
    next_attempt: Option<Instant>,
    delay: Duration,
    reason: Option<MicrophoneRecoveryReason>,
    target_revision: Option<u64>,
}

impl Default for MicrophoneRecovery {
    fn default() -> Self {
        Self {
            next_attempt: None,
            delay: MICROPHONE_RETRY_INITIAL,
            reason: None,
            target_revision: None,
        }
    }
}

impl MicrophoneRecovery {
    fn request_selection_change(&mut self, revision: u64, now: Instant) {
        if self.target_revision != Some(revision) {
            self.next_attempt = Some(now);
            self.delay = MICROPHONE_RETRY_INITIAL;
            self.target_revision = Some(revision);
            if self.reason != Some(MicrophoneRecoveryReason::StreamFailed) {
                self.reason = Some(MicrophoneRecoveryReason::SelectionChanged);
            }
        }
    }

    /// Resolves the input again at `at`. A pending reopen or selection
    /// change resolves it anyway, so it is left alone.
    fn request_route_check(&mut self, at: Instant) {
        match self.reason {
            None => {
                self.reason = Some(MicrophoneRecoveryReason::RouteChanged);
                self.next_attempt = Some(at);
                self.delay = MICROPHONE_RETRY_INITIAL;
            }
            Some(MicrophoneRecoveryReason::RouteChanged) => {
                self.next_attempt = Some(self.next_attempt.map_or(at, |next| next.min(at)));
            }
            Some(_) => {}
        }
    }

    fn request_stream_recovery(&mut self, now: Instant) {
        if self.reason != Some(MicrophoneRecoveryReason::StreamFailed) {
            self.next_attempt = Some(now);
            self.delay = MICROPHONE_RETRY_INITIAL;
            self.reason = Some(MicrophoneRecoveryReason::StreamFailed);
        }
    }

    fn blocks_audio(&self) -> bool {
        self.reason == Some(MicrophoneRecoveryReason::StreamFailed)
    }

    fn should_attempt(&self, now: Instant) -> bool {
        self.next_attempt.is_some_and(|next| now >= next)
    }

    fn failed(&mut self, now: Instant) {
        self.next_attempt = Some(now + self.delay);
        self.delay = self.delay.saturating_mul(2).min(MICROPHONE_RETRY_MAX);
    }

    fn recovered(&mut self) {
        self.next_attempt = None;
        self.delay = MICROPHONE_RETRY_INITIAL;
        self.reason = None;
        self.target_revision = None;
    }
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
                selection: Selection::InUse,
                watch_service_restarts: false,
                restart_generation: audio_service_restart_generation(),
                hex_microphone: crate::hex_microphone::capture_feed(48_000).1,
            },
            sender,
        )
    }

    #[cfg(test)]
    pub fn watch_service_restarts_for_test(&mut self) {
        self.watch_service_restarts = true;
        self.restart_generation = audio_service_restart_generation();
    }

    /// The OS's default input, or a real one standing in when the default is
    /// Hex Microphone or missing.
    pub fn open_system_default() -> Result<Self> {
        match open_configured_input(&OpenRequest {
            device_override: None,
            selected_device: None,
            fallback: SelectedFallback::Immediate,
            keep: None,
        })? {
            Opened::Input(input) => Ok(*input),
            Opened::Unchanged { .. } => unreachable!("nothing to keep"),
        }
    }

    pub fn open_named(name: &str) -> Result<Self> {
        let host = cpal::default_host();
        let (devices, names) = input_devices_of(&host)?;
        let plan = resolve_input(Some(name), None, &names, None, |_| false, false)
            .map_err(|e| eyre!(e))?;
        let InputTarget::Named(index) = plan.target else {
            unreachable!("an override resolves by name");
        };
        Self::open_device(devices[index].clone(), names[index].clone(), false)
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    pub fn open_matching(query: &str) -> Result<Self> {
        let host = cpal::default_host();
        let (devices, names) = input_devices_of(&host)?;
        let query_lower = query.to_lowercase();
        let index = names
            .iter()
            .position(|name| name.to_lowercase().contains(&query_lower))
            .ok_or_else(|| eyre!("microphone is unavailable: {query}"))?;
        Self::open_device(devices[index].clone(), names[index].clone(), false)
    }

    /// Opens `device`. `follows_default` marks cpal's default-input handle,
    /// whose Windows stream reports a default change as invalidation while
    /// it keeps running on the old device.
    fn open_device(device: Device, device_name: String, follows_default: bool) -> Result<Self> {
        #[cfg(target_os = "macos")]
        watch_input_devices();
        let restart_generation = audio_service_restart_generation();
        let supported = device
            .default_input_config()
            .wrap_err("could not read the input device configuration")?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let sample_rate = config.sample_rate;
        let channels = usize::from(config.channels);
        let (sender, chunks) = mpsc::channel();
        let (error_sender, stream_errors) = mpsc::sync_channel(1);
        let reporter = StreamErrorReporter {
            sender: error_sender,
            default_name: follows_default.then(|| device_name.clone()),
        };
        let (feed, hex_microphone) = crate::hex_microphone::capture_feed(sample_rate);

        let stream = match sample_format {
            SampleFormat::F32 => build_stream(
                &device,
                &config,
                channels,
                sender,
                reporter.clone(),
                feed,
                |sample: f32| sample,
            )?,
            SampleFormat::I16 => build_stream(
                &device,
                &config,
                channels,
                sender,
                reporter.clone(),
                feed,
                |sample: i16| sample as f32 / i16::MAX as f32,
            )?,
            SampleFormat::U16 => build_stream(
                &device,
                &config,
                channels,
                sender,
                reporter.clone(),
                feed,
                |sample: u16| sample as f32 / 32768.0 - 1.0,
            )?,
            format => return Err(eyre!("unsupported microphone sample format: {format:?}")),
        };
        stream
            .play()
            .wrap_err("could not start microphone capture")?;

        Ok(Self {
            _stream: Some(stream),
            chunks,
            sample_rate,
            device_name,
            selection: Selection::InUse,
            stream_errors,
            watch_service_restarts: true,
            restart_generation,
            hex_microphone,
        })
    }

    /// Whether this stream's audio feeds Hex Microphone. Only the dictation
    /// listener's current input does; a meeting's second stream on the same
    /// microphone must not.
    pub fn feed_hex_microphone(&self, on: bool) {
        self.hex_microphone.set(on);
    }

    pub fn recv_timeout(&self, timeout: Duration) -> AudioInputEvent {
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
            Ok((samples, captured_through)) => AudioInputEvent::Chunk {
                samples,
                captured_through,
            },
            Err(RecvTimeoutError::Timeout) => AudioInputEvent::Timeout,
            Err(RecvTimeoutError::Disconnected) => {
                AudioInputEvent::StreamFailed("microphone audio channel disconnected".into())
            }
        }
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
        }
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

    pub fn is_recovering(&self) -> bool {
        self.recovery.blocks_audio()
    }

    /// Makes `input` the listener's microphone and the only stream feeding
    /// Hex Microphone, stopping the previous stream first.
    fn install(&mut self, input: AudioInput) {
        if let Some(previous) = self.input.take() {
            previous.feed_hex_microphone(false);
            drop(previous);
        }
        input.feed_hex_microphone(true);
        self.input = Some(input);
    }

    pub fn close(&mut self) {
        if let Some(previous) = self.input.take() {
            previous.feed_hex_microphone(false);
        }
        self.replacement = None;
        self.recovery.recovered();
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
                    if let Some(input) = &mut self.input {
                        input.selection = selection;
                    }
                    self.active_revision = opened_revision;
                    self.recovery.recovered();
                    self.after_resolving(now);
                }
                Ok(Opened::Input(replacement)) if capture_idle || self.input.is_none() => {
                    self.install(*replacement);
                    self.active_revision = opened_revision;
                    self.recovery.recovered();
                    self.after_resolving(now);
                    tracing::info!(device = %self.device_name(), "dictation microphone reopened");
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
                    tracing::warn!(
                        %error,
                        "could not reopen dictation microphone; retrying"
                    );
                }
            }
        }
        // A change while an open is in flight is seen once it finishes.
        if self.input.is_some() && self.device_override.is_none() && self.replacement.is_none() {
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
            } => RecoveringAudioInputEvent::Chunk {
                samples,
                captured_through,
            },
            AudioInputEvent::Timeout => RecoveringAudioInputEvent::Timeout,
            AudioInputEvent::StreamFailed(error) => {
                tracing::warn!(%error, device = %input.device_name, "microphone stream stopped; reopening it");
                self.recovery.request_stream_recovery(Instant::now());
                RecoveringAudioInputEvent::Interrupted
            }
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
        let request = OpenRequest {
            device_override: self.device_override.clone(),
            selected_device: self.selected_device.clone(),
            fallback: SelectedFallback::AfterRetries,
            keep,
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
    /// built-in microphone with the lid closed, or missing: the built-in
    /// microphone while the lid is open, else the first usable one listed.
    StandIn(usize),
}

const LID_CLOSED_ERROR: &str = "the built-in microphone is off while the lid is closed";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InputPlan {
    target: InputTarget,
    selection: Selection,
}

/// Resolves the input Hex captures from: `--device` exactly as named, else
/// the saved microphone while it is listed, else the system default. Hex
/// never captures from its own virtual microphone, so a default that is Hex
/// Microphone (or none) falls back to a real input. With `lid_closed`, the
/// built-in microphone is switched off in hardware though still listed, so
/// it is passed over the same way.
fn resolve_input(
    device_override: Option<&str>,
    selected: Option<&str>,
    names: &[String],
    default: Option<&str>,
    is_built_in: impl Fn(usize) -> bool,
    lid_closed: bool,
) -> std::result::Result<InputPlan, String> {
    let is_hex = crate::hex_microphone::is_hex_virtual_device_name;
    let position = |name: &str| names.iter().position(|listed| listed == name);
    let off = |index: usize| lid_closed && is_built_in(index);
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
    if !names.is_empty()
        && default.is_some_and(|name| !is_hex(name) && !position(name).is_some_and(off))
    {
        return Ok(InputPlan {
            target: InputTarget::SystemDefault,
            selection,
        });
    }
    let usable = || (0..names.len()).filter(|&index| !is_hex(&names[index]) && !off(index));
    usable()
        .find(|&index| is_built_in(index))
        .or_else(|| usable().next())
        .map(|index| InputPlan {
            target: InputTarget::StandIn(index),
            selection,
        })
        .ok_or_else(|| {
            if (0..names.len()).any(off) {
                LID_CLOSED_ERROR.into()
            } else {
                "no microphone is available".into()
            }
        })
}

/// Whether a MacBook's lid is closed, which switches its built-in microphone
/// off in hardware while CoreAudio still lists it, often as the default
/// input. Read from `AppleClamshellState` on IOKit's power root domain;
/// Macs without a lid, and other OSes, read open.
#[cfg(not(target_os = "macos"))]
fn lid_closed() -> bool {
    false
}

#[cfg(target_os = "macos")]
use lid::closed as lid_closed;

#[cfg(target_os = "macos")]
mod lid {
    use std::ffi::{c_char, c_void};
    use std::sync::OnceLock;

    use objc2_core_foundation::CFString;

    const MAIN_PORT_DEFAULT: u32 = 0;
    /// `kIOPMMessageClamshellStateChange`:
    /// `iokit_family_msg(sub_iokit_powermanagement, 0x100)`.
    const CLAMSHELL_STATE_CHANGE: u32 = 0xE003_4100;

    type InterestCallback = unsafe extern "C" fn(*mut c_void, u32, u32, *mut c_void);

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOServiceMatching(name: *const c_char) -> *mut c_void;
        fn IOServiceGetMatchingService(main_port: u32, matching: *mut c_void) -> u32;
        fn IORegistryEntryCreateCFProperty(
            entry: u32,
            key: *const c_void,
            allocator: *const c_void,
            options: u32,
        ) -> *const c_void;
        fn IONotificationPortCreate(main_port: u32) -> *mut c_void;
        fn IONotificationPortSetDispatchQueue(port: *mut c_void, queue: *mut c_void);
        fn IOServiceAddInterestNotification(
            port: *mut c_void,
            service: u32,
            interest_type: *const c_char,
            callback: InterestCallback,
            refcon: *mut c_void,
            notification: *mut u32,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFBooleanTrue: *const c_void;
        fn CFRelease(value: *const c_void);
    }

    unsafe extern "C" {
        fn dispatch_get_global_queue(identifier: isize, flags: usize) -> *mut c_void;
    }

    fn root_domain() -> Option<u32> {
        static ROOT: OnceLock<Option<u32>> = OnceLock::new();
        *ROOT.get_or_init(|| {
            // SAFETY: the matching dictionary is consumed by the lookup, and
            // the returned service is kept for the life of the process.
            let service = unsafe {
                let matching = IOServiceMatching(c"IOPMrootDomain".as_ptr());
                if matching.is_null() {
                    return None;
                }
                IOServiceGetMatchingService(MAIN_PORT_DEFAULT, matching)
            };
            (service != 0).then_some(service)
        })
    }

    pub fn closed() -> bool {
        let Some(root) = root_domain() else {
            return false;
        };
        let key = CFString::from_str("AppleClamshellState");
        let key: *const CFString = &*key;
        // SAFETY: `root` is a live service and `key` a live CFString; the
        // copied property is released after the comparison.
        unsafe {
            let value = IORegistryEntryCreateCFProperty(root, key.cast(), std::ptr::null(), 0);
            if value.is_null() {
                return false;
            }
            let closed = value == kCFBooleanTrue;
            CFRelease(value);
            closed
        }
    }

    /// Calls `changed` on a dispatch queue whenever the lid opens or closes.
    pub fn watch(changed: fn()) {
        static CHANGED: OnceLock<fn()> = OnceLock::new();
        unsafe extern "C" fn on_message(
            _refcon: *mut c_void,
            _service: u32,
            message: u32,
            _argument: *mut c_void,
        ) {
            if message == CLAMSHELL_STATE_CHANGE
                && let Some(changed) = CHANGED.get()
            {
                changed();
            }
        }
        let Some(root) = root_domain() else {
            return;
        };
        if CHANGED.set(changed).is_err() {
            return;
        }
        // SAFETY: the port and notification live for the process, and the
        // callback only reads a static.
        let status = unsafe {
            let port = IONotificationPortCreate(MAIN_PORT_DEFAULT);
            if port.is_null() {
                -1
            } else {
                IONotificationPortSetDispatchQueue(port, dispatch_get_global_queue(0, 0));
                let mut notification = 0;
                IOServiceAddInterestNotification(
                    port,
                    root,
                    c"IOGeneralInterest".as_ptr(),
                    on_message,
                    std::ptr::null_mut(),
                    &mut notification,
                )
            }
        };
        if status != 0 {
            tracing::warn!(status, "could not watch the lid for microphone changes");
        }
    }
}

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
    let host = cpal::default_host();
    let (devices, names) = input_devices_of(&host)?;
    let (default, default_name) = default_input_of(&host).unzip();
    let lid_closed = lid_closed();
    let resolve = |selected: Option<&str>| {
        resolve_input(
            request.device_override.as_deref(),
            selected,
            &names,
            default_name.as_deref(),
            |index| is_built_in(&devices[index]),
            lid_closed,
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
                lid_closed,
                "the system default input is Hex Microphone, off with the lid closed, or missing; using another microphone"
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
        let mut input = AudioInput::open_device(device, name, follows_default)?;
        input.selection = plan.selection;
        Ok(Opened::Input(Box::new(input)))
    };
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

/// Opens the microphone a meeting records, resolved like dictation's, or
/// keeps the open one named `keep` when it resolves to that again.
#[cfg(target_os = "macos")]
pub fn open_meeting_input(selected_device: Option<&str>, keep: Option<&str>) -> Result<Opened> {
    open_configured_input(&OpenRequest {
        device_override: None,
        selected_device: selected_device.map(str::to_owned),
        fallback: SelectedFallback::Immediate,
        keep: keep.map(str::to_owned),
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
    /// The built-in microphone, listed but off because the lid is closed.
    pub off_with_lid_closed: Option<String>,
    /// The OS default is `off_with_lid_closed`, so `system_default` stands in.
    pub default_is_off_with_lid_closed: bool,
}

pub fn input_devices() -> Result<InputDevices> {
    #[cfg(target_os = "macos")]
    watch_input_devices();
    let host = cpal::default_host();
    let (devices, listed) = input_devices_of(&host)?;
    let default = default_input_of(&host).map(|(_, name)| name);
    let default_is_hex_microphone = default
        .as_deref()
        .is_some_and(crate::hex_microphone::is_hex_virtual_device_name);
    let lid_closed = lid_closed();
    let off_with_lid_closed = (0..devices.len())
        .find(|&index| lid_closed && is_built_in(&devices[index]))
        .map(|index| listed[index].clone());
    let default_is_off_with_lid_closed =
        off_with_lid_closed.is_some() && default == off_with_lid_closed;
    let system_default = resolve_input(
        None,
        None,
        &listed,
        default.as_deref(),
        |index| is_built_in(&devices[index]),
        lid_closed,
    )
    .ok()
    .and_then(|plan| match plan.target {
        InputTarget::SystemDefault => default.clone(),
        InputTarget::Named(index) | InputTarget::StandIn(index) => Some(listed[index].clone()),
    })
    .filter(|_| cfg!(not(target_os = "linux")));
    let mut names = listed
        .into_iter()
        .filter(|name| !crate::hex_microphone::is_hex_virtual_device_name(name))
        .collect::<Vec<_>>();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    Ok(InputDevices {
        names,
        system_default,
        default_is_hex_microphone,
        off_with_lid_closed,
        default_is_off_with_lid_closed,
    })
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub fn input_device_names() -> Result<Vec<String>> {
    input_devices().map(|devices| devices.names)
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn is_built_in(_device: &Device) -> bool {
    false
}

/// Whether CoreAudio reports `device` as built in, such as a MacBook's
/// microphone.
#[cfg(target_os = "macos")]
fn is_built_in(device: &Device) -> bool {
    use std::ffi::c_void;
    use std::ptr::NonNull;

    use objc2_core_audio::{
        AudioObjectGetPropertyData, AudioObjectID, AudioObjectPropertyAddress,
        AudioObjectPropertySelector, kAudioDevicePropertyTransportType,
        kAudioDeviceTransportTypeBuiltIn, kAudioHardwarePropertyTranslateUIDToDevice,
        kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    };
    use objc2_core_foundation::CFString;

    fn read_u32(
        object: AudioObjectID,
        selector: AudioObjectPropertySelector,
        qualifier: Option<NonNull<c_void>>,
        qualifier_size: u32,
    ) -> Option<u32> {
        let mut address = AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };
        let mut value = 0_u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                NonNull::from(&mut address),
                qualifier_size,
                qualifier.map_or(std::ptr::null(), |qualifier| qualifier.as_ptr()),
                NonNull::from(&mut size),
                NonNull::from(&mut value).cast(),
            )
        };
        (status == 0 && size as usize == std::mem::size_of::<u32>()).then_some(value)
    }

    let Ok(id) = device.id() else {
        return false;
    };
    let uid = CFString::from_str(id.id());
    let uid_ref: *const CFString = &*uid;
    let Some(device_id) = read_u32(
        kAudioObjectSystemObject as AudioObjectID,
        kAudioHardwarePropertyTranslateUIDToDevice,
        Some(NonNull::from(&uid_ref).cast()),
        std::mem::size_of::<*const CFString>() as u32,
    )
    .filter(|device_id| *device_id != 0) else {
        return false;
    };
    read_u32(device_id, kAudioDevicePropertyTransportType, None, 0)
        == Some(kAudioDeviceTransportTypeBuiltIn)
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
    if matches!(
        error.kind(),
        cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied
    ) {
        // CoreAudio reports processor overloads, common while a cold device
        // starts, without stopping the stream. Real sample gaps still surface as
        // capture discontinuities.
        tracing::warn!(%error, "microphone stream glitched; continuing");
        *STREAM_GLITCHED_AT
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(Instant::now());
        return;
    }
    tracing::error!(%error, "microphone stream failed");
    let _ = sender.try_send(error.to_string());
}

fn build_stream<T: SizedSample>(
    device: &Device,
    config: &StreamConfig,
    channels: usize,
    sender: Sender<(Vec<f32>, CaptureInstant)>,
    reporter: StreamErrorReporter,
    mut feed: crate::hex_microphone::CaptureFeed,
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
            "KIT PODS",
            "MacBook Pro Microphone",
            "USB Mic",
        ]);
        let built_in = |index: usize| index == 2;
        let plan =
            |selected, default| resolve_input(None, selected, &names, default, built_in, false);
        let found = |target, selection| Ok(InputPlan { target, selection });

        // A listed saved microphone wins over the default.
        assert_eq!(
            plan(Some("USB Mic"), Some("KIT PODS")),
            found(Named(3), Selection::InUse)
        );
        // System default follows the OS default, even a Bluetooth headset.
        assert_eq!(
            plan(None, Some("KIT PODS")),
            found(SystemDefault, Selection::InUse)
        );
        // A missing saved microphone stands in with the default.
        assert_eq!(
            plan(Some("Studio Display Microphone"), Some("KIT PODS")),
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
        let no_built_in =
            resolve_input(None, None, &names, Some("Hex Microphone"), |_| false, false);
        assert_eq!(no_built_in, found(StandIn(1), Selection::InUse));
        assert_eq!(plan(None, None), found(StandIn(2), Selection::InUse));
        // A saved Hex Microphone is never captured from.
        assert_eq!(
            plan(Some("Hex Microphone"), Some("KIT PODS")),
            found(SystemDefault, Selection::InUse)
        );
        // No real inputs at all.
        assert_eq!(
            resolve_input(
                None,
                None,
                &listed(&["Hex Microphone"]),
                Some("Hex Microphone"),
                built_in,
                false
            ),
            Err("no microphone is available".into())
        );
        assert_eq!(
            resolve_input(None, Some("USB Mic"), &[], None, built_in, false),
            Err("no microphone is available".into())
        );
        assert_eq!(
            resolve_input(
                None,
                None,
                &[],
                Some("Default Audio Device"),
                built_in,
                false
            ),
            Err("no microphone is available".into())
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
        let built_in = |index: usize| index == 1;
        let closed =
            |selected, default| resolve_input(None, selected, &names, default, built_in, true);
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
                built_in,
                false
            ),
            found(SystemDefault, Selection::InUse)
        );
        // With nothing else to use, say why rather than record silence.
        let only_built_in = listed(&["Hex Microphone", "MacBook Pro Microphone"]);
        for default in [Some("MacBook Pro Microphone"), Some("Hex Microphone"), None] {
            assert_eq!(
                resolve_input(None, None, &only_built_in, default, built_in, true),
                Err(LID_CLOSED_ERROR.into())
            );
        }
        assert_eq!(
            resolve_input(
                None,
                Some("MacBook Pro Microphone"),
                &only_built_in,
                Some("MacBook Pro Microphone"),
                built_in,
                true
            ),
            Err(LID_CLOSED_ERROR.into())
        );
    }

    #[test]
    fn device_override_wins_and_never_falls_back() {
        let names = listed(&["Hex Microphone", "KIT PODS", "USB Mic"]);
        let plan = |device_override| {
            resolve_input(
                device_override,
                Some("KIT PODS"),
                &names,
                Some("KIT PODS"),
                |_| false,
                false,
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
        let default = Arc::new(Mutex::new("KIT PODS".to_owned()));
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
        input.finish_initial_open(Ok(named_input("KIT PODS")), Instant::now());

        // The OS default changes during a recording: nothing opens yet.
        *default.lock().unwrap() = "MacBook Pro Microphone".into();
        TEST_ROUTE_GENERATION.fetch_add(1, Ordering::AcqRel);
        assert!(matches!(
            settle(&mut input, false),
            RecoveringAudioInputEvent::Timeout
        ));
        assert!(!input.is_opening());
        assert_eq!(input.device_name(), "KIT PODS");

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
    /// until then stands in with "KIT PODS" for the given reason.
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
                    ("KIT PODS", missing)
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
        let mut stand_in = named_input("KIT PODS");
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
        assert_eq!(input.device_name(), "KIT PODS");
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
}

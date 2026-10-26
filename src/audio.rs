use std::ops::Add;
use std::sync::Mutex;
#[cfg(target_os = "macos")]
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError};
use std::time::{Duration, Instant};

static AUDIO_SERVICE_RESTART_GENERATION: AtomicU64 = AtomicU64::new(0);
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

#[cfg(target_os = "macos")]
fn ensure_coreaudio_restart_listener() {
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

    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        const SYSTEM_OBJECT: u32 = 1;
        const SERVICE_RESTARTED_SELECTOR: u32 = u32::from_be_bytes(*b"srst");
        const GLOBAL_SCOPE: u32 = u32::from_be_bytes(*b"glob");
        let addr = AudioObjectPropertyAddress {
            m_selector: SERVICE_RESTARTED_SELECTOR,
            m_scope: GLOBAL_SCOPE,
            m_element: 0,
        };
        let _ = unsafe {
            AudioObjectAddPropertyListener(
                SYSTEM_OBJECT,
                &addr,
                on_service_restarted,
                std::ptr::null_mut(),
            )
        };
    });
}

use color_eyre::eyre::{Result, WrapErr, eyre};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, SizedSample, Stream, StreamConfig};

#[cfg(target_os = "macos")]
const AUTOMATIC_INPUT_DEVICE_PREFERENCES: &[&str] = crate::config::INPUT_DEVICES;
#[cfg(any(target_os = "linux", target_os = "windows"))]
const AUTOMATIC_INPUT_DEVICE_PREFERENCES: &[&str] = &[];

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

type InputOpenResult = (u64, Result<AudioInput, String>);

pub struct RecoveringAudioInput {
    input: Option<AudioInput>,
    device_override: Option<String>,
    selected_device: Option<String>,
    active_revision: u64,
    recovery: MicrophoneRecovery,
    replacement: Option<Receiver<InputOpenResult>>,
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

/// How an open treats a selected microphone that cannot be opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectedFallback {
    /// Warm recovery keeps retrying the selection with backoff.
    Never,
    /// Startup falls back to automatic selection immediately.
    Immediate,
    /// An on-demand cold open retries the selection, then falls back.
    AfterRetries,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum MicrophoneRecoveryReason {
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

    pub fn open(device_queries: &[&str]) -> Result<Self> {
        let host = cpal::default_host();
        let device = find_device(&host, device_queries)?;
        Self::open_device(device)
    }

    pub fn open_named(name: &str) -> Result<Self> {
        if crate::hex_microphone::is_hex_virtual_device_name(name) {
            return Err(eyre!(
                "Hex cannot capture from its own virtual microphone: {name}"
            ));
        }
        let host = cpal::default_host();
        let device = host
            .input_devices()
            .wrap_err("could not enumerate input devices")?
            .find(|device| device.to_string() == name)
            .ok_or_else(|| eyre!("microphone is unavailable: {name}"))?;
        Self::open_device(device)
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    pub fn open_matching(query: &str) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .input_devices()
            .wrap_err("could not enumerate input devices")?
            .find(|device| device_matches(device, query))
            .ok_or_else(|| eyre!("microphone is unavailable: {query}"))?;
        Self::open_device(device)
    }

    fn open_device(device: Device) -> Result<Self> {
        #[cfg(target_os = "macos")]
        ensure_coreaudio_restart_listener();
        let restart_generation = audio_service_restart_generation();
        let device_name = device.to_string();
        let supported = device
            .default_input_config()
            .wrap_err("could not read the input device configuration")?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let sample_rate = config.sample_rate;
        let channels = usize::from(config.channels);
        let (sender, chunks) = mpsc::channel();
        let (error_sender, stream_errors) = mpsc::sync_channel(1);
        let (feed, hex_microphone) = crate::hex_microphone::capture_feed(sample_rate);

        let stream = match sample_format {
            SampleFormat::F32 => build_stream(
                &device,
                &config,
                channels,
                sender,
                error_sender,
                feed,
                |sample: f32| sample,
            )?,
            SampleFormat::I16 => build_stream(
                &device,
                &config,
                channels,
                sender,
                error_sender,
                feed,
                |sample: i16| sample as f32 / i16::MAX as f32,
            )?,
            SampleFormat::U16 => build_stream(
                &device,
                &config,
                channels,
                sender,
                error_sender,
                feed,
                |sample: u16| sample as f32 / 32768.0 - 1.0,
            )?,
            format => return Err(eyre!("unsupported microphone sample format: {format:?}")),
        };
        stream
            .play()
            .wrap_err("could not start microphone capture")?;
        tracing::info!(
            device = %device_name,
            sample_rate,
            channels,
            "opened dictation microphone"
        );

        Ok(Self {
            _stream: Some(stream),
            chunks,
            sample_rate,
            device_name,
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
        let opened = open_configured_input(
            device_override,
            selected_device,
            SelectedFallback::Immediate,
        );
        input.finish_initial_open(opened, Instant::now());
        input
    }

    fn finish_initial_open(&mut self, opened: Result<AudioInput>, now: Instant) {
        match opened {
            Ok(opened) => self.install(opened),
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
        }
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
            Some("HEX nonexistent microphone for pending open test"),
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
                Ok(replacement) if capture_idle || self.input.is_none() => {
                    self.install(replacement);
                    self.active_revision = opened_revision;
                    self.recovery.recovered();
                    tracing::info!(device = %self.device_name(), "dictation microphone reopened");
                    return RecoveringAudioInputEvent::Reopened;
                }
                Ok(_) => {
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
        let device_override = self.device_override.clone();
        let selected_device = self.selected_device.clone();
        let fallback = if self.input.is_none() {
            SelectedFallback::AfterRetries
        } else {
            SelectedFallback::Never
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = open_configured_input(
                device_override.as_deref(),
                selected_device.as_deref(),
                fallback,
            )
            .map_err(|error| error.to_string());
            let _ = sender.send((revision, result));
        });
        self.replacement = Some(receiver);
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

fn open_configured_input(
    device_override: Option<&str>,
    selected_device: Option<&str>,
    fallback: SelectedFallback,
) -> Result<AudioInput> {
    if let Some(device) = device_override {
        return AudioInput::open_named(device);
    }
    let Some(device) = selected_device else {
        return AudioInput::open(AUTOMATIC_INPUT_DEVICE_PREFERENCES);
    };
    open_selected(
        device,
        fallback,
        || AudioInput::open_named(device),
        || AudioInput::open(AUTOMATIC_INPUT_DEVICE_PREFERENCES),
        std::thread::sleep,
    )
}

/// Opens the microphone a meeting records: the saved selection while it is
/// available, otherwise the automatic preference order.
#[cfg(target_os = "macos")]
pub fn open_meeting_input(selected_device: Option<&str>) -> Result<AudioInput> {
    open_configured_input(None, selected_device, SelectedFallback::Immediate)
}

fn open_selected<T>(
    device: &str,
    fallback: SelectedFallback,
    mut open_selected: impl FnMut() -> Result<T>,
    open_automatic: impl FnOnce() -> Result<T>,
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
                tracing::warn!(%error, device, attempt, "could not open the selected microphone; retrying");
                attempt += 1;
                sleep(SELECTED_MICROPHONE_RETRY_DELAY);
            }
        }
    };
    if fallback == SelectedFallback::Never {
        return Err(error);
    }
    tracing::warn!(%error, device, "selected microphone is unavailable; using automatic selection");
    open_automatic()
}

pub fn input_device_names() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let mut names = host
        .input_devices()
        .wrap_err("could not enumerate input devices")?
        .map(|device| device.to_string())
        .filter(|name| !crate::hex_microphone::is_hex_virtual_device_name(name))
        .collect::<Vec<_>>();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    Ok(names)
}

fn device_matches(device: &Device, query: &str) -> bool {
    device
        .to_string()
        .to_lowercase()
        .contains(&query.to_lowercase())
}

fn find_device(host: &cpal::Host, queries: &[&str]) -> Result<Device> {
    let devices: Vec<_> = host
        .input_devices()
        .wrap_err("could not enumerate input devices")?
        .filter(|device| !crate::hex_microphone::is_hex_virtual_device_name(&device.to_string()))
        .collect();
    for query in queries {
        if let Some(device) = devices.iter().find(|device| device_matches(device, query)) {
            return Ok(device.clone());
        }
    }
    let default = host
        .default_input_device()
        .filter(|device| !crate::hex_microphone::is_hex_virtual_device_name(&device.to_string()))
        .or_else(|| devices.first().cloned())
        .ok_or_else(|| {
            eyre!(
                "no preferred or default input device is available (preferred: {})",
                queries.join(", ")
            )
        })?;
    Ok(default)
}

fn mono<T>(samples: &[T], channels: usize, convert: impl Fn(&T) -> f32) -> Vec<f32> {
    samples
        .chunks(channels)
        .map(|frame| frame.iter().map(&convert).sum::<f32>() / channels as f32)
        .collect()
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
    error_sender: SyncSender<String>,
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
            move |error| report_stream_error(&error_sender, error),
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
        assert_eq!(
            open(SelectedFallback::Never, 1),
            (Err("device is waking".into()), 1, 0)
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
        sender.send((3, Ok(replacement))).unwrap();
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
        sender.send((3, Ok(opened))).unwrap();

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
            Some("HEX nonexistent microphone for pending recovery test"),
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
        sender.send((1, Ok(reopened))).unwrap();

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

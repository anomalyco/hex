use std::ops::Add;
#[cfg(target_os = "macos")]
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};

#[cfg(target_os = "macos")]
const AUTOMATIC_INPUT_DEVICE_PREFERENCES: &[&str] = crate::config::INPUT_DEVICES;
#[cfg(target_os = "linux")]
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

    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        Duration::from_nanos(self.0.saturating_sub(earlier.0))
    }

    pub fn duration_since(self, earlier: Self) -> Duration {
        self.saturating_duration_since(earlier)
    }
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
    _stream: Stream,
    chunks: Receiver<(Vec<f32>, CaptureInstant)>,
    pub sample_rate: u32,
    pub device_name: String,
    stream_errors: Receiver<String>,
}

pub enum AudioInputEvent {
    Chunk {
        samples: Vec<f32>,
        captured_through: CaptureInstant,
    },
    Timeout,
    StreamFailed(String),
}

pub struct RecoveringAudioInput {
    input: AudioInput,
    device_override: Option<String>,
    selected_device: Option<String>,
    active_revision: u64,
    recovery: MicrophoneRecovery,
    replacement: Option<Receiver<(u64, Result<AudioInput, String>)>>,
}

pub enum RecoveringAudioInputEvent {
    Chunk {
        samples: Vec<f32>,
        captured_through: CaptureInstant,
    },
    Timeout,
    Interrupted,
    Reopened,
}

const MICROPHONE_RETRY_INITIAL: Duration = Duration::from_millis(250);
const MICROPHONE_RETRY_MAX: Duration = Duration::from_secs(5);

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
    pub fn open(device_queries: &[&str]) -> Result<Self> {
        let host = cpal::default_host();
        let device = find_device(&host, device_queries)?;
        Self::open_device(device)
    }

    pub fn open_named(name: &str) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .input_devices()
            .wrap_err("could not enumerate input devices")?
            .find(|device| device.to_string() == name)
            .ok_or_else(|| eyre!("microphone is unavailable: {name}"))?;
        Self::open_device(device)
    }

    #[cfg(target_os = "linux")]
    pub fn open_matching(query: &str) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .input_devices()
            .wrap_err("could not enumerate input devices")?
            .find(|device| {
                device
                    .to_string()
                    .to_lowercase()
                    .contains(&query.to_lowercase())
            })
            .ok_or_else(|| eyre!("microphone is unavailable: {query}"))?;
        Self::open_device(device)
    }

    fn open_device(device: Device) -> Result<Self> {
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

        let stream = match sample_format {
            SampleFormat::F32 => {
                build_f32_stream(&device, &config, channels, sender, error_sender)?
            }
            SampleFormat::I16 => {
                build_i16_stream(&device, &config, channels, sender, error_sender)?
            }
            SampleFormat::U16 => {
                build_u16_stream(&device, &config, channels, sender, error_sender)?
            }
            format => return Err(eyre!("unsupported microphone sample format: {format:?}")),
        };
        stream
            .play()
            .wrap_err("could not start microphone capture")?;

        Ok(Self {
            _stream: stream,
            chunks,
            sample_rate,
            device_name,
            stream_errors,
        })
    }

    pub fn recv_timeout(&self, timeout: Duration) -> AudioInputEvent {
        if let Ok(error) = self.stream_errors.try_recv() {
            return AudioInputEvent::StreamFailed(error);
        }
        match self.chunks.recv_timeout(timeout) {
            Ok((samples, captured_through)) => match self.stream_errors.try_recv() {
                Ok(error) => AudioInputEvent::StreamFailed(error),
                Err(_) => AudioInputEvent::Chunk {
                    samples,
                    captured_through,
                },
            },
            Err(RecvTimeoutError::Timeout) => match self.stream_errors.try_recv() {
                Ok(error) => AudioInputEvent::StreamFailed(error),
                Err(_) => AudioInputEvent::Timeout,
            },
            Err(RecvTimeoutError::Disconnected) => AudioInputEvent::StreamFailed(
                self.stream_errors
                    .try_recv()
                    .unwrap_or_else(|_| "microphone audio channel disconnected".into()),
            ),
        }
    }
}

impl RecoveringAudioInput {
    pub fn open(
        device_override: Option<&str>,
        selection_revision: u64,
        selected_device: Option<&str>,
    ) -> Result<Self> {
        let input = if let Some(device) = device_override {
            AudioInput::open_named(device)?
        } else if let Some(device) = selected_device {
            match AudioInput::open_named(device) {
                Ok(input) => input,
                Err(error) => {
                    tracing::warn!(%error, device, "selected microphone is unavailable; using automatic selection");
                    AudioInput::open(AUTOMATIC_INPUT_DEVICE_PREFERENCES)?
                }
            }
        } else {
            AudioInput::open(AUTOMATIC_INPUT_DEVICE_PREFERENCES)?
        };
        Ok(Self {
            input,
            device_override: device_override.map(str::to_owned),
            selected_device: selected_device.map(str::to_owned),
            active_revision: selection_revision,
            recovery: MicrophoneRecovery::default(),
            replacement: None,
        })
    }

    pub fn request_selection(&mut self, revision: u64, selected_device: Option<&str>) {
        if self.device_override.is_some() || revision == self.active_revision {
            return;
        }
        self.selected_device = selected_device.map(str::to_owned);
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
                Ok(replacement) if capture_idle => {
                    self.input = replacement;
                    self.active_revision = opened_revision;
                    self.recovery.recovered();
                    tracing::info!(device = %self.input.device_name, "dictation microphone reopened");
                    return RecoveringAudioInputEvent::Reopened;
                }
                Ok(_) => {
                    self.recovery.failed(now);
                }
                Err(error) => {
                    self.recovery.failed(now);
                    tracing::warn!(
                        %error,
                        device = %self.input.device_name,
                        "could not reopen dictation microphone; retrying"
                    );
                }
            }
        }
        if capture_idle && self.recovery.should_attempt(now) && self.replacement.is_none() {
            self.start_replacement();
        }
        if self.recovery.blocks_audio() {
            std::thread::sleep(timeout);
            return RecoveringAudioInputEvent::Timeout;
        }
        match self.input.recv_timeout(timeout) {
            AudioInputEvent::Chunk {
                samples,
                captured_through,
            } => RecoveringAudioInputEvent::Chunk {
                samples,
                captured_through,
            },
            AudioInputEvent::Timeout => RecoveringAudioInputEvent::Timeout,
            AudioInputEvent::StreamFailed(error) => {
                tracing::warn!(%error, device = %self.input.device_name, "microphone stream stopped; reopening it");
                self.recovery.request_stream_recovery(Instant::now());
                RecoveringAudioInputEvent::Interrupted
            }
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.input.sample_rate
    }

    pub fn device_name(&self) -> &str {
        &self.input.device_name
    }

    fn start_replacement(&mut self) {
        let revision = self
            .recovery
            .target_revision
            .unwrap_or(self.active_revision);
        let device = self
            .device_override
            .clone()
            .or_else(|| self.selected_device.clone());
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = match device {
                Some(device) => AudioInput::open_named(&device),
                None => AudioInput::open(AUTOMATIC_INPUT_DEVICE_PREFERENCES),
            }
            .map_err(|error| error.to_string());
            let _ = sender.send((revision, result));
        });
        self.replacement = Some(receiver);
    }

    fn poll_replacement(&mut self) -> Option<(u64, Result<AudioInput, String>)> {
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

pub fn input_device_names() -> Result<Vec<String>> {
    let host = cpal::default_host();
    let mut names = host
        .input_devices()
        .wrap_err("could not enumerate input devices")?
        .map(|device| device.to_string())
        .collect::<Vec<_>>();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    Ok(names)
}

fn find_device(host: &cpal::Host, queries: &[&str]) -> Result<Device> {
    let devices: Vec<_> = host
        .input_devices()
        .wrap_err("could not enumerate input devices")?
        .collect();
    for query in queries {
        if let Some(device) = devices.iter().find(|device| {
            device
                .to_string()
                .to_lowercase()
                .contains(&query.to_lowercase())
        }) {
            return Ok(device.clone());
        }
    }
    host.default_input_device().ok_or_else(|| {
        eyre!(
            "no preferred or default input device is available (preferred: {})",
            queries.join(", ")
        )
    })
}

fn mono<T>(samples: &[T], channels: usize, convert: impl Fn(&T) -> f32) -> Vec<f32> {
    samples
        .chunks(channels)
        .map(|frame| frame.iter().map(&convert).sum::<f32>() / channels as f32)
        .collect()
}

fn send(
    sender: &Sender<(Vec<f32>, CaptureInstant)>,
    chunk: Vec<f32>,
    captured_through: CaptureInstant,
) {
    let _ = sender.send((chunk, captured_through));
}

fn stream_error(sender: &SyncSender<String>, error: cpal::Error) {
    report_stream_error(sender, error.to_string());
}

fn report_stream_error(sender: &SyncSender<String>, error: String) {
    tracing::error!(%error, "microphone stream failed");
    let _ = sender.try_send(error);
}

fn build_f32_stream(
    device: &Device,
    config: &StreamConfig,
    channels: usize,
    sender: Sender<(Vec<f32>, CaptureInstant)>,
    error_sender: SyncSender<String>,
) -> Result<Stream> {
    let sample_rate = config.sample_rate;
    device
        .build_input_stream(
            *config,
            move |data: &[f32], info| {
                let captured_through = captured_through(info, data.len() / channels, sample_rate);
                send(
                    &sender,
                    mono(data, channels, |sample| *sample),
                    captured_through,
                )
            },
            move |error| stream_error(&error_sender, error),
            None,
        )
        .wrap_err("could not open f32 microphone stream")
}

fn build_i16_stream(
    device: &Device,
    config: &StreamConfig,
    channels: usize,
    sender: Sender<(Vec<f32>, CaptureInstant)>,
    error_sender: SyncSender<String>,
) -> Result<Stream> {
    let sample_rate = config.sample_rate;
    device
        .build_input_stream(
            *config,
            move |data: &[i16], info| {
                let captured_through = captured_through(info, data.len() / channels, sample_rate);
                send(
                    &sender,
                    mono(data, channels, |sample| *sample as f32 / i16::MAX as f32),
                    captured_through,
                )
            },
            move |error| stream_error(&error_sender, error),
            None,
        )
        .wrap_err("could not open i16 microphone stream")
}

fn build_u16_stream(
    device: &Device,
    config: &StreamConfig,
    channels: usize,
    sender: Sender<(Vec<f32>, CaptureInstant)>,
    error_sender: SyncSender<String>,
) -> Result<Stream> {
    let sample_rate = config.sample_rate;
    device
        .build_input_stream(
            *config,
            move |data: &[u16], info| {
                let captured_through = captured_through(info, data.len() / channels, sample_rate);
                send(
                    &sender,
                    mono(data, channels, |sample| *sample as f32 / 32768.0 - 1.0),
                    captured_through,
                )
            },
            move |error| stream_error(&error_sender, error),
            None,
        )
        .wrap_err("could not open u16 microphone stream")
}

fn captured_through(
    info: &cpal::InputCallbackInfo,
    frames: usize,
    sample_rate: u32,
) -> CaptureInstant {
    let captured_at = CaptureInstant::from_stream(info.timestamp().capture);
    let nanos = (frames as u128 * 1_000_000_000 / u128::from(sample_rate)) as u64;
    let duration = Duration::from_nanos(nanos);
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

        report_stream_error(&sender, "Device sample rate changed".into());

        assert_eq!(errors.try_recv().unwrap(), "Device sample rate changed");
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
}

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};

pub struct AudioInput {
    _stream: Stream,
    chunks: Receiver<Vec<f32>>,
    pub sample_rate: u32,
    pub device_name: String,
    dropped_chunks: Arc<AtomicU64>,
    stream_errors: Receiver<String>,
}

pub enum AudioInputEvent {
    Chunk(Vec<f32>),
    Timeout,
    StreamFailed(String),
}

pub struct RecoveringAudioInput {
    input: AudioInput,
    device_override: Option<String>,
    selected_device: Option<String>,
    active_revision: u64,
    recovery: MicrophoneRecovery,
    dropped_chunks: u64,
    last_drop_report: Instant,
}

pub enum RecoveringAudioInputEvent {
    Chunk(Vec<f32>),
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
        let (sender, chunks) = mpsc::sync_channel(32);
        let (error_sender, stream_errors) = mpsc::sync_channel(1);
        let dropped_chunks = Arc::new(AtomicU64::new(0));

        let stream = match sample_format {
            SampleFormat::F32 => build_f32_stream(
                &device,
                &config,
                channels,
                sender,
                dropped_chunks.clone(),
                error_sender,
            )?,
            SampleFormat::I16 => build_i16_stream(
                &device,
                &config,
                channels,
                sender,
                dropped_chunks.clone(),
                error_sender,
            )?,
            SampleFormat::U16 => build_u16_stream(
                &device,
                &config,
                channels,
                sender,
                dropped_chunks.clone(),
                error_sender,
            )?,
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
            dropped_chunks,
            stream_errors,
        })
    }

    pub fn take_dropped_chunks(&self) -> u64 {
        self.dropped_chunks.swap(0, Ordering::Relaxed)
    }

    pub fn recv_timeout(&self, timeout: Duration) -> AudioInputEvent {
        if let Ok(error) = self.stream_errors.try_recv() {
            return AudioInputEvent::StreamFailed(error);
        }
        match self.chunks.recv_timeout(timeout) {
            Ok(chunk) => match self.stream_errors.try_recv() {
                Ok(error) => AudioInputEvent::StreamFailed(error),
                Err(_) => AudioInputEvent::Chunk(chunk),
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
                    AudioInput::open(crate::config::INPUT_DEVICES)?
                }
            }
        } else {
            AudioInput::open(crate::config::INPUT_DEVICES)?
        };
        Ok(Self {
            input,
            device_override: device_override.map(str::to_owned),
            selected_device: selected_device.map(str::to_owned),
            active_revision: selection_revision,
            recovery: MicrophoneRecovery::default(),
            dropped_chunks: 0,
            last_drop_report: Instant::now(),
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
        self.report_dropped_chunks();
        let now = Instant::now();
        if capture_idle && self.recovery.should_attempt(now) {
            match self.open_replacement() {
                Ok(replacement) => {
                    self.input = replacement;
                    if let Some(revision) = self.recovery.target_revision {
                        self.active_revision = revision;
                    }
                    self.recovery.recovered();
                    tracing::info!(device = %self.input.device_name, "dictation microphone reopened");
                    return RecoveringAudioInputEvent::Reopened;
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
        if self.recovery.blocks_audio() {
            std::thread::sleep(timeout);
            return RecoveringAudioInputEvent::Timeout;
        }
        match self.input.recv_timeout(timeout) {
            AudioInputEvent::Chunk(samples) => RecoveringAudioInputEvent::Chunk(samples),
            AudioInputEvent::Timeout => RecoveringAudioInputEvent::Timeout,
            AudioInputEvent::StreamFailed(error) => {
                tracing::warn!(%error, device = %self.input.device_name, "microphone stream stopped; reopening it");
                self.recovery.request_stream_recovery(Instant::now());
                RecoveringAudioInputEvent::Interrupted
            }
        }
    }

    pub fn is_recovering(&self) -> bool {
        self.recovery.blocks_audio()
    }

    pub fn sample_rate(&self) -> u32 {
        self.input.sample_rate
    }

    pub fn device_name(&self) -> &str {
        &self.input.device_name
    }

    fn open_replacement(&self) -> Result<AudioInput> {
        match self
            .device_override
            .as_deref()
            .or(self.selected_device.as_deref())
        {
            Some(device) => AudioInput::open_named(device),
            None => AudioInput::open(crate::config::INPUT_DEVICES),
        }
    }

    fn report_dropped_chunks(&mut self) {
        self.dropped_chunks += self.input.take_dropped_chunks();
        if self.last_drop_report.elapsed() < Duration::from_secs(1) {
            return;
        }
        if self.dropped_chunks > 0 {
            tracing::warn!(
                dropped_audio_chunks = self.dropped_chunks,
                "microphone chunks were dropped because audio consumption fell behind"
            );
            self.dropped_chunks = 0;
        }
        self.last_drop_report = Instant::now();
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

fn send(sender: &SyncSender<Vec<f32>>, dropped_chunks: &AtomicU64, chunk: Vec<f32>) {
    if matches!(sender.try_send(chunk), Err(TrySendError::Full(_))) {
        dropped_chunks.fetch_add(1, Ordering::Relaxed);
    }
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
    sender: SyncSender<Vec<f32>>,
    dropped_chunks: Arc<AtomicU64>,
    error_sender: SyncSender<String>,
) -> Result<Stream> {
    device
        .build_input_stream(
            *config,
            move |data: &[f32], _| {
                send(
                    &sender,
                    &dropped_chunks,
                    mono(data, channels, |sample| *sample),
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
    sender: SyncSender<Vec<f32>>,
    dropped_chunks: Arc<AtomicU64>,
    error_sender: SyncSender<String>,
) -> Result<Stream> {
    device
        .build_input_stream(
            *config,
            move |data: &[i16], _| {
                send(
                    &sender,
                    &dropped_chunks,
                    mono(data, channels, |sample| *sample as f32 / i16::MAX as f32),
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
    sender: SyncSender<Vec<f32>>,
    dropped_chunks: Arc<AtomicU64>,
    error_sender: SyncSender<String>,
) -> Result<Stream> {
    device
        .build_input_stream(
            *config,
            move |data: &[u16], _| {
                send(
                    &sender,
                    &dropped_chunks,
                    mono(data, channels, |sample| *sample as f32 / 32768.0 - 1.0),
                )
            },
            move |error| stream_error(&error_sender, error),
            None,
        )
        .wrap_err("could not open u16 microphone stream")
}

#[cfg(test)]
mod tests {
    use super::*;

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

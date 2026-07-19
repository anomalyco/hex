use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};

use color_eyre::eyre::{Result, WrapErr, eyre};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};

pub struct AudioInput {
    _stream: Stream,
    pub chunks: Receiver<Vec<f32>>,
    pub sample_rate: u32,
    pub device_name: String,
    dropped_chunks: Arc<AtomicU64>,
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
        let dropped_chunks = Arc::new(AtomicU64::new(0));

        let stream = match sample_format {
            SampleFormat::F32 => {
                build_f32_stream(&device, &config, channels, sender, dropped_chunks.clone())?
            }
            SampleFormat::I16 => {
                build_i16_stream(&device, &config, channels, sender, dropped_chunks.clone())?
            }
            SampleFormat::U16 => {
                build_u16_stream(&device, &config, channels, sender, dropped_chunks.clone())?
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
            dropped_chunks,
        })
    }

    pub fn take_dropped_chunks(&self) -> u64 {
        self.dropped_chunks.swap(0, Ordering::Relaxed)
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

fn stream_error(error: cpal::Error) {
    tracing::error!(%error, "microphone stream failed");
}

fn build_f32_stream(
    device: &Device,
    config: &StreamConfig,
    channels: usize,
    sender: SyncSender<Vec<f32>>,
    dropped_chunks: Arc<AtomicU64>,
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
            stream_error,
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
            stream_error,
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
            stream_error,
            None,
        )
        .wrap_err("could not open u16 microphone stream")
}

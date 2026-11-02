//! Which format a microphone stream opens in: the device's default, else,
//! when the driver refuses it, the other formats the device lists.

use std::sync::mpsc::{self, Receiver};

use color_eyre::eyre::{Result, WrapErr, eyre};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{
    BufferSize, Device, SampleFormat, Stream, StreamConfig, SupportedBufferSize,
    SupportedStreamConfig,
};

use super::{CaptureInstant, StreamErrorReporter, build_stream};

/// A started microphone stream and the channels its callback feeds.
pub(super) struct StartedStream {
    pub stream: Stream,
    pub chunks: Receiver<(Vec<f32>, CaptureInstant)>,
    pub stream_errors: Receiver<String>,
    pub hex_microphone: crate::audio::hex_microphone::CaptureFeedSwitch,
    pub sample_rate: u32,
}

/// The most formats tried after a device refuses its default one.
const MAX_ALTERNATIVE_INPUT_CONFIGS: usize = 12;

/// Starts a stream in the device's default format. Some drivers (Intel
/// Smart Sound on Windows, #988) refuse a stream in the format they report
/// as their default, so the other formats they list are tried before
/// failing with the default's error.
pub(super) fn start_stream_with_fallback(
    device: &Device,
    device_name: &str,
    default_name: Option<String>,
) -> Result<StartedStream> {
    let supported = device
        .default_input_config()
        .wrap_err("could not read the input device configuration")?;
    let error = match start_stream(
        device,
        &supported,
        BufferSize::Default,
        default_name.clone(),
    ) {
        Ok(started) => return Ok(started),
        Err(error) => error,
    };
    tracing::warn!(
        error = format!("{error:#}"),
        device = %device_name,
        "the microphone refused its default format; trying the others it lists"
    );
    // Some drivers refuse every format with the backend's default buffer
    // (Intel Smart Sound answered E_INVALIDARG for each, #988) yet accept
    // the same format with an explicit buffer duration, so each format is
    // tried both ways.
    let mut candidates = vec![(supported.clone(), fixed_buffer_size(&supported))];
    for candidate in alternative_input_configs(device, &supported) {
        candidates.push((candidate.clone(), BufferSize::Default));
        let fixed = fixed_buffer_size(&candidate);
        candidates.push((candidate, fixed));
    }
    for (candidate, buffer_size) in candidates {
        match start_stream(device, &candidate, buffer_size, default_name.clone()) {
            Ok(started) => {
                tracing::info!(
                    device = %device_name,
                    format = %describe_config(&candidate),
                    buffer = ?buffer_size,
                    "opened the microphone in another format"
                );
                return Ok(started);
            }
            Err(error) => tracing::warn!(
                error = format!("{error:#}"),
                device = %device_name,
                buffer = ?buffer_size,
                "the microphone refused another format"
            ),
        }
    }
    Err(error)
}

/// An explicit buffer of about 20 ms at `config`'s rate, within the range
/// the device reports.
fn fixed_buffer_size(config: &SupportedStreamConfig) -> BufferSize {
    let frames = (config.sample_rate() / 50).max(1);
    let frames = match *config.buffer_size() {
        SupportedBufferSize::Range { min, max } if min <= max => frames.clamp(min, max),
        _ => frames,
    };
    BufferSize::Fixed(frames)
}

fn is_supported_sample_format(format: SampleFormat) -> bool {
    matches!(
        format,
        SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16 | SampleFormat::I32
    )
}

/// Channels, rate, and sample format, for logs and errors.
fn describe_config(config: &SupportedStreamConfig) -> String {
    format!(
        "{} channels, {} Hz, {:?}",
        config.channels(),
        config.sample_rate(),
        config.sample_format()
    )
}

/// The other formats `device` lists, at the default rate where they allow
/// it, without `default` and duplicates.
fn alternative_input_configs(
    device: &Device,
    default: &SupportedStreamConfig,
) -> Vec<SupportedStreamConfig> {
    let ranges = match device.supported_input_configs() {
        Ok(ranges) => ranges,
        Err(error) => {
            tracing::warn!(%error, "could not list the microphone's formats");
            return Vec::new();
        }
    };
    let mut candidates: Vec<SupportedStreamConfig> = Vec::new();
    for range in ranges {
        if !is_supported_sample_format(range.sample_format()) {
            continue;
        }
        let (min, max) = (range.min_sample_rate(), range.max_sample_rate());
        if min > max {
            continue;
        }
        let rate = default.sample_rate().max(min).min(max);
        let candidate = range.with_sample_rate(rate);
        let same = |other: &SupportedStreamConfig| {
            other.channels() == candidate.channels()
                && other.sample_rate() == candidate.sample_rate()
                && other.sample_format() == candidate.sample_format()
        };
        if same(default) || candidates.iter().any(same) {
            continue;
        }
        candidates.push(candidate);
        if candidates.len() >= MAX_ALTERNATIVE_INPUT_CONFIGS {
            break;
        }
    }
    candidates
}

/// Opens and starts a stream on `device` in `supported`'s format. The error
/// names the format the device refused.
fn start_stream(
    device: &Device,
    supported: &SupportedStreamConfig,
    buffer_size: BufferSize,
    default_name: Option<String>,
) -> Result<StartedStream> {
    let sample_format = supported.sample_format();
    let mut config: StreamConfig = supported.clone().into();
    config.buffer_size = buffer_size;
    let sample_rate = config.sample_rate;
    let channels = usize::from(config.channels);
    let (sender, chunks) = mpsc::channel();
    let (error_sender, stream_errors) = mpsc::sync_channel(1);
    let reporter = StreamErrorReporter {
        sender: error_sender,
        default_name,
    };
    let (feed, hex_microphone) = crate::audio::hex_microphone::capture_feed(sample_rate);
    let stream = match sample_format {
        SampleFormat::F32 => build_stream(
            device,
            &config,
            channels,
            sender,
            reporter,
            feed,
            |sample: f32| sample,
        ),
        SampleFormat::I16 => build_stream(
            device,
            &config,
            channels,
            sender,
            reporter,
            feed,
            |sample: i16| sample as f32 / i16::MAX as f32,
        ),
        SampleFormat::U16 => build_stream(
            device,
            &config,
            channels,
            sender,
            reporter,
            feed,
            |sample: u16| sample as f32 / 32768.0 - 1.0,
        ),
        SampleFormat::I32 => build_stream(
            device,
            &config,
            channels,
            sender,
            reporter,
            feed,
            |sample: i32| sample as f32 / i32::MAX as f32,
        ),
        format => Err(eyre!("unsupported microphone sample format: {format:?}")),
    }
    .and_then(|stream| {
        stream
            .play()
            .wrap_err("could not start microphone capture")?;
        Ok(stream)
    })
    // One message, so a log line or status that shows only the outer error
    // still names the refused format and the driver's reason.
    .map_err(|error| {
        eyre!(
            "could not open the microphone ({}): {error:#}",
            describe_config(supported)
        )
    })?;
    Ok(StartedStream {
        stream,
        chunks,
        stream_errors,
        hex_microphone,
        sample_rate,
    })
}

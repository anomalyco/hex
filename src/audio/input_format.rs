//! Which format a microphone stream opens in: the device's default, else,
//! when the driver refuses it, the other formats the device lists. A
//! silence-triggered open makes the device renegotiate its sample rate first.

use std::sync::mpsc::{self, Receiver};

use color_eyre::eyre::{Result, WrapErr, eyre};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{
    BufferSize, Device, SampleFormat, Stream, StreamConfig, SupportedBufferSize,
    SupportedStreamConfig,
};

use super::{CaptureInstant, StreamErrorReporter, build_stream};

/// A running microphone stream; dropping it stops capture.
pub(super) enum InputStream {
    Cpal {
        _stream: Stream,
    },
    #[cfg(target_os = "windows")]
    Wasapi {
        _capture: super::wasapi_capture::WasapiCapture,
    },
}

/// A started microphone stream and the channels its callback feeds.
pub(super) struct StartedStream {
    pub stream: InputStream,
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
///
/// `renegotiate_rate` marks a silence recovery's open: the device's clock can
/// be stuck, so its rate changes before the real stream opens (#1575).
pub(super) fn start_stream_with_fallback(
    device: &Device,
    device_name: &str,
    default_name: Option<String>,
    renegotiate_rate: bool,
) -> Result<StartedStream> {
    let supported = device
        .default_input_config()
        .wrap_err("could not read the input device configuration")?;
    // A device a call app let go can stay on a clock that delivers only zeros
    // until its rate changes. A throwaway stream at another rate makes the
    // driver renegotiate; the real stream opens in the format remembered
    // before it, which sets the device back.
    #[cfg(target_os = "macos")]
    if renegotiate_rate {
        renegotiate_sample_rate(device, device_name, &supported);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = renegotiate_rate;
    // ALSA's default period, PipeWire's included, can be large: about a
    // second of audio per chunk put speech from before the press into the
    // pre-roll (#1309). Ask for about 20 ms first.
    #[cfg(target_os = "linux")]
    if let Some(&period) =
        fixed_buffer_sizes(supported.sample_rate(), supported.buffer_size()).first()
    {
        match start_stream(device, &supported, period, default_name.clone()) {
            Ok(started) => return Ok(started),
            Err(error) => tracing::warn!(
                error = format!("{error:#}"),
                device = %device_name,
                buffer = ?period,
                "the microphone refused a short period; using its default"
            ),
        }
    }
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
    // Intel Smart Sound refused every stream cpal built, at any format and
    // buffer, so Hex tries its own WASAPI paths next (#988).
    #[cfg(target_os = "windows")]
    match super::wasapi_capture::start(device, device_name) {
        Ok(started) => return Ok(started),
        Err(wasapi_error) => tracing::warn!(
            error = format!("{wasapi_error:#}"),
            device = %device_name,
            cpal_thread_apartment = %super::wasapi_capture::apartment(),
            "the microphone refused every direct wasapi path; trying the formats it lists"
        ),
    }
    // Some drivers refuse every format with the backend's default buffer
    // (Intel Smart Sound answered E_INVALIDARG for each, #988) yet accept
    // the same format with an explicit buffer duration, so each format is
    // tried with explicit buffers too.
    let mut candidates = fixed_buffer_sizes(supported.sample_rate(), supported.buffer_size())
        .into_iter()
        .map(|buffer| (supported, buffer))
        .collect::<Vec<_>>();
    for candidate in alternative_input_configs(device, &supported) {
        candidates.push((candidate, BufferSize::Default));
        for fixed in fixed_buffer_sizes(candidate.sample_rate(), candidate.buffer_size()) {
            candidates.push((candidate, fixed));
        }
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

/// Explicit buffers to try at `rate`: about 20 ms within the range the
/// device reports, then 20 ms and 100 ms as they are. The reported range
/// can top out at the device's short default period, which the Intel Smart
/// Sound driver refuses in shared mode while it takes 20 ms or more (#988).
fn fixed_buffer_sizes(rate: u32, reported: &SupportedBufferSize) -> Vec<BufferSize> {
    let twenty_ms = (rate / 50).max(1);
    let clamped = match *reported {
        SupportedBufferSize::Range { min, max } if min <= max => twenty_ms.clamp(min, max),
        _ => twenty_ms,
    };
    let mut sizes = Vec::with_capacity(3);
    for frames in [clamped, twenty_ms, (rate / 10).max(1)] {
        let size = BufferSize::Fixed(frames);
        if !sizes.contains(&size) {
            sizes.push(size);
        }
    }
    sizes
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
    let mut config: StreamConfig = (*supported).into();
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
        stream: InputStream::Cpal { _stream: stream },
        chunks,
        stream_errors,
        hex_microphone,
        sample_rate,
    })
}

/// Opens and immediately stops a stream at another sample rate, so the device
/// renegotiates its clock before the real stream opens. A failure only logs:
/// the real open carries on as it is.
#[cfg(target_os = "macos")]
fn renegotiate_sample_rate(device: &Device, device_name: &str, supported: &SupportedStreamConfig) {
    let from_rate = supported.sample_rate();
    let Some(via_rate) = other_sample_rate(device, from_rate) else {
        tracing::debug!(
            device = %device_name,
            from_rate,
            "the microphone lists no other sample rate to renegotiate"
        );
        return;
    };
    let throwaway = SupportedStreamConfig::new(
        supported.channels(),
        via_rate,
        *supported.buffer_size(),
        supported.sample_format(),
    );
    match start_stream(device, &throwaway, BufferSize::Default, None) {
        Ok(started) => {
            drop(started);
            tracing::info!(
                device = %device_name,
                from_rate,
                via_rate,
                "renegotiated the microphone's sample rate after digital silence"
            );
        }
        Err(error) => tracing::warn!(
            error = format!("{error:#}"),
            device = %device_name,
            via_rate,
            "could not renegotiate the microphone's sample rate; opening it as it is"
        ),
    }
}

/// A sample rate `device` lists other than `current`: 44.1 kHz before 48 kHz
/// for a 48 kHz device and the reverse otherwise, then the bounds of the
/// formats it lists.
#[cfg(target_os = "macos")]
fn other_sample_rate(device: &Device, current: u32) -> Option<u32> {
    let ranges = match device.supported_input_configs() {
        Ok(ranges) => ranges.collect::<Vec<_>>(),
        Err(error) => {
            tracing::warn!(%error, "could not list the microphone's formats");
            return None;
        }
    };
    let listed = |rate: u32| {
        ranges.iter().any(|range| {
            is_supported_sample_format(range.sample_format())
                && range.min_sample_rate() <= rate
                && rate <= range.max_sample_rate()
        })
    };
    let preferred = [
        if current == 48_000 { 44_100 } else { 48_000 },
        44_100,
        48_000,
    ];
    preferred
        .into_iter()
        .chain(
            ranges
                .iter()
                .flat_map(|range| [range.min_sample_rate(), range.max_sample_rate()]),
        )
        .find(|rate| *rate != current && listed(*rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_buffers_go_past_a_short_reported_range() {
        // A range that tops out at a 10 ms period still gets 20 ms and 100 ms.
        let reported = SupportedBufferSize::Range { min: 144, max: 480 };
        assert_eq!(
            fixed_buffer_sizes(48_000, &reported),
            vec![
                BufferSize::Fixed(480),
                BufferSize::Fixed(960),
                BufferSize::Fixed(4_800),
            ]
        );
        assert_eq!(
            fixed_buffer_sizes(48_000, &SupportedBufferSize::Unknown),
            vec![BufferSize::Fixed(960), BufferSize::Fixed(4_800)]
        );
    }
}

//! The Windows side of meetings: system audio through WASAPI loopback of
//! every active output, and Explorer and the Recycle Bin.
//!
//! cpal opens an input stream on an output device in loopback mode. Loopback
//! delivers nothing while nothing plays, so silence is not a failure here:
//! the track writer pads the gap once audio resumes. A call app often plays
//! on a device other than the default output (a headset set as the default
//! communication device, or the speaker chosen in the app; #1221), so every
//! active output is heard and mixed into the one system track. A change in
//! the list of outputs reopens the source.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample, StreamConfig};

use super::SAMPLE_RATE;
use super::capture::{LiveSource, POLL_INTERVAL, PacketSink, To16k};

const QUEUE_CAPACITY: usize = 256;
/// How often the source checks that the list of outputs has not changed.
const ROUTE_CHECK: Duration = Duration::from_secs(2);
/// 16 kHz samples each output's audio is held back before mixing, so two
/// outputs playing at once line up instead of following each other.
const MIX_HOLD: usize = SAMPLE_RATE as usize / 20;

/// Windows needs no consent to hear the outputs.
pub(super) fn ensure_system_audio() -> Result<()> {
    Ok(())
}

pub(super) fn open_system_audio(_sink: &PacketSink) -> Result<Box<dyn LiveSource>> {
    LoopbackSource::start().map(|source| Box::new(source) as Box<dyn LiveSource>)
}

fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|description| description.name().to_owned())
        .unwrap_or_default()
}

/// Every active output with its name, the default first.
fn outputs() -> Result<Vec<(cpal::Device, String)>> {
    let host = cpal::default_host();
    let default = host
        .default_output_device()
        .map(|device| device_name(&device));
    let mut outputs = host
        .output_devices()
        .wrap_err("could not list the audio outputs")?
        .map(|device| {
            let name = device_name(&device);
            (device, name)
        })
        .collect::<Vec<_>>();
    if let Some(default) = default
        && let Some(at) = outputs.iter().position(|(_, name)| *name == default)
    {
        outputs[..=at].rotate_right(1);
    }
    if outputs.is_empty() {
        return Err(eyre!("no audio output device is available"));
    }
    Ok(outputs)
}

fn output_names() -> Option<Vec<String>> {
    let mut names = outputs()
        .ok()?
        .into_iter()
        .map(|(_, name)| name)
        .collect::<Vec<_>>();
    names.sort();
    Some(names)
}

/// One output's loopback stream and its 16 kHz audio not yet mixed.
struct Output {
    name: String,
    stream: Option<cpal::Stream>,
    resample: To16k,
    pending: Vec<f32>,
}

/// Every active output through WASAPI loopback, downmixed to mono and mixed.
struct LoopbackSource {
    outputs: Vec<Output>,
    buffers: Receiver<(usize, Vec<f32>)>,
    errors: Receiver<(usize, String)>,
    names: Vec<String>,
    route_checked: Instant,
}

impl LoopbackSource {
    fn start() -> Result<Self> {
        let devices = outputs()?;
        let (sender, buffers) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (error_sender, errors) = mpsc::sync_channel(devices.len().max(1));
        let mut opened = Vec::new();
        let mut first_error = None;
        for (device, name) in &devices {
            let index = opened.len();
            match open_output(device, index, sender.clone(), error_sender.clone()) {
                Ok((stream, rate)) => {
                    tracing::info!(device = %name, rate, "system audio loopback started");
                    opened.push(Output {
                        name: name.clone(),
                        stream: Some(stream),
                        resample: To16k::new(rate)?,
                        pending: Vec::new(),
                    });
                }
                Err(error) => {
                    tracing::warn!(
                        device = %name,
                        error = %format!("{error:#}"),
                        "could not hear an audio output"
                    );
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        if opened.is_empty() {
            return Err(first_error.unwrap_or_else(|| eyre!("no audio output device is available")));
        }
        let mut names = devices
            .into_iter()
            .map(|(_, name)| name)
            .collect::<Vec<_>>();
        names.sort();
        Ok(Self {
            outputs: opened,
            buffers,
            errors,
            names,
            route_checked: Instant::now(),
        })
    }
}

/// Opens and starts `device`'s loopback, sending its audio tagged `index`.
fn open_output(
    device: &cpal::Device,
    index: usize,
    sender: SyncSender<(usize, Vec<f32>)>,
    errors: SyncSender<(usize, String)>,
) -> Result<(cpal::Stream, u32)> {
    let supported = device
        .default_output_config()
        .wrap_err("could not read the output device configuration")?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let stream = match sample_format {
        SampleFormat::F32 => loopback(device, &config, index, sender, errors, |s: f32| s)?,
        SampleFormat::I16 => loopback(device, &config, index, sender, errors, |s: i16| {
            f32::from(s) / f32::from(i16::MAX)
        })?,
        SampleFormat::I32 => loopback(device, &config, index, sender, errors, |s: i32| {
            s as f32 / i32::MAX as f32
        })?,
        format => return Err(eyre!("unsupported output sample format: {format:?}")),
    };
    stream
        .play()
        .wrap_err("could not start system audio loopback")?;
    Ok((stream, config.sample_rate))
}

/// Mixes each output's pending audio up to the latest output's position less
/// `hold`, leaving the rest for the next call. An output that delivered
/// nothing (nothing plays on it) adds nothing.
fn mix(pending: &mut [&mut Vec<f32>], hold: usize) -> Vec<f32> {
    let length = pending
        .iter()
        .map(|samples| samples.len())
        .max()
        .unwrap_or(0)
        .saturating_sub(hold);
    let mut mixed = vec![0.0f32; length];
    for samples in pending.iter_mut() {
        let take = samples.len().min(length);
        for (out, sample) in mixed.iter_mut().zip(samples.drain(..take)) {
            *out += sample;
        }
    }
    for sample in &mut mixed {
        *sample = sample.clamp(-1.0, 1.0);
    }
    mixed
}

fn loopback<T: SizedSample + Send + 'static>(
    device: &cpal::Device,
    config: &StreamConfig,
    index: usize,
    sender: SyncSender<(usize, Vec<f32>)>,
    errors: SyncSender<(usize, String)>,
    convert: impl Fn(T) -> f32 + Send + 'static,
) -> Result<cpal::Stream> {
    let channels = usize::from(config.channels).max(1);
    device
        .build_input_stream(
            *config,
            move |data: &[T], _| {
                let mono = data
                    .chunks_exact(channels)
                    .map(|frame| frame.iter().map(|&s| convert(s)).sum::<f32>() / channels as f32)
                    .collect();
                let _ = sender.try_send((index, mono));
            },
            move |error: cpal::Error| {
                // A buffer discontinuity (WASAPI's data-discontinuity flag)
                // is reported while the stream keeps delivering audio, so
                // it must not end the source: reopening on each one left
                // the system track empty (#1035).
                if matches!(
                    error.kind(),
                    cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied
                ) {
                    tracing::debug!(%error, "system audio loopback glitched; continuing");
                    return;
                }
                let _ = errors.try_send((index, error.to_string()));
            },
            None,
        )
        .wrap_err("could not open system audio loopback")
}

impl LoopbackSource {
    fn take(&mut self, (index, samples): (usize, Vec<f32>)) {
        if let Some(output) = self.outputs.get_mut(index)
            && output.stream.is_some()
        {
            let resampled = output.resample.push(&samples);
            output.pending.extend(resampled);
        }
    }
}

impl LiveSource for LoopbackSource {
    fn poll(&mut self, sink: &PacketSink) -> std::result::Result<(), String> {
        // A failing output stops alone; the others keep the track going.
        while let Ok((index, error)) = self.errors.try_recv() {
            if let Some(output) = self.outputs.get_mut(index)
                && output.stream.take().is_some()
            {
                tracing::warn!(
                    device = %output.name,
                    %error,
                    "system audio loopback stopped for one output"
                );
            }
        }
        if self.outputs.iter().all(|output| output.stream.is_none()) {
            return Err("every system audio loopback stopped".into());
        }
        if self.route_checked.elapsed() >= ROUTE_CHECK {
            self.route_checked = Instant::now();
            if let Some(names) = output_names()
                && names != self.names
            {
                return Err("the audio outputs changed".into());
            }
        }
        match self.buffers.recv_timeout(POLL_INTERVAL) {
            Ok(first) => {
                self.take(first);
                while let Ok(next) = self.buffers.try_recv() {
                    self.take(next);
                }
                let mut pending = self
                    .outputs
                    .iter_mut()
                    .map(|output| &mut output.pending)
                    .collect::<Vec<_>>();
                sink.send(SAMPLE_RATE, mix(&mut pending, MIX_HOLD), None);
                Ok(())
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(()),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("system audio loopback disconnected".into())
            }
        }
    }

    fn describe(&self) -> &'static str {
        "wasapi-loopback"
    }
}

/// Opens Explorer with `path` selected.
pub(crate) fn reveal(path: &Path) {
    let mut select = std::ffi::OsString::from("/select,");
    select.push(path);
    if let Err(error) = std::process::Command::new("explorer.exe")
        .arg(select)
        .spawn()
    {
        tracing::error!(%error, "could not reveal meeting file");
    }
}

/// Moves `path` to the Recycle Bin, where it can be restored.
pub(super) fn move_to_trash(path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW,
        SHFileOperationW,
    };
    let absolute = std::path::absolute(path)?;
    // A list of paths, each ended by a NUL, and the list by a second one.
    let mut from = absolute.as_os_str().encode_wide().collect::<Vec<u16>>();
    from.extend([0, 0]);
    let mut operation = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    // SAFETY: `from` is double-NUL-terminated and outlives the call.
    let status = unsafe { SHFileOperationW(&mut operation) };
    if status != 0 || operation.fAnyOperationsAborted != 0 {
        return Err(eyre!(
            "could not move {} to the Recycle Bin (error {status:#x})",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::mix;

    #[test]
    fn audio_on_any_output_reaches_the_system_track() {
        // Only the headset plays (#1221): its audio is kept, less the hold.
        let mut speakers = Vec::new();
        let mut headset = vec![0.25; 10];
        let mixed = mix(&mut [&mut speakers, &mut headset], 4);
        assert_eq!(mixed, vec![0.25; 6]);
        assert_eq!(headset.len(), 4);

        // Both play: their audio adds up sample by sample, clamped.
        let mut speakers = vec![0.5; 8];
        let mut headset = vec![0.75; 6];
        let mixed = mix(&mut [&mut speakers, &mut headset], 2);
        assert_eq!(mixed, vec![1.0; 6]);
        assert_eq!((speakers.len(), headset.len()), (2, 0));

        // Nothing plays: nothing is sent.
        let mut idle = Vec::new();
        assert!(mix(&mut [&mut idle], 2).is_empty());
    }
}

//! The Windows side of meetings: system audio through WASAPI loopback of the
//! default output, and Explorer and the Recycle Bin.
//!
//! cpal opens an input stream on an output device in loopback mode. Loopback
//! delivers nothing while nothing plays, so silence is not a failure here:
//! the track writer pads the gap once audio resumes. A change of the default
//! output reopens the source on the new one.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample, StreamConfig};

use super::SAMPLE_RATE;
use super::capture::{LiveSource, POLL_INTERVAL, PacketSink, To16k};

const QUEUE_CAPACITY: usize = 256;
/// How often the source checks that the default output has not changed.
const ROUTE_CHECK: Duration = Duration::from_secs(2);

/// Windows needs no consent to hear the default output.
pub(super) fn ensure_system_audio() -> Result<()> {
    Ok(())
}

pub(super) fn open_system_audio(_sink: &PacketSink) -> Result<Box<dyn LiveSource>> {
    LoopbackSource::start().map(|source| Box::new(source) as Box<dyn LiveSource>)
}

fn default_output() -> Result<(cpal::Device, String)> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| eyre!("no audio output device is available"))?;
    let name = device
        .description()
        .map(|description| description.name().to_owned())
        .unwrap_or_default();
    Ok((device, name))
}

/// The default output through WASAPI loopback, downmixed to mono.
struct LoopbackSource {
    _stream: cpal::Stream,
    buffers: Receiver<Vec<f32>>,
    errors: Receiver<String>,
    resample: To16k,
    device_name: String,
    route_checked: Instant,
}

impl LoopbackSource {
    fn start() -> Result<Self> {
        let (device, device_name) = default_output()?;
        let supported = device
            .default_output_config()
            .wrap_err("could not read the output device configuration")?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let (sender, buffers) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (error_sender, errors) = mpsc::sync_channel(1);
        let stream = match sample_format {
            SampleFormat::F32 => loopback(&device, &config, sender, error_sender, |s: f32| s)?,
            SampleFormat::I16 => loopback(&device, &config, sender, error_sender, |s: i16| {
                f32::from(s) / f32::from(i16::MAX)
            })?,
            SampleFormat::I32 => loopback(&device, &config, sender, error_sender, |s: i32| {
                s as f32 / i32::MAX as f32
            })?,
            format => return Err(eyre!("unsupported output sample format: {format:?}")),
        };
        stream
            .play()
            .wrap_err("could not start system audio loopback")?;
        tracing::info!(
            device = device_name,
            rate = config.sample_rate,
            "system audio loopback started"
        );
        Ok(Self {
            _stream: stream,
            buffers,
            errors,
            resample: To16k::new(config.sample_rate)?,
            device_name,
            route_checked: Instant::now(),
        })
    }
}

fn loopback<T: SizedSample + Send + 'static>(
    device: &cpal::Device,
    config: &StreamConfig,
    sender: SyncSender<Vec<f32>>,
    errors: SyncSender<String>,
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
                let _ = sender.try_send(mono);
            },
            move |error| {
                let _ = errors.try_send(error.to_string());
            },
            None,
        )
        .wrap_err("could not open system audio loopback")
}

impl LiveSource for LoopbackSource {
    fn poll(&mut self, sink: &PacketSink) -> std::result::Result<(), String> {
        if let Ok(error) = self.errors.try_recv() {
            return Err(error);
        }
        if self.route_checked.elapsed() >= ROUTE_CHECK {
            self.route_checked = Instant::now();
            if let Ok((_, name)) = default_output()
                && name != self.device_name
            {
                return Err(format!("the default output changed to {name}"));
            }
        }
        match self.buffers.recv_timeout(POLL_INTERVAL) {
            Ok(mut samples) => {
                while let Ok(next) = self.buffers.try_recv() {
                    samples.extend(next);
                }
                sink.send(SAMPLE_RATE, self.resample.push(&samples), None);
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

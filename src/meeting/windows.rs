//! The Windows side of meetings: system audio through WASAPI loopback of the
//! call app's outputs, and Explorer and the Recycle Bin.
//!
//! cpal opens an input stream on an output device in loopback mode. Loopback
//! delivers nothing while nothing plays, so silence is not a failure here:
//! the track writer pads the gap once audio resumes. A call app often plays
//! on a device other than the default output (a headset set as the default
//! communication device, or the speaker chosen in the app; #1221). A call
//! recording hears the outputs that app's own processes play on, preferring
//! its microphone holder's, because a virtual mixer (SteelSeries Sonar,
//! VB-Cable, Voicemeeter) also plays the user's processed microphone and
//! replays the call on the physical output; a manual recording hears every
//! active output. A change in the chosen outputs reopens the source.
//!
//! Labs › Choose meeting audio replaces that with one source: one output
//! endpoint, looped back alone and recorded as silence while it is unplugged,
//! or one app, through WASAPI process loopback
//! ([process_loopback](self::process_loopback)). A chosen source never falls
//! back to every output. With the lab off, or all computer audio chosen, this
//! file behaves exactly as before.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Report, Result, WrapErr, eyre};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample, StreamConfig};
use windows::Win32::Media::Audio::{eCapture, eRender};

use super::SAMPLE_RATE;
use super::audio_source::{SourceChoice, effective};
use super::capture::{LiveSource, MIX_HOLD, POLL_INTERVAL, PacketSink, To16k, mix};
use super::loopback_route::{self, Choice};
use crate::microphone_activity::{
    EndpointSessions, active_endpoint_sessions, process_canonical_bundle_id,
};

#[path = "windows/process_loopback.rs"]
mod process_loopback;

const QUEUE_CAPACITY: usize = 256;
/// How often the source checks that the outputs to hear have not changed.
const ROUTE_CHECK: Duration = Duration::from_secs(2);

/// Windows needs no consent to hear the outputs.
pub(super) fn ensure_system_audio() -> Result<()> {
    Ok(())
}

/// Opens the system track's audio: every output as always, or the one output
/// or app Labs › Choose meeting audio names. A chosen source re-reads the
/// choice on its route check and reopens when it differs, so a turned-off
/// lab, or a new choice, applies mid-recording.
pub(super) fn open_system_audio(
    _sink: &PacketSink,
    application: Option<&str>,
) -> Result<Box<dyn LiveSource>> {
    match effective() {
        SourceChoice::All => {
            LoopbackSource::start(application).map(|source| Box::new(source) as Box<dyn LiveSource>)
        }
        choice @ SourceChoice::Output { .. } => {
            SingleOutputSource::start(choice).map(|source| Box::new(source) as Box<dyn LiveSource>)
        }
        choice @ SourceChoice::App { .. } => process_loopback::AppSource::start(choice)
            .map(|source| Box::new(source) as Box<dyn LiveSource>),
    }
}

fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|description| description.name().to_owned())
        .unwrap_or_default()
}

/// The WASAPI endpoint id cpal carries in its `DeviceId`: the same string
/// `IMMDevice::GetId` gives the audio session enumeration.
fn endpoint_id(device: &cpal::Device) -> Option<String> {
    device.id().ok().map(|id| id.id().to_owned())
}

/// One output cpal listed, with the endpoint id the OS names in sessions.
struct OutputDevice {
    device: cpal::Device,
    name: String,
    endpoint: Option<String>,
}

/// Every active output with its name, the default first.
fn outputs() -> Result<Vec<OutputDevice>> {
    let host = cpal::default_host();
    let default = host
        .default_output_device()
        .map(|device| device_name(&device));
    let mut outputs = host
        .output_devices()
        .wrap_err("could not list the audio outputs")?
        .map(|device| OutputDevice {
            endpoint: endpoint_id(&device),
            name: device_name(&device),
            device,
        })
        .collect::<Vec<_>>();
    if let Some(default) = default
        && let Some(at) = outputs.iter().position(|output| output.name == default)
    {
        outputs[..=at].rotate_right(1);
    }
    if outputs.is_empty() {
        return Err(eyre!("no audio output device is available"));
    }
    Ok(outputs)
}

/// The string naming an output: cpal's endpoint id where the device has
/// one, else its name. Labs › Choose meeting audio lists these, a saved
/// Output choice names one, and [`SingleOutputSource`] matches one, so all
/// three have to agree.
fn endpoint_key(endpoint: Option<&str>, name: &str) -> String {
    endpoint
        .filter(|id| !id.is_empty())
        .map_or_else(|| name.to_owned(), str::to_owned)
}

/// [`endpoint_key`] for a listed output.
fn endpoint_of(device: &OutputDevice) -> String {
    endpoint_key(device.endpoint.as_deref(), &device.name)
}

/// Every listed output's endpoint id, or its name where cpal has no id: the
/// every-output route check compares exactly these, as it always has.
fn output_endpoints(devices: &[OutputDevice]) -> Vec<String> {
    let mut endpoints = devices
        .iter()
        .map(|device| {
            device
                .endpoint
                .clone()
                .unwrap_or_else(|| device.name.clone())
        })
        .collect::<Vec<_>>();
    endpoints.sort();
    endpoints
}

/// The listed outputs the Meetings pane offers, the default first, as
/// `(endpoint, name)`: the same endpoint strings this file matches a saved
/// Output choice against.
pub(super) fn output_choices() -> Vec<(String, String)> {
    match outputs() {
        Ok(devices) => devices
            .into_iter()
            .map(|device| (endpoint_of(&device), device.name))
            .collect(),
        Err(error) => {
            tracing::debug!(%error, "could not list the audio outputs");
            Vec::new()
        }
    }
}

/// The position of the chosen endpoint among the listed ones.
fn chosen_index(endpoints: &[String], chosen: &str) -> Option<usize> {
    endpoints.iter().position(|endpoint| endpoint == chosen)
}

/// The outputs to loop back: those of `chosen`, or every one.
fn selected_outputs<'a>(
    devices: &'a [OutputDevice],
    chosen: Option<&Choice>,
) -> Vec<&'a OutputDevice> {
    match chosen {
        Some(Choice::MicHolder(endpoints) | Choice::App(endpoints)) => devices
            .iter()
            .filter(|device| {
                device
                    .endpoint
                    .as_ref()
                    .is_some_and(|id| endpoints.contains(id))
            })
            .collect(),
        Some(Choice::Every) | None => devices.iter().collect(),
    }
}

/// The call app's render endpoints now, or `None` when the OS could not list
/// the sessions.
fn app_route(application: &str) -> Option<Choice> {
    match route(application) {
        Ok(choice) => Some(choice),
        Err(error) => {
            tracing::debug!(
                %error,
                "could not read the call app's audio sessions; keeping the current outputs"
            );
            None
        }
    }
}

fn route(application: &str) -> Result<Choice> {
    let render = active_endpoint_sessions(eRender)?;
    let capture = active_endpoint_sessions(eCapture)?;
    let pids = render
        .iter()
        .chain(&capture)
        .flat_map(|endpoint| endpoint.pids.iter().copied())
        .collect::<BTreeSet<_>>();
    let call_app = pids
        .into_iter()
        .filter(|&pid| {
            process_canonical_bundle_id(pid).is_some_and(|canonical| canonical == application)
        })
        .collect::<HashSet<_>>();
    Ok(loopback_route::choose(
        Some(application),
        &plain(render),
        &plain(capture),
        |pid| call_app.contains(&pid),
    ))
}

fn plain(endpoints: Vec<EndpointSessions>) -> Vec<(String, Vec<u32>)> {
    endpoints
        .into_iter()
        .map(|endpoint| (endpoint.endpoint_id, endpoint.pids))
        .collect()
}

/// One output's loopback stream and its 16 kHz audio not yet mixed.
struct Output {
    name: String,
    stream: Option<cpal::Stream>,
    resample: To16k,
    pending: Vec<f32>,
}

/// The chosen outputs through WASAPI loopback, downmixed to mono and mixed.
struct LoopbackSource {
    outputs: Vec<Output>,
    buffers: Receiver<(usize, Vec<f32>)>,
    errors: Receiver<(usize, String)>,
    /// Every listed output's endpoint at open, for the every-output check.
    listed: Vec<String>,
    /// The call app's canonical bundle identifier, for a call recording.
    application: Option<String>,
    /// What the app is following: its own endpoints, or `None` for every
    /// output.
    chosen: Option<Choice>,
    /// Whether hearing every output should still reopen when the app plays.
    watch_app: bool,
    route_checked: Instant,
}

impl LoopbackSource {
    fn start(application: Option<&str>) -> Result<Self> {
        let devices = outputs()?;
        let listed = output_endpoints(&devices);
        let application = application.map(str::to_owned);
        // Why the outputs below are heard, logged once with them.
        let mut reason = if application.is_some() {
            "every output: no app session"
        } else {
            "every output: no app"
        };
        let mut chosen = None;
        let mut watch_app = false;
        if let Some(application) = application.as_deref() {
            match app_route(application) {
                Some(Choice::MicHolder(endpoints)) => {
                    chosen = Some(Choice::MicHolder(endpoints));
                    reason = "the call app's microphone holder";
                }
                Some(Choice::App(endpoints)) => {
                    chosen = Some(Choice::App(endpoints));
                    reason = "the call app";
                }
                // The app plays nothing yet: hear every output until it does.
                Some(Choice::Every) => watch_app = true,
                // A failed enumeration may recover; until it does, every
                // output is heard and the route check keeps looking.
                None => {
                    watch_app = true;
                    reason = "every output: the app's sessions could not be read";
                }
            }
        }
        let mut selected = selected_outputs(&devices, chosen.as_ref());
        let (sender, buffers) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (error_sender, errors) = mpsc::sync_channel(devices.len().max(1));
        let (mut opened, mut first_error) =
            open_outputs(&selected, sender.clone(), error_sender.clone())?;
        if opened.is_empty() && chosen.is_some() {
            // The app's outputs exist but none would open: hear every output
            // rather than lose the system track. Its endpoints are not
            // watched again, so a route check cannot loop on reopening them.
            tracing::warn!("the call app's audio outputs would not open; hearing every output");
            chosen = None;
            watch_app = false;
            reason = "every output: the app's outputs would not open";
            selected = devices.iter().collect();
            (opened, first_error) = open_outputs(&selected, sender, error_sender)?;
        }
        if opened.is_empty() {
            return Err(first_error.unwrap_or_else(|| eyre!("no audio output device is available")));
        }
        let heard = selected
            .iter()
            .map(|device| device.name.as_str())
            .collect::<Vec<_>>();
        tracing::info!(
            app = application.as_deref().unwrap_or("manual"),
            reason,
            outputs = ?heard,
            "meeting system audio loopback outputs chosen"
        );
        Ok(Self {
            outputs: opened,
            buffers,
            errors,
            listed,
            application,
            chosen,
            watch_app,
            route_checked: Instant::now(),
        })
    }

    /// Why this source should reopen on a route check, if it should: the
    /// choice is no longer every output, the listed outputs changed while
    /// hearing every one, or the app's own endpoints changed (see
    /// `loopback_route::should_reopen`). A temporary absence of the app's
    /// sessions, or an enumeration failure, keeps the current outputs. With
    /// Labs › Choose meeting audio off the choice is every output, so it never
    /// reopens for one.
    fn route_change(&self) -> Option<&'static str> {
        if !effective().is_all() {
            return Some("the meeting audio choice changed");
        }
        let outputs_changed = || {
            outputs()
                .ok()
                .map(|devices| output_endpoints(&devices))
                .is_some_and(|endpoints| endpoints != self.listed)
                .then_some("the audio outputs changed")
        };
        match &self.chosen {
            None if !self.watch_app => outputs_changed(),
            None => outputs_changed().or_else(|| {
                let choice = self.application.as_deref().and_then(app_route)?;
                (choice != Choice::Every).then_some("the call app started playing")
            }),
            Some(chosen) => {
                let choice = self.application.as_deref().and_then(app_route)?;
                loopback_route::should_reopen(chosen, &choice)
                    .then_some("the call app's outputs changed")
            }
        }
    }
}

/// Opens every selected output's loopback, logging each failure. Returns the
/// opened streams and the first error, for an empty result's report.
fn open_outputs(
    devices: &[&OutputDevice],
    sender: SyncSender<(usize, Vec<f32>)>,
    errors: SyncSender<(usize, String)>,
) -> Result<(Vec<Output>, Option<Report>)> {
    let mut opened = Vec::new();
    let mut first_error = None;
    for device in devices {
        let index = opened.len();
        match open_output(&device.device, index, sender.clone(), errors.clone()) {
            Ok((stream, rate)) => {
                tracing::info!(device = %device.name, rate, "system audio loopback started");
                opened.push(Output {
                    name: device.name.clone(),
                    stream: Some(stream),
                    resample: To16k::new(rate)?,
                    pending: Vec::new(),
                });
            }
            Err(error) => {
                tracing::warn!(
                    device = %device.name,
                    error = %format!("{error:#}"),
                    "could not hear an audio output"
                );
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    Ok((opened, first_error))
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
            if let Some(reason) = self.route_change() {
                return Err(reason.into());
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

/// The one output Labs › Choose meeting audio names, looped back alone.
///
/// A chosen output is never replaced by another: while it is unplugged this
/// source sends nothing, and the track writer pads the gap. The 2 s route
/// check reports the endpoint appearing or going away, and the supervisor
/// reopens the source then: it opens the output, or waits in silence until
/// the endpoint is listed again. A stream that fails while the endpoint is
/// still listed returns an error, so the supervisor reopens it with its
/// bounded backoff, and an output that is listed but would not open fails the
/// open the same way. The choice follows the device across plugging,
/// switching, and Bluetooth profile changes.
struct SingleOutputSource {
    /// The choice this source opened with, so a different one reopens it.
    choice: SourceChoice,
    /// The chosen endpoint, as [`endpoint_key`] names it.
    endpoint: String,
    /// Kept while no stream is open, so the audio channel never disconnects.
    sender: SyncSender<(usize, Vec<f32>)>,
    buffers: Receiver<(usize, Vec<f32>)>,
    errors: Receiver<(usize, String)>,
    /// The output's stream, while the endpoint is listed and it opened.
    output: Option<Output>,
    /// Whether the endpoint was listed when this source opened.
    listed: bool,
    route_checked: Instant,
}

impl SingleOutputSource {
    fn start(choice: SourceChoice) -> Result<Self> {
        let Some(endpoint) = choice.output_id().map(str::to_owned) else {
            return Err(eyre!("the meeting audio choice is not an output device"));
        };
        let devices = outputs()?;
        let endpoints = devices.iter().map(endpoint_of).collect::<Vec<_>>();
        let index = chosen_index(&endpoints, &endpoint);
        let (sender, buffers) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (error_sender, errors) = mpsc::sync_channel(1);
        let output = match index {
            Some(index) => {
                let device = &devices[index];
                let (stream, rate) = open_output(&device.device, 0, sender.clone(), error_sender)?;
                tracing::info!(
                    device = %device.name,
                    endpoint,
                    rate,
                    "the chosen meeting audio output started"
                );
                Some(Output {
                    name: device.name.clone(),
                    stream: Some(stream),
                    resample: To16k::new(rate)?,
                    pending: Vec::new(),
                })
            }
            None => {
                tracing::info!(
                    endpoint,
                    "the chosen meeting audio output isn't connected; recording silence until it returns"
                );
                None
            }
        };
        Ok(Self {
            choice,
            endpoint,
            sender,
            buffers,
            errors,
            output,
            listed: index.is_some(),
            route_checked: Instant::now(),
        })
    }

    /// Whether the chosen endpoint is listed differently from when this
    /// source opened, so the supervisor reopens it into the right state. A
    /// listing that cannot be read keeps the source as it is.
    fn presence_change(&self) -> Option<&'static str> {
        let listed = outputs()
            .ok()?
            .iter()
            .map(endpoint_of)
            .any(|endpoint| endpoint == self.endpoint);
        (listed != self.listed).then_some("the chosen meeting audio output changed")
    }

    fn take(&mut self, (index, samples): (usize, Vec<f32>)) {
        if index == 0
            && let Some(output) = self.output.as_mut()
        {
            let resampled = output.resample.push(&samples);
            output.pending.extend(resampled);
        }
    }
}

impl LiveSource for SingleOutputSource {
    fn poll(&mut self, sink: &PacketSink) -> std::result::Result<(), String> {
        // A stream that failed reopens the source while its endpoint is
        // still listed; one whose endpoint went away reopens into silence.
        while let Ok((_, error)) = self.errors.try_recv() {
            if let Some(output) = self.output.take() {
                tracing::warn!(
                    device = %output.name,
                    %error,
                    "the chosen meeting audio output's loopback stopped"
                );
            }
        }
        if self.listed && self.output.is_none() {
            return Err("the chosen audio output's loopback stopped".into());
        }
        if self.route_checked.elapsed() >= ROUTE_CHECK {
            self.route_checked = Instant::now();
            // Another choice, or a turned-off lab, reopens into it: a choice
            // applies mid-recording, not from the next stream.
            if effective() != self.choice {
                return Err("the meeting audio choice changed".into());
            }
            if let Some(reason) = self.presence_change() {
                return Err(reason.into());
            }
        }
        match self.buffers.recv_timeout(POLL_INTERVAL) {
            Ok(first) => {
                self.take(first);
                while let Ok(next) = self.buffers.try_recv() {
                    self.take(next);
                }
                let samples = match self.output.as_mut() {
                    Some(output) => mix(&mut [&mut output.pending], MIX_HOLD),
                    None => Vec::new(),
                };
                sink.send(SAMPLE_RATE, samples, None);
                Ok(())
            }
            // The chosen output is not there: nothing to record now, and the
            // track writer pads the gap (as it does for any silent output).
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(()),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("the chosen audio output's loopback disconnected".into())
            }
        }
    }

    fn describe(&self) -> &'static str {
        "wasapi-endpoint-loopback"
    }
}

/// Opens Explorer with `path` selected.
pub(crate) fn reveal(path: &Path) {
    use std::os::windows::process::CommandExt;

    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    if let Err(error) = std::process::Command::new("explorer.exe")
        .raw_arg(super::explorer_select(&path))
        .spawn()
    {
        tracing::error!(%error, "could not reveal meeting file");
    }
}

/// Opens Explorer at the folder `path` itself, rather than selecting it in
/// its parent.
pub(crate) fn open_folder(path: &Path) {
    use std::os::windows::process::CommandExt;

    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    if let Err(error) = std::process::Command::new("explorer.exe")
        .raw_arg(super::explorer_open(&path))
        .spawn()
    {
        tracing::error!(%error, "could not open meeting folder");
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
    use super::*;

    /// An output with cpal's endpoint id and one whose backend gave none:
    /// both are named the way the Meetings pane lists them, so the pane, the
    /// saved choice, and the capture source compare the same strings.
    #[test]
    fn an_output_is_named_by_its_endpoint_id_or_its_name() {
        let id = "{0.0.0.00000000}.{784DE0B4-F887-4314-83B9-4CCA2BC6AC3B}";
        assert_eq!(endpoint_key(Some(id), "Speakers"), id);
        assert_eq!(endpoint_key(None, "Speakers"), "Speakers");
        assert_eq!(endpoint_key(Some(""), "Speakers"), "Speakers");
    }

    /// A saved output records that endpoint alone: a missing one matches
    /// nothing and never another output.
    #[test]
    fn a_saved_output_matches_only_its_own_endpoint() {
        let listed = [
            endpoint_key(Some("{0.0.0.00000000}.{1}"), "Speakers"),
            endpoint_key(None, "Headphones"),
        ];
        assert_eq!(chosen_index(&listed, "{0.0.0.00000000}.{1}"), Some(0));
        assert_eq!(chosen_index(&listed, "Headphones"), Some(1));
        assert_eq!(chosen_index(&listed, "Headphones "), None);
        assert_eq!(chosen_index(&listed, "{0.0.0.00000000}.{2}"), None);
        assert_eq!(chosen_index(&[], "Headphones"), None);
    }
}

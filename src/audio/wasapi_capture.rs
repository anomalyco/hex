//! A Windows microphone stream opened directly through WASAPI when cpal's
//! stream will not open. Some drivers (Intel Smart Sound, #988) refuse every
//! stream cpal builds with E_INVALIDARG, and in one process even the
//! endpoint's unchanged mix format, so this tries several activation and
//! initialization paths in turn and logs every argument of each attempt.
//! Frames are averaged to mono here; the capture timeline resamples them.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::thread::JoinHandle;
use std::time::Duration;

use color_eyre::eyre::{Result, eyre};
use cpal::Device;
use cpal::traits::DeviceTrait;
use windows::Win32::Foundation::{CloseHandle, E_FAIL, HANDLE};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
    ActivateAudioInterfaceAsync, DEVINTERFACE_AUDIO_CAPTURE, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
    IAudioCaptureClient, IAudioClient, IAudioClient3, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, WAVEFORMATEX, eCapture, eConsole,
};
use windows::Win32::System::Com::{
    APTTYPE, APTTYPE_MAINSTA, APTTYPE_MTA, APTTYPE_NA, APTTYPE_STA, APTTYPEQUALIFIER, CLSCTX_ALL,
    COINIT_MULTITHREADED, CoCreateInstance, CoGetApartmentType, CoInitializeEx, CoTaskMemFree,
    StringFromIID,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::{HRESULT, HSTRING, IUnknown, Interface, PCWSTR, Ref, implement};

use super::hex_microphone::{CaptureFeed, CaptureFeedSwitch, capture_feed};
use super::input_format::{InputStream, StartedStream};
use super::mix_format::{MixFormat, float_format_bytes};
use super::{CaptureInstant, duration_for_frames};

/// How long opening, across every path, may take before Hex gives up.
const OPEN_TIMEOUT: Duration = Duration::from_secs(15);
/// How long one `ActivateAudioInterfaceAsync` may take to complete.
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(3);
/// The polling path's buffer: one second, which the Intel Smart Sound
/// driver accepted without event callbacks in a standalone probe (#988).
const POLLING_BUFFER_HNS: i64 = 10_000_000;
/// Shared mode requires a zero periodicity.
const PERIODICITY_HNS: i64 = 0;
/// The format the engine converts to on the auto-convert path.
const CONVERTED_SAMPLE_RATE: u32 = 16_000;
const SPEAKER_FRONT_CENTER: u32 = 0x4;
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const EVENT_WAIT_MS: u32 = 100;

/// A running capture thread; dropping it stops capture.
pub(super) struct WasapiCapture {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for WasapiCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// One way of activating and initializing the endpoint, tried in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Path {
    /// `IAudioClient::Initialize` in the mix format, event callback, buffer 0.
    Event,
    /// `IAudioClient::Initialize` in the mix format, no flags, one-second
    /// buffer, polled.
    Polling,
    /// `IAudioClient3::InitializeSharedAudioStream` in the mix format at the
    /// engine's default period.
    SharedAudioStream,
    /// `ActivateAudioInterfaceAsync` on the endpoint's device interface path,
    /// then as `Event`.
    InterfaceActivation,
    /// `ActivateAudioInterfaceAsync` on the default-capture interface, then as
    /// `Event`; only when this endpoint is the default.
    DefaultInterfaceActivation,
    /// `IAudioClient::Initialize` with AUTOCONVERTPCM | SRC_DEFAULT_QUALITY,
    /// asking the engine for 16 kHz mono float.
    AutoConvert,
}

const PATHS: [Path; 6] = [
    Path::Event,
    Path::Polling,
    Path::SharedAudioStream,
    Path::InterfaceActivation,
    Path::DefaultInterfaceActivation,
    Path::AutoConvert,
];

impl Path {
    fn flags(self) -> u32 {
        match self {
            Self::Polling => 0,
            Self::AutoConvert => {
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                    | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY
            }
            _ => AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
        }
    }

    fn buffer_hns(self) -> i64 {
        match self {
            Self::Polling => POLLING_BUFFER_HNS,
            _ => 0,
        }
    }

    fn polls(self) -> bool {
        self == Self::Polling
    }

    fn activation(self) -> &'static str {
        match self {
            Self::InterfaceActivation => "ActivateAudioInterfaceAsync(endpoint interface path)",
            Self::DefaultInterfaceActivation => {
                "ActivateAudioInterfaceAsync(DEVINTERFACE_AUDIO_CAPTURE default)"
            }
            _ => "IMMDevice::Activate(CLSCTX_ALL, no activation params)",
        }
    }

    fn initialize(self) -> &'static str {
        match self {
            Self::SharedAudioStream => "IAudioClient3::InitializeSharedAudioStream",
            _ => "IAudioClient::Initialize",
        }
    }

    fn format_source(self) -> &'static str {
        match self {
            Self::AutoConvert => "requested 16 kHz mono float, engine converts",
            _ => "GetMixFormat pointer, unchanged",
        }
    }
}

type Ready = std::result::Result<(u32, CaptureFeedSwitch), String>;

/// Opens `device` through WASAPI directly and starts capture.
pub(super) fn start(device: &Device, device_name: &str) -> Result<StartedStream> {
    let name = device_name.to_owned();
    let id = device
        .id()
        .map_err(|error| eyre!("could not read the microphone's id: {error}"))?
        .id()
        .to_owned();
    let (sender, chunks) = mpsc::channel();
    let (error_sender, stream_errors) = mpsc::sync_channel(1);
    let (ready_sender, ready) = mpsc::sync_channel(1);
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker = std::thread::Builder::new()
        .name("wasapi-capture".into())
        .spawn(move || {
            // MTA, matching the standalone WASAPI calls that open this
            // driver; balanced implicitly at thread exit.
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            let streams = Streams {
                chunks: sender,
                errors: error_sender,
            };
            run(&id, &name, &worker_stop, &ready_sender, &streams);
        })
        .map_err(|error| eyre!("could not start the microphone thread: {error}"))?;
    let (sample_rate, hex_microphone) = match ready.recv_timeout(OPEN_TIMEOUT) {
        Ok(Ok(opened)) => opened,
        Ok(Err(error)) => {
            let _ = worker.join();
            return Err(eyre!(error));
        }
        Err(_) => {
            // The thread is stuck in the driver; it exits once it returns.
            stop.store(true, Ordering::Release);
            return Err(eyre!("the microphone did not open through WASAPI in time"));
        }
    };
    Ok(StartedStream {
        stream: InputStream::Wasapi {
            _capture: WasapiCapture {
                stop,
                worker: Some(worker),
            },
        },
        chunks,
        stream_errors,
        hex_microphone,
        sample_rate,
    })
}

struct Streams {
    chunks: Sender<(Vec<f32>, CaptureInstant)>,
    errors: SyncSender<String>,
}

/// An initialized, started client and how to wait for its packets.
struct Opened {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    format: MixFormat,
    path: Path,
    event: Option<OwnedEvent>,
}

struct OwnedEvent(HANDLE);

impl Drop for OwnedEvent {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn run(id: &str, name: &str, stop: &AtomicBool, ready: &SyncSender<Ready>, streams: &Streams) {
    let opened = match open(id, name) {
        Ok(opened) => opened,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let (mut feed, switch) = capture_feed(opened.format.sample_rate);
    if ready.send(Ok((opened.format.sample_rate, switch))).is_err() {
        let _ = unsafe { opened.client.Stop() };
        return;
    }
    if let Err(error) = capture(&opened, stop, &mut feed, &streams.chunks) {
        let _ = streams.errors.try_send(error);
    }
    let _ = unsafe { opened.client.Stop() };
}

/// Tries each path on a fresh client until one starts. The error names the
/// mix format and every path tried.
fn open(id: &str, name: &str) -> std::result::Result<Opened, String> {
    let device = endpoint(id)
        .map_err(|error| format!("could not find the microphone: {}", hresult(&error)))?;
    let is_default = default_capture_id().is_some_and(|default| default == id);
    let mut failures = Vec::new();
    let mut format_text = None;
    for path in PATHS {
        if path == Path::DefaultInterfaceActivation && !is_default {
            continue;
        }
        let mut attempt = Attempt::default();
        let result = open_with(&device, id, path, &mut attempt);
        // One line per attempt with every argument it passed, to compare
        // against a standalone WASAPI call on the same endpoint (#988).
        tracing::info!(
            endpoint_id = %id,
            endpoint_name = %name,
            executable = %std::env::current_exe().map(|path| path.display().to_string()).unwrap_or_default(),
            path = ?path,
            activation = path.activation(),
            interface_path = %attempt.interface_path,
            iid = ?IAudioClient::IID,
            initialize = path.initialize(),
            apartment = %apartment(),
            fresh_client = true,
            initialize_calls_on_client = 1,
            client_properties = "none set",
            share_mode = "AUDCLNT_SHAREMODE_SHARED",
            flags = format!("{:#x}", path.flags()),
            buffer_hns = path.buffer_hns(),
            periodicity_hns = PERIODICITY_HNS,
            engine_period_frames = %attempt.engine_period,
            session_guid = "null",
            format_source = path.format_source(),
            mix_format_bytes = %attempt.mix_bytes,
            format_bytes = %attempt.format_bytes,
            is_format_supported = %attempt.is_format_supported,
            device_period_hns = %attempt.device_period,
            result = %result.as_ref().map_or_else(|failure| failure.error.as_str(), |_| "ok"),
            "wasapi initialize attempt"
        );
        match result {
            Ok(opened) => {
                tracing::info!(
                    format = %opened.format,
                    path = ?opened.path,
                    flags = format!("{:#x}", path.flags()),
                    buffer_hns = path.buffer_hns(),
                    "opened the microphone through wasapi"
                );
                return Ok(opened);
            }
            Err(failure) => {
                if let Some(format) = failure.mix {
                    format_text = Some(format.to_string());
                }
                failures.push(format!(
                    "{path:?} (flags {:#x}, buffer {} hns): {}",
                    path.flags(),
                    path.buffer_hns(),
                    failure.error
                ));
            }
        }
    }
    tracing::warn!(
        endpoint_id = %id,
        "the microphone refused every wasapi path"
    );
    Err(format!(
        "could not open the microphone through wasapi (mix format {}): {}",
        format_text.as_deref().unwrap_or("unknown"),
        failures.join("; ")
    ))
}

struct Failure {
    mix: Option<MixFormat>,
    error: String,
}

/// What one attempt observed before initializing, for its diagnostic line.
struct Attempt {
    interface_path: String,
    mix_bytes: String,
    format_bytes: String,
    is_format_supported: String,
    device_period: String,
    engine_period: String,
}

impl Default for Attempt {
    fn default() -> Self {
        let unread = || "not read".to_owned();
        Self {
            interface_path: "none".to_owned(),
            mix_bytes: unread(),
            format_bytes: unread(),
            is_format_supported: unread(),
            device_period: unread(),
            engine_period: "not used".to_owned(),
        }
    }
}

/// The calling thread's COM apartment, as `CoGetApartmentType` reports it.
pub(super) fn apartment() -> String {
    let mut kind = APTTYPE::default();
    let mut qualifier = APTTYPEQUALIFIER::default();
    match unsafe { CoGetApartmentType(&mut kind, &mut qualifier) } {
        Ok(()) => {
            let kind = match kind {
                APTTYPE_STA => "STA",
                APTTYPE_MTA => "MTA",
                APTTYPE_NA => "NA",
                APTTYPE_MAINSTA => "main STA",
                _ => "unknown",
            };
            format!("{kind} (qualifier {})", qualifier.0)
        }
        Err(error) => format!("unknown: {}", hresult(&error)),
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn open_with(
    device: &IMMDevice,
    id: &str,
    path: Path,
    attempt: &mut Attempt,
) -> std::result::Result<Opened, Failure> {
    let fail = |mix: Option<MixFormat>, step: &str, error: windows::core::Error| Failure {
        mix,
        error: format!("{step}: {}", hresult(&error)),
    };
    let client: IAudioClient = match path {
        Path::InterfaceActivation | Path::DefaultInterfaceActivation => {
            let interface_path = match path {
                Path::InterfaceActivation => endpoint_interface_path(id),
                _ => default_capture_interface()
                    .map_err(|error| fail(None, "StringFromIID", error))?,
            };
            attempt.interface_path = interface_path.clone();
            activate_interface(&interface_path)
                .map_err(|error| fail(None, "ActivateAudioInterfaceAsync", error))?
        }
        _ => unsafe { device.Activate(CLSCTX_ALL, None) }
            .map_err(|error| fail(None, "Activate", error))?,
    };
    let mix = MixFormatPtr(
        unsafe { client.GetMixFormat() }.map_err(|error| fail(None, "GetMixFormat", error))?,
    );
    if let Some(bytes) = mix.bytes() {
        attempt.mix_bytes = hex_bytes(bytes);
    }
    let mix_format = mix.read().ok_or_else(|| Failure {
        mix: None,
        error: "GetMixFormat returned an unreadable format".into(),
    })?;
    let fail = |step: &str, error: windows::core::Error| fail(Some(mix_format), step, error);
    // The auto-convert path passes its own format; every other path passes
    // the pointer GetMixFormat returned.
    let converted = (path == Path::AutoConvert)
        .then(|| float_format_bytes(1, CONVERTED_SAMPLE_RATE, SPEAKER_FRONT_CENTER));
    let (format_ptr, format) = match &converted {
        Some(bytes) => (
            bytes.as_ptr().cast::<WAVEFORMATEX>(),
            MixFormat::parse(bytes).ok_or_else(|| Failure {
                mix: Some(mix_format),
                error: "the requested format is unreadable".into(),
            })?,
        ),
        None => (mix.0.cast_const(), mix_format),
    };
    attempt.format_bytes = match &converted {
        Some(bytes) => hex_bytes(bytes),
        None => attempt.mix_bytes.clone(),
    };
    attempt.is_format_supported = is_format_supported(&client, format_ptr);
    let (mut default_period, mut minimum_period) = (0i64, 0i64);
    attempt.device_period = match unsafe {
        client.GetDevicePeriod(Some(&mut default_period), Some(&mut minimum_period))
    } {
        Ok(()) => format!("default {default_period}, minimum {minimum_period}"),
        Err(error) => hresult(&error),
    };
    if !format.is_decodable() {
        return Err(Failure {
            mix: Some(mix_format),
            error: format!("Hex cannot decode this format ({format})"),
        });
    }
    if path == Path::SharedAudioStream {
        let client3: IAudioClient3 = client
            .cast()
            .map_err(|error| fail("QueryInterface(IAudioClient3)", error))?;
        let (mut default, mut fundamental, mut minimum, mut maximum) = (0u32, 0u32, 0u32, 0u32);
        unsafe {
            client3.GetSharedModeEnginePeriod(
                format_ptr,
                &mut default,
                &mut fundamental,
                &mut minimum,
                &mut maximum,
            )
        }
        .map_err(|error| fail("GetSharedModeEnginePeriod", error))?;
        attempt.engine_period = format!(
            "default {default}, fundamental {fundamental}, minimum {minimum}, maximum {maximum}"
        );
        unsafe { client3.InitializeSharedAudioStream(path.flags(), default, format_ptr, None) }
            .map_err(|error| fail("InitializeSharedAudioStream", error))?;
    } else {
        unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                path.flags(),
                path.buffer_hns(),
                PERIODICITY_HNS,
                format_ptr,
                None,
            )
        }
        .map_err(|error| fail("Initialize", error))?;
    }
    let event = if path.polls() {
        None
    } else {
        let event = OwnedEvent(
            unsafe { CreateEventW(None, false, false, PCWSTR::null()) }
                .map_err(|error| fail("CreateEvent", error))?,
        );
        unsafe { client.SetEventHandle(event.0) }.map_err(|error| fail("SetEventHandle", error))?;
        Some(event)
    };
    let capture: IAudioCaptureClient =
        unsafe { client.GetService() }.map_err(|error| fail("GetService", error))?;
    unsafe { client.Start() }.map_err(|error| fail("Start", error))?;
    Ok(Opened {
        client,
        capture,
        format,
        path,
        event,
    })
}

fn endpoint(id: &str) -> windows::core::Result<IMMDevice> {
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
    let id = HSTRING::from(id);
    unsafe { enumerator.GetDevice(PCWSTR(id.as_ptr())) }
}

/// The current default capture endpoint's id.
fn default_capture_id() -> Option<String> {
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.ok()?;
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eCapture, eConsole) }.ok()?;
    let id = unsafe { device.GetId() }.ok()?;
    let text = unsafe { id.to_string() }.ok();
    unsafe { CoTaskMemFree(Some(id.as_ptr().cast_const().cast())) };
    text
}

/// The device interface path of the capture endpoint `id`, as
/// `MediaDevice` lists it.
fn endpoint_interface_path(id: &str) -> String {
    format!(r"\\?\SWD#MMDEVAPI#{id}#{{{DEVINTERFACE_AUDIO_CAPTURE:?}}}").to_lowercase()
}

/// `DEVINTERFACE_AUDIO_CAPTURE` as a string, which activates the default
/// capture endpoint and follows it.
fn default_capture_interface() -> windows::core::Result<String> {
    let text = unsafe { StringFromIID(&DEVINTERFACE_AUDIO_CAPTURE)? };
    let path = unsafe { text.to_string() }.map_err(|_| windows::core::Error::from(E_FAIL));
    unsafe { CoTaskMemFree(Some(text.as_ptr().cast_const().cast())) };
    path
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct ActivationDone(mpsc::Sender<windows::core::Result<IAudioClient>>);

impl IActivateAudioInterfaceCompletionHandler_Impl for ActivationDone_Impl {
    fn ActivateCompleted(
        &self,
        operation: Ref<'_, IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let result = operation.ok().and_then(|operation| {
            let mut status = HRESULT::default();
            let mut interface: Option<IUnknown> = None;
            unsafe { operation.GetActivateResult(&mut status, &mut interface)? };
            status.ok()?;
            interface
                .ok_or_else(|| windows::core::Error::from(E_FAIL))?
                .cast::<IAudioClient>()
        });
        let _ = self.0.send(result);
        Ok(())
    }
}

/// Activates an `IAudioClient` on `interface_path` through
/// `ActivateAudioInterfaceAsync`, waiting for its completion.
fn activate_interface(interface_path: &str) -> windows::core::Result<IAudioClient> {
    let (sender, done) = mpsc::channel();
    let handler: IActivateAudioInterfaceCompletionHandler = ActivationDone(sender).into();
    let interface_path = HSTRING::from(interface_path);
    unsafe {
        ActivateAudioInterfaceAsync(
            PCWSTR(interface_path.as_ptr()),
            &IAudioClient::IID,
            None,
            &handler,
        )?
    };
    done.recv_timeout(ACTIVATION_TIMEOUT)
        .map_err(|_| windows::core::Error::new(E_FAIL, "activation did not complete in time"))?
}

/// The `WAVEFORMATEX` `GetMixFormat` allocated, freed on drop.
struct MixFormatPtr(*mut WAVEFORMATEX);

impl MixFormatPtr {
    /// The `WAVEFORMATEX` and its `cbSize` extension bytes, as returned.
    fn bytes(&self) -> Option<&[u8]> {
        if self.0.is_null() {
            return None;
        }
        // SAFETY: GetMixFormat returns a valid WAVEFORMATEX followed by
        // `cbSize` bytes of extension.
        Some(unsafe {
            let extra = usize::from(std::ptr::addr_of!((*self.0).cbSize).read_unaligned());
            std::slice::from_raw_parts(
                self.0.cast::<u8>(),
                std::mem::size_of::<WAVEFORMATEX>() + extra,
            )
        })
    }

    fn read(&self) -> Option<MixFormat> {
        MixFormat::parse(self.bytes()?)
    }
}

impl Drop for MixFormatPtr {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0.cast_const().cast())) };
    }
}

/// `IsFormatSupported` for `format` in shared mode, freeing any closest
/// match it allocates.
fn is_format_supported(client: &IAudioClient, format: *const WAVEFORMATEX) -> String {
    let mut closest: *mut WAVEFORMATEX = std::ptr::null_mut();
    let result =
        unsafe { client.IsFormatSupported(AUDCLNT_SHAREMODE_SHARED, format, Some(&mut closest)) };
    let has_closest = !closest.is_null();
    drop(MixFormatPtr(closest));
    format!(
        "{:#010x}{}",
        result.0 as u32,
        if has_closest {
            " with a closest match"
        } else {
            ""
        }
    )
}

/// Delivers packets until `stop` or a WASAPI failure, which it returns.
fn capture(
    opened: &Opened,
    stop: &AtomicBool,
    feed: &mut CaptureFeed,
    chunks: &Sender<(Vec<f32>, CaptureInstant)>,
) -> std::result::Result<(), String> {
    let block = usize::from(opened.format.block_align);
    let rate = opened.format.sample_rate;
    while !stop.load(Ordering::Acquire) {
        match &opened.event {
            Some(event) => {
                unsafe { WaitForSingleObject(event.0, EVENT_WAIT_MS) };
            }
            None => std::thread::sleep(POLL_INTERVAL),
        }
        loop {
            let packet = unsafe { opened.capture.GetNextPacketSize() }
                .map_err(|error| format!("microphone stream failed: {}", hresult(&error)))?;
            if packet == 0 {
                break;
            }
            let mut data = std::ptr::null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            let mut qpc_position = 0u64;
            unsafe {
                opened.capture.GetBuffer(
                    &mut data,
                    &mut frames,
                    &mut flags,
                    None,
                    Some(&mut qpc_position),
                )
            }
            .map_err(|error| format!("microphone stream failed: {}", hresult(&error)))?;
            let frame_count = frames as usize;
            let samples = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                vec![0.0; frame_count]
            } else {
                // SAFETY: GetBuffer returned `frames` frames of `block` bytes.
                let bytes = unsafe { std::slice::from_raw_parts(data, frame_count * block) };
                opened.format.decode_mono(bytes)
            };
            unsafe { opened.capture.ReleaseBuffer(frames) }
                .map_err(|error| format!("microphone stream failed: {}", hresult(&error)))?;
            if samples.is_empty() {
                continue;
            }
            // QPC positions are in 100 ns units, as cpal's WASAPI timestamps.
            let captured_at = CaptureInstant::from_nanos(qpc_position.saturating_mul(100));
            let captured_through = captured_at
                .checked_add(duration_for_frames(samples.len(), rate))
                .unwrap_or(captured_at);
            feed.push(&samples);
            if chunks.send((samples, captured_through)).is_err() {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn hresult(error: &windows::core::Error) -> String {
    format!("{} ({:#010x})", error.message(), error.code().0 as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_interface_path_matches_media_device_ids() {
        assert_eq!(
            endpoint_interface_path("{0.0.1.00000000}.{784DE0B4-F887-4314-83B9-4CCA2BC6AC3B}"),
            r"\\?\swd#mmdevapi#{0.0.1.00000000}.{784de0b4-f887-4314-83b9-4cca2bc6ac3b}#{2eef81be-33fa-4800-9670-1cd474972c3f}"
        );
    }
}

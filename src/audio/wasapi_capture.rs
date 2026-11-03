//! A Windows microphone stream opened directly through WASAPI in the
//! endpoint's exact shared-mode mix format, with a zero buffer duration and
//! periodicity. Some drivers (Intel Smart Sound, #988) refuse every stream
//! cpal builds with E_INVALIDARG while they accept the format `GetMixFormat`
//! returns as-is, so this is the fallback when cpal's stream will not open.
//! Frames are averaged to mono here; the capture timeline resamples them.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::thread::JoinHandle;
use std::time::Duration;

use color_eyre::eyre::{Result, eyre};
use cpal::Device;
use cpal::traits::DeviceTrait;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    IAudioCaptureClient, IAudioClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
    WAVEFORMATEX,
};
use windows::Win32::System::Com::{
    APTTYPE, APTTYPE_MAINSTA, APTTYPE_MTA, APTTYPE_NA, APTTYPE_STA, APTTYPEQUALIFIER, CLSCTX_ALL,
    COINIT_MULTITHREADED, CoCreateInstance, CoGetApartmentType, CoInitializeEx, CoTaskMemFree,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::{HSTRING, Interface, PCWSTR};

use super::hex_microphone::{CaptureFeed, CaptureFeedSwitch, capture_feed};
use super::input_format::{InputStream, StartedStream};
use super::mix_format::MixFormat;
use super::{CaptureInstant, duration_for_frames};

/// How long opening may take before Hex gives up on this path.
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);
/// The polling fallback's buffer: one second, which the Intel Smart Sound
/// driver accepted without event callbacks in #988.
const POLLING_BUFFER_HNS: i64 = 10_000_000;
/// Shared mode requires a zero periodicity.
const PERIODICITY_HNS: i64 = 0;
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

/// How the stream learns that audio is ready.
#[derive(Clone, Copy, Debug)]
enum Wakeup {
    /// `AUDCLNT_STREAMFLAGS_EVENTCALLBACK` with a zero buffer duration.
    Event,
    /// No stream flags, a one-second buffer, and a polling loop.
    Polling,
}

impl Wakeup {
    fn flags(self) -> u32 {
        match self {
            Self::Event => AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
            Self::Polling => 0,
        }
    }

    fn buffer_hns(self) -> i64 {
        match self {
            Self::Event => 0,
            Self::Polling => POLLING_BUFFER_HNS,
        }
    }
}

type Ready = std::result::Result<(u32, CaptureFeedSwitch), String>;

/// Opens `device` in its mix format and starts capture.
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
            return Err(eyre!(
                "the microphone did not open in its mix format in time"
            ));
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
    wakeup: Wakeup,
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

/// Activates `id`, reads its mix format, and initializes a fresh client
/// in exactly that format, first event-driven, then polled. The error names
/// the mix format and every flag and buffer tried.
fn open(id: &str, name: &str) -> std::result::Result<Opened, String> {
    let device = endpoint(id)
        .map_err(|error| format!("could not find the microphone: {}", hresult(&error)))?;
    let mut failures = Vec::new();
    let mut format_text = None;
    for wakeup in [Wakeup::Event, Wakeup::Polling] {
        let mut attempt = Attempt::default();
        let result = open_with(&device, wakeup, &mut attempt);
        // One line per attempt with every argument Initialize received, to
        // compare against a standalone WASAPI call on the same endpoint (#988).
        tracing::info!(
            endpoint_id = %id,
            endpoint_name = %name,
            executable = %std::env::current_exe().map(|path| path.display().to_string()).unwrap_or_default(),
            activation = "IMMDevice::Activate(CLSCTX_ALL, no activation params)",
            iid = ?IAudioClient::IID,
            apartment = %apartment(),
            fresh_client = true,
            initialize_calls_on_client = 1,
            client_properties = "none set",
            share_mode = "AUDCLNT_SHAREMODE_SHARED",
            flags = format!("{:#x}", wakeup.flags()),
            buffer_hns = wakeup.buffer_hns(),
            periodicity_hns = PERIODICITY_HNS,
            session_guid = "null",
            format_source = "GetMixFormat pointer, unchanged",
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
                    wakeup = ?opened.wakeup,
                    flags = format!("{:#x}", wakeup.flags()),
                    buffer_hns = wakeup.buffer_hns(),
                    "opened the microphone in its exact mix format"
                );
                return Ok(opened);
            }
            Err(failure) => {
                if let Some(format) = failure.format {
                    format_text = Some(format.to_string());
                }
                tracing::warn!(
                    format = failure.format.map(|format| format.to_string()).unwrap_or_default(),
                    wakeup = ?wakeup,
                    flags = format!("{:#x}", wakeup.flags()),
                    buffer_hns = wakeup.buffer_hns(),
                    periodicity_hns = PERIODICITY_HNS,
                    error = %failure.error,
                    "the microphone refused its exact mix format"
                );
                failures.push(format!(
                    "{wakeup:?} (flags {:#x}, buffer {} hns): {}",
                    wakeup.flags(),
                    wakeup.buffer_hns(),
                    failure.error
                ));
            }
        }
    }
    Err(format!(
        "could not open the microphone in its mix format ({}): {}",
        format_text.as_deref().unwrap_or("unknown"),
        failures.join("; ")
    ))
}

struct Failure {
    format: Option<MixFormat>,
    error: String,
}

/// What one attempt observed before Initialize, for its diagnostic line.
struct Attempt {
    format_bytes: String,
    is_format_supported: String,
    device_period: String,
}

impl Default for Attempt {
    fn default() -> Self {
        let unread = || "not read".to_owned();
        Self {
            format_bytes: unread(),
            is_format_supported: unread(),
            device_period: unread(),
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

fn open_with(
    device: &IMMDevice,
    wakeup: Wakeup,
    attempt: &mut Attempt,
) -> std::result::Result<Opened, Failure> {
    let fail = |format: Option<MixFormat>, step: &str, error: windows::core::Error| Failure {
        format,
        error: format!("{step}: {}", hresult(&error)),
    };
    let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }
        .map_err(|error| fail(None, "Activate", error))?;
    let mix = MixFormatPtr(
        unsafe { client.GetMixFormat() }.map_err(|error| fail(None, "GetMixFormat", error))?,
    );
    if let Some(bytes) = mix.bytes() {
        attempt.format_bytes = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
    }
    let format = mix.read().ok_or_else(|| Failure {
        format: None,
        error: "GetMixFormat returned an unreadable format".into(),
    })?;
    attempt.is_format_supported = is_format_supported(&client, &mix);
    let (mut default_period, mut minimum_period) = (0i64, 0i64);
    attempt.device_period = match unsafe {
        client.GetDevicePeriod(Some(&mut default_period), Some(&mut minimum_period))
    } {
        Ok(()) => format!("default {default_period}, minimum {minimum_period}"),
        Err(error) => hresult(&error),
    };
    if !format.is_decodable() {
        return Err(Failure {
            format: Some(format),
            error: "Hex cannot decode this mix format".into(),
        });
    }
    unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            wakeup.flags(),
            wakeup.buffer_hns(),
            PERIODICITY_HNS,
            mix.0,
            None,
        )
    }
    .map_err(|error| fail(Some(format), "Initialize", error))?;
    let event = match wakeup {
        Wakeup::Event => {
            let event = OwnedEvent(
                unsafe { CreateEventW(None, false, false, PCWSTR::null()) }
                    .map_err(|error| fail(Some(format), "CreateEvent", error))?,
            );
            unsafe { client.SetEventHandle(event.0) }
                .map_err(|error| fail(Some(format), "SetEventHandle", error))?;
            Some(event)
        }
        Wakeup::Polling => None,
    };
    let capture: IAudioCaptureClient =
        unsafe { client.GetService() }.map_err(|error| fail(Some(format), "GetService", error))?;
    unsafe { client.Start() }.map_err(|error| fail(Some(format), "Start", error))?;
    Ok(Opened {
        client,
        capture,
        format,
        wakeup,
        event,
    })
}

fn endpoint(id: &str) -> windows::core::Result<IMMDevice> {
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
    let id = HSTRING::from(id);
    unsafe { enumerator.GetDevice(PCWSTR(id.as_ptr())) }
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

/// `IsFormatSupported` for the unchanged mix format in shared mode, freeing
/// any closest match it allocates.
fn is_format_supported(client: &IAudioClient, mix: &MixFormatPtr) -> String {
    let mut closest: *mut WAVEFORMATEX = std::ptr::null_mut();
    let result =
        unsafe { client.IsFormatSupported(AUDCLNT_SHAREMODE_SHARED, mix.0, Some(&mut closest)) };
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

impl Drop for MixFormatPtr {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0.cast_const().cast())) };
    }
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

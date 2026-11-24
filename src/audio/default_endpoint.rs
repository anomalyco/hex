//! Windows' default audio endpoint as one of cpal's specific devices, and the
//! notification client that says when the default changes.
//!
//! cpal 0.18 registers a default-device monitor for every stream built on a
//! default device handle, and its callbacks initialize COM as apartment
//! threaded on the Windows thread-pool threads they run on. Those threads stay
//! STA for the rest of the process, and `ActivateAudioInterfaceAsync`, which
//! only default handles use, then fails with `RPC_E_CHANGED_MODE`. A specific
//! endpoint activates through `IMMDevice::Activate` and registers no monitor,
//! so Hex resolves the default endpoint's id and picks the matching listed
//! device instead (the upstream fix, RustAudio/cpal#1413, is unmerged).

use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{
    DEVICE_STATE, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, MMDeviceEnumerator, eCapture, eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
};
use windows::core::{PCWSTR, implement};

use super::routes::{notify_input_route_changed, notify_output_route_changed};

/// How long a resolve may wait for the endpoint thread, which is idle except
/// for these calls.
const RESOLVE_TIMEOUT: Duration = Duration::from_millis(500);

/// The current default input, as one of the devices `input_devices()` lists,
/// or `None` when the endpoint or its listed device cannot be found. Never
/// cpal's default-input handle.
pub(super) fn default_input(host: &cpal::Host) -> Option<cpal::Device> {
    let id = endpoint_id(eCapture)?;
    host.input_devices()
        .ok()?
        .find(|device| listed_id(device).is_some_and(|listed| listed == id))
}

/// The current default output, as one of the devices `output_devices()` lists.
pub(super) fn default_output(host: &cpal::Host) -> Option<cpal::Device> {
    let id = endpoint_id(eRender)?;
    host.output_devices()
        .ok()?
        .find(|device| listed_id(device).is_some_and(|listed| listed == id))
}

/// The current default capture endpoint's id, which names the same endpoint
/// the direct WASAPI paths activate.
pub(super) fn default_capture_id() -> Option<String> {
    endpoint_id(eCapture)
}

fn listed_id(device: &cpal::Device) -> Option<String> {
    device.id().ok().map(|id| id.id().to_owned())
}

/// Starts the endpoint thread, once, so its notification client reports a
/// default change even while nothing resolves the default. Cheap after the
/// first call.
pub(super) fn watch() {
    let _ = requests();
}

fn endpoint_id(flow: EDataFlow) -> Option<String> {
    let requests = requests()?;
    let (reply, answer) = mpsc::sync_channel(1);
    requests.send(Request { flow, reply }).ok()?;
    answer.recv_timeout(RESOLVE_TIMEOUT).ok().flatten()
}

/// The one endpoint thread, started on first use. It owns the COM apartment
/// and the enumerator, so no Hex thread has to initialize COM to read the
/// default, and its notification client can report changes without COM calls
/// of its own.
fn requests() -> Option<&'static Sender<Request>> {
    static REQUESTS: OnceLock<Option<Sender<Request>>> = OnceLock::new();
    REQUESTS
        .get_or_init(|| {
            let (requests, receiver) = mpsc::channel();
            std::thread::Builder::new()
                .name("hex-audio-endpoints".into())
                .spawn(move || run(receiver))
                .inspect_err(|error| {
                    tracing::warn!(%error, "could not start the audio endpoint thread");
                })
                .ok()
                .map(|_| requests)
        })
        .as_ref()
}

/// One what-is-the-default request and where its answer goes.
struct Request {
    flow: EDataFlow,
    reply: SyncSender<Option<String>>,
}

fn run(requests: Receiver<Request>) {
    // SAFETY: This thread lives for the process, so its apartment does too.
    // It never hosts a stream or a window and only reads the enumerator.
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if initialized.is_err() {
        tracing::warn!(%initialized, "could not initialize COM for the audio endpoint thread");
    }
    let enumerator = unsafe {
        CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
    }
    .inspect_err(|error| tracing::warn!(%error, "could not read the default audio endpoint"))
    .ok();
    if let Some(enumerator) = &enumerator {
        watch_default(enumerator);
    }
    for Request { flow, reply } in requests {
        let id = enumerator
            .as_ref()
            .and_then(|enumerator| endpoint_id_on(enumerator, flow));
        let _ = reply.send(id);
    }
}

fn endpoint_id_on(enumerator: &IMMDeviceEnumerator, flow: EDataFlow) -> Option<String> {
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(flow, eConsole) }.ok()?;
    let id = unsafe { device.GetId() }.ok()?;
    let text = unsafe { id.to_string() }.ok();
    unsafe { CoTaskMemFree(Some(id.as_ptr().cast_const().cast())) };
    text
}

/// Registers the client that bumps the route generations on any endpoint
/// change. It is kept alive by the caller for as long as the thread answers.
fn watch_default(enumerator: &IMMDeviceEnumerator) {
    let client: IMMNotificationClient = EndpointNotifications.into();
    if let Err(error) = unsafe { enumerator.RegisterEndpointNotificationCallback(&client) } {
        tracing::warn!(%error, "could not watch the default audio endpoints");
    }
}

/// The endpoint notifications. Every callback runs on a COM notification
/// thread and only bumps a generation: no COM calls, logging, or locks, so it
/// cannot deadlock the enumerator or initialize an apartment there.
#[implement(IMMNotificationClient)]
struct EndpointNotifications;

impl IMMNotificationClient_Impl for EndpointNotifications_Impl {
    fn OnDeviceStateChanged(
        &self,
        _device_id: &PCWSTR,
        _state: DEVICE_STATE,
    ) -> windows::core::Result<()> {
        notify_both();
        Ok(())
    }

    fn OnDeviceAdded(&self, _device_id: &PCWSTR) -> windows::core::Result<()> {
        notify_both();
        Ok(())
    }

    fn OnDeviceRemoved(&self, _device_id: &PCWSTR) -> windows::core::Result<()> {
        notify_both();
        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        _device_id: &PCWSTR,
    ) -> windows::core::Result<()> {
        // The endpoint Hex resolves is the console default; the communication
        // default changes beside it.
        if role == eConsole && flow == eCapture {
            notify_input_route_changed();
        } else if role == eConsole && flow == eRender {
            notify_output_route_changed();
        }
        Ok(())
    }

    fn OnPropertyValueChanged(
        &self,
        _device_id: &PCWSTR,
        _key: &PROPERTYKEY,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

fn notify_both() {
    notify_input_route_changed();
    notify_output_route_changed();
}

//! Microphone users on Windows: the processes with an active audio session
//! on any capture device, from `IAudioSessionManager2`. Only session
//! metadata is read; no audio is captured.

use color_eyre::eyre::{Result, eyre};
use windows::Win32::Media::Audio::{
    AudioSessionStateActive, DEVICE_STATE_ACTIVE, EDataFlow, IAudioSessionControl2,
    IAudioSessionManager2, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
};
use windows::core::Interface;

use super::{ActiveMicrophoneApplication, canonical_bundle_id_for, deduplicated};

/// One active endpoint and the pids with an active session on it.
pub(crate) struct EndpointSessions {
    pub(crate) endpoint_id: String,
    pub(crate) pids: Vec<u32>,
}

/// Every active endpoint of `flow` and the pids with an active
/// (`AudioSessionStateActive`) session on it, its own pid excluded.
pub(crate) fn active_endpoint_sessions(flow: EDataFlow) -> Result<Vec<EndpointSessions>> {
    // SAFETY: Joins this thread to the multithreaded apartment. A thread
    // already in it gets `S_FALSE` and a thread already in a single-threaded
    // apartment gets `RPC_E_CHANGED_MODE`; both are fine, because the calls
    // below need no message pump and do not depend on the apartment.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let own_pid = std::process::id();
    let endpoints = (|| -> windows::core::Result<Vec<EndpointSessions>> {
        // SAFETY: Plain COM calls on interfaces this function owns. `GetId`
        // hands back a `CoTaskMemAlloc` string that is read once into a
        // `String` and freed once with `CoTaskMemFree`, never used after.
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let devices = enumerator.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE)?;
            let mut endpoints = Vec::new();
            for device in 0..devices.GetCount()? {
                let Ok(device) = devices.Item(device) else {
                    continue;
                };
                let Ok(id) = device.GetId() else {
                    continue;
                };
                let endpoint_id = id.to_string();
                CoTaskMemFree(Some(id.as_ptr().cast_const().cast()));
                let Ok(endpoint_id) = endpoint_id else {
                    continue;
                };
                let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                    continue;
                };
                let Ok(sessions) = manager.GetSessionEnumerator() else {
                    continue;
                };
                let mut pids = Vec::new();
                for session in 0..sessions.GetCount()? {
                    let Ok(control) = sessions.GetSession(session) else {
                        continue;
                    };
                    if control.GetState().ok() != Some(AudioSessionStateActive) {
                        continue;
                    }
                    if let Ok(control) = control.cast::<IAudioSessionControl2>()
                        && let Ok(pid) = control.GetProcessId()
                        && pid != 0
                        && pid != own_pid
                    {
                        pids.push(pid);
                    }
                }
                endpoints.push(EndpointSessions { endpoint_id, pids });
            }
            Ok(endpoints)
        }
    })()
    .map_err(|error| eyre!("could not read audio sessions: {error}"))?;
    Ok(endpoints)
}

/// The processes capturing from a microphone now.
pub fn active_microphone_applications() -> Result<Vec<ActiveMicrophoneApplication>> {
    let mut applications = Vec::new();
    for endpoint in active_endpoint_sessions(eCapture)? {
        for pid in endpoint.pids {
            let Some(path) = process_path(pid) else {
                continue;
            };
            let Some(executable) = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
            else {
                continue;
            };
            let name = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| executable.clone());
            applications.push(ActiveMicrophoneApplication {
                pid: pid as i32,
                bundle_id: executable,
                name,
            });
        }
    }
    Ok(deduplicated(applications))
}

/// The canonical bundle identifier of `pid`'s executable, when Hex knows it.
pub(crate) fn process_canonical_bundle_id(pid: u32) -> Option<&'static str> {
    let path = process_path(pid)?;
    let executable = path.file_name()?.to_string_lossy().into_owned();
    let name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned());
    canonical_bundle_id_for(&executable, name.as_deref())
}

fn process_path(pid: u32) -> Option<std::path::PathBuf> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    // SAFETY: The handle is checked, used once, and closed.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buffer = [0_u16; 1024];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length);
        CloseHandle(process);
        (ok != 0).then(|| String::from_utf16_lossy(&buffer[..length as usize]).into())
    }
}

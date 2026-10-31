//! Microphone users on Windows: the processes with an active audio session
//! on any capture device, from `IAudioSessionManager2`. Only session
//! metadata is read; no audio is captured.

use color_eyre::eyre::{Result, eyre};
use windows::Win32::Media::Audio::{
    AudioSessionStateActive, DEVICE_STATE_ACTIVE, IAudioSessionControl2, IAudioSessionManager2,
    IMMDeviceEnumerator, MMDeviceEnumerator, eCapture,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::core::Interface;

use super::{ActiveMicrophoneApplication, deduplicated};

/// The processes capturing from a microphone now.
pub fn active_microphone_applications() -> Result<Vec<ActiveMicrophoneApplication>> {
    // SAFETY: Joins this thread to the multithreaded apartment; a thread
    // already in it gets `S_FALSE`, which is fine.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let own_pid = std::process::id();
    let mut applications = Vec::new();
    let sessions = (|| -> windows::core::Result<Vec<u32>> {
        // SAFETY: Plain COM calls on interfaces this function owns.
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let devices = enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)?;
            let mut pids = Vec::new();
            for device in 0..devices.GetCount()? {
                let Ok(device) = devices.Item(device) else {
                    continue;
                };
                let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                    continue;
                };
                let Ok(sessions) = manager.GetSessionEnumerator() else {
                    continue;
                };
                for session in 0..sessions.GetCount()? {
                    let Ok(control) = sessions.GetSession(session) else {
                        continue;
                    };
                    if control.GetState().ok() != Some(AudioSessionStateActive) {
                        continue;
                    }
                    if let Ok(control) = control.cast::<IAudioSessionControl2>()
                        && let Ok(pid) = control.GetProcessId()
                    {
                        pids.push(pid);
                    }
                }
            }
            Ok(pids)
        }
    })()
    .map_err(|error| eyre!("could not read microphone sessions: {error}"))?;
    for pid in sessions {
        if pid == 0 || pid == own_pid {
            continue;
        }
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
    Ok(deduplicated(applications))
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

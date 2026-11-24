//! Microphone users on Windows: the processes with an active audio session
//! on any capture device, from `IAudioSessionManager2`, and the apps still
//! playing through an active render session. Only session metadata is read;
//! no audio is captured.

use color_eyre::eyre::{Result, eyre};
use windows::Win32::Media::Audio::{
    AudioSessionStateActive, DEVICE_STATE_ACTIVE, EDataFlow, IAudioSessionControl2,
    IAudioSessionManager2, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
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

/// Whether any process of the app `canonical_bundle_id` (its helpers
/// included) has an active render session: a call keeps its output open,
/// muted or not, and a hung-up call closes it with the microphone (#835).
pub(crate) fn application_running_output(canonical_bundle_id: &str) -> Result<bool> {
    Ok(active_endpoint_sessions(eRender)?
        .iter()
        .flat_map(|endpoint| endpoint.pids.iter().copied())
        .any(|pid| {
            process_canonical_bundle_id(pid)
                .is_some_and(|canonical| canonical == canonical_bundle_id)
        }))
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

/// One running process: its pid, its parent's pid, its executable's
/// lowercased file name, and when it started (Windows FILETIME ticks,
/// `None` when the process cannot be opened or a listing did not ask for
/// it), which tells a real parent from a recycled pid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RunningProcess {
    pub(crate) pid: u32,
    pub(crate) parent: u32,
    pub(crate) name: String,
    pub(crate) started: Option<u64>,
}

/// Every running process, from one Toolhelp snapshot, reading start times
/// only when `starts` says attribution needs them: listing apps does, while
/// the 2 s route check compares roots by pid and parent alone. `None` when
/// the list cannot be read.
pub(crate) fn running_processes(starts: bool) -> Option<Vec<RunningProcess>> {
    use windows::Win32::Foundation::CloseHandle;
    // SAFETY: The snapshot handle is checked, walked once, and closed; each
    // entry is a PROCESSENTRY32W this call owns.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry).is_err() {
            let _ = CloseHandle(snapshot);
            return None;
        }
        let mut processes = Vec::new();
        loop {
            processes.push(RunningProcess {
                pid: entry.th32ProcessID,
                parent: entry.th32ParentProcessID,
                name: entry_name(&entry),
                started: if starts {
                    process_started(entry.th32ProcessID)
                } else {
                    None
                },
            });
            if Process32NextW(snapshot, &mut entry).is_err() {
                break;
            }
        }
        let _ = CloseHandle(snapshot);
        Some(processes)
    }
}

/// When `pid` started, in Windows FILETIME ticks, or `None` when it cannot be
/// opened (a protected or already exited process).
fn process_started(pid: u32) -> Option<u64> {
    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: The handle is checked, used once, and closed; every FILETIME is
    // this call's own local.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut creation = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let read = GetProcessTimes(process, &mut creation, &mut exited, &mut kernel, &mut user);
        let _ = CloseHandle(process);
        read.ok()?;
        Some((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
    }
}

/// A PROCESSENTRY32W's executable file name, lowercased to compare with an
/// app id.
fn entry_name(entry: &PROCESSENTRY32W) -> String {
    let length = entry
        .szExeFile
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(entry.szExeFile.len());
    String::from_utf16_lossy(&entry.szExeFile[..length]).to_ascii_lowercase()
}

/// The processes an audio session is never attributed past: the desktop, the
/// service host, the service control manager, and the session manager. An
/// app's helper walks up to its app, and never to one of these.
const SYSTEM_EXECUTABLES: [&str; 4] =
    ["explorer.exe", "svchost.exe", "services.exe", "wininit.exe"];

/// Whether `child` is a known helper of `parent`, so its audio belongs to the
/// parent's app: a WebView2 helper such as the new Teams' or Outlook's
/// `msedgewebview2.exe` children is its host app, and a browser's renderer,
/// GPU, and utility processes run the browser's own executable.
fn is_helper_of(child: &str, parent: &str) -> bool {
    child.eq_ignore_ascii_case("msedgewebview2.exe") || child.eq_ignore_ascii_case(parent)
}

/// Whether an executable can name one app in Labs › Choose meeting audio. A
/// WebView2 helper that no host claimed would capture every WebView2 app on
/// the computer, so it never lists as an app of its own; an attributed
/// helper is already its host's executable.
pub(crate) fn capturable_application(id: &str) -> bool {
    !id.eq_ignore_ascii_case("msedgewebview2.exe")
}

/// Whether `parent` demonstrably started before `child`, so a recycled pid
/// cannot stand in for a launcher that never started it.
fn started_before(parent: Option<u64>, child: Option<u64>) -> bool {
    matches!((parent, child), (Some(parent), Some(child)) if parent < child)
}

/// The process whose executable names the app playing `pid`'s audio. A
/// process climbs to its parent only when it is a known helper of it (a
/// WebView2 helper, or the same executable as a browser's own children) and
/// the parent started first; anything else is its own app, so an app
/// launched by Chrome, Outlook, Terminal, VS Code, Steam, or Hex is not
/// mistaken for its launcher and captured as the launcher's whole tree.
/// The walk stops at a shell or system process, at a reserved pid (0 to 4,
/// the System and Idle processes), at Hex's own process, at a parent that
/// has exited, and at a cycle, so it always ends; a pid the list does not
/// hold answers itself. `own_pid` is Hex's process, which is never the app.
pub(crate) fn application_pid(pid: u32, processes: &[RunningProcess], own_pid: u32) -> u32 {
    if pid == own_pid {
        return pid;
    }
    let find = |pid: u32| processes.iter().find(|process| process.pid == pid);
    let mut app = pid;
    let mut seen = vec![pid];
    while let Some(parent) = find(app).map(|process| process.parent) {
        if parent <= 4 || parent == own_pid || seen.contains(&parent) {
            break;
        }
        let (Some(child), Some(parent_process)) = (find(app), find(parent)) else {
            break;
        };
        if SYSTEM_EXECUTABLES
            .iter()
            .any(|system| parent_process.name.eq_ignore_ascii_case(system))
        {
            break;
        }
        if !is_helper_of(&child.name, &parent_process.name)
            || !started_before(parent_process.started, child.started)
        {
            break;
        }
        seen.push(parent);
        app = parent;
    }
    app
}

/// `pid`'s executable file name, lowercased (such as `ms-teams.exe`), and
/// its file stem as a display name, for Labs › Choose meeting audio.
pub(crate) fn process_application(pid: u32) -> Option<(String, String)> {
    let path = process_path(pid)?;
    let executable = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    let name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| executable.clone());
    Some((executable, name))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Hex's process, which no audio session is ever attributed to.
    const OWN: u32 = 9_000;

    fn process(pid: u32, parent: u32, name: &str, started: u64) -> RunningProcess {
        RunningProcess {
            pid,
            parent,
            name: name.into(),
            started: Some(started),
        }
    }

    /// A call's WebView2 children are the app that plays its audio, so the
    /// pane lists Teams rather than every WebView2 tree on the machine, and
    /// the walk never climbs into the shell or the services.
    #[test]
    fn an_audio_session_belongs_to_its_top_most_ancestor_outside_the_shell() {
        let processes = [
            process(4, 0, "system", 1),
            process(200, 4, "services.exe", 2),
            process(300, 4, "explorer.exe", 3),
            process(400, 300, "ms-teams.exe", 10),
            process(401, 400, "msedgewebview2.exe", 11),
            process(402, 401, "msedgewebview2.exe", 12),
            process(500, 300, "chrome.exe", 20),
            process(501, 500, "chrome.exe", 21),
            // A service host's helper plays: the walk stops before the
            // service host, so the helper stays the app rather than the host
            // being listed.
            process(600, 4, "svchost.exe", 30),
            process(601, 600, "audiodg.exe", 31),
            // The parent already exited, and a pid the list does not hold.
            process(800, 999, "zoom.exe", 40),
        ];
        assert_eq!(application_pid(401, &processes, OWN), 400);
        assert_eq!(application_pid(402, &processes, OWN), 400);
        assert_eq!(application_pid(400, &processes, OWN), 400);
        assert_eq!(application_pid(501, &processes, OWN), 500);
        assert_eq!(
            application_pid(601, &processes, OWN),
            601,
            "the service host is never the app"
        );
        assert_eq!(application_pid(800, &processes, OWN), 800);
        assert_eq!(application_pid(900, &processes, OWN), 900);
        // Explorer itself stays the app it is, rather than climbing to
        // services.
        assert_eq!(application_pid(300, &processes, OWN), 300);
    }

    /// An app another process launched is its own app: only a known helper
    /// climbs, so a game Steam started, a site Chrome opened, a program
    /// Terminal or VS Code ran, or a helper Hex hosts is never captured as
    /// the launcher's whole tree.
    #[test]
    fn an_app_launched_by_another_app_is_its_own_app() {
        let processes = [
            process(300, 4, "explorer.exe", 1),
            process(500, 300, "chrome.exe", 10),
            process(501, 500, "discord.exe", 11),
            process(600, 300, "code.exe", 20),
            process(601, 600, "windowsterminal.exe", 21),
            process(700, 4, "steam.exe", 30),
            process(701, 700, "game.exe", 31),
            process(800, 4, "hex.exe", 40),
            process(801, 800, "msedgewebview2.exe", 41),
        ];
        assert_eq!(application_pid(501, &processes, 800), 501);
        assert_eq!(application_pid(601, &processes, 800), 601);
        assert_eq!(application_pid(701, &processes, 800), 701);
        // Hex's own process is never the app its helper belongs to.
        assert_eq!(application_pid(801, &processes, 800), 801);
        assert_eq!(application_pid(800, &processes, 800), 800);
    }

    /// A browser runs its renderer, GPU, and utility processes from its own
    /// executable, so an audio session in one of them is the browser.
    #[test]
    fn a_browser_child_sharing_its_executable_is_the_browser() {
        let processes = [
            process(300, 4, "explorer.exe", 1),
            process(500, 300, "chrome.exe", 10),
            process(501, 500, "chrome.exe", 11),
            process(502, 501, "chrome.exe", 12),
        ];
        assert_eq!(application_pid(502, &processes, OWN), 500);
        assert_eq!(application_pid(501, &processes, OWN), 500);
    }

    /// New Teams and Outlook play call audio from WebView2 helpers; the
    /// helper is its host app, so the pane lists and captures the app once.
    #[test]
    fn a_webview_helper_merges_with_the_app_that_hosts_it() {
        let processes = [
            process(300, 4, "explorer.exe", 1),
            process(400, 300, "outlook.exe", 10),
            process(401, 400, "msedgewebview2.exe", 11),
            process(402, 401, "msedgewebview2.exe", 12),
        ];
        assert_eq!(application_pid(401, &processes, OWN), 400);
        assert_eq!(application_pid(402, &processes, OWN), 400);
    }

    /// Windows reuses pids, so a parent whose process started after the child
    /// cannot have launched it; a pair with no readable start time is left
    /// apart too, because it cannot prove the parent came first.
    #[test]
    fn a_parent_that_started_after_its_child_does_not_merge() {
        let processes = [
            process(500, 4, "chrome.exe", 20),
            process(501, 500, "chrome.exe", 10),
            process(600, 4, "outlook.exe", 30),
            process(601, 600, "msedgewebview2.exe", 25),
        ];
        assert_eq!(application_pid(501, &processes, OWN), 501);
        assert_eq!(application_pid(601, &processes, OWN), 601);
        let unreadable = [
            process(700, 4, "chrome.exe", 20),
            RunningProcess {
                pid: 701,
                parent: 700,
                name: "chrome.exe".into(),
                started: None,
            },
        ];
        assert_eq!(application_pid(701, &unreadable, OWN), 701);
    }

    /// A parent chain that loops back on itself ends at the last app before
    /// the loop, so the walk never hangs.
    #[test]
    fn a_parent_cycle_stops_the_ancestor_walk() {
        let processes = [
            process(10, 11, "app.exe", 3),
            process(11, 12, "app.exe", 2),
            process(12, 10, "app.exe", 1),
        ];
        assert_eq!(application_pid(10, &processes, OWN), 12);
    }

    /// A WebView2 helper whose host is in the snapshot is that host; one
    /// without a host is never an app of its own, because choosing it would
    /// record every app that uses WebView2.
    #[test]
    fn an_unattributed_webview_helper_is_not_a_capturable_app() {
        let hosted = [
            process(300, 4, "explorer.exe", 1),
            process(400, 300, "outlook.exe", 10),
            process(401, 400, "msedgewebview2.exe", 11),
        ];
        let app = application_pid(401, &hosted, OWN);
        assert_eq!(app, 400);
        assert!(capturable_application("outlook.exe"));
        // Its host is gone, so the helper is all the snapshot has.
        let orphaned = [
            process(300, 4, "explorer.exe", 1),
            process(401, 999, "msedgewebview2.exe", 11),
        ];
        let app = application_pid(401, &orphaned, OWN);
        assert_eq!(app, 401);
        assert!(!capturable_application("msedgewebview2.exe"));
        assert!(!capturable_application("MSEDGEWEBVIEW2.EXE"));
    }

    /// A name from the snapshot is compared without regard to the case the
    /// executable was saved as.
    #[test]
    fn an_app_id_matches_its_executable_regardless_of_case() {
        let processes = [
            process(100, 4, "MS-Teams.EXE", 10),
            process(101, 100, "msedgewebview2.exe", 11),
        ];
        assert_eq!(application_pid(101, &processes, OWN), 100);
        assert_eq!(
            processes[0].name.to_ascii_lowercase(),
            "ms-teams.exe",
            "the listing lowercases it as the saved app id"
        );
    }
}

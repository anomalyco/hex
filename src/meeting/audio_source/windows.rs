//! Windows' meeting audio sources: the active render sessions' apps and the
//! listed output endpoints.

use windows::Win32::Media::Audio::eRender;

use super::SourceEntry;

/// The applications with an active render session now, each named by the
/// process that owns it: a WebView2 helper is its host app, and a browser's
/// own children share its executable, so the new Teams plays a call's audio
/// from `msedgewebview2.exe` but is listed as `ms-teams.exe` and captured
/// with its whole tree. Any other child, such as an app Chrome or Hex
/// launched, is its own app. A WebView2 helper with no host in the snapshot
/// is not listed at all: capturing `msedgewebview2.exe` would record every
/// app that uses WebView2. The id is that app's lowercase executable file
/// name, which a call keeps across updates of its display name.
pub(super) fn playing_apps() -> Vec<SourceEntry> {
    let Ok(endpoints) = crate::microphone_activity::active_endpoint_sessions(eRender) else {
        return Vec::new();
    };
    // Without a snapshot each session pid answers for itself, as before.
    // Listing reads start times, because attribution compares them.
    let processes = crate::microphone_activity::running_processes(true).unwrap_or_default();
    let own_pid = std::process::id();
    let mut apps: Vec<SourceEntry> = Vec::new();
    for pid in endpoints
        .iter()
        .flat_map(|endpoint| endpoint.pids.iter().copied())
    {
        let app = crate::microphone_activity::application_pid(pid, &processes, own_pid);
        if app == own_pid {
            continue;
        }
        let Some((id, name)) = crate::microphone_activity::process_application(app) else {
            continue;
        };
        if !crate::microphone_activity::capturable_application(&id) {
            continue;
        }
        if !apps.iter().any(|app| app.id == id) {
            apps.push(SourceEntry { id, name });
        }
    }
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps
}

/// The listed output endpoints, the default first, named exactly as the
/// capture source matches a saved output choice.
pub(super) fn output_devices() -> Vec<SourceEntry> {
    crate::meeting::os::output_choices()
        .into_iter()
        .map(|(id, name)| SourceEntry { id, name })
        .collect()
}

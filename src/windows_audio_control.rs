//! Controls other applications' audio while a dictation records: mutes or
//! lowers their WASAPI render sessions, or pauses playing media through the
//! system media-transport sessions, restoring on finish or cancel. It also
//! keeps the computer from idle sleep while the dictation records.
//!
//! All COM/WinRT work runs on a dedicated worker thread so the dictation
//! loop never blocks on audio plumbing; failures are logged and dictation
//! proceeds unaffected.

use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;

use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows::Win32::Media::Audio::{
    IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume,
    MMDeviceEnumerator, eMultimedia, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::core::Interface;

/// Block this worker thread until a WinRT async operation completes.
/// windows-future keeps its blocking `join` behind a private trait, so we
/// poll the inherent status accessor instead; these are one-shot calls on a
/// dedicated thread, never the dictation loop.
fn wait<T: windows::core::RuntimeType>(
    operation: windows_future::IAsyncOperation<T>,
) -> windows::core::Result<T> {
    while operation.Status()? == windows_future::AsyncStatus::Started {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    operation.GetResults()
}

use crate::app_settings::RecordingAudioBehavior as WhileDictating;
use crate::idle_sleep::PreventSleep;
use crate::output_ducking;

enum Command {
    Suppress,
    Restore,
    Shutdown,
}

/// Undo state for one suppression round.
enum Suppressed {
    Sessions(Vec<ISimpleAudioVolume>),
    Ducked(output_ducking::Guard),
    Media(Vec<GlobalSystemMediaTransportControlsSession>),
}

/// What one recording holds until it is restored.
struct Held {
    audio: Option<Suppressed>,
    _sleep: Option<PreventSleep>,
}

pub struct AudioSuppressor {
    commands: Sender<Command>,
    worker: Option<JoinHandle<()>>,
}

impl AudioSuppressor {
    /// A worker that applies the configured behavior and prevents idle
    /// sleep while a dictation records.
    pub fn start(mode: WhileDictating) -> Self {
        let (commands, receiver) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("windows-audio-suppressor".into())
            .spawn(move || {
                unsafe {
                    // Balanced implicitly at thread exit; MTA also covers the
                    // agile WinRT media-session objects.
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                }
                let mut held: Option<Held> = None;
                while let Ok(command) = receiver.recv() {
                    match command {
                        Command::Suppress if held.is_none() => {
                            held = Some(Held {
                                audio: match mode {
                                    WhileDictating::Mute => mute_other_sessions(),
                                    WhileDictating::Duck => duck_other_sessions(),
                                    WhileDictating::PauseMedia => pause_playing_media(),
                                    WhileDictating::DoNothing => None,
                                },
                                _sleep: crate::idle_sleep::prevent_sleep(),
                            });
                        }
                        Command::Suppress => {}
                        Command::Restore => restore(held.take()),
                        Command::Shutdown => {
                            restore(held.take());
                            return;
                        }
                    }
                }
            });
        if let Err(error) = &worker {
            tracing::warn!(%error, "could not start Windows audio control");
        }
        Self {
            commands,
            worker: worker.ok(),
        }
    }

    pub fn suppress(&self) {
        let _ = self.commands.send(Command::Suppress);
    }

    pub fn restore(&self) {
        let _ = self.commands.send(Command::Restore);
    }
}

impl Drop for AudioSuppressor {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Every other process's render session on the default output.
fn other_sessions() -> windows::core::Result<Vec<ISimpleAudioVolume>> {
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)? };
    let manager: IAudioSessionManager2 = unsafe { device.Activate(CLSCTX_ALL, None)? };
    let sessions = unsafe { manager.GetSessionEnumerator()? };
    let count = unsafe { sessions.GetCount()? };
    let own_pid = std::process::id();
    let mut volumes = Vec::new();
    for index in 0..count {
        let Ok(control) = (unsafe { sessions.GetSession(index) }) else {
            continue;
        };
        if let Ok(control2) = control.cast::<IAudioSessionControl2>()
            && let Ok(pid) = unsafe { control2.GetProcessId() }
            && pid == own_pid
        {
            continue;
        }
        if let Ok(volume) = control.cast::<ISimpleAudioVolume>() {
            volumes.push(volume);
        }
    }
    Ok(volumes)
}

/// Mute every other process's render session on the default output,
/// remembering only the sessions this round actually muted.
fn mute_other_sessions() -> Option<Suppressed> {
    let muted = other_sessions().map(|sessions| {
        sessions
            .into_iter()
            .filter(|volume| {
                let already_muted =
                    unsafe { volume.GetMute() }.map_or(true, |mute| mute.as_bool());
                !already_muted && unsafe { volume.SetMute(true, std::ptr::null()) }.is_ok()
            })
            .collect::<Vec<_>>()
    });
    match muted {
        Ok(muted) if muted.is_empty() => None,
        Ok(muted) => Some(Suppressed::Sessions(muted)),
        Err(error) => {
            tracing::warn!(%error, "could not mute other audio sessions");
            None
        }
    }
}

/// Lower every other process's render session, so Hex's own tones stay
/// at full volume. Sessions that start while recording are left alone.
fn duck_other_sessions() -> Option<Suppressed> {
    output_ducking::Guard::start(crate::app_settings::recording_duck_volume(), || {
        unsafe {
            // The ramp runs on its own thread, balanced at its exit.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        match other_sessions() {
            Ok(sessions) => Some(Sessions(sessions)),
            Err(error) => {
                tracing::warn!(%error, "could not lower other audio sessions");
                None
            }
        }
    })
    .map(Suppressed::Ducked)
}

/// Other apps' session volumes, lowered and restored together.
struct Sessions(Vec<ISimpleAudioVolume>);

impl output_ducking::Volume for Sessions {
    fn read(&self) -> Option<Vec<f32>> {
        self.0
            .iter()
            .map(|volume| unsafe { volume.GetMasterVolume() }.ok())
            .collect()
    }

    fn write(&mut self, values: &[f32]) -> bool {
        self.0.iter().zip(values).fold(true, |written, (volume, &value)| {
            written
                & unsafe { volume.SetMasterVolume(value.clamp(0.0, 1.0), std::ptr::null()) }
                    .is_ok()
        })
    }
}

/// Pause every currently-playing system media session, remembering which
/// ones to resume.
fn pause_playing_media() -> Option<Suppressed> {
    let paused = (|| -> windows::core::Result<Vec<GlobalSystemMediaTransportControlsSession>> {
        let manager = wait(GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?)?;
        let mut paused = Vec::new();
        for session in manager.GetSessions()? {
            let playing = session
                .GetPlaybackInfo()
                .and_then(|info| info.PlaybackStatus())
                == Ok(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing);
            if playing
                && session
                    .TryPauseAsync()
                    .map(wait)
                    .and_then(|paused| paused)
                    .unwrap_or(false)
            {
                paused.push(session);
            }
        }
        Ok(paused)
    })();
    match paused {
        Ok(paused) if paused.is_empty() => None,
        Ok(paused) => Some(Suppressed::Media(paused)),
        Err(error) => {
            tracing::warn!(%error, "could not pause playing media");
            None
        }
    }
}

fn restore(held: Option<Held>) {
    match held.and_then(|held| held.audio) {
        None => {}
        Some(Suppressed::Ducked(mut guard)) => guard.restore(),
        Some(Suppressed::Sessions(sessions)) => {
            for volume in sessions {
                if let Err(error) = unsafe { volume.SetMute(false, std::ptr::null()) } {
                    tracing::warn!(%error, "could not unmute an audio session");
                }
            }
        }
        Some(Suppressed::Media(sessions)) => {
            for session in sessions {
                let resumed = session.TryPlayAsync().and_then(wait).unwrap_or(false);
                if !resumed {
                    tracing::warn!("could not resume a paused media session");
                }
            }
        }
    }
}

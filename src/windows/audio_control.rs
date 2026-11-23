//! Controls other applications' audio while a dictation records: mutes or
//! lowers their WASAPI render sessions, or pauses playing media through the
//! system media-transport sessions, restoring on finish or cancel. It also
//! keeps the computer from idle sleep while the dictation records.
//!
//! All COM/WinRT work runs on a dedicated worker thread so the dictation
//! loop never blocks on audio plumbing; failures are logged and dictation
//! proceeds unaffected.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

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
use crate::audio::effect_holds::{Effect, Hold, SessionRequest, next_session_id, serve_sessions};
use crate::audio::idle_sleep::PreventSleep;
use crate::audio::output_ducking;
use crate::audio::output_route;

/// Undo state for one suppression round.
enum Suppressed {
    Sessions(Vec<ISimpleAudioVolume>),
    Ducked(output_ducking::Guard),
    Media(Vec<GlobalSystemMediaTransportControlsSession>),
}

/// One held environment effect; dropping it restores what it changed.
enum HeldEffect {
    Sleep(#[allow(dead_code)] Option<PreventSleep>),
    Audio(Option<Suppressed>),
}

impl Drop for HeldEffect {
    fn drop(&mut self) {
        if let Self::Audio(audio) = self {
            restore(audio.take());
        }
    }
}

fn hold_effect(effect: Effect) -> HeldEffect {
    match effect {
        Effect::Sleep => HeldEffect::Sleep(crate::audio::idle_sleep::prevent_sleep()),
        Effect::Mute => HeldEffect::Audio(suppress(WhileDictating::Mute)),
        Effect::Duck => HeldEffect::Audio(suppress(WhileDictating::Duck)),
        Effect::Pause => HeldEffect::Audio(suppress(WhileDictating::PauseMedia)),
    }
}

/// One shared COM MTA worker owns every held Windows audio/sleep effect so
/// overlapping sessions (dictation + Annotate capture) compose cleanly and
/// never double-duck or restore prematurely.
fn shared_worker() -> Option<Sender<SessionRequest>> {
    static WORKER: OnceLock<Option<Sender<SessionRequest>>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            let (requests, receiver) = mpsc::channel();
            let spawned = std::thread::Builder::new()
                .name("windows-audio-suppressor".into())
                .spawn(move || {
                    unsafe {
                        // Balanced implicitly at thread exit; MTA also covers the
                        // agile WinRT media-session objects.
                        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                    }
                    serve_sessions(receiver, Duration::ZERO, hold_effect);
                });
            if let Err(error) = &spawned {
                tracing::warn!(%error, "could not start Windows audio control");
            }
            spawned.ok().map(|_| requests)
        })
        .clone()
}

pub struct AudioSuppressor {
    id: u64,
    hold: Hold,
    requests: Option<Sender<SessionRequest>>,
    active: AtomicBool,
}

impl AudioSuppressor {
    /// "While dictating" for a capture, which does not prevent idle sleep.
    pub fn new(mode: WhileDictating) -> Self {
        Self::for_hold(Hold::capture(mode))
    }

    /// "While dictating" for a dictation, which also prevents idle sleep.
    pub fn start(mode: WhileDictating) -> Self {
        Self::for_hold(Hold::dictation(mode))
    }

    fn for_hold(hold: Hold) -> Self {
        if hold.is_empty() {
            return Self {
                id: next_session_id(),
                hold,
                requests: None,
                active: AtomicBool::new(false),
            };
        }
        Self {
            id: next_session_id(),
            hold,
            requests: shared_worker(),
            active: AtomicBool::new(false),
        }
    }

    #[cfg(test)]
    fn with_requests_for_test(hold: Hold, requests: Sender<SessionRequest>) -> Self {
        Self {
            id: next_session_id(),
            hold,
            requests: (!hold.is_empty()).then_some(requests),
            active: AtomicBool::new(false),
        }
    }

    pub fn suppress(&self) {
        if let Some(requests) = &self.requests {
            self.active.store(true, Ordering::Release);
            let _ = requests.send(SessionRequest::Suppress {
                id: self.id,
                hold: self.hold,
            });
        }
    }

    pub fn restore(&self) {
        if let Some(requests) = &self.requests {
            self.active.store(false, Ordering::Release);
            let _ = requests.send(SessionRequest::Restore { id: self.id });
        }
    }
}

impl Drop for AudioSuppressor {
    fn drop(&mut self) {
        let Some(requests) = &self.requests else {
            return;
        };
        if self.active.swap(false, Ordering::AcqRel) {
            let (ack, done) = mpsc::sync_channel(1);
            if requests
                .send(SessionRequest::Drop {
                    id: self.id,
                    ack: Some(ack),
                })
                .is_ok()
            {
                let _ = done.recv();
            }
        } else {
            let _ = requests.send(SessionRequest::Drop {
                id: self.id,
                ack: None,
            });
        }
    }
}

/// What one recording changes: `mode` ("While dictating"), or nothing when
/// Labs › Only on speakers is on and the default output is headphones. The
/// route is read once per recording, here on the worker thread, so the
/// dictation loop never waits on cpal; with the lab off it is not read at
/// all. Idle sleep is not part of this.
fn suppress(mode: WhileDictating) -> Option<Suppressed> {
    if mode == WhileDictating::DoNothing {
        return None;
    }
    if crate::labs::speakers_only() && !output_route::applies(output_route::default_output_route())
    {
        tracing::info!("left other audio alone while dictating on headphones");
        return None;
    }
    match mode {
        WhileDictating::Mute => mute_other_sessions(),
        WhileDictating::Duck => duck_other_sessions(),
        WhileDictating::PauseMedia => pause_playing_media(),
        WhileDictating::DoNothing => None,
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
                let already_muted = unsafe { volume.GetMute() }.map_or(true, |mute| mute.as_bool());
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
        self.0
            .iter()
            .zip(values)
            .fold(true, |written, (volume, &value)| {
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

fn restore(audio: Option<Suppressed>) {
    match audio {
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    fn barrier(requests: &Sender<SessionRequest>) {
        let (ack, done) = mpsc::sync_channel(1);
        requests.send(SessionRequest::Barrier(ack)).unwrap();
        done.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn capture_do_nothing_starts_no_worker_and_never_prevents_sleep() {
        let capture = AudioSuppressor::new(WhileDictating::DoNothing);
        assert!(capture.requests.is_none());
        assert!(!capture.hold.wants(Effect::Sleep));
        capture.suppress();
        capture.restore();

        let capture_duck = AudioSuppressor::with_requests_for_test(
            Hold::capture(WhileDictating::Duck),
            mpsc::channel().0,
        );
        assert!(!capture_duck.hold.wants(Effect::Sleep));
        assert!(capture_duck.hold.wants(Effect::Duck));
    }

    #[test]
    fn overlapping_dictation_and_capture_restore_original_volume() {
        let values = Arc::new(Mutex::new(vec![1.0_f32, 0.75]));
        let effects = Arc::new(Mutex::new(Vec::new()));
        struct SharedVolume(Arc<Mutex<Vec<f32>>>);
        impl output_ducking::Volume for SharedVolume {
            fn read(&self) -> Option<Vec<f32>> {
                Some(self.0.lock().unwrap().clone())
            }
            fn write(&mut self, next: &[f32]) -> bool {
                *self.0.lock().unwrap() = next.to_vec();
                true
            }
        }
        let (requests, receiver) = mpsc::channel();
        let worker_values = values.clone();
        let worker_effects = effects.clone();
        let worker = std::thread::spawn(move || {
            serve_sessions(receiver, Duration::ZERO, move |effect| {
                worker_effects.lock().unwrap().push(effect);
                if effect == Effect::Duck {
                    let volume = SharedVolume(worker_values.clone());
                    output_ducking::Guard::start(0.2, move || Some(volume))
                } else {
                    None
                }
            });
        });

        let dictation = AudioSuppressor::with_requests_for_test(
            Hold::dictation(WhileDictating::Duck),
            requests.clone(),
        );
        let capture = AudioSuppressor::with_requests_for_test(
            Hold::capture(WhileDictating::Duck),
            requests.clone(),
        );

        dictation.suppress();
        barrier(&requests);
        std::thread::sleep(Duration::from_millis(200));
        assert!((values.lock().unwrap()[0] - 0.2).abs() < 0.02);

        // Capture overlaps: Duck is not re-acquired, Sleep is not touched again.
        capture.suppress();
        barrier(&requests);
        assert_eq!(*effects.lock().unwrap(), vec![Effect::Sleep, Effect::Duck]);
        assert!((values.lock().unwrap()[0] - 0.2).abs() < 0.02);

        // Dictation ends first; volume stays lowered for the capture.
        dictation.restore();
        barrier(&requests);
        assert!((values.lock().unwrap()[0] - 0.2).abs() < 0.02);

        // Dropping capture synchronously restores the original 1.0/0.75 volume.
        drop(capture);
        let restored = values.lock().unwrap().clone();
        assert!((restored[0] - 1.0).abs() < 0.01, "got {restored:?}");
        assert!((restored[1] - 0.75).abs() < 0.01, "got {restored:?}");

        drop(dictation);
        drop(requests);
        worker.join().unwrap();
    }
}

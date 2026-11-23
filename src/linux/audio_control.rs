//! Mutes, lowers, or pauses other audio while a Linux dictation records,
//! and keeps the computer from idle sleep. Muting uses PipeWire's `wpctl`,
//! or PulseAudio's `pactl`; lowering ramps the default sink's channels with
//! `pactl` (or its one volume with `wpctl`); pausing asks every playing
//! MPRIS player over the session D-Bus. All restore only what they changed.
//! A worker thread does the work so the dictation loop never waits on a
//! slow player or logind.

use std::process::Command;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use zbus::{Connection, OwnedVariant};

use crate::app_settings::RecordingAudioBehavior;
use crate::audio::effect_holds::{Effect, Hold, SessionRequest, next_session_id, serve_sessions};
use crate::audio::idle_sleep::PreventSleep;
use crate::audio::output_ducking;
use crate::audio::output_route;

const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const DBUS_TIMEOUT: Duration = Duration::from_millis(500);
const RESTORE_GRACE: Duration = Duration::from_millis(300);

/// One shared worker thread owns every held Linux audio/sleep effect so
/// overlapping sessions (dictation + Annotate capture) compose cleanly and
/// never double-duck or restore prematurely.
fn shared_worker() -> Option<Sender<SessionRequest>> {
    static WORKER: OnceLock<Option<Sender<SessionRequest>>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            let (requests, receiver) = mpsc::channel();
            let spawned = std::thread::Builder::new()
                .name("linux-audio-control".into())
                .spawn(move || serve_sessions(receiver, RESTORE_GRACE, hold_effect));
            if let Err(error) = &spawned {
                tracing::warn!(%error, "could not start Linux audio control");
            }
            spawned.ok().map(|_| requests)
        })
        .clone()
}

pub(crate) struct AudioSuppressor {
    id: u64,
    hold: Hold,
    requests: Option<Sender<SessionRequest>>,
}

impl AudioSuppressor {
    /// "While dictating" for a capture, which does not prevent sleep.
    pub(crate) fn new(behavior: RecordingAudioBehavior) -> Self {
        Self::start(Hold::capture(behavior))
    }

    /// "While dictating" for a dictation, which also prevents idle sleep.
    pub(crate) fn dictation(behavior: RecordingAudioBehavior) -> Self {
        Self::start(Hold::dictation(behavior))
    }

    fn start(hold: Hold) -> Self {
        if hold.is_empty() {
            return Self {
                id: next_session_id(),
                hold,
                requests: None,
            };
        }
        Self {
            id: next_session_id(),
            hold,
            requests: shared_worker(),
        }
    }

    #[cfg(test)]
    fn with_requests_for_test(hold: Hold, requests: Sender<SessionRequest>) -> Self {
        Self {
            id: next_session_id(),
            hold,
            requests: (!hold.is_empty()).then_some(requests),
        }
    }

    pub(crate) fn suppress(&self) {
        if let Some(requests) = &self.requests {
            let _ = requests.send(SessionRequest::Suppress {
                id: self.id,
                hold: self.hold,
            });
        }
    }

    pub(crate) fn restore(&self) {
        if let Some(requests) = &self.requests {
            let _ = requests.send(SessionRequest::Restore { id: self.id });
        }
    }

    /// Restores what recording changed and runs `then` once a muted or
    /// lowered output is audible again (or at once if another session still
    /// holds it), so a tone it plays is heard. Anything else runs `then` at
    /// once, never behind a slow media player.
    pub(crate) fn restore_then(&self, then: impl FnOnce() + Send + 'static) {
        let Some(requests) = &self.requests else {
            return then();
        };
        if let Err(mpsc::SendError(SessionRequest::RestoreThen { then, .. })) =
            requests.send(SessionRequest::RestoreThen {
                id: self.id,
                then: Box::new(then),
            })
        {
            then();
        }
    }
}

impl Drop for AudioSuppressor {
    fn drop(&mut self) {
        if let Some(requests) = &self.requests {
            let _ = requests.send(SessionRequest::Drop {
                id: self.id,
                ack: None,
            });
        }
    }
}

/// One held environment effect; dropping it restores what it changed.
enum HeldEffect {
    Sleep(#[allow(dead_code)] Option<PreventSleep>),
    Audio(Suppressed),
}

impl Drop for HeldEffect {
    fn drop(&mut self) {
        if let Self::Audio(audio) = self {
            restore_audio(std::mem::replace(audio, Suppressed::Nothing));
        }
    }
}

fn hold_effect(effect: Effect) -> HeldEffect {
    match effect {
        Effect::Sleep => HeldEffect::Sleep(crate::audio::idle_sleep::prevent_sleep()),
        Effect::Mute => HeldEffect::Audio(suppress(RecordingAudioBehavior::Mute)),
        Effect::Duck => HeldEffect::Audio(suppress(RecordingAudioBehavior::Duck)),
        Effect::Pause => HeldEffect::Audio(suppress(RecordingAudioBehavior::PauseMedia)),
    }
}

enum Suppressed {
    Muted(Mixer),
    Ducked(output_ducking::Guard),
    Paused(Vec<String>),
    Nothing,
}

fn suppress(behavior: RecordingAudioBehavior) -> Suppressed {
    if behavior == RecordingAudioBehavior::DoNothing {
        return Suppressed::Nothing;
    }
    // Read once per recording, here on the worker thread, so the dictation
    // loop never waits on the sound server. With Labs › Only on speakers
    // on, headphones leave other audio alone for this recording; idle sleep
    // is not part of this. With the lab off nothing runs pactl.
    if crate::labs::speakers_only() && !output_route::applies(output_route::default_output_route())
    {
        tracing::info!("left other audio alone while dictating on headphones");
        return Suppressed::Nothing;
    }
    match behavior {
        RecordingAudioBehavior::Mute => match Mixer::find() {
            Some(mixer) if mixer.mute(true) => Suppressed::Muted(mixer),
            Some(_) => Suppressed::Nothing,
            None => {
                tracing::warn!("cannot mute: neither wpctl nor pactl is installed");
                Suppressed::Nothing
            }
        },
        RecordingAudioBehavior::Duck => output_ducking::Guard::start(
            crate::app_settings::recording_duck_volume(),
            SinkVolume::find,
        )
        .map_or(Suppressed::Nothing, Suppressed::Ducked),
        RecordingAudioBehavior::PauseMedia => Suppressed::Paused(pause_players()),
        RecordingAudioBehavior::DoNothing => Suppressed::Nothing,
    }
}

fn restore_audio(audio: Suppressed) {
    match audio {
        Suppressed::Muted(mixer) => {
            mixer.mute(false);
        }
        Suppressed::Ducked(mut guard) => guard.restore(),
        Suppressed::Paused(players) => resume_players(&players),
        Suppressed::Nothing => {}
    }
}

/// The default sink's volume for lowering: each channel through `pactl`,
/// which keeps the balance, else the one volume `wpctl` reports.
enum SinkVolume {
    Pactl,
    Wpctl,
}

impl SinkVolume {
    fn find() -> Option<Self> {
        [Self::Pactl, Self::Wpctl]
            .into_iter()
            .find(|volume| output_ducking::Volume::read(volume).is_some())
    }
}

/// pactl's raw volume for 100%.
const PA_VOLUME_NORM: f32 = 65536.0;

impl output_ducking::Volume for SinkVolume {
    fn read(&self) -> Option<Vec<f32>> {
        match self {
            Self::Pactl => {
                parse_pactl_volume(&run("pactl", &["get-sink-volume", "@DEFAULT_SINK@"])?)
            }
            Self::Wpctl => {
                parse_wpctl_volume(&run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])?)
                    .map(|volume| vec![volume])
            }
        }
    }

    fn write(&mut self, values: &[f32]) -> bool {
        match self {
            Self::Pactl => {
                let raw = values
                    .iter()
                    .map(|value| ((value * PA_VOLUME_NORM).round() as u32).to_string())
                    .collect::<Vec<_>>();
                let mut args = vec!["set-sink-volume", "@DEFAULT_SINK@"];
                args.extend(raw.iter().map(String::as_str));
                run("pactl", &args).is_some()
            }
            Self::Wpctl => values.first().is_some_and(|value| {
                run(
                    "wpctl",
                    &["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{value:.4}")],
                )
                .is_some()
            }),
        }
    }
}

/// Each channel's raw volume from `pactl get-sink-volume`, such as
/// `Volume: front-left: 32768 /  50% / -18.06 dB,   front-right: ...`.
fn parse_pactl_volume(output: &str) -> Option<Vec<f32>> {
    let line = output
        .lines()
        .find(|line| line.trim_start().starts_with("Volume:"))?;
    let values = line
        .trim_start()
        .trim_start_matches("Volume:")
        .split(',')
        .map(|channel| {
            let (_, rest) = channel.split_once(':')?;
            let raw: u32 = rest.split('/').next()?.trim().parse().ok()?;
            Some(raw as f32 / PA_VOLUME_NORM)
        })
        .collect::<Option<Vec<_>>>()?;
    (!values.is_empty()).then_some(values)
}

/// The volume from `wpctl get-volume`, such as `Volume: 0.47 [MUTED]`.
fn parse_wpctl_volume(output: &str) -> Option<f32> {
    output
        .trim()
        .strip_prefix("Volume:")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
        .filter(|volume: &f32| volume.is_finite() && *volume >= 0.0)
}

/// The command-line mixer for the default output.
#[derive(Clone, Copy)]
enum Mixer {
    Wpctl,
    Pactl,
}

impl Mixer {
    fn find() -> Option<Self> {
        [Self::Wpctl, Self::Pactl]
            .into_iter()
            .find(|mixer| mixer.muted().is_some())
    }

    fn muted(self) -> Option<bool> {
        let output = match self {
            Self::Wpctl => run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])?,
            Self::Pactl => run("pactl", &["get-sink-mute", "@DEFAULT_SINK@"])?,
        };
        Some(match self {
            Self::Wpctl => output.contains("[MUTED]"),
            Self::Pactl => output.contains("yes"),
        })
    }

    /// Mutes or unmutes the default output. Muting reports whether this
    /// changed anything, so an already muted output is never unmuted later.
    fn mute(self, mute: bool) -> bool {
        if mute && self.muted() != Some(false) {
            return false;
        }
        let value = if mute { "1" } else { "0" };
        match self {
            Self::Wpctl => run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", value]),
            Self::Pactl => run("pactl", &["set-sink-mute", "@DEFAULT_SINK@", value]),
        }
        .is_some()
    }
}

fn run(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Runs MPRIS calls on the worker thread, which blocks on them anyway.
fn with_session_bus<T: Default>(work: impl AsyncFnOnce(&Connection) -> T) -> T {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::warn!(%error, "cannot start the D-Bus runtime");
            return T::default();
        }
    };
    runtime.block_on(async {
        match tokio::time::timeout(DBUS_TIMEOUT, Connection::session()).await {
            Ok(Ok(bus)) => work(&bus).await,
            Ok(Err(error)) => {
                tracing::warn!(%error, "cannot reach the session D-Bus");
                T::default()
            }
            Err(_) => {
                tracing::warn!("the session D-Bus did not answer");
                T::default()
            }
        }
    })
}

async fn call<B, R>(
    bus: &Connection,
    destination: &str,
    path: &str,
    interface: &str,
    method: &str,
    body: &B,
) -> Option<R>
where
    B: serde::Serialize + zbus::Type,
    R: for<'d> serde::Deserialize<'d> + zbus::Type,
{
    let call = bus.call_method(Some(destination), path, Some(interface), method, body);
    let reply = tokio::time::timeout(DBUS_TIMEOUT, call).await.ok()?.ok()?;
    reply.body().deserialize().ok()
}

/// Pauses every MPRIS player that is playing, and returns their bus names.
fn pause_players() -> Vec<String> {
    with_session_bus(async |bus| {
        let names: Vec<String> = call(
            bus,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "ListNames",
            &(),
        )
        .await
        .unwrap_or_default();
        let mut paused = Vec::new();
        for name in names
            .into_iter()
            .filter(|name| name.starts_with(MPRIS_PREFIX))
        {
            if playback_status(bus, &name).await.as_deref() == Some("Playing")
                && call::<_, ()>(bus, &name, MPRIS_PATH, MPRIS_PLAYER, "Pause", &())
                    .await
                    .is_some()
            {
                paused.push(name);
            }
        }
        paused
    })
}

async fn playback_status(bus: &Connection, name: &str) -> Option<String> {
    let status: OwnedVariant = call(
        bus,
        name,
        MPRIS_PATH,
        "org.freedesktop.DBus.Properties",
        "Get",
        &(MPRIS_PLAYER, "PlaybackStatus"),
    )
    .await?;
    <&str>::try_from(&status).ok().map(str::to_owned)
}

fn resume_players(players: &[String]) {
    if players.is_empty() {
        return;
    }
    with_session_bus(async |bus| {
        for name in players {
            let _ = call::<_, ()>(bus, name, MPRIS_PATH, MPRIS_PLAYER, "Play", &()).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sink_volumes_parse_per_channel() {
        assert_eq!(
            parse_pactl_volume(
                "Volume: front-left: 32768 /  50% / -18.06 dB,   front-right: 65536 / 100% / 0.00 dB\n        balance 0.00\n"
            ),
            Some(vec![0.5, 1.0])
        );
        assert_eq!(parse_pactl_volume("Connection failure"), None);
        assert_eq!(parse_wpctl_volume("Volume: 0.47\n"), Some(0.47));
        assert_eq!(parse_wpctl_volume("Volume: 0.25 [MUTED]\n"), Some(0.25));
    }

    #[test]
    fn doing_nothing_starts_no_worker() {
        let suppressor = AudioSuppressor::new(RecordingAudioBehavior::DoNothing);
        assert!(suppressor.requests.is_none());
        suppressor.suppress();
        suppressor.restore();
        let (ran, done) = mpsc::channel();
        suppressor.restore_then(move || ran.send(()).unwrap());
        assert!(done.try_recv().is_ok());
    }

    fn barrier(requests: &Sender<SessionRequest>) {
        let (ack, done) = mpsc::sync_channel(1);
        requests.send(SessionRequest::Barrier(ack)).unwrap();
        done.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn restoring_without_a_mute_runs_its_callback_first() {
        use std::sync::{Arc, Mutex};
        let events = Arc::new(Mutex::new(Vec::new()));
        let (requests, receiver) = mpsc::channel();
        let worker_events = events.clone();
        struct Logged(Effect, Arc<Mutex<Vec<&'static str>>>);
        impl Drop for Logged {
            fn drop(&mut self) {
                if self.0 == Effect::Pause {
                    self.1.lock().unwrap().push("resume");
                }
            }
        }
        let worker = std::thread::spawn(move || {
            serve_sessions(receiver, RESTORE_GRACE, move |effect| {
                Logged(effect, worker_events.clone())
            });
        });

        let suppressor = AudioSuppressor::with_requests_for_test(
            Hold::capture(RecordingAudioBehavior::PauseMedia),
            requests.clone(),
        );
        suppressor.suppress();
        let cb_events = events.clone();
        suppressor.restore_then(move || cb_events.lock().unwrap().push("then"));
        let idle_events = events.clone();
        suppressor.restore_then(move || idle_events.lock().unwrap().push("idle"));
        barrier(&requests);
        drop(suppressor);
        drop(requests);
        worker.join().unwrap();
        assert_eq!(*events.lock().unwrap(), ["then", "resume", "idle"]);
    }

    #[test]
    fn back_to_back_restore_and_suppress_stay_suppressed() {
        use std::sync::{Arc, Mutex};
        let log = Arc::new(Mutex::new(Vec::new()));
        let (requests, receiver) = mpsc::channel();
        let worker_log = log.clone();
        struct Logged(Arc<Mutex<Vec<&'static str>>>);
        impl Drop for Logged {
            fn drop(&mut self) {
                self.0.lock().unwrap().push("restore");
            }
        }
        let worker = std::thread::spawn(move || {
            serve_sessions(receiver, RESTORE_GRACE, move |_| {
                worker_log.lock().unwrap().push("suppress");
                Logged(worker_log.clone())
            });
        });
        let suppressor = AudioSuppressor::with_requests_for_test(
            Hold::capture(RecordingAudioBehavior::Mute),
            requests.clone(),
        );
        // Double-tap: the first tap's restore is followed at once by a start.
        suppressor.suppress();
        suppressor.restore();
        suppressor.suppress();
        std::thread::sleep(RESTORE_GRACE * 2);
        barrier(&requests);
        assert_eq!(*log.lock().unwrap(), ["suppress"]);
        // A lone restore applies once the grace period passes.
        suppressor.restore();
        std::thread::sleep(RESTORE_GRACE * 2);
        barrier(&requests);
        assert_eq!(*log.lock().unwrap(), ["suppress", "restore"]);
        // A later recording suppresses again.
        suppressor.suppress();
        drop(suppressor);
        drop(requests);
        worker.join().unwrap();
        assert_eq!(
            *log.lock().unwrap(),
            ["suppress", "restore", "suppress", "restore"]
        );
    }

    #[test]
    fn overlapping_dictation_and_capture_restore_original_volume() {
        use std::sync::{Arc, Mutex};
        let values = Arc::new(Mutex::new(vec![1.0_f32, 0.75]));
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
        let worker = std::thread::spawn(move || {
            serve_sessions(receiver, Duration::ZERO, move |effect| {
                if effect == Effect::Duck {
                    let volume = SharedVolume(worker_values.clone());
                    output_ducking::Guard::start(0.2, move || Some(volume))
                } else {
                    None
                }
            });
        });

        let dictation = AudioSuppressor::with_requests_for_test(
            Hold::dictation(RecordingAudioBehavior::Duck),
            requests.clone(),
        );
        let capture = AudioSuppressor::with_requests_for_test(
            Hold::capture(RecordingAudioBehavior::Duck),
            requests.clone(),
        );

        dictation.suppress();
        barrier(&requests);
        std::thread::sleep(Duration::from_millis(200));
        assert!((values.lock().unwrap()[0] - 0.2).abs() < 0.02);

        capture.suppress();
        barrier(&requests);
        // Overlapping capture must not duck 20% down to 4%.
        assert!((values.lock().unwrap()[0] - 0.2).abs() < 0.02);

        // Dictation ends first; volume stays lowered for the capture.
        dictation.restore();
        barrier(&requests);
        assert!((values.lock().unwrap()[0] - 0.2).abs() < 0.02);

        // Capture drops; original volume (1.0, 0.75) is restored cleanly.
        drop(capture);
        barrier(&requests);
        let restored = values.lock().unwrap().clone();
        assert!((restored[0] - 1.0).abs() < 0.01, "got {restored:?}");
        assert!((restored[1] - 0.75).abs() < 0.01, "got {restored:?}");

        drop(dictation);
        drop(requests);
        worker.join().unwrap();
    }
}

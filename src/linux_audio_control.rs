//! Mutes or pauses other audio while a Linux dictation records. Muting
//! uses PipeWire's `wpctl`, or PulseAudio's `pactl`; pausing asks every
//! playing MPRIS player over the session D-Bus. Both restore only what
//! they changed. A worker thread does the work so the dictation loop never
//! waits on a slow player.

use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use zbus::{Connection, OwnedVariant};

use crate::app_settings::RecordingAudioBehavior;

const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const DBUS_TIMEOUT: Duration = Duration::from_millis(500);
const RESTORE_GRACE: Duration = Duration::from_millis(300);

enum Request {
    Suppress,
    Restore,
    /// Restores, running the callback once the output is audible again.
    RestoreThen(Box<dyn FnOnce() + Send>),
}

pub(crate) struct AudioSuppressor {
    requests: Option<Sender<Request>>,
}

impl AudioSuppressor {
    pub(crate) fn new(behavior: RecordingAudioBehavior) -> Self {
        if behavior == RecordingAudioBehavior::DoNothing {
            return Self { requests: None };
        }
        let (requests, receiver) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("linux-audio-control".into())
            .spawn(move || serve(receiver, || suppress(behavior), restore, restore_then));
        if let Err(error) = &spawned {
            tracing::warn!(%error, "could not start Linux audio control");
        }
        Self {
            requests: spawned.ok().map(|_| requests),
        }
    }

    pub(crate) fn suppress(&self) {
        if let Some(requests) = &self.requests {
            let _ = requests.send(Request::Suppress);
        }
    }

    pub(crate) fn restore(&self) {
        if let Some(requests) = &self.requests {
            let _ = requests.send(Request::Restore);
        }
    }

    /// Restores what recording changed and runs `then` once a muted output
    /// is unmuted, so a tone it plays is heard. Anything else runs `then` at
    /// once, never behind a slow media player.
    pub(crate) fn restore_then(&self, then: impl FnOnce() + Send + 'static) {
        let Some(requests) = &self.requests else {
            return then();
        };
        if let Err(mpsc::SendError(Request::RestoreThen(then))) =
            requests.send(Request::RestoreThen(Box::new(then)))
        {
            then();
        }
    }
}

/// Applies requests in order. A double-tap restores and suppresses back to
/// back, before wpctl or the player reports the restored state, so a plain
/// restore waits briefly and is dropped when another recording starts.
fn serve<S>(
    receiver: Receiver<Request>,
    mut suppress: impl FnMut() -> S,
    mut restore: impl FnMut(S),
    mut restore_then: impl FnMut(Option<S>, Box<dyn FnOnce() + Send>),
) {
    let mut suppressed: Option<S> = None;
    let mut next = None;
    loop {
        let request = match next.take() {
            Some(request) => request,
            None => match receiver.recv() {
                Ok(request) => request,
                Err(_) => break,
            },
        };
        match request {
            Request::Suppress if suppressed.is_none() => suppressed = Some(suppress()),
            Request::Suppress => {}
            Request::Restore if suppressed.is_some() => {
                match receiver.recv_timeout(RESTORE_GRACE) {
                    Ok(Request::Suppress) => {}
                    Ok(request @ Request::RestoreThen(_)) => next = Some(request),
                    Ok(Request::Restore) | Err(RecvTimeoutError::Timeout) => {
                        restore(suppressed.take().unwrap());
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            Request::Restore => {}
            Request::RestoreThen(then) => restore_then(suppressed.take(), then),
        }
    }
    if let Some(suppressed) = suppressed {
        restore(suppressed);
    }
}

enum Suppressed {
    Muted(Mixer),
    Paused(Vec<String>),
    Nothing,
}

fn suppress(behavior: RecordingAudioBehavior) -> Suppressed {
    match behavior {
        RecordingAudioBehavior::Mute => match Mixer::find() {
            Some(mixer) if mixer.mute(true) => Suppressed::Muted(mixer),
            Some(_) => Suppressed::Nothing,
            None => {
                tracing::warn!("cannot mute: neither wpctl nor pactl is installed");
                Suppressed::Nothing
            }
        },
        RecordingAudioBehavior::PauseMedia => Suppressed::Paused(pause_players()),
        RecordingAudioBehavior::DoNothing | RecordingAudioBehavior::Duck => Suppressed::Nothing,
    }
}

fn restore(suppressed: Suppressed) {
    match suppressed {
        Suppressed::Muted(mixer) => {
            mixer.mute(false);
        }
        Suppressed::Paused(players) => resume_players(&players),
        Suppressed::Nothing => {}
    }
}

fn restore_then(suppressed: Option<Suppressed>, then: Box<dyn FnOnce() + Send>) {
    match suppressed {
        Some(muted @ Suppressed::Muted(_)) => {
            restore(muted);
            then();
        }
        other => {
            then();
            if let Some(other) = other {
                restore(other);
            }
        }
    }
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
    fn doing_nothing_starts_no_worker() {
        let suppressor = AudioSuppressor::new(RecordingAudioBehavior::DoNothing);
        assert!(suppressor.requests.is_none());
        suppressor.suppress();
        suppressor.restore();
        let (ran, done) = mpsc::channel();
        suppressor.restore_then(move || ran.send(()).unwrap());
        assert!(done.try_recv().is_ok());
    }

    #[test]
    fn restoring_without_a_mute_runs_its_callback_first() {
        let (order, events) = mpsc::channel();
        let ran = order.clone();
        restore_then(
            Some(Suppressed::Paused(Vec::new())),
            Box::new(move || ran.send("then").unwrap()),
        );
        restore_then(None, Box::new(move || order.send("idle").unwrap()));
        assert_eq!(events.try_iter().collect::<Vec<_>>(), ["then", "idle"]);
    }

    #[test]
    fn back_to_back_restore_and_suppress_stay_suppressed() {
        use std::sync::{Arc, Mutex};
        let log = Arc::new(Mutex::new(Vec::new()));
        let (requests, receiver) = mpsc::channel();
        let (suppress_log, restore_log) = (log.clone(), log.clone());
        let worker = std::thread::spawn(move || {
            serve(
                receiver,
                move || suppress_log.lock().unwrap().push("suppress"),
                move |()| restore_log.lock().unwrap().push("restore"),
                |_, then: Box<dyn FnOnce() + Send>| then(),
            )
        });
        // Double-tap: the first tap's restore is followed at once by a start.
        for request in [Request::Suppress, Request::Restore, Request::Suppress] {
            requests.send(request).unwrap();
        }
        std::thread::sleep(RESTORE_GRACE * 2);
        assert_eq!(*log.lock().unwrap(), ["suppress"]);
        // A lone restore applies once the grace period passes.
        requests.send(Request::Restore).unwrap();
        std::thread::sleep(RESTORE_GRACE * 2);
        assert_eq!(*log.lock().unwrap(), ["suppress", "restore"]);
        // A later recording suppresses again.
        requests.send(Request::Suppress).unwrap();
        drop(requests);
        worker.join().unwrap();
        assert_eq!(
            *log.lock().unwrap(),
            ["suppress", "restore", "suppress", "restore"]
        );
    }
}

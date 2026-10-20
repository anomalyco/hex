//! Mutes or pauses other audio while a Linux dictation records. Muting
//! uses PipeWire's `wpctl`, or PulseAudio's `pactl`; pausing asks every
//! playing MPRIS player over the session D-Bus. Both restore only what
//! they changed. A worker thread does the work so the dictation loop never
//! waits on a slow player.

use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use gtk::gio;
use gtk::glib::{self, ToVariant};

use crate::app_settings::RecordingAudioBehavior;

const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const DBUS_TIMEOUT_MS: i32 = 500;
const RESTORE_GRACE: Duration = Duration::from_millis(300);

enum Request {
    Suppress,
    Restore,
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
            .spawn(move || serve(receiver, || suppress(behavior), restore));
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
}

/// Applies requests in order. A double-tap restores and suppresses back to
/// back, before wpctl or the player reports the restored state, so a restore
/// waits briefly and is dropped when another recording starts.
fn serve<S>(
    receiver: Receiver<Request>,
    mut suppress: impl FnMut() -> S,
    mut restore: impl FnMut(S),
) {
    let mut suppressed: Option<S> = None;
    while let Ok(request) = receiver.recv() {
        match request {
            Request::Suppress if suppressed.is_none() => suppressed = Some(suppress()),
            Request::Restore if suppressed.is_some() => {
                match receiver.recv_timeout(RESTORE_GRACE) {
                    Ok(Request::Suppress) => {}
                    Ok(Request::Restore) | Err(RecvTimeoutError::Timeout) => {
                        restore(suppressed.take().unwrap());
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            Request::Restore | Request::Suppress => {}
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
        RecordingAudioBehavior::DoNothing => Suppressed::Nothing,
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

fn session_bus() -> Option<gio::DBusConnection> {
    gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
        .inspect_err(|error| tracing::warn!(%error, "cannot reach the session D-Bus"))
        .ok()
}

fn call(
    bus: &gio::DBusConnection,
    destination: &str,
    path: &str,
    interface: &str,
    method: &str,
    parameters: Option<&glib::Variant>,
) -> Option<glib::Variant> {
    bus.call_sync(
        Some(destination),
        path,
        interface,
        method,
        parameters,
        None,
        gio::DBusCallFlags::NONE,
        DBUS_TIMEOUT_MS,
        gio::Cancellable::NONE,
    )
    .ok()
}

/// Pauses every MPRIS player that is playing, and returns their bus names.
fn pause_players() -> Vec<String> {
    let Some(bus) = session_bus() else {
        return Vec::new();
    };
    let Some(names) = call(
        &bus,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "ListNames",
        None,
    )
    .and_then(|reply| reply.child_value(0).get::<Vec<String>>()) else {
        return Vec::new();
    };
    names
        .into_iter()
        .filter(|name| name.starts_with(MPRIS_PREFIX))
        .filter(|name| playback_status(&bus, name).as_deref() == Some("Playing"))
        .filter(|name| call(&bus, name, MPRIS_PATH, MPRIS_PLAYER, "Pause", None).is_some())
        .collect()
}

fn playback_status(bus: &gio::DBusConnection, name: &str) -> Option<String> {
    let reply = call(
        bus,
        name,
        MPRIS_PATH,
        "org.freedesktop.DBus.Properties",
        "Get",
        Some(&(MPRIS_PLAYER, "PlaybackStatus").to_variant()),
    )?;
    reply.child_value(0).as_variant()?.get::<String>()
}

fn resume_players(players: &[String]) {
    if players.is_empty() {
        return;
    }
    let Some(bus) = session_bus() else {
        return;
    };
    for name in players {
        let _ = call(&bus, name, MPRIS_PATH, MPRIS_PLAYER, "Play", None);
    }
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

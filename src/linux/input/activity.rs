//! Whether the person used the keyboard or pointer between two dictation
//! pastes on Wayland, which ends a paste continuation
//! ([`crate::continuation`]).
//!
//! Hex reads no input devices there, so it asks the compositor when the seat
//! goes quiet and when it wakes (`ext_idle_notifier_v1`, at version 2 for the
//! notification idle inhibitors don't silence). That gives bursts of input
//! without saying what was pressed, and Hex's own shortcut and paste keys are
//! bursts too. A burst that begins and ends at Hex's own input is Hex's; any
//! other is the person's and ends the continuation.
//!
//! X11, and a desktop without the protocol, report nothing: dictations paste
//! there without joining.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_seat::{self, WlSeat};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{
    self, ExtIdleNotificationV1,
};
use wayland_protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::{
    self, ExtIdleNotifierV1,
};

use crate::continuation::InputActivity;
use crate::linux::session::LinuxSession;

/// How long the seat is quiet before the compositor ends a burst.
const QUIET: Duration = Duration::from_millis(250);
/// How far a burst's first or last input may be from the moment Hex noted
/// its own: a shortcut reaches Hex through the portal or a `hex` command
/// after the compositor saw the key.
const SLACK: Duration = Duration::from_millis(200);
/// How many of Hex's own inputs are remembered; a burst holds a few.
const OWN_KEPT: usize = 16;
/// The first `ext_idle_notifier_v1` with the input-only notification.
const INPUT_VERSION: u32 = 2;

static WATCH: OnceLock<Option<Arc<Watch>>> = OnceLock::new();

struct Watch {
    bursts: Mutex<Bursts>,
    /// Cleared when the compositor stops answering.
    watching: AtomicBool,
}

/// Starts watching the seat, once; later calls do nothing.
pub(crate) fn start() {
    WATCH.get_or_init(|| {
        if !LinuxSession::detect().is_wayland() {
            return None;
        }
        watch()
            .inspect_err(|why| {
                tracing::info!(why, "back-to-back dictations will not join on this desktop");
            })
            .ok()
    });
}

/// Notes Hex's own input: a shortcut event, or keys it is sending.
pub(crate) fn own() {
    if let Some(Some(watch)) = WATCH.get() {
        watch.bursts().own(Instant::now());
    }
}

/// Runs `send`, which sends Hex's own keys.
pub(crate) fn sending<T>(send: impl FnOnce() -> T) -> T {
    own();
    let sent = send();
    own();
    sent
}

/// The input revision a paste belongs to, or `None` when nothing reports
/// the person's input.
pub(crate) fn revision() -> Option<u64> {
    let watch = WATCH.get()?.as_ref()?;
    watch
        .watching
        .load(Ordering::Acquire)
        .then(|| watch.bursts().revision(Instant::now()))
}

impl Watch {
    fn bursts(&self) -> std::sync::MutexGuard<'_, Bursts> {
        self.bursts.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The seat's bursts of input, and which of them were Hex's own.
#[derive(Default)]
struct Bursts {
    activity: InputActivity,
    /// When the burst in progress began; `None` while the seat is quiet.
    began: Option<Instant>,
    /// Hex's own recent input, oldest first.
    own: VecDeque<Instant>,
}

impl Bursts {
    fn own(&mut self, now: Instant) {
        if self.own.len() == OWN_KEPT {
            self.own.pop_front();
        }
        self.own.push_back(now);
    }

    fn resumed(&mut self, now: Instant) {
        self.began = Some(now);
    }

    /// The seat went quiet, [`QUIET`] after the burst's last input.
    fn idled(&mut self, now: Instant) {
        let Some(began) = self.began.take() else {
            return;
        };
        let ended = now.checked_sub(QUIET).map_or(began, |at| at.max(began));
        if !(self.is_own(began) && self.is_own(ended)) {
            self.activity.invalidate();
        }
    }

    /// The revision for a paste at `now`. A burst still in progress that
    /// the person began counts already, and is judged from here on, so the
    /// paste's own keys in it don't end the continuation this paste starts.
    fn revision(&mut self, now: Instant) -> u64 {
        if self.began.is_some_and(|began| !self.is_own(began)) {
            self.activity.invalidate();
            self.began = Some(now);
        }
        self.activity.revision()
    }

    fn is_own(&self, at: Instant) -> bool {
        self.own.iter().any(|own| {
            let apart = at
                .checked_duration_since(*own)
                .unwrap_or_else(|| own.duration_since(at));
            apart <= SLACK
        })
    }
}

fn watch() -> Result<Arc<Watch>, String> {
    let connection =
        Connection::connect_to_env().map_err(|error| format!("no Wayland connection: {error}"))?;
    let (globals, mut queue) = registry_queue_init::<Watcher>(&connection)
        .map_err(|error| format!("no registry: {error}"))?;
    let qh = queue.handle();
    let seat = globals
        .bind::<WlSeat, Watcher, ()>(&qh, 1..=1, ())
        .map_err(|_| "the desktop offers no seat".to_owned())?;
    let notifier = globals
        .bind::<ExtIdleNotifierV1, Watcher, ()>(&qh, INPUT_VERSION..=INPUT_VERSION, ())
        .map_err(|_| {
            format!("the desktop offers no ext_idle_notifier_v1 version {INPUT_VERSION}")
        })?;
    notifier.get_input_idle_notification(QUIET.as_millis() as u32, &seat, &qh, ());
    let watch = Arc::new(Watch {
        bursts: Mutex::new(Bursts {
            // The compositor reports the seat quiet first, so input from
            // before this moment reads as a burst the person began.
            began: Some(Instant::now()),
            ..Bursts::default()
        }),
        watching: AtomicBool::new(true),
    });
    let mut watcher = Watcher(watch.clone());
    thread::Builder::new()
        .name("hex-input-activity".to_owned())
        .spawn(move || {
            // The queue reads from this connection for the thread's lifetime.
            let _connection = connection;
            loop {
                if let Err(error) = queue.blocking_dispatch(&mut watcher) {
                    watcher.0.watching.store(false, Ordering::Release);
                    tracing::warn!(%error, "the compositor stopped reporting input; dictations no longer join");
                    return;
                }
            }
        })
        .map_err(|error| format!("could not start the input watcher: {error}"))?;
    Ok(watch)
}

struct Watcher(Arc<Watch>);

impl Dispatch<ExtIdleNotificationV1, ()> for Watcher {
    fn event(
        watcher: &mut Self,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let now = Instant::now();
        match event {
            ext_idle_notification_v1::Event::Resumed => watcher.0.bursts().resumed(now),
            ext_idle_notification_v1::Event::Idled => watcher.0.bursts().idled(now),
            _ => {}
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for Watcher {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for Watcher {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtIdleNotifierV1, ()> for Watcher {
    fn event(
        _: &mut Self,
        _: &ExtIdleNotifierV1,
        _: ext_idle_notifier_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::continuation::Continuation;

    /// One dictation pasted and recorded, with the seat quiet again after
    /// the paste's own keys.
    struct Session {
        bursts: Bursts,
        continuation: Continuation,
        now: Instant,
    }

    impl Session {
        fn after_a_dictation() -> Self {
            let mut session = Self {
                bursts: Bursts::default(),
                continuation: Continuation::default(),
                now: Instant::now(),
            };
            session.paste("Hello there.");
            session.quiet();
            session
        }

        fn wait(&mut self, milliseconds: u64) {
            self.now += Duration::from_millis(milliseconds);
        }

        /// Input reaches the compositor: it resumes a quiet seat.
        fn input(&mut self) {
            if self.bursts.began.is_none() {
                self.bursts.resumed(self.now);
            }
        }

        /// The person types or clicks.
        fn person(&mut self) {
            self.input();
        }

        /// A Hex shortcut: the key, then Hex hearing of it.
        fn shortcut(&mut self) {
            self.input();
            self.wait(20);
            self.bursts.own(self.now);
        }

        /// The seat goes quiet after its last input.
        fn quiet(&mut self) {
            self.now += QUIET;
            self.bursts.idled(self.now);
        }

        fn paste(&mut self, text: &str) -> String {
            let revision = self.bursts.revision(self.now);
            let text = self
                .continuation
                .join(revision, true, text)
                .unwrap_or_else(|| text.to_owned());
            self.bursts.own(self.now);
            self.wait(10);
            self.input();
            self.wait(10);
            self.bursts.own(self.now);
            self.continuation.record(revision, true, text.clone());
            text
        }

        /// Holds the shortcut for a dictation and pastes what it heard.
        fn dictate(&mut self, text: &str) -> String {
            self.shortcut();
            self.quiet();
            self.wait(2_000);
            self.shortcut();
            self.wait(150);
            self.paste(text)
        }
    }

    #[test]
    fn back_to_back_dictations_join() {
        let mut session = Session::after_a_dictation();
        session.wait(3_000);
        assert_eq!(session.dictate("And again"), " And again");
        session.quiet();
        session.wait(3_000);
        assert_eq!(session.dictate("and once more."), " and once more.");
    }

    #[test]
    fn the_persons_input_between_dictations_ends_the_join() {
        let mut session = Session::after_a_dictation();
        session.wait(3_000);
        session.person();
        session.quiet();
        session.wait(3_000);
        assert_eq!(session.dictate("And again"), "And again");
    }

    #[test]
    fn input_just_before_the_shortcut_ends_the_join() {
        let mut session = Session::after_a_dictation();
        session.wait(3_000);
        session.person();
        session.wait(230);
        assert_eq!(session.dictate("And again"), "And again");
    }

    #[test]
    fn input_while_dictating_ends_the_join() {
        let mut session = Session::after_a_dictation();
        session.wait(3_000);
        session.shortcut();
        session.wait(230);
        session.person();
        session.quiet();
        session.wait(2_000);
        session.shortcut();
        session.wait(150);
        assert_eq!(session.paste("And again"), "And again");
    }

    #[test]
    fn input_still_in_progress_at_the_paste_ends_the_join() {
        let mut session = Session::after_a_dictation();
        session.wait(3_000);
        session.shortcut();
        session.quiet();
        session.wait(2_000);
        session.shortcut();
        session.quiet();
        session.person();
        session.wait(100);
        assert_eq!(session.paste("And again"), "And again");
        // That burst ends at the paste's own keys, so the next one joins.
        session.quiet();
        session.wait(3_000);
        assert_eq!(session.dictate("and more"), " and more");
    }

    #[test]
    fn input_after_the_paste_in_its_burst_ends_the_join() {
        let mut session = Session {
            bursts: Bursts::default(),
            continuation: Continuation::default(),
            now: Instant::now(),
        };
        session.paste("Hello there.");
        session.wait(230);
        session.person();
        session.quiet();
        session.wait(3_000);
        assert_eq!(session.dictate("And again"), "And again");
    }

    /// Manual probe of the real compositor, hands off the keyboard and mouse:
    /// `cargo test the_compositor_reports_input -- --ignored`. Needs `wtype`.
    #[test]
    #[ignore]
    fn the_compositor_reports_input() {
        let key = || {
            let sent = std::process::Command::new("wtype")
                .args(["-M", "shift", "-m", "shift"])
                .status();
            assert!(sent.is_ok_and(|status| status.success()));
        };
        let settle = || thread::sleep(QUIET * 3);
        start();
        settle();
        let before = revision().expect("the compositor offers the input idle notification");

        sending(key);
        settle();
        assert_eq!(revision(), Some(before), "Hex's own keys ended the join");

        key();
        settle();
        assert_ne!(revision(), Some(before), "other input did not end the join");
    }
}

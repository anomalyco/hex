//! Pointer and keyboard input on the capture clock: events a host forwards from its own tap
//! through a bounded queue, and the clicks of the core's optional tap.
//!
//! Keys are kept only as `⌘`/`⌃` chords by default, never as typing, and every key event is dropped
//! while macOS reports secure event input (a password field has focus).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cursor::{Collectors, CursorButton, CursorButtonState, Point, RawCursorButtonEvent};
use crate::time::{HostTime, signed_millis};

const DRAIN_POLL: Duration = Duration::from_millis(50);

/// The modifiers held with an event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifiers {
    pub command: bool,
    pub control: bool,
    pub option: bool,
    pub shift: bool,
}

/// One input event, in global top-left points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputEvent {
    Button {
        button: CursorButton,
        down: bool,
        x: f64,
        y: f64,
        modifiers: Modifiers,
    },
    /// The pointer moved, with or without a button held.
    Moved {
        x: f64,
        y: f64,
        modifiers: Modifiers,
    },
    Scroll {
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
    },
    ModifiersChanged {
        x: f64,
        y: f64,
        modifiers: Modifiers,
    },
    /// A key press: its name on the user's layout, never the text it types.
    Key {
        name: Option<String>,
        modifiers: Modifiers,
        autorepeat: bool,
    },
}

/// Which key events the log keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyPolicy {
    /// Fresh presses held with `⌘` or `⌃`. Keys without them are typing and are dropped.
    #[default]
    Chords,
    /// No key events at all.
    Off,
}

/// An event the host saw at `at` on the host clock.
#[derive(Clone, Debug, PartialEq)]
pub struct HostEvent {
    pub at: HostTime,
    pub event: InputEvent,
}

/// One logged event, `t_ms` from the capture's clock anchor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecordedInput {
    pub t_ms: i64,
    pub host_time: u64,
    #[serde(flatten)]
    pub event: InputEvent,
}

/// The host's end of the input queue. Sending never blocks: a full queue drops the event and
/// counts it.
#[derive(Clone)]
pub struct HostInputSender {
    sender: SyncSender<HostEvent>,
    keys: KeyPolicy,
    dropped: Arc<AtomicU64>,
    secure: fn() -> bool,
}

/// The core's end, handed over in [`Input::Host`](crate::Input::Host).
pub struct HostInputReceiver {
    receiver: Receiver<HostEvent>,
    keys: KeyPolicy,
    dropped: Arc<AtomicU64>,
    secure: fn() -> bool,
}

/// A bounded input queue holding at most `capacity` events.
#[must_use]
pub fn host_channel(capacity: usize, keys: KeyPolicy) -> (HostInputSender, HostInputReceiver) {
    channel_with(capacity, keys, secure_input_enabled)
}

fn channel_with(
    capacity: usize,
    keys: KeyPolicy,
    secure: fn() -> bool,
) -> (HostInputSender, HostInputReceiver) {
    let (sender, receiver) = mpsc::sync_channel(capacity.max(1));
    let dropped = Arc::new(AtomicU64::new(0));
    (
        HostInputSender {
            sender,
            keys,
            dropped: Arc::clone(&dropped),
            secure,
        },
        HostInputReceiver {
            receiver,
            keys,
            dropped,
            secure,
        },
    )
}

impl HostInputSender {
    /// Queues one event. Keys the policy rejects, and every key while secure input is on, never
    /// enter the queue. Returns false once the recording no longer takes input.
    pub fn send(&self, at: HostTime, event: InputEvent) -> bool {
        let Some(event) = filter(event, self.keys, self.secure) else {
            return true;
        };
        match self.sender.try_send(HostEvent { at, event }) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                true
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    /// Events dropped because the queue was full.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// Applies the key policy and the secure-input rule. Non-key events pass unchanged.
#[must_use]
pub fn filter_key(event: InputEvent, keys: KeyPolicy, secure_input: bool) -> Option<InputEvent> {
    let InputEvent::Key {
        modifiers,
        autorepeat,
        ..
    } = &event
    else {
        return Some(event);
    };
    if secure_input || *autorepeat || keys == KeyPolicy::Off {
        return None;
    }
    (modifiers.command || modifiers.control).then_some(event)
}

fn filter(event: InputEvent, keys: KeyPolicy, secure: fn() -> bool) -> Option<InputEvent> {
    let secure_input = matches!(event, InputEvent::Key { .. }) && secure();
    filter_key(event, keys, secure_input)
}

/// Whether macOS has secure event input on, as it does while a password field has focus.
#[must_use]
pub fn secure_input_enabled() -> bool {
    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn IsSecureEventInputEnabled() -> u8;
    }
    // SAFETY: a side-effect-free query with no arguments.
    unsafe { IsSecureEventInputEnabled() != 0 }
}

/// The recording's input log, shared by the host drain and the core's tap.
pub(crate) struct InputLog {
    anchor: HostTime,
    events: Mutex<Vec<RecordedInput>>,
    collectors: Collectors,
}

impl InputLog {
    pub(crate) fn new(anchor: HostTime, collectors: Collectors) -> Arc<Self> {
        Arc::new(Self {
            anchor,
            events: Mutex::new(Vec::new()),
            collectors,
        })
    }

    pub(crate) fn collectors(&self) -> &Collectors {
        &self.collectors
    }

    pub(crate) fn record(&self, at: HostTime, event: InputEvent) {
        if at < self.anchor {
            return;
        }
        if let InputEvent::Button {
            button, down, x, y, ..
        } = &event
        {
            self.collectors.observe_click(RawCursorButtonEvent {
                host_time: at.ticks(),
                point: Point { x: *x, y: *y },
                button: *button,
                state: if *down {
                    CursorButtonState::Pressed
                } else {
                    CursorButtonState::Released
                },
            });
        }
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(RecordedInput {
                t_ms: signed_millis(self.anchor, at),
                host_time: at.ticks(),
                event,
            });
    }

    /// The log in time order.
    pub(crate) fn take(&self) -> Vec<RecordedInput> {
        let mut events =
            std::mem::take(&mut *self.events.lock().unwrap_or_else(PoisonError::into_inner));
        events.sort_by_key(|event| event.host_time);
        events
    }
}

/// Drains the host's queue into the log for as long as the recording runs.
pub(crate) struct HostInputDrain {
    stopping: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl HostInputDrain {
    pub(crate) fn start(receiver: HostInputReceiver, log: Arc<InputLog>) -> std::io::Result<Self> {
        let stopping = Arc::new(AtomicBool::new(false));
        let dropped = Arc::clone(&receiver.dropped);
        let thread_stopping = Arc::clone(&stopping);
        let thread = thread::Builder::new()
            .name("capture-host-input".into())
            .spawn(move || {
                let deliver = |event: HostEvent| {
                    if let Some(input) = filter(event.event, receiver.keys, receiver.secure) {
                        log.record(event.at, input);
                    }
                };
                loop {
                    match receiver.receiver.recv_timeout(DRAIN_POLL) {
                        Ok(event) => deliver(event),
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                    if thread_stopping.load(Ordering::Acquire) {
                        while let Ok(event) = receiver.receiver.try_recv() {
                            deliver(event);
                        }
                        break;
                    }
                }
            })?;
        Ok(Self {
            stopping,
            dropped,
            thread: Some(thread),
        })
    }

    /// Delivers what is queued, then joins. Returns the count the host's queue dropped.
    pub(crate) fn stop(&mut self) -> u64 {
        self.stopping.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::warn!("host input drain panicked");
        }
        self.dropped.load(Ordering::Relaxed)
    }
}

impl Drop for HostInputDrain {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The capture-clock button event for a mouse transition from the core's tap.
pub(crate) fn tap_button(
    button: CursorButton,
    state: CursorButtonState,
    x: f64,
    y: f64,
    modifiers: Modifiers,
) -> InputEvent {
    InputEvent::Button {
        button,
        down: state == CursorButtonState::Pressed,
        x,
        y,
        modifiers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursor::{CursorCollector, Rect};

    const COMMAND: Modifiers = Modifiers {
        command: true,
        control: false,
        option: false,
        shift: false,
    };
    const SHIFT: Modifiers = Modifiers {
        command: false,
        control: false,
        option: false,
        shift: true,
    };

    fn key(name: &str, modifiers: Modifiers) -> InputEvent {
        InputEvent::Key {
            name: Some(name.into()),
            modifiers,
            autorepeat: false,
        }
    }

    fn moved() -> InputEvent {
        InputEvent::Moved {
            x: 1.0,
            y: 2.0,
            modifiers: Modifiers::default(),
        }
    }

    #[test]
    fn keys_are_chords_only_by_default() {
        assert!(filter_key(key("S", COMMAND), KeyPolicy::Chords, false).is_some());
        let control = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        assert!(filter_key(key("C", control), KeyPolicy::Chords, false).is_some());
        assert_eq!(filter_key(key("a", SHIFT), KeyPolicy::Chords, false), None);
        assert_eq!(
            filter_key(key("a", Modifiers::default()), KeyPolicy::Chords, false),
            None
        );
        let option = Modifiers {
            option: true,
            ..Modifiers::default()
        };
        assert_eq!(filter_key(key("å", option), KeyPolicy::Chords, false), None);
    }

    #[test]
    fn autorepeats_and_the_off_policy_drop_keys() {
        let repeat = InputEvent::Key {
            name: Some("Z".into()),
            modifiers: COMMAND,
            autorepeat: true,
        };
        assert_eq!(filter_key(repeat, KeyPolicy::Chords, false), None);
        assert_eq!(filter_key(key("S", COMMAND), KeyPolicy::Off, false), None);
        assert_eq!(filter_key(moved(), KeyPolicy::Off, true), Some(moved()));
    }

    #[test]
    fn secure_input_drops_every_key_but_not_the_pointer() {
        assert_eq!(filter_key(key("V", COMMAND), KeyPolicy::Chords, true), None);
        assert_eq!(filter_key(moved(), KeyPolicy::Chords, true), Some(moved()));

        let (sender, receiver) = channel_with(8, KeyPolicy::Chords, || true);
        assert!(sender.send(HostTime::from_ticks(1), key("V", COMMAND)));
        assert!(sender.send(HostTime::from_ticks(2), moved()));
        let queued = receiver.receiver.try_iter().collect::<Vec<_>>();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].event, moved());
    }

    #[test]
    fn a_full_queue_drops_and_counts_without_blocking() {
        let (sender, receiver) = channel_with(2, KeyPolicy::Chords, || false);
        let started = std::time::Instant::now();
        for tick in 0..100 {
            assert!(sender.send(HostTime::from_ticks(tick), moved()));
        }
        assert!(started.elapsed() < Duration::from_millis(50));
        assert_eq!(sender.dropped(), 98);
        drop(receiver);
        assert!(!sender.send(HostTime::from_ticks(101), moved()));
    }

    #[test]
    fn drain_logs_on_the_capture_clock_and_feeds_clicks_to_collectors() {
        let collector = Arc::new(Mutex::new(CursorCollector::for_capture(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
            false,
        )));
        let anchor = HostTime::now();
        let log = InputLog::new(anchor, Collectors(vec![Arc::clone(&collector)]));
        let (sender, receiver) = channel_with(16, KeyPolicy::Chords, || false);
        let mut drain = HostInputDrain::start(receiver, Arc::clone(&log)).unwrap();
        let later = HostTime::from_ticks(anchor.ticks() + HostTime::from_nanos(5_000_000).ticks());
        sender.send(
            HostTime::from_ticks(anchor.ticks().saturating_sub(1)),
            moved(),
        );
        sender.send(
            later,
            InputEvent::Button {
                button: CursorButton::Left,
                down: true,
                x: 50.0,
                y: 50.0,
                modifiers: Modifiers::default(),
            },
        );
        sender.send(later, key("a", Modifiers::default()));
        assert_eq!(drain.stop(), 0);

        let events = log.take();
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0].t_ms, 5);
        let json = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(json["type"], "button");
        assert_eq!(json["button"], "left");
        assert_eq!(
            collector
                .lock()
                .unwrap()
                .close()
                .diagnostics
                .click_events_received,
            1
        );
    }
}

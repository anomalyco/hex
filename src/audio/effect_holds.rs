//! Shared reference-counted ownership of "While dictating" effects (idle-sleep
//! prevention, output muting, output lowering, and media pausing).
//!
//! Each effect is held from the first active session that wants it until the
//! last one ends, so overlapping sessions (for example, a dictation and an
//! Annotate capture) compose cleanly: Lower audio reads the true original
//! volume once and restores it only when the last session finishes, and a
//! capture never prevents idle sleep.

#[cfg(any(not(target_os = "macos"), test))]
use std::collections::HashMap;
#[cfg(any(not(target_os = "macos"), test))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(any(not(target_os = "macos"), test))]
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
#[cfg(any(not(target_os = "macos"), test))]
use std::time::{Duration, Instant};

use crate::app_settings::RecordingAudioBehavior;

/// One thing the recording environment changes while at least one session
/// wants it. Each is held from the first session that wants it until the last
/// one ends, so overlapping sessions with different policies compose and none
/// restores what another still needs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Effect {
    Sleep,
    Mute,
    Duck,
    Pause,
}

impl Effect {
    /// Restoration order: output is audible again before a session's
    /// callback, and resuming media never delays it.
    pub(crate) const ALL: [Self; 4] = [Self::Sleep, Self::Mute, Self::Duck, Self::Pause];

    pub(crate) const fn index(self) -> usize {
        self as usize
    }
}

/// What one session asks for: dictation prevents idle sleep and applies
/// "While dictating"; a capture applies "While dictating" only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Hold {
    pub(crate) dictation: bool,
    pub(crate) audio: RecordingAudioBehavior,
}

impl Hold {
    pub(crate) const fn dictation(audio: RecordingAudioBehavior) -> Self {
        Self {
            dictation: true,
            audio,
        }
    }

    #[cfg_attr(
        not(any(hex_capture, not(target_os = "macos"), test)),
        allow(dead_code)
    )]
    pub(crate) const fn capture(audio: RecordingAudioBehavior) -> Self {
        Self {
            dictation: false,
            audio,
        }
    }

    pub(crate) fn wants(self, effect: Effect) -> bool {
        match effect {
            Effect::Sleep => self.dictation,
            Effect::Mute => self.audio == RecordingAudioBehavior::Mute,
            Effect::Duck => self.audio == RecordingAudioBehavior::Duck,
            Effect::Pause => self.audio == RecordingAudioBehavior::PauseMedia,
        }
    }

    #[cfg(any(not(target_os = "macos"), test))]
    pub(crate) fn is_empty(self) -> bool {
        !self.dictation && self.audio == RecordingAudioBehavior::DoNothing
    }
}

/// Reference-counted table of currently held effects. Dropping a held value
/// restores that effect.
pub(crate) struct EffectHolds<H> {
    sessions: [u32; Effect::ALL.len()],
    held: [Option<H>; Effect::ALL.len()],
}

impl<H> EffectHolds<H> {
    pub(crate) fn new() -> Self {
        Self {
            sessions: [0; Effect::ALL.len()],
            held: std::array::from_fn(|_| None),
        }
    }

    pub(crate) fn acquire(&mut self, wanted: Hold, mut hold: impl FnMut(Effect) -> H) {
        for effect in Effect::ALL.into_iter().filter(|e| wanted.wants(*e)) {
            let count = &mut self.sessions[effect.index()];
            *count = count.saturating_add(1);
            if *count == 1 {
                self.held[effect.index()] = Some(hold(effect));
            }
        }
    }

    /// Releases `wanted`, running `then` (if given) once output-affecting
    /// effects (`Mute`, `Duck`) have been restored (or immediately when
    /// another session keeps them held), and before `Pause` resumes media.
    pub(crate) fn release<F: FnOnce()>(&mut self, wanted: Hold, mut then: Option<F>) {
        for effect in Effect::ALL {
            if effect == Effect::Pause
                && let Some(then) = then.take()
            {
                then();
            }
            if !wanted.wants(effect) {
                continue;
            }
            let count = &mut self.sessions[effect.index()];
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.held[effect.index()] = None;
            }
        }
        if let Some(then) = then {
            then();
        }
    }

    #[cfg(any(not(target_os = "macos"), test))]
    pub(crate) fn release_all(&mut self) {
        for effect in Effect::ALL {
            self.sessions[effect.index()] = 0;
            self.held[effect.index()] = None;
        }
    }
}

#[cfg(any(not(target_os = "macos"), test))]
pub(crate) enum SessionRequest {
    Suppress {
        id: u64,
        hold: Hold,
    },
    Restore {
        id: u64,
    },
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    RestoreThen {
        id: u64,
        then: Box<dyn FnOnce() + Send>,
    },
    Drop {
        id: u64,
        ack: Option<SyncSender<()>>,
    },
    #[cfg(test)]
    Barrier(SyncSender<()>),
}

#[cfg(any(not(target_os = "macos"), test))]
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(any(not(target_os = "macos"), test))]
pub(crate) fn next_session_id() -> u64 {
    NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
}

/// Serves overlapping suppressor sessions against one shared [`EffectHolds`].
///
/// When `restore_grace` is nonzero (Linux), a plain `Restore` waits that long
/// before releasing the session's hold so a back-to-back double-tap `Suppress`
/// on the same session cancels it without bouncing the mixer or media player.
/// `RestoreThen` and `Drop` release immediately.
#[cfg(any(not(target_os = "macos"), test))]
pub(crate) fn serve_sessions<H>(
    receiver: Receiver<SessionRequest>,
    restore_grace: Duration,
    mut hold_effect: impl FnMut(Effect) -> H,
) {
    struct Entry {
        hold: Hold,
        restore_at: Option<Instant>,
    }

    let mut holds = EffectHolds::new();
    let mut sessions: HashMap<u64, Entry> = HashMap::new();

    loop {
        let now = Instant::now();
        let expired: Vec<u64> = sessions
            .iter()
            .filter_map(|(&id, entry)| entry.restore_at.is_some_and(|at| at <= now).then_some(id))
            .collect();
        for id in expired {
            if let Some(entry) = sessions.remove(&id) {
                holds.release(entry.hold, None::<fn()>);
            }
        }

        let next_deadline = sessions.values().filter_map(|entry| entry.restore_at).min();
        let request = match next_deadline {
            Some(deadline) => {
                match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(request) => request,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            None => match receiver.recv() {
                Ok(request) => request,
                Err(_) => break,
            },
        };

        match request {
            SessionRequest::Suppress { id, hold } => {
                if let Some(entry) = sessions.get_mut(&id) {
                    entry.restore_at = None;
                } else {
                    sessions.insert(
                        id,
                        Entry {
                            hold,
                            restore_at: None,
                        },
                    );
                    holds.acquire(hold, &mut hold_effect);
                }
            }
            SessionRequest::Restore { id } => {
                if restore_grace.is_zero() {
                    if let Some(entry) = sessions.remove(&id) {
                        holds.release(entry.hold, None::<fn()>);
                    }
                } else if let Some(entry) = sessions.get_mut(&id)
                    && entry.restore_at.is_none()
                {
                    entry.restore_at = Some(Instant::now() + restore_grace);
                }
            }
            SessionRequest::RestoreThen { id, then } => {
                if let Some(entry) = sessions.remove(&id) {
                    holds.release(entry.hold, Some(then));
                } else {
                    then();
                }
            }
            SessionRequest::Drop { id, ack } => {
                if let Some(entry) = sessions.remove(&id) {
                    holds.release(entry.hold, None::<fn()>);
                }
                if let Some(ack) = ack {
                    let _ = ack.send(());
                }
            }
            #[cfg(test)]
            SessionRequest::Barrier(ack) => {
                let _ = ack.send(());
            }
        }
    }
    holds.release_all();
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, mpsc};

    use super::*;
    use crate::audio::output_ducking;

    #[derive(Clone)]
    struct SharedVolume {
        values: Arc<Mutex<Vec<f32>>>,
    }

    impl output_ducking::Volume for SharedVolume {
        fn read(&self) -> Option<Vec<f32>> {
            Some(self.values.lock().unwrap().clone())
        }

        fn write(&mut self, values: &[f32]) -> bool {
            *self.values.lock().unwrap() = values.to_vec();
            true
        }
    }

    fn barrier(requests: &mpsc::Sender<SessionRequest>) {
        let (ack, done) = mpsc::sync_channel(1);
        requests.send(SessionRequest::Barrier(ack)).unwrap();
        done.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn capture_with_do_nothing_is_empty_and_dictation_only_prevents_sleep() {
        let capture = Hold::capture(RecordingAudioBehavior::DoNothing);
        assert!(capture.is_empty());
        assert!(!capture.wants(Effect::Sleep));

        let capture_duck = Hold::capture(RecordingAudioBehavior::Duck);
        assert!(!capture_duck.is_empty());
        assert!(!capture_duck.wants(Effect::Sleep));
        assert!(capture_duck.wants(Effect::Duck));

        let dictation = Hold::dictation(RecordingAudioBehavior::DoNothing);
        assert!(!dictation.is_empty());
        assert!(dictation.wants(Effect::Sleep));
        assert!(!dictation.wants(Effect::Duck));
    }

    /// Overlapping dictation + Annotate capture under Lower audio must duck
    /// once from the true original volume (100% -> 20%) and restore back to
    /// 100% only after the last session ends — never reading 20% as the
    /// capture's original and leaving output stuck at 20%.
    #[test]
    fn overlapping_duck_sessions_restore_original_volume() {
        let values = Arc::new(Mutex::new(vec![1.0_f32, 0.8]));
        let events = Arc::new(Mutex::new(Vec::new()));
        let (requests, receiver) = mpsc::channel();

        let worker_values = values.clone();
        let worker_events = events.clone();
        let worker = std::thread::spawn(move || {
            struct Guarded {
                effect: Effect,
                _duck: Option<output_ducking::Guard>,
                events: Arc<Mutex<Vec<(&'static str, Effect)>>>,
            }
            impl Drop for Guarded {
                fn drop(&mut self) {
                    // Restore ducking before recording the event so the
                    // volume is already back to original once logged.
                    self._duck.take();
                    self.events.lock().unwrap().push(("restore", self.effect));
                }
            }
            serve_sessions(receiver, Duration::ZERO, move |effect| {
                worker_events.lock().unwrap().push(("hold", effect));
                let duck = if effect == Effect::Duck {
                    let volume = SharedVolume {
                        values: worker_values.clone(),
                    };
                    output_ducking::Guard::start(0.2, move || Some(volume))
                } else {
                    None
                };
                Guarded {
                    effect,
                    _duck: duck,
                    events: worker_events.clone(),
                }
            });
        });

        let dictation_id = next_session_id();
        let capture_id = next_session_id();
        let dictation_hold = Hold::dictation(RecordingAudioBehavior::Duck);
        let capture_hold = Hold::capture(RecordingAudioBehavior::Duck);

        // 1. Dictation begins: holds Sleep and Duck, lowering 1.0/0.8 -> 0.2/0.16.
        requests
            .send(SessionRequest::Suppress {
                id: dictation_id,
                hold: dictation_hold,
            })
            .unwrap();
        barrier(&requests);
        std::thread::sleep(Duration::from_millis(200));
        {
            let current = values.lock().unwrap();
            assert!(
                (current[0] - 0.2).abs() < 0.02,
                "expected 0.2, got {:?}",
                *current
            );
            assert!(
                (current[1] - 0.16).abs() < 0.02,
                "expected 0.16, got {:?}",
                *current
            );
        }

        // 2. Capture overlaps: must NOT duck a second time (no 4%) or hold Sleep.
        requests
            .send(SessionRequest::Suppress {
                id: capture_id,
                hold: capture_hold,
            })
            .unwrap();
        barrier(&requests);
        assert_eq!(
            *events.lock().unwrap(),
            vec![("hold", Effect::Sleep), ("hold", Effect::Duck)]
        );
        {
            let current = values.lock().unwrap();
            assert!(
                (current[0] - 0.2).abs() < 0.02,
                "expected 0.2, got {:?}",
                *current
            );
        }

        // 3. Dictation ends first: Sleep releases, Duck stays held at 20%.
        requests
            .send(SessionRequest::Restore { id: dictation_id })
            .unwrap();
        barrier(&requests);
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                ("hold", Effect::Sleep),
                ("hold", Effect::Duck),
                ("restore", Effect::Sleep),
            ]
        );
        {
            let current = values.lock().unwrap();
            assert!(
                (current[0] - 0.2).abs() < 0.02,
                "expected 0.2, got {:?}",
                *current
            );
        }

        // 4. Capture drops: Duck releases and ramps back to the original 1.0/0.8.
        let (ack, done) = mpsc::sync_channel(1);
        requests
            .send(SessionRequest::Drop {
                id: capture_id,
                ack: Some(ack),
            })
            .unwrap();
        done.recv_timeout(Duration::from_secs(5)).unwrap();
        {
            let current = values.lock().unwrap();
            assert!(
                (current[0] - 1.0).abs() < 0.01,
                "expected 1.0, got {:?}",
                *current
            );
            assert!(
                (current[1] - 0.8).abs() < 0.01,
                "expected 0.8, got {:?}",
                *current
            );
        }

        drop(requests);
        worker.join().unwrap();
    }

    #[test]
    fn overlapping_sessions_with_grace_and_restore_then_compose() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (requests, receiver) = mpsc::channel();
        let worker_log = log.clone();
        let grace = Duration::from_millis(60);

        struct Logged(Effect, Arc<Mutex<Vec<String>>>);
        impl Drop for Logged {
            fn drop(&mut self) {
                self.1.lock().unwrap().push(format!("restore:{:?}", self.0));
            }
        }

        let worker = std::thread::spawn(move || {
            serve_sessions(receiver, grace, move |effect| {
                worker_log.lock().unwrap().push(format!("hold:{effect:?}"));
                Logged(effect, worker_log.clone())
            });
        });

        let a = next_session_id();
        let b = next_session_id();

        // Session A suppresses, calls plain restore, and session B starts
        // during A's grace window: Mute stays held past A's grace expiration.
        requests
            .send(SessionRequest::Suppress {
                id: a,
                hold: Hold::dictation(RecordingAudioBehavior::Mute),
            })
            .unwrap();
        requests.send(SessionRequest::Restore { id: a }).unwrap();
        requests
            .send(SessionRequest::Suppress {
                id: b,
                hold: Hold::capture(RecordingAudioBehavior::Mute),
            })
            .unwrap();
        std::thread::sleep(grace * 2);
        barrier(&requests);
        assert_eq!(
            *log.lock().unwrap(),
            vec!["hold:Sleep", "hold:Mute", "restore:Sleep"]
        );

        // Session B calls RestoreThen: Mute is restored BEFORE callback runs.
        let cb_log = log.clone();
        requests
            .send(SessionRequest::RestoreThen {
                id: b,
                then: Box::new(move || cb_log.lock().unwrap().push("then".into())),
            })
            .unwrap();
        barrier(&requests);
        assert_eq!(
            *log.lock().unwrap(),
            vec![
                "hold:Sleep",
                "hold:Mute",
                "restore:Sleep",
                "restore:Mute",
                "then",
            ]
        );

        drop(requests);
        worker.join().unwrap();
    }
}

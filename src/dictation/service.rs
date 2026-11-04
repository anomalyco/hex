//! Running-app dictation a local API client owns (`/dictations`): the
//! controls the API sends a capture loop, owner tokens, lease expiry, level
//! metering, and the bounded record of how each dictation ended. The macOS
//! recognition loop drives these controls itself; Linux drives them through
//! [`ServiceDictations`].

use std::collections::VecDeque;
use std::sync::mpsc::SyncSender;
#[cfg(any(test, target_os = "linux"))]
use std::sync::mpsc::TrySendError;
use std::time::{Duration, Instant};

/// How long a dictation lives without a heartbeat from its owner.
pub(crate) const LEASE: Duration = Duration::from_secs(10);
pub(crate) const COMPLETION_LIMIT: usize = 16;
/// `/finish` on a dictation the microphone ended before it finished.
pub(crate) const MICROPHONE_LOST: &str = "microphone-lost";
#[cfg(any(test, target_os = "linux"))]
const LEVEL_INTERVAL: Duration = Duration::from_millis(30);

/// One `/dictations` request for the capture loop. Each carries the owner
/// token its start issued; a mismatched token is `dictation-not-active`.
#[derive(Debug)]
pub enum DictationControl {
    Start {
        source: String,
        owner_token: String,
        levels: SyncSender<DictationLevel>,
        reply: SyncSender<Result<ProgrammaticDictationStart, String>>,
    },
    AttachAudio {
        id: u64,
        owner_token: String,
        audio: SyncSender<DictationAudioChunk>,
        reply: SyncSender<Result<(), String>>,
    },
    Finish {
        id: u64,
        owner_token: String,
        reply: SyncSender<Result<ProgrammaticDictationResult, String>>,
    },
    Cancel {
        id: u64,
        owner_token: String,
        reply: SyncSender<Result<(), String>>,
    },
    Heartbeat {
        id: u64,
        owner_token: String,
        reply: SyncSender<Result<(), String>>,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct DictationLevel {
    pub rms_db: f32,
    pub peak_db: f32,
}

#[derive(Clone, Debug)]
pub struct ProgrammaticDictationResult {
    pub transcript: String,
    pub duration_ms: u64,
}

#[derive(Debug)]
pub struct ProgrammaticDictationStart {
    pub id: u64,
    pub sample_rate: u32,
}

#[derive(Debug)]
pub struct DictationAudioChunk {
    pub samples: Vec<f32>,
}

#[derive(Clone)]
pub(crate) enum TerminalOutcome {
    Finished(Result<ProgrammaticDictationResult, String>),
    Cancelled,
}

/// How one dictation ended, kept so a repeated `/finish` or `/cancel` gets
/// the same answer.
#[derive(Clone)]
pub(crate) struct Completion {
    pub(crate) id: u64,
    pub(crate) owner_token: String,
    pub(crate) outcome: TerminalOutcome,
}

impl Completion {
    pub(crate) fn matches_owner(&self, id: u64, owner_token: &str) -> bool {
        self.id == id && self.owner_token == owner_token
    }
}

pub(crate) fn remember_completion(completions: &mut VecDeque<Completion>, completion: Completion) {
    completions.push_back(completion);
    while completions.len() > COMPLETION_LIMIT {
        completions.pop_front();
    }
}

pub(crate) fn finish_with_result(
    id: u64,
    owner_token: String,
    replies: Vec<SyncSender<Result<ProgrammaticDictationResult, String>>>,
    result: Result<ProgrammaticDictationResult, String>,
    completions: &mut VecDeque<Completion>,
) {
    for reply in replies {
        let _ = reply.send(result.clone());
    }
    remember_completion(
        completions,
        Completion {
            id,
            owner_token,
            outcome: TerminalOutcome::Finished(result),
        },
    );
}

pub(crate) fn lease_expired(deadline: Instant, now: Instant) -> bool {
    now >= deadline
}

pub(crate) fn level_for(samples: &[f32]) -> DictationLevel {
    let (sum_squares, peak) = samples
        .iter()
        .fold((0.0_f64, 0.0_f32), |(sum, peak), sample| {
            (
                sum + f64::from(*sample) * f64::from(*sample),
                peak.max(sample.abs()),
            )
        });
    let rms = if samples.is_empty() {
        0.0
    } else {
        (sum_squares / samples.len() as f64).sqrt() as f32
    };
    DictationLevel {
        rms_db: amplitude_db(rms),
        peak_db: amplitude_db(peak),
    }
}

fn amplitude_db(amplitude: f32) -> f32 {
    (20.0 * amplitude.max(0.000_001).log10()).max(-120.0)
}

#[cfg(any(test, target_os = "linux"))]
pub(crate) use owner::{Effect, ServiceDictations};

#[cfg(any(test, target_os = "linux"))]
mod owner {
    use super::*;

    /// What the capture loop does after [`ServiceDictations::control`].
    #[derive(Debug, Eq, PartialEq)]
    pub(crate) enum Effect {
        None,
        /// Start recording for the client, then answer through
        /// [`ServiceDictations::started`] once the sample rate is known.
        Start,
        /// Stop recording `id` without a tail, then report
        /// [`ServiceDictations::transcribing`] or
        /// [`ServiceDictations::complete`].
        Finish(u64),
        /// Drop the client's recording.
        Cancel,
    }

    struct Active {
        id: u64,
        owner_token: String,
        source: String,
        levels: SyncSender<DictationLevel>,
        audio: Option<SyncSender<DictationAudioChunk>>,
        last_level: Option<Instant>,
        lease_deadline: Instant,
        /// The start's answer, sent once the microphone's rate is known.
        start_reply: Option<SyncSender<Result<ProgrammaticDictationStart, String>>>,
    }

    impl Active {
        fn matches_owner(&self, id: u64, owner_token: &str) -> bool {
            self.id == id && self.owner_token == owner_token
        }
    }

    struct Finishing {
        id: u64,
        owner_token: String,
        source: String,
        duration_ms: u64,
        replies: Vec<SyncSender<Result<ProgrammaticDictationResult, String>>>,
    }

    /// The one dictation a local API client owns on a capture loop that
    /// records it beside its shortcut captures (Linux). Only its owner token
    /// finishes, cancels, or renews it; a lapsed lease cancels only it.
    #[derive(Default)]
    pub(crate) struct ServiceDictations {
        next_id: u64,
        active: Option<Active>,
        finishing: Vec<Finishing>,
        completions: VecDeque<Completion>,
    }

    impl ServiceDictations {
        /// Whether a client's dictation records (or waits for the microphone).
        pub(crate) fn is_active(&self) -> bool {
            self.active.is_some()
        }

        /// Whether the active dictation still waits for the microphone.
        pub(crate) fn is_starting(&self) -> bool {
            self.active
                .as_ref()
                .is_some_and(|active| active.start_reply.is_some())
        }

        /// Handles one control. `busy`: another capture records, so a start
        /// is refused rather than ending it.
        pub(crate) fn control(
            &mut self,
            control: DictationControl,
            busy: bool,
            now: Instant,
        ) -> Effect {
            match control {
                DictationControl::Start {
                    source,
                    owner_token,
                    levels,
                    reply,
                } => {
                    if busy || self.active.is_some() {
                        let _ = reply.send(Err("dictation-busy".into()));
                        return Effect::None;
                    }
                    self.next_id = self.next_id.wrapping_add(1).max(1);
                    tracing::info!(
                        dictation_id = self.next_id,
                        source,
                        "service dictation started"
                    );
                    self.active = Some(Active {
                        id: self.next_id,
                        owner_token,
                        source,
                        levels,
                        audio: None,
                        last_level: None,
                        lease_deadline: now + LEASE,
                        start_reply: Some(reply),
                    });
                    Effect::Start
                }
                DictationControl::AttachAudio {
                    id,
                    owner_token,
                    audio,
                    reply,
                } => {
                    let answer = match self
                        .active
                        .as_mut()
                        .filter(|active| active.matches_owner(id, &owner_token))
                    {
                        None => Err("dictation-not-active".into()),
                        Some(active) if active.audio.is_some() => {
                            Err("audio-already-subscribed".into())
                        }
                        Some(active) => {
                            active.audio = Some(audio);
                            Ok(())
                        }
                    };
                    let _ = reply.send(answer);
                    Effect::None
                }
                DictationControl::Heartbeat {
                    id,
                    owner_token,
                    reply,
                } => {
                    let answer = match self
                        .active
                        .as_mut()
                        .filter(|active| active.matches_owner(id, &owner_token))
                    {
                        Some(active) => {
                            active.lease_deadline = now + LEASE;
                            Ok(())
                        }
                        None => Err("dictation-not-active".into()),
                    };
                    let _ = reply.send(answer);
                    Effect::None
                }
                DictationControl::Finish {
                    id,
                    owner_token,
                    reply,
                } => {
                    if let Some(finishing) = self.finishing.iter_mut().find(|finishing| {
                        finishing.id == id && finishing.owner_token == owner_token
                    }) {
                        finishing.replies.push(reply);
                        return Effect::None;
                    }
                    if let Some(completion) = self.completion(id, &owner_token) {
                        let _ = reply.send(match &completion.outcome {
                            TerminalOutcome::Finished(result) => result.clone(),
                            TerminalOutcome::Cancelled => Err("cancelled".into()),
                        });
                        return Effect::None;
                    }
                    let Some(active) = self
                        .active
                        .take_if(|active| active.matches_owner(id, &owner_token))
                    else {
                        let _ = reply.send(Err("dictation-not-active".into()));
                        return Effect::None;
                    };
                    self.finishing.push(Finishing {
                        id,
                        owner_token,
                        source: active.source,
                        duration_ms: 0,
                        replies: vec![reply],
                    });
                    Effect::Finish(id)
                }
                DictationControl::Cancel {
                    id,
                    owner_token,
                    reply,
                } => {
                    if self.completion(id, &owner_token).is_some_and(|completion| {
                        matches!(completion.outcome, TerminalOutcome::Cancelled)
                    }) {
                        let _ = reply.send(Ok(()));
                        return Effect::None;
                    }
                    if self
                        .finishing
                        .iter()
                        .any(|finishing| finishing.id == id && finishing.owner_token == owner_token)
                    {
                        let _ = reply.send(Err("dictation-finishing".into()));
                        return Effect::None;
                    }
                    let Some(active) = self
                        .active
                        .take_if(|active| active.matches_owner(id, &owner_token))
                    else {
                        let _ = reply.send(Err("dictation-not-active".into()));
                        return Effect::None;
                    };
                    tracing::info!(
                        dictation_id = id,
                        source = active.source,
                        "service dictation cancelled"
                    );
                    self.end(active, TerminalOutcome::Cancelled);
                    let _ = reply.send(Ok(()));
                    Effect::Cancel
                }
            }
        }

        /// Answers the start once the microphone delivers audio. False when
        /// its client already gave up: the loop drops the recording.
        pub(crate) fn started(&mut self, sample_rate: u32) -> bool {
            let Some(active) = self.active.as_mut() else {
                return false;
            };
            let Some(reply) = active.start_reply.take() else {
                return true;
            };
            let id = active.id;
            if reply
                .send(Ok(ProgrammaticDictationStart { id, sample_rate }))
                .is_ok()
            {
                return true;
            }
            let active = self.active.take().expect("the start was checked");
            tracing::warn!(dictation_id = id, "service dictation start went unanswered");
            self.end(active, TerminalOutcome::Cancelled);
            false
        }

        /// The microphone ended the dictation: a start still waiting answers
        /// `code`; a recording one finishes with `microphone-lost`.
        pub(crate) fn lost(&mut self, code: &str) {
            let Some(mut active) = self.active.take() else {
                return;
            };
            tracing::warn!(
                dictation_id = active.id,
                source = active.source,
                code,
                "service dictation ended"
            );
            if let Some(reply) = active.start_reply.take() {
                let _ = reply.send(Err(code.into()));
            }
            self.end(
                active,
                TerminalOutcome::Finished(Err(MICROPHONE_LOST.into())),
            );
        }

        /// Cancels the dictation whose owner stopped renewing its lease.
        /// True when the loop must drop its recording.
        pub(crate) fn expire(&mut self, now: Instant) -> bool {
            let Some(active) = self
                .active
                .take_if(|active| lease_expired(active.lease_deadline, now))
            else {
                return false;
            };
            tracing::warn!(
                dictation_id = active.id,
                source = active.source,
                "service dictation lease expired"
            );
            self.end(active, TerminalOutcome::Cancelled);
            true
        }

        /// Meters and forwards one chunk of the client's recording.
        pub(crate) fn audio(&mut self, samples: &[f32], now: Instant) {
            let Some(active) = self.active.as_mut() else {
                return;
            };
            if active
                .last_level
                .is_none_or(|last| now.duration_since(last) >= LEVEL_INTERVAL)
            {
                let _ = active.levels.try_send(level_for(samples));
                active.last_level = Some(now);
            }
            if let Some(audio) = &active.audio
                && let Err(TrySendError::Disconnected(_)) = audio.try_send(DictationAudioChunk {
                    samples: samples.to_vec(),
                })
            {
                active.audio = None;
            }
        }

        /// The finished recording `id` is transcribing.
        pub(crate) fn transcribing(&mut self, id: u64, duration_ms: u64) {
            if let Some(finishing) = self
                .finishing
                .iter_mut()
                .find(|finishing| finishing.id == id)
            {
                finishing.duration_ms = duration_ms;
                tracing::info!(
                    dictation_id = id,
                    source = finishing.source,
                    duration_ms,
                    "service dictation submitted"
                );
            }
        }

        /// Answers every `/finish` waiting on `id`.
        pub(crate) fn complete(&mut self, id: u64, result: Result<String, String>) {
            let Some(index) = self
                .finishing
                .iter()
                .position(|finishing| finishing.id == id)
            else {
                return;
            };
            let finishing = self.finishing.swap_remove(index);
            match &result {
                Ok(transcript) => tracing::info!(
                    dictation_id = id,
                    source = finishing.source,
                    transcript_chars = transcript.chars().count(),
                    duration_ms = finishing.duration_ms,
                    "service dictation ended"
                ),
                Err(error) => tracing::warn!(
                    dictation_id = id,
                    source = finishing.source,
                    error,
                    "service dictation ended"
                ),
            }
            let duration_ms = finishing.duration_ms;
            finish_with_result(
                id,
                finishing.owner_token,
                finishing.replies,
                result.map(|transcript| ProgrammaticDictationResult {
                    transcript,
                    duration_ms,
                }),
                &mut self.completions,
            );
        }

        fn completion(&self, id: u64, owner_token: &str) -> Option<&Completion> {
            self.completions
                .iter()
                .rev()
                .find(|completion| completion.matches_owner(id, owner_token))
        }

        fn end(&mut self, active: Active, outcome: TerminalOutcome) {
            remember_completion(
                &mut self.completions,
                Completion {
                    id: active.id,
                    owner_token: active.owner_token,
                    outcome,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, Receiver};

    use super::*;

    #[test]
    fn meter_reports_rms_and_peak_in_decibels() {
        let level = level_for(&[0.5, -0.5, 0.0, 0.0]);
        assert!((level.rms_db - -9.0309).abs() < 0.001);
        assert!((level.peak_db - -6.0206).abs() < 0.001);
        assert_eq!(level_for(&[]).rms_db, -120.0);
    }

    #[test]
    fn lease_expires_at_its_deadline() {
        let now = Instant::now();
        let deadline = now + LEASE;
        assert!(!lease_expired(
            deadline,
            deadline - Duration::from_millis(1)
        ));
        assert!(lease_expired(deadline, deadline));
    }

    #[test]
    fn completion_cache_is_bounded() {
        let mut completions = VecDeque::new();
        for id in 0..COMPLETION_LIMIT as u64 + 2 {
            remember_completion(
                &mut completions,
                Completion {
                    id,
                    owner_token: format!("owner-{id}"),
                    outcome: TerminalOutcome::Cancelled,
                },
            );
        }
        assert_eq!(completions.len(), COMPLETION_LIMIT);
        assert_eq!(completions.front().map(|completion| completion.id), Some(2));
    }

    struct Client {
        owner_token: String,
        levels: Receiver<DictationLevel>,
        start: Receiver<Result<ProgrammaticDictationStart, String>>,
    }

    fn start(
        owner: &mut ServiceDictations,
        owner_token: &str,
        busy: bool,
        now: Instant,
    ) -> (Effect, Client) {
        let (levels, level_receiver) = mpsc::sync_channel(8);
        let (reply, start) = mpsc::sync_channel(1);
        let effect = owner.control(
            DictationControl::Start {
                source: "test".into(),
                owner_token: owner_token.into(),
                levels,
                reply,
            },
            busy,
            now,
        );
        (
            effect,
            Client {
                owner_token: owner_token.into(),
                levels: level_receiver,
                start,
            },
        )
    }

    fn ask<T>(
        owner: &mut ServiceDictations,
        now: Instant,
        build: impl FnOnce(SyncSender<Result<T, String>>) -> DictationControl,
    ) -> (Effect, Result<T, String>) {
        let (reply, answer) = mpsc::sync_channel(4);
        let effect = owner.control(build(reply), false, now);
        (effect, answer.try_recv().expect("answered at once"))
    }

    fn cancel(owner: &mut ServiceDictations, id: u64, token: &str) -> (Effect, Result<(), String>) {
        ask(owner, Instant::now(), |reply| DictationControl::Cancel {
            id,
            owner_token: token.into(),
            reply,
        })
    }

    #[test]
    fn a_start_answers_once_audio_arrives_and_refuses_while_another_capture_records() {
        let mut owner = ServiceDictations::default();
        let now = Instant::now();
        // A shortcut dictation records: the client is refused, not the
        // shortcut ended.
        let (effect, client) = start(&mut owner, "hex_a", true, now);
        assert_eq!(effect, Effect::None);
        assert_eq!(
            client.start.try_recv().unwrap().unwrap_err(),
            "dictation-busy"
        );
        assert!(!owner.is_active());

        let (effect, client) = start(&mut owner, "hex_a", false, now);
        assert_eq!(effect, Effect::Start);
        assert!(owner.is_starting());
        assert!(client.start.try_recv().is_err());
        assert!(owner.started(48_000));
        let started = client.start.try_recv().unwrap().unwrap();
        assert_eq!((started.id, started.sample_rate), (1, 48_000));
        assert!(!owner.is_starting());

        // One client dictation at a time.
        let (effect, other) = start(&mut owner, "hex_b", false, now);
        assert_eq!(effect, Effect::None);
        assert_eq!(
            other.start.try_recv().unwrap().unwrap_err(),
            "dictation-busy"
        );

        owner.audio(&[0.5, -0.5], now);
        owner.audio(&[0.5, -0.5], now + Duration::from_millis(5));
        assert!(client.levels.try_recv().is_ok());
        assert!(client.levels.try_recv().is_err(), "levels are throttled");
        owner.audio(&[0.5], now + LEVEL_INTERVAL);
        assert!(client.levels.try_recv().is_ok());
    }

    #[test]
    fn a_start_its_client_abandoned_is_dropped() {
        let mut owner = ServiceDictations::default();
        let (_, client) = start(&mut owner, "hex_a", false, Instant::now());
        drop(client);
        assert!(!owner.started(16_000));
        assert!(!owner.is_active());
    }

    #[test]
    fn only_the_owner_token_finishes_cancels_or_renews() {
        let mut owner = ServiceDictations::default();
        let now = Instant::now();
        let (_, client) = start(&mut owner, "hex_a", false, now);
        assert!(owner.started(16_000));

        let (effect, answer) = cancel(&mut owner, 1, "hex_wrong");
        assert_eq!(effect, Effect::None);
        assert_eq!(answer.unwrap_err(), "dictation-not-active");
        let (_, answer) = ask(&mut owner, now, |reply| DictationControl::Heartbeat {
            id: 2,
            owner_token: client.owner_token.clone(),
            reply,
        });
        assert_eq!(answer.unwrap_err(), "dictation-not-active");
        let (effect, answer) = ask(&mut owner, now, |reply| DictationControl::Finish {
            id: 1,
            owner_token: "hex_wrong".into(),
            reply,
        });
        assert_eq!(effect, Effect::None);
        assert_eq!(answer.unwrap_err(), "dictation-not-active");
        assert!(owner.is_active());

        let (effect, answer) = cancel(&mut owner, 1, &client.owner_token);
        assert_eq!(effect, Effect::Cancel);
        answer.unwrap();
        assert!(!owner.is_active());
        // Cancelling again is answered the same way, and finish reports it.
        let (effect, answer) = cancel(&mut owner, 1, &client.owner_token);
        assert_eq!(effect, Effect::None);
        answer.unwrap();
        let (_, answer) = ask(&mut owner, now, |reply| DictationControl::Finish {
            id: 1,
            owner_token: client.owner_token.clone(),
            reply,
        });
        assert_eq!(answer.unwrap_err(), "cancelled");
    }

    #[test]
    fn finishing_answers_every_waiting_finish_with_the_transcript() {
        let mut owner = ServiceDictations::default();
        let now = Instant::now();
        let (_, client) = start(&mut owner, "hex_a", false, now);
        assert!(owner.started(16_000));
        let finish = |owner: &mut ServiceDictations| {
            let (reply, answer) = mpsc::sync_channel(1);
            let effect = owner.control(
                DictationControl::Finish {
                    id: 1,
                    owner_token: client.owner_token.clone(),
                    reply,
                },
                false,
                now,
            );
            (effect, answer)
        };
        let (effect, first) = finish(&mut owner);
        assert_eq!(effect, Effect::Finish(1));
        let (effect, second) = finish(&mut owner);
        assert_eq!(effect, Effect::None);
        // A finishing dictation can no longer be cancelled.
        let (_, answer) = cancel(&mut owner, 1, &client.owner_token);
        assert_eq!(answer.unwrap_err(), "dictation-finishing");

        owner.transcribing(1, 875);
        owner.complete(1, Ok("raw words".into()));
        for answer in [first, second] {
            let result = answer.try_recv().unwrap().unwrap();
            assert_eq!(
                (result.transcript.as_str(), result.duration_ms),
                ("raw words", 875)
            );
        }
        let (_, again) = finish(&mut owner);
        assert_eq!(again.try_recv().unwrap().unwrap().transcript, "raw words");
    }

    #[test]
    fn a_lapsed_lease_cancels_only_that_clients_dictation() {
        let mut owner = ServiceDictations::default();
        let now = Instant::now();
        let (_, client) = start(&mut owner, "hex_a", false, now);
        assert!(owner.started(16_000));
        assert!(!owner.expire(now + LEASE - Duration::from_millis(1)));

        let renewed = now + Duration::from_secs(5);
        let (_, answer) = ask(&mut owner, renewed, |reply| DictationControl::Heartbeat {
            id: 1,
            owner_token: client.owner_token.clone(),
            reply,
        });
        answer.unwrap();
        assert!(!owner.expire(now + LEASE));
        assert!(owner.expire(renewed + LEASE));
        assert!(!owner.is_active());
        assert!(!owner.expire(renewed + LEASE * 2), "nothing else to expire");
        let (_, answer) = ask(&mut owner, now, |reply| DictationControl::Finish {
            id: 1,
            owner_token: client.owner_token.clone(),
            reply,
        });
        assert_eq!(answer.unwrap_err(), "cancelled");
    }

    #[test]
    fn a_lost_microphone_ends_the_dictation() {
        let mut owner = ServiceDictations::default();
        let now = Instant::now();
        // Still opening: the start itself fails.
        let (_, waiting) = start(&mut owner, "hex_a", false, now);
        owner.lost("microphone-unavailable");
        assert_eq!(
            waiting.start.try_recv().unwrap().unwrap_err(),
            "microphone-unavailable"
        );

        let (_, client) = start(&mut owner, "hex_b", false, now);
        assert!(owner.started(16_000));
        owner.lost("microphone-interrupted");
        let (_, answer) = ask(&mut owner, now, |reply| DictationControl::Finish {
            id: 2,
            owner_token: client.owner_token.clone(),
            reply,
        });
        assert_eq!(answer.unwrap_err(), MICROPHONE_LOST);
    }
}

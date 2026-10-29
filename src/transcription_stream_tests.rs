use std::sync::atomic::{AtomicBool, AtomicUsize};

use super::*;

/// What the fake engine did, shared with the test.
#[derive(Default)]
struct Fake {
    busy: AtomicBool,
    not_streaming: AtomicBool,
    lost_model: AtomicBool,
    lives: Mutex<Vec<Arc<FakeLiveState>>>,
    offline: Mutex<Vec<usize>>,
}

#[derive(Default)]
struct FakeLiveState {
    closed: AtomicBool,
    fed: AtomicUsize,
}

#[derive(Clone, Default)]
pub(crate) struct FakeEngine(Arc<Fake>);

/// An engine source for route tests: a fake streaming engine holding the
/// default selection.
pub(crate) fn fake_source() -> EngineSource {
    Arc::new(|| {
        Ok((
            Box::new(FakeEngine::default()) as Box<dyn Engine>,
            selection(),
        ))
    })
}

impl FakeEngine {
    fn lives(&self) -> Vec<Arc<FakeLiveState>> {
        self.0.lives.lock().unwrap().clone()
    }

    fn set_busy(&self, busy: bool) {
        self.0.busy.store(busy, Ordering::SeqCst);
    }

    /// What dictation's first request does to an open window stream.
    fn preempt(&self) {
        self.set_busy(true);
        for live in self.lives() {
            live.closed.store(true, Ordering::SeqCst);
        }
    }
}

impl Engine for FakeEngine {
    fn dictation_busy(&self) -> bool {
        self.0.busy.load(Ordering::SeqCst)
    }

    fn streams(&self) -> bool {
        !self.0.not_streaming.load(Ordering::SeqCst)
    }

    fn holds_model(&self) -> bool {
        !self.0.lost_model.load(Ordering::SeqCst)
    }

    fn begin(&self) -> Option<Box<dyn Live>> {
        if !self.streams() {
            return None;
        }
        let state = Arc::new(FakeLiveState::default());
        self.0.lives.lock().unwrap().push(state.clone());
        Some(Box::new(FakeLive(state)))
    }

    fn transcribe(&self, window: Vec<f32>, stop: &dyn Fn() -> bool) -> Result<String, String> {
        while self.dictation_busy() {
            if stop() {
                return Err("stopped".into());
            }
            thread::sleep(Duration::from_millis(5));
        }
        self.0.offline.lock().unwrap().push(window.len());
        Ok(format!("offline {}", window.len()))
    }

    fn quitting(&self) -> bool {
        false
    }
}

struct FakeLive(Arc<FakeLiveState>);

impl Live for FakeLive {
    fn open(&self) -> bool {
        !self.0.closed.load(Ordering::SeqCst)
    }

    fn feed(&mut self, samples: &[f32]) {
        assert!(self.open(), "fed a closed stream");
        self.0.fed.fetch_add(samples.len(), Ordering::SeqCst);
    }

    fn in_flight(&self) -> usize {
        0
    }

    fn partial(&self) -> Option<Partial> {
        Some(Partial {
            stable: format!("heard {}", self.0.fed.load(Ordering::SeqCst)),
            tentative: String::new(),
        })
    }

    fn finish(self: Box<Self>) -> Result<String, String> {
        if !self.open() {
            return Err(crate::transcription_engine::PREEMPTED.into());
        }
        Ok(format!("live {}", self.0.fed.load(Ordering::SeqCst)))
    }
}

fn selection() -> TranscriptionSelection {
    TranscriptionSelection::default()
}

fn create(streams: &Streams, engine: &FakeEngine) -> Arc<Stream> {
    streams
        .create(
            engine.clone(),
            SampleFormat::F32le,
            selection(),
            "owner".into(),
        )
        .unwrap()
}

fn finish(stream: &Stream) -> End {
    stream
        .finish(Duration::from_secs(10), &|| false)
        .expect("the stream ended")
}

fn eventually(mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(started.elapsed() < Duration::from_secs(10), "timed out");
        thread::sleep(Duration::from_millis(5));
    }
}

fn texts(end: &End) -> Vec<String> {
    match end {
        End::Final { segments, .. } => segments.iter().map(|s| s.text.clone()).collect(),
        other => panic!("the stream did not finish: {other:?}"),
    }
}

#[test]
fn sample_formats_decode_little_endian_and_reject_partial_or_non_finite_samples() {
    let f32le = [0.5_f32, -0.25]
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect::<Vec<_>>();
    assert_eq!(SampleFormat::F32le.decode(&f32le), Some(vec![0.5, -0.25]));
    assert_eq!(SampleFormat::F32le.decode(&f32le[..5]), None);
    assert_eq!(SampleFormat::F32le.decode(&f32::NAN.to_le_bytes()), None);
    let s16le = [16_384_i16, -32_768]
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect::<Vec<_>>();
    assert_eq!(SampleFormat::S16le.decode(&s16le), Some(vec![0.5, -1.0]));
    assert_eq!(SampleFormat::S16le.decode(&s16le[..3]), None);
    assert_eq!(SampleFormat::parse("s16le"), Some(SampleFormat::S16le));
    assert_eq!(SampleFormat::parse("wav"), None);
}

#[test]
fn a_window_streams_with_partials_and_finishes_through_its_live_stream() {
    let engine = FakeEngine::default();
    let streams = Streams::default();
    let stream = create(&streams, &engine);
    assert!(stream.partials);
    stream.push(&[0.0; 8_000]).unwrap();
    eventually(|| {
        engine
            .lives()
            .first()
            .is_some_and(|live| live.fed.load(Ordering::SeqCst) == 8_000)
    });
    let (events, received) = std::sync::mpsc::channel();
    let reader = {
        let stream = stream.clone();
        thread::spawn(move || {
            stream.subscribe(
                Duration::from_secs(5),
                &|| false,
                &mut |event| events.send(event.clone()).is_ok(),
                &mut || true,
            );
        })
    };
    let mut events = vec![received.recv().unwrap()];
    assert!(matches!(
        &events[0],
        Event::Partial { segment: 0, stable, .. } if stable == "heard 8000"
    ));
    let end = finish(&stream);
    assert_eq!(texts(&end), ["live 8000"]);
    let End::Final {
        segments,
        duration_ms,
        ..
    } = &end
    else {
        unreachable!()
    };
    assert_eq!((segments[0].start_ms, segments[0].end_ms), (0, 500));
    assert_eq!(*duration_ms, 500);
    reader.join().unwrap();
    events.extend(received.try_iter());
    assert!(matches!(
        &events[1..],
        [Event::Segment(segment), Event::Final { .. }] if segment.text == "live 8000"
    ));
    assert!(engine.0.offline.lock().unwrap().is_empty());
}

#[test]
fn windows_are_cut_when_full() {
    let engine = FakeEngine::default();
    let streams = Streams::default();
    let stream = create(&streams, &engine);
    for _ in 0..26 {
        stream.push(&[0.0; SAMPLE_RATE]).unwrap();
    }
    let end = finish(&stream);
    assert_eq!(
        texts(&end),
        [
            format!("live {MAX_WINDOW_SAMPLES}"),
            format!("live {}", 26 * SAMPLE_RATE - MAX_WINDOW_SAMPLES)
        ]
    );
}

#[test]
fn nothing_is_fed_while_dictation_is_busy_and_the_backlog_catches_up_after() {
    let engine = FakeEngine::default();
    let streams = Streams::default();
    let stream = create(&streams, &engine);
    engine.set_busy(true);
    stream.push(&[0.0; 16_000]).unwrap();
    thread::sleep(Duration::from_millis(100));
    assert!(
        engine.lives().is_empty(),
        "a stream opened during dictation"
    );
    let finishing = {
        let stream = stream.clone();
        thread::spawn(move || finish(&stream))
    };
    thread::sleep(Duration::from_millis(100));
    assert!(!finishing.is_finished(), "finished during dictation");
    engine.set_busy(false);
    assert_eq!(texts(&finishing.join().unwrap()), ["live 16000"]);
}

#[test]
fn dictation_closing_a_window_stream_restarts_the_window_after_dictation() {
    let engine = FakeEngine::default();
    let streams = Streams::default();
    let stream = create(&streams, &engine);
    stream.push(&[0.0; 4_800]).unwrap();
    eventually(|| engine.lives().len() == 1 && engine.lives()[0].fed.load(Ordering::SeqCst) > 0);
    engine.preempt();
    stream.push(&[0.0; 3_200]).unwrap();
    thread::sleep(Duration::from_millis(100));
    assert_eq!(engine.lives().len(), 1, "a stream opened during dictation");
    engine.set_busy(false);
    // A new stream decodes the whole window from its start.
    eventually(|| {
        engine
            .lives()
            .get(1)
            .is_some_and(|live| live.fed.load(Ordering::SeqCst) == 8_000)
    });
    assert_eq!(texts(&finish(&stream)), ["live 8000"]);
}

#[test]
fn a_window_whose_stream_gave_way_is_transcribed_offline() {
    let engine = FakeEngine::default();
    let streams = Streams::default();
    let stream = create(&streams, &engine);
    stream.push(&[0.0; 4_800]).unwrap();
    eventually(|| !engine.lives().is_empty());
    engine.preempt();
    engine.0.not_streaming.store(true, Ordering::SeqCst);
    let finishing = {
        let stream = stream.clone();
        thread::spawn(move || finish(&stream))
    };
    thread::sleep(Duration::from_millis(50));
    assert!(
        engine.0.offline.lock().unwrap().is_empty(),
        "ran during dictation"
    );
    engine.set_busy(false);
    assert_eq!(texts(&finishing.join().unwrap()), ["offline 4800"]);
}

#[test]
fn a_model_that_cannot_stream_sends_only_final_segments() {
    let engine = FakeEngine::default();
    engine.0.not_streaming.store(true, Ordering::SeqCst);
    let streams = Streams::default();
    let stream = create(&streams, &engine);
    assert!(!stream.partials);
    stream.push(&[0.0; 3_200]).unwrap();
    assert_eq!(texts(&finish(&stream)), ["offline 3200"]);
}

#[test]
fn one_stream_at_a_time_and_cancel_frees_the_slot() {
    let engine = FakeEngine::default();
    let streams = Streams::default();
    let first = create(&streams, &engine);
    assert!(first.owned_by("owner") && !first.owned_by("other"));
    assert_eq!(
        streams
            .create(engine.clone(), SampleFormat::S16le, selection(), "x".into())
            .err(),
        Some(CreateError::Busy)
    );
    first.cancel();
    assert_eq!(finish(&first), End::Cancelled);
    assert_eq!(first.push(&[0.0]), Err(PushError::Ended));
    let second = create(&streams, &engine);
    assert_ne!(second.id, first.id);
    assert!(streams.get(first.id).is_none());
    assert!(streams.get(second.id).is_some());
}

#[test]
fn a_stream_without_audio_or_heartbeats_expires() {
    let engine = FakeEngine::default();
    let streams = Streams::with_lease(Duration::from_millis(400));
    let kept = create(&streams, &engine);
    for _ in 0..4 {
        thread::sleep(Duration::from_millis(150));
        assert!(kept.heartbeat());
    }
    let end = kept.finish(Duration::from_secs(5), &|| false);
    assert!(end.is_some_and(|end| matches!(end, End::Final { .. })));

    let lapsed = create(&streams, &engine);
    let end = lapsed.finish_after_lease_for_test();
    assert!(matches!(
        end,
        End::Failed {
            code: "lease-expired",
            ..
        }
    ));
}

#[test]
fn audio_is_refused_past_the_backlog_and_after_finishing() {
    let engine = FakeEngine::default();
    engine.set_busy(true);
    let streams = Streams::default();
    let stream = create(&streams, &engine);
    stream.push(&vec![0.0; MAX_BACKLOG_SAMPLES]).unwrap();
    assert_eq!(stream.push(&[0.0]), Err(PushError::BacklogFull));
    stream.cancel();
    finish(&stream);

    engine.set_busy(false);
    let stream = create(&streams, &engine);
    stream.push(&[0.0; 160]).unwrap();
    let end = finish(&stream);
    assert!(matches!(end, End::Final { .. }));
    assert_eq!(stream.push(&[0.0]), Err(PushError::Ended));
}

#[test]
fn a_changed_model_ends_the_stream() {
    let engine = FakeEngine::default();
    let streams = Streams::default();
    let stream = create(&streams, &engine);
    stream.push(&[0.0; 3_200]).unwrap();
    engine.0.lost_model.store(true, Ordering::SeqCst);
    assert!(matches!(
        finish(&stream),
        End::Failed {
            code: MODEL_CHANGED,
            ..
        }
    ));
}

impl Stream {
    /// Waits for the end without finishing, so the lease can lapse.
    fn finish_after_lease_for_test(&self) -> End {
        let mut state = self.shared.lock();
        loop {
            if let Some(end) = &state.end {
                return end.clone();
            }
            state = self.shared.changed.wait_timeout(state, POLL).unwrap().0;
        }
    }
}

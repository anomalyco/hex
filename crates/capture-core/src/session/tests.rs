use std::sync::mpsc::Sender;

use super::*;
use crate::cursor::CursorCollector;
use crate::plan::{AudioPlan, CursorDrawing, Exclusions, Input, Plan, SourcePlan};

/// What the fake stream saw, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Call {
    Detach,
    RemoveHandlers,
    OutputFinished,
    Stop,
    Exclude(Vec<u32>),
}

#[derive(Clone, Copy)]
enum Outcome {
    Finish,
    Fail,
    Silent,
}

struct FakeStream {
    calls: Arc<Mutex<Vec<Call>>>,
    events: Sender<RecordingEvent>,
    /// How long the output takes to report after it is removed.
    finish_after: Duration,
    /// How long removing the output blocks, as `ScreenCaptureKit`'s removal waits for the file.
    detach_blocks: Duration,
    outcome: Outcome,
    stop_hangs: bool,
    exclude_fails: bool,
}

impl FakeStream {
    fn log(&self, call: Call) {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(call);
    }
}

impl SourceStream for FakeStream {
    fn detach_output(&self) -> Result<(), String> {
        self.log(Call::Detach);
        let calls = Arc::clone(&self.calls);
        let events = self.events.clone();
        let finish_after = self.finish_after;
        let outcome = self.outcome;
        thread::spawn(move || {
            thread::sleep(finish_after);
            let event = match outcome {
                Outcome::Finish => RecordingEvent::Finished,
                Outcome::Fail => RecordingEvent::Failed("encoder failed".into()),
                Outcome::Silent => return,
            };
            calls
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(Call::OutputFinished);
            let _ = events.send(event);
        });
        thread::sleep(self.detach_blocks);
        Ok(())
    }

    fn remove_handlers(&mut self) -> Result<(), String> {
        self.log(Call::RemoveHandlers);
        Ok(())
    }

    fn stopper(&self) -> Stopper {
        let calls = Arc::clone(&self.calls);
        let hangs = self.stop_hangs;
        Box::new(move || {
            calls
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(Call::Stop);
            if hangs {
                thread::sleep(Duration::from_secs(2));
            }
            Ok(())
        })
    }

    fn recorded_duration(&self) -> Option<MediaTime> {
        None
    }

    fn exclude(&self, exclude: &Exclusions) -> Result<(), Error> {
        self.log(Call::Exclude(exclude.windows.clone()));
        if self.exclude_fails {
            return Err(Error::Exclusion("the display is gone".into()));
        }
        Ok(())
    }
}

struct Fixture {
    _dir: crate::test_dir::TempDir,
    active: Active,
    calls: Arc<Mutex<Vec<Call>>>,
    events: Sender<RecordingEvent>,
}

fn fixture(configure: impl FnOnce(&mut FakeStream)) -> Fixture {
    let dir = crate::test_dir::tempdir().unwrap();
    let names = Names {
        videos: vec!["recording.mp4".into()],
        ..Names::default()
    };
    fs::write(dir.path().join("recording.mp4"), b"not a movie").unwrap();
    let anchor = HostTime::now();
    let mut active = Active::new(dir.path().to_path_buf(), names, anchor);
    active.timeouts = Timeouts {
        callback: Duration::from_secs(2),
        stream_stop: Duration::from_millis(100),
    };
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (sender, receiver) = mpsc::channel();
    let mut stream = FakeStream {
        calls: Arc::clone(&calls),
        events: sender.clone(),
        finish_after: Duration::from_millis(80),
        detach_blocks: Duration::ZERO,
        outcome: Outcome::Finish,
        stop_hangs: false,
        exclude_fails: false,
    };
    configure(&mut stream);
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
    };
    active.sources.push(SourceRun {
        source: Source::Display(1),
        name: "recording.mp4".into(),
        path: dir.path().join("recording.mp4"),
        stream: Box::new(stream),
        events: RecordingEvents::new(receiver),
        collector: Arc::new(Mutex::new(CursorCollector::new(rect))),
        attached: anchor,
        width: 200,
        height: 200,
        rect,
        audio_tracks: Vec::new(),
    });
    Fixture {
        _dir: dir,
        active,
        calls,
        events: sender,
    }
}

fn recording(active: Active) -> Recording {
    Recording {
        active: Some(active),
        external_sink: None,
    }
}

fn calls(fixture: &Arc<Mutex<Vec<Call>>>) -> Vec<Call> {
    fixture
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

#[test]
fn stop_returns_at_once_while_the_output_finishes() {
    let fixture = fixture(|stream| {
        stream.detach_blocks = Duration::from_millis(200);
        stream.finish_after = Duration::from_millis(300);
    });
    let recording = recording(fixture.active);

    let started = Instant::now();
    let finishing = recording.stop();
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_millis(10), "stop took {elapsed:?}");
    assert!(!finishing.is_finished());

    let finished = finishing.wait().unwrap();
    assert_eq!(finished.error(), None);
    assert_eq!(finished.manifest.sources.len(), 1);
    let manifest = Manifest::read(&finished.dir).unwrap();
    assert_eq!(manifest.clock, Clock::MachHostNs);
    assert!(manifest.anchor_ns <= HostTime::now().as_nanos());
    assert_eq!(
        manifest.sources[0].start_ns, manifest.anchor_ns,
        "with no frame, a source starts where its output was attached"
    );
}

#[test]
fn the_stream_keeps_running_until_the_detached_output_finishes() {
    let fixture = fixture(|_| {});
    let calls_log = Arc::clone(&fixture.calls);
    let finished = recording(fixture.active).stop().wait().unwrap();
    assert_eq!(finished.error(), None);
    assert_eq!(
        calls(&calls_log),
        [
            Call::Detach,
            Call::RemoveHandlers,
            Call::OutputFinished,
            Call::Stop
        ]
    );
}

#[test]
fn exclusions_reach_every_live_stream_without_stopping_it() {
    let mut fixture = fixture(|_| {});
    let window_calls = Arc::new(Mutex::new(Vec::new()));
    let (sender, receiver) = mpsc::channel();
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
    };
    fs::write(fixture.active.dir.join("window.mp4"), b"not a movie").unwrap();
    fixture.active.names.videos.push("window.mp4".into());
    fixture.active.sources.insert(
        0,
        SourceRun {
            source: Source::Window(9),
            name: "window.mp4".into(),
            path: fixture.active.dir.join("window.mp4"),
            stream: Box::new(FakeStream {
                calls: Arc::clone(&window_calls),
                events: sender,
                finish_after: Duration::from_millis(10),
                detach_blocks: Duration::ZERO,
                outcome: Outcome::Finish,
                stop_hangs: false,
                exclude_fails: true,
            }),
            events: RecordingEvents::new(receiver),
            collector: Arc::new(Mutex::new(CursorCollector::new(rect))),
            attached: fixture.active.anchor,
            width: 100,
            height: 100,
            rect,
            audio_tracks: Vec::new(),
        },
    );
    let calls_log = Arc::clone(&fixture.calls);
    let recording = recording(fixture.active);

    let exclude = Exclusions {
        windows: vec![4, 5],
        apps: Vec::new(),
    };
    assert!(matches!(
        recording.set_exclusions(&exclude),
        Err(Error::Exclusion(message)) if message == "the display is gone"
    ));
    // The failing source does not keep the other from updating.
    assert_eq!(calls(&calls_log), [Call::Exclude(vec![4, 5])]);
    assert_eq!(calls(&window_calls), [Call::Exclude(vec![4, 5])]);
    recording
        .set_exclusions(&Exclusions::default())
        .unwrap_err();
    assert_eq!(
        calls(&calls_log),
        [Call::Exclude(vec![4, 5]), Call::Exclude(Vec::new())]
    );

    let finished = recording.stop().wait().unwrap();
    assert_eq!(finished.error(), None);
    assert_eq!(
        calls(&calls_log)[2..],
        [
            Call::Detach,
            Call::RemoveHandlers,
            Call::OutputFinished,
            Call::Stop
        ]
    );
}

#[test]
fn a_failed_output_is_reported_and_the_stream_still_stops() {
    let fixture = fixture(|stream| stream.outcome = Outcome::Fail);
    let calls_log = Arc::clone(&fixture.calls);
    let finished = recording(fixture.active).stop().wait().unwrap();
    assert_eq!(finished.error(), Some("recording failed: encoder failed"));
    assert_eq!(calls(&calls_log).last(), Some(&Call::Stop));
}

#[test]
fn a_system_stream_stop_keeps_a_file_the_output_still_finishes() {
    let fixture = fixture(|_| {});
    fixture
        .events
        .send(RecordingEvent::StreamStopped(
            "stopped by the system".into(),
        ))
        .unwrap();
    let calls_log = Arc::clone(&fixture.calls);
    let finished = recording(fixture.active).stop().wait().unwrap();
    assert_eq!(finished.error(), None);
    // The system already stopped the stream; it is not stopped again.
    assert!(!calls(&calls_log).contains(&Call::Stop));
    assert!(finished.video(0).unwrap().exists());
}

#[test]
fn a_hung_stream_stop_is_bounded() {
    let fixture = fixture(|stream| stream.stop_hangs = true);
    let started = Instant::now();
    let finished = recording(fixture.active).stop().wait().unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(
        finished.error(),
        Some(
            "failed to stop recording: ScreenCaptureKit stopped responding while stopping the recording"
        )
    );
}

#[test]
fn an_output_that_never_reports_times_out() {
    let mut fixture = fixture(|stream| stream.outcome = Outcome::Silent);
    fixture.active.timeouts.callback = Duration::from_millis(50);
    let calls_log = Arc::clone(&fixture.calls);
    let finished = recording(fixture.active).stop().wait().unwrap();
    assert_eq!(
        finished.error(),
        Some("timed out waiting for recording finalization")
    );
    assert_eq!(calls(&calls_log).last(), Some(&Call::Stop));
}

#[test]
fn cancel_removes_the_files_and_writes_no_manifest() {
    let fixture = fixture(|_| {});
    let dir = fixture.active.dir.clone();
    let mut finishing = recording(fixture.active).cancel();
    let finished = loop {
        if let Some(result) = finishing.try_wait() {
            break result.unwrap();
        }
        thread::sleep(Duration::from_millis(5));
    };
    assert!(finishing.try_wait().is_none());
    assert!(!dir.join("recording.mp4").exists());
    assert!(!dir.join(MANIFEST_NAME).exists());
    assert_eq!(finished.manifest.sources.len(), 1);
}

#[test]
fn recording_errors_outrank_ancillary_stop_errors() {
    let error = preferred_stop_error(
        Some(Error::Recording("encoder failed".into())),
        Some(Error::Stop("handler removal failed".into())),
    );
    assert!(matches!(error, Some(Error::Recording(message)) if message == "encoder failed"));

    let error = preferred_stop_error(
        Some(Error::FinishTimeout),
        Some(Error::Stop("stream stop failed".into())),
    );
    assert!(matches!(error, Some(Error::FinishTimeout)));
}

#[test]
fn system_stream_stop_is_distinct_from_output_failure() {
    let (sender, receiver) = mpsc::channel();
    sender
        .send(RecordingEvent::StreamStopped("stopped by system".into()))
        .unwrap();
    sender.send(RecordingEvent::Finished).unwrap();
    let mut events = RecordingEvents::new(receiver);

    assert!(matches!(
        events.health(),
        Err(Error::StreamStopped(message)) if message == "stopped by system"
    ));
    assert!(matches!(
        events.recv_timeout(Duration::ZERO),
        Ok(RecordingEvent::StreamStopped(_))
    ));
    assert!(matches!(
        events.recv_timeout(Duration::ZERO),
        Ok(RecordingEvent::Finished)
    ));
}

#[test]
fn health_reports_failure_without_consuming_the_finalization_event() {
    let (sender, receiver) = mpsc::channel();
    sender.send(RecordingEvent::Started).unwrap();
    sender
        .send(RecordingEvent::Failed("encoder failed".into()))
        .unwrap();
    let mut events = RecordingEvents::new(receiver);

    assert!(matches!(
        events.health(),
        Err(Error::Recording(message)) if message == "encoder failed"
    ));
    assert!(matches!(
        events.health(),
        Err(Error::Recording(message)) if message == "encoder failed"
    ));
    assert!(matches!(
        events.recv_timeout(Duration::ZERO),
        Ok(RecordingEvent::Started)
    ));
    assert!(matches!(
        events.recv_timeout(Duration::ZERO),
        Ok(RecordingEvent::Failed(message)) if message == "encoder failed"
    ));
}

#[test]
fn stop_stream_within_reports_an_unresponsive_stop() {
    let result = stop_stream_within(
        Box::new(|| {
            thread::sleep(Duration::from_secs(2));
            Ok(())
        }),
        Duration::from_millis(20),
    );
    assert_eq!(result, Err(STREAM_STOP_UNRESPONSIVE.to_owned()));
    assert_eq!(
        stop_stream_within(Box::new(|| Err("no".into())), Duration::from_secs(1)),
        Err("no".to_owned())
    );
}

fn plan(dir: &Path) -> Plan {
    Plan {
        sources: vec![Source::Display(1).into()],
        cursor: CursorDrawing::Hidden,
        audio: AudioPlan::default(),
        input: Input::None,
        frames: None,
        exclude: Exclusions::default(),
        fps: 60,
        dir: dir.to_path_buf(),
        names: Names {
            videos: vec!["recording.mp4".into()],
            ..Names::default()
        },
    }
}

#[test]
fn an_invalid_plan_is_refused_before_touching_the_screen() {
    let dir = crate::test_dir::tempdir().unwrap();
    let mut zero_fps = plan(dir.path());
    zero_fps.fps = 0;
    assert!(matches!(
        crate::Recorder::start(zero_fps),
        Err(Error::InvalidFrameRate)
    ));

    let mut duplicates = plan(dir.path());
    duplicates.sources = vec![Source::Display(1).into(), Source::Display(1).into()];
    duplicates.names.videos = vec!["a.mp4".into(), "b.mp4".into()];
    assert!(matches!(
        crate::Recorder::start(duplicates),
        Err(Error::DuplicateSources)
    ));

    fs::write(dir.path().join("recording.mp4"), b"keep me").unwrap();
    assert!(matches!(
        crate::Recorder::start(plan(dir.path())),
        Err(Error::Output(error)) if error.kind() == std::io::ErrorKind::AlreadyExists
    ));
    assert_eq!(
        fs::read(dir.path().join("recording.mp4")).unwrap(),
        b"keep me"
    );
}

/// Needs Screen Recording permission and a display; records one second of the main display.
#[test]
#[ignore = "needs Screen Recording permission (TCC) and a live display"]
fn records_the_main_display() {
    let dir = crate::test_dir::tempdir().unwrap();
    let main = objc2_core_graphics::CGMainDisplayID();
    let mut plan = plan(dir.path());
    plan.sources = vec![Source::Display(main).into()];
    plan.names.cursor = Some("cursor.json".into());
    let recording = crate::Recorder::start(plan).unwrap();
    thread::sleep(Duration::from_secs(1));
    let finished = recording.stop().wait().unwrap();
    assert_eq!(finished.error(), None);
    assert!(finished.manifest.sources[0].duration_ms >= 900);
}

/// Needs Screen Recording permission and a display; records a region at a capped size.
#[test]
#[ignore = "needs Screen Recording permission (TCC) and a live display"]
fn records_a_region_at_its_long_edge() {
    let dir = crate::test_dir::tempdir().unwrap();
    let main = objc2_core_graphics::CGMainDisplayID();
    let mut plan = plan(dir.path());
    plan.sources = vec![SourcePlan {
        source: Source::Region {
            display: main,
            rect: crate::Rect {
                x: 100.0,
                y: 100.0,
                width: 800.0,
                height: 400.0,
            },
        },
        long_edge: Some(1_280),
    }];
    let recording = crate::Recorder::start(plan).unwrap();
    thread::sleep(Duration::from_secs(1));
    let finished = recording.stop().wait().unwrap();
    assert_eq!(finished.error(), None);
    let source = &finished.manifest.sources[0];
    assert!(source.width <= 1_280 && source.width >= source.height * 2 - 2);
}

/// Needs Screen Recording permission and a display.
#[test]
#[ignore = "needs Screen Recording permission (TCC) and a live display"]
fn screenshots_the_main_display() {
    let main = objc2_core_graphics::CGMainDisplayID();
    let image =
        crate::screenshot(&Source::Display(main), &Exclusions::default(), Some(640)).unwrap();
    assert_eq!(image.width.max(image.height), 640);
    assert_eq!(
        image.rgba.len(),
        image.width as usize * image.height as usize * 4
    );
}

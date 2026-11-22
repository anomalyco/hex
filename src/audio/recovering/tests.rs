use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, eyre};

use crate::audio::CaptureInstant;
use crate::audio::hex_microphone;
use crate::audio::open::OpenRequest;
use crate::audio::recovery::{MICROPHONE_RETRY_INITIAL, MicrophoneRecovery};
use crate::audio::routes::notify_audio_service_restarted;
use crate::audio::stream::{AudioInput, Opened, STREAM_PROVEN_UPTIME, Selection};

use super::*;

#[test]
fn a_microphone_that_cannot_open_is_reported_once_per_outage() {
    let mut input = RecoveringAudioInput::failed_for_test("Gone microphone");
    let failure = input
        .take_failure()
        .expect("the startup failure is reported");
    assert!(failure.contains("Gone microphone"), "{failure}");
    assert_eq!(input.take_failure(), None);
    // Retries within the same outage stay quiet.
    input.outage.get_or_insert(Some("still gone".into()));
    assert_eq!(input.take_failure(), None);
    // Closing ends the outage; the next one is reported again.
    input.close();
    input.outage.get_or_insert(Some("gone again".into()));
    assert_eq!(input.take_failure().as_deref(), Some("gone again"));
}

static TEST_ROUTE_GENERATION: AtomicU64 = AtomicU64::new(0);

fn test_route_generation() -> u64 {
    TEST_ROUTE_GENERATION.load(Ordering::Acquire)
}

/// A route generation only this test changes, so another test's route
/// change cannot open the stream it is about to fail.
static DEAD_STREAM_ROUTE_GENERATION: AtomicU64 = AtomicU64::new(0);

fn dead_stream_route_generation() -> u64 {
    DEAD_STREAM_ROUTE_GENERATION.load(Ordering::Acquire)
}

/// Polls until any open the input starts has finished, returning the
/// last event.
fn settle(input: &mut RecoveringAudioInput, capture_idle: bool) -> RecoveringAudioInputEvent {
    let mut event = input.recv_timeout(Duration::ZERO, capture_idle);
    let deadline = Instant::now() + Duration::from_secs(5);
    while input.is_opening() && Instant::now() < deadline {
        event = input.recv_timeout(Duration::from_millis(1), capture_idle);
    }
    event
}

fn named_input(name: &str) -> AudioInput {
    let (mut input, samples) = AudioInput::channel_for_test();
    // A dropped sender reads as a failed stream.
    std::mem::forget(samples);
    input.device_name = name.into();
    input
}

#[test]
fn a_default_change_switches_only_while_capture_is_idle() {
    let default = Arc::new(Mutex::new("ALEX PODS".to_owned()));
    let opener_default = default.clone();
    let mut input =
        RecoveringAudioInput::with_opener_for_test(None, test_route_generation, move |request| {
            let name = opener_default.lock().unwrap().clone();
            if request.keep.as_deref() == Some(name.as_str()) {
                return Ok(Opened::Unchanged {
                    selection: Selection::InUse,
                });
            }
            Ok(Opened::Input(Box::new(named_input(&name))))
        });
    input.finish_initial_open(Ok(named_input("ALEX PODS")), Instant::now());

    // The OS default changes during a recording: nothing opens yet.
    *default.lock().unwrap() = "MacBook Pro Microphone".into();
    TEST_ROUTE_GENERATION.fetch_add(1, Ordering::AcqRel);
    assert!(matches!(
        settle(&mut input, false),
        RecoveringAudioInputEvent::Timeout
    ));
    assert!(!input.is_opening());
    assert_eq!(input.device_name(), "ALEX PODS");

    // Once capture is idle, the new default replaces the old one.
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Reopened
    ));
    assert_eq!(input.device_name(), "MacBook Pro Microphone");

    // A device change that leaves the default alone keeps the stream.
    TEST_ROUTE_GENERATION.fetch_add(1, Ordering::AcqRel);
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Timeout
    ));
    assert!(input.recovery.reason.is_none());
    assert_eq!(input.device_name(), "MacBook Pro Microphone");
}

#[test]
fn a_voice_isolation_change_reopens_only_while_capture_is_idle() {
    let opens = Arc::new(Mutex::new(Vec::new()));
    let keeps = Arc::new(Mutex::new(Vec::new()));
    let recorded_opens = opens.clone();
    let recorded_keeps = keeps.clone();
    let mut input =
        RecoveringAudioInput::with_opener_for_test(None, test_route_generation, move |request| {
            recorded_opens
                .lock()
                .unwrap()
                .push(request.voice_processing);
            recorded_keeps.lock().unwrap().push(request.keep.clone());
            if request.keep.as_deref() == Some("USB Mic") {
                return Ok(Opened::Unchanged {
                    selection: Selection::InUse,
                });
            }
            let mut opened = named_input("USB Mic");
            opened.voice_isolation = request.voice_processing;
            Ok(Opened::Input(Box::new(opened)))
        });
    input.finish_initial_open(Ok(named_input("USB Mic")), Instant::now());
    assert!(!input.voice_isolation);

    // A flip while a capture runs changes no stream and opens nothing.
    input.request_voice_isolation(true, false);
    assert!(input.voice_isolation);
    assert!(!input.is_opening());
    assert!(opens.lock().unwrap().is_empty());
    assert!(!input.input.as_ref().unwrap().voice_isolation);

    // Idle, the same device reopens through voice processing rather than
    // being kept as unchanged.
    input.request_voice_isolation(true, true);
    assert!(input.is_opening());
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Reopened
    ));
    assert_eq!(*opens.lock().unwrap(), vec![true]);
    assert_eq!(*keeps.lock().unwrap(), vec![None]);
    assert!(input.input.as_ref().unwrap().voice_isolation);

    // Asking for the path already open changes nothing.
    input.request_voice_isolation(true, true);
    assert!(!input.is_opening());

    // Off reopens the raw stream.
    input.request_voice_isolation(false, true);
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Reopened
    ));
    assert_eq!(*opens.lock().unwrap(), vec![true, false]);
    assert!(!input.input.as_ref().unwrap().voice_isolation);
}

/// An input saved as "USB Mic" that opens once `connected` is set and
/// until then stands in with "ALEX PODS" for the given reason.
fn standing_in(
    missing: Selection,
    reports_new_devices: bool,
    route_generation: fn() -> u64,
) -> (
    RecoveringAudioInput,
    Arc<std::sync::atomic::AtomicBool>,
    Instant,
) {
    let connected = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let opener_connected = connected.clone();
    let mut input = RecoveringAudioInput::with_opener_for_test(
        Some("USB Mic"),
        route_generation,
        move |request| {
            let (name, selection) = if opener_connected.load(Ordering::Acquire) {
                ("USB Mic", Selection::InUse)
            } else {
                ("ALEX PODS", missing)
            };
            if request.keep.as_deref() == Some(name) {
                return Ok(Opened::Unchanged { selection });
            }
            let mut opened = named_input(name);
            opened.selection = selection;
            Ok(Opened::Input(Box::new(opened)))
        },
    );
    input.reports_new_devices = reports_new_devices;
    let mut stand_in = named_input("ALEX PODS");
    stand_in.selection = missing;
    let started = Instant::now();
    input.finish_initial_open(Ok(stand_in), started);
    (input, connected, started)
}

#[test]
fn a_missing_selection_uses_the_default_until_it_returns() {
    let (mut input, connected, started) = standing_in(Selection::Unlisted, false, || 0);
    assert_eq!(
        input.recovery.next_attempt,
        Some(started + MICROPHONE_RETRY_INITIAL)
    );

    // Still missing: the default keeps running and the next look backs off.
    input.recovery.next_attempt = Some(Instant::now());
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Timeout
    ));
    assert_eq!(input.device_name(), "ALEX PODS");
    assert_eq!(input.selection_retry, MICROPHONE_RETRY_INITIAL * 4);

    connected.store(true, Ordering::Release);
    input.recovery.next_attempt = Some(Instant::now());
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Reopened
    ));
    assert_eq!(input.device_name(), "USB Mic");
    assert_eq!(input.input.as_ref().unwrap().selection, Selection::InUse);
    assert!(input.recovery.next_attempt.is_none());
    assert_eq!(input.selected_device.as_deref(), Some("USB Mic"));
}

static DEVICE_ARRIVALS: AtomicU64 = AtomicU64::new(0);

#[test]
fn where_the_os_reports_new_devices_only_a_failed_open_is_retried() {
    let arrivals = || DEVICE_ARRIVALS.load(Ordering::Acquire);

    // An unlisted microphone waits for the OS to report a device.
    let (mut input, connected, _) = standing_in(Selection::Unlisted, true, arrivals);
    assert!(input.recovery.next_attempt.is_none());
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Timeout
    ));
    assert!(!input.is_opening());
    connected.store(true, Ordering::Release);
    DEVICE_ARRIVALS.fetch_add(1, Ordering::AcqRel);
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Reopened
    ));
    assert_eq!(input.device_name(), "USB Mic");

    // A listed microphone that would not open is looked at again.
    let (input, _, started) = standing_in(Selection::Unopenable, true, arrivals);
    assert_eq!(
        input.recovery.next_attempt,
        Some(started + MICROPHONE_RETRY_INITIAL)
    );
}

#[test]
fn closed_input_applies_selection_without_opening() {
    let mut input = RecoveringAudioInput::closed(None, 3, Some("Old microphone"));

    input.request_selection(4, Some("Next microphone"));

    assert!(!input.is_open());
    assert_eq!(input.active_revision, 4);
    assert_eq!(input.selected_device.as_deref(), Some("Next microphone"));
    assert!(input.replacement.is_none());
}

#[test]
fn failed_startup_retry_keeps_backoff_without_an_open_stream() {
    let mut input = RecoveringAudioInput::closed(None, 3, Some("Missing microphone"));
    let started = Instant::now();
    input.finish_initial_open(Err(eyre!("device unavailable")), started);
    assert!(input.is_recovering());
    assert!(
        !input
            .recovery
            .should_attempt(started + Duration::from_millis(249))
    );
    assert!(
        input
            .recovery
            .should_attempt(started + Duration::from_millis(250))
    );
    let (sender, receiver) = mpsc::channel();
    input.replacement = Some(receiver);
    sender.send((3, Err("device unavailable".into()))).unwrap();

    let before = Instant::now();
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Timeout
    ));
    let after = Instant::now();
    assert!(!input.is_open());
    assert!(!input.is_opening());
    assert!(input.is_recovering());
    let next_attempt = input.recovery.next_attempt.unwrap();
    assert!(next_attempt >= before + Duration::from_millis(500));
    assert!(next_attempt <= after + Duration::from_millis(500));

    input.cancel_open();
    input.request_open();
    assert!(input.is_recovering());
    assert!(!input.is_opening());
    assert_eq!(input.recovery.next_attempt, Some(next_attempt));

    input.request_selection(4, Some("Next microphone"));
    assert!(input.is_recovering());
    assert_eq!(input.recovery.target_revision, Some(4));
    assert_eq!(input.active_revision, 3);
    assert_eq!(input.selected_device.as_deref(), Some("Next microphone"));
    assert!(input.recovery.should_attempt(Instant::now()));
}

#[test]
fn only_the_listeners_current_input_feeds_hex_microphone() {
    let (meeting, _) = AudioInput::channel_for_test();
    let (first, _) = AudioInput::channel_for_test();
    let (replacement, _) = AudioInput::channel_for_test();
    let first_feed = first.hex_microphone.clone();
    let replacement_feed = replacement.hex_microphone.clone();

    let mut input = RecoveringAudioInput::closed(None, 3, Some("Test microphone"));
    input.finish_initial_open(Ok(first), Instant::now());
    assert!(first_feed.is_on());
    assert!(!meeting.hex_microphone.is_on());

    let (sender, receiver) = mpsc::channel();
    input.replacement = Some(receiver);
    sender
        .send((3, Ok(Opened::Input(Box::new(replacement)))))
        .unwrap();
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Reopened
    ));
    assert!(!first_feed.is_on());
    assert!(replacement_feed.is_on());

    input.close();
    assert!(!replacement_feed.is_on());
}

#[test]
fn startup_retry_recovers_metadata_and_audio_without_restarting() {
    let mut input = RecoveringAudioInput::closed(None, 3, Some("Test microphone"));
    input.finish_initial_open(Err(eyre!("device unavailable")), Instant::now());
    let (sender, receiver) = mpsc::channel();
    input.replacement = Some(receiver);
    let (opened, samples) = AudioInput::channel_for_test();
    sender
        .send((3, Ok(Opened::Input(Box::new(opened)))))
        .unwrap();

    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Reopened
    ));
    assert!(!input.is_recovering());
    assert!(!input.is_opening());
    assert!(input.recovery.next_attempt.is_none());
    assert_eq!(input.sample_rate(), 48_000);
    assert_eq!(input.device_name(), "Test microphone");
    let at = CaptureInstant::from_nanos(60_000_000_000);
    samples.send((vec![0.25; 480], at)).unwrap();
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Chunk { samples, captured_through }
            if samples == vec![0.25; 480] && captured_through == at
    ));
}

#[test]
fn startup_recovery_opens_even_when_a_capture_is_pending() {
    let mut input = RecoveringAudioInput::closed(
        Some("Hex nonexistent microphone for pending recovery test"),
        3,
        None,
    );
    input.request_recovery();
    input.request_selection(4, Some("Ignored selection under CLI override"));
    assert_eq!(input.active_revision, 3);
    assert!(input.selected_device.is_none());

    assert!(matches!(
        input.recv_timeout(Duration::ZERO, false),
        RecoveringAudioInputEvent::Timeout
    ));
    assert!(input.is_opening());
    input.cancel_open();
    assert!(input.is_opening());
    input.close();
    assert!(!input.is_opening());
    assert!(!input.is_recovering());
    assert!(input.recovery.next_attempt.is_none());
}

#[test]
fn failed_on_demand_open_does_not_retry_while_idle() {
    let mut input = RecoveringAudioInput::closed(None, 3, None);
    let (sender, receiver) = mpsc::channel();
    input.replacement = Some(receiver);
    sender.send((3, Err("device unavailable".into()))).unwrap();

    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::OpenFailed(error) if error == "device unavailable"
    ));
    assert!(!input.is_recovering());
    assert!(!input.is_opening());
    assert!(input.recovery.next_attempt.is_none());
}

#[test]
fn microphone_recovery_retries_with_bounded_backoff_and_resets() {
    let started = Instant::now();
    let mut recovery = MicrophoneRecovery::default();

    recovery.request_selection_change(1, started);
    assert!(!recovery.blocks_audio());
    assert!(recovery.should_attempt(started));

    recovery.failed(started);
    recovery.request_selection_change(1, started + Duration::from_millis(100));
    assert!(!recovery.should_attempt(started + Duration::from_millis(249)));
    assert!(recovery.should_attempt(started + Duration::from_millis(250)));

    let mut now = started + Duration::from_millis(250);
    for expected_delay in [500, 1_000, 2_000, 4_000, 5_000, 5_000] {
        recovery.failed(now);
        assert!(!recovery.should_attempt(now + Duration::from_millis(expected_delay - 1)));
        now += Duration::from_millis(expected_delay);
        assert!(recovery.should_attempt(now));
    }

    recovery.recovered();
    assert!(!recovery.blocks_audio());
    assert!(!recovery.should_attempt(now));
    recovery.request_stream_recovery(now);
    assert!(recovery.blocks_audio());
    recovery.failed(now);
    recovery.request_stream_recovery(now + Duration::from_millis(100));
    recovery.request_selection_change(2, now + Duration::from_millis(100));
    assert!(recovery.blocks_audio());
    assert!(recovery.should_attempt(now + Duration::from_millis(100)));
}

#[test]
fn audio_service_restart_interrupts_stream_and_recovers() {
    let mut input = RecoveringAudioInput::closed(None, 1, Some("Test microphone"));
    let (mut opened, _samples) = AudioInput::channel_for_test();
    opened.watch_service_restarts_for_test();
    input.finish_initial_open(Ok(opened), Instant::now());
    assert!(input.is_open());

    notify_audio_service_restarted();
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Interrupted
    ));
    assert!(input.is_recovering());

    let (sender, receiver) = mpsc::channel();
    input.replacement = Some(receiver);
    let (reopened, samples_after) = AudioInput::channel_for_test();
    sender
        .send((1, Ok(Opened::Input(Box::new(reopened)))))
        .unwrap();

    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Reopened
    ));
    assert!(!input.is_recovering());
    let at = CaptureInstant::from_nanos(10_000_000_000);
    samples_after.send((vec![0.5; 240], at)).unwrap();
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Chunk { samples, captured_through }
            if samples == vec![0.5; 240] && captured_through == at
    ));
}

#[test]
fn a_stream_that_stops_delivering_audio_is_reopened() {
    let stall = Duration::from_millis(400);
    let mut input = RecoveringAudioInput::closed(None, 1, Some("Test microphone"));
    let (mut opened, samples) = AudioInput::channel_for_test();
    opened.watch_stalls_for_test(stall);
    input.finish_initial_open(Ok(opened), Instant::now());

    // Delivering audio keeps the stream alive.
    let at = CaptureInstant::from_nanos(10_000_000_000);
    std::thread::sleep(stall / 4);
    samples.send((vec![0.5; 240], at)).unwrap();
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Chunk { .. }
    ));
    std::thread::sleep(stall / 4);
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Timeout
    ));

    // A quiet stream with no error counts as failed and recovers.
    std::thread::sleep(stall);
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Interrupted
    ));
    assert!(input.is_recovering());
}

/// What the opener saw of the previous stream's feed switch when it ran.
fn recording_opener(
    seen: &Arc<Mutex<Option<bool>>>,
    switch: &hex_microphone::CaptureFeedSwitch,
) -> impl Fn(&OpenRequest) -> Result<Opened> + Send + Sync + 'static {
    let seen = seen.clone();
    let switch = switch.clone();
    move |_| {
        *seen.lock().unwrap() = Some(switch.is_on());
        Ok(Opened::Input(Box::new(named_input(
            "Replacement microphone",
        ))))
    }
}

#[test]
fn a_dead_stream_is_stopped_before_its_replacement_opens() {
    // A failed stream lets go of the device before its replacement opens.
    let (failed, samples) = AudioInput::channel_for_test();
    let failed_switch = failed.hex_microphone.clone();
    let seen = Arc::new(Mutex::new(None));
    let mut input = RecoveringAudioInput::with_opener_for_test(
        None,
        dead_stream_route_generation,
        recording_opener(&seen, &failed_switch),
    );
    input.finish_initial_open(Ok(failed), Instant::now());
    drop(samples);
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Interrupted
    ));
    // A stream that never delivered waits out its first backoff; let the
    // wait pass.
    input.recovery.next_attempt = Some(Instant::now());
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Reopened
    ));
    assert_eq!(*seen.lock().unwrap(), Some(false));
    assert!(!failed_switch.is_on());

    // A route change still keeps the old stream until the new one installs.
    let (old, samples) = AudioInput::channel_for_test();
    let old_switch = old.hex_microphone.clone();
    let seen = Arc::new(Mutex::new(None));
    let mut routed = RecoveringAudioInput::with_opener_for_test(
        None,
        dead_stream_route_generation,
        recording_opener(&seen, &old_switch),
    );
    routed.finish_initial_open(Ok(old), Instant::now());
    std::mem::forget(samples);
    DEAD_STREAM_ROUTE_GENERATION.fetch_add(1, Ordering::AcqRel);
    assert!(matches!(
        settle(&mut routed, true),
        RecoveringAudioInputEvent::Reopened
    ));
    assert_eq!(*seen.lock().unwrap(), Some(true));
    assert!(!old_switch.is_on());
}

/// A route generation only the silence test changes, so another test's
/// route change cannot reopen its microphone.
static SILENCE_ROUTE_GENERATION: AtomicU64 = AtomicU64::new(0);

fn silence_route_generation() -> u64 {
    SILENCE_ROUTE_GENERATION.load(Ordering::Acquire)
}

#[test]
fn a_silent_stream_reopens_with_a_rate_renegotiation() {
    let renegotiations = Arc::new(Mutex::new(Vec::new()));
    let seen = renegotiations.clone();
    let mut input = RecoveringAudioInput::with_opener_for_test(
        None,
        silence_route_generation,
        move |request| {
            seen.lock().unwrap().push(request.renegotiate_rate);
            Ok(Opened::Input(Box::new(named_input("Test microphone"))))
        },
    );
    input.finish_initial_open(Ok(named_input("Test microphone")), Instant::now());

    // A dictation of digital silence reopens the microphone, and that open
    // renegotiates the device's sample rate first (#1575).
    input.request_silence_recovery();
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Reopened
    ));
    // A stream failure reopens the same microphone without it.
    input.request_recovery();
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Reopened
    ));
    // Only the silence open renegotiated; the next open took nothing.
    assert_eq!(*renegotiations.lock().unwrap(), vec![true, false]);
}

/// A route generation only the stall tests change, so another test's
/// route change cannot end a stalled stream's wait.
static STALLED_ROUTE_GENERATION: AtomicU64 = AtomicU64::new(0);

fn stalled_route_generation() -> u64 {
    STALLED_ROUTE_GENERATION.load(Ordering::Acquire)
}

/// A stream whose sender is gone: polling it reads as a failed stream,
/// with no device behind it.
fn stalling_input() -> AudioInput {
    let (mut input, samples) = AudioInput::channel_for_test();
    drop(samples);
    input.device_name = "Stalling microphone".into();
    input
}

/// A stream's chunk sender, kept so a test can stop the stream.
type ChunkSender = Sender<(Vec<f32>, CaptureInstant)>;

/// Opens a live stream and keeps its sender, so a test can stop it.
fn opener_keeping_sender(
    sender: &Arc<Mutex<Option<ChunkSender>>>,
) -> impl Fn(&OpenRequest) -> Result<Opened> + Send + Sync + 'static {
    let sender = sender.clone();
    move |_| {
        let (mut input, samples) = AudioInput::channel_for_test();
        input.device_name = "Reopened microphone".into();
        *sender.lock().unwrap() = Some(samples);
        Ok(Opened::Input(Box::new(input)))
    }
}

#[test]
fn a_stream_that_keeps_stalling_backs_off_to_a_minute() {
    let mut input = RecoveringAudioInput::with_opener_for_test(
        None,
        || 0,
        |_| Ok(Opened::Input(Box::new(stalling_input()))),
    );
    input.finish_initial_open(Ok(stalling_input()), Instant::now());

    let mut delays = Vec::new();
    for _ in 0..10 {
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Interrupted
        ));
        delays.push(input.recovery.delay);
        // Let the wait pass without sleeping; the reopen keeps it.
        input.recovery.next_attempt = Some(Instant::now());
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Reopened
        ));
    }
    assert_eq!(
        delays,
        [
            500, 1_000, 2_000, 4_000, 8_000, 16_000, 32_000, 60_000, 60_000, 60_000
        ]
        .map(Duration::from_millis)
    );

    // The last wait is a full minute, not the next stall timeout.
    let failed_at = Instant::now();
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Interrupted
    ));
    assert!(input.recovery.next_attempt.unwrap() >= failed_at + Duration::from_secs(60));
}

#[test]
fn a_reopened_stream_keeps_the_stall_backoff_its_predecessors_grew() {
    let fails = Arc::new(AtomicBool::new(false));
    let opener_fails = fails.clone();
    let mut input = RecoveringAudioInput::with_opener_for_test(
        None,
        || 0,
        move |_| {
            if opener_fails.load(Ordering::Acquire) {
                return Err(eyre!("microphone is busy"));
            }
            Ok(Opened::Input(Box::new(stalling_input())))
        },
    );
    input.finish_initial_open(Ok(stalling_input()), Instant::now());

    // Six streams that never deliver take the wait to 16 s.
    for expected in [500, 1_000, 2_000, 4_000, 8_000, 16_000] {
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Interrupted
        ));
        assert_eq!(input.recovery.delay, Duration::from_millis(expected));
        input.recovery.next_attempt = Some(Instant::now());
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Reopened
        ));
        assert_eq!(input.recovery.delay, Duration::from_millis(expected));
        assert!(input.recovery.next_attempt.is_none());
    }

    // The reopened stream stops too, and its failure waits the grown 16 s.
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Interrupted
    ));
    assert_eq!(input.recovery.delay, Duration::from_secs(32));
    assert!(!input.recovery.should_attempt(Instant::now()));

    // Let that wait pass into a failed open: it waits the grown 32 s and
    // keeps growing to the minute, not back to the five-second open cap.
    fails.store(true, Ordering::Release);
    input.recovery.next_attempt = Some(Instant::now());
    let before = Instant::now();
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Timeout
    ));
    assert!(input.is_opening());
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Timeout
    ));
    assert!(input.recovery.next_attempt.unwrap() >= before + Duration::from_secs(32));
    assert_eq!(input.recovery.delay, Duration::from_secs(60));
}

#[test]
fn a_stream_that_delivers_steadily_reopens_at_once_and_forgets_the_backoff() {
    let sender = Arc::new(Mutex::new(None));
    let mut input =
        RecoveringAudioInput::with_opener_for_test(None, || 0, opener_keeping_sender(&sender));
    input.finish_initial_open(Ok(stalling_input()), Instant::now());
    // Two streams that never deliver take the wait to 1 s.
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Interrupted
    ));
    input.recovery.next_attempt = Some(Instant::now());
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Reopened
    ));
    // The reopened stream stops before it delivers anything either.
    drop(sender.lock().unwrap().take());
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Interrupted
    ));
    input.recovery.next_attempt = Some(Instant::now());
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Reopened
    ));
    assert_eq!(input.recovery.delay, Duration::from_secs(1));

    // Delivering across the proof window forgets that wait.
    input.first_delivery = Some(Instant::now() - STREAM_PROVEN_UPTIME);
    let at = CaptureInstant::from_nanos(30_000_000_000);
    sender
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .send((vec![0.5; 240], at))
        .unwrap();
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Chunk { .. }
    ));
    assert_eq!(input.recovery.delay, MICROPHONE_RETRY_INITIAL);

    // Its failure reopens at once, as a working stream always has, with
    // the backoff it no longer needs.
    drop(sender.lock().unwrap().take());
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Interrupted
    ));
    assert!(input.recovery.should_attempt(Instant::now()));
    assert_eq!(input.recovery.delay, MICROPHONE_RETRY_INITIAL);
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Reopened
    ));
}

#[test]
fn a_stream_that_stops_before_proving_itself_backs_off() {
    let sender = Arc::new(Mutex::new(None));
    let mut input =
        RecoveringAudioInput::with_opener_for_test(None, || 0, opener_keeping_sender(&sender));
    input.finish_initial_open(Ok(stalling_input()), Instant::now());
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Interrupted
    ));
    input.recovery.next_attempt = Some(Instant::now());
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Reopened
    ));

    // One chunk is not proof: the stream stopped before delivering across
    // the proof window, so its next reopen backs off instead of looping.
    let at = CaptureInstant::from_nanos(1_000_000_000);
    sender
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .send((vec![0.5; 240], at))
        .unwrap();
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Chunk { .. }
    ));
    assert!(!input.proven);
    let failed_at = Instant::now();
    drop(sender.lock().unwrap().take());
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Interrupted
    ));
    assert!(!input.recovery.should_attempt(Instant::now()));
    assert!(input.recovery.next_attempt.unwrap() >= failed_at + Duration::from_millis(500));
    assert_eq!(input.recovery.delay, Duration::from_secs(1));
}

#[test]
fn a_route_change_ends_a_stall_wait_without_resetting_its_backoff() {
    let connected = Arc::new(AtomicBool::new(false));
    let opener_connected = connected.clone();
    let mut input =
        RecoveringAudioInput::with_opener_for_test(None, stalled_route_generation, move |_| {
            let opened = if opener_connected.load(Ordering::Acquire) {
                named_input("Next microphone")
            } else {
                stalling_input()
            };
            Ok(Opened::Input(Box::new(opened)))
        });
    input.finish_initial_open(Ok(stalling_input()), Instant::now());
    for _ in 0..2 {
        assert!(matches!(
            input.recv_timeout(Duration::ZERO, true),
            RecoveringAudioInputEvent::Interrupted
        ));
        input.recovery.next_attempt = Some(Instant::now());
        assert!(matches!(
            settle(&mut input, true),
            RecoveringAudioInputEvent::Reopened
        ));
    }

    // The next failed stream waits out the grown backoff.
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Interrupted
    ));
    assert_eq!(input.recovery.delay, Duration::from_secs(2));
    assert!(!input.recovery.should_attempt(Instant::now()));
    // The dead stream was released when its wait began.
    assert!(!input.is_open());

    // The lid opening resolves the input at once, not in two seconds,
    // and keeps the wait the stalled streams grew.
    connected.store(true, Ordering::Release);
    STALLED_ROUTE_GENERATION.fetch_add(1, Ordering::AcqRel);
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Timeout
    ));
    assert!(input.is_opening());
    assert_eq!(input.recovery.delay, Duration::from_secs(2));
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Reopened
    ));
    assert_eq!(input.device_name(), "Next microphone");
    assert_eq!(input.recovery.delay, Duration::from_secs(2));
}

#[test]
fn a_press_waiting_for_a_stalled_stream_reopens_it_anyway() {
    let mut input = RecoveringAudioInput::with_opener_for_test(
        None,
        || 0,
        |_| Ok(Opened::Input(Box::new(named_input("Reopened microphone")))),
    );
    input.finish_initial_open(Ok(stalling_input()), Instant::now());
    // A stream that delivered steadily keeps its device until its
    // replacement opens, so the failed stream is still installed.
    input.proven = true;
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Interrupted
    ));
    assert!(input.input.is_some());
    assert!(input.is_recovering());

    // The press asks for it now, and keeps `capture_idle` false while it
    // waits; the replacement must still start.
    input.retry_now();
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, false),
        RecoveringAudioInputEvent::Timeout
    ));
    assert!(input.is_opening());
    assert!(matches!(
        settle(&mut input, false),
        RecoveringAudioInputEvent::Reopened
    ));
    assert_eq!(input.device_name(), "Reopened microphone");
}

#[test]
fn a_dictation_press_retries_a_stalled_microphone_at_once() {
    let mut input = RecoveringAudioInput::with_opener_for_test(
        None,
        || 0,
        |_| Ok(Opened::Input(Box::new(stalling_input()))),
    );
    input.finish_initial_open(Ok(stalling_input()), Instant::now());
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Interrupted
    ));
    assert!(!input.recovery.should_attempt(Instant::now()));
    let delay = input.recovery.delay;

    input.retry_now();
    assert!(input.recovery.should_attempt(Instant::now()));
    assert!(matches!(
        input.recv_timeout(Duration::ZERO, true),
        RecoveringAudioInputEvent::Timeout
    ));
    assert!(input.is_opening());
    // The press opens the microphone now; it does not start the backoff
    // over for the streams that keep stalling.
    assert_eq!(input.recovery.delay, delay);
    assert!(matches!(
        settle(&mut input, true),
        RecoveringAudioInputEvent::Reopened
    ));
    assert_eq!(input.recovery.delay, delay);
}

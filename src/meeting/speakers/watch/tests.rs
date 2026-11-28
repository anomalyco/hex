use std::fs;

use super::*;
use crate::accessibility::tree::Node;
use crate::meeting::speakers::SpeakerName;
use crate::meeting::speakers::adapter::{AppParticipantId, Observation, ObservedParticipant};

fn folder(name: &str) -> std::path::PathBuf {
    let folder =
        std::env::temp_dir().join(format!("hex-speaker-watch-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    folder
}

/// A clock whose recording has audio from `first_audio_us` on, with the
/// lab on, and the end that stops it.
fn clock(first_audio_us: u64) -> (Sender<()>, Clock, Arc<AtomicU64>) {
    let (stop, stopped) = mpsc::channel();
    let first_audio = Arc::new(AtomicU64::new(first_audio_us));
    let clock = Clock {
        capture: Instant::now(),
        first_audio: first_audio.clone(),
        stopped,
        reading: || true,
    };
    (stop, clock, first_audio)
}

/// An invented call app that always shows Alice speaking.
fn alice_speaking(_: &Snapshot) -> Observation {
    let id = AppParticipantId::parse("U0002ALICE").unwrap();
    Observation::Active {
        roster: vec![ObservedParticipant {
            id: id.clone(),
            name: SpeakerName::parse("Alice"),
            is_local: false,
        }],
        floor: Some(id),
        local_speaking: false,
    }
}

fn a_window() -> Snapshot {
    Snapshot {
        windows: vec![Node::default()],
        ..Default::default()
    }
}

#[test]
fn the_clock_counts_from_the_first_audio_and_waits_for_it() {
    // Audio first arrived a day into the capture clock, which no test
    // reaches: time has not started.
    let (_stop, late, _) = clock(86_400_000_000);
    assert_eq!(late.now_ms(), 0);

    let (_stop, mut waiting, first_audio) = clock(NO_AUDIO_YET);
    assert_eq!(waiting.now_ms(), 0);
    let arrives = thread::spawn(move || {
        thread::sleep(Duration::from_millis(120));
        first_audio.store(0, Ordering::Relaxed);
    });
    // The wait is over only once there is audio to count from.
    let asked = Instant::now();
    assert_eq!(waiting.wait(0), Wait::Elapsed);
    assert!(asked.elapsed() >= Duration::from_millis(100));
    assert!(waiting.now_ms() >= 100);
    arrives.join().unwrap();
}

#[test]
fn the_clock_stops_with_the_recording_or_the_lab() {
    let (stop, mut running, _) = clock(0);
    assert_eq!(running.wait(0), Wait::Elapsed);
    drop(stop);
    // At once, however long was asked for, and before the first audio too.
    let asked = Instant::now();
    assert_eq!(running.wait(60_000), Wait::Stopped);
    assert!(asked.elapsed() < Duration::from_secs(5));
    let (stop, mut silent, _) = clock(NO_AUDIO_YET);
    drop(stop);
    assert_eq!(silent.wait(0), Wait::Stopped);

    // Labs › Speaker names turned off while recording.
    let (_stop, mut off, _) = clock(0);
    off.reading = || false;
    assert_eq!(off.wait(0), Wait::Stopped);
    assert_eq!(off.pause(0), Wait::Stopped);
}

#[test]
fn only_a_call_app_hex_can_read_gets_a_reader() {
    let (bundle_id, _) = readable("com.tinyspeck.slackmacgap").unwrap();
    assert_eq!(bundle_id, "com.tinyspeck.slackmacgap");
    for other in [
        "us.zoom.xos",
        "com.google.Chrome",
        "invalid.hex.no-such-call-app",
        "",
    ] {
        assert!(readable(other).is_none(), "{other}");
    }
    // The lab is off in a test: no thread, whatever the call app.
    let (_stop, clock, first_audio) = clock(0);
    let reader = SpeakerReader::start(
        &folder("gate"),
        Some("com.tinyspeck.slackmacgap"),
        clock.capture,
        first_audio,
    );
    assert!(reader.is_none());
    assert!(!folder("gate").join("speakers.ndjson").exists());
}

#[test]
fn a_reader_saves_what_it_sees_and_dropping_it_stops_and_joins_it() {
    let folder = folder("thread");
    let file = folder.join("speakers.ndjson");
    let (_, clock, _) = clock(0);
    let directory = folder.clone();
    let reader = SpeakerReader::spawn(
        |stopped| Clock { stopped, ..clock },
        move |mut clock| read(&directory, &alice_speaking, a_window, &mut clock),
    )
    .unwrap();

    // The first look is made at once.
    let asked = Instant::now();
    while !file.exists() && asked.elapsed() < Duration::from_secs(10) {
        thread::sleep(Duration::from_millis(10));
    }
    let saved = fs::read_to_string(&file).unwrap();
    assert!(
        saved.starts_with(
            r#"{"kind":"participant","key":"p1","name":"Alice","is_local":false,"at_ms":"#
        ),
        "{saved}"
    );
    assert!(saved.contains(r#""kind":"floor""#) && saved.contains(r#""holder":"p1""#));
    assert!(!saved.contains("U0002ALICE"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    // Dropped mid-rest: the drop does not wait the rest out, the thread is
    // gone when it returns, and nothing is written after it.
    let asked = Instant::now();
    drop(reader);
    assert!(asked.elapsed() < Duration::from_secs(5));
    let after = fs::read_to_string(&file).unwrap();
    thread::sleep(Duration::from_millis(600));
    assert_eq!(fs::read_to_string(&file).unwrap(), after);
    fs::remove_dir_all(folder).unwrap();
}

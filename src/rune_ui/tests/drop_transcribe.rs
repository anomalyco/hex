//! Labs › Transcribe dropped audio, headless: the window takes a file only
//! with the lab on, says what Hex reads for one it can't, and queues what it
//! takes one at a time on a stand-in worker, so no model loads.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::*;
use crate::rune_ui::drop_transcribe::{DropTranscribe, Reporter};
use crate::transcription::models::TranscriptionSelection;

/// What a stand-in worker was asked to do. Its reporters are kept, as a real
/// worker holds them until it answers, so the job stays in flight.
#[derive(Default)]
struct Started {
    files: Vec<PathBuf>,
    selections: Vec<TranscriptionSelection>,
    cancels: Vec<Arc<AtomicBool>>,
    reporters: Vec<Reporter>,
}

/// The window with Labs › Transcribe dropped audio on or off, and the
/// stand-in worker its files would run on.
fn window(lab: bool) -> (Runtime<Ui>, Arc<Mutex<Started>>) {
    let started: Arc<Mutex<Started>> = Arc::default();
    let record = started.clone();
    let mut state = Ui::with(AppSettings::default(), Service::Fixture(Vec::new()));
    state.settings.labs.drop_transcribe = lab;
    // Previews and tests record platform requests instead of performing them.
    state.host = crate::rune_ui::platform::PlatformHost::Fixture(Vec::new());
    state.drop = DropTranscribe::with_runner(Arc::new(move |file, selection, cancel, reporter| {
        reporter.progress(Some(0.5));
        let mut started = record.lock().unwrap_or_else(|error| error.into_inner());
        started.files.push(file);
        started.selections.push(selection);
        started.cancels.push(cancel);
        started.reporters.push(reporter);
    }));
    (runtime_for(state, false, PANE_SIZE), started)
}

/// Where a drop lands: the pane's title, inside the window.
fn landing(rt: &Runtime<Ui>) -> Option<(f32, f32)> {
    rt.find("pane-title")
}

fn files_ran(started: &Arc<Mutex<Started>>) -> Vec<PathBuf> {
    started
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .files
        .clone()
}

#[test]
fn a_dropped_file_does_nothing_with_the_lab_off() {
    let (mut rt, started) = window(false);
    let at = landing(&rt);
    rt.drag_files(vec![PathBuf::from("/recordings/talk.wav")], at);
    settle(&mut rt);
    assert!(
        rt.drop_hovered().is_none(),
        "the window takes no drag with the lab off"
    );
    assert!(rt.find("drop-hint").is_none(), "and shows no hint");

    rt.drop_files(vec![PathBuf::from("/recordings/talk.wav")], at);
    settle(&mut rt);
    assert!(rt.state.drop.running().is_none(), "nothing was queued");
    assert!(files_ran(&started).is_empty(), "no worker ran");
    assert!(rt.find("drop-transcribe-running").is_none());
}

#[test]
fn a_dropped_file_hovers_then_queues_a_job() {
    let (mut rt, started) = window(true);
    let at = landing(&rt);
    let file = PathBuf::from("/recordings/talk.wav");
    rt.drag_files(vec![file.clone()], at);
    settle(&mut rt);
    assert_eq!(
        rt.drop_hovered().as_deref(),
        Some("kit-root"),
        "the window root is the drop target"
    );
    assert!(rt.find("drop-hint").is_some());
    assert!(
        texts(&rt)
            .iter()
            .any(|text| text == "Drop to save a transcript")
    );

    rt.drop_files(vec![file.clone()], at);
    settle(&mut rt);
    assert!(
        rt.find("drop-hint").is_none(),
        "the hint leaves with the drop"
    );
    assert_eq!(rt.state.drop.running(), Some(file.as_path()));
    assert_eq!(files_ran(&started), vec![file.clone()]);
    assert_eq!(
        started
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .selections,
        vec![rt.state.settings.transcription.clone()],
        "the file runs at the dictation selection in use"
    );
    // The toast names the file, how far it got, and what cancels it.
    let shown = texts(&rt);
    assert!(shown.iter().any(|text| text == "talk.wav"), "{shown:?}");
    assert!(
        shown.iter().any(|text| text == "Transcribing… 50%"),
        "{shown:?}"
    );
    assert!(shown.iter().any(|text| text == "Cancel"), "{shown:?}");
    assert!(rt.find("drop-transcribe-running").is_some());
}

#[test]
fn dropped_files_queue_one_at_a_time() {
    let (mut rt, started) = window(true);
    let at = landing(&rt);
    let first = PathBuf::from("/recordings/first.mp3");
    let second = PathBuf::from("/recordings/second.wav");
    rt.drop_files(vec![first.clone(), second.clone()], at);
    settle(&mut rt);
    assert_eq!(rt.state.drop.running(), Some(first.as_path()));
    assert_eq!(rt.state.drop.waiting(), vec![second.as_path()]);
    assert_eq!(
        files_ran(&started),
        vec![first],
        "one worker runs while the other waits"
    );
    assert!(
        texts(&rt)
            .iter()
            .any(|text| text == "Transcribing… 50% · 1 more queued"),
        "{:?}",
        texts(&rt)
    );
}

#[test]
fn cancel_stops_the_running_file_and_the_queue() {
    let (mut rt, started) = window(true);
    let at = landing(&rt);
    rt.drop_files(
        vec![
            PathBuf::from("/recordings/first.wav"),
            PathBuf::from("/recordings/second.wav"),
        ],
        at,
    );
    settle(&mut rt);
    rt.update(|ui, _| super::super::drop_transcribe::cancel(ui));
    settle(&mut rt);
    // The worker is still winding down, so its file stays tracked: nothing
    // else may start until it answers.
    assert_eq!(
        rt.state.drop.running(),
        Some(Path::new("/recordings/first.wav"))
    );
    assert!(rt.state.drop.cancelling());
    assert!(rt.state.drop.waiting().is_empty());
    assert!(
        started
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .cancels
            .iter()
            .all(|cancel| cancel.load(Ordering::Acquire)),
        "the worker is told to stop"
    );
    let shown = texts(&rt);
    assert!(
        shown.iter().any(|text| text == "Cancelling…"),
        "the toast says it is cancelling: {shown:?}"
    );
    assert!(
        !shown.iter().any(|text| text == "Cancel"),
        "a cancelled file offers no second Cancel: {shown:?}"
    );
}

/// Cancel used to forget the running file at once, so a drop right after it
/// started a second worker (and a second transcription service) while the
/// first was still loading or running.
#[test]
fn a_drop_after_cancel_waits_for_the_cancelled_worker() {
    let (mut rt, started) = window(true);
    let at = landing(&rt);
    let first = PathBuf::from("/recordings/first.wav");
    let second = PathBuf::from("/recordings/second.wav");
    rt.drop_files(vec![first.clone()], at);
    settle(&mut rt);
    rt.update(|ui, _| super::super::drop_transcribe::cancel(ui));
    settle(&mut rt);

    rt.drop_files(vec![second.clone()], at);
    settle(&mut rt);
    assert_eq!(
        files_ran(&started),
        vec![first.clone()],
        "one worker runs while the cancelled one finishes"
    );
    assert_eq!(rt.state.drop.waiting(), vec![second.as_path()]);
    assert!(rt.state.drop.cancelling());

    // The cancelled worker answers: only now does the queued file start.
    let reporter = started
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .reporters
        .remove(0);
    reporter.done(Err(
        crate::transcription::file_transcript::FileTranscriptError::Cancelled,
    ));
    settle(&mut rt);
    assert_eq!(files_ran(&started), vec![first, second]);
    assert!(!rt.state.drop.cancelling());
    assert!(rt.state.drop.waiting().is_empty(), "the drop started");
}

/// Turning the lab off cancels and forgets the running file and its queue, but
/// its worker still winding down keeps its job: a drop while off starts
/// nothing, and only the worker's answer lets it be forgotten.
#[test]
fn turning_the_lab_off_cancels_the_running_file_and_waits_for_it() {
    let (mut rt, started) = window(true);
    let at = landing(&rt);
    let first = PathBuf::from("/recordings/first.wav");
    let third = PathBuf::from("/recordings/third.wav");
    rt.drop_files(
        vec![first.clone(), PathBuf::from("/recordings/second.wav")],
        at,
    );
    settle(&mut rt);

    rt.update(|ui, _| ui.settings.labs.drop_transcribe = false);
    settle(&mut rt);
    assert_eq!(
        rt.state.drop.running(),
        Some(first.as_path()),
        "its worker has not answered yet"
    );
    assert!(
        rt.state.drop.cancelling(),
        "turning it off cancels the file"
    );
    assert!(rt.state.drop.waiting().is_empty(), "the queue is forgotten");
    assert!(
        started
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .cancels[0]
            .load(Ordering::Acquire),
        "the worker is told to stop"
    );
    assert!(
        rt.find("drop-transcribe-running").is_none(),
        "the window shows nothing of the lab it left"
    );

    rt.drop_files(vec![third.clone()], at);
    settle(&mut rt);
    assert_eq!(
        files_ran(&started),
        vec![first.clone()],
        "a drop while off starts nothing"
    );

    // The worker answers while off: only now is its job forgotten.
    let reporter = started
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .reporters
        .remove(0);
    reporter.done(Err(
        crate::transcription::file_transcript::FileTranscriptError::Cancelled,
    ));
    settle(&mut rt);
    assert!(
        rt.state.drop.running().is_none(),
        "the cancelled job is forgotten"
    );
    assert_eq!(
        files_ran(&started),
        vec![first.clone()],
        "still nothing new started"
    );

    // On again, a drop gets a fresh worker.
    rt.update(|ui, _| ui.settings.labs.drop_transcribe = true);
    settle(&mut rt);
    rt.drop_files(vec![third.clone()], at);
    settle(&mut rt);
    assert_eq!(files_ran(&started), vec![first, third.clone()]);
    assert_eq!(rt.state.drop.running(), Some(third.as_path()));
}

/// A cancel is checked before the transcript is saved, but one can land just
/// after that check: the saved file exists, so its toast says Saved.
#[test]
fn a_cancel_after_the_last_check_still_says_saved() {
    let (mut rt, started) = window(true);
    let at = landing(&rt);
    let saved = PathBuf::from("/recordings/talk.txt");
    rt.drop_files(vec![PathBuf::from("/recordings/talk.wav")], at);
    settle(&mut rt);
    rt.update(|ui, _| super::super::drop_transcribe::cancel(ui));
    settle(&mut rt);

    let reporter = started
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .reporters
        .remove(0);
    reporter.done(Ok(saved));
    settle(&mut rt);
    let shown = texts(&rt);
    assert!(
        shown.iter().any(|text| text == "Saved talk.txt"),
        "the file was saved before the cancel landed: {shown:?}"
    );
    assert!(rt.state.drop.running().is_none());
}

/// An MP3 whose header says no duration has no percentage to show, so the
/// toast says "Transcribing…" and a spinner instead of a stuck 0%.
#[test]
fn a_file_of_unknown_length_says_transcribing_without_a_percentage() {
    let mut state = Ui::with(AppSettings::default(), Service::Fixture(Vec::new()));
    state.settings.labs.drop_transcribe = true;
    let held: Arc<Mutex<Option<Reporter>>> = Arc::default();
    let keep = held.clone();
    state.drop = DropTranscribe::with_runner(Arc::new(move |_, _, _, reporter| {
        reporter.progress(None);
        *keep.lock().unwrap_or_else(|error| error.into_inner()) = Some(reporter);
    }));
    let mut rt = runtime_for(state, false, PANE_SIZE);
    let at = landing(&rt);
    rt.drop_files(vec![PathBuf::from("/recordings/live.mp3")], at);
    settle(&mut rt);
    let shown = texts(&rt);
    assert!(
        shown.iter().any(|text| text == "Transcribing…"),
        "{shown:?}"
    );
    assert!(
        !shown
            .iter()
            .any(|text| text.starts_with("Transcribing… ") && text.contains('%')),
        "no percentage is invented: {shown:?}"
    );
}

#[test]
fn a_file_hex_cannot_read_is_refused() {
    let (mut rt, started) = window(true);
    let at = landing(&rt);
    // WMA has no reader here, so the drop target refuses it.
    let file = PathBuf::from("/recordings/talk.wma");
    rt.drag_files(vec![file.clone()], at);
    settle(&mut rt);
    assert!(
        texts(&rt).iter().any(|text| {
            text == "Hex can transcribe WAV, MP3, AAC, M4A, MP4, MOV, AIFF, CAF, FLAC, OGG, Opus, WebM, or MKV files"
        }),
        "{:?}",
        texts(&rt)
    );

    rt.drop_files(vec![file], at);
    settle(&mut rt);
    assert!(files_ran(&started).is_empty(), "nothing ran");
    assert!(rt.state.drop.running().is_none());
    assert!(
        rt.find("drop-transcribe-rejected").is_some(),
        "the toast says what Hex reads"
    );
}

/// The Labs card's own button: Wayland reports no file drags, so a card
/// action opens the same queue's file dialog.
#[test]
fn the_labs_card_offers_a_file_choice_only_with_its_lab_on() {
    let build = |lab: bool| {
        let mut state = Ui::with(AppSettings::default(), Service::Fixture(Vec::new()));
        state.settings.labs.drop_transcribe = lab;
        let mut rt = runtime_for(state, false, TALL_SIZE);
        open_labs(&mut rt);
        rt
    };
    let shown = |rt: &Runtime<Ui>| {
        rt.elements()
            .iter()
            .any(|element| element.key.as_deref() == Some("drop-transcribe-choose"))
    };
    assert!(!shown(&build(false)), "off, its card has no action");
    assert!(shown(&build(true)), "on, its card offers the dialog");
}

#[test]
fn a_saved_transcript_says_where_it_is_and_dismisses() {
    let saved = PathBuf::from("/recordings/talk.txt");
    let answer = saved.clone();
    let mut state = Ui::with(AppSettings::default(), Service::Fixture(Vec::new()));
    state.settings.labs.drop_transcribe = true;
    state.host = crate::rune_ui::platform::PlatformHost::Fixture(Vec::new());
    state.drop = DropTranscribe::with_runner(Arc::new(move |_, _, _, reporter| {
        reporter.done(Ok(answer.clone()));
    }));
    let mut rt = runtime_for(state, false, PANE_SIZE);
    let at = landing(&rt);
    rt.drop_files(vec![PathBuf::from("/recordings/talk.wav")], at);
    settle(&mut rt);
    let shown = texts(&rt);
    assert!(
        shown.iter().any(|text| text == "Saved talk.txt"),
        "{shown:?}"
    );
    assert!(
        shown.iter().any(|text| text == "/recordings/talk.txt"),
        "the whole saved path shows: {shown:?}"
    );
    assert!(
        shown.iter().any(|text| text == "Show in folder"),
        "{shown:?}"
    );
    assert!(rt.state.drop.running().is_none());
    assert!(rt.find("drop-transcribe-running").is_none());

    // Show in folder goes through the host, as the Meetings pane's reveal
    // does: a headless window records the request instead of opening a file
    // manager.
    press_labelled(&mut rt, "Show in folder");
    let asked: Vec<_> = match &rt.state.host {
        crate::rune_ui::platform::PlatformHost::Fixture(asked) => asked.clone(),
        _ => panic!("a fixture records platform requests"),
    };
    assert_eq!(
        asked,
        [crate::rune_ui::platform::PlatformRequest::Meeting(
            crate::rune_ui::platform::MeetingRequest::Reveal(
                crate::meeting::RevealTarget::Markdown(saved)
            )
        )]
    );

    tap(&mut rt, "close");
    settle(&mut rt);
    assert!(
        rt.find("drop-transcribe-saved").is_none(),
        "the toast's close dismisses it"
    );
}

/// Presses the button a person would read as `label`.
fn press_labelled(rt: &mut Runtime<Ui>, label: &str) {
    let button = rt
        .elements()
        .into_iter()
        .find(|element| element.role == "button" && element.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no button says {label:?}"));
    let (x, y) = (
        button.rect[0] + button.rect[2] * 0.5,
        button.rect[1] + button.rect[3] * 0.5,
    );
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

/// A file Hex reads starts with nothing on screen but the window's own
/// panes: the drop surface is not part of it until a drag or a file arrives.
#[test]
fn the_drop_surface_shows_nothing_at_rest() {
    let (rt, _) = window(true);
    assert!(rt.find("drop-hint").is_none());
    assert!(rt.find("drop-transcribe-running").is_none());
    assert!(rt.state.drop.waiting().is_empty());
    assert!(rt.state.drop.running().is_none());
}

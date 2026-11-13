//! Labs › Transcribe dropped audio, headless: the window takes a file only
//! with the lab on, says what Hex reads for one it can't, and queues what it
//! takes one at a time on a stand-in worker, so no model loads.

use std::path::PathBuf;
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
    state.drop = DropTranscribe::with_runner(Arc::new(move |file, selection, cancel, reporter| {
        reporter.progress(0.5);
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
    assert!(rt.state.drop.running().is_none());
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
    assert!(rt.find("drop-transcribe-running").is_none());
}

#[test]
fn a_file_hex_cannot_read_is_refused() {
    let (mut rt, started) = window(true);
    let at = landing(&rt);
    let file = PathBuf::from("/recordings/talk.flac");
    rt.drag_files(vec![file.clone()], at);
    settle(&mut rt);
    assert!(
        texts(&rt)
            .iter()
            .any(|text| text == "Hex can transcribe WAV and MP3 files"),
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

    tap(&mut rt, "close");
    settle(&mut rt);
    assert!(
        rt.find("drop-transcribe-saved").is_none(),
        "the toast's close dismisses it"
    );
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

//! Labs › Transcribe dropped audio: the window's drop target, the lab card's
//! "Transcribe a file…" button, the queue that transcribes one file at a
//! time on a worker thread, and the toast that says how each went.
//!
//! A file dropped anywhere on the window with an extension this build reads
//! is queued; a drop Hex can't use says what it can read instead. A drag
//! over the window shows the hint. Wayland reports no file drags, so the
//! card's button opens the native file dialog for the same queue. With the
//! lab off the window has no drop target and none of this shows.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use rune::{El, Tx, col, el, icon, row, text};
use rune_kit::{ProgressCircle, Toast, ToastRegion};

use super::Ui;
use super::controls::Paint;
use super::icons::WAVE;
use super::lists::small_button;
use crate::desktop::file_dialog::{Dialog, Filter, Pending, Target};
use crate::desktop::i18n::{tr, tr_fill};
use crate::labs::Feature;
use crate::transcription::file_transcript::{
    FileTranscriptError, extensions, transcribe_audio_file,
};
use crate::transcription::models::TranscriptionSelection;

/// The window's dropped files: the drag over it, what waits for the worker,
/// the file running now, and the last toast.
pub(super) struct DropTranscribe {
    /// What a file drag over the window would do now.
    pub(super) drag: Drag,
    /// The files waiting for the worker, in the order they arrived.
    pub(super) queue: VecDeque<PathBuf>,
    /// The file the worker is on.
    pub(super) job: Option<Job>,
    /// The last toast, until it is dismissed or another file starts.
    pub(super) notice: Option<Notice>,
    /// The open file dialog's answer.
    chooser: Option<Pending>,
    /// Who runs a file: the real worker, or a test's stand-in.
    runner: Runner,
}

/// What a file drag over the window would do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Drag {
    /// No file drag is over the window.
    #[default]
    None,
    /// A drag carrying at least one file this build reads.
    Accepted,
    /// A drag carrying none of them.
    Rejected,
}

/// One file's transcription on a worker thread.
pub(super) struct Job {
    pub(super) path: PathBuf,
    /// The fraction of the file decoded, as the worker last reported it.
    progress: f32,
    events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
}

/// What a worker reports: how far it has decoded the file, then how it went.
enum Event {
    Progress(f32),
    Done(Result<PathBuf, FileTranscriptError>),
}

/// The last toast: what the window says about a drop until it is dismissed.
#[derive(Clone, Debug)]
pub(super) enum Notice {
    /// Files Hex can't read, so nothing was transcribed.
    Rejected,
    /// A transcript saved at this path.
    Saved(PathBuf),
    /// A file Hex could not transcribe, and why.
    Failed { name: String, error: String },
    /// The native file dialog could not open.
    Chooser(String),
}

/// Who runs a file: the real transcription on its own thread, or a test's
/// stand-in that never loads a model.
pub(super) type Runner =
    Arc<dyn Fn(PathBuf, TranscriptionSelection, Arc<AtomicBool>, Reporter) + Send + Sync>;

/// A worker's side of a job: where its events go, and the window's waker.
pub(super) struct Reporter {
    events: Sender<Event>,
    wake: Option<rune::Waker>,
}

impl Reporter {
    fn send(&self, event: Event) {
        let _ = self.events.send(event);
        if let Some(wake) = &self.wake {
            wake();
        }
    }

    /// Reports how much of the file the worker has decoded.
    pub(super) fn progress(&self, fraction: f32) {
        self.send(Event::Progress(fraction));
    }

    /// Reports the file it saved, or why it could not.
    pub(super) fn done(self, result: Result<PathBuf, FileTranscriptError>) {
        self.send(Event::Done(result));
    }
}

impl Default for DropTranscribe {
    fn default() -> Self {
        Self {
            drag: Drag::None,
            queue: VecDeque::new(),
            job: None,
            notice: None,
            chooser: None,
            runner: Arc::new(worker),
        }
    }
}

impl DropTranscribe {
    /// Starts the next queued file when the worker is free: one file at a
    /// time, in the order they arrived. `selection` is the dictation model
    /// and language every file in this run uses.
    fn start_next(&mut self, wake: Option<rune::Waker>, selection: TranscriptionSelection) {
        if self.job.is_some() {
            return;
        }
        let Some(path) = self.queue.pop_front() else {
            return;
        };
        let (events, receiver) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        (self.runner)(
            path.clone(),
            selection,
            cancel.clone(),
            Reporter { events, wake },
        );
        self.job = Some(Job {
            path,
            progress: 0.0,
            events: receiver,
            cancel,
        });
    }

    /// Stops the file running now and forgets the ones queued behind it.
    pub(super) fn cancel(&mut self) {
        self.queue.clear();
        if let Some(job) = self.job.take() {
            job.cancel.store(true, Ordering::Release);
        }
        self.notice = None;
    }

    /// Forgets every file, cancelling the one running: turning the lab off
    /// leaves nothing of it behind.
    fn clear(&mut self) -> bool {
        let shown = self.drag != Drag::None
            || self.job.is_some()
            || !self.queue.is_empty()
            || self.notice.is_some()
            || self.chooser.is_some();
        self.drag = Drag::None;
        self.queue.clear();
        if let Some(job) = self.job.take() {
            job.cancel.store(true, Ordering::Release);
        }
        self.notice = None;
        self.chooser = None;
        shown
    }

    /// The file the worker is on, if one is.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn running(&self) -> Option<&Path> {
        self.job.as_ref().map(|job| job.path.as_path())
    }

    /// The files still waiting behind the running one.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn waiting(&self) -> Vec<&Path> {
        self.queue.iter().map(PathBuf::as_path).collect()
    }

    /// A file drag over the window, for the preview's hover and its golden.
    pub(super) fn fixture_dragging(drag: Drag) -> Self {
        Self {
            drag,
            ..Self::default()
        }
    }

    /// A running fixture: one file at 42% with two behind it, on a stand-in
    /// worker that holds its place so no model loads.
    pub(super) fn fixture_running() -> Self {
        let held: Arc<Mutex<Option<Reporter>>> = Arc::default();
        let keep = held.clone();
        let mut pane = Self {
            runner: Arc::new(move |_, _, _, reporter| {
                *keep.lock().unwrap_or_else(|error| error.into_inner()) = Some(reporter);
            }),
            ..Self::default()
        };
        pane.queue = VecDeque::from([
            PathBuf::from("/recordings/Standup.m4a"),
            PathBuf::from("/recordings/Design sync.m4a"),
            PathBuf::from("/recordings/Interview.mp3"),
        ]);
        pane.start_next(None, TranscriptionSelection::default());
        if let Some(job) = &mut pane.job {
            job.progress = 0.42;
        }
        // A drop of several files: the one before this saved while this runs.
        pane.notice = Some(Notice::Saved(PathBuf::from("/recordings/Standup 2.txt")));
        pane
    }

    /// A pane whose files a test's stand-in runs instead of a model.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn with_runner(runner: Runner) -> Self {
        Self {
            runner,
            ..Self::default()
        }
    }
}

/// Whether this window has the lab: its own switch, on this OS. The window's
/// saved switch decides what it draws; every file is checked against
/// `crate::labs::drop_transcribe` again where it starts.
pub(super) fn on(ui: &Ui) -> bool {
    ui.settings.labs.drop_transcribe && Feature::DropTranscribe.on_this_os()
}

/// The window root's file drop target: the whole window takes a file this
/// build reads, says what Hex reads for one it can't, and shows the hint
/// while a drag is over it. The lab off sets none of these.
pub(super) fn drop_target(root: El) -> El {
    root.on_drop_files(|ui: &mut Ui, files, tx| dropped(ui, files, tx))
        .accepts_extensions(extensions())
        .on_drop_hover(|ui: &mut Ui, over, tx| hover(ui, over, Drag::Accepted, tx))
        .on_drop_rejected_hover(|ui: &mut Ui, over, tx| hover(ui, over, Drag::Rejected, tx))
        .on_drop_rejected(|ui: &mut Ui, files, tx| rejected(ui, files, tx))
}

/// A drag entered, moved over, or left the window.
fn hover(ui: &mut Ui, over: bool, drag: Drag, tx: &mut Tx) {
    let next = if over { drag } else { Drag::None };
    if ui.drop.drag == next {
        return;
    }
    ui.drop.drag = next;
    // A drag is direct manipulation: the hint arrives without a fade.
    tx.instant();
}

/// Files this build reads landed on the window: they queue in order.
fn dropped(ui: &mut Ui, files: &[PathBuf], tx: &mut Tx) {
    ui.drop.drag = Drag::None;
    if files.is_empty() {
        return;
    }
    ui.drop.notice = None;
    ui.drop.queue.extend(files.iter().cloned());
    start(ui);
    tx.instant();
}

/// Files this build can't read landed on the window.
fn rejected(ui: &mut Ui, files: &[PathBuf], tx: &mut Tx) {
    ui.drop.drag = Drag::None;
    if files.is_empty() {
        return;
    }
    ui.drop.notice = Some(Notice::Rejected);
    tx.instant();
}

/// Starts the next queued file at the dictation selection in use.
fn start(ui: &mut Ui) {
    let wake = ui.modes.waker.clone();
    let selection = ui.settings.transcription.clone();
    ui.drop.start_next(wake, selection);
}

/// Cancels the dropped files: the one running and every one behind it.
pub(super) fn cancel(ui: &mut Ui) {
    ui.drop.cancel();
}

/// Opens the native file dialog for one file this build reads, for the
/// desktops that report no file drags.
pub(super) fn choose(ui: &mut Ui) {
    if ui.drop.chooser.is_some() {
        return;
    }
    let waker = ui.modes.waker.clone();
    ui.drop.chooser = Some(crate::desktop::file_dialog::open(
        Dialog {
            title: tr("Transcribe a file").into(),
            target: Target::File(vec![Filter {
                label: tr("Audio files").into(),
                extensions: extensions(),
            }]),
            directory: dirs::audio_dir(),
        },
        move || {
            if let Some(waker) = &waker {
                waker();
            }
        },
    ));
}

/// The file dialog's answer: a chosen file joins the queue.
fn poll_chooser(ui: &mut Ui) -> bool {
    let Some(choice) = ui.drop.chooser.as_ref().and_then(Pending::poll) else {
        return false;
    };
    ui.drop.chooser = None;
    match choice {
        Ok(Some(path)) => {
            ui.drop.notice = None;
            ui.drop.queue.push_back(path);
            start(ui);
        }
        Ok(None) => {}
        Err(error) => ui.drop.notice = Some(Notice::Chooser(error)),
    }
    true
}

/// Takes the running file's progress and answer, then starts the next one.
fn poll_job(ui: &mut Ui) -> bool {
    let mut changed = false;
    loop {
        let Some(job) = ui.drop.job.as_mut() else {
            return changed;
        };
        match job.events.try_recv() {
            Ok(Event::Progress(fraction)) => {
                job.progress = fraction.clamp(0.0, 1.0);
                changed = true;
            }
            Ok(Event::Done(result)) => {
                let job = ui.drop.job.take().expect("the job just read");
                ui.drop.notice = Some(match result {
                    Ok(path) => Notice::Saved(path),
                    Err(error) => Notice::Failed {
                        name: name_of(&job.path),
                        error: error.to_string(),
                    },
                });
                start(ui);
                return true;
            }
            Err(TryRecvError::Empty) => return changed,
            Err(TryRecvError::Disconnected) => {
                let job = ui.drop.job.take().expect("the job just read");
                ui.drop.notice = Some(Notice::Failed {
                    name: name_of(&job.path),
                    error: tr("Transcription stopped.").to_owned(),
                });
                start(ui);
                return true;
            }
        }
    }
}

/// Takes the file dialog's answer and the worker's reports. Turning the lab
/// off cancels and forgets everything it started.
pub(super) fn poll(ui: &mut Ui) -> bool {
    if !on(ui) {
        return ui.drop.clear();
    }
    let chosen = poll_chooser(ui);
    poll_job(ui) || chosen
}

/// The window's drop surface: the hint while a file drag is over the window,
/// else the toasts of the last drop's outcome and the file running now.
pub(super) fn overlay(ui: &Ui, p: Paint) -> Option<El> {
    if let Some(hint) = hint(ui, p) {
        return Some(hint);
    }
    status(ui, p)
}

/// The hint over the whole window while a file drag is on it: what dropping
/// does, or what Hex reads instead.
fn hint(ui: &Ui, p: Paint) -> Option<El> {
    let accepted = match ui.drop.drag {
        Drag::None => return None,
        Drag::Accepted => true,
        Drag::Rejected => false,
    };
    let (title, detail) = if accepted {
        (
            tr("Drop to save a transcript").to_owned(),
            Some(formats().to_owned()),
        )
    } else {
        (tr_fill("Hex can transcribe {} files", formats()), None)
    };
    let ink = if accepted { p.accent() } else { p.danger() };
    let mut card = col()
        .key("drop-hint")
        .role("status")
        .label(title.clone())
        .description(detail.clone().unwrap_or_default())
        .padding(20.0)
        .gap(8.0)
        .items_center()
        .radius(p.k.radius.lg)
        .fill(p.k.c.bg_base)
        .border(1.0, ink)
        .child(icon(&WAVE, 22.0).color(ink).a11y_hidden())
        .child(
            text(title)
                .key("title")
                .font(p.k.ty.ui_medium)
                .color(p.text())
                .a11y_hidden(),
        );
    if let Some(detail) = detail {
        card = card.child(
            text(detail)
                .key("detail")
                .font(super::controls::DESCRIPTION)
                .color(p.muted())
                .a11y_hidden(),
        );
    }
    for shadow in &p.k.elevation.floating {
        card = card.box_shadow(*shadow);
    }
    Some(
        el().key("drop-hint-layer")
            .role("none")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .center()
            .fill(p.k.c.scrim)
            .child(card),
    )
}

/// The status toasts: the last drop's outcome above, and the file running
/// now with its progress and Cancel below it. A drop of three files says
/// each one saved while the next runs.
fn status(ui: &Ui, p: Paint) -> Option<El> {
    let mut region = ToastRegion::new();
    let mut any = false;
    if let Some(notice) = &ui.drop.notice {
        region = region.toast(finished(p, notice));
        any = true;
    }
    if let Some(job) = &ui.drop.job {
        region = region.toast(running(p, job, ui.drop.queue.len()));
        any = true;
    }
    any.then(|| region.build(p.k))
}

/// The file running now: its name, a ring and a percentage of its audio
/// decoded, how many wait behind it, and Cancel.
fn running(p: Paint, job: &Job, queued: usize) -> El {
    let percent = (job.progress * 100.0).round();
    let mut description = tr_fill("Transcribing… {}%", &format!("{percent:.0}"));
    if queued > 0 {
        description.push_str(" · ");
        description.push_str(&tr_fill("{} more queued", &queued.to_string()));
    }
    Toast::new("drop-transcribe-running", name_of(&job.path))
        .lead(
            ProgressCircle::new(job.progress)
                .size(18.0)
                .label(tr("Transcribing"))
                .build(p.k),
        )
        .description(description)
        .action(p.k, tr("Cancel"), false, |ui: &mut Ui, _: &mut Tx| {
            cancel(ui)
        })
        .build(p.k)
}

/// The last drop's outcome: a transcript saved, a file that failed, a drop
/// Hex can't read, or a file dialog that did not open.
fn finished(p: Paint, notice: &Notice) -> El {
    match notice {
        Notice::Rejected => Toast::new(
            "drop-transcribe-rejected",
            tr_fill("Hex can transcribe {} files", formats()),
        )
        .on_close(dismiss)
        .build(p.k),
        Notice::Saved(path) => {
            Toast::new("drop-transcribe-saved", tr_fill("Saved {}", &name_of(path)))
                .description(path.display().to_string())
                .action(p.k, tr("Show in folder"), true, {
                    let path = path.clone();
                    move |_: &mut Ui, _: &mut Tx| crate::meeting::reveal(&path)
                })
                .on_close(dismiss)
                .build(p.k)
        }
        Notice::Failed { name, error } => Toast::new(
            "drop-transcribe-failed",
            tr_fill("Could not transcribe {}", name),
        )
        .description(error.clone())
        .on_close(dismiss)
        .build(p.k),
        Notice::Chooser(error) => {
            Toast::new("drop-transcribe-chooser", tr("A file could not be chosen."))
                .description(error.clone())
                .on_close(dismiss)
                .build(p.k)
        }
    }
}

/// Dismisses the toast.
fn dismiss(ui: &mut Ui) {
    ui.drop.notice = None;
}

/// Labs › Transcribe dropped audio's card while on: the file dialog's
/// button, for desktops that report no file drags.
pub(super) fn card(p: Paint) -> El {
    // A row keeps the button its own width, as the card's other controls do.
    row()
        .key("drop-transcribe-row")
        .items_start()
        .child(small_button(
            p,
            "drop-transcribe-choose",
            tr("Transcribe a file…"),
            |ui: &mut Ui, _: &mut Tx| choose(ui),
        ))
}

/// A file's name, or its whole path when it has none.
fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The kinds of file this build reads, as a phrase for the hint and the
/// refusal.
fn formats() -> &'static str {
    if cfg!(target_os = "macos") {
        tr("WAV, MP3, M4A, AIFF, and CAF")
    } else {
        tr("WAV and MP3")
    }
}

/// The real worker: transcribes `path` with `selection` on its own thread,
/// reporting progress and then the file it saved.
fn worker(
    path: PathBuf,
    selection: TranscriptionSelection,
    cancel: Arc<AtomicBool>,
    reporter: Reporter,
) {
    let spawned = std::thread::Builder::new()
        .name("hex-transcribe-file".into())
        .spawn({
            let reporter = Reporter {
                events: reporter.events.clone(),
                wake: reporter.wake.clone(),
            };
            move || {
                let mut progress = |fraction| reporter.progress(fraction);
                let result = transcribe_audio_file(&path, selection, cancel, &mut progress);
                match &result {
                    Ok(saved) => tracing::info!(
                        file = %path.display(),
                        saved = %saved.display(),
                        "a dropped file was transcribed"
                    ),
                    Err(error) => tracing::warn!(
                        file = %path.display(),
                        %error,
                        "a dropped file was not transcribed"
                    ),
                }
                reporter.done(result);
            }
        });
    if let Err(error) = spawned {
        reporter.done(Err(FileTranscriptError::TranscriptionFailed(
            error.to_string(),
        )));
    }
}

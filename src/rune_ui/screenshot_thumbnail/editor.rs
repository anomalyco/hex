//! Labs › Screenshots: the editor a click on the thumbnail opens, a panel
//! floating over the display under the pointer, as CleanShot's is. It
//! shows the shot with a pen, an arrow, a box, text, and a crop, and the
//! edited shot goes to the clipboard, a new file beside the first, or an
//! OpenCode session. Nothing changes the shot already saved. Escape or ✕
//! closes it; until then it stays, and the panel moves by its edges.
//!
//! While it's open, a dictation talks over the shot (`screenshots::narration`):
//! its words show under the shot and go with it to a session.
//!
//! A Screenshots recording opens here too, to trim ([`clip`]); its marks
//! tools stay hidden.

pub(crate) mod clip;
mod export;
pub(crate) mod marks;
mod narration;
mod view;
mod zoom;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use rune::{Key, KeyEvent, OverlayHandle, Phase, Tx};

use self::clip::Clip;
use self::export::{Export, deliver, load_sessions};
use self::marks::{Mark, Rect, Tool};
use crate::app_settings::ExportSize;
use crate::desktop::file_dialog;
use crate::desktop::i18n::{tr, tr_fill};
use crate::rune_ui::capture_capsule::surface::CHIP_H;
pub(crate) use narration::Narration;
use narration::{GENERATION, OPEN};
#[cfg(target_os = "macos")]
pub(crate) use narration::{can_claim, heard, listen, narration};
pub(crate) use zoom::From;

/// Around the shot and the toolbar, inside the panel.
const PAD: f32 = 8.0;
/// Between the toolbar and the shot.
const GAP: f32 = 8.0;
/// Wide enough for the toolbar whatever the shot.
const MIN_W: f32 = 560.0;
/// Kept clear around the panel on the display.
const MARGIN: f32 = 48.0;
/// The most sessions the Send to… list shows.
const LIST_ROWS: usize = 8;
/// How long a note such as "Copied" stays.
const NOTE_S: f64 = 2.0;
/// How long a close takes, before the overlay hides.
const LEAVE_S: f64 = 0.3;
/// How often the playhead moves while a recording plays.
const PLAY_STEP: Duration = Duration::from_millis(33);

/// The shot being edited.
pub(crate) struct Shot {
    pub(crate) image: rune::Image,
    pub(crate) rgba: Arc<Vec<u8>>,
    /// Its size in pixels.
    pub(crate) size: [u32; 2],
    /// Its pixels per point.
    pub(crate) scale: f32,
}

/// An OpenCode session the shot can go to.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Dest {
    pub(crate) session: String,
    pub(crate) name: String,
    pub(crate) folder: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum Sessions {
    Loading,
    Ready(Vec<Dest>),
    Failed,
}

/// The open Send to… list.
#[derive(Clone, Debug, PartialEq)]
struct SendList {
    query: String,
    sessions: Sessions,
    selected: usize,
}

/// Where an edited shot goes.
#[derive(Clone, Debug)]
enum Out {
    Copy,
    /// To the file the save panel chose.
    Save(PathBuf),
    Send(Dest),
}

/// The toolbar button an export was pressed on, which shows its progress.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pressed {
    Copy,
    Save,
    Send,
}

impl Out {
    fn pressed(&self) -> Pressed {
        match self {
            Self::Copy => Pressed::Copy,
            Self::Save(_) => Pressed::Save,
            Self::Send(_) => Pressed::Send,
        }
    }
}

/// A dictation talking over the shot, or none.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Listen {
    #[default]
    Idle,
    Recording,
    Transcribing,
}

#[derive(Default)]
pub(crate) struct Editor {
    shot: Option<Shot>,
    /// The shot's saved file, whose Captures entry keeps the words said
    /// over it.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    file: Option<PathBuf>,
    /// A recording open to trim, in place of a shot.
    clip: Option<Clip>,
    /// The size a recording exports at, `screenshots.export_size`.
    export_size: ExportSize,
    /// A recording's export under way: the button pressed and the share
    /// done. Another waits until it's over.
    exporting: Option<(Pressed, f32)>,
    /// The display's size in points.
    screen: [f32; 2],
    /// The panel's top-left, once it's been moved.
    pos: Option<[f32; 2]>,
    /// Where it was when a move began.
    grab: [f32; 2],
    tool: Tool,
    marks: Vec<Mark>,
    /// The mark a drag is making.
    draft: Option<Mark>,
    crop: Option<Rect>,
    /// A crop drag's corners.
    cropping: Option<([f32; 2], [f32; 2])>,
    /// Text being typed: its top-left and what's typed.
    typing: Option<([f32; 2], String)>,
    send: Option<SendList>,
    /// What was said over the shot, every dictation's words joined.
    words: String,
    listen: Listen,
    generation: u64,
    narration: Option<Arc<Narration>>,
    /// A send picked while a dictation was still going: it goes once the
    /// words land, and never if Escape cancels them.
    waiting: Option<Dest>,
    /// A short note and the animation seconds it appeared.
    note: Option<(String, f64)>,
    /// An export that failed: its note stays beside Retry until the next
    /// export.
    failed: Option<Out>,
    /// The save panel Save opened, until it answers.
    saving: Option<file_dialog::Pending>,
    on: bool,
    leaving: Option<f64>,
    /// The shot flying from its card and back, when a card opened it.
    zoom: Option<zoom::Zoom>,
    now: f64,
    overlay: Option<OverlayHandle<Editor>>,
}

/// Where the panel's parts lie, in the overlay's points.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    panel: [f32; 4],
    /// The shot inside the panel.
    image: [f32; 4],
    /// A recording's play button and time, then its filmstrip, under its
    /// picture; empty for a shot.
    transport: [f32; 4],
    strip: [f32; 4],
    /// The row under the shot: the microphone and the words.
    words: [f32; 4],
    /// Points per shot pixel.
    fit: f32,
}

/// The most lines of words under the shot; more end in "…" there, and all
/// of them are sent.
const WORDS_LINES: usize = 3;
/// Between the words row's text and its edges, top and bottom.
const WORDS_PY: f32 = 6.0;

/// How tall the words row is for `words` in a row `width` points wide: a
/// chip's height, or taller for up to [`WORDS_LINES`] lines. Lines are
/// estimated from an average glyph width so the panel sizes before layout;
/// the text itself clamps to the same count.
fn words_height(words: &str, width: f32) -> f32 {
    let style = crate::rune_ui::capture_capsule::surface::medium();
    let text_w = (width - 2.0 * (CHIP_H + GAP)).max(1.0);
    let per_line = (text_w / (style.size * 0.55)).max(1.0);
    let lines = (words.chars().count() as f32 / per_line)
        .ceil()
        .clamp(1.0, WORDS_LINES as f32);
    CHIP_H.max(lines * style.line_height + 2.0 * WORDS_PY)
}

impl Editor {
    /// `shot` open on a display `screen` points large, as a still.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn still(shot: Shot, screen: [f32; 2]) -> Self {
        Self {
            shot: Some(shot),
            screen,
            on: true,
            ..Self::default()
        }
    }

    fn load(&mut self, shot: Shot, screen: [f32; 2]) {
        self.give_back();
        *self = Self {
            shot: Some(shot),
            screen,
            on: true,
            now: self.now,
            overlay: self.overlay.take(),
            tool: self.tool,
            ..Self::default()
        };
    }

    fn load_clip(&mut self, clip: Clip, screen: [f32; 2]) {
        self.give_back();
        *self = Self {
            clip: Some(clip),
            export_size: crate::app_settings::screenshot_settings().export_size,
            screen,
            on: true,
            now: self.now,
            overlay: self.overlay.take(),
            tool: self.tool,
            // An export of the recording shown before reports to nothing.
            generation: GENERATION.fetch_add(1, Ordering::Relaxed),
            ..Self::default()
        };
    }

    /// The picture shown: the shot, or the recording's frame.
    fn picture(&self) -> Option<&Shot> {
        match &self.clip {
            Some(clip) => clip.picture(),
            None => self.shot.as_ref(),
        }
    }

    /// The words under the picture: what was said over the shot, or inside
    /// the recording's span kept.
    fn words(&self) -> Cow<'_, str> {
        match &self.clip {
            Some(clip) => Cow::Owned(clip.words_in_trim()),
            None => Cow::Borrowed(&self.words),
        }
    }

    fn layout(&self) -> Layout {
        let [sw, sh] = self.screen;
        let (size, scale) = self
            .picture()
            .map_or(([1, 1], 1.0), |shot| (shot.size, shot.scale));
        let (w, h) = (size[0].max(1) as f32, size[1].max(1) as f32);
        let words = self.words();
        let strip_h = if self.clip.is_some() {
            clip::STRIP_H + GAP
        } else {
            0.0
        };
        let room_w = (sw - 2.0 * (MARGIN + PAD)).max(1.0);
        let words_h = words_height(&words, room_w);
        let room_h = (sh - 2.0 * (MARGIN + PAD) - CHIP_H - 2.0 * GAP - strip_h - words_h).max(1.0);
        let fit = (room_w / w).min(room_h / h).min(1.0 / scale.max(1.0));
        let (iw, ih) = ((w * fit).round(), (h * fit).round());
        let pw = (iw + 2.0 * PAD).max(MIN_W);
        let words_h = words_height(&words, pw - 2.0 * PAD);
        let ph = PAD + CHIP_H + GAP + ih + GAP + strip_h + words_h + PAD;
        let [x, y] = self.pos.unwrap_or([(sw - pw) * 0.5, (sh - ph) * 0.5]);
        let [x, y] = clamp_panel([x, y], [pw, ph], self.screen);
        let iy = PAD + CHIP_H + GAP;
        let sy = iy + ih + GAP;
        Layout {
            panel: [x, y, pw, ph],
            image: [((pw - iw) * 0.5).round(), iy, iw, ih],
            transport: [PAD, sy, clip::TRANSPORT_W, strip_h - GAP],
            strip: [
                PAD + clip::TRANSPORT_W + GAP,
                sy,
                pw - 2.0 * PAD - clip::TRANSPORT_W - GAP,
                strip_h - GAP,
            ],
            words: [PAD, sy + strip_h, pw - 2.0 * PAD, words_h],
            fit,
        }
    }

    fn close(&mut self, tx: &mut Tx) {
        if self.on {
            *OPEN.lock().unwrap_or_else(|e| e.into_inner()) = None;
            if let Some(clip) = &mut self.clip {
                clip.pause();
            }
            self.on = false;
            self.send = None;
            self.waiting = None;
            tx.animated();
            if self.zoom_out() {
                tx.wake_after(Duration::ZERO);
            } else {
                self.leaving = Some(self.now);
                tx.wake_after(Duration::from_secs_f64(LEAVE_S));
            }
        }
    }

    fn keys(&self) {
        if let Some(overlay) = &self.overlay {
            overlay.request_keys();
        }
    }

    fn pick(&mut self, tool: Tool) {
        self.finish_typing();
        self.send = None;
        self.tool = tool;
    }

    fn finish_typing(&mut self) {
        let scale = self.scale();
        if let Some((at, text)) = self.typing.take()
            && let Some(mark) = (Mark::Text { at, text }).finished(scale)
        {
            self.marks.push(mark);
        }
    }

    fn undo(&mut self) {
        if self.typing.take().is_some() {
            return;
        }
        if self.marks.pop().is_none() {
            self.crop = None;
        }
    }

    fn scale(&self) -> f32 {
        self.shot.as_ref().map_or(1.0, |shot| shot.scale)
    }

    fn note(&mut self, note: String, tx: &mut Tx) {
        self.note = Some((note, self.now));
        tx.wake_after(Duration::from_secs_f64(NOTE_S));
    }

    /// An export to `out` failed for the reason `note` gives: it stays
    /// shown, with Retry, and nothing went out.
    fn failed(&mut self, note: String, out: Out, tx: &mut Tx) {
        self.note = Some((note, self.now));
        self.failed = Some(out);
        tx.animated();
    }

    /// The export that failed, again.
    fn retry(&mut self, tx: &mut Tx) {
        if let Some(out) = self.failed.clone() {
            self.deliver(out, tx);
        }
    }

    /// A drag on the shot, at `at` in its pixels.
    fn draw(&mut self, phase: Phase, at: [f32; 2]) {
        let Some(shot) = &self.shot else {
            return;
        };
        let (size, scale) = (shot.size, shot.scale);
        let at = [
            at[0].clamp(0.0, size[0] as f32),
            at[1].clamp(0.0, size[1] as f32),
        ];
        match (self.tool, phase) {
            (Tool::Text, Phase::Down) => {
                self.finish_typing();
                let line = marks::TEXT_PT * scale * 1.2;
                self.typing = Some(([at[0], at[1] - line * 0.5], String::new()));
            }
            (Tool::Text, _) => {}
            (Tool::Crop, Phase::Down) => self.cropping = Some((at, at)),
            (Tool::Crop, Phase::Move) => {
                if let Some((_, to)) = &mut self.cropping {
                    *to = at;
                }
            }
            (Tool::Crop, Phase::Up) => {
                if let Some((from, _)) = self.cropping.take() {
                    self.crop = marks::crop_rect(from, at, size, scale);
                }
            }
            (tool, Phase::Down) => {
                self.draft = Some(match tool {
                    Tool::Arrow => Mark::Arrow { from: at, to: at },
                    Tool::Box => Mark::Box { from: at, to: at },
                    _ => Mark::Pen(vec![at]),
                });
            }
            (_, Phase::Move) => match &mut self.draft {
                Some(Mark::Pen(points)) => points.push(at),
                Some(Mark::Arrow { to, .. } | Mark::Box { to, .. }) => *to = at,
                _ => {}
            },
            (_, Phase::Up) => {
                if let Some(mark) = self.draft.take().and_then(|mark| mark.finished(scale)) {
                    self.marks.push(mark);
                }
            }
        }
    }

    fn open_send(&mut self) {
        self.finish_typing();
        if self.send.take().is_some() {
            return;
        }
        self.send = Some(SendList {
            query: String::new(),
            sessions: Sessions::Loading,
            selected: 0,
        });
        load_sessions(self.overlay.clone());
    }

    /// A dictation over the shot is now `listen`. Leaving it idle keeps the
    /// words already heard.
    fn listen(&mut self, listen: Listen, tx: &mut Tx) {
        if !self.on {
            return;
        }
        self.listen = listen;
        if listen == Listen::Idle
            && let Some(dest) = self.waiting.take()
        {
            self.deliver(Out::Send(dest), tx);
        }
    }

    /// A dictation's words, or none when it had none; only while one is
    /// being transcribed, so words Escape cancelled never land.
    fn heard(&mut self, words: Option<String>, tx: &mut Tx) {
        if self.listen != Listen::Transcribing {
            return;
        }
        if let Some(words) = words.as_deref().map(str::trim).filter(|w| !w.is_empty()) {
            if !self.words.is_empty() {
                self.words.push(' ');
            }
            self.words.push_str(words);
        }
        self.listen(Listen::Idle, tx);
    }

    /// Escape while a dictation talks over the shot: its words never land,
    /// and a send waiting for them never goes.
    fn cancel_listening(&mut self) -> bool {
        if self.listen == Listen::Idle {
            return false;
        }
        self.listen = Listen::Idle;
        if let Some(request) = self.narration.take() {
            request.cancelled.store(true, Ordering::Release);
        }
        self.waiting = None;
        true
    }

    /// The file the editor opened: the shot's, or the recording's movie.
    fn source(&self) -> Option<&std::path::Path> {
        match &self.clip {
            Some(clip) => Some(&clip.movie),
            None => self.file.as_deref(),
        }
    }

    /// The save panel Save and ⌘S open: the opened file's own name, in the
    /// folder Save last went to, else the screenshots folder.
    fn save_dialog(&self) -> file_dialog::Dialog {
        let settings = crate::app_settings::screenshot_settings();
        let recording = self.clip.is_some();
        let (label, extensions): (&str, &'static [&'static str]) = if recording {
            ("MPEG-4 movie", &["mp4"])
        } else {
            ("PNG image", &["png"])
        };
        let file_name = self
            .source()
            .and_then(std::path::Path::file_name)
            .map_or_else(
                || unsaved_name(&settings.folder()),
                |name| name.to_string_lossy().into_owned(),
            );
        file_dialog::Dialog {
            title: tr(if recording { "Export" } else { "Save" }).to_owned(),
            target: file_dialog::Target::Save {
                filters: vec![file_dialog::Filter {
                    label: label.to_owned(),
                    extensions,
                }],
                file_name,
            },
            directory: Some(settings.export_folder()),
        }
    }

    /// Save: asks where with the native save panel, above the editor.
    /// Nothing is written, and the button shows no progress, until a path
    /// is chosen; a panel already open stays the one.
    fn choose_save(&mut self, tx: &mut Tx) {
        self.finish_typing();
        self.send = None;
        tx.animated();
        if !self.on || self.saving.is_some() || self.exporting.is_some() {
            return;
        }
        if self.picture().is_none() {
            return;
        }
        let dialog = self.save_dialog();
        let overlay = self.overlay.clone();
        self.saving = Some(file_dialog::open_over_overlays(dialog, move || {
            if let Some(overlay) = &overlay {
                overlay.update(Editor::save_answered);
            }
        }));
    }

    /// The save panel closed: takes its answer back to the editor.
    fn save_answered(&mut self, tx: &mut Tx) {
        let Some(choice) = self.saving.as_ref().and_then(file_dialog::Pending::poll) else {
            return;
        };
        self.saving = None;
        self.keys();
        self.save_to(choice, tx);
    }

    /// The panel's answer: a path gets the edited shot or the span kept;
    /// a cancel leaves everything as it was.
    fn save_to(&mut self, choice: file_dialog::Choice, tx: &mut Tx) {
        if !self.on {
            return;
        }
        match choice {
            Ok(Some(path)) => self.deliver(Out::Save(path), tx),
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, "could not open the save panel");
                self.note(tr("Couldn't save").to_owned(), tx);
            }
        }
    }

    /// The edited shot, to `out`, on its own thread. A send waits for a
    /// dictation still going.
    fn deliver(&mut self, out: Out, tx: &mut Tx) {
        self.finish_typing();
        self.send = None;
        if self.failed.take().is_some() {
            self.note = None;
        }
        if let Out::Send(dest) = &out
            && self.listen != Listen::Idle
        {
            self.note(
                tr_fill("Sending to {} once your words are in…", &dest.name),
                tx,
            );
            self.waiting = Some(dest.clone());
            return;
        }
        if self.clip.is_some() {
            self.deliver_clip(out, tx);
            return;
        }
        let Some(shot) = &self.shot else {
            return;
        };
        let job = Export {
            image: shot.image.clone(),
            rgba: shot.rgba.clone(),
            size: shot.size,
            scale: shot.scale,
            marks: self.marks.clone(),
            crop: self.crop,
        };
        if let Out::Send(dest) = &out {
            self.note(tr_fill("Sending to {}…", &dest.name), tx);
        }
        let words = self.words.trim().to_owned();
        let overlay = self.overlay.clone();
        let spawned = std::thread::Builder::new()
            .name("hex-screenshot-export".into())
            .spawn(move || {
                let result = job.png().and_then(|png| deliver(&out, &png, &words));
                let Some(overlay) = overlay else {
                    return;
                };
                match result {
                    Ok(()) => {
                        let note = match &out {
                            Out::Copy => tr("Copied").to_owned(),
                            Out::Save(path) => return saved(&overlay, path, None),
                            Out::Send(dest) => tr_fill("Sent to {}", &dest.name),
                        };
                        overlay.update(move |s, tx| s.note(note, tx));
                    }
                    Err(error) => {
                        tracing::warn!(error = %format!("{error:#}"), "could not export the edited screenshot");
                        let note = export_failure(&error, &out);
                        overlay.update(move |s, tx| s.failed(note, out, tx));
                    }
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not start a screenshot export");
        }
    }
}

impl Editor {
    /// The recording's span kept, to `out`, on its own thread, at the
    /// export size: the movie file on the clipboard, saved in the
    /// screenshots folder, or sent as a contact sheet with its words and
    /// the movie's path. A shrink shows its progress on the button pressed.
    fn deliver_clip(&mut self, out: Out, tx: &mut Tx) {
        let Some(clip) = &self.clip else {
            return;
        };
        if self.exporting.is_some() {
            return;
        }
        let settings = crate::app_settings::screenshot_settings();
        let shrink = clip::Shrink::of(self.export_size, &settings);
        let job = export::ClipExport {
            movie: clip.movie.clone(),
            trim: clip.trimmed().then_some(clip.trim),
            shrink,
            length_ms: clip.trim[1] - clip.trim[0],
            sheet: clip::sheet(&clip.sheet_frames()),
            words: clip.words_in_trim(),
        };
        let pressed = out.pressed();
        if shrink.is_some() {
            self.exporting = Some((pressed, 0.0));
            tx.animated();
        } else if let Out::Send(dest) = &out {
            self.note(tr_fill("Sending to {}…", &dest.name), tx);
        }
        let overlay = self.overlay.clone();
        let generation = self.generation;
        let spawned = std::thread::Builder::new()
            .name("hex-recording-export".into())
            .spawn(move || {
                let mut shown = 0u8;
                let mut progress = |share: f64| {
                    let percent = (share * 100.0).floor().clamp(0.0, 99.0) as u8;
                    if percent <= shown {
                        return;
                    }
                    shown = percent;
                    if let Some(overlay) = &overlay {
                        overlay.update(move |s, tx| {
                            if s.generation != generation {
                                return;
                            }
                            if let Some((_, done)) = &mut s.exporting {
                                *done = f32::from(percent) / 100.0;
                                tx.animated();
                            }
                        });
                    }
                };
                let result = job.deliver(&out, &mut progress);
                let Some(overlay) = overlay else {
                    return;
                };
                match result {
                    Ok(()) => {
                        let note = match &out {
                            Out::Copy => tr("Movie copied").to_owned(),
                            Out::Save(path) => {
                                return saved(&overlay, path, Some(generation));
                            }
                            Out::Send(dest) => tr_fill("Sent to {}", &dest.name),
                        };
                        overlay.update(move |s, tx| {
                            if s.generation == generation {
                                s.exporting = None;
                                s.note(note, tx);
                            }
                        });
                    }
                    Err(error) => {
                        tracing::warn!(error = %format!("{error:#}"), "could not export the recording");
                        let note = export_failure(&error, &out);
                        overlay.update(move |s, tx| {
                            if s.generation == generation {
                                s.exporting = None;
                                s.failed(note, out, tx);
                            }
                        });
                    }
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not start a recording export");
            self.exporting = None;
        }
    }

    /// A recording now exports at `size`, remembered as
    /// `screenshots.export_size`.
    fn pick_export_size(&mut self, size: ExportSize) {
        if self.export_size == size {
            return;
        }
        self.export_size = size;
        #[cfg(target_os = "macos")]
        crate::rune_ui::capture_capsule::toolbar::remember("export size", move |settings| {
            settings.export_size = size;
        });
    }

    /// About how large the recording's span kept is, exported at `size`.
    fn estimate(&self, size: ExportSize) -> Option<u64> {
        let clip = self.clip.as_ref()?;
        let settings = crate::app_settings::screenshot_settings();
        Some(clip.estimate(size, settings.quality, settings.fps()))
    }
}

/// A save to `path` is written: the panel starts in its folder next time,
/// and the note says so. Saved over the file the editor opened, that file
/// now holds what the editor shows, so the editor closes rather than let a
/// second save draw the marks or cut the span again. A recording's
/// `generation` drops the answer once another is open.
fn saved(overlay: &OverlayHandle<Editor>, path: &std::path::Path, generation: Option<u64>) {
    #[cfg(target_os = "macos")]
    if let Some(folder) = path.parent().map(std::path::Path::to_path_buf) {
        crate::rune_ui::capture_capsule::toolbar::remember("export folder", move |settings| {
            settings.export_folder = Some(folder.clone());
        });
    }
    let path = path.to_path_buf();
    overlay.update(move |s, tx| {
        if generation.is_some_and(|generation| generation != s.generation) {
            return;
        }
        s.exporting = None;
        if s.source() == Some(path.as_path()) {
            s.close(tx);
        } else {
            s.note(tr("Saved").to_owned(), tx);
        }
    });
}

/// The name a shot with no file of its own is offered under.
fn unsaved_name(folder: &std::path::Path) -> String {
    #[cfg(target_os = "macos")]
    let path = crate::screenshots::save::free_path(folder, crate::screenshots::save::Stamp::now());
    #[cfg(not(target_os = "macos"))]
    let path = folder.join("Hex Screenshot.png");
    path.file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

/// What the editor says when an export to `out` failed with `error`.
fn export_failure(error: &anyhow::Error, out: &Out) -> String {
    if error.downcast_ref::<export::MarksNotDrawn>().is_some() {
        return tr("Couldn't draw your marks").to_owned();
    }
    match out {
        Out::Copy => tr("Couldn't copy").to_owned(),
        Out::Save(_) => tr("Couldn't save").to_owned(),
        Out::Send(_) => tr("Couldn't send").to_owned(),
    }
}

/// `pos` moved so the panel stays on the display.
fn clamp_panel(pos: [f32; 2], size: [f32; 2], screen: [f32; 2]) -> [f32; 2] {
    [
        pos[0].clamp(0.0, (screen[0] - size[0]).max(0.0)).round(),
        pos[1].clamp(0.0, (screen[1] - size[1]).max(0.0)).round(),
    ]
}

/// The first [`LIST_ROWS`] sessions matching every word of `query` in their
/// title, folder, or id, title matches first, as the capsule's search.
fn matching<'a>(dests: &'a [Dest], query: &str) -> Vec<&'a Dest> {
    let query = query.to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut found: Vec<(bool, &Dest)> = dests
        .iter()
        .filter_map(|dest| {
            let title = dest.name.to_lowercase();
            let folder = dest.folder.as_deref().unwrap_or_default().to_lowercase();
            let id = dest.session.to_lowercase();
            let in_title = words.iter().all(|word| title.contains(word));
            let found = in_title
                || words
                    .iter()
                    .all(|word| [&title, &folder, &id].iter().any(|t| t.contains(word)));
            found.then_some((in_title, dest))
        })
        .collect();
    found.sort_by_key(|(in_title, _)| !in_title);
    found.truncate(LIST_ROWS);
    found.into_iter().map(|(_, dest)| dest).collect()
}

// ─── Keys and timers ─────────────────────────────────────────────────────────

fn on_key(s: &mut Editor, key: &KeyEvent, tx: &mut Tx) -> bool {
    if !s.on {
        return false;
    }
    tx.animated();
    if key.is_command('z') {
        s.undo();
        return true;
    }
    if key.is_command('c') && s.typing.is_none() {
        s.deliver(Out::Copy, tx);
        return true;
    }
    if key.is_command('s') {
        s.choose_save(tx);
        return true;
    }
    if let Some((_, typed)) = &mut s.typing {
        match &key.key {
            Key::Text(t) if !key.mods.cmd => typed.extend(t.chars().filter(|c| !c.is_control())),
            Key::Backspace => {
                typed.pop();
            }
            Key::Enter | Key::Escape => s.finish_typing(),
            _ => return false,
        }
        return true;
    }
    if let Some(list) = &mut s.send {
        let shown = match &list.sessions {
            Sessions::Ready(dests) => matching(dests, &list.query).into_iter().cloned().collect(),
            _ => Vec::new(),
        };
        match &key.key {
            Key::Text(t) if !key.mods.cmd => {
                list.query.extend(t.chars().filter(|c| !c.is_control()));
                list.selected = 0;
            }
            Key::Backspace => {
                list.query.pop();
                list.selected = 0;
            }
            Key::Down => list.selected = (list.selected + 1).min(shown.len().saturating_sub(1)),
            Key::Up => list.selected = list.selected.saturating_sub(1),
            Key::Enter => {
                if let Some(dest) = shown.get(list.selected).cloned() {
                    s.deliver(Out::Send(dest), tx);
                }
            }
            Key::Escape => s.send = None,
            _ => return false,
        }
        return true;
    }
    if let (Key::Text(t), Some(clip)) = (&key.key, &mut s.clip)
        && t == " "
        && !key.mods.cmd
    {
        clip.toggle_play();
        tx.wake_after(Duration::ZERO);
        return true;
    }
    if key.key == Key::Escape {
        // Escape first cancels a dictation over the shot; the panel stays.
        if s.cancel_listening() {
            talk(Talk::Cancel);
        } else {
            s.close(tx);
        }
        return true;
    }
    false
}

fn poll(s: &mut Editor, now: f64, tx: &mut Tx) -> bool {
    s.now = now;
    let mut moved = s.step_zoom(now, tx);
    if s.zooming_out() {
        return moved;
    }
    if let Some(clip) = &mut s.clip
        && clip.playing
    {
        moved |= clip.tick(now);
        tx.wake_after(PLAY_STEP);
    }
    if let Some(at) = s.leaving {
        if now - at >= LEAVE_S {
            s.leaving = None;
            s.shot = None;
            s.clip = None;
            if let Some(overlay) = &s.overlay {
                overlay.hide();
            }
            return true;
        }
        tx.wake_after(Duration::from_secs_f64(at + LEAVE_S - now));
        return false;
    }
    if let Some((_, at)) = &s.note
        && s.failed.is_none()
    {
        let left = at + NOTE_S - now;
        if left <= 0.0 {
            s.note = None;
            return true;
        }
        tx.wake_after(Duration::from_secs_f64(left));
    }
    moved
}

// ─── Opening, exporting, sending ─────────────────────────────────────────────

static EDITOR: OnceLock<OverlayHandle<Editor>> = OnceLock::new();

/// What the microphone chip and Escape ask of the listener.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Talk {
    /// Start a dictation, or finish the one going, as the shortcut does.
    Toggle,
    /// Cancel the dictation going; nothing is transcribed or sent.
    Cancel,
}

fn talk(talk: Talk) {
    #[cfg(target_os = "macos")]
    crate::screenshots::narration::talk(talk);
    #[cfg(not(target_os = "macos"))]
    let _ = talk;
}

/// Adds the editor's overlay, hidden. Hidden, it holds no GPU surface.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) {
    use rune::{
        DisplayTarget, KeyPolicy, OverlayInput, OverlayOptions, OverlayPlacement, OverlaySpec,
    };
    let spec = OverlaySpec::new("Hex screenshot editor", (1440.0, 900.0), view::view)
        .options(OverlayOptions {
            placement: Some(OverlayPlacement::FullScreen),
            display: DisplayTarget::UnderPointer,
            exclude_from_capture: true,
            input: OverlayInput::Regions,
            keys: KeyPolicy::OnDemand,
            ..Default::default()
        })
        .release_gpu_when_hidden()
        .on_key(on_key)
        .poll(poll);
    let overlay = app.overlay(spec, Editor::default());
    let handle = overlay.clone();
    overlay.update(move |s, _| s.overlay = Some(handle));
    let _ = EDITOR.set(overlay);
}

/// Opens the Screenshots recording `movie`, with the `words` said over it,
/// to trim, in place of anything open. Safe from any thread; reads its
/// frames on its own.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn open_recording(movie: PathBuf, words: Vec<clip::Word>) {
    open_recording_from(movie, words, None);
}

/// [`open_recording`] from card `from`, whose picture flies up into the
/// editor.
pub(crate) fn open_recording_from(movie: PathBuf, words: Vec<clip::Word>, from: Option<From>) {
    let Some(overlay) = EDITOR.get().cloned() else {
        zoom::unopened(from);
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("hex-recording-editor".into())
        .spawn(move || {
            let clip = match clip::load(movie.clone(), words) {
                Ok(clip) => clip,
                Err(error) => {
                    tracing::warn!(%error, movie = %movie.display(), "could not open the recording");
                    zoom::unopened(from);
                    return;
                }
            };
            let screen = export::screen_under_pointer();
            let handle = overlay.clone();
            overlay.update(move |s, tx| {
                // Dictation stays dictation: nothing talks over a recording.
                *OPEN.lock().unwrap_or_else(|e| e.into_inner()) = None;
                s.load_clip(clip, screen);
                s.zoom = from.map(zoom::Zoom::new);
                if let Some(clip) = &mut s.clip {
                    let player = rune::Player::file(clip.movie.clone());
                    player.set_muted(false);
                    player.preload();
                    clip.player = Some(player);
                }
                handle.show();
                handle.request_keys();
                tx.animated();
            });
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not open the recording editor");
        zoom::unopened(from);
    }
}

/// Opens the shot at `path` in the editor with `words` already said over
/// it, in place of anything open. Safe from any thread; reads the file on
/// its own.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn open_with(path: PathBuf, words: String) {
    open_from(path, words, None);
}

/// [`open_with`] from card `from`, whose shot flies up into the editor.
pub(crate) fn open_from(path: PathBuf, words: String, from: Option<From>) {
    let Some(overlay) = EDITOR.get().cloned() else {
        zoom::unopened(from);
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("hex-screenshot-editor".into())
        .spawn(move || {
            let shot = match export::decode(&path) {
                Ok(shot) => shot,
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "could not open the screenshot");
                    zoom::unopened(from);
                    return;
                }
            };
            let screen = export::screen_under_pointer();
            let handle = overlay.clone();
            overlay.update(move |s, tx| {
                s.load(shot, screen);
                s.zoom = from.map(zoom::Zoom::new);
                s.file = Some(path);
                s.words = words;
                s.listen = Listen::Idle;
                s.waiting = None;
                s.generation = GENERATION.fetch_add(1, Ordering::Relaxed);
                *OPEN.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some((s.generation, s.file.clone().unwrap()));
                handle.show();
                handle.request_keys();
                tx.animated();
            });
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not open the screenshot editor");
        zoom::unopened(from);
    }
}

#[cfg(test)]
mod tests;

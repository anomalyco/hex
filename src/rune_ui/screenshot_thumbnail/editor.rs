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
mod view;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use rune::{Key, KeyEvent, OverlayHandle, Phase, Tx};

use self::clip::Clip;
use self::export::{Export, deliver, load_sessions};
use self::marks::{Mark, Rect, Tool};
use crate::desktop::i18n::{tr, tr_fill};
use crate::rune_ui::capture_capsule::surface::CHIP_H;

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
    Save,
    Send(Dest),
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
    /// A send picked while a dictation was still going: it goes once the
    /// words land, and never if Escape cancels them.
    waiting: Option<Dest>,
    /// A short note and the animation seconds it appeared.
    note: Option<(String, f64)>,
    on: bool,
    leaving: Option<f64>,
    now: f64,
    overlay: Option<OverlayHandle<Editor>>,
}

/// Where the panel's parts lie, in the overlay's points.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    panel: [f32; 4],
    /// The shot inside the panel.
    image: [f32; 4],
    /// A recording's filmstrip, under its picture; empty for a shot.
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
        *self = Self {
            clip: Some(clip),
            screen,
            on: true,
            now: self.now,
            overlay: self.overlay.take(),
            tool: self.tool,
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
            strip: [PAD, sy, pw - 2.0 * PAD, strip_h - GAP],
            words: [PAD, sy + strip_h, pw - 2.0 * PAD, words_h],
            fit,
        }
    }

    fn close(&mut self, tx: &mut Tx) {
        if self.on {
            OPEN.store(false, Ordering::Release);
            self.on = false;
            self.send = None;
            self.waiting = None;
            self.leaving = Some(self.now);
            tx.animated();
            tx.wake_after(Duration::from_secs_f64(LEAVE_S));
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
            #[cfg(target_os = "macos")]
            if let Some(file) = &self.file {
                crate::screenshots::remember_words(file.clone(), self.words.clone());
            }
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
        self.waiting = None;
        true
    }

    /// The edited shot, to `out`, on its own thread. A send waits for a
    /// dictation still going.
    fn deliver(&mut self, out: Out, tx: &mut Tx) {
        self.finish_typing();
        self.send = None;
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
                let note = match job.png().and_then(|png| deliver(&out, &png, &words)) {
                    Ok(()) => match &out {
                        Out::Copy => tr("Copied").to_owned(),
                        Out::Save => tr("Saved").to_owned(),
                        Out::Send(dest) => tr_fill("Sent to {}", &dest.name),
                    },
                    Err(error) => {
                        tracing::warn!(%error, "could not export the edited screenshot");
                        match &out {
                            Out::Copy => tr("Couldn't copy").to_owned(),
                            Out::Save => tr("Couldn't save").to_owned(),
                            Out::Send(_) => tr("Couldn't send").to_owned(),
                        }
                    }
                };
                if let Some(overlay) = overlay {
                    overlay.update(move |s, tx| s.note(note, tx));
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not start a screenshot export");
        }
    }
}

impl Editor {
    /// The recording's span kept, to `out`, on its own thread: saved as a
    /// movie in the screenshots folder, or sent as a contact sheet with its
    /// words and the movie's path. It never goes to the clipboard.
    fn deliver_clip(&mut self, out: Out, tx: &mut Tx) {
        let Some(clip) = &self.clip else {
            return;
        };
        if matches!(out, Out::Copy) {
            return;
        }
        let job = export::ClipExport {
            movie: clip.movie.clone(),
            trim: clip.trimmed().then_some(clip.trim),
            length_ms: clip.trim[1] - clip.trim[0],
            sheet: clip::sheet(&clip.sheet_frames()),
            words: clip.words_in_trim(),
        };
        if let Out::Send(dest) = &out {
            self.note(tr_fill("Sending to {}…", &dest.name), tx);
        }
        let overlay = self.overlay.clone();
        let spawned = std::thread::Builder::new()
            .name("hex-recording-export".into())
            .spawn(move || {
                let note = match job.deliver(&out) {
                    Ok(()) => match &out {
                        Out::Send(dest) => tr_fill("Sent to {}", &dest.name),
                        _ => tr("Saved").to_owned(),
                    },
                    Err(error) => {
                        tracing::warn!(%error, "could not export the recording");
                        match &out {
                            Out::Send(_) => tr("Couldn't send").to_owned(),
                            _ => tr("Couldn't save").to_owned(),
                        }
                    }
                };
                if let Some(overlay) = overlay {
                    overlay.update(move |s, tx| s.note(note, tx));
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not start a recording export");
        }
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
    if key.is_command('c') && s.typing.is_none() && s.clip.is_none() {
        s.deliver(Out::Copy, tx);
        return true;
    }
    if key.is_command('s') {
        s.deliver(Out::Save, tx);
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
    if let Some((_, at)) = &s.note {
        let left = at + NOTE_S - now;
        if left <= 0.0 {
            s.note = None;
            return true;
        }
        tx.wake_after(Duration::from_secs_f64(left));
    }
    false
}

// ─── Opening, exporting, sending ─────────────────────────────────────────────

static EDITOR: OnceLock<OverlayHandle<Editor>> = OnceLock::new();
/// Whether a shot is open, for the listener's thread.
static OPEN: AtomicBool = AtomicBool::new(false);

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

/// Whether a shot is open in the editor. Safe from any thread.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn is_open() -> bool {
    OPEN.load(Ordering::Acquire)
}

/// A dictation over the open shot is now `listen`. Safe from any thread.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn listen(listen: Listen) {
    if let Some(overlay) = EDITOR.get() {
        overlay.update(move |s, tx| {
            s.listen(listen, tx);
            tx.animated();
        });
    }
}

/// A dictation over the open shot ended with `words`, or none. Safe from
/// any thread.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn heard(words: Option<String>) {
    if let Some(overlay) = EDITOR.get() {
        overlay.update(move |s, tx| {
            s.heard(words, tx);
            tx.animated();
        });
    }
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
        .animate_reveal()
        .release_gpu_when_hidden()
        .on_key(on_key)
        .poll(poll);
    let overlay = app.overlay(spec, Editor::default());
    let handle = overlay.clone();
    overlay.update(move |s, _| s.overlay = Some(handle));
    let _ = EDITOR.set(overlay);
}

/// Opens the shot at `path` in the editor, in place of any open. Safe from
/// any thread; reads the file on its own.
pub(crate) fn open(path: PathBuf) {
    open_with(path, String::new());
}

/// Opens the Screenshots recording `movie`, with the `words` said over it,
/// to trim, in place of anything open. Safe from any thread; reads its
/// frames on its own.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn open_recording(movie: PathBuf, words: Vec<clip::Word>) {
    let Some(overlay) = EDITOR.get().cloned() else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("hex-recording-editor".into())
        .spawn(move || {
            let clip = match clip::load(movie.clone(), words) {
                Ok(clip) => clip,
                Err(error) => {
                    tracing::warn!(%error, movie = %movie.display(), "could not open the recording");
                    return;
                }
            };
            let screen = export::screen_under_pointer();
            let handle = overlay.clone();
            overlay.update(move |s, tx| {
                // Dictation stays dictation: nothing talks over a recording.
                OPEN.store(false, Ordering::Release);
                s.load_clip(clip, screen);
                handle.show();
                handle.request_keys();
                tx.animated();
            });
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not open the recording editor");
    }
}

/// [`open`] with `words` already said over the shot.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn open_with(path: PathBuf, words: String) {
    let Some(overlay) = EDITOR.get().cloned() else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("hex-screenshot-editor".into())
        .spawn(move || {
            let shot = match export::decode(&path) {
                Ok(shot) => shot,
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "could not open the screenshot");
                    return;
                }
            };
            let screen = export::screen_under_pointer();
            let handle = overlay.clone();
            overlay.update(move |s, tx| {
                s.load(shot, screen);
                s.file = Some(path);
                s.words = words;
                s.listen = Listen::Idle;
                s.waiting = None;
                OPEN.store(true, Ordering::Release);
                handle.show();
                handle.request_keys();
                tx.animated();
            });
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not open the screenshot editor");
    }
}

#[cfg(test)]
mod tests;

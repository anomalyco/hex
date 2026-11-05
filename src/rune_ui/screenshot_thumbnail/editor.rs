//! Labs › Screenshots: the editor a click on the thumbnail opens, a panel
//! floating over the display under the pointer, as CleanShot's is. It
//! shows the shot with a pen, an arrow, a box, text, and a crop, and the
//! edited shot goes to the clipboard, a new file beside the first, or an
//! OpenCode session. Nothing changes the shot already saved. Escape or ✕
//! closes it; until then it stays, and the panel moves by its edges.

mod export;
pub(crate) mod marks;
mod view;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use rune::{Key, KeyEvent, OverlayHandle, Phase, Tx};

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

#[derive(Default)]
pub(crate) struct Editor {
    shot: Option<Shot>,
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
    /// Points per shot pixel.
    fit: f32,
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

    fn layout(&self) -> Layout {
        let [sw, sh] = self.screen;
        let (size, scale) = self
            .shot
            .as_ref()
            .map_or(([1, 1], 1.0), |shot| (shot.size, shot.scale));
        let (w, h) = (size[0].max(1) as f32, size[1].max(1) as f32);
        let room_w = (sw - 2.0 * (MARGIN + PAD)).max(1.0);
        let room_h = (sh - 2.0 * (MARGIN + PAD) - CHIP_H - GAP).max(1.0);
        let fit = (room_w / w).min(room_h / h).min(1.0 / scale.max(1.0));
        let (iw, ih) = ((w * fit).round(), (h * fit).round());
        let pw = (iw + 2.0 * PAD).max(MIN_W);
        let ph = PAD + CHIP_H + GAP + ih + PAD;
        let [x, y] = self.pos.unwrap_or([(sw - pw) * 0.5, (sh - ph) * 0.5]);
        let [x, y] = clamp_panel([x, y], [pw, ph], self.screen);
        Layout {
            panel: [x, y, pw, ph],
            image: [((pw - iw) * 0.5).round(), PAD + CHIP_H + GAP, iw, ih],
            fit,
        }
    }

    fn close(&mut self, tx: &mut Tx) {
        if self.on {
            self.on = false;
            self.send = None;
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

    /// The edited shot, to `out`, on its own thread.
    fn deliver(&mut self, out: Out, tx: &mut Tx) {
        self.finish_typing();
        self.send = None;
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
        let overlay = self.overlay.clone();
        let spawned = std::thread::Builder::new()
            .name("hex-screenshot-export".into())
            .spawn(move || {
                let note = match job.png().and_then(|png| deliver(&out, &png)) {
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
        s.close(tx);
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

//! The shots and recordings just taken, as cards stacked in the
//! bottom-right corner of the display under the pointer, as CleanShot's
//! Quick Access Overlay shows them: the newest nearest the corner, the older
//! ones above it. Each stays until it's closed. A click opens one in the
//! editor, a drag carries its file out, ✕ closes it, and a right-click
//! offers the rest. While the pointer is on a card, its keys work
//! (`screenshots::key` takes them in the session tap): ⌘C copies, ⌘S shows
//! the file in Finder, ⌘E opens the editor, ⌘W or Escape closes it, Return
//! keeps it and closes. A dictation while cards show pastes as usual; only
//! the open editor takes the words (`screenshots::narration`).
//!
//! A Screenshots recording lands here too, as its first frame with its
//! length; a click opens it in the editor to trim.
//!
//! The cards closed last are kept, so the menu bar's Restore Last Capture
//! brings the newest of them back ([`Thumbnail::restore`]).

mod card;
pub(crate) mod editor;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rune::{OverlayHandle, OverlaySpec, ScreenEdge, Spring, Tx};

use crate::desktop::i18n::tr;

/// The overlay, before a shot gives it its display's size: a laptop's.
pub(crate) const NOMINAL: (f32, f32) = (1440.0, 900.0);
/// The display a fixture card rests on, small enough to read in a golden.
const PREVIEW: (f32, f32) = (400.0, 300.0);
const CARD_MAX: (f32, f32) = (240.0, 168.0);
/// The corner the card's own corner follows. macOS gives no display's
/// corner radius (and a MacBook's bottom corners are square glass), so this
/// is a design constant: the card's corner is concentric with it.
const SCREEN_RADIUS: f32 = 30.0;
/// From the display's edges to the cards'.
const INSET: f32 = 16.0;
/// Between stacked cards.
const GAP: f32 = 12.0;
/// Concentric with [`SCREEN_RADIUS`], and so the capture capsule's shell.
const RADIUS: f32 = SCREEN_RADIUS - INSET;
/// ✕ sits inside the card's corner, its corner concentric with the card's.
const CLOSE_INSET: f32 = 6.0;
const CLOSE_RADIUS: f32 = RADIUS - CLOSE_INSET;
const FADE: Spring = Spring::new(0.22, 0.0);
/// In on the HUD family's spring, a little bounce; out shorter, none. The
/// older cards make room on it too.
const ENTER: Spring = super::capture_capsule::tuning::spring(crate::rune_hud::spring::MORPH);
const LEAVE: Spring = Spring::new(0.24, 0.0);
const REDUCED: Spring =
    super::capture_capsule::tuning::spring(crate::rune_hud::spring::REDUCED_FADE);
/// How far it leans back as it travels, about its bottom-right corner.
const TRAVEL_SCALE: f32 = 0.94;
/// The shot growing in from the rectangle it was taken from: one spring,
/// and no bounce, so its size never overshoots.
const ZOOM: Spring = Spring::new(0.34, 0.0);
/// Once in this long, a card that grew in from its rectangle leaves past
/// the display's edge instead, as any other card does.
const SETTLE_S: f64 = 1.0;
/// How long leaving takes, before the empty overlay hides.
const LEAVE_S: f64 = 0.4;
/// How long a note such as "Copied" shows on its card.
const NOTE_S: f64 = 1.6;
/// The recording pill's height, inside the card's bottom-left corner.
const PILL_H: f32 = 24.0;

/// How many closed cards Restore Last Capture can bring back.
const RECENT: usize = 10;

/// Whether the pointer is on a card, for the session tap's thread.
static HOVERED: AtomicBool = AtomicBool::new(false);
/// Whether a closed card can come back, for the menu bar.
static RESTORABLE: AtomicBool = AtomicBool::new(false);

/// Whether Restore Last Capture has a card to bring back. Safe from any
/// thread.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn restorable() -> bool {
    RESTORABLE.load(Ordering::Acquire)
}

/// Whether the pointer is on a card, so its keys go to it. Safe from any
/// thread.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn hovered() -> bool {
    HOVERED.load(Ordering::Acquire)
}

/// The shot a card shows.
pub(crate) struct Shot {
    /// The PNG, or a recording's movie, both in the screenshots folder.
    pub(crate) path: PathBuf,
    pub(crate) image: rune::Image,
    /// Its size in pixels.
    pub(crate) size: [u32; 2],
    pub(crate) recording: Option<Recording>,
}

/// What a recording's card adds to its first frame.
#[derive(Clone, Debug)]
pub(crate) struct Recording {
    pub(crate) duration_ms: u64,
    /// The words said over it, for the editor.
    pub(crate) words: Vec<editor::clip::Word>,
}

/// One card in the stack.
struct Entry {
    id: u64,
    shot: Shot,
    /// Where it was taken from on the display, in points, when it was an
    /// area or a window: it grows in from there.
    capture: Option<[f32; 4]>,
    /// Animation seconds it first drew; none until then.
    born: Option<f64>,
    /// A short note on it and the animation seconds it appeared.
    note: Option<(String, f64)>,
}

/// What a key or the menu does to a card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CardKey {
    /// ⌘C: the file (and a shot's picture) on the clipboard.
    Copy,
    /// ⌘S: the file, already saved, shown in Finder.
    Reveal,
    /// ⌘E: the editor.
    Edit,
    /// ⌘W or Escape.
    Close,
    /// Return: the file is already saved, so this closes it. Only the
    /// macOS session tap sends it.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Keep,
    /// The menu's Move to Trash.
    Trash,
}

#[derive(Default)]
pub(crate) struct Cards {
    /// Oldest first; the last is nearest the corner.
    cards: Vec<Entry>,
    /// The cards closed last, the newest last, at most [`RECENT`].
    closed: Vec<Entry>,
    /// The display's size in points, where the cards rest in its corner.
    screen: (f32, f32),
    /// The card under the pointer.
    hovered: Option<u64>,
    next: u64,
    /// Animation seconds the last card left; the overlay hides once it's
    /// gone.
    leaving: Option<f64>,
    now: f64,
    overlay: Option<OverlayHandle<Cards>>,
}

impl Cards {
    /// `shots` showing at rest on a fixture display `screen` points large,
    /// the first oldest, with the pointer on the one `hovered` names.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn still(shots: Vec<Shot>, screen: (f32, f32), hovered: Option<usize>) -> Self {
        let mut cards = Self {
            screen,
            ..Self::default()
        };
        for shot in shots {
            cards.push(shot, None);
        }
        for card in &mut cards.cards {
            card.born = Some(0.0);
        }
        cards.hovered = hovered.and_then(|i| cards.cards.get(i)).map(|card| card.id);
        cards
    }

    /// One `shot` showing, as a still on the preview display.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn one(shot: Shot, hovered: bool) -> Self {
        Self::still(vec![shot], PREVIEW, hovered.then_some(0))
    }

    /// Adds `shot` nearest the corner. The oldest cards that no longer fit
    /// on the display close.
    fn push(&mut self, shot: Shot, capture: Option<[f32; 4]>) -> u64 {
        self.next += 1;
        let id = self.next;
        self.cards.push(Entry {
            id,
            shot,
            capture,
            born: None,
            note: None,
        });
        self.leaving = None;
        let room = self.screen.1 - 2.0 * INSET;
        while self.cards.len() > 1 && self.stack_height() > room {
            let oldest = self.cards[0].id;
            self.retire(oldest);
        }
        id
    }

    /// Brings the card closed last back, nearest the corner, unless its
    /// file is gone; false when there's none.
    fn restore(&mut self) -> bool {
        while let Some(card) = self.closed.pop() {
            if card.shot.path.exists() {
                self.push(card.shot, None);
                self.restorable();
                return true;
            }
        }
        self.restorable();
        false
    }

    fn restorable(&self) {
        RESTORABLE.store(!self.closed.is_empty(), Ordering::Release);
    }

    /// Takes card `id` out of the stack, keeping it for Restore.
    fn retire(&mut self, id: u64) {
        if let Some(mut card) = self.remove(id) {
            card.note = None;
            card.capture = None;
            card.born = None;
            self.closed.push(card);
            if self.closed.len() > RECENT {
                self.closed.remove(0);
            }
            self.restorable();
        }
    }

    /// The stack's height, cards and gaps.
    fn stack_height(&self) -> f32 {
        let heights: f32 = self.cards.iter().map(|card| card_size(&card.shot).1).sum();
        heights + GAP * self.cards.len().saturating_sub(1) as f32
    }

    /// Each card's id and rectangle on the display, oldest first: the newest
    /// in the corner, each older one above the one after it.
    fn slots(&self) -> Vec<(u64, [f32; 4])> {
        let (sw, sh) = self.screen;
        let mut bottom = sh - INSET;
        let mut slots: Vec<(u64, [f32; 4])> = self
            .cards
            .iter()
            .rev()
            .map(|card| {
                let (w, h) = card_size(&card.shot);
                let y = bottom - h;
                bottom = y - GAP;
                (card.id, [sw - INSET - w, y, w, h])
            })
            .collect();
        slots.reverse();
        slots
    }

    fn get(&self, id: u64) -> Option<&Entry> {
        self.cards.iter().find(|card| card.id == id)
    }

    /// Takes card `id` out of the stack; the ones above it close the gap.
    fn remove(&mut self, id: u64) -> Option<Entry> {
        let at = self.cards.iter().position(|card| card.id == id)?;
        if self.hovered == Some(id) {
            self.hover(None);
        }
        Some(self.cards.remove(at))
    }

    fn hover(&mut self, id: Option<u64>) {
        self.hovered = id;
        HOVERED.store(id.is_some(), Ordering::Release);
    }

    /// Closes card `id`: it leaves, and the cards above it move down.
    /// Restore Last Capture can bring it back.
    fn close(&mut self, id: u64, tx: &mut Tx) {
        if self.get(id).is_some() {
            self.retire(id);
            tx.animated();
            tx.wake_after(Duration::ZERO);
        }
    }

    /// Takes card `id` away for good, its file going to the Trash.
    fn discard(&mut self, id: u64, tx: &mut Tx) {
        if self.remove(id).is_some() {
            tx.animated();
            tx.wake_after(Duration::ZERO);
        }
    }

    fn note(&mut self, id: u64, note: String, tx: &mut Tx) {
        let now = self.now;
        if let Some(card) = self.cards.iter_mut().find(|card| card.id == id) {
            card.note = Some((note, now));
            tx.animated();
            tx.wake_after(Duration::from_secs_f64(NOTE_S));
        }
    }

    /// What `key` does to card `id`.
    fn act(&mut self, id: u64, key: CardKey, tx: &mut Tx) {
        let Some(card) = self.get(id) else {
            return;
        };
        let path = card.shot.path.clone();
        let recording = card.shot.recording.is_some();
        match key {
            CardKey::Edit => self.open(id, tx),
            CardKey::Close | CardKey::Keep => self.close(id, tx),
            CardKey::Copy => {
                self.note(id, tr("Copied").to_owned(), tx);
                files::copy(path, recording, self.overlay.clone(), id);
            }
            CardKey::Reveal => files::reveal(&path),
            CardKey::Trash => {
                self.discard(id, tx);
                files::trash(path);
            }
        }
    }

    /// Opens card `id` in the editor; the card closes.
    fn open(&mut self, id: u64, tx: &mut Tx) {
        let Some(card) = self.get(id) else {
            return;
        };
        match &card.shot.recording {
            Some(recording) => {
                editor::open_recording(card.shot.path.clone(), recording.words.clone())
            }
            None => editor::open(card.shot.path.clone()),
        }
        self.close(id, tx);
    }
}

/// A card's size: its shot fitted into [`CARD_MAX`].
fn card_size(shot: &Shot) -> (f32, f32) {
    let [w, h] = shot.size;
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let fit = (CARD_MAX.0 / w).min(CARD_MAX.1 / h);
    ((w * fit).round().max(48.0), (h * fit).round().max(32.0))
}

fn poll(s: &mut Cards, now: f64, tx: &mut Tx) -> bool {
    s.now = now;
    let mut changed = false;
    let mut next: Option<f64> = None;
    let mut soonest = |at: f64| next = Some(next.map_or(at, |n: f64| n.min(at)));
    for card in &mut s.cards {
        let born = *card.born.get_or_insert(now);
        if card.capture.is_some() && now - born < SETTLE_S {
            soonest(born + SETTLE_S);
        } else if card.capture.take().is_some() {
            changed = true;
        }
        if let Some((_, at)) = &card.note {
            if now - at >= NOTE_S {
                card.note = None;
                changed = true;
            } else {
                soonest(at + NOTE_S);
            }
        }
    }
    if s.cards.is_empty() {
        let at = *s.leaving.get_or_insert(now);
        if now - at >= LEAVE_S {
            s.leaving = None;
            if let Some(overlay) = &s.overlay {
                overlay.hide();
            }
            return true;
        }
        soonest(at + LEAVE_S);
    }
    if let Some(at) = next {
        tx.wake_after(Duration::from_secs_f64((at - now).max(0.0)));
    }
    changed
}

/// The cards' overlay, as the screenshot thread holds it.
#[derive(Clone)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct Thumbnail {
    overlay: OverlayHandle<Cards>,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl Thumbnail {
    /// Adds the overlay, hidden, made the first time it shows.
    pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) -> Self {
        let spec = OverlaySpec {
            name: "Hex screenshot",
            size: NOMINAL,
            edge: ScreenEdge::Bottom,
            margin: 0.0,
            align: rune::Align::End,
            inset: 0.0,
            interactive: true,
            view: card::view,
        }
        .options(rune::OverlayOptions {
            display: rune::DisplayTarget::UnderPointer,
            exclude_from_capture: true,
            input: rune::OverlayInput::Regions,
            ..Default::default()
        })
        .animate_reveal()
        .poll(poll);
        let overlay = app.overlay(spec, Cards::default());
        let handle = overlay.clone();
        overlay.update(move |s, _| s.overlay = Some(handle));
        Self { overlay }
    }

    /// Shows the shot at `path`, from `rgba` (`width` × `height` pixels,
    /// already small), on the display `screen` points across, nearest the
    /// corner, above any before it. `capture` is the rectangle it was taken
    /// from on that display, in points, when it was an area or a window.
    /// Safe from any thread; waits for the decode.
    pub(crate) fn show(
        &self,
        path: PathBuf,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        screen: (f32, f32),
        capture: Option<[f32; 4]>,
    ) {
        self.present(path, None, width, height, rgba, screen, capture);
    }

    /// Shows the recording saved at `movie`, from its first frame, as
    /// [`Self::show`] does a shot. Nothing talks over it: it already has its
    /// words.
    pub(crate) fn show_recording(
        &self,
        movie: PathBuf,
        recording: Recording,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        screen: (f32, f32),
    ) {
        self.present(movie, Some(recording), width, height, rgba, screen, None);
    }

    /// Brings back the card closed last, nearest the corner of the display
    /// the cards were last on. Safe from any thread.
    pub(crate) fn restore(&self) {
        let handle = self.overlay.clone();
        self.overlay.update(move |s, tx| {
            if s.restore() {
                handle.show();
                tx.animated();
                tx.wake_after(Duration::ZERO);
            }
        });
    }

    /// What `key` does to the card under the pointer, if any. Safe from
    /// any thread.
    pub(crate) fn key(&self, key: CardKey) {
        self.overlay.update(move |s, tx| {
            if let Some(id) = s.hovered {
                s.act(id, key, tx);
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn present(
        &self,
        path: PathBuf,
        recording: Option<Recording>,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        screen: (f32, f32),
        capture: Option<[f32; 4]>,
    ) {
        let images = rune::ImageCache::new(std::sync::Arc::new(|| {}));
        let image = images.rgba(width, height, rgba);
        if image.wait().is_err() {
            return;
        }
        // The overlay is the whole display, so a card can start wherever
        // its shot was taken; the stack follows the newest to its display.
        self.overlay.resize(screen);
        let handle = self.overlay.clone();
        self.overlay.update(move |s, tx| {
            s.screen = screen;
            let shot = Shot {
                path,
                image,
                size: [width, height],
                recording,
            };
            s.push(shot, capture);
            handle.show();
            tx.animated();
            tx.wake_after(Duration::ZERO);
        });
    }
}

/// What a card does with its file, off the main thread.
pub(crate) mod files {
    use std::path::{Path, PathBuf};

    use rune::OverlayHandle;

    use super::Cards;

    /// The file on the clipboard, and a shot's picture with it; a failure
    /// says so on card `id`.
    pub(super) fn copy(
        path: PathBuf,
        recording: bool,
        overlay: Option<OverlayHandle<Cards>>,
        id: u64,
    ) {
        spawn("hex-card-copy", move || {
            #[cfg(target_os = "macos")]
            let copied = crate::screenshots::save::copy_saved(&path, recording);
            #[cfg(not(target_os = "macos"))]
            let copied: anyhow::Result<()> = {
                let _ = (&path, recording);
                Err(anyhow::anyhow!("cards copy on macOS only"))
            };
            if let Err(error) = copied {
                tracing::warn!(%error, path = %path.display(), "could not copy the capture");
                if let Some(overlay) = overlay {
                    overlay.update(move |s, tx| {
                        s.note(id, crate::desktop::i18n::tr("Couldn't copy").to_owned(), tx)
                    });
                }
            }
        });
    }

    pub(super) fn reveal(path: &Path) {
        if let Err(error) = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
        {
            tracing::warn!(%error, "could not show the capture in Finder");
        }
    }

    pub(crate) fn trash(path: PathBuf) {
        spawn("hex-card-trash", move || {
            #[cfg(target_os = "macos")]
            match crate::screenshots::save::trash(&path) {
                Ok(()) => crate::history::captures::forget(&path),
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "could not trash the capture");
                    crate::feedback::play(crate::feedback::Tone::Error);
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = path;
        });
    }

    fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
        if let Err(error) = std::thread::Builder::new().name(name.into()).spawn(work) {
            tracing::warn!(%error, "could not start {name}");
        }
    }
}

#[cfg(test)]
mod tests;

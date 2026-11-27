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
//!
//! A shot never fades in: its card starts as the shot itself, exactly where
//! and as large as it was taken, flat and square, and flies into the
//! corner, growing its corner and shadow as it lifts off ([`flight`]). A
//! click flies it on into the editor, which flies it back on close.

mod card;
pub(crate) mod editor;
mod flight;
#[cfg(target_os = "macos")]
pub(crate) mod spaces;

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use rune::{OverlayHandle, OverlaySpec, ScreenEdge, Spring, Tx};

use self::flight::Flight;

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
/// Out past the display's edge: short, no bounce.
const LEAVE: Spring = Spring::new(0.24, 0.0);
const REDUCED: Spring =
    super::capture_capsule::tuning::spring(crate::rune_hud::spring::REDUCED_FADE);
/// How far it leans back as it leaves, about its bottom-right corner.
const TRAVEL_SCALE: f32 = 0.94;
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
/// Whether the cards' overlay is shown (asked to show and not yet hidden),
/// for the pump that keeps it on every Space.
static SHOWN: AtomicBool = AtomicBool::new(false);
/// The cards' overlay, for the editor to hand a card back to.
static CARDS: OnceLock<OverlayHandle<Cards>> = OnceLock::new();
/// The card open in the editor and where it rests now, for the editor to
/// fly back to; none once it's gone.
#[cfg(not(test))]
static HOME: Mutex<Option<(u64, [f32; 4])>> = Mutex::new(None);
#[cfg(test)]
thread_local! {
    // Each test's cards and editor run on its own thread.
    static HOME: Mutex<Option<(u64, [f32; 4])>> = const { Mutex::new(None) };
}

/// [`HOME`], locked.
fn with_home<R>(f: impl FnOnce(&mut Option<(u64, [f32; 4])>) -> R) -> R {
    let lock = |home: &Mutex<_>| f(&mut home.lock().unwrap_or_else(|e| e.into_inner()));
    #[cfg(not(test))]
    return lock(&HOME);
    #[cfg(test)]
    return HOME.with(lock);
}

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
    /// Where it is on the display, heading for its slot: from where it was
    /// taken, or in past the display's edge.
    flight: Flight,
    /// The shot as sharp as it was taken, while the card is still larger
    /// than its own picture; dropped once it lands.
    sharp: Option<rune::Image>,
    /// A short note on it and the animation seconds it appeared.
    note: Option<(String, f64)>,
    lent: Lent,
}

/// A card open in the editor: the editor draws its shot from where the
/// card is, and the card waits, unseen, in its slot for the shot to fly
/// back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Lent {
    #[default]
    No,
    /// Asked for; the card shows until the editor draws it.
    Lending,
    /// The editor draws it.
    Away,
}

/// A shot or recording just taken, for its card.
pub(crate) struct Taken {
    /// The PNG, or a recording's movie.
    pub(crate) path: PathBuf,
    /// Its picture, already small, `width` × `height` pixels.
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
    /// The same picture larger, for the card's first frames, when it was
    /// taken larger than the small one.
    pub(crate) sharp: Option<(u32, u32, Vec<u8>)>,
    /// The display's size in points.
    pub(crate) screen: (f32, f32),
    /// Where on that display it was taken from, in points: the card starts
    /// there.
    pub(crate) capture: Option<[f32; 4]>,
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
    /// When the cards' flights last stepped.
    stepped: Option<f64>,
    /// Whether Reduce Motion is on, as the view last saw it.
    reduced: Cell<bool>,
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
            cards.push(shot, None, None);
        }
        cards.land();
        cards.hovered = hovered.and_then(|i| cards.cards.get(i)).map(|card| card.id);
        cards
    }

    /// One `shot` showing, as a still on the preview display.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn one(shot: Shot, hovered: bool) -> Self {
        Self::still(vec![shot], PREVIEW, hovered.then_some(0))
    }

    /// Adds `shot` nearest the corner, flying in from `capture`, where it
    /// was taken on the display, or else past the display's edge. The
    /// oldest cards that no longer fit on the display close.
    fn push(&mut self, shot: Shot, capture: Option<[f32; 4]>, sharp: Option<rune::Image>) -> u64 {
        self.next += 1;
        let id = self.next;
        self.cards.push(Entry {
            id,
            shot,
            flight: Flight::at([0.0; 4], 1.0),
            sharp,
            note: None,
            lent: Lent::No,
        });
        self.leaving = None;
        let room = self.screen.1 - 2.0 * INSET;
        while self.cards.len() > 1 && self.stack_height() > room {
            let oldest = self.cards[0].id;
            self.retire(oldest);
        }
        if let Some((_, slot)) = self.slots().into_iter().find(|(at, _)| *at == id)
            && let Some(card) = self.cards.iter_mut().find(|card| card.id == id)
        {
            let [x, y, w, h] = slot;
            card.flight = match capture {
                // As the shot itself, flat on the display it was part of.
                Some(rect) => Flight::at(rect, 0.0),
                None => Flight::at([x + w + INSET, y, w, h], 1.0),
            };
        }
        id
    }

    /// Every card in its slot at once, as a still shows them.
    fn land(&mut self) {
        let slots = self.slots();
        for (card, (_, slot)) in self.cards.iter_mut().zip(slots) {
            card.flight.jump(slot, 1.0);
            card.sharp = None;
        }
    }

    /// Brings the card closed last back, nearest the corner, unless its
    /// file is gone; false when there's none.
    fn restore(&mut self) -> bool {
        while let Some(card) = self.closed.pop() {
            if card.shot.path.exists() {
                self.push(card.shot, None, None);
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
            card.sharp = None;
            card.lent = Lent::No;
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
        with_home(|home| {
            if home.is_some_and(|(lent, _)| lent == id) {
                *home = None;
            }
        });
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
        let Some(card) = self.get(id).filter(|card| card.lent == Lent::No) else {
            return;
        };
        let path = card.shot.path.clone();
        let recording = card.shot.recording.is_some();
        match key {
            CardKey::Edit => self.open(id, tx),
            CardKey::Close | CardKey::Keep => self.close(id, tx),
            CardKey::Copy => files::copy(path, recording, self.overlay.clone(), id),
            CardKey::Reveal => files::reveal(&path),
            CardKey::Trash => {
                self.discard(id, tx);
                files::trash(path);
            }
        }
    }

    /// Opens card `id` in the editor, which flies its shot up from the
    /// card; the card waits, unseen, for it to fly back.
    fn open(&mut self, id: u64, tx: &mut Tx) {
        let Some(from) = self.lend_out(id) else {
            return;
        };
        let Some(card) = self.get(id) else {
            return;
        };
        let path = card.shot.path.clone();
        match &card.shot.recording {
            Some(recording) => {
                editor::open_recording_from(path, recording.words.clone(), Some(from));
            }
            None => editor::open_from(path, String::new(), Some(from)),
        }
        tx.wake_after(Duration::ZERO);
    }

    /// Card `id` lent to the editor, still showing until the editor draws
    /// it: where it is now, for the editor's shot to start from.
    fn lend_out(&mut self, id: u64) -> Option<editor::From> {
        let reduced = self.reduced.get();
        let card = self
            .cards
            .iter_mut()
            .find(|card| card.id == id && card.lent == Lent::No)?;
        card.lent = Lent::Lending;
        let from = editor::From {
            card: id,
            rect: card.flight.rect(),
            reduced,
        };
        let target = card.flight.target();
        with_home(|home| *home = Some((id, target)));
        if self.hovered == Some(id) {
            self.hover(None);
        }
        Some(from)
    }

    /// The editor draws card `id` now, or hands it back.
    fn lend(&mut self, id: u64, lent: Lent, tx: &mut Tx) {
        if let Some(card) = self.cards.iter_mut().find(|card| card.id == id) {
            card.lent = lent;
            if lent == Lent::No {
                with_home(|home| {
                    if home.is_some_and(|(at, _)| at == id) {
                        *home = None;
                    }
                });
            }
            if self.reduced.get() {
                tx.animated();
            }
            tx.wake_after(Duration::ZERO);
        }
    }

    /// Where the card open in the editor rests now, for the editor.
    fn publish_home(&self) {
        with_home(|home| {
            if let Some((id, _)) = *home {
                *home = self
                    .cards
                    .iter()
                    .zip(self.slots())
                    .find(|(card, _)| card.id == id && card.lent != Lent::No)
                    .map(|(_, slot)| slot);
            }
        });
    }
}

/// The editor draws card `id`'s shot now, or hands it back to the stack.
/// Safe from any thread.
fn lend(id: u64, lent: Lent) {
    if let Some(cards) = CARDS.get() {
        cards.update(move |s, tx| s.lend(id, lent, tx));
    }
    #[cfg(test)]
    tests::LENT.with(|lent_now| lent_now.borrow_mut().push((id, lent)));
}

/// Where card `id`, open in the editor, rests now; none once it's gone.
fn home(id: u64) -> Option<[f32; 4]> {
    with_home(|home| home.filter(|(at, _)| *at == id).map(|(_, rect)| rect))
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
    let dt = flight::tick(&mut s.stepped, now);
    let mut changed = false;
    let mut next: Option<f64> = None;
    let mut soonest = |at: f64| next = Some(next.map_or(at, |n: f64| n.min(at)));
    let reduced = s.reduced.get();
    let mut flying = false;
    let slots = s.slots();
    for (card, (_, slot)) in s.cards.iter_mut().zip(slots) {
        // A card the editor draws holds still in its slot for the shot to
        // fly back to.
        if reduced || card.lent == Lent::Away {
            if card.flight.rect() != slot || card.flight.lift() < 1.0 {
                card.flight.jump(slot, 1.0);
                changed = true;
            }
        } else if card.lent == Lent::No {
            if card.flight.step(slot, 1.0, dt) {
                soonest(now);
                flying = true;
                changed = true;
            } else if card.flight.rect() != slot {
                changed = true;
            }
        }
        if card.flight.resting() && card.sharp.take().is_some() {
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
                SHOWN.store(false, Ordering::Release);
                overlay.hide();
            }
            return true;
        }
        soonest(at + LEAVE_S);
    }
    // A flight that starts later starts from its first frame, not from
    // this one.
    if !flying {
        s.stepped = None;
    }
    s.publish_home();
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
        .poll(poll);
        let overlay = app.overlay(spec, Cards::default());
        let handle = overlay.clone();
        overlay.update(move |s, _| s.overlay = Some(handle));
        let _ = CARDS.set(overlay.clone());
        Self { overlay }
    }

    /// Shows the shot `taken` nearest the corner of its display, above any
    /// before it, flying in from where it was taken. Safe from any thread;
    /// waits for the decode.
    pub(crate) fn show(&self, taken: Taken) {
        self.present(taken, None);
    }

    /// Shows the recording `taken`, from its first frame, as [`Self::show`]
    /// does a shot. Nothing talks over it: it already has its words.
    pub(crate) fn show_recording(&self, taken: Taken, recording: Recording) {
        self.present(taken, Some(recording));
    }

    /// Brings back the card closed last, nearest the corner of the display
    /// the cards were last on. Safe from any thread.
    pub(crate) fn restore(&self) {
        let handle = self.overlay.clone();
        self.overlay.update(move |s, tx| {
            if s.restore() {
                SHOWN.store(true, Ordering::Release);
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

    fn present(&self, taken: Taken, recording: Option<Recording>) {
        let Taken {
            path,
            width,
            height,
            rgba,
            sharp,
            screen,
            capture,
        } = taken;
        let images = rune::ImageCache::new(std::sync::Arc::new(|| {}));
        let image = images.rgba(width, height, rgba);
        let sharp = sharp.map(|(w, h, rgba)| images.rgba(w, h, rgba));
        if image.wait().is_err() {
            return;
        }
        let sharp = sharp.filter(|sharp| sharp.wait().is_ok());
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
            s.push(shot, capture, sharp);
            SHOWN.store(true, Ordering::Release);
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
    use crate::desktop::i18n::tr;

    /// The file on the clipboard, and a shot's picture with it. Card `id`
    /// says what was copied once the clipboard has it, or that it wasn't.
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
            let note = match copied {
                Ok(()) => copied_note(recording),
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "could not copy the capture");
                    tr("Couldn't copy")
                }
            };
            if let Some(overlay) = overlay {
                overlay.update(move |s, tx| s.note(id, note.to_owned(), tx));
            }
        });
    }

    /// What a card says once its copy is on the clipboard: a shot's
    /// picture, or a recording's movie file.
    pub(super) fn copied_note(recording: bool) -> &'static str {
        if recording {
            tr("Movie copied")
        } else {
            tr("Copied")
        }
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
                Ok(()) => {
                    crate::history::captures::forget(&path);
                    crate::history::annotations::forget(&path);
                }
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

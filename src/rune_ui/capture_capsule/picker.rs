//! The on-screen picker: while the capsule picks a Window, an Area, or the
//! Screen, a full-screen Rune overlay on the pointer's display dims all but
//! what a click would record, tints and outlines it, and labels it, as
//! Scrawl's picker window and its Metal dim do. A click on a window records
//! it (on no window, the display), a drag records the area, and on Screen a
//! click records the display.
//!
//! It takes the pointer only while it shows and never takes focus: Escape
//! and the capture shortcut reach the capsule through Hex's tap, and the
//! shortcut records what the picker shows ([`PickerOverlay::aim`]).
//!
//! [`Scene::at`] decides what shows from the mode, the windows, the pointer
//! and the drag; the view only draws it, and Rune's springs move the target
//! between windows. The dim is one element, a border reaching past every
//! edge of the display around the target, so it moves with the target in
//! one piece.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rune::{
    Cursor, Cx, El, Live, OverlayHandle, OverlayInput, Phase, Spring, TextStyle, Transition, Tx,
    el, row, text,
};

use super::flow::{Action, Mode};
use super::surface::{ellipsize, measure};
use crate::capture_host::CaptureTarget;
use crate::rune_ui::controls::Paint;

/// The dim outside the target, as Scrawl's.
const DIM: f32 = 0.32;
/// A window's tint, and its outline.
const TINT: f32 = 0.13;
const WINDOW_RADIUS: f32 = 10.0;
const WINDOW_RING: f32 = 2.0;
const AREA_RING: f32 = 1.0;
/// The dark hairline just outside the area's white ring, so the area reads
/// over white content as well as dark (Scrawl's vanishes over white).
const AREA_EDGE: f32 = 0.5;
/// How far the dim reaches past the target: past any display's far corner.
const REACH: f32 = 8192.0;
/// The target moving to the next window, growing, or closing.
const MOVE: Spring = Spring::new(0.24, 0.0);
const FADE: Spring = Spring::new(0.20, 0.0);
const BADGE_H: f32 = 22.0;
const BADGE_PX: f32 = 8.0;
const BADGE_GAP: f32 = 8.0;
const BADGE_MARGIN: f32 = 8.0;
const BADGE_MAX: f32 = 480.0;
const BADGE_TEXT: TextStyle = TextStyle::new(12.0, 530.0).features("'tnum' 1");
/// The smallest area the picker records, points a side.
pub(crate) const MIN_AREA_SIDE: f32 = 36.0;
/// The camera that stands in for the pointer over windows.
const CAMERA: f32 = 40.0;
pub(crate) const FULL_SCREEN: &str = "Full screen · Click to record";
/// How long the picker's exit gets before its overlay hides.
const HIDE_AFTER_S: f64 = 0.35;
/// A pick that started no capture this long after shows the picker again.
const RETRY_AFTER_S: f64 = 2.0;
/// How often the open picker looks for the pointer on another display.
const DISPLAY_EVERY: Duration = Duration::from_millis(100);

// ─── Geometry ────────────────────────────────────────────────────────────────

/// A window the picker can record, in global top-left points.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Window {
    pub(crate) id: u32,
    pub(crate) frame: [f32; 4],
    pub(crate) label: String,
}

impl Window {
    fn from_target(target: &scrawl_capture::model::TargetInfo) -> Option<Self> {
        let f = target.frame;
        let frame = [f.x, f.y, f.width, f.height].map(|v| v as f32);
        Some(Self {
            id: target.window_id?,
            label: window_label(target.app.as_deref(), target.window_title.as_deref(), frame),
            frame,
        })
    }
}

/// "App — Title", the app alone when the title adds nothing, else the
/// window's size.
pub(crate) fn window_label(app: Option<&str>, title: Option<&str>, frame: [f32; 4]) -> String {
    match (app, title) {
        (Some(app), Some(title)) if !title.is_empty() && title != app => {
            format!("{app} — {title}")
        }
        (Some(app), _) => app.to_owned(),
        _ => size_label(frame[2], frame[3]),
    }
}

fn size_label(w: f32, h: f32) -> String {
    format!("{} × {}", w.round() as i32, h.round() as i32)
}

/// The front-most window holding `p`; `windows` run front to back.
pub(crate) fn window_at(windows: &[Window], [x, y]: [f32; 2]) -> Option<&Window> {
    windows.iter().find(|w| {
        let [wx, wy, ww, wh] = w.frame;
        x >= wx && x <= wx + ww && y >= wy && y <= wy + wh
    })
}

fn clamp_point([x, y]: [f32; 2], [w, h]: [f32; 2]) -> [f32; 2] {
    [x.clamp(0.0, w), y.clamp(0.0, h)]
}

/// The rect between a drag's ends, kept on the display.
fn drag_rect(a: [f32; 2], b: [f32; 2], size: [f32; 2]) -> [f32; 4] {
    let [a, b] = [a, b].map(|p| clamp_point(p, size));
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        (a[0] - b[0]).abs(),
        (a[1] - b[1]).abs(),
    ]
}

/// A dragged area in the display's points, if it's big enough to record.
pub(crate) fn area_rect(a: [f32; 2], b: [f32; 2], size: [f32; 2]) -> Option<[f32; 4]> {
    let rect = drag_rect(a, b, size);
    (rect[2] >= MIN_AREA_SIDE && rect[3] >= MIN_AREA_SIDE).then_some(rect)
}

fn clip_rect([x, y, w, h]: [f32; 4], [sw, sh]: [f32; 2]) -> [f32; 4] {
    let x0 = x.clamp(0.0, sw);
    let y0 = y.clamp(0.0, sh);
    [
        x0,
        y0,
        (x + w).clamp(0.0, sw) - x0,
        (y + h).clamp(0.0, sh) - y0,
    ]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Anchor {
    /// Just outside the bottom-right corner (an area), flipping above or
    /// inside near the display's edges.
    BelowTrailing,
    /// Centred on the target.
    Center,
}

/// Where a badge of `size` sits by `rect`, kept on the display.
fn place_badge(
    [x, y, w, h]: [f32; 4],
    [bw, bh]: [f32; 2],
    [sw, sh]: [f32; 2],
    anchor: Anchor,
) -> [f32; 2] {
    let (bx, by) = match anchor {
        Anchor::BelowTrailing => {
            let below = y + h + BADGE_GAP;
            let above = y - BADGE_GAP - bh;
            let by = if below + bh <= sh - BADGE_MARGIN {
                below
            } else if above >= BADGE_MARGIN {
                above
            } else {
                y + h - BADGE_GAP - bh
            };
            (x + w - bw, by)
        }
        Anchor::Center => (x + (w - bw) / 2.0, y + (h - bh) / 2.0),
    };
    let max_x = (sw - BADGE_MARGIN - bw).max(BADGE_MARGIN);
    let max_y = (sh - BADGE_MARGIN - bh).max(BADGE_MARGIN);
    [bx.clamp(BADGE_MARGIN, max_x), by.clamp(BADGE_MARGIN, max_y)]
}

// ─── Scene ───────────────────────────────────────────────────────────────────

/// What the target is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Nothing yet: the dim covers the display, closed onto a point.
    None,
    Window,
    Area,
    Screen,
}

/// The badge: its rect and text, and the key its text crossfades on.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Badge {
    pub(crate) rect: [f32; 4],
    pub(crate) text: String,
    key: String,
}

impl Badge {
    /// Nothing to say yet: mounted unseen at `at`, so the first badge fades
    /// in rather than appearing.
    fn empty([x, y]: [f32; 2]) -> Self {
        Self {
            rect: [x, y, 2.0 * BADGE_PX, BADGE_H],
            text: String::new(),
            key: String::new(),
        }
    }
}

/// What the picker draws, in the display's points.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Scene {
    /// False while it fades out (and before it fades in).
    pub(crate) on: bool,
    pub(crate) size: [f32; 2],
    pub(crate) kind: Kind,
    pub(crate) target: [f32; 4],
    pub(crate) badge: Option<Badge>,
    /// Over windows, a camera stands in for the pointer.
    pub(crate) camera: bool,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            on: false,
            size: [1440.0, 900.0],
            kind: Kind::None,
            target: [720.0, 450.0, 0.0, 0.0],
            badge: None,
            camera: false,
        }
    }
}

/// Where the picker is: what it picks, on which display, over what.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Aim {
    pub(crate) mode: Mode,
    /// The display, in global points.
    pub(crate) display: [f32; 4],
    /// Front to back, in global points; `None` while they load.
    pub(crate) windows: Option<Vec<Window>>,
    /// The pointer and a drag's start, in the display's points.
    pub(crate) pointer: Option<[f32; 2]>,
    pub(crate) drag: Option<[f32; 2]>,
}

impl Aim {
    fn size(&self) -> [f32; 2] {
        [self.display[2], self.display[3]]
    }

    fn global(&self, [x, y]: [f32; 2]) -> [f64; 2] {
        [
            f64::from(self.display[0] + x),
            f64::from(self.display[1] + y),
        ]
    }

    /// The window under the pointer, in global points.
    fn hovered(&self) -> Option<&Window> {
        let [x, y] = self.pointer?;
        window_at(
            self.windows.as_deref()?,
            [self.display[0] + x, self.display[1] + y],
        )
    }

    /// What a click (or the capture shortcut) records now: the window under
    /// the pointer (on none, its display), a dragged area big enough to
    /// record, or the display. `None` while windows load, or for an area
    /// with no drag.
    pub(crate) fn target(&self) -> Option<CaptureTarget> {
        let at = self.global(self.pointer.unwrap_or([0.0, 0.0]));
        match self.mode {
            Mode::Window => {
                self.windows.as_ref()?;
                Some(match self.hovered() {
                    Some(window) => CaptureTarget::Window { id: window.id },
                    None => CaptureTarget::Display { at },
                })
            }
            Mode::Area => {
                let rect = area_rect(self.drag?, self.pointer?, self.size())?;
                let [x, y] = self.global([rect[0], rect[1]]);
                Some(CaptureTarget::Area {
                    rect: [x, y, f64::from(rect[2]), f64::from(rect[3])],
                })
            }
            Mode::Screen | Mode::Iphone => Some(CaptureTarget::Display { at }),
        }
    }

    /// What the capture shortcut records: what a click would, else (an
    /// area not yet dragged) the display, as Scrawl's shortcut does.
    pub(crate) fn shortcut(&self) -> Option<CaptureTarget> {
        self.target().or_else(|| {
            (self.mode == Mode::Area).then(|| CaptureTarget::Display {
                at: self.global(self.pointer.unwrap_or([0.0, 0.0])),
            })
        })
    }
}

impl Scene {
    /// What shows for `aim`; an empty target closes onto `rest`.
    pub(crate) fn at(aim: &Aim, rest: [f32; 2]) -> Self {
        let size = aim.size();
        let empty = [rest[0], rest[1], 0.0, 0.0];
        let screen = || (Kind::Screen, [0.0, 0.0, size[0], size[1]]);
        let (kind, target) = match aim.mode {
            Mode::Window if aim.windows.is_none() => (Kind::None, empty),
            Mode::Window => aim.hovered().map_or_else(screen, |window| {
                let [x, y, w, h] = window.frame;
                (
                    Kind::Window,
                    clip_rect([x - aim.display[0], y - aim.display[1], w, h], size),
                )
            }),
            Mode::Area => match (aim.drag, aim.pointer) {
                (Some(a), Some(b)) => (Kind::Area, drag_rect(a, b, size)),
                _ => (Kind::None, empty),
            },
            Mode::Screen | Mode::Iphone => screen(),
        };
        let label = match kind {
            Kind::None => None,
            Kind::Window => aim.hovered().map(|w| w.label.clone()),
            Kind::Area => Some(size_label(target[2], target[3])),
            Kind::Screen => Some(FULL_SCREEN.to_owned()),
        };
        let badge = label.map(|label| {
            let max = (size[0] - 2.0 * (BADGE_MARGIN + BADGE_PX) - 2.0).min(BADGE_MAX);
            let text = ellipsize(&label, BADGE_TEXT, max);
            let w = measure(&text, BADGE_TEXT).ceil() + 2.0 * BADGE_PX;
            let anchor = if kind == Kind::Area {
                Anchor::BelowTrailing
            } else {
                Anchor::Center
            };
            let [x, y] = place_badge(target, [w, BADGE_H], size, anchor);
            Badge {
                rect: [x, y, w, BADGE_H],
                // An area's size counts in place; a new window's name
                // crossfades.
                key: if kind == Kind::Area {
                    "size".into()
                } else {
                    text.clone()
                },
                text,
            }
        });
        Self {
            on: true,
            size,
            kind,
            target,
            badge,
            camera: aim.mode == Mode::Window,
        }
    }
}

// ─── The overlay ─────────────────────────────────────────────────────────────

/// The picker overlay's state.
pub(crate) struct PickerState {
    /// What the capsule picks; `None` closes the picker.
    want: Option<Mode>,
    aim: Option<Aim>,
    scene: Scene,
    /// Where an empty target closes onto, and grows from.
    rest: [f32; 2],
    /// The camera's top-left, following the pointer without a re-render.
    camera: Live<[f32; 2]>,
    /// The badge last shown, kept while it fades, and how much it shows.
    /// Its fade is presentation-only, so it still fades in while a drag's
    /// instant commits move it.
    badge: Badge,
    badge_alpha: Live<f32>,
    /// Committed the hidden pose on the new display; fades in next poll.
    opening: bool,
    /// Animation seconds a pick hid the picker at.
    picked: Option<f64>,
    /// Polls until a pick's fade starts: never in the commit of the drag
    /// that made it, whose instant moves would land the fade at once.
    fading_in: u8,
    /// Animation seconds the scene went off at, until the overlay hides.
    off_since: Option<f64>,
    /// When it last looked for the pointer on another display.
    looked: f64,
    now: f64,
    /// Window lists asked for; a reply to an older one is dropped.
    loads: u64,
    /// What the capture shortcut records, for the capsule.
    shortcut: Arc<Mutex<Option<CaptureTarget>>>,
    act: Option<super::Act>,
    overlay: Option<OverlayHandle<PickerState>>,
    shown: bool,
    /// Draws a plain desktop with its windows under the picker (stills and
    /// clips).
    backdrop: bool,
}

impl PickerState {
    fn new() -> Self {
        Self {
            want: None,
            aim: None,
            scene: Scene::default(),
            rest: [720.0, 450.0],
            camera: Live::new([0.0, 0.0]),
            badge: Badge::empty([720.0, 450.0]),
            badge_alpha: Live::new(0.0),
            opening: false,
            picked: None,
            fading_in: 0,
            off_since: Some(f64::NEG_INFINITY),
            looked: f64::NEG_INFINITY,
            now: 0.0,
            loads: 0,
            shortcut: Arc::default(),
            act: None,
            overlay: None,
            shown: false,
            backdrop: false,
        }
    }

    /// `aim` at rest, shown or (`on` false) closed, as a still or the start
    /// of a clip.
    #[cfg(test)]
    pub(crate) fn still(aim: Aim, on: bool) -> Self {
        let mut state = Self::new();
        state.rest = aim.pointer.unwrap_or(state.rest);
        state.badge = Badge::empty(state.rest);
        state.want = on.then_some(aim.mode);
        state.aim = Some(aim);
        state.backdrop = true;
        state.restage();
        state.scene.on = on;
        state.off_since = (!on).then_some(f64::NEG_INFINITY);
        if let Some([x, y]) = state.aim.as_ref().and_then(|aim| aim.pointer) {
            state.camera.set([x - CAMERA * 0.5, y - CAMERA * 0.5]);
        }
        state
    }

    #[cfg(test)]
    pub(crate) fn set_act(&mut self, act: super::Act) {
        self.act = Some(act);
    }

    #[cfg(test)]
    pub(crate) fn want(&mut self, mode: Option<Mode>) {
        self.want = mode;
    }

    /// Whether the scene shows, rather than fading out or gone.
    #[cfg(test)]
    pub(crate) fn showing(&self) -> bool {
        self.scene.on
    }

    #[cfg(test)]
    pub(crate) fn aim(&self) -> Option<&Aim> {
        self.aim.as_ref()
    }

    /// Moves the scene to the aim. True when it changed.
    fn restage(&mut self) -> bool {
        let Some(aim) = &self.aim else {
            return false;
        };
        *self.shortcut.lock().unwrap_or_else(|e| e.into_inner()) = aim.shortcut();
        let mut scene = Scene::at(aim, self.rest);
        if scene.kind == Kind::None && self.scene.kind != Kind::None {
            // The target closes onto the pointer, and grows from it next.
            self.rest = aim.pointer.unwrap_or(self.rest);
            scene = Scene::at(aim, self.rest);
        }
        scene.on = self.scene.on;
        match &scene.badge {
            Some(badge) => {
                self.badge = badge.clone();
                self.badge_alpha.set(1.0);
            }
            None => {
                self.badge_alpha.set(0.0);
            }
        }
        if scene == self.scene {
            return false;
        }
        self.scene = scene;
        true
    }

    /// The pointer moved over the picker.
    pub(crate) fn moved(&mut self, at: [f32; 2], tx: &mut Tx) {
        self.camera
            .set([at[0] - CAMERA * 0.5, at[1] - CAMERA * 0.5]);
        let Some(aim) = self.aim.as_mut().filter(|_| self.picked.is_none()) else {
            tx.live_only();
            return;
        };
        aim.pointer = Some(clamp_point(at, aim.size()));
        if self.restage() {
            // A new window springs to it; a drag follows the pointer.
            if self.aim.as_ref().is_some_and(|aim| aim.drag.is_some()) {
                tx.instant();
            } else {
                tx.animated();
            }
        } else {
            tx.live_only();
        }
    }

    /// A press, drag, or release on the picker.
    pub(crate) fn dragged(&mut self, phase: Phase, at: [f32; 2], cancelled: bool, tx: &mut Tx) {
        if self.picked.is_some() || !self.scene.on {
            return;
        }
        let Some(aim) = &mut self.aim else {
            return;
        };
        let at = clamp_point(at, aim.size());
        aim.pointer = Some(at);
        match (aim.mode, phase) {
            (Mode::Area, Phase::Down) => {
                aim.drag = Some(at);
                self.rest = at;
                // The target lands at the press, closed, and opens with
                // the drag.
                tx.instant();
                self.restage();
            }
            (Mode::Area, Phase::Move) => {
                tx.instant();
                self.restage();
            }
            (Mode::Area, Phase::Up) => {
                let target = (!cancelled).then(|| aim.target()).flatten();
                match target {
                    Some(target) => self.pick(target, tx),
                    None => {
                        // Too small to record: it closes onto the release.
                        aim.drag = None;
                        tx.animated();
                        self.restage();
                    }
                }
            }
            (_, Phase::Down) => {
                if let Some(target) = aim.target() {
                    self.pick(target, tx);
                }
            }
            _ => {}
        }
    }

    /// Records `target`: the picker fades and lets the pointer through
    /// while the capture starts.
    fn pick(&mut self, target: CaptureTarget, tx: &mut Tx) {
        if let Some(act) = &self.act {
            act(Action::Start(target));
        }
        self.picked();
        tx.wake_after(Duration::ZERO);
    }

    fn picked(&mut self) {
        *self.shortcut.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.picked = Some(self.now);
        self.fading_in = 2;
        if let Some(overlay) = &self.overlay {
            overlay.set_input(OverlayInput::None);
        }
    }

    /// Opens on the display under the pointer, or moves there: the hidden
    /// pose lands on it now and fades in next poll. True when it moved, so
    /// the poll commits that pose.
    fn place(&mut self, mode: Mode, tx: &mut Tx) -> bool {
        pointer_display()
            .is_some_and(|(display, pointer)| self.place_on(mode, display, pointer, tx))
    }

    /// [`Self::place`] on `display`, a global frame, with the pointer at
    /// the global `pointer`.
    pub(super) fn place_on(
        &mut self,
        mode: Mode,
        display: [f32; 4],
        pointer: [f32; 2],
        tx: &mut Tx,
    ) -> bool {
        let fresh = self.aim.as_ref().is_none_or(|aim| aim.display != display);
        if !fresh {
            return false;
        }
        let windows = self.aim.take().and_then(|aim| aim.windows);
        let pointer = [pointer[0] - display[0], pointer[1] - display[1]];
        self.rest = pointer;
        self.badge = Badge::empty(pointer);
        self.camera
            .set([pointer[0] - CAMERA * 0.5, pointer[1] - CAMERA * 0.5]);
        self.aim = Some(Aim {
            mode,
            display,
            windows,
            pointer: Some(pointer),
            drag: None,
        });
        if let Some(overlay) = &self.overlay {
            overlay.set_placement(Some(placement(display)));
            overlay.resize((display[2], display[3]));
        }
        // On another display it starts over: hidden there now, fading in
        // next poll.
        self.scene.on = false;
        self.restage();
        self.opening = true;
        self.off_since = None;
        tx.instant();
        tx.wake_after(Duration::ZERO);
        true
    }

    /// Asks for the windows, off the main thread.
    fn load_windows(&mut self) {
        self.loads += 1;
        let load = self.loads;
        let Some(overlay) = self.overlay.clone() else {
            return;
        };
        let spawned = std::thread::Builder::new()
            .name("capture-picker-windows".into())
            .spawn(move || {
                let windows: Vec<Window> = scrawl_capture::capture::list_selectable_windows()
                    .iter()
                    .filter_map(Window::from_target)
                    .collect();
                overlay.update(move |s, tx| {
                    if s.loads != load {
                        return;
                    }
                    if let Some(aim) = &mut s.aim {
                        aim.windows = Some(windows);
                        if s.restage() {
                            tx.animated();
                        }
                    }
                });
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not list windows for the capture picker");
        }
    }

    /// Follows the capsule: opens, changes mode, or closes.
    fn follow(&mut self, tx: &mut Tx) -> bool {
        let want = self.want;
        // A pick keeps its mode while the capture starts, so the picker
        // doesn't open again under it.
        let have = self
            .aim
            .as_ref()
            .map(|aim| aim.mode)
            .filter(|_| self.off_since.is_none() || self.opening || self.picked.is_some());
        if want == have {
            return false;
        }
        match want {
            Some(Mode::Iphone) | None => {
                *self.shortcut.lock().unwrap_or_else(|e| e.into_inner()) = None;
                self.scene.on = false;
                self.fading_in = 0;
                self.opening = false;
                self.picked = None;
                if let Some(aim) = &mut self.aim {
                    aim.drag = None;
                }
                self.aim = self.aim.take().filter(|_| self.overlay.is_none());
                true
            }
            Some(mode) => {
                self.picked = None;
                self.fading_in = 0;
                if let Some(overlay) = &self.overlay {
                    overlay.set_input(OverlayInput::All);
                }
                let was = self.aim.as_ref().map(|aim| aim.mode);
                if mode == Mode::Window && was != Some(Mode::Window) {
                    if let Some(aim) = &mut self.aim {
                        aim.windows = None;
                    }
                    self.load_windows();
                }
                if have.is_none() {
                    if self.overlay.is_some() {
                        self.aim = None;
                        self.place(mode, tx);
                    } else if let Some(aim) = &mut self.aim {
                        // Headless: the display stays.
                        aim.mode = mode;
                        aim.drag = None;
                        self.scene.on = false;
                        self.restage();
                        self.opening = true;
                        self.off_since = None;
                        tx.instant();
                        tx.wake_after(Duration::ZERO);
                    }
                    return true;
                }
                if let Some(aim) = &mut self.aim {
                    aim.mode = mode;
                    aim.drag = None;
                }
                self.restage();
                tx.animated();
                true
            }
        }
    }
}

/// The global frame of the display under the pointer, and the pointer.
fn pointer_display() -> Option<([f32; 4], [f32; 2])> {
    use scrawl_capture::screens;
    let pointer = screens::pointer()?;
    let displays = screens::displays();
    let display = &displays[screens::index_at(&displays, pointer)?];
    Some((display.frame.map(|v| v as f32), pointer.map(|v| v as f32)))
}

pub(crate) fn poll(s: &mut PickerState, now: f64, tx: &mut Tx) -> bool {
    s.now = now;
    let mut changed = s.follow(tx);
    if s.opening && !changed {
        // The hidden pose is on screen: fade in from it.
        s.opening = false;
        s.scene.on = true;
        changed = true;
    }
    if s.fading_in > 0 {
        s.fading_in -= 1;
        if s.fading_in == 0 {
            s.scene.on = false;
            changed = true;
        } else {
            tx.wake_after(Duration::ZERO);
        }
    }
    if let Some(at) = s.picked
        && s.aim.is_some()
        && now - at >= RETRY_AFTER_S
    {
        s.picked = None;
        s.scene.on = true;
        if let Some(overlay) = &s.overlay {
            overlay.set_input(OverlayInput::All);
        }
        changed = true;
    } else if let Some(at) = s.picked {
        tx.wake_after(Duration::from_secs_f64((at + RETRY_AFTER_S - now).max(0.0)));
    }
    // It follows the pointer to another display, between drags.
    if s.overlay.is_some()
        && s.scene.on
        && let Some(aim) = &s.aim
        && aim.drag.is_none()
    {
        let mode = aim.mode;
        if now - s.looked >= DISPLAY_EVERY.as_secs_f64() {
            s.looked = now;
            changed |= s.place(mode, tx);
        }
        tx.wake_after(DISPLAY_EVERY);
    }

    let off = !s.scene.on && !s.opening;
    let since = match (off, s.off_since) {
        (true, None) => *s.off_since.insert(now),
        (true, Some(since)) => since,
        (false, _) => {
            s.off_since = None;
            now
        }
    };
    let leaving = since + HIDE_AFTER_S - now;
    if off && leaving > 0.0 {
        tx.wake_after(Duration::from_secs_f64(leaving));
    }
    let shown = !off || leaving > 0.0;
    if s.shown != shown {
        s.shown = shown;
        if let Some(overlay) = &s.overlay {
            if shown {
                overlay.show();
            } else {
                overlay.hide();
                if s.want.is_none() || s.want == Some(Mode::Iphone) {
                    s.aim = None;
                }
            }
        }
    }
    changed
}

/// Where the picker covers `display`, a global frame (top-left origin, as
/// Scrawl's screens give it). Rune reads a rect from its display's
/// top-left, so the overlay targets the primary display (whose top-left is
/// the global origin), never the one under the pointer, which would add
/// that display's origin a second time.
pub(super) const DISPLAY: rune::DisplayTarget = rune::DisplayTarget::Primary;

pub(super) fn placement(display: [f32; 4]) -> rune::OverlayPlacement {
    rune::OverlayPlacement::Rect {
        x: display[0],
        y: display[1],
        width: display[2],
        height: display[3],
    }
}

/// The picker overlay, as the app and the capsule hold it.
#[derive(Clone)]
pub(crate) struct PickerOverlay {
    overlay: OverlayHandle<PickerState>,
    shortcut: Arc<Mutex<Option<CaptureTarget>>>,
}

impl PickerOverlay {
    /// Adds the overlay, prewarmed and hidden. `act` starts what it picks;
    /// it runs on the main thread, so it must only hand the action on.
    pub(crate) fn install<S: 'static>(
        app: &mut rune::App<S>,
        act: impl Fn(Action) + Send + Sync + 'static,
    ) -> Self {
        use rune::{OverlayLevel, OverlayOptions, OverlayPlacement, OverlaySpec};
        let spec = OverlaySpec::new("Hex capture picker", (1440.0, 900.0), view)
            .options(OverlayOptions {
                placement: Some(OverlayPlacement::FullScreen),
                display: DISPLAY,
                // Under the capsule, so its chips still take clicks.
                level: OverlayLevel::AboveStatus,
                exclude_from_capture: true,
                input: OverlayInput::All,
                prewarm: true,
                ..Default::default()
            })
            .poll(poll)
            .on_created(|c| {
                if let Some(window) = c.window_number {
                    crate::capture_host::exclude_window(
                        crate::capture_host::OwnWindow::Picker,
                        window,
                    );
                }
            });
        let mut state = PickerState::new();
        state.act = Some(Arc::new(act));
        let shortcut = state.shortcut.clone();
        let overlay = app.overlay(spec, state);
        let handle = overlay.clone();
        overlay.update(move |s, _| s.overlay = Some(handle));
        Self { overlay, shortcut }
    }

    /// Shows the picker for `mode`, or closes it; safe from any thread.
    pub(crate) fn set(&self, mode: Option<Mode>) {
        self.overlay.update(move |s, tx| {
            s.want = mode;
            tx.wake_after(Duration::ZERO);
        });
    }

    /// The capsule's Record, or the capture shortcut while it picks,
    /// records what the picker shows, as a click would.
    pub(crate) fn resolve(&self, action: Action) -> Action {
        match action {
            Action::Record(Mode::Window | Mode::Area | Mode::Screen) => {
                self.take_shortcut().map_or(action, Action::Start)
            }
            action => action,
        }
    }

    /// What the capture shortcut records now, and the picker fades as a
    /// click's pick would. `None` while windows load.
    fn take_shortcut(&self) -> Option<CaptureTarget> {
        let target = *self.shortcut.lock().unwrap_or_else(|e| e.into_inner());
        if target.is_some() {
            self.overlay.update(|s, tx| {
                s.picked();
                tx.wake_after(Duration::ZERO);
            });
        }
        target
    }
}

// ─── View ────────────────────────────────────────────────────────────────────

pub(crate) fn view(s: &PickerState, cx: &mut Cx<PickerState>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let k = p.k;
    let scene = &s.scene;
    let [w, h] = scene.size;
    let cursor = match s.aim.as_ref().map(|aim| aim.mode) {
        Some(Mode::Area) => Cursor::Crosshair,
        Some(Mode::Window) => Cursor::Hidden,
        _ => Cursor::Pointer,
    };
    let mut root = el()
        .key("picker")
        .size(w, h)
        .cursor(cursor)
        .on_pointer_move_local(|s: &mut PickerState, e, tx| s.moved([e.x, e.y], tx))
        .on_drag(|s: &mut PickerState, d, tx| {
            s.dragged(d.phase, [d.x, d.y], d.cancelled(), tx);
        });
    if s.backdrop {
        root = root.child(desktop(p, s));
    }
    let [x, y, tw, th] = scene.target;
    let (radius, ring, ring_color, edge_color, tint) = match scene.kind {
        Kind::Window => (
            WINDOW_RADIUS,
            WINDOW_RING,
            k.alpha(k.c.border_focus, 0.93),
            k.alpha(k.c.black, 0.0),
            k.alpha(k.c.border_focus, TINT),
        ),
        Kind::Area | Kind::Screen => (
            0.0,
            AREA_RING,
            k.alpha(k.c.white, 0.92),
            k.alpha(k.c.black, AREA_EDGE),
            k.alpha(k.c.white, 0.0),
        ),
        Kind::None => (
            0.0,
            AREA_RING,
            k.alpha(k.c.white, 0.0),
            k.alpha(k.c.black, 0.0),
            k.alpha(k.c.white, 0.0),
        ),
    };
    let mut layer = el()
        .key("layer")
        .trace_name("picker")
        .absolute(0.0, 0.0)
        .size(w, h)
        .opacity(if scene.on { 1.0 } else { 0.0 })
        .opacity_motion(FADE)
        .motion(MOVE)
        .child(
            el().key("dim")
                .trace_name("dim")
                .absolute(x - REACH, y - REACH)
                .size(tw + 2.0 * REACH, th + 2.0 * REACH)
                .radius(radius + REACH)
                .border(REACH, k.alpha(k.c.black, DIM))
                .child(
                    el().key("target")
                        .trace_name("target")
                        .absolute(REACH, REACH)
                        .size(tw, th)
                        .radius(radius)
                        .fill(tint)
                        .child(
                            el().trace_name("edge")
                                .absolute(-AREA_RING, -AREA_RING)
                                .size(tw + 2.0 * AREA_RING, th + 2.0 * AREA_RING)
                                .radius(radius + AREA_RING)
                                .border(AREA_RING, edge_color)
                                .child(
                                    el().trace_name("ring")
                                        .absolute(AREA_RING, AREA_RING)
                                        .size(tw, th)
                                        .radius(radius)
                                        .border(ring, ring_color),
                                ),
                        ),
                ),
        );
    {
        let badge = &s.badge;
        let [bx, by, bw, bh] = badge.rect;
        // As the capsule's text swaps: the old title softens out quickly.
        let reduced = cx.is_reduced_motion();
        let swap = Transition::blur(if reduced { 0.0 } else { super::PRESENCE_BLUR })
            .combined(Transition::scale(if reduced { 1.0 } else { 0.96 }))
            .springs(rune::SpringPair::new(
                FADE,
                super::tuning::get().presence_exit,
            ));
        let mut pill = row()
            .key("badge")
            .trace_name("badge")
            .absolute(bx, by)
            .size(bw, bh)
            .radius(bh * 0.5)
            .fill(k.c.bg_base)
            .border(p.hair, k.c.border_base)
            .live_opacity(s.badge_alpha.smooth(FADE));
        for shadow in &k.elevation.floating {
            pill = pill.box_shadow(*shadow);
        }
        // The outgoing text keeps its own width as it fades, so the pill
        // clips it rather than letting a longer title spill past its edge.
        layer = layer.child(
            pill.child(
                el().key("badge-clip")
                    .absolute(0.0, 0.0)
                    .size(bw, bh)
                    .clip_rounded(bh * 0.5)
                    .child(
                        el().key(format!("text:{}", badge.key))
                            .absolute(BADGE_PX, 0.0)
                            .size(bw - 2.0 * BADGE_PX, bh)
                            .child(
                                row()
                                    .center()
                                    .size(bw - 2.0 * BADGE_PX, bh)
                                    .child(text(&badge.text).font(BADGE_TEXT).color(p.text())),
                            )
                            .transition(swap),
                    ),
            ),
        );
    }
    root = root.child(layer);
    // Beside the layer rather than in it, as the pointer it stands in for.
    if scene.camera {
        root = root.child(
            camera(p)
                .live_offset(&s.camera)
                .opacity(if scene.on { 1.0 } else { 0.0 })
                .opacity_motion(FADE),
        );
    }
    root
}

/// Scrawl's camera cursor: a dark disc with a camera on it.
fn camera(p: Paint) -> El {
    let k = p.k;
    let mut disc = el()
        .key("camera")
        .trace_name("camera")
        .absolute(0.0, 0.0)
        .size(CAMERA, CAMERA)
        .radius(CAMERA * 0.5)
        .fill(k.c.bg_base)
        .border(p.hair, k.c.border_base);
    for shadow in &k.elevation.floating {
        disc = disc.box_shadow(*shadow);
    }
    let ink = p.text();
    disc.child(
        el().absolute(15.5, 10.5)
            .size(9.0, 2.5)
            .corners([1.5, 1.5, 0.0, 0.0])
            .fill(ink),
    )
    .child(
        el().trace_name("camera-body")
            .absolute(9.5, 13.0)
            .size(21.0, 14.5)
            .radius(3.5)
            .fill(ink)
            .child(
                el().absolute(6.0, 2.75)
                    .size(9.0, 9.0)
                    .radius(4.5)
                    .fill(k.c.bg_base)
                    .child(
                        el().absolute(2.25, 2.25)
                            .size(4.5, 4.5)
                            .radius(2.25)
                            .fill(ink),
                    ),
            ),
    )
}

/// A plain desktop with the aim's windows on it, back to front.
fn desktop(p: Paint, s: &PickerState) -> El {
    let k = p.k;
    let [w, h] = s.scene.size;
    let mut desk = el().size(w, h).fill(k.c.layer_03);
    let Some(aim) = &s.aim else {
        return desk;
    };
    for window in aim.windows.iter().flatten().rev() {
        let [x, y, ww, wh] = window.frame;
        let mut pane = el()
            .absolute(x - aim.display[0], y - aim.display[1])
            .size(ww, wh)
            .radius(WINDOW_RADIUS)
            .fill(k.c.bg_base)
            .border(p.hair, k.c.border_base);
        for shadow in &k.elevation.floating {
            pane = pane.box_shadow(*shadow);
        }
        desk = desk.child(
            pane.child(
                el().absolute(0.0, 0.0)
                    .size(ww, 28.0)
                    .corners([WINDOW_RADIUS, WINDOW_RADIUS, 0.0, 0.0])
                    .fill(k.c.layer_01),
            )
            .child(
                el().absolute(16.0, 44.0)
                    .size((ww - 32.0).max(0.0) * 0.6, 10.0)
                    .radius(5.0)
                    .fill(k.c.layer_02),
            )
            .child(
                el().absolute(16.0, 64.0)
                    .size((ww - 32.0).max(0.0) * 0.4, 10.0)
                    .radius(5.0)
                    .fill(k.c.layer_02),
            ),
        );
    }
    desk
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn windows() -> Vec<Window> {
        vec![
            Window {
                id: 1,
                frame: [120.0, 90.0, 520.0, 340.0],
                label: "Brave Browser — problems.kitlangton.com".into(),
            },
            Window {
                id: 2,
                frame: [420.0, 240.0, 460.0, 300.0],
                label: "Zed — hex".into(),
            },
        ]
    }

    pub(crate) fn aim(mode: Mode, pointer: [f32; 2]) -> Aim {
        Aim {
            mode,
            display: [0.0, 0.0, 960.0, 600.0],
            windows: Some(windows()),
            pointer: Some(pointer),
            drag: None,
        }
    }

    #[test]
    fn a_window_is_labelled_by_its_app_and_title() {
        let f = [0.0, 0.0, 800.4, 600.0];
        assert_eq!(window_label(Some("Zed"), Some("hex"), f), "Zed — hex");
        assert_eq!(window_label(Some("Finder"), Some("Finder"), f), "Finder");
        assert_eq!(window_label(Some("Finder"), Some(""), f), "Finder");
        assert_eq!(window_label(None, Some("x"), f), "800 × 600");
    }

    #[test]
    fn the_front_most_window_under_the_pointer_is_the_target() {
        let windows = windows();
        assert_eq!(window_at(&windows, [500.0, 300.0]).map(|w| w.id), Some(1));
        assert_eq!(window_at(&windows, [800.0, 500.0]).map(|w| w.id), Some(2));
        assert!(window_at(&windows, [20.0, 20.0]).is_none());
    }

    #[test]
    fn an_area_is_kept_on_the_display_and_needs_a_minimum_side() {
        let size = [960.0, 600.0];
        assert_eq!(
            area_rect([100.0, 100.0], [40.0, 400.0], size),
            Some([40.0, 100.0, 60.0, 300.0])
        );
        assert_eq!(
            area_rect([900.0, 500.0], [1200.0, 700.0], size),
            Some([900.0, 500.0, 60.0, 100.0])
        );
        assert_eq!(area_rect([100.0, 100.0], [130.0, 400.0], size), None);
    }

    #[test]
    fn the_badge_stays_on_the_display() {
        let size = [960.0, 600.0];
        // Below the area's trailing corner, else above it, else inside.
        assert_eq!(
            place_badge(
                [100.0, 100.0, 200.0, 100.0],
                [80.0, 22.0],
                size,
                Anchor::BelowTrailing
            ),
            [220.0, 208.0]
        );
        assert_eq!(
            place_badge(
                [100.0, 480.0, 200.0, 100.0],
                [80.0, 22.0],
                size,
                Anchor::BelowTrailing
            ),
            [220.0, 450.0]
        );
        assert_eq!(
            place_badge(
                [0.0, 0.0, 960.0, 600.0],
                [80.0, 22.0],
                size,
                Anchor::BelowTrailing
            ),
            [872.0, 570.0]
        );
        assert_eq!(
            place_badge([0.0, 0.0, 40.0, 40.0], [80.0, 22.0], size, Anchor::Center),
            [8.0, 9.0]
        );
    }

    #[test]
    fn a_click_records_the_window_under_it_or_its_display() {
        let on = aim(Mode::Window, [500.0, 300.0]);
        assert_eq!(on.target(), Some(CaptureTarget::Window { id: 1 }));
        let off = Aim {
            display: [1440.0, -200.0, 960.0, 600.0],
            pointer: Some([20.0, 30.0]),
            windows: Some(Vec::new()),
            ..aim(Mode::Window, [0.0, 0.0])
        };
        assert_eq!(
            off.target(),
            Some(CaptureTarget::Display {
                at: [1460.0, -170.0]
            })
        );
        let loading = Aim {
            windows: None,
            ..on
        };
        assert_eq!(loading.target(), None);
    }

    #[test]
    fn an_area_records_once_dragged_and_the_shortcut_records_the_display_before() {
        let mut area = aim(Mode::Area, [300.0, 200.0]);
        assert_eq!(area.target(), None);
        assert_eq!(
            area.shortcut(),
            Some(CaptureTarget::Display { at: [300.0, 200.0] })
        );
        area.drag = Some([100.0, 120.0]);
        assert_eq!(
            area.target(),
            Some(CaptureTarget::Area {
                rect: [100.0, 120.0, 200.0, 80.0]
            })
        );
        assert_eq!(area.shortcut(), area.target());
    }

    #[test]
    fn the_scene_shows_the_window_the_screen_or_the_drag() {
        let rest = [0.0, 0.0];
        let window = Scene::at(&aim(Mode::Window, [500.0, 300.0]), rest);
        assert_eq!(
            (window.kind, window.target),
            (Kind::Window, [120.0, 90.0, 520.0, 340.0])
        );
        assert!(window.camera);
        let badge = window.badge.unwrap();
        assert_eq!(badge.text, "Brave Browser — problems.kitlangton.com");
        // Centred on the window.
        assert!((badge.rect[0] + badge.rect[2] * 0.5 - 380.0).abs() < 0.5);

        let desk = Scene::at(&aim(Mode::Window, [20.0, 20.0]), rest);
        assert_eq!(
            (desk.kind, desk.target),
            (Kind::Screen, [0.0, 0.0, 960.0, 600.0])
        );
        assert_eq!(desk.badge.unwrap().text, FULL_SCREEN);

        let mut area = aim(Mode::Area, [300.0, 200.0]);
        let idle = Scene::at(&area, [300.0, 200.0]);
        assert_eq!(
            (idle.kind, idle.target, idle.badge),
            (Kind::None, [300.0, 200.0, 0.0, 0.0], None)
        );
        area.drag = Some([100.0, 120.0]);
        let drag = Scene::at(&area, rest);
        assert_eq!(drag.badge.unwrap().text, "200 × 80");
        assert!(!drag.camera);
    }

    #[test]
    fn a_window_off_the_display_edge_is_clipped_to_it() {
        let mut aim = aim(Mode::Window, [100.0, 100.0]);
        aim.windows = Some(vec![Window {
            id: 3,
            frame: [-200.0, 50.0, 400.0, 300.0],
            label: "Notes".into(),
        }]);
        let scene = Scene::at(&aim, [0.0, 0.0]);
        assert_eq!(scene.target, [0.0, 50.0, 200.0, 300.0]);
    }
}

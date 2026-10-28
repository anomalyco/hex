//! "How the shortcut works": a modal from General's Shortcut mode row that
//! shows Hold and Toggle side by side, and Double-tap to lock under Hold. Each
//! choice draws a keycap and two lanes, the key above and what Hex records
//! below, like a chart recorder: a pen holds still at the lanes' right end
//! while the tape scrolls left under it, so each press draws on at the pen and
//! slides away into a soft left edge. Picking a card is the setting.
//!
//! The tape runs on the window's poll clock while the modal is open; Reduce
//! Motion shows one whole take, still.

use std::cell::Cell;
use std::time::Duration;

use rune::{
    Color, Cursor, Cx, El, LineCap, Shape, SpringPair, Transition, Tx, canvas, col, el, row, text,
};

use super::Ui;
use super::controls::{
    ButtonKind, DESCRIPTION, KEY_GAP, KeySize, Paint, TRAY_PADDING, button, folds, key_tray,
    keycap, pressable_row, row_switch,
};
use crate::desktop_i18n::tr;
use crate::dictation::ShortcutMode;

const MODAL_WIDTH: f32 = 440.0;
const PADDING: f32 = 18.0;
const CARD_PADDING: f32 = 12.0;
const CARD_GAP: f32 = 10.0;
const KEYCAP_WIDTH: f32 = KeySize::Large.min_width();
/// Milliseconds between the keys of a chord going down, and coming up
/// (see `key_down`).
const CHORD_STAGGER_DOWN: f32 = 30.0;
const CHORD_STAGGER_UP: f32 = 15.0;
/// However long the chord, its last key goes down within this of the
/// first and comes up within half of it, so even a 5-key chord is held
/// whole through each of Double-tap to lock's 120 ms taps.
const CHORD_SPREAD: f32 = 60.0;
const VISUAL_GAP: f32 = 10.0;
/// The lanes beside a card's keycaps are never narrower than this: a long
/// chord widens the panel instead.
const MIN_LANES: f32 = 64.0;
/// How far a widened panel stays inside the window.
const WINDOW_MARGIN: f32 = 24.0;
/// How much Double-tap to lock adds to the panel, until it has been laid out.
const LOCK_ESTIMATE: f32 = 106.0;
/// The pen overhangs the lanes by 4 points above and below.
const LANES_HEIGHT: f32 = 25.0;
const LANE: f32 = 6.0;
const KEY_LANE_Y: f32 = 7.0;
const RECORD_LANE_Y: f32 = 18.0;
/// The empty tape past the pen.
const PEN_LEAD: f32 = 5.0;
/// The soft left edge the tape slides out under.
const TAPE_FADE: f32 = 18.0;
/// A tap keeps drawing at the pen after the key comes up until it is this
/// wide, so it is never a sliver on narrow lanes.
const MIN_TAP: f32 = 7.0;

/// One take of each script, in milliseconds; the scripts repeat end to end.
const LOOP: f64 = 3800.0;
/// How much time the tape shows between its left edge and the pen.
const WINDOW: f64 = 3000.0;
/// The still frame: a whole take on the tape, the key up.
pub(super) const DONE: f32 = 2700.0;
/// Frozen snapshots stop the tape mid-take.
pub(super) const SNAPSHOT_MS: f32 = 1800.0;
/// Room around the Double-tap row inside its folding slot for the focus ring.
const RING_ROOM: f32 = 4.0;
const FRAME: Duration = Duration::from_micros(16_667);

/// A press under 300 ms never records on its own, and a second tap released
/// within 300 ms of the first locks; each script stays inside those rules.
struct Script {
    presses: &'static [(f32, f32)],
    recording: (f32, f32),
    /// The hands-free stretch after a lock, drawn striped.
    locked: Option<(f32, f32)>,
}

const HOLD: Script = Script {
    presses: &[(400.0, 2400.0)],
    recording: (400.0, 2400.0),
    locked: None,
};
const TOGGLE: Script = Script {
    presses: &[(400.0, 540.0), (2400.0, 2540.0)],
    recording: (400.0, 2400.0),
    locked: None,
};
const LOCK: Script = Script {
    presses: &[(300.0, 420.0), (520.0, 640.0), (2400.0, 2540.0)],
    recording: (300.0, 640.0),
    locked: Some((640.0, 2400.0)),
};

#[derive(Default)]
pub(super) struct Explainer {
    pub(super) open: bool,
    /// The poll clock when the tape started.
    start: Option<f64>,
    /// Since the tape started, so takes before it never show.
    elapsed_ms: f64,
    /// The poll clock when the next frame's wake is due, so each frame asks
    /// for one.
    next_frame: f64,
    /// A fixed frame for snapshots.
    held_ms: Option<f32>,
    /// Reduce Motion, as the last view saw it: the tape stops.
    reduced: Cell<bool>,
    /// The body's height as last laid out under Hold, with Double-tap to
    /// lock. The panel centres at its Hold height in either mode, so
    /// switching modes moves only its bottom edge.
    hold_body: Cell<Option<f32>>,
}

impl Explainer {
    /// Milliseconds of tape since it started.
    fn now(&self) -> f64 {
        if self.reduced.get() {
            f64::from(DONE)
        } else {
            self.held_ms.map_or(self.elapsed_ms, f64::from)
        }
    }

    /// Where the pen is in the current take.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn time(&self) -> f32 {
        (self.now() % LOOP) as f32
    }
}

/// Opens the modal over General, alone: it ends a shortcut recording and
/// closes the model picker and any menu.
pub(super) fn open(ui: &mut Ui) {
    super::settings::cancel_capture(ui);
    super::picker::close(ui);
    super::sound_sets::close(ui);
    ui.open = None;
    ui.explainer.open = true;
    ui.explainer.start = None;
    ui.explainer.elapsed_ms = 0.0;
    ui.explainer.next_frame = 0.0;
}

pub(super) fn close(ui: &mut Ui) {
    ui.explainer.open = false;
}

/// Opens the modal, held at `held_ms` or looping when `None`.
pub(super) fn fixture(ui: &mut Ui, held_ms: Option<f32>) {
    open(ui);
    ui.explainer.held_ms = held_ms;
}

/// Advances the loop while the modal shows. True when it moved.
pub(super) fn poll(ui: &mut Ui, clock: f64, visible: bool, tx: &mut Tx) -> bool {
    let explainer = &mut ui.explainer;
    if !explainer.open || !visible || explainer.held_ms.is_some() || explainer.reduced.get() {
        return false;
    }
    let start = *explainer.start.get_or_insert(clock);
    explainer.elapsed_ms = (clock - start) * 1000.0;
    if clock >= explainer.next_frame {
        explainer.next_frame = clock + FRAME.as_secs_f64();
        tx.wake_after(FRAME);
    }
    true
}

/// Rune reads Reduce Motion again when the window gains focus, after that
/// frame's poll, so a stopped tape wakes once for the next poll to see it.
pub(super) fn window_focused(ui: &mut Ui, focused: bool, tx: &mut Tx) {
    let explainer = &mut ui.explainer;
    if focused && explainer.open && explainer.reduced.get() {
        explainer.next_frame = 0.0;
        tx.wake_after(FRAME);
    }
}

/// The "How it works" link under Shortcut mode. A focusable element rather
/// than a text link, so closing the modal can return focus to it.
pub(super) fn link(p: Paint) -> El {
    let label = tr("How it works");
    row()
        .key("shortcut-help")
        .role("button")
        .label(label)
        .focusable(true)
        .focus_ring(2.0, 2.0, p.k.c.border_focus)
        .radius(p.k.radius.sm)
        .cursor(Cursor::Pointer)
        .on_press(|ui: &mut Ui| open(ui))
        .child(text(label).key("label").font(DESCRIPTION).color(p.accent()))
}

pub(super) fn modal(ui: &Ui, cx: &Cx<Ui>, p: Paint) -> Option<[El; 2]> {
    // Kept while closed too, so the poll after opening already knows.
    ui.explainer.reduced.set(cx.system_reduced_motion());
    if !ui.explainer.open {
        return None;
    }
    let t = ui.explainer.now();
    // A still tape needs no edge to slide out under.
    let fade = if ui.explainer.reduced.get() {
        0.0
    } else {
        TAPE_FADE
    };
    let c = &p.k.c;
    let colors = Colors::new(p);
    let keys = ui.settings.dictation_hotkey.keycaps();
    let hold = ui.settings.shortcut_mode == ShortcutMode::Hold;
    let window = cx.layout("explainer-layer");
    let keys_width = cx
        .layout("mode-hold/visual/keys")
        .map_or_else(|| keys_estimate(&keys), |[_, _, width, _]| width);
    let width = panel_width(keys_width, window.map(|[_, _, width, _]| width));
    // The lanes draw at the width they lay out at, whatever the keycaps take.
    let lanes = |owner: &str, estimate: f32| {
        cx.layout(&format!("{owner}/visual/lanes"))
            .map_or(estimate, |[_, _, width, _]| width)
    };
    let inside = width - 2.0 * PADDING;
    let card_lanes = (inside - CARD_GAP) / 2.0 - 2.0 * CARD_PADDING - keys_width - VISUAL_GAP;
    let lock_lanes = inside - 2.0 * CARD_PADDING - keys_width - VISUAL_GAP;
    let cards = row()
        .key("modes")
        .equal_columns(2)
        .gap(CARD_GAP)
        .child(card(
            ui,
            p,
            ShortcutMode::Hold,
            tr("Hold"),
            tr("Hold to talk. Let go to paste."),
            visual(
                p,
                &colors,
                &keys,
                &HOLD,
                lanes("mode-hold", card_lanes),
                t,
                fade,
            ),
        ))
        .child(card(
            ui,
            p,
            ShortcutMode::Toggle,
            tr("Toggle"),
            tr("Press to start, press to stop."),
            visual(
                p,
                &colors,
                &keys,
                &TOGGLE,
                lanes("mode-toggle", card_lanes),
                t,
                fade,
            ),
        ));
    let lock_lanes = lanes("double-tap", lock_lanes);
    // Double-tap to lock is Hold's. Its slot stays and clips it, so it folds
    // away under the footer as the panel's bottom edge rises, never over it.
    let lock = col()
        .key("lock")
        .clip_soft(0.0)
        .ml(-RING_ROOM)
        .mr(-RING_ROOM)
        .px(RING_ROOM)
        .mb(-RING_ROOM)
        .pb(RING_ROOM)
        .children(hold.then(|| {
            lock_row(ui, p, &colors, &keys, lock_lanes, t, fade)
                .mt(CARD_GAP)
                .transition(folds())
        }));
    let body = col().key("explainer-body").child(cards).child(lock);
    let mut panel = col()
        .key("shortcut-explainer")
        .role("dialog")
        .label(tr("How the shortcut works"))
        .w(width)
        .pt(PADDING)
        .px(PADDING)
        .pb(14.0)
        .gap(14.0)
        .radius(p.k.radius.xl)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak)
        // Presses inside the panel stay inside.
        .on_press(|_: &mut Ui| {})
        .child(
            text(tr("How the shortcut works"))
                .key("title")
                .font(p.k.ty.body_medium)
                .color(p.text()),
        )
        .child(body)
        .child(
            row().key("footer").justify_end().child(
                button(
                    p,
                    "close-explainer",
                    tr("Close"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| close(ui),
                )
                .default_button(),
            ),
        );
    for shadow in &p.k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    let layer = |key: &str| {
        el().key(key)
            .role("none")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
    };
    // Centred at its Hold height in either mode, so the cards hold still
    // under the pointer and Hold and Toggle move only the bottom edge.
    let hold_body = &ui.explainer.hold_body;
    let laid_height = |key| cx.layout(key).map(|[_, _, _, height]| height);
    let body_height = laid_height("shortcut-explainer/explainer-body");
    // A pass that still sees Toggle's layout, its slot empty, measures nothing.
    let unfolded =
        laid_height("explainer-body/lock").is_some_and(|slot| slot > LOCK_ESTIMATE / 2.0);
    if hold && unfolded {
        hold_body.set(body_height);
    }
    let placed = match window.zip(laid_height("shortcut-explainer")) {
        Some(([_, _, _, window], height)) => {
            let folded = match (hold, hold_body.get(), body_height) {
                (true, _, _) => 0.0,
                (false, Some(full), Some(body)) => full - body,
                (false, _, _) => LOCK_ESTIMATE,
            };
            layer("explainer-layer")
                .justify_center()
                .items_start()
                .pt(((window - height - folded) / 2.0).max(0.0))
        }
        None => layer("explainer-layer").center(),
    };
    Some([
        layer("explainer-scrim")
            .fill(c.scrim)
            .on_press(close)
            .transition(Transition::opacity().springs(overlay_springs(p))),
        placed
            .transition(panel_transition(p))
            // Keys stay inside, Escape or a press outside closes, and focus
            // returns to the link that opened it.
            .child(panel.modal(true, close)),
    ])
}

/// The key tray's width until layout measures it: a one-glyph legend is
/// square, a word about 7.5 points a letter.
fn keys_estimate(keys: &[String]) -> f32 {
    let caps: f32 = keys
        .iter()
        .map(|label| match label.chars().count() {
            0 | 1 => KEYCAP_WIDTH,
            letters => (14.0 + 7.5 * letters as f32).max(KEYCAP_WIDTH),
        })
        .sum();
    caps + KEY_GAP * keys.len().saturating_sub(1) as f32 + 2.0 * TRAY_PADDING
}

/// The panel is wide enough for each card's lanes to keep `MIN_LANES`
/// beside `keys` points of keycaps, as far as the window allows.
fn panel_width(keys: f32, window: Option<f32>) -> f32 {
    let card = 2.0 * CARD_PADDING + keys + VISUAL_GAP + MIN_LANES;
    let wanted = 2.0 * card + CARD_GAP + 2.0 * PADDING;
    let room = window.map_or(f32::INFINITY, |window| window - 2.0 * WINDOW_MARGIN);
    wanted.min(room).max(MODAL_WIDTH)
}

/// How the panel enters and leaves: the same as the picker's.
fn panel_transition(p: Paint) -> Transition {
    super::controls::modal_transition(p)
}

fn overlay_springs(p: Paint) -> SpringPair {
    SpringPair::new(p.k.motion.overlay, p.k.motion.overlay)
}

fn card(ui: &Ui, p: Paint, mode: ShortcutMode, title: &str, caption: &str, visual: El) -> El {
    let c = &p.k.c;
    let selected = ui.settings.shortcut_mode == mode;
    let key = match mode {
        ShortcutMode::Hold => "mode-hold",
        ShortcutMode::Toggle => "mode-toggle",
    };
    col()
        .key(key)
        .role("button")
        .label(title)
        .focusable(true)
        .focus_ring(2.0, 2.0, c.border_focus)
        .padding(CARD_PADDING)
        .gap(10.0)
        .radius(p.k.radius.lg)
        .border(
            if selected { 1.5 } else { p.hair },
            if selected {
                c.border_focus
            } else {
                c.border_weak
            },
        )
        .hover(c.hover)
        .press(c.pressed)
        .on_press(move |ui: &mut Ui| pick(ui, mode))
        .child(visual)
        .child(
            col()
                .key("copy")
                .gap(2.0)
                .child(
                    text(title)
                        .key("name")
                        .font(p.k.ty.ui_medium)
                        .color(p.text()),
                )
                .child(
                    text(caption)
                        .key("caption")
                        .font(DESCRIPTION)
                        .color(p.muted())
                        .wrap(),
                ),
        )
}

/// The same save path as Shortcut mode's segmented control.
fn pick(ui: &mut Ui, mode: ShortcutMode) {
    if ui.settings.shortcut_mode != mode {
        ui.update(|settings| settings.shortcut_mode = mode);
    }
}

fn lock_row(
    ui: &Ui,
    p: Paint,
    colors: &Colors,
    keys: &[String],
    width: f32,
    now: f64,
    fade: f32,
) -> El {
    let c = &p.k.c;
    // Like General's switch rows, the switch shows focus for the row.
    pressable_row(col())
        .key("double-tap")
        .label(tr("Double-tap to lock"))
        .py(10.0)
        .px(CARD_PADDING)
        .gap(8.0)
        .radius(p.k.radius.lg)
        .border(p.hair, c.border_weak)
        .on_press(flip_lock)
        .child(
            row()
                .key("top")
                .gap(12.0)
                .center_y()
                .child(
                    col()
                        .key("copy")
                        .grow(1.0)
                        .shrink()
                        .gap(2.0)
                        .child(
                            text(tr("Double-tap to lock"))
                                .key("name")
                                .font(p.k.ty.ui_medium)
                                .color(p.text()),
                        )
                        .child(
                            text(tr("Tap twice to talk hands-free. Press once to stop."))
                                .key("caption")
                                .font(DESCRIPTION)
                                .color(p.muted())
                                .wrap(),
                        ),
                )
                .child(row_switch(p, ui.settings.double_tap_lock)),
        )
        .child(visual(p, colors, keys, &LOCK, width, now, fade))
}

/// Double-tap to lock, as General's row flips it.
pub(super) fn flip_lock(ui: &mut Ui) {
    let enabled = !ui.settings.double_tap_lock;
    ui.update(|settings| {
        settings.double_tap_lock = enabled;
        if !enabled {
            settings.double_tap_only = false;
        }
    });
}

/// Whether key `index` of a `keys`-key chord is down `t` ms into the take.
/// A chord's keys go down one after another from the press, then come up
/// in the same order from its end, so each key rests between taps.
fn key_down(script: &Script, keys: usize, index: usize, t: f32) -> bool {
    let gaps = keys.saturating_sub(1).max(1) as f32;
    let step_down = CHORD_STAGGER_DOWN.min(CHORD_SPREAD / gaps);
    let step_up = CHORD_STAGGER_UP.min(CHORD_SPREAD / 2.0 / gaps);
    let index = index as f32;
    script
        .presses
        .iter()
        .any(|&(d, u)| d + index * step_down <= t && t < u + index * step_up)
}

/// The shortcut's keys in one tray, pressing in turn, beside the lanes
/// drawn `width` points wide, whose tape fades out over its first `fade`
/// points.
fn visual(
    p: Paint,
    colors: &Colors,
    keys: &[String],
    script: &Script,
    width: f32,
    now: f64,
    fade: f32,
) -> El {
    let t = (now % LOOP) as f32;
    let down = |index: usize| key_down(script, keys.len(), index, t);
    let fill = |el: El| el.absolute(0.0, 0.0).w_full().h_full();
    row()
        .key("visual")
        .role("none")
        .h(KeySize::Large.height() + 2.0 * TRAY_PADDING)
        .gap(VISUAL_GAP)
        .center_y()
        .child(key_tray(
            p,
            "keys",
            KeySize::Large,
            keys.iter()
                .enumerate()
                .map(|(index, label)| keycap(p, index, label, KeySize::Large, down(index))),
        ))
        .child(
            el().key("lanes")
                .role("none")
                .grow(1.0)
                .shrink()
                .h(LANES_HEIGHT)
                .child(fill(canvas(tracks(colors, width)).key("tracks")))
                .child(
                    fill(el().key("tape").role("none"))
                        .clip_fade(fade, 0.0, 0.0, 0.0)
                        .child(fill(canvas(tape(colors, script, width, now)).key("marks"))),
                )
                .child(fill(canvas(pen(colors, width)).key("pen"))),
        )
}

/// Opaque lane colours over the panel, so overlapping pieces never darken.
struct Colors {
    track: Color,
    key: Color,
    recording: Color,
    locked: Color,
    playhead: Color,
}

impl Colors {
    fn new(p: Paint) -> Self {
        let c = &p.k.c;
        let base = c.bg_base.alpha(1.0);
        let over =
            |under: Color, color: Color, amount: f32| under.mix(color.alpha(1.0), color.a * amount);
        let track = over(base, c.layer_03, 1.0);
        let recording = over(base, p.danger(), 1.0);
        Self {
            track,
            key: over(track, p.text(), 0.55),
            recording,
            locked: track.mix(recording, 0.35),
            playhead: p.text().alpha(p.text().a * 0.45),
        }
    }
}

/// Lanes too narrow to hold the pen draw nothing.
fn too_narrow(width: f32) -> bool {
    width <= PEN_LEAD
}

/// The empty lanes, the full width.
fn tracks(colors: &Colors, width: f32) -> Shape {
    let mut shape = Shape::new();
    if too_narrow(width) {
        return shape;
    }
    for y in [KEY_LANE_Y, RECORD_LANE_Y] {
        shape = bar(shape, 0.0, width, y, colors.track, (true, true));
    }
    shape.view_box(width, LANES_HEIGHT)
}

/// Where the tape meets the pen.
fn pen_x(width: f32) -> f32 {
    width - PEN_LEAD
}

fn pen(colors: &Colors, width: f32) -> Shape {
    if too_narrow(width) {
        return Shape::new();
    }
    let x = pen_x(width);
    Shape::new()
        .line(
            [x, 0.75],
            [x, LANES_HEIGHT - 0.75],
            1.5,
            LineCap::Round,
            colors.playhead,
        )
        .view_box(width, LANES_HEIGHT)
}

/// Where the tape shows `ms` when the pen is at `now`: the pen at the
/// lanes' right end, `WINDOW` before it at their left.
fn at(ms: f64, now: f64, width: f32) -> f32 {
    let pen = pen_x(width);
    pen - ((now - ms) / WINDOW) as f32 * pen
}

/// Where the stretch from `d` to `u` of the take starting at `from` lies on
/// the tape at `now`, at least `min` wide but never past the pen, or `None`
/// before it starts and once it has scrolled off.
fn drawn((d, u): (f32, f32), from: f64, now: f64, width: f32, min: f32) -> Option<(f32, f32)> {
    let d = from + f64::from(d);
    if d >= now {
        return None;
    }
    let start = at(d, now, width);
    let end = at((from + f64::from(u)).min(now), now, width);
    let end = end.max(start + min).min(pen_x(width));
    (end > 0.0).then_some((start, end))
}

/// The takes on the tape at `now`, from the first one.
fn takes(now: f64) -> impl Iterator<Item = f64> {
    let first = ((now - WINDOW) / LOOP).floor().max(0.0) as u32;
    let last = (now / LOOP).floor() as u32;
    (first..=last).map(|take| f64::from(take) * LOOP)
}

/// Each take on the tape at `now`: every stretch draws up to the pen and
/// keeps its place on the tape as it scrolls.
fn tape(colors: &Colors, script: &Script, width: f32, now: f64) -> Shape {
    let mut shape = Shape::new();
    if too_narrow(width) {
        return shape;
    }
    for from in takes(now) {
        let drawn = |stretch, min| drawn(stretch, from, now, width, min);
        for &press in script.presses {
            if let Some((start, end)) = drawn(press, MIN_TAP) {
                shape = bar(shape, start, end, KEY_LANE_Y, colors.key, (true, true));
            }
        }
        if let Some((start, end)) = drawn(script.recording, 0.0) {
            let ends = (true, script.locked.is_none());
            shape = bar(shape, start, end, RECORD_LANE_Y, colors.recording, ends);
        }
        if let Some((start, end)) = script.locked.and_then(|locked| drawn(locked, 0.0)) {
            shape = bar(
                shape,
                start,
                end,
                RECORD_LANE_Y,
                colors.locked,
                (false, true),
            );
            shape = stripes(
                shape,
                start,
                end - LANE / 2.0,
                RECORD_LANE_Y,
                colors.recording,
            );
        }
    }
    shape.view_box(width, LANES_HEIGHT)
}

/// A lane-thick bar from `start` to `end`, each end round or square.
fn bar(
    mut shape: Shape,
    start: f32,
    end: f32,
    y: f32,
    color: Color,
    (round_start, round_end): (bool, bool),
) -> Shape {
    let width = end - start;
    if width <= 0.0 {
        return shape;
    }
    let radius = (LANE / 2.0).min(width / 2.0);
    let middle = start + width / 2.0;
    // A bar narrower than the lane is an upright pill.
    shape = if width < LANE {
        let reach = (LANE / 2.0 - radius).max(0.01);
        shape.line(
            [middle, y - reach],
            [middle, y + reach],
            width,
            LineCap::Round,
            color,
        )
    } else {
        let a = start + radius;
        let b = (end - radius).max(a + 0.01);
        shape.line([a, y], [b, y], LANE, LineCap::Round, color)
    };
    let square = |shape: Shape, from: f32, to: f32| {
        shape.fill(
            [
                [from, y - LANE / 2.0],
                [to, y - LANE / 2.0],
                [to, y + LANE / 2.0],
                [from, y + LANE / 2.0],
            ],
            color,
        )
    };
    if !round_start {
        shape = square(shape, start, start + radius.max(0.01).min(width));
    }
    if !round_end {
        shape = square(shape, end - radius.max(0.01).min(width), end);
    }
    shape
}

/// Diagonal stripes over the hands-free stretch, anchored at its start so
/// they ride the tape with it. Only those still on the tape are drawn.
fn stripes(mut shape: Shape, start: f32, end: f32, y: f32, color: Color) -> Shape {
    const PERIOD: f32 = 7.0;
    const BAND: f32 = 3.5;
    let shown = start.max(0.0);
    if end <= shown {
        return shape;
    }
    let (top, bottom) = (y - LANE / 2.0, y + LANE / 2.0);
    // The first band that can reach the tape, in phase with `start`.
    let mut left = start - LANE + ((shown - start) / PERIOD).floor() * PERIOD;
    while left < end {
        let band = [
            [left, bottom],
            [left + BAND, bottom],
            [left + BAND + LANE, top],
            [left + LANE, top],
        ];
        let clipped = clip_x(&band, shown, end);
        if clipped.len() >= 3 {
            shape = shape.fill(clipped, color);
        }
        left += PERIOD;
    }
    shape
}

/// A convex polygon cut to `left..=right`.
fn clip_x(points: &[[f32; 2]], left: f32, right: f32) -> Vec<[f32; 2]> {
    let cut = |points: Vec<[f32; 2]>, inside: &dyn Fn(f32) -> bool, edge: f32| {
        let mut out = Vec::with_capacity(points.len() + 2);
        for (i, &a) in points.iter().enumerate() {
            let b = points[(i + 1) % points.len()];
            if inside(a[0]) {
                out.push(a);
            }
            if inside(a[0]) != inside(b[0]) {
                let s = (edge - a[0]) / (b[0] - a[0]);
                out.push([edge, a[1] + (b[1] - a[1]) * s]);
            }
        }
        out
    };
    let points = cut(points.to_vec(), &|x| x >= left, left);
    if points.is_empty() {
        return points;
    }
    cut(points, &|x| x <= right, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stripes_stay_inside_their_stretch() {
        let band = [[0.0, 6.0], [3.5, 6.0], [9.5, 0.0], [6.0, 0.0]];
        let clipped = clip_x(&band, 2.0, 8.0);
        assert!(clipped.iter().all(|[x, _]| (2.0..=8.0).contains(x)));
        assert!(clip_x(&band, 20.0, 30.0).is_empty());
    }

    /// Each stretch's ends on the tape in the first four takes, or `None`
    /// before it starts.
    fn ends(script: &Script, width: f32, now: f64) -> Vec<Option<(f32, f32)>> {
        let mut ends = Vec::new();
        for take in 0..4 {
            let from = f64::from(take) * LOOP;
            for &press in script.presses {
                ends.push(drawn(press, from, now, width, MIN_TAP));
            }
            ends.push(drawn(script.recording, from, now, width, 0.0));
        }
        ends
    }

    #[test]
    fn nothing_draws_past_the_pen() {
        let width = 60.0;
        for script in [HOLD, TOGGLE, LOCK] {
            let mut now = 0.0;
            while now < 3.0 * LOOP {
                for (start, end) in ends(&script, width, now).into_iter().flatten() {
                    assert!(start <= end && end <= pen_x(width) + 1e-3, "at {now} ms");
                }
                now += 5.0;
            }
        }
    }

    /// One frame to the next, the tape moves the same small step: no
    /// stretch ever jumps, widens ahead of time, or resets.
    #[test]
    fn the_tape_scrolls_without_steps() {
        let width = 120.0;
        let step = 1000.0 / 60.0;
        let travel = (step / WINDOW) as f32 * pen_x(width);
        for script in [HOLD, TOGGLE, LOCK] {
            let mut now = 0.0;
            while now < 3.0 * LOOP {
                let (before, after) = (ends(&script, width, now), ends(&script, width, now + step));
                for (a, b) in before.iter().zip(&after) {
                    let (Some(a), Some(b)) = (a, b) else { continue };
                    assert!(
                        (a.0 - b.0 - travel).abs() < 1e-2,
                        "a start jumped at {now} ms"
                    );
                    assert!(b.1 - a.1 >= -travel - 1e-2, "an end jumped at {now} ms");
                }
                now += step;
            }
        }
    }

    /// The still frame shows each script's whole take, on the tape and up.
    #[test]
    fn the_still_frame_shows_a_whole_take() {
        let width = 60.0;
        for script in [HOLD, TOGGLE, LOCK] {
            let now = f64::from(DONE);
            let ends: Vec<_> = ends(&script, width, now).into_iter().flatten().collect();
            assert_eq!(ends.len(), script.presses.len() + 1);
            for (start, end) in ends {
                assert!(start > 0.0 && end <= pen_x(width), "{start}..{end}");
            }
            assert!(script.presses.iter().all(|&(_, u)| u < DONE));
        }
    }

    /// However long the chord, every press holds all its keys down
    /// together, and every key comes back up before the next press.
    #[test]
    fn every_press_holds_the_whole_chord() {
        for script in [HOLD, TOGGLE, LOCK] {
            for keys in 1..=6 {
                let all = |t: f32, down: bool| {
                    (0..keys).all(|index| key_down(&script, keys, index, t) == down)
                };
                for (press, &(d, u)) in script.presses.iter().enumerate() {
                    let held = (d as u32..u as u32)
                        .filter(|&t| all(t as f32, true))
                        .count();
                    assert!(held >= 40, "{keys} keys held together {held} ms");
                    if let Some(&(next, _)) = script.presses.get(press + 1) {
                        assert!(
                            (u as u32..next as u32).any(|t| all(t as f32, false)),
                            "{keys} keys rest before the next press"
                        );
                    }
                }
            }
        }
    }

    /// Lanes squeezed narrower than the pen's lead draw nothing at all.
    #[test]
    fn narrow_lanes_draw_nothing() {
        let colors = Colors {
            track: Color::WHITE,
            key: Color::WHITE,
            recording: Color::WHITE,
            locked: Color::WHITE,
            playhead: Color::WHITE,
        };
        for width in [-20.0, 0.0, PEN_LEAD] {
            assert!(tracks(&colors, width).is_empty());
            assert!(pen(&colors, width).is_empty());
            assert!(tape(&colors, &LOCK, width, 1800.0).is_empty());
        }
        assert!(!tape(&colors, &LOCK, PEN_LEAD + 20.0, 1800.0).is_empty());
    }

    /// A stretch that has scrolled off the tape is no longer drawn, and
    /// stripes start no further left than the tape.
    #[test]
    fn the_past_leaves_the_tape() {
        let width = 60.0;
        let now = f64::from(HOLD.recording.1) + WINDOW + 1.0;
        assert_eq!(drawn(HOLD.recording, 0.0, now, width, 0.0), None);
        // Fifteen periods to the left, in the same phase: only the part on
        // the tape is drawn, the same as a stretch starting at its edge.
        let stripes = |start| stripes(Shape::new(), start, 20.0, 10.0, Color::WHITE);
        assert!(!stripes(0.0).is_empty());
        assert_eq!(stripes(-15.0 * 7.0), stripes(0.0));
    }

    #[test]
    fn each_script_presses_the_key_while_it_records() {
        for script in [HOLD, TOGGLE, LOCK] {
            let (start, _) = script.recording;
            assert!(script.presses.iter().any(|&(d, _)| d == start));
            assert!(
                script
                    .presses
                    .iter()
                    .all(|&(d, u)| 0.0 < d && d < u && u < DONE)
            );
        }
    }
}

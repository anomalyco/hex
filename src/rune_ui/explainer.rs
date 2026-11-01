//! Shortcut mode's modal, from General's Shortcut mode row: the four ways
//! the shortcut can trigger (`shortcut_mode::Trigger`), each with its keycaps
//! and two lanes, the key above and what Hex records below, like a chart
//! recorder: a pen holds still at the lanes' right end while the tape scrolls
//! left under it, so each press draws on at the pen and slides away into a
//! soft left edge. Picking an option is the setting.
//!
//! The tape runs on the window's poll clock while the modal is open; Reduce
//! Motion shows one whole take, still.

use std::cell::Cell;
use std::time::Duration;

use rune::{Color, Cx, El, LineCap, Shape, SpringPair, Transition, Tx, canvas, col, el, row, text};

use super::Ui;
use super::controls::{
    ButtonKind, DESCRIPTION, KEY_GAP, KeySize, Paint, TRAY_PADDING, button, key_tray, keycap,
};
use super::shortcut_mode::{Trigger, pick};
use crate::desktop::i18n::tr;

const MODAL_WIDTH: f32 = 600.0;
const PADDING: f32 = 18.0;
const OPTION_PADDING: f32 = 12.0;
const OPTION_GAP: f32 = 8.0;
/// The name and caption beside each option's keys and lanes.
const COPY_WIDTH: f32 = 280.0;
const COPY_GAP: f32 = 16.0;
/// An option this shortcut can't use dims its keys and lanes.
const UNAVAILABLE_OPACITY: f32 = 0.4;
const KEYCAP_WIDTH: f32 = KeySize::Large.min_width();
/// Milliseconds between the keys of a chord going down, and coming up
/// (see `key_down`).
const CHORD_STAGGER_DOWN: f32 = 30.0;
const CHORD_STAGGER_UP: f32 = 15.0;
/// However long the chord, its last key goes down within this of the
/// first and comes up within half of it, so even a 5-key chord is held
/// whole through each 120 ms double-tap.
const CHORD_SPREAD: f32 = 60.0;
pub(super) const VISUAL_GAP: f32 = 10.0;
/// The lanes beside an option's keycaps are never narrower than this: a
/// long chord widens the panel instead.
const MIN_LANES: f32 = 64.0;
/// How far a widened panel stays inside the window.
const WINDOW_MARGIN: f32 = 24.0;
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
const FRAME: Duration = Duration::from_micros(16_667);

/// A press under 300 ms never records on its own, and a second tap released
/// within 300 ms of the first locks; each script stays inside those rules.
pub(super) struct Script {
    presses: &'static [(f32, f32)],
    /// Earlier recordings in the same take, drawn solid.
    before: &'static [(f32, f32)],
    recording: (f32, f32),
    /// The hands-free stretch after a lock, drawn striped.
    locked: Option<(f32, f32)>,
}

/// Hold to talk, let go to paste.
pub(super) const HOLD: Script = Script {
    presses: &[(400.0, 2400.0)],
    before: &[],
    recording: (400.0, 2400.0),
    locked: None,
};
/// A held talk, then a double-tap that talks hands-free until the next press.
pub(super) const HOLD_OR_LOCK: Script = Script {
    presses: &[
        (80.0, 980.0),
        (1300.0, 1420.0),
        (1520.0, 1640.0),
        (2560.0, 2680.0),
    ],
    before: &[(80.0, 980.0)],
    recording: (1300.0, 1640.0),
    locked: Some((1640.0, 2560.0)),
};
/// One tap starts hands-free until the next press.
pub(super) const TAP: Script = Script {
    presses: &[(400.0, 540.0), (2400.0, 2540.0)],
    before: &[],
    recording: (400.0, 540.0),
    locked: Some((540.0, 2400.0)),
};
/// Two taps, then hands-free from the second until the next press.
pub(super) const DOUBLE_TAP: Script = Script {
    presses: &[(300.0, 420.0), (520.0, 640.0), (2400.0, 2540.0)],
    before: &[],
    recording: (640.0, 640.0),
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
    super::vocabulary::close(ui);
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
    let current = Trigger::of(&ui.settings);
    let window = cx.layout_size("explainer-layer");
    let first = Trigger::ALL[0].key();
    let keys_width = cx
        .layout_size(&format!("{first}/visual/keys"))
        .map_or_else(|| keys_estimate(&keys), |[width, _]| width);
    let width = panel_width(keys_width, window.map(|[width, _]| width));
    // The lanes draw at the width they lay out at, whatever the keycaps take.
    let estimate = width
        - 2.0 * PADDING
        - 2.0 * OPTION_PADDING
        - COPY_WIDTH
        - COPY_GAP
        - keys_width
        - VISUAL_GAP;
    let options = col()
        .key("options")
        .gap(OPTION_GAP)
        .children(Trigger::ALL.into_iter().map(|trigger| {
            let lanes = cx
                .layout_size(&format!("{}/visual/lanes", trigger.key()))
                .map_or(estimate, |[width, _]| width);
            option(
                ui,
                p,
                trigger,
                trigger == current,
                visual(p, &colors, &keys, trigger.script(), lanes, t, fade),
            )
        }));
    let mut panel = col()
        .key("shortcut-mode-modal")
        .role("dialog")
        .label(tr("Shortcut mode"))
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
            text(tr("Shortcut mode"))
                .key("title")
                .font(p.k.ty.body_medium)
                .color(p.text()),
        )
        .child(options)
        .child(
            row().key("footer").justify_end().child(
                button(
                    p,
                    "shortcut-mode-done",
                    tr("Done"),
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
    Some([
        layer("explainer-scrim")
            .fill(c.scrim)
            .on_press(close)
            .transition(Transition::opacity().springs(overlay_springs(p))),
        layer("explainer-layer")
            .center()
            .transition(panel_transition(p))
            // Keys stay inside, Escape or a press outside closes, and focus
            // returns to the row that opened it.
            .child(panel.modal(true, close)),
    ])
}

/// The key tray's width until layout measures it: a one-glyph legend is
/// square, a word about 7.5 points a letter.
pub(super) fn keys_estimate(keys: &[String]) -> f32 {
    let caps: f32 = keys
        .iter()
        .map(|label| match label.chars().count() {
            0 | 1 => KEYCAP_WIDTH,
            letters => (14.0 + 7.5 * letters as f32).max(KEYCAP_WIDTH),
        })
        .sum();
    caps + KEY_GAP * keys.len().saturating_sub(1) as f32 + 2.0 * TRAY_PADDING
}

/// The panel is wide enough for each option's lanes to keep `MIN_LANES`
/// beside `keys` points of keycaps, as far as the window allows.
fn panel_width(keys: f32, window: Option<f32>) -> f32 {
    let option = 2.0 * OPTION_PADDING + COPY_WIDTH + COPY_GAP + keys + VISUAL_GAP + MIN_LANES;
    let wanted = option + 2.0 * PADDING;
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

/// One trigger: its name and what it does beside its keys and lanes.
/// Picking it is the setting. One this shortcut can't use says why and
/// takes no focus or press.
fn option(ui: &Ui, p: Paint, trigger: Trigger, selected: bool, visual: El) -> El {
    let c = &p.k.c;
    let unavailable = trigger.unavailable(&ui.settings);
    let caption = unavailable
        .clone()
        .unwrap_or_else(|| trigger.caption().to_string());
    let mut option = row()
        .key(trigger.key())
        .role("radio")
        .label(trigger.name())
        .padding(OPTION_PADDING)
        .gap(COPY_GAP)
        .center_y()
        .radius(p.k.radius.lg)
        .border(
            if selected { 1.5 } else { p.hair },
            if selected {
                c.border_focus
            } else {
                c.border_weak
            },
        )
        .child(
            col()
                .key("copy")
                .w(COPY_WIDTH)
                .gap(2.0)
                .child(
                    text(trigger.name())
                        .key("name")
                        .font(p.k.ty.ui_medium)
                        .color(if unavailable.is_some() {
                            p.faint()
                        } else {
                            p.text()
                        }),
                )
                .child(
                    text(caption)
                        .key("caption")
                        .font(DESCRIPTION)
                        .color(p.muted())
                        .wrap(),
                ),
        )
        .child(visual.grow(1.0).shrink().opacity(if unavailable.is_some() {
            UNAVAILABLE_OPACITY
        } else {
            1.0
        }));
    if unavailable.is_none() {
        option = option
            .focusable(true)
            .focus_ring(2.0, 2.0, c.border_focus)
            .hover(c.hover)
            .press(c.pressed)
            .on_press(move |ui: &mut Ui| pick(ui, trigger));
    }
    option
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
pub(super) fn visual(
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
pub(super) struct Colors {
    track: Color,
    key: Color,
    recording: Color,
    locked: Color,
    playhead: Color,
}

impl Colors {
    pub(super) fn new(p: Paint) -> Self {
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
        for &stretch in script.before {
            if let Some((start, end)) = drawn(stretch, 0.0) {
                shape = bar(
                    shape,
                    start,
                    end,
                    RECORD_LANE_Y,
                    colors.recording,
                    (true, true),
                );
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
                (script.recording.0 == script.recording.1, true),
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
        for script in [HOLD, HOLD_OR_LOCK, TAP, DOUBLE_TAP] {
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
        for script in [HOLD, HOLD_OR_LOCK, TAP, DOUBLE_TAP] {
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
        for script in [HOLD, HOLD_OR_LOCK, TAP, DOUBLE_TAP] {
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
        for script in [HOLD, HOLD_OR_LOCK, TAP, DOUBLE_TAP] {
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
            assert!(tape(&colors, &DOUBLE_TAP, width, 1800.0).is_empty());
        }
        assert!(!tape(&colors, &DOUBLE_TAP, PEN_LEAD + 20.0, 1800.0).is_empty());
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
        for script in [HOLD, HOLD_OR_LOCK, TAP, DOUBLE_TAP] {
            // Double-tap starts as the second tap comes up.
            let (start, _) = script.recording;
            assert!(
                script
                    .presses
                    .iter()
                    .any(|&(d, u)| d == start || u == start)
            );
            for &(start, _) in script.before {
                assert!(script.presses.iter().any(|&(d, _)| d == start));
            }
            assert!(
                script
                    .presses
                    .iter()
                    .all(|&(d, u)| 0.0 < d && d < u && u < DONE)
            );
        }
    }
}

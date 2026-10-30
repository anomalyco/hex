//! The ink overlay: a click-through Rune overlay over the recording
//! capture's target that draws what the pointer marks, as Scrawl's Metal
//! overlay does. Shift-drawn ink holds 650 ms after it lifts, then fades;
//! the laser trails a 0.42 s tapering tail with a soft glow and a hot core;
//! a dot marks the pointer while Shift is held;
//! and a full-screen capture's edges glow red for its first second.
//!
//! While Annotate draws (`capture_host::drawing`: its key held, or the
//! capsule's pen on), the overlay takes the pointer so clicks never reach
//! the app, and shows a pen cursor; the capsule stays clickable above it.
//! On Linux the overlay covers the screen and leaves a hole where the
//! capsule's pill is, and its pointer moves are the strokes
//! (`capture_host::draw`); on the Mac the tap draws them.
//!
//! [`Scene::at`] decides what shows from the capture's strokes and clock;
//! the view only draws it, and the overlay asks for frames only while a
//! capture records.

use std::sync::atomic::Ordering;
use std::time::Duration;

use rune::{Color, El, LineCap, OverlayHandle, Shape, Tx, canvas, el};
use scrawl_capture::capture::InkSnapshot;
use scrawl_capture::sketch::{slice_laser_window, smooth_ink_points, smooth_laser_points};

use crate::capture_host::InkSource;

/// How long the laser's tail trails its tip.
const TAIL_MS: u64 = 420;
/// Lifted ink stays this long, then fades over as long again.
const INK_HOLD_MS: u64 = 650;
const INK_FADE_MS: u64 = 650;
/// The start glow rises over the first 0.12 s and is gone by 1.1 s.
const GLOW_RISE_MS: f32 = 120.0;
const GLOW_FALL_MS: [f32; 2] = [350.0, 1100.0];
/// While a capture records, the overlay looks for new strokes this often.
const FRAME: Duration = Duration::from_micros(16_667);

/// Scrawl's ink and laser colours: content drawn on the screen, like a
/// pen's, not interface chrome, so they stay the same in either appearance.
fn ink_red() -> Color {
    rune::color::hex("#ff3b30").unwrap_or(Color::WHITE)
}

fn laser_red() -> Color {
    rune::color::hex("#ff2930").unwrap_or(Color::WHITE)
}

fn laser_hot() -> Color {
    rune::color::hex("#fff7f7").unwrap_or(Color::WHITE)
}

/// A stroke's points, each with its pressure or weight.
type Points = Vec<([f32; 2], f32)>;

/// What the overlay draws at one moment, in the target's points.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Scene {
    pub(crate) size: [f32; 2],
    /// Each ink stroke's points with their pressure, and its opacity.
    ink: Vec<(Points, f32)>,
    /// Each laser tail's points with their weight, 0 at the tail.
    laser: Vec<Points>,
    /// The pointer while Shift is held.
    tip: Option<[f32; 2]>,
    /// The start glow's strength, 0 to 1.
    glow: f32,
}

impl Scene {
    /// What shows `now_ms` into a capture with `ink`; `tip` is the pointer
    /// in global points while Shift is held.
    pub(crate) fn at(ink: &InkSnapshot, now_ms: u64, tip: Option<[f32; 2]>) -> Self {
        let frame = ink.target;
        let (x, y) = (frame.x as f32, frame.y as f32);
        let (w, h) = (
            (frame.width as f32).max(1.0),
            (frame.height as f32).max(1.0),
        );
        let to_screen = |nx: f32, ny: f32| [nx * w, ny * h];
        let mut scene = Scene {
            size: [w, h],
            tip: tip.map(|[px, py]| [px - x, py - y]),
            ..Scene::default()
        };
        let strokes = ink
            .strokes
            .iter()
            .map(|stroke| (stroke, false))
            .chain(ink.laser.iter().map(|stroke| (stroke, true)))
            .chain(ink.live.iter().map(|stroke| (stroke, true)));
        for (stroke, live) in strokes {
            if stroke.laser {
                if stroke.t1 + TAIL_MS < now_ms {
                    continue;
                }
                let tail = slice_laser_window(&stroke.timed_points(), now_ms, TAIL_MS, to_screen);
                let tail = smooth_laser_points(&tail);
                if tail.len() >= 2 {
                    scene.laser.push(tail);
                }
            } else {
                let alpha = if live {
                    1.0
                } else {
                    ink_alpha(now_ms.saturating_sub(stroke.t1))
                };
                if alpha <= 0.0 {
                    continue;
                }
                let points: Vec<[f32; 2]> = stroke
                    .normalized_points()
                    .into_iter()
                    .map(|[nx, ny]| to_screen(nx, ny))
                    .collect();
                let smoothed = smooth_ink_points(&points);
                if !smoothed.is_empty() {
                    scene.ink.push((smoothed, alpha));
                }
            }
        }
        if ink.glow {
            scene.glow = start_glow(now_ms as f32);
        }
        scene
    }

    /// Nothing shows, and nothing will until new input.
    fn is_empty(&self) -> bool {
        self.ink.is_empty() && self.laser.is_empty() && self.tip.is_none() && self.glow <= 0.0
    }
}

/// A lifted ink stroke's opacity `age_ms` after it lifted.
fn ink_alpha(age_ms: u64) -> f32 {
    if age_ms <= INK_HOLD_MS {
        return 1.0;
    }
    let t = ((age_ms - INK_HOLD_MS) as f32 / INK_FADE_MS as f32).clamp(0.0, 1.0);
    (1.0 - t) * (1.0 - t)
}

/// The start glow's strength `ms` into the capture.
fn start_glow(ms: f32) -> f32 {
    let smooth = |a: f32, b: f32, x: f32| {
        let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    smooth(0.0, GLOW_RISE_MS, ms) * (1.0 - smooth(GLOW_FALL_MS[0], GLOW_FALL_MS[1], ms))
}

// ─── The overlay ─────────────────────────────────────────────────────────────

/// The ink overlay's state.
pub(crate) struct InkState {
    source: Option<InkSource>,
    scene: Scene,
    /// The target the overlay covers, in global points.
    placed: Option<[f32; 4]>,
    overlay: Option<OverlayHandle<InkState>>,
    shown: bool,
    /// The overlay takes the pointer to draw.
    drawing: bool,
}

impl InkState {
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn scene(&self) -> &Scene {
        &self.scene
    }

    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn set_scene(&mut self, scene: Scene) {
        self.scene = scene;
    }

    pub(crate) fn still(scene: Scene) -> Self {
        Self {
            source: None,
            scene,
            placed: None,
            overlay: None,
            shown: true,
            drawing: false,
        }
    }
}

fn poll(s: &mut InkState, _now: f64, tx: &mut Tx) -> bool {
    if s.overlay.is_none() {
        return false;
    }
    s.source = crate::capture_host::ink_source();
    let Some(source) = &s.source else {
        if s.shown {
            s.shown = false;
            s.placed = None;
            s.scene = Scene::default();
            if let Some(overlay) = &s.overlay {
                overlay.hide();
            }
            return true;
        }
        return false;
    };
    let telemetry = &source.telemetry;
    let ink = telemetry.ink_snapshot();
    let tip = telemetry.shift_held.load(Ordering::Acquire).then(|| {
        *telemetry
            .pointer_screen
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    });
    #[cfg(target_os = "linux")]
    let mut scene = Scene::at(&ink, source.now_ms(), tip);
    #[cfg(not(target_os = "linux"))]
    let scene = Scene::at(&ink, source.now_ms(), tip);
    // Linux learns the screen's size from the first stroke, so the start
    // glow would be drawn at a guess.
    #[cfg(target_os = "linux")]
    {
        scene.glow = 0.0;
    }
    let frame = ink.target;
    let rect = [frame.x as f32, frame.y as f32, scene.size[0], scene.size[1]];
    let drawing = crate::capture_host::drawing();
    if drawing != s.drawing {
        s.drawing = drawing;
        if let Some(overlay) = &s.overlay {
            overlay.set_input(if !drawing {
                rune::OverlayInput::None
            } else if cfg!(target_os = "linux") {
                // Everything but the capsule's pill (`draw_surface`).
                rune::OverlayInput::Regions
            } else {
                rune::OverlayInput::All
            });
            // For Escape, where the desktop gives the overlay the keyboard.
            if drawing {
                overlay.request_keys();
            }
        }
        if let Some(changed) = ON_DRAWING.get() {
            changed(drawing);
        }
    }
    if let Some(overlay) = &s.overlay {
        // Linux keeps the whole screen: Wayland says nothing of where the
        // recorded screen is.
        if cfg!(not(target_os = "linux")) && s.placed != Some(rect) {
            s.placed = Some(rect);
            overlay.set_placement(Some(rune::OverlayPlacement::Rect {
                x: rect[0],
                y: rect[1],
                width: rect[2],
                height: rect[3],
            }));
            overlay.resize((rect[2], rect[3]));
        }
        if !s.shown {
            s.shown = true;
            overlay.show();
        }
    }
    tx.wake_after(FRAME);
    let changed = scene != s.scene;
    if changed {
        // The scene already holds every fade and tail at this instant;
        // springs on top would only make the ink lag the pointer.
        tx.instant();
    }
    s.scene = scene;
    changed
}

/// Told when drawing starts or stops (the capsule lights its pen).
static ON_DRAWING: std::sync::OnceLock<Box<dyn Fn(bool) + Send + Sync>> =
    std::sync::OnceLock::new();

/// Calls `changed` whenever drawing starts or stops; set once.
pub(crate) fn on_drawing(changed: impl Fn(bool) + Send + Sync + 'static) {
    let _ = ON_DRAWING.set(Box::new(changed));
}

/// The pen the pointer becomes while Annotate draws, its tip the hot spot.
fn pen_cursor() -> rune::Cursor {
    rune::Cursor::Image(rune::CursorImage::png(
        include_bytes!("../../../assets/icons/annotate-pen-cursor@2x.png"),
        [3.5, 28.5],
    ))
}

/// The ink overlay, as the app holds it.
#[derive(Clone)]
pub(crate) struct InkOverlay {
    overlay: OverlayHandle<InkState>,
}

impl InkOverlay {
    /// Adds the overlay, prewarmed and hidden; it shows over a capture's
    /// target once its engine records.
    pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) -> Self {
        use rune::{
            DisplayTarget, OverlayInput, OverlayLevel, OverlayOptions, OverlayPlacement,
            OverlaySpec,
        };
        let spec = OverlaySpec::new("Hex ink", (1440.0, 900.0), view)
            .options(OverlayOptions {
                placement: Some(OverlayPlacement::FullScreen),
                display: DisplayTarget::Primary,
                level: OverlayLevel::AboveStatus,
                exclude_from_capture: !crate::capture_host::own_windows_recorded(),
                input: OverlayInput::None,
                prewarm: true,
                // Escape while drawing, asked for only then.
                keys: rune::KeyPolicy::OnDemand,
                ..Default::default()
            })
            .poll(poll)
            .on_created(|c| {
                if let Some(window) = c.window_number {
                    crate::capture_host::exclude_window(
                        crate::capture_host::OwnWindow::Ink,
                        window,
                    );
                }
            });
        let mut state = InkState::still(Scene::default());
        state.shown = false;
        let overlay = app.overlay(spec, state);
        let handle = overlay.clone();
        overlay.update(move |s, _| s.overlay = Some(handle));
        Self { overlay }
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    /// Lets screen recordings see the ink, or keeps it out.
    pub(crate) fn show_in_recordings(&self, shown: bool) {
        self.overlay.set_exclude_from_capture(!shown);
    }

    /// Looks for a recording capture's strokes now; it keeps looking while
    /// one records. Safe from any thread.
    pub(crate) fn wake(&self) {
        self.overlay.update(|_, tx| tx.wake_after(Duration::ZERO));
    }
}

// ─── View ────────────────────────────────────────────────────────────────────

pub(crate) fn view(s: &InkState, cx: &mut rune::Cx<InkState>) -> El {
    let scene = &s.scene;
    let [w, h] = scene.size;
    #[cfg(not(target_os = "linux"))]
    let mut root = {
        let _ = cx;
        let root = el().size(w.max(1.0), h.max(1.0));
        if s.drawing {
            root.cursor(pen_cursor())
                .hit()
                .focusable(true)
                .on_key(escape)
        } else {
            root
        }
    };
    #[cfg(target_os = "linux")]
    let mut root = draw_surface(s.drawing, cx.env().scale);
    if scene.is_empty() {
        return root;
    }
    if scene.glow > 0.0 {
        let edge = Shape::new().stroke_with(
            vec![[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]],
            true,
            20.0,
            LineCap::Butt,
            ink_red(),
        );
        root = root.child(
            canvas(edge)
                .absolute(0.0, 0.0)
                .size(w, h)
                .blur(14.0)
                .opacity(0.45 * scene.glow),
        );
    }
    // Each stroke fades as one, so its overlapping segments don't darken.
    for (points, alpha) in &scene.ink {
        let segments = points.windows(2).map(|pair| {
            let width = |p: f32| 4.5 * p.clamp(0.55, 1.15);
            (
                pair[0].0,
                pair[1].0,
                (width(pair[0].1) + width(pair[1].1)) * 0.5,
            )
        });
        let dot = (points.len() == 1).then(|| (points[0].0, points[0].0, 4.5 * points[0].1));
        if let Some(layer) = layer(segments.chain(dot), ink_red(), 2.0) {
            root = root.child(layer.opacity(*alpha));
        }
    }
    // The laser: a soft glow, its ruby body, and a white-hot core, each
    // tapering with the tail's weight.
    let laser = |radius: fn(f32) -> f32| {
        scene.laser.iter().flat_map(move |tail| {
            tail.windows(2).filter_map(move |pair| {
                let weight = (pair[0].1 + pair[1].1) * 0.5;
                (weight > 0.002).then(|| (pair[0].0, pair[1].0, 2.0 * radius(weight.min(1.25))))
            })
        })
    };
    if let Some(glow) = layer(laser(|w| 6.2 * (0.28 + 0.72 * w)), laser_red(), 12.0) {
        root = root.child(glow.blur(5.0).opacity(0.42));
    }
    if let Some(body) = layer(laser(|w| 2.45 * w.powf(0.68)), laser_red(), 2.0) {
        root = root.child(body.opacity(0.88));
    }
    if let Some(core) = layer(laser(|w| 0.9 * w.powf(0.82)), laser_hot(), 2.0) {
        root = root.child(core.opacity(0.95));
    }
    if let Some([x, y]) = scene.tip {
        root = root.child(
            el().absolute(x - 2.0, y - 2.0)
                .size(4.0, 4.0)
                .radius(2.0)
                .fill(ink_red()),
        );
    }
    root
}

/// Escape on the overlay, where the desktop gives it the keyboard, stops
/// drawing, never the recording.
fn escape(_: &mut InkState, key: &rune::KeyEvent, _: &mut Tx) -> bool {
    key.key == rune::Key::Escape && crate::capture_host::exit_drawing()
}

/// The whole screen. While Annotate draws, every pointer move draws and
/// the screen takes the pointer, except a hole over the capsule's pill (top
/// centre, where the capsule's panel sits), so the pen can be clicked off.
/// `scale` is the screen's pixels per point.
#[cfg(target_os = "linux")]
fn draw_surface(drawing: bool, scale: f32) -> El {
    use super::surface::PANEL_W;
    use rune::{col, row};
    let root = el().w_full().h_full();
    if !drawing {
        return root;
    }
    let taken = || el().hit().cursor(pen_cursor());
    let [px, py, pw, ph] = crate::capture_host::capsule_pill().unwrap_or([0.0; 4]);
    // The pill's offset from the panel's centre, which is the screen's.
    let off = px + pw * 0.5 - PANEL_W * 0.5;
    let top = super::PANEL_MARGIN + py;
    let regions = col()
        .w_full()
        .h_full()
        .child(taken().w_full().h(top.max(0.0)))
        .child(
            row()
                .w_full()
                .h(ph)
                .child(taken().grow(1.0).h_full())
                .child(taken().w((2.0 * off).max(0.0)).h_full())
                .child(el().w(pw).h_full())
                .child(taken().w((-2.0 * off).max(0.0)).h_full())
                .child(taken().grow(1.0).h_full()),
        )
        .child(taken().w_full().grow(1.0));
    root.focusable(true)
        .on_key(escape)
        .on_pointer_move_local(
            move |_: &mut InkState, p: rune::PointerEvent, tx: &mut Tx| {
                crate::capture_host::draw(p.x, p.y, scale);
                tx.wake_after(Duration::ZERO);
            },
        )
        .child(regions)
}

/// One canvas holding `segments` (from, to, width), cropped to their bounds
/// plus `pad` so a blur or opacity layer covers only what it draws.
fn layer(
    segments: impl Iterator<Item = ([f32; 2], [f32; 2], f32)>,
    color: Color,
    pad: f32,
) -> Option<El> {
    let segments: Vec<_> = segments.collect();
    let reach = segments.iter().map(|s| s.2).fold(0.0_f32, f32::max) * 0.5 + pad;
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    for (a, b, _) in &segments {
        for p in [a, b] {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
    }
    if segments.is_empty() {
        return None;
    }
    let origin = [lo[0] - reach, lo[1] - reach];
    let local = |p: [f32; 2]| [p[0] - origin[0], p[1] - origin[1]];
    let shape = segments.iter().fold(Shape::new(), |shape, &(a, b, width)| {
        if a == b {
            shape.line(
                local(a),
                [local(a)[0] + 0.01, local(a)[1]],
                width,
                LineCap::Round,
                color,
            )
        } else {
            shape.line(local(a), local(b), width, LineCap::Round, color)
        }
    });
    Some(
        canvas(shape)
            .absolute(origin[0], origin[1])
            .size(hi[0] - lo[0] + 2.0 * reach, hi[1] - lo[1] + 2.0 * reach),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use scrawl_capture::model::{SCALE, Stroke, TargetFrame};

    use super::*;

    pub(crate) fn frame() -> TargetFrame {
        TargetFrame {
            x: 0.0,
            y: 0.0,
            width: 560.0,
            height: 320.0,
            scale: 2.0,
        }
    }

    /// A stroke through `points` (0…1), `ms` apart, starting at `t0`.
    pub(crate) fn stroke(points: &[[f32; 2]], t0: u64, ms: u64, laser: bool) -> Stroke {
        let mut pts = Vec::new();
        for (k, [x, y]) in points.iter().enumerate() {
            pts.extend([
                (x * SCALE as f32) as i32,
                (y * SCALE as f32) as i32,
                (k as u64 * ms) as i32,
            ]);
        }
        Stroke {
            t0,
            t1: t0 + ms * (points.len() as u64 - 1),
            fw: 560,
            fh: 320,
            on: None,
            pin: None,
            laser,
            pts,
        }
    }

    /// A scribble along a sine, `n` points from `from` to `to`.
    pub(crate) fn wave(from: [f32; 2], to: [f32; 2], n: usize, amp: f32) -> Vec<[f32; 2]> {
        (0..n)
            .map(|k| {
                let t = k as f32 / (n - 1) as f32;
                [
                    from[0] + (to[0] - from[0]) * t,
                    from[1] + (to[1] - from[1]) * t + amp * (t * std::f32::consts::TAU).sin(),
                ]
            })
            .collect()
    }

    fn snapshot(strokes: Vec<Stroke>, laser: Option<Stroke>, glow: bool) -> InkSnapshot {
        InkSnapshot {
            target: frame(),
            strokes,
            live: None,
            laser,
            glow,
        }
    }

    #[test]
    fn lifted_ink_holds_then_fades_away() {
        let ink = snapshot(
            vec![stroke(&wave([0.1, 0.5], [0.6, 0.5], 24, 0.1), 0, 10, false)],
            None,
            false,
        );
        let lifted = 230;
        let alpha = |ms| Scene::at(&ink, ms, None).ink.first().map_or(0.0, |s| s.1);
        assert_eq!(alpha(lifted + 600), 1.0);
        let fading = alpha(lifted + 980);
        assert!(fading > 0.1 && fading < 0.5, "{fading}");
        assert!(Scene::at(&ink, lifted + 1_300, None).is_empty());
        // Fading never steps: each 60 Hz frame changes it by little.
        let mut last = 1.0;
        for ms in (lifted..lifted + 1_400).step_by(16) {
            let now = alpha(ms);
            assert!(last - now < 0.06, "ink fades {last} → {now} at {ms} ms");
            last = now;
        }
    }

    #[test]
    fn the_laser_trails_a_tail_and_leaves_nothing() {
        let laser = stroke(&wave([0.1, 0.3], [0.9, 0.6], 40, 0.05), 100, 12, true);
        let end = laser.t1;
        let ink = snapshot(vec![], Some(laser), false);
        let tail = Scene::at(&ink, end, None);
        assert_eq!(tail.laser.len(), 1);
        let points = &tail.laser[0];
        assert!(
            points.first().unwrap().1 < points.last().unwrap().1,
            "the tip is heaviest"
        );
        assert!(Scene::at(&ink, end + TAIL_MS + 1, None).laser.is_empty());
    }

    #[test]
    fn a_full_screen_start_glows_briefly() {
        let ink = snapshot(vec![], None, true);
        assert_eq!(Scene::at(&ink, 0, None).glow, 0.0);
        assert!(Scene::at(&ink, 200, None).glow > 0.9);
        assert!(Scene::at(&ink, 1_100, None).is_empty());
    }

    #[test]
    fn the_tip_follows_the_pointer_in_the_target() {
        let mut ink = snapshot(vec![], None, false);
        ink.target.x = 100.0;
        ink.target.y = 50.0;
        assert_eq!(
            Scene::at(&ink, 0, Some([300.0, 250.0])).tip,
            Some([200.0, 200.0])
        );
    }
}

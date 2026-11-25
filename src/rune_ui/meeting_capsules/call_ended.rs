//! The capsule that says a recorded call ended (#835). A recording with an
//! automatic stop counts down [`AUTO_STOP_GRACE`] unless kept; one without
//! keeps going until Stop.
//!
//! The call app's icon sits inside a concentric rounded-rectangle drain ring
//! (`stroke_radius = ICON_RADIUS + RING_GAP + RING_STROKE / 2`). The drain is
//! a render-time [`Tween`] (`live_trim`), so it moves every frame with no view
//! rebuilds; only the seconds count rebuilds, once a second. It fades over its
//! last moment, so nothing pops when it ends. Under Reduce Motion the drain
//! steps once a second with the count and leaves out the bright head.

use std::f32::consts::{FRAC_PI_2, PI};
use std::time::Duration;

use rune::color::{srgb_decode, srgb_encode};
use rune::{Color, Cursor, El, LineCap, Live, Shape, Tween, canvas, col, el, row, text};

use super::super::controls::{CAPSULE, Paint};
use super::{ACTIONS_INSET, CAPSULE_INSET, Capsule, button, panel_radius, presence};
use crate::calendar::CallProvider;
use crate::desktop::i18n::{tr, tr_fill};
use crate::meeting::detection::AUTO_STOP_GRACE;

/// The call-ended capsule's window width, room for its lines beside its
/// buttons.
pub(super) const PANEL_WIDTH: f32 = 460.0;

/// The call app's icon (or rounded-rect fallback tile) inside the ring.
pub(super) const ICON_SIZE: f32 = 26.0;
/// Matches the capsule buttons' corner radius (`p.k.radius.md`) and macOS app
/// icons' rounded-rect silhouette.
const ICON_RADIUS: f32 = CAPSULE.inner(CAPSULE.button_inset);
/// Fallback provider glyph size inside the `ICON_SIZE` tile.
const GLYPH_SIZE: f32 = 15.0;
/// Uniform clear gap between the icon's outer edge and the ring's inner edge.
const RING_GAP: f32 = 3.0;
/// Stroke width of the draining rounded-rectangle ring.
const RING_STROKE: f32 = 2.0;
/// Outer bounding box of the ring: icon + gap on both sides + stroke on both sides.
const RING_SIZE: f32 = ICON_SIZE + 2.0 * RING_GAP + 2.0 * RING_STROKE;
/// Distance from the ring canvas edge to the stroke's centreline.
const STROKE_INSET: f32 = RING_STROKE * 0.5;
/// Centreline corner radius, concentric with the icon across `RING_GAP`.
const STROKE_RADIUS: f32 = ICON_RADIUS + RING_GAP + STROKE_INSET;
/// Left padding inside the 56 pt body: matches top/bottom `(56 - 36) / 2 = 10`.
const BODY_PL: f32 = CAPSULE.row_px;
/// Points sampled along each 90° rounded corner.
const CORNER_STEPS: usize = 20;

/// The drain fades out over its last this long.
const FADE_S: f64 = 0.18;
/// How much of the drain the bright head covers.
const HEAD: f32 = 0.02;
/// How strongly the track shows under the drain.
const TRACK: f32 = 0.12;

/// A recording's automatic stop, drawn as it drains.
#[derive(Clone)]
pub(super) struct Countdown {
    line: Tween<[f32; 2]>,
    head: Tween<[f32; 2]>,
    fade: Tween<f32>,
}

impl Countdown {
    /// The drain with `left` of the grace to go.
    pub(super) fn new(left: Duration) -> Self {
        let past = past(left);
        let left = left.as_secs_f64().min(AUTO_STOP_GRACE.as_secs_f64());
        let over = Duration::from_secs_f64(left);
        // Opacity holds at 1 (clamped) until the last FADE_S, then fades.
        let full = (left / FADE_S) as f32;
        Self {
            line: Tween::linear([past, 1.0], [1.0, 1.0], over),
            head: Tween::linear([past, past + HEAD], [1.0, 1.0 + HEAD], over),
            fade: Tween::linear(full, 0.0, over),
        }
    }
}

/// How far the drain's moving end has gone with `left` of the grace to go.
fn past(left: Duration) -> f32 {
    let grace = AUTO_STOP_GRACE.as_secs_f64();
    (1.0 - left.as_secs_f64().min(grace) / grace) as f32
}

/// The seconds a countdown shows with `left` to go: never 0 while it shows.
pub(super) fn seconds(left: Duration) -> u32 {
    (left.as_secs_f64().ceil() as u32).max(1)
}

/// What the capsule shows: the call app, its loaded icon if any, and how
/// long until the recording stops on its own (`None` when it doesn't).
pub(super) struct Content<'a> {
    pub(super) app: &'a str,
    pub(super) icon: Option<&'a rune::Pixels>,
    pub(super) countdown: Option<(&'a Countdown, Duration)>,
}

/// The call-ended capsule in its window.
pub(super) fn view(
    p: Paint,
    capsule: &Capsule,
    content: Content<'_>,
    reduced: bool,
    keep: impl Fn(&mut Capsule) + 'static,
    stop: impl Fn(&mut Capsule) + 'static,
) -> El {
    let c = &p.k.c;
    let title = tr_fill("{} call ended", call_name(content.app));
    let (status, status_font) = match content.countdown {
        Some((_, left)) => (
            tr_fill("Recording stops in {} s", &seconds(left).to_string()),
            p.tabular(p.meta()),
        ),
        None => (tr("Hex is still recording.").to_owned(), p.meta()),
    };
    let stop_label = if content.countdown.is_some() {
        tr("Stop now")
    } else {
        tr("Stop & transcribe")
    };
    let actions = row()
        .key("actions")
        .gap(8.0)
        .child(button(
            p,
            "keep-recording",
            tr("Keep recording"),
            false,
            keep,
        ))
        .child(button(p, "stop-meeting", stop_label, true, stop));
    let lines = col()
        .key("lines")
        .grow(1.0)
        .min_w(0.0)
        .child(
            text(title.clone())
                .key("title")
                .truncate()
                .font(p.k.ty.ui_medium)
                .color(p.text())
                .a11y_hidden(),
        )
        .child(
            text(status)
                .key("subtitle")
                .truncate()
                .font(status_font)
                .color(p.faint()),
        );
    // With no countdown the icon keeps its place, without a ring.
    let mut badge = row().key("ring").size(RING_SIZE, RING_SIZE).center();
    if let Some((countdown, left)) = content.countdown {
        badge = badge
            .child(track(p))
            .child(drain(p, countdown, left, reduced));
    }
    badge = badge.child(app_icon(p, content.app, content.icon));
    let body = row()
        .key("pill")
        .role("status")
        .label(title)
        .w_full()
        .h_full()
        .center_y()
        .pl(BODY_PL)
        .pr(ACTIONS_INSET)
        .gap(12.0)
        .radius(panel_radius(p))
        .fill(c.bg_base)
        .border(p.hair, c.border_base)
        .child(badge)
        .child(lines)
        .child(actions)
        .hit()
        .cursor(Cursor::Grab)
        .on_drag(|c: &mut Capsule, drag, tx| c.drag(drag, tx));
    row()
        .key("capsule")
        .w_full()
        .h_full()
        .px(CAPSULE_INSET)
        .py(CAPSULE_INSET)
        .child(presence::anchored(capsule, body))
}

fn app_icon(p: Paint, app: &str, icon: Option<&rune::Pixels>) -> El {
    match icon {
        Some(pixels) => el()
            .key("icon")
            .size(ICON_SIZE, ICON_SIZE)
            .radius(ICON_RADIUS)
            .pixels_fit(pixels, rune::SurfaceFit::Fit)
            .a11y_hidden(),
        None => row()
            .key("icon")
            .size(ICON_SIZE, ICON_SIZE)
            .center()
            .radius(ICON_RADIUS)
            .fill(p.k.c.layer_02)
            .border(p.hair, p.k.c.border_weak)
            .a11y_hidden()
            .child(
                rune::icon(provider_icon(app), GLYPH_SIZE)
                    .key("glyph")
                    .color(p.muted()),
            ),
    }
}

/// "Teams" for "Microsoft Teams", "Slack" for a huddle.
fn call_name(app: &str) -> &str {
    match CallProvider::from_meeting_app(app) {
        Some(CallProvider::Teams) => "Teams",
        Some(provider) => provider.label(),
        None => app,
    }
}

fn provider_icon(app: &str) -> &'static rune::Icon {
    match CallProvider::from_meeting_app(app) {
        Some(provider) => super::super::platform_settings::provider_icon(provider),
        None => &super::super::icons::CALL,
    }
}

fn layer(shape: Shape) -> El {
    canvas(shape.view_box(RING_SIZE, RING_SIZE))
        .absolute(0.0, 0.0)
        .size(RING_SIZE, RING_SIZE)
}

/// The faint whole rounded-rect path under the drain.
fn track(p: Paint) -> El {
    let path = rounded_rect(RING_SIZE, STROKE_INSET, STROKE_RADIUS);
    let ink = tint(p.k.c.bg_base, p.text(), TRACK);
    layer(Shape::new().stroke_with(path, false, RING_STROKE, LineCap::Round, ink)).key("track")
}

/// The line from the drain's moving end to its finish, and its bright head.
/// Under Reduce Motion it steps once a second, with the count, and has no head.
fn drain(p: Paint, countdown: &Countdown, left: Duration, reduced: bool) -> El {
    let path = rounded_rect(RING_SIZE, STROKE_INSET, STROKE_RADIUS);
    let ground = p.k.c.bg_base;
    let stroke = |color| {
        layer(Shape::new().stroke_with(
            path.clone(),
            false,
            RING_STROKE,
            LineCap::Round,
            opaque(ground, color),
        ))
    };
    let drain = el()
        .key("drain")
        .absolute(0.0, 0.0)
        .size(RING_SIZE, RING_SIZE);
    if reduced {
        let at = past(Duration::from_secs_f64(seconds(left) as f64));
        return drain.child(
            stroke(p.muted())
                .key("line")
                .live_trim(&Live::new([at, 1.0])),
        );
    }
    drain
        .live_opacity(&countdown.fade)
        .child(stroke(p.muted()).key("line").live_trim(&countdown.line))
        .child(stroke(p.text()).key("head").live_trim(&countdown.head))
}

/// A `size` × `size` rounded rectangle's stroke centreline, `inset` in from
/// the canvas edge with corner radius `stroke_radius`, from the top centre
/// clockwise and back.
fn rounded_rect(size: f32, inset: f32, stroke_radius: f32) -> Vec<[f32; 2]> {
    let (x0, y0, x1, y1) = (inset, inset, size - inset, size - inset);
    let r = stroke_radius
        .min((x1 - x0) * 0.5)
        .min((y1 - y0) * 0.5)
        .max(0.01);
    let mut points = vec![[size * 0.5, y0]];
    let corners = [
        (x1 - r, y0 + r, -FRAC_PI_2),
        (x1 - r, y1 - r, 0.0),
        (x0 + r, y1 - r, FRAC_PI_2),
        (x0 + r, y0 + r, PI),
    ];
    for (cx, cy, start) in corners {
        for i in 0..=CORNER_STEPS {
            let a = start + FRAC_PI_2 * i as f32 / CORNER_STEPS as f32;
            let point = [cx + r * a.cos(), cy + r * a.sin()];
            if points.last() != Some(&point) {
                points.push(point);
            }
        }
    }
    points.push([size * 0.5, y0]);
    points
}

/// Flattens `ink` over `ground` into an opaque colour, so a stroked path
/// never shows darker at its own joints or round caps.
fn opaque(ground: Color, ink: Color) -> Color {
    tint(ground, ink, ink.a)
}

/// `ink` `amount` of the way over `ground`, as an alpha would show it on
/// screen, opaque, so a line never shows its own joints.
fn tint(ground: Color, ink: Color, amount: f32) -> Color {
    let m =
        |a: f32, b: f32| srgb_decode(srgb_encode(a) + (srgb_encode(b) - srgb_encode(a)) * amount);
    Color {
        r: m(ground.r, ink.r),
        g: m(ground.g, ink.g),
        b: m(ground.b, ink.b),
        a: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_never_shows_zero_and_rounds_up() {
        assert_eq!(seconds(Duration::from_millis(29_400)), 30);
        assert_eq!(seconds(Duration::from_secs(12)), 12);
        assert_eq!(seconds(Duration::ZERO), 1);
    }

    #[test]
    fn the_call_is_named_as_people_say_it() {
        assert_eq!(call_name("Microsoft Teams"), "Teams");
        assert_eq!(call_name("Slack Huddle"), "Slack");
        assert_eq!(call_name("Zoom"), "Zoom");
        assert_eq!(call_name("FaceTime"), "FaceTime");
    }

    #[test]
    fn the_drain_ring_is_concentric_with_the_icon_and_fits_its_box() {
        // Icon edge sits at RING_STROKE + RING_GAP = 5 pt from the ring box;
        // stroke inner edge sits at RING_STROKE = 2 pt, leaving RING_GAP = 3 pt
        // uniformly on every straight side and along every corner arc.
        let icon_corner_centre = RING_STROKE + RING_GAP + ICON_RADIUS;
        let stroke_corner_centre = STROKE_INSET + STROKE_RADIUS;
        assert!(
            (icon_corner_centre - stroke_corner_centre).abs() < 1e-4,
            "arc centres differ: icon {icon_corner_centre}, stroke {stroke_corner_centre}"
        );
        assert!(
            ((STROKE_RADIUS - STROKE_INSET) - (ICON_RADIUS + RING_GAP)).abs() < 1e-4,
            "inner stroke radius must equal icon_radius + gap"
        );

        let path = rounded_rect(RING_SIZE, STROKE_INSET, STROKE_RADIUS);
        assert_eq!(path.first(), Some(&[RING_SIZE * 0.5, STROKE_INSET]));
        assert_eq!(path.last(), Some(&[RING_SIZE * 0.5, STROKE_INSET]));
        assert!(
            path.iter().all(|[x, y]| {
                (STROKE_INSET - 0.01..=RING_SIZE - STROKE_INSET + 0.01).contains(x)
                    && (STROKE_INSET - 0.01..=RING_SIZE - STROKE_INSET + 0.01).contains(y)
            }),
            "every stroke point stays inside the inset ring box"
        );
    }
}

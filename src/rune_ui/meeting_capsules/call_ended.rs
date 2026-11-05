//! The capsule that says a recorded call ended (#835). A recording Hex
//! started on its own stops [`AUTO_STOP_GRACE`] later unless kept, and the
//! capsule counts that down; one the user started keeps going until Stop.
//!
//! Three designs to choose between, picked by `HEX_CALL_ENDED_VARIANT`
//! (`ring`, `bar`, or `outline`); only the chosen one stays.
//!
//! The drain is a render-time [`Tween`] (`live_trim`), so it moves every
//! frame with no view rebuilds; only the seconds count rebuilds, once a
//! second. It fades over its last moment, so nothing pops when it ends.
//! Under Reduce Motion the bright head that rides its moving end is left
//! out and the plain line drains alone.

use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::time::Duration;

use rune::color::{srgb_decode, srgb_encode};
use rune::{Color, El, LineCap, Live, Shape, Tween, canvas, col, el, row, text};
use rune_kit::{Button, ButtonVariant};

use super::super::controls::{Paint, PushSize};
use super::ACTIONS_INSET;
use crate::calendar::CallProvider;
use crate::desktop::i18n::{tr, tr_fill};
use crate::meeting::detection::AUTO_STOP_GRACE;

/// Between the capsule's window and its body, all round.
const MARGIN: f32 = 4.0;
/// The drain fades out over its last this long.
const FADE_S: f64 = 0.18;
/// How much of the drain the bright head covers.
const HEAD: f32 = 0.02;
/// How strongly the track shows under the drain.
const TRACK: f32 = 0.12;

/// The ring around the call app's icon.
const RING: f32 = 36.0;
const RING_STROKE: f32 = 2.0;
/// The bar along the capsule's foot.
const BAR_STROKE: f32 = 2.0;
/// The bar and the icon in from the body's left edge.
const BAR_INSET: f32 = 16.0;
/// The bar's centre, up from the body's bottom edge.
const BAR_RISE: f32 = 4.0;
/// The capsule's own outline.
const OUTLINE_STROKE: f32 = 1.5;
const CORNER_STEPS: usize = 16;

/// The design the call-ended capsule takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Variant {
    /// The call app's icon in a ring that drains around it.
    Ring,
    /// A thin bar along the capsule's foot that drains to the left.
    Bar,
    /// One line, with the capsule's own outline draining clockwise.
    Outline,
}

impl Variant {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 3] = [Self::Ring, Self::Bar, Self::Outline];

    /// Temporary, until one design is chosen: `HEX_CALL_ENDED_VARIANT`.
    pub(crate) fn current() -> Self {
        match std::env::var("HEX_CALL_ENDED_VARIANT").as_deref() {
            Ok("bar") => Self::Bar,
            Ok("outline") => Self::Outline,
            _ => Self::Ring,
        }
    }

    /// The capsule's window.
    pub(crate) fn size(self) -> (f32, f32) {
        match self {
            Self::Ring | Self::Bar => (460.0, 64.0),
            Self::Outline => (540.0, 48.0),
        }
    }

    fn body(self) -> [f32; 2] {
        let (w, h) = self.size();
        [w - 2.0 * MARGIN, h - 2.0 * MARGIN]
    }

    fn radius(self, p: Paint) -> f32 {
        match self {
            Self::Ring | Self::Bar => p.k.radius.md + ACTIONS_INSET,
            Self::Outline => p.k.radius.sm + ACTIONS_INSET,
        }
    }

    fn stroke(self) -> f32 {
        match self {
            Self::Ring => RING_STROKE,
            Self::Bar => BAR_STROKE,
            Self::Outline => OUTLINE_STROKE,
        }
    }

    /// The drain's path, `[w, h]` big, in the order it gives way: from the
    /// top clockwise round the ring and the outline, from the right along
    /// the bar.
    fn path(self, p: Paint) -> ([f32; 2], Vec<[f32; 2]>) {
        let [w, h] = self.body();
        match self {
            Self::Ring => ([RING, RING], circle(RING * 0.5, (RING - RING_STROKE) * 0.5)),
            Self::Bar => {
                let (bw, y) = (w - BAR_INSET - ACTIONS_INSET, BAR_STROKE);
                (
                    [bw, 2.0 * BAR_STROKE],
                    vec![[bw - BAR_STROKE * 0.5, y], [BAR_STROKE * 0.5, y]],
                )
            }
            Self::Outline => ([w, h], outline(w, h, self.radius(p), OUTLINE_STROKE * 0.5)),
        }
    }
}

/// A recording's automatic stop, drawn as it drains.
#[derive(Clone)]
pub(crate) struct Countdown {
    line: Tween<[f32; 2]>,
    head: Tween<[f32; 2]>,
    fade: Tween<f32>,
}

impl Countdown {
    /// The drain with `left` of the grace to go.
    pub(crate) fn new(left: Duration) -> Self {
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

/// What the capsule says: the call app, and how long until the recording
/// stops on its own (`None` when it doesn't).
pub(crate) struct Content<'a> {
    pub(crate) app: &'a str,
    pub(crate) countdown: Option<(&'a Countdown, Duration)>,
}

/// The seconds a countdown shows with `left` to go: never 0 while it shows.
pub(crate) fn seconds(left: Duration) -> u32 {
    (left.as_secs_f64().ceil() as u32).max(1)
}

/// The capsule's body for `variant`. `keep` and `stop` answer its buttons.
pub(crate) fn body<S: 'static>(
    p: Paint,
    variant: Variant,
    content: Content,
    reduced: bool,
    keep: impl Fn(&mut S) + 'static,
    stop: impl Fn(&mut S) + 'static,
) -> El {
    let c = &p.k.c;
    let title = tr_fill("{} call ended", call_name(content.app));
    let status = match content.countdown {
        Some((_, left)) => tr_fill("Recording stops in {} s", &seconds(left).to_string()),
        None => tr("Hex is still recording.").to_owned(),
    };
    let compact = variant == Variant::Outline;
    let actions = row()
        .key("actions")
        .gap(if compact { 6.0 } else { 8.0 })
        .child(action(
            p,
            "keep-recording",
            tr("Keep recording"),
            false,
            compact,
            keep,
        ))
        .child(action(
            p,
            "stop-meeting",
            if content.countdown.is_some() {
                tr("Stop now")
            } else {
                tr("Stop & transcribe")
            },
            true,
            compact,
            stop,
        ));
    let title_text = text(title.clone())
        .key("title")
        .truncate()
        .font(p.k.ty.ui_medium)
        .color(p.text())
        .a11y_hidden();
    let status_text = text(status)
        .key("subtitle")
        .truncate()
        .font(p.k.ty.small.features("'tnum' 1"))
        .color(p.faint());
    let icon = rune::icon(
        provider_icon(content.app),
        if compact { 16.0 } else { 18.0 },
    )
    .key("icon")
    .color(p.muted())
    .a11y_hidden();
    let drain = content
        .countdown
        .map(|(countdown, left)| drain(p, variant, countdown, left, reduced));
    let body = row()
        .key("pill")
        .role("status")
        .label(title)
        .w_full()
        .h_full()
        .center_y()
        .radius(variant.radius(p))
        .fill(c.bg_base)
        .border(p.hair, c.border_base)
        .pr(ACTIONS_INSET);
    match variant {
        Variant::Ring => {
            // With no countdown the icon keeps its place, without a ring.
            let mut ring = row().key("ring").size(RING, RING).center();
            if let Some(drain) = drain {
                ring = ring.child(track(p, variant)).child(drain);
            }
            body.pl(10.0)
                .gap(12.0)
                .child(ring.child(icon))
                .child(lines(title_text, status_text))
                .child(actions)
        }
        Variant::Bar => {
            let [_, h] = variant.body();
            let ([bw, bh], _) = variant.path(p);
            let mut body = body
                .pl(BAR_INSET)
                .gap(12.0)
                // Clear of the bar.
                .pb(if drain.is_some() { BAR_RISE } else { 0.0 })
                .child(icon)
                .child(lines(title_text, status_text))
                .child(actions);
            if let Some(drain) = drain {
                // From the icon's left edge to the buttons' right edge.
                body = body.child(
                    el().key("bar")
                        .absolute(BAR_INSET, h - BAR_RISE - bh * 0.5 - p.hair)
                        .size(bw, bh)
                        .child(track(p, variant))
                        .child(drain),
                );
            }
            body
        }
        Variant::Outline => {
            let [w, h] = variant.body();
            let mut body = body
                .pl(14.0)
                .gap(8.0)
                .child(icon)
                .child(title_text.shrink())
                .child(status_text.grow(1.0).shrink())
                .child(actions);
            if let Some(drain) = drain {
                body = body.child(
                    el().key("outline")
                        .absolute(-p.hair, -p.hair)
                        .size(w, h)
                        .child(drain),
                );
            }
            body
        }
    }
}

fn lines(title: El, status: El) -> El {
    col()
        .key("lines")
        .grow(1.0)
        .min_w(0.0)
        .child(title)
        .child(status)
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

fn action<S: 'static>(
    p: Paint,
    key: &str,
    label: &str,
    primary: bool,
    compact: bool,
    press: impl Fn(&mut S) + 'static,
) -> El {
    let button = Button::new(label)
        .variant(if primary {
            ButtonVariant::Contrast
        } else {
            ButtonVariant::Neutral
        })
        .on_press(press);
    if compact {
        button.build_small_push(p.k)
    } else {
        button.build_push(p.k)
    }
    .key(key)
}

fn layer(size: [f32; 2], shape: Shape) -> El {
    let [w, h] = size;
    canvas(shape.view_box(w, h)).absolute(0.0, 0.0).size(w, h)
}

/// The faint whole path under the drain. The outline's is the border.
fn track(p: Paint, variant: Variant) -> El {
    let (size, path) = variant.path(p);
    let ink = tint(p.k.c.bg_base, p.text(), TRACK);
    layer(
        size,
        Shape::new().stroke_with(path, false, variant.stroke(), LineCap::Round, ink),
    )
    .key("track")
}

/// The line from the drain's moving end to its finish, and its head. Under
/// Reduce Motion it steps once a second, with the count, and has no head.
fn drain(p: Paint, variant: Variant, countdown: &Countdown, left: Duration, reduced: bool) -> El {
    let (size, path) = variant.path(p);
    let stroke = |color| {
        layer(
            size,
            Shape::new().stroke_with(path.clone(), false, variant.stroke(), LineCap::Round, color),
        )
    };
    let drain = el().key("drain").absolute(0.0, 0.0).size(size[0], size[1]);
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

/// A circle `r` round the centre of a `2c` square, from the top clockwise.
fn circle(c: f32, r: f32) -> Vec<[f32; 2]> {
    const STEPS: usize = 96;
    (0..=STEPS)
        .map(|i| {
            let a = -FRAC_PI_2 + TAU * i as f32 / STEPS as f32;
            [c + r * a.cos(), c + r * a.sin()]
        })
        .collect()
}

/// A `w` × `h` rounded rectangle's outline `inset` in from its edge, from
/// the top centre clockwise and back.
fn outline(w: f32, h: f32, radius: f32, inset: f32) -> Vec<[f32; 2]> {
    let (x0, y0, x1, y1) = (inset, inset, w - inset, h - inset);
    let r = (radius - inset)
        .min((y1 - y0) * 0.5)
        .min((x1 - x0) * 0.5)
        .max(0.01);
    let mut points = vec![[w * 0.5, y0]];
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
    points.push([w * 0.5, y0]);
    points
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

/// The capsule on its own, for `--snap` scenes and the playground: `left`
/// of the grace to go on the scene's clock, or no countdown.
pub(crate) struct Preview {
    pub(crate) variant: Variant,
    app: String,
    countdown: Option<(Countdown, f64)>,
    /// The scene's clock when it first polled, and now.
    start: Option<f64>,
    now: f64,
}

impl Preview {
    pub(crate) fn new(variant: Variant, app: &str, left: Option<f64>) -> Self {
        Self {
            variant,
            app: app.to_owned(),
            countdown: left.map(|left| (Countdown::new(Duration::from_secs_f64(left)), left)),
            start: None,
            now: 0.0,
        }
    }

    fn left(&self) -> Option<f64> {
        self.countdown
            .as_ref()
            .map(|(_, left)| (left - self.now).max(0.0))
    }

    /// Follows the scene's clock; true when the seconds shown changed.
    pub(crate) fn poll(&mut self, clock: f64, tx: &mut rune::Tx) -> bool {
        let shown = |preview: &Self| preview.left().map(|l| seconds(Duration::from_secs_f64(l)));
        let before = shown(self);
        let start = *self.start.get_or_insert(clock);
        self.now = clock - start;
        if let Some(left) = self.left().filter(|left| *left > 1.0) {
            // Until the count next changes.
            tx.wake_after(Duration::from_secs_f64(
                (left - (left.ceil() - 1.0)).max(0.01),
            ));
        }
        shown(self) != before
    }

    /// The capsule window, centred on the scene's ground.
    pub(crate) fn view<S: 'static>(&self, p: Paint, reduced: bool) -> El {
        let (w, h) = self.variant.size();
        let countdown = self
            .countdown
            .as_ref()
            .zip(self.left())
            .map(|((countdown, _), left)| (countdown, Duration::from_secs_f64(left)));
        let body = body::<S>(
            p,
            self.variant,
            Content {
                app: &self.app,
                countdown,
            },
            reduced,
            |_| {},
            |_| {},
        )
        .hit();
        row()
            .key("call-ended-stage")
            .w_full()
            .h_full()
            .center()
            .fill(p.k.c.bg_deep)
            .child(row().key("capsule").size(w, h).padding(MARGIN).child(body))
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
    fn every_drain_path_fits_its_box() {
        let p = Paint::new(true, 2.0);
        for variant in Variant::ALL {
            let ([w, h], path) = variant.path(p);
            assert!(path.len() >= 2);
            assert!(
                path.iter()
                    .all(|[x, y]| (-0.01..=w + 0.01).contains(x) && (-0.01..=h + 0.01).contains(y)),
                "{variant:?}"
            );
        }
    }
}

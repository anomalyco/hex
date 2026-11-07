//! Three quieter takes on the finished capsule's states, for a pick: Saved
//! (copied, not sent), Queued (waiting for a session that can't open the
//! file yet), and the Send-to list. Each take draws its three states with
//! the capsule's own pieces, one line and a few words each; a reason that
//! needs more sits in the element's tooltip. A design prototype: nothing
//! here is wired to the flow.

use std::sync::{Arc, OnceLock};

use rune::{Color, Cx, El, LineCap, Path, Shape, Story, canvas, col, el, row};

use super::surface::{
    CHIP_H, CHIP_PX, CHIP_RADIUS, HEADER_H, ISLAND_H, ISLAND_INSET, LIST_PAD, LIST_Y, PANEL_W,
    ROW_H, SHELL_RADIUS, THUMB_H, THUMB_W, THUMB_X, TOP, medium, small,
};
use super::view::{BASELINE_DROP, THUMB_RADIUS, close_glyph, hint, label, mark_stroke};
use super::{HEADER_PB, Paint, ROW_PX, ROW_RADIUS, rounded};

/// The open capsule and its list share this width.
const OPEN_W: f32 = 340.0;
/// Under the capsule, as the goldens leave room for its shadow.
const BOTTOM: f32 = 36.0;
/// Between the capsule and the list under it.
const LIST_GAP: f32 = LIST_Y - TOP - ISLAND_H;
/// After the status words, before the action.
const ACTION_GAP: f32 = 10.0;
const ICON: f32 = 14.0;

/// Why a Queued capture waits, for the tooltip: the line is only "for SEER".
const WHY: &str = "SEER's OpenCode is on another Mac and can't open this file. Hex keeps trying.";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Take {
    /// Words alone: the status, then the action as a quiet word.
    Words,
    /// A status dot and one filled action.
    Button,
    /// The suggested session in the capsule: one click sends there.
    Destination,
}

pub(crate) struct Takes {
    take: Take,
    thumb: Option<rune::Image>,
}

pub(crate) fn add<S: 'static>(playground: rune::Playground<S>) -> rune::Playground<S> {
    playground.story(
        Story::new("Prototypes/Capture capsule/quiet states", init, view)
            .variant("words", |s: &mut Takes| s.take = Take::Words)
            .variant("button", |s: &mut Takes| s.take = Take::Button)
            .variant("destination", |s: &mut Takes| s.take = Take::Destination),
    )
}

fn init() -> Takes {
    Takes {
        take: Take::Words,
        thumb: thumb(),
    }
}

/// The goldens' keyframe (an OpenCode settings screen).
fn thumb() -> Option<rune::Image> {
    static IMAGES: OnceLock<rune::ImageCache> = OnceLock::new();
    let images = IMAGES.get_or_init(|| rune::ImageCache::new(Arc::new(|| {})));
    let image =
        images.load(include_bytes!("../../../tests/fixtures/capture-thumbnail.png").to_vec());
    image.wait().ok()?;
    while image.pixels().size() == (0, 0) {
        std::thread::yield_now();
    }
    Some(image)
}

fn view(s: &Takes, cx: &mut Cx<Takes>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let thumb = s.thumb.as_ref().map(|image| image.pixels());
    let (saved, queued, (open, list)) = match s.take {
        Take::Words => (
            words::saved(p, thumb),
            words::queued(p, thumb),
            words::open(p, thumb),
        ),
        Take::Button => (
            button::saved(p, thumb),
            button::queued(p, thumb),
            button::open(p, thumb),
        ),
        Take::Destination => (
            destination::saved(p, thumb),
            destination::queued(p, thumb),
            destination::open(p, thumb),
        ),
    };
    col()
        .key("takes")
        .w(PANEL_W)
        .fill(p.k.c.layer_03)
        .child(stage("qs-saved", saved, None))
        .child(stage("qs-queued", queued, None))
        .child(stage("qs-open", open, Some(list)))
}

/// One state on the backdrop, where the capsule rests in its panel.
fn stage(key: &str, line: El, list: Option<El>) -> El {
    col()
        .key(key)
        .w(PANEL_W)
        .items_center()
        .pt(TOP)
        .pb(BOTTOM)
        .gap(LIST_GAP)
        .child(line)
        .children(list)
}

// ─── Shared pieces ───────────────────────────────────────────────────────────

/// The capsule's walls, `w` wide or as wide as what's in them.
fn shell(p: Paint, w: Option<f32>) -> El {
    let k = p.k;
    let mut shell = row()
        .key("shell")
        .center_y()
        .h(ISLAND_H)
        .px(ISLAND_INSET)
        .radius(SHELL_RADIUS)
        .fill(k.c.bg_base);
    if let Some(w) = w {
        shell = shell.w(w);
    }
    for shadow in &k.elevation.overlay {
        shell = shell.box_shadow(*shadow);
    }
    rimmed(p, shell)
}

/// Dark, a hairline lit from above, as the capsule and its list wear.
fn rimmed(p: Paint, e: El) -> El {
    let k = p.k;
    if k.dark {
        e.border_gradient(0.5, k.light(0.12), k.light(0.03))
    } else {
        e
    }
}

/// The capture's first frame, where the real capsule draws it; its label
/// starts 8 pt after.
fn lead(p: Paint, thumb: Option<&rune::Pixels>) -> El {
    let mut card = el()
        .size(THUMB_W, THUMB_H)
        .radius(THUMB_RADIUS)
        .fill(p.k.c.layer_02);
    if let Some(pixels) = thumb {
        card = card.pixels_fit(pixels, rune::SurfaceFit::Fill);
    }
    let card = card.child(
        el().absolute(0.0, 0.0)
            .size(THUMB_W, THUMB_H)
            .radius(THUMB_RADIUS)
            .border(p.hair, p.k.alpha(p.k.c.text_base, 0.14)),
    );
    row()
        .key("thumb")
        .center_y()
        .h(CHIP_H)
        .pl(THUMB_X)
        .pr(8.0)
        .child(card)
}

fn close(p: Paint) -> El {
    row()
        .key("close")
        .center()
        .size(CHIP_H, CHIP_H)
        .radius(CHIP_RADIUS)
        .hover(p.k.c.hover)
        .press(p.k.c.pressed)
        .child(close_glyph(p))
        .role("button")
        .label("Close")
}

/// Words in `color`, then smaller ones on their baseline.
fn note(content: &str, color: Color) -> El {
    label(content, small(), color).offset(0.0, (medium().size - small().size) * BASELINE_DROP)
}

fn gap(w: f32) -> El {
    el().w(w)
}

fn spacer() -> El {
    el().grow(1.0)
}

/// The list's panel, `OPEN_W` wide, hugging its rows.
fn panel(p: Paint, body: Vec<El>) -> El {
    let k = p.k;
    let mut panel = col()
        .key("list")
        .w(OPEN_W)
        .px(LIST_PAD)
        .pt(LIST_PAD)
        .pb(LIST_PAD)
        .radius(SHELL_RADIUS)
        .fill(k.c.bg_base)
        .children(body);
    for shadow in &k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    rimmed(p, panel)
}

/// A section's quiet name over its rows, and what sits on its right.
fn header(p: Paint, title: &str, right: Option<El>) -> El {
    row()
        .key(format!("section-{title}"))
        .items_end()
        .h(HEADER_H)
        .px(ROW_PX)
        .pb(HEADER_PB)
        .child(label(title, small(), p.faint()))
        .child(spacer())
        .children(right)
}

/// A session's row: its name, and a dim reason when it has one.
fn entry(p: Paint, name: &str, why: Option<&str>) -> El {
    row()
        .key(format!("row-{name}"))
        .center_y()
        .h(ROW_H)
        .px(ROW_PX)
        .radius(ROW_RADIUS)
        .hover(p.k.c.hover)
        .press(p.k.c.pressed)
        .child(label(name, medium(), p.text()))
        .child(spacer())
        .children(why.map(|why| note(why, p.faint())))
        .role("button")
        .label(name.to_owned())
}

/// One of the capsule's line marks on its 24 × 24 grid.
fn mark(paths: &[Path], size: f32, color: Color) -> El {
    let width = mark_stroke(size) * 1.25;
    let shape = paths
        .iter()
        .fold(Shape::new().view_box(24.0, 24.0), |shape, path| {
            shape.stroke_path(path, width, LineCap::Round, color)
        });
    canvas(shape).size(size, size).a11y_hidden()
}

/// "Send to": an arrow.
fn arrow(color: Color) -> El {
    let line = |a: [f32; 2], b: [f32; 2]| Path::new().move_to(a).line_to(b);
    mark(
        &[
            line([4.5, 12.0], [19.0, 12.0]),
            Path::new()
                .move_to([13.0, 6.0])
                .line_to([19.0, 12.0])
                .line_to([13.0, 18.0]),
        ],
        ICON,
        color,
    )
}

/// "Waiting": a clock.
fn clock(color: Color) -> El {
    mark(
        &[
            rounded(3.0, 3.0, 18.0, 18.0, 9.0),
            Path::new()
                .move_to([12.0, 7.5])
                .line_to([12.0, 12.0])
                .line_to([15.0, 14.0]),
        ],
        ICON,
        color,
    )
}

fn chevron(p: Paint, open: bool) -> El {
    let glyph = if open {
        p.k.glyph.chevron_up
    } else {
        p.k.glyph.chevron_down
    };
    rune::icon(glyph, ICON).color(p.muted())
}

const RECENT: [&str; 3] = ["HEX", "HOME", "RUNE"];

// ─── A: words ────────────────────────────────────────────────────────────────

/// Words alone. The status in the muted ink, the action after it as a
/// word in the text's, and no icon but ✕. The list names its two kinds.
mod words {
    use super::*;

    /// The action: a word that lights on hover, as a menu item does.
    fn action(p: Paint, name: &str, open: bool) -> El {
        let mut chip = row()
            .key(format!("action-{name}"))
            .center_y()
            .h(CHIP_H)
            .px(CHIP_PX)
            .radius(CHIP_RADIUS)
            .hover(p.k.c.hover)
            .press(p.k.c.pressed)
            .child(label(name, medium(), p.text()))
            .role("button")
            .label(name.to_owned());
        if open {
            chip = chip.fill(p.k.c.layer_02);
        }
        chip
    }

    pub(super) fn saved(p: Paint, thumb: Option<&rune::Pixels>) -> El {
        shell(p, None)
            .child(lead(p, thumb))
            .child(label("Copied", medium(), p.muted()))
            .child(gap(ACTION_GAP))
            .child(action(p, "Send…", false))
            .child(close(p))
    }

    pub(super) fn queued(p: Paint, thumb: Option<&rune::Pixels>) -> El {
        let status = row()
            .key("status")
            .center_y()
            .gap(4.0)
            .child(label("Waiting for", medium(), p.muted()))
            .child(label("SEER", medium(), p.text()))
            .help(WHY);
        shell(p, None)
            .child(lead(p, thumb))
            .child(status)
            .child(gap(ACTION_GAP))
            .child(action(p, "Elsewhere…", false))
            .child(close(p))
    }

    pub(super) fn open(p: Paint, thumb: Option<&rune::Pixels>) -> (El, El) {
        let line = shell(p, Some(OPEN_W))
            .child(lead(p, thumb))
            .child(label("Copied", medium(), p.muted()))
            .child(spacer())
            .child(action(p, "Send…", true))
            .child(close(p));
        let shift = label("⇧ always send here", small(), p.faint()).key("shift");
        let mut body = vec![
            header(p, "Suggested", Some(shift)),
            entry(p, "SEER", Some("same page")),
            header(p, "Recent", None).h(HEADER_H + 4.0),
        ];
        body.extend(RECENT.iter().map(|name| entry(p, name, None)));
        (line, panel(p, body))
    }
}

// ─── B: one button ───────────────────────────────────────────────────────────

/// A status, then one filled action on the right. Queued wears a dot in
/// the warning's colour, so waiting reads before its word does. The list
/// leads with its one suggestion; the recent sessions sit under it as
/// chips.
mod button {
    use super::*;

    const H: f32 = 28.0;

    fn action(p: Paint, name: &str, open: bool) -> El {
        let k = p.k;
        row()
            .key(format!("action-{name}"))
            .center_y()
            .h(H)
            .px(CHIP_PX)
            .radius(CHIP_RADIUS - 2.0)
            .fill(if open { k.c.layer_03 } else { k.c.layer_02 })
            .hover(k.c.hover)
            .press(k.c.pressed)
            .child(label(name, medium(), p.text()))
            .role("button")
            .label(name.to_owned())
    }

    pub(super) fn saved(p: Paint, thumb: Option<&rune::Pixels>) -> El {
        shell(p, None)
            .child(lead(p, thumb))
            .child(label("Copied", medium(), p.text()))
            .child(gap(2.0 * ACTION_GAP))
            .child(action(p, "Send", false))
            .child(gap(2.0))
            .child(close(p))
    }

    pub(super) fn queued(p: Paint, thumb: Option<&rune::Pixels>) -> El {
        let status = row()
            .key("status")
            .center_y()
            .gap(6.0)
            .child(super::super::super::controls::status_dot(p.warning()))
            .child(label("SEER", medium(), p.text()))
            .child(note("queued", p.muted()))
            .help(WHY);
        shell(p, None)
            .child(lead(p, thumb))
            .child(status)
            .child(gap(2.0 * ACTION_GAP))
            .child(action(p, "Redirect", false))
            .child(gap(2.0))
            .child(close(p))
    }

    pub(super) fn open(p: Paint, thumb: Option<&rune::Pixels>) -> (El, El) {
        let k = p.k;
        let line = shell(p, Some(OPEN_W))
            .child(lead(p, thumb))
            .child(label("Copied", medium(), p.text()))
            .child(spacer())
            .child(action(p, "Send", true))
            .child(gap(2.0))
            .child(close(p));
        let best = row()
            .key("best")
            .center_y()
            .h(ROW_H + 6.0)
            .px(ROW_PX)
            .gap(8.0)
            .radius(ROW_RADIUS)
            .fill(k.c.layer_02)
            .hover(k.c.hover)
            .press(k.c.pressed)
            .child(label("SEER", medium(), p.text()))
            .child(note("same page", p.faint()))
            .child(spacer())
            .child(arrow(p.muted()))
            .role("button")
            .label("SEER");
        let chips = row()
            .key("recent")
            .center_y()
            .gap(6.0)
            .px(ROW_PX - 4.0)
            .children(RECENT.iter().map(|name| {
                row()
                    .key(format!("chip-{name}"))
                    .center_y()
                    .h(H)
                    .px(10.0)
                    .radius(ROW_RADIUS)
                    .border(p.hair, k.c.border_weak)
                    .hover(k.c.hover)
                    .press(k.c.pressed)
                    .child(label(name, medium(), p.text()))
                    .role("button")
                    .label(name.to_string())
            }));
        // ⇧ is explained beside the chips it applies to, on their line.
        let chips = chips
            .child(spacer())
            .child(hint(p, "Always", &["⇧"]).key("shift").pr(4.0));
        let body = vec![
            best,
            header(p, "Recent", None).h(HEADER_H + 6.0),
            chips,
            gap(0.0).h(4.0),
        ];
        (line, panel(p, body))
    }
}

// ─── C: the destination ──────────────────────────────────────────────────────

/// The capsule names where it would go: "Copied [→ SEER ⌄]" sends there in
/// one click, and its chevron opens the rest. Queued is the same chip with
/// a clock for the arrow, so the way out is where the eye already is. The
/// list's rule parts the suggestion from the recent sessions, and ⇧ is
/// explained on the row under the pointer.
mod destination {
    use super::*;

    fn split(p: Paint, icon: El, name: &str, open: bool, help: Option<&str>) -> El {
        let k = p.k;
        let mut send = row()
            .key("send")
            .center_y()
            .h(CHIP_H)
            .pl(10.0)
            .pr(10.0)
            .gap(6.0)
            .radius(CHIP_RADIUS)
            .hover(k.c.hover)
            .press(k.c.pressed)
            .child(icon)
            .child(label(name, medium(), p.text()))
            .role("button")
            .label(format!("Send to {name}"));
        if let Some(help) = help {
            send = send.help(help);
        }
        let more = row()
            .key("more")
            .center()
            .size(26.0, CHIP_H)
            .radius(CHIP_RADIUS)
            .hover(k.c.hover)
            .press(k.c.pressed)
            .child(chevron(p, open))
            .role("button")
            .label("Send elsewhere");
        row()
            .key("split")
            .center_y()
            .h(CHIP_H)
            .pr(2.0)
            .radius(CHIP_RADIUS)
            .fill(if open { k.c.layer_03 } else { k.c.layer_02 })
            .child(send)
            .child(el().size(1.0, 16.0).fill(k.c.border_weak))
            .child(more)
    }

    pub(super) fn saved(p: Paint, thumb: Option<&rune::Pixels>) -> El {
        shell(p, None)
            .child(lead(p, thumb))
            .child(label("Copied", medium(), p.muted()))
            .child(gap(ACTION_GAP + 2.0))
            .child(split(p, arrow(p.muted()), "SEER", false, None))
            .child(gap(2.0))
            .child(close(p))
    }

    pub(super) fn queued(p: Paint, thumb: Option<&rune::Pixels>) -> El {
        shell(p, None)
            .child(lead(p, thumb))
            .child(label("Queued", medium(), p.muted()))
            .child(gap(ACTION_GAP + 2.0))
            .child(split(p, clock(p.warning()), "SEER", false, Some(WHY)))
            .child(gap(2.0))
            .child(close(p))
    }

    pub(super) fn open(p: Paint, thumb: Option<&rune::Pixels>) -> (El, El) {
        let k = p.k;
        let line = shell(p, Some(OPEN_W))
            .child(lead(p, thumb))
            .child(label("Copied", medium(), p.muted()))
            .child(spacer())
            .child(split(p, arrow(p.muted()), "SEER", true, None))
            .child(gap(2.0))
            .child(close(p));
        let rule = row()
            .key("rule")
            .h(9.0)
            .px(ROW_PX)
            .center_y()
            .child(el().h(p.hair).grow(1.0).fill(k.c.border_weak));
        let mut body = vec![entry(p, "SEER", Some("same page")), rule];
        for name in RECENT {
            // Stills have no pointer: HEX stands for the row under it.
            body.push(if name == "HEX" {
                entry(p, name, None)
                    .fill(k.c.layer_02)
                    .child(note("⇧ always here", p.faint()))
            } else {
                entry(p, name, None)
            });
        }
        (line, panel(p, body))
    }
}

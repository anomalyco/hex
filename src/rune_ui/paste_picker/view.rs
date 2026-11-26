//! The picker's card: recent rows down a rail, each led by its picture or a
//! dictation's mark, and the selected row's preview beside them: a capture's
//! picture over its words, or a dictation's whole text, with Copy and Paste
//! (and, for a capture, Open in Editor). The rail scrolls through the whole
//! list, building only the rows it shows. The card states what it draws; its
//! owner decides what each [`Act`] does.

use std::borrow::Cow;
use std::path::Path;
use std::sync::Arc;

use rune::{Cursor, El, GrowthSpec, ScrollSpec, SurfaceFit, Tx, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize};

use crate::desktop::i18n::{event_age_at, tr};
use crate::paste::recent::{Entry, Id};
use crate::rune_ui::controls::{CAPSULE, METRICS, Paint, PushSize};
use crate::rune_ui::screenshot_thumbnail::editor::clip::duration_label;

use super::wheel;

static DICTATION_ICON: &rune::Icon = &rune_theme_opencode::icons::WAVEFORM;
static PICTURE_ICON: &rune::Icon = &rune_theme_opencode::icons::OUTLINE_PHOTO;

pub(super) const CARD_W: f32 = 720.0;
const RAIL_W: f32 = 300.0;
/// The rows the rail shows at once, the rest scrolling: the card is this
/// tall whatever it holds, so the preview never moves as the selection does.
pub(super) const ROWS: usize = 8;
const BODY_H: f32 =
    2.0 * CAPSULE.row_inset + ROWS as f32 * METRICS.row_h + (ROWS as f32 - 1.0) * CAPSULE.row_gap;
/// A row and the gap below it.
const STEP: f32 = METRICS.row_h + CAPSULE.row_gap;
/// The rail's scroll.
pub(super) const RAIL: &str = "paste-rail";
/// A dictation's whole text, scrolling in the preview.
const WORDS: &str = "paste-words-scroll";
/// A row's picture, the shape of a display.
pub(super) const LEAD: (f32, f32) = (40.0, 28.0);
/// The preview's text edge to text edge: a 16:10 capture fills it.
pub(super) const PICTURE_W: f32 = CARD_W - RAIL_W - 2.0 * METRICS.row_px;
/// A taller capture (a window, a phone) is drawn this tall at most.
const PICTURE_MAX_H: f32 = 244.0;

/// A picture's size on the card.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum Size {
    Lead,
    Preview,
}

impl Size {
    pub(super) fn width(self) -> f32 {
        match self {
            Self::Lead => LEAD.0,
            Self::Preview => PICTURE_W,
        }
    }
}

/// What a press on the card asks of its owner.
#[derive(Clone)]
pub(super) enum Act {
    /// A row was clicked.
    Select(Id),
    Paste(Id),
    /// Opens a capture in the editor.
    Open(Id),
    /// Copies the row at this index.
    Copy(usize),
    Dismiss,
    History,
}

/// Row `i`'s key: its place in this invocation's list, which never changes
/// while the picker is open.
pub(super) fn row_name(i: usize) -> String {
    format!("paste-row-{i}")
}

fn row_key<S>(_: &S, i: usize) -> Cow<'_, str> {
    row_name(i).into()
}

/// How the rail follows the selection: from its top when the picker opens
/// (the selection is then among the first rows, or revealed), and just far
/// enough to show a row the keys moved to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Reveal {
    Top,
    Row(usize),
    Stay,
}

pub(super) fn reveal(opened: bool, was: usize, cursor: usize, rows: usize) -> Reveal {
    let selected = cursor < rows;
    if opened {
        if selected && cursor >= ROWS {
            Reveal::Row(cursor)
        } else {
            Reveal::Top
        }
    } else if selected && cursor != was {
        Reveal::Row(cursor)
    } else {
        Reveal::Stay
    }
}

/// Moves the rail as `reveal` says.
pub(super) fn follow(tx: &mut Tx, reveal: Reveal) {
    match reveal {
        Reveal::Top => tx.scroll_to_y(RAIL, 0.0),
        Reveal::Row(i) => tx.scroll_into_view_in(RAIL, row_name(i)),
        Reveal::Stay => {}
    }
}

/// A picture source the rail's rows can keep: they're built as they scroll
/// in, from the window's state.
pub(super) trait Pixels<S>:
    Fn(&S, &Path, Size) -> Option<rune::Pixels> + Copy + 'static
{
}
impl<S, F: Fn(&S, &Path, Size) -> Option<rune::Pixels> + Copy + 'static> Pixels<S> for F {}

/// What the card draws.
pub(super) struct Shown<'a> {
    pub rows: &'a Arc<[Entry]>,
    pub cursor: usize,
    pub now: u64,
    /// Paste Last's keycaps.
    pub trigger: &'a [String],
}

/// The whole overlay: the card centred over a clear layer that closes the
/// picker when pressed outside it.
pub(super) fn stage<S: 'static>(
    shown: &Shown,
    s: &S,
    p: Paint,
    pixels: impl Pixels<S>,
    act: impl Fn(&mut S, Act, &mut Tx) + Copy + 'static,
) -> El {
    col()
        .key("paste-stage")
        .role("none")
        .w_full()
        .h_full()
        .center()
        .hit()
        .on_press_tx(move |s: &mut S, tx| act(s, Act::Dismiss, tx))
        .child(card(shown, s, p, pixels, act))
}

pub(super) fn card<S: 'static>(
    shown: &Shown,
    s: &S,
    p: Paint,
    pixels: impl Pixels<S>,
    act: impl Fn(&mut S, Act, &mut Tx) + Copy + 'static,
) -> El {
    let c = &p.k.c;
    let look = |path: &Path, size| pixels(s, path, size);
    let body = if shown.rows.is_empty() {
        row().key("paste-body").h(BODY_H).center().child(
            text(tr("Nothing here yet"))
                .key("paste-empty")
                .font(p.description())
                .color(p.muted()),
        )
    } else {
        row()
            .key("paste-body")
            .h(BODY_H)
            .child(rail(shown, p, pixels, act))
            .child(el().w(p.hair).h_full().fill(c.border_weak))
            .child(preview(shown, p, &look, act))
    };
    let mut panel = col()
        .key("paste-card")
        .role("dialog")
        .label(tr("Paste"))
        .w(CARD_W)
        .radius(CAPSULE.radius)
        .fill(c.bg_base)
        .border(p.hair, c.border_base)
        // Children sit inside the border, so their fills never paint over it.
        .padding(p.hair)
        .clip_soft(0.0)
        .hover_motion_within(rune::Anim::INSTANT, rune::Anim::INSTANT)
        .hit()
        // Presses inside the card stay inside.
        .on_press(|_: &mut S| {})
        .child(body)
        .child(el().w_full().h(p.hair).fill(c.border_weak))
        .child(footer(shown, p, act));
    for shadow in &p.k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    panel
}

/// The recent rows, all of them, scrolling: only the rows in view (and a
/// little either side) are built, so a deep list opens as fast as a short
/// one and asks for only the pictures it shows.
fn rail<S: 'static>(
    shown: &Shown,
    p: Paint,
    pixels: impl Pixels<S>,
    act: impl Fn(&mut S, Act, &mut Tx) + Copy + 'static,
) -> El {
    let rows = shown.rows.clone();
    let (cursor, now, count) = (shown.cursor, shown.now, shown.rows.len());
    let list = col()
        .key("paste-rows")
        .role("list")
        .w_full()
        .padding(CAPSULE.row_inset)
        .growth(GrowthSpec::default())
        .virtual_rows(count, STEP, row_key::<S>, move |s: &S, i| {
            let line = line(
                &rows[i],
                i == cursor,
                now,
                p,
                &|path, size| pixels(s, path, size),
                act,
            );
            // The gap below each row but the last, inside it, so every row
            // stands one step below the one before.
            let gap = if i + 1 < count { CAPSULE.row_gap } else { 0.0 };
            col().w_full().pb(gap).child(line)
        });
    let viewport = col()
        .key(RAIL)
        .w(RAIL_W)
        .h_full()
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    wheel::vertical::<S>(viewport, RAIL).child(list)
}

/// One row: its lead, its first words, and its kind and age. A press
/// selects it by its frozen identity, never its place.
fn line<S: 'static>(
    entry: &Entry,
    selected: bool,
    now: u64,
    p: Paint,
    pixels: &dyn Fn(&Path, Size) -> Option<rune::Pixels>,
    act: impl Fn(&mut S, Act, &mut Tx) + Copy + 'static,
) -> El {
    let c = &p.k.c;
    let kind = match &entry.row.id {
        Id::Capture(_) => tr("Annotation"),
        Id::Dictation(_) if entry.row.cancelled => tr("Cancelled"),
        Id::Dictation(_) if entry.detail.raw => tr("Raw"),
        Id::Dictation(_) => tr("Dictation"),
    };
    let meta = format!("{kind} · {}", event_age_at(entry.row.ready_at, now));
    let title = entry.row.preview.split_whitespace().collect::<Vec<_>>();
    let title = if title.is_empty() {
        kind.to_owned()
    } else {
        title.join(" ")
    };
    let mut line = row()
        .key("paste-line")
        .role("listitem")
        .label(title.clone())
        .description(meta.clone())
        .h(METRICS.row_h)
        .px(CAPSULE.row_px)
        .gap(12.0)
        .center_y()
        .radius(p.k.radius.lg)
        .child(lead(p, entry, pixels))
        .child(
            col()
                .key("words")
                .gap(2.0)
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .child(
                    text(title)
                        .key("title")
                        .font(p.strong())
                        .color(p.text())
                        .truncate()
                        .a11y_hidden(),
                )
                .child(
                    text(meta)
                        .key("meta")
                        .font(p.tabular(p.meta()))
                        .color(if entry.row.cancelled {
                            p.faint()
                        } else {
                            p.muted()
                        })
                        .truncate()
                        .a11y_hidden(),
                ),
        );
    if selected {
        line = line.fill(c.layer_02);
    }
    let id = entry.row.id.clone();
    line.hover(c.hover)
        .press(c.pressed)
        .hit()
        .focusable(false)
        .on_press_tx(move |s: &mut S, tx| act(s, Act::Select(id.clone()), tx))
}

/// What leads a row: a capture's picture, or a dictation's mark on a tile
/// the same size, so every title starts on one edge.
fn lead(p: Paint, entry: &Entry, pixels: &dyn Fn(&Path, Size) -> Option<rune::Pixels>) -> El {
    let c = &p.k.c;
    let tile = el()
        .key("lead")
        .size(LEAD.0, LEAD.1)
        .radius(p.k.radius.sm)
        .a11y_hidden();
    let rim = || {
        el().absolute(0.0, 0.0)
            .size(LEAD.0, LEAD.1)
            .radius(p.k.radius.sm)
            .border(p.hair, c.border_weak)
    };
    let glyph = |mark: &'static rune::Icon| icon(mark, METRICS.icon).color(c.icon_muted);
    match &entry.row.id {
        Id::Dictation(_) => tile.fill(c.layer_01).center().child(glyph(DICTATION_ICON)),
        Id::Capture(_) => {
            let picture = entry
                .detail
                .picture
                .as_deref()
                .and_then(|path| pixels(path, Size::Lead));
            match picture {
                Some(picture) => tile
                    .fill(c.layer_01)
                    .clip_soft(0.0)
                    .pixels_fit(&picture, SurfaceFit::Fill)
                    .child(rim()),
                None => tile
                    .fill(c.layer_01)
                    .center()
                    .child(glyph(PICTURE_ICON))
                    .child(rim()),
            }
        }
    }
}

/// The selected row, whole: what a paste would insert, and what can be done
/// with it.
fn preview<S: 'static>(
    shown: &Shown,
    p: Paint,
    pixels: &dyn Fn(&Path, Size) -> Option<rune::Pixels>,
    act: impl Fn(&mut S, Act, &mut Tx) + Copy + 'static,
) -> El {
    let pane = col()
        .key("paste-preview")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .h_full()
        .padding(METRICS.row_px)
        .gap(12.0);
    // Only cancelled rows: nothing is selected until one is chosen.
    let Some(entry) = shown.rows.get(shown.cursor) else {
        return pane;
    };
    match &entry.row.id {
        Id::Capture(_) => {
            let words = (!entry.detail.text.is_empty()).then(|| {
                text(entry.detail.text.clone())
                    .key("paste-words")
                    .font(p.k.ty.body)
                    .color(p.text())
                    .wrap()
                    .max_lines(3)
            });
            pane.children(picture(p, entry, pixels, act))
                .children(words)
                .child(el().grow(1.0))
                .child(actions(shown, p, entry, act))
        }
        // The whole text; one longer than the pane scrolls.
        Id::Dictation(_) => pane
            .child(
                wheel::vertical::<S>(
                    col()
                        .key(WORDS)
                        .w_full()
                        .grow(1.0)
                        .shrink()
                        .min_h(0.0)
                        .scroll(ScrollSpec {
                            stick_bottom: false,
                        }),
                    WORDS,
                )
                .child(
                    text(entry.detail.text.clone())
                        .key("paste-words")
                        .font(p.k.ty.body)
                        .color(p.text())
                        .wrap(),
                ),
            )
            .child(actions(shown, p, entry, act)),
    }
}

/// A capture's picture, whole and never cropped: as wide as the preview's
/// text, or narrower when that would make it too tall. A press opens it in
/// the editor.
fn picture<S: 'static>(
    p: Paint,
    entry: &Entry,
    pixels: &dyn Fn(&Path, Size) -> Option<rune::Pixels>,
    act: impl Fn(&mut S, Act, &mut Tx) + Copy + 'static,
) -> Option<El> {
    let c = &p.k.c;
    let path = entry.detail.picture.as_deref()?;
    let loaded = pixels(path, Size::Preview);
    let ratio = loaded.as_ref().map_or(1.6, |picture| {
        let (w, h) = picture.size();
        w as f32 / h.max(1) as f32
    });
    let (w, h) = if PICTURE_W / ratio <= PICTURE_MAX_H {
        (PICTURE_W, PICTURE_W / ratio)
    } else {
        (PICTURE_MAX_H * ratio, PICTURE_MAX_H)
    };
    let mut frame = el()
        .key("paste-picture")
        .role("button")
        .label(tr("Open in Editor"))
        .size(w, h)
        .radius(p.k.radius.md)
        .fill(c.layer_01)
        .clip_soft(0.0);
    if let Some(picture) = &loaded {
        frame = frame.pixels_fit(picture, SurfaceFit::Fit);
    }
    let id = entry.row.id.clone();
    Some(
        frame
            .child(
                el().absolute(0.0, 0.0)
                    .size(w, h)
                    .radius(p.k.radius.md)
                    .border(p.hair, c.border_weak),
            )
            .cursor(Cursor::Pointer)
            .hit()
            .on_press_tx(move |s: &mut S, tx| act(s, Act::Open(id.clone()), tx)),
    )
}

/// What the rail doesn't say about the row, then its actions, the one that
/// pastes last.
fn actions<S: 'static>(
    shown: &Shown,
    p: Paint,
    entry: &Entry,
    act: impl Fn(&mut S, Act, &mut Tx) + Copy + 'static,
) -> El {
    let detail = &entry.detail;
    let length = (detail.duration_ms > 0).then(|| duration_label(detail.duration_ms));
    let facts = [detail.application.clone(), length, detail.model.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    let push = |key: &'static str, label: &str, what: Act| {
        Button::new(label)
            .variant(ButtonVariant::Neutral)
            .on_press_tx(move |s: &mut S, tx| act(s, what.clone(), tx))
            .build_push(p.k)
            .key(key)
    };
    let id = entry.row.id.clone();
    let paste = Button::new(tr("Paste"))
        .variant(ButtonVariant::Contrast)
        .label_with(|font, ink| {
            row()
                .gap(6.0)
                .center_y()
                .child(text(tr("Paste")).font(font).color(ink))
                .child(text("↵").font(font).color(ink.alpha(0.7)))
        })
        .on_press_tx({
            let id = id.clone();
            move |s: &mut S, tx| act(s, Act::Paste(id.clone()), tx)
        })
        .build_push(p.k)
        .key("paste-commit");
    let open = matches!(id, Id::Capture(_))
        .then(|| push("paste-open", tr("Open in Editor"), Act::Open(id.clone())));
    row()
        .key("paste-actions")
        .gap(8.0)
        .center_y()
        .child(
            text(facts)
                .key("paste-facts")
                .font(p.description())
                .color(p.muted())
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .truncate(),
        )
        .child(push("paste-copy", tr("Copy"), Act::Copy(shown.cursor)))
        .children(open)
        .child(paste)
}

/// The keys the card doesn't show on a button, then the way to everything
/// older.
fn footer<S: 'static>(
    shown: &Shown,
    p: Paint,
    act: impl Fn(&mut S, Act, &mut Tx) + Copy + 'static,
) -> El {
    let hint = |key: &'static str, keys: String, label: &str| {
        row()
            .key(key)
            .gap(4.0)
            .center_y()
            .child(text(keys).font(p.meta()).color(p.muted()))
            .child(text(label.to_owned()).font(p.meta()).color(p.faint()))
    };
    let mut hints = row().key("paste-hints").gap(12.0).center_y().a11y_hidden();
    if !shown.trigger.is_empty() && shown.rows.len() > 1 {
        hints = hints.child(hint("hint-next", shown.trigger.concat(), tr("Next")));
    }
    hints = hints.child(hint("hint-close", "esc".into(), tr("Close")));
    // The button's label ends on the preview's text edge.
    row()
        .key("paste-footer")
        .h(CAPSULE.header_h)
        .pl(METRICS.row_px)
        .pr(6.0)
        .center_y()
        .child(hints)
        .child(el().grow(1.0))
        .child(
            Button::new(tr("Show all History"))
                .variant(ButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .on_press_tx(move |s: &mut S, tx| act(s, Act::History, tx))
                .build(p.k)
                .key("paste-show-history"),
        )
}

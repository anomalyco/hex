//! The picker's card: recent rows down a rail, each led by its picture or a
//! dictation's mark, and the selected row's preview beside them: a capture's
//! picture over its words, or a dictation's whole text, with what Paste,
//! Edit, and Copy do to it. The card states what it draws; its owner decides
//! what each [`Act`] does.

use std::path::Path;

use rune::{Cursor, El, Key, ScrollSpec, SurfaceFit, TextEdit, Tx, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize};

use crate::desktop::i18n::{event_age_at, tr};
use crate::paste::recent::{Entry, Id};
use crate::rune_ui::controls::{CAPSULE, METRICS, Paint, PushSize};
use crate::rune_ui::screenshot_thumbnail::editor::clip::duration_label;

static DICTATION_ICON: &rune::Icon = &rune_theme_opencode::icons::WAVEFORM;
static PICTURE_ICON: &rune::Icon = &rune_theme_opencode::icons::OUTLINE_PHOTO;

pub(super) const CARD_W: f32 = 720.0;
const RAIL_W: f32 = 300.0;
/// The rows a picker lists at most: the card is this tall whatever it holds,
/// so the preview never moves as the selection does.
const ROWS: f32 = 8.0;
const BODY_H: f32 = 2.0 * CAPSULE.row_inset + ROWS * METRICS.row_h + (ROWS - 1.0) * CAPSULE.row_gap;
/// A row's picture, the shape of a display.
pub(super) const LEAD: (f32, f32) = (40.0, 28.0);
/// The preview's text edge to text edge: a 16:10 capture fills it.
pub(super) const PICTURE_W: f32 = CARD_W - RAIL_W - 2.0 * METRICS.row_px;
/// A taller capture (a window, a phone) is drawn this tall at most.
const PICTURE_MAX_H: f32 = 244.0;
/// The words' box hangs this far past the text edge, so its hover and focus
/// ring have room while the words stay on the edge.
const WORDS_HANG: f32 = 8.0;

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
    /// A dictation's words become editable; a capture opens in the editor.
    Edit(Id),
    /// Copies the row at this index.
    Copy(usize),
    PasteEdited,
    StopEditing,
    Dismiss,
    History,
}

/// What the card draws.
pub(super) struct Shown<'a> {
    pub rows: &'a [Entry],
    pub cursor: usize,
    pub now: u64,
    /// The selected dictation's words while they are edited.
    pub draft: Option<&'a TextEdit>,
    /// Paste Last's keycaps.
    pub trigger: &'a [String],
}

/// The whole overlay: the card centred over a clear layer that closes the
/// picker when pressed outside it.
pub(super) fn stage<S: 'static>(
    shown: &Shown,
    p: Paint,
    pixels: &dyn Fn(&Path, Size) -> Option<rune::Pixels>,
    draft: fn(&mut S) -> &mut TextEdit,
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
        .child(card(shown, p, pixels, draft, act))
}

pub(super) fn card<S: 'static>(
    shown: &Shown,
    p: Paint,
    pixels: &dyn Fn(&Path, Size) -> Option<rune::Pixels>,
    draft: fn(&mut S) -> &mut TextEdit,
    act: impl Fn(&mut S, Act, &mut Tx) + Copy + 'static,
) -> El {
    let c = &p.k.c;
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
            .child(preview(shown, p, pixels, draft, act))
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

/// The recent rows. While a dictation's words are edited the rail steps
/// back and takes no presses: the keys and the pointer are the editor's.
fn rail<S: 'static>(
    shown: &Shown,
    p: Paint,
    pixels: &dyn Fn(&Path, Size) -> Option<rune::Pixels>,
    act: impl Fn(&mut S, Act, &mut Tx) + Copy + 'static,
) -> El {
    let c = &p.k.c;
    let editing = shown.draft.is_some();
    let mut list = col()
        .key("paste-rows")
        .role("list")
        .w(RAIL_W)
        .padding(CAPSULE.row_inset)
        .gap(CAPSULE.row_gap)
        .opacity(if editing { 0.5 } else { 1.0 });
    for (i, entry) in shown.rows.iter().enumerate() {
        let kind = match &entry.row.id {
            Id::Capture(_) => tr("Annotation"),
            Id::Dictation(_) if entry.row.cancelled => tr("Cancelled"),
            Id::Dictation(_) if entry.detail.raw => tr("Raw"),
            Id::Dictation(_) => tr("Dictation"),
        };
        let meta = format!("{kind} · {}", event_age_at(entry.row.ready_at, shown.now));
        let title = entry.row.preview.split_whitespace().collect::<Vec<_>>();
        let title = if title.is_empty() {
            kind.to_owned()
        } else {
            title.join(" ")
        };
        let mut line = row()
            .key(format!("paste-row-{i}"))
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
        if i == shown.cursor {
            line = line.fill(c.layer_02);
        }
        if !editing {
            let id = entry.row.id.clone();
            line = line
                .hover(c.hover)
                .press(c.pressed)
                .hit()
                .focusable(false)
                .on_press_tx(move |s: &mut S, tx| act(s, Act::Select(id.clone()), tx));
        }
        list = list.child(line);
    }
    list
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
    draft: fn(&mut S) -> &mut TextEdit,
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
    let id = entry.row.id.clone();
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
        Id::Dictation(_) => {
            let c = &p.k.c;
            let words = match shown.draft {
                Some(edit) => rune::text_input(edit, p.k.ty.body)
                    .key("paste-draft")
                    .label(tr("Edit"))
                    .w_full()
                    .color(p.text())
                    .caret(p.text(), p.k.selection())
                    .on_edit(draft)
                    .on_submit(move |s: &mut S, tx| act(s, Act::PasteEdited, tx))
                    .on_input_key(move |s: &mut S, key: &rune::KeyEvent, tx| {
                        let leave = key.key == Key::Escape;
                        if leave {
                            act(s, Act::StopEditing, tx);
                        }
                        leave
                    }),
                None => text(entry.detail.text.clone())
                    .key("paste-words")
                    .font(p.k.ty.body)
                    .color(p.text())
                    .wrap(),
            };
            // One box for reading and editing, so the words never move
            // between the two: only its ring and the caret come and go.
            let words = col()
                .key("paste-words-box")
                .grow(1.0)
                .shrink()
                .min_h(0.0)
                .mx(-WORDS_HANG)
                .mt(-WORDS_HANG)
                .padding(WORDS_HANG)
                .radius(p.k.radius.md)
                .child(
                    col()
                        .key("paste-words-scroll")
                        .w_full()
                        .grow(1.0)
                        .shrink()
                        .min_h(0.0)
                        .scroll(ScrollSpec {
                            stick_bottom: false,
                        })
                        .child(words),
                );
            let words = if shown.draft.is_some() {
                words.outline(1.0, -1.0, c.border_focus)
            } else {
                words
                    .role("button")
                    .label(tr("Edit"))
                    .hover(c.hover)
                    .cursor(Cursor::Text)
                    .hit()
                    .on_press_tx(move |s: &mut S, tx| act(s, Act::Edit(id.clone()), tx))
            };
            pane.child(words).child(actions(shown, p, entry, act))
        }
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
            .on_press_tx(move |s: &mut S, tx| act(s, Act::Edit(id.clone()), tx)),
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
    let editing = shown.draft.is_some();
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
            move |s: &mut S, tx| {
                let what = if editing {
                    Act::PasteEdited
                } else {
                    Act::Paste(id.clone())
                };
                act(s, what, tx)
            }
        })
        .build_push(p.k)
        .key("paste-commit");
    let others = if editing {
        vec![push("paste-edit-cancel", tr("Cancel"), Act::StopEditing)]
    } else {
        let edit = match &id {
            Id::Capture(_) => tr("Open in Editor"),
            Id::Dictation(_) => tr("Edit"),
        };
        vec![
            push("paste-copy", tr("Copy"), Act::Copy(shown.cursor)),
            push("paste-edit", edit, Act::Edit(id.clone())),
        ]
    };
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
        .children(others)
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
    if shown.draft.is_some() {
        hints = hints.child(hint("hint-back", "esc".into(), tr("Back")));
    } else {
        if !shown.trigger.is_empty() && shown.rows.len() > 1 {
            hints = hints.child(hint("hint-next", shown.trigger.concat(), tr("Next")));
        }
        hints = hints.child(hint("hint-close", "esc".into(), tr("Close")));
    }
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

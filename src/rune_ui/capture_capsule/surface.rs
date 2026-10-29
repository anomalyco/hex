//! Where the capture capsule's pieces rest and how each change moves,
//! decided in Rust: [`layout`] places what [`Pres`] shows, and [`Surface`]
//! keeps what is mounted, its keys, and the spring each piece moves on
//! next. The view only draws it; Rune animates between the targets.
//!
//! Ported from Scrawl's Rune capsule (`scrawl-pill-lab`, `rune_pill`).
//! Every change moves at once, as Dynamic Island's: the walls and what
//! stays on one spring, what leaves and what comes in blurring, scaling and
//! fading over the same move. Only an entry takes two frames: the origin
//! pose, then rest.

use std::cell::RefCell;

use rune::{Spring, TextStyle};

use super::flow::{Done, Lead, Mode, Pres};
use super::notch;
use super::tuning::{self, CapsuleFlow, TopLine};

/// The overlay's panel: wide enough for the open list, and tall enough for
/// it and the shadow under it.
pub(crate) const PANEL_W: f32 = 640.0;
pub(crate) const PANEL_H: f32 = 424.0;
/// Panel points the capsule is centred on, and its top.
pub(crate) const CX: f32 = PANEL_W * 0.5;
pub(crate) const TOP: f32 = 22.0;
pub(crate) const ISLAND_H: f32 = 40.0;
pub(crate) const ISLAND_INSET: f32 = 4.0;
pub(crate) const CHIP_H: f32 = ISLAND_H - 2.0 * ISLAND_INSET;
/// The red dot's ink spans 12…20 of the lead's 32 pt square; the timer
/// starts 8 pt after it.
pub(crate) const LEAD_TEXT_X: f32 = 28.0;
pub(crate) const PROCESSING_TEXT_X: f32 = 31.5;
const PROCESSING_TRAIL: f32 = 8.5;
/// The check's ink spans 12…20, as the dot's does; its label starts where
/// the timer does.
pub(crate) const DONE_TEXT_X: f32 = LEAD_TEXT_X;
/// The capture's first frame in the check's place: a 28 × 20 card 8 pt in
/// from the capsule's curve, its label 8 pt after it.
pub(crate) const THUMB_X: f32 = 8.0;
pub(crate) const THUMB_W: f32 = 28.0;
pub(crate) const THUMB_H: f32 = 20.0;

/// Where the finished capsule's label starts after its lead.
pub(crate) fn done_text_x(lead: Lead) -> f32 {
    match lead {
        Lead::Thumbnail => THUMB_X + THUMB_W + 8.0,
        Lead::Check | Lead::Warning => DONE_TEXT_X,
    }
}
const CHIP_GAP: f32 = 2.0;
const GROUP_GAP: f32 = 6.0;
/// How far ✕ tucks into the action before it.
const CLOSE_HUG: f32 = 6.0;
pub(crate) const CHIP_PX: f32 = 12.0;
/// After the lead's text, equal to the dot's 12 pt inset.
const LEAD_TRAIL: f32 = 12.0;
/// A mode chip: its icon, the gap after it, and the padding either side.
pub(crate) const SEG_ICON: f32 = 14.0;
pub(crate) const SEG_ICON_GAP: f32 = 5.0;
const SEG_PX: f32 = 10.0;
/// Between a chip's label and its shortcut.
pub(crate) const HINT_GAP: f32 = 6.0;

/// The open capsule is as wide as the list under it; its field takes what
/// the rest leaves.
pub(crate) const LIST_W: f32 = 420.0;
const FIELD_MIN: f32 = 120.0;
pub(crate) const LIST_Y: f32 = TOP + ISLAND_H + 6.0;
pub(crate) const LIST_PAD: f32 = 6.0;
pub(crate) const HEADER_H: f32 = 24.0;
pub(crate) const ROW_H: f32 = 30.0;

/// The cursor's and the notch's card: at least this wide, a row per
/// guess under its top line.
pub(crate) const CARD_W: f32 = 280.0;
pub(crate) const PICK_H: f32 = 32.0;
/// The card's hint line under its guesses.
pub(crate) const CARD_HINTS_H: f32 = 28.0;
/// The capsule is a rounded rect, its corners the list's, so the two
/// match and the square thumbnail sits in a corner that follows it.
pub(crate) const SHELL_RADIUS: f32 = 14.0;
/// A chip's corners, concentric with the shell's.
pub(crate) const CHIP_RADIUS: f32 = SHELL_RADIUS - ISLAND_INSET;
/// The walls' corners never round more than the one-line capsule's, so
/// the line grows into a card without its corners changing.
pub(crate) fn corner(h: f32) -> f32 {
    (h * 0.5).min(SHELL_RADIUS)
}
/// Where the cursor's card rests in the panel, its top-left: the app moves
/// the panel so the pointer sits just above and left of it.
pub(crate) const CURSOR_X: f32 = 196.0;
pub(crate) const CURSOR_Y: f32 = 132.0;
/// The pointer's tip, up and left of the card's corner.
pub(crate) const POINTER_OFFSET: [f32; 2] = [12.0, 16.0];
/// The menu-bar flow folds the capsule toward the panel's top right, the
/// way Hex's menu-bar item lies.
pub(crate) const MENU_FOLD: [f32; 4] = [PANEL_W - 72.0, 2.0, 30.0, 14.0];

/// Where the capsule rests in the panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Anchor {
    /// Under the menu bar, centred: every flow but the two below.
    #[default]
    Top,
    /// Hanging from the notch's bottom edge (the panel's top), grown out
    /// of the notch.
    Notch,
    /// The cursor's card, at [`CURSOR_X`], [`CURSOR_Y`].
    Cursor,
}

pub(crate) const MEDIUM: TextStyle = TextStyle::new(13.0, 530.0);
pub(crate) const STRONG: TextStyle = TextStyle::new(13.0, 600.0);
/// Tabular digits, so a new second never changes the timer's width.
pub(crate) const TIMER: TextStyle = STRONG.features("'tnum' 1");
pub(crate) const SMALL: TextStyle = TextStyle::new(11.5, 530.0);
pub(crate) const PROCESSING: &str = "Processing…";

/// A shortcut as the capsule says it: one quiet word, "⌥⇧V". Symbols run
/// together, as macOS menus write them; a named key stands apart.
pub(crate) fn chord<S: AsRef<str>>(keys: &[S]) -> String {
    let mut out = String::new();
    for key in keys {
        let key = key.as_ref();
        let word = key.chars().count() > 1;
        if !out.is_empty() && (word || out.ends_with(|c: char| c.is_ascii_alphabetic())) {
            out.push(' ');
        }
        out.push_str(key);
    }
    out
}

/// The finished capsule's label: quieter in [`TopLine::Quiet`].
pub(crate) fn done_style() -> TextStyle {
    match tuning::get().top_line {
        TopLine::Quiet => MEDIUM,
        TopLine::Icons => STRONG,
    }
}

/// [`TopLine::Icons`] says nothing for a plain copy: the check is enough.
pub(crate) fn bare(done: &Done) -> bool {
    tuning::get().top_line == TopLine::Icons && done.text.is_empty() && done.why.is_none()
}

/// The capsule unfolds from this fraction of its height, anchored on its
/// top edge.
const TOP_FROM: f32 = 0.7;

thread_local! {
    static TEXT: RefCell<rune::text::TextSystem> = RefCell::new(
        rune::text::TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES),
    );
}

/// `text`'s width in points as Rune draws it in `style`.
pub(crate) fn measure(text: &str, style: TextStyle) -> f32 {
    TEXT.with_borrow_mut(|t| t.measure(text, style, 2.0).0)
}

/// `text` cut to fit `max` points, ending in "…" when cut.
pub(crate) fn ellipsize(text: &str, style: TextStyle, max: f32) -> String {
    if measure(text, style) <= max {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    (1..chars.len())
        .rev()
        .map(|n| format!("{}…", chars[..n].iter().collect::<String>().trim_end()))
        .find(|cut| measure(cut, style) <= max)
        .unwrap_or_else(|| "…".into())
}

/// The Undo chip's label; the seconds left show in the ring.
pub(crate) const UNDO_LABEL: &str = "Undo";
/// The recording's ✕ asks before it discards.
pub(crate) const KEEP_LABEL: &str = "Keep recording";
pub(crate) const DISCARD_LABEL: &str = "Discard";

/// Something the capsule shows, or a click on a row of its list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Id {
    Mode(Mode),
    Record,
    Cancel,
    Finish,
    /// Asked after the recording's ✕: back to recording.
    Keep,
    /// Asked after the recording's ✕: ends it, keeping nothing.
    Discard,
    Close,
    /// The recording dot and timer.
    Lead,
    Processing,
    /// The finished capsule's check (or warning) and its label.
    Done,
    /// A quiet word on why, after the label.
    Why,
    /// The Undo chip, while a send can be taken back.
    Undo,
    /// Pastes the capture.
    Paste,
    /// The Send to… chip.
    Ask,
    Inspect,
    /// The open capsule's field.
    Field,
    Row(usize),
    /// A guess in the capsule itself: a chip in the menu-bar flow, a row
    /// in the cursor's and the notch's card.
    Pick(usize),
    /// The card's key hint under its guesses ("Send 1–5").
    CardHints,
    /// The toast's next guess (⇥).
    Next,
}

impl Id {
    /// Its name in keys and the motion trace (Scrawl's trace names).
    pub(crate) fn trace_key(self) -> &'static str {
        match self {
            Id::Mode(Mode::Area) => "chip:Area",
            Id::Mode(Mode::Window) => "chip:Window",
            Id::Mode(Mode::Screen) => "chip:Screen",
            Id::Mode(Mode::Iphone) => "chip:iPhone",
            Id::Record => "record",
            Id::Cancel => "cancel",
            Id::Finish => "finish",
            Id::Keep => "keep",
            Id::Discard => "discard",
            Id::Close => "close",
            Id::Lead => "lead",
            Id::Processing => "processing",
            Id::Done => "done",
            Id::Why => "why",
            Id::Undo => "undo",
            Id::Paste => "paste",
            Id::Ask => "ask",
            Id::Inspect => "inspect",
            Id::Field => "field",
            Id::Row(_) => "row",
            Id::Pick(_) => "pick",
            Id::CardHints => "card-hints",
            Id::Next => "next",
        }
    }

    const fn group(self) -> u8 {
        match self {
            Id::Lead | Id::Processing | Id::Done | Id::Why => 0,
            Id::Mode(_) => 1,
            Id::Close => 3,
            _ => 2,
        }
    }

    /// The space between `self` and the item after it. The finished
    /// capsule's label stands clear of its actions, and the actions and ✕
    /// sit together as one cluster, so Undo never floats between them.
    fn gap_before(self, next: Id) -> f32 {
        match (self, next) {
            (Id::Done, Id::Why) => CHIP_GAP,
            // The field's well starts the label's trail after it: 12 pt,
            // as the lead's inset is.
            (Id::Done | Id::Why, Id::Field) => 0.0,
            // The label's trail and the action's padding already part
            // them.
            (Id::Done | Id::Why, _) => GROUP_GAP,
            // ✕'s circle has air of its own, so it tucks into a worded
            // action's right padding; drawn later, it takes the overlap's
            // presses. Icons beside ✕ keep one even step.
            (Id::Undo | Id::Ask | Id::Next | Id::Pick(_), Id::Close) => -CLOSE_HUG,
            (Id::Paste | Id::Inspect, Id::Close) => CHIP_GAP,
            _ if self.group() == next.group() => CHIP_GAP,
            _ => GROUP_GAP,
        }
    }

    /// Its place left to right, for drawing order.
    fn order(self) -> u8 {
        match self {
            Id::Lead => 0,
            Id::Processing => 1,
            Id::Done => 2,
            Id::Why => 3,
            Id::Mode(Mode::Area) => 4,
            Id::Mode(Mode::Window) => 5,
            Id::Mode(Mode::Screen) => 6,
            Id::Mode(Mode::Iphone) => 7,
            Id::Record => 8,
            Id::Cancel | Id::Keep => 9,
            Id::Finish | Id::Discard => 10,
            Id::Field => 11,
            Id::Undo => 12,
            Id::Paste => 13,
            Id::Inspect => 14,
            Id::Next => 15,
            Id::Pick(row) => 16 + row.min(1) as u8,
            Id::Ask => 18,
            Id::Close => 20,
            Id::Row(_) | Id::CardHints => 21,
        }
    }
}

/// Separates a note's text from the lead label it's beside, in its
/// `ident`.
const NOTE_OF: char = '\u{1f}';

/// A piece of the capsule at rest. `ident` is what makes it the same
/// piece across changes: a new label on the finished capsule's lead is a
/// new piece that crossfades with the old, while the Undo chip stays
/// itself as it counts down and the field as it's typed into.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    pub(crate) id: Id,
    pub(crate) ident: String,
    pub(crate) rect: [f32; 4],
    pub(crate) active: bool,
}

fn mode_width(mode: Mode) -> f32 {
    measure(mode.label(), MEDIUM) + 2.0 * SEG_PX + SEG_ICON + SEG_ICON_GAP
}

fn chip_width(label: f32) -> f32 {
    label + 2.0 * CHIP_PX
}

/// A chip's label, then its shortcut in quiet text, or a plain chip
/// without one.
pub(crate) fn keyed_width(keys: &[&str], label: &str, style: TextStyle) -> f32 {
    if keys.is_empty() {
        return chip_width(measure(label, style));
    }
    chip_width(measure(label, style) + HINT_GAP + measure(&chord(keys), SMALL))
}

pub(crate) fn clock_width(clock: &str) -> f32 {
    measure(clock, TIMER)
}

/// The widest a row gets: the panel, less room for the shadow.
const ROW_MAX: f32 = PANEL_W - 32.0;

/// A row's width from its items' widths, with its gaps and insets.
fn row_width(raw: &[(Id, String, f32, bool)]) -> f32 {
    let gaps: f32 = raw
        .windows(2)
        .map(|pair| pair[0].0.gap_before(pair[1].0))
        .sum();
    raw.iter().map(|r| r.2).sum::<f32>() + gaps + 2.0 * ISLAND_INSET
}

/// The finished capsule's items. A label too long for the row is cut
/// shorter until it fits, to no less than `CUT_MIN`.
fn done_items(done: &Done, raw: &mut Vec<(Id, String, f32, bool)>) {
    const CUT_MIN: f32 = 48.0;
    let start = raw.len();
    let mut cap = f32::INFINITY;
    loop {
        done_row(done, cap, raw);
        if row_width(&raw[start..]) <= ROW_MAX || cap <= CUT_MIN {
            return;
        }
        cap = if cap.is_finite() { cap - 8.0 } else { 200.0 };
        raw.truncate(start);
    }
}

fn done_row(done: &Done, cap: f32, raw: &mut Vec<(Id, String, f32, bool)>) {
    let text_x = done_text_x(done.lead);
    if bare(done) || done.text.is_empty() {
        // The lead alone, as wide as the dot's.
        raw.push((Id::Done, String::new(), text_x, false));
    } else {
        let style = done_style();
        let text = ellipsize(&done.text, style, cap + 60.0);
        raw.push((
            Id::Done,
            text.clone(),
            text_x + measure(&text, style) + LEAD_TRAIL,
            false,
        ));
    }
    let text = raw.last().map(|r| r.1.clone()).unwrap_or_default();
    if let Some(why) = &done.why {
        // The note is a new piece whenever the lead's label is, so it
        // crossfades beside it rather than sliding through the new label.
        raw.push((
            Id::Why,
            format!("{why}{NOTE_OF}{text}"),
            measure(why, SMALL) + LEAD_TRAIL * 0.5,
            false,
        ));
    }
    let typing = done
        .open
        .as_ref()
        .is_some_and(|(_, list)| !list.suggesting());
    if typing {
        // The search takes the rest of the capsule; its keys are in the
        // list's footer.
        raw.push((Id::Field, String::new(), FIELD_MIN, false));
        return;
    }
    // One action at most, the most urgent: take a send back, paste or
    // inspect the capture (two quiet icons), or open the list. Where to
    // send it lives in the list under the capsule, never in chips beside
    // the label.
    let listed = done.open.is_some();
    if done.cycle {
        raw.push((
            Id::Next,
            String::new(),
            keyed_width(&["⇥"], "Next", MEDIUM),
            false,
        ));
    }
    if tuning::get().flow == CapsuleFlow::MenuBar {
        for (row, (name, _)) in done.picks.iter().enumerate() {
            let key = (row + 1).to_string();
            raw.push((
                Id::Pick(row),
                name.clone(),
                keyed_width(&[&key], name, MEDIUM),
                false,
            ));
        }
    }
    if done.undo {
        raw.push((
            Id::Undo,
            String::new(),
            keyed_width(&[], UNDO_LABEL, MEDIUM),
            false,
        ));
    } else {
        if done.hand_off {
            raw.push((Id::Paste, String::new(), CHIP_H, false));
            raw.push((Id::Inspect, String::new(), CHIP_H, false));
        }
        // Where to send it is a click away, beside Paste, never opened for you.
        if let (Some(ask), false) = (&done.ask, listed) {
            raw.push((Id::Ask, ask.clone(), keyed_width(&[], ask, MEDIUM), false));
        }
    }
    raw.push((Id::Close, String::new(), CHIP_H, false));
}

/// The resting capsule `[x, y, w, h]` and its items for `pres`, in panel
/// points.
///
/// Every state is centred on [`CX`], except the recording capsule with its
/// chips: it centres the chip row (`Area … ✕`) where the picker puts the
/// same row and hangs the lead off its left, so switching to Area or Window
/// moves no chip.
#[cfg(test)]
pub(crate) fn layout(pres: &Pres, clock: &str) -> ([f32; 4], Vec<Item>) {
    layout_at(pres, clock, Anchor::Top)
}

/// The guesses `pres` lists as rows in a card, not as chips.
fn card_picks(pres: &Pres) -> &[(String, String)] {
    match pres {
        Pres::Done(done) if tuning::get().flow != CapsuleFlow::MenuBar => &done.picks,
        _ => &[],
    }
}

/// [`layout`] resting at `anchor`, with a card's rows under the top line.
pub(crate) fn layout_at(pres: &Pres, clock: &str, anchor: Anchor) -> ([f32; 4], Vec<Item>) {
    let (mut shell, mut items) = line(pres, clock);
    let picks = card_picks(pres);
    let (dx, dy) = match anchor {
        Anchor::Top => (0.0, 0.0),
        Anchor::Notch => (0.0, -TOP),
        Anchor::Cursor => (CURSOR_X - shell[0], CURSOR_Y - TOP),
    };
    shell[0] += dx;
    shell[1] += dy;
    for item in &mut items {
        item.rect[0] += dx;
        item.rect[1] += dy;
    }
    let rows_y = shell[1] + ISLAND_H - 2.0;
    for (row, (name, why)) in picks.iter().enumerate() {
        items.push(Item {
            id: Id::Pick(row),
            ident: format!("{name}{NOTE_OF}{why}"),
            rect: [
                shell[0] + ISLAND_INSET,
                rows_y + row as f32 * PICK_H,
                shell[2] - 2.0 * ISLAND_INSET,
                PICK_H,
            ],
            active: false,
        });
    }
    if !picks.is_empty() {
        let hints_y = rows_y + picks.len() as f32 * PICK_H;
        let digits = match picks.len() {
            1 => "1".to_owned(),
            n => format!("1–{n}"),
        };
        items.push(Item {
            id: Id::CardHints,
            ident: digits,
            rect: [
                shell[0] + ISLAND_INSET,
                hints_y,
                shell[2] - 2.0 * ISLAND_INSET,
                CARD_HINTS_H,
            ],
            active: false,
        });
        shell[3] = hints_y + CARD_HINTS_H + ISLAND_INSET - shell[1];
    }
    // The notch's walls run up past the screen's top edge, so only its
    // bottom corners round, and it's never narrower than the notch.
    if anchor == Anchor::Notch {
        let wider = (notch::MIN_WIDTH - shell[2]).max(0.0);
        shell[0] -= wider * 0.5;
        shell[2] += wider;
        shell[1] -= notch::REACH;
        shell[3] += notch::REACH;
    }
    (shell, items)
}

/// The capsule's one line: [`layout`] before any anchor or card.
fn line(pres: &Pres, clock: &str) -> ([f32; 4], Vec<Item>) {
    let mut raw: Vec<(Id, String, f32, bool)> = Vec::with_capacity(8);
    let mut held_row = false;
    let modes = |raw: &mut Vec<(Id, String, f32, bool)>, active: Mode, phone: bool| {
        for mode in Mode::chips(phone) {
            raw.push((
                Id::Mode(mode),
                String::new(),
                mode_width(mode),
                mode == active,
            ));
        }
    };
    match pres {
        Pres::Hidden => return ([CX - 48.0, TOP, 96.0, ISLAND_H], Vec::new()),
        Pres::Selecting { mode, phone } => {
            modes(&mut raw, *mode, *phone);
            if mode.shows_record() {
                raw.push((
                    Id::Record,
                    String::new(),
                    chip_width(measure("Record", STRONG)),
                    false,
                ));
            }
            raw.push((Id::Close, String::new(), CHIP_H, false));
        }
        Pres::Recording {
            chips,
            phone,
            discarding,
        } => {
            let lead = LEAD_TEXT_X + clock_width(clock) + LEAD_TRAIL;
            raw.push((Id::Lead, String::new(), lead, false));
            if *discarding {
                raw.push((
                    Id::Keep,
                    String::new(),
                    chip_width(measure(KEEP_LABEL, MEDIUM)),
                    false,
                ));
                raw.push((
                    Id::Discard,
                    String::new(),
                    chip_width(measure(DISCARD_LABEL, STRONG)),
                    false,
                ));
            } else if *chips {
                held_row = true;
                modes(&mut raw, Mode::Screen, *phone);
                raw.push((Id::Close, String::new(), CHIP_H, false));
            } else {
                raw.push((
                    Id::Cancel,
                    String::new(),
                    chip_width(measure("Cancel", MEDIUM)),
                    false,
                ));
                raw.push((
                    Id::Finish,
                    String::new(),
                    chip_width(measure("Finish", STRONG)),
                    false,
                ));
            }
        }
        Pres::Processing => {
            let w = PROCESSING_TEXT_X + measure(PROCESSING, STRONG) + PROCESSING_TRAIL;
            raw.push((Id::Processing, String::new(), w, false));
        }
        Pres::Done(done) => done_items(done, &mut raw),
    }
    let gap_after = |i: usize| raw[i].0.gap_before(raw[i + 1].0);
    let mut widths: Vec<f32> = raw.iter().map(|r| r.2).collect();
    let span = |widths: &[f32], from: usize| -> f32 {
        widths[from..].iter().sum::<f32>()
            + (from..raw.len().saturating_sub(1))
                .map(gap_after)
                .sum::<f32>()
    };
    // An open list is never narrower than `LIST_W`, and the capsule above
    // it is as wide: the search grows, or ✕ moves out to the list's edge.
    // A card's top line is as wide as the card, as the list's is.
    let listed = matches!(pres, Pres::Done(Done { open: Some(_), .. }));
    let min_w = if listed {
        LIST_W
    } else if !card_picks(pres).is_empty() {
        CARD_W
    } else {
        0.0
    };
    let mut spread = 0.0;
    if let Some(field) = raw.iter().position(|r| r.0 == Id::Field) {
        let grow = (LIST_W - 2.0 * ISLAND_INSET - span(&widths, 0)).max(0.0);
        widths[field] += grow;
    } else if min_w > 0.0 && raw.last().is_some_and(|r| r.0 == Id::Close) {
        spread = (min_w - 2.0 * ISLAND_INSET - span(&widths, 0)).max(0.0);
    }
    let (l, r) = if held_row {
        let row = span(&widths, 1);
        (
            CX - row * 0.5 - gap_after(0) - widths[0] - ISLAND_INSET,
            CX + row * 0.5 + ISLAND_INSET,
        )
    } else {
        let w = span(&widths, 0) + spread + 2.0 * ISLAND_INSET;
        (CX - w * 0.5, CX + w * 0.5)
    };
    let mut x = l + ISLAND_INSET;
    let mut items = Vec::with_capacity(raw.len());
    for (i, (id, ident, _, active)) in raw.iter().enumerate() {
        let w = widths[i];
        items.push(Item {
            id: *id,
            ident: ident.clone(),
            rect: [x, TOP + ISLAND_INSET, w, CHIP_H],
            active: *active,
        });
        if i + 1 < raw.len() {
            x += w + gap_after(i);
        }
        if i + 2 == raw.len() {
            x += spread;
        }
    }
    ([l, TOP, r - l, ISLAND_H], items)
}

/// Where the capsule enters from and leaves to: its own width,
/// [`TOP_FROM`] of its height, or at the notch, the notch.
fn pose(rest: [f32; 4], anchor: Anchor) -> [f32; 4] {
    if anchor == Anchor::Notch {
        return notch::tucked();
    }
    [rest[0], rest[1], rest[2], rest[3] * TOP_FROM]
}

/// Something mounted in the capsule. Its key carries a generation, so a
/// chip that leaves and comes back is a new element entering in place
/// rather than the old one flying back from where it faded.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Shown {
    pub(crate) id: Id,
    pub(crate) ident: String,
    pub(crate) key: String,
    pub(crate) rect: [f32; 4],
    pub(crate) active: bool,
    /// The spring for this element's next layout change.
    pub(crate) motion: Spring,
}

impl Shown {
    fn is(&self, item: &Item) -> bool {
        self.id == item.id && self.ident == item.ident
    }

    /// What it reads: its `ident`, less the lead a note belongs to.
    pub(crate) fn text(&self) -> &str {
        self.ident.split(NOTE_OF).next().unwrap_or_default()
    }

    /// A card row's quiet reason, after its name.
    pub(crate) fn note(&self) -> &str {
        self.ident.split(NOTE_OF).nth(1).unwrap_or_default()
    }
}

pub(crate) struct Surface {
    pub(crate) pres: Pres,
    pub(crate) clock: String,
    pub(crate) reduced: bool,
    /// The capsule's resting rect, whether it shows, and the springs for
    /// its next change.
    pub(crate) shell: [f32; 4],
    pub(crate) shell_on: bool,
    pub(crate) shell_motion: Spring,
    pub(crate) shell_fade: Spring,
    pub(crate) items: Vec<Shown>,
    pub(crate) highlight: Option<Shown>,
    /// The next commit lands with no animation (an entry's origin pose).
    pub(crate) instant: bool,
    /// Counts the width morphs (each one lights the edge glow) and the last
    /// one's settle time.
    pub(crate) morphs: u32,
    pub(crate) morph_settle: f32,
    /// Phase changes for the motion trace.
    pub(crate) events: Vec<String>,
    /// Where the capsule rests (`capture.capsule_flow`): the app moves
    /// the overlay when it changes.
    pub(crate) anchor: Anchor,
    /// An entry's second frame, from the origin pose to rest, and where it
    /// goes.
    entry: Option<(f64, [f32; 4], Vec<Item>)>,
    generation: u64,
}

impl Default for Surface {
    fn default() -> Self {
        Self::new()
    }
}

impl Surface {
    pub(crate) fn new() -> Self {
        let shell = [CX - 60.0, TOP, 120.0, ISLAND_H];
        Self {
            pres: Pres::Hidden,
            clock: "0:00".into(),
            reduced: false,
            shell: pose(shell, Anchor::Top),
            shell_on: false,
            shell_motion: tuning::get().island,
            shell_fade: tuning::get().island_scale,
            items: Vec::new(),
            highlight: None,
            instant: false,
            morphs: 0,
            morph_settle: 0.0,
            events: Vec::new(),
            anchor: Anchor::Top,
            entry: None,
            generation: 0,
        }
    }

    /// `pres` at rest, as a first frame shows it (headless goldens).
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "headless stills and motion runs")
    )]
    pub(crate) fn settled(mut self, pres: Pres) -> Self {
        self.anchor = anchor_for(&pres, self.anchor);
        let (rest, items) = layout_at(&pres, &self.clock, self.anchor);
        self.shell_on = !matches!(pres, Pres::Hidden);
        self.shell = if self.shell_on {
            rest
        } else {
            pose(rest, self.anchor)
        };
        self.items = items
            .iter()
            .map(|item| self.shown(item, tuning::get().island))
            .collect();
        self.highlight = self.highlight_for(&items, tuning::get().island);
        self.pres = pres;
        self
    }

    fn shown(&mut self, item: &Item, motion: Spring) -> Shown {
        self.generation += 1;
        Shown {
            id: item.id,
            ident: item.ident.clone(),
            key: format!("{}#{}", item.id.trace_key(), self.generation),
            rect: item.rect,
            active: item.active,
            motion,
        }
    }

    fn highlight_for(&mut self, items: &[Item], motion: Spring) -> Option<Shown> {
        let active = items.iter().find(|i| i.active)?;
        self.generation += 1;
        Some(Shown {
            id: active.id,
            ident: String::new(),
            key: format!("highlight#{}", self.generation),
            rect: active.rect,
            active: true,
            motion,
        })
    }

    /// The timer reads `clock`. True when the capsule re-lays out for it (a
    /// longer timer); the view draws the digits from a `LiveText`.
    pub(crate) fn set_clock(&mut self, clock: &str, now: f64) -> bool {
        if self.clock == clock {
            return false;
        }
        let resize = (clock_width(&self.clock) - clock_width(clock)).abs() > 0.01;
        self.clock = clock.to_owned();
        if resize && matches!(self.pres, Pres::Recording { .. }) {
            let same = self.pres.clone();
            self.transition(same, now);
        }
        resize
    }

    /// Shows `next`. Retargets keep every element's position and velocity
    /// (Rune's springs).
    pub(crate) fn set(&mut self, next: Pres, now: f64) {
        if next == self.pres {
            return;
        }
        if std::mem::discriminant(&next) != std::mem::discriminant(&self.pres) {
            self.events.push(format!("phase:{}", next.phase()));
        }
        let prev = std::mem::replace(&mut self.pres, next);
        self.transition(prev, now);
    }

    /// Retargets from `prev` to `self.pres`.
    fn transition(&mut self, prev: Pres, now: f64) {
        let next = self.pres.clone();
        self.entry = None;
        self.instant = false;
        let was = self.anchor;
        self.anchor = anchor_for(&next, was);
        // A move to the pointer is a new place: the capsule enters there.
        let moved = was != self.anchor && self.anchor == Anchor::Cursor;

        if matches!(next, Pres::Hidden) {
            // Exit in place: the capsule settles back to its origin pose as
            // it fades, and every item leaves with it. In the menu-bar flow
            // a finished capture folds up into the menu bar instead.
            let exit = Spring::new(tuning::get().island_scale.duration * 0.9, 0.0);
            let fold = tuning::get().flow == CapsuleFlow::MenuBar && matches!(prev, Pres::Done(_));
            self.shell = if fold {
                MENU_FOLD
            } else {
                pose(self.shell, self.anchor)
            };
            if fold {
                self.shell_motion = tuning::get().island;
                self.shell_fade = Spring::new(tuning::get().island.duration, 0.0);
                self.shell_on = false;
                self.items.clear();
                self.highlight = None;
                return;
            }
            self.shell_on = false;
            self.shell_motion = exit;
            self.shell_fade = exit;
            self.items.clear();
            self.highlight = None;
            return;
        }

        let (rest, items) = layout_at(&next, &self.clock, self.anchor);
        if !self.shell_on || moved || matches!(prev, Pres::Hidden) {
            // Entry: this commit puts the capsule at its origin pose,
            // invisibly and instantly; the next frame springs it to rest.
            self.shell = pose(rest, self.anchor);
            self.shell_on = false;
            self.instant = true;
            self.items.clear();
            self.highlight = None;
            // Due after this commit, never in the same poll.
            self.entry = Some((now + 1e-4, rest, items));
            return;
        }
        self.apply(rest, items);
    }

    /// Every change at once, as Dynamic Island's: the walls and the pieces
    /// that stay move on one spring, while what leaves and what comes in
    /// blur, scale and fade over that same move. Nothing waits for a wall
    /// or for another piece.
    fn apply(&mut self, rest: [f32; 4], items: Vec<Item>) {
        let island = tuning::get().island;
        if self.shell_on {
            if (rest[2] - self.shell[2]).abs() > 1.0 && !self.reduced {
                self.morphs = self.morphs.wrapping_add(1);
                self.morph_settle = island.duration;
            }
            self.shell_motion = island;
            self.shell_fade = island;
        } else {
            let entry = tuning::get().island_scale;
            self.shell_motion = entry;
            self.shell_fade = Spring::new(entry.duration * 0.9, 0.0);
        }
        self.shell = rest;
        self.shell_on = true;

        self.items.retain(|s| items.iter().any(|i| s.is(i)));
        for item in &items {
            if let Some(shown) = self.items.iter_mut().find(|s| s.is(item)) {
                shown.motion = island;
                shown.rect = item.rect;
                shown.active = item.active;
            }
        }
        // The highlight rides its chip when the chip travels and trails a
        // change of mode.
        let active = items.iter().find(|i| i.active).cloned();
        match (&mut self.highlight, &active) {
            (Some(h), Some(a)) => {
                h.motion = if h.id == a.id {
                    island
                } else {
                    tuning::get().highlight
                };
                h.id = a.id;
                h.rect = a.rect;
            }
            (h, None) => *h = None,
            (None, Some(_)) => self.highlight = self.highlight_for(&items, island),
        }
        for item in &items {
            if !self.items.iter().any(|s| s.is(item)) {
                let shown = self.shown(item, island);
                self.items.push(shown);
            }
        }
        self.items.sort_by_key(|s| s.id.order());
    }

    /// Runs an entry's second frame when it's due. True when something
    /// changed.
    pub(crate) fn tick(&mut self, now: f64) -> bool {
        if !self
            .entry
            .as_ref()
            .is_some_and(|(due, ..)| *due <= now + 1e-9)
        {
            return false;
        }
        let (_, rest, items) = self.entry.take().expect("due");
        self.apply(rest, items);
        true
    }

    /// When the entry's second frame is due, in clock seconds.
    pub(crate) fn next_due(&self) -> Option<f64> {
        self.entry.as_ref().map(|(due, ..)| *due)
    }
}

/// Where `pres` rests in the flow in use, `was` where it rested before:
/// the cursor's card stays at the pointer through its send and Undo, and
/// a new capture starts back under the menu bar.
pub(crate) fn anchor_for(pres: &Pres, was: Anchor) -> Anchor {
    match tuning::get().flow {
        CapsuleFlow::Notch => Anchor::Notch,
        CapsuleFlow::Cursor => match pres {
            Pres::Done(done) if !done.picks.is_empty() => Anchor::Cursor,
            Pres::Done(_) => was,
            Pres::Hidden if was == Anchor::Cursor => was,
            _ => Anchor::Top,
        },
        CapsuleFlow::List | CapsuleFlow::Toast | CapsuleFlow::MenuBar => Anchor::Top,
    }
}

#[cfg(test)]
mod tests {
    use super::super::flow::{Dest, Footer, Lead, List};
    use super::*;

    fn recording(chips: bool) -> Pres {
        Pres::Recording {
            chips,
            phone: false,
            discarding: false,
        }
    }

    /// Steps the surface like frames at 120 Hz until `until`.
    fn run(s: &mut Surface, from: f64, until: f64) -> f64 {
        let mut now = from;
        while now < until {
            now += 1.0 / 120.0;
            s.tick(now);
        }
        now
    }

    fn ruled(undo: bool) -> Pres {
        Pres::Done(Done {
            why: Some("rule".into()),
            undo,
            ask: Some("Send to…".into()),
            ..Done::plain(Lead::Check, "Sent to SEER")
        })
    }

    fn open() -> Pres {
        Pres::Done(Done {
            open: Some((
                String::new(),
                List {
                    suggested: vec![Dest::new("SEER", "same page")],
                    recent: vec![Dest::new("HOME", "yesterday")],
                    selected: 0,
                    footer: Footer::KeepCopied,
                    always: None,
                },
            )),
            ..Done::plain(Lead::Check, "")
        })
    }

    fn offered() -> Pres {
        Pres::Done(Done {
            text: "Send to".into(),
            picks: vec![
                ("SEER".into(), "same page".into()),
                ("HEX".into(), "last sent".into()),
                ("HOME".into(), "yesterday".into()),
            ],
            ..Done::plain(Lead::Check, "Send to")
        })
    }

    /// The cursor's card: its top line, then a row per guess inside the
    /// walls, at the pointer; its send stays there, and a new capture goes
    /// back under the menu bar.
    #[test]
    fn the_cursor_card_lists_its_guesses_at_the_pointer() {
        tuning::with_flow(CapsuleFlow::Cursor, || {
            let mut s = Surface::new();
            s.set(Pres::Processing, 0.0);
            run(&mut s, 0.0, 1.0);
            assert_eq!(s.anchor, Anchor::Top);
            s.set(offered(), 1.0);
            assert_eq!(s.anchor, Anchor::Cursor);
            assert!(s.instant, "it enters at the pointer, not flying there");
            run(&mut s, 1.0, 2.0);
            let [x, y, w, h] = s.shell;
            assert_eq!([x, y], [CURSOR_X, CURSOR_Y]);
            assert!(w >= CARD_W - 0.5);
            let rows: Vec<&Shown> = s
                .items
                .iter()
                .filter(|i| matches!(i.id, Id::Pick(_)))
                .collect();
            assert_eq!(rows.len(), 3);
            for row in rows {
                let [rx, ry, rw, rh] = row.rect;
                assert!(rx >= x && rx + rw <= x + w && ry >= y + ISLAND_H - 4.0);
                assert!(ry + rh <= y + h, "{:?} inside the card", row.ident);
            }
            s.set(Pres::Done(Done::plain(Lead::Check, "Sent to SEER")), 2.0);
            assert_eq!(s.anchor, Anchor::Cursor);
            s.set(Pres::Hidden, 3.0);
            s.set(recording(false), 4.0);
            assert_eq!(s.anchor, Anchor::Top);
        });
    }

    /// The notch hangs from the top edge: its walls run past it, so only
    /// its bottom corners show, and its items sit just under the edge.
    #[test]
    fn the_notch_hangs_from_the_top_edge() {
        tuning::with_flow(CapsuleFlow::Notch, || {
            let s = Surface::new().settled(recording(false));
            assert_eq!(s.anchor, Anchor::Notch);
            assert!(s.shell[1] < 0.0 && s.shell[1] + s.shell[3] > ISLAND_H - 1.0);
            assert!(
                s.items
                    .iter()
                    .all(|i| (i.rect[1] - ISLAND_INSET).abs() < 1e-3)
            );
            let card = Surface::new().settled(offered());
            assert_eq!(
                card.items
                    .iter()
                    .filter(|i| matches!(i.id, Id::Pick(_)))
                    .count(),
                3
            );
        });
    }

    /// The menu-bar flow's guesses are chips with their numbers, and the
    /// capsule folds up toward the menu bar as it goes.
    #[test]
    fn the_menu_bar_flow_folds_up_toward_the_menu_bar() {
        tuning::with_flow(CapsuleFlow::MenuBar, || {
            let mut s = Surface::new().settled(offered());
            let chips = s
                .items
                .iter()
                .filter(|i| matches!(i.id, Id::Pick(_)))
                .count();
            assert_eq!(chips, 3);
            assert_eq!(s.shell[3], ISLAND_H, "one line, no card");
            s.set(Pres::Hidden, 1.0);
            assert_eq!(s.shell, MENU_FOLD);
            assert!(!s.shell_on);
        });
    }

    #[test]
    fn switching_to_area_keeps_every_chip_still() {
        let (_, rec) = layout(&recording(true), "0:00");
        let (_, pick) = layout(
            &Pres::Selecting {
                mode: Mode::Area,
                phone: false,
            },
            "0:00",
        );
        for item in &pick {
            let before = rec.iter().find(|r| r.id == item.id).expect("same chips");
            assert_eq!(before.rect, item.rect, "{:?} moved", item.id);
        }
    }

    #[test]
    fn layouts_are_inset_gapped_and_fit_the_panel() {
        for pres in [
            recording(false),
            recording(true),
            Pres::Processing,
            Pres::Selecting {
                mode: Mode::Screen,
                phone: false,
            },
            ruled(true),
            open(),
        ] {
            let (shell, items) = layout(&pres, "0:00");
            let first = items.first().unwrap().rect;
            let last = items.last().unwrap().rect;
            assert!((first[0] - shell[0] - ISLAND_INSET).abs() < 1e-3);
            assert!((shell[0] + shell[2] - last[0] - last[2] - ISLAND_INSET).abs() < 1e-3);
            assert!(
                shell[0] >= 0.0 && shell[0] + shell[2] <= PANEL_W,
                "{pres:?}"
            );
            for pair in items.windows(2) {
                let gap = pair[1].rect[0] - pair[0].rect[0] - pair[0].rect[2];
                let want = pair[0].id.gap_before(pair[1].id);
                assert!((gap - want).abs() < 1e-3, "{gap} != {want}");
                assert!(
                    [CHIP_GAP, GROUP_GAP, 0.0, -CLOSE_HUG].contains(&want),
                    "{want}"
                );
            }
        }
        let (shell, _) = layout(&open(), "0:00");
        assert!(
            (shell[2] - LIST_W).abs() < 0.5,
            "the open capsule is the list's width"
        );
    }

    /// The Undo chip is laid out as wide as it's drawn: its label in
    /// MEDIUM, with no shortcut beside it.
    #[test]
    fn the_undo_chip_is_as_wide_as_its_label_is_drawn() {
        let (_, items) = layout(&ruled(true), "0:00");
        let undo = items.iter().find(|i| i.id == Id::Undo).unwrap();
        assert_eq!(undo.rect[2], keyed_width(&[], UNDO_LABEL, MEDIUM));
        assert_ne!(
            keyed_width(&[], UNDO_LABEL, MEDIUM),
            keyed_width(&[], UNDO_LABEL, STRONG)
        );
    }

    #[test]
    fn a_new_label_is_a_new_piece() {
        let mut s = Surface::new().settled(ruled(false));
        let old = s
            .items
            .iter()
            .find(|i| i.id == Id::Done)
            .unwrap()
            .key
            .clone();
        s.set(Pres::Done(Done::plain(Lead::Check, "Taken back")), 1.0);
        run(&mut s, 1.0, 2.0);
        let new = s
            .items
            .iter()
            .find(|i| i.id == Id::Done)
            .unwrap()
            .key
            .clone();
        assert_ne!(old, new, "crossfades rather than swapping text in place");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn entry_lands_the_pose_instantly_then_springs_to_rest_with_its_content() {
        let mut s = Surface::new();
        s.set(recording(true), 0.0);
        assert!(s.instant && !s.shell_on && s.items.is_empty());
        assert!(s.shell[3] < ISLAND_H, "origin pose is shorter");
        s.instant = false;
        assert!(s.tick(0.001));
        assert!(s.shell_on && s.shell[3] == ISLAND_H);
        assert_eq!(s.items.len(), 5, "lead, three chips and close, at once");
        assert!(
            s.highlight
                .as_ref()
                .is_some_and(|h| h.id == Id::Mode(Mode::Screen))
        );
        assert_eq!(s.next_due(), None);
    }

    /// Dynamic Island's way: the walls, what leaves and what comes in all
    /// start on the same frame; nothing waits for anything.
    #[test]
    fn every_change_moves_at_once() {
        let mut s = Surface::new();
        s.set(recording(true), 0.0);
        let now = run(&mut s, 0.0, 1.0);
        let wide = s.shell;
        s.set(recording(false), now);
        assert!(s.shell[2] < wide[2], "the walls move at once");
        assert!(!s.items.iter().any(|i| i.id == Id::Mode(Mode::Area)));
        assert!(s.items.iter().any(|i| i.id == Id::Finish));
        assert_eq!(s.next_due(), None, "nothing is left for later");
        assert!(s.items.iter().all(|i| i.motion == tuning::get().island));

        s.set(Pres::Processing, now + 1.0);
        s.set(ruled(true), now + 2.0);
        assert!(s.items.iter().any(|i| i.id == Id::Close));
        assert!(s.items.iter().any(|i| i.id == Id::Undo));
    }

    #[test]
    fn a_written_copy_offers_paste_and_inspect_as_icons() {
        let copied = Pres::Done(Done {
            hand_off: true,
            ..Done::plain(Lead::Check, "")
        });
        let (_, items) = layout(&copied, "0:00");
        let ids: Vec<Id> = items.iter().map(|i| i.id).collect();
        assert_eq!(ids, [Id::Done, Id::Paste, Id::Inspect, Id::Close]);
        for id in [Id::Paste, Id::Inspect] {
            let icon = items.iter().find(|i| i.id == id).unwrap();
            assert_eq!(icon.rect[2], CHIP_H, "{id:?} is an icon");
        }
    }

    #[test]
    fn shortcuts_read_as_one_quiet_word() {
        assert_eq!(chord(&["⌥", "⇧", "V"]), "⌥⇧V");
        assert_eq!(chord(&["⇧", "⏎"]), "⇧⏎");
        assert_eq!(chord(&["esc"]), "esc");
        assert_eq!(chord(&["fn", "V"]), "fn V");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_mode_change_in_place_slides_the_highlight() {
        let mut s = Surface::new().settled(Pres::Selecting {
            mode: Mode::Screen,
            phone: false,
        });
        let before = s.highlight.clone().unwrap();
        s.set(
            Pres::Selecting {
                mode: Mode::Area,
                phone: false,
            },
            1.0,
        );
        run(&mut s, 1.0, 1.1);
        let after = s.highlight.clone().unwrap();
        assert_eq!(after.key, before.key, "one highlight travels");
        assert_eq!(after.id, Id::Mode(Mode::Area));
        assert_eq!(after.motion, tuning::get().highlight);
    }

    #[test]
    fn an_interrupted_transition_drops_its_pending_steps() {
        let mut s = Surface::new();
        s.set(recording(true), 0.0);
        s.tick(0.001);
        s.set(Pres::Hidden, 0.002);
        run(&mut s, 0.002, 1.0);
        assert!(s.items.is_empty() && !s.shell_on);
    }

    #[test]
    fn long_names_end_in_an_ellipsis() {
        let cut = ellipsize("OpenCode Desktop · Problems board rewrite", MEDIUM, 120.0);
        assert!(
            cut.ends_with('…') && measure(&cut, MEDIUM) <= 120.0,
            "{cut}"
        );
        assert_eq!(ellipsize("HEX", MEDIUM, 120.0), "HEX");
    }
}

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

use super::flow::{Done, Lead, Mode, Pen, Pres};
use super::tuning;
use crate::desktop::i18n::tr;
use crate::rune_hud::spring::REDUCED_FADE;
use crate::rune_ui::controls::{CAPSULE, Paint};

/// The overlay's panel: wide enough for the open list, and tall enough for
/// it and the shadow under it.
pub(crate) const PANEL_W: f32 = 640.0;
pub(crate) const PANEL_H: f32 = 424.0;
/// Panel points the capsule is centred on, and its top.
pub(crate) const CX: f32 = PANEL_W * 0.5;
pub(crate) const TOP: f32 = 22.0;
pub(crate) const ISLAND_H: f32 = CAPSULE.island_h;
pub(crate) const ISLAND_INSET: f32 = CAPSULE.chip_inset;
pub(crate) const CHIP_H: f32 = ISLAND_H - 2.0 * ISLAND_INSET;
/// The red dot's ink spans 12…20 of the lead's 32 pt square; the timer
/// starts 8 pt after it.
pub(crate) const LEAD_TEXT_X: f32 = 28.0;
pub(crate) const PROCESSING_TEXT_X: f32 = 31.5;
const PROCESSING_TRAIL: f32 = 8.5;
/// The check's ink spans 12…20, as the dot's does; its label starts where
/// the timer does.
pub(crate) const DONE_TEXT_X: f32 = LEAD_TEXT_X;
/// The capture's first frame in the check's place: a 28 × 20 card nested
/// in the capsule's left corners, its label 8 pt after it. Centred in the
/// row it clears the top and bottom walls by the shell's radius less its
/// own (10 pt), so it sits as far from the left wall and its corners turn
/// about the shell's centres. The list's 16 pt margin is for words, not
/// for this.
pub(crate) const THUMB_W: f32 = 28.0;
pub(crate) const THUMB_H: f32 = 20.0;
pub(crate) const THUMB_RADIUS: f32 = SHELL_RADIUS - ISLAND_INSET - (CHIP_H - THUMB_H) * 0.5;
pub(crate) const THUMB_X: f32 = SHELL_RADIUS - THUMB_RADIUS - ISLAND_INSET;

/// Where the finished capsule's label starts after its lead.
pub(crate) fn done_text_x(lead: Lead) -> f32 {
    match lead {
        Lead::Thumbnail => THUMB_X + THUMB_W + 8.0,
        Lead::Check | Lead::Warning => DONE_TEXT_X,
    }
}
const CHIP_GAP: f32 = 2.0;
const GROUP_GAP: f32 = 6.0;
/// The finished line's buttons (Paste, Search, Inspect, ✕) draw 2 pt
/// inside their chips: 28 pt boxes 6 pt apart and 6 pt in from the walls.
pub(crate) const BUTTON_INSET: f32 = 2.0;
/// From the finished line's words to Paste's fill.
const PASTE_LEAD: f32 = 16.0;
/// How far ✕ tucks into the action before it.
pub(crate) const CLOSE_HUG: f32 = 6.0;
pub(crate) const CHIP_PX: f32 = 12.0;
/// After the lead's text, equal to the dot's 12 pt inset.
const LEAD_TRAIL: f32 = 12.0;
/// The microphone's level after the timer: three 2 pt bars 2 pt apart, 6 pt
/// after the digits.
pub(crate) const LEVEL_GAP: f32 = 6.0;
pub(crate) const LEVEL_W: f32 = 10.0;
/// A mode chip: its icon, the gap after it, and the padding inside its
/// 28 pt box. The icon's ink sits inside its square, so its side is trimmed
/// to 8 pt and reads as the 10 pt after the word.
pub(crate) const SEG_ICON: f32 = 14.0;
pub(crate) const SEG_ICON_GAP: f32 = 5.0;
pub(crate) const SEG_PL: f32 = 8.0;
pub(crate) const SEG_PR: f32 = 10.0;
/// Between a chip's label and its shortcut.
pub(crate) const HINT_GAP: f32 = 6.0;
/// The split destination chip's leading icon and trailing `⌄` half.
pub(crate) const SEND_ICON: f32 = 14.0;
/// Inside the send button: 8 pt before the arrow (its ink starts 2.5 pt
/// inside its square, so it reads as 10.5), 4 pt to the name, and 10 pt
/// after the name, so the arrow and the name sit evenly between its ends.
pub(crate) const SEND_PL: f32 = 8.0;
pub(crate) const SEND_ICON_GAP: f32 = 4.0;
pub(crate) const SEND_PR: f32 = 10.0;
pub(crate) const CHEVRON_W: f32 = CHIP_H - 2.0 * BUTTON_INSET;
/// Between the split chip's send button and its ⌄ button: two buttons side
/// by side, never one fill with a rule inside it.
pub(crate) const SPLIT_GAP: f32 = 4.0;

/// The open capsule is as wide as the list under it; its field takes what
/// the rest leaves.
pub(crate) const LIST_W: f32 = 360.0;
pub(crate) const LIST_Y: f32 = TOP + ISLAND_H + 6.0;
pub(crate) const LIST_PAD: f32 = CAPSULE.row_inset;
pub(crate) const HEADER_H: f32 = 24.0;
/// A list row, and the search field over them: the capsules' 32 pt action row.
pub(crate) const ROW_H: f32 = CAPSULE.row_h;

/// The capsule is a rounded rect, its corners the list's, so the two
/// match and the square thumbnail sits in a corner that follows it.
pub(crate) const SHELL_RADIUS: f32 = CAPSULE.radius;
/// A chip's corners, concentric with the shell's.
pub(crate) const CHIP_RADIUS: f32 = CAPSULE.inner(CAPSULE.chip_inset);
/// The walls' corners never round more than the one-line capsule's, so
/// the line grows into a card without its corners changing.
pub(crate) fn corner(h: f32) -> f32 {
    (h * 0.5).min(SHELL_RADIUS)
}
/// The kit's type, the same in either appearance: its labels in
/// `ui_medium`, its notes and hints in `small_medium`.
pub(crate) fn medium() -> TextStyle {
    rune_theme_opencode::kit(false).ty.ui_medium
}
pub(crate) fn small() -> TextStyle {
    rune_theme_opencode::kit(false).ty.small_medium
}
/// "Processing…" in the interface's language.
pub(crate) fn processing() -> &'static str {
    tr("Processing…")
}
/// Tabular digits, so a new second never changes the timer's width.
pub(crate) fn clock_style() -> TextStyle {
    Paint::new(false, 1.0).clock()
}

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

/// The finished capsule's label. Both appearances share its metrics.
pub(crate) fn done_style() -> TextStyle {
    medium()
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
pub(crate) fn undo_label() -> &'static str {
    tr("Undo")
}
/// The recording's ✕ asks before it discards.
pub(crate) fn keep_label() -> &'static str {
    tr("Keep recording")
}
pub(crate) fn discard_label() -> &'static str {
    tr("Discard")
}

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
    /// The unified split destination chip (`[→ ACME ⌄]` or `[⏱ ACME ⌄]`).
    Split,
    /// Left half of the split chip: sends immediately to the top suggestion.
    QuickSend,
    /// Right `⌄` half of the split chip: opens the Send-to list.
    More,
    /// Post-send `Always…` button beside Undo.
    Always,
    /// Options inside the inline `Always…` configurator popover.
    AlwaysScopeNarrow,
    AlwaysScopeWide,
    AlwaysToSession,
    AlwaysToProject,
    AlwaysSave,
    /// Opens Hex's Settings window directly to `Modal::RoutingRules`.
    OpenRules,
    /// Pastes the capture.
    Paste,
    /// The Send to… chip.
    Ask,
    Search,
    Inspect,
    /// The open capsule's field.
    Field,
    Row(usize),
    /// The recording's pen, lit while drawing; a click toggles drawing.
    Pen,
    /// The recording's source button beside the pen: it opens Area, Window,
    /// Screen and iPhone on demand, so recording starts at once and a
    /// different target stays one click away.
    Source,
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
            Id::Split => "split",
            Id::QuickSend => "quick-send",
            Id::More => "more",
            Id::Always => "always",
            Id::AlwaysScopeNarrow => "always-scope-narrow",
            Id::AlwaysScopeWide => "always-scope-wide",
            Id::AlwaysToSession => "always-to-session",
            Id::AlwaysToProject => "always-to-project",
            Id::AlwaysSave => "always-save",
            Id::OpenRules => "open-rules",
            Id::Paste => "paste",
            Id::Ask => "ask",
            Id::Search => "search",
            Id::Inspect => "inspect",
            Id::Field => "field",
            Id::Row(_) => "row",
            Id::Pen => "pen",
            Id::Source => "source",
        }
    }

    /// Whether a lit chip of this kind wears the sliding highlight: the mode
    /// chips, whose selection travels. Every other lit button fills its own
    /// box, and a queued split chip is two buttons, never one fill.
    const fn lights(self) -> bool {
        matches!(self, Id::Mode(_))
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
    /// `self_active` / `next_active` distinguish a filled pill `Id::Ask`
    /// (Allow, Restart Hex, Try again) from plain text asks.
    fn gap_before(self, self_active: bool, next: Id, next_active: bool) -> f32 {
        let next_button = matches!(next, Id::Paste | Id::Undo | Id::Split | Id::Always)
            || (next == Id::Ask && next_active);
        let self_button = matches!(self, Id::Paste | Id::Undo | Id::Split | Id::Always)
            || (self == Id::Ask && self_active);
        match (self, next) {
            (Id::Done, Id::Why) => CHIP_GAP,
            // The field's well starts the label's trail after it: 12 pt,
            // as the lead's inset is.
            (Id::Done | Id::Why, Id::Field) => 0.0,
            // Button fills (Paste, Undo, Allow, Restart Hex, Try again)
            // start 16 pt after the words, as far as the lead sits from the
            // wall and ✕ from the other: the words' trail, this, and the
            // box's 2 pt inset in its chip.
            (Id::Done, _) if next_button => PASTE_LEAD - LEAD_TRAIL - BUTTON_INSET,
            (Id::Why, _) if next_button => PASTE_LEAD - LEAD_TRAIL * 0.5 - BUTTON_INSET,
            // The label's trail and the action's padding already part
            // them.
            (Id::Done | Id::Why, _) => GROUP_GAP,
            // The recording's pen, source, stop and ✕ are one even row: 28 pt
            // boxes 6 pt apart, with no rule and no wider step between them
            // (the same-group arm below).
            // Boxed buttons and icons keep one even step beside ✕.
            (_, Id::Close) if self_button => CHIP_GAP,
            (Id::Inspect | Id::Search, Id::Close) => CHIP_GAP,
            // The picker's line is one row of boxes too: the lit mode,
            // Record and ✕ stand 6 pt apart, so ✕ sits midway between the
            // fill before it and the wall.
            (Id::Mode(_) | Id::Record, Id::Close) | (Id::Mode(_), Id::Record) => CHIP_GAP,
            // ✕'s circle has air of its own, so it tucks into a plain worded
            // action's right padding; drawn later, it takes the overlap's
            // presses.
            (Id::Ask, Id::Close) => -CLOSE_HUG,
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
            Id::Why | Id::Pen | Id::Source => 3,
            Id::Mode(Mode::Area) => 4,
            Id::Mode(Mode::Window) => 5,
            Id::Mode(Mode::Screen) => 6,
            Id::Mode(Mode::Iphone) => 7,
            Id::Record => 8,
            Id::Cancel | Id::Keep => 9,
            Id::Finish | Id::Discard => 10,
            Id::Field => 11,
            Id::Always => 12,
            Id::Undo => 13,
            Id::Split => 14,
            Id::Paste => 14,
            Id::Inspect => 15,
            Id::QuickSend
            | Id::More
            | Id::AlwaysScopeNarrow
            | Id::AlwaysScopeWide
            | Id::AlwaysToSession
            | Id::AlwaysToProject
            | Id::AlwaysSave
            | Id::OpenRules => 22,
            Id::Ask | Id::Search => 18,
            Id::Close => 20,
            Id::Row(_) => 21,
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
    measure(mode.label(), medium()) + SEG_PL + SEG_PR + SEG_ICON + SEG_ICON_GAP + 2.0 * BUTTON_INSET
}

fn chip_width(label: f32) -> f32 {
    label + 2.0 * CHIP_PX
}

#[cfg(test)]
thread_local! { pub(super) static FIXTURE_SHORTCUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

/// The capture shortcut's keycaps, as Settings draws them.
pub(crate) fn shortcut_keycaps() -> Vec<String> {
    #[cfg(test)]
    if FIXTURE_SHORTCUT.get() {
        return ["⌥", "⇧", "S"].map(str::to_owned).into();
    }
    crate::app_settings::capture_hotkey_keycaps()
}

/// Width of the left send/queued half inside the split destination chip.
pub(crate) fn split_send_width(name: &str) -> f32 {
    SEND_PL + SEND_ICON + SEND_ICON_GAP + measure(name, medium()) + SEND_PR
}

/// Total chip width of the split destination chip: the send button, a
/// 4 pt gap and the square ⌄ button.
pub(crate) fn split_width(name: &str) -> f32 {
    split_send_width(name) + SPLIT_GAP + CHEVRON_W + 2.0 * BUTTON_INSET
}

/// A chip's label, then its shortcut in quiet text, or a plain chip
/// without one.
pub(crate) fn keyed_width(keys: &[&str], label: &str, style: TextStyle) -> f32 {
    if keys.is_empty() {
        return chip_width(measure(label, style));
    }
    chip_width(measure(label, style) + HINT_GAP + measure(&chord(keys), small()))
}

pub(crate) fn clock_width(clock: &str) -> f32 {
    measure(clock, clock_style())
}

/// The widest a row gets: the panel, less room for the shadow.
const ROW_MAX: f32 = PANEL_W - 32.0;

/// A row's width from its items' widths, with its gaps and insets.
fn row_width(raw: &[(Id, String, f32, bool)]) -> f32 {
    let gaps: f32 = raw
        .windows(2)
        .map(|pair| pair[0].0.gap_before(pair[0].3, pair[1].0, pair[1].3))
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
    if done.text.is_empty() {
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
            measure(why, small()) + LEAD_TRAIL * 0.5,
            false,
        ));
    }
    // Searching, the capsule keeps its own line (`Copied [→ SEER | ⌄] ✕`):
    // the search field lives in the list under it.
    // One action at most, the most urgent: take a send back, paste or
    // inspect the capture (two quiet icons), or open the list. Where to
    // send it lives in the list under the capsule, never in chips beside
    // the label.
    let listed = done.open.is_some();
    if let Some(lit) = done.always_button {
        let label = tr("Always…");
        raw.push((
            Id::Always,
            label.into(),
            keyed_width(&[], label, medium()),
            lit,
        ));
    }
    if done.undo {
        raw.push((
            Id::Undo,
            String::new(),
            keyed_width(&[], undo_label(), medium()),
            false,
        ));
    } else if let Some(split) = &done.split {
        let name = ellipsize(&split.name, medium(), cap);
        raw.push((
            Id::Split,
            name.clone(),
            split_width(&name),
            split.kind == super::flow::SplitKind::Queued,
        ));
    } else {
        if done.paste {
            let label = tr("Paste");
            raw.push((
                Id::Paste,
                label.into(),
                keyed_width(&[], label, medium()).max(44.0),
                false,
            ));
        }
        // A filled primary ask (Allow, Restart Hex, Try again) sits in
        // Paste's slot before any icon button (Inspect).
        if let Some(ask) = done
            .ask
            .as_ref()
            .filter(|_| done.ask_button && (!listed || done.ask_keys))
        {
            let width = keyed_width(&[], ask, medium()).max(44.0);
            raw.push((Id::Ask, ask.clone(), width, true));
        }
        if done.inspect && !done.inspect_preview() {
            raw.push((Id::Inspect, String::new(), CHIP_H, false));
        }
        // Where to send it is a click away, beside Paste, never opened for you.
        // Over the suggestions it's Search, with the capture shortcut that
        // opens it.
        if let Some(ask) = done
            .ask
            .as_ref()
            .filter(|_| !done.ask_button && (!listed || done.ask_keys))
        {
            if listed && done.ask_keys {
                raw.push((Id::Search, ask.clone(), CHIP_H, false));
            } else {
                // A shortcut ask (Annotate ⌥⇧S) is a filled button, its own
                // box beside ✕ rather than words ✕ tucks into.
                let width = if done.ask_keys {
                    let keys = shortcut_keycaps();
                    let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
                    keyed_width(&keys, ask, medium()) + 2.0 * BUTTON_INSET
                } else {
                    keyed_width(&[], ask, medium())
                };
                raw.push((Id::Ask, ask.clone(), width, done.ask_keys));
            }
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
    layout_at(pres, clock, false)
}

/// A `metered` recording shows its microphone's level after the timer.
pub(crate) fn layout_at(pres: &Pres, clock: &str, metered: bool) -> ([f32; 4], Vec<Item>) {
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
                    chip_width(measure(tr("Record"), medium())),
                    false,
                ));
            }
            raw.push((Id::Close, String::new(), CHIP_H, false));
        }
        Pres::Recording {
            chips,
            phone,
            discarding,
            pen,
        } => {
            let level = if metered { LEVEL_GAP + LEVEL_W } else { 0.0 };
            let lead = LEAD_TEXT_X + clock_width(clock) + level + LEAD_TRAIL;
            raw.push((Id::Lead, String::new(), lead, false));
            // The chips' row lines up with the picker's, so the pen waits
            // for them to fold.
            if *discarding {
                raw.push((
                    Id::Keep,
                    String::new(),
                    chip_width(measure(keep_label(), medium())),
                    false,
                ));
                raw.push((
                    Id::Discard,
                    String::new(),
                    chip_width(measure(discard_label(), medium())),
                    false,
                ));
            } else if *chips {
                held_row = true;
                modes(&mut raw, Mode::Screen, *phone);
                raw.push((Id::Close, String::new(), CHIP_H, false));
            } else {
                // Icons only, each a square as tall as the row: the
                // toolbar (the pen, lit while drawing, and the source that
                // opens Area / Window / Screen / iPhone), then stop and
                // cancel, parted by space, with no divider.
                raw.push((Id::Pen, String::new(), CHIP_H, *pen == Pen::On));
                raw.push((Id::Source, String::new(), CHIP_H, false));
                raw.push((Id::Finish, String::new(), CHIP_H, false));
                raw.push((Id::Cancel, String::new(), CHIP_H, false));
            }
        }
        Pres::Processing => {
            let w = PROCESSING_TEXT_X + measure(processing(), medium()) + PROCESSING_TRAIL;
            raw.push((Id::Processing, String::new(), w, false));
        }
        Pres::Done(done) => done_items(done, &mut raw),
    }
    let gap_after = |i: usize| raw[i].0.gap_before(raw[i].3, raw[i + 1].0, raw[i + 1].3);
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
    let configuring = matches!(
        pres,
        Pres::Done(Done {
            always_popover: Some(_),
            ..
        })
    );
    let min_w = if listed {
        LIST_W
    } else if configuring {
        super::always_view::POP_W
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
    // The room a wide list leaves goes between the label and the actions,
    // so they and ✕ stay one cluster at the right.
    let cluster = raw
        .iter()
        .position(|r| r.0.group() != 0)
        .unwrap_or(raw.len().saturating_sub(1));
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
        if i + 1 == cluster {
            x += spread;
        }
    }
    ([l, TOP, r - l, ISLAND_H], items)
}

/// Where the capsule enters from and leaves to: its own width,
/// [`TOP_FROM`] of its height.
fn pose(rest: [f32; 4]) -> [f32; 4] {
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
}

pub(crate) struct Surface {
    pub(crate) pres: Pres,
    pub(crate) clock: String,
    /// A recording shows its microphone's level after the timer.
    pub(crate) metered: bool,
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
            metered: false,
            reduced: false,
            shell: pose(shell),
            shell_on: false,
            shell_motion: tuning::get().island,
            shell_fade: tuning::get().island_scale,
            items: Vec::new(),
            highlight: None,
            instant: false,
            morphs: 0,
            morph_settle: 0.0,
            events: Vec::new(),
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
        let (rest, items) = layout_at(&pres, &self.clock, self.metered);
        self.shell_on = !matches!(pres, Pres::Hidden);
        self.shell = if self.shell_on { rest } else { pose(rest) };
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
        let active = items.iter().find(|i| i.active && i.id.lights())?;
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

    /// Whether a recording shows its microphone's level. True when the
    /// capsule re-lays out for it.
    pub(crate) fn set_metered(&mut self, metered: bool, now: f64) -> bool {
        if self.metered == metered {
            return false;
        }
        self.metered = metered;
        let recording = matches!(self.pres, Pres::Recording { .. });
        if recording {
            let same = self.pres.clone();
            self.transition(same, now);
        }
        recording
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

        if matches!(next, Pres::Hidden) {
            // Exit in place: the capsule settles back to its origin pose as
            // it fades, and every item leaves with it. Under Reduce Motion
            // it stays where it is and only fades.
            if !self.reduced {
                self.shell = pose(self.shell);
            }
            let motion = tuning::get().island_scale;
            self.shell_on = false;
            self.shell_motion = motion;
            self.shell_fade = self.fade(motion);
            self.items.clear();
            self.highlight = None;
            return;
        }

        let (rest, items) = layout_at(&next, &self.clock, self.metered);
        if !self.shell_on || matches!(prev, Pres::Hidden) {
            // Entry: this commit puts the capsule at its origin pose,
            // invisibly and instantly; the next frame springs it to rest.
            // Under Reduce Motion it starts at rest and only fades in.
            self.shell = if self.reduced { rest } else { pose(rest) };
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
            self.shell_fade = self.fade(island);
        } else {
            let entry = tuning::get().island_scale;
            self.shell_motion = entry;
            self.shell_fade = self.fade(entry);
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
        let active = items.iter().find(|i| i.active && i.id.lights()).cloned();
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

    /// The shell's fade beside `motion`: its time with no bounce, as the
    /// HUD fades, or under Reduce Motion the HUD's short fade.
    fn fade(&self, motion: Spring) -> Spring {
        if self.reduced {
            tuning::spring(REDUCED_FADE)
        } else {
            Spring::new(motion.duration, 0.0)
        }
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

#[cfg(test)]
mod tests {
    use super::super::flow::{Dest, Footer, Lead, List};
    use super::*;

    fn recording(chips: bool) -> Pres {
        Pres::Recording {
            chips,
            phone: false,
            discarding: false,
            pen: super::super::flow::Pen::Off,
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
                rune::TextEdit::default(),
                List {
                    rows: vec![
                        Dest::new("SEER", "same page"),
                        Dest::new("HOME", "yesterday"),
                    ],
                    title: "Send to",
                    min_rows: 0,
                    selected: 0,
                    target: Some(0),
                    footer: Footer::Search { remember: false },
                    always: None,
                    queued: false,
                    suggested: 1,
                },
            )),
            ..Done::plain(Lead::Check, "")
        })
    }

    /// A recording that shows its microphone's level makes room for it
    /// after the timer, and only then.
    #[test]
    fn a_metered_recording_makes_room_for_its_level() {
        let (plain, _) = layout_at(&recording(false), "0:07", false);
        let (metered, _) = layout_at(&recording(false), "0:07", true);
        assert!((metered[2] - plain[2] - (LEVEL_GAP + LEVEL_W)).abs() < 1e-3);
        let (processing, _) = layout_at(&Pres::Processing, "0:07", false);
        assert_eq!(layout_at(&Pres::Processing, "0:07", true).0, processing);
    }

    /// Under Reduce Motion the capsule never travels: it comes in at rest,
    /// goes out where it is, and only fades, on the HUD's short fade. No
    /// change of width lights the edge glow.
    #[test]
    fn reduce_motion_only_fades_the_capsule() {
        let mut s = Surface::new();
        s.reduced = true;
        s.set(recording(false), 0.0);
        let (rest, _) = layout_at(&s.pres, &s.clock, s.metered);
        assert_eq!(s.shell, rest, "it enters at rest");
        let now = run(&mut s, 0.0, 1.0);
        assert!(s.shell_on);
        assert_eq!(s.shell_fade, tuning::spring(REDUCED_FADE));
        s.set(Pres::Processing, now);
        s.set(Pres::Done(Done::plain(Lead::Check, "Copied")), now + 1.0);
        assert_eq!(s.morphs, 0, "it lights no glow");
        let shown = s.shell;
        s.set(Pres::Hidden, now + 2.0);
        assert_eq!(s.shell, shown, "it leaves where it is");
        assert!(!s.shell_on);
        assert_eq!(s.shell_fade, tuning::spring(REDUCED_FADE));
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
                let want = pair[0]
                    .id
                    .gap_before(pair[0].active, pair[1].id, pair[1].active);
                // An open list's capsule is the list's width: the room that
                // leaves goes after the label, before the actions.
                let spread = matches!(pres, Pres::Done(Done { open: Some(_), .. }))
                    && matches!(pair[0].id, Id::Done | Id::Why);
                assert!(
                    (gap - want).abs() < 1e-3 || (spread && gap > want),
                    "{gap} != {want}"
                );
                assert!(
                    [
                        CHIP_GAP,
                        CHIP_GAP + 2.0,
                        GROUP_GAP,
                        PASTE_LEAD - LEAD_TRAIL * 0.5 - BUTTON_INSET,
                        0.0,
                        -CLOSE_HUG,
                    ]
                    .contains(&want),
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
    /// the kit's `ui_medium`, with no shortcut beside it.
    #[test]
    fn the_undo_chip_is_as_wide_as_its_label_is_drawn() {
        let (_, items) = layout(&ruled(true), "0:00");
        let undo = items.iter().find(|i| i.id == Id::Undo).unwrap();
        assert_eq!(undo.rect[2], keyed_width(&[], undo_label(), medium()));
        assert_ne!(
            keyed_width(&[], undo_label(), medium()),
            keyed_width(&[], undo_label(), small())
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
    fn a_written_copy_has_room_for_the_paste_label_beside_inspect() {
        let copied = Pres::Done(Done {
            paste: true,
            inspect: true,
            ..Done::plain(Lead::Check, "")
        });
        let (_, items) = layout(&copied, "0:00");
        let ids: Vec<Id> = items.iter().map(|i| i.id).collect();
        assert_eq!(ids, [Id::Done, Id::Paste, Id::Inspect, Id::Close]);
        let paste = &items[1];
        assert_eq!(paste.ident, tr("Paste"));
        assert_eq!(paste.rect[2], chip_width(measure(&paste.ident, medium())));
        assert!(paste.rect[0] + paste.rect[2] < items[2].rect[0]);
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
        let cut = ellipsize("OpenCode Desktop · Problems board rewrite", medium(), 120.0);
        assert!(
            cut.ends_with('…') && measure(&cut, medium()) <= 120.0,
            "{cut}"
        );
        assert_eq!(ellipsize("HEX", medium(), 120.0), "HEX");
    }
}

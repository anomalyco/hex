//! Where the capture capsule's pieces rest and how each change moves,
//! decided in Rust: [`layout`] places what [`Pres`] shows, and [`Surface`]
//! keeps what is mounted, its keys, and the spring each piece moves on
//! next. The view only draws it; Rune animates between the targets.
//!
//! Ported from Scrawl's Rune capsule (`scrawl-pill-lab`, `rune_pill`).
//! Timing a spring can't express lives here as pending steps the overlay's
//! poll runs: narrowing, the walls start [`tuning::Tuning::shell_wait_s`] after the content
//! so leaving chips are already fading when a wall reaches them, and an
//! entering chip mounts once the walls have room for it, predicted from the
//! same closed-form spring Rune animates the walls on.

use std::cell::RefCell;

use rune::{Spring, TextStyle};

use super::flow::{Done, Mode, Pres};
use super::tuning;
use crate::rune_ui::controls::KeySize;

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
const CHIP_GAP: f32 = 2.0;
const GROUP_GAP: f32 = 6.0;
pub(crate) const CHIP_PX: f32 = 12.0;
/// After the lead's text, equal to the dot's 12 pt inset.
const LEAD_TRAIL: f32 = 12.0;
/// A mode chip: its icon, the gap after it, and the padding either side.
pub(crate) const SEG_ICON: f32 = 14.0;
pub(crate) const SEG_ICON_GAP: f32 = 5.0;
const SEG_PX: f32 = 10.0;
/// Between a chip's keycaps and its label.
pub(crate) const KEYCAP_GAP: f32 = 6.0;

/// The open capsule is as wide as the list under it; its field takes what
/// the rest leaves.
pub(crate) const LIST_W: f32 = 420.0;
const FIELD_MIN: f32 = 120.0;
pub(crate) const LIST_Y: f32 = TOP + ISLAND_H + 6.0;
pub(crate) const LIST_PAD: f32 = 6.0;
pub(crate) const HEADER_H: f32 = 24.0;
pub(crate) const ROW_H: f32 = 30.0;
pub(crate) const FOOTER_H: f32 = 28.0;

pub(crate) const MEDIUM: TextStyle = TextStyle::new(13.0, 530.0);
pub(crate) const STRONG: TextStyle = TextStyle::new(13.0, 600.0);
/// Tabular digits, so a new second never changes the timer's width.
pub(crate) const TIMER: TextStyle = STRONG.features("'tnum' 1");
pub(crate) const SMALL: TextStyle = TextStyle::new(11.5, 530.0);
pub(crate) const PROCESSING: &str = "Processing…";

/// The Ask chip's keycaps: the send-to list's shortcut
/// (`capture.list_hotkey`); none while it has none.
pub(crate) fn ask_keys() -> Vec<String> {
    crate::app_settings::list_keycaps()
}

/// The capsule unfolds from this fraction of its height, anchored on its
/// top edge.
const TOP_FROM: f32 = 0.7;
const ENTRY_CONTENT_DELAY_S: f32 = 0.016;
/// Opening, sliding content takes this many times the walls' duration.
const FOLLOW: f32 = 1.15;

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

/// A chord's small keycaps in their tray, as `controls::keycaps` draws
/// them.
pub(crate) fn keycaps_width(keys: &[&str]) -> f32 {
    let size = KeySize::Small;
    size.tray_width(keys.iter().map(|key| measure(key, size.legend())))
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
    /// Pastes the capture: the one accent action.
    Paste,
    /// A route chip, best first.
    Route(usize),
    /// The send-to list's chip (⌥⇧V).
    Ask,
    Inspect,
    /// The open capsule's field.
    Field,
    Row(usize),
    /// The suggestions' "Always send <site> here".
    Always,
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
            Id::Route(0) => "route0",
            Id::Route(1) => "route1",
            Id::Route(2) => "route2",
            Id::Route(_) => "route3",
            Id::Ask => "ask",
            Id::Inspect => "inspect",
            Id::Field => "field",
            Id::Row(_) => "row",
            Id::Always => "always",
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
            Id::Route(i) => 14 + i.min(3) as u8,
            Id::Ask => 18,
            Id::Inspect => 19,
            Id::Close => 20,
            Id::Row(_) | Id::Always => 21,
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

/// A chip with keycaps before its label, or a plain chip without any.
pub(crate) fn keyed_width(keys: &[&str], label: &str, style: TextStyle) -> f32 {
    if keys.is_empty() {
        return chip_width(measure(label, style));
    }
    keycaps_width(keys) + KEYCAP_GAP + measure(label, style) + 2.0 * CHIP_PX - 4.0
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
        .map(|pair| {
            if pair[0].0.group() == pair[1].0.group() {
                CHIP_GAP
            } else {
                GROUP_GAP
            }
        })
        .sum();
    raw.iter().map(|r| r.2).sum::<f32>() + gaps + 2.0 * ISLAND_INSET
}

/// The finished capsule's items. When long session names would take the
/// row past [`ROW_MAX`], the label and route chips give way together, cut
/// to a shrinking width until it fits. Names cut shorter than
/// `ROUTE_CUT_MIN` stop reading as names, so the last route chip goes
/// instead and the cut starts over; the only one left may be cut to
/// `CUT_MIN`.
fn done_items(done: &Done, raw: &mut Vec<(Id, String, f32, bool)>) {
    const ROUTE_CUT_MIN: f32 = 80.0;
    const CUT_MIN: f32 = 48.0;
    let start = raw.len();
    let mut routes = done.routes.len();
    let mut cap = f32::INFINITY;
    loop {
        done_row(done, &done.routes[..routes], cap, raw);
        let fits = row_width(&raw[start..]) <= ROW_MAX;
        if fits || (cap <= CUT_MIN && routes <= 1) {
            return;
        }
        if cap <= ROUTE_CUT_MIN && routes > 1 {
            routes -= 1;
            cap = f32::INFINITY;
        } else {
            cap = if cap.is_finite() { cap - 8.0 } else { 200.0 };
        }
        raw.truncate(start);
    }
}

fn done_row(done: &Done, routes: &[String], cap: f32, raw: &mut Vec<(Id, String, f32, bool)>) {
    let text = ellipsize(&done.text, STRONG, cap + 60.0);
    raw.push((
        Id::Done,
        text.clone(),
        DONE_TEXT_X + measure(&text, STRONG) + LEAD_TRAIL,
        false,
    ));
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
    if done
        .open
        .as_ref()
        .is_some_and(|(_, list)| !list.suggesting())
    {
        raw.push((Id::Field, String::new(), FIELD_MIN, false));
    }
    if done.undo {
        raw.push((
            Id::Undo,
            String::new(),
            keyed_width(&[], UNDO_LABEL, MEDIUM),
            false,
        ));
    }
    if done.hand_off {
        raw.push((
            Id::Paste,
            String::new(),
            chip_width(measure("Paste", STRONG)),
            false,
        ));
    }
    for (i, route) in routes.iter().enumerate() {
        // The first chip's "Send to " doesn't count against its name.
        let verb = if i == 0 {
            measure("Send to ", MEDIUM)
        } else {
            0.0
        };
        let route = ellipsize(route, MEDIUM, cap + verb);
        let w = chip_width(measure(&route, MEDIUM));
        raw.push((Id::Route(i), route, w, false));
    }
    if let Some(ask) = &done.ask {
        raw.push((
            Id::Ask,
            ask.clone(),
            keyed_width(
                &done.ask_keys.iter().map(String::as_str).collect::<Vec<_>>(),
                ask,
                MEDIUM,
            ),
            false,
        ));
    }
    if done.hand_off {
        raw.push((
            Id::Inspect,
            String::new(),
            chip_width(measure("Inspect", MEDIUM)),
            false,
        ));
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
pub(crate) fn layout(pres: &Pres, clock: &str) -> ([f32; 4], Vec<Item>) {
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
    let gap_after = |i: usize| {
        if raw[i].0.group() == raw[i + 1].0.group() {
            CHIP_GAP
        } else {
            GROUP_GAP
        }
    };
    let mut widths: Vec<f32> = raw.iter().map(|r| r.2).collect();
    let span = |widths: &[f32], from: usize| -> f32 {
        widths[from..].iter().sum::<f32>()
            + (from..raw.len().saturating_sub(1))
                .map(gap_after)
                .sum::<f32>()
    };
    if let Some(field) = raw.iter().position(|r| r.0 == Id::Field) {
        let grow = (LIST_W - 2.0 * ISLAND_INSET - span(&widths, 0)).max(0.0);
        widths[field] += grow;
    }
    let (l, r) = if held_row {
        let row = span(&widths, 1);
        (
            CX - row * 0.5 - gap_after(0) - widths[0] - ISLAND_INSET,
            CX + row * 0.5 + ISLAND_INSET,
        )
    } else {
        let w = span(&widths, 0) + 2.0 * ISLAND_INSET;
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
    }
    ([l, TOP, r - l, ISLAND_H], items)
}

/// Where the capsule enters from and leaves to: its own width,
/// [`TOP_FROM`] of its height.
fn pose(rest: [f32; 4]) -> [f32; 4] {
    [rest[0], rest[1], rest[2], rest[3] * TOP_FROM]
}

/// One edge of the capsule, followed on the same closed-form spring Rune
/// animates it on, so an entering chip can wait for the wall to pass it.
#[derive(Clone, Copy, Debug)]
struct Wall {
    x0: f32,
    v0: f32,
    target: f32,
    t0: f64,
    spring: Spring,
}

impl Wall {
    fn at_rest(x: f32) -> Self {
        Self {
            x0: x,
            v0: 0.0,
            target: x,
            t0: 0.0,
            spring: tuning::get().highlight,
        }
    }

    fn at(&self, t: f64) -> (f32, f32) {
        let dt = (t - self.t0).max(0.0) as f32;
        if (self.x0 - self.target).abs() < 1e-4 && self.v0.abs() < 1e-4 {
            return (self.target, 0.0);
        }
        self.spring.sample(self.x0, self.v0, self.target, dt)
    }

    fn to(&mut self, t: f64, target: f32, spring: Spring) {
        let (x, v) = self.at(t);
        *self = Self {
            x0: x,
            v0: v,
            target,
            t0: t,
            spring,
        };
    }

    fn snap(&mut self, t: f64, x: f32) {
        *self = Self {
            x0: x,
            v0: 0.0,
            target: x,
            t0: t,
            spring: self.spring,
        };
    }
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

#[derive(Clone, Debug)]
enum Step {
    /// The capsule's walls narrowing, after the content has started
    /// ([`tuning::Tuning::shell_wait_s`]).
    Shell([f32; 4], Spring),
    /// The second frame of an entry: from the origin pose to rest.
    Entry,
    Mount(Shown),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    ChipsOpen,
    ChipsClose,
    Switch,
    Phase,
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
    pending: Vec<(f64, u64, Step)>,
    target: Option<([f32; 4], Vec<Item>, Change)>,
    walls: [Wall; 3],
    generation: u64,
    seq: u64,
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
            pending: Vec::new(),
            target: None,
            walls: [
                Wall::at_rest(shell[0]),
                Wall::at_rest(shell[0] + shell[2]),
                Wall::at_rest(TOP + shell[3] * TOP_FROM),
            ],
            generation: 0,
            seq: 0,
        }
    }

    /// `pres` at rest, as a first frame shows it (headless goldens).
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "headless stills and motion runs")
    )]
    pub(crate) fn settled(mut self, pres: Pres) -> Self {
        let (rest, items) = layout(&pres, &self.clock);
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

    /// The fade an element enters with (it leaves on [`tuning::Tuning::presence_exit`]).
    pub(crate) fn presence(&self) -> Spring {
        Spring::new(tuning::get().reveal_enter_s * 1.6, 0.0)
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
        self.seq += 1;
        let t = now;
        self.pending.clear();
        self.target = None;
        self.instant = false;

        if matches!(next, Pres::Hidden) {
            // Exit in place: the capsule settles back to its origin pose as
            // it fades, and every item leaves with it.
            let exit = Spring::new(tuning::get().island_scale.duration * 0.9, 0.0);
            self.shell = pose(self.shell);
            self.shell_on = false;
            self.shell_motion = exit;
            self.shell_fade = exit;
            self.walls[2].to(t, self.shell[1] + self.shell[3], exit);
            self.items.clear();
            self.highlight = None;
            return;
        }

        let (rest, items) = layout(&next, &self.clock);
        if !self.shell_on || matches!(prev, Pres::Hidden) {
            // Entry: this commit puts the capsule at its origin pose,
            // invisibly and instantly; the next frame springs it to rest.
            self.shell = pose(rest);
            self.shell_on = false;
            self.instant = true;
            self.items.clear();
            self.highlight = None;
            let walls = [rest[0], rest[0] + rest[2], self.shell[1] + self.shell[3]];
            for (wall, x) in self.walls.iter_mut().zip(walls) {
                wall.snap(t, x);
            }
            self.target = Some((rest, items, Change::Phase));
            // Due after this commit, never in the same poll.
            self.pending.push((now + 1e-4, self.seq, Step::Entry));
            return;
        }

        let change = match (&prev, &next) {
            (Pres::Recording { chips: false, .. }, Pres::Recording { chips: true, .. }) => {
                Change::ChipsOpen
            }
            (Pres::Recording { chips: true, .. }, Pres::Recording { chips: false, .. }) => {
                Change::ChipsClose
            }
            (Pres::Recording { .. }, Pres::Selecting { .. }) => Change::Switch,
            _ => Change::Phase,
        };
        let leaving = self.items.iter().any(|s| !items.iter().any(|i| s.is(i)));
        self.items.retain(|s| items.iter().any(|i| s.is(i)));
        if !items.iter().any(|i| i.active) {
            self.highlight = None;
        }
        let narrowing = rest[2] < self.shell[2] - 1.0;
        self.target = Some((rest, items, change));
        let wait = if narrowing && leaving && !self.reduced {
            tuning::get().shell_wait_s
        } else {
            0.0
        };
        self.apply_geometry(now, wait);
    }

    /// The walls (after `wait` animation seconds) and staying items move to
    /// the target; entering items are scheduled for when the walls have
    /// room for them.
    fn apply_geometry(&mut self, now: f64, wait: f32) {
        let Some((rest, items, change)) = self.target.take() else {
            return;
        };
        let t = now;
        let opening = rest[2] >= self.shell[2] - 1.0;
        let (shell_spring, item_spring) = match change {
            Change::ChipsOpen => (
                tuning::get().reveal_open,
                Spring::new(tuning::get().reveal_open.duration * FOLLOW, 0.0),
            ),
            Change::ChipsClose => (
                tuning::get().reveal_close,
                Spring::new(tuning::get().reveal_close.duration, 0.0),
            ),
            Change::Switch => (
                tuning::get().switch_width,
                Spring::new(tuning::get().switch_slide.duration, 0.0),
            ),
            Change::Phase => (
                tuning::get().island,
                Spring::new(
                    tuning::get().island.duration * if opening { FOLLOW } else { 1.0 },
                    0.0,
                ),
            ),
        };
        let entry = !self.shell_on;
        if entry {
            self.shell = rest;
            self.shell_motion = tuning::get().island_scale;
            self.shell_fade = Spring::new(tuning::get().island_scale.duration * 0.9, 0.0);
            self.walls[2].to(t, rest[1] + rest[3], tuning::get().island_scale);
        } else {
            if (rest[2] - self.shell[2]).abs() > 1.0 && !self.reduced {
                self.morphs = self.morphs.wrapping_add(1);
                self.morph_settle = shell_spring.duration;
            }
            let at = t + f64::from(wait);
            self.walls[0].to(at, rest[0], shell_spring);
            self.walls[1].to(at, rest[0] + rest[2], shell_spring);
            self.walls[2].to(at, rest[1] + rest[3], shell_spring);
            if wait <= 0.0 {
                self.move_shell(rest, shell_spring);
            } else {
                let due = now + f64::from(wait);
                self.pending
                    .push((due, self.seq, Step::Shell(rest, shell_spring)));
            }
        }
        self.shell_on = true;

        for item in &items {
            if let Some(shown) = self.items.iter_mut().find(|s| s.is(item)) {
                shown.motion = item_spring;
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
                    item_spring
                } else {
                    tuning::get().highlight
                };
                h.id = a.id;
                h.rect = a.rect;
            }
            (h, None) => *h = None,
            (None, Some(_)) => {}
        }

        let presence = self.presence();
        let mut rank = 0;
        for item in &items {
            if self.items.iter().any(|s| s.is(item)) {
                continue;
            }
            let room = if entry {
                ENTRY_CONTENT_DELAY_S + self.room_delay(item.rect, t, true)
            } else {
                self.room_delay(item.rect, t, false)
            };
            let delay = room + tuning::get().stagger_s * rank as f32;
            rank += 1;
            let shown = self.shown(item, presence);
            if item.active && self.highlight.is_none() {
                let h = self
                    .highlight_for(std::slice::from_ref(item), presence)
                    .expect("active");
                self.pending
                    .push((now + f64::from(delay), self.seq, Step::Mount(h)));
            }
            if delay <= 1e-4 {
                self.items.push(shown);
            } else {
                self.pending
                    .push((now + f64::from(delay), self.seq, Step::Mount(shown)));
            }
        }
        self.sort_items();
    }

    fn move_shell(&mut self, rest: [f32; 4], spring: Spring) {
        self.shell = rest;
        self.shell_motion = spring;
        self.shell_fade = spring;
    }

    /// Animation seconds until the walls clear `rect` by most of its
    /// resting inset (and, on entry, until the bottom nearly reaches it).
    fn room_delay(&self, rect: [f32; 4], t: f64, entry: bool) -> f32 {
        let [l, r, _] = self.walls.map(|w| w.target);
        let need_l = rect[0] - (rect[0] - l).max(0.0) * 0.6;
        let need_r = rect[0] + rect[2] + (r - rect[0] - rect[2]).max(0.0) * 0.6;
        let need_b = if entry {
            rect[1] + rect[3] - 2.0
        } else {
            f32::NEG_INFINITY
        };
        let mut tau = 0.0_f32;
        while tau < 1.5 {
            let at = t + f64::from(tau);
            let ok = self.walls[0].at(at).0 <= need_l + 0.01
                && self.walls[1].at(at).0 >= need_r - 0.01
                && self.walls[2].at(at).0 >= need_b - 0.01;
            if ok {
                return tau;
            }
            tau += 1.0 / 240.0;
        }
        tau
    }

    fn sort_items(&mut self) {
        self.items.sort_by_key(|s| s.id.order());
    }

    /// Runs the steps that are due. True when something changed.
    pub(crate) fn tick(&mut self, now: f64) -> bool {
        let mut changed = false;
        while let Some(i) = self
            .pending
            .iter()
            .position(|(due, seq, _)| *due <= now + 1e-9 && *seq == self.seq)
        {
            let (_, _, step) = self.pending.remove(i);
            changed = true;
            match step {
                Step::Entry => self.apply_geometry(now, 0.0),
                Step::Shell(rest, spring) => self.move_shell(rest, spring),
                Step::Mount(shown) if shown.key.starts_with("highlight") => {
                    self.highlight = Some(shown);
                }
                Step::Mount(shown) => {
                    self.items.push(shown);
                    self.sort_items();
                }
            }
        }
        self.pending.retain(|(_, seq, _)| *seq == self.seq);
        changed
    }

    /// The next due step, in clock seconds.
    pub(crate) fn next_due(&self) -> Option<f64> {
        self.pending
            .iter()
            .map(|(due, ..)| *due)
            .min_by(f64::total_cmp)
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
            ask_keys: ask_keys(),
            ..Done::plain(Lead::Check, "Sent to SEER")
        })
    }

    fn open() -> Pres {
        Pres::Done(Done {
            open: Some((
                String::new(),
                List {
                    suggested: vec![Dest::new("SEER", "guess")],
                    recent: vec![Dest::new("HOME", "yesterday")],
                    selected: 0,
                    footer: Footer::KeepCopied,
                    always: None,
                },
            )),
            ..Done::plain(Lead::Check, "Copied")
        })
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
                assert!(gap == CHIP_GAP || gap == GROUP_GAP, "{gap}");
            }
        }
        let (shell, _) = layout(&open(), "0:00");
        assert!(
            (shell[2] - LIST_W).abs() < 0.5,
            "the open capsule is the list's width"
        );
    }

    /// The Undo chip is laid out as wide as it's drawn: its label in
    /// MEDIUM, after its keycaps.
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

    #[test]
    fn entry_lands_the_pose_instantly_then_springs_to_rest_with_content_after() {
        let mut s = Surface::new();
        s.set(recording(true), 0.0);
        assert!(s.instant && !s.shell_on && s.items.is_empty());
        assert!(s.shell[3] < ISLAND_H, "origin pose is shorter");
        s.instant = false;
        assert!(s.tick(0.001));
        assert!(s.shell_on && s.shell[3] == ISLAND_H);
        assert!(s.items.is_empty(), "content waits for the shell");
        run(&mut s, 0.001, 0.6);
        assert_eq!(s.items.len(), 5, "lead, three chips and close");
        assert!(
            s.highlight
                .as_ref()
                .is_some_and(|h| h.id == Id::Mode(Mode::Screen))
        );
    }

    #[test]
    fn narrowing_walls_follow_the_content_and_opening_waits_for_room() {
        let mut s = Surface::new();
        s.set(recording(true), 0.0);
        let now = run(&mut s, 0.0, 1.0);
        let wide = s.shell;
        s.set(recording(false), now);
        assert_eq!(s.shell, wide, "walls start after the chips begin to fade");
        assert!(!s.items.iter().any(|i| i.id == Id::Mode(Mode::Area)));
        let now = run(&mut s, now, now + 0.03);
        assert_eq!(s.shell, wide);
        assert!(
            s.items.iter().any(|i| i.id == Id::Finish),
            "Finish crossfades over the chips leaving under it"
        );
        let now = run(&mut s, now, now + 0.05);
        assert!(s.shell[2] < wide[2]);

        let now = run(&mut s, now, now + 1.0);
        s.set(Pres::Processing, now);
        let now = run(&mut s, now, now + 1.0);
        s.set(ruled(true), now);
        assert!(
            !s.items.iter().any(|i| i.id == Id::Close),
            "close waits for the wall"
        );
        run(&mut s, now, now + 1.0);
        assert!(s.items.iter().any(|i| i.id == Id::Close));
    }

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

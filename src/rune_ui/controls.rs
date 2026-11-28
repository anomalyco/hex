//! Settings controls on `rune-kit`, themed with OpenCode's design language
//! (`rune-theme-opencode`): the card, row, switch, segmented control,
//! select-style menus, buttons, keycaps, and note. Colours,
//! type, radii, elevation, and motion come from the kit's tokens.

mod keycaps;

pub(super) use keycaps::*;

use rune::{
    Color, Cursor, El, LoopSpec, SpringPair, TextStyle, Transition, Tx, col, el, row, text,
};
use rune_kit::{
    Button, ButtonVariant, ControlSize, Kit, Menu, MenuItem, Placement, SegmentedControl, Select,
    SelectAppearance, Switch,
};

use super::Ui;

/// Row descriptions and secondary copy.
pub(super) const DESCRIPTION: TextStyle = TextStyle::new(12.0, 440.0).lh(16.0);
/// User-content detail titles (such as a meeting's name) and headline stat numbers.
pub(super) const DISPLAY: TextStyle = TextStyle::new(18.0, 600.0).lh(24.0);
/// A document's title where it leads its own column at full width: a
/// meeting's name over its summary and transcript. Above [`DISPLAY`], and
/// tracked in a touch as large type wants.
pub(super) const HEADLINE: TextStyle = TextStyle::new(22.0, 600.0).lh(28.0).tracking(-0.3);
const SELECT_WIDTH: f32 = 220.0;
/// A select whose rows carry a description: wide enough for a full choice
/// name beside the hint under it, which takes whatever is left.
const DESCRIBED_SELECT_WIDTH: f32 = 260.0;
/// The microphone field, wide enough for "System default" and a faint
/// "Studio Display Microphone".
pub(super) const DEVICE_SELECT_WIDTH: f32 = 320.0;
/// Narrower than this, a row moves its control below the copy instead of
/// squeezing the description to a word per line (tiling desktops ignore
/// the window's 860 px minimum width).
const COPY_MIN_WIDTH: f32 = 100.0;

/// A settings page's and split pane's measurements: raised cards of rows under
/// muted labels. The title, section labels, and row text share the gutter;
/// cards hang outward by `row_px` on both sides, so row controls end where the
/// title bar's actions do.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(super) struct Metrics {
    /// The page's text edge: the title's, section labels', and rows'.
    pub(super) gutter: f32,
    pub(super) section_gap: f32,
    /// From a section label to its card.
    pub(super) label_gap: f32,
    /// A row's text inset inside its card, and how far cards hang past the
    /// gutter.
    pub(super) row_px: f32,
    pub(super) row_h: f32,
    pub(super) row_h_described: f32,
    /// Pane header height.
    pub(super) header_h: f32,
    /// Compact row height (sidebar items, menu rows).
    pub(super) row_h_compact: f32,
    /// Two-line list item height in split panes.
    pub(super) item_h: f32,
    /// Leading row icon size.
    pub(super) icon: f32,
    /// Leading icon column width (icon + gap) in settings rows and list rows.
    pub(super) icon_lead: f32,
    /// Fixed list column width in list + detail panes.
    pub(super) list_w: f32,
    /// Gap between list column and detail card.
    pub(super) split_gap: f32,
    /// Detail card inner padding on all sides.
    pub(super) detail_pad: f32,
    /// Fact label column width in detail views.
    pub(super) fact_label_w: f32,
    /// Centred empty-state and option copy width.
    pub(super) copy_w: f32,
    /// The Meetings setup column: its header, card, and action.
    pub(super) setup_w: f32,
}

pub(super) const METRICS: Metrics = Metrics {
    gutter: 32.0,
    section_gap: 24.0,
    label_gap: 8.0,
    row_px: 16.0,
    row_h: 48.0,
    row_h_described: 60.0,
    header_h: 52.0,
    row_h_compact: 28.0,
    item_h: 52.0,
    icon: 16.0,
    icon_lead: 28.0,
    list_w: 320.0,
    split_gap: 12.0,
    detail_pad: 24.0,
    fact_label_w: 120.0,
    copy_w: 280.0,
    setup_w: 480.0,
};

/// Standard modal sheet widths. A modal is a small page and reuses the page's
/// rails: card edge at `gutter - row_px` (16), title and footer on `gutter` (32).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModalWidth {
    /// Sounds, Dictation stats.
    Narrow = 460,
    /// Calls, Auto-send rules.
    Medium = 560,
    /// Shortcut mode, Vocabulary.
    Wide = 640,
}

impl ModalWidth {
    pub(super) const fn px(self) -> f32 {
        self as u16 as f32
    }
}

/// Shared vertical rhythm and window clearance for modal sheets.
#[derive(Clone, Copy)]
pub(super) struct Modal {
    pub(super) top: f32,
    pub(super) bottom: f32,
    pub(super) gap: f32,
    pub(super) margin: f32,
}

pub(super) const MODAL: Modal = Modal {
    top: 20.0,
    bottom: 16.0,
    gap: 16.0,
    margin: 24.0,
};

/// Unified capsule geometry shared across overlay surfaces (Meetings, Annotate,
/// and Screenshots). Every nested shape obeys `R_inner = CAPSULE.radius - inset`
/// and lands on a kit radius token (`xl`, `lg`, `md`).
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct Capsule {
    pub(crate) radius: f32,
    pub(crate) window_inset: f32,
    pub(crate) chip_inset: f32,
    pub(crate) row_inset: f32,
    pub(crate) button_inset: f32,
    pub(crate) row_px: f32,
    pub(crate) icon: f32,
    pub(crate) icon_gap: f32,
    pub(crate) header_h: f32,
    pub(crate) row_h: f32,
    pub(crate) row_gap: f32,
    pub(crate) island_h: f32,
    pub(crate) banner_h: f32,
    pub(crate) close: f32,
    pub(crate) panel_w: f32,
}

#[allow(dead_code)]
pub(crate) const CAPSULE: Capsule = Capsule {
    radius: 14.0,
    window_inset: 4.0,
    chip_inset: 4.0,
    row_inset: 6.0,
    button_inset: 8.0,
    row_px: 10.0,
    icon: 16.0,
    icon_gap: 8.0,
    header_h: 36.0,
    row_h: 44.0,
    row_gap: 2.0,
    island_h: 40.0,
    banner_h: 44.0,
    close: 28.0,
    panel_w: 252.0,
};

#[allow(dead_code)]
impl Capsule {
    pub(crate) const fn inner(self, inset: f32) -> f32 {
        self.radius - inset
    }
    pub(crate) const fn text_rail(self) -> f32 {
        self.row_inset + self.row_px
    }
    pub(crate) const fn icon_spine(self) -> f32 {
        self.text_rail() + self.icon / 2.0
    }
    pub(crate) const fn label_rail(self) -> f32 {
        self.text_rail() + self.icon + self.icon_gap
    }
}

// The concentric radius table, checked at compile time.
const _: () = assert!(CAPSULE.inner(CAPSULE.chip_inset) == 10.0); // radius.xl
const _: () = assert!(CAPSULE.inner(CAPSULE.row_inset) == 8.0); // radius.lg
const _: () = assert!(CAPSULE.inner(CAPSULE.button_inset) == 6.0); // radius.md
const _: () = assert!(CAPSULE.text_rail() == METRICS.row_px);

/// A settings page's column of sections: cards hang `row_px` past the
/// gutter, so their rows' text lands on it.
pub(super) fn page(key: &str) -> El {
    col()
        .key(key)
        .px(METRICS.gutter - METRICS.row_px)
        .gap(METRICS.section_gap)
}

/// A page of free-standing content with no cards, on the gutter itself.
pub(super) fn bare_page(key: &str) -> El {
    col().key(key).px(METRICS.gutter).gap(METRICS.section_gap)
}

/// Everything a control needs to paint itself.
#[derive(Clone, Copy)]
pub(crate) struct Paint {
    pub(super) k: &'static Kit,
    pub(super) hair: f32,
}

impl Paint {
    pub(super) fn new(dark: bool, scale: f32) -> Self {
        Self {
            k: rune_theme_opencode::kit(dark),
            hair: 1.0 / scale,
        }
    }
    /// A section label: above its card, on the page's text edge. Screen
    /// readers move between sections by it.
    pub(super) fn section_label(self, title: &str) -> El {
        text(title)
            .key("header")
            .role("heading")
            .level(2)
            .font(self.label())
            .color(self.k.c.text_muted)
    }
    /// Pane title, modal title, and kind detail titles.
    pub(super) fn title(self) -> TextStyle {
        self.k.ty.body_medium
    }
    /// User-content detail titles (such as a meeting's name) and headline stat numbers.
    #[allow(dead_code)]
    pub(super) fn display(self) -> TextStyle {
        DISPLAY
    }
    /// Section label above a card and a row's value (value button, borderless select).
    pub(super) fn label(self) -> TextStyle {
        self.k.ty.ui_medium.lh(SECTION_LABEL_LINE)
    }
    /// Row title and sidebar item.
    #[allow(dead_code)]
    pub(super) fn row_title(self) -> TextStyle {
        self.k.ty.ui
    }
    /// List item title, capsule title, capsule primary row, and button label.
    #[allow(dead_code)]
    pub(super) fn strong(self) -> TextStyle {
        self.k.ty.ui_medium
    }
    /// Row description, note, detail meta, capsule detail and summary, and empty-state hints.
    #[allow(dead_code)]
    pub(super) fn description(self) -> TextStyle {
        DESCRIPTION
    }
    /// List item meta, sidebar footer, and compact hints.
    #[allow(dead_code)]
    pub(super) fn meta(self) -> TextStyle {
        self.k.ty.small
    }
    /// Timer, elapsed time, and countdown (`ui_medium` with tabular numerals).
    #[allow(dead_code)]
    pub(super) fn clock(self) -> TextStyle {
        self.tabular(self.k.ty.ui_medium)
    }
    /// Applies tabular numerals (`'tnum' 1`) so digits changing in place or
    /// stacked in a column stay aligned.
    #[allow(dead_code)]
    pub(super) fn tabular(self, style: TextStyle) -> TextStyle {
        style.features("'tnum' 1")
    }
    pub(super) fn text(self) -> Color {
        self.k.c.text_base
    }
    pub(super) fn muted(self) -> Color {
        self.k.c.text_muted
    }
    pub(super) fn faint(self) -> Color {
        self.k.c.text_faint
    }
    pub(super) fn danger(self) -> Color {
        self.k.c.danger.fg
    }
    pub(super) fn success(self) -> Color {
        self.k.c.success.fg
    }
    pub(super) fn warning(self) -> Color {
        self.k.c.warning.fg
    }
    pub(super) fn accent(self) -> Color {
        self.k.c.text_accent
    }
}

/// A row of a [`card`]: always there, or a slot of rows that folds away.
pub(super) enum CardRow {
    Shown(Box<El>),
    /// The slot stays in the card and clips its rows, so they close under
    /// the rows below as those slide up, rather than fading over them, and
    /// the separator above folds with them.
    Folding {
        key: &'static str,
        rows: Vec<CardRow>,
    },
}

impl From<El> for CardRow {
    fn from(row: El) -> Self {
        CardRow::Shown(Box::new(row))
    }
}

/// A [`CardRow::Folding`] slot keyed `key`, open while it holds rows.
pub(super) fn folding<R: Into<CardRow>>(
    key: &'static str,
    rows: impl IntoIterator<Item = R>,
) -> CardRow {
    CardRow::Folding {
        key,
        rows: rows.into_iter().map(Into::into).collect(),
    }
}

/// A titled card of rows with hairline separators between them.
pub(super) fn section(p: Paint, key: &str, title: &str, rows: Vec<impl Into<CardRow>>) -> El {
    col()
        .key(key)
        .gap(METRICS.label_gap)
        .child(p.section_label(title).mx(METRICS.row_px))
        .child(card(p, "card", rows))
}

/// A [`section`] with a control on its label's line (Advanced, Add). The
/// line keeps the label's height, so a taller control overhangs it rather
/// than pushing the card down, and the control ends where the rows'
/// controls do.
pub(super) fn section_with(
    p: Paint,
    key: &str,
    title: &str,
    accessory: El,
    rows: Vec<impl Into<CardRow>>,
) -> El {
    col()
        .key(key)
        .gap(METRICS.label_gap)
        .child(
            row()
                .key("header-row")
                .h(SECTION_LABEL_LINE)
                .mx(METRICS.row_px)
                .gap(8.0)
                .center_y()
                .child(p.section_label(title).grow(1.0))
                .child(accessory),
        )
        .child(card(p, "card", rows))
}

const SECTION_LABEL_LINE: f32 = 16.0;

/// A raised card of rows with hairline separators between them.
pub(super) fn card(p: Paint, key: &str, rows: Vec<impl Into<CardRow>>) -> El {
    raised(p, key).children(stack(p, rows.into_iter().map(Into::into).collect()))
}

/// The card's raised surface, empty.
pub(super) fn raised(p: Paint, key: &str) -> El {
    let mut surface = col().key(key).fill(p.k.c.bg_base).radius(p.k.radius.lg);
    for shadow in &p.k.elevation.raised {
        surface = surface.box_shadow(*shadow);
    }
    surface
}

/// `rows` as a card's children, a separator between each pair.
fn stack(p: Paint, rows: Vec<CardRow>) -> Vec<El> {
    let mut children = Vec::new();
    for (index, row) in rows.into_iter().enumerate() {
        match row {
            CardRow::Shown(row) => {
                children.extend((index > 0).then(|| separator(p, format!("sep{}", index - 1))));
                children.push(*row);
            }
            CardRow::Folding { key, rows } => {
                let lead =
                    (index > 0 && !rows.is_empty()).then(|| separator(p, "sep-above".into()));
                let rows = lead.into_iter().chain(stack(p, rows));
                children.push(
                    col()
                        .key(key)
                        .clip_soft(0.0)
                        .children(rows.map(|row| row.transition(folds()))),
                );
            }
        }
    }
    children
}

/// How a folding slot's rows come and go. The slot's clip does most of the
/// work, so they fade only partway, and leave on a slower spring than the
/// slot closes on, so no sliver of them pops away.
pub(super) fn folds() -> Transition {
    let mut transition = Transition::opacity().springs(SpringPair::new(
        rune::Spring::new(0.3, 0.0),
        rune::Spring::new(0.45, 0.0),
    ));
    transition.insertion.opacity = 0.4;
    transition.removal.opacity = 0.4;
    transition
}

fn separator(p: Paint, key: String) -> El {
    el().key(key)
        .h(p.hair)
        .ml(METRICS.row_px)
        .mr(METRICS.row_px)
        .fill(p.k.c.border_muted)
        .a11y_hidden()
}

/// A folder as a label for the row that chooses it: a path under the home
/// folder reads from `~` ("~/Pictures/Hex Shots"), else it reads whole.
pub(super) fn folder_label(folder: &std::path::Path) -> String {
    crate::app_paths::home_dir()
        .and_then(|home| {
            folder
                .strip_prefix(home)
                .ok()
                .map(|relative| format!("~/{}", relative.display()))
        })
        .unwrap_or_else(|| folder.display().to_string())
}

/// A setting: title and optional description on the left, its control on
/// the right.
pub(super) fn setting_row(
    p: Paint,
    key: &str,
    title: &str,
    description: Option<&str>,
    control: El,
) -> El {
    described_row(
        p,
        key,
        title,
        description.map(|text| (text.to_string(), p.muted())),
        control,
    )
}

/// A [`setting_row`] whose description has its own colour (errors, progress).
pub(super) fn described_row(
    p: Paint,
    key: &str,
    title: &str,
    description: Option<(String, Color)>,
    control: El,
) -> El {
    let said = description.as_ref().map(|(copy, _)| copy.clone());
    let description = description.map(|(copy, color)| described_copy(copy, color));
    described(
        copy_row(p, key, None, title, description, control, false),
        said,
    )
}

/// A row that belongs to the [`icon_row`] above it: its title starts where
/// that row's title does.
pub(super) fn nested(row: El) -> El {
    row.pl(METRICS.row_px + METRICS.icon_lead)
}

/// One numbered step of a how-to card: the number, the title with the
/// step's keys beside it when it has any, and the description under them.
#[cfg(hex_capture)]
pub(super) fn step_row(
    p: Paint,
    key: &str,
    number: usize,
    title: &str,
    keys: Option<El>,
    description: &str,
) -> El {
    // As tall as a keycaps tray, so a step with keys lines up with one
    // without.
    const LINE: f32 = 28.0;
    // A fixed width, so every step's copy starts at the same x.
    let number = row().key("number").w(16.0).h(LINE).center_y().child(
        text(number.to_string())
            .font(p.k.ty.ui)
            .color(p.faint())
            .a11y_hidden(),
    );
    let heading = row()
        .key("title")
        .min_h(LINE)
        .gap(8.0)
        .center_y()
        .child(
            text(title)
                .key("label")
                .font(p.k.ty.ui)
                .color(p.text())
                .a11y_hidden(),
        )
        .children(keys);
    let step = row()
        .key(key)
        .px(METRICS.row_px)
        .py(8.0)
        .gap(8.0)
        .items_start()
        .label(title)
        .child(number)
        .child(
            col()
                .key("copy")
                .grow(1.0)
                .shrink()
                .child(heading)
                .child(described_copy(description, p.muted())),
        );
    described(step, Some(description.to_string()))
}

/// A row's description line. The row reads it to assistive technology as
/// its description ([`described`]), so the line itself stays out of the
/// tree.
fn described_copy(copy: impl Into<String>, color: Color) -> El {
    text(copy.into())
        .key("description")
        .font(DESCRIPTION)
        .color(color)
        .wrap()
        .a11y_hidden()
}

/// `row` with `description` as its accessible description.
fn described(row: El, description: Option<String>) -> El {
    match description {
        Some(description) => row.description(description),
        None => row,
    }
}

/// A [`setting_row`] led by an icon in the icon colour.
pub(super) fn icon_row(
    p: Paint,
    key: &str,
    glyph: &'static rune::Icon,
    title: &str,
    description: Option<&str>,
    control: El,
) -> El {
    icon_row_tagged(p, key, glyph, title, None, description, control)
}

/// An [`icon_row`] with a small tag after its title.
pub(super) fn icon_row_tagged(
    p: Paint,
    key: &str,
    glyph: &'static rune::Icon,
    title: &str,
    tag: Option<El>,
    description: Option<&str>,
    control: El,
) -> El {
    let said = description.map(str::to_owned);
    let description = description.map(|copy| described_copy(copy, p.muted()));
    described(
        copy_row_tagged(p, key, Some(glyph), title, tag, description, control, false),
        said,
    )
}

/// An [`icon_row`] whose description has its own colour (errors, progress).
pub(super) fn icon_described_row(
    p: Paint,
    key: &str,
    glyph: &'static rune::Icon,
    title: &str,
    description: Option<(String, Color)>,
    control: El,
) -> El {
    let said = description.as_ref().map(|(copy, _)| copy.clone());
    let description = description.map(|(copy, color)| described_copy(copy, color));
    described(
        copy_row(p, key, Some(glyph), title, description, control, false),
        said,
    )
}

/// A [`select`]'s field showing its one choice, disabled.
pub(super) fn fixed_select(p: Paint, key: &str, name: &str, label: &str) -> El {
    Select::new::<&str>(format!("{key}-field"), &[])
        .appearance(SelectAppearance::Field)
        .width(SELECT_WIDTH)
        .current(Some(label))
        .disabled(true)
        .trigger(p.k)
        .key(key)
        .label(name)
        .a11y_value(label)
        .disabled(true)
}

/// A switch an organization holds off (`crate::policy`): shown off and
/// disabled with the managed note; pressing it does nothing.
pub(super) fn managed_row(
    p: Paint,
    key: &str,
    glyph: Option<&'static rune::Icon>,
    title: &str,
) -> El {
    let switch = managed_switch(p).a11y_hidden();
    let row = match glyph {
        Some(glyph) => icon_row(p, key, glyph, title, Some(super::managed_note()), switch),
        None => setting_row(p, key, title, Some(super::managed_note()), switch),
    };
    row.role("switch").checked(false).disabled(true)
}

/// An off switch that can't be turned on.
pub(super) fn managed_switch(p: Paint) -> El {
    rune_kit::Switch::new(false)
        .disabled(true)
        .build(p.k)
        .key("switch")
        .focusable(false)
}

/// A whole-row switch led by an icon.
pub(super) fn icon_toggle_row<'a>(
    p: Paint,
    key: &str,
    glyph: &'static rune::Icon,
    title: &str,
    description: impl Into<Option<&'a str>>,
    on: bool,
    flip: impl Fn(&mut Ui) + 'static,
) -> El {
    pressable_row(
        icon_row(p, key, glyph, title, description.into(), row_switch(p, on)),
        on,
    )
    .on_press(flip)
}

pub(super) fn copy_row(
    p: Paint,
    key: &str,
    glyph: Option<&'static rune::Icon>,
    title: &str,
    description: Option<El>,
    control: El,
    compact: bool,
) -> El {
    copy_row_tagged(p, key, glyph, title, None, description, control, compact)
}

/// A [`copy_row`] with a small tag after its title.
#[allow(clippy::too_many_arguments)]
pub(super) fn copy_row_tagged(
    p: Paint,
    key: &str,
    glyph: Option<&'static rune::Icon>,
    title: &str,
    tag: Option<El>,
    description: Option<El>,
    control: El,
    compact: bool,
) -> El {
    let m = METRICS;
    let height = if description.is_some() && !compact {
        m.row_h_described
    } else {
        m.row_h
    };
    // The row is named by its title, so the title's text stays out of the
    // accessibility tree.
    let title_text = || {
        text(title)
            .key("label")
            .font(p.k.ty.ui)
            .color(p.text())
            .a11y_hidden()
    };
    let copy = col()
        .key("copy")
        .w(COPY_MIN_WIDTH)
        .min_w(COPY_MIN_WIDTH)
        .grow(1.0)
        .gap(if compact { 0.0 } else { 2.0 })
        .child(match tag {
            None => title_text(),
            Some(tag) => row()
                .key("title")
                .gap(6.0)
                .center_y()
                .child(title_text())
                .child(tag),
        })
        .children(description);
    row()
        .key(key)
        .min_h(height)
        .px(m.row_px)
        .py(if compact { 4.0 } else { 8.0 })
        .gaps(8.0, 24.0)
        .center_y()
        .justify_between()
        .flex_wrap()
        .radius(p.k.radius.md)
        .label(title)
        .child(match glyph {
            None => copy,
            Some(glyph) => row()
                .key("lead")
                .w(COPY_MIN_WIDTH + METRICS.icon_lead)
                .min_w(COPY_MIN_WIDTH + METRICS.icon_lead)
                .grow(1.0)
                .gap(METRICS.icon_lead - METRICS.icon)
                .center_y()
                .child(
                    rune::icon(glyph, METRICS.icon)
                        .key("icon")
                        .color(p.k.c.icon_base)
                        .a11y_hidden(),
                )
                .child(copy),
        })
        .child(control)
}

/// The switch of a row that toggles: the row takes focus and presses, and
/// the switch shows its state and the focus ring. The row is the switch
/// assistive technology reads ([`pressable_row`]), so this one stays out of
/// the tree.
pub(super) fn row_switch(p: Paint, on: bool) -> El {
    Switch::new(on)
        .build(p.k)
        .key("switch")
        .focusable(false)
        .a11y_hidden()
}

/// A whole-row switch: the row presses, the switch shows the state.
pub(super) fn toggle_row<'a>(
    p: Paint,
    key: &str,
    title: &str,
    description: impl Into<Option<&'a str>>,
    on: bool,
    flip: impl Fn(&mut Ui) + 'static,
) -> El {
    pressable_row(
        setting_row(p, key, title, description.into(), row_switch(p, on)),
        on,
    )
    .on_press(flip)
}

/// A settings row that toggles when pressed anywhere: a switch, named by
/// its title and on while `on`. Only its switch reacts to the pointer; the
/// row itself never paints a hover or press fill.
pub(super) fn pressable_row(row: El, on: bool) -> El {
    row.role("switch").checked(on).focusable(true)
}

/// A few exclusive choices: equal segments sized for the longest label.
pub(super) fn segmented(
    p: Paint,
    key: &str,
    labels: &[&str],
    selected: usize,
    pick: impl Fn(&mut Ui, usize) + Clone + 'static,
) -> El {
    segmented_tx(p, key, labels, selected, move |ui, index, _| {
        pick(ui, index)
    })
}

/// A [`segmented`] whose pick also gets the transaction, to swap views
/// without animating.
pub(super) fn segmented_tx(
    p: Paint,
    key: &str,
    labels: &[&str],
    selected: usize,
    pick: impl Fn(&mut Ui, usize, &mut Tx) + Clone + 'static,
) -> El {
    SegmentedControl::new(labels, Some(selected))
        .fit()
        .on_pick(pick)
        .build(p.k)
        .key(key)
        .role("segmented")
}

/// The inset either side of a segment's label, as `SegmentedControl` pads it.
const SEGMENT_PX: f32 = 12.0;

/// How wide a segment's label draws: its text at the segment's font, the
/// kit's UI text (a line height takes no width).
pub(super) fn segment_label_width(p: Paint, label: &str) -> f32 {
    rune::text::measure(label, &[], p.k.ty.ui, None).0
}

/// A [`segmented_tx`] that keeps the room the widest label a segment may ever
/// show needs: `reserved` holds one such label per segment. A control whose
/// labels count something (`Turned on · 3`) then stays the same size as the
/// count changes, so it never moves and a tab never slides out from under the
/// pointer that is about to press it.
pub(super) fn segmented_reserved_tx(
    p: Paint,
    key: &str,
    labels: &[&str],
    reserved: &[&str],
    selected: usize,
    pick: impl Fn(&mut Ui, usize, &mut Tx) + Clone + 'static,
) -> El {
    debug_assert_eq!(labels.len(), reserved.len(), "one reservation per segment");
    let widest = labels
        .iter()
        .zip(reserved)
        .map(|(label, reserve)| segment_label_width(p, label).max(segment_label_width(p, reserve)))
        .fold(0.0_f32, f32::max);
    let segments = labels.len().max(1) as f32;
    SegmentedControl::new(labels, Some(selected))
        // Rounded up, so the kit's own layout never has to truncate a label.
        .width(Some((widest + 2.0 * SEGMENT_PX).ceil() * segments))
        .on_pick(pick)
        .build(p.k)
        .key(key)
        .role("segmented")
}

/// One choice of a [`select`].
pub(super) struct Choice {
    pub(super) label: String,
    /// Faint text after the label on the field and under it in the menu,
    /// such as what a choice resolves to.
    pub(super) detail: Option<String>,
    pub(super) selected: bool,
    pub(super) choose: Box<dyn Fn(&mut Ui)>,
}

/// A select field whose choices open in an anchored menu above the page.
/// Assistive technology reads it as `name` (what it chooses), with `label`,
/// the current choice, as its value.
#[allow(clippy::too_many_arguments)]
pub(super) fn select(
    p: Paint,
    key: &str,
    name: &str,
    label: &str,
    open: bool,
    choices: Vec<Choice>,
    toggle: impl Fn(&mut Ui) + 'static,
    layer: &mut Vec<El>,
) -> El {
    select_sized(
        p,
        key,
        name,
        label,
        None,
        open,
        choices,
        toggle,
        layer,
        SELECT_WIDTH,
    )
}

/// The press that opens a select's menu, telling Rune to reveal the chosen
/// row: a menu shows only six or seven rows of a long list, so a choice
/// further down would open below the edge with its check out of sight.
/// `scope` is the key around that menu's rows, so rows keyed alike
/// elsewhere never answer for it.
pub(super) fn revealing_press(
    open: bool,
    scope: String,
    chosen: Option<String>,
    toggle: impl Fn(&mut Ui) + 'static,
) -> impl Fn(&mut Ui, &mut Tx) + 'static {
    move |ui: &mut Ui, tx: &mut Tx| {
        toggle(ui);
        if !open && let Some(chosen) = chosen.clone() {
            tx.scroll_into_view_in(scope.clone(), chosen);
        }
    }
}

/// A [`select`] `width` points wide, for labels such as device names that
/// would truncate at the usual width. A `detail` follows the field's label
/// in faint text.
#[allow(clippy::too_many_arguments)]
pub(super) fn select_sized(
    p: Paint,
    key: &str,
    name: &str,
    label: &str,
    detail: Option<&str>,
    open: bool,
    choices: Vec<Choice>,
    toggle: impl Fn(&mut Ui) + 'static,
    layer: &mut Vec<El>,
    width: f32,
) -> El {
    let chosen = choices
        .iter()
        .position(|choice| choice.selected)
        .map(|index| format!("{key}-item{index}"));
    let press = revealing_press(open, format!("{key}-menu"), chosen, toggle);
    let trigger = match detail {
        None => Select::new::<&str>(format!("{key}-field"), &[])
            .appearance(SelectAppearance::Field)
            .width(width)
            .current(Some(label))
            .open(open)
            .trigger(p.k)
            .on_press_tx(press)
            .label(name)
            .a11y_value(label),
        Some(detail) => detailed_field(p, key, label, detail, open, width)
            .label(name)
            .on_press_tx(press),
    };
    // A described row stacks its hint, so the menu needs room for both.
    let menu_width = if choices.iter().any(|choice| choice.detail.is_some()) {
        width.max(DESCRIBED_SELECT_WIDTH)
    } else {
        width
    };
    let mut menu = Menu::new(key)
        .trigger(trigger)
        .placement(Placement::BottomEnd)
        .gutter(4.0)
        .min_width(menu_width)
        .open(open)
        .on_close(|ui: &mut Ui| ui.open = None);
    for choice in choices {
        let mut item = MenuItem::new(choice.label).checked(choice.selected);
        if let Some(detail) = choice.detail {
            // A sentence, not a shortcut: the kit puts it on a second line,
            // so the choice's name keeps the room it needs and only the
            // detail truncates. As a shortcut it would squeeze the name to
            // an ellipsis.
            item = item.description(detail);
        }
        menu = menu.item(item.on_select(move |ui: &mut Ui| (choice.choose)(ui)));
    }
    menu.build(p.k, layer)
}

/// The kit's Field select trigger with a faint `detail` after its label,
/// which the kit's `Select` cannot show.
fn detailed_field(p: Paint, key: &str, label: &str, detail: &str, open: bool, width: f32) -> El {
    let c = &p.k.c;
    let chevron = if open {
        p.k.glyph.chevron_up
    } else {
        p.k.glyph.chevron_down
    };
    let mut field = row()
        .key(format!("{key}-field-trigger"))
        .role("combobox")
        .a11y_value(format!("{label}, {detail}"))
        .expanded(open)
        .center_y()
        .focusable(true)
        .focus_ring(1.0, 0.0, c.border_focus)
        .w(width)
        .h(28.0)
        .pr(6.0)
        .gap(8.0)
        .radius(p.k.radius.md)
        .fill(if open {
            source_over(c.hover, c.bg_base)
        } else {
            c.bg_base
        })
        .gradient(p.k.light(0.02), p.k.light(0.0))
        .child(
            row()
                .key("value")
                .grow(1.0)
                .shrink()
                .pl(8.0)
                .gap(6.0)
                .center_y()
                .child(
                    text(label)
                        .key("label")
                        .font(p.k.ty.ui_compact)
                        .color(c.text_base)
                        .truncate(),
                )
                .child(
                    text(detail)
                        .key("detail")
                        .font(p.k.ty.ui_compact)
                        .color(p.faint())
                        .shrink()
                        .truncate(),
                )
                // The field's value says the same.
                .a11y_hidden(),
        )
        .child(
            el().size(20.0, 20.0)
                .center()
                .child(rune::icon(chevron, 16.0).key("chevron").color(c.icon_muted))
                .a11y_hidden(),
        );
    for shadow in p.k.shadow_layers(&p.k.elevation.button_neutral) {
        field = field.box_shadow(shadow);
    }
    if !open {
        field = field.hover(c.hover);
    }
    field
}

/// Source-over of straight-alpha colours, for a forced hover fill.
fn source_over(top: Color, bottom: Color) -> Color {
    let a = top.a + bottom.a * (1.0 - top.a);
    if a <= 0.0 {
        return Color::TRANSPARENT;
    }
    let mix = |t: f32, b: f32| (t * top.a + b * bottom.a * (1.0 - top.a)) / a;
    Color {
        r: mix(top.r, bottom.r),
        g: mix(top.g, bottom.g),
        b: mix(top.b, bottom.b),
        a,
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum ButtonKind {
    /// The one main action on screen: the kit's contrast button.
    Primary,
    Secondary,
    /// Destructive or armed confirmations.
    Danger,
}

/// Builds push buttons at Hex's sizes, which keep tighter padding and
/// corners than the kit's `ControlSize`s of the same height.
pub(super) trait PushSize {
    /// 28 pt, the kit's Large height.
    fn build_push(self, k: &Kit) -> El;
    /// 24 pt, for actions inside list rows.
    fn build_small_push(self, k: &Kit) -> El;
}

impl PushSize for Button {
    fn build_push(self, k: &Kit) -> El {
        self.control_size(ControlSize::Large)
            .build(k)
            .px(11.0)
            .radius(k.radius.md)
    }

    fn build_small_push(self, k: &Kit) -> El {
        self.control_size(ControlSize::Regular)
            .build(k)
            .px(9.0)
            .radius(k.radius.sm)
    }
}

pub(super) fn button(
    p: Paint,
    key: &str,
    label: &str,
    kind: ButtonKind,
    press: impl Fn(&mut Ui, &mut Tx) + 'static,
) -> El {
    button_enabled(p, key, label, kind, true, press)
}

/// A [`button`] that keeps its place but can't be pressed while `enabled` is
/// false, so an action with nothing to do never silently does nothing.
pub(super) fn button_enabled(
    p: Paint,
    key: &str,
    label: &str,
    kind: ButtonKind,
    enabled: bool,
    press: impl Fn(&mut Ui, &mut Tx) + 'static,
) -> El {
    let variant = match kind {
        ButtonKind::Primary => ButtonVariant::Contrast,
        ButtonKind::Secondary => ButtonVariant::Neutral,
        ButtonKind::Danger => ButtonVariant::Danger,
    };
    Button::new(label)
        .variant(variant)
        .disabled(!enabled)
        .on_press_tx(press)
        .build_push(p.k)
        .key(key)
}

/// An icon-only Secondary button the height of [`button`].
#[cfg(target_os = "macos")]
pub(super) fn icon_button(
    p: Paint,
    key: &str,
    icon: &'static rune::Icon,
    label: &str,
    press: impl Fn(&mut Ui) + 'static,
) -> El {
    rune_kit::IconButton::new(icon)
        .variant(rune_kit::IconButtonVariant::Neutral)
        .control_size(ControlSize::Large)
        .label(label)
        .on_press(press)
        .build(p.k)
        .key(key)
}

/// A quiet text action beside a row's control, such as a shortcut's Reset.
/// Assistive technology reads `label`, which says more than the short `text`.
pub(super) fn quiet_button(
    p: Paint,
    key: &str,
    text: &str,
    label: &str,
    press: impl Fn(&mut Ui) + 'static,
) -> El {
    Button::new(text)
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .on_press(press)
        .build(p.k)
        .label(label)
        .key(key)
}

/// The type of a row's value: a borderless select's, a [`value_button`]'s.
pub(super) fn value_font(p: Paint) -> TextStyle {
    p.label()
}

/// A value that opens something, drawn like the kit's borderless select.
/// Assistive technology reads it as `name` with `label` as its value.
pub(super) fn value_button(
    p: Paint,
    key: &str,
    name: &str,
    label: &str,
    glyph: &'static rune::Icon,
    press: impl Fn(&mut Ui) + 'static,
) -> El {
    let c = &p.k.c;
    row()
        .key(key)
        .role("button")
        .label(name)
        .a11y_value(label)
        .focusable(true)
        .focus_ring(2.0, 0.0, c.border_focus)
        .h(24.0)
        .pl(8.0)
        .pr(4.0)
        .mr(-9.0)
        .gap(4.0)
        .center_y()
        .radius(p.k.radius.sm)
        .hover(c.hover)
        .press(c.pressed)
        .on_press(press)
        .child(
            text(label.to_string())
                .key("value")
                .font(value_font(p))
                .color(c.text_base)
                .truncate()
                .a11y_hidden(),
        )
        .child(
            rune::icon(glyph, 16.0)
                .key("chevron")
                .color(c.icon_muted)
                .a11y_hidden(),
        )
}

/// The one modal shell: title on the text rail, body on the card edge, and
/// footer on the control rail.
pub(super) fn modal_panel(
    p: Paint,
    key: &str,
    title: &str,
    width: ModalWidth,
    body: Vec<El>,
    footer: Vec<El>,
) -> El {
    let c = &p.k.c;
    col()
        .key(key)
        .role("dialog")
        .label(title)
        .w(width.px())
        .pt(MODAL.top)
        .px(METRICS.gutter - METRICS.row_px)
        .pb(MODAL.bottom)
        .gap(MODAL.gap)
        .radius(p.k.radius.xl)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak)
        // Presses inside the panel stay inside.
        .on_press(|_: &mut Ui| {})
        .child(
            text(title)
                .key("title")
                .mx(METRICS.row_px)
                .font(p.title())
                .color(p.text()),
        )
        .children(body)
        .child(
            row()
                .key("footer")
                .mx(METRICS.row_px)
                .justify_end()
                .center_y()
                .gap(8.0)
                .children(footer),
        )
}

/// The capsule confirmation block: no icon, title and detail on the 16 pt text
/// rail, equal-width Keep (neutral) and destructive (danger) buttons at inset
/// 8 so their radius-6 corners are concentric with the radius-14 shell.
/// `top` when the header sits above the block so the buttons sit on the free
/// edge.
pub(crate) fn capsule_confirm<S: 'static>(
    p: Paint,
    title: &str,
    detail: &str,
    top: bool,
    disabled: bool,
    keep: (&str, impl Fn(&mut S, &mut Tx) + 'static),
    act: (&str, impl Fn(&mut S, &mut Tx) + 'static),
) -> El {
    fn choice<S: 'static>(
        p: Paint,
        key: &'static str,
        label: &str,
        variant: ButtonVariant,
        disabled: bool,
        press: impl Fn(&mut S, &mut Tx) + 'static,
    ) -> El {
        let button = Button::new(label)
            .variant(variant)
            .build_push(p.k)
            .key(key)
            .grow(1.0)
            .shrink()
            .min_w(0.0);
        if disabled {
            button.disabled(true).opacity(0.5)
        } else {
            button
                .cursor(Cursor::Pointer)
                .no_window_drag()
                .on_press_tx(press)
        }
    }
    let (keep_label, on_keep) = keep;
    let (act_label, on_act) = act;
    let block = col()
        .key("confirm")
        .role("alertdialog")
        .label(title)
        .w(CAPSULE.panel_w)
        .gap(12.0)
        .child(
            col()
                .key("copy")
                .px(CAPSULE.text_rail())
                .gap(2.0)
                .child(
                    text(title)
                        .key("title")
                        .font(p.strong())
                        .color(p.text())
                        .truncate(),
                )
                .child(
                    text(detail)
                        .key("detail")
                        .font(p.description())
                        .color(p.muted())
                        .truncate(),
                ),
        )
        .child(
            row()
                .key("actions")
                .w_full()
                .px(CAPSULE.button_inset)
                .gap(8.0)
                .child(choice(
                    p,
                    "keep",
                    keep_label,
                    ButtonVariant::Neutral,
                    disabled,
                    on_keep,
                ))
                .child(choice(
                    p,
                    "act",
                    act_label,
                    ButtonVariant::Danger,
                    disabled,
                    on_act,
                )),
        );
    // The buttons keep the panel's edge inset; the 4 pt the header leaves
    // goes between the question and the header.
    if top {
        block.pt(4.0).pb(CAPSULE.button_inset)
    } else {
        block.pt(CAPSULE.button_inset).pb(4.0)
    }
}

/// Wraps modal body items in a scrollable column that hugs its content while
/// it fits and shrinks to scroll inside `modal_sheet` when the window is
/// shorter than the body. Spans the modal's full width so card shadows stay
/// unclipped and the scroll bar runs down the modal's right edge.
pub(super) fn modal_scroll_body(p: Paint, key: &str, items: Vec<El>) -> El {
    let card_edge = METRICS.gutter - METRICS.row_px;
    const SHADOW_ROOM: f32 = 4.0;
    let content = col()
        .key("body-content")
        .px(card_edge)
        .py(SHADOW_ROOM)
        .gap(MODAL.gap)
        .children(items);
    super::list_detail::fit_scroll_body(p.k, key, content)
        .mx(-card_edge)
        .my(-SHADOW_ROOM)
}

/// Wraps `panel` in a modal sheet's overlay elevation, scrim, and centered
/// modal layer: keys stay inside, Escape or a press outside runs `close`, and
/// focus returns to the control that opened it.
pub(super) fn modal_sheet(p: Paint, prefix: &str, mut panel: El, close: fn(&mut Ui)) -> [El; 2] {
    for shadow in &p.k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    let layer = |key: String| {
        col()
            .key(key)
            .role("none")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
    };
    [
        layer(format!("{prefix}-scrim"))
            .fill(p.k.c.scrim)
            .on_press(close)
            .transition(modal_scrim_transition(p)),
        layer(format!("{prefix}-layer"))
            .py(MODAL.margin)
            .center()
            .transition(modal_transition(p))
            .child(panel.shrink().min_h(0.0).modal(true, close)),
    ]
}

/// How a modal's surface enters and leaves: scaled from 0.96 about its
/// centre, with a fade. Its text scales with its boxes, so the panel moves
/// as one surface.
fn modal_transition(p: Paint) -> Transition {
    Transition::scale(0.96).springs(overlay_springs(p))
}

/// The scrim behind a modal: the surface's springs, fading only.
fn modal_scrim_transition(p: Paint) -> Transition {
    Transition::opacity().springs(overlay_springs(p))
}

fn overlay_springs(p: Paint) -> SpringPair {
    SpringPair::new(p.k.motion.overlay, p.k.motion.overlay)
}

/// Decoration: the text beside it says what it means.
pub(super) fn status_dot(color: Color) -> El {
    el().key("dot")
        .size(8.0, 8.0)
        .radius(4.0)
        .fill(color)
        .a11y_hidden()
}

/// Breathes between full and this opacity every two seconds; full under
/// Reduce Motion.
const PULSE: LoopSpec = LoopSpec::opacity(2.0, 0.0, 1.0, 0.45);

/// A dot for something live: recording, listening, or waiting on macOS.
pub(super) fn pulsing_dot(color: Color) -> El {
    status_dot(color).looping(PULSE)
}

/// A one-line note inside a card, between rows.
pub(super) fn note(key: &str, copy: &str, color: Color) -> El {
    row().key(key).px(METRICS.row_px).py(8.0).child(
        text(copy)
            .key("copy")
            .font(DESCRIPTION)
            .color(color)
            .wrap()
            .grow(1.0),
    )
}

/// The keyboard highlight in a search popover's results: Up and Down move
/// it, Enter picks it, and it returns to the first result whenever the query
/// changes.
#[derive(Default)]
pub(super) struct Highlight {
    query: String,
    index: usize,
}

impl Highlight {
    /// The highlighted result among `count` for `query`.
    pub(super) fn index(&self, query: &str, count: usize) -> Option<usize> {
        if count == 0 {
            None
        } else if self.query == query {
            Some(self.index.min(count - 1))
        } else {
            Some(0)
        }
    }

    /// Moves the highlight for Up or Down; false for any other key.
    pub(super) fn key(&mut self, key: &rune::Key, query: &str, count: usize) -> bool {
        let down = match key {
            rune::Key::Down => true,
            rune::Key::Up => false,
            _ => return false,
        };
        let current = self.index(query, count).unwrap_or(0);
        self.query = query.to_string();
        self.index = if down {
            (current + 1).min(count.saturating_sub(1))
        } else {
            current.saturating_sub(1)
        };
        true
    }
}

pub(super) static PLAY: rune::Icon = rune::Icon {
    name: "hex-play",
    view: 20.0,
    body: r#"<path d="M6.5 5.1v9.8c0 .5.5.8.9.5l7.6-4.9c.4-.2.4-.8 0-1L7.4 4.6c-.4-.3-.9 0-.9.5Z" fill="currentColor"/>"#,
};

pub(super) static PAUSE: rune::Icon = rune::Icon {
    name: "hex-pause",
    view: 20.0,
    body: r#"<rect x="5.5" y="4.5" width="3" height="11" rx="1" fill="currentColor"/><rect x="11.5" y="4.5" width="3" height="11" rx="1" fill="currentColor"/>"#,
};

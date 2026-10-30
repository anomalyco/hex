//! Settings controls on `rune-kit`, themed with OpenCode's design language
//! (`rune-theme-opencode`): the card, row, switch, segmented control,
//! select-style menus, buttons, keycaps, note, and text field. Colours,
//! type, radii, elevation, and motion come from the kit's tokens.

use rune::{
    Color, Edge, El, LoopSpec, Shadow, Spring, SpringPair, TextEdit, TextStyle, Transition, Tx,
    col, el, row, text,
};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Menu, MenuItem,
    Placement, SegmentedControl, Select, SelectAppearance, Switch, TextInput,
};

use super::Ui;

/// Row descriptions and secondary copy.
pub(super) const DESCRIPTION: TextStyle = TextStyle::new(12.0, 440.0).lh(16.0);
const SELECT_WIDTH: f32 = 220.0;
/// The microphone field, wide enough for "System default" and a faint
/// "Studio Display Microphone".
pub(super) const DEVICE_SELECT_WIDTH: f32 = 320.0;
/// Narrower than this, a row moves its control below the copy instead of
/// squeezing the description to a word per line (tiling desktops ignore
/// the window's 860 px minimum width).
const COPY_MIN_WIDTH: f32 = 100.0;
/// An [`icon_row`]'s icon and the gap after it.
const ICON_LEAD: f32 = 28.0;

/// A settings page's measurements: raised cards of rows under muted
/// labels. The title, section labels, and row text share the gutter; cards
/// hang outward by `row_px` on both sides, so row controls end where the
/// title bar's actions do.
#[derive(Clone, Copy)]
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
}

pub(super) const METRICS: Metrics = Metrics {
    gutter: 32.0,
    section_gap: 24.0,
    label_gap: 8.0,
    row_px: 16.0,
    row_h: 48.0,
    row_h_described: 60.0,
};

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
pub(super) struct Paint {
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
    /// A section label: above its card, on the page's text edge.
    pub(super) fn section_label(self, title: &str) -> El {
        text(title)
            .key("header")
            .font(self.k.ty.ui_medium.lh(SECTION_LABEL_LINE))
            .color(self.k.c.text_muted)
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
    let description = description.map(|(copy, color)| {
        text(copy)
            .key("description")
            .font(DESCRIPTION)
            .color(color)
            .wrap()
    });
    copy_row(p, key, None, title, description, control, false)
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
    let description = description.map(|copy| {
        text(copy)
            .key("description")
            .font(DESCRIPTION)
            .color(p.muted())
            .wrap()
    });
    copy_row(p, key, Some(glyph), title, description, control, false)
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
    let description = description.map(|(copy, color)| {
        text(copy)
            .key("description")
            .font(DESCRIPTION)
            .color(color)
            .wrap()
    });
    copy_row(p, key, Some(glyph), title, description, control, false)
}

/// A [`select`]'s field showing its one choice, disabled.
pub(super) fn fixed_select(p: Paint, key: &str, label: &str) -> El {
    Select::new::<&str>(format!("{key}-field"), &[])
        .appearance(SelectAppearance::Field)
        .width(SELECT_WIDTH)
        .current(Some(label))
        .disabled(true)
        .trigger(p.k)
        .key(key)
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
    pressable_row(icon_row(
        p,
        key,
        glyph,
        title,
        description.into(),
        row_switch(p, on),
    ))
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
    let m = METRICS;
    let height = if description.is_some() && !compact {
        m.row_h_described
    } else {
        m.row_h
    };
    let copy = col()
        .key("copy")
        .w(COPY_MIN_WIDTH)
        .min_w(COPY_MIN_WIDTH)
        .grow(1.0)
        .gap(if compact { 0.0 } else { 2.0 })
        .child(text(title).key("label").font(p.k.ty.ui).color(p.text()))
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
                .w(COPY_MIN_WIDTH + ICON_LEAD)
                .min_w(COPY_MIN_WIDTH + ICON_LEAD)
                .grow(1.0)
                .gap(ICON_LEAD - 16.0)
                .center_y()
                .child(rune::icon(glyph, 16.0).key("icon").color(p.k.c.icon_base))
                .child(copy),
        })
        .child(control)
}

/// The switch of a row that toggles: the row takes focus and presses, and
/// the switch shows its state and the focus ring.
pub(super) fn row_switch(p: Paint, on: bool) -> El {
    Switch::new(on).build(p.k).key("switch").focusable(false)
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
    pressable_row(setting_row(
        p,
        key,
        title,
        description.into(),
        row_switch(p, on),
    ))
    .on_press(flip)
}

/// A settings row that toggles when pressed anywhere. Only its switch
/// reacts to the pointer; the row itself never paints a hover or press
/// fill.
pub(super) fn pressable_row(row: El) -> El {
    row.role("toggle").focusable(true)
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

/// One choice of a [`select`].
pub(super) struct Choice {
    pub(super) label: String,
    /// Faint text after the label, such as what a choice resolves to.
    pub(super) detail: Option<String>,
    pub(super) selected: bool,
    pub(super) choose: Box<dyn Fn(&mut Ui)>,
}

/// A select field whose choices open in an anchored menu above the page.
pub(super) fn select(
    p: Paint,
    key: &str,
    label: &str,
    open: bool,
    choices: Vec<Choice>,
    toggle: impl Fn(&mut Ui) + 'static,
    layer: &mut Vec<El>,
) -> El {
    select_sized(
        p,
        key,
        label,
        None,
        open,
        choices,
        toggle,
        layer,
        SELECT_WIDTH,
    )
}

/// A [`select`] `width` points wide, for labels such as device names that
/// would truncate at the usual width. A `detail` follows the field's label
/// in faint text.
#[allow(clippy::too_many_arguments)]
pub(super) fn select_sized(
    p: Paint,
    key: &str,
    label: &str,
    detail: Option<&str>,
    open: bool,
    choices: Vec<Choice>,
    toggle: impl Fn(&mut Ui) + 'static,
    layer: &mut Vec<El>,
    width: f32,
) -> El {
    let trigger = match detail {
        None => Select::new::<&str>(format!("{key}-field"), &[])
            .appearance(SelectAppearance::Field)
            .width(width)
            .current(Some(label))
            .open(open)
            .on_toggle(toggle)
            .trigger(p.k),
        Some(detail) => detailed_field(p, key, label, detail, open, width).on_press(toggle),
    };
    let mut menu = Menu::new(key)
        .trigger(trigger)
        .placement(Placement::BottomEnd)
        .gutter(4.0)
        .min_width(width)
        .open(open)
        .on_close(|ui: &mut Ui| ui.open = None);
    for choice in choices {
        let mut item = MenuItem::new(choice.label).checked(choice.selected);
        if let Some(detail) = choice.detail {
            item = item.shortcut(detail);
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
        .label(format!("{label}, {detail}"))
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
                ),
        )
        .child(
            el().size(20.0, 20.0)
                .center()
                .child(rune::icon(chevron, 16.0).key("chevron").color(c.icon_muted)),
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
    let variant = match kind {
        ButtonKind::Primary => ButtonVariant::Contrast,
        ButtonKind::Secondary => ButtonVariant::Neutral,
        ButtonKind::Danger => ButtonVariant::Danger,
    };
    Button::new(label)
        .variant(variant)
        .on_press_tx(press)
        .build_push(p.k)
        .key(key)
}

/// An icon-only Secondary button the height of [`button`].
pub(super) fn icon_button(
    p: Paint,
    key: &str,
    icon: &'static rune::Icon,
    label: &str,
    press: impl Fn(&mut Ui) + 'static,
) -> El {
    IconButton::new(icon)
        .variant(IconButtonVariant::Neutral)
        .control_size(ControlSize::Large)
        .label(label)
        .on_press(press)
        .build(p.k)
        .key(key)
}

/// How a modal's surface enters and leaves: scaled from 0.96 about its
/// centre, with a fade. Its text scales with its boxes, so the panel moves
/// as one surface.
pub(super) fn modal_transition(p: Paint) -> Transition {
    Transition::scale(0.96).springs(overlay_springs(p))
}

/// The scrim behind a modal: the surface's springs, fading only.
pub(super) fn modal_scrim_transition(p: Paint) -> Transition {
    Transition::opacity().springs(overlay_springs(p))
}

fn overlay_springs(p: Paint) -> SpringPair {
    SpringPair::new(p.k.motion.overlay, p.k.motion.overlay)
}

/// How big a keycap is: `Small` in a row's tray, `Large` in the explainer.
///
/// Every key is a low-profile scissor key, after Apple's Magic Keyboard: a
/// flat face on a cap whose skirt shows below it. A press sinks the face
/// into the skirt; the key's bounds never change.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum KeySize {
    Small,
    Large,
}

impl KeySize {
    /// Height, skirt included.
    pub(super) const fn height(self) -> f32 {
        match self {
            Self::Small => 22.0,
            Self::Large => 28.0,
        }
    }

    /// A one-glyph key is square.
    pub(super) const fn min_width(self) -> f32 {
        self.height()
    }

    /// The skirt showing below a key at rest.
    const fn skirt(self) -> f32 {
        match self {
            Self::Small => 1.5,
            Self::Large => 2.0,
        }
    }

    /// A press sinks the face most of the way, leaving a hairline of skirt.
    const fn travel(self) -> f32 {
        self.skirt() - KEY_RIM
    }

    const fn face_radius(self) -> f32 {
        match self {
            Self::Small => 4.0,
            Self::Large => 5.0,
        }
    }

    pub(super) fn legend(self) -> TextStyle {
        match self {
            Self::Small => TextStyle::new(11.0, 400.0).lh(14.0),
            Self::Large => TextStyle::new(13.0, 400.0).lh(16.0),
        }
    }

    /// Beside the legend on the face.
    const fn face_padding(self) -> f32 {
        match self {
            Self::Small => 6.0,
            Self::Large => 8.0,
        }
    }
}

/// The cap's rim beside and above the face.
const KEY_RIM: f32 = 0.5;
/// A face sinks fast and comes back up a little slower, both without bounce.
const KEY_DOWN: Spring = Spring::new(0.08, 0.0);
const KEY_UP: Spring = Spring::new(0.18, 0.0);
/// Between the keycaps of a chord.
pub(super) const KEY_GAP: f32 = 3.0;
/// Between a tray's edge and its keys: a `Small` tray is as tall as a
/// button.
pub(super) const TRAY_PADDING: f32 = 3.0;

/// The keys' material, from the theme's tokens: in light a warm off-white
/// face (the warning cream warms the neutral layers) on a cap that shades
/// to warm grey; in dark a space-black face on a cap that falls to the
/// deepest background.
pub(super) struct KeyMaterial {
    pub(super) cap_top: Color,
    pub(super) cap_bottom: Color,
    pub(super) face: Color,
    pub(super) ink: Color,
    /// The tight shadow under the cap and the soft one it casts at rest.
    pub(super) contact: f32,
    pub(super) cast: f32,
}

impl KeyMaterial {
    pub(super) fn new(k: &Kit) -> Self {
        let c = &k.c;
        if k.dark {
            let face = c.bg_base.mix(c.layer_01, 0.6);
            Self {
                cap_top: c.layer_03.mix(c.layer_04, 0.35),
                cap_bottom: c.bg_deep,
                face,
                ink: c.text_base.mix(c.text_faint, 0.15),
                contact: 0.8,
                cast: 0.5,
            }
        } else {
            let warm = c.warning.bg;
            Self {
                cap_top: c.white,
                cap_bottom: c.layer_04.mix(c.text_faint, 0.28).mix(c.warning.fg, 0.05),
                face: c.layer_02.mix(warm, 0.35),
                ink: c.text_base.mix(c.text_muted, 0.15),
                contact: 0.2,
                cast: 0.12,
            }
        }
    }
}

/// One tray of keys per shortcut: the keys sit in a well recessed into
/// the surface, lit from above so its top edge falls into shadow. The well
/// clips its keys, so while it springs to a new shortcut's width they never
/// poke out over what is beside it.
pub(super) fn key_tray(
    p: Paint,
    key: &str,
    size: KeySize,
    keys: impl IntoIterator<Item = El>,
) -> El {
    let k = p.k;
    let c = &k.c;
    let (top, bottom, rim, well) = if k.dark {
        (
            c.bg_deep,
            c.bg_base.mix(c.layer_01, 0.7),
            k.light(0.07),
            0.6,
        )
    } else {
        (
            c.layer_04.mix(c.layer_03, 0.35),
            c.layer_03.mix(c.layer_02, 0.6),
            k.dark_a(0.07),
            0.14,
        )
    };
    // Concentric with the caps inside it.
    let radius = size.face_radius() + KEY_RIM + TRAY_PADDING;
    row()
        .key(key)
        .padding(TRAY_PADDING)
        .gap(KEY_GAP)
        .center_y()
        .radius(radius)
        .continuous_corners()
        .clip_rounded(radius)
        .fill(bottom)
        .gradient(top, bottom)
        .border(p.hair.max(0.5), rim)
        .box_shadow(Shadow {
            y: 1.0,
            blur: 2.5,
            color: k.dark_a(well),
            inset: true,
            ..Shadow::default()
        })
        .children(keys)
}

/// A shortcut's keys at rest, in one `Small` tray. Keys a new shortcut
/// adds rise into the well, and keys it drops leave through its top.
pub(super) fn keycaps(p: Paint, key: &str, parts: &[String]) -> El {
    key_tray(
        p,
        key,
        KeySize::Small,
        parts.iter().enumerate().map(|(index, part)| {
            keycap(p, index, part, KeySize::Small, false).transition(tray_roll())
        }),
    )
}

/// The width [`keycaps`] draws `parts` at, with `measure` giving a
/// legend's width.
#[cfg(hex_capture)]
pub(super) fn keycaps_width(parts: &[String], measure: impl Fn(&str, TextStyle) -> f32) -> f32 {
    let size = KeySize::Small;
    let caps: f32 = parts
        .iter()
        .map(|part| {
            (measure(part, size.legend()) + 2.0 * (size.face_padding() + KEY_RIM))
                .max(size.min_width())
        })
        .sum();
    caps + KEY_GAP * parts.len().saturating_sub(1) as f32 + 2.0 * TRAY_PADDING
}

/// In from the well's bottom, out through its top, inside its clip.
pub(super) fn tray_roll() -> Transition {
    Transition::push(Edge::Bottom)
}

/// One key: a face on a cap whose skirt shows below it. A press sinks the
/// face into the skirt, tightens the cap's shadow and shades the face
/// toward its bottom edge.
pub(super) fn keycap(p: Paint, index: usize, label: &str, size: KeySize, down: bool) -> El {
    let k = p.k;
    let m = KeyMaterial::new(k);
    let pressed = if down { 1.0 } else { 0.0 };
    let travel = size.travel() * pressed;
    let face_height = size.height() - KEY_RIM - size.skirt();
    // Shadow offsets never spring, so the resting and pressed casts are two
    // layers that trade strength.
    let cast = |y: f32, blur: f32, strength: f32| Shadow {
        y,
        blur,
        color: k.dark_a(m.cast * strength),
        ..Shadow::default()
    };
    let cap = col()
        .key("cap")
        .px(KEY_RIM)
        .pt(KEY_RIM)
        .pb(size.skirt() - travel)
        .radius(size.face_radius() + KEY_RIM)
        .continuous_corners()
        .fill(m.cap_bottom)
        .gradient(m.cap_top, m.cap_bottom)
        .shadow(0.5, 0.5, k.dark_a(m.contact))
        .box_shadow(cast(size.skirt(), 2.0 * size.skirt(), 1.0 - pressed))
        .box_shadow(cast(KEY_RIM, 2.0, pressed))
        .child(
            row()
                .key("face")
                .min_w(size.min_width() - 2.0 * KEY_RIM)
                .h(face_height)
                .px(size.face_padding())
                .center()
                .radius(size.face_radius())
                .continuous_corners()
                .fill(m.face)
                .gradient(
                    k.light(0.03 - 0.03 * pressed),
                    k.dark_a(0.02 + 0.06 * pressed),
                )
                .child(
                    text(label.to_string())
                        .key("label")
                        .font(size.legend())
                        .color(m.ink),
                ),
        );
    col()
        .key(format!("keycap{index}"))
        .h(size.height())
        .pt(travel)
        .motion(if down { KEY_DOWN } else { KEY_UP })
        .child(cap)
}

pub(super) fn status_dot(color: Color) -> El {
    el().key("dot").size(8.0, 8.0).radius(4.0).fill(color)
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

pub(super) fn text_field(
    p: Paint,
    key: &str,
    edit: &TextEdit,
    placeholder: &str,
    access: fn(&mut Ui) -> &mut TextEdit,
    submit: fn(&mut Ui, &mut Tx),
) -> El {
    TextInput::new(edit)
        .placeholder(placeholder)
        .width(Some(280.0))
        .on_edit(access)
        .on_submit(submit)
        .build(p.k)
        .key(key)
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

//! The list-and-detail layout Commands, Meetings, and History share: the
//! list on the pane title's edge with no rule beside it, and the selected
//! item in a raised card. A hovered row takes a hover fill and its action
//! (Copy) takes the time's place. Detail actions are ghost icons in the
//! card's corner.

use rune::{Align, El, Icon, col, el, icon, row, text};
use rune_kit::ScrollBar;

use super::Ui;
use super::controls::{DESCRIPTION, METRICS, Paint};

/// How far boxes (fields, row fills, the card) hang past the gutter, so the
/// text inside them lands on the title's edge.
pub(super) const HANG: f32 = METRICS.gutter - METRICS.row_px;
/// The list column's width, its hanging edge included.
const LIST_W: f32 = METRICS.list_w;
/// How far a list fades as rows slide under the search field.
const LIST_FADE: (f32, f32) = (18.0, 28.0);
/// The pane bar's thumb: a sleek 4 pt pill at rest and 6 under the pointer,
/// 2 pt in from the scrolling body's right edge.
const PANE_THUMB: f32 = 4.0;
const PANE_THUMB_HOVER: f32 = 6.0;
/// The list column's inset on both sides, so row fills sit as far from the
/// divider as from the panel's edge and row text lands on the title's rail.
const LIST_X: f32 = HANG;
/// The detail's inset: the gutter, so its right edge matches the header's
/// actions and its text rail matches the title's rhythm.
const DETAIL_X: f32 = METRICS.gutter;
/// From the rule under the header to the search field and the detail's title.
const BODY_TOP: f32 = 16.0;
/// The search field's height (the kit's `TextInput`). The detail's first row
/// is as tall and starts where the field does, so the field, the title and
/// the title's actions share one centre line across the divider.
pub(super) const BAR_H: f32 = 28.0;
/// A status tag's height. The meta line is as tall with or without a tag,
/// so the body under it holds still when a recording finishes.
pub(super) const META_H: f32 = 18.0;
/// A quiet header action's height, a step under the pane header's buttons.
pub(super) const QUIET_H: f32 = 24.0;

pub(super) static COPY_ICON: &Icon = &rune_theme_opencode::icons::OUTLINE_COPY;
pub(super) static TRASH_ICON: &Icon = &rune_theme_opencode::icons::TRASH;
// Only macOS History's Edit uses it, now Rename is in the meeting's menu.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) static EDIT_ICON: &Icon = &rune_theme_opencode::icons::EDIT;
pub(super) static HISTORY_ICON: &Icon = &rune_theme_opencode::icons::STATUS;
pub(super) static SEARCH_ICON: &Icon = &rune_theme_opencode::icons::MAGNIFYING_GLASS;

/// The pane's body, a full-bleed split: a hairline under the header, then
/// `list` (a [`list_column`]), a hairline divider, and the detail filling
/// the panel to its right and bottom edges.
pub(super) fn body(p: Paint, key: &str, list: El, detail: El) -> El {
    col()
        .key(key)
        .grow(1.0)
        .shrink()
        .min_h(0.0)
        .child(hairline(p, "body-rule").h(p.hair))
        .child(
            row()
                .key("split")
                .grow(1.0)
                .shrink()
                .min_h(0.0)
                .child(list)
                .child(hairline(p, "split-rule").w(p.hair).h_full())
                .child(
                    col()
                        .key("detail-card")
                        .grow(1.0)
                        .shrink()
                        .min_w(0.0)
                        .px(DETAIL_X)
                        .pt(BODY_TOP)
                        .clip_soft(0.0)
                        .child(detail),
                ),
        )
}

fn hairline(p: Paint, key: &str) -> El {
    el().key(key).fill(p.k.c.border_weak).a11y_hidden()
}

/// The search field over the scrolling list, both inset alike from the
/// panel's edge and the divider; the list's bar runs down the divider.
pub(super) fn list_column(p: Paint, search: El, scroll_key: &str, list: El) -> El {
    col()
        .key("list-column")
        .w(LIST_W + LIST_X)
        .h_full()
        .child(
            col()
                .key("search-row")
                .px(LIST_X)
                .pt(BODY_TOP)
                .child(search),
        )
        .child(super::faded_body(
            p.k,
            scroll_key,
            super::lists::source_list(list).px(LIST_X),
            LIST_FADE,
        ))
}

/// Notices (errors, the opt-in card) above the body, hung like the cards.
pub(super) fn notices(children: Vec<El>) -> El {
    col()
        .key("notices")
        .px(HANG)
        .pb(16.0)
        .gap(8.0)
        .children(children)
}

/// A group heading inside a list (a task, a day), on the rows' text rail
/// over any leading icons, as section labels sit over settings rows.
pub(super) fn group(p: Paint, key: &str, label: &str, first: bool) -> El {
    text(label)
        .key(key)
        .role("heading")
        .level(3)
        .font(p.k.ty.small_medium)
        .color(p.faint())
        .px(METRICS.row_px)
        .pt(if first { 0.0 } else { 16.0 })
        .pb(6.0)
}

/// A folded section's chevron, pointing at its label.
pub(super) static FOLDED: Icon = Icon {
    name: "hex-fold-closed",
    view: 16.0,
    body: r#"<path d="M6 4l4 4-4 4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>"#,
};
/// An open section's chevron, pointing down at its content.
pub(super) static UNFOLDED: Icon = Icon {
    name: "hex-fold-open",
    view: 16.0,
    body: r#"<path d="M4 6l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

/// One list row's words: the title over its meta line, and the time (or
/// another short value) on the right, aligned down the list. `hover` takes
/// the time's place while the pointer is over the row.
pub(super) struct Item {
    pub(super) key: String,
    pub(super) title: String,
    pub(super) meta: String,
    /// The meta line's colour when it reports a state (Recording, Failed).
    pub(super) meta_ink: Option<rune::Color>,
    /// An optional one-line muted note under `meta` (a failed meeting's reason).
    pub(super) note: Option<String>,
    pub(super) time: String,
    pub(super) hover: Vec<El>,
    pub(super) selected: bool,
}

/// Leading icon size and gap, matching settings rows' `METRICS.icon_lead`.
const LEADING_ICON: f32 = METRICS.icon;
const LEADING_GAP: f32 = METRICS.icon_lead - METRICS.icon;

/// One selectable list row with a small faint icon before its title when
/// `leading` is set; assistive technology hears the title alone.
pub(super) fn item_with_icon(
    p: Paint,
    item: Item,
    leading: Option<&'static Icon>,
    select: impl Fn(&mut Ui) + 'static,
) -> El {
    let selected = item.selected;
    let row = item_body(p, item, leading);
    super::lists::source_item(
        super::lists::select_at_once(row, selected, select),
        selected,
    )
}

/// [`item_with_icon`] whose press its pane wires itself, for a list that
/// selects several (History's Shift-, Command-, and drag-selection): `press`
/// gets the finished row, which it draws and gives its gesture.
pub(super) fn item_with_press(p: Paint, item: Item, press: impl Fn(El) -> El) -> El {
    let selected = item.selected;
    super::lists::source_item(press(item_body(p, item, None)), selected)
}

/// One row's box, words, and selected fill, before any press handling.
pub(super) fn item_body(p: Paint, item: Item, leading: Option<&'static Icon>) -> El {
    let c = &p.k.c;
    // A row of the list: its title names it and its meta and time describe
    // it, so their texts stay out of the tree.
    let described = [
        Some(item.meta.as_str()),
        item.note.as_deref(),
        Some(item.time.as_str()),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    let time = text(item.time)
        .key("time")
        .font(p.tabular(p.meta()))
        .color(p.faint())
        .align(Align::End)
        .a11y_hidden();
    let end = if item.hover.is_empty() {
        time
    } else {
        el().key("end").child(time.group_opacity(1.0, 0.0)).child(
            row()
                .key("hover")
                .gap(2.0)
                .center_y()
                .pin(None, Some(-5.0), Some(row_action_inset(p)), None)
                .group_opacity(0.0, 1.0)
                .children(item.hover),
        )
    };
    let words = col()
        .key("words")
        .gap(2.0)
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .child(
            row()
                .key("top")
                .gap(12.0)
                .center_y()
                .child(
                    text(item.title.clone())
                        .key("title")
                        .font(p.k.ty.ui_medium)
                        .color(p.text())
                        .grow(1.0)
                        .shrink()
                        .truncate()
                        .a11y_hidden(),
                )
                .child(end),
        )
        .child(
            text(item.meta)
                .key("meta")
                .font(p.k.ty.small)
                .color(item.meta_ink.unwrap_or(p.muted()))
                .truncate()
                .a11y_hidden(),
        )
        .children(item.note.map(|note| {
            text(note)
                .key("note")
                .font(p.k.ty.small)
                .color(p.muted())
                .truncate()
                .a11y_hidden()
        }));
    // The icon in its own column, centred on the title's line,
    // so the title and the lines under it share one left edge.
    let mut row_el = row()
        .key(item.key)
        .role("listitem")
        .label(item.title)
        .description(described)
        .min_h(METRICS.item_h)
        .px(METRICS.row_px)
        .py(8.0)
        .gap(LEADING_GAP)
        .items_start()
        .radius(p.k.radius.md)
        .hover(c.hover)
        .hover_group()
        .children(leading.map(|glyph| {
            el().key("leading")
                .w(LEADING_ICON)
                .h(p.k.ty.ui_medium.line_px())
                .center()
                .child(
                    icon(glyph, LEADING_ICON)
                        .key("leading-icon")
                        .color(c.icon_faint),
                )
                .a11y_hidden()
        }))
        .child(words);
    if item.selected {
        row_el = row_el.fill(c.layer_02);
    }
    row_el
}

/// The card's title block: the title over its meta line, actions at the
/// top right.
pub(super) fn card_header(title: El, meta: Vec<El>, actions: Vec<El>) -> El {
    row()
        .key("detail-header")
        .gap(8.0)
        .items_start()
        .child(
            col()
                .key("title")
                .grow(1.0)
                .shrink()
                .gap(4.0)
                .child(title)
                .child(row().key("meta").gap(8.0).center_y().children(meta)),
        )
        .child(
            row()
                .key("actions")
                .gap(2.0)
                .mt(-2.0)
                .mr(-6.0)
                .center_y()
                .children(actions),
        )
}

/// A detail's utility bar: what the item is on the left, its quiet actions
/// on the right. The bar is as tall as the search field and starts where it
/// does, so both sides of the divider share one centre line, and the last
/// action ends on the gutter, under the pane header's. The title is left to
/// the document under it.
pub(super) fn utility_bar(meta: Vec<El>, actions: Vec<El>) -> El {
    row()
        .key("detail-bar")
        .min_h(BAR_H)
        .gap(12.0)
        .center_y()
        .child(meta_row(meta).grow(1.0).shrink().min_w(0.0))
        .child(row().key("actions").gap(2.0).center_y().children(actions))
}

/// A detail's meta line: a status tag, if any, then the meta words, centred
/// on one line [`META_H`] tall.
pub(super) fn meta_row(meta: Vec<El>) -> El {
    row()
        .key("meta")
        .min_h(META_H)
        .gap(8.0)
        .center_y()
        .children(meta)
}

/// A quiet action in a detail's header: a ghost button whose icon and label
/// rest muted, a step under the title's ink, and take the text colour under
/// the pointer. With no `label` it is the icon alone in a square. `name`
/// says in full what it does: the accessible name, and the tooltip when the
/// label is shorter. `open` is whether the menu it opens is open, for an
/// action that opens one: it holds the pressed look meanwhile.
pub(super) fn quiet_action(
    p: Paint,
    key: &str,
    glyph: &'static Icon,
    label: Option<&str>,
    name: &str,
    open: Option<bool>,
    press: impl Fn(&mut Ui, &mut rune::Tx) + 'static,
) -> El {
    let hovered = p.text();
    let glyph_ink = if open == Some(true) {
        hovered
    } else {
        p.k.c.icon_muted
    };
    let glyph_size = if label.is_some() { 14.0 } else { METRICS.icon };
    let shown = label.map(str::to_owned);
    let mut button = rune_kit::Button::new(name)
        .variant(rune_kit::ButtonVariant::GhostMuted)
        .label_with(move |font, ink| {
            row()
                .gap(6.0)
                .center_y()
                .child(
                    icon(glyph, glyph_size)
                        .key("icon")
                        .color(glyph_ink)
                        .group_color(hovered),
                )
                .children(shown.map(|label| {
                    text(label)
                        .key("label")
                        .font(font)
                        .color(ink)
                        .group_color(hovered)
                }))
        })
        .on_press_tx(press);
    if let Some(open) = open {
        button = button.expanded(open);
    }
    if label != Some(name) {
        button = button.help(name);
    }
    let button = button
        .build(p.k)
        .h(QUIET_H)
        .radius(p.k.radius.sm)
        .description("")
        .hover_group()
        .key(key);
    if label.is_some() {
        button.px(8.0)
    } else {
        button.px(0.0).w(QUIET_H)
    }
}

/// The detail's heading: the document's title, at its column's whole width.
pub(super) const CARD_HEADING: rune::TextStyle = super::controls::HEADLINE;

/// [`card_title`] at [`CARD_HEADING`], for the meeting's card.
pub(super) fn card_heading(p: Paint, title: &str) -> El {
    text(title)
        .key("detail-title")
        .role("heading")
        .level(2)
        .font(CARD_HEADING)
        .color(p.text())
        .wrap()
}

pub(super) fn card_title(p: Paint, title: &str) -> El {
    text(title)
        .key("detail-title")
        .role("heading")
        .level(2)
        .font(p.k.ty.body_medium)
        .color(p.text())
        .wrap()
}

pub(super) fn meta_text(p: Paint, meta: &str) -> El {
    text(meta)
        .key("meta-line")
        .font(DESCRIPTION)
        .color(p.muted())
        .truncate()
}

/// The card's scrolling content under its header, clear of the scroll bar.
pub(super) fn card_scroll(p: Paint, key: &str, content: El) -> El {
    card_scroll_sticky(p, key, content, false)
}

/// A [`card_scroll`] that, with `stick_bottom`, follows content added at
/// its end while scrolled to the bottom.
pub(super) fn card_scroll_sticky(p: Paint, key: &str, content: El, stick_bottom: bool) -> El {
    // The viewport spans the detail's whole width, so its bar runs down the
    // panel's right edge; the content keeps the detail's inset.
    faded_body_sticky(p.k, key, content.px(DETAIL_X), LIST_FADE, stick_bottom).mx(-DETAIL_X)
}

/// The window's faded scroll body (`super::faded_body`); with
/// `stick_bottom` it keeps its newest content in view while scrolled to the
/// bottom (a recording meeting's transcript).
pub(super) fn faded_body_sticky(
    k: &rune_kit::Kit,
    key: &str,
    content: El,
    (top, bottom): (f32, f32),
    stick_bottom: bool,
) -> El {
    // Both shrink below their content in a fixed-height parent (#906).
    let viewport = col().key(key).grow(1.0).shrink().min_h(0.0);
    let viewport = viewport
        .clip_fade(0.0, top, 0.0, bottom)
        .scroll(super::ScrollSpec { stick_bottom })
        .child(col().key("inset").pt(top).pb(bottom).child(content));
    let style = scroll_style(k);
    viewport
        .scroll_bar_styled(style)
        .grow(1.0)
        .shrink()
        .min_h(0.0)
}

/// The bar every scrolling body in a list-and-detail pane draws.
fn scroll_style(k: &rune_kit::Kit) -> rune_kit::ScrollBarStyle {
    rune_kit::ScrollBarStyle::from_kit(k)
        .thumb(k.c.text_faint)
        .thumb_hover(k.c.text_muted)
        .width(PANE_THUMB)
        .hover_width(PANE_THUMB_HOVER)
}

/// The room a scrolling body leaves for its scroll bar's press strip.
pub(super) fn scroll_gutter(k: &rune_kit::Kit) -> f32 {
    scroll_style(k).strip()
}

/// The right inset a row's hover actions take: the row's own padding already
/// clears the bar, so their right edge stays on the text's edge.
pub(super) fn row_action_inset(p: Paint) -> f32 {
    (scroll_gutter(p.k) - METRICS.row_px).max(0.0)
}

/// A quiet empty state: an icon, a short title, and at most one line.
pub(super) fn empty_state(
    p: Paint,
    key: &str,
    glyph: &'static Icon,
    title: &str,
    hint: Option<&str>,
) -> El {
    col()
        .key(key)
        .grow(1.0)
        .shrink()
        .center()
        .gap(6.0)
        .pb(40.0)
        .child(
            el().key("badge")
                .size(40.0, 40.0)
                .mb(6.0)
                .radius(p.k.radius.lg)
                .fill(p.k.c.layer_01)
                .center()
                .child(icon(glyph, 20.0).key("icon").color(p.k.c.icon_faint))
                .a11y_hidden(),
        )
        .child(
            text(title)
                .key("title")
                .font(p.k.ty.ui_medium)
                .color(p.text()),
        )
        .children(hint.map(|hint| {
            text(hint)
                .key("hint")
                .font(DESCRIPTION)
                .color(p.muted())
                .align(Align::Center)
                .wrap()
                .w(METRICS.copy_w)
        }))
}

/// Label and value rows separated by hairlines on the text edges.
pub(super) fn facts(p: Paint, facts: Vec<(&str, String)>) -> El {
    let mut list = col().key("facts");
    for (index, (label, value)) in facts.into_iter().enumerate() {
        if index > 0 {
            list = list.child(
                el().key(format!("sep{index}"))
                    .h(p.hair)
                    .fill(p.k.c.border_weak)
                    .a11y_hidden(),
            );
        }
        list = list.child(
            row()
                .key(format!("fact{index}"))
                .py(8.0)
                .gap(16.0)
                .child(
                    text(label)
                        .key("label")
                        .font(DESCRIPTION)
                        .color(p.muted())
                        .w(METRICS.fact_label_w),
                )
                .child(
                    text(value)
                        .key("value")
                        .font(DESCRIPTION)
                        .color(p.text())
                        .wrap()
                        .grow(1.0)
                        .shrink(),
                ),
        );
    }
    list
}

/// A small ghost icon button for the card's header.
pub(super) fn ghost_icon(
    p: Paint,
    key: &str,
    glyph: &'static Icon,
    label: &str,
    press: impl Fn(&mut Ui, &mut rune::Tx) + 'static,
) -> El {
    rune_kit::IconButton::new(glyph)
        .variant(rune_kit::IconButtonVariant::Ghost)
        .control_size(rune_kit::ControlSize::Small)
        .help(label)
        .on_press_tx(press)
        .build(p.k)
        .key(key)
}

/// A row's hover action: a pointer shortcut, since keys move between rows
/// and the card has the same action.
pub(super) fn row_action(
    p: Paint,
    key: &str,
    glyph: &'static Icon,
    label: &str,
    press: impl Fn(&mut Ui, &mut rune::Tx) + 'static,
) -> El {
    ghost_icon(p, key, glyph, label, press).focusable(false)
}

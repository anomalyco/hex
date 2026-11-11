//! The list-and-detail layout Commands, Meetings, and History share: the
//! list on the pane title's edge with no rule beside it, and the selected
//! item in a raised card. A hovered row takes a hover fill and its action
//! (Copy) takes the time's place. Detail actions are ghost icons in the
//! card's corner.

use rune::{Align, El, Icon, col, el, icon, row, text};
use rune_kit::ScrollBar;

use super::Ui;
use super::controls::{DESCRIPTION, METRICS, Paint, raised};

/// How far boxes (fields, row fills, the card) hang past the gutter, so the
/// text inside them lands on the title's edge.
pub(super) const HANG: f32 = METRICS.gutter - METRICS.row_px;
/// The list column's width, its hanging edge included.
const LIST_W: f32 = 316.0;
/// How far a list fades as rows slide under the search field.
const LIST_FADE: (f32, f32) = (18.0, 28.0);
/// Room the card's text leaves for its scroll bar.
const SCROLL_BAR: f32 = 12.0;

/// The pane bar's thumb: 8 pt at rest and 10 under the pointer, so it stays
/// inside the card's 12 pt scroll-bar room.
const PANE_THUMB: f32 = 8.0;
const PANE_THUMB_HOVER: f32 = 10.0;

pub(super) static COPY_ICON: &Icon = &rune_theme_opencode::icons::OUTLINE_COPY;
pub(super) static TRASH_ICON: &Icon = &rune_theme_opencode::icons::TRASH;
pub(super) static EDIT_ICON: &Icon = &rune_theme_opencode::icons::EDIT;
pub(super) static HISTORY_ICON: &Icon = &rune_theme_opencode::icons::STATUS;
pub(super) static SEARCH_ICON: &Icon = &rune_theme_opencode::icons::MAGNIFYING_GLASS;

/// The pane's body: `list` (a [`list_column`]) beside the detail's raised
/// card, which hangs as far past the right gutter as the rows do past the
/// left.
pub(super) fn body(p: Paint, key: &str, list: El, detail: El) -> El {
    row().key(key).grow(1.0).shrink().child(list).child(
        raised(p, "detail-card")
            .grow(1.0)
            .shrink()
            .min_w(0.0)
            .ml(12.0)
            .mr(HANG)
            .mb(HANG)
            .px(24.0)
            .pt(20.0)
            .clip_soft(0.0)
            .child(detail),
    )
}

/// The search field over the scrolling list, both on the title's edge.
pub(super) fn list_column(p: Paint, search: El, scroll_key: &str, list: El) -> El {
    col()
        .key("list-column")
        .w(LIST_W)
        .h_full()
        .pl(HANG)
        .child(search)
        .child(super::faded_body(
            p.k,
            scroll_key,
            super::lists::source_list(list),
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

/// A group heading inside a list (a task, a day), on the rows' text edge.
pub(super) fn group(p: Paint, key: &str, label: &str, first: bool) -> El {
    text(label)
        .key(key)
        .role("heading")
        .level(3)
        .font(p.k.ty.small_medium)
        .color(p.faint())
        .px(METRICS.row_px)
        .pt(if first { 0.0 } else { 18.0 })
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
    pub(super) time: String,
    pub(super) hover: Vec<El>,
    pub(super) selected: bool,
}

/// The size of a row's leading icon, about the meta line's height.
const LEADING_ICON: f32 = 14.0;

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
fn item_body(p: Paint, item: Item, leading: Option<&'static Icon>) -> El {
    let c = &p.k.c;
    // A row of the list: its title names it and its meta and time describe
    // it, so their texts stay out of the tree.
    let described = [item.meta.as_str(), item.time.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let time = text(item.time)
        .key("time")
        .font(p.k.ty.small)
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
                .pin(None, Some(-5.0), Some(-6.0), None)
                .group_opacity(0.0, 1.0)
                .children(item.hover),
        )
    };
    let mut row_el = col()
        .key(item.key)
        .role("listitem")
        .label(item.title.clone())
        .description(described)
        .px(METRICS.row_px)
        .py(9.0)
        .gap(2.0)
        .radius(p.k.radius.md)
        .hover(c.hover)
        .hover_group()
        .child(
            row()
                .key("top")
                .gap(12.0)
                .center_y()
                .children(leading.map(|glyph| {
                    icon(glyph, LEADING_ICON)
                        .key("leading-icon")
                        .color(c.icon_faint)
                        .mr(-6.0)
                        .a11y_hidden()
                }))
                .child(
                    text(item.title)
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
        );
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
                .gap(3.0)
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
    faded_body_sticky(p.k, key, content.pr(SCROLL_BAR), LIST_FADE, stick_bottom)
}

/// The window's faded scroll body (`super::faded_body`); with
/// `stick_bottom` it keeps its newest content in view while scrolled to the
/// bottom (Labs › Follow live transcript).
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
    let style = rune_kit::ScrollBarStyle::from_kit(k)
        .thumb(k.c.text_faint)
        .thumb_hover(k.c.text_muted)
        .width(PANE_THUMB)
        .hover_width(PANE_THUMB_HOVER);
    viewport
        .scroll_bar_styled(style)
        .grow(1.0)
        .shrink()
        .min_h(0.0)
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
                .w(300.0)
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
                .py(9.0)
                .gap(16.0)
                .child(
                    text(label)
                        .key("label")
                        .font(DESCRIPTION)
                        .color(p.muted())
                        .w(112.0),
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
        .label(label)
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

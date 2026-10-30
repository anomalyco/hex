//! The list-and-detail layout Commands, Meetings, and History share: the
//! list on the pane title's edge with no rule beside it, and the selected
//! item in a raised card. A hovered row takes a hover fill and its action
//! (Copy) takes the time's place. Detail actions are ghost icons in the
//! card's corner.

use rune::{Align, El, Icon, col, el, icon, row, text};

use super::Ui;
use super::controls::{DESCRIPTION, METRICS, Paint, raised};

/// How far boxes (fields, row fills, the card) hang past the gutter, so the
/// text inside them lands on the title's edge.
pub(super) const HANG: f32 = METRICS.gutter - METRICS.row_px;
/// The list column's width, its hanging edge included.
const LIST_W: f32 = 316.0;
/// How far a list fades as rows slide under the search field.
const LIST_FADE: (f32, f32) = (18.0, 28.0);

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
pub(super) fn list_column(search: El, scroll_key: &str, list: El) -> El {
    col()
        .key("list-column")
        .w(LIST_W)
        .h_full()
        .pl(HANG)
        .child(search)
        .child(super::faded_body(
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
        .font(p.k.ty.small_medium)
        .color(p.faint())
        .px(METRICS.row_px)
        .pt(if first { 0.0 } else { 18.0 })
        .pb(6.0)
}

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

/// A selectable list row hung past the gutter: a hover fill (and the row's
/// action in the time's place), a quiet fill when selected. Nothing on it
/// animates.
pub(super) fn item(p: Paint, item: Item, select: impl Fn(&mut Ui) + 'static) -> El {
    let c = &p.k.c;
    let time = text(item.time)
        .key("time")
        .font(p.k.ty.small)
        .color(p.faint())
        .align(Align::End);
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
        .role("button")
        .label(item.title.clone())
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
                .child(
                    text(item.title)
                        .key("title")
                        .font(p.k.ty.ui_medium)
                        .color(p.text())
                        .grow(1.0)
                        .shrink()
                        .truncate(),
                )
                .child(end),
        )
        .child(
            text(item.meta)
                .key("meta")
                .font(p.k.ty.small)
                .color(item.meta_ink.unwrap_or(p.muted()))
                .truncate(),
        );
    if item.selected {
        row_el = row_el.fill(c.layer_02);
    }
    let row_el = super::lists::select_at_once(row_el, item.selected, select);
    super::lists::source_item(row_el, item.selected)
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

/// The card's scrolling content under its header.
pub(super) fn card_scroll(key: &str, content: El) -> El {
    super::faded_body(key, content, LIST_FADE)
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
                .child(icon(glyph, 20.0).key("icon").color(p.k.c.icon_faint)),
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
                    .fill(p.k.c.border_weak),
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

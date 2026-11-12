//! The Labs store: a search and the Discover and Turned on tabs in the
//! title bar, group chips, the newest labs as featured cards, then every
//! lab as a card in two columns under its group's header. A header folds
//! its group; a search or chip shows every match open. A card's switch
//! turns its lab on or off; a lab with settings of its own shows them on
//! its card while on.

use rune::{El, Icon, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Switch, TextInput};

use super::super::Ui;
use super::super::controls::{DESCRIPTION, METRICS, Paint, PushSize, segmented_tx};
use super::super::list_detail::{FOLDED, UNFOLDED};
use super::super::lists::{Tone, tone_tag};
use super::{Row, Tab, extras, flip, group, row_key, shown};
use crate::desktop::i18n::{tr, tr_fill};
use crate::labs::Group;

/// The search field beside the title.
pub(in crate::rune_ui) fn search_field(ui: &Ui, p: Paint) -> El {
    let count = shown(ui).len();
    let mut field = TextInput::new(&ui.lab_store.search)
        .placeholder(tr_fill("Search {} labs", &count.to_string()))
        .width(Some(SEARCH_WIDTH))
        .leading_icon(p.k.glyph.search)
        .on_edit(|ui: &mut Ui| &mut ui.lab_store.search);
    if !ui.lab_store.search.text.is_empty() {
        field = field.clear_button();
    }
    field.build(p.k).key("labs-search")
}

const SEARCH_WIDTH: f32 = 220.0;
/// Between cards, across and down.
const GAP: f32 = 8.0;

/// Discover and Turned on, at the title bar's end.
pub(in crate::rune_ui) fn tabs(ui: &Ui, p: Paint) -> El {
    let on = shown(ui).iter().filter(|row| row.on).count();
    let turned_on = tr_fill("Turned on · {}", &on.to_string());
    let labels = [tr("Discover"), turned_on.as_str()];
    let selected = match ui.lab_store.tab {
        Tab::Discover => 0,
        Tab::On => 1,
    };
    segmented_tx(
        p,
        "labs-tab",
        &labels,
        selected,
        |ui: &mut Ui, index: usize, tx: &mut Tx| {
            tx.settle();
            ui.lab_store.tab = if index == 0 { Tab::Discover } else { Tab::On };
        },
    )
}

/// The page under the title bar: the chips, then the scrolling cards.
pub(in crate::rune_ui) fn page(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let rows = shown(ui);
    let store = &ui.lab_store;
    let query = store.query();
    let newest = rows.iter().map(|row| row.lab.added()).max().unwrap_or(0);
    let is_new = |row: &Row| newest > 0 && row.lab.added() == newest;
    let mut content = col().key("content").px(METRICS.gutter).gap(12.0);
    let browsing = store.tab == Tab::Discover && store.group.is_none() && query.is_empty();
    // Labs in New in Labs aren't repeated in the grid below.
    let mut featured_labs = Vec::new();
    if browsing {
        let mut featured: Vec<&Row> = rows.iter().filter(|row| is_new(row)).collect();
        featured.sort_by_key(|row| std::cmp::Reverse((row.lab.added(), row.lab.index())));
        featured.truncate(2);
        featured_labs = featured.iter().map(|row| row.lab).collect();
        if !featured.is_empty() {
            content = content
                .child(heading(p, "new-heading", tr("New in Labs")))
                .child(pair(
                    "featured",
                    featured
                        .into_iter()
                        .map(|row| card(ui, p, row, true, true, layer))
                        .collect(),
                ));
        }
    }
    let matching: Vec<&Row> = rows
        .iter()
        .filter(|row| store.tab == Tab::Discover || row.on)
        .filter(|row| store.group.is_none_or(|group| row.group == group))
        .filter(|row| matches(row, &query))
        .filter(|row| !featured_labs.contains(&row.lab))
        .collect();
    if matching.is_empty() {
        let empty = if store.tab == Tab::On && query.is_empty() {
            tr("Nothing turned on yet")
        } else {
            tr("No labs match")
        };
        content = content.child(
            col().key("empty").py(48.0).center().child(
                text(empty)
                    .key("empty-copy")
                    .font(p.k.ty.ui)
                    .color(p.muted()),
            ),
        );
    } else {
        // A search or chip shows every match, whatever its group's fold.
        let filtering = !query.is_empty() || store.group.is_some();
        for group in Group::ALL {
            let members: Vec<&Row> = matching
                .iter()
                .copied()
                .filter(|row| row.group == group)
                .collect();
            if members.is_empty() {
                continue;
            }
            let open = filtering || group::open(ui, group, &rows);
            content = content.child(group_header(p, group, members.len(), open, filtering));
            if open {
                let mut grid = col().key(format!("grid-{}", group.id())).gap(GAP);
                for (index, pair_rows) in members.chunks(2).enumerate() {
                    grid = grid.child(pair(
                        &format!("grid-row{index}"),
                        pair_rows
                            .iter()
                            .map(|row| card(ui, p, row, is_new(row), false, layer))
                            .collect(),
                    ));
                }
                content = content.child(grid);
            }
        }
    }
    content = content.child(footer(p));
    col()
        .key("labs-store")
        .grow(1.0)
        .shrink()
        .min_h(0.0)
        .child(chips(&rows, store.group, p))
        .child(super::super::scroll_body(p.k, "scroll", content))
}

/// Whether `row` matches a lowercase search.
fn matches(row: &Row, query: &str) -> bool {
    query.is_empty()
        || [row.title, row.about, group::name(row.group)]
            .iter()
            .any(|copy| copy.to_lowercase().contains(query))
}

fn heading(p: Paint, key: &str, title: &str) -> El {
    p.section_label(title).key(key)
}

/// Two cards side by side, equally wide; a lone card keeps half the row.
fn pair(key: &str, cards: Vec<El>) -> El {
    row()
        .key(key)
        .gap(GAP)
        .equal_columns(2)
        .items_stretch()
        .children(cards.into_iter().map(|card| card.min_w(0.0)))
}

/// A group's header: its name and count. While browsing it is a button
/// with a chevron that folds the group (remembered); under a search or
/// chip it is a plain heading.
fn group_header(p: Paint, group: Group, count: usize, open: bool, filtering: bool) -> El {
    let c = &p.k.c;
    let name = group::name(group);
    let mut header = row()
        .key(format!("group-{}", group.id()))
        .gap(6.0)
        .center_y()
        .child(heading(p, "label", name).a11y_hidden())
        .child(
            text(count.to_string())
                .key("count")
                .font(p.k.ty.small)
                .color(p.faint())
                .a11y_hidden(),
        );
    if filtering {
        return header.role("heading").level(2).label(name).mt(8.0);
    }
    header = header
        .child(
            rune::icon(if open { &UNFOLDED } else { &FOLDED }, 14.0)
                .key("chevron")
                .color(c.icon_faint)
                .a11y_hidden(),
        )
        .role("button")
        .label(name)
        .expanded(open)
        .focusable(true)
        .focus_ring(2.0, 0.0, c.border_focus)
        .radius(p.k.radius.sm)
        .hover(c.hover)
        .press(c.pressed)
        .on_press(move |ui: &mut Ui| group::fold(ui, group));
    // As wide as its words, with the hover fill just past them.
    row()
        .key(format!("group-{}-slot", group.id()))
        .role("none")
        .mt(8.0)
        .ml(-4.0)
        .child(header.px(4.0).py(2.0))
}

/// All, then a chip per group with its count.
fn chips(rows: &[Row], selected: Option<Group>, p: Paint) -> El {
    let mut chips = row()
        .key("chips")
        .px(METRICS.gutter)
        .pb(8.0)
        .gap(6.0)
        .flex_wrap()
        .child(chip(
            p,
            "chip-all",
            tr("All"),
            rows.len(),
            selected.is_none(),
            None,
        ));
    for group in Group::ALL {
        let count = rows.iter().filter(|row| row.group == group).count();
        if count == 0 {
            continue;
        }
        chips = chips.child(chip(
            p,
            &format!("chip-{}", group.id()),
            group::name(group),
            count,
            selected == Some(group),
            Some(group),
        ));
    }
    chips
}

fn chip(
    p: Paint,
    key: &str,
    label: &str,
    count: usize,
    selected: bool,
    group: Option<Group>,
) -> El {
    let c = &p.k.c;
    let chip = row()
        .key(key)
        .role("radio")
        .label(label)
        .h(26.0)
        .px(10.0)
        .gap(6.0)
        .center_y()
        .radius(13.0)
        .border(
            p.hair,
            if selected {
                c.border_base
            } else {
                c.border_weak
            },
        )
        .hover(c.hover)
        .press(c.pressed)
        .child(text(label).key("label").font(p.k.ty.ui).color(if selected {
            p.text()
        } else {
            p.muted()
        }))
        .child(
            text(count.to_string())
                .key("count")
                .font(p.k.ty.small)
                .color(p.faint()),
        )
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| {
            tx.settle();
            ui.lab_store.group = group;
        });
    if selected {
        chip.fill(c.layer_02)
    } else {
        chip
    }
}

/// A rounded neutral square with the lab's icon.
fn tile(p: Paint, icon: &'static Icon, size: f32) -> El {
    el().key("tile")
        .size(size, size)
        .center()
        .radius(if size > 40.0 {
            p.k.radius.lg
        } else {
            p.k.radius.md
        })
        .fill(p.k.c.layer_02)
        .border(p.hair, p.k.c.border_weak)
        .child(
            rune::icon(icon, (size * 0.5).round())
                .key("glyph")
                .color(p.k.c.icon_base),
        )
}

/// One lab as a card: tile, name and tags on one line, up to two lines of
/// description, its switch, and while on any settings of its own.
/// `featured` cards sit under New in Labs, so they carry no New tag.
fn card(ui: &Ui, p: Paint, entry: &Row, new: bool, featured: bool, layer: &mut Vec<El>) -> El {
    let c = &p.k.c;
    let lab = entry.lab;
    let mut tags = Vec::new();
    if new && !featured {
        tags.push(tone_tag(p, "new-tag", tr("New"), Tone::Accent));
    }
    if lab.audience() == crate::labs::Audience::Dev {
        tags.push(tone_tag(p, "dev-tag", tr("Dev"), Tone::Neutral));
    }
    let title = row()
        .key("title-row")
        .min_w(0.0)
        .gap(6.0)
        .center_y()
        .child(
            text(entry.title)
                .key("title")
                .font(p.k.ty.ui_medium)
                .color(p.text())
                .truncate()
                .shrink()
                .min_w(0.0),
        )
        .children(tags)
        .child(el().key("spacer").grow(1.0).min_w(8.0))
        .child(lab_switch(p, entry).key(row_key(lab)));
    // The description runs under the switch too.
    let mut copy = col()
        .key("copy")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(2.0)
        .child(title)
        .child(
            text(entry.about)
                .key("description")
                .font(DESCRIPTION)
                .color(p.muted())
                .max_lines(2),
        );
    if let Some(extras) = extras(ui, p, entry, layer) {
        copy = copy.child(extras.mt(8.0));
    }
    let key = if featured {
        format!("lab-featured-{}", lab.id())
    } else {
        format!("lab-card-{}", lab.id())
    };
    row()
        .key(key)
        .role("group")
        .label(entry.title)
        .padding(CARD_PADDING)
        .gap(10.0)
        .items_start()
        .radius(p.k.radius.lg)
        .child(tile(p, entry.icon, TILE))
        .child(copy)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak)
}

const CARD_PADDING: f32 = 12.0;
const TILE: f32 = 32.0;

/// A lab's switch: it flips the lab; a locked lab's is disabled.
fn lab_switch(p: Paint, entry: &Row) -> El {
    let lab = entry.lab;
    Switch::new(entry.on)
        .control_size(ControlSize::Small)
        .label(entry.title)
        .hide_label()
        .disabled(entry.locked)
        .on_toggle(move |ui: &mut Ui| flip(ui, lab))
        .build(p.k)
}

/// What Labs is, Run setup again, and Send feedback.
fn footer(p: Paint) -> El {
    let run = Button::new(tr("Run setup again"))
        .variant(ButtonVariant::Ghost)
        .on_press_tx(|ui: &mut Ui, tx: &mut Tx| {
            if let Err(error) = super::super::onboarding_kit::live::restart(ui, tx) {
                ui.error = Some(error);
            }
        })
        .build_small_push(p.k)
        .key("restart-onboarding");
    row()
        .key("footer")
        .mt(12.0)
        .gap(8.0)
        .center_y()
        .child(
            text(tr("Unfinished features. They may change or break."))
                .key("lede")
                .font(DESCRIPTION)
                .color(p.muted())
                .wrap()
                .grow(1.0)
                .shrink(),
        )
        .child(run)
        .child(feedback_button(p, "send-feedback", ButtonVariant::Neutral))
}

/// Opens Labs' feedback form.
fn feedback_button(p: Paint, key: &str, variant: ButtonVariant) -> El {
    Button::new(tr("Send feedback"))
        .variant(variant)
        .on_press(|ui: &mut Ui| {
            if ui.live()
                && let Err(error) =
                    crate::desktop::opencode_catalog::open_url(crate::labs::FEEDBACK_URL)
            {
                ui.error = Some(format!("{error:#}"));
            }
        })
        .build_small_push(p.k)
        .key(key)
}

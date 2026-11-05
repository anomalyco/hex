//! The Labs page as a settings pane (t1402's directions): rows in Hex's
//! settings cards with the rows' standard switch, and one way in, the
//! section labels. No search, chips, tabs, folds, or featured cards; a
//! new lab carries a New tag in its row instead.

use rune::{El, Tx, row};
use rune_kit::{Button, ButtonVariant, Switch};

use super::super::controls::{
    CardRow, METRICS, Paint, PushSize, page as settings_page, pressable_row, section, section_with,
    tagged_row,
};
use super::super::lists::{Tone, tone_tag};
use super::super::{Pane, Ui};
use super::{Lab, Layout, Row, flip, group, list_extras, row_key, shown, store};
use crate::desktop::i18n::tr;
use crate::labs::{Audience, Feature, Group};

pub(super) fn page(ui: &Ui, p: Paint, layout: Layout) -> El {
    let rows = shown(ui);
    let newest = rows.iter().map(|row| row.lab.added()).max().unwrap_or(0);
    let is_new = |row: &Row| newest > 0 && row.lab.added() == newest;
    let mut content = settings_page("content");
    match layout {
        Layout::Store | Layout::List => {
            for group in Group::ALL {
                let members: Vec<&Row> = rows.iter().filter(|row| row.group == group).collect();
                if members.is_empty() {
                    continue;
                }
                content = content.child(section(
                    p,
                    &format!("group-{}", group.id()),
                    group::name(group),
                    card_rows(ui, p, &members, |row| tags(p, row, is_new(row), None)),
                ));
            }
        }
        Layout::ByPane => {
            for pane in panes() {
                let members: Vec<&Row> = rows.iter().filter(|row| home(row) == pane).collect();
                if members.is_empty() {
                    continue;
                }
                let rows = card_rows(ui, p, &members, |row| tags(p, row, is_new(row), None));
                let key = format!("pane-{}", pane.key());
                content = content.child(if pane.shown(ui) {
                    section_with(p, &key, pane.title(), open_button(p, pane), rows)
                } else {
                    section(p, &key, pane.title(), rows)
                });
            }
        }
        Layout::OnFirst => {
            let on: Vec<&Row> = rows.iter().filter(|row| row.on).collect();
            let mut off: Vec<&Row> = rows.iter().filter(|row| !row.on).collect();
            // The newest lead, where New in Labs used to be.
            off.sort_by_key(|row| !is_new(row));
            let tagged = |row: &Row| {
                let pane = (!is_pane(row.lab)).then(|| home(row));
                tags(p, row, is_new(row), pane)
            };
            if !on.is_empty() {
                content = content.child(section(
                    p,
                    "turned-on",
                    tr("Turned on"),
                    card_rows(ui, p, &on, tagged),
                ));
            }
            if !off.is_empty() {
                content = content.child(section(
                    p,
                    "others",
                    tr("Everything else"),
                    card_rows(ui, p, &off, tagged),
                ));
            }
        }
    }
    content = content.child(store::footer(p).mx(METRICS.row_px));
    super::super::scroll_body(p.k, "scroll", content)
}

/// The panes labs move to when they graduate, in the sidebar's order.
fn panes() -> Vec<Pane> {
    let mut panes = vec![Pane::General, Pane::Output, Pane::History, Pane::Meetings];
    #[cfg(hex_capture)]
    panes.push(Pane::Annotate);
    panes.push(Pane::VoiceFilter);
    panes
}

/// The pane a lab's setting moves to when it graduates.
fn home(row: &Row) -> Pane {
    if row.lab == Lab::Feature(Feature::VoiceFilter) {
        return Pane::VoiceFilter;
    }
    match row.group {
        Group::Dictation | Group::Shortcuts | Group::Models => Pane::General,
        Group::Output => Pane::Output,
        Group::History => Pane::History,
        Group::Meetings => Pane::Meetings,
        #[cfg(hex_capture)]
        Group::Annotate => Pane::Annotate,
        #[cfg(not(hex_capture))]
        Group::Annotate => Pane::General,
    }
}

/// Whether a lab is a pane of its own, so it needs no pane tag.
fn is_pane(lab: Lab) -> bool {
    match lab {
        Lab::Feature(feature) => matches!(feature, Feature::Meetings | Feature::VoiceFilter),
        #[cfg(hex_capture)]
        Lab::Annotate => true,
    }
}

/// Goes to `pane`, on its section's label line.
fn open_button(p: Paint, pane: Pane) -> El {
    Button::new(tr("Open"))
        .variant(ButtonVariant::Ghost)
        .on_press_tx(move |ui: &mut Ui, tx: &mut Tx| ui.select_pane(pane, tx))
        .build_small_push(p.k)
        .key("open")
}

/// A row's tags: New, Dev, and in On first the pane it moves to.
fn tags(p: Paint, row: &Row, new: bool, pane: Option<Pane>) -> Vec<El> {
    let mut tags = Vec::new();
    if let Some(pane) = pane {
        tags.push(tone_tag(p, "pane-tag", pane.title(), Tone::Neutral));
    }
    if new {
        tags.push(tone_tag(p, "new-tag", tr("New"), Tone::Accent));
    }
    if row.lab.audience() == Audience::Dev {
        tags.push(tone_tag(p, "dev-tag", tr("Dev"), Tone::Neutral));
    }
    tags
}

/// Each lab's row, and under one that's on, its own settings.
fn card_rows(ui: &Ui, p: Paint, members: &[&Row], tags: impl Fn(&Row) -> Vec<El>) -> Vec<CardRow> {
    let mut rows = Vec::new();
    for entry in members {
        rows.push(lab_row(p, entry, tags(entry)).into());
        if let Some(extras) = list_extras(ui, p, entry) {
            rows.push(extras.into());
        }
    }
    rows
}

/// One lab: a whole-row switch at the settings rows' size; a locked
/// lab's is disabled and its description says why.
fn lab_row(p: Paint, entry: &Row, tags: Vec<El>) -> El {
    let lab = entry.lab;
    let switch = Switch::new(entry.on)
        .disabled(entry.locked)
        .build(p.k)
        .key("switch")
        .focusable(false)
        .a11y_hidden();
    let tag = (!tags.is_empty()).then(|| row().key("tags").gap(4.0).center_y().children(tags));
    let about = (!entry.about.is_empty()).then_some(entry.about);
    let item = pressable_row(
        tagged_row(p, row_key(lab), None, entry.title, tag, about, switch),
        entry.on,
    );
    if entry.locked {
        item.disabled(true)
    } else {
        item.on_press(move |ui: &mut Ui| flip(ui, lab))
    }
}

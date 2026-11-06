//! Settings › General's search (Labs › Settings search): the field at the
//! top of the pane, the query it holds, the rows each section keeps, and the
//! pane that assembles them.

use rune::{El, Key, KeyEvent, row};
use rune_kit::TextInput;

use super::super::controls::{CardRow, METRICS, Paint, page, section};
use super::super::list_detail::{SEARCH_ICON, empty_state};
use super::super::platform_settings as os;
use super::super::settings as s;
use super::super::{Pane, Ui, scroll_body};
use super::{advanced_open, advanced_toggle, application, basics, dictation, output};
use crate::desktop::i18n::tr;
use crate::labs::Feature;

const SEARCH_WIDTH: f32 = 320.0;

/// The pane, with the search field first while its lab is on. With a query,
/// every matching row shows whatever its fold, empty sections hide, and
/// nothing matching shows the empty state; without one it is the pane as
/// before.
pub(crate) fn pane(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let enabled = Feature::SettingsSearch.on(&ui.settings);
    let query = if enabled {
        ui.general_search.text.trim()
    } else {
        ""
    };
    let searching = !query.is_empty();
    let mut content = page("content");
    if enabled {
        content = content.child(search_row(ui, p));
    }
    if searching {
        content = content.children(os::error_section(ui, p));
    } else {
        content = content
            .children(os::permissions_section(ui, p))
            .children(os::error_section(ui, p));
        if ui.platform.caps.listener_control {
            content = content.child(s::listener_card(ui, p));
        }
        if !ui.records_shortcuts() {
            content = content.child(s::desktop_commands(ui, p));
        }
    }
    let mut matched = false;
    if let Some(basics) = basics(ui, p, layer, query) {
        content = content.child(basics);
        matched = true;
    }
    if !searching {
        content = content.child(advanced_toggle(ui, p, Pane::General));
    }
    if advanced_open(ui, Pane::General) || searching {
        for (key, title, rows) in [
            (
                "more-dictation",
                tr("Dictation"),
                dictation(ui, p, layer, query),
            ),
            ("output", tr("Output"), output(ui, p, layer, query)),
            (
                "application",
                tr("Application"),
                application(ui, p, layer, query),
            ),
        ] {
            if !rows.is_empty() {
                content = content.child(section(p, key, title, rows));
                matched = true;
            }
        }
    }
    if searching && !matched {
        content = content.child(empty_state(
            p,
            "settings-empty",
            SEARCH_ICON,
            tr("No settings match"),
            Some(tr("Try another search")),
        ));
    }
    scroll_body(p.k, "scroll", content)
}

/// The single-line field, cleared by its button and focused by ⌘F.
fn search_row(ui: &Ui, p: Paint) -> El {
    let mut field = TextInput::new(&ui.general_search)
        .placeholder(tr("Search settings"))
        .width(Some(SEARCH_WIDTH))
        .leading_icon(p.k.glyph.search)
        .on_edit(|ui: &mut Ui| &mut ui.general_search);
    if !ui.general_search.text.is_empty() {
        field = field.clear_button();
    }
    row()
        .key("settings-search-row")
        .mx(METRICS.row_px)
        .child(field.build(p.k).key("settings-search"))
}

/// ⌘F (Ctrl+F off macOS) on General puts the caret in its search.
fn focus_search(ui: &mut Ui) {
    ui.general_search.focused = true;
}

/// ⌘F (Ctrl+F off macOS): Labs' search while Labs is open, else General's
/// while its search lab is on.
pub(crate) fn focus_search_key(ui: &mut Ui, event: &KeyEvent) -> bool {
    if event.key != Key::Command('f') || event.mods.shift {
        return false;
    }
    if ui.labs_open {
        super::super::labs::focus_search(ui);
        return true;
    }
    if ui.pane == Pane::General && Feature::SettingsSearch.on(&ui.settings) {
        focus_search(ui);
        return true;
    }
    false
}

/// Rows under construction, kept only while their title, or a description
/// given to [`Rows::push_described`], contains the query.
pub(super) struct Rows<'a> {
    query: &'a str,
    rows: Vec<CardRow>,
}

impl<'a> Rows<'a> {
    /// Every row shows while `query` is empty.
    pub(super) fn new(query: &'a str) -> Self {
        Self {
            query,
            rows: Vec::new(),
        }
    }

    /// Keeps `row` when its title matches.
    pub(super) fn push(&mut self, title: &str, row: impl Into<CardRow>) {
        self.push_described(title, None, row);
    }

    /// Keeps `row` when its title or `description` matches, and skips an
    /// empty fold while searching: it takes no space, so it is no match.
    pub(super) fn push_described(
        &mut self,
        title: &str,
        description: Option<&str>,
        row: impl Into<CardRow>,
    ) {
        let row = row.into();
        if matches(self.query, title, description) && (self.query.is_empty() || !empty_fold(&row)) {
            self.rows.push(row);
        }
    }

    /// Keeps `rows` together when `title` matches, without the empty folds
    /// while searching.
    pub(super) fn extend(&mut self, title: &str, rows: impl IntoIterator<Item = CardRow>) {
        if matches(self.query, title, None) {
            let rows = rows.into_iter();
            if self.query.is_empty() {
                self.rows.extend(rows);
            } else {
                self.rows.extend(rows.filter(|row| !empty_fold(row)));
            }
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub(super) fn into_rows(self) -> Vec<CardRow> {
        self.rows
    }
}

/// A fold with nothing under it draws nothing.
fn empty_fold(row: &CardRow) -> bool {
    matches!(row, CardRow::Folding { rows, .. } if rows.is_empty())
}

/// Whether a case-insensitive `query` appears in `title` or `description`;
/// an empty query matches every row.
pub(super) fn matches(query: &str, title: &str, description: Option<&str>) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || title.to_lowercase().contains(&query)
        || description.is_some_and(|description| description.to_lowercase().contains(&query))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_titles_and_descriptions_ignoring_case_and_spaces() {
        assert!(matches("mic", "Microphone", None));
        assert!(matches("MIC", "Microphone", None));
        assert!(matches("  microphone  ", "Microphone", None));
        assert!(matches("model", "Speech model", Some("Download a model")));
        assert!(matches(
            "download",
            "Speech model",
            Some("Download a model")
        ));
        assert!(!matches("model", "Microphone", None));
        assert!(!matches("model", "Microphone", Some("System input")));
        assert!(matches("", "Microphone", None));
    }
}

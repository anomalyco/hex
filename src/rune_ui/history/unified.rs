//! The gated Mac History window. Row actions stay with their existing
//! owners; this view cannot reinterpret a file as something to paste.
use super::*;
use crate::capture_host::paste_source::Id as Receipt;
use crate::capture_host::paste_source::Scope;
use crate::history::timeline::{self, Owner, PasteId, Row};
use crate::history::{annotations, captures as saved};
use crate::rune_ui::{
    Pane as Destination,
    captures::{self, Listed},
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Filter {
    #[default]
    All,
    Dictations,
    Scrawls,
    Screenshots,
    Recordings,
    Captures,
}
impl Filter {
    fn label(self) -> &'static str {
        match self {
            Self::All => tr("All types"),
            Self::Dictations => tr("Dictations"),
            Self::Scrawls => tr("Scrawls"),
            Self::Screenshots => tr("Screenshots"),
            Self::Recordings => tr("Screen recordings"),
            Self::Captures => tr("Captures"),
        }
    }
    fn accepts(self, kind: Self) -> bool {
        self == Self::All || self == kind || (self == Self::Captures && kind != Self::Dictations)
    }
}
#[derive(Clone, PartialEq, Eq)]
struct Sources {
    library: bool,
    annotate: bool,
    root: PathBuf,
    days: Option<u32>,
}

/// Merge on owner paths, not labels, ids from a different index, or text.
/// A legacy library annotation and a ready Hex receipt for the same bundle
/// produce one row with the completion timestamp.
type FileRows = (Vec<Listed>, BTreeMap<PathBuf, Receipt>);
fn capture_rows(s: &Sources, now: u64) -> FileRows {
    let files = if s.library {
        saved::list("")
    } else {
        Vec::new()
    };
    capture_rows_from(s, now, files)
}

fn capture_rows_from(s: &Sources, now: u64, indexed: Vec<saved::CaptureEntry>) -> FileRows {
    let mut bundles = BTreeMap::new();
    let mut receipts = BTreeMap::new();
    if s.library {
        for a in annotations::in_folder(&s.root, now) {
            bundles.insert(a.bundle.clone(), a);
        }
    }
    if s.annotate {
        for ready in timeline::marked(&Scope::new(&s.root, s.days), usize::MAX, now) {
            let Owner::Paste(PasteId::Capture(id)) = ready.id else {
                continue;
            };
            if let Some(mut a) = bundles
                .remove(id.bundle())
                .or_else(|| annotations::read(id.bundle()))
            {
                a.at_ms = ready.ready_at;
                receipts.insert(a.bundle.clone(), id);
                bundles.insert(a.bundle.clone(), a);
            }
        }
    }
    let mut files = BTreeMap::new();
    if s.library {
        for e in indexed {
            files.entry(e.file.clone()).or_insert(e);
        }
    }
    let mut result = Vec::new();
    for a in bundles.into_values() {
        let row = Listed::Annotated(a);
        files.remove(row.file());
        result.push(row);
    }
    result.extend(files.into_values().map(Listed::Saved));
    (result, receipts)
}

#[cfg(test)]
mod proof;

#[derive(Default)]
pub(in crate::rune_ui) struct Pane {
    pub(in crate::rune_ui) enabled: bool,
    filter: Filter,
    source: Option<Sources>,
    last_read: Option<Instant>,
    revision: u64,
    receipts: BTreeMap<PathBuf, Receipt>,
    reader: Option<Receiver<FileRows>>,
}
impl Pane {
    pub(super) fn live() -> Self {
        Self {
            enabled: crate::paste::recent::enabled(),
            ..Default::default()
        }
    }
}

pub(in crate::rune_ui) fn destination(ui: &mut Ui, pane: Destination) -> Destination {
    if ui.history.unified.enabled && pane == Destination::Captures {
        filter(ui, Filter::Captures);
        Destination::History
    } else {
        pane
    }
}
pub(in crate::rune_ui) fn poll(ui: &mut Ui) -> bool {
    if !ui.history.unified.enabled
        || ui.pane != Destination::History
        || ui.labs_open
        || ui.captures.fixture
        || ui.window.as_ref().is_some_and(|w| !w.is_visible())
    {
        return false;
    }
    let settings = &ui.settings.capture;
    let sources = Sources {
        library: ui.settings.labs.screenshots,
        annotate: settings.enabled,
        root: settings.captures_dir(),
        days: settings.bundle_days,
    };
    let state = &mut ui.history.unified;
    let mut changed = false;
    if state.source.as_ref() != Some(&sources) {
        state.reader = None;
        state.last_read = None;
        ui.captures.entries.clear();
        state.receipts.clear();
        state.source = Some(sources.clone());
        changed = true;
    }
    if let Some(rx) = &state.reader {
        match rx.try_recv() {
            Ok((rows, receipts)) => {
                changed |= rows != ui.captures.entries || receipts != state.receipts;
                state.receipts = receipts;
                ui.captures.entries = rows;
                state.reader = None;
            }
            Err(TryRecvError::Empty) => return changed,
            Err(TryRecvError::Disconnected) => state.reader = None,
        }
    }
    let revision = saved::revision();
    if (sources.library || sources.annotate)
        && (state.revision != revision
            || state
                .last_read
                .is_none_or(|when| when.elapsed() >= Duration::from_secs(5)))
    {
        state.last_read = Some(Instant::now());
        state.revision = revision;
        let (tx, rx) = channel();
        if std::thread::Builder::new()
            .name("history-captures".into())
            .spawn(move || {
                let _ = tx.send(capture_rows(&sources, crate::history::now_ms()));
            })
            .is_ok()
        {
            state.reader = Some(rx);
        }
    }
    changed
}

fn filter(ui: &mut Ui, value: Filter) {
    ui.captures.confirm_trash = None;
    ui.open = None;
    ui.history.unified.filter = value;
    ui.history.selected = None;
    ui.history.selection.clear();
    ui.history.delete_armed = None;
    ui.history.clear_armed = false;
}

pub(crate) fn show_all(ui: &mut Ui) {
    filter(ui, Filter::All);
    ui.history.search = TextEdit::default();
    ui.history.reload();
}
fn types(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> El {
    let current = ui.history.unified.filter;
    let menu = Menu::Pick("history-type");
    let open = ui.open == Some(menu);
    let trigger = Select::new::<&str>("history-type", &[])
        .appearance(SelectAppearance::Inline)
        .current(Some(current.label()))
        .open(open)
        .on_toggle(move |ui: &mut Ui| ui.open = (ui.open != Some(menu)).then_some(menu))
        .trigger(p.k)
        .label(tr("Type"))
        .a11y_value(current.label());
    let mut menu = KitMenu::new("history-types")
        .trigger(trigger)
        .placement(Placement::BottomEnd)
        .open(open)
        .on_close(|ui: &mut Ui| ui.open = None);
    for f in [
        Filter::All,
        Filter::Dictations,
        Filter::Scrawls,
        Filter::Screenshots,
        Filter::Recordings,
    ] {
        if matches!(f, Filter::Screenshots | Filter::Recordings) && !ui.settings.labs.screenshots {
            continue;
        }
        if f == Filter::Scrawls && !(ui.settings.capture.enabled || ui.settings.labs.screenshots) {
            continue;
        }
        menu = menu.item(
            MenuItem::new(f.label())
                .checked(current == f)
                .on_select(move |ui: &mut Ui| filter(ui, f)),
        );
    }
    menu.build(p.k, layer)
}

#[derive(Clone, Copy)]
enum Entry<'a> {
    Words(&'a HistoryEntry),
    File(&'a Listed),
}
impl Entry<'_> {
    fn kind(self) -> Filter {
        match self {
            Self::Words(_) => Filter::Dictations,
            Self::File(Listed::Annotated(_)) => Filter::Scrawls,
            Self::File(Listed::Saved(e)) => match e.kind {
                saved::CaptureKind::Shot => Filter::Screenshots,
                saved::CaptureKind::Recording => Filter::Recordings,
            },
        }
    }
    fn projected(self, ui: &Ui) -> Row<Owner> {
        match self {
            Self::Words(e) => Row::words(
                e,
                !ui.settings.history_retention.is_off()
                    && matches!(
                        e.kind,
                        HistoryKind::Dictation | HistoryKind::Send | HistoryKind::Raw
                    )
                    && !e.final_text.trim().is_empty(),
            ),
            Self::File(Listed::Annotated(a)) => Row::file(
                a.at_ms,
                &a.bundle,
                ui.history.unified.receipts.get(&a.bundle),
            ),
            Self::File(Listed::Saved(e)) => Row::file(e.at_ms, &e.file, None),
        }
    }
    fn matches(self, terms: &[String]) -> bool {
        match self {
            Self::Words(_) => true,
            Self::File(e) => {
                let fields = match e {
                    Listed::Saved(e) => format!("{} {} {}", e.words, e.name(), e.kind.label()),
                    Listed::Annotated(a) => format!(
                        "{} {} {} annotation scrawl",
                        a.words,
                        a.app.as_deref().unwrap_or(""),
                        a.sent_to.as_deref().unwrap_or("")
                    ),
                };
                let fields = fields.to_lowercase();
                terms.iter().all(|s| fields.contains(s))
            }
        }
    }
}
fn rows<'a>(ui: &'a Ui) -> Vec<Entry<'a>> {
    let query: Vec<_> = ui
        .history
        .search
        .text
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    let mut rows = ui
        .history
        .entries
        .iter()
        .map(Entry::Words)
        .chain(ui.captures.entries.iter().map(Entry::File))
        .filter(|r| ui.history.unified.filter.accepts(r.kind()) && r.matches(&query))
        .collect::<Vec<_>>();
    rows.sort_by_cached_key(|e| {
        let r = e.projected(ui);
        std::cmp::Reverse((r.ready_at, r.id))
    });
    rows
}
pub(super) fn view(ui: &Ui, p: Paint, layer: &mut Vec<El>) -> (Vec<El>, El) {
    let mut actions = vec![types(ui, p, layer)];
    actions.extend(header_actions(ui, p, layer));
    let rows = rows(ui);
    let n = rows.len();
    let mut list = col().key("history-list").label(tr("History"));
    for (i, r) in rows.into_iter().enumerate() {
        let el = match r {
            Entry::File(e) => captures::item(ui, p, e),
            Entry::Words(e) => {
                let id = e.id;
                let primary = ui.history.selected == Some(id);
                ld::item_with_press(
                    p,
                    Item {
                        key: format!("history-entry{id}"),
                        title: e.final_text.replace('\n', " "),
                        meta: if e.cancelled {
                            format!("{} · {}", tr(e.kind.label()), tr("Cancelled"))
                        } else {
                            e.application.as_ref().map_or_else(
                                || tr(e.kind.label()).to_owned(),
                                |app| format!("{} · {app}", tr(e.kind.label())),
                            )
                        },
                        meta_ink: None,
                        time: event_age(e.timestamp_ms),
                        hover: vec![copy_action(ui, p, id, true)],
                        selected: ui.history.is_selected(id),
                    },
                    move |el| selection::gesture(el, id, primary),
                )
            }
        };
        list = list.child(el.position_in_set(i + 1, n));
    }
    if n == 0 {
        list = list.child(ld::empty_state(
            p,
            "history-empty",
            ld::HISTORY_ICON,
            tr("No matching items"),
            None,
        ));
    }
    let search = TextInput::new(&ui.history.search)
        .placeholder(tr("Search history"))
        .width(None)
        .leading_icon(p.k.glyph.search)
        .on_edit(|ui: &mut Ui| &mut ui.history.search)
        .build(p.k)
        .key("history-search");
    let column = ld::list_column(p, search, "history-scroll", list);
    let body = if ui.history.selected.is_some() {
        ld::body(p, "history-body", column, detail(ui, p, layer))
    } else {
        column.w_full().pr(ld::HANG)
    };
    if let Some(modal) = stats_modal(ui, p) {
        layer.extend(modal);
    }
    let mut pane = col().key("history-pane").grow(1.).shrink();
    if let Some(error) = &ui.history.error {
        pane = pane.child(error_panel(
            p,
            "history-error",
            tr("History could not be loaded."),
            error,
            Tone::Danger,
        ));
    }
    (actions, pane.child(body))
}

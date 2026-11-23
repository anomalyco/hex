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
use rune::el;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// A capture's picture in the detail: as wide as the card, and this tall at
/// most.
const PICTURE_W: f32 = 720.0;
const PICTURE_H: f32 = 300.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::rune_ui) enum Filter {
    #[default]
    All,
    Dictations,
    Annotations,
    Screenshots,
    Recordings,
    Captures,
}
impl Filter {
    fn label(self) -> &'static str {
        match self {
            Self::All => tr("All types"),
            Self::Dictations => tr("Dictations"),
            Self::Annotations => tr("Annotations"),
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
    /// The capture or annotation row whose detail shows (`Listed::key`).
    /// Choosing a dictation clears it; it is never a file to paste.
    pub(in crate::rune_ui) picked: Option<PathBuf>,
    source: Option<Sources>,
    last_read: Option<Instant>,
    revision: u64,
    receipts: BTreeMap<PathBuf, Receipt>,
    reader: Option<Receiver<FileRows>>,
}
impl Pane {
    /// The type filter the pane draws, for scenes and tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(in crate::rune_ui) fn set_filter(&mut self, value: Filter) {
        self.filter = value;
    }

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
    ui.history.unified.picked = None;
    ui.history.selected = None;
    ui.history.selection.clear();
    ui.history.delete_armed = None;
    ui.history.clear_armed = false;
}

/// Opens the whole History from the picker's Show all row.
pub(crate) fn show_all_history(ui: &mut Ui, tx: &mut Tx) {
    show_all(ui);
    super::super::show(ui, super::super::Destination::History, tx);
}

pub(crate) fn show_all(ui: &mut Ui) {
    filter(ui, Filter::All);
    ui.history.search = TextEdit::default();
    ui.history.reload(ui.settings.labs.history_stats);
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
        Filter::Annotations,
        Filter::Screenshots,
        Filter::Recordings,
    ] {
        if matches!(f, Filter::Screenshots | Filter::Recordings) && !ui.settings.labs.screenshots {
            continue;
        }
        if f == Filter::Annotations
            && !(ui.settings.capture.enabled || ui.settings.labs.screenshots)
        {
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
            Self::File(Listed::Annotated(_)) => Filter::Annotations,
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
                        "{} {} {} annotation",
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
/// A capture row's identity in the list: the saved file, or an
/// annotation's bundle.
fn listed_key(entry: &Listed) -> PathBuf {
    match entry {
        Listed::Saved(e) => e.file.clone(),
        Listed::Annotated(a) => a.bundle.clone(),
    }
}

/// Chooses a capture row: its detail shows beside the list, and no
/// dictation stays selected.
fn pick(ui: &mut Ui, key: PathBuf) {
    ui.captures.confirm_trash = None;
    ui.history.selected = None;
    ui.history.selection.clear();
    ui.history.delete_armed = None;
    ui.history.unified.picked = Some(key);
}

/// The chosen capture, while it is one of the rows the filter and search
/// leave showing.
fn picked<'a>(ui: &'a Ui, rows: &[Entry<'a>]) -> Option<&'a Listed> {
    let key = ui.history.unified.picked.as_ref()?;
    rows.iter().find_map(|row| match row {
        Entry::File(entry) if &listed_key(entry) == key => Some(*entry),
        _ => None,
    })
}

/// The whole pane's empty state: nothing retained and nothing captured.
fn empty(ui: &Ui, p: Paint) -> El {
    let retention = ui.settings.history_retention;
    if retention.is_off() {
        ld::empty_state(
            p,
            "history-empty",
            ld::HISTORY_ICON,
            tr("History is off."),
            Some(tr("Choose how long to keep dictations to see them here.")),
        )
    } else {
        ld::empty_state(
            p,
            "history-empty",
            ld::HISTORY_ICON,
            tr("Nothing here yet"),
            Some(tr("Dictations, annotations, and captures show here.")),
        )
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
/// A capture row's card: its kind, its picture, its words, and how it was
/// made, with the actions its file has (Open in Editor, Copy, Show in
/// Finder, Move to Trash behind a confirmation).
fn file_detail(ui: &Ui, p: Paint, entry: &Listed) -> El {
    use crate::rune_ui::list_detail as ld;
    let id = entry.id();
    let at = entry.at_ms();
    let armed = ui
        .captures
        .confirm_trash
        .as_ref()
        .is_some_and(|(armed, _)| *armed == id);
    let mut actions = Vec::new();
    if !armed {
        if matches!(entry, Listed::Annotated(_)) {
            actions.push(ld::ghost_icon(
                p,
                "file-open",
                ld::EDIT_ICON,
                tr("Open in Editor"),
                move |ui: &mut Ui, _| captures::open(ui, id),
            ));
        }
        actions.push(captures::copy_action(ui, p, id, false));
        actions.push(ld::ghost_icon(
            p,
            "file-reveal",
            captures::FINDER_ICON,
            tr("Show in Finder"),
            move |ui: &mut Ui, _| captures::reveal(ui, id),
        ));
        actions.push(ld::ghost_icon(
            p,
            "file-trash",
            ld::TRASH_ICON,
            tr("Move to Trash…"),
            move |ui: &mut Ui, _| captures::trash(ui, id),
        ));
    }
    let confirm = armed.then(|| {
        row()
            .key("trash-confirm")
            .gap(8.0)
            .pt(14.0)
            .child(button(
                p,
                "file-confirm-trash",
                tr("Move to Trash"),
                ButtonKind::Danger,
                move |ui: &mut Ui, _| captures::trash(ui, id),
            ))
            .child(button(
                p,
                "file-cancel-trash",
                tr("Cancel"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| ui.captures.confirm_trash = None,
            ))
    });
    let mut body = col().key("detail-body").gap(20.0);
    if let Some(pixels) = ui.captures.picture(entry, PICTURE_W) {
        let (width, height) = pixels.size();
        body = body.child(
            el().key("picture")
                .aspect_fit(width as f32 / height.max(1) as f32)
                .max_h(PICTURE_H)
                .radius(p.k.radius.sm)
                .fill(p.k.c.layer_02)
                .clip_soft(0.0)
                .pixels_fit(&pixels, rune::SurfaceFit::Fill)
                .child(
                    el().absolute(0.0, 0.0)
                        .w_full()
                        .h_full()
                        .radius(p.k.radius.sm)
                        .border(p.hair, p.k.c.border_weak),
                ),
        );
    }
    let title = entry.title();
    if !title.trim().is_empty() && title != entry_kind_label(entry) {
        body = body.child(
            text(title)
                .key("words")
                .font(p.k.ty.body)
                .color(p.text())
                .wrap(),
        );
    }
    let mut facts = Vec::new();
    if let Listed::Annotated(annotation) = entry {
        if let Some(app) = &annotation.app {
            facts.push((tr("Application"), app.clone()));
        }
        if let Some(sent) = &annotation.sent_to {
            facts.push((tr("Sent to"), sent.clone()));
        }
    }
    body = body.child(ld::facts(p, facts));
    col()
        .key("file-detail")
        .grow(1.0)
        .shrink()
        .child(ld::card_header(
            ld::card_title(p, entry_kind_label(entry)),
            vec![ld::meta_text(p, &event_age(at))],
            actions,
        ))
        .children(confirm)
        .child(ld::card_scroll(p, "detail-scroll", body))
}

/// The kind a capture row's card titles itself with, as the list's meta
/// leads with it.
fn entry_kind_label(entry: &Listed) -> &'static str {
    match entry {
        Listed::Annotated(_) => tr("Annotation"),
        Listed::Saved(e) => tr(e.kind.label()),
    }
}

pub(super) fn view(ui: &Ui, p: Paint, cx: &mut Cx<Ui>, layer: &mut Vec<El>) -> (Vec<El>, El) {
    // With no captures to mix in, the filter would list one kind.
    let mut actions = Vec::new();
    if ui.settings.capture.enabled || ui.settings.labs.screenshots {
        actions.push(types(ui, p, layer));
    }
    actions.extend(header_actions(ui, p, layer));
    let all_rows = rows(ui);
    let n = all_rows.len();
    let searching = !ui.history.search.text.trim().is_empty();
    let mut list = col().key("history-list").label(tr("History"));
    for (i, r) in all_rows.iter().copied().enumerate() {
        // One row shape for every kind: the title over its kind and
        // application (or length, or size), the age on the right, and Copy
        // where the age was while the pointer is over it.
        let el = match r {
            Entry::File(e) => {
                let key = std::rc::Rc::new(listed_key(e));
                let picked = ui.history.unified.picked.as_ref() == Some(&*key);
                let id = e.id();
                ld::item_with_press(
                    p,
                    Item {
                        key: format!("history-file{id}"),
                        title: e.title(),
                        meta: captures::meta(e),
                        meta_ink: None,
                        time: event_age(e.at_ms()),
                        hover: vec![captures::copy_action(ui, p, id, true)],
                        selected: picked,
                    },
                    move |el| {
                        super::super::lists::select_at_once(el, picked, {
                            let key = key.clone();
                            move |ui: &mut Ui| pick(ui, (*key).clone())
                        })
                    },
                )
            }
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
                        // A cancelled dictation is History-only; its kind
                        // line says so, faint, under the words it kept.
                        meta_ink: e.cancelled.then(|| p.faint()),
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
    let picked = picked(ui, &all_rows);
    // A search or the type filter can leave the chosen dictation out of the
    // list; its card goes with it, as History's own search does.
    let chosen = ui.history.selected.is_some_and(|id| {
        all_rows
            .iter()
            .any(|row| matches!(row, Entry::Words(e) if e.id == id))
    });
    let detail = match picked {
        Some(entry) => file_detail(ui, p, entry),
        None if chosen => detail(ui, p, layer),
        None => ld::empty_state(
            p,
            "history-detail",
            ld::HISTORY_ICON,
            tr("Select a history entry."),
            None,
        ),
    };
    // One empty state for a History with nothing at all; every other state
    // keeps the list's width and the card beside it, as History always does.
    let body = if n == 0 && !searching {
        empty(ui, p)
    } else {
        ld::body(p, "history-body", column, detail)
    };
    if let Some(modal) = super::stats::modal(ui, p, cx) {
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
    (
        actions,
        pane.children(super::stats::strip(ui, p)).child(body),
    )
}

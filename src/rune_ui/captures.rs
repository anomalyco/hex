//! The Captures pane (Labs › Screenshots): the shots and recordings the Lab
//! saved in the last 30 days, newest first, each with its thumbnail, kind
//! and length, time, and the words said over it, which the search reads.
//! A click opens one in the editor and a drag carries its file out; Copy and
//! Move to Trash wait on the row's hover. It reads the capture history
//! (`history::captures`) only while it shows, and again only once that
//! changes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use rune::{Align, Cursor, El, Icon, TextEdit, Tx, col, el, row, text};
use rune_kit::TextInput;

use super::Ui;
use super::controls::{METRICS, Paint};
use super::list_detail as ld;
use crate::desktop::i18n::{event_age, tr};
use crate::history::captures::{self, CaptureEntry, CaptureKind};

pub(super) static CAPTURES_ICON: Icon = Icon {
    name: "hex-captures",
    view: 24.0,
    body: include_str!("../../assets/icons/captures.svg"),
};
static FINDER_ICON: &Icon = &rune_theme_opencode::icons::FOLDER;

/// A row's thumbnail, the shape of a display.
const THUMB: (f32, f32) = (72.0, 45.0);
/// How far the list fades as rows slide under the search field.
const FADE: (f32, f32) = (18.0, 28.0);
/// The most thumbnails kept decoded.
const MAX_THUMBS: usize = 240;

#[derive(Default)]
pub(super) struct CapturesPane {
    pub(super) entries: Vec<CaptureEntry>,
    search: TextEdit,
    /// The query the entries were read for.
    searched: String,
    /// The capture history's revision they were read at; `None` before the
    /// first read.
    revision: Option<u64>,
    /// Decodes the thumbnails off the window's thread; the window sets it.
    cache: Option<rune::ImageCache>,
    thumbs: RefCell<HashMap<u64, rune::Image>>,
    /// The entry whose Copy was pressed last.
    copied: Option<u64>,
    /// A fixture: its entries and thumbnails are set, never read.
    fixture: bool,
}

impl CapturesPane {
    pub(super) fn set_cache(&mut self, cache: rune::ImageCache) {
        self.cache = Some(cache);
    }

    /// While the pane shows, reads the list again when the search or the
    /// capture history changed.
    pub(super) fn poll(&mut self, visible: bool) -> bool {
        if !visible || self.fixture {
            return false;
        }
        let revision = captures::revision();
        if self.search.text == self.searched && self.revision == Some(revision) {
            return false;
        }
        let previous = std::mem::take(&mut self.entries);
        self.searched = self.search.text.clone();
        self.revision = Some(revision);
        self.entries = captures::list(&self.searched);
        self.entries != previous
    }

    fn entry(&self, id: u64) -> Option<&CaptureEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// Entry `id`'s thumbnail, asked for the first time it shows; none
    /// while it decodes or when it has none.
    fn thumbnail(&self, id: u64) -> Option<rune::Pixels> {
        let mut thumbs = self.thumbs.borrow_mut();
        if !thumbs.contains_key(&id) {
            let cache = self.cache.as_ref()?;
            let path = captures::thumbnail(id)?;
            if thumbs.len() >= MAX_THUMBS {
                thumbs.clear();
            }
            thumbs.insert(id, cache.file(path, THUMB.0, 2.0));
        }
        let image = &thumbs[&id];
        (image.is_ready() && image.error().is_none()).then(|| image.pixels().clone())
    }

    /// Three captures, one a recording with words said over it, with drawn
    /// thumbnails, for scenes.
    pub(super) fn fixture() -> Self {
        let now = crate::history::now_ms();
        let entry = |id: u64, minutes: u64, kind, name: &str, words: &str| CaptureEntry {
            id,
            at_ms: now - minutes * 60_000 - 30_000,
            kind,
            file: PathBuf::from(format!("/Users/alex/Desktop/{name}")),
            size: [2880, 1800],
            duration_ms: if kind == CaptureKind::Recording {
                42_000
            } else {
                0
            },
            words: words.into(),
            timed: Vec::new(),
        };
        let pane = Self {
            entries: vec![
                entry(
                    3,
                    2,
                    CaptureKind::Shot,
                    "Hex Screenshot 2026-11-08 at 14.03.22.png",
                    "",
                ),
                entry(
                    2,
                    48,
                    CaptureKind::Recording,
                    "Hex Recording 2026-11-08 at 13.17.05.mp4",
                    "Here the deploy button stays grey after the build finishes, even though the logs say it passed.",
                ),
                entry(
                    1,
                    26 * 60,
                    CaptureKind::Shot,
                    "Hex Screenshot 2026-11-07 at 11.40.51.png",
                    "The settings sheet clips its last row on a small display.",
                ),
            ],
            fixture: true,
            ..Self::default()
        };
        let cache = rune::ImageCache::new(std::sync::Arc::new(|| {}));
        for (id, hue) in [(3, [40, 90, 180]), (2, [120, 80, 190]), (1, [40, 150, 110])] {
            let image = cache.rgba(160, 100, fixture_thumbnail(160, 100, hue));
            let _ = image.wait();
            while image.pixels().size() == (0, 0) {
                std::thread::yield_now();
            }
            pane.thumbs.borrow_mut().insert(id, image);
        }
        Self {
            cache: Some(cache),
            ..pane
        }
    }

    /// No captures yet, for scenes.
    pub(super) fn fixture_empty() -> Self {
        Self {
            fixture: true,
            ..Self::default()
        }
    }
}

/// A desktop-ish thumbnail: a light window on a backdrop of `rgb`.
fn fixture_thumbnail(w: u32, h: u32, [r, g, b]: [u8; 3]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let window = x > w / 6 && x < w * 5 / 6 && y > h / 5 && y < h * 4 / 5;
            let bar = window && y < h / 5 + h / 14;
            rgba.extend(match (window, bar) {
                (true, true) => [222, 222, 226, 255],
                (true, false) => [250, 250, 250, 255],
                _ => [r, g.saturating_add((40 * y / h) as u8), b, 255],
            });
        }
    }
    rgba
}

/// What a row says under its title: the kind, and a recording's length or
/// a shot's size.
fn meta(entry: &CaptureEntry) -> String {
    let detail = match entry.kind {
        CaptureKind::Recording => {
            super::screenshot_thumbnail::editor::clip::duration_label(entry.duration_ms)
        }
        CaptureKind::Shot => format!("{} × {}", entry.size[0], entry.size[1]),
    };
    format!("{} · {detail}", tr(entry.kind.label()))
}

/// Opens entry `id` in the editor: a shot with the words said over it, a
/// recording to trim.
fn open(ui: &mut Ui, id: u64) {
    let Some(entry) = ui.captures.entry(id) else {
        return;
    };
    #[cfg(target_os = "macos")]
    {
        use super::screenshot_thumbnail::editor;
        match entry.kind {
            CaptureKind::Shot => editor::open_with(entry.file.clone(), entry.words.clone()),
            CaptureKind::Recording => editor::open_recording(
                entry.file.clone(),
                entry
                    .timed
                    .iter()
                    .map(|word| editor::clip::Word {
                        t0: word.t0,
                        t1: word.t1,
                        text: word.text.clone(),
                    })
                    .collect(),
            ),
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = entry;
}

fn copy(ui: &mut Ui, id: u64) {
    let Some(entry) = ui.captures.entry(id) else {
        return;
    };
    let (file, recording) = (entry.file.clone(), entry.kind == CaptureKind::Recording);
    ui.captures.copied = Some(id);
    #[cfg(target_os = "macos")]
    {
        let spawned = std::thread::Builder::new()
            .name("hex-capture-copy".into())
            .spawn(move || {
                if let Err(error) = crate::screenshots::save::copy_saved(&file, recording) {
                    tracing::warn!(%error, "could not copy the capture");
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not copy the capture");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (file, recording);
}

fn reveal(ui: &mut Ui, id: u64) {
    if let Some(entry) = ui.captures.entry(id)
        && let Err(error) = std::process::Command::new("open")
            .arg("-R")
            .arg(&entry.file)
            .spawn()
    {
        tracing::warn!(%error, "could not show the capture in Finder");
    }
}

/// Moves entry `id`'s file to the Trash; it leaves the list at once.
fn trash(ui: &mut Ui, id: u64) {
    let Some(at) = ui.captures.entries.iter().position(|entry| entry.id == id) else {
        return;
    };
    let entry = ui.captures.entries.remove(at);
    super::screenshot_thumbnail::files::trash(entry.file);
}

/// The search over the list, or the empty state.
pub(super) fn pane(ui: &Ui, p: Paint) -> El {
    let pane = &ui.captures;
    let searching = !pane.searched.trim().is_empty();
    if pane.entries.is_empty() && !searching {
        return ld::empty_state(
            p,
            "captures-empty",
            &CAPTURES_ICON,
            tr("No captures yet."),
            Some(tr("Shots and recordings from the last 30 days show here.")),
        );
    }
    let mut list = col().key("captures-list").label(tr("Captures"));
    let count = pane.entries.len();
    for (index, entry) in pane.entries.iter().enumerate() {
        list = list.child(item(ui, p, entry).position_in_set(index + 1, count));
    }
    if pane.entries.is_empty() {
        list = list.child(ld::empty_state(
            p,
            "captures-empty",
            ld::SEARCH_ICON,
            tr("No captures match."),
            None,
        ));
    }
    let search = TextInput::new(&pane.search)
        .placeholder(tr("Search captures"))
        .width(None)
        .leading_icon(p.k.glyph.search)
        .on_edit(|ui: &mut Ui| &mut ui.captures.search)
        .build(p.k)
        .key("captures-search");
    col()
        .key("captures-pane")
        .grow(1.0)
        .shrink()
        .px(ld::HANG)
        .child(search)
        .child(super::faded_body(
            p.k,
            "captures-scroll",
            super::lists::source_list(list),
            FADE,
        ))
}

/// One capture: its thumbnail, its words (or file name) over its kind and
/// length, and its time, which Copy, Show in Finder, and Move to Trash take
/// the place of on hover.
fn item(ui: &Ui, p: Paint, entry: &CaptureEntry) -> El {
    let c = &p.k.c;
    let id = entry.id;
    let words = entry.words.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = if words.is_empty() {
        entry.name()
    } else {
        words
    };
    let meta = meta(entry);
    let time = event_age(entry.at_ms);
    let mut thumb = el()
        .key("thumb")
        .size(THUMB.0, THUMB.1)
        .radius(p.k.radius.sm)
        .fill(c.layer_01)
        .clip_soft(0.0)
        .a11y_hidden();
    if let Some(pixels) = ui.captures.thumbnail(id) {
        thumb = thumb.pixels_fit(&pixels, rune::SurfaceFit::Fill);
    }
    let thumb = thumb.child(
        el().absolute(0.0, 0.0)
            .size(THUMB.0, THUMB.1)
            .radius(p.k.radius.sm)
            .border(p.hair, c.border_weak),
    );
    let copied = ui.captures.copied == Some(id);
    let actions = vec![
        ld::row_action(
            p,
            "row-copy",
            if copied {
                p.k.glyph.check_small
            } else {
                ld::COPY_ICON
            },
            tr("Copy"),
            move |ui: &mut Ui, _| copy(ui, id),
        ),
        ld::row_action(
            p,
            "row-reveal",
            FINDER_ICON,
            tr("Show in Finder"),
            move |ui: &mut Ui, _| reveal(ui, id),
        ),
        ld::row_action(
            p,
            "row-trash",
            ld::TRASH_ICON,
            tr("Move to Trash"),
            move |ui: &mut Ui, _| trash(ui, id),
        ),
    ];
    let end = el()
        .key("end")
        .child(
            text(time.clone())
                .key("time")
                .font(p.k.ty.small)
                .color(p.faint())
                .align(Align::End)
                .a11y_hidden()
                .group_opacity(1.0, 0.0),
        )
        .child(
            row()
                .key("hover")
                .gap(2.0)
                .center_y()
                .pin(None, Some(-5.0), Some(-6.0), None)
                .group_opacity(0.0, 1.0)
                .children(actions),
        );
    let row_el = row()
        .key(format!("capture{id}"))
        .role("listitem")
        .label(title.clone())
        .description(format!("{meta} · {time}"))
        .px(METRICS.row_px)
        .py(8.0)
        .gap(12.0)
        .center_y()
        .radius(p.k.radius.md)
        .hover(c.hover)
        .hover_group()
        .cursor(Cursor::Pointer)
        .child(thumb)
        .child(
            col()
                .key("words")
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .gap(2.0)
                .child(
                    row()
                        .key("top")
                        .gap(12.0)
                        .center_y()
                        .child(
                            text(title)
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
                    text(meta)
                        .key("meta")
                        .font(p.k.ty.small)
                        .color(p.muted())
                        .truncate()
                        .a11y_hidden(),
                ),
        )
        .on_press_tx(move |ui: &mut Ui, _: &mut Tx| open(ui, id))
        .drag_out(move |ui: &Ui| {
            ui.captures
                .entry(id)
                .map(|entry| rune::DragItem::new().file(entry.file.clone()))
        });
    super::lists::source_item(row_el, false)
}

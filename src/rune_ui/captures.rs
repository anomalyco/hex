//! The Captures pane (Labs › Screenshots): every capture from the last 30
//! days, newest first: the shots and recordings the Lab saved, and
//! Annotate's annotations and replays, each with its thumbnail, kind and
//! length, time, and the words said over it, which the search reads. A
//! click opens one in the editor and a drag carries its file out; Copy and
//! Move to Trash wait on the row's hover. It reads the capture history
//! (`history::captures`) and the captures folder (`history::annotations`)
//! only while it shows, and again only once either changes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rune::{Align, Cursor, El, Icon, TextEdit, Tx, col, el, row, text};
use rune_kit::TextInput;

use super::Ui;
use super::controls::{METRICS, Paint};
use super::list_detail as ld;
use crate::desktop::i18n::{event_age, tr};
use crate::history::annotations::{self, Annotation};
#[cfg(any(target_os = "macos", test))]
use crate::history::captures::CaptureWord;
use crate::history::captures::{self, CaptureEntry, CaptureKind};

pub(super) static CAPTURES_ICON: Icon = Icon {
    name: "hex-captures",
    view: 24.0,
    body: include_str!("../../assets/icons/captures.svg"),
};
pub(super) static FINDER_ICON: &Icon = &rune_theme_opencode::icons::FOLDER;

/// A row's thumbnail, the shape of a display.
const THUMB: (f32, f32) = (72.0, 45.0);
/// How far the list fades as rows slide under the search field.
const FADE: (f32, f32) = (18.0, 28.0);
/// The most thumbnails kept decoded.
const MAX_THUMBS: usize = 240;

/// The row whose copy just reached the clipboard, set by the copying thread.
static COPIED: Mutex<Option<u64>> = Mutex::new(None);

/// One row: a shot or recording the Lab saved, or an Annotate capture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Listed {
    Saved(CaptureEntry),
    Annotated(Annotation),
}

impl Listed {
    pub(super) fn id(&self) -> u64 {
        match self {
            Self::Saved(entry) => entry.id,
            Self::Annotated(annotation) => annotation.id,
        }
    }

    pub(super) fn at_ms(&self) -> u64 {
        match self {
            Self::Saved(entry) => entry.at_ms,
            Self::Annotated(annotation) => annotation.at_ms,
        }
    }

    /// Its words, else a name.
    pub(super) fn title(&self) -> String {
        let words = match self {
            Self::Saved(entry) => &entry.words,
            Self::Annotated(annotation) => &annotation.words,
        };
        let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
        if !words.is_empty() {
            return words;
        }
        match self {
            Self::Saved(entry) => entry.name(),
            Self::Annotated(annotation) => annotation.app.clone().unwrap_or_else(|| {
                annotation
                    .bundle
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default()
            }),
        }
    }

    /// What a drag carries out and Show in Finder points at: the saved
    /// file, or an annotation's movie, else its picture, else its bundle.
    pub(super) fn file(&self) -> &Path {
        match self {
            Self::Saved(entry) => &entry.file,
            Self::Annotated(annotation) => annotation
                .movie
                .as_deref()
                .or(annotation.picture.as_deref())
                .unwrap_or(&annotation.bundle),
        }
    }
}

#[derive(Default)]
pub(super) struct CapturesPane {
    pub(super) view: super::scrawl::View,
    pub(super) confirm_trash: Option<(u64, PathBuf)>,
    pub(super) entries: Vec<Listed>,
    search: TextEdit,
    /// The query the entries were read for.
    searched: String,
    /// The capture history's and the captures folder's revisions they were
    /// read at; `None` before the first read.
    revision: Option<(u64, u64)>,
    /// Decodes the thumbnails off the window's thread; the window sets it.
    cache: Option<rune::ImageCache>,
    thumbs: RefCell<HashMap<u64, rune::Image>>,
    /// Larger pictures for the unified History's detail, decoded from the
    /// same files; a fixture fills them directly.
    #[cfg(all(target_os = "macos", hex_capture))]
    pictures: RefCell<HashMap<u64, rune::Image>>,
    /// The entry whose Copy was pressed last.
    copied: Option<u64>,
    /// A fixture: its entries and thumbnails are set, never read.
    pub(super) fixture: bool,
}

impl CapturesPane {
    pub(super) fn set_cache(&mut self, cache: rune::ImageCache) {
        self.cache = Some(cache);
    }

    /// Entry `entry`'s picture at `width` points, for the unified History's
    /// detail; the list's own 72-point thumbs stay in [`Self::thumbnail`].
    #[cfg(all(target_os = "macos", hex_capture))]
    pub(super) fn picture(&self, entry: &Listed, width: f32) -> Option<rune::Pixels> {
        if let Some(image) = self.pictures.borrow().get(&entry.id())
            && image.is_ready()
            && image.error().is_none()
        {
            return Some(image.pixels().clone());
        }
        let cache = self.cache.as_ref()?;
        let path = match entry {
            Listed::Saved(_) => captures::thumbnail(entry.id())?,
            Listed::Annotated(annotation) => annotation.picture.clone()?,
        };
        let image: rune::Image = cache.file(path, width, 2.0);
        (image.is_ready() && image.error().is_none()).then(|| image.pixels().clone())
    }

    /// While the pane shows, reads the list again when the search, the
    /// capture history, or the captures folder changed, and shows a copy
    /// that reached the clipboard.
    pub(super) fn poll(&mut self, visible: bool) -> bool {
        let copied = COPIED.lock().unwrap_or_else(|e| e.into_inner()).take();
        let changed = copied.is_some();
        if copied.is_some() {
            self.copied = copied;
        }
        if !visible || self.fixture {
            return changed;
        }
        annotations::refresh();
        let revision = (captures::revision(), annotations::revision());
        if self.search.text == self.searched && self.revision == Some(revision) {
            return changed;
        }
        let previous = std::mem::take(&mut self.entries);
        self.searched = self.search.text.clone();
        self.revision = Some(revision);
        self.entries = merged(
            captures::list(&self.searched),
            annotations::list(&self.searched),
        );
        self.entries != previous || changed
    }

    fn entry(&self, id: u64) -> Option<&Listed> {
        self.entries.iter().find(|entry| entry.id() == id)
    }

    /// Entry `id`'s thumbnail, asked for the first time it shows; none
    /// while it decodes or when it has none.
    fn thumbnail(&self, entry: &Listed) -> Option<rune::Pixels> {
        let id = entry.id();
        let mut thumbs = self.thumbs.borrow_mut();
        if !thumbs.contains_key(&id) {
            let cache = self.cache.as_ref()?;
            let path = match entry {
                Listed::Saved(_) => captures::thumbnail(id)?,
                Listed::Annotated(annotation) => annotation.picture.clone()?,
            };
            if thumbs.len() >= MAX_THUMBS {
                thumbs.clear();
            }
            thumbs.insert(id, cache.file(path, THUMB.0, 2.0));
        }
        let image = &thumbs[&id];
        (image.is_ready() && image.error().is_none()).then(|| image.pixels().clone())
    }

    /// Four captures, one a recording and one an annotation with words said
    /// over them, with drawn thumbnails, for scenes.
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
        let bundle =
            PathBuf::from("/Users/alex/Library/Application Support/Scrawl/captures/1.scrawl");
        let annotation = Annotation {
            id: 4,
            manifest_id: "1".into(),
            movie: Some(bundle.join("recording.mp4")),
            bundle,
            at_ms: now - 20 * 60_000,
            duration_ms: 18_000,
            app: Some("Brave Browser".into()),
            words: "The send button overlaps the counter once the list scrolls.".into(),
            timed: Vec::new(),
            sent_to: Some("Hex".into()),
            expired: false,
            picture: None,
        };
        let pane = Self {
            entries: vec![
                Listed::Saved(entry(
                    3,
                    2,
                    CaptureKind::Shot,
                    "Hex Screenshot 2026-11-08 at 14.03.22.png",
                    "",
                )),
                Listed::Annotated(annotation),
                Listed::Saved(entry(
                    2,
                    48,
                    CaptureKind::Recording,
                    "Hex Recording 2026-11-08 at 13.17.05.mp4",
                    "Here the deploy button stays grey after the build finishes, even though the logs say it passed.",
                )),
                Listed::Saved(entry(
                    1,
                    26 * 60,
                    CaptureKind::Shot,
                    "Hex Screenshot 2026-11-07 at 11.40.51.png",
                    "The settings sheet clips its last row on a small display.",
                )),
            ],
            fixture: true,
            ..Self::default()
        };
        let cache = rune::ImageCache::new(std::sync::Arc::new(|| {}));
        for (id, hue) in [
            (3, [40, 90, 180]),
            (4, [190, 110, 50]),
            (2, [120, 80, 190]),
            (1, [40, 150, 110]),
        ] {
            let image = cache.rgba(160, 100, fixture_thumbnail(160, 100, hue));
            let _ = image.wait();
            while image.pixels().size() == (0, 0) {
                std::thread::yield_now();
            }
            pane.thumbs.borrow_mut().insert(id, image);
            #[cfg(all(target_os = "macos", hex_capture))]
            pane.pictures
                .borrow_mut()
                .insert(id, cache.rgba(720, 450, fixture_thumbnail(720, 450, hue)));
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

/// The Lab's captures and Annotate's in one list, newest first.
fn merged(saved: Vec<CaptureEntry>, annotated: Vec<Annotation>) -> Vec<Listed> {
    let mut listed: Vec<Listed> = saved
        .into_iter()
        .map(Listed::Saved)
        .chain(annotated.into_iter().map(Listed::Annotated))
        .collect();
    listed.sort_by_key(|entry| std::cmp::Reverse(entry.at_ms()));
    listed
}

/// What a row says under its title: the kind, and a recording's length or
/// a shot's size, or that an annotation's movie has expired.
pub(super) fn meta(entry: &Listed) -> String {
    let duration = super::screenshot_thumbnail::editor::clip::duration_label;
    match entry {
        Listed::Saved(entry) => {
            let detail = match entry.kind {
                CaptureKind::Recording => duration(entry.duration_ms),
                CaptureKind::Shot => format!("{} × {}", entry.size[0], entry.size[1]),
            };
            format!("{} · {detail}", tr(entry.kind.label()))
        }
        Listed::Annotated(annotation) if annotation.expired => {
            format!("{} · {}", tr("Annotation"), tr("Video expired"))
        }
        Listed::Annotated(annotation) if annotation.duration_ms > 0 => {
            format!(
                "{} · {}",
                tr("Annotation"),
                duration(annotation.duration_ms)
            )
        }
        Listed::Annotated(_) => tr("Annotation").to_owned(),
    }
}

/// Opens entry `id` in the editor: a shot with the words said over it, a
/// recording or an annotation's movie to trim, or the picture of an
/// annotation whose movie expired.
pub(super) fn open(ui: &mut Ui, id: u64) {
    let Some(entry) = ui.captures.entry(id) else {
        return;
    };
    #[cfg(target_os = "macos")]
    match entry {
        Listed::Saved(entry) => {
            use super::screenshot_thumbnail::editor;
            match entry.kind {
                CaptureKind::Shot => editor::open_with(entry.file.clone(), entry.words.clone()),
                CaptureKind::Recording => editor::open_recording(
                    entry.file.clone(),
                    editor_words(&entry.words, &entry.timed, entry.duration_ms),
                ),
            }
        }
        Listed::Annotated(annotation) => open_annotation(annotation),
    }
    #[cfg(not(target_os = "macos"))]
    let _ = entry;
}

/// Opens an annotation in the editor: its movie to trim, or its picture
/// once the movie has expired. Safe from any thread.
#[cfg(target_os = "macos")]
pub(super) fn open_annotation(annotation: &Annotation) {
    use super::screenshot_thumbnail::editor;
    if let Some(movie) = &annotation.movie {
        editor::open_recording(
            movie.clone(),
            editor_words(&annotation.words, &annotation.timed, annotation.duration_ms),
        );
    } else if let Some(picture) = &annotation.picture {
        editor::open_with(picture.clone(), annotation.words.clone());
    }
}

#[cfg(target_os = "macos")]
fn editor_words(
    text: &str,
    timed: &[CaptureWord],
    duration_ms: u64,
) -> Vec<super::screenshot_thumbnail::editor::clip::Word> {
    words_for_editor(text, timed, duration_ms)
        .into_iter()
        .map(|word| super::screenshot_thumbnail::editor::clip::Word {
            t0: word.t0,
            t1: word.t1,
            text: word.text,
        })
        .collect()
}

/// An entry's words for the editor: its own when the model timed them, else
/// its text spread over its length as estimated times, the way the words path
/// fills them for a model that gives none (GigaAM v3). A capture already
/// written without times still opens with its words instead of reading
/// "Nothing was said".
#[cfg(any(target_os = "macos", test))]
fn words_for_editor(text: &str, timed: &[CaptureWord], duration_ms: u64) -> Vec<CaptureWord> {
    if !timed.is_empty() {
        return timed.to_vec();
    }
    scrawl_capture::transcribe::spread_words(text, 0, duration_ms)
        .1
        .into_iter()
        .map(|word| CaptureWord {
            t0: word.t0,
            t1: word.t1,
            text: word.text,
        })
        .collect()
}

/// Copies entry `id` as its card or Annotate would; its Copy shows a check
/// once the clipboard has it.
pub(super) fn copy(ui: &mut Ui, id: u64) {
    let Some(entry) = ui.captures.entry(id).cloned() else {
        return;
    };
    ui.captures.copied = None;
    let spawned = std::thread::Builder::new()
        .name("hex-capture-copy".into())
        .spawn(move || match copy_entry(&entry) {
            Ok(()) => *COPIED.lock().unwrap_or_else(|e| e.into_inner()) = Some(id),
            Err(error) => {
                tracing::warn!(error = format!("{error:#}"), "could not copy the capture");
                crate::feedback::play(crate::feedback::Tone::Error);
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not copy the capture");
    }
}

/// A shot's picture and file, a recording's file, or an annotation's
/// summary on the clipboard, as the Mac's cards and Annotate copy them.
#[cfg(target_os = "macos")]
fn copy_entry(entry: &Listed) -> anyhow::Result<()> {
    match entry {
        Listed::Saved(saved) => crate::screenshots::save::copy_saved(
            &saved.file,
            saved.kind == CaptureKind::Recording,
        ),
        Listed::Annotated(annotation) => objc2::rc::autoreleasepool(|_| {
            scrawl_capture::handoff::copy_bundle_to_clipboard(&annotation.bundle)
        }),
    }
}

/// A saved shot or recording as a file (a file drop on Windows, a file URI
/// on Linux), or an annotation's summary as Annotate's own Copy puts it.
#[cfg(not(target_os = "macos"))]
fn copy_entry(entry: &Listed) -> anyhow::Result<()> {
    match entry {
        Listed::Saved(saved) => crate::history::recordings::copy_to_clipboard(&saved.file),
        Listed::Annotated(annotation) => crate::capture_host::copy_to_clipboard(&annotation.bundle),
    }
}

/// What the reveal action reads: Finder on macOS, the file manager's
/// folder (Explorer with the item selected, or its folder on Linux)
/// elsewhere.
pub(super) fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        tr("Show in Finder")
    } else {
        tr("Show in folder")
    }
}

/// Shows entry `id`'s file selected in Finder or Explorer, or opens its
/// folder on Linux.
pub(super) fn reveal(ui: &mut Ui, id: u64) {
    if let Some(entry) = ui.captures.entry(id) {
        crate::meeting::reveal(entry.file());
    }
}

/// Arms two-step delete confirmation on the first click, and moves entry
/// `id`'s file or annotation bundle to the Trash on the confirming click.
pub(super) fn trash(ui: &mut Ui, id: u64) {
    let Some(at) = ui
        .captures
        .entries
        .iter()
        .position(|entry| entry.id() == id)
    else {
        return;
    };
    let entry = &ui.captures.entries[at];
    let target = (
        id,
        match entry {
            Listed::Annotated(a) => a.bundle.clone(),
            Listed::Saved(e) => e.file.clone(),
        },
    );
    if ui.captures.confirm_trash.take().as_ref() != Some(&target) {
        ui.captures.confirm_trash = Some(target);
        return;
    }
    let path = match ui.captures.entries.remove(at) {
        Listed::Saved(entry) => entry.file,
        Listed::Annotated(annotation) => annotation.bundle,
    };
    if !ui.captures.fixture {
        super::screenshot_thumbnail::files::trash(path);
    }
}

/// The search over the list, or the empty state.
pub(super) fn pane(ui: &Ui, p: Paint) -> El {
    let pane = &ui.captures;
    let screenshots = ui.settings.labs.screenshots;
    let searching = !pane.searched.trim().is_empty();
    if pane.entries.is_empty() && !searching {
        let (icon, title, detail) = if screenshots {
            (
                &CAPTURES_ICON,
                tr("No captures yet."),
                tr("Shots, recordings, and annotations from the last 30 days show here."),
            )
        } else {
            (
                &super::sidebar::SCRAWL_ICON,
                tr("No annotations yet."),
                tr("Annotations from the last 30 days show here."),
            )
        };
        return ld::empty_state(p, "captures-empty", icon, title, Some(detail));
    }
    let mut list = col().key("captures-list").label(if screenshots {
        tr("Captures")
    } else {
        tr("Annotations")
    });
    let count = pane.entries.len();
    for (index, entry) in pane.entries.iter().enumerate() {
        list = list.child(item(ui, p, entry).position_in_set(index + 1, count));
    }
    if pane.entries.is_empty() {
        list = list.child(ld::empty_state(
            p,
            "captures-empty",
            ld::SEARCH_ICON,
            if screenshots {
                tr("No captures match.")
            } else {
                tr("No annotations match.")
            },
            None,
        ));
    }
    let search = TextInput::new(&pane.search)
        .placeholder(if screenshots {
            tr("Search captures")
        } else {
            tr("Search annotations")
        })
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

/// The Copy action a unified History row shows in its hover, in the age's
/// place: a capture's file or bundle goes on the clipboard.
#[cfg(all(target_os = "macos", hex_capture))]
pub(super) fn copy_action(ui: &Ui, p: Paint, id: u64, in_row: bool) -> El {
    let copied = ui.captures.copied == Some(id);
    ld::row_action(
        p,
        if in_row { "row-copy" } else { "detail-copy" },
        if copied {
            p.k.glyph.check_small
        } else {
            ld::COPY_ICON
        },
        if copied { tr("Copied") } else { tr("Copy") },
        move |ui: &mut Ui, _| copy(ui, id),
    )
}

/// One capture: its thumbnail, its words (or file name) over its kind and
/// length, and its time, which Copy, Show in Finder, and Move to Trash take
/// the place of on hover.
pub(super) fn item(ui: &Ui, p: Paint, entry: &Listed) -> El {
    let c = &p.k.c;
    let id = entry.id();
    let title = entry.title();
    let meta = meta(entry);
    let time = event_age(entry.at_ms());
    let mut thumb = el()
        .key("thumb")
        .size(THUMB.0, THUMB.1)
        .radius(p.k.radius.sm)
        .fill(c.layer_01)
        .clip_soft(0.0)
        .a11y_hidden();
    if let Some(pixels) = ui.captures.thumbnail(entry) {
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
            reveal_label(),
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
    let confirming = ui
        .captures
        .confirm_trash
        .as_ref()
        .is_some_and(|(armed, _)| *armed == id);
    let actions = if confirming {
        vec![
            ld::row_action(
                p,
                "trash-cancel",
                p.k.glyph.close,
                tr("Cancel"),
                |ui: &mut Ui, _| ui.captures.confirm_trash = None,
            ),
            ld::row_action(
                p,
                "trash-confirm",
                p.k.glyph.check_small,
                tr("Confirm delete"),
                move |ui: &mut Ui, _| trash(ui, id),
            ),
        ]
    } else {
        actions
    };
    let time_el = text(time.clone())
        .key("time")
        .font(p.k.ty.small)
        .color(p.faint())
        .align(Align::End)
        .a11y_hidden();
    let time_el = if confirming {
        time_el.opacity(0.0)
    } else {
        time_el.group_opacity(1.0, 0.0)
    };
    let hover_el = row()
        .key("hover")
        .gap(2.0)
        .center_y()
        .pin(None, Some(-5.0), Some(ld::row_action_inset(p)), None)
        .children(actions);
    let hover_el = if confirming {
        hover_el.opacity(1.0)
    } else {
        hover_el.group_opacity(0.0, 1.0)
    };
    let end = el().key("end").child(time_el).child(hover_el);
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
                .map(|entry| rune::DragItem::new().file(entry.file().to_path_buf()))
        });
    super::lists::source_item(row_el, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A capture whose model gave no word times still opens in the editor
    /// with its words spread over its length, rather than "Nothing was said".
    #[test]
    fn an_untimed_capture_gets_its_words_spread_over_its_length() {
        let spread = words_for_editor("make it blue", &[], 4_000);
        assert_eq!(
            spread
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>(),
            ["make", "it", "blue"]
        );
        assert_eq!((spread[0].t0, spread[2].t1), (0, 4_000));

        assert!(words_for_editor("   ", &[], 4_000).is_empty(), "silence");

        let timed = vec![CaptureWord {
            t0: 100,
            t1: 300,
            text: "make".into(),
        }];
        assert_eq!(
            words_for_editor("make it blue", &timed, 4_000),
            timed,
            "a model's own times stay"
        );
    }

    #[test]
    fn deleting_a_capture_always_requires_confirmation() {
        let mut ui = super::super::scenes::portal_fixture();
        ui.captures = CapturesPane::fixture();
        let before = ui.captures.entries.len();
        let id = ui.captures.entries[0].id();

        // First click arms confirmation and keeps the row.
        trash(&mut ui, id);
        assert_eq!(ui.captures.entries.len(), before);
        assert_eq!(
            ui.captures.confirm_trash.as_ref().map(|(a, _)| *a),
            Some(id)
        );

        // Second click on the same row removes it and disarms.
        trash(&mut ui, id);
        assert_eq!(ui.captures.entries.len(), before - 1);
        assert!(ui.captures.confirm_trash.is_none());
    }
}

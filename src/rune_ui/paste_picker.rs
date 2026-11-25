//! The focus-free Paste Last picker, centred on the display under the
//! pointer: recent rows beside the selected row's preview. A row returns its
//! frozen owner identity; the output worker, not the view, resolves what
//! will be inserted. While open it covers that display, so a click outside
//! the card closes it without reaching the app.
//!
//! Two panels draw the one card. The picker never takes the keys. Editing a
//! dictation's words needs them, so the editor is a second panel that takes
//! them on demand and shows only while words are edited; the picker's own
//! path is the same with or without it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use super::controls::Paint;
use crate::paste::browse::{self, View};
use crate::paste::recent::Id;
use rune::{El, TextEdit, Tx};
use view::{Act, Shown, Size};

mod fixture;
mod view;

/// A calm appear and a quicker leave; the card never moves or scales.
const FADE_IN: Duration = Duration::from_millis(120);
const FADE_OUT: Duration = Duration::from_millis(100);

/// One of the two panels: what the picker last showed, and whether this
/// panel is the one showing it.
#[derive(Default)]
struct Panel {
    view: View,
    shown: bool,
    /// The picker was open, on either panel, as of the last change.
    open: bool,
    /// The selected dictation's words while they are edited.
    draft: TextEdit,
}

/// The captures' pictures, decoded once for both panels, so the editor has
/// them the moment it takes over. Kept for one invocation.
#[derive(Default)]
struct Pictures {
    cache: Option<rune::ImageCache>,
    held: HashMap<(PathBuf, Size), rune::Image>,
}

static PICTURES: LazyLock<Mutex<Pictures>> = LazyLock::new(Default::default);

/// A capture's picture, asked for the first time it shows; none while it
/// decodes.
fn pixels(path: &Path, size: Size) -> Option<rune::Pixels> {
    let mut pictures = PICTURES.lock().unwrap_or_else(|error| error.into_inner());
    let key = (path.to_owned(), size);
    if !pictures.held.contains_key(&key) {
        let image = pictures.cache.as_ref()?.file(path, size.width(), 2.0);
        pictures.held.insert(key.clone(), image);
    }
    let image = &pictures.held[&key];
    (image.is_ready() && image.error().is_none()).then(|| image.pixels().clone())
}

pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) {
    use rune::{
        DisplayTarget, KeyPolicy, OverlayInput, OverlayLevel, OverlayOptions, OverlayPlacement,
        OverlaySpec,
    };
    // Installed whatever the lab says, hidden until a picker opens, so turning
    // Labs › Paste Last picker on applies without a restart.
    let options = |keys| OverlayOptions {
        placement: Some(OverlayPlacement::FullScreen),
        input: OverlayInput::Regions,
        keys,
        display: DisplayTarget::UnderPointer,
        level: OverlayLevel::AboveOverlays,
        exclude_from_capture: true,
        ..Default::default()
    };
    let size = (view::CARD_W, 560.0);
    let picker = app.overlay(
        OverlaySpec::new("Hex Paste Last", size, panel)
            .options(options(KeyPolicy::None))
            .release_gpu_when_hidden()
            .setup(setup),
        Panel::default(),
    );
    let editor = app.overlay(
        OverlaySpec::new("Hex Paste Last editor", size, panel)
            .options(options(KeyPolicy::OnDemand))
            .release_gpu_when_hidden()
            .on_key(editor_key),
        Panel::default(),
    );
    browse::install(Box::new(move |next| {
        route(&picker, next.clone(), false);
        route(&editor, next, true);
    }));
}

/// A decoded picture wakes the picker, which is what shows while they load.
fn setup(_: &mut Panel, waker: rune::Waker) {
    PICTURES
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .cache = Some(rune::ImageCache::new(waker));
}

/// Shows the picker's `next` state on the panel it belongs to: the editor
/// while words are edited, else the picker.
fn route(panel: &rune::OverlayHandle<Panel>, next: View, editor: bool) {
    let handle = panel.clone();
    panel.update(move |s, tx| {
        if next.revision <= s.view.revision {
            return;
        }
        let (was, swap) = (s.shown, s.open && next.open);
        s.shown = next.open && next.editing == editor;
        s.open = next.open;
        if s.shown {
            if !was {
                if !swap {
                    PICTURES
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .held
                        .clear();
                }
                s.draft = TextEdit::default();
                if let Some(entry) = next.rows.get(next.cursor).filter(|_| editor) {
                    s.draft = TextEdit::new(entry.detail.text.clone());
                    s.draft.focused = true;
                }
            }
            s.view = next;
        } else {
            // The fade out keeps the rows and selection it closed on.
            s.view.revision = next.revision;
            s.view.epoch = next.epoch;
            s.view.open = false;
        }
        tx.instant();
        if s.shown && !was {
            // Taking over from the other panel, this one is there at once
            // and the other leaves under it, so the card never dips.
            handle.fade_in(if swap { Duration::ZERO } else { FADE_IN });
            if editor {
                handle.request_keys();
            }
        } else if !s.shown && was {
            handle.fade_out(FADE_OUT);
        }
    });
}

fn panel(s: &Panel, cx: &mut rune::Cx<Panel>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let epoch = s.view.epoch;
    let editing = s.view.open && s.view.editing;
    let trigger = crate::app_settings::paste_last_keycaps();
    view::stage(
        &Shown {
            rows: &s.view.rows,
            cursor: s.view.cursor,
            now: crate::history::now_ms(),
            draft: editing.then_some(&s.draft),
            trigger: &trigger,
        },
        p,
        &pixels,
        |s: &mut Panel| &mut s.draft,
        move |s: &mut Panel, act, tx| perform(s, epoch, act, tx),
    )
}

/// Escape with the caret outside the words still steps back from editing.
fn editor_key(s: &mut Panel, key: &rune::KeyEvent, _: &mut Tx) -> bool {
    let leave = key.key == rune::Key::Escape && s.view.editing;
    if leave {
        browse::stop_editing(s.view.epoch);
    }
    leave
}

fn perform(s: &mut Panel, epoch: u64, act: Act, tx: &mut Tx) {
    match act {
        Act::Select(id) => browse::select(epoch, id),
        Act::Paste(id) => browse::click(epoch, id),
        Act::Edit(id) => browse::edit(epoch, id),
        Act::PasteEdited => browse::paste_edited(epoch, s.draft.text.clone()),
        Act::StopEditing => browse::stop_editing(epoch),
        Act::Copy(at) => {
            let Some(entry) = s.view.rows.get(at) else {
                return;
            };
            match &entry.row.id {
                Id::Dictation(_) => tx.copy(entry.detail.text.clone()),
                Id::Capture(id) => copy_capture(id.bundle().to_owned()),
            }
            browse::dismiss(epoch);
        }
        Act::Dismiss => {
            browse::dismiss(epoch);
        }
        Act::History => {
            if browse::dismiss(epoch) {
                crate::macos_rune::open_paste_history();
            }
        }
    }
}

/// Copies a capture as Annotate does, off the panel's thread.
fn copy_capture(bundle: PathBuf) {
    let spawned = std::thread::Builder::new()
        .name("hex-paste-copy".into())
        .spawn(move || {
            let copied = objc2::rc::autoreleasepool(|_| {
                scrawl_capture::handoff::copy_bundle_to_clipboard(&bundle)
            });
            if let Err(error) = copied {
                tracing::warn!(error = format!("{error:#}"), "could not copy the capture");
                crate::feedback::play(crate::feedback::Tone::Error);
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not copy the capture");
    }
}

/// Opens the capture in `bundle` in the editor, as History's Open in Editor
/// does: its movie to trim, or its picture once the movie has expired.
pub(crate) fn open_in_editor(bundle: &Path) {
    if let Some(annotation) = crate::history::annotations::read(bundle) {
        super::captures::open_annotation(&annotation);
    }
}

/// A scene's picker over the window: which row is selected, and whether its
/// words are being edited. Presses work on it as they do on the picker, so
/// Playground can try the card without a paste.
#[derive(Default)]
pub(super) struct Preview {
    shown: bool,
    cursor: usize,
    editing: bool,
    draft: TextEdit,
}

impl Preview {
    /// The newest row selected: a capture, its picture in the preview.
    pub(super) fn capture() -> Self {
        Self {
            shown: true,
            ..Self::default()
        }
    }

    /// The newest dictation selected, its whole text in the preview.
    pub(super) fn dictation() -> Self {
        Self {
            cursor: 1,
            ..Self::capture()
        }
    }

    /// That dictation's words being edited.
    pub(super) fn editing() -> Self {
        let mut draft = TextEdit::new(fixture::NEWEST_WORDS.replace("Thursday", "Friday"));
        draft.focused = true;
        Self {
            editing: true,
            draft,
            ..Self::dictation()
        }
    }

    fn perform(&mut self, act: Act) {
        let rows = fixture::rows();
        let at = |id: &Id| rows.iter().position(|entry| &entry.row.id == id);
        match act {
            Act::Select(id) if !self.editing => self.cursor = at(&id).unwrap_or(self.cursor),
            Act::Edit(id @ Id::Dictation(_)) => {
                if let Some(at) = at(&id) {
                    self.cursor = at;
                    self.draft = TextEdit::new(rows[at].detail.text.clone());
                    self.draft.focused = true;
                    self.editing = true;
                }
            }
            Act::StopEditing | Act::PasteEdited => self.editing = false,
            _ => {}
        }
    }
}

/// The picker over the window for the `mac-paste-picker` scenes, on fixed
/// rows that resolve to nothing.
pub(super) fn preview(preview: &Preview, p: Paint) -> Option<El> {
    if !preview.shown {
        return None;
    }
    let rows = fixture::rows();
    let trigger = ["⌥", "⇧", "V"].map(str::to_owned);
    Some(
        view::stage(
            &Shown {
                rows: &rows,
                cursor: preview.cursor,
                now: fixture::NOW,
                draft: preview.editing.then_some(&preview.draft),
                trigger: &trigger,
            },
            p,
            &fixture::pixels,
            |ui: &mut super::Ui| &mut ui.paste_preview.draft,
            |ui: &mut super::Ui, act, _| ui.paste_preview.perform(act),
        )
        .absolute(0.0, 0.0),
    )
}

#[cfg(test)]
mod proof;

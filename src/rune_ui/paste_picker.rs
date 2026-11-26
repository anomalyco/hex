//! The focus-free Paste Last picker, centred on the display under the
//! pointer: recent rows beside the selected row's preview. A row returns its
//! frozen owner identity; the output worker, not the view, resolves what
//! will be inserted. While open it covers that display, so a click outside
//! the card closes it without reaching the app.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use super::controls::Paint;
use crate::paste::browse::{self, View};
use crate::paste::recent::Id;
use rune::{El, Tx};
use view::{Act, Shown, Size};

mod fixture;
mod view;
mod wheel;

/// A calm appear and a quicker leave; the card never moves or scales.
const FADE_IN: Duration = Duration::from_millis(120);
const FADE_OUT: Duration = Duration::from_millis(100);

/// What the picker last showed, and its captures' pictures.
#[derive(Default)]
struct Panel {
    view: View,
    images: Option<rune::ImageCache>,
    /// Decoded for one invocation, and dropped when the next one opens.
    pictures: Mutex<HashMap<(PathBuf, Size), rune::Image>>,
}

impl Panel {
    /// A capture's picture, asked for the first time it shows; none while
    /// it decodes.
    fn pixels(&self, path: &Path, size: Size) -> Option<rune::Pixels> {
        let mut pictures = self
            .pictures
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let key = (path.to_owned(), size);
        if !pictures.contains_key(&key) {
            let image = self.images.as_ref()?.file(path, size.width(), 2.0);
            pictures.insert(key.clone(), image);
        }
        let image = &pictures[&key];
        (image.is_ready() && image.error().is_none()).then(|| image.pixels().clone())
    }
}

pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) {
    use rune::{
        DisplayTarget, KeyPolicy, OverlayInput, OverlayLevel, OverlayOptions, OverlayPlacement,
        OverlaySpec,
    };
    // Installed whatever the lab says, hidden until a picker opens, so turning
    // Labs › Paste Last picker on applies without a restart.
    let spec = OverlaySpec::new("Hex Paste Last", (view::CARD_W, 560.0), panel)
        .options(OverlayOptions {
            placement: Some(OverlayPlacement::FullScreen),
            input: OverlayInput::Regions,
            keys: KeyPolicy::None,
            display: DisplayTarget::UnderPointer,
            level: OverlayLevel::AboveOverlays,
            exclude_from_capture: true,
            ..Default::default()
        })
        .release_gpu_when_hidden()
        // A decoded picture wakes the picker to draw it.
        .setup(|s: &mut Panel, waker| s.images = Some(rune::ImageCache::new(waker)));
    let panel = app.overlay(spec, Panel::default());
    browse::install(Box::new(move |next| {
        let handle = panel.clone();
        panel.update(move |s, tx| {
            if next.revision <= s.view.revision {
                return;
            }
            let was = s.view.open;
            let cursor = s.view.cursor;
            if next.open {
                if !was {
                    s.pictures
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .clear();
                }
                s.view = next;
            } else {
                // The fade out keeps the rows and selection it closed on.
                s.view.revision = next.revision;
                s.view.epoch = next.epoch;
                s.view.open = false;
            }
            tx.instant();
            if s.view.open {
                let reveal = view::reveal(!was, cursor, s.view.cursor, s.view.rows.len());
                view::follow(tx, reveal);
            }
            if s.view.open && !was {
                handle.fade_in(FADE_IN);
            } else if !s.view.open && was {
                handle.fade_out(FADE_OUT);
            }
        });
    }));
}

fn panel(s: &Panel, cx: &mut rune::Cx<Panel>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let epoch = s.view.epoch;
    let trigger = crate::app_settings::paste_last_keycaps();
    view::stage(
        &Shown {
            rows: &s.view.rows,
            cursor: s.view.cursor,
            now: crate::history::now_ms(),
            trigger: &trigger,
        },
        s,
        p,
        |s: &Panel, path: &Path, size| s.pixels(path, size),
        move |s: &mut Panel, act, tx| perform(s, epoch, act, tx),
    )
}

fn perform(s: &mut Panel, epoch: u64, act: Act, tx: &mut Tx) {
    match act {
        Act::Select(id) => browse::select(epoch, id),
        Act::Paste(id) => browse::click(epoch, id),
        Act::Open(id) => browse::open_capture(epoch, id),
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

/// A scene's picker over the window, and which row it has selected. A press
/// on a row selects it, so Playground can try the card without a paste.
#[derive(Default)]
pub(super) struct Preview {
    shown: bool,
    cursor: usize,
}

impl Preview {
    /// The newest row selected: a capture, its picture in the preview.
    pub(super) fn capture() -> Self {
        Self {
            shown: true,
            cursor: 0,
        }
    }

    /// The newest dictation selected, its whole text in the preview.
    pub(super) fn dictation() -> Self {
        Self {
            shown: true,
            cursor: 1,
        }
    }

    fn perform(&mut self, act: Act) {
        if let Act::Select(id) = act {
            let at = fixture::rows().iter().position(|entry| entry.row.id == id);
            self.cursor = at.unwrap_or(self.cursor);
        }
    }
}

/// The picker over the window for the `mac-paste-picker` scenes, on fixed
/// rows that resolve to nothing.
pub(super) fn preview(ui: &super::Ui, p: Paint) -> Option<El> {
    let preview = &ui.paste_preview;
    if !preview.shown {
        return None;
    }
    let rows = fixture::rows().into();
    let trigger = ["⌥", "⇧", "V"].map(str::to_owned);
    Some(
        view::stage(
            &Shown {
                rows: &rows,
                cursor: preview.cursor,
                now: fixture::NOW,
                trigger: &trigger,
            },
            ui,
            p,
            |_: &super::Ui, path: &Path, size| fixture::pixels(path, size),
            |ui: &mut super::Ui, act, _| ui.paste_preview.perform(act),
        )
        .absolute(0.0, 0.0),
    )
}

#[cfg(test)]
mod proof;
#[cfg(test)]
mod tests;

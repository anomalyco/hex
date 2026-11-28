//! Editor state transitions, clipboard/file drop handling, context menu, and
//! selection handles for stacked image layers.

use std::path::PathBuf;

use rune::{El, Phase, PointerEvent, PopupItem, PopupMenu, Tx, el};

#[cfg(target_os = "macos")]
use super::image_layer::read_macos_clipboard_image;
use super::image_layer::{
    CASCADE_PT, Corner, DecodedImage, HANDLE_REACH_PT, IMAGE_EXTENSIONS, ImageDrag, ImageMark,
    Snapshot, build_image_mark, decode_image_bytes, decode_image_file, hit_body, hit_corner,
    initial_rect, is_image_path, move_rect, resize_rect,
};
use super::marks::{self, Mark, Rect, Tool};
use super::{Editor, Out};
use crate::desktop::i18n::tr;
use crate::rune_ui::controls::Paint;

/// Maximum number of undo snapshots retained per open editor session.
const HISTORY_LIMIT: usize = 100;
/// Corner resize handle square size, in points.
const HANDLE_SIZE: f32 = 8.0;

// ─── Editor image-layer & history operations ─────────────────────────────────

impl Editor {
    pub(super) fn snapshot(&self) -> Snapshot {
        Snapshot {
            marks: self.marks.clone(),
            crop: self.crop,
            selected_image: self.selected_image,
        }
    }

    pub(super) fn push_undo(&mut self, before: Snapshot) {
        self.undo_stack.push(before);
        if self.undo_stack.len() > HISTORY_LIMIT {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    pub(super) fn undo(&mut self) {
        if self.typing.take().is_some() {
            return;
        }
        self.image_drag = None;
        if let Some(prev) = self.undo_stack.pop() {
            let current = self.snapshot();
            self.redo_stack.push(current);
            self.marks = prev.marks;
            self.crop = prev.crop;
            self.selected_image = prev.selected_image;
            return;
        }
        // Backwards compatibility if marks/crop were set directly in tests.
        let current = self.snapshot();
        if self.marks.pop().is_some() {
            self.redo_stack.push(current);
            self.sync_selected_image();
        } else if self.crop.take().is_some() {
            self.redo_stack.push(current);
        }
    }

    pub(super) fn redo(&mut self) {
        if self.typing.is_some() {
            return;
        }
        self.image_drag = None;
        if let Some(next) = self.redo_stack.pop() {
            let current = self.snapshot();
            self.undo_stack.push(current);
            self.marks = next.marks;
            self.crop = next.crop;
            self.selected_image = next.selected_image;
        }
    }

    fn sync_selected_image(&mut self) {
        if let Some(id) = self.selected_image
            && !self
                .marks
                .iter()
                .any(|m| matches!(m, Mark::Image(img) if img.id == id))
        {
            self.selected_image = None;
        }
    }

    pub(super) fn selected_image_mark(&self) -> Option<&ImageMark> {
        let id = self.selected_image?;
        self.marks.iter().find_map(|m| match m {
            Mark::Image(img) if img.id == id => Some(img),
            _ => None,
        })
    }

    pub(super) fn image_mark(&self, id: u64) -> Option<&ImageMark> {
        self.marks.iter().find_map(|m| match m {
            Mark::Image(img) if img.id == id => Some(img),
            _ => None,
        })
    }

    pub(super) fn image_mark_mut(&mut self, id: u64) -> Option<&mut ImageMark> {
        self.marks.iter_mut().find_map(|m| match m {
            Mark::Image(img) if img.id == id => Some(img),
            _ => None,
        })
    }

    /// Topmost image layer containing `at` (in shot pixels).
    pub(super) fn image_at(&self, at: [f32; 2]) -> Option<u64> {
        self.marks.iter().rev().find_map(|m| match m {
            Mark::Image(img) if hit_body(img.rect, at) => Some(img.id),
            _ => None,
        })
    }

    /// Moves image `id` to the top of the mark stack; returns true if its z-order changed.
    pub(super) fn bring_image_to_front(&mut self, id: u64) -> bool {
        let Some(pos) = self
            .marks
            .iter()
            .position(|m| matches!(m, Mark::Image(img) if img.id == id))
        else {
            return false;
        };
        if pos + 1 == self.marks.len() {
            return false;
        }
        let mark = self.marks.remove(pos);
        self.marks.push(mark);
        true
    }

    /// Inserts a decoded image onto the canvas, selects it on top of the stack,
    /// and records an undo snapshot.
    pub(super) fn insert_decoded_image(
        &mut self,
        decoded: DecodedImage,
        drop_at: Option<[f32; 2]>,
    ) -> anyhow::Result<u64> {
        let Some(shot) = &self.shot else {
            anyhow::bail!("no screenshot open");
        };
        let (canvas, scale) = (shot.size, shot.scale);
        let existing: Vec<Rect> = self
            .marks
            .iter()
            .filter_map(|m| match m {
                Mark::Image(img) => Some(img.rect),
                _ => None,
            })
            .collect();
        let rect = initial_rect(
            canvas,
            [decoded.width, decoded.height],
            drop_at,
            &existing,
            scale,
        );
        self.next_image_id += 1;
        let id = self.next_image_id;
        let mark = build_image_mark(id, rect, decoded)?;
        let before = self.snapshot();
        self.finish_typing();
        self.send = None;
        self.marks.push(Mark::Image(mark));
        self.selected_image = Some(id);
        self.push_undo(before);
        Ok(id)
    }

    /// Removes the currently selected image layer, recording an undo snapshot.
    pub(super) fn delete_selected_image(&mut self) -> bool {
        let Some(id) = self.selected_image else {
            return false;
        };
        let Some(pos) = self
            .marks
            .iter()
            .position(|m| matches!(m, Mark::Image(img) if img.id == id))
        else {
            self.selected_image = None;
            return false;
        };
        let before = self.snapshot();
        self.marks.remove(pos);
        self.selected_image = None;
        self.image_drag = None;
        self.push_undo(before);
        true
    }

    /// Handles pointer gestures on the canvas: image corner resize, image body
    /// move, click-to-select image, or active drawing/crop/text tool.
    pub(super) fn handle_canvas_drag(&mut self, phase: Phase, at: [f32; 2], fit: f32) {
        let Some(shot) = &self.shot else {
            return;
        };
        let (size, scale) = (shot.size, shot.scale);
        let clamped = [
            at[0].clamp(0.0, size[0] as f32),
            at[1].clamp(0.0, size[1] as f32),
        ];

        match phase {
            Phase::Down => {
                self.pending_image_click = None;
                if let Some(selected) = self.selected_image_mark().cloned() {
                    let reach = HANDLE_REACH_PT / fit.max(0.01);
                    if let Some(corner) = hit_corner(selected.rect, at, reach) {
                        self.image_drag = Some(ImageDrag::Resize {
                            id: selected.id,
                            corner,
                            start_rect: selected.rect,
                            before: self.snapshot(),
                        });
                        return;
                    }
                    if let Some(hit_id) = self.image_at(at) {
                        let before = self.snapshot();
                        self.selected_image = Some(hit_id);
                        self.bring_image_to_front(hit_id);
                        if let Some(img) = self.image_mark(hit_id) {
                            self.image_drag = Some(ImageDrag::Move {
                                id: hit_id,
                                start_rect: img.rect,
                                start_at: at,
                                before,
                            });
                        }
                        return;
                    }
                    // Clicked outside every image: deselect cleanly without leaving stray ink.
                    self.selected_image = None;
                    return;
                }
                if self.tool != Tool::Crop
                    && self.tool != Tool::Text
                    && let Some(hit_id) = self.image_at(clamped)
                {
                    self.pending_image_click = Some((hit_id, clamped));
                }
                self.draw_tool(phase, clamped);
            }
            Phase::Move => {
                if let Some(drag) = self.image_drag.clone() {
                    match drag {
                        ImageDrag::Move {
                            id,
                            start_rect,
                            start_at,
                            ..
                        } => {
                            let next = move_rect(
                                start_rect,
                                [at[0] - start_at[0], at[1] - start_at[1]],
                                size,
                                scale,
                            );
                            if let Some(img) = self.image_mark_mut(id) {
                                img.rect = next;
                            }
                        }
                        ImageDrag::Resize {
                            id,
                            corner,
                            start_rect,
                            ..
                        } => {
                            let next = resize_rect(start_rect, corner, at, size, scale, false);
                            if let Some(img) = self.image_mark_mut(id) {
                                img.rect = next;
                            }
                        }
                    }
                    return;
                }
                self.draw_tool(phase, clamped);
            }
            Phase::Up => {
                if let Some(drag) = self.image_drag.take() {
                    let (id, start_rect, before) = match drag {
                        ImageDrag::Move {
                            id,
                            start_rect,
                            before,
                            ..
                        }
                        | ImageDrag::Resize {
                            id,
                            start_rect,
                            before,
                            ..
                        } => (id, start_rect, before),
                    };
                    let changed = self
                        .image_mark(id)
                        .is_some_and(|img| img.rect != start_rect)
                        || self.marks != before.marks;
                    if changed {
                        self.push_undo(before);
                    }
                    return;
                }
                if let Some((hit_id, down_at)) = self.pending_image_click.take()
                    && (clamped[0] - down_at[0]).hypot(clamped[1] - down_at[1])
                        < marks::MIN_PT * scale
                {
                    self.draft = None;
                    let before = self.snapshot();
                    self.selected_image = Some(hit_id);
                    if self.bring_image_to_front(hit_id) {
                        self.push_undo(before);
                    }
                    return;
                }
                self.draw_tool(phase, clamped);
            }
        }
    }

    /// Requests clipboard contents off the UI thread so `⌘V` / Paste can insert an image.
    pub(super) fn request_paste(&mut self, tx: &mut Tx) {
        if !self.on || self.clip.is_some() || self.shot.is_none() {
            return;
        }
        tx.read_clipboard(|s: &mut Editor, content, tx| s.handle_clipboard(content, tx));
    }

    /// Inserts an image from Rune's `ClipboardContent` (raw PNG/TIFF image or copied file URL).
    pub(super) fn handle_clipboard(&mut self, content: &rune::ClipboardContent, tx: &mut Tx) {
        if !self.on || self.clip.is_some() || self.shot.is_none() {
            return;
        }
        if let Some(img) = content.image() {
            let bytes = img.png().to_vec();
            self.spawn_decode_bytes(vec![bytes], None, tx);
            return;
        }
        let image_files: Vec<PathBuf> = content
            .files()
            .iter()
            .filter(|path| is_image_path(path))
            .cloned()
            .collect();
        if !image_files.is_empty() {
            self.drop_image_files(&image_files, None, tx);
            return;
        }
        #[cfg(target_os = "macos")]
        {
            if let Some(overlay) = self.overlay.clone() {
                let generation = self.generation;
                let spawned = std::thread::Builder::new()
                    .name("hex-editor-paste".into())
                    .spawn(move || {
                        let decoded = read_macos_clipboard_image();
                        overlay.update(move |s, tx| {
                            if s.generation != generation || !s.on {
                                return;
                            }
                            match decoded {
                                Some(img) => {
                                    if let Err(error) = s.insert_decoded_image(img, None) {
                                        tracing::warn!(%error, "could not paste clipboard image");
                                        s.note(tr("Couldn't paste image").to_owned(), tx);
                                    }
                                    tx.animated();
                                }
                                None => {
                                    s.note(tr("No image on clipboard").to_owned(), tx);
                                    tx.animated();
                                }
                            }
                        });
                    });
                if spawned.is_ok() {
                    return;
                }
            }
        }
        self.note(tr("No image on clipboard").to_owned(), tx);
        tx.animated();
    }

    /// Loads dropped image files onto the canvas at `drop_at` (in shot pixels, or centered).
    pub(super) fn drop_image_files(
        &mut self,
        paths: &[PathBuf],
        drop_at: Option<[f32; 2]>,
        tx: &mut Tx,
    ) {
        self.drop_hover = false;
        if !self.on || self.clip.is_some() || self.shot.is_none() {
            return;
        }
        let files: Vec<PathBuf> = paths
            .iter()
            .filter(|path| is_image_path(path))
            .cloned()
            .collect();
        if files.is_empty() {
            return;
        }
        let Some(overlay) = self.overlay.clone() else {
            // Headless / test mode: decode synchronously.
            for (i, path) in files.iter().enumerate() {
                let offset = drop_at.map(|[x, y]| {
                    let bump = i as f32 * CASCADE_PT * self.scale();
                    [x + bump, y + bump]
                });
                match decode_image_file(path).and_then(|img| self.insert_decoded_image(img, offset))
                {
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(%error, path = %path.display(), "could not load dropped image");
                        self.note(tr("Couldn't paste image").to_owned(), tx);
                    }
                }
            }
            tx.animated();
            return;
        };
        let generation = self.generation;
        let scale = self.scale();
        let spawned = std::thread::Builder::new()
            .name("hex-editor-drop".into())
            .spawn(move || {
                let decoded: Vec<anyhow::Result<DecodedImage>> =
                    files.iter().map(|p| decode_image_file(p)).collect();
                overlay.update(move |s, tx| {
                    if s.generation != generation || !s.on {
                        return;
                    }
                    let mut failed = false;
                    for (i, res) in decoded.into_iter().enumerate() {
                        let offset = drop_at.map(|[x, y]| {
                            let bump = i as f32 * CASCADE_PT * scale;
                            [x + bump, y + bump]
                        });
                        match res.and_then(|img| s.insert_decoded_image(img, offset)) {
                            Ok(_) => {}
                            Err(error) => {
                                tracing::warn!(%error, "could not insert dropped image");
                                failed = true;
                            }
                        }
                    }
                    if failed {
                        s.note(tr("Couldn't paste image").to_owned(), tx);
                    }
                    tx.animated();
                });
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not start dropped image decode");
        }
    }

    /// Loads raw image byte payloads (from clipboard or macOS drag data) onto the canvas.
    pub(super) fn spawn_decode_bytes(
        &mut self,
        payloads: Vec<Vec<u8>>,
        drop_at: Option<[f32; 2]>,
        tx: &mut Tx,
    ) {
        self.drop_hover = false;
        if !self.on || self.clip.is_some() || self.shot.is_none() || payloads.is_empty() {
            return;
        }
        let Some(overlay) = self.overlay.clone() else {
            for (i, bytes) in payloads.iter().enumerate() {
                let offset = drop_at.map(|[x, y]| {
                    let bump = i as f32 * CASCADE_PT * self.scale();
                    [x + bump, y + bump]
                });
                match decode_image_bytes(bytes)
                    .and_then(|img| self.insert_decoded_image(img, offset))
                {
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(%error, "could not decode image bytes");
                        self.note(tr("Couldn't paste image").to_owned(), tx);
                    }
                }
            }
            tx.animated();
            return;
        };
        let generation = self.generation;
        let scale = self.scale();
        let spawned = std::thread::Builder::new()
            .name("hex-editor-image-decode".into())
            .spawn(move || {
                let decoded: Vec<anyhow::Result<DecodedImage>> =
                    payloads.iter().map(|b| decode_image_bytes(b)).collect();
                overlay.update(move |s, tx| {
                    if s.generation != generation || !s.on {
                        return;
                    }
                    let mut failed = false;
                    for (i, res) in decoded.into_iter().enumerate() {
                        let offset = drop_at.map(|[x, y]| {
                            let bump = i as f32 * CASCADE_PT * scale;
                            [x + bump, y + bump]
                        });
                        match res.and_then(|img| s.insert_decoded_image(img, offset)) {
                            Ok(_) => {}
                            Err(error) => {
                                tracing::warn!(%error, "could not insert pasted image");
                                failed = true;
                            }
                        }
                    }
                    if failed {
                        s.note(tr("Couldn't paste image").to_owned(), tx);
                    }
                    tx.animated();
                });
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "could not start image decode");
        }
    }

    /// Opens the screenshot in Preview (macOS) or the default viewer. If unmodified
    /// and backed by a file on disk, opens that file directly; otherwise exports
    /// the current composited edits to a temp PNG and opens it.
    pub(super) fn open_in_preview(&mut self, tx: &mut Tx) {
        self.finish_typing();
        self.send = None;
        if !self.on || self.clip.is_some() {
            return;
        }
        let unmodified = self.marks.is_empty() && self.crop.is_none();
        if unmodified && let Some(path) = self.file.clone().filter(|p| p.exists()) {
            super::super::files::open_in_preview(path);
            return;
        }
        self.deliver(Out::Preview, tx);
    }

    /// Maps a point in overlay points to shot pixels if it falls inside the canvas.
    #[cfg(target_os = "macos")]
    pub(super) fn overlay_point_to_canvas(&self, point: [f32; 2]) -> Option<[f32; 2]> {
        let layout = self.layout();
        let [px, py, ..] = layout.panel;
        let [ix, iy, iw, ih] = layout.image;
        let cx = point[0] - (px + ix);
        let cy = point[1] - (py + iy);
        if cx >= 0.0 && cx <= iw && cy >= 0.0 && cy <= ih {
            Some([cx / layout.fit.max(0.001), cy / layout.fit.max(0.001)])
        } else {
            None
        }
    }
}

// ─── Canvas UI helpers for image layers & context menu ───────────────────────

/// One stacked image layer positioned on the canvas at `fit` points a pixel.
pub(super) fn image_mark_el(img: &ImageMark, fit: f32) -> El {
    let [x, y, w, h] = img.rect;
    el().absolute(x * fit, y * fit)
        .size(w * fit, h * fit)
        .pixels_fit(img.image.pixels(), rune::SurfaceFit::Fill)
}

/// Selection outline and four corner resize handles for the active image layer.
pub(super) fn selection_overlay(p: Paint, rect: Rect, fit: f32) -> El {
    let k = p.k;
    let [x, y, w, h] = [rect[0] * fit, rect[1] * fit, rect[2] * fit, rect[3] * fit];
    let mut group = el().key("image-selection").absolute(0.0, 0.0).child(
        el().key("image-outline")
            .absolute(x, y)
            .size(w, h)
            .border(1.5, p.accent()),
    );
    let half = HANDLE_SIZE * 0.5;
    for (i, corner) in Corner::ALL.into_iter().enumerate() {
        let [cx, cy] = corner.position([x, y, w, h]);
        group = group.child(
            el().key(format!("image-handle-{i}"))
                .absolute(cx - half, cy - half)
                .size(HANDLE_SIZE, HANDLE_SIZE)
                .radius(k.radius.xs)
                .fill(k.c.white)
                .border(1.5, p.accent()),
        );
    }
    group
}

/// Accent outline and subtle tint drawn over the canvas while dragging an image file over it.
pub(super) fn drop_hover_overlay(p: Paint, size: [f32; 2], radius: f32) -> El {
    el().key("drop-hover")
        .absolute(0.0, 0.0)
        .size(size[0], size[1])
        .radius(radius)
        .fill(p.k.alpha(p.accent(), 0.08))
        .border(2.0, p.accent())
}

/// Attaches drop-file handlers onto `panel` so drops outside the canvas land centered.
pub(super) fn panel_drop_target(panel: El, is_shot: bool) -> El {
    if !is_shot {
        return panel;
    }
    panel
        .on_drop_files(|s: &mut Editor, files, tx| s.drop_image_files(files, None, tx))
        .accepts_extensions(IMAGE_EXTENSIONS)
        .on_drop_hover(|s: &mut Editor, over, tx| {
            if s.drop_hover != over {
                s.drop_hover = over;
                tx.instant();
            }
        })
}

/// Attaches drag, right-click menu, and file-drop handlers onto the canvas element.
pub(super) fn attach_canvas_input(image: El, is_shot: bool, cursor: rune::Cursor, fit: f32) -> El {
    if !is_shot {
        return image
            .hit()
            .on_secondary_press_local(move |s: &mut Editor, press, tx| {
                canvas_menu(s, press, fit, tx);
            });
    }
    image
        .hit()
        .cursor(cursor)
        .on_drag(move |s: &mut Editor, d: &mut rune::Drag, tx: &mut Tx| {
            if d.phase == Phase::Down {
                s.send = None;
                s.keys();
            }
            s.handle_canvas_drag(d.phase, [d.x / fit, d.y / fit], fit);
            tx.wake_after(std::time::Duration::ZERO);
        })
        .on_secondary_press_local(move |s: &mut Editor, press, tx| {
            canvas_menu(s, press, fit, tx);
        })
        .on_drop_files(move |s: &mut Editor, files, tx| {
            let at = tx.drop_point().map(|pt| [pt.x / fit, pt.y / fit]);
            s.drop_image_files(files, at, tx);
        })
        .accepts_extensions(IMAGE_EXTENSIONS)
        .on_drop_hover(|s: &mut Editor, over, tx| {
            if s.drop_hover != over {
                s.drop_hover = over;
                tx.instant();
            }
        })
}

/// Right-click context menu for the screenshot editor canvas.
pub(super) fn canvas_menu(s: &mut Editor, press: PointerEvent, fit: f32, tx: &mut Tx) {
    let is_shot = s.clip.is_none();
    if is_shot {
        let at = [press.x / fit.max(0.001), press.y / fit.max(0.001)];
        if let Some(hit_id) = s.image_at(at) {
            s.selected_image = Some(hit_id);
        }
    }
    let has_source = s.source().is_some();
    let has_selected_image = is_shot && s.selected_image.is_some();
    let mut menu = PopupMenu::new();
    if is_shot {
        menu = menu.item(PopupItem::new(
            "preview",
            super::super::files::preview_label(),
        ));
    }
    menu = menu.item(PopupItem::new("copy", tr("Copy")));
    if is_shot {
        menu = menu.item(PopupItem::new("paste", tr("Paste")));
    }
    menu = menu.item(PopupItem::new(
        "save",
        tr(if is_shot { "Save" } else { "Export" }),
    ));
    if has_source {
        menu = menu.item(PopupItem::new("reveal", tr("Show in Finder")));
    }
    if has_selected_image {
        menu = menu
            .separator()
            .item(PopupItem::new("delete-image", tr("Delete image")));
    }
    tx.popup_menu(
        menu,
        press.window(),
        |s: &mut Editor, picked, tx| match picked {
            Some("preview") => s.open_in_preview(tx),
            Some("copy") => s.deliver(Out::Copy, tx),
            Some("paste") => s.request_paste(tx),
            Some("save") => s.choose_save(tx),
            Some("reveal") => {
                if let Some(path) = s.source().map(std::path::Path::to_path_buf) {
                    super::super::files::reveal(&path);
                }
            }
            Some("delete-image") => {
                s.delete_selected_image();
                tx.animated();
            }
            _ => {}
        },
    );
}

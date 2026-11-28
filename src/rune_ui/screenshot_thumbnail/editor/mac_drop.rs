//! Native macOS `NSDraggingDestination` on the screenshot editor overlay panel.
//!
//! Rune installs `NSDraggingDestination` on ordinary windows, not overlay panels,
//! so attaching a lightweight subclass to the editor panel's content view lets
//! Finder files, browser images, and dragged Hex thumbnail cards land directly on
//! the screenshot canvas with exact drop coordinates.

use std::ffi::CStr;
use std::path::PathBuf;

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{MainThreadMarker, msg_send, sel};
use objc2_app_kit::{NSApplication, NSPasteboard, NSPasteboardItem, NSView};
use objc2_foundation::{NSArray, NSPoint, NSString, NSURL};

use super::EDITOR;
use super::image_layer::is_image_path;

const FILE_URL: &str = "public.file-url";
const IMAGE_TYPES: [&str; 6] = [
    "public.png",
    "public.jpeg",
    "public.tiff",
    "org.webmproject.webp",
    "public.heic",
    "com.compuserve.gif",
];

/// Registers image drag-and-drop on the overlay panel identified by `window_number`.
pub(super) fn attach(window_number: Option<u32>) {
    let Some(number) = window_number else {
        return;
    };
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    // SAFETY: called on the main thread from `OverlaySpec::on_created` while the panel is live.
    autoreleasepool(|_| unsafe {
        let app = NSApplication::sharedApplication(mtm);
        let Some(window) = app.windowWithWindowNumber(number as isize) else {
            return;
        };
        let Some(view) = window.contentView() else {
            return;
        };
        let view_obj: &AnyObject = &view;
        if !subclass_view(view_obj) {
            return;
        }
        let types = registered_types();
        view.registerForDraggedTypes(&NSArray::from_retained_slice(&types));
    });
}

fn registered_types() -> Vec<Retained<NSString>> {
    [FILE_URL, "NSFilenamesPboardType"]
        .into_iter()
        .chain(IMAGE_TYPES)
        .map(NSString::from_str)
        .collect()
}

unsafe fn has_subclass(obj: &AnyObject, prefix: &[u8]) -> bool {
    let mut cls: Option<&AnyClass> = Some(obj.class());
    while let Some(c) = cls {
        if c.name().to_bytes().starts_with(prefix) {
            return true;
        }
        cls = c.superclass();
    }
    false
}

unsafe fn kvo_observed(obj: &AnyObject) -> bool {
    let name: &CStr = obj.class().name();
    name.to_bytes().starts_with(b"NSKVONotifying_")
}

unsafe fn subclass_view(view: &AnyObject) -> bool {
    // SAFETY: `view` is a live ObjC content view on the main thread.
    if unsafe { has_subclass(view, b"HexEditorDrops_") } {
        return true;
    }
    if unsafe { has_subclass(view, b"RuneDrops_") } || unsafe { kvo_observed(view) } {
        return false;
    }
    let base = view.class();
    let Ok(name) =
        std::ffi::CString::new(format!("HexEditorDrops_{}", base.name().to_string_lossy()))
    else {
        return false;
    };
    let class = match ClassBuilder::new(&name, base) {
        Some(mut b) => {
            // SAFETY: selectors and signatures match AppKit's `NSDraggingDestination`.
            unsafe {
                b.add_method(
                    sel!(draggingEntered:),
                    entered as unsafe extern "C" fn(_, _, _) -> _,
                );
                b.add_method(
                    sel!(draggingUpdated:),
                    updated as unsafe extern "C" fn(_, _, _) -> _,
                );
                b.add_method(
                    sel!(draggingExited:),
                    exited as unsafe extern "C" fn(_, _, _),
                );
                b.add_method(
                    sel!(prepareForDragOperation:),
                    prepare as unsafe extern "C" fn(_, _, _) -> _,
                );
                b.add_method(
                    sel!(performDragOperation:),
                    perform as unsafe extern "C" fn(_, _, _) -> _,
                );
            }
            b.register()
        }
        None => match AnyClass::get(&name) {
            Some(c) => c,
            None => return false,
        },
    };
    // SAFETY: swaps only this instance's isa to an ivar-free subclass of its exact class.
    unsafe { AnyObject::set_class(view, class) };
    true
}

fn file_path(url: &NSString) -> Option<PathBuf> {
    let url = NSURL::URLWithString(url)?;
    let url = url.filePathURL()?;
    Some(PathBuf::from(url.path()?.to_string()))
}

enum DragPayload {
    Files(Vec<PathBuf>),
    Images(Vec<Vec<u8>>),
    None,
}

fn read_pasteboard(pb: &NSPasteboard) -> DragPayload {
    let Some(items) = pb.pasteboardItems() else {
        return DragPayload::None;
    };
    let mut files = Vec::new();
    let mut images = Vec::new();
    let file_url_type = NSString::from_str(FILE_URL);
    for item in items.iter() {
        if let Some(url) = item.stringForType(&file_url_type)
            && let Some(path) = file_path(&url)
            && is_image_path(&path)
        {
            files.push(path);
            continue;
        }
        if let Some(bytes) = image_bytes_from_item(&item) {
            images.push(bytes);
        }
    }
    if !files.is_empty() {
        DragPayload::Files(files)
    } else if !images.is_empty() {
        DragPayload::Images(images)
    } else {
        DragPayload::None
    }
}

fn has_supported_image(pb: &NSPasteboard) -> bool {
    let Some(items) = pb.pasteboardItems() else {
        return false;
    };
    let file_url_type = NSString::from_str(FILE_URL);
    for item in items.iter() {
        if let Some(url) = item.stringForType(&file_url_type)
            && let Some(path) = file_path(&url)
            && is_image_path(&path)
        {
            return true;
        }
        let types: Vec<String> = item.types().iter().map(|t| t.to_string()).collect();
        if IMAGE_TYPES
            .iter()
            .any(|kind| types.iter().any(|t| t == kind))
        {
            return true;
        }
    }
    false
}

fn image_bytes_from_item(item: &NSPasteboardItem) -> Option<Vec<u8>> {
    for kind in IMAGE_TYPES {
        let key = NSString::from_str(kind);
        if let Some(data) = item.dataForType(&key)
            && !data.is_empty()
        {
            return Some(data.to_vec());
        }
    }
    None
}

unsafe fn drag_location(view: &NSView, info: &AnyObject) -> [f32; 2] {
    // SAFETY: `info` is a live `NSDraggingInfo` passed by AppKit on the main thread.
    let p: NSPoint = unsafe { msg_send![info, draggingLocation] };
    let p = view.convertPoint_fromView(p, None);
    let y = if view.isFlipped() {
        p.y
    } else {
        view.bounds().size.height - p.y
    };
    [p.x as f32, y as f32]
}

unsafe fn drag_pasteboard(info: &AnyObject) -> Option<Retained<NSPasteboard>> {
    // SAFETY: `info` is a live `NSDraggingInfo` passed by AppKit on the main thread.
    unsafe { msg_send![info, draggingPasteboard] }
}

unsafe fn drag_view(this: &AnyObject) -> &NSView {
    // SAFETY: this IMP is installed only on an `NSView` subclass.
    unsafe { &*(this as *const AnyObject).cast::<NSView>() }
}

fn set_hover(over: bool) {
    if let Some(overlay) = EDITOR.get() {
        overlay.update(move |s, tx| {
            if s.on && s.clip.is_none() && s.drop_hover != over {
                s.drop_hover = over;
                tx.instant();
            }
        });
    }
}

const NS_DRAG_OPERATION_NONE: usize = 0;
const NS_DRAG_OPERATION_COPY: usize = 1;

unsafe extern "C" fn entered(_this: &AnyObject, _sel: Sel, info: &AnyObject) -> usize {
    autoreleasepool(|_| {
        // SAFETY: `info` is a live `NSDraggingInfo` on the main thread.
        let ok = unsafe { drag_pasteboard(info) }.is_some_and(|pb| has_supported_image(&pb));
        set_hover(ok);
        if ok {
            NS_DRAG_OPERATION_COPY
        } else {
            NS_DRAG_OPERATION_NONE
        }
    })
}

unsafe extern "C" fn updated(_this: &AnyObject, _sel: Sel, info: &AnyObject) -> usize {
    autoreleasepool(|_| {
        // SAFETY: `info` is a live `NSDraggingInfo` on the main thread.
        let ok = unsafe { drag_pasteboard(info) }.is_some_and(|pb| has_supported_image(&pb));
        set_hover(ok);
        if ok {
            NS_DRAG_OPERATION_COPY
        } else {
            NS_DRAG_OPERATION_NONE
        }
    })
}

unsafe extern "C" fn exited(_this: &AnyObject, _sel: Sel, _info: &AnyObject) {
    set_hover(false);
}

unsafe extern "C" fn prepare(_this: &AnyObject, _sel: Sel, info: &AnyObject) -> Bool {
    autoreleasepool(|_| {
        // SAFETY: `info` is a live `NSDraggingInfo` on the main thread.
        let ok = unsafe { drag_pasteboard(info) }.is_some_and(|pb| has_supported_image(&pb));
        Bool::new(ok)
    })
}

unsafe extern "C" fn perform(this: &AnyObject, _sel: Sel, info: &AnyObject) -> Bool {
    autoreleasepool(|_| {
        set_hover(false);
        // SAFETY: `this` is our subclassed `NSView` and `info` is a live `NSDraggingInfo`.
        let point = unsafe { drag_location(drag_view(this), info) };
        let Some(pb) = (unsafe { drag_pasteboard(info) }) else {
            return Bool::NO;
        };
        let payload = read_pasteboard(&pb);
        let Some(overlay) = EDITOR.get() else {
            return Bool::NO;
        };
        match payload {
            DragPayload::Files(files) => {
                overlay.update(move |s, tx| {
                    let at = s.overlay_point_to_canvas(point);
                    s.drop_image_files(&files, at, tx);
                });
                Bool::YES
            }
            DragPayload::Images(images) => {
                overlay.update(move |s, tx| {
                    let at = s.overlay_point_to_canvas(point);
                    s.spawn_decode_bytes(images, at, tx);
                });
                Bool::YES
            }
            DragPayload::None => Bool::NO,
        }
    })
}

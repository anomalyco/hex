//! The typing model, memory-mapped read-only: `hk_model_*`.

use std::ffi::{OsStr, c_char, c_int};
use std::fs::File;
use std::os::fd::BorrowedFd;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Arc;

use hex_keyboard_core::model::{Model, SortedWords};
use memmap2::{Mmap, MmapOptions};

use crate::guard;

/// A model handle: the decoders made from it keep their own reference.
pub struct HkModel {
    pub(crate) model: Arc<Model>,
}

/// A read-only map of a model file: clean, file-backed pages the OS can drop under pressure.
struct Mapped(Mmap);

impl AsRef<[u8]> for Mapped {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

fn model(map: Mmap) -> Option<*mut HkModel> {
    let model = Model::new(Box::new(Mapped(map))).ok()?;
    Some(Box::into_raw(Box::new(HkModel {
        model: Arc::new(model),
    })))
}

pub(crate) fn open_path(path: &Path) -> *mut HkModel {
    guard(None, || {
        let file = File::open(path).ok()?;
        // SAFETY: a read-only shared map (memmap2's `Mmap::map` is MAP_SHARED; the Swift
        // reader's was MAP_PRIVATE, which reads the same for a file nobody writes): clean,
        // file-backed pages. The model files ship inside the app and are never written while
        // mapped.
        let map = unsafe { Mmap::map(&file) }.ok()?;
        model(map)
    })
    .unwrap_or(std::ptr::null_mut())
}

pub(crate) fn open_fd(fd: c_int, offset: i64, length: i64) -> *mut HkModel {
    if fd < 0 || offset < 0 || length <= 0 {
        return std::ptr::null_mut();
    }
    guard(None, || {
        let (offset, length) = (u64::try_from(offset).ok()?, u64::try_from(length).ok()?);
        // SAFETY: only borrowed for the dup below, which leaves the caller's descriptor as it
        // was; a descriptor that isn't open fails there cleanly (EBADF), as the caller is
        // told any value is safe.
        let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
        // A duplicate of our own to map from, closed once mapped: the map outlives it.
        let file = File::from(borrowed.try_clone_to_owned().ok()?);
        let size = file.metadata().ok()?.len();
        if offset.checked_add(length)? > size {
            return None;
        }
        // SAFETY: a read-only shared map (MAP_SHARED) of bytes inside the file (checked
        // above): clean, file-backed pages. It ships in the APK and isn't written while
        // mapped.
        let map = unsafe {
            MmapOptions::new()
                .offset(offset)
                .len(usize::try_from(length).ok()?)
                .map(&file)
        }
        .ok()?;
        drop(file);
        model(map)
    })
    .unwrap_or(std::ptr::null_mut())
}

/// # Safety
/// `path` is NULL or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_open(path: *const c_char) -> *mut HkModel {
    if path.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: non-NULL, and the caller promises a NUL-terminated string.
    let bytes = unsafe { std::ffi::CStr::from_ptr(path) }.to_bytes();
    open_path(Path::new(OsStr::from_bytes(bytes)))
}

/// # Safety
/// Any values are safe; `fd` is only borrowed for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_open_fd(fd: c_int, offset: i64, length: i64) -> *mut HkModel {
    open_fd(fd, offset, length)
}

/// # Safety
/// `model` is NULL or from `hk_model_open*`, and not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_free(model: *mut HkModel) {
    if model.is_null() {
        return;
    }
    // SAFETY: made by Box::into_raw in `model` and, the caller promises, not freed before.
    guard((), || drop(unsafe { Box::from_raw(model) }));
}

/// # Safety
/// `model` is NULL or a live handle from `hk_model_open*`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hk_model_word_count(model: *const HkModel) -> usize {
    // SAFETY: NULL or a live handle, as the caller promises.
    let Some(model) = (unsafe { model.as_ref() }) else {
        return 0;
    };
    guard(0, || model.model.word_count())
}

//! C interface to `hex-keyboard-core` (`include/hex_keyboard.h`), and with the `jni` feature
//! the Android IME's JNI entry points. All `unsafe` code of the keyboard core lives here.
//!
//! Every entry point checks its pointers and runs its body under [`guard`], so a Rust panic
//! comes back as NULL or a nonzero status and never unwinds into Swift, C or the JVM. The
//! hosts are iOS and Android (and the Mac or Linux machine running the tests): Unix only.

#[cfg(feature = "jni")]
mod jni;
mod model;
mod swipe;
mod typing;

use std::ffi::{CStr, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};

pub use model::{HkModel, hk_model_free, hk_model_open, hk_model_open_fd, hk_model_word_count};
pub use swipe::{
    HkCandidate, HkExtraWord, HkKeyCentre, HkKeys, HkSwipeDecoder, HkSwipeParameters,
    HkSwipeReading, hk_swipe_contraction_cost, hk_swipe_cost_of_contraction,
    hk_swipe_default_parameters, hk_swipe_free, hk_swipe_new, hk_swipe_own_word_cost,
    hk_swipe_read, hk_swipe_reading_free,
};
pub use typing::{
    HkIsWordValid, HkSwipeContext, HkSwipeTyped, HkSwipeWords, HkTextReplacement,
    hk_swipe_is_slid_tap, hk_swipe_needs_space, hk_swipe_rank, hk_swipe_reads_as_name,
    hk_swipe_starts_and_ends_with, hk_swipe_typed, hk_swipe_typed_free, hk_swipe_words_free,
    hk_swipe_words_new,
};

/// `HK_ABI_VERSION` in `include/hex_keyboard.h`.
pub const HK_ABI_VERSION: u32 = 2;

#[unsafe(no_mangle)]
pub extern "C" fn hk_abi_version() -> u32 {
    HK_ABI_VERSION
}

/// Runs `body`, or returns `fallback` if it panics. The panic is caught here, at the
/// boundary, so it never unwinds into the caller.
fn guard<T>(fallback: T, body: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or(fallback)
}

/// `count` items at `pointer`, which may be NULL only when `count` is 0. None when it's NULL
/// with items, misaligned, or larger than a slice can be.
///
/// # Safety
/// A non-NULL `pointer` must point to `count` initialised items that stay valid and
/// unchanged for `'a`.
unsafe fn slice<'a, T>(pointer: *const T, count: usize) -> Option<&'a [T]> {
    if count == 0 {
        return Some(&[]);
    }
    let bytes = count.checked_mul(size_of::<T>())?;
    if pointer.is_null() || !pointer.is_aligned() || bytes > isize::MAX as usize {
        return None;
    }
    // SAFETY: non-NULL, aligned and within isize::MAX bytes (checked above); the caller
    // promises `count` initialised items that live for 'a.
    Some(unsafe { std::slice::from_raw_parts(pointer, count) })
}

/// A NUL-terminated UTF-8 string, or None when `pointer` is NULL or the bytes aren't UTF-8.
///
/// # Safety
/// A non-NULL `pointer` must point to a NUL-terminated string valid for `'a`.
unsafe fn utf8<'a>(pointer: *const c_char) -> Option<&'a str> {
    if pointer.is_null() {
        return None;
    }
    // SAFETY: non-NULL, and the caller promises a NUL-terminated string valid for 'a.
    unsafe { CStr::from_ptr(pointer) }.to_str().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_comes_back_as_the_fallback() {
        assert_eq!(guard(7, || -> i32 { panic!("inside the core") }), 7);
        assert_eq!(guard(7, || 3), 3);
    }

    #[test]
    fn the_abi_version_matches_the_header() {
        let header = include_str!("../include/hex_keyboard.h");
        let line = header
            .lines()
            .find_map(|l| l.strip_prefix("#define HK_ABI_VERSION "))
            .expect("the header defines HK_ABI_VERSION");
        assert_eq!(line.trim().parse::<u32>().unwrap(), hk_abi_version());
    }

    #[test]
    fn slices_reject_null_with_items() {
        // SAFETY: NULL with no items is never read; NULL with items must be refused.
        unsafe {
            assert_eq!(slice::<u32>(std::ptr::null(), 0), Some(&[][..]));
            assert_eq!(slice::<u32>(std::ptr::null(), 3), None);
        }
    }
}

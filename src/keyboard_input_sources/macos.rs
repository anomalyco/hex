//! Main-thread-only TIS observation, independent of keyboard shortcuts. Hex
//! reads input sources only while keyboard model switching is on.

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::MainThreadMarker;
use objc2_core_foundation::CFString;

use crate::transcription::keyboard_model_switching::InputSource;

type Ref = *const c_void;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCopyCurrentKeyboardInputSource() -> Ref;
    fn TISCreateInputSourceList(properties: Ref, include_all_installed: u8) -> Ref;
    fn TISGetInputSourceProperty(source: Ref, key: Ref) -> Ref;
    static kTISPropertyInputSourceID: Ref;
    static kTISPropertyLocalizedName: Ref;
    static kTISPropertyInputSourceLanguages: Ref;
    static kTISPropertyInputSourceCategory: Ref;
    static kTISCategoryKeyboardInputSource: Ref;
    static kTISPropertyInputSourceIsSelectCapable: Ref;
    static kTISNotifySelectedKeyboardInputSourceChanged: Ref;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: Ref);
    fn CFEqual(left: Ref, right: Ref) -> u8;
    fn CFBooleanGetValue(value: Ref) -> u8;
    fn CFArrayGetCount(array: Ref) -> isize;
    fn CFArrayGetValueAtIndex(array: Ref, index: isize) -> Ref;
    fn CFNotificationCenterGetDistributedCenter() -> Ref;
    fn CFNotificationCenterAddObserver(
        center: Ref,
        observer: Ref,
        callback: unsafe extern "C" fn(Ref, *mut c_void, Ref, Ref, Ref),
        name: Ref,
        object: Ref,
        suspension_behavior: isize,
    );
    fn CFNotificationCenterRemoveObserver(center: Ref, observer: Ref, name: Ref, object: Ref);
}

/// Owns a +1 CF value. Property values remain borrowed until this drops.
struct Owned(Ref);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: only create/copy results are wrapped in Owned.
            unsafe { CFRelease(self.0) };
        }
    }
}

unsafe fn string_property(source: Ref, key: Ref) -> Option<String> {
    // SAFETY: the live TIS source owns a CFString for either string property.
    let value = unsafe { TISGetInputSourceProperty(source, key) };
    (!value.is_null()).then(|| unsafe { &*value.cast::<CFString>() }.to_string())
}

unsafe fn languages(source: Ref) -> Vec<String> {
    // SAFETY: the live TIS source owns this CFArray of CFStrings.
    unsafe {
        let array = TISGetInputSourceProperty(source, kTISPropertyInputSourceLanguages);
        if array.is_null() {
            return Vec::new();
        }
        (0..CFArrayGetCount(array))
            .map(|index| CFArrayGetValueAtIndex(array, index))
            .filter(|value| !value.is_null())
            .map(|value| (*value.cast::<CFString>()).to_string())
            .collect()
    }
}

/// Every Mac has input sources.
pub(crate) fn available() -> bool {
    true
}

pub(crate) fn current(_mtm: MainThreadMarker) -> Option<String> {
    // SAFETY: TIS calls run on the UI main thread; the copy is released here.
    unsafe {
        let source = Owned(TISCopyCurrentKeyboardInputSource());
        if source.0.is_null() {
            return None;
        }
        string_property(source.0, kTISPropertyInputSourceID)
    }
}

/// Enabled selectable keyboard sources, not all installed sources or palettes.
pub(crate) fn enabled(_mtm: MainThreadMarker) -> Vec<InputSource> {
    // SAFETY: main-thread TIS access; the array retains its source objects.
    unsafe {
        let sources = Owned(TISCreateInputSourceList(ptr::null(), 0));
        if sources.0.is_null() {
            return Vec::new();
        }
        let mut result = Vec::new();
        for index in 0..CFArrayGetCount(sources.0) {
            let source = CFArrayGetValueAtIndex(sources.0, index);
            let category = TISGetInputSourceProperty(source, kTISPropertyInputSourceCategory);
            let selectable =
                TISGetInputSourceProperty(source, kTISPropertyInputSourceIsSelectCapable);
            if category.is_null()
                || CFEqual(category, kTISCategoryKeyboardInputSource) == 0
                || selectable.is_null()
                || CFBooleanGetValue(selectable) == 0
            {
                continue;
            }
            if let (Some(id), Some(name)) = (
                string_property(source, kTISPropertyInputSourceID),
                string_property(source, kTISPropertyLocalizedName),
            ) {
                result.push(InputSource {
                    id,
                    name,
                    languages: languages(source),
                });
            }
        }
        result.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        result.dedup_by(|a, b| a.id == b.id);
        result
    }
}

/// Distributed notifications only mark dirty; the app's main-loop pump reads TIS.
/// The marker also keeps this owner on the main thread through teardown.
struct Watcher {
    dirty: Box<AtomicBool>,
    mtm: MainThreadMarker,
}

thread_local! {
    /// Present only while keyboard model switching is on.
    static WATCHER: RefCell<Option<Watcher>> = const { RefCell::new(None) };
}

/// Starts or stops observing input-source changes. A new observer reports
/// the current source once, so the runtime learns it when switching turns on.
pub(crate) fn observe(mtm: MainThreadMarker, on: bool) {
    WATCHER.with(|slot| {
        let mut slot = slot.borrow_mut();
        if on && slot.is_none() {
            *slot = Some(Watcher::new(mtm));
        } else if !on {
            *slot = None;
        }
    });
}

/// The current source when it changed since the last call, if observing.
pub(crate) fn take_change(_mtm: MainThreadMarker) -> Option<Option<String>> {
    WATCHER.with(|slot| slot.borrow().as_ref().and_then(Watcher::poll))
}

impl Watcher {
    fn new(mtm: MainThreadMarker) -> Self {
        let watcher = Self {
            dirty: Box::new(AtomicBool::new(true)),
            mtm,
        };
        // SAFETY: the boxed flag has a stable address and outlives registration.
        // 4 = DeliverImmediately, including when Hex is in the background.
        unsafe {
            CFNotificationCenterAddObserver(
                CFNotificationCenterGetDistributedCenter(),
                (&*watcher.dirty as *const AtomicBool).cast(),
                changed,
                kTISNotifySelectedKeyboardInputSourceChanged,
                ptr::null(),
                4,
            );
        }
        watcher
    }

    fn poll(&self) -> Option<Option<String>> {
        self.dirty
            .swap(false, Ordering::AcqRel)
            .then(|| current(self.mtm))
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // SAFETY: unregister on the main thread before dropping the boxed flag.
        unsafe {
            CFNotificationCenterRemoveObserver(
                CFNotificationCenterGetDistributedCenter(),
                (&*self.dirty as *const AtomicBool).cast(),
                kTISNotifySelectedKeyboardInputSourceChanged,
                ptr::null(),
            );
        }
    }
}

unsafe extern "C" fn changed(_: Ref, observer: *mut c_void, _: Ref, _: Ref, _: Ref) {
    // SAFETY: observer is the flag registered by Watcher, removed before free.
    if let Some(dirty) = unsafe { observer.cast::<AtomicBool>().as_ref() } {
        dirty.store(true, Ordering::Release);
    }
}

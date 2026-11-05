//! Main-thread-only TIS observation. One observer runs for the app's life:
//! every change rebuilds the keyboard layout snapshot, and keyboard model
//! switching reads the selected source only while it is on.

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
    changes: Box<Changes>,
    /// Keyboard model switching wants the selected source.
    switching: bool,
    mtm: MainThreadMarker,
}

/// What the one observer marks on every change, each taken by its reader.
struct Changes {
    /// The keyboard layout snapshot must be rebuilt.
    layout: AtomicBool,
    /// Keyboard model switching has not yet seen the selected source.
    source: AtomicBool,
}

thread_local! {
    /// Present from GUI startup for the app's life.
    static WATCHER: RefCell<Option<Watcher>> = const { RefCell::new(None) };
}

/// Starts observing input-source changes for the app's life; later calls do
/// nothing. Hex rebuilds its keyboard layout snapshot on every change.
pub(crate) fn start(mtm: MainThreadMarker) {
    WATCHER.with(|slot| {
        slot.borrow_mut().get_or_insert_with(|| Watcher::new(mtm));
    });
}

/// Turns reporting the selected source to keyboard model switching on or
/// off. Turning it on reports the current source once, so the runtime learns
/// it when switching turns on.
pub(crate) fn observe(mtm: MainThreadMarker, on: bool) {
    start(mtm);
    WATCHER.with(|slot| {
        if let Some(watcher) = slot.borrow_mut().as_mut()
            && watcher.switching != on
        {
            watcher.switching = on;
            watcher.changes.source.store(on, Ordering::Release);
        }
    });
}

/// The current source when it changed since the last call, while keyboard
/// model switching is on.
pub(crate) fn take_change(_mtm: MainThreadMarker) -> Option<Option<String>> {
    WATCHER.with(|slot| slot.borrow().as_ref().and_then(Watcher::poll))
}

/// Whether the selected source changed since the last call, so the keyboard
/// layout snapshot must be rebuilt.
pub(crate) fn take_layout_change(_mtm: MainThreadMarker) -> bool {
    WATCHER.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|watcher| watcher.changes.layout.swap(false, Ordering::AcqRel))
    })
}

impl Watcher {
    fn new(mtm: MainThreadMarker) -> Self {
        let watcher = Self {
            changes: Box::new(Changes {
                layout: AtomicBool::new(false),
                source: AtomicBool::new(false),
            }),
            switching: false,
            mtm,
        };
        // SAFETY: the boxed flags have a stable address and outlive registration.
        // 4 = DeliverImmediately, including when Hex is in the background.
        unsafe {
            CFNotificationCenterAddObserver(
                CFNotificationCenterGetDistributedCenter(),
                (&*watcher.changes as *const Changes).cast(),
                changed,
                kTISNotifySelectedKeyboardInputSourceChanged,
                ptr::null(),
                4,
            );
        }
        watcher
    }

    fn poll(&self) -> Option<Option<String>> {
        (self.switching && self.changes.source.swap(false, Ordering::AcqRel))
            .then(|| current(self.mtm))
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // SAFETY: unregister on the main thread before dropping the boxed flags.
        unsafe {
            CFNotificationCenterRemoveObserver(
                CFNotificationCenterGetDistributedCenter(),
                (&*self.changes as *const Changes).cast(),
                kTISNotifySelectedKeyboardInputSourceChanged,
                ptr::null(),
            );
        }
    }
}

unsafe extern "C" fn changed(_: Ref, observer: *mut c_void, _: Ref, _: Ref, _: Ref) {
    // SAFETY: observer is the flags registered by Watcher, removed before free.
    if let Some(changes) = unsafe { observer.cast::<Changes>().as_ref() } {
        changes.layout.store(true, Ordering::Release);
        changes.source.store(true, Ordering::Release);
        // The pump idles between events; this wakes it to read the change.
        crate::macos_rune::wake_pump();
    }
}

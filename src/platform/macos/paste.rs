use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::time::Instant;

use color_eyre::eyre::{Result, eyre};
use objc2::rc::Retained;
#[cfg(test)]
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
#[cfg(test)]
use objc2_app_kit::{NSPasteboardItem, NSPasteboardWriting};
#[cfg(test)]
use objc2_foundation::NSArray;
use objc2_foundation::NSString;

use crate::keyboard;
use crate::suppression::InputActivity;

pub struct Paster {
    clipboard: Retained<NSPasteboard>,
    activity: InputActivity,
    continuation: Option<Continuation>,
    clipboard_restore: Arc<Mutex<ClipboardRestore>>,
    prepared_clipboard: Option<PreparedClipboard>,
}

struct PreparedClipboard {
    change_count: isize,
    snapshot: ClipboardSnapshot,
    capture_ms: u128,
}

struct Continuation {
    revision: u64,
    inserted: String,
}

#[derive(Default)]
struct ClipboardRestore {
    generation: u64,
    original: Option<ClipboardSnapshot>,
    last_change_count: Option<isize>,
}

impl ClipboardRestore {
    fn register(
        &mut self,
        previous: ClipboardSnapshot,
        previous_change_count: isize,
        inserted_change_count: isize,
    ) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        if self.last_change_count != Some(previous_change_count) {
            self.original = Some(previous);
        }
        self.last_change_count = Some(inserted_change_count);
        self.generation
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ClipboardSnapshot {
    items: Vec<Vec<ClipboardFlavor>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ClipboardFlavor {
    data_type: String,
    data: Vec<u8>,
    flags: u32,
}

type PasteboardRef = *const c_void;
type PasteboardItemId = *mut c_void;
type CfTypeRef = *const c_void;
type CfStringRef = *const c_void;
type CfArrayRef = *const c_void;
type CfDataRef = *const c_void;

const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const BAD_PASTEBOARD_FLAVOR: i32 = -25133;
const DUPLICATE_PASTEBOARD_FLAVOR: i32 = -25134;
const SEND_AFTER_PASTE_DELAY: Duration = Duration::from_millis(100);
const SYSTEM_TRANSLATED_FLAVOR: u32 = 1 << 8;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn PasteboardCreate(name: CfStringRef, pasteboard: *mut PasteboardRef) -> i32;
    fn PasteboardSynchronize(pasteboard: PasteboardRef) -> u32;
    fn PasteboardClear(pasteboard: PasteboardRef) -> i32;
    fn PasteboardGetItemCount(pasteboard: PasteboardRef, count: *mut usize) -> i32;
    fn PasteboardGetItemIdentifier(
        pasteboard: PasteboardRef,
        index: isize,
        item: *mut PasteboardItemId,
    ) -> i32;
    fn PasteboardCopyItemFlavors(
        pasteboard: PasteboardRef,
        item: PasteboardItemId,
        flavors: *mut CfArrayRef,
    ) -> i32;
    fn PasteboardGetItemFlavorFlags(
        pasteboard: PasteboardRef,
        item: PasteboardItemId,
        flavor: CfStringRef,
        flags: *mut u32,
    ) -> i32;
    fn PasteboardCopyItemFlavorData(
        pasteboard: PasteboardRef,
        item: PasteboardItemId,
        flavor: CfStringRef,
        data: *mut CfDataRef,
    ) -> i32;
    fn PasteboardPutItemFlavor(
        pasteboard: PasteboardRef,
        item: PasteboardItemId,
        flavor: CfStringRef,
        data: CfDataRef,
        flags: u32,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFArrayGetCount(array: CfArrayRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CfArrayRef, index: isize) -> *const c_void;
    fn CFDataCreate(allocator: *const c_void, bytes: *const u8, length: isize) -> CfDataRef;
    fn CFDataGetBytePtr(data: CfDataRef) -> *const u8;
    fn CFDataGetLength(data: CfDataRef) -> isize;
    fn CFStringGetLength(value: CfStringRef) -> isize;
    fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    fn CFStringGetCString(
        value: CfStringRef,
        buffer: *mut c_char,
        buffer_size: isize,
        encoding: u32,
    ) -> bool;
    fn CFStringCreateWithBytes(
        allocator: *const c_void,
        bytes: *const u8,
        length: isize,
        encoding: u32,
        is_external_representation: u8,
    ) -> CfStringRef;
    fn CFRelease(value: CfTypeRef);
}

struct PasteboardHandle(PasteboardRef);

impl Drop for PasteboardHandle {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}

impl Paster {
    pub fn new(activity: InputActivity) -> Result<Self> {
        Ok(Self {
            clipboard: NSPasteboard::generalPasteboard(),
            activity,
            continuation: None,
            clipboard_restore: Arc::new(Mutex::new(ClipboardRestore::default())),
            prepared_clipboard: None,
        })
    }

    pub fn prepare(&mut self) {
        let change_count = self.clipboard.changeCount();
        if self
            .prepared_clipboard
            .as_ref()
            .is_some_and(|prepared| prepared.change_count == change_count)
        {
            return;
        }
        self.prepared_clipboard = None;
        let _restore = self
            .clipboard_restore
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let started = Instant::now();
        match capture_clipboard(&self.clipboard) {
            Ok(snapshot) => {
                self.prepared_clipboard = Some(PreparedClipboard {
                    change_count,
                    snapshot,
                    capture_ms: started.elapsed().as_millis(),
                });
            }
            Err(error) => tracing::debug!(%error, "eager clipboard capture failed"),
        }
    }

    pub fn paste(&mut self, text: &str) -> Result<()> {
        self.paste_text(text)
    }

    fn paste_text(&mut self, text: &str) -> Result<()> {
        let revision = self.activity.revision();
        let text = self
            .continuation
            .as_ref()
            .filter(|continuation| continuation.revision == revision)
            .map_or_else(
                || text.to_string(),
                |continuation| join(&continuation.inserted, text),
            );
        let mut restore = self
            .clipboard_restore
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let previous_change_count = self.clipboard.changeCount();
        let prepared = self.prepared_clipboard.take();
        let previous = if let Some(prepared) = prepared
            && prepared.change_count == previous_change_count
        {
            tracing::debug!(
                clipboard_capture_ms = prepared.capture_ms,
                "used eagerly captured clipboard"
            );
            prepared.snapshot
        } else {
            let started = Instant::now();
            let snapshot = capture_clipboard(&self.clipboard)?;
            tracing::debug!(
                clipboard_capture_ms = started.elapsed().as_millis(),
                "captured clipboard at paste time"
            );
            snapshot
        };
        if self.clipboard.changeCount() != previous_change_count {
            return Err(eyre!("clipboard changed while HEX was preserving it"));
        }
        if let Err(error) = write_clipboard_text(&self.clipboard, &text) {
            if let Err(restore_error) = restore_clipboard(&self.clipboard, &previous) {
                tracing::error!(%restore_error, "could not recover the clipboard after a failed write");
            }
            return Err(error);
        }
        let inserted_change_count = self.clipboard.changeCount();
        let generation = restore.register(previous, previous_change_count, inserted_change_count);
        drop(restore);

        let clipboard_restore = self.clipboard_restore.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            let mut restore = clipboard_restore
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if restore.generation != generation {
                return;
            }
            let clipboard = NSPasteboard::generalPasteboard();
            if restore.last_change_count != Some(clipboard.changeCount()) {
                restore.original = None;
                restore.last_change_count = None;
                return;
            }
            let Some(previous) = restore.original.take() else {
                return;
            };
            let result = restore_clipboard(&clipboard, &previous);
            restore.last_change_count = None;
            if let Err(error) = result {
                tracing::warn!(%error, "could not restore clipboard after paste");
            }
        });

        keyboard::post_command('v')?;
        self.continuation = Some(Continuation {
            revision,
            inserted: text,
        });
        Ok(())
    }

    pub fn paste_and_send(&mut self, text: &str) -> Result<()> {
        self.paste(text)?;
        send_after_paste(thread::sleep, keyboard::post_enter)?;
        self.continuation = None;
        Ok(())
    }

    pub fn paste_standalone(&mut self, text: &str) -> Result<()> {
        self.continuation = None;
        self.paste(text)?;
        self.continuation = None;
        Ok(())
    }
}

fn send_after_paste(
    wait: impl FnOnce(Duration),
    post_enter: impl FnOnce() -> Result<()>,
) -> Result<()> {
    wait(SEND_AFTER_PASTE_DELAY);
    post_enter()
}

fn capture_clipboard(clipboard: &NSPasteboard) -> Result<ClipboardSnapshot> {
    let capture_started = Instant::now();
    let pasteboard = create_pasteboard(&clipboard.name())?;
    unsafe { PasteboardSynchronize(pasteboard.0) };
    let mut item_count = 0;
    check_status(
        unsafe { PasteboardGetItemCount(pasteboard.0, &mut item_count) },
        "count clipboard items",
    )?;
    let mut items = Vec::with_capacity(item_count);
    for index in 1..=item_count {
        let mut item = ptr::null_mut();
        check_status(
            unsafe { PasteboardGetItemIdentifier(pasteboard.0, index as isize, &mut item) },
            "identify a clipboard item",
        )?;
        let mut flavors = ptr::null();
        check_status(
            unsafe { PasteboardCopyItemFlavors(pasteboard.0, item, &mut flavors) },
            "list clipboard formats",
        )?;
        if flavors.is_null() {
            return Err(eyre!("could not list clipboard formats"));
        }
        let flavor_count = unsafe { CFArrayGetCount(flavors) };
        let has_authoritative_flavor = (0..flavor_count).any(|flavor_index| {
            let flavor = unsafe { CFArrayGetValueAtIndex(flavors, flavor_index) };
            if flavor.is_null() {
                return false;
            }
            let mut flags = 0;
            (unsafe { PasteboardGetItemFlavorFlags(pasteboard.0, item, flavor, &mut flags) }) == 0
                && flags & SYSTEM_TRANSLATED_FLAVOR == 0
        });
        let result = (0..flavor_count)
            .map(|flavor_index| {
                let flavor = unsafe { CFArrayGetValueAtIndex(flavors, flavor_index) };
                if flavor.is_null() {
                    return Err(eyre!("clipboard format was unavailable"));
                }
                let data_type = cf_string(flavor)?;
                let mut flags = 0;
                check_status(
                    unsafe { PasteboardGetItemFlavorFlags(pasteboard.0, item, flavor, &mut flags) },
                    "inspect clipboard format",
                )?;
                if !should_preserve_flavor(flags, has_authoritative_flavor) {
                    tracing::debug!(%data_type, "skipping synthesized clipboard format");
                    return Ok(None);
                }
                let mut data = ptr::null();
                let flavor_started = Instant::now();
                let status =
                    unsafe { PasteboardCopyItemFlavorData(pasteboard.0, item, flavor, &mut data) };
                let flavor_ms = flavor_started.elapsed().as_millis();
                if flavor_ms >= 100 {
                    tracing::warn!(%data_type, flavor_ms, "clipboard format was slow to materialize");
                }
                if status == BAD_PASTEBOARD_FLAVOR {
                    tracing::debug!(%data_type, "skipping unavailable clipboard format");
                    return Ok(None);
                }
                check_status(status, "preserve clipboard format")?;
                if data.is_null() {
                    return Err(eyre!("clipboard format data was unavailable"));
                }
                let bytes = cf_data(data);
                unsafe { CFRelease(data) };
                Ok(Some(ClipboardFlavor {
                    data_type,
                    data: bytes?,
                    flags: flags & 0x0f,
                }))
            })
            .collect::<Result<Vec<_>>>();
        unsafe { CFRelease(flavors) };
        items.push(result?.into_iter().flatten().collect());
    }
    let capture_ms = capture_started.elapsed().as_millis();
    if capture_ms >= 100 {
        tracing::warn!(capture_ms, item_count, "clipboard snapshot was slow");
    }
    Ok(ClipboardSnapshot { items })
}

fn should_preserve_flavor(flags: u32, has_authoritative_flavor: bool) -> bool {
    !has_authoritative_flavor || flags & SYSTEM_TRANSLATED_FLAVOR == 0
}

fn write_clipboard_text(clipboard: &NSPasteboard, text: &str) -> Result<()> {
    clipboard.clearContents();
    let text = NSString::from_str(text);
    let data_type = unsafe { NSPasteboardTypeString };
    clipboard
        .setString_forType(&text, data_type)
        .then_some(())
        .ok_or_else(|| eyre!("could not write the transcript to the clipboard"))
}

fn restore_clipboard(clipboard: &NSPasteboard, snapshot: &ClipboardSnapshot) -> Result<()> {
    let pasteboard = create_pasteboard(&clipboard.name())?;
    unsafe { PasteboardSynchronize(pasteboard.0) };
    check_status(
        unsafe { PasteboardClear(pasteboard.0) },
        "clear the clipboard",
    )?;
    for (item_index, item) in snapshot.items.iter().enumerate() {
        let item_id = (item_index + 1) as PasteboardItemId;
        for flavor in item {
            let data_type = cf_string_create(&flavor.data_type)?;
            let data = unsafe {
                CFDataCreate(
                    ptr::null(),
                    flavor.data.as_ptr(),
                    flavor.data.len() as isize,
                )
            };
            if data.is_null() {
                unsafe { CFRelease(data_type) };
                return Err(eyre!("could not encode a clipboard format"));
            }
            let status = unsafe {
                PasteboardPutItemFlavor(pasteboard.0, item_id, data_type, data, flavor.flags)
            };
            unsafe {
                CFRelease(data);
                CFRelease(data_type);
            }
            if status == DUPLICATE_PASTEBOARD_FLAVOR {
                tracing::debug!(data_type = %flavor.data_type, "skipping synthesized clipboard format");
                continue;
            }
            check_status(status, "restore clipboard format")?;
        }
    }
    Ok(())
}

#[cfg(test)]
fn write_items(clipboard: &NSPasteboard, items: Vec<Retained<NSPasteboardItem>>) -> Result<()> {
    let objects = items
        .iter()
        .map(|item| ProtocolObject::from_ref(&**item))
        .collect::<Vec<&ProtocolObject<dyn NSPasteboardWriting>>>();
    let objects = NSArray::from_slice(&objects);
    clipboard
        .writeObjects(&objects)
        .then_some(())
        .ok_or_else(|| eyre!("could not restore the clipboard"))
}

fn create_pasteboard(name: &NSString) -> Result<PasteboardHandle> {
    let mut pasteboard = ptr::null();
    check_status(
        unsafe { PasteboardCreate((name as *const NSString).cast(), &mut pasteboard) },
        "open the clipboard",
    )?;
    (!pasteboard.is_null())
        .then_some(PasteboardHandle(pasteboard))
        .ok_or_else(|| eyre!("could not open the clipboard"))
}

fn check_status(status: i32, action: &str) -> Result<()> {
    (status == 0)
        .then_some(())
        .ok_or_else(|| eyre!("could not {action} (status {status})"))
}

fn cf_string_create(value: &str) -> Result<CfStringRef> {
    let value = unsafe {
        CFStringCreateWithBytes(
            ptr::null(),
            value.as_ptr(),
            value.len() as isize,
            CF_STRING_ENCODING_UTF8,
            0,
        )
    };
    (!value.is_null())
        .then_some(value)
        .ok_or_else(|| eyre!("could not encode a clipboard format name"))
}

fn cf_string(value: CfStringRef) -> Result<String> {
    let length = unsafe { CFStringGetLength(value) };
    let capacity = unsafe { CFStringGetMaximumSizeForEncoding(length, CF_STRING_ENCODING_UTF8) }
        .saturating_add(1);
    let mut bytes = vec![0; capacity as usize];
    if !unsafe {
        CFStringGetCString(
            value,
            bytes.as_mut_ptr().cast(),
            capacity,
            CF_STRING_ENCODING_UTF8,
        )
    } {
        return Err(eyre!("could not decode a clipboard format name"));
    }
    let length = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8(bytes[..length].to_vec()).map_err(Into::into)
}

fn cf_data(value: CfDataRef) -> Result<Vec<u8>> {
    let length = unsafe { CFDataGetLength(value) };
    if length < 0 {
        return Err(eyre!("clipboard format had an invalid length"));
    }
    if length == 0 {
        return Ok(Vec::new());
    }
    let bytes = unsafe { CFDataGetBytePtr(value) };
    if bytes.is_null() {
        return Err(eyre!("clipboard format data was unavailable"));
    }
    Ok(unsafe { std::slice::from_raw_parts(bytes, length as usize) }.to_vec())
}

fn join(previous: &str, next: &str) -> String {
    let sentence_start = ends_sentence(previous);
    let mut next = set_initial_case(next, sentence_start);
    let needs_space = previous
        .chars()
        .next_back()
        .is_some_and(|character| !character.is_whitespace() && !is_opening(character))
        && next
            .chars()
            .next()
            .is_some_and(|character| !character.is_whitespace() && !is_closing(character));
    if needs_space {
        next.insert(0, ' ');
    }
    next
}

fn ends_sentence(text: &str) -> bool {
    text.trim_end()
        .trim_end_matches(['\'', '"', ')', ']', '}'])
        .ends_with(['.', '?', '!'])
}

fn is_opening(character: char) -> bool {
    matches!(character, '(' | '[' | '{')
}

fn is_closing(character: char) -> bool {
    matches!(
        character,
        ',' | '.' | '?' | '!' | ';' | ':' | ')' | ']' | '}'
    )
}

fn set_initial_case(text: &str, uppercase: bool) -> String {
    let Some((index, character)) = text
        .char_indices()
        .find(|(_, character)| character.is_alphabetic())
    else {
        return text.to_string();
    };
    if uppercase && character.is_lowercase() {
        return replace_character(text, index, character.to_uppercase());
    }
    if !uppercase && character.is_uppercase() && sentence_initial_word(text, index) {
        return replace_character(text, index, character.to_lowercase());
    }
    text.to_string()
}

fn sentence_initial_word(text: &str, start: usize) -> bool {
    let word = text[start..]
        .split(|character: char| !character.is_alphabetic() && character != '\'')
        .next()
        .unwrap_or_default();
    matches!(
        word.to_ascii_lowercase().as_str(),
        "a" | "an"
            | "and"
            | "as"
            | "because"
            | "but"
            | "for"
            | "if"
            | "nor"
            | "or"
            | "so"
            | "the"
            | "then"
            | "though"
            | "to"
            | "when"
            | "while"
            | "yet"
    )
}

fn replace_character(text: &str, index: usize, replacement: impl Iterator<Item = char>) -> String {
    let character_length = text[index..].chars().next().unwrap().len_utf8();
    let mut output = String::with_capacity(text.len());
    output.push_str(&text[..index]);
    output.extend(replacement);
    output.push_str(&text[index + character_length..]);
    output
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use objc2_foundation::NSData;

    static PASTEBOARD_TEST: Mutex<()> = Mutex::new(());

    #[test]
    fn send_waits_for_the_target_to_apply_the_paste_before_pressing_enter() {
        let steps = RefCell::new(Vec::new());

        send_after_paste(
            |delay| steps.borrow_mut().push(("wait", Some(delay))),
            || {
                steps.borrow_mut().push(("enter", None));
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(
            steps.into_inner(),
            vec![("wait", Some(SEND_AFTER_PASTE_DELAY)), ("enter", None)]
        );
    }

    #[test]
    fn joins_contiguous_dictation_with_sentence_aware_spacing() {
        assert_eq!(
            join("Because if I don't do that,", "And if you see it."),
            " and if you see it."
        );
        assert_eq!(
            join("And if you see it.", "well, that works."),
            " Well, that works."
        );
        assert_eq!(join("Already spaced. ", "Next sentence."), "Next sentence.");
        assert_eq!(join("Hello", ", world."), ", world.");
    }

    #[test]
    fn preserves_likely_proper_nouns_mid_sentence() {
        assert_eq!(join("Open", "GitHub next."), " GitHub next.");
        assert_eq!(join("Message", "Slack now."), " Slack now.");
    }

    #[test]
    fn rapid_pastes_keep_the_original_clipboard_for_the_latest_restore() {
        let mut restore = ClipboardRestore::default();
        let original = ClipboardSnapshot {
            items: vec![vec![ClipboardFlavor {
                data_type: "public.png".into(),
                data: vec![1, 2, 3],
                flags: 0,
            }]],
        };
        let first_insert = ClipboardSnapshot {
            items: vec![vec![ClipboardFlavor {
                data_type: "public.utf8-plain-text".into(),
                data: b"first".to_vec(),
                flags: 0,
            }]],
        };

        let first = restore.register(original.clone(), 10, 12);
        let second = restore.register(first_insert, 12, 14);

        assert_ne!(first, second);
        assert_eq!(restore.original, Some(original));
        assert_eq!(restore.last_change_count, Some(14));
    }

    #[test]
    fn clipboard_snapshot_preserves_multiple_items_and_formats() {
        let _test = PASTEBOARD_TEST.lock().unwrap();
        let clipboard = NSPasteboard::pasteboardWithUniqueName();
        let expected = [
            vec![
                ("com.hex.fixture.text", b"text".to_vec()),
                ("com.hex.fixture.rich", b"rich text".to_vec()),
            ],
            vec![("com.hex.fixture.image", vec![1, 2, 3, 4])],
        ];
        let items = expected
            .iter()
            .map(|types| {
                let item = NSPasteboardItem::new();
                for (data_type, bytes) in types {
                    assert!(item.setData_forType(
                        &NSData::with_bytes(bytes),
                        &NSString::from_str(data_type)
                    ));
                }
                item
            })
            .collect();
        write_items(&clipboard, items).unwrap();
        let snapshot = capture_clipboard(&clipboard).unwrap();

        clipboard.clearContents();
        restore_clipboard(&clipboard, &snapshot).unwrap();

        assert_eq!(capture_clipboard(&clipboard).unwrap(), snapshot);
    }

    #[test]
    fn eager_clipboard_capture_refreshes_a_stale_snapshot() {
        let _test = PASTEBOARD_TEST.lock().unwrap();
        let clipboard = NSPasteboard::pasteboardWithUniqueName();
        let item = NSPasteboardItem::new();
        assert!(item.setData_forType(
            &NSData::with_bytes(b"first"),
            &NSString::from_str("com.hex.fixture.first")
        ));
        clipboard.clearContents();
        write_items(&clipboard, vec![item]).unwrap();
        let mut paster = Paster {
            clipboard: clipboard.clone(),
            activity: InputActivity::default(),
            continuation: None,
            clipboard_restore: Arc::new(Mutex::new(ClipboardRestore::default())),
            prepared_clipboard: None,
        };
        paster.prepare();
        let first_change_count = paster.prepared_clipboard.as_ref().unwrap().change_count;

        let item = NSPasteboardItem::new();
        assert!(item.setData_forType(
            &NSData::with_bytes(b"second"),
            &NSString::from_str("com.hex.fixture.second")
        ));
        clipboard.clearContents();
        write_items(&clipboard, vec![item]).unwrap();
        paster.prepare();
        let prepared = paster.prepared_clipboard.as_ref().unwrap();

        assert_ne!(prepared.change_count, first_change_count);
        assert_eq!(prepared.snapshot, capture_clipboard(&clipboard).unwrap());
    }

    #[test]
    fn skips_regenerable_system_translations() {
        assert!(!should_preserve_flavor(SYSTEM_TRANSLATED_FLAVOR, true));
        assert!(should_preserve_flavor(0, true));
        assert!(should_preserve_flavor(SYSTEM_TRANSLATED_FLAVOR, false));
    }

    #[test]
    fn image_snapshot_keeps_its_authoritative_format() {
        let _test = PASTEBOARD_TEST.lock().unwrap();
        let clipboard = NSPasteboard::pasteboardWithUniqueName();
        let item = NSPasteboardItem::new();
        assert!(item.setData_forType(
            &NSData::with_bytes(include_bytes!("../../../app/AppIcon.icon/Assets/Image.png")),
            &NSString::from_str("public.png")
        ));
        clipboard.clearContents();
        write_items(&clipboard, vec![item]).unwrap();

        let snapshot = capture_clipboard(&clipboard).unwrap();
        assert_eq!(
            snapshot
                .items
                .iter()
                .flatten()
                .map(|flavor| flavor.data_type.as_str())
                .collect::<Vec<_>>(),
            vec!["public.png"]
        );
        clipboard.clearContents();
        restore_clipboard(&clipboard, &snapshot).unwrap();
        assert_eq!(capture_clipboard(&clipboard).unwrap(), snapshot);
    }
}

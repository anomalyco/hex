use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

use color_eyre::eyre::{Result, eyre};

type InputSourceRef = *const c_void;
type EventRef = *mut c_void;

const KEY_ACTION_DISPLAY: u16 = 3;
const NO_DEAD_KEYS: isize = 0;
const NO_MODIFIER_STATE: u32 = 0;
/// Carbon `cmdKey >> 8`, the UCKeyTranslate modifier state for the Command layer.
const COMMAND_MODIFIER_STATE: u32 = 1;
const HID_EVENT_TAP: u32 = 0;
const COMMAND_KEY_CODE: u16 = 55;
const COMMAND_FLAG: u64 = 1 << 20;
const SHIFT_KEY_CODE: u16 = 56;
const SHIFT_FLAG: u64 = 1 << 17;
const RETURN_KEY_CODE: u16 = 36;
/// Quartz keeps at most 20 UTF-16 units of a keyboard event's Unicode string.
const MAX_UNICODE_UNITS: usize = 20;
const EVENT_SOURCE_USER_DATA: u32 = 42;
/// A hole in the virtual key codes, between keypad `*` (0x43) and keypad `+`
/// (0x45): no keyboard sends it, and of the 264 layouts macOS ships only
/// Unicode Hex Input maps it (to U+0000, never with Option alone). Unlike
/// F13–F20 or the JIS keys it has no system action and nothing binds it.
pub const CHORD_BREAKER_KEY_CODE: u16 = 0x44;
pub const SYNTHETIC_EVENT_MARKER: i64 = 0x0056_4f49_4345;
static KEY_CODES: OnceLock<LayoutCodes> = OnceLock::new();
static LAYOUT_ACCESS: Mutex<()> = Mutex::new(());
#[cfg(test)]
pub(crate) static INPUT_SOURCE_QUERIES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCopyCurrentKeyboardLayoutInputSource() -> InputSourceRef;
    fn TISCopyCurrentASCIICapableKeyboardLayoutInputSource() -> InputSourceRef;
    static kTISPropertyUnicodeKeyLayoutData: *const c_void;
    fn TISGetInputSourceProperty(
        input_source: InputSourceRef,
        property_key: *const c_void,
    ) -> *const c_void;
    fn UCKeyTranslate(
        layout: *const u8,
        key_code: u16,
        key_action: u16,
        modifier_state: u32,
        keyboard_type: u32,
        options: isize,
        dead_key_state: *mut u32,
        max_length: isize,
        actual_length: *mut isize,
        output: *mut u16,
    ) -> i32;
    fn LMGetKbdType() -> u8;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
    fn CFRelease(value: *const c_void);
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreateKeyboardEvent(source: *mut c_void, key: u16, down: bool) -> EventRef;
    fn CGEventSetFlags(event: EventRef, flags: u64);
    fn CGEventKeyboardSetUnicodeString(event: EventRef, length: usize, text: *const u16);
    fn CGEventSetIntegerValueField(event: EventRef, field: u32, value: i64);
    fn CGEventPost(tap: u32, event: EventRef);
}

/// Character key codes for the active layout's plain and Command layers.
/// Layouts such as "Dvorak – QWERTY ⌘" switch to a different arrangement while
/// Command is held, so Command shortcuts must resolve against that layer.
struct LayoutCodes {
    plain: HashMap<char, u16>,
    command: HashMap<char, u16>,
}

impl LayoutCodes {
    fn key_code(&self, character: char, command: bool) -> Option<u16> {
        let character = character.to_ascii_lowercase();
        command
            .then(|| self.command.get(&character))
            .flatten()
            .or_else(|| self.plain.get(&character))
            .copied()
    }
}

/// The key that produces `character` on the active layout's plain layer.
pub fn key_code_for(character: char) -> Result<u16> {
    resolve_key_code(character, false)
}

fn resolve_key_code(character: char, command: bool) -> Result<u16> {
    let cached = || {
        KEY_CODES.get().map(|codes| {
            codes
                .key_code(character, command)
                .ok_or_else(|| eyre!("current keyboard layout has no key for {character:?}"))
        })
    };
    if let Some(result) = cached() {
        return result;
    }
    // Headless callers retain live-layout resolution. GUI callers must prewarm
    // on the main thread; even a cache miss then returns without entering TIS.
    let _access = LAYOUT_ACCESS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(result) = cached() {
        return result;
    }
    let source = LayoutSource::current()?;
    let layout = source.layout_bytes()?;
    let mut target = [0; 4];
    let target = character.encode_utf8(&mut target);
    let find = |modifier_state| {
        (0..128).find(|&key_code| {
            translate(layout, key_code, modifier_state)
                .is_some_and(|translated| translated.eq_ignore_ascii_case(target))
        })
    };
    let result = command
        .then(|| find(COMMAND_MODIFIER_STATE))
        .flatten()
        .or_else(|| find(NO_MODIFIER_STATE));
    drop(source);
    // Non-Latin layouts produce no ASCII letters with an empty modifier state;
    // resolve those against the ASCII-capable layout like AppKit does.
    result
        .or_else(|| ascii_capable_key_code(character))
        .ok_or_else(|| eyre!("current keyboard layout has no key for {character:?}"))
}

/// The character `code` types with Command held on the active layout, read
/// from the startup snapshot only (never TIS): a capture's key log names keys
/// with it.
#[cfg(hex_capture)]
pub fn command_character(code: u16) -> Option<char> {
    let codes = KEY_CODES.get()?;
    codes
        .command
        .iter()
        .chain(&codes.plain)
        .find(|(_, key_code)| **key_code == code)
        .map(|(character, _)| *character)
}

/// GUI startup must build this snapshot on the main thread before starting workers.
/// Later lookups, including misses, never re-enter TIS/TSM from a worker thread.
pub fn initialize_layout() -> Result<()> {
    if KEY_CODES.get().is_some() {
        return Ok(());
    }
    let _access = LAYOUT_ACCESS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if KEY_CODES.get().is_some() {
        return Ok(());
    }
    let source = LayoutSource::current()?;
    let mut codes = collect_layout_codes(&source, NO_MODIFIER_STATE)?;
    let command = collect_layout_codes(&source, COMMAND_MODIFIER_STATE).unwrap_or_default();
    drop(source);
    // Non-Latin layouts (Russian, Hebrew, …) produce no ASCII letters with an
    // empty modifier state, so shortcuts like Cmd+V would miss. Appkit resolves
    // those against the ASCII-capable layout; mirror that here by filling the
    // missing letters from the ASCII-capable input source.
    if ('a'..='z').any(|character| !codes.contains_key(&character))
        && let Some(ascii_codes) = ascii_capable_key_codes()
    {
        fill_missing_ascii_letters(&mut codes, ascii_codes);
    }
    let _ = KEY_CODES.set(LayoutCodes {
        plain: codes,
        command,
    });
    Ok(())
}

fn ascii_capable_key_codes() -> Option<HashMap<char, u16>> {
    // Called only while LAYOUT_ACCESS is held, before the GUI snapshot is published.
    collect_layout_codes(&LayoutSource::ascii_capable()?, NO_MODIFIER_STATE).ok()
}

fn fill_missing_ascii_letters(codes: &mut HashMap<char, u16>, fallback: HashMap<char, u16>) {
    for (character, key_code) in fallback {
        if character.is_ascii_alphabetic() {
            codes.entry(character).or_insert(key_code);
        }
    }
}

/// Resolves a character against the ASCII-capable layout for non-Latin
/// active layouts, mirroring how AppKit resolves shortcuts such as Cmd+V.
fn ascii_capable_key_code(character: char) -> Option<u16> {
    if !character.is_ascii_alphabetic() {
        return None;
    }
    ascii_capable_key_codes()?
        .get(&character.to_ascii_lowercase())
        .copied()
}

fn collect_layout_codes(source: &LayoutSource, modifier_state: u32) -> Result<HashMap<char, u16>> {
    let layout = source.layout_bytes()?;
    Ok(collect_key_codes((0..128).filter_map(|key_code| {
        let translated = translate(layout, key_code, modifier_state)?;
        let character = translated.chars().next()?.to_ascii_lowercase();
        Some((character, key_code))
    })))
}

fn collect_key_codes(entries: impl IntoIterator<Item = (char, u16)>) -> HashMap<char, u16> {
    entries
        .into_iter()
        .fold(HashMap::new(), |mut codes, (character, key_code)| {
            codes.entry(character).or_insert(key_code);
            codes
        })
}

pub fn post_command(character: char) -> Result<()> {
    let key_code = resolve_key_code(character, true)?;
    post_key_code(key_code, &[(COMMAND_FLAG, COMMAND_KEY_CODE)], 1)
}

/// Types `text` as synthesized key events, never touching the clipboard. Line
/// breaks are Shift-Return so chat boxes insert a newline instead of sending.
pub fn type_text(text: &str) -> Result<()> {
    let mut events = Vec::new();
    for stroke in typing_strokes(text) {
        match stroke {
            Stroke::Text(units) => {
                for down in [true, false] {
                    let event = KeyboardEvent::new((0, down, 0))?;
                    event.set_unicode(&units);
                    events.push(event);
                }
            }
            Stroke::LineBreak => {
                for spec in
                    shortcut_event_specs(RETURN_KEY_CODE, &[(SHIFT_FLAG, SHIFT_KEY_CODE)], 1)
                {
                    events.push(KeyboardEvent::new(spec)?);
                }
            }
        }
    }
    for event in events {
        event.post();
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum Stroke {
    Text(Vec<u16>),
    LineBreak,
}

/// Splits text into key events of at most `MAX_UNICODE_UNITS` UTF-16 units,
/// the most one event carries, without splitting a surrogate pair. `\r\n`,
/// `\n`, and `\r` each become one line break.
fn typing_strokes(text: &str) -> Vec<Stroke> {
    let mut strokes = Vec::new();
    let mut units = Vec::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if matches!(character, '\r' | '\n') {
            if character == '\r' {
                characters.next_if_eq(&'\n');
            }
            if !units.is_empty() {
                strokes.push(Stroke::Text(std::mem::take(&mut units)));
            }
            strokes.push(Stroke::LineBreak);
            continue;
        }
        let mut encoded = [0; 2];
        let encoded = character.encode_utf16(&mut encoded);
        if units.len() + encoded.len() > MAX_UNICODE_UNITS {
            strokes.push(Stroke::Text(std::mem::take(&mut units)));
        }
        units.extend_from_slice(encoded);
    }
    if !units.is_empty() {
        strokes.push(Stroke::Text(units));
    }
    strokes
}

/// One press of a key that types nothing, under the modifier `flags` held
/// now, so other apps' modifier-tap detectors read the hold as a chord, as
/// they do for any key pressed while the modifier is down. It is posted at
/// the HID tap, so HID and session taps and NSEvent global monitors see it;
/// Hex's annotated-session tap then drops it before the focused app, which
/// would otherwise beep at an unhandled key.
pub fn post_chord_breaker(flags: u64) -> Result<()> {
    let events = chord_breaker_event_specs(flags)
        .map(KeyboardEvent::new)
        .collect::<Result<Vec<_>>>()?;
    for event in events {
        event.post();
    }
    Ok(())
}

fn chord_breaker_event_specs(flags: u64) -> impl Iterator<Item = (u16, bool, u64)> {
    [true, false]
        .into_iter()
        .map(move |down| (CHORD_BREAKER_KEY_CODE, down, flags))
}

fn post_key_code(key_code: u16, modifiers: &[(u64, u16)], count: u8) -> Result<()> {
    let specs = shortcut_event_specs(key_code, modifiers, count);
    let events = specs.map(KeyboardEvent::new).collect::<Result<Vec<_>>>()?;
    for event in events {
        event.post();
    }
    Ok(())
}

fn shortcut_event_specs(
    key_code: u16,
    modifiers: &[(u64, u16)],
    count: u8,
) -> impl Iterator<Item = (u16, bool, u64)> {
    let flags = modifiers.iter().fold(0, |flags, (flag, _)| flags | flag);
    let mut active_flags = 0;
    let mut remaining_flags = flags;
    modifiers
        .iter()
        .map(move |(flag, key)| {
            active_flags |= flag;
            (*key, true, active_flags)
        })
        .chain((0..count).flat_map(move |_| [(key_code, true, flags), (key_code, false, flags)]))
        .chain(modifiers.iter().rev().map(move |(flag, key)| {
            remaining_flags &= !flag;
            (*key, false, remaining_flags)
        }))
}

struct KeyboardEvent(EventRef);

impl KeyboardEvent {
    fn new((key, down, flags): (u16, bool, u64)) -> Result<Self> {
        // SAFETY: Null selects the default event source. The owned event is released in Drop.
        let event = unsafe { CGEventCreateKeyboardEvent(std::ptr::null_mut(), key, down) };
        if event.is_null() {
            return Err(eyre!("could not create keyboard event"));
        }
        unsafe {
            CGEventSetFlags(event, flags);
            CGEventSetIntegerValueField(event, EVENT_SOURCE_USER_DATA, SYNTHETIC_EVENT_MARKER);
        }
        Ok(Self(event))
    }

    fn post(&self) {
        // SAFETY: This value owns a valid CoreGraphics keyboard event.
        unsafe { CGEventPost(HID_EVENT_TAP, self.0) };
    }

    fn set_unicode(&self, text: &[u16]) {
        // SAFETY: The event is valid and Quartz copies the provided UTF-16 buffer.
        unsafe { CGEventKeyboardSetUnicodeString(self.0, text.len(), text.as_ptr()) };
    }
}

impl Drop for KeyboardEvent {
    fn drop(&mut self) {
        // SAFETY: This is the final owner of the retained CoreGraphics event.
        unsafe { CFRelease(self.0.cast_const()) };
    }
}

/// A retained TIS keyboard-layout input source, released on drop.
struct LayoutSource(InputSourceRef);

impl LayoutSource {
    /// The active layout, falling back to the ASCII-capable layout.
    fn current() -> Result<Self> {
        #[cfg(test)]
        INPUT_SOURCE_QUERIES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // SAFETY: TIS copy functions return retained immutable input-source objects.
        let source = unsafe { TISCopyCurrentKeyboardLayoutInputSource() };
        if !source.is_null() {
            return Ok(Self(source));
        }
        Self::ascii_capable().ok_or_else(|| eyre!("no keyboard layout input source is available"))
    }

    fn ascii_capable() -> Option<Self> {
        // SAFETY: TIS copy functions return retained immutable input-source objects.
        let source = unsafe { TISCopyCurrentASCIICapableKeyboardLayoutInputSource() };
        (!source.is_null()).then_some(Self(source))
    }

    /// The UCKeyboardLayout bytes, valid while this source is alive.
    fn layout_bytes(&self) -> Result<*const u8> {
        // SAFETY: The retained source remains alive for every property read.
        let layout_data =
            unsafe { TISGetInputSourceProperty(self.0, kTISPropertyUnicodeKeyLayoutData) };
        if layout_data.is_null() {
            return Err(eyre!("active keyboard layout has no Unicode mapping"));
        }
        // SAFETY: The property is CFData containing a UCKeyboardLayout for this source.
        Ok(unsafe { CFDataGetBytePtr(layout_data) })
    }
}

impl Drop for LayoutSource {
    fn drop(&mut self) {
        // SAFETY: TIS copy functions return a retained source that we own.
        unsafe { CFRelease(self.0) };
    }
}

fn translate(layout: *const u8, key_code: u16, modifier_state: u32) -> Option<String> {
    let mut dead_key_state = 0;
    let mut output = [0_u16; 4];
    let mut length = 0;
    // SAFETY: `layout` points into retained TIS layout data and output buffers are valid.
    let status = unsafe {
        UCKeyTranslate(
            layout,
            key_code,
            KEY_ACTION_DISPLAY,
            modifier_state,
            LMGetKbdType().into(),
            NO_DEAD_KEYS,
            &mut dead_key_state,
            output.len() as isize,
            &mut length,
            output.as_mut_ptr(),
        )
    };
    (status == 0 && length > 0)
        .then(|| String::from_utf16(&output[..length as usize]).ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPTION_FLAG: u64 = 1 << 19;
    const CONTROL_KEY_CODE: u16 = 59;
    const CONTROL_FLAG: u64 = 1 << 18;

    /// Hex's taps let its own keystrokes through by this mark, so a posted
    /// shortcut (the capture hand-off's ⌘V included) never reads as the
    /// user's chord.
    #[test]
    fn posted_keystrokes_carry_hexs_marker() {
        #[link(name = "ApplicationServices", kind = "framework")]
        unsafe extern "C" {
            fn CGEventGetIntegerValueField(event: EventRef, field: u32) -> i64;
        }
        let chord_breaker = chord_breaker_event_specs(OPTION_FLAG).collect::<Vec<_>>();
        assert_eq!(
            chord_breaker,
            [
                (CHORD_BREAKER_KEY_CODE, true, OPTION_FLAG),
                (CHORD_BREAKER_KEY_CODE, false, OPTION_FLAG)
            ]
        );
        for event in
            shortcut_event_specs(9, &[(COMMAND_FLAG, COMMAND_KEY_CODE)], 1).chain(chord_breaker)
        {
            let event = KeyboardEvent::new(event).unwrap();
            // SAFETY: the event is valid until `event` drops.
            let mark = unsafe { CGEventGetIntegerValueField(event.0, EVENT_SOURCE_USER_DATA) };
            assert_eq!(mark, SYNTHETIC_EVENT_MARKER);
        }
    }

    #[test]
    fn typing_splits_text_into_twenty_unit_events_without_splitting_surrogate_pairs() {
        let text = |text: &str| Stroke::Text(text.encode_utf16().collect());
        assert_eq!(typing_strokes(""), []);
        assert_eq!(typing_strokes("hello"), [text("hello")]);
        assert_eq!(
            typing_strokes("abcdefghijklmnopqrstuvwxyz"),
            [text("abcdefghijklmnopqrst"), text("uvwxyz")]
        );
        // 19 units, then a surrogate pair that would make 21, moves whole.
        assert_eq!(
            typing_strokes("abcdefghijklmnopqrs😀z"),
            [text("abcdefghijklmnopqrs"), text("😀z")]
        );
        assert_eq!(
            typing_strokes("😀😀😀😀😀😀😀😀😀😀😀"),
            [text("😀😀😀😀😀😀😀😀😀😀"), text("😀")]
        );
        for stroke in typing_strokes(&"é漢😀a".repeat(40)) {
            let Stroke::Text(units) = stroke else {
                panic!("unexpected line break");
            };
            assert!(units.len() <= MAX_UNICODE_UNITS);
            assert!(String::from_utf16(&units).is_ok());
        }
    }

    #[test]
    fn typing_sends_each_line_break_as_one_shift_return() {
        let text = |text: &str| Stroke::Text(text.encode_utf16().collect());
        assert_eq!(
            typing_strokes("one\ntwo\r\nthree\rfour\n\n"),
            [
                text("one"),
                Stroke::LineBreak,
                text("two"),
                Stroke::LineBreak,
                text("three"),
                Stroke::LineBreak,
                text("four"),
                Stroke::LineBreak,
                Stroke::LineBreak,
            ]
        );
        assert_eq!(
            shortcut_event_specs(RETURN_KEY_CODE, &[(SHIFT_FLAG, SHIFT_KEY_CODE)], 1)
                .collect::<Vec<_>>(),
            [
                (SHIFT_KEY_CODE, true, SHIFT_FLAG),
                (RETURN_KEY_CODE, true, SHIFT_FLAG),
                (RETURN_KEY_CODE, false, SHIFT_FLAG),
                (SHIFT_KEY_CODE, false, 0),
            ]
        );
    }

    #[test]
    fn non_latin_snapshot_gets_shortcut_letters_without_replacing_active_keys() {
        let mut codes = collect_key_codes([('ж', 41), ('v', 12)]);
        let fallback = collect_key_codes([('v', 9), ('c', 8), ('c', 88), ('1', 18)]);
        fill_missing_ascii_letters(&mut codes, fallback);
        assert_eq!(codes.get(&'ж'), Some(&41));
        assert_eq!(codes.get(&'v'), Some(&12));
        assert_eq!(codes.get(&'c'), Some(&8));
        assert!(!codes.contains_key(&'1'));

        let mut non_latin = collect_key_codes([('מ', 9)]);
        fill_missing_ascii_letters(&mut non_latin, collect_key_codes([('v', 9)]));
        assert_eq!(non_latin.get(&'v'), Some(&9));
        assert_eq!(non_latin.get(&'מ'), Some(&9));
    }

    #[test]
    fn command_shortcuts_resolve_against_the_command_layer() {
        // "Dvorak – QWERTY ⌘": V is keycode 47 normally but keycode 9 with Command.
        let dvorak_qwerty_command = LayoutCodes {
            plain: collect_key_codes([('v', 47), ('.', 9)]),
            command: collect_key_codes([('v', 9), ('.', 47)]),
        };
        assert_eq!(dvorak_qwerty_command.key_code('v', true), Some(9));
        assert_eq!(dvorak_qwerty_command.key_code('V', true), Some(9));
        assert_eq!(dvorak_qwerty_command.key_code('v', false), Some(47));

        let qwerty = LayoutCodes {
            plain: collect_key_codes([('v', 9)]),
            command: collect_key_codes([('v', 9)]),
        };
        assert_eq!(qwerty.key_code('v', true), Some(9));
        assert_eq!(qwerty.key_code('v', false), Some(9));

        // A Command layer without Latin letters falls back to the plain snapshot,
        // which non-Latin layouts fill from the ASCII-capable layout.
        let non_latin = LayoutCodes {
            plain: collect_key_codes([('м', 9), ('v', 9)]),
            command: collect_key_codes([('м', 9)]),
        };
        assert_eq!(non_latin.key_code('v', true), Some(9));
        assert_eq!(non_latin.key_code('x', true), None);
    }

    #[test]
    fn layout_cache_prefers_the_first_key_that_produces_a_character() {
        let codes = collect_key_codes([('2', 19), ('2', 84)]);

        assert_eq!(codes.get(&'2'), Some(&19));
    }

    #[test]
    fn shortcut_modifier_events_track_physical_flag_transitions() {
        let events =
            shortcut_event_specs(19, &[(CONTROL_FLAG, CONTROL_KEY_CODE)], 1).collect::<Vec<_>>();

        assert_eq!(
            events,
            vec![
                (CONTROL_KEY_CODE, true, CONTROL_FLAG),
                (19, true, CONTROL_FLAG),
                (19, false, CONTROL_FLAG),
                (CONTROL_KEY_CODE, false, 0),
            ]
        );
    }
}

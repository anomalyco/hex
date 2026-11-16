use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Mutex, RwLock};

use color_eyre::eyre::{Result, eyre};

type InputSourceRef = *const c_void;
type EventRef = *mut c_void;

const KEY_ACTION_DISPLAY: u16 = 3;
/// The no-dead-keys option clear: a dead key starts a dead-key state and
/// reports no character, which is what the layout snapshot wants for the
/// character a key types.
const NO_DEAD_KEYS: isize = 0;
/// `kUCKeyTranslateNoDeadKeysMask` (bit 0): a dead key reports its base
/// character (`^`, `´`, `` ` ``) instead of starting a dead-key state.
const KEY_TRANSLATE_NO_DEAD_KEYS: isize = 1;
const NO_MODIFIER_STATE: u32 = 0;
/// Carbon `cmdKey >> 8`, the UCKeyTranslate modifier state for the Command layer.
const COMMAND_MODIFIER_STATE: u32 = 1;
/// Carbon `shiftKey >> 8`, the UCKeyTranslate modifier state for the Shift layer.
const SHIFT_MODIFIER_STATE: u32 = 2;
const HID_EVENT_TAP: u32 = 0;
const COMMAND_KEY_CODE: u16 = 55;
const COMMAND_FLAG: u64 = 1 << 20;
const SHIFT_KEY_CODE: u16 = 56;
const SHIFT_FLAG: u64 = 1 << 17;
const RETURN_KEY_CODE: u16 = 36;
/// Right ⌘ through Fn: keys a real keyboard reports as flags-changed events.
const MODIFIER_KEY_CODES: std::ops::RangeInclusive<u16> = 54..=63;
const FLAGS_CHANGED_EVENT: u32 = 12;
/// Pause between a posted shortcut's events. Remote desktop and VM clients
/// track modifiers from their own events and can read a burst posted in one
/// instant as the bare key, typing "v".
const SHORTCUT_EVENT_GAP: std::time::Duration = std::time::Duration::from_millis(15);
/// Pause between a paced typing's events, so a client that takes each key from
/// its own event stream has time to forward one before the next arrives.
const PACED_TYPING_GAP: std::time::Duration = std::time::Duration::from_millis(4);
/// Quartz keeps at most 20 UTF-16 units of a keyboard event's Unicode string.
const MAX_UNICODE_UNITS: usize = 20;
const EVENT_SOURCE_USER_DATA: u32 = 42;
/// A hole in the virtual key codes, between keypad `*` (0x43) and keypad `+`
/// (0x45): no keyboard sends it, and of the 264 layouts macOS ships only
/// Unicode Hex Input maps it (to U+0000, never with Option alone). Unlike
/// F13–F20 or the JIS keys it has no system action and nothing binds it.
pub const CHORD_BREAKER_KEY_CODE: u16 = 0x44;
pub const SYNTHETIC_EVENT_MARKER: i64 = 0x0056_4f49_4345;
static KEY_CODES: LayoutSnapshot = LayoutSnapshot::new();
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
    fn CGEventSetType(event: EventRef, event_type: u32);
    fn CGEventKeyboardSetUnicodeString(event: EventRef, length: usize, text: *const u16);
    fn CGEventSetIntegerValueField(event: EventRef, field: u32, value: i64);
    fn CGEventPost(tap: u32, event: EventRef);
}

/// Character key codes for the active layout's plain, Shift, and Command
/// layers. Layouts such as "Dvorak – QWERTY ⌘" switch to a different
/// arrangement while Command is held, so Command shortcuts must resolve
/// against that layer; typing a character has to know whether the layout
/// types it with Shift.
#[derive(Debug, PartialEq)]
struct LayoutCodes {
    plain: HashMap<char, u16>,
    shift: HashMap<char, u16>,
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

    /// The key that types `character` with no Command held, and whether it
    /// needs Shift. Case matters here: `A` needs Shift where `a` does not.
    fn typing_key(&self, character: char) -> Option<(u16, bool)> {
        match self.plain.get(&character) {
            Some(&key_code) => Some((key_code, false)),
            None => self.shift.get(&character).map(|&key_code| (key_code, true)),
        }
    }
}

/// The published layout codes. The main thread replaces them whole when the
/// selected input source changes; any thread reads the current ones without
/// touching TIS, and sees either the old codes or the new, never a mix.
struct LayoutSnapshot(RwLock<Option<Arc<LayoutCodes>>>);

impl LayoutSnapshot {
    const fn new() -> Self {
        Self(RwLock::new(None))
    }

    fn get(&self) -> Option<Arc<LayoutCodes>> {
        self.0
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    /// Publishes `codes`; returns whether they differ from the current ones.
    fn publish(&self, codes: LayoutCodes) -> bool {
        let mut current = self.0.write().unwrap_or_else(|error| error.into_inner());
        if current.as_deref() == Some(&codes) {
            return false;
        }
        *current = Some(Arc::new(codes));
        true
    }
}

/// The key that produces `character` on the active layout's plain layer.
pub fn key_code_for(character: char) -> Result<u16> {
    resolve_key_code(character, false)
}

/// The key that produces `character` in a shortcut, on the Command layer
/// when the shortcut holds Command.
#[cfg_attr(test, allow(dead_code))]
pub fn shortcut_key_code(character: char, command: bool) -> Result<u16> {
    resolve_key_code(character, command)
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
    // Headless callers retain live-layout resolution. GUI callers publish the
    // snapshot on the main thread; even a cache miss then returns without
    // entering TIS.
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

/// The base character `code` types on the active layout, translated with the
/// no-dead-keys option so a dead key names itself. The shortcut recorder
/// falls back to it when AppKit gives no character for a key: on a German
/// layout the key left of 1 (ISO section, virtual key code 10) is `^`.
///
/// Enters TIS, so only the main thread or a headless CLI process may call it.
pub fn key_label(code: u16) -> Option<String> {
    with_layout(|layout| {
        translate_with_options(layout, code, NO_MODIFIER_STATE, KEY_TRANSLATE_NO_DEAD_KEYS)
    })
    .flatten()
}

/// The key whose base character is `character` on the active layout,
/// translated with the no-dead-keys option: `^` names the ISO section key on
/// a German layout, where AppKit reports no character for the dead key.
///
/// Enters TIS, so only the main thread or a headless CLI process may call it.
pub fn key_code_for_base_label(character: char) -> Option<u16> {
    let mut target = [0; 4];
    let target = character.encode_utf8(&mut target);
    with_layout(|layout| {
        (0..128).find(|&key_code| {
            translate_with_options(
                layout,
                key_code,
                NO_MODIFIER_STATE,
                KEY_TRANSLATE_NO_DEAD_KEYS,
            )
            .is_some_and(|translated| translated.eq_ignore_ascii_case(target))
        })
    })
    .flatten()
}

/// Runs `read` against the active layout's bytes under the layout lock, as
/// every other native layout lookup does.
///
/// Enters TIS, so only the main thread or a headless CLI process may call it.
fn with_layout<T>(read: impl FnOnce(*const u8) -> T) -> Option<T> {
    let _access = LAYOUT_ACCESS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let source = LayoutSource::current().ok()?;
    let layout = source.layout_bytes().ok()?;
    Some(read(layout))
}

/// The character `code` types with Command held on the active layout, read
/// from the published snapshot only (never TIS): a capture's key log names
/// keys with it.
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

/// GUI startup must build this snapshot on the main thread before starting
/// workers; [`refresh_layout`] rebuilds it there when the selected input
/// source changes. Lookups, including misses, never re-enter TIS/TSM from a
/// worker thread.
pub fn initialize_layout() -> Result<()> {
    if KEY_CODES.get().is_some() {
        return Ok(());
    }
    let _access = LAYOUT_ACCESS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if KEY_CODES.get().is_none() {
        KEY_CODES.publish(read_layout()?);
    }
    Ok(())
}

/// Rebuilds the snapshot from the now-selected input source, on the main
/// thread, and publishes it whole. Returns whether any key moved.
pub fn refresh_layout(_mtm: objc2::MainThreadMarker) -> Result<bool> {
    let _access = LAYOUT_ACCESS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    Ok(KEY_CODES.publish(read_layout()?))
}

fn read_layout() -> Result<LayoutCodes> {
    let source = LayoutSource::current()?;
    let mut codes = collect_layout_codes(&source, NO_MODIFIER_STATE)?;
    let shift = collect_shifted_key_codes(&source).unwrap_or_default();
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
    Ok(LayoutCodes {
        plain: codes,
        shift,
        command,
    })
}

fn ascii_capable_key_codes() -> Option<HashMap<char, u16>> {
    // Called only while LAYOUT_ACCESS is held: on the main thread for the GUI
    // snapshot, or by a headless caller without one.
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

/// The characters keys type with Shift held, spelled as they type (`A`, `!`)
/// rather than folded to lowercase: typing an uppercase letter or a shifted
/// symbol looks the character up as it stands.
fn collect_shifted_key_codes(source: &LayoutSource) -> Result<HashMap<char, u16>> {
    let layout = source.layout_bytes()?;
    Ok(collect_key_codes((0..128).filter_map(|key_code| {
        let translated = translate(layout, key_code, SHIFT_MODIFIER_STATE)?;
        Some((translated.chars().next()?, key_code))
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

/// One press of Return with no modifier held, as a person sends a message.
pub fn post_return() -> Result<()> {
    post_key_code(RETURN_KEY_CODE, &[], 1)
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

/// Types `text` as synthesized key events, never touching the clipboard, one
/// character at a time and paced so a client that forwards each key from its
/// own event stream keeps them all. Unlike [`type_text`], each character also
/// carries the key the active layout types it with, so a client that reads
/// scancodes gets the right key, and Shift goes down and up around a character
/// the layout needs it for. Line breaks are Shift-Return, as `type_text`
/// sends them.
///
/// `keep_going` is asked before each character's events. Typing can take
/// seconds, so a caller watches the app in front with it; when it says stop,
/// nothing further is posted, no character is left half posted, and the text
/// already typed stays typed. Every event is built before the first posts, so
/// a creation failure posts nothing.
pub fn type_text_paced(text: &str, keep_going: impl FnMut() -> bool) -> Result<()> {
    let groups = paced_typing_groups(text, typing_key)
        .into_iter()
        .map(|group| {
            group
                .into_iter()
                .map(|event| {
                    let created = KeyboardEvent::new((event.key_code, event.down, event.flags))?;
                    if let Some(character) = event.typed {
                        let mut units = [0; 2];
                        created.set_unicode(character.encode_utf16(&mut units));
                    }
                    Ok(created)
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    let completed = post_paced_typing(&groups, keep_going, KeyboardEvent::post, || {
        std::thread::sleep(PACED_TYPING_GAP)
    });
    if !completed {
        return Err(eyre!("typing stopped before the text was finished"));
    }
    Ok(())
}

/// Posts `groups` in order, every event after the first `wait` apart, and
/// asks `keep_going` before each group. A group is one character's events, so
/// a stop never leaves a Shift down without its up. Returns whether every
/// group was posted.
fn post_paced_typing<E>(
    groups: &[Vec<E>],
    mut keep_going: impl FnMut() -> bool,
    mut post: impl FnMut(&E),
    mut wait: impl FnMut(),
) -> bool {
    let mut posted = 0;
    for group in groups {
        if !keep_going() {
            return false;
        }
        for event in group {
            if posted > 0 {
                wait();
            }
            posted += 1;
            post(event);
        }
    }
    true
}

/// The key that types `character` on the active layout, and whether it needs
/// Shift. Read from the published snapshot only, so a worker thread never
/// enters TIS; a process without one (a headless CLI) types every character
/// from its Unicode string alone, as [`type_text`] does.
fn typing_key(character: char) -> Option<(u16, bool)> {
    KEY_CODES.get()?.typing_key(character)
}

/// One key event of a paced typing: its key, its edge, the modifiers held,
/// and, on the key's own down and up, the character it types.
#[derive(Debug, PartialEq, Eq)]
struct PacedTypingEvent {
    key_code: u16,
    down: bool,
    flags: u64,
    typed: Option<char>,
}

/// Splits `text` into one group of key events per character: its key down/up
/// pair, each carrying the key `key_for` resolves for it, with Shift as its
/// own flags-changed events when that key needs it. A character no key types
/// keeps key code 0 and its Unicode string alone. `\r\n`, `\n`, and `\r` each
/// become one Shift-Return group.
fn paced_typing_groups(
    text: &str,
    key_for: impl Fn(char) -> Option<(u16, bool)>,
) -> Vec<Vec<PacedTypingEvent>> {
    let mut groups = Vec::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if matches!(character, '\r' | '\n') {
            if character == '\r' {
                characters.next_if_eq(&'\n');
            }
            let modifiers = [(SHIFT_FLAG, SHIFT_KEY_CODE)];
            groups.push(
                shortcut_event_specs(RETURN_KEY_CODE, &modifiers, 1)
                    .map(|(key_code, down, flags)| PacedTypingEvent {
                        key_code,
                        down,
                        flags,
                        typed: None,
                    })
                    .collect(),
            );
            continue;
        }
        let (key_code, shift) = key_for(character).unwrap_or((0, false));
        let modifiers: &[(u64, u16)] = if shift {
            &[(SHIFT_FLAG, SHIFT_KEY_CODE)]
        } else {
            &[]
        };
        groups.push(
            shortcut_event_specs(key_code, modifiers, 1)
                .map(|(key, down, flags)| PacedTypingEvent {
                    key_code: key,
                    down,
                    flags,
                    typed: (key == key_code).then_some(character),
                })
                .collect(),
        );
    }
    groups
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
    for (index, event) in events.iter().enumerate() {
        if index > 0 {
            std::thread::sleep(SHORTCUT_EVENT_GAP);
        }
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
            // A real keyboard reports a modifier key as flags-changed, never
            // key down or up; clients that track modifiers from those events
            // (remote desktops, VMs) otherwise miss the held ⌘.
            if MODIFIER_KEY_CODES.contains(&key) {
                CGEventSetType(event, FLAGS_CHANGED_EVENT);
            }
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
    translate_with_options(layout, key_code, modifier_state, NO_DEAD_KEYS)
}

fn translate_with_options(
    layout: *const u8,
    key_code: u16,
    modifier_state: u32,
    options: isize,
) -> Option<String> {
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
            options,
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

    /// Remote desktop clients (#1048) read ⌘ from flags-changed events, as a
    /// real keyboard sends them; the shortcut's own key stays key down/up.
    #[test]
    fn posted_modifiers_are_flags_changed_events() {
        #[link(name = "ApplicationServices", kind = "framework")]
        unsafe extern "C" {
            fn CGEventGetType(event: EventRef) -> u32;
        }
        const KEY_DOWN: u32 = 10;
        const KEY_UP: u32 = 11;
        let types = shortcut_event_specs(9, &[(COMMAND_FLAG, COMMAND_KEY_CODE)], 1)
            .map(|spec| {
                let event = KeyboardEvent::new(spec).unwrap();
                // SAFETY: the event is valid until `event` drops.
                unsafe { CGEventGetType(event.0) }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            types,
            [FLAGS_CHANGED_EVENT, KEY_DOWN, KEY_UP, FLAGS_CHANGED_EVENT]
        );
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

    /// One paced typing event as the assertions below read it: the key, its
    /// edge, the modifiers held, and the character it types.
    type PacedEvent = (u16, bool, u64, Option<char>);

    /// A layout's typing groups, as the assertions below read them.
    fn paced(text: &str, key_for: impl Fn(char) -> Option<(u16, bool)>) -> Vec<Vec<PacedEvent>> {
        paced_typing_groups(text, key_for)
            .into_iter()
            .map(|group| {
                group
                    .into_iter()
                    .map(|event| (event.key_code, event.down, event.flags, event.typed))
                    .collect()
            })
            .collect()
    }

    /// Each character is one group: a key down/up pair carrying its own
    /// Unicode string and, when its key needs Shift, the layout's Shift key as
    /// flags-changed events around it. A character no key types keeps key code
    /// 0 and its Unicode string alone, a surrogate pair whole in one group.
    #[test]
    fn paced_typing_sends_one_key_pair_per_character() {
        let layout = |character| match character {
            'v' => Some((9, false)),
            'A' => Some((0, true)),
            _ => None,
        };
        assert_eq!(
            paced("vA", layout),
            vec![
                vec![(9, true, 0, Some('v')), (9, false, 0, Some('v'))],
                vec![
                    (SHIFT_KEY_CODE, true, SHIFT_FLAG, None),
                    (0, true, SHIFT_FLAG, Some('A')),
                    (0, false, SHIFT_FLAG, Some('A')),
                    (SHIFT_KEY_CODE, false, 0, None),
                ],
            ]
        );
        assert_eq!(
            paced("😀", |_| None),
            vec![vec![(0, true, 0, Some('😀')), (0, false, 0, Some('😀'))]]
        );
    }

    #[test]
    fn paced_typing_sends_each_line_break_as_one_shift_return() {
        assert_eq!(
            paced("\r\n", |_| Some((9, false))),
            vec![vec![
                (SHIFT_KEY_CODE, true, SHIFT_FLAG, None),
                (RETURN_KEY_CODE, true, SHIFT_FLAG, None),
                (RETURN_KEY_CODE, false, SHIFT_FLAG, None),
                (SHIFT_KEY_CODE, false, 0, None),
            ]]
        );
    }

    /// Typing can take seconds, so a caller can stop it between characters:
    /// whole groups only, so a Shift down is never left without its up, and
    /// nothing posts once the callback says stop.
    #[test]
    fn paced_typing_stops_between_characters() {
        let groups = vec![vec![1_u8, 2], vec![3, 4], vec![5]];
        let typed = std::cell::Cell::new(0_usize);
        let mut posts = Vec::new();
        let mut waits = 0;
        let completed = post_paced_typing(
            &groups,
            || typed.get() < 2,
            |event| {
                typed.set(typed.get() + 1);
                posts.push(*event);
            },
            || waits += 1,
        );
        assert!(!completed);
        assert_eq!(posts, [1, 2], "the second character never starts");
        assert_eq!(waits, 1, "a wait between every post after the first");

        let asked = std::cell::Cell::new(0_usize);
        let mut posts = Vec::new();
        let mut waits = 0;
        assert!(!post_paced_typing(
            &groups,
            || {
                asked.set(asked.get() + 1);
                false
            },
            |event| posts.push(*event),
            || waits += 1,
        ));
        assert!(
            posts.is_empty(),
            "a stop before the first character types nothing"
        );
        assert_eq!(asked.get(), 1, "asked once, before the first character");
        assert_eq!(waits, 0);

        let mut posts = Vec::new();
        let mut waits = 0;
        assert!(post_paced_typing(
            &groups,
            || true,
            |event| posts.push(*event),
            || waits += 1,
        ));
        assert_eq!(posts, [1, 2, 3, 4, 5]);
        assert_eq!(waits, 4);
    }

    /// Typing resolves a character exactly as it stands: `a` and `A` are the
    /// same key with Shift between them, and a symbol only a shifted key types
    /// needs Shift too. The Command layer never matters to typing.
    #[test]
    fn typing_resolves_the_key_and_whether_it_needs_shift() {
        let codes = LayoutCodes {
            plain: collect_key_codes([('a', 0), ('1', 18)]),
            shift: collect_key_codes([('A', 0), ('!', 18)]),
            command: collect_key_codes([('a', 47)]),
        };
        assert_eq!(codes.typing_key('a'), Some((0, false)));
        assert_eq!(codes.typing_key('A'), Some((0, true)));
        assert_eq!(codes.typing_key('1'), Some((18, false)));
        assert_eq!(codes.typing_key('!'), Some((18, true)));
        assert_eq!(codes.typing_key('?'), None);
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
            shift: collect_key_codes([('V', 47)]),
            command: collect_key_codes([('v', 9), ('.', 47)]),
        };
        assert_eq!(dvorak_qwerty_command.key_code('v', true), Some(9));
        assert_eq!(dvorak_qwerty_command.key_code('V', true), Some(9));
        assert_eq!(dvorak_qwerty_command.key_code('v', false), Some(47));

        let qwerty = LayoutCodes {
            plain: collect_key_codes([('v', 9)]),
            shift: collect_key_codes([('V', 9)]),
            command: collect_key_codes([('v', 9)]),
        };
        assert_eq!(qwerty.key_code('v', true), Some(9));
        assert_eq!(qwerty.key_code('v', false), Some(9));

        // A Command layer without Latin letters falls back to the plain snapshot,
        // which non-Latin layouts fill from the ASCII-capable layout.
        let non_latin = LayoutCodes {
            plain: collect_key_codes([('м', 9), ('v', 9)]),
            shift: collect_key_codes([('М', 9)]),
            command: collect_key_codes([('м', 9)]),
        };
        assert_eq!(non_latin.key_code('v', true), Some(9));
        assert_eq!(non_latin.key_code('x', true), None);
    }

    fn dvorak() -> LayoutCodes {
        LayoutCodes {
            plain: collect_key_codes([('v', 47), ('.', 9)]),
            shift: collect_key_codes([('V', 47)]),
            command: collect_key_codes([('v', 47), ('.', 9)]),
        }
    }

    fn us() -> LayoutCodes {
        LayoutCodes {
            plain: collect_key_codes([('v', 9), ('.', 47)]),
            shift: collect_key_codes([('V', 9)]),
            command: collect_key_codes([('v', 9), ('.', 47)]),
        }
    }

    /// Launched on Dvorak, then switched to U.S. (t1304): ⌘V must post the
    /// U.S. V key once the main thread publishes the new layout.
    #[test]
    fn a_published_layout_replaces_the_one_before() {
        let snapshot = LayoutSnapshot::new();
        assert!(snapshot.get().is_none());
        assert!(snapshot.publish(dvorak()));
        assert_eq!(snapshot.get().unwrap().key_code('v', true), Some(47));
        assert!(
            !snapshot.publish(dvorak()),
            "the same layout changes nothing"
        );
        assert!(snapshot.publish(us()));
        assert_eq!(snapshot.get().unwrap().key_code('v', true), Some(9));
        assert!(snapshot.publish(dvorak()));
        assert_eq!(snapshot.get().unwrap().key_code('v', true), Some(47));
    }

    /// Workers read while the main thread publishes: each read is the old
    /// layout or the new one, never a mix of the two.
    #[test]
    fn readers_see_a_whole_layout_while_it_is_replaced() {
        let snapshot = Arc::new(LayoutSnapshot::new());
        snapshot.publish(dvorak());
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let readers = (0..4)
            .map(|_| {
                let snapshot = snapshot.clone();
                let stop = stop.clone();
                std::thread::spawn(move || {
                    let (dvorak, us) = (dvorak(), us());
                    let mut reads = 0_u64;
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) || reads == 0 {
                        let codes = snapshot.get().expect("published before reading");
                        assert!(*codes == dvorak || *codes == us, "torn layout: {codes:?}");
                        reads += 1;
                    }
                })
            })
            .collect::<Vec<_>>();
        for round in 0..2_000 {
            snapshot.publish(if round % 2 == 0 { us() } else { dvorak() });
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for reader in readers {
            reader.join().unwrap();
        }
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

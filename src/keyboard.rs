use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::OnceLock;

use color_eyre::eyre::{Result, eyre};

#[derive(Clone, Copy, Debug)]
pub enum Key {
    Character(char),
    Home,
    End,
    Up,
    Down,
    Left,
    Right,
    Enter,
    #[allow(dead_code)]
    Escape,
}

#[derive(Clone, Copy, Debug)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const NONE: Self = Self(0);
    pub const COMMAND: Self = Self(1 << 0);
    pub const SHIFT: Self = Self(1 << 1);
    pub const OPTION: Self = Self(1 << 2);
    pub const CONTROL: Self = Self(1 << 3);

    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

type InputSourceRef = *const c_void;
type EventRef = *mut c_void;

const KEY_ACTION_DISPLAY: u16 = 3;
const NO_DEAD_KEYS: isize = 0;
const HID_EVENT_TAP: u32 = 0;
const COMMAND_KEY_CODE: u16 = 55;
const COMMAND_FLAG: u64 = 1 << 20;
const SHIFT_KEY_CODE: u16 = 56;
const SHIFT_FLAG: u64 = 1 << 17;
const OPTION_KEY_CODE: u16 = 58;
const OPTION_FLAG: u64 = 1 << 19;
const CONTROL_KEY_CODE: u16 = 59;
const CONTROL_FLAG: u64 = 1 << 18;
const EVENT_SOURCE_USER_DATA: u32 = 42;
pub const SYNTHETIC_EVENT_MARKER: i64 = 0x0056_4f49_4345;
static KEY_CODES: OnceLock<HashMap<char, u16>> = OnceLock::new();

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

pub fn key_code_for(character: char) -> Result<u16> {
    if let Some(key_code) = KEY_CODES
        .get()
        .and_then(|codes| codes.get(&character.to_ascii_lowercase()))
    {
        return Ok(*key_code);
    }
    let source = input_source()?;
    // SAFETY: The retained TIS source remains alive until after all property reads.
    let layout_data =
        unsafe { TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) };
    if layout_data.is_null() {
        unsafe { CFRelease(source) };
        return Err(eyre!("active keyboard layout has no Unicode mapping"));
    }
    // SAFETY: The property is CFData containing a UCKeyboardLayout for this source.
    let layout = unsafe { CFDataGetBytePtr(layout_data) };
    let result = (0..128).find(|&key_code| {
        translate(layout, key_code)
            .is_some_and(|translated| translated.eq_ignore_ascii_case(&character.to_string()))
    });
    // SAFETY: TIS copy functions return a retained source.
    unsafe { CFRelease(source) };
    result.ok_or_else(|| eyre!("current keyboard layout has no key for {character:?}"))
}

pub fn initialize_layout() -> Result<()> {
    if KEY_CODES.get().is_some() {
        return Ok(());
    }
    let source = input_source()?;
    let layout_data =
        unsafe { TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) };
    if layout_data.is_null() {
        unsafe { CFRelease(source) };
        return Err(eyre!("active keyboard layout has no Unicode mapping"));
    }
    let layout = unsafe { CFDataGetBytePtr(layout_data) };
    let codes = (0..128)
        .filter_map(|key_code| {
            let translated = translate(layout, key_code)?;
            let character = translated.chars().next()?.to_ascii_lowercase();
            Some((character, key_code))
        })
        .collect();
    unsafe { CFRelease(source) };
    let _ = KEY_CODES.set(codes);
    Ok(())
}

pub fn post_command(character: char) -> Result<()> {
    post_shortcut(Key::Character(character), Modifiers::COMMAND)
}

pub fn post_enter() -> Result<()> {
    post_shortcut(Key::Enter, Modifiers::NONE)
}

pub fn type_text(text: &str) -> Result<()> {
    for character in text.chars() {
        let mut encoded = [0; 2];
        let encoded = character.encode_utf16(&mut encoded);
        let down = KeyboardEvent::new((0, true, 0))?;
        let up = KeyboardEvent::new((0, false, 0))?;
        down.set_unicode(encoded);
        up.set_unicode(encoded);
        down.post();
        up.post();
    }
    Ok(())
}

pub fn post_shortcut(key: Key, modifiers: Modifiers) -> Result<()> {
    post_repeated_shortcut(key, modifiers, 1)
}

pub fn post_repeated_shortcut(key: Key, modifiers: Modifiers, count: u8) -> Result<()> {
    if count == 0 {
        return Ok(());
    }
    let key_code = match key {
        Key::Character(character) => key_code_for(character)?,
        Key::Home => 115,
        Key::End => 119,
        Key::Up => 126,
        Key::Down => 125,
        Key::Left => 123,
        Key::Right => 124,
        Key::Enter => 36,
        Key::Escape => 53,
    };
    let modifiers = [
        (Modifiers::COMMAND, COMMAND_FLAG, COMMAND_KEY_CODE),
        (Modifiers::SHIFT, SHIFT_FLAG, SHIFT_KEY_CODE),
        (Modifiers::OPTION, OPTION_FLAG, OPTION_KEY_CODE),
        (Modifiers::CONTROL, CONTROL_FLAG, CONTROL_KEY_CODE),
    ]
    .into_iter()
    .filter_map(|(modifier, flag, key)| modifiers.contains(modifier).then_some((flag, key)))
    .collect::<Vec<_>>();
    post_key_code(key_code, &modifiers, count)
}

fn post_key_code(key_code: u16, modifiers: &[(u64, u16)], count: u8) -> Result<()> {
    let flags = modifiers.iter().fold(0, |flags, (flag, _)| flags | flag);
    let specs = modifiers
        .iter()
        .map(|(_, key)| (*key, true, 0))
        .chain((0..count).flat_map(|_| [(key_code, true, flags), (key_code, false, flags)]))
        .chain(modifiers.iter().rev().map(|(_, key)| (*key, false, 0)));
    let events = specs.map(KeyboardEvent::new).collect::<Result<Vec<_>>>()?;
    for event in events {
        event.post();
    }
    Ok(())
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

fn input_source() -> Result<InputSourceRef> {
    // SAFETY: TIS copy functions return retained immutable input-source objects.
    let source = unsafe { TISCopyCurrentKeyboardLayoutInputSource() };
    if !source.is_null() {
        return Ok(source);
    }
    let fallback = unsafe { TISCopyCurrentASCIICapableKeyboardLayoutInputSource() };
    (!fallback.is_null())
        .then_some(fallback)
        .ok_or_else(|| eyre!("no keyboard layout input source is available"))
}

fn translate(layout: *const u8, key_code: u16) -> Option<String> {
    let mut dead_key_state = 0;
    let mut output = [0_u16; 4];
    let mut length = 0;
    // SAFETY: `layout` points into retained TIS layout data and output buffers are valid.
    let status = unsafe {
        UCKeyTranslate(
            layout,
            key_code,
            KEY_ACTION_DISPLAY,
            0,
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

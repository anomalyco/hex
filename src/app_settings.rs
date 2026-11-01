use std::fs;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::sync::{OnceLock, RwLock};

use color_eyre::eyre::{Result, eyre};
#[cfg(target_os = "macos")]
use objc2::MainThreadMarker;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use serde::{Deserialize, Serialize};

use crate::transcription::models::{TranscriptionModelId, TranscriptionSelection};

mod persistence;

static RECORDING_DUCK_VOLUME: AtomicU32 = AtomicU32::new(0.2_f32.to_bits());
static RECORDING_AUDIO_BEHAVIOR: AtomicU8 = AtomicU8::new(0);
static DOUBLE_TAP_LOCK: AtomicBool = AtomicBool::new(true);
static DOUBLE_TAP_ONLY: AtomicBool = AtomicBool::new(false);
static SHORTCUT_TOGGLE: AtomicBool = AtomicBool::new(false);
static MEETINGS_ENABLED: AtomicBool = AtomicBool::new(false);
static PAUSE_BROWSER_MEDIA: AtomicBool = AtomicBool::new(false);
static HEX_MICROPHONE_ENABLED: AtomicBool = AtomicBool::new(false);
static MEETING_NOTES_ENABLED: AtomicBool = AtomicBool::new(true);
static MEETING_NOTES_OPENCODE: AtomicBool = AtomicBool::new(false);
static MEETING_INDICATOR_ENABLED: AtomicBool = AtomicBool::new(true);
static MEETING_INDICATOR_CORNER: AtomicU8 = AtomicU8::new(0);
/// [`MeetingRetention::encode`]d; [`RETENTION_UNSET`] until settings apply.
static MEETING_RETENTION: AtomicU8 = AtomicU8::new(RETENTION_UNSET);
const RETENTION_UNSET: u8 = u8::MAX;
static MEETING_OUTPUT_FOLDER: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();
static MEETING_PROVIDERS: OnceLock<RwLock<Vec<MeetingProviderChoice>>> = OnceLock::new();
static CALENDAR_SETTINGS: OnceLock<RwLock<CalendarSettings>> = OnceLock::new();
static MICROPHONE_POLICY: AtomicU8 = AtomicU8::new(0);
static CUSTOM_TRANSFORMATIONS_ENABLED: AtomicBool = AtomicBool::new(false);
static INDICATOR_POSITION: AtomicU8 = AtomicU8::new(0);
static HOTKEYS: OnceLock<RwLock<RuntimeHotkeys>> = OnceLock::new();
#[cfg(target_os = "macos")]
static PASTE_KEY_CODE: OnceLock<u16> = OnceLock::new();
static HOTKEY_CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Distinct names for the tests' settings folders.
#[cfg(test)]
static SETTINGS_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static TRANSCRIPTION_SELECTION: OnceLock<RwLock<RuntimeTranscriptionSelection>> = OnceLock::new();
static MICROPHONE_SELECTION: OnceLock<RwLock<RuntimeMicrophoneSelection>> = OnceLock::new();
#[cfg(hex_capture)]
static CAPTURE_SETTINGS: OnceLock<RwLock<CaptureSettings>> = OnceLock::new();
#[cfg(hex_capture)]
static SCRAWL_APP_RUNNING: AtomicBool = AtomicBool::new(false);
#[cfg(hex_capture)]
static CAPTURE_ON: AtomicBool = AtomicBool::new(false);
/// [`CaptureSettings::capsule_flow`] for the capsule, without a lock.
#[cfg(hex_capture)]
static CAPSULE_FLOW: AtomicU8 = AtomicU8::new(0);

#[derive(Default)]
struct RuntimeTranscriptionSelection {
    revision: u64,
    selection: TranscriptionSelection,
}

#[derive(Default)]
struct RuntimeMicrophoneSelection {
    revision: u64,
    device: Option<String>,
}

pub const SHIFT_KEY_MASK: u64 = 1 << 17;
pub const CONTROL_KEY_MASK: u64 = 1 << 18;
pub const OPTION_KEY_MASK: u64 = 1 << 19;
pub const COMMAND_KEY_MASK: u64 = 1 << 20;
pub const FUNCTION_KEY_MASK: u64 = 1 << 23;
pub const HOTKEY_MODIFIERS_MASK: u64 =
    SHIFT_KEY_MASK | CONTROL_KEY_MASK | OPTION_KEY_MASK | COMMAND_KEY_MASK | FUNCTION_KEY_MASK;
pub const LEFT_CONTROL_MASK: u64 = 0x0000_0001;
pub const LEFT_SHIFT_MASK: u64 = 0x0000_0002;
pub const RIGHT_SHIFT_MASK: u64 = 0x0000_0004;
pub const LEFT_COMMAND_MASK: u64 = 0x0000_0008;
pub const RIGHT_COMMAND_MASK: u64 = 0x0000_0010;
pub const LEFT_OPTION_MASK: u64 = 0x0000_0020;
pub const RIGHT_OPTION_MASK: u64 = 0x0000_0040;
pub const RIGHT_CONTROL_MASK: u64 = 0x0000_2000;

/// Which of a modifier's keys a shortcut holds. `Either` is one key on
/// either side (or both, as before chords); `Both` is the left and the
/// right key together, such as both Options.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModifierSide {
    Left,
    Right,
    #[default]
    Either,
    Both,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct HotkeyModifiers {
    #[serde(deserialize_with = "deserialize_modifier_side")]
    pub control: Option<ModifierSide>,
    #[serde(deserialize_with = "deserialize_modifier_side")]
    pub option: Option<ModifierSide>,
    #[serde(deserialize_with = "deserialize_modifier_side")]
    pub shift: Option<ModifierSide>,
    #[serde(deserialize_with = "deserialize_modifier_side")]
    pub command: Option<ModifierSide>,
    pub function: bool,
}

fn deserialize_modifier_side<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<ModifierSide>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Bool(false)) => Ok(None),
        Some(serde_json::Value::Bool(true)) => Ok(Some(ModifierSide::Either)),
        Some(serde_json::Value::String(value)) => {
            serde_json::from_value(serde_json::Value::String(value))
                .map(Some)
                .map_err(D::Error::custom)
        }
        Some(_) => Err(D::Error::custom(
            "modifier side must be left, right, either, or null",
        )),
    }
}

impl HotkeyModifiers {
    pub const fn option() -> Self {
        Self {
            option: Some(ModifierSide::Either),
            control: None,
            shift: None,
            command: None,
            function: false,
        }
    }

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub const fn control() -> Self {
        Self {
            control: Some(ModifierSide::Either),
            option: None,
            shift: None,
            command: None,
            function: false,
        }
    }

    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub const fn control_option() -> Self {
        Self {
            control: Some(ModifierSide::Either),
            option: Some(ModifierSide::Either),
            shift: None,
            command: None,
            function: false,
        }
    }

    pub const fn option_command() -> Self {
        Self {
            option: Some(ModifierSide::Either),
            command: Some(ModifierSide::Either),
            control: None,
            shift: None,
            function: false,
        }
    }

    pub const fn is_empty(self) -> bool {
        self.control.is_none()
            && self.option.is_none()
            && self.shift.is_none()
            && self.command.is_none()
            && !self.function
    }

    /// How many keys the modifiers hold: both sides of one count as two.
    pub const fn count(self) -> u8 {
        side_count(self.control)
            + side_count(self.option)
            + side_count(self.shift)
            + side_count(self.command)
            + self.function as u8
    }

    /// The sides a recorder keeps from the modifiers held at a modifier-only
    /// chord's peak: a lone modifier keeps its side (the side picker can
    /// widen it), both sides of one stay both (both Options), and every
    /// other side becomes either, so ⌥⌘ works from either side.
    pub fn recorded_sides(self) -> Self {
        if self.count() == 1 {
            return self;
        }
        let keep_both = |side: Option<ModifierSide>| match side {
            Some(ModifierSide::Both) => Some(ModifierSide::Both),
            side => either_side(side),
        };
        Self {
            control: keep_both(self.control),
            option: keep_both(self.option),
            shift: keep_both(self.shift),
            command: keep_both(self.command),
            function: self.function,
        }
    }

    pub const fn without_side_constraints(self) -> Self {
        Self {
            control: either_side(self.control),
            option: either_side(self.option),
            shift: either_side(self.shift),
            command: either_side(self.command),
            function: self.function,
        }
    }

    /// Every key `required` names is among these held ones; others may be
    /// held too.
    fn contains(self, required: Self) -> bool {
        side_covers(self.control, required.control)
            && side_covers(self.option, required.option)
            && side_covers(self.shift, required.shift)
            && side_covers(self.command, required.command)
            && (!required.function || self.function)
    }

    /// Exactly the keys `required` names are held: no other modifier, and
    /// no other side of one it names.
    fn holds_exactly(self, required: Self) -> bool {
        side_exact(self.control, required.control)
            && side_exact(self.option, required.option)
            && side_exact(self.shift, required.shift)
            && side_exact(self.command, required.command)
            && self.function == required.function
    }

    /// One keycap per modifier key, so both Options show as two.
    fn sided(self) -> impl Iterator<Item = (Modifier, ModifierSide)> {
        let function = self.function.then_some(ModifierSide::Either);
        [
            (Modifier::Function, function),
            (Modifier::Control, self.control),
            (Modifier::Option, self.option),
            (Modifier::Shift, self.shift),
            (Modifier::Command, self.command),
        ]
        .into_iter()
        .flat_map(|(modifier, side)| {
            let sides: &[ModifierSide] = match side {
                None => &[],
                Some(ModifierSide::Both) => &[ModifierSide::Left, ModifierSide::Right],
                Some(ModifierSide::Left) => &[ModifierSide::Left],
                Some(ModifierSide::Right) => &[ModifierSide::Right],
                Some(ModifierSide::Either) => &[ModifierSide::Either],
            };
            sides.iter().map(move |side| (modifier, *side))
        })
    }

    #[cfg(target_os = "macos")]
    fn keycaps(self) -> impl Iterator<Item = &'static str> {
        self.sided()
            .map(|(modifier, side)| side_keycap(side, modifier.keycap()))
    }

    /// Each modifier key's keycap and the side it must be pressed on.
    /// Linux binds no side, so its keycaps never name one.
    fn sided_keycaps(self) -> impl Iterator<Item = (&'static str, ModifierSide)> {
        self.sided().map(|(modifier, side)| {
            let side = if cfg!(target_os = "linux") {
                ModifierSide::Either
            } else {
                side
            };
            (modifier.keycap(), side)
        })
    }

    #[cfg(not(target_os = "macos"))]
    fn keycaps(self) -> impl Iterator<Item = &'static str> {
        self.sided_keycaps().map(|(label, _)| label)
    }
}

/// One modifier, as a chord names it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Modifier {
    Function,
    Control,
    Option,
    Shift,
    Command,
}

impl Modifier {
    /// The Mac's symbols; Windows names Command the Windows key and Option
    /// Alt, and Linux names them Super and Alt.
    pub const fn keycap(self) -> &'static str {
        match self {
            Self::Function => "fn",
            #[cfg(target_os = "macos")]
            Self::Control => "⌃",
            #[cfg(target_os = "macos")]
            Self::Option => "⌥",
            #[cfg(target_os = "macos")]
            Self::Shift => "⇧",
            #[cfg(target_os = "macos")]
            Self::Command => "⌘",
            #[cfg(not(target_os = "macos"))]
            Self::Control => "Ctrl",
            #[cfg(not(target_os = "macos"))]
            Self::Option => "Alt",
            #[cfg(not(target_os = "macos"))]
            Self::Shift => "Shift",
            #[cfg(target_os = "windows")]
            Self::Command => "Win",
            #[cfg(target_os = "linux")]
            Self::Command => "Super",
        }
    }
}

const fn side_count(side: Option<ModifierSide>) -> u8 {
    match side {
        None => 0,
        Some(ModifierSide::Both) => 2,
        Some(_) => 1,
    }
}

const fn either_side(side: Option<ModifierSide>) -> Option<ModifierSide> {
    match side {
        Some(_) => Some(ModifierSide::Either),
        None => None,
    }
}

/// `held` is what the keyboard reports for one modifier: a side, `Both`,
/// or `Either` for input that names no side. `Either` asks for any key of
/// it, held alone or with its twin.
fn side_covers(held: Option<ModifierSide>, required: Option<ModifierSide>) -> bool {
    match (held, required) {
        (_, None) => true,
        (None, Some(_)) => false,
        (Some(_), Some(ModifierSide::Either)) => true,
        (Some(ModifierSide::Both), Some(ModifierSide::Left | ModifierSide::Right)) => true,
        (Some(held), Some(required)) => held == required,
    }
}

fn side_exact(held: Option<ModifierSide>, required: Option<ModifierSide>) -> bool {
    match (held, required) {
        (None, None) => true,
        (Some(_), Some(ModifierSide::Either)) => true,
        (Some(held), Some(required)) => held == required,
        _ => false,
    }
}

#[cfg(target_os = "macos")]
fn side_keycap(side: ModifierSide, symbol: &'static str) -> &'static str {
    match (side, symbol) {
        (ModifierSide::Left, "⌃") => "L⌃",
        (ModifierSide::Right, "⌃") => "R⌃",
        (ModifierSide::Left, "⌥") => "L⌥",
        (ModifierSide::Right, "⌥") => "R⌥",
        (ModifierSide::Left, "⇧") => "L⇧",
        (ModifierSide::Right, "⇧") => "R⇧",
        (ModifierSide::Left, "⌘") => "L⌘",
        (ModifierSide::Right, "⌘") => "R⌘",
        _ => symbol,
    }
}

/// A shortcut's non-modifier key. macOS identifies it by `code`, the virtual
/// key code resolved through the active keyboard layout. Windows and Linux
/// identify it by `label`, the key's name ("Space", "V", "F12", "PageUp"),
/// which their input layers translate to virtual keys, X11 keysyms, or the
/// shortcuts portal's key names; `code` is zero there. A settings file never moves between OSes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HotkeyKey {
    pub code: u16,
    pub label: String,
}

/// Reserved input codes for the 32 mouse buttons CoreGraphics can report.
const MOUSE_INPUT_CODES: std::ops::Range<u16> = 0x8000..0x8020;

impl HotkeyKey {
    /// macOS mouse buttons occupy a separate input-code range, beyond every
    /// native keyboard code used by macOS. A shared trigger code lets keyboard
    /// and mouse bindings use the same hold, toggle, conflict and persistence
    /// rules. UI labels are one-based (Mouse 1 = left, Mouse 2 = right, Mouse 3 = middle).
    pub fn mouse(button: u16) -> Option<Self> {
        Self::mouse_code(button).map(|code| Self {
            code,
            label: format!("Mouse {}", button + 1),
        })
    }

    /// Allocation-free encoding for the event tap.
    pub fn mouse_code(button: u16) -> Option<u16> {
        (button < 32).then(|| MOUSE_INPUT_CODES.start + button)
    }

    #[cfg(target_os = "macos")]
    pub fn mouse_button(code: u16) -> Option<u16> {
        code.checked_sub(MOUSE_INPUT_CODES.start)
            .filter(|button| *button < 32)
    }

    pub fn is_mouse(&self) -> bool {
        MOUSE_INPUT_CODES.contains(&self.code)
    }

    /// A key identified by name on Windows and Linux.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn named(name: &str) -> Self {
        if let Some(button) = Self::mouse_number(name) {
            return Self::mouse(button - 1).expect("validated mouse number");
        }
        Self {
            code: 0,
            label: key_label(name),
        }
    }

    /// The lowercase key name Windows and Linux input layers resolve.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn name(&self) -> String {
        if self.is_mouse() {
            return format!("mouse{}", self.code - MOUSE_INPUT_CODES.start + 1);
        }
        self.label.to_ascii_lowercase()
    }

    /// One-based logical button number shared by every platform.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn mouse_number(name: &str) -> Option<u16> {
        name.to_ascii_lowercase()
            .strip_prefix("mouse")?
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|number| (1..=32).contains(number))
    }

    fn same_key(&self, other: &Self) -> bool {
        if cfg!(target_os = "macos") {
            self.code == other.code
        } else {
            self.label.eq_ignore_ascii_case(&other.label)
        }
    }
}

/// The keycap label for a key name: one character in uppercase, otherwise
/// the name with a leading capital ("space" -> "Space", "f12" -> "F12").
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn key_label(name: &str) -> String {
    let name = name.trim();
    let mut characters = name.chars();
    match characters.next() {
        Some(first) if name.chars().count() == 1 => first.to_uppercase().collect(),
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

/// A shortcut as a chord: the set of keys held together, any order and
/// nothing else. Modifiers name their side (or either), and at most one
/// other key or mouse button joins them. Settings store it as `keys`
/// ([`ChordKey`]); files from before chords stored `modifiers` and `key`,
/// which load as the same chord and are written back as `keys` once.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HotkeyBinding {
    pub modifiers: HotkeyModifiers,
    pub key: Option<HotkeyKey>,
}

/// One key of a stored chord: a modifier with its side, or a key.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ChordKey {
    Modifier {
        modifier: ChordModifier,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        side: Option<ChordSide>,
    },
    Key(HotkeyKey),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChordModifier {
    Fn,
    Control,
    Option,
    Shift,
    Command,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChordSide {
    Left,
    Right,
    Any,
}

#[derive(Deserialize, Serialize)]
struct StoredChord {
    keys: Vec<ChordKey>,
}

/// The shape settings stored before chords.
#[derive(Deserialize)]
struct LegacyBinding {
    #[serde(default = "legacy_default_modifiers")]
    modifiers: HotkeyModifiers,
    #[serde(default = "legacy_default_key")]
    key: Option<HotkeyKey>,
}

fn legacy_default_modifiers() -> HotkeyModifiers {
    HotkeyBinding::default().modifiers
}

fn legacy_default_key() -> Option<HotkeyKey> {
    HotkeyBinding::default().key
}

impl HotkeyBinding {
    /// The chord's keys in a fixed order: fn, Control, Option, Shift,
    /// Command (left before right), then the key.
    pub fn chord_keys(&self) -> Vec<ChordKey> {
        let mut keys = Vec::new();
        if self.modifiers.function {
            keys.push(ChordKey::Modifier {
                modifier: ChordModifier::Fn,
                side: None,
            });
        }
        for (modifier, side) in [
            (ChordModifier::Control, self.modifiers.control),
            (ChordModifier::Option, self.modifiers.option),
            (ChordModifier::Shift, self.modifiers.shift),
            (ChordModifier::Command, self.modifiers.command),
        ] {
            let sides: &[ChordSide] = match side {
                None => &[],
                Some(ModifierSide::Left) => &[ChordSide::Left],
                Some(ModifierSide::Right) => &[ChordSide::Right],
                Some(ModifierSide::Either) => &[ChordSide::Any],
                Some(ModifierSide::Both) => &[ChordSide::Left, ChordSide::Right],
            };
            keys.extend(sides.iter().map(|side| ChordKey::Modifier {
                modifier,
                side: Some(*side),
            }));
        }
        keys.extend(self.key.clone().map(ChordKey::Key));
        keys
    }

    /// The chord of `keys`, which may come in any order. Both sides of a
    /// modifier are `Both`; a key twice, a side beside "any", or a second
    /// key besides modifiers is refused.
    pub fn from_chord_keys(keys: impl IntoIterator<Item = ChordKey>) -> Result<Self, String> {
        let mut binding = Self {
            modifiers: HotkeyModifiers::default(),
            key: None,
        };
        for key in keys {
            match key {
                ChordKey::Key(key) => {
                    if binding.key.is_some() {
                        return Err("a shortcut holds at most one key besides modifiers".into());
                    }
                    binding.key = Some(key);
                }
                ChordKey::Modifier {
                    modifier: ChordModifier::Fn,
                    side,
                } => {
                    if binding.modifiers.function {
                        return Err("fn appears twice".into());
                    }
                    if side.is_some_and(|side| side != ChordSide::Any) {
                        return Err("fn has no side".into());
                    }
                    binding.modifiers.function = true;
                }
                ChordKey::Modifier { modifier, side } => {
                    let slot = match modifier {
                        ChordModifier::Control => &mut binding.modifiers.control,
                        ChordModifier::Option => &mut binding.modifiers.option,
                        ChordModifier::Shift => &mut binding.modifiers.shift,
                        ChordModifier::Command => &mut binding.modifiers.command,
                        ChordModifier::Fn => unreachable!("fn is matched above"),
                    };
                    let side = match side.unwrap_or(ChordSide::Any) {
                        ChordSide::Left => ModifierSide::Left,
                        ChordSide::Right => ModifierSide::Right,
                        ChordSide::Any => ModifierSide::Either,
                    };
                    *slot = Some(match (*slot, side) {
                        (None, side) => side,
                        (Some(ModifierSide::Left), ModifierSide::Right)
                        | (Some(ModifierSide::Right), ModifierSide::Left) => ModifierSide::Both,
                        _ => return Err(format!("{modifier:?} appears twice").to_lowercase()),
                    });
                }
            }
        }
        Ok(binding)
    }
}

impl Serialize for HotkeyBinding {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        StoredChord {
            keys: self.chord_keys(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for HotkeyBinding {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        use serde::de::Error;
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.get("keys").is_some() {
            let chord: StoredChord = serde_json::from_value(value).map_err(D::Error::custom)?;
            return Self::from_chord_keys(chord.keys).map_err(D::Error::custom);
        }
        let legacy: LegacyBinding = serde_json::from_value(value).map_err(D::Error::custom)?;
        Ok(Self {
            modifiers: legacy.modifiers,
            key: legacy.key,
        })
    }
}

impl Default for HotkeyBinding {
    /// Option on macOS, Ctrl+Space on Windows, Alt+Space on Linux, and
    /// Ctrl+Alt+Space on GNOME, whose window menu takes Alt+Space (#723).
    fn default() -> Self {
        #[cfg(target_os = "macos")]
        let binding = Self {
            modifiers: HotkeyModifiers::option(),
            key: None,
        };
        #[cfg(target_os = "windows")]
        let binding = Self::chord(HotkeyModifiers::control(), "space");
        #[cfg(target_os = "linux")]
        let binding = Self::linux_dictation_default(crate::linux::portal::gnome());
        binding
    }
}

impl HotkeyBinding {
    #[cfg(target_os = "linux")]
    fn linux_dictation_default(gnome: bool) -> Self {
        if gnome {
            Self::chord(HotkeyModifiers::control_option(), "space")
        } else {
            Self::chord(HotkeyModifiers::option(), "space")
        }
    }

    /// Modifiers plus a key identified by name (Windows and Linux).
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    fn chord(modifiers: HotkeyModifiers, key: &str) -> Self {
        Self {
            modifiers,
            key: Some(HotkeyKey::named(key)),
        }
    }

    /// Option-Shift-V on macOS; Ctrl+Alt+V elsewhere.
    pub fn paste_last_default() -> Self {
        #[cfg(target_os = "macos")]
        let binding = Self {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Either),
                shift: Some(ModifierSide::Either),
                ..Default::default()
            },
            key: Some(HotkeyKey {
                code: paste_key_code(),
                label: "V".into(),
            }),
        };
        #[cfg(not(target_os = "macos"))]
        let binding = Self::chord(HotkeyModifiers::control_option(), "v");
        binding
    }

    /// Option-Shift-S, Scrawl's capture shortcut; F9 on Linux. Annotate's
    /// one key is held to draw while the other hand moves the mouse, so on
    /// Linux it's a single key one hand holds easily: F9 is free in Hex's
    /// defaults (Alt+Space, Ctrl+Alt+V) and in the common desktops', where
    /// F10 opens GTK's menu bar, F11 is full screen and F12 is often a
    /// dropdown terminal.
    #[cfg(hex_capture)]
    pub fn capture_default() -> Self {
        #[cfg(target_os = "macos")]
        let binding = Self {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Either),
                shift: Some(ModifierSide::Either),
                ..Default::default()
            },
            key: Some(HotkeyKey {
                code: layout_key_code('s').unwrap_or(1),
                label: "S".into(),
            }),
        };
        #[cfg(not(target_os = "macos"))]
        let binding = Self::chord(HotkeyModifiers::default(), "f9");
        binding
    }

    pub fn paste_meeting_default() -> Self {
        Self {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Either),
                control: Some(ModifierSide::Either),
                ..Default::default()
            },
            key: Some(HotkeyKey {
                code: paste_key_code(),
                label: "V".into(),
            }),
        }
    }
}

/// Scrawl's record shortcut from its app's `{key_code, carbon_modifiers,
/// label}` file; a shortcut without a modifier is not Scrawl's.
#[cfg(hex_capture)]
fn scrawl_shortcut(bytes: &[u8]) -> Option<HotkeyBinding> {
    #[derive(Deserialize)]
    struct Shortcut {
        key_code: u16,
        carbon_modifiers: u32,
        label: String,
    }
    const COMMAND: u32 = 1 << 8;
    const SHIFT: u32 = 1 << 9;
    const OPTION: u32 = 1 << 11;
    const CONTROL: u32 = 1 << 12;
    let shortcut: Shortcut = serde_json::from_slice(bytes).ok()?;
    let held = |mask: u32| (shortcut.carbon_modifiers & mask != 0).then_some(ModifierSide::Either);
    let modifiers = HotkeyModifiers {
        control: held(CONTROL),
        option: held(OPTION),
        shift: held(SHIFT),
        command: held(COMMAND),
        function: false,
    };
    let label: String = shortcut
        .label
        .chars()
        .filter(|c| !matches!(c, '⌃' | '⌥' | '⇧' | '⌘'))
        .collect();
    (!modifiers.is_empty() && !label.is_empty()).then_some(HotkeyBinding {
        modifiers,
        key: Some(HotkeyKey {
            code: shortcut.key_code,
            label,
        }),
    })
}

#[cfg(target_os = "macos")]
fn paste_key_code() -> u16 {
    *PASTE_KEY_CODE.get_or_init(|| layout_key_code('v').unwrap_or(9))
}

/// The active layout's key for `character`. GUI startup snapshots the layout
/// on the main thread; unit tests run on worker threads, where reaching TIS is
/// unsupported, so they resolve against the ANSI layout instead.
#[cfg(target_os = "macos")]
fn layout_key_code(character: char) -> Option<u16> {
    if cfg!(test) {
        return match character {
            'e' => Some(14),
            's' => Some(1),
            'v' => Some(9),
            _ => None,
        };
    }
    crate::keyboard::key_code_for(character).ok()
}

#[cfg(not(target_os = "macos"))]
fn paste_key_code() -> u16 {
    0
}

impl HotkeyBinding {
    pub fn keycaps(&self) -> Vec<String> {
        self.modifiers
            .keycaps()
            .map(str::to_string)
            .chain(self.key.iter().map(|key| key.label.clone()))
            .collect()
    }

    /// Each keycap with the side it must be pressed on: `Either` for the
    /// key, and for every modifier but on macOS.
    pub fn sided_keycaps(&self) -> Vec<(String, ModifierSide)> {
        self.modifiers
            .sided_keycaps()
            .map(|(label, side)| (label.to_string(), side))
            .chain(
                self.key
                    .iter()
                    .map(|key| (key.label.clone(), ModifierSide::Either)),
            )
            .collect()
    }

    /// A left click (Mouse 1) with no modifier. Matching it would swallow
    /// every click, Settings' own included, so no shortcut may be one.
    pub fn is_bare_primary_click(&self) -> bool {
        self.modifiers.is_empty()
            && self.key.as_ref().is_some_and(|key| {
                key.code == MOUSE_INPUT_CODES.start
                    || HotkeyKey::mouse_number(&key.label) == Some(1)
            })
    }

    pub fn runtime(&self) -> RuntimeHotkey {
        RuntimeHotkey {
            modifiers: self.modifiers,
            key_code: self.key.as_ref().map(|key| key.code),
        }
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        let same_key = match (&self.key, &other.key) {
            (Some(left), Some(right)) => left.same_key(right),
            (None, None) => true,
            _ => false,
        };
        same_key && modifiers_overlap(self.modifiers, other.modifiers)
    }

    /// The side of a chord that is one modifier key, such as Right Option;
    /// none for every other chord, whose keys name their own sides.
    pub fn standalone_side(&self) -> Option<ModifierSide> {
        if self.key.is_some() || self.modifiers.function || self.modifiers.count() != 1 {
            return None;
        }
        [
            self.modifiers.control,
            self.modifiers.option,
            self.modifiers.shift,
            self.modifiers.command,
        ]
        .into_iter()
        .flatten()
        .next()
    }

    /// Moves a one-modifier chord to `side`; other chords are unchanged.
    pub fn set_standalone_side(&mut self, side: ModifierSide) {
        if self.standalone_side().is_none() || side == ModifierSide::Both {
            return;
        }
        for current in [
            &mut self.modifiers.control,
            &mut self.modifiers.option,
            &mut self.modifiers.shift,
            &mut self.modifiers.command,
        ] {
            if current.is_some() {
                *current = Some(side);
            }
        }
    }
}

fn modifiers_overlap(left: HotkeyModifiers, right: HotkeyModifiers) -> bool {
    modifier_overlap(left.control, right.control)
        && modifier_overlap(left.option, right.option)
        && modifier_overlap(left.shift, right.shift)
        && modifier_overlap(left.command, right.command)
        && left.function == right.function
}

/// Whether one press could match both: `Either` takes any side, `Both`
/// only both.
fn modifier_overlap(left: Option<ModifierSide>, right: Option<ModifierSide>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(ModifierSide::Either), Some(_)) | (Some(_), Some(ModifierSide::Either)) => true,
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RuntimeHotkey {
    pub modifiers: HotkeyModifiers,
    pub key_code: Option<u16>,
}

impl RuntimeHotkey {
    pub fn is_empty(self) -> bool {
        self.modifiers.is_empty() && self.key_code.is_none()
    }

    /// Exactly the binding's modifier keys are down: a sided binding wants
    /// its side alone, and both sides of a modifier only for `Both`.
    pub fn exact_modifiers(self, flags: u64) -> bool {
        let flags = self.without_implied_function(flags);
        modifiers_from_flags(flags).holds_exactly(self.modifiers)
    }

    /// macOS marks every press of a function, arrow, or navigation key with
    /// SecondaryFn whether or not Fn is held, and the recorder leaves it out
    /// of such a binding. Only for a binding on one of those keys that does
    /// not ask for Fn is the flag ignored; Fn is never stripped otherwise.
    fn without_implied_function(self, flags: u64) -> u64 {
        if cfg!(target_os = "macos")
            && !self.modifiers.function
            && self.key_code.is_some_and(key_reports_function_flag)
        {
            flags & !FUNCTION_KEY_MASK
        } else {
            flags
        }
    }

    /// Every modifier key of the binding is down, whatever else is.
    pub fn required_modifiers_down(self, flags: u64) -> bool {
        modifiers_from_flags(flags).contains(self.modifiers)
    }

    /// The device flags of each modifier the binding holds on both sides,
    /// whose two keys must go down together (`suppression`).
    pub fn both_sides_masks(self) -> impl Iterator<Item = (u64, u64)> {
        [
            (
                self.modifiers.control,
                LEFT_CONTROL_MASK,
                RIGHT_CONTROL_MASK,
            ),
            (self.modifiers.option, LEFT_OPTION_MASK, RIGHT_OPTION_MASK),
            (self.modifiers.shift, LEFT_SHIFT_MASK, RIGHT_SHIFT_MASK),
            (
                self.modifiers.command,
                LEFT_COMMAND_MASK,
                RIGHT_COMMAND_MASK,
            ),
        ]
        .into_iter()
        .filter(|(side, ..)| *side == Some(ModifierSide::Both))
        .map(|(_, left, right)| (left, right))
    }

    pub fn matches_key_press(self, code: u16, flags: u64) -> bool {
        self.key_code == Some(code) && self.exact_modifiers(flags)
    }
}

/// macOS virtual key codes whose events always carry SecondaryFn: F1–F20,
/// the arrows, Home, End, Page Up, Page Down, Forward Delete, and Help.
fn key_reports_function_flag(code: u16) -> bool {
    const CODES: [u16; 30] = [
        122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111, 105, 107, 113, 106, 64, 79, 80, 90,
        123, 124, 125, 126, 115, 119, 116, 121, 117, 114,
    ];
    CODES.contains(&code)
}

pub fn modifiers_from_flags(flags: u64) -> HotkeyModifiers {
    HotkeyModifiers {
        control: side_from_flags(
            flags,
            CONTROL_KEY_MASK,
            LEFT_CONTROL_MASK,
            RIGHT_CONTROL_MASK,
        ),
        option: side_from_flags(flags, OPTION_KEY_MASK, LEFT_OPTION_MASK, RIGHT_OPTION_MASK),
        shift: side_from_flags(flags, SHIFT_KEY_MASK, LEFT_SHIFT_MASK, RIGHT_SHIFT_MASK),
        command: side_from_flags(
            flags,
            COMMAND_KEY_MASK,
            LEFT_COMMAND_MASK,
            RIGHT_COMMAND_MASK,
        ),
        function: flags & FUNCTION_KEY_MASK != 0,
    }
}

/// Input without device flags (synthetic events) names no side.
fn side_from_flags(flags: u64, general: u64, left: u64, right: u64) -> Option<ModifierSide> {
    match (flags & left != 0, flags & right != 0) {
        (true, false) => Some(ModifierSide::Left),
        (false, true) => Some(ModifierSide::Right),
        (true, true) => Some(ModifierSide::Both),
        (false, false) if flags & general != 0 => Some(ModifierSide::Either),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeHotkeys {
    pub dictation: RuntimeHotkey,
    pub paste_last: Option<RuntimeHotkey>,
    pub paste_meeting: Option<RuntimeHotkey>,
    /// The capture shortcut, while captures are on (`cfg(hex_capture)`).
    pub capture: Option<RuntimeHotkey>,
    /// Dictate without processing, while it is bound.
    pub raw_dictation: Option<RuntimeHotkey>,
    /// Labs › Second dictation shortcut (macOS), while it is on and bound.
    pub second_dictation: Option<RuntimeHotkey>,
}

impl RuntimeHotkeys {
    /// Every shortcut Hex matches, for suppression and chord evidence.
    pub fn all(self) -> [Option<RuntimeHotkey>; 6] {
        [
            Some(self.dictation),
            self.paste_last,
            self.paste_meeting,
            self.capture,
            self.raw_dictation,
            self.second_dictation,
        ]
    }
}

impl Default for RuntimeHotkeys {
    fn default() -> Self {
        Self {
            dictation: HotkeyBinding::default().runtime(),
            paste_last: Some(HotkeyBinding::paste_last_default().runtime()),
            paste_meeting: crate::DEVELOPER_FEATURES_ENABLED
                .then(|| HotkeyBinding::paste_meeting_default().runtime()),
            capture: None,
            raw_dictation: None,
            second_dictation: None,
        }
    }
}

/// Screen corner where the floating meeting recording indicator snaps.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum MeetingIndicatorCorner {
    #[default]
    TopRight = 0,
    TopLeft = 1,
    BottomRight = 2,
    BottomLeft = 3,
}

impl MeetingIndicatorCorner {
    pub const fn is_right(self) -> bool {
        matches!(self, Self::TopRight | Self::BottomRight)
    }

    pub const fn is_top(self) -> bool {
        matches!(self, Self::TopRight | Self::TopLeft)
    }

    const fn decode(value: u8) -> Self {
        match value {
            1 => Self::TopLeft,
            2 => Self::BottomRight,
            3 => Self::BottomLeft,
            _ => Self::TopRight,
        }
    }
}

/// Opt-in meeting recording. Detection, the Meetings pane, and the meeting
/// paste shortcut stay off in release builds until `enabled` is set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct MeetingSettings {
    pub enabled: bool,
    /// Folder for finished Markdown transcripts; `None` uses the default.
    pub output_folder: Option<PathBuf>,
    /// Title meetings and write a two-sentence summary with Apple
    /// Intelligence when it is available.
    pub summarize_with_apple_intelligence: bool,
    /// Title meetings and write a two-sentence summary with the OpenCode
    /// model, which sends the transcript to it. On a Mac, only when Apple
    /// Intelligence is unavailable or off.
    pub summarize_with_opencode: bool,
    /// Show the floating recording indicator while a meeting is recording.
    pub show_recording_indicator: bool,
    /// Screen corner the floating recording indicator snaps to after dragging.
    pub recording_indicator_corner: MeetingIndicatorCorner,
    /// What to do when a call from each website or app is detected. Older
    /// files only had a `never_offer` list, whose entries carry no `policy`
    /// and so read as Never offer.
    #[serde(alias = "never_offer")]
    pub providers: Vec<MeetingProviderChoice>,
    /// Reminders before calendar meetings (macOS). Off until the user
    /// connects a calendar.
    pub calendar: CalendarSettings,
    /// How long finished meetings are kept before they move to the Trash.
    /// `None` until the user chooses: meetings recorded since the default
    /// began are kept 24 hours, and older ones stay. Left out of the file
    /// while unchosen: a build from before it can't read `null` (#744).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention: Option<MeetingRetention>,
}

/// How long a finished meeting is kept, counted from when it ended.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingRetention {
    Day,
    Week,
    Month,
    Forever,
}

impl MeetingRetention {
    pub const ALL: [Self; 4] = [Self::Day, Self::Week, Self::Month, Self::Forever];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Day => "24 hours",
            Self::Week => "7 days",
            Self::Month => "30 days",
            Self::Forever => "Forever",
        }
    }

    pub const fn max_age_ms(self) -> Option<u64> {
        const DAY_MS: u64 = 24 * 60 * 60 * 1_000;
        match self {
            Self::Day => Some(DAY_MS),
            Self::Week => Some(7 * DAY_MS),
            Self::Month => Some(30 * DAY_MS),
            Self::Forever => None,
        }
    }

    /// The default for meetings recorded since it began.
    pub const DEFAULT: Self = Self::Day;

    const fn encode(retention: Option<Self>) -> u8 {
        match retention {
            Some(Self::Day) => 0,
            Some(Self::Week) => 1,
            Some(Self::Month) => 2,
            Some(Self::Forever) => 3,
            None => 4,
        }
    }

    const fn decode(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Day),
            1 => Some(Self::Week),
            2 => Some(Self::Month),
            3 => Some(Self::Forever),
            _ => None,
        }
    }
}

/// The connected calendar and its meeting reminders.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct CalendarSettings {
    /// Set once the user clicks Connect calendar and macOS grants access;
    /// Hex never asks for calendar access before that.
    pub connected: bool,
    /// Calendars left out of reminders, by EventKit identifier. Every
    /// calendar is included by default, so new ones are too.
    pub excluded_calendars: Vec<String>,
    /// Show a reminder capsule before each meeting.
    pub remind: bool,
    /// How many minutes before a meeting starts the reminder shows; zero
    /// shows it at the start.
    pub lead_minutes: u8,
}

/// The reminder lead times Settings offers, in minutes.
pub const CALENDAR_LEAD_MINUTES: [u8; 4] = [0, 1, 2, 5];

impl Default for CalendarSettings {
    fn default() -> Self {
        Self {
            connected: false,
            excluded_calendars: Vec::new(),
            remind: true,
            lead_minutes: 1,
        }
    }
}

impl CalendarSettings {
    /// Whether reminders should run: connected, and wanted.
    pub fn reminders_active(&self) -> bool {
        self.connected && self.remind
    }

    /// The lead time, snapped to an offered choice.
    pub fn lead(&self) -> std::time::Duration {
        let minutes = if CALENDAR_LEAD_MINUTES.contains(&self.lead_minutes) {
            self.lead_minutes
        } else {
            1
        };
        std::time::Duration::from_secs(u64::from(minutes) * 60)
    }

    pub fn includes(&self, calendar: &str) -> bool {
        !self
            .excluded_calendars
            .iter()
            .any(|excluded| excluded == calendar)
    }
}

/// The call services Settings always lists, by provider key (the app's
/// canonical bundle ID, or the website host for browser services) and
/// English name.
pub const KNOWN_MEETING_PROVIDERS: [(&str, &str); 5] = [
    ("com.tinyspeck.slackmacgap", "Slack huddles"),
    ("us.zoom.xos", "Zoom"),
    ("meet.google.com", "Google Meet"),
    ("com.microsoft.teams2", "Microsoft Teams"),
    ("com.apple.FaceTime", "FaceTime"),
];

/// What Hex does when it detects a call from a provider.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingPolicy {
    /// Offer to record.
    #[default]
    Ask,
    /// Start recording without an offer.
    AlwaysRecord,
    /// Neither offer nor record.
    NeverOffer,
}

/// A website host or an app's canonical bundle ID, the name shown in
/// Settings, and the user's choice for its calls. Unlisted providers Ask.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MeetingProviderChoice {
    pub key: String,
    pub label: String,
    /// Always written; missing only in a legacy `never_offer` entry.
    #[serde(default = "legacy_never_offer")]
    pub policy: MeetingPolicy,
}

fn legacy_never_offer() -> MeetingPolicy {
    MeetingPolicy::NeverOffer
}

impl Default for MeetingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            output_folder: None,
            summarize_with_apple_intelligence: true,
            summarize_with_opencode: false,
            show_recording_indicator: true,
            recording_indicator_corner: MeetingIndicatorCorner::TopRight,
            providers: Vec::new(),
            calendar: CalendarSettings::default(),
            retention: None,
        }
    }
}

impl MeetingSettings {
    pub fn available(&self) -> bool {
        crate::DEVELOPER_FEATURES_ENABLED || self.enabled
    }

    pub fn resolved_output_folder(&self) -> PathBuf {
        self.output_folder
            .clone()
            .filter(|folder| !folder.as_os_str().is_empty())
            .unwrap_or_else(default_meeting_output_folder)
    }
}

/// Experimental opt-in features hidden by default.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct ExperimentalSettings {
    pub hex_microphone: bool,
    pub hex_microphone_mode: crate::audio::voice_filter::VoiceFilterMode,
    pub hex_microphone_comfort_noise: bool,
    pub hex_microphone_strength: f32,
}

impl Default for ExperimentalSettings {
    fn default() -> Self {
        Self {
            hex_microphone: false,
            hex_microphone_mode: crate::audio::voice_filter::VoiceFilterMode::Muffled,
            hex_microphone_comfort_noise: true,
            hex_microphone_strength: 1.0,
        }
    }
}

/// The `labs` switches (`crate::labs`), every one off by default. Labs
/// features that already had a home keep it: `meetings.enabled`,
/// `experimental.hex_microphone`, `paste_with_shift`,
/// `keyboard_model_switching.enabled`, `pause_browser_media`, and
/// `share_dictation_shortcut`. An older
/// `labs.enabled` master switch is read and ignored.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct LabsSettings {
    /// Live text beside the HUD while dictating.
    pub live_text: bool,
    /// Live chapters while a meeting records.
    pub live_chapters: bool,
    /// The playground in the window.
    pub playground: bool,
    /// Parakeet transcription biased toward the vocabulary.
    pub vocabulary_biasing: bool,
    /// Mouse buttons may be recorded as shortcuts (macOS, issue 209).
    pub mouse_shortcuts: bool,
    /// The engine unloads the speech model after `unload_idle_minutes`
    /// unused (issue 442).
    pub unload_idle: bool,
    /// 5, 15 (the default), 30, or 60.
    pub unload_idle_minutes: u32,
    /// Paste last dictation's row offers its default shortcut back
    /// (issue 613).
    pub restore_shortcut_default: bool,
    /// A pasted dictation stays on the clipboard instead of the previous
    /// clipboard coming back (macOS, issue 200).
    pub keep_on_clipboard: bool,
    /// A dictation is put on the clipboard and never pasted (macOS,
    /// issue 629).
    pub copy_only: bool,
    /// Spoken "new line", "new paragraph", and "open/close quote" become
    /// line breaks and quotation marks (issue 630).
    pub spoken_formatting: bool,
    /// Filler words such as "um" and "uh" are removed from dictations
    /// (issue 631).
    pub filler_words: bool,
    /// `hex meetings show --srt|--vtt` exports a meeting transcript as
    /// timed subtitles (macOS, issue 633).
    pub subtitle_export: bool,
    /// `second_dictation_hotkey` dictates like the dictation shortcut
    /// (macOS, issue 627).
    pub second_dictation_shortcut: bool,
    /// A pasted dictation gets one leading space when the character before
    /// the cursor is one a space belongs after (macOS, issue 632).
    pub smart_spacing: bool,
    /// Spoken punctuation and symbol words ("comma", "hashtag",
    /// "underscore") become characters joined to their words (issue 678).
    pub spoken_symbols: bool,
    /// The Local transcription picker offers a sort (Recommended, Size,
    /// Name) and a "Supports the saved language" toggle (issue 645).
    pub picker_sort_filter: bool,
    /// The shortcut recorder keeps the side of each modifier in a
    /// modifier-only chord, such as Right ⌘ + Right ⌥ (macOS, issue 710).
    pub sided_modifier_chords: bool,
    /// In Toggle mode, a press held past the hold threshold dictates until
    /// it lifts; a short tap still starts and stops (macOS, issue 711).
    pub tap_or_hold: bool,
    /// The Local transcription picker shows each model's raw error rate
    /// (WER or CER), its benchmark, source, and conditions (issue 647).
    pub benchmark_details: bool,
    /// The Local transcription picker can add a GGUF speech model from disk
    /// (issues 648 and 743).
    pub custom_models: bool,
}

/// The idle times Labs › Unload model when idle offers, in minutes.
pub const UNLOAD_IDLE_MINUTES: [u32; 4] = [5, 15, 30, 60];

impl Default for LabsSettings {
    fn default() -> Self {
        Self {
            live_text: false,
            live_chapters: false,
            playground: false,
            vocabulary_biasing: false,
            mouse_shortcuts: false,
            unload_idle: false,
            unload_idle_minutes: 15,
            restore_shortcut_default: false,
            keep_on_clipboard: false,
            copy_only: false,
            spoken_formatting: false,
            filler_words: false,
            subtitle_export: false,
            second_dictation_shortcut: false,
            smart_spacing: false,
            spoken_symbols: false,
            picker_sort_filter: false,
            sided_modifier_chords: false,
            tap_or_hold: false,
            benchmark_details: false,
            custom_models: false,
        }
    }
}

/// Whether the Voice Filter runs: the listener streams the Hex Microphone
/// and Hex watches its driver. Its Labs toggle (`experimental.hex_microphone`,
/// or `HEX_EXPERIMENTAL_MIC`) or an installed driver turns it on, so a driver
/// apps already use keeps receiving audio.
pub fn voice_filter_enabled(setting: bool, driver_installed: bool) -> bool {
    setting || env_hex_experimental_mic() || driver_installed
}

pub fn env_hex_experimental_mic() -> bool {
    std::env::var("HEX_EXPERIMENTAL_MIC")
        .ok()
        .is_some_and(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes"))
}

pub fn default_meeting_output_folder() -> PathBuf {
    dirs::document_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join("Documents")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(if crate::app_paths::is_macos_test_build() {
            "Hex Test Meetings"
        } else {
            "Hex Meetings"
        })
}

/// Where the recording HUD appears while dictating.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum IndicatorPosition {
    #[default]
    Top = 0,
    Bottom = 1,
    Hidden = 2,
}

/// Which release-service channel a direct install updates from
/// (`crate::updater`).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    Stable,
    Dev,
}

impl UpdateChannel {
    pub const ALL: [Self; 2] = [Self::Stable, Self::Dev];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Dev => "dev",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Dev => "Dev",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// `None` follows the build: the channel it was published on (stamped at
    /// build time), else `dev` for a dev version, else its install's channel.
    /// Another channel than the build's is a switch the user confirms.
    pub channel: Option<UpdateChannel>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingAudioBehavior {
    Mute,
    Duck,
    PauseMedia,
    #[default]
    DoNothing,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct DictationProcessingSettings {
    pub default_mode: DictationMode,
    pub modes: Vec<DictationMode>,
}

impl Default for DictationProcessingSettings {
    fn default() -> Self {
        Self {
            default_mode: DictationMode {
                name: "Global".into(),
                ..Default::default()
            },
            modes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct DictationMode {
    pub name: String,
    pub applications: Vec<String>,
    pub browser_hosts: Vec<String>,
    pub replacements: Vec<TextReplacement>,
    pub transformations: Vec<String>,
    pub post_processing: DictationPostProcessing,
}

/// Persisted OpenCode rewrite settings for one mode.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct DictationPostProcessing {
    pub enabled: bool,
    pub prompt: String,
    /// Optional `provider/model` key from the OpenCode catalog.
    pub model: Option<String>,
    pub variant: Option<String>,
    pub deadline_seconds: u64,
}

impl Default for DictationPostProcessing {
    fn default() -> Self {
        Self {
            enabled: false,
            prompt: "Rewrite the transcript into clear, natural text while preserving the speaker's intended meaning.".into(),
            model: None,
            variant: None,
            deadline_seconds: 30,
        }
    }
}

/// Annotate: Scrawl's captures inside Hex, on macOS, Linux, and Windows
/// (`capture_host`). On by default; off means no capture work at all.
#[cfg(hex_capture)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct CaptureSettings {
    pub enabled: bool,
    /// Annotate's one key: tap to start and stop, hold while recording to
    /// draw.
    pub hotkey: HotkeyBinding,
    pub transcriber: CaptureTranscriber,
    /// Where new captures go; `None` is Scrawl's own captures folder.
    pub folder: Option<PathBuf>,
    /// Days a capture keeps its video, audio, and full-size frames; `None`
    /// keeps them.
    pub media_days: Option<u32>,
    /// Days a capture is kept at all; `None` keeps it.
    pub bundle_days: Option<u32>,
    /// Where a written capture is sent among OpenCode sessions: Scrawl's
    /// router settings, imported once from Scrawl.
    pub routing: CaptureRouting,
    /// Lets screen recordings, Hex's own captures included, show Hex's
    /// overlays: the capture capsule, picker, and ink, the dictation HUD,
    /// and the meeting capsules. Off keeps them out (`capture_host`).
    pub show_overlays_in_recordings: bool,
    /// How the capsule offers where to send a finished capture, while Kit
    /// compares the directions.
    pub capsule_flow: CapsuleFlow,
}

/// The capsule's flow after a capture stops, one of five prototypes.
#[cfg(hex_capture)]
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapsuleFlow {
    /// (a) "Copied", and the suggestions in a list under the capsule.
    #[default]
    List,
    /// (b) One quiet line, "Send to SEER ⏎"; ⇥ moves to the next guess.
    Toast,
    /// (c) A short hint of the guesses in the capsule, which then folds
    /// into Hex's menu-bar item.
    MenuBar,
    /// (d) The guesses in a small card at the pointer.
    Cursor,
    /// (e) A pill hanging from the top edge that grows down into the
    /// guesses.
    Notch,
}

#[cfg(hex_capture)]
impl CapsuleFlow {
    pub const ALL: [Self; 5] = [
        Self::List,
        Self::Toast,
        Self::MenuBar,
        Self::Cursor,
        Self::Notch,
    ];
}

#[cfg(hex_capture)]
impl Default for CaptureSettings {
    fn default() -> Self {
        let week = scrawl_capture::retention::Preset::Week.policy();
        Self {
            enabled: true,
            hotkey: HotkeyBinding::capture_default(),
            transcriber: CaptureTranscriber::default(),
            folder: None,
            media_days: week.media_days,
            bundle_days: week.bundle_days,
            routing: CaptureRouting::default(),
            show_overlays_in_recordings: false,
            capsule_flow: CapsuleFlow::default(),
        }
    }
}

/// Scrawl's router settings (`scrawl_capture::RouterConfig`, its
/// camelCase JSON). Never holds a Jev key: Jev is called with
/// `$OPENCODE_API_KEY` or OpenCode's own credential, so `hex settings`
/// never shows one.
#[cfg(hex_capture)]
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(
    from = "scrawl_capture::RouterConfig",
    into = "scrawl_capture::RouterConfig"
)]
pub struct CaptureRouting(scrawl_capture::RouterConfig);

// The config's only non-`Eq` field is a JSON value, which compares totally.
#[cfg(hex_capture)]
impl Eq for CaptureRouting {}

#[cfg(hex_capture)]
impl From<scrawl_capture::RouterConfig> for CaptureRouting {
    fn from(mut config: scrawl_capture::RouterConfig) -> Self {
        config.jev_api_key = None;
        Self(config)
    }
}

#[cfg(hex_capture)]
impl From<CaptureRouting> for scrawl_capture::RouterConfig {
    fn from(routing: CaptureRouting) -> Self {
        routing.0
    }
}

#[cfg(hex_capture)]
impl CaptureRouting {
    pub fn config(&self) -> &scrawl_capture::RouterConfig {
        &self.0
    }
}

#[cfg(hex_capture)]
impl CaptureSettings {
    /// The folder new captures go to.
    pub fn captures_dir(&self) -> PathBuf {
        self.folder
            .clone()
            .unwrap_or_else(scrawl_capture::handoff::builtin_captures_dir)
    }

    pub fn retention(&self) -> scrawl_capture::retention::Policy {
        scrawl_capture::retention::Policy {
            media_days: self.media_days,
            bundle_days: self.bundle_days,
        }
    }

    pub fn set_retention(&mut self, policy: scrawl_capture::retention::Policy) {
        self.media_days = policy.media_days;
        self.bundle_days = policy.bundle_days;
    }
}

/// What turns a capture's narration into words.
#[cfg(hex_capture)]
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureTranscriber {
    /// The dictation model Hex already has loaded, with word times.
    #[default]
    Parakeet,
    /// Apple's speech recognizer, as Scrawl uses by default.
    Apple,
}

#[cfg(hex_capture)]
impl CaptureTranscriber {
    pub const ALL: [Self; 2] = [Self::Parakeet, Self::Apple];
}

pub use crate::dictation::text_replacements::TextReplacement;

impl RecordingAudioBehavior {
    pub const ALL: [Self; 4] = [Self::Mute, Self::PauseMedia, Self::Duck, Self::DoNothing];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Mute => "Mute",
            Self::Duck => "Lower audio",
            Self::PauseMedia => "Pause media",
            Self::DoNothing => "Do nothing",
        }
    }

    const fn encoded(self) -> u8 {
        match self {
            Self::Mute => 0,
            Self::Duck => 3,
            Self::PauseMedia => 1,
            Self::DoNothing => 2,
        }
    }

    fn decode(value: u8) -> Self {
        match value {
            3 => Self::Duck,
            1 => Self::PauseMedia,
            2 => Self::DoNothing,
            _ => Self::Mute,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct AppSettings {
    /// Retired spoken-commands opt-in, kept untouched in `settings.json`.
    #[serde(
        default,
        rename = "commands_enabled",
        deserialize_with = "deserialize_retired_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub retired_commands_enabled: Option<serde_json::Value>,
    pub release_microphone_while_idle: bool,
    pub sound_effects: bool,
    pub sound_effect_volume: f32,
    /// Which feedback sounds play (`crate::feedback`).
    pub sound_theme: crate::feedback::SoundSet,
    pub microphone: Option<String>,
    /// System default records from the built-in microphone, else another
    /// real input, rather than a Bluetooth default.
    pub avoid_bluetooth_microphone: bool,
    pub recording_audio_behavior: RecordingAudioBehavior,
    /// Fraction of the original output volume while ducking.
    pub recording_duck_volume: f32,
    /// With Pause media on macOS, also pause a browser that is audibly playing.
    pub pause_browser_media: bool,
    pub double_tap_lock: bool,
    pub double_tap_only: bool,
    pub shortcut_mode: crate::dictation::ShortcutMode,
    pub dictation_hotkey: HotkeyBinding,
    /// Retired Voice Action shortcut, kept untouched in `settings.json`.
    #[serde(
        default,
        rename = "edit_hotkey",
        deserialize_with = "deserialize_retired_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub retired_edit_hotkey: Option<serde_json::Value>,
    pub paste_last_hotkey: Option<HotkeyBinding>,
    /// Dictate without processing: records like the dictation shortcut and
    /// pastes the transcript as heard. Unbound by default.
    pub raw_dictation_hotkey: Option<HotkeyBinding>,
    /// Labs › Second dictation shortcut: another shortcut that dictates
    /// exactly as `dictation_hotkey` does (macOS). Unbound by default and
    /// ignored while its Labs toggle is off.
    pub second_dictation_hotkey: Option<HotkeyBinding>,
    pub show_dock_icon: bool,
    pub transcription: TranscriptionSelection,
    pub transcription_recents: Vec<TranscriptionSelection>,
    pub keyboard_model_switching: KeyboardModelSwitching,
    pub dictation_processing: DictationProcessingSettings,
    /// Retired Voice Action settings, kept untouched in `settings.json`.
    #[serde(
        default,
        rename = "voice_action",
        deserialize_with = "deserialize_retired_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub retired_voice_action: Option<serde_json::Value>,
    pub history_retention: crate::history::HistoryRetention,
    pub meetings: MeetingSettings,
    #[cfg(hex_capture)]
    pub capture: CaptureSettings,
    /// Capture settings from a build with captures, kept as they were.
    #[cfg(not(hex_capture))]
    #[serde(rename = "capture", skip_serializing_if = "Option::is_none")]
    pub capture_from_other_build: Option<serde_json::Value>,
    #[serde(default)]
    pub experimental: ExperimentalSettings,
    /// The Labs features' own switches (`crate::labs`).
    pub labs: LabsSettings,
    /// Start listening when Hex opens (Windows and Linux, where the
    /// listener can be off).
    pub enable_on_launch: bool,
    /// Paste with Ctrl-Shift-V for terminals (Linux and Windows).
    pub paste_with_shift: bool,
    /// Let other apps see the dictation shortcut's key presses too
    /// (Windows), so a push-to-talk key can drive both.
    pub share_dictation_shortcut: bool,
    /// A mouse-button shortcut went back to its default because Labs ›
    /// Mouse shortcuts is off; the window says so once, then clears it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub mouse_shortcut_notice: bool,
    /// The interface language code; `None` follows the system language.
    pub ui_language: Option<String>,
    pub indicator_position: IndicatorPosition,
    pub updates: UpdateSettings,
    /// The shared vocabulary (`crate::dictation::vocabulary`). Left out of the file
    /// while empty, so settings without one stay as they were.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub vocabulary: Vec<crate::dictation::vocabulary::VocabularyEntry>,
    /// How readily transcription swaps heard words for a vocabulary term.
    pub vocabulary_boost: crate::dictation::vocabulary::VocabularyBoost,
    /// One-time settings changes already applied. A file that predates a
    /// change lacks its ID, so the change runs once on the next load; new
    /// settings start with every ID because they need none.
    #[serde(default)]
    pub migrations: Vec<String>,
    #[serde(skip_serializing)]
    pub text_replacements: Vec<TextReplacement>,
}

/// Moves English dictation to the streaming English model once.
const STREAMING_ENGLISH_MIGRATION: &str = "streaming-english-model";
/// Annotate became a default feature: on for everyone once.
#[cfg(hex_capture)]
const ANNOTATE_DEFAULT_MIGRATION: &str = "annotate-on-by-default";
/// Settings saved by an `anomaly.hex` build. Up to 2.3.1 Hex was
/// `com.kitlangton.hex2` and used the same folder, so a file without this
/// ID may carry onboarding finished under grants macOS no longer gives the
/// new identity; the macOS launch checks setup once
/// (`onboarding::recheck_after_identity_change`).
const ANOMALY_IDENTITY_MIGRATION: &str = "anomaly-hex-identity";
#[cfg(hex_capture)]
const MIGRATIONS: &[&str] = &[
    STREAMING_ENGLISH_MIGRATION,
    ANNOTATE_DEFAULT_MIGRATION,
    ANOMALY_IDENTITY_MIGRATION,
    MEETING_RETENTION_DEFAULT_MIGRATION,
];
#[cfg(not(hex_capture))]
const MIGRATIONS: &[&str] = &[
    STREAMING_ENGLISH_MIGRATION,
    ANOMALY_IDENTITY_MIGRATION,
    MEETING_RETENTION_DEFAULT_MIGRATION,
];
/// Scrawl's settings were read into `capture` once. Left out of
/// `MIGRATIONS` so new settings import them too.
#[cfg(hex_capture)]
const SCRAWL_IMPORT_MIGRATION: &str = "scrawl-capture-settings";
/// Scrawl's router settings were read into `capture.routing` once: its own
/// migration, as it came after the first import.
#[cfg(hex_capture)]
const SCRAWL_ROUTING_IMPORT_MIGRATION: &str = "scrawl-routing-settings";
/// Meeting retention first saved `forever` as its default; that default is
/// now unchosen (24 hours for new meetings, older ones kept).
const MEETING_RETENTION_DEFAULT_MIGRATION: &str = "meeting-retention-default";

fn deserialize_retired_value<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde_json::Value::deserialize(deserializer).map(Some)
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            retired_commands_enabled: None,
            release_microphone_while_idle: false,
            sound_effects: true,
            sound_effect_volume: 0.5,
            sound_theme: crate::feedback::SoundSet::default(),
            microphone: None,
            avoid_bluetooth_microphone: true,
            recording_audio_behavior: RecordingAudioBehavior::DoNothing,
            recording_duck_volume: 0.2,
            pause_browser_media: false,
            double_tap_lock: true,
            double_tap_only: false,
            shortcut_mode: crate::dictation::ShortcutMode::Hold,
            dictation_hotkey: HotkeyBinding::default(),
            retired_edit_hotkey: None,
            paste_last_hotkey: Some(HotkeyBinding::paste_last_default()),
            raw_dictation_hotkey: None,
            second_dictation_hotkey: None,
            show_dock_icon: true,
            transcription: default_transcription(),
            transcription_recents: Vec::new(),
            keyboard_model_switching: KeyboardModelSwitching::default(),
            dictation_processing: DictationProcessingSettings::default(),
            retired_voice_action: None,
            history_retention: crate::history::HistoryRetention::default(),
            meetings: MeetingSettings::default(),
            #[cfg(hex_capture)]
            capture: CaptureSettings::default(),
            #[cfg(not(hex_capture))]
            capture_from_other_build: None,
            experimental: ExperimentalSettings::default(),
            labs: LabsSettings::default(),
            enable_on_launch: true,
            paste_with_shift: false,
            share_dictation_shortcut: false,
            mouse_shortcut_notice: false,
            ui_language: None,
            indicator_position: IndicatorPosition::default(),
            updates: UpdateSettings::default(),
            vocabulary: Vec::new(),
            vocabulary_boost: crate::dictation::vocabulary::VocabularyBoost::default(),
            migrations: MIGRATIONS.iter().map(|id| id.to_string()).collect(),
            text_replacements: Vec::new(),
        }
    }
}

/// macOS input-source IDs mapped to the model and language they select. An
/// absent entry means "Don't switch"; this never changes the model merely
/// because dictation starts.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct KeyboardModelSwitching {
    pub enabled: bool,
    pub mappings: std::collections::BTreeMap<String, KeyboardModelChoice>,
}

/// What one input source selects: a model and a language it supports.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KeyboardModelChoice {
    pub model: TranscriptionModelId,
    pub language: String,
}

impl KeyboardModelChoice {
    /// The picker's downloadable models only; Apple Speech is never mapped.
    pub fn validate(&self) -> Result<()> {
        let selection = TranscriptionSelection {
            model: self.model,
            language: self.language.clone(),
        };
        // An import stays mapped while it is missing or off, like the
        // saved selection; switching to it then runs the default model.
        if self.model.is_custom() {
            return crate::transcription::models::validate_saved(&selection);
        }
        let model = crate::transcription::models::validate(&selection)?;
        match model.runtime {
            crate::transcription::models::ModelRuntime::Gguf(_) => Ok(()),
            crate::transcription::models::ModelRuntime::AppleSpeech => {
                Err(eyre!("{} can't be switched to by input source", model.name))
            }
        }
    }
}

/// A new install starts with the model recommended for the person's
/// language, so first-run setup can download it without asking.
fn default_transcription() -> TranscriptionSelection {
    crate::transcription::models::recommended_selection(&crate::desktop::i18n::preferred_language())
}

impl AppSettings {
    pub fn transcription_for_model(&self, id: TranscriptionModelId) -> TranscriptionSelection {
        if self.transcription.model == id {
            return self.transcription.clone();
        }
        if let Some(selection) = self.transcription_recents.iter().rev().find(|selection| {
            selection.model == id && crate::transcription::models::validate_saved(selection).is_ok()
        }) {
            return selection.clone();
        }
        let model = crate::transcription::models::definition(id);
        TranscriptionSelection {
            model: id,
            language: model.language_for(&self.transcription.language).into(),
        }
    }

    pub fn remember_transcription(&mut self, selection: TranscriptionSelection) {
        self.transcription_recents.push(self.transcription.clone());
        self.transcription_recents.push(selection.clone());
        self.normalize_transcription_recents();
        self.transcription = selection;
    }

    fn normalize_transcription_recents(&mut self) {
        let mut seen = std::collections::HashSet::new();
        self.transcription_recents.reverse();
        self.transcription_recents.retain(|selection| {
            crate::transcription::models::validate_saved(selection).is_ok()
                && seen.insert(selection.model)
        });
        self.transcription_recents.reverse();
    }

    pub fn save_transcription(&mut self, selection: TranscriptionSelection) -> Result<()> {
        self.save_transcription_with(selection, Self::save)
    }

    fn save_transcription_with(
        &mut self,
        selection: TranscriptionSelection,
        save: impl FnOnce(&Self) -> Result<()>,
    ) -> Result<()> {
        crate::transcription::models::validate(&selection)?;
        let mut candidate = self.clone();
        candidate.remember_transcription(selection);
        self.commit_with(candidate, save)
    }

    /// Persist the entire candidate before replacing the current editor value.
    /// The candidate may include unrelated debounced edits, which must survive
    /// both failed immediate changes and successful commits.
    pub(crate) fn commit_with(
        &mut self,
        candidate: Self,
        save: impl FnOnce(&Self) -> Result<()>,
    ) -> Result<()> {
        save(&candidate)?;
        *self = candidate;
        Ok(())
    }

    fn normalize_double_tap_settings(&mut self) {
        if !self.double_tap_lock || self.dictation_hotkey.key.is_none() {
            self.double_tap_only = false;
        }
    }

    pub fn load() -> Result<Self> {
        let path = path()?;
        #[cfg_attr(not(hex_capture), allow(unused_mut))]
        let mut settings = Self::load_from(&path)?;
        #[cfg(hex_capture)]
        {
            let read = |path: PathBuf| fs::read(path).ok();
            let engine = read(scrawl_capture::handoff::default_config_path());
            let mut imported = false;
            if !settings.has_migration(SCRAWL_IMPORT_MIGRATION) {
                settings.import_scrawl_settings(
                    engine.as_deref(),
                    dirs::data_dir()
                        .and_then(|data| read(data.join("scrawl/config.json")))
                        .as_deref(),
                );
                settings.migrations.push(SCRAWL_IMPORT_MIGRATION.into());
                imported = true;
            }
            if !settings.has_migration(SCRAWL_ROUTING_IMPORT_MIGRATION) {
                settings.import_scrawl_routing(engine.as_deref());
                settings
                    .migrations
                    .push(SCRAWL_ROUTING_IMPORT_MIGRATION.into());
                imported = true;
            }
            if imported
                && path.exists()
                && persistence::healthy_file(&path)
                && let Err(error) = settings.write_to(&path)
            {
                tracing::warn!(%error, "could not record the Scrawl settings import");
            }
        }
        settings.apply_runtime();
        Ok(settings)
    }

    /// Reads Scrawl's settings into `capture`, never writing Scrawl's
    /// files: `engine` is `~/.config/scrawl/config.json` (transcriber,
    /// captures folder, retention) and `app` is the app's shortcut file.
    /// Missing or unreadable values keep Hex's defaults.
    #[cfg(hex_capture)]
    fn import_scrawl_settings(&mut self, engine: Option<&[u8]>, app: Option<&[u8]>) {
        if let Some(config) = engine.and_then(|bytes| {
            serde_json::from_slice::<serde_json::Value>(bytes)
                .ok()
                .filter(serde_json::Value::is_object)
        }) {
            if config
                .get("transcriber")
                .and_then(serde_json::Value::as_str)
                == Some("apple")
            {
                self.capture.transcriber = CaptureTranscriber::Apple;
            }
            let home = dirs::home_dir();
            if let Some(folder) = config
                .get("capturesDir")
                .or_else(|| config.get("captures_dir"))
                .and_then(serde_json::Value::as_str)
                .map(|dir| match (dir.strip_prefix("~/"), &home) {
                    (Some(rest), Some(home)) => home.join(rest),
                    _ => PathBuf::from(dir),
                })
                .filter(|dir| dir.is_absolute())
            {
                self.capture.folder = Some(folder);
            }
            if let Some(policy) = config
                .get("retention")
                .and_then(|value| serde_json::from_value(value.clone()).ok())
            {
                self.capture.set_retention(policy);
            }
        }
        if let Some(binding) = app.and_then(scrawl_shortcut)
            && !self.hotkey_in_use(HotkeySlot::Capture, &binding)
        {
            self.capture.hotkey = binding;
        }
    }

    /// Reads the router's settings from Scrawl's `config.json` (`engine`):
    /// its rules, `autoSend`, and the `smartRouting` and `jevRouting`
    /// experiments, leniently as Scrawl reads them, leaving out the Jev key.
    /// A missing or unreadable file keeps the defaults.
    #[cfg(hex_capture)]
    fn import_scrawl_routing(&mut self, engine: Option<&[u8]>) {
        let Some(serde_json::Value::Object(root)) =
            engine.and_then(|bytes| serde_json::from_slice(bytes).ok())
        else {
            return;
        };
        let mut warnings = Vec::new();
        self.capture.routing =
            scrawl_capture::router_config_from_scrawl(&root, &mut warnings).into();
        // A warning quotes the value it dropped, which may be the Jev key.
        if !warnings.is_empty() {
            tracing::info!(
                count = warnings.len(),
                "Scrawl routing settings were left out"
            );
        }
    }

    /// The saved `labs` switches, read without loading, normalizing, or
    /// writing anything else.
    pub fn saved_labs() -> LabsSettings {
        #[derive(Deserialize)]
        struct File {
            #[serde(default)]
            labs: LabsSettings,
        }
        path()
            .ok()
            .and_then(|path| fs::read(path).ok())
            .and_then(|data| serde_json::from_slice::<File>(&data).ok())
            .map(|file| file.labs)
            .unwrap_or_default()
    }

    /// The saved `updates.channel`, read without loading, normalizing, or
    /// writing anything else, so the updater thread never races a save.
    pub fn saved_update_channel() -> Option<UpdateChannel> {
        #[derive(Deserialize)]
        struct File {
            #[serde(default)]
            updates: UpdateSettings,
        }
        let data = fs::read(path().ok()?).ok()?;
        serde_json::from_slice::<File>(&data).ok()?.updates.channel
    }

    pub fn save(&self) -> Result<()> {
        let path = path()?;
        self.write_to(&path)?;
        self.apply_runtime();
        Ok(())
    }

    fn validate_recording_duck_volume(&self) -> Result<()> {
        if !self.recording_duck_volume.is_finite()
            || !(0.0..=1.0).contains(&self.recording_duck_volume)
        {
            return Err(eyre!("recording_duck_volume must be between 0 and 1"));
        }
        Ok(())
    }

    fn apply_runtime(&self) {
        MICROPHONE_POLICY.store(
            u8::from(self.release_microphone_while_idle),
            Ordering::Release,
        );
        crate::audio::set_avoid_bluetooth_microphone(self.avoid_bluetooth_microphone);
        CUSTOM_TRANSFORMATIONS_ENABLED.store(
            transformations_used(&self.dictation_processing),
            Ordering::Release,
        );
        crate::feedback::set_enabled(self.sound_effects);
        crate::feedback::set_volume(self.sound_effect_volume.clamp(0.0, 1.0));
        crate::feedback::set_sound_set(self.sound_theme);
        crate::desktop::i18n::apply(self.ui_language.as_deref());
        INDICATOR_POSITION.store(self.indicator_position as u8, Ordering::Relaxed);
        RECORDING_DUCK_VOLUME.store(self.recording_duck_volume.to_bits(), Ordering::Relaxed);
        RECORDING_AUDIO_BEHAVIOR.store(self.recording_audio_behavior.encoded(), Ordering::Relaxed);
        PAUSE_BROWSER_MEDIA.store(self.pause_browser_media, Ordering::Relaxed);
        DOUBLE_TAP_LOCK.store(self.double_tap_lock, Ordering::Relaxed);
        DOUBLE_TAP_ONLY.store(
            self.double_tap_lock && self.double_tap_only && self.dictation_hotkey.key.is_some(),
            Ordering::Relaxed,
        );
        SHORTCUT_TOGGLE.store(
            self.shortcut_mode == crate::dictation::ShortcutMode::Toggle,
            Ordering::Relaxed,
        );
        MEETINGS_ENABLED.store(
            self.meetings.available() && crate::labs::Feature::Meetings.shipped(),
            Ordering::Release,
        );
        crate::labs::apply(&self.labs);
        #[cfg(hex_capture)]
        {
            *CAPTURE_SETTINGS
                .get_or_init(Default::default)
                .write()
                .unwrap_or_else(|error| error.into_inner()) = self.capture.clone();
            CAPTURE_ON.store(self.capture_on(), Ordering::Release);
            crate::capture_host::record_own_windows(self.capture.show_overlays_in_recordings);
            let flow = CapsuleFlow::ALL
                .iter()
                .position(|flow| *flow == self.capture.capsule_flow)
                .unwrap_or_default();
            CAPSULE_FLOW.store(flow as u8, Ordering::Release);
        }
        HEX_MICROPHONE_ENABLED.store(self.experimental.hex_microphone, Ordering::Release);
        crate::audio::hex_microphone::sync_from_settings(
            self.experimental.hex_microphone_mode,
            self.experimental.hex_microphone_comfort_noise,
            self.experimental.hex_microphone_strength,
        );
        MEETING_NOTES_ENABLED.store(
            self.meetings.summarize_with_apple_intelligence,
            Ordering::Release,
        );
        MEETING_NOTES_OPENCODE.store(self.meetings.summarize_with_opencode, Ordering::Release);
        MEETING_INDICATOR_ENABLED.store(self.meetings.show_recording_indicator, Ordering::Release);
        MEETING_INDICATOR_CORNER.store(
            self.meetings.recording_indicator_corner as u8,
            Ordering::Release,
        );
        MEETING_RETENTION.store(
            MeetingRetention::encode(self.meetings.retention),
            Ordering::Release,
        );
        *MEETING_OUTPUT_FOLDER
            .get_or_init(Default::default)
            .write()
            .unwrap_or_else(|error| error.into_inner()) =
            Some(self.meetings.resolved_output_folder());
        *MEETING_PROVIDERS
            .get_or_init(Default::default)
            .write()
            .unwrap_or_else(|error| error.into_inner()) = self.meetings.providers.clone();
        *CALENDAR_SETTINGS
            .get_or_init(Default::default)
            .write()
            .unwrap_or_else(|error| error.into_inner()) = self.meetings.calendar.clone();
        *HOTKEYS
            .get_or_init(Default::default)
            .write()
            .unwrap_or_else(|error| error.into_inner()) = self.runtime_hotkeys();
        // An import that can't run stays saved; the listener runs the
        // default model until it is fixed.
        set_transcription_selection(&crate::transcription::custom_models::runnable(
            &self.transcription,
        ));
        set_microphone_selection(self.microphone.as_deref());
        #[cfg(target_os = "macos")]
        crate::dictation::config::update_dictation_profiles(&self.dictation_processing);
        crate::dictation::vocabulary::apply(&self.vocabulary, self.vocabulary_boost);
    }

    /// Mode activations saved while Finder showed filename extensions carry a
    /// `.app` suffix that never matched the foreground application name.
    #[cfg(not(target_os = "macos"))]
    fn normalize_mode_application_names(&mut self) {}

    #[cfg(target_os = "macos")]
    fn normalize_mode_application_names(&mut self) {
        let modes = std::iter::once(&mut self.dictation_processing.default_mode)
            .chain(self.dictation_processing.modes.iter_mut());
        for mode in modes {
            let mut mode_migrated = false;
            for application in &mut mode.applications {
                let normalized =
                    crate::dictation::context::strip_bundle_extension(application).to_owned();
                if normalized != *application {
                    *application = normalized;
                    mode_migrated = true;
                }
            }
            if mode_migrated {
                mode.applications.sort_by_key(|name| name.to_lowercase());
                mode.applications.dedup();
            }
        }
    }

    fn migrate_legacy_replacements(&mut self) {
        if self.text_replacements.is_empty() {
            return;
        }
        if self
            .dictation_processing
            .default_mode
            .replacements
            .is_empty()
        {
            self.dictation_processing.default_mode.replacements = self.text_replacements.clone();
        }
        for mode in &mut self.dictation_processing.modes {
            if mode.replacements.is_empty() {
                mode.replacements = self.text_replacements.clone();
            }
        }
        self.text_replacements.clear();
    }

    /// Applies each one-time change this file has not seen, then records it
    /// so a later choice, such as switching back, sticks.
    fn apply_one_time_migrations(&mut self) {
        self.apply_one_time_migrations_with(|model| {
            crate::transcription::models::is_installed(model, "en")
        });
    }

    fn apply_one_time_migrations_with(
        &mut self,
        installed: impl Fn(&crate::transcription::models::ModelDefinition) -> bool,
    ) {
        if !self.has_migration(STREAMING_ENGLISH_MIGRATION) {
            self.switch_english_to_streaming_model(installed);
            self.migrations.push(STREAMING_ENGLISH_MIGRATION.into());
        }
        if !self.has_migration(MEETING_RETENTION_DEFAULT_MIGRATION) {
            if self.meetings.retention == Some(MeetingRetention::Forever) {
                self.meetings.retention = None;
            }
            self.migrations
                .push(MEETING_RETENTION_DEFAULT_MIGRATION.into());
        }
        #[cfg(hex_capture)]
        if !self.has_migration(ANNOTATE_DEFAULT_MIGRATION) {
            self.capture.enabled = true;
            self.migrations.push(ANNOTATE_DEFAULT_MIGRATION.into());
        }
    }

    fn has_migration(&self, id: &str) -> bool {
        self.migrations.iter().any(|applied| applied == id)
    }

    /// Whether these settings come from before the `anomaly.hex` identity.
    #[cfg(target_os = "macos")]
    pub(crate) fn predates_anomaly_identity(&self) -> bool {
        !self.has_migration(ANOMALY_IDENTITY_MIGRATION)
    }

    /// Records the identity check in the file at `path`, changing nothing else.
    #[cfg(target_os = "macos")]
    pub(crate) fn record_anomaly_identity(&mut self, path: &std::path::Path) -> Result<()> {
        if self.predates_anomaly_identity() {
            self.migrations.push(ANOMALY_IDENTITY_MIGRATION.into());
            self.write_to(path)?;
        }
        Ok(())
    }

    /// English dictation moves to Parakeet Unified English, which streams
    /// while the shortcut is held. The previous model stays downloaded and
    /// in the recent models, one click away. Other languages and Auto keep
    /// their model because Parakeet Unified is English-only. Only an installed
    /// model is switched to, so an upgrade never leaves dictation waiting on a
    /// download.
    fn switch_english_to_streaming_model(
        &mut self,
        installed: impl Fn(&crate::transcription::models::ModelDefinition) -> bool,
    ) {
        if self.transcription.language != "en"
            || self.transcription.model == TranscriptionModelId::ParakeetUnifiedEnglish
        {
            return;
        }
        let model =
            crate::transcription::models::definition(TranscriptionModelId::ParakeetUnifiedEnglish);
        if !installed(model) {
            return;
        }
        let selection = TranscriptionSelection {
            model: model.id,
            language: "en".into(),
        };
        if crate::transcription::models::validate(&selection).is_err() {
            return;
        }
        tracing::info!(
            from = self.transcription.model.as_str(),
            "switched English dictation to the streaming English model"
        );
        self.remember_transcription(selection);
    }

    fn migrate_disabled_transcription_model(&mut self) {
        if self.transcription.model.is_custom()
            || crate::transcription::models::definition(self.transcription.model).available()
        {
            return;
        }
        tracing::warn!(
            model = self.transcription.model.as_str(),
            "replaced a disabled transcription model"
        );
        self.transcription = TranscriptionSelection::default();
    }

    pub fn runtime_hotkeys(&self) -> RuntimeHotkeys {
        RuntimeHotkeys {
            dictation: self.dictation_hotkey.runtime(),
            paste_last: self.paste_last_hotkey.as_ref().map(HotkeyBinding::runtime),
            paste_meeting: self
                .meetings
                .available()
                .then(|| HotkeyBinding::paste_meeting_default().runtime()),
            #[cfg(hex_capture)]
            capture: self.capture_on().then(|| self.capture.hotkey.runtime()),
            #[cfg(not(hex_capture))]
            capture: None,
            raw_dictation: self
                .raw_dictation_hotkey
                .as_ref()
                .map(HotkeyBinding::runtime),
            second_dictation: self
                .active_second_dictation_hotkey()
                .map(HotkeyBinding::runtime),
        }
    }

    /// Labs › Second dictation shortcut's binding while it takes effect:
    /// macOS, its toggle on, bound, and sharing no key with another active
    /// shortcut. Otherwise Hex matches, suppresses, and checks nothing for
    /// it.
    pub fn active_second_dictation_hotkey(&self) -> Option<&HotkeyBinding> {
        if !cfg!(target_os = "macos") || !self.labs.second_dictation_shortcut {
            return None;
        }
        self.second_dictation_hotkey
            .as_ref()
            .filter(|binding| !self.hotkey_in_use(HotkeySlot::SecondDictation, binding))
    }

    /// Annotate is on: the person hasn't turned it off.
    #[cfg(hex_capture)]
    pub fn capture_on(&self) -> bool {
        self.capture.enabled
    }

    /// A shortcut saved as a bare left click swallowed every click, so its
    /// owner couldn't change it in Settings. Such a shortcut goes back to its
    /// default when settings load.
    /// With Labs › Mouse shortcuts off, a mouse-button shortcut (issue 209)
    /// goes back to its default and the window notes it once.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn repair_mouse_shortcuts(&mut self) {
        if self.labs.mouse_shortcuts {
            return;
        }
        let mouse = |binding: &HotkeyBinding| binding.key.as_ref().is_some_and(HotkeyKey::is_mouse);
        if mouse(&self.dictation_hotkey) {
            self.dictation_hotkey = HotkeyBinding::default();
            self.mouse_shortcut_notice = true;
        }
        if self.paste_last_hotkey.as_ref().is_some_and(mouse) {
            self.paste_last_hotkey = Some(HotkeyBinding::paste_last_default());
            self.mouse_shortcut_notice = true;
        }
        if self.raw_dictation_hotkey.as_ref().is_some_and(mouse) {
            self.raw_dictation_hotkey = None;
            self.mouse_shortcut_notice = true;
        }
        if self.second_dictation_hotkey.as_ref().is_some_and(mouse) {
            self.second_dictation_hotkey = None;
            self.mouse_shortcut_notice = true;
        }
        #[cfg(hex_capture)]
        if mouse(&self.capture.hotkey) {
            self.capture.hotkey = HotkeyBinding::capture_default();
            self.mouse_shortcut_notice = true;
        }
        if self.mouse_shortcut_notice {
            tracing::info!("reset mouse-button shortcuts: Labs › Mouse shortcuts is off");
        }
    }

    fn repair_bare_primary_clicks(&mut self) {
        if self.dictation_hotkey.is_bare_primary_click() {
            tracing::warn!("replaced a left-click dictation shortcut with the default");
            self.dictation_hotkey = HotkeyBinding::default();
        }
        if self
            .paste_last_hotkey
            .as_ref()
            .is_some_and(HotkeyBinding::is_bare_primary_click)
        {
            tracing::warn!("replaced a left-click paste-last shortcut with the default");
            self.paste_last_hotkey = Some(HotkeyBinding::paste_last_default());
        }
        if self
            .raw_dictation_hotkey
            .as_ref()
            .is_some_and(HotkeyBinding::is_bare_primary_click)
        {
            tracing::warn!("cleared a left-click raw dictation shortcut");
            self.raw_dictation_hotkey = None;
        }
        if self
            .second_dictation_hotkey
            .as_ref()
            .is_some_and(HotkeyBinding::is_bare_primary_click)
        {
            tracing::warn!("cleared a left-click second dictation shortcut");
            self.second_dictation_hotkey = None;
        }
        #[cfg(hex_capture)]
        if self.capture.hotkey.is_bare_primary_click() {
            tracing::warn!("replaced a left-click capture shortcut with the default");
            self.capture.hotkey = HotkeyBinding::capture_default();
        }
    }
}

pub fn hotkey_conflicts(
    candidate: &HotkeyBinding,
    others: impl IntoIterator<Item = HotkeyBinding>,
) -> bool {
    others
        .into_iter()
        .any(|binding| candidate.overlaps(&binding))
}

/// A shortcut Settings and `hex settings` record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeySlot {
    Dictation,
    PasteLast,
    RawDictation,
    #[cfg(hex_capture)]
    Capture,
    /// Labs › Second dictation shortcut (macOS).
    SecondDictation,
}

impl AppSettings {
    /// Whether `binding` overlaps a shortcut `slot` must not share: the other
    /// active shortcuts and, where meetings exist (macOS), meeting paste. The
    /// two capture shortcuts never share a key, on or off, since turning
    /// captures on can't pull them apart.
    pub fn hotkey_in_use(&self, slot: HotkeySlot, binding: &HotkeyBinding) -> bool {
        let mut others = Vec::new();
        if slot != HotkeySlot::Dictation {
            others.push(self.dictation_hotkey.clone());
        }
        if slot != HotkeySlot::PasteLast
            && let Some(paste) = &self.paste_last_hotkey
        {
            others.push(paste.clone());
        }
        if slot != HotkeySlot::RawDictation
            && let Some(raw) = &self.raw_dictation_hotkey
        {
            others.push(raw.clone());
        }
        if slot != HotkeySlot::SecondDictation
            && cfg!(target_os = "macos")
            && self.labs.second_dictation_shortcut
            && let Some(second) = &self.second_dictation_hotkey
        {
            others.push(second.clone());
        }
        if cfg!(target_os = "macos") && self.meetings.available() {
            others.push(HotkeyBinding::paste_meeting_default());
        }
        #[cfg(hex_capture)]
        if slot != HotkeySlot::Capture && self.capture_on() {
            others.push(self.capture.hotkey.clone());
        }
        hotkey_conflicts(binding, others)
    }
}

fn set_microphone_selection(device: Option<&str>) {
    let device = device
        .map(str::trim)
        .filter(|device| !device.is_empty())
        .map(str::to_string);
    let state = MICROPHONE_SELECTION.get_or_init(Default::default);
    let mut state = state.write().unwrap_or_else(|error| error.into_inner());
    if state.device != device {
        state.revision = state.revision.wrapping_add(1);
        state.device = device;
    }
}

pub fn microphone_selection() -> (u64, Option<String>) {
    let state = MICROPHONE_SELECTION.get_or_init(Default::default);
    let state = state.read().unwrap_or_else(|error| error.into_inner());
    (state.revision, state.device.clone())
}

fn set_transcription_selection(selection: &TranscriptionSelection) {
    let state = TRANSCRIPTION_SELECTION.get_or_init(Default::default);
    let mut state = state.write().unwrap_or_else(|error| error.into_inner());
    if state.selection != *selection {
        state.revision = state.revision.wrapping_add(1);
        state.selection = selection.clone();
    }
}

pub fn transcription_selection() -> (u64, TranscriptionSelection) {
    let state = TRANSCRIPTION_SELECTION.get_or_init(Default::default);
    let state = state.read().unwrap_or_else(|error| error.into_inner());
    (state.revision, state.selection.clone())
}

pub fn recording_duck_volume() -> f32 {
    f32::from_bits(RECORDING_DUCK_VOLUME.load(Ordering::Relaxed))
}

pub fn recording_audio_behavior() -> RecordingAudioBehavior {
    RecordingAudioBehavior::decode(RECORDING_AUDIO_BEHAVIOR.load(Ordering::Relaxed))
}

pub fn pause_browser_media() -> bool {
    PAUSE_BROWSER_MEDIA.load(Ordering::Relaxed)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MicrophonePolicy {
    pub release_while_idle: bool,
}

pub fn microphone_policy() -> MicrophonePolicy {
    let policy = MICROPHONE_POLICY.load(Ordering::Acquire);
    MicrophonePolicy {
        release_while_idle: policy != 0,
    }
}

/// Whether the global mode or any mode runs a transformation.
pub fn transformations_used(processing: &DictationProcessingSettings) -> bool {
    !processing.default_mode.transformations.is_empty()
        || processing
            .modes
            .iter()
            .any(|mode| !mode.transformations.is_empty())
}

/// Where the recording HUD appears, as last saved.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn indicator_position() -> IndicatorPosition {
    match INDICATOR_POSITION.load(Ordering::Relaxed) {
        1 => IndicatorPosition::Bottom,
        2 => IndicatorPosition::Hidden,
        _ => IndicatorPosition::Top,
    }
}

pub fn custom_transformations_enabled() -> bool {
    CUSTOM_TRANSFORMATIONS_ENABLED.load(Ordering::Acquire)
}

pub fn double_tap_lock() -> bool {
    DOUBLE_TAP_LOCK.load(Ordering::Relaxed)
}

pub fn double_tap_only() -> bool {
    DOUBLE_TAP_ONLY.load(Ordering::Relaxed)
}

pub fn shortcut_toggle() -> bool {
    SHORTCUT_TOGGLE.load(Ordering::Relaxed)
}

/// Whether meeting detection and recording may run: always in developer
/// builds, otherwise only after the user opts in.
pub fn meetings_enabled() -> bool {
    MEETINGS_ENABLED.load(Ordering::Acquire)
}

/// [`voice_filter_enabled`] for the running listener.
pub fn hex_microphone_enabled() -> bool {
    crate::labs::Feature::VoiceFilter.shipped()
        && voice_filter_enabled(
            HEX_MICROPHONE_ENABLED.load(Ordering::Acquire),
            crate::audio::hex_microphone::driver_bundle_installed(),
        )
}

/// Apple Intelligence may write meeting notes (macOS).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn meeting_notes_enabled() -> bool {
    MEETING_NOTES_ENABLED.load(Ordering::Acquire)
}

/// The OpenCode model may write meeting notes.
pub fn meeting_notes_with_opencode() -> bool {
    MEETING_NOTES_OPENCODE.load(Ordering::Acquire)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn meeting_indicator_enabled() -> bool {
    MEETING_INDICATOR_ENABLED.load(Ordering::Acquire)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn meeting_indicator_corner() -> MeetingIndicatorCorner {
    MeetingIndicatorCorner::decode(MEETING_INDICATOR_CORNER.load(Ordering::Acquire))
}

/// The user's choices for detected calls, by website or app.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn meeting_provider_choices() -> Vec<MeetingProviderChoice> {
    MEETING_PROVIDERS
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// The connected calendar's reminder settings, as last applied.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// [`MeetingSettings::retention`] for the running meeting controller.
/// The retention the user chose, `None` until they choose. A process that
/// never loaded settings (`hex mcp`, `hex meetings`) reads it from
/// `settings.json` without migrating or writing anything.
pub fn meeting_retention() -> Option<MeetingRetention> {
    match MEETING_RETENTION.load(Ordering::Acquire) {
        RETENTION_UNSET if cfg!(test) => None,
        RETENTION_UNSET => stored_meeting_retention(),
        value => MeetingRetention::decode(value),
    }
}

fn stored_meeting_retention() -> Option<MeetingRetention> {
    let data = fs::read(path().ok()?).ok()?;
    let settings: serde_json::Value = serde_json::from_slice(&data).ok()?;
    serde_json::from_value(settings.get("meetings")?.get("retention")?.clone()).ok()
}

pub fn calendar_settings() -> CalendarSettings {
    CALENDAR_SETTINGS
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// Sets one provider's policy; returns whether the settings changed.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn set_meeting_provider(meetings: &mut MeetingSettings, choice: MeetingProviderChoice) -> bool {
    match meetings
        .providers
        .iter_mut()
        .find(|known| known.key == choice.key)
    {
        Some(known) if known.policy == choice.policy => false,
        Some(known) => {
            known.policy = choice.policy;
            true
        }
        None => {
            meetings.providers.push(choice);
            true
        }
    }
}

/// Saves a choice made from a meeting capsule while the Settings window is
/// closed.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn save_meeting_provider(choice: MeetingProviderChoice) {
    match AppSettings::load() {
        Ok(mut settings) => {
            if set_meeting_provider(&mut settings.meetings, choice)
                && let Err(error) = settings.save()
            {
                tracing::warn!(%error, "could not save a meeting provider choice");
            }
        }
        Err(error) => tracing::warn!(%error, "could not load settings to save a meeting choice"),
    }
}

/// Saves the corner the recording capsule was dragged to.
pub fn save_meeting_indicator_corner(corner: MeetingIndicatorCorner) {
    match AppSettings::load() {
        Ok(mut settings) => {
            if settings.meetings.recording_indicator_corner == corner {
                return;
            }
            settings.meetings.recording_indicator_corner = corner;
            if let Err(error) = settings.save() {
                tracing::warn!(%error, "could not save the recording capsule's corner");
            }
        }
        Err(error) => tracing::warn!(%error, "could not load settings to save the capsule corner"),
    }
}

pub fn meeting_output_folder() -> PathBuf {
    MEETING_OUTPUT_FOLDER
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
        .unwrap_or_else(default_meeting_output_folder)
}

pub fn runtime_hotkeys() -> RuntimeHotkeys {
    #[cfg_attr(not(hex_capture), allow(unused_mut))]
    let mut hotkeys = *HOTKEYS
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner());
    #[cfg(hex_capture)]
    if scrawl_app_running() {
        hotkeys.capture = None;
    }
    hotkeys
}

pub fn dictation_hotkey() -> RuntimeHotkey {
    runtime_hotkeys().dictation
}

/// The saved capture settings (`cfg(hex_capture)` builds).
#[cfg(hex_capture)]
pub fn capture_settings() -> CaptureSettings {
    CAPTURE_SETTINGS
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// The capture shortcut's keycaps (`capture.hotkey`), without cloning the
/// capture settings.
#[cfg(hex_capture)]
pub fn capture_hotkey_keycaps() -> Vec<String> {
    CAPTURE_SETTINGS
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .hotkey
        .keycaps()
}

/// The saved [`CapsuleFlow`], without cloning the capture settings.
#[cfg(hex_capture)]
pub fn capsule_flow() -> CapsuleFlow {
    CapsuleFlow::ALL
        .get(usize::from(CAPSULE_FLOW.load(Ordering::Acquire)))
        .copied()
        .unwrap_or_default()
}

/// Annotate is on (`capture.enabled`).
#[cfg(hex_capture)]
pub fn capture_on() -> bool {
    CAPTURE_ON.load(Ordering::Acquire)
}

/// Scrawl's own app is running, so it owns the capture shortcut and Hex
/// doesn't match it.
#[cfg(hex_capture)]
pub fn scrawl_app_running() -> bool {
    SCRAWL_APP_RUNNING.load(Ordering::Acquire)
}

#[cfg(hex_capture)]
pub fn set_scrawl_app_running(running: bool) {
    SCRAWL_APP_RUNNING.store(running, Ordering::Release);
}

pub fn hotkey_capture_active() -> bool {
    HOTKEY_CAPTURE_ACTIVE.load(Ordering::Acquire)
}

pub fn set_hotkey_capture_active(active: bool) {
    HOTKEY_CAPTURE_ACTIVE.store(active, Ordering::Release);
}

/// The Dock policy while no Settings window is open. An open window always
/// shows the icon: hiding it deactivates Hex asynchronously, which would drop
/// that window behind other applications.
pub fn dock_icon_visible(show_dock_icon: bool, status_item_available: bool) -> bool {
    show_dock_icon || !status_item_available
}

#[cfg(target_os = "macos")]
pub fn set_dock_icon_visible(visible: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        tracing::warn!("Dock icon visibility must be changed on the main thread");
        return;
    };
    let application = NSApplication::sharedApplication(mtm);
    let policy = if visible {
        NSApplicationActivationPolicy::Regular
    } else {
        NSApplicationActivationPolicy::Accessory
    };
    if application.activationPolicy() != policy && !application.setActivationPolicy(policy) {
        tracing::warn!(visible, "could not change Dock icon visibility");
    }
}

#[cfg(target_os = "macos")]
pub fn hide_application() {
    let Some(marker) = MainThreadMarker::new() else {
        tracing::warn!("cannot hide Hex outside the main thread");
        return;
    };
    NSApplication::sharedApplication(marker).hide(None);
}

pub(crate) fn path() -> Result<PathBuf> {
    Ok(crate::app_paths::support_dir()?.join("settings.json"))
}

/// Whether settings were ever saved; a first run has none and onboards.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn exists() -> bool {
    path().is_ok_and(|path| path.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GNOME's window menu takes Alt+Space, so new GNOME setups dictate
    /// with Ctrl+Alt+Space; it shares no key with Paste last or Annotate.
    #[cfg(target_os = "linux")]
    #[test]
    fn gnome_dictates_with_a_key_gnome_leaves_free() {
        assert_eq!(
            HotkeyBinding::linux_dictation_default(false).keycaps(),
            ["Alt", "Space"]
        );
        let gnome = HotkeyBinding::linux_dictation_default(true);
        assert_eq!(gnome.keycaps(), ["Ctrl", "Alt", "Space"]);
        assert_ne!(gnome, HotkeyBinding::paste_last_default());
        #[cfg(hex_capture)]
        assert_ne!(gnome, HotkeyBinding::capture_default());
    }

    /// Annotate is on out of the box, so its shortcut and the rest of the
    /// defaults share no key on any OS.
    #[cfg(hex_capture)]
    #[test]
    fn annotates_default_shortcuts_share_no_key() {
        let settings = AppSettings::default();
        assert!(settings.capture.enabled);
        assert!(!settings.hotkey_in_use(HotkeySlot::Capture, &settings.capture.hotkey));
    }

    /// A settings file from before Annotate was on by default turns it on
    /// once.
    #[cfg(hex_capture)]
    #[test]
    fn annotate_turns_on_once_for_older_settings() {
        let mut settings = AppSettings {
            migrations: vec![STREAMING_ENGLISH_MIGRATION.into()],
            ..AppSettings::default()
        };
        settings.capture.enabled = false;
        settings.apply_one_time_migrations_with(|_| true);
        assert!(settings.capture.enabled);
        settings.capture.enabled = false;
        settings.apply_one_time_migrations_with(|_| true);
        assert!(!settings.capture.enabled, "only once");
    }

    #[test]
    fn keyboard_model_switching_is_opt_in_and_source_mappings_round_trip() {
        let mut settings: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!settings.keyboard_model_switching.enabled);
        assert!(settings.keyboard_model_switching.mappings.is_empty());
        settings.keyboard_model_switching.enabled = true;
        settings.keyboard_model_switching.mappings.insert(
            "com.apple.keylayout.Spanish".into(),
            KeyboardModelChoice {
                model: TranscriptionModelId::ParakeetV3,
                language: "es".into(),
            },
        );
        let restored: AppSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(
            restored.keyboard_model_switching,
            settings.keyboard_model_switching
        );
    }

    #[test]
    fn loading_drops_keyboard_mappings_to_a_language_the_model_lacks() {
        let dir = TestSettingsDir::new("keyboard-mappings");
        let mut settings = AppSettings::default();
        let russian = KeyboardModelChoice {
            model: TranscriptionModelId::ParakeetV3,
            language: "ru".into(),
        };
        settings
            .keyboard_model_switching
            .mappings
            .insert("Russian".into(), russian.clone());
        settings.keyboard_model_switching.mappings.insert(
            "Japanese".into(),
            KeyboardModelChoice {
                model: TranscriptionModelId::ParakeetUnifiedEnglish,
                language: "ja".into(),
            },
        );
        settings.write_to(&dir.path).unwrap();
        let loaded = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(
            loaded.keyboard_model_switching.mappings,
            [("Russian".to_owned(), russian)].into()
        );
    }

    #[test]
    fn loading_folds_saved_recognition_hints_into_the_vocabulary_once() {
        use crate::dictation::vocabulary::VocabularyEntry;
        let dir = TestSettingsDir::new("folded-hints");
        let mut file = serde_json::to_value(AppSettings {
            transcription: TranscriptionSelection {
                model: TranscriptionModelId::WhisperLargeV3Turbo,
                language: "en".into(),
            },
            vocabulary: vec![VocabularyEntry {
                term: "Rune".into(),
                sounds_like: vec!["rune's".into()],
            }],
            sound_effect_volume: 0.25,
            ..AppSettings::default()
        })
        .unwrap();
        file["transcription"]["recognition_hints"] = "Hex, rune\nKaffar".into();
        file["transcription_recents"] = serde_json::json!([
            {"model": "parakeet_v3", "language": "de", "recognition_hints": "Old; Hex"},
            {"model": "qwen3_asr06_b", "language": "en", "recognition_hints": "Rodolfo"},
        ]);
        fs::write(&dir.path, serde_json::to_vec(&file).unwrap()).unwrap();
        let loaded = AppSettings::load_from(&dir.path).unwrap();
        let terms: Vec<_> = loaded
            .vocabulary
            .iter()
            .map(|entry| entry.term.as_str())
            .collect();
        assert_eq!(terms, ["Rune", "Hex", "Kaffar", "Rodolfo", "Old"]);
        assert_eq!(loaded.vocabulary[0].sounds_like, ["rune's"]);
        assert_eq!(loaded.sound_effect_volume, 0.25);
        let written = fs::read_to_string(&dir.path).unwrap();
        assert!(!written.contains("recognition_hints"));
        assert_eq!(
            AppSettings::load_from(&dir.path).unwrap().vocabulary,
            loaded.vocabulary
        );
    }

    /// Labs › Second dictation shortcut (issue 627): its binding takes
    /// effect only on macOS with its toggle on, never sharing a key with
    /// another shortcut, and the other shortcuts' checks count it.
    #[test]
    fn the_second_dictation_shortcut_needs_its_lab_and_its_own_keys() {
        let function = HotkeyBinding {
            modifiers: HotkeyModifiers {
                function: true,
                ..HotkeyModifiers::default()
            },
            key: None,
        };
        let mut settings = AppSettings {
            second_dictation_hotkey: Some(function.clone()),
            ..AppSettings::default()
        };
        assert!(!settings.labs.second_dictation_shortcut);
        assert_eq!(settings.runtime_hotkeys().second_dictation, None);
        assert!(!settings.hotkey_in_use(HotkeySlot::RawDictation, &function));

        settings.labs.second_dictation_shortcut = true;
        let active = settings.runtime_hotkeys().second_dictation;
        if cfg!(target_os = "macos") {
            assert_eq!(active, Some(function.runtime()));
            assert!(settings.hotkey_in_use(HotkeySlot::RawDictation, &function));
            assert!(settings.hotkey_in_use(HotkeySlot::Dictation, &function));
        } else {
            assert_eq!(active, None);
        }

        // The dictation shortcut's own keys are refused, and a saved overlap
        // is dropped rather than matched twice.
        let dictation = settings.dictation_hotkey.clone();
        assert!(settings.hotkey_in_use(HotkeySlot::SecondDictation, &dictation));
        settings.second_dictation_hotkey = Some(dictation);
        assert_eq!(settings.runtime_hotkeys().second_dictation, None);
    }

    #[test]
    fn ducking_settings_are_opt_in_bounded_and_round_trip() {
        let mut settings: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            settings.recording_audio_behavior,
            RecordingAudioBehavior::DoNothing
        );
        assert_eq!(settings.recording_duck_volume, 0.2);
        settings.recording_audio_behavior = RecordingAudioBehavior::Duck;
        settings.recording_duck_volume = 0.37;
        let decoded: AppSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(
            decoded.recording_audio_behavior,
            RecordingAudioBehavior::Duck
        );
        assert_eq!(decoded.recording_duck_volume, 0.37);
        for invalid in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
            settings.recording_duck_volume = invalid;
            assert!(settings.validate_recording_duck_volume().is_err());
        }
        for value in [0.0, 0.2, 1.0] {
            settings.recording_duck_volume = value;
            assert!(settings.validate_recording_duck_volume().is_ok());
        }
    }

    #[test]
    fn mouse_names_have_one_portable_representation() {
        for number in 1..=32 {
            let label = format!("Mouse {number}");
            let key = HotkeyKey::named(&label);
            assert!(key.is_mouse());
            assert_eq!(key.name(), format!("mouse{number}"));
            assert_eq!(key, HotkeyKey::mouse(number - 1).unwrap());
            assert_eq!(HotkeyKey::named(&key.name()), key);
        }
        for invalid in ["mouse0", "mouse33", "mouse", "mouse-1"] {
            assert_eq!(HotkeyKey::mouse_number(invalid), None);
        }
    }

    #[test]
    fn a_saved_bare_left_click_shortcut_returns_to_the_default_on_load() {
        let left_click = HotkeyBinding {
            modifiers: HotkeyModifiers::default(),
            key: HotkeyKey::mouse(0),
        };
        assert!(left_click.is_bare_primary_click());
        let option_click = HotkeyBinding {
            modifiers: HotkeyModifiers::option(),
            key: HotkeyKey::mouse(0),
        };
        assert!(!option_click.is_bare_primary_click());
        let back_button = HotkeyBinding {
            modifiers: HotkeyModifiers::default(),
            key: HotkeyKey::mouse(3),
        };
        assert!(!back_button.is_bare_primary_click());

        let dir = TestSettingsDir::new("left-click-shortcut");
        let settings = AppSettings {
            dictation_hotkey: left_click,
            ..AppSettings::default()
        };
        fs::write(&dir.path, serde_json::to_vec(&settings).unwrap()).unwrap();
        let loaded = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(loaded.dictation_hotkey, HotkeyBinding::default());

        let mut settings = AppSettings {
            dictation_hotkey: back_button.clone(),
            ..AppSettings::default()
        };
        // macOS keeps a mouse shortcut only with Labs › Mouse shortcuts on.
        settings.labs.mouse_shortcuts = true;
        fs::write(&dir.path, serde_json::to_vec(&settings).unwrap()).unwrap();
        let loaded = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(loaded.dictation_hotkey, back_button);
    }

    #[test]
    fn the_setting_or_an_installed_driver_turns_the_voice_filter_on_and_labs_never_does() {
        if env_hex_experimental_mic() {
            return;
        }
        assert!(!voice_filter_enabled(false, false));
        assert!(voice_filter_enabled(true, false));
        assert!(voice_filter_enabled(false, true));
    }

    #[test]
    fn mouse_shortcuts_go_back_to_their_default_while_their_labs_toggle_is_off() {
        let mouse = HotkeyBinding {
            modifiers: HotkeyModifiers::default(),
            key: HotkeyKey::mouse(3),
        };
        let mut settings = AppSettings {
            dictation_hotkey: mouse.clone(),
            ..Default::default()
        };
        settings.labs.mouse_shortcuts = true;
        settings.repair_mouse_shortcuts();
        assert_eq!(settings.dictation_hotkey, mouse);
        assert!(!settings.mouse_shortcut_notice);
        settings.labs.mouse_shortcuts = false;
        settings.repair_mouse_shortcuts();
        assert_eq!(settings.dictation_hotkey, HotkeyBinding::default());
        assert!(settings.mouse_shortcut_notice);
    }

    #[test]
    fn missing_fields_receive_defaults() {
        let settings: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.retired_commands_enabled, None);
        assert!(!settings.release_microphone_while_idle);
        assert!(settings.sound_effects);
        assert_eq!(settings.sound_effect_volume, 0.5);
        assert_eq!(settings.microphone, None);
        assert_eq!(
            settings.recording_audio_behavior,
            RecordingAudioBehavior::DoNothing
        );
        assert!(!settings.pause_browser_media);
        assert!(settings.double_tap_lock);
        assert!(!settings.double_tap_only);
        assert_eq!(settings.dictation_hotkey, HotkeyBinding::default());
        assert_eq!(
            settings.paste_last_hotkey,
            Some(HotkeyBinding::paste_last_default())
        );
        assert!(settings.show_dock_icon);
        assert_eq!(settings.transcription, AppSettings::default().transcription);
        assert!(
            !settings
                .dictation_processing
                .default_mode
                .post_processing
                .enabled
        );
        assert!(!settings.meetings.enabled);
        assert_eq!(settings.meetings.output_folder, None);
        assert!(settings.meetings.summarize_with_apple_intelligence);
        assert!(settings.meetings.show_recording_indicator);
        assert_eq!(
            settings.meetings.recording_indicator_corner,
            MeetingIndicatorCorner::TopRight
        );
        assert!(
            settings
                .meetings
                .resolved_output_folder()
                .ends_with("Hex Meetings")
        );
        assert!(settings.text_replacements.is_empty());
    }

    #[test]
    fn calendar_reminders_stay_off_until_connected_and_survive_older_files() {
        let older: AppSettings =
            serde_json::from_str(r#"{"meetings":{"enabled":true,"providers":[]}}"#).unwrap();
        assert_eq!(older.meetings.calendar, CalendarSettings::default());
        assert!(!older.meetings.calendar.connected);
        assert!(!older.meetings.calendar.reminders_active());
        assert_eq!(
            older.meetings.calendar.lead(),
            std::time::Duration::from_secs(60)
        );

        let partial: AppSettings =
            serde_json::from_str(r#"{"meetings":{"calendar":{"connected":true}}}"#).unwrap();
        assert!(partial.meetings.calendar.reminders_active());
        assert!(partial.meetings.calendar.includes("any-calendar"));

        let mut calendar = partial.meetings.calendar.clone();
        calendar.excluded_calendars.push("work".into());
        calendar.lead_minutes = 5;
        let written = serde_json::to_value(&calendar).unwrap();
        let reread: CalendarSettings = serde_json::from_value(written).unwrap();
        assert_eq!(reread, calendar);
        assert!(!reread.includes("work"));
        assert_eq!(reread.lead(), std::time::Duration::from_secs(300));

        // A hand-edited lead time outside the choices falls back to a minute.
        let odd: CalendarSettings = serde_json::from_str(r#"{"lead_minutes":7}"#).unwrap();
        assert_eq!(odd.lead(), std::time::Duration::from_secs(60));
    }

    #[test]
    fn meeting_providers_ask_by_default_and_legacy_never_offer_entries_stay_never() {
        let fresh: AppSettings = serde_json::from_str(r#"{"meetings":{"enabled":true}}"#).unwrap();
        assert!(fresh.meetings.providers.is_empty());

        let legacy: AppSettings = serde_json::from_str(
            r#"{"meetings":{"never_offer":[{"key":"problems.kitlangton.com","label":"problems.kitlangton.com"}]}}"#,
        )
        .unwrap();
        assert_eq!(
            legacy.meetings.providers,
            vec![MeetingProviderChoice {
                key: "problems.kitlangton.com".into(),
                label: "problems.kitlangton.com".into(),
                policy: MeetingPolicy::NeverOffer,
            }]
        );

        let mut meetings = legacy.meetings.clone();
        assert!(set_meeting_provider(
            &mut meetings,
            MeetingProviderChoice {
                key: "com.tinyspeck.slackmacgap".into(),
                label: "Slack huddles".into(),
                policy: MeetingPolicy::AlwaysRecord,
            }
        ));
        let written = serde_json::to_value(&meetings).unwrap();
        assert!(written.get("never_offer").is_none());
        assert_eq!(written["providers"][0]["policy"], "never_offer");
        assert_eq!(written["providers"][1]["policy"], "always_record");
        let reread: MeetingSettings = serde_json::from_value(written).unwrap();
        assert_eq!(reread, meetings);
        // Choosing Ask again keeps the row, so Settings still lists the site.
        let mut asked = reread.clone();
        assert!(set_meeting_provider(
            &mut asked,
            MeetingProviderChoice {
                policy: MeetingPolicy::Ask,
                ..reread.providers[0].clone()
            }
        ));
        assert_eq!(asked.providers.len(), 2);
        assert_eq!(asked.providers[0].policy, MeetingPolicy::Ask);
    }

    #[test]
    fn existing_meeting_settings_summarize_by_default() {
        let settings: AppSettings =
            serde_json::from_str(r#"{"meetings":{"enabled":true}}"#).unwrap();
        assert!(settings.meetings.enabled);
        assert!(settings.meetings.summarize_with_apple_intelligence);
        assert!(settings.meetings.show_recording_indicator);
        assert_eq!(
            settings.meetings.recording_indicator_corner,
            MeetingIndicatorCorner::TopRight
        );
    }

    fn english(model: TranscriptionModelId) -> String {
        format!(
            r#"{{"transcription":{{"model":"{}","language":"en"}}}}"#,
            model.as_str()
        )
    }

    #[test]
    fn english_dictation_switches_to_the_installed_streaming_model_once() {
        let mut settings: AppSettings =
            serde_json::from_str(&english(TranscriptionModelId::ParakeetV3)).unwrap();
        settings.apply_one_time_migrations_with(|_| true);
        assert_eq!(
            settings.transcription.model,
            TranscriptionModelId::ParakeetUnifiedEnglish
        );
        assert!(settings.has_migration(STREAMING_ENGLISH_MIGRATION));
        assert!(
            settings
                .transcription_recents
                .iter()
                .any(|recent| recent.model == TranscriptionModelId::ParakeetV3),
            "the previous model stays one click away"
        );

        // Switching back after the migration sticks.
        let dir = TestSettingsDir::new("streaming-english");
        let mut chosen = settings.clone();
        chosen.transcription.model = TranscriptionModelId::ParakeetV3;
        chosen.write_to(&dir.path).unwrap();
        let reloaded = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(
            reloaded.transcription.model,
            TranscriptionModelId::ParakeetV3
        );
    }

    #[test]
    fn english_dictation_keeps_its_model_when_the_streaming_model_is_missing() {
        let mut settings: AppSettings =
            serde_json::from_str(&english(TranscriptionModelId::ParakeetV3)).unwrap();
        settings.apply_one_time_migrations_with(|_| false);
        assert_eq!(
            settings.transcription.model,
            TranscriptionModelId::ParakeetV3
        );
        assert!(
            settings.has_migration(STREAMING_ENGLISH_MIGRATION),
            "a later download never switches the model behind the user's back"
        );
    }

    #[test]
    fn other_languages_and_auto_keep_their_model() {
        for language in ["de", "auto"] {
            let model = crate::transcription::models::choices_for_runtime(language)
                .iter()
                .map(|choice| choice.model.id)
                .find(|model| *model != TranscriptionModelId::ParakeetUnifiedEnglish)
                .expect("a model for the language");
            let dir = TestSettingsDir::new("streaming-other");
            fs::write(
                &dir.path,
                format!(
                    r#"{{"transcription":{{"model":"{}","language":"{language}"}}}}"#,
                    model.as_str()
                ),
            )
            .unwrap();
            let settings = AppSettings::load_from(&dir.path).unwrap();
            assert_eq!(settings.transcription.model, model);
            assert_eq!(settings.transcription.language, language);
            assert!(settings.has_migration(STREAMING_ENGLISH_MIGRATION));
        }
    }

    #[test]
    fn new_settings_need_no_one_time_migration() {
        assert!(AppSettings::default().has_migration(STREAMING_ENGLISH_MIGRATION));
        let dir = TestSettingsDir::new("streaming-new");
        let settings = AppSettings {
            transcription: TranscriptionSelection {
                model: TranscriptionModelId::ParakeetV3,
                language: "en".into(),
            },
            ..AppSettings::default()
        };
        settings.write_to(&dir.path).unwrap();
        let reloaded = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(
            reloaded.transcription.model,
            TranscriptionModelId::ParakeetV3
        );
    }

    #[test]
    fn vocabulary_round_trips_and_stays_out_of_files_without_one() {
        use crate::dictation::vocabulary::VocabularyEntry;
        // A file from before the vocabulary loads with an empty one and is
        // not rewritten to gain the field.
        let dir = TestSettingsDir::new("vocabulary");
        let older = r#"{"sound_effects": false}"#;
        fs::write(&dir.path, older).unwrap();
        let settings = AppSettings::load_from(&dir.path).unwrap();
        assert!(settings.vocabulary.is_empty());
        let written: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&dir.path).unwrap()).unwrap();
        assert!(written.get("vocabulary").is_none());
        assert!(
            serde_json::to_value(AppSettings::default())
                .unwrap()
                .get("vocabulary")
                .is_none()
        );

        let filled = AppSettings {
            vocabulary: vec![
                VocabularyEntry {
                    term: "Cappuccino".into(),
                    sounds_like: vec!["capuchin".into(), "cap a chino".into()],
                },
                VocabularyEntry {
                    term: "Rune".into(),
                    sounds_like: vec![],
                },
            ],
            ..settings
        };
        filled.write_to(&dir.path).unwrap();
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&dir.path).unwrap()).unwrap();
        assert_eq!(
            json["vocabulary"],
            serde_json::json!([
                {"term": "Cappuccino", "sounds_like": ["capuchin", "cap a chino"]},
                {"term": "Rune"},
            ])
        );
        assert_eq!(json["vocabulary_boost"], "normal");
        let reloaded = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(reloaded.vocabulary, filled.vocabulary);
    }

    struct TestSettingsDir {
        directory: PathBuf,
        path: PathBuf,
    }

    impl TestSettingsDir {
        fn new(prefix: &str) -> Self {
            let directory = std::env::temp_dir().join(format!(
                "hex-{prefix}-{}-{}",
                std::process::id(),
                SETTINGS_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&directory).unwrap();
            let path = directory.join("settings.json");
            Self { directory, path }
        }
    }

    impl Drop for TestSettingsDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    /// #744: a value this build can't read costs only that setting, and the
    /// file (perhaps a newer build's) is left as it was.
    #[test]
    fn an_unreadable_setting_keeps_its_default_and_the_rest_loads() {
        let dir = TestSettingsDir::new("lenient");
        let file = r#"{
            "sound_effects": false,
            "history_retention": null,
            "show_dock_icon": "sometimes",
            "meetings": {"enabled": true, "retention": "quarterly"},
            "recording_duck_volume": 7.0
        }"#;
        fs::write(&dir.path, file).unwrap();
        let settings = AppSettings::load_from(&dir.path).unwrap();
        let defaults = AppSettings::default();
        assert!(!settings.sound_effects);
        assert!(settings.meetings.enabled);
        assert_eq!(settings.history_retention, defaults.history_retention);
        assert_eq!(settings.show_dock_icon, defaults.show_dock_icon);
        assert_eq!(settings.meetings.retention, None);
        assert_eq!(
            settings.recording_duck_volume,
            defaults.recording_duck_volume
        );
        assert_eq!(
            fs::read_to_string(&dir.path).unwrap(),
            file,
            "not rewritten"
        );

        // `null` reads as unchosen, and unchosen is never written as `null`,
        // which 3.1.0 can't read.
        fs::write(&dir.path, r#"{"meetings": {"retention": null}}"#).unwrap();
        assert_eq!(
            AppSettings::load_from(&dir.path)
                .unwrap()
                .meetings
                .retention,
            None
        );
        let saved = serde_json::to_value(AppSettings::default()).unwrap();
        assert!(saved["meetings"].get("retention").is_none());

        // Even malformed JSON defaults in memory and stays in place.
        fs::write(&dir.path, "{ not json").unwrap();
        assert_eq!(AppSettings::load_from(&dir.path).unwrap(), defaults);
        assert_eq!(fs::read_to_string(&dir.path).unwrap(), "{ not json");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn new_settings_require_onboarding_across_reloads() {
        use crate::desktop::onboarding::{completion_recorded_at, record_completion_at};
        use std::os::unix::fs::PermissionsExt;

        let dir = TestSettingsDir::new("new-onboarding");
        assert!(!completion_recorded_at(&dir.directory));

        let mut settings = AppSettings::load_from(&dir.path).unwrap();
        assert!(!dir.path.exists());
        assert_eq!(settings.sound_effects, AppSettings::default().sound_effects);
        settings.sound_effects = false;
        settings.double_tap_lock = false;
        settings.write_to(&dir.path).unwrap();
        assert!(dir.path.is_file());
        assert!(!dir.directory.join("models").exists());
        assert!(!completion_recorded_at(&dir.directory));
        assert!(!dir.directory.join("onboarding-complete").exists());
        assert_eq!(
            fs::metadata(dir.directory.join("onboarding-pending"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600,
        );
        drop(settings);

        let reloaded = AppSettings::load_from(&dir.path).unwrap();
        assert!(!reloaded.sound_effects);
        assert!(!reloaded.double_tap_lock);
        reloaded.write_to(&dir.path).unwrap();
        assert!(!completion_recorded_at(&dir.directory));
        assert!(!dir.directory.join("onboarding-complete").exists());

        record_completion_at(&dir.directory).unwrap();
        assert!(completion_recorded_at(&dir.directory));
        drop(reloaded);
        AppSettings::load_from(&dir.path).unwrap();
        assert!(completion_recorded_at(&dir.directory));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn loading_strips_finder_bundle_extensions_from_mode_applications() {
        let dir = TestSettingsDir::new("mode-applications");
        fs::write(
            &dir.path,
            br#"{"dictation_processing":{"modes":[{"name":"Code","applications":["Zed.app","Ghostty.app","Zed"]}]}}"#,
        )
        .unwrap();

        let settings = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(
            settings.dictation_processing.modes[0].applications,
            vec!["Ghostty".to_string(), "Zed".to_string()]
        );
        let persisted: AppSettings = serde_json::from_slice(&fs::read(&dir.path).unwrap()).unwrap();
        assert_eq!(
            persisted.dictation_processing.modes[0].applications,
            vec!["Ghostty".to_string(), "Zed".to_string()]
        );
    }

    #[test]
    fn loading_persists_normalizations_only_when_they_change_settings() {
        let dir = TestSettingsDir::new("normalized-settings");
        fs::write(
            &dir.path,
            br#"{"text_replacements":[{"matched_phrase":"open code","output":"OpenCode"}]}"#,
        )
        .unwrap();

        let settings = AppSettings::load_from(&dir.path).unwrap();
        assert!(settings.text_replacements.is_empty());
        let persisted = fs::read_to_string(&dir.path).unwrap();
        assert!(!persisted.contains("text_replacements"));
        assert!(persisted.contains("OpenCode"));

        let written = fs::metadata(&dir.path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let reloaded = AppSettings::load_from(&dir.path).unwrap();
        assert_eq!(
            reloaded.dictation_processing.default_mode.replacements,
            settings.dictation_processing.default_mode.replacements
        );
        assert_eq!(
            fs::metadata(&dir.path).unwrap().modified().unwrap(),
            written
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn rewriting_legacy_rust_settings_preserves_onboarding_migration() {
        let dir = TestSettingsDir::new("legacy-onboarding");
        fs::write(&dir.path, b"{}").unwrap();

        let settings = AppSettings::load_from(&dir.path).unwrap();
        settings.write_to(&dir.path).unwrap();
        assert!(!dir.directory.join("onboarding-pending").exists());
        assert!(crate::desktop::onboarding::completion_recorded_at(
            &dir.directory
        ));
        assert!(dir.directory.join("onboarding-complete").is_file());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn first_settings_write_requires_the_pending_marker() {
        let dir = TestSettingsDir::new("pending-onboarding");
        fs::create_dir(dir.directory.join("onboarding-pending")).unwrap();

        assert!(AppSettings::default().write_to(&dir.path).is_err());
        assert!(!dir.path.exists());
    }

    #[test]
    fn legacy_sleep_prevention_setting_is_ignored() {
        let settings: AppSettings =
            serde_json::from_str(r#"{"prevent_system_sleep":false}"#).unwrap();
        let serialized = serde_json::to_value(settings).unwrap();

        assert!(serialized.get("prevent_system_sleep").is_none());
    }

    #[test]
    fn an_unset_microphone_is_system_default_and_a_saved_one_is_kept() {
        let automatic: AppSettings = serde_json::from_str(r#"{"microphone":null}"#).unwrap();
        assert_eq!(automatic.microphone, None);
        let absent: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(absent.microphone, None);
        let saved: AppSettings =
            serde_json::from_str(r#"{"microphone":"Studio Display Microphone"}"#).unwrap();
        assert_eq!(
            saved.microphone.as_deref(),
            Some("Studio Display Microphone")
        );
    }

    #[test]
    fn retired_commands_settings_load_cleanly_and_survive_saves() {
        let directory = std::env::temp_dir().join(format!(
            "hex-retired-commands-{}-{}",
            std::process::id(),
            SETTINGS_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.json");

        let mut initial = serde_json::to_value(AppSettings::default()).unwrap();
        assert!(initial.get("commands_enabled").is_none());
        initial["commands_enabled"] = serde_json::json!(true);
        initial["release_microphone_while_idle"] = serde_json::json!(true);
        let raw = serde_json::to_vec_pretty(&initial).unwrap();
        fs::write(&path, &raw).unwrap();

        let mut loaded = AppSettings::load_from(&path).unwrap();
        assert!(loaded.release_microphone_while_idle);
        assert_eq!(
            fs::read(&path).unwrap(),
            raw,
            "loading already-normalized settings does not rewrite retired keys away"
        );

        loaded.sound_effect_volume = 0.75;
        loaded.write_to(&path).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["commands_enabled"], serde_json::json!(true));
        assert_eq!(
            saved["release_microphone_while_idle"],
            serde_json::json!(true)
        );

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn retired_voice_action_settings_load_cleanly_and_survive_saves() {
        let directory = std::env::temp_dir().join(format!(
            "hex-retired-voice-action-{}-{}",
            std::process::id(),
            SETTINGS_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.json");

        let mut initial = serde_json::to_value(AppSettings::default()).unwrap();
        initial["edit_hotkey"] = serde_json::json!({
            "modifiers": {"option": "either", "command": "either"},
            "key": null
        });
        initial["voice_action"] = serde_json::json!({
            "enabled": true,
            "model": "anthropic/claude-sonnet-4-5",
            "variant": "fast",
            "deadline_seconds": 45
        });
        let raw = serde_json::to_vec_pretty(&initial).unwrap();
        fs::write(&path, &raw).unwrap();

        let mut loaded = AppSettings::load_from(&path).unwrap();
        assert_eq!(
            fs::read(&path).unwrap(),
            raw,
            "loading already-normalized settings does not rewrite retired keys away"
        );

        loaded.sound_effect_volume = 0.75;
        loaded.write_to(&path).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["edit_hotkey"], initial["edit_hotkey"]);
        assert_eq!(saved["voice_action"], initial["voice_action"]);

        let null_edit: AppSettings = serde_json::from_str(r#"{"edit_hotkey":null}"#).unwrap();
        assert_eq!(
            serde_json::to_value(null_edit).unwrap()["edit_hotkey"],
            serde_json::Value::Null
        );

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn transcription_selection_round_trips() {
        let settings = AppSettings {
            transcription: TranscriptionSelection {
                model: crate::transcription::models::TranscriptionModelId::WhisperLargeV3Turbo,
                language: "zh".into(),
            },
            ..AppSettings::default()
        };

        let encoded = serde_json::to_string(&settings).unwrap();
        let decoded: AppSettings = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.transcription, settings.transcription);
        assert!(crate::transcription::models::validate(&decoded.transcription).is_ok());
    }

    #[test]
    fn model_switches_remember_language_without_changing_other_settings() {
        let french = TranscriptionSelection {
            model: TranscriptionModelId::WhisperLargeV3Turbo,
            language: "fr".into(),
        };
        let mut settings = AppSettings {
            show_dock_icon: false,
            microphone: Some("Test microphone".into()),
            ..Default::default()
        };
        settings.remember_transcription(french.clone());
        assert_eq!(
            settings
                .transcription_for_model(TranscriptionModelId::ParakeetV2)
                .language,
            "en"
        );
        assert_eq!(
            settings
                .transcription_for_model(TranscriptionModelId::CohereTranscribe)
                .language,
            "fr"
        );
        settings.remember_transcription(TranscriptionSelection::default());
        let encoded = serde_json::to_vec(&settings).unwrap();
        let mut decoded: AppSettings = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(
            decoded.transcription_for_model(TranscriptionModelId::WhisperLargeV3Turbo),
            french
        );
        for _ in 0..100 {
            decoded.remember_transcription(french.clone());
            decoded.remember_transcription(TranscriptionSelection::default());
        }
        assert_eq!(decoded.transcription_recents.len(), 2);
        assert!(!decoded.show_dock_icon);
        assert_eq!(decoded.microphone.as_deref(), Some("Test microphone"));
        let old: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(old.transcription_recents.is_empty());
    }

    #[test]
    fn failed_model_selection_save_preserves_selection_recents_and_unsaved_edits() {
        let mut settings = AppSettings::default();
        settings.dictation_processing.default_mode.name = "Unsaved mode".into();
        let before = serde_json::to_value(&settings).unwrap();
        let candidate = TranscriptionSelection {
            model: TranscriptionModelId::WhisperLargeV3Turbo,
            language: "fr".into(),
        };
        let error = settings
            .save_transcription_with(candidate.clone(), |next| {
                assert_eq!(next.transcription, candidate);
                assert_eq!(next.dictation_processing.default_mode.name, "Unsaved mode");
                Err(eyre!("fixture save failed"))
            })
            .unwrap_err();
        assert_eq!(error.to_string(), "fixture save failed");
        assert_eq!(serde_json::to_value(&settings).unwrap(), before);
        settings
            .save_transcription_with(candidate.clone(), |_| Ok(()))
            .unwrap();
        assert_eq!(settings.transcription, candidate);
        assert_eq!(
            settings.dictation_processing.default_mode.name,
            "Unsaved mode"
        );
    }

    #[test]
    fn invalid_remembered_languages_are_not_offered_by_model_switching() {
        let mut settings = AppSettings {
            transcription_recents: vec![
                TranscriptionSelection {
                    model: TranscriptionModelId::ParakeetV2,
                    language: "fr".into(),
                },
                TranscriptionSelection {
                    model: TranscriptionModelId::WhisperLargeV3Turbo,
                    language: "not-a-language".into(),
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            settings
                .transcription_for_model(TranscriptionModelId::ParakeetV2)
                .language,
            "en"
        );
        assert_eq!(
            settings
                .transcription_for_model(TranscriptionModelId::WhisperLargeV3Turbo)
                .language,
            "en"
        );
        settings.normalize_transcription_recents();
        assert!(settings.transcription_recents.is_empty());
    }

    #[test]
    fn automatic_language_selection_round_trips_explicitly() {
        let settings = AppSettings {
            transcription: TranscriptionSelection {
                model: crate::transcription::models::TranscriptionModelId::WhisperLargeV3Turbo,
                language: crate::transcription::models::AUTO_LANGUAGE.into(),
            },
            ..AppSettings::default()
        };

        let encoded = serde_json::to_string(&settings).unwrap();
        let decoded: AppSettings = serde_json::from_str(&encoded).unwrap();

        assert!(encoded.contains(r#""language":"auto""#));
        assert_eq!(decoded.transcription, settings.transcription);
        assert!(crate::transcription::models::validate(&decoded.transcription).is_ok());
    }

    #[test]
    fn disabled_apple_speech_selection_migrates_to_the_default_model() {
        let mut settings: AppSettings =
            serde_json::from_str(r#"{"transcription":{"model":"apple_speech","language":"de"}}"#)
                .unwrap();

        settings.migrate_disabled_transcription_model();
        assert_eq!(settings.transcription, TranscriptionSelection::default());
    }

    #[test]
    fn legacy_global_replacements_migrate_into_every_mode() {
        let mut settings: AppSettings = serde_json::from_str(
            r#"{"text_replacements":[{"matched_phrase":"open code","output":"OpenCode"}]}"#,
        )
        .unwrap();
        settings.dictation_processing.modes.push(DictationMode {
            name: "Messages".into(),
            ..DictationMode::default()
        });
        settings.migrate_legacy_replacements();

        assert_eq!(
            settings.dictation_processing.default_mode.replacements,
            [TextReplacement {
                matched_phrase: "open code".into(),
                output: "OpenCode".into(),
            }]
        );
        assert_eq!(
            settings.dictation_processing.modes[0].replacements,
            settings.dictation_processing.default_mode.replacements
        );
        assert!(settings.text_replacements.is_empty());
        assert!(
            !serde_json::to_string(&settings)
                .unwrap()
                .contains("text_replacements")
        );
    }

    #[test]
    fn hotkey_runtime_preserves_modifier_sides_and_key_code() {
        let binding = HotkeyBinding {
            modifiers: HotkeyModifiers {
                control: Some(ModifierSide::Left),
                option: None,
                shift: Some(ModifierSide::Right),
                command: None,
                function: true,
            },
            key: Some(HotkeyKey {
                code: 49,
                label: "Space".into(),
            }),
        };
        let runtime = binding.runtime();

        assert_eq!(runtime.modifiers, binding.modifiers);
        assert_eq!(runtime.key_code, Some(49));
    }

    #[test]
    fn shortcut_mode_defaults_to_hold_and_round_trips() {
        let legacy: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy.shortcut_mode, crate::dictation::ShortcutMode::Hold);
        let settings = AppSettings {
            shortcut_mode: crate::dictation::ShortcutMode::Toggle,
            ..Default::default()
        };
        let decoded: AppSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(
            decoded.shortcut_mode,
            crate::dictation::ShortcutMode::Toggle
        );
    }

    #[test]
    fn modifier_sides_round_trip_and_legacy_booleans_decode_as_either() {
        let settings = AppSettings {
            dictation_hotkey: HotkeyBinding {
                modifiers: HotkeyModifiers {
                    option: Some(ModifierSide::Right),
                    command: Some(ModifierSide::Left),
                    ..Default::default()
                },
                key: Some(HotkeyKey {
                    code: 0,
                    label: "A".into(),
                }),
            },
            paste_last_hotkey: None,
            double_tap_only: true,
            ..Default::default()
        };

        let encoded = serde_json::to_string(&settings).unwrap();
        let decoded: AppSettings = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.dictation_hotkey, settings.dictation_hotkey);
        assert_eq!(decoded.paste_last_hotkey, None);
        assert!(decoded.double_tap_only);
        // Settings saved before dictation streamed still load.
        assert!(
            serde_json::from_str::<AppSettings>(r#"{"incremental_transcription":false}"#).is_ok()
        );

        let legacy: HotkeyModifiers = serde_json::from_str(
            r#"{"control":false,"option":true,"shift":false,"command":false,"function":false}"#,
        )
        .unwrap();
        assert_eq!(legacy.option, Some(ModifierSide::Either));
    }

    #[test]
    fn side_constraints_can_be_removed_from_chord_modifiers() {
        let modifiers = HotkeyModifiers {
            option: Some(ModifierSide::Left),
            command: Some(ModifierSide::Right),
            function: true,
            ..Default::default()
        };

        assert_eq!(
            modifiers.without_side_constraints(),
            HotkeyModifiers {
                option: Some(ModifierSide::Either),
                command: Some(ModifierSide::Either),
                function: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn side_aware_matching_and_conflicts_share_the_same_algebra() {
        let left = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Left),
                ..Default::default()
            },
            key: Some(HotkeyKey {
                code: 0,
                label: "A".into(),
            }),
        };
        let right = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Right),
                ..Default::default()
            },
            key: left.key.clone(),
        };
        let either = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Either),
                ..Default::default()
            },
            key: left.key.clone(),
        };

        assert!(!left.overlaps(&right));
        assert!(left.overlaps(&either));
        assert!(
            left.runtime()
                .matches_key_press(0, OPTION_KEY_MASK | LEFT_OPTION_MASK,)
        );
        assert!(
            !left
                .runtime()
                .matches_key_press(0, OPTION_KEY_MASK | RIGHT_OPTION_MASK,)
        );
        assert!(
            either
                .runtime()
                .matches_key_press(0, OPTION_KEY_MASK | RIGHT_OPTION_MASK,)
        );
    }

    #[test]
    fn runtime_side_constraints_require_the_requested_device_flag() {
        for (modifier, general, left, right) in [
            (
                "control",
                CONTROL_KEY_MASK,
                LEFT_CONTROL_MASK,
                RIGHT_CONTROL_MASK,
            ),
            (
                "option",
                OPTION_KEY_MASK,
                LEFT_OPTION_MASK,
                RIGHT_OPTION_MASK,
            ),
            ("shift", SHIFT_KEY_MASK, LEFT_SHIFT_MASK, RIGHT_SHIFT_MASK),
            (
                "command",
                COMMAND_KEY_MASK,
                LEFT_COMMAND_MASK,
                RIGHT_COMMAND_MASK,
            ),
        ] {
            for side in ["left", "right", "either"] {
                let binding: HotkeyBinding = serde_json::from_value(serde_json::json!({
                    "modifiers": { (modifier): side },
                    "key": { "code": 0, "label": "A" },
                }))
                .unwrap();
                let binding = binding.runtime();
                // Both sides held hold a sided binding's key, but only an
                // either-side binding matches them exactly.
                for (device_flags, held, exact) in [
                    (0, side == "either", side == "either"),
                    (left, side != "right", side != "right"),
                    (right, side != "left", side != "left"),
                    (left | right, true, side == "either"),
                ] {
                    let flags = general | device_flags;
                    assert_eq!(
                        binding.required_modifiers_down(flags),
                        held,
                        "{side:?}, flags={flags:#x}"
                    );
                    assert_eq!(
                        binding.matches_key_press(0, flags),
                        exact,
                        "{side:?}, flags={flags:#x}"
                    );
                    assert!(!binding.exact_modifiers(flags | FUNCTION_KEY_MASK));
                    assert_eq!(
                        binding.required_modifiers_down(flags | FUNCTION_KEY_MASK),
                        held
                    );
                }
            }
        }
    }

    #[test]
    fn default_paste_hotkeys_follow_build_capabilities() {
        for hotkeys in [
            RuntimeHotkeys::default(),
            AppSettings::default().runtime_hotkeys(),
        ] {
            assert_eq!(
                hotkeys.paste_last,
                Some(HotkeyBinding::paste_last_default().runtime())
            );
            #[cfg(debug_assertions)]
            assert_eq!(
                hotkeys.paste_meeting,
                Some(HotkeyBinding::paste_meeting_default().runtime())
            );
            #[cfg(not(debug_assertions))]
            assert_eq!(hotkeys.paste_meeting, None);
        }
        let mut opted_in = AppSettings::default();
        opted_in.meetings.enabled = true;
        assert_eq!(
            opted_in.runtime_hotkeys().paste_meeting,
            Some(HotkeyBinding::paste_meeting_default().runtime())
        );
    }

    #[test]
    fn side_selection_changes_one_modifier_chords_only() {
        let mut standalone = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Left),
                ..Default::default()
            },
            key: None,
        };
        assert_eq!(standalone.standalone_side(), Some(ModifierSide::Left));
        standalone.set_standalone_side(ModifierSide::Right);
        assert_eq!(standalone.modifiers.option, Some(ModifierSide::Right));
        standalone.set_standalone_side(ModifierSide::Both);
        assert_eq!(standalone.modifiers.option, Some(ModifierSide::Right));

        for modifiers in [
            HotkeyModifiers {
                option: Some(ModifierSide::Left),
                command: Some(ModifierSide::Left),
                ..Default::default()
            },
            HotkeyModifiers {
                option: Some(ModifierSide::Both),
                ..Default::default()
            },
        ] {
            let chord = HotkeyBinding {
                modifiers,
                key: None,
            };
            assert_eq!(chord.standalone_side(), None, "{modifiers:?}");
        }

        let mut chord = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Left),
                ..Default::default()
            },
            key: Some(HotkeyKey {
                code: 0,
                label: "A".into(),
            }),
        };
        assert_eq!(chord.standalone_side(), None);
        chord.set_standalone_side(ModifierSide::Right);
        assert_eq!(chord.modifiers.option, Some(ModifierSide::Left));
    }

    #[test]
    fn shortcuts_store_as_chords_and_old_files_convert_once() {
        let both_options = HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(ModifierSide::Both),
                ..Default::default()
            },
            key: None,
        };
        assert_eq!(
            serde_json::to_value(&both_options).unwrap(),
            serde_json::json!({ "keys": [
                { "modifier": "option", "side": "left" },
                { "modifier": "option", "side": "right" },
            ]})
        );
        let chord: HotkeyBinding = serde_json::from_value(serde_json::json!({ "keys": [
            { "code": 49, "label": "Space" },
            { "modifier": "option", "side": "right" },
            { "modifier": "option", "side": "left" },
        ]}))
        .unwrap();
        assert_eq!(
            chord.modifiers.option,
            Some(ModifierSide::Both),
            "any order"
        );
        assert_eq!(chord.key.unwrap().code, 49);
        for refused in [
            serde_json::json!({ "keys": [
                { "modifier": "option", "side": "left" },
                { "modifier": "option", "side": "left" },
            ]}),
            serde_json::json!({ "keys": [
                { "modifier": "option", "side": "any" },
                { "modifier": "option", "side": "right" },
            ]}),
            serde_json::json!({ "keys": [
                { "code": 49, "label": "Space" },
                { "code": 0, "label": "A" },
            ]}),
            serde_json::json!({ "keys": [{ "modifier": "hyper" }] }),
        ] {
            assert!(
                serde_json::from_value::<HotkeyBinding>(refused.clone()).is_err(),
                "{refused}"
            );
        }

        // Every shortcut in a file from before chords is the same chord after.
        let legacy = serde_json::json!({
            "dictation_hotkey": { "modifiers": { "option": "right" }, "key": null },
            "edit_hotkey": { "modifiers": { "option": true, "command": true, "control": false }, "key": null },
            "paste_last_hotkey": {
                "modifiers": { "option": "either", "shift": "either" },
                "key": { "code": 9, "label": "V" }
            },
        });
        let settings: AppSettings = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(
            settings.dictation_hotkey.modifiers,
            HotkeyModifiers {
                option: Some(ModifierSide::Right),
                ..Default::default()
            }
        );
        let stored = serde_json::to_value(&settings).unwrap();
        assert_eq!(stored["edit_hotkey"], legacy["edit_hotkey"]);
        assert_eq!(
            stored["dictation_hotkey"],
            serde_json::json!({ "keys": [{ "modifier": "option", "side": "right" }] })
        );
        assert_eq!(
            stored["paste_last_hotkey"],
            serde_json::json!({ "keys": [
                { "modifier": "option", "side": "any" },
                { "modifier": "shift", "side": "any" },
                { "code": 9, "label": "V" },
            ]})
        );
        let reloaded: AppSettings = serde_json::from_value(stored).unwrap();
        assert_eq!(reloaded.dictation_hotkey, settings.dictation_hotkey);
        assert_eq!(reloaded.retired_edit_hotkey, settings.retired_edit_hotkey);
        assert_eq!(reloaded.paste_last_hotkey, settings.paste_last_hotkey);
        for binding in [
            HotkeyBinding::default(),
            HotkeyBinding::paste_last_default(),
            HotkeyBinding::paste_meeting_default(),
        ] {
            let json = serde_json::to_value(&binding).unwrap();
            assert_eq!(
                serde_json::from_value::<HotkeyBinding>(json).unwrap(),
                binding
            );
        }
    }

    #[test]
    fn both_sides_conflict_only_with_either_side_and_match_only_together() {
        let option = |side| HotkeyBinding {
            modifiers: HotkeyModifiers {
                option: Some(side),
                ..Default::default()
            },
            key: None,
        };
        let both = option(ModifierSide::Both);
        assert!(both.overlaps(&option(ModifierSide::Either)));
        assert!(!both.overlaps(&option(ModifierSide::Left)));
        assert!(!both.overlaps(&option(ModifierSide::Right)));
        assert_eq!(both.modifiers.count(), 2);
        let both = both.runtime();
        let general = OPTION_KEY_MASK;
        assert!(both.exact_modifiers(general | LEFT_OPTION_MASK | RIGHT_OPTION_MASK));
        assert!(!both.exact_modifiers(general | LEFT_OPTION_MASK));
        assert!(!both.exact_modifiers(general | RIGHT_OPTION_MASK));
        assert!(!both.exact_modifiers(general), "input naming no side");
        assert!(
            !both
                .exact_modifiers(general | LEFT_OPTION_MASK | RIGHT_OPTION_MASK | COMMAND_KEY_MASK)
        );
        assert!(both.required_modifiers_down(
            general | LEFT_OPTION_MASK | RIGHT_OPTION_MASK | COMMAND_KEY_MASK
        ));
        assert_eq!(
            both.both_sides_masks().collect::<Vec<_>>(),
            [(LEFT_OPTION_MASK, RIGHT_OPTION_MASK)]
        );
        assert_eq!(
            option(ModifierSide::Either)
                .runtime()
                .both_sides_masks()
                .count(),
            0
        );
    }

    #[test]
    fn recorded_sides_keep_one_modifiers_side_and_both_sides() {
        let lone = HotkeyModifiers {
            option: Some(ModifierSide::Right),
            ..Default::default()
        };
        assert_eq!(lone.recorded_sides(), lone);
        let pair = HotkeyModifiers {
            option: Some(ModifierSide::Left),
            command: Some(ModifierSide::Right),
            ..Default::default()
        };
        assert_eq!(pair.recorded_sides(), HotkeyModifiers::option_command());
        let both = HotkeyModifiers {
            option: Some(ModifierSide::Both),
            shift: Some(ModifierSide::Left),
            ..Default::default()
        };
        assert_eq!(
            both.recorded_sides(),
            HotkeyModifiers {
                option: Some(ModifierSide::Both),
                shift: Some(ModifierSide::Either),
                ..Default::default()
            }
        );
    }

    #[cfg(hex_capture)]
    #[test]
    fn scrawls_routing_imports_without_its_jev_key() {
        let engine = br#"{
            "autoSend": false,
            "jevApiKey": "not-a-real-key",
            "experiments": { "smartRouting": true, "jevRouting": "on" },
            "routing": {
                "sites": { "http://localhost:5173": "~/code/site" },
                "undoWindowSec": 120
            }
        }"#;
        let mut settings = AppSettings::default();
        settings.import_scrawl_routing(Some(engine));
        let config = settings.capture.routing.config();
        assert!(!config.send_captures);
        assert!(config.smart_routing && config.jev_routing);
        assert_eq!(config.undo_window_sec, Some(120));
        assert_eq!(
            config
                .sites
                .get("http://localhost:5173")
                .map(String::as_str),
            Some("~/code/site")
        );
        assert_eq!(config.jev_api_key, None);

        let saved = serde_json::to_string(&settings).unwrap();
        assert!(!saved.contains("not-a-real-key"));
        let read: AppSettings = serde_json::from_str(&saved).unwrap();
        assert_eq!(read.capture.routing, settings.capture.routing);

        // A key typed into settings.json is dropped on read.
        let typed = saved.replace("\"jevApiKey\":null", "\"jevApiKey\":\"x\"");
        assert_ne!(typed, saved);
        let read: AppSettings = serde_json::from_str(&typed).unwrap();
        assert_eq!(read.capture.routing.config().jev_api_key, None);

        // Nothing to read keeps the defaults.
        let mut settings = AppSettings::default();
        settings.import_scrawl_routing(None);
        settings.import_scrawl_routing(Some(b"not json"));
        assert_eq!(settings.capture.routing, CaptureRouting::default());
    }
}

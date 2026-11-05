//! Shortcut bindings: the modifiers with their sides, keys and mouse
//! buttons, the stored chord and its legacy shape, each binding's defaults,
//! overlap rules, and the runtime form the input layers match against
//! device flags.

use super::*;

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

/// Which of a modifier's keys a shortcut holds. `Either` is one key on either side (or both,
/// as before chords); `Both` is the left and the right key together, such as both Options.
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

pub(super) fn deserialize_modifier_side<'de, D>(
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

    /// Every key `required` names is among these held ones; others may be held too.
    pub(super) fn contains(self, required: Self) -> bool {
        side_covers(self.control, required.control)
            && side_covers(self.option, required.option)
            && side_covers(self.shift, required.shift)
            && side_covers(self.command, required.command)
            && (!required.function || self.function)
    }

    /// Exactly the keys `required` names are held: no other modifier, and
    /// no other side of one it names.
    pub(super) fn holds_exactly(self, required: Self) -> bool {
        side_exact(self.control, required.control)
            && side_exact(self.option, required.option)
            && side_exact(self.shift, required.shift)
            && side_exact(self.command, required.command)
            && self.function == required.function
    }

    /// One keycap per modifier key, so both Options show as two.
    pub(super) fn sided(self) -> impl Iterator<Item = (Modifier, ModifierSide)> {
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
    pub(super) fn keycaps(self) -> impl Iterator<Item = &'static str> {
        self.sided()
            .map(|(modifier, side)| side_keycap(side, modifier.keycap()))
    }

    /// Each modifier key's keycap and the side it must be pressed on.
    /// Linux binds no side, so its keycaps never name one.
    pub(super) fn sided_keycaps(self) -> impl Iterator<Item = (&'static str, ModifierSide)> {
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
    pub(super) fn keycaps(self) -> impl Iterator<Item = &'static str> {
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

pub(super) const fn side_count(side: Option<ModifierSide>) -> u8 {
    match side {
        None => 0,
        Some(ModifierSide::Both) => 2,
        Some(_) => 1,
    }
}

pub(super) const fn either_side(side: Option<ModifierSide>) -> Option<ModifierSide> {
    match side {
        Some(_) => Some(ModifierSide::Either),
        None => None,
    }
}

/// `held` is what the keyboard reports for one modifier: a side, `Both`,
/// or `Either` for input that names no side. `Either` asks for any key of
/// it, held alone or with its twin.
pub(super) fn side_covers(held: Option<ModifierSide>, required: Option<ModifierSide>) -> bool {
    match (held, required) {
        (_, None) => true,
        (None, Some(_)) => false,
        (Some(_), Some(ModifierSide::Either)) => true,
        (Some(ModifierSide::Both), Some(ModifierSide::Left | ModifierSide::Right)) => true,
        (Some(held), Some(required)) => held == required,
    }
}

pub(super) fn side_exact(held: Option<ModifierSide>, required: Option<ModifierSide>) -> bool {
    match (held, required) {
        (None, None) => true,
        (Some(_), Some(ModifierSide::Either)) => true,
        (Some(held), Some(required)) => held == required,
        _ => false,
    }
}

#[cfg(target_os = "macos")]
pub(super) fn side_keycap(side: ModifierSide, symbol: &'static str) -> &'static str {
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
pub(super) const MOUSE_INPUT_CODES: std::ops::Range<u16> = 0x8000..0x8020;

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

    /// What its keycap shows: the label, except that on Windows a
    /// punctuation key shows what it types on the active layout now (#815).
    pub fn keycap(&self) -> String {
        #[cfg(target_os = "windows")]
        return crate::windows::input::keycap_label(&self.label);
        #[cfg(not(target_os = "windows"))]
        self.label.clone()
    }

    /// The lowercase ASCII letter a keyboard key's keycap shows, if any.
    #[cfg(target_os = "macos")]
    fn letter(&self) -> Option<char> {
        let mut characters = self.label.chars();
        match (characters.next(), characters.next()) {
            (Some(letter), None) if letter.is_ascii_alphabetic() => {
                Some(letter.to_ascii_lowercase())
            }
            _ => None,
        }
    }

    pub(super) fn same_key(&self, other: &Self) -> bool {
        if cfg!(target_os = "macos") {
            self.code == other.code
        } else {
            self.label.eq_ignore_ascii_case(&other.label)
        }
    }
}

/// The keycap label for a key name: one character in uppercase, otherwise
/// each word with a leading capital ("space" -> "Space", "f12" -> "F12",
/// "num enter" -> "Num Enter").
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(super) fn key_label(name: &str) -> String {
    let name = name.trim();
    if shortcut_repair::is_scroll_lock(name) {
        return "Scroll Lock".into();
    }
    // `section`, as `hex settings` may name the § key.
    if name.eq_ignore_ascii_case("section") {
        return "§".into();
    }
    if name.chars().count() == 1 {
        return name.to_uppercase();
    }
    name.split(' ')
        .map(|word| {
            let mut characters = word.chars();
            characters
                .next()
                .map(|first| first.to_uppercase().chain(characters).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
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
pub(super) struct StoredChord {
    pub(super) keys: Vec<ChordKey>,
}

/// The shape settings stored before chords.
#[derive(Deserialize)]
pub(super) struct LegacyBinding {
    #[serde(default = "legacy_default_modifiers")]
    pub(super) modifiers: HotkeyModifiers,
    #[serde(default = "legacy_default_key")]
    pub(super) key: Option<HotkeyKey>,
}

pub(super) fn legacy_default_modifiers() -> HotkeyModifiers {
    HotkeyBinding::default().modifiers
}

pub(super) fn legacy_default_key() -> Option<HotkeyKey> {
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
    /// Ctrl+Alt+Space on GNOME, whose window menu takes Alt+Space (#723), and
    /// on KDE Plasma, whose KRunner does (#898).
    fn default() -> Self {
        #[cfg(target_os = "macos")]
        let binding = Self {
            modifiers: HotkeyModifiers::option(),
            key: None,
        };
        #[cfg(target_os = "windows")]
        let binding = Self::chord(HotkeyModifiers::control(), "space");
        #[cfg(target_os = "linux")]
        let binding =
            Self::linux_dictation_default(crate::linux::portal::desktop_takes_alt_space());
        binding
    }
}

impl HotkeyBinding {
    #[cfg(target_os = "linux")]
    pub(super) fn linux_dictation_default(alt_space_taken: bool) -> Self {
        if alt_space_taken {
            Self::chord(HotkeyModifiers::control_option(), "space")
        } else {
            Self::chord(HotkeyModifiers::option(), "space")
        }
    }

    /// Modifiers plus a key identified by name (Windows and Linux).
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub(super) fn chord(modifiers: HotkeyModifiers, key: &str) -> Self {
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

    /// Command-Shift-4, macOS's own area screenshot (Labs › Screenshots):
    /// the "4" key's ANSI position, as macOS binds it on every layout.
    pub fn screenshot_default() -> Self {
        #[cfg(target_os = "macos")]
        let binding = Self {
            modifiers: HotkeyModifiers {
                command: Some(ModifierSide::Either),
                shift: Some(ModifierSide::Either),
                ..Default::default()
            },
            key: Some(HotkeyKey {
                code: 21,
                label: "4".into(),
            }),
        };
        #[cfg(not(target_os = "macos"))]
        let binding = Self::chord(
            HotkeyModifiers {
                control: Some(ModifierSide::Either),
                shift: Some(ModifierSide::Either),
                ..Default::default()
            },
            "4",
        );
        binding
    }

    /// Escape, with no modifier.
    pub fn cancel_default() -> Self {
        #[cfg(target_os = "macos")]
        let binding = Self {
            modifiers: HotkeyModifiers::default(),
            // Escape's virtual key code, the same on every layout.
            key: Some(HotkeyKey {
                code: 53,
                label: "Esc".into(),
            }),
        };
        #[cfg(not(target_os = "macos"))]
        let binding = Self::chord(HotkeyModifiers::default(), "escape");
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
pub(super) fn scrawl_shortcut(bytes: &[u8]) -> Option<HotkeyBinding> {
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
pub(super) fn paste_key_code() -> u16 {
    layout_key_code('v').unwrap_or(9)
}

/// The active layout's key for `character`.
#[cfg(target_os = "macos")]
pub(super) fn layout_key_code(character: char) -> Option<u16> {
    shortcut_layout_key_code(character, false)
}

/// The active layout's key for `character` in a shortcut, on the Command
/// layer when it holds Command. The main thread publishes the layout at
/// launch and again when the input source changes; unit tests run on worker
/// threads, where reaching TIS is unsupported, so they resolve against
/// [`test_layout`] instead.
#[cfg(target_os = "macos")]
fn shortcut_layout_key_code(character: char, command: bool) -> Option<u16> {
    #[cfg(test)]
    {
        let _ = command;
        test_layout::key_code(character)
    }
    #[cfg(not(test))]
    crate::keyboard::shortcut_key_code(character, command).ok()
}

/// The ANSI layout's letter keys the tests resolve against, or another
/// layout one test sets on its own thread.
#[cfg(all(test, target_os = "macos"))]
pub(crate) mod test_layout {
    use std::cell::RefCell;
    use std::collections::HashMap;

    thread_local! {
        static LAYOUT: RefCell<Option<HashMap<char, u16>>> = const { RefCell::new(None) };
    }

    pub(crate) fn key_code(character: char) -> Option<u16> {
        LAYOUT.with(|layout| match layout.borrow().as_ref() {
            Some(layout) => layout.get(&character).copied(),
            None => match character {
                'e' => Some(14),
                's' => Some(1),
                'v' => Some(9),
                _ => None,
            },
        })
    }

    /// Resolves letters on `layout` on this thread until it is set again.
    pub(crate) fn set(layout: impl IntoIterator<Item = (char, u16)>) {
        LAYOUT.with(|current| *current.borrow_mut() = Some(layout.into_iter().collect()));
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn paste_key_code() -> u16 {
    0
}

impl HotkeyBinding {
    pub fn keycaps(&self) -> Vec<String> {
        self.modifiers
            .keycaps()
            .map(str::to_string)
            .chain(self.key.iter().map(HotkeyKey::keycap))
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
                    .map(|key| (key.keycap(), ModifierSide::Either)),
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

    /// What the input layers match. On macOS a letter key follows its
    /// letter: its keycap shows the letter, so it matches the key that types
    /// it on the layout published now, and keeps its recorded key when the
    /// layout has none.
    pub fn runtime(&self) -> RuntimeHotkey {
        RuntimeHotkey {
            modifiers: self.modifiers,
            key_code: self.key.as_ref().map(|key| {
                #[cfg(target_os = "macos")]
                if let Some(letter) = key.letter()
                    && let Some(code) =
                        shortcut_layout_key_code(letter, self.modifiers.command.is_some())
                {
                    return code;
                }
                key.code
            }),
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

pub(super) fn modifiers_overlap(left: HotkeyModifiers, right: HotkeyModifiers) -> bool {
    modifier_overlap(left.control, right.control)
        && modifier_overlap(left.option, right.option)
        && modifier_overlap(left.shift, right.shift)
        && modifier_overlap(left.command, right.command)
        && left.function == right.function
}

/// Whether one press could match both: `Either` takes any side, `Both`
/// only both.
pub(super) fn modifier_overlap(left: Option<ModifierSide>, right: Option<ModifierSide>) -> bool {
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
    pub(super) fn without_implied_function(self, flags: u64) -> u64 {
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
pub(super) fn key_reports_function_flag(code: u16) -> bool {
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
pub(super) fn side_from_flags(
    flags: u64,
    general: u64,
    left: u64,
    right: u64,
) -> Option<ModifierSide> {
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
    /// Labs › Annotate 15 s replay shortcut (macOS 27+), while on and bound.
    pub capture_replay: Option<RuntimeHotkey>,
    /// Dictate without processing, while it is bound.
    pub raw_dictation: Option<RuntimeHotkey>,
    /// Labs › Second dictation shortcut (macOS), while it is on and bound.
    pub second_dictation: Option<RuntimeHotkey>,
    /// Toggle mode's cancel shortcut. Not among [`RuntimeHotkeys::all`]:
    /// Hex takes it only while a toggled recording runs.
    pub cancel: Option<RuntimeHotkey>,
}

impl RuntimeHotkeys {
    /// Every shortcut Hex matches, for suppression and chord evidence.
    pub fn all(self) -> [Option<RuntimeHotkey>; 7] {
        [
            Some(self.dictation),
            self.paste_last,
            self.paste_meeting,
            self.capture,
            self.capture_replay,
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
            capture_replay: None,
            raw_dictation: None,
            second_dictation: None,
            cancel: None,
        }
    }
}

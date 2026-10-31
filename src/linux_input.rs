//! Linux dictation shortcuts: the one state machine every input feeds, the
//! X11 key grabs, the Wayland shortcuts portal, and the `hex` commands a
//! desktop keybinding runs. Dictation sees only [`HotkeyEvent`]s.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};
use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::ErrorKind;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{ButtonIndex, ConnectionExt, EventMask, GrabMode, ModMask, Window};
use x11rb::rust_connection::RustConnection;

use crate::app_settings::{HotkeyBinding, HotkeyKey, HotkeyModifiers, ModifierSide};
use crate::desktop_host::{DesktopGrantedShortcut, DesktopShortcut, DesktopShortcuts};
use crate::linux_session::LinuxSession;

/// Temporary pointer grab while Settings records a shortcut. Keyboard input
/// stays with Rune; Escape, focus loss, saving and closing all drop the grab.
pub(crate) struct MouseCapture {
    connection: RustConnection,
    window: Window,
    grabbed: bool,
    modifiers: [u16; 4],
}

impl MouseCapture {
    pub(crate) fn start() -> Result<Self> {
        let (connection, root, keymap) = connect_x11()?;
        let window = connection.get_input_focus()?.reply()?.focus;
        let pid_atom = connection.intern_atom(false, b"_NET_WM_PID")?.reply()?.atom;
        let mut ancestor = window;
        loop {
            if ancestor == root || ancestor <= 1 {
                return Err(eyre!("Hex must be focused to record a mouse shortcut"));
            }
            let property = connection
                .get_property(
                    false,
                    ancestor,
                    pid_atom,
                    x11rb::protocol::xproto::AtomEnum::CARDINAL,
                    0,
                    1,
                )?
                .reply()?;
            if let Some(pid) = property.value32().and_then(|mut values| values.next()) {
                if pid != std::process::id() {
                    return Err(eyre!("Hex must be focused to record a mouse shortcut"));
                }
                break;
            }
            ancestor = connection.query_tree(ancestor)?.reply()?.parent;
        }
        let mut modifiers = [0; 4];
        for (slot, symbols) in modifiers.iter_mut().zip([
            &[XK_CONTROL_L, XK_CONTROL_R][..],
            &[XK_ALT_L, XK_ALT_R][..],
            &[XK_SHIFT_L, XK_SHIFT_R][..],
            &[XK_SUPER_L, XK_SUPER_R][..],
        ]) {
            *slot = u16::from(keymap.modifier_for(&connection, symbols)?);
        }
        Ok(Self {
            connection,
            window,
            grabbed: false,
            modifiers,
        })
    }

    pub(crate) fn poll(&mut self) -> Result<Option<LinuxHotkey>> {
        if self.connection.get_input_focus()?.reply()?.focus != self.window {
            return Err(eyre!("Shortcut recording lost focus"));
        }
        if !self.grabbed {
            // Wait for the click that opened Record to be released.
            if u16::from(self.connection.query_pointer(self.window)?.reply()?.mask) & 0x1f00 != 0 {
                return Ok(None);
            }
            let status = self
                .connection
                .grab_pointer(
                    false,
                    self.window,
                    EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                    x11rb::NONE,
                    x11rb::NONE,
                    x11rb::CURRENT_TIME,
                )?
                .reply()?
                .status;
            if status == x11rb::protocol::xproto::GrabStatus::ALREADY_GRABBED {
                return Ok(None);
            }
            if status != x11rb::protocol::xproto::GrabStatus::SUCCESS {
                return Err(eyre!("Could not record mouse input: {status:?}"));
            }
            self.grabbed = true;
        }
        while let Some(event) = self.connection.poll_for_event()? {
            if let Event::ButtonPress(event) = event {
                let size = self.connection.get_geometry(self.window)?.reply()?;
                if !event.same_screen
                    || event.event_x < 0
                    || event.event_y < 0
                    || event.event_x as u16 >= size.width
                    || event.event_y as u16 >= size.height
                {
                    return Err(eyre!("Shortcut recording clicked outside Hex"));
                }
                let Some(key) = mouse_name(event.detail) else {
                    continue;
                }; // wheel scrolling is not a button binding
                let state = u16::from(event.state);
                let [control, alt, shift, super_key] = self.modifiers.map(|mask| state & mask != 0);
                return Ok(Some(LinuxHotkey {
                    control,
                    alt,
                    shift,
                    super_key,
                    key,
                }));
            }
        }
        Ok(None)
    }
}

impl Drop for MouseCapture {
    fn drop(&mut self) {
        if self.grabbed {
            let _ = self.connection.ungrab_pointer(x11rb::CURRENT_TIME);
            let _ = self.connection.flush();
        }
    }
}

/// A shortcut as X11 grabs and the portal see it: held modifiers and a
/// named key. Settings store it as a portable `HotkeyBinding`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub(crate) struct LinuxHotkey {
    pub(crate) control: bool,
    pub(crate) alt: bool,
    pub(crate) shift: bool,
    pub(crate) super_key: bool,
    pub(crate) key: String,
}

impl Default for LinuxHotkey {
    fn default() -> Self {
        Self {
            control: false,
            alt: true,
            shift: false,
            super_key: false,
            key: "space".into(),
        }
    }
}

/// Linux names Command the Super key and Option Alt; keys go by name.
impl From<&HotkeyBinding> for LinuxHotkey {
    fn from(binding: &HotkeyBinding) -> Self {
        Self {
            control: binding.modifiers.control.is_some(),
            alt: binding.modifiers.option.is_some(),
            shift: binding.modifiers.shift.is_some(),
            super_key: binding.modifiers.command.is_some(),
            key: binding
                .key
                .as_ref()
                .map(HotkeyKey::name)
                .unwrap_or_default(),
        }
    }
}

impl From<&LinuxHotkey> for HotkeyBinding {
    fn from(hotkey: &LinuxHotkey) -> Self {
        let side = |held: bool| held.then_some(ModifierSide::Either);
        Self {
            modifiers: HotkeyModifiers {
                control: side(hotkey.control),
                option: side(hotkey.alt),
                shift: side(hotkey.shift),
                command: side(hotkey.super_key),
                function: false,
            },
            key: (!hotkey.key.trim().is_empty()).then(|| HotkeyKey::named(&hotkey.key)),
        }
    }
}

impl TryFrom<DesktopShortcut> for LinuxHotkey {
    type Error = color_eyre::Report;

    fn try_from(shortcut: DesktopShortcut) -> Result<Self> {
        if shortcut.function {
            return Err(eyre!("the Fn modifier cannot be registered on Linux"));
        }
        Ok(Self {
            control: shortcut.control,
            alt: shortcut.alt,
            shift: shortcut.shift,
            super_key: shortcut.platform,
            key: if shortcut.key == " " {
                "space".into()
            } else {
                shortcut.key.to_ascii_lowercase()
            },
        })
    }
}

/// Logical numbering is consistent with macOS/Windows; X11 reserves 4–7
/// for scroll wheels and puts auxiliary buttons at 8 and above.
pub(crate) fn mouse_button(name: &str) -> Option<u8> {
    Some(match HotkeyKey::mouse_number(name)? {
        1 => 1,
        2 => 3,
        3 => 2,
        number => (number + 4) as u8,
    })
}

pub(crate) fn mouse_name(button: u8) -> Option<String> {
    let number = match button {
        1 => 1,
        2 => 3,
        3 => 2,
        8..=36 => u16::from(button) - 4,
        _ => return None,
    };
    Some(format!("mouse{number}"))
}

impl LinuxHotkey {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.key.trim().is_empty() {
            return Err(eyre!("Linux hotkeys require a non-modifier key"));
        }
        Ok(())
    }

    pub(crate) fn label(&self) -> String {
        HotkeyBinding::from(self).keycaps().join("+")
    }

    /// The trigger the shortcuts portal asks the desktop for, in the
    /// XDG shortcuts format: `CTRL+ALT+space`.
    pub(crate) fn portal_trigger(&self) -> Result<String> {
        if mouse_button(&self.key).is_some() {
            return Err(eyre!(
                "Wayland's shortcuts portal cannot request mouse buttons; bind the hex commands in the desktop settings"
            ));
        }
        let key = keysym_name(&self.key)?;
        let mut parts = Vec::new();
        for (held, name) in [
            (self.control, "CTRL"),
            (self.alt, "ALT"),
            (self.shift, "SHIFT"),
            (self.super_key, "LOGO"),
        ] {
            if held {
                parts.push(name);
            }
        }
        parts.push(&key);
        Ok(parts.join("+"))
    }

    pub(crate) fn modifier_mask(
        &self,
        connection: &RustConnection,
        keymap: &Keymap,
    ) -> Result<ModMask> {
        let mut result = ModMask::default();
        for (enabled, symbols) in [
            (self.control, &[XK_CONTROL_L, XK_CONTROL_R][..]),
            (self.alt, &[XK_ALT_L, XK_ALT_R][..]),
            (self.shift, &[XK_SHIFT_L, XK_SHIFT_R][..]),
            (self.super_key, &[XK_SUPER_L, XK_SUPER_R][..]),
        ] {
            if enabled {
                result |= keymap.modifier_for(connection, symbols)?;
            }
        }
        Ok(result)
    }
}

/// The xkb keysym name of a key Settings records.
fn keysym_name(key: &str) -> Result<String> {
    let key = key.to_ascii_lowercase();
    let name = match key.as_str() {
        "space" | " " => "space",
        "enter" | "return" => "Return",
        "tab" => "Tab",
        "backspace" => "BackSpace",
        "-" | "minus" => "minus",
        "=" | "equal" => "equal",
        "[" | "bracketleft" => "bracketleft",
        "]" | "bracketright" => "bracketright",
        ";" | "semicolon" => "semicolon",
        "'" | "apostrophe" => "apostrophe",
        "`" | "grave" => "grave",
        "\\" | "backslash" => "backslash",
        "," | "comma" => "comma",
        "." | "period" => "period",
        "/" | "slash" => "slash",
        key if key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric() => key,
        key if key.starts_with('f')
            && key[1..]
                .parse::<u32>()
                .is_ok_and(|number| (1..=24).contains(&number)) =>
        {
            return Ok(key.to_ascii_uppercase());
        }
        _ => return Err(eyre!("unsupported shortcut key: {key}")),
    };
    Ok(name.into())
}

const XK_SPACE: u32 = 0x20;
const XK_ESCAPE: u32 = 0xff1b;
pub(crate) const XK_SHIFT_L: u32 = 0xffe1;
pub(crate) const XK_SHIFT_R: u32 = 0xffe2;
pub(crate) const XK_CONTROL_L: u32 = 0xffe3;
pub(crate) const XK_CONTROL_R: u32 = 0xffe4;
pub(crate) const XK_ALT_L: u32 = 0xffe9;
pub(crate) const XK_ALT_R: u32 = 0xffea;
pub(crate) const XK_SUPER_L: u32 = 0xffeb;
pub(crate) const XK_SUPER_R: u32 = 0xffec;
const XK_NUM_LOCK: u32 = 0xff7f;
pub(crate) const KEY_PRESS: u8 = 2;
pub(crate) const KEY_RELEASE: u8 = 3;
const RELEASE_GRACE: Duration = Duration::from_millis(50);
pub(crate) const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(300);
/// Desktops that repeat a held shortcut's press (KWin) repeat it sooner
/// than this, so a press after a longer gap is a new press.
const KEY_REPEAT_GAP: Duration = Duration::from_secs(1);
const POLL_INTERVAL: Duration = Duration::from_millis(5);
/// Binding through the portal can wait for the user to approve a dialog.
const PORTAL_READY_TIMEOUT: Duration = Duration::from_secs(65);

pub(crate) fn connect_x11() -> Result<(RustConnection, Window, Keymap)> {
    let (connection, screen) =
        RustConnection::connect(None).wrap_err("could not connect to X11")?;
    let root = connection.setup().roots[screen].root;
    let keymap = Keymap::read(&connection)?;
    Ok((connection, root, keymap))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HotkeyEvent {
    /// `raw`: Dictate without processing started it.
    Start {
        raw: bool,
    },
    Finish,
    /// `hex dictation stop --print` finished the dictation: its transcript
    /// goes to the reply instead of being pasted.
    FinishPrinting(crate::control::PrintReply),
    Cancel,
    PasteLast,
    /// The Annotate key went down: nothing recording, a capture starts;
    /// recording, the capture host waits to tell a tap from a hold.
    Capture,
    /// It came up: a tap stops the recording, a hold stops drawing.
    CaptureUp,
}

/// What a `hex` command asks of the running listener. Each one is
/// idempotent: starting while recording or stopping while idle does nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum ShortcutCommand {
    DictationStart,
    DictationStop,
    DictationToggle,
    DictationCancel,
    PasteLast,
    /// `hex dictation start --raw` and `toggle --raw`.
    DictationStartRaw,
    DictationToggleRaw,
}

/// What the listener's command queue carries: a `hex` command, or a
/// printing stop with the reply only its own connection waits on.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ListenerCommand {
    Shortcut(ShortcutCommand),
    StopAndPrint(crate::control::PrintReply),
}

impl From<ShortcutCommand> for ListenerCommand {
    fn from(command: ShortcutCommand) -> Self {
        Self::Shortcut(command)
    }
}

/// How the dictation shortcut behaves, and the shortcuts registered
/// beside it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HotkeyOptions {
    pub(crate) double_tap: bool,
    /// Only a double-tap starts recording, which then stays locked.
    pub(crate) double_tap_only: bool,
    pub(crate) toggle: bool,
    pub(crate) paste_last: Option<LinuxHotkey>,
    /// Dictate without processing, while it is bound.
    pub(crate) raw_dictation: Option<LinuxHotkey>,
    /// Annotate's one key, while Annotate is on: tap to start and stop,
    /// hold to draw.
    pub(crate) capture: Option<LinuxHotkey>,
}

/// Whether Annotate draws now, so Escape is Hex's (it stops drawing).
fn annotate_drawing() -> bool {
    #[cfg(hex_capture)]
    return crate::capture_host::drawing();
    #[cfg(not(hex_capture))]
    false
}

pub(crate) const ESCAPE_CANCEL_WARNING: &str =
    "Escape cancel unavailable: another application holds Escape";

/// What the running listener reports about its shortcuts.
#[derive(Clone, Debug, Default)]
pub(crate) struct HotkeyStatus {
    escape_unavailable: Arc<AtomicBool>,
    shortcuts: Arc<Mutex<DesktopShortcuts>>,
    /// Whether the shortcuts portal can show the desktop's dialog for them.
    configurable: Arc<AtomicBool>,
    /// Settings asked for that dialog.
    configure: Arc<AtomicBool>,
    /// The person disabled Hex: clear its shortcuts from the desktop's list
    /// when the listener stops.
    forget: Arc<AtomicBool>,
    keys: ShortcutKeys,
}

impl HotkeyStatus {
    pub(crate) fn warning(&self) -> Option<String> {
        self.escape_unavailable
            .load(Ordering::Acquire)
            .then(|| ESCAPE_CANCEL_WARNING.into())
    }

    pub(crate) fn update_escape_availability(&self, available: bool) -> bool {
        let was_unavailable = self.escape_unavailable.swap(!available, Ordering::AcqRel);
        !available && !was_unavailable
    }

    pub(crate) fn shortcuts(&self) -> DesktopShortcuts {
        self.shortcuts
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    /// When the shortcut that asked for a paste has let go of its keys.
    pub(crate) fn keys(&self) -> ShortcutKeys {
        self.keys.clone()
    }

    pub(crate) fn configurable(&self) -> bool {
        self.configurable.load(Ordering::Acquire)
    }

    /// Opens the desktop's dialog for Hex's shortcuts, when it has one.
    pub(crate) fn configure(&self) {
        self.configure.store(true, Ordering::Release);
    }

    /// Clears Hex's shortcuts from the desktop's list once the listener
    /// stops, as Disable does; a restart for a settings change keeps them.
    pub(crate) fn forget_on_stop(&self) {
        self.forget.store(true, Ordering::Release);
    }

    fn set_shortcuts(&self, shortcuts: DesktopShortcuts) {
        *self
            .shortcuts
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = shortcuts;
    }
}

/// How long after a Wayland shortcut's last event its modifiers may still be
/// down. Hex cannot see them there, so this is the bounded estimate.
const MODIFIER_GRACE: Duration = Duration::from_millis(250);
/// The longest a press without its release holds a paste back, for desktops
/// that drop the release.
const HELD_LIMIT: Duration = Duration::from_secs(2);

/// When the keys of a Hex shortcut may still be down. Some Wayland
/// compositors (Hyprland) add every held physical modifier to virtual
/// keyboard input, turning a paste's Ctrl+V into Ctrl+Alt+V. Hex reads no
/// input devices, so the paste waits on the shortcut's own events: until a
/// modifier shortcut's release (portal Deactivated) plus [`MODIFIER_GRACE`],
/// and [`MODIFIER_GRACE`] after a `hex` command, whose keybinding is unknown.
#[derive(Clone, Debug, Default)]
pub(crate) struct ShortcutKeys(Arc<Mutex<HeldKeys>>);

#[derive(Debug, Default)]
struct HeldKeys {
    /// Modifier shortcuts pressed without their release, by [`Shortcut::index`].
    held: u8,
    last: Option<Instant>,
}

impl HeldKeys {
    /// When a paste may send its shortcut, or `None` for now.
    fn quiet_at(&self) -> Option<Instant> {
        let last = self.last?;
        Some(
            last + if self.held != 0 {
                HELD_LIMIT
            } else {
                MODIFIER_GRACE
            },
        )
    }
}

impl ShortcutKeys {
    fn update(&self, change: impl FnOnce(&mut HeldKeys)) {
        let mut keys = self.0.lock().unwrap_or_else(|error| error.into_inner());
        change(&mut keys);
        keys.last = Some(Instant::now());
    }

    fn pressed(&self, shortcut: Shortcut) {
        self.update(|keys| keys.held |= 1 << shortcut.index());
    }

    fn released(&self, shortcut: Shortcut) {
        self.update(|keys| keys.held &= !(1 << shortcut.index()));
    }

    fn command(&self) {
        self.update(|_| {});
    }

    fn released_all(&self) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .held = 0;
    }

    /// Waits until the shortcut that asked for a paste has plausibly let
    /// go of its modifiers, or a release ends the wait early.
    pub(crate) fn settle(&self, stop: &AtomicBool) -> Result<()> {
        loop {
            if stop.load(Ordering::Acquire) {
                return Err(eyre!("paste cancelled while stopping the listener"));
            }
            let quiet_at = self
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .quiet_at();
            let Some(wait) = quiet_at.and_then(|at| at.checked_duration_since(Instant::now()))
            else {
                return Ok(());
            };
            thread::sleep(wait.min(Duration::from_millis(20)));
        }
    }
}

/// Whether a portal trigger holds modifiers: the one the desktop granted,
/// else the one Hex asked for.
fn trigger_has_modifiers(granted: Option<&str>, requested: &LinuxHotkey) -> bool {
    match granted.map(str::trim).filter(|trigger| !trigger.is_empty()) {
        Some(trigger) => crate::linux_portal::readable_trigger(trigger).contains('+'),
        None => requested.control || requested.alt || requested.shift || requested.super_key,
    }
}

/// The portal id of a shortcut asking for `trigger`. A desktop keeps the
/// trigger it stored for an id and ignores a later `preferred_trigger`
/// (GNOME, KDE), so a shortcut changed in Hex gets a new id, which the
/// desktop binds to the new trigger (#575). An unchanged shortcut keeps its
/// id, and with it any trigger the user assigned in the desktop.
fn portal_shortcut_id(base: &str, trigger: &str) -> String {
    format!("{base}-{}", trigger.replace('+', "-"))
}

/// The trigger the desktop granted a shortcut, by the id Hex bound it under.
fn granted_trigger<'a>(
    granted: &'a std::collections::HashMap<String, Option<String>>,
    ids: &[Option<String>; Shortcut::ALL.len()],
    shortcut: Shortcut,
) -> Option<&'a str> {
    ids[shortcut.index()]
        .as_deref()
        .and_then(|id| granted.get(id))
        .and_then(Option::as_deref)
}

/// A shortcut a backend reports pressed or released.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Shortcut {
    Dictation,
    PasteLast,
    Capture,
    RawDictation,
}

impl Shortcut {
    const ALL: [Self; 4] = [
        Self::Dictation,
        Self::PasteLast,
        Self::Capture,
        Self::RawDictation,
    ];

    fn index(self) -> usize {
        self as usize
    }

    /// The portal's stable id and the description desktops show.
    fn portal_id(self) -> (&'static str, &'static str) {
        match self {
            Self::Dictation => ("dictation", "Dictation"),
            Self::PasteLast => ("paste-last", "Paste last dictation"),
            Self::Capture => ("capture", "Annotate"),
            Self::RawDictation => ("raw-dictation", "Dictate without processing"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Input {
    Pressed(Shortcut),
    Released(Shortcut),
    Escape,
    /// The backend can no longer tell which keys are down.
    Lost,
    Command(ShortcutCommand),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Capture {
    Idle,
    /// Double-tap-only: the first tap is down and waits for the second.
    Armed,
    Held {
        second_tap: bool,
    },
    Locked,
}

/// The shortcut rules every input shares: hold, double-tap lock, toggle,
/// double-tap only, Escape, Paste Last, and commands.
struct ShortcutState {
    double_tap: bool,
    double_tap_only: bool,
    /// Every press acts as the second tap of a double-tap: releasing locks.
    toggle: bool,
    capture: Capture,
    last_release: Option<Instant>,
    last_press: Option<Instant>,
    /// The dictation shortcut was pressed and its release hasn't arrived.
    down: bool,
    /// Dictate without processing, not the dictation shortcut, owns the
    /// capture (or the last one).
    raw: bool,
}

impl ShortcutState {
    fn new(options: &HotkeyOptions) -> Self {
        Self {
            double_tap: options.double_tap,
            double_tap_only: options.double_tap_only,
            toggle: options.toggle,
            capture: Capture::Idle,
            last_release: None,
            last_press: None,
            down: false,
            raw: false,
        }
    }

    /// Escape cancels only while something records.
    fn wants_escape(&self) -> bool {
        matches!(self.capture, Capture::Held { .. } | Capture::Locked)
    }

    fn idle(&self) -> bool {
        matches!(self.capture, Capture::Idle | Capture::Armed)
    }

    fn cancel(&mut self) -> Option<HotkeyEvent> {
        let active = self.wants_escape();
        self.capture = Capture::Idle;
        self.last_release = None;
        active.then_some(HotkeyEvent::Cancel)
    }

    fn start_dictation(&mut self, raw: bool) -> Option<HotkeyEvent> {
        if !self.idle() {
            return None;
        }
        // Like a locked capture: a shortcut press or `stop` finishes it.
        self.raw = raw;
        self.capture = Capture::Locked;
        self.last_release = None;
        Some(HotkeyEvent::Start { raw })
    }

    /// Hands an idle state to the shortcut pressed, or reports that the
    /// other shortcut's capture owns its keys. A tap of one never counts
    /// toward the other's double tap.
    fn claim(&mut self, raw: bool) -> bool {
        if self.raw == raw {
            return true;
        }
        if !self.idle() {
            return false;
        }
        self.raw = raw;
        self.capture = Capture::Idle;
        self.last_release = None;
        self.last_press = None;
        self.down = false;
        true
    }

    fn stop_dictation(&mut self) -> Option<HotkeyEvent> {
        if !matches!(self.capture, Capture::Held { .. } | Capture::Locked) {
            return None;
        }
        self.capture = Capture::Idle;
        self.last_release = None;
        Some(HotkeyEvent::Finish)
    }

    fn update(&mut self, input: Input, now: Instant) -> Option<HotkeyEvent> {
        match input {
            // Escape while Annotate draws stops drawing, never the recording.
            #[cfg(hex_capture)]
            Input::Escape if !self.wants_escape() && crate::capture_host::exit_drawing() => None,
            Input::Escape => self.cancel(),
            Input::Lost => {
                self.down = false;
                self.cancel()
            }
            Input::Pressed(shortcut @ (Shortcut::Dictation | Shortcut::RawDictation)) => {
                if !self.claim(shortcut == Shortcut::RawDictation) {
                    return None;
                }
                // A press with no release since the last one, sooner than a
                // new press could come, is the desktop repeating a held key.
                let repeat = self.down
                    && self.last_press.is_some_and(|pressed| {
                        now.saturating_duration_since(pressed) < KEY_REPEAT_GAP
                    });
                self.last_press = Some(now);
                self.down = true;
                if repeat {
                    return None;
                }
                match self.capture {
                    Capture::Locked => {
                        self.capture = Capture::Idle;
                        self.last_release = None;
                        Some(HotkeyEvent::Finish)
                    }
                    Capture::Idle => {
                        let second_tap = self.toggle
                            || self.last_release.take().is_some_and(|released| {
                                now.checked_duration_since(released)
                                    .is_some_and(|gap| gap < DOUBLE_TAP_WINDOW)
                            });
                        if self.double_tap_only && !second_tap {
                            self.capture = Capture::Armed;
                            return None;
                        }
                        self.capture = Capture::Held { second_tap };
                        Some(HotkeyEvent::Start { raw: self.raw })
                    }
                    // The desktop never sent the release that would have
                    // finished or locked this capture (Hyprland's Lua binds and
                    // GNOME's portal drop it when the modifier comes up first),
                    // so this new press finishes it.
                    Capture::Held { .. } => {
                        self.capture = Capture::Idle;
                        self.last_release = None;
                        Some(HotkeyEvent::Finish)
                    }
                    Capture::Armed => None,
                }
            }
            Input::Released(shortcut @ (Shortcut::Dictation | Shortcut::RawDictation)) => {
                if (shortcut == Shortcut::RawDictation) != self.raw {
                    return None;
                }
                self.down = false;
                match self.capture {
                    Capture::Armed => {
                        self.capture = Capture::Idle;
                        self.last_release = Some(now);
                        None
                    }
                    Capture::Held { second_tap: true } => {
                        self.capture = Capture::Locked;
                        self.last_release = None;
                        None
                    }
                    Capture::Held { second_tap: false } => {
                        self.capture = Capture::Idle;
                        self.last_release = self.double_tap.then_some(now);
                        Some(HotkeyEvent::Finish)
                    }
                    Capture::Idle | Capture::Locked => None,
                }
            }
            Input::Pressed(Shortcut::PasteLast) | Input::Command(ShortcutCommand::PasteLast) => {
                self.idle().then_some(HotkeyEvent::PasteLast)
            }
            Input::Released(Shortcut::PasteLast) => None,
            Input::Pressed(Shortcut::Capture) => Some(HotkeyEvent::Capture),
            Input::Released(Shortcut::Capture) => Some(HotkeyEvent::CaptureUp),
            Input::Command(command) => match command {
                ShortcutCommand::DictationStart => self.start_dictation(false),
                ShortcutCommand::DictationStartRaw => self.start_dictation(true),
                ShortcutCommand::DictationStop => self.stop_dictation(),
                ShortcutCommand::DictationToggle => self
                    .stop_dictation()
                    .or_else(|| self.start_dictation(false)),
                ShortcutCommand::DictationToggleRaw => {
                    self.stop_dictation().or_else(|| self.start_dictation(true))
                }
                ShortcutCommand::DictationCancel => self.cancel(),
                ShortcutCommand::PasteLast => unreachable!("handled above"),
            },
        }
    }
}

/// `hex` commands for whichever monitor runs now. The listener keeps one
/// queue while it registers its shortcuts again after a settings change.
#[derive(Clone)]
pub(crate) struct CommandQueue(Arc<Mutex<Receiver<ListenerCommand>>>);

impl From<Receiver<ListenerCommand>> for CommandQueue {
    fn from(receiver: Receiver<ListenerCommand>) -> Self {
        Self(Arc::new(Mutex::new(receiver)))
    }
}

/// The monitor thread's side: the state machine and its outputs. Backends
/// feed inputs, update their Escape registration, then [`Dispatcher::flush`],
/// so Escape is registered before dictation hears that recording started.
struct Dispatcher {
    state: ShortcutState,
    sender: SyncSender<HotkeyEvent>,
    commands: CommandQueue,
    keys: ShortcutKeys,
    pending: Vec<HotkeyEvent>,
}

impl Dispatcher {
    fn new(
        options: &HotkeyOptions,
        sender: SyncSender<HotkeyEvent>,
        commands: CommandQueue,
        keys: ShortcutKeys,
    ) -> Self {
        Self {
            state: ShortcutState::new(options),
            sender,
            commands,
            keys,
            pending: Vec::new(),
        }
    }

    fn input(&mut self, input: Input) {
        self.pending
            .extend(self.state.update(input, Instant::now()));
    }

    /// Feeds every queued `hex` command.
    fn commands(&mut self) {
        // Without a sender (`hex dictate`), only shortcuts remain.
        let commands = self
            .commands
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let queued: Vec<_> = commands.try_iter().collect();
        drop(commands);
        for command in queued {
            self.keys.command();
            match command {
                ListenerCommand::Shortcut(command) => self.input(Input::Command(command)),
                // Recording stops now, so a following start isn't blocked.
                ListenerCommand::StopAndPrint(reply) => match self.state.stop_dictation() {
                    Some(_) => self.pending.push(HotkeyEvent::FinishPrinting(reply)),
                    None => reply.send(crate::control::PrintedTranscript::NotRecording),
                },
            }
        }
    }

    /// Returns false once dictation stopped listening.
    fn flush(&mut self) -> Result<bool> {
        for event in self.pending.drain(..) {
            if !send_event(&self.sender, event)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

pub(crate) struct LinuxHotkeyMonitor {
    pub(crate) events: Receiver<HotkeyEvent>,
    pub(crate) errors: Receiver<String>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LinuxHotkeyMonitor {
    /// Registers the shortcuts on X11 or through the Wayland shortcuts
    /// portal; without the portal only `commands` drive dictation.
    pub(crate) fn start(
        binding: LinuxHotkey,
        options: HotkeyOptions,
        status: HotkeyStatus,
        commands: CommandQueue,
    ) -> Result<Self> {
        Self::start_for(binding, options, LinuxSession::detect(), status, commands)
    }

    fn start_for(
        binding: LinuxHotkey,
        options: HotkeyOptions,
        session: LinuxSession,
        status: HotkeyStatus,
        commands: CommandQueue,
    ) -> Result<Self> {
        let (sender, events) = mpsc::sync_channel(16);
        let (error_sender, errors) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let started = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_started = started.clone();
        let worker = thread::spawn(move || {
            let keys = status.keys();
            let backend = Backend {
                stop: worker_stop,
                ready: ready_sender.clone(),
                started: worker_started.clone(),
                status,
            };
            let dispatcher = Dispatcher::new(&options, sender, commands, keys);
            let result = match session {
                LinuxSession::X11 => backend.run_x11(dispatcher, binding, options),
                LinuxSession::Wayland => backend.run_wayland(dispatcher, binding, options),
            };
            if let Err(error) = result {
                let message = format!("{error:#}");
                if worker_started.load(Ordering::Acquire) {
                    let _ = error_sender.send(message);
                } else {
                    let _ = ready_sender.try_send(Err(eyre!(message)));
                }
            }
        });
        // Own the worker before waiting so every startup failure stops and joins it.
        let monitor = Self {
            events,
            errors,
            stop,
            worker: Some(worker),
        };
        let timeout = match session {
            LinuxSession::X11 => Duration::from_secs(2),
            LinuxSession::Wayland => PORTAL_READY_TIMEOUT,
        };
        ready_receiver
            .recv_timeout(timeout)
            .map_err(|_| eyre!("timed out registering the Linux dictation shortcut"))??;
        Ok(monitor)
    }
}

impl Drop for LinuxHotkeyMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// The worker thread's lifecycle: stop flag, startup handshake, status.
struct Backend {
    stop: Arc<AtomicBool>,
    ready: SyncSender<Result<()>>,
    started: Arc<AtomicBool>,
    status: HotkeyStatus,
}

impl Backend {
    fn stopping(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    fn mark_ready(&self, shortcuts: DesktopShortcuts) {
        self.status.set_shortcuts(shortcuts);
        self.started.store(true, Ordering::Release);
        self.ready.try_send(Ok(())).ok();
    }

    fn run_wayland(
        self,
        dispatcher: Dispatcher,
        binding: LinuxHotkey,
        options: HotkeyOptions,
    ) -> Result<()> {
        match crate::linux_portal::Portal::connect(&self.stop)? {
            Some(portal) => self.run_portal(portal, dispatcher, binding, options),
            None => {
                tracing::info!("no GlobalShortcuts portal; dictation follows the hex commands");
                self.run_commands(dispatcher)
            }
        }
    }

    /// Only `hex` commands bound in the desktop drive dictation.
    fn run_commands(self, mut dispatcher: Dispatcher) -> Result<()> {
        self.mark_ready(DesktopShortcuts::Commands);
        while !self.stopping() {
            dispatcher.commands();
            if !dispatcher.flush()? {
                break;
            }
            thread::sleep(POLL_INTERVAL);
        }
        Ok(())
    }

    fn run_portal(
        self,
        mut portal: crate::linux_portal::Portal,
        mut dispatcher: Dispatcher,
        binding: LinuxHotkey,
        options: HotkeyOptions,
    ) -> Result<()> {
        let requested = [
            Some((Shortcut::Dictation, &binding)),
            options
                .paste_last
                .as_ref()
                .map(|key| (Shortcut::PasteLast, key)),
            options.capture.as_ref().map(|key| (Shortcut::Capture, key)),
            options
                .raw_dictation
                .as_ref()
                .map(|key| (Shortcut::RawDictation, key)),
        ];
        let mut shortcuts = Vec::new();
        let mut ids: [Option<String>; Shortcut::ALL.len()] = Default::default();
        for (shortcut, key) in requested.into_iter().flatten() {
            let (base, description) = shortcut.portal_id();
            // Keep keyboard shortcuts usable after moving X11 settings to
            // Wayland. Mouse actions remain available through the CLI.
            if mouse_button(&key.key).is_some() {
                continue;
            }
            let trigger = key.portal_trigger()?;
            let id = portal_shortcut_id(base, &trigger);
            ids[shortcut.index()] = Some(id.clone());
            shortcuts.push(crate::linux_portal::Shortcut {
                id,
                description,
                trigger: Some(trigger),
            });
        }
        // Every shortcut, cancel included, goes in this one bind: a second
        // bind would replace the set on KDE.
        shortcuts.push(crate::linux_portal::Shortcut::cancel());
        let granted = portal.bind(&shortcuts, &self.stop)?;
        let project = |granted: &std::collections::HashMap<String, Option<String>>| {
            requested
                .iter()
                .flatten()
                .map(|(shortcut, _)| DesktopGrantedShortcut {
                    name: shortcut.portal_id().1.into(),
                    trigger: granted_trigger(granted, &ids, *shortcut)
                        .map(crate::linux_portal::readable_trigger),
                })
                .collect::<Vec<_>>()
        };
        let modifiers = |granted: &std::collections::HashMap<String, Option<String>>| {
            Shortcut::ALL.map(|shortcut| {
                requested.iter().flatten().any(|(requested, key)| {
                    *requested == shortcut
                        && trigger_has_modifiers(granted_trigger(granted, &ids, shortcut), key)
                })
            })
        };
        let mut with_modifiers = modifiers(&granted);
        let granted = project(&granted);
        for shortcut in granted.iter().filter(|shortcut| shortcut.trigger.is_none()) {
            tracing::warn!(name = %shortcut.name, "the desktop did not grant a Hex shortcut");
        }
        self.status
            .configurable
            .store(portal.configurable(), Ordering::Release);
        self.status.configure.store(false, Ordering::Release);
        self.mark_ready(DesktopShortcuts::Portal(granted));
        let shortcut_for = |id: &str| {
            Shortcut::ALL
                .into_iter()
                .find(|shortcut| ids[shortcut.index()].as_deref() == Some(id))
        };
        while !self.stopping() {
            for event in portal.events() {
                let input = match event {
                    crate::linux_portal::Event::Pressed(id)
                        if id == crate::linux_portal::CANCEL_ID =>
                    {
                        Input::Escape
                    }
                    crate::linux_portal::Event::Pressed(id) => match shortcut_for(&id) {
                        Some(shortcut) => {
                            if with_modifiers[shortcut.index()] {
                                dispatcher.keys.pressed(shortcut);
                            }
                            Input::Pressed(shortcut)
                        }
                        None => continue,
                    },
                    crate::linux_portal::Event::Released(id) => match shortcut_for(&id) {
                        Some(shortcut) => {
                            if with_modifiers[shortcut.index()] {
                                dispatcher.keys.released(shortcut);
                            }
                            Input::Released(shortcut)
                        }
                        None => continue,
                    },
                    crate::linux_portal::Event::Changed(changed) => {
                        with_modifiers = modifiers(&changed);
                        self.status
                            .set_shortcuts(DesktopShortcuts::Portal(project(&changed)));
                        continue;
                    }
                    crate::linux_portal::Event::Closed => {
                        dispatcher.keys.released_all();
                        dispatcher.input(Input::Lost);
                        let _ = dispatcher.flush();
                        return Err(eyre!("the desktop closed Hex's shortcuts portal session"));
                    }
                };
                dispatcher.input(input);
            }
            dispatcher.commands();
            if self.status.configure.swap(false, Ordering::AcqRel)
                && let Err(error) = portal.configure()
            {
                tracing::warn!(
                    error = format!("{error:#}"),
                    "could not open the desktop's shortcut settings"
                );
            }
            if !dispatcher.flush()? {
                break;
            }
            thread::sleep(POLL_INTERVAL);
        }
        if self.status.forget.load(Ordering::Acquire)
            && let Err(error) = portal.forget()
        {
            tracing::warn!(
                error = format!("{error:#}"),
                "could not clear Hex's shortcuts from the desktop"
            );
        }
        Ok(())
    }

    fn run_x11(
        self,
        mut dispatcher: Dispatcher,
        binding: LinuxHotkey,
        options: HotkeyOptions,
    ) -> Result<()> {
        let (connection, root, keymap) = connect_x11()?;
        let mut grabs = X11Grabs {
            connection: &connection,
            root,
            num_lock: keymap
                .modifier_for(&connection, &[XK_NUM_LOCK])
                .unwrap_or(ModMask::M2),
            escape: keymap.keycode(XK_ESCAPE)?,
            keys: Vec::new(),
            escape_grabbed: Vec::new(),
        };
        let requested = [
            Some((Shortcut::Dictation, binding)),
            options.paste_last.map(|key| (Shortcut::PasteLast, key)),
            options.capture.map(|key| (Shortcut::Capture, key)),
            options
                .raw_dictation
                .map(|key| (Shortcut::RawDictation, key)),
        ];
        for (shortcut, key) in requested.into_iter().flatten() {
            let mouse = mouse_button(&key.key);
            let code = match mouse {
                Some(button) => button,
                None => keymap.keycode(keysym(&key.key)?)?,
            };
            let mask = key.modifier_mask(&connection, &keymap)?;
            grabs
                .grab(shortcut, code, mask, mouse.is_some())
                .wrap_err_with(|| {
                    format!("{} is already in use by another X11 client", key.label())
                })?;
        }
        connection.flush()?;
        self.mark_ready(DesktopShortcuts::Registered);

        let lock_bits = u16::from(ModMask::LOCK) | u16::from(grabs.num_lock);
        let mut keys = X11Keys::default();
        let mut escape_down = false;
        while !self.stopping() {
            let mut inputs = keys.expire(Instant::now());
            if let Some(event) = connection.poll_for_event()? {
                match event {
                    Event::KeyPress(event) if event.detail == grabs.escape => {
                        escape_down = true;
                        inputs.push(Input::Escape);
                    }
                    Event::KeyRelease(event) if event.detail == grabs.escape => {
                        escape_down = false;
                    }
                    Event::KeyPress(event) => {
                        let mask = u16::from(event.state) & 0xff & !lock_bits;
                        let shortcut = grabs
                            .keys
                            .iter()
                            .find(|grab| {
                                !grab.mouse && grab.code == event.detail && grab.mask == mask
                            })
                            .or_else(|| {
                                grabs.keys.iter().find(|grab| {
                                    !grab.mouse
                                        && grab.code == event.detail
                                        && keys.held(grab.shortcut)
                                })
                            })
                            .map(|grab| grab.shortcut);
                        inputs.extend(shortcut.and_then(|shortcut| keys.press(shortcut)));
                    }
                    Event::KeyRelease(event) => {
                        for grab in grabs
                            .keys
                            .iter()
                            .filter(|grab| !grab.mouse && grab.code == event.detail)
                        {
                            keys.release(grab.shortcut, Instant::now());
                        }
                    }
                    Event::ButtonPress(event) => {
                        let mask = u16::from(event.state) & 0xff & !lock_bits;
                        if let Some(grab) = grabs.keys.iter().find(|grab| {
                            grab.mouse && grab.code == event.detail && grab.mask == mask
                        }) {
                            inputs.extend(keys.press(grab.shortcut));
                        }
                    }
                    Event::ButtonRelease(event) => {
                        for grab in grabs
                            .keys
                            .iter()
                            .filter(|grab| grab.mouse && grab.code == event.detail)
                        {
                            // Buttons do not autorepeat: preserve the release boundary.
                            if keys.held[grab.shortcut.index()] {
                                keys.held[grab.shortcut.index()] = false;
                                inputs.push(Input::Released(grab.shortcut));
                            }
                        }
                    }
                    _ => {}
                }
            } else if inputs.is_empty() {
                thread::sleep(POLL_INTERVAL);
            }
            for input in inputs {
                dispatcher.input(input);
            }
            dispatcher.commands();
            // Keep Escape until its release so the release never reaches the app.
            let wants_escape = dispatcher.state.wants_escape() || annotate_drawing() || escape_down;
            if wants_escape && grabs.escape_grabbed.is_empty() {
                let available = grabs.grab_escape()?;
                if self.status.update_escape_availability(available) {
                    tracing::warn!("{ESCAPE_CANCEL_WARNING}; continuing without Escape-cancel");
                }
            } else if !wants_escape && !grabs.escape_grabbed.is_empty() {
                grabs.release_escape()?;
            }
            if !dispatcher.flush()? {
                return Ok(());
            }
        }
        Ok(())
    }
}

/// Turns X11 key events into presses and releases. X11 auto-repeat sends a
/// release before every repeated press, so a release counts only once
/// [`RELEASE_GRACE`] passes without a new press.
#[derive(Default)]
struct X11Keys {
    held: [bool; Shortcut::ALL.len()],
    releasing: [Option<Instant>; Shortcut::ALL.len()],
}

impl X11Keys {
    fn held(&self, shortcut: Shortcut) -> bool {
        self.held[shortcut.index()]
    }

    fn press(&mut self, shortcut: Shortcut) -> Option<Input> {
        let index = shortcut.index();
        if self.releasing[index].take().is_some() || self.held[index] {
            return None;
        }
        self.held[index] = true;
        Some(Input::Pressed(shortcut))
    }

    fn release(&mut self, shortcut: Shortcut, now: Instant) {
        let index = shortcut.index();
        if self.held[index] {
            self.releasing[index] = Some(now);
        }
    }

    fn expire(&mut self, now: Instant) -> Vec<Input> {
        let mut released = Vec::new();
        for shortcut in Shortcut::ALL {
            let index = shortcut.index();
            if self.releasing[index]
                .is_some_and(|at| now.saturating_duration_since(at) >= RELEASE_GRACE)
            {
                self.releasing[index] = None;
                self.held[index] = false;
                released.push(Input::Released(shortcut));
            }
        }
        released
    }
}

struct X11Grab {
    mouse: bool,
    shortcut: Shortcut,
    code: u8,
    mask: u16,
    variants: [ModMask; 4],
}

/// Passive grabs on the root window, released on drop.
struct X11Grabs<'a> {
    connection: &'a RustConnection,
    root: Window,
    num_lock: ModMask,
    escape: u8,
    keys: Vec<X11Grab>,
    escape_grabbed: Vec<ModMask>,
}

impl X11Grabs<'_> {
    fn grab(
        &mut self,
        shortcut: Shortcut,
        code: u8,
        mask: ModMask,
        mouse: bool,
    ) -> Result<(), ReplyError> {
        let variants = lock_variants(mask, self.num_lock);
        if mouse {
            for (index, modifiers) in variants.iter().copied().enumerate() {
                let result = self
                    .connection
                    .grab_button(
                        false,
                        self.root,
                        EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE,
                        GrabMode::ASYNC,
                        GrabMode::ASYNC,
                        x11rb::NONE,
                        x11rb::NONE,
                        ButtonIndex::from(code),
                        modifiers,
                    )?
                    .check();
                if let Err(error) = result {
                    for previous in &variants[..index] {
                        self.connection
                            .ungrab_button(ButtonIndex::from(code), self.root, *previous)?
                            .check()?;
                    }
                    return Err(error);
                }
            }
        } else {
            grab_variants(self.connection, self.root, code, &variants)?;
        }
        self.keys.push(X11Grab {
            mouse,
            shortcut,
            code,
            mask: u16::from(mask),
            variants,
        });
        Ok(())
    }

    /// Returns false when another client holds Escape.
    fn grab_escape(&mut self) -> Result<bool> {
        let bare = lock_variants(ModMask::default(), self.num_lock);
        self.escape_grabbed = grab_escape(self.connection, self.root, self.escape, &bare)?;
        self.connection.flush()?;
        Ok(!self.escape_grabbed.is_empty())
    }

    fn release_escape(&mut self) -> Result<()> {
        for modifiers in self.escape_grabbed.drain(..) {
            self.connection
                .ungrab_key(self.escape, self.root, modifiers)?
                .check()?;
        }
        self.connection.flush()?;
        Ok(())
    }
}

impl Drop for X11Grabs<'_> {
    fn drop(&mut self) {
        for &modifiers in &self.escape_grabbed {
            let _ = self
                .connection
                .ungrab_key(self.escape, self.root, modifiers);
        }
        for grab in &self.keys {
            for modifiers in grab.variants {
                if grab.mouse {
                    let _ = self.connection.ungrab_button(
                        ButtonIndex::from(grab.code),
                        self.root,
                        modifiers,
                    );
                } else {
                    let _ = self.connection.ungrab_key(grab.code, self.root, modifiers);
                }
            }
        }
        let _ = self.connection.flush();
    }
}

pub(crate) fn send_event(sender: &SyncSender<HotkeyEvent>, event: HotkeyEvent) -> Result<bool> {
    match sender.try_send(event) {
        Ok(()) => Ok(true),
        Err(TrySendError::Full(_)) => Err(eyre!("Linux hotkey event queue overflow")),
        Err(TrySendError::Disconnected(_)) => Ok(false),
    }
}

/// True when the server refused a passive grab because another client holds it.
/// `GrabKey` with a conflicting grab fails with `BadAccess` (`Access`); any
/// other error (connection loss, bad value) must stay fatal.
fn is_grab_conflict(error: &ReplyError) -> bool {
    matches!(
        error,
        ReplyError::X11Error(error) if error.error_kind == ErrorKind::Access
    )
}

fn grab_escape(
    connection: &RustConnection,
    root: Window,
    escape: u8,
    bare_modifiers: &[ModMask],
) -> Result<Vec<ModMask>> {
    match grab_variants(connection, root, escape, &[ModMask::ANY]) {
        Ok(()) => Ok(vec![ModMask::ANY]),
        Err(error) if is_grab_conflict(&error) => {
            // A modified Escape shortcut must not prevent grabbing plain Escape.
            match grab_variants(connection, root, escape, bare_modifiers) {
                Ok(()) => Ok(bare_modifiers.to_vec()),
                Err(error) if is_grab_conflict(&error) => Ok(Vec::new()),
                Err(error) => Err(error).wrap_err("could not grab the Escape key for cancel"),
            }
        }
        Err(error) => Err(error).wrap_err("could not grab the Escape key for cancel"),
    }
}

fn keysym(key: &str) -> Result<u32> {
    let key = key.to_ascii_lowercase();
    match key.as_str() {
        "space" => Ok(XK_SPACE),
        "enter" | "return" => Ok(0xff0d),
        "tab" => Ok(0xff09),
        "backspace" => Ok(0xff08),
        key if key.len() == 1 && key.as_bytes()[0].is_ascii_graphic() => {
            Ok(u32::from(key.as_bytes()[0]))
        }
        key if key.starts_with('f') => key[1..]
            .parse::<u32>()
            .ok()
            .filter(|number| (1..=24).contains(number))
            .map(|number| 0xffbd + number)
            .ok_or_else(|| eyre!("unsupported X11 function key: {key}")),
        _ => Err(eyre!("unsupported X11 hotkey key: {key}")),
    }
}

fn grab_variants(
    connection: &RustConnection,
    root: Window,
    key: u8,
    modifiers: &[ModMask],
) -> Result<(), ReplyError> {
    let mut grabbed = Vec::new();
    for &modifiers in modifiers {
        let result = connection
            .grab_key(
                false,
                root,
                modifiers,
                key,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )
            .map_err(ReplyError::from)
            .and_then(|cookie| cookie.check());
        if let Err(error) = result {
            // Wait for each release: another client may grab the same key
            // right after this returns, and must not race the ungrab.
            for modifiers in grabbed {
                if let Ok(cookie) = connection.ungrab_key(key, root, modifiers) {
                    let _ = cookie.check();
                }
            }
            return Err(error);
        }
        grabbed.push(modifiers);
    }
    Ok(())
}

fn lock_variants(base: ModMask, num_lock: ModMask) -> [ModMask; 4] {
    [
        base,
        base | ModMask::LOCK,
        base | num_lock,
        base | ModMask::LOCK | num_lock,
    ]
}

pub(crate) struct Keymap {
    min: u8,
    symbols_per_keycode: usize,
    symbols: Vec<u32>,
}

impl Keymap {
    pub(crate) fn read(connection: &RustConnection) -> Result<Self> {
        let setup = connection.setup();
        let min = setup.min_keycode;
        let count = setup.max_keycode - min + 1;
        let mapping = connection.get_keyboard_mapping(min, count)?.reply()?;
        Ok(Self {
            min,
            symbols_per_keycode: usize::from(mapping.keysyms_per_keycode),
            symbols: mapping.keysyms,
        })
    }

    pub(crate) fn keycode(&self, keysym: u32) -> Result<u8> {
        self.symbols
            .chunks(self.symbols_per_keycode)
            .position(|symbols| symbols.contains(&keysym))
            .map(|index| self.min + index as u8)
            .ok_or_else(|| eyre!("active X11 keymap has no key for keysym {keysym:#x}"))
    }

    pub(crate) fn modifier_for(
        &self,
        connection: &RustConnection,
        keysyms: &[u32],
    ) -> Result<ModMask> {
        let keycodes = keysyms
            .iter()
            .filter_map(|keysym| self.keycode(*keysym).ok())
            .collect::<Vec<_>>();
        let mapping = connection.get_modifier_mapping()?.reply()?;
        let width = usize::from(mapping.keycodes_per_modifier());
        mapping
            .keycodes
            .chunks(width)
            .position(|codes| codes.iter().any(|code| keycodes.contains(code)))
            .map(|index| ModMask::from(1_u16 << index))
            .ok_or_else(|| eyre!("active X11 keymap does not map the requested modifier"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use HotkeyEvent::{Cancel, Finish, PasteLast};
    const START: HotkeyEvent = HotkeyEvent::Start { raw: false };
    use ShortcutCommand as Command;

    struct Test {
        state: ShortcutState,
        now: Instant,
        events: Vec<HotkeyEvent>,
    }

    impl Test {
        fn new(double_tap: bool) -> Self {
            Self::with(HotkeyOptions {
                double_tap,
                ..HotkeyOptions::default()
            })
        }

        fn with(options: HotkeyOptions) -> Self {
            Self {
                state: ShortcutState::new(&options),
                now: Instant::now(),
                events: Vec::new(),
            }
        }

        fn input(&mut self, input: Input) {
            if let Some(event) = self.state.update(input, self.now) {
                self.events.push(event);
            }
        }

        fn press(&mut self, shortcut: Shortcut) {
            self.input(Input::Pressed(shortcut));
        }

        fn release(&mut self, shortcut: Shortcut) {
            self.input(Input::Released(shortcut));
        }

        fn command(&mut self, command: ShortcutCommand) {
            self.input(Input::Command(command));
        }

        fn tap(&mut self) {
            self.press(Shortcut::Dictation);
            self.release(Shortcut::Dictation);
        }

        fn lock(&mut self) {
            self.tap();
            self.now += Duration::from_millis(20);
            self.tap();
            assert_eq!(self.events, [START, Finish, START]);
            assert_eq!(self.state.capture, Capture::Locked);
            self.events.clear();
        }
    }

    #[test]
    fn raw_dictation_owns_the_capture_it_starts() {
        const RAW: HotkeyEvent = HotkeyEvent::Start { raw: true };
        let mut test = Test::new(true);
        test.press(Shortcut::RawDictation);
        // The dictation shortcut can't finish or join another's capture.
        test.tap();
        test.release(Shortcut::RawDictation);
        assert_eq!(test.events, [RAW, Finish]);
        // A dictation tap right after a raw one is not its second tap.
        test.now += Duration::from_millis(20);
        test.tap();
        assert_eq!(test.events, [RAW, Finish, START, Finish]);
        assert_eq!(test.state.capture, Capture::Idle);
        test.command(Command::DictationToggleRaw);
        test.command(Command::DictationToggleRaw);
        assert_eq!(test.events, [RAW, Finish, START, Finish, RAW, Finish]);
    }

    #[test]
    fn mouse_numbering_skips_wheel_buttons_and_round_trips() {
        for (name, native) in [
            ("mouse1", 1),
            ("mouse2", 3),
            ("mouse3", 2),
            ("mouse4", 8),
            ("mouse5", 9),
            ("mouse32", 36),
        ] {
            assert_eq!(mouse_button(name), Some(native));
            assert_eq!(mouse_name(native).as_deref(), Some(name));
            let key = LinuxHotkey {
                key: name.into(),
                ..LinuxHotkey::default()
            };
            assert_eq!(LinuxHotkey::from(&HotkeyBinding::from(&key)), key);
            assert!(
                key.portal_trigger().is_err(),
                "the XDG portal has no mouse trigger syntax"
            );
        }
        for wheel in 4..=7 {
            assert_eq!(mouse_name(wheel), None);
        }
        assert_eq!(mouse_button("mouse33"), None);
    }

    #[test]
    #[ignore = "requires an isolated X11 server (Xvfb)"]
    fn x11_mouse_grab_delivers_hold_and_release() {
        let harness = X11TestHarness::new();
        let binding = LinuxHotkey {
            control: false,
            alt: false,
            shift: false,
            super_key: false,
            key: "mouse4".into(),
        };
        let (monitor, _commands) = x11_monitor(binding, double_tap(), Default::default());
        send_key(&harness.connection, harness.root, 4, 8);
        assert_eq!(recv_event(&monitor), HotkeyEvent::Start { raw: false });
        send_key(&harness.connection, harness.root, 5, 8);
        assert_eq!(recv_event(&monitor), HotkeyEvent::Finish);
    }

    #[test]
    #[ignore = "requires an isolated X11 server (Xvfb)"]
    fn x11_mouse_recorder_captures_auxiliary_button_and_releases_its_grab() {
        use x11rb::protocol::xproto::{
            AtomEnum, CreateWindowAux, InputFocus, PropMode, WindowClass,
        };
        use x11rb::wrapper::ConnectionExt as _;
        let harness = X11TestHarness::new();
        let c = &harness.connection;
        let window = c.generate_id().unwrap();
        c.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            harness.root,
            0,
            0,
            200,
            200,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new(),
        )
        .unwrap()
        .check()
        .unwrap();
        let atom = c
            .intern_atom(false, b"_NET_WM_PID")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        c.change_property32(
            PropMode::REPLACE,
            window,
            atom,
            AtomEnum::CARDINAL,
            &[std::process::id()],
        )
        .unwrap()
        .check()
        .unwrap();
        c.map_window(window).unwrap().check().unwrap();
        c.set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        c.warp_pointer(x11rb::NONE, window, 0, 0, 0, 0, 50, 50)
            .unwrap()
            .check()
            .unwrap();
        send_key(c, harness.root, 4, 1);
        let mut capture = MouseCapture::start().unwrap();
        assert!(capture.poll().unwrap().is_none());
        assert!(
            !capture.grabbed,
            "the opening click must not become the binding"
        );
        send_key(c, harness.root, 5, 1);
        assert!(capture.poll().unwrap().is_none());
        assert!(capture.grabbed);
        send_key(c, harness.root, 4, 9);
        let deadline = Instant::now() + Duration::from_secs(1);
        let captured = loop {
            if let Some(binding) = capture.poll().unwrap() {
                break binding;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(captured.key, "mouse5");
        send_key(c, harness.root, 5, 9);
        drop(capture);
        let status = c
            .grab_pointer(
                false,
                window,
                EventMask::BUTTON_PRESS,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                x11rb::NONE,
                x11rb::NONE,
                x11rb::CURRENT_TIME,
            )
            .unwrap()
            .reply()
            .unwrap()
            .status;
        assert_eq!(status, x11rb::protocol::xproto::GrabStatus::SUCCESS);
        c.ungrab_pointer(x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        c.destroy_window(window).unwrap().check().unwrap();
    }

    #[test]
    fn default_binding_is_alt_space() {
        assert_eq!(LinuxHotkey::default().label(), "Alt+Space");
    }

    #[test]
    fn modifier_only_bindings_are_rejected() {
        let binding = LinuxHotkey {
            key: String::new(),
            ..LinuxHotkey::default()
        };
        assert!(binding.validate().is_err());
        assert!(binding.portal_trigger().is_err());
    }

    #[test]
    fn standalone_function_keys_are_accepted() {
        let binding = LinuxHotkey {
            alt: false,
            key: "f12".into(),
            ..LinuxHotkey::default()
        };
        assert_eq!(binding.label(), "F12");
        assert!(binding.validate().is_ok());
    }

    #[test]
    fn hotkeys_round_trip_through_the_portable_binding() {
        let hotkey = LinuxHotkey {
            control: true,
            super_key: true,
            key: "f12".into(),
            ..LinuxHotkey::default()
        };
        assert_eq!(LinuxHotkey::from(&HotkeyBinding::from(&hotkey)), hotkey);
    }

    #[test]
    fn portal_triggers_use_xdg_modifiers_and_xkb_key_names() {
        let trigger = |control, alt, shift, super_key, key: &str| {
            LinuxHotkey {
                control,
                alt,
                shift,
                super_key,
                key: key.into(),
            }
            .portal_trigger()
            .unwrap()
        };
        assert_eq!(trigger(false, true, false, false, "space"), "ALT+space");
        assert_eq!(
            trigger(true, true, true, true, "f9"),
            "CTRL+ALT+SHIFT+LOGO+F9"
        );
        assert_eq!(trigger(true, false, false, false, "-"), "CTRL+minus");
        assert_eq!(trigger(true, false, false, false, "enter"), "CTRL+Return");
        assert_eq!(trigger(false, false, false, true, "Q"), "LOGO+q");
        assert_eq!(trigger(false, false, false, false, "7"), "7");
        for unsupported in ["f25", "escape", "é"] {
            assert!(
                LinuxHotkey {
                    key: unsupported.into(),
                    ..LinuxHotkey::default()
                }
                .portal_trigger()
                .is_err()
            );
        }
    }

    #[test]
    fn a_changed_portal_trigger_gets_a_new_shortcut_id() {
        let id = |trigger: &str| portal_shortcut_id(Shortcut::Dictation.portal_id().0, trigger);
        assert_eq!(id("ALT+space"), "dictation-ALT-space");
        assert_eq!(id("ALT+space"), id("ALT+space"));
        assert_ne!(id("ALT+space"), id("CTRL+ALT+space"));
        let granted = std::collections::HashMap::from([
            (
                "dictation".to_string(),
                Some("Press <alt>space".to_string()),
            ),
            (id("CTRL+F9"), Some("Press <ctrl>F9".to_string())),
        ]);
        let mut ids: [Option<String>; Shortcut::ALL.len()] = Default::default();
        ids[Shortcut::Dictation.index()] = Some(id("CTRL+F9"));
        assert_eq!(
            granted_trigger(&granted, &ids, Shortcut::Dictation),
            Some("Press <ctrl>F9")
        );
        assert_eq!(granted_trigger(&granted, &ids, Shortcut::PasteLast), None);
    }

    #[test]
    fn hold_starts_on_press_and_finishes_on_release() {
        let mut test = Test::new(false);
        test.press(Shortcut::Dictation);
        test.now += Duration::from_millis(500);
        test.release(Shortcut::Dictation);
        test.now += Duration::from_millis(20);
        test.tap();
        assert_eq!(test.events, [START, Finish, START, Finish]);
    }

    #[test]
    fn duplicate_presses_and_releases_do_not_start_finish_or_lock() {
        let mut test = Test::new(true);
        test.press(Shortcut::Dictation);
        test.press(Shortcut::Dictation);
        test.release(Shortcut::Dictation);
        test.now += Duration::from_secs(1);
        test.release(Shortcut::Dictation);
        assert_eq!(test.events, [START, Finish]);
    }

    #[test]
    fn double_tap_window_has_an_explicit_boundary() {
        for (delay, expected) in [
            (299, vec![START, Finish, START]),
            (300, vec![START, Finish, START, Finish]),
        ] {
            let mut test = Test::new(true);
            test.tap();
            test.now += Duration::from_millis(delay);
            test.tap();
            assert_eq!(test.events, expected);
        }
    }

    #[test]
    fn second_tap_locks_until_the_shortcut_is_pressed_again() {
        let mut test = Test::new(true);
        test.lock();
        test.release(Shortcut::Dictation);
        assert!(test.events.is_empty());
        test.press(Shortcut::Dictation);
        assert_eq!(test.events, [Finish]);
        // The release of the finishing press does nothing.
        test.release(Shortcut::Dictation);
        test.now += Duration::from_secs(1);
        test.tap();
        assert_eq!(test.events, [Finish, START, Finish]);
    }

    #[test]
    fn double_tap_only_ignores_a_single_hold_and_locks_on_the_second_tap() {
        let mut test = Test::with(HotkeyOptions {
            double_tap: true,
            double_tap_only: true,
            ..HotkeyOptions::default()
        });
        test.press(Shortcut::Dictation);
        test.now += Duration::from_millis(600);
        test.release(Shortcut::Dictation);
        assert!(test.events.is_empty(), "a hold alone never records");
        test.now += Duration::from_secs(1);
        test.tap();
        test.now += Duration::from_millis(20);
        test.tap();
        assert_eq!(test.events, [START]);
        assert_eq!(test.state.capture, Capture::Locked);
        test.tap();
        assert_eq!(test.events, [START, Finish]);
    }

    #[test]
    fn toggle_mode_starts_on_a_tap_and_finishes_on_the_next() {
        for double_tap in [false, true] {
            let mut test = Test::with(HotkeyOptions {
                double_tap,
                toggle: true,
                ..HotkeyOptions::default()
            });
            test.tap();
            assert_eq!(test.events, [START]);
            assert_eq!(test.state.capture, Capture::Locked);
            test.now += Duration::from_secs(5);
            test.tap();
            assert_eq!(test.events, [START, Finish]);
            // A quick third tap starts a fresh capture rather than locking a double tap.
            test.now += Duration::from_millis(20);
            test.tap();
            assert_eq!(test.events, [START, Finish, START]);
            test.input(Input::Escape);
            assert_eq!(test.events, [START, Finish, START, Cancel]);
        }
    }

    #[test]
    fn a_capture_that_would_lock_finishes_when_the_desktop_drops_its_release() {
        // Hyprland's Lua binds send no release when Alt comes up before Z.
        for toggle in [true, false] {
            let mut test = Test::with(HotkeyOptions {
                double_tap: !toggle,
                toggle,
                ..HotkeyOptions::default()
            });
            if !toggle {
                test.tap();
                test.now += Duration::from_millis(20);
            }
            test.press(Shortcut::Dictation);
            assert_eq!(test.state.capture, Capture::Held { second_tap: true });
            test.events.clear();
            test.now += Duration::from_secs(5);
            test.press(Shortcut::Dictation);
            assert_eq!(test.events, [Finish]);
            assert_eq!(test.state.capture, Capture::Idle);
            test.release(Shortcut::Dictation);
            test.now += Duration::from_secs(1);
            test.press(Shortcut::Dictation);
            assert_eq!(test.events, [Finish, START]);
        }
    }

    #[test]
    fn a_hold_whose_release_never_arrives_finishes_on_the_next_new_press() {
        // GNOME's portal repeats a held shortcut's activation and sends no
        // deactivation when Ctrl comes up before Space.
        for double_tap in [false, true] {
            let mut test = Test::new(double_tap);
            test.press(Shortcut::Dictation);
            for _ in 0..20 {
                test.now += Duration::from_millis(33);
                test.press(Shortcut::Dictation);
            }
            assert_eq!(test.events, [START], "repeats never finish the hold");
            assert_eq!(test.state.capture, Capture::Held { second_tap: false });
            // Ctrl up first: the repeats stop and no release comes.
            test.now += Duration::from_secs(3);
            test.press(Shortcut::Dictation);
            assert_eq!(test.events, [START, Finish]);
            // The finishing press's own repeats and release start nothing.
            for _ in 0..20 {
                test.now += Duration::from_millis(33);
                test.press(Shortcut::Dictation);
            }
            test.release(Shortcut::Dictation);
            assert_eq!(test.events, [START, Finish]);
            assert_eq!(test.state.capture, Capture::Idle);
            test.now += Duration::from_secs(1);
            test.tap();
            assert_eq!(test.events, [START, Finish, START, Finish]);
        }
    }

    #[test]
    fn a_balanced_release_finishes_a_repeating_hold() {
        // Space first, or both together: the desktop sends the release.
        let mut test = Test::new(false);
        test.press(Shortcut::Dictation);
        for _ in 0..20 {
            test.now += Duration::from_millis(33);
            test.press(Shortcut::Dictation);
        }
        test.release(Shortcut::Dictation);
        assert_eq!(test.events, [START, Finish]);
        // A quick new press after a release is a press, not a repeat.
        test.now += Duration::from_millis(100);
        test.press(Shortcut::Dictation);
        assert_eq!(test.events, [START, Finish, START]);
    }

    #[test]
    fn a_repeating_held_press_never_finishes_the_capture() {
        // KWin repeats a held global shortcut's press.
        let mut test = Test::with(HotkeyOptions {
            toggle: true,
            ..HotkeyOptions::default()
        });
        test.press(Shortcut::Dictation);
        test.now += Duration::from_millis(900);
        for _ in 0..100 {
            test.press(Shortcut::Dictation);
            test.now += Duration::from_millis(40);
        }
        test.release(Shortcut::Dictation);
        assert_eq!(test.events, [START]);
        assert_eq!(test.state.capture, Capture::Locked);
    }

    #[test]
    fn escape_during_hold_ignores_the_remaining_release() {
        let mut test = Test::new(true);
        test.press(Shortcut::Dictation);
        test.input(Input::Escape);
        test.release(Shortcut::Dictation);
        test.tap();
        assert_eq!(test.events, [START, Cancel, START, Finish]);
    }

    #[test]
    fn escape_from_a_locked_capture_does_not_swallow_the_next_shortcut() {
        let mut test = Test::new(true);
        test.lock();
        test.input(Input::Escape);
        test.tap();
        assert_eq!(test.events, [Cancel, START, Finish]);
    }

    #[test]
    fn escape_while_idle_does_nothing() {
        let mut test = Test::new(false);
        test.input(Input::Escape);
        assert!(test.events.is_empty());
        assert!(!test.state.wants_escape());
    }

    #[test]
    fn escape_is_wanted_only_while_something_records() {
        let mut test = Test::new(true);
        assert!(!test.state.wants_escape());
        test.press(Shortcut::Dictation);
        assert!(test.state.wants_escape());
        test.release(Shortcut::Dictation);
        assert!(!test.state.wants_escape());
    }

    #[test]
    fn lost_input_cancels_and_forgets_taps() {
        for locked in [false, true] {
            let mut test = Test::new(true);
            if locked {
                test.lock();
            } else {
                test.press(Shortcut::Dictation);
                test.events.clear();
            }
            test.input(Input::Lost);
            assert_eq!(test.events, [Cancel]);
            assert_eq!(test.state.capture, Capture::Idle);
            assert!(test.state.last_release.is_none());
            test.release(Shortcut::Dictation);
            test.tap();
            assert_eq!(test.events, [Cancel, START, Finish]);
        }
    }

    #[test]
    fn paste_last_fires_only_while_idle() {
        let mut test = Test::new(true);
        test.press(Shortcut::PasteLast);
        test.release(Shortcut::PasteLast);
        test.command(Command::PasteLast);
        assert_eq!(test.events, [PasteLast, PasteLast]);
        test.press(Shortcut::Dictation);
        test.press(Shortcut::PasteLast);
        test.command(Command::PasteLast);
        assert_eq!(test.events, [PasteLast, PasteLast, START]);
    }

    #[test]
    fn dictation_commands_are_idempotent() {
        let mut test = Test::new(true);
        test.command(Command::DictationStop);
        test.command(Command::DictationCancel);
        assert!(test.events.is_empty(), "stopping while idle does nothing");
        test.command(Command::DictationStart);
        test.command(Command::DictationStart);
        assert_eq!(
            test.events,
            [START],
            "starting while recording does nothing"
        );
        test.command(Command::DictationStop);
        test.command(Command::DictationStop);
        assert_eq!(test.events, [START, Finish]);
        // A stop never counts as the first tap of a double-tap.
        test.tap();
        assert_eq!(test.events, [START, Finish, START, Finish]);
    }

    #[test]
    fn dictation_toggle_and_cancel_commands() {
        let mut test = Test::new(false);
        test.command(Command::DictationToggle);
        test.command(Command::DictationToggle);
        test.command(Command::DictationToggle);
        test.command(Command::DictationCancel);
        test.command(Command::DictationCancel);
        assert_eq!(test.events, [START, Finish, START, Cancel]);
    }

    #[test]
    fn commands_and_shortcuts_share_one_capture() {
        let mut test = Test::new(true);
        // A shortcut press finishes a capture a command started.
        test.command(Command::DictationStart);
        test.tap();
        // A stop command finishes a held shortcut.
        test.now += Duration::from_secs(1);
        test.press(Shortcut::Dictation);
        test.command(Command::DictationStop);
        test.release(Shortcut::Dictation);
        // A locked double-tap stops through toggle.
        test.now += Duration::from_secs(1);
        test.lock_from(&[START, Finish, START, Finish]);
        test.command(Command::DictationToggle);
        assert_eq!(
            test.events,
            [START, Finish, START, Finish, START, Finish, START, Finish]
        );
    }

    impl Test {
        fn lock_from(&mut self, before: &[HotkeyEvent]) {
            self.tap();
            self.now += Duration::from_millis(20);
            self.tap();
            let mut expected = before.to_vec();
            expected.extend([START, Finish, START]);
            assert_eq!(self.events, expected);
            assert_eq!(self.state.capture, Capture::Locked);
        }
    }

    #[test]
    fn x11_auto_repeat_pairs_never_release_the_shortcut() {
        let start = Instant::now();
        let mut keys = X11Keys::default();
        assert_eq!(
            keys.press(Shortcut::Dictation),
            Some(Input::Pressed(Shortcut::Dictation))
        );
        for repeat in 1..5 {
            let at = start + Duration::from_millis(30 * repeat);
            keys.release(Shortcut::Dictation, at);
            assert!(keys.expire(at + Duration::from_millis(10)).is_empty());
            assert_eq!(keys.press(Shortcut::Dictation), None);
        }
        // Detectable auto-repeat sends presses alone.
        assert_eq!(keys.press(Shortcut::Dictation), None);
        assert!(keys.held(Shortcut::Dictation));
    }

    #[test]
    fn x11_release_counts_after_the_grace_period() {
        let start = Instant::now();
        let mut keys = X11Keys::default();
        keys.press(Shortcut::Dictation);
        keys.release(Shortcut::Dictation, start);
        assert!(keys.expire(start + RELEASE_GRACE / 2).is_empty());
        assert_eq!(
            keys.expire(start + RELEASE_GRACE),
            [Input::Released(Shortcut::Dictation)]
        );
        assert!(keys.expire(start + RELEASE_GRACE * 2).is_empty());
        assert!(!keys.held(Shortcut::Dictation));
        assert_eq!(
            keys.press(Shortcut::Dictation),
            Some(Input::Pressed(Shortcut::Dictation))
        );
    }

    #[test]
    fn x11_ignores_releases_of_keys_it_never_saw_pressed() {
        let start = Instant::now();
        let mut keys = X11Keys::default();
        keys.release(Shortcut::PasteLast, start);
        assert!(keys.expire(start + Duration::from_secs(1)).is_empty());
    }

    #[test]
    fn x11_shortcuts_track_independently() {
        let start = Instant::now();
        let mut keys = X11Keys::default();
        keys.press(Shortcut::Dictation);
        keys.press(Shortcut::PasteLast);
        keys.release(Shortcut::PasteLast, start);
        assert_eq!(
            keys.expire(start + RELEASE_GRACE),
            [Input::Released(Shortcut::PasteLast)]
        );
        assert!(keys.held(Shortcut::Dictation));
    }

    use std::sync::MutexGuard;
    use x11rb::protocol::xtest;

    static X11_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct X11TestHarness {
        _guard: MutexGuard<'static, ()>,
        connection: RustConnection,
        root: Window,
        keymap: Keymap,
        alt: u8,
        space: u8,
        escape: u8,
    }

    impl X11TestHarness {
        fn new() -> Self {
            let guard = X11_TEST_LOCK
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let (connection, root, keymap) = connect_x11().unwrap();
            let alt = keymap.keycode(XK_ALT_L).unwrap();
            let space = keymap.keycode(XK_SPACE).unwrap();
            let escape = keymap.keycode(XK_ESCAPE).unwrap();
            Self {
                _guard: guard,
                connection,
                root,
                keymap,
                alt,
                space,
                escape,
            }
        }

        fn send_key(&self, type_: u8, key: u8) {
            send_key(&self.connection, self.root, type_, key);
        }

        fn tap_alt_space(&self) {
            for (type_, key) in [
                (KEY_PRESS, self.alt),
                (KEY_PRESS, self.space),
                (KEY_RELEASE, self.space),
                (KEY_RELEASE, self.alt),
            ] {
                self.send_key(type_, key);
            }
        }
    }

    fn recv_event(monitor: &LinuxHotkeyMonitor) -> HotkeyEvent {
        monitor.events.recv_timeout(Duration::from_secs(1)).unwrap()
    }

    fn send_key(connection: &RustConnection, root: Window, type_: u8, key: u8) {
        xtest::fake_input(connection, type_, key, 0, root, 0, 0, 0)
            .unwrap()
            .check()
            .unwrap();
        connection.flush().unwrap();
    }

    fn x11_monitor(
        binding: LinuxHotkey,
        options: HotkeyOptions,
        status: HotkeyStatus,
    ) -> (LinuxHotkeyMonitor, SyncSender<ListenerCommand>) {
        let (commands, receiver) = mpsc::sync_channel(4);
        let monitor = LinuxHotkeyMonitor::start_for(
            binding,
            options,
            LinuxSession::X11,
            status,
            receiver.into(),
        )
        .unwrap();
        (monitor, commands)
    }

    fn double_tap() -> HotkeyOptions {
        HotkeyOptions {
            double_tap: true,
            ..HotkeyOptions::default()
        }
    }

    #[test]
    fn lock_variants_preserve_the_configured_modifier() {
        assert_eq!(
            lock_variants(ModMask::M1, ModMask::M2),
            [
                ModMask::M1,
                ModMask::M1 | ModMask::LOCK,
                ModMask::M1 | ModMask::M2,
                ModMask::M1 | ModMask::LOCK | ModMask::M2,
            ]
        );
    }

    #[test]
    fn standalone_f12_resolves_to_the_x11_keysym() {
        assert_eq!(keysym("f12").unwrap(), 0xffc9);
    }

    #[test]
    fn full_event_queue_returns_an_error_instead_of_blocking_the_worker() {
        let (sender, receiver) = mpsc::sync_channel(1);
        assert!(send_event(&sender, HotkeyEvent::Start { raw: false }).unwrap());
        assert!(send_event(&sender, HotkeyEvent::Finish).is_err());
        assert_eq!(
            receiver.try_recv().unwrap(),
            HotkeyEvent::Start { raw: false }
        );
        drop(receiver);
        assert!(!send_event(&sender, HotkeyEvent::Cancel).unwrap());
    }

    fn grab_error(kind: ErrorKind) -> ReplyError {
        ReplyError::X11Error(x11rb::x11_utils::X11Error {
            error_kind: kind,
            error_code: 10,
            sequence: 12,
            bad_value: 1722,
            minor_opcode: 0,
            major_opcode: 33,
            extension_name: None,
            request_name: Some("GrabKey"),
        })
    }

    #[test]
    fn escape_grab_conflict_is_not_fatal() {
        assert!(is_grab_conflict(&grab_error(ErrorKind::Access)));
    }

    #[test]
    fn non_conflict_grab_errors_stay_fatal() {
        for kind in [ErrorKind::Value, ErrorKind::Window, ErrorKind::Match] {
            assert!(
                !is_grab_conflict(&grab_error(kind)),
                "{kind:?} must stay fatal"
            );
        }
    }

    #[test]
    fn a_paste_waits_for_a_modifier_shortcut_to_let_go() {
        let keys = HeldKeys::default();
        assert_eq!(keys.quiet_at(), None);
        let now = Instant::now();
        let held = HeldKeys {
            held: 1 << Shortcut::PasteLast.index(),
            last: Some(now),
        };
        assert_eq!(held.quiet_at(), Some(now + HELD_LIMIT));
        let released = HeldKeys {
            held: 0,
            last: Some(now),
        };
        assert_eq!(released.quiet_at(), Some(now + MODIFIER_GRACE));
    }

    #[test]
    fn granted_triggers_decide_whether_a_shortcut_holds_modifiers() {
        let alt_space = LinuxHotkey::default();
        assert!(trigger_has_modifiers(None, &alt_space));
        assert!(trigger_has_modifiers(Some(""), &alt_space));
        assert!(!trigger_has_modifiers(Some("Press End"), &alt_space));
        assert!(trigger_has_modifiers(
            Some("Press <ctrl><alt>v"),
            &alt_space
        ));
        let end = LinuxHotkey {
            control: false,
            alt: false,
            shift: false,
            super_key: false,
            key: "End".into(),
        };
        assert!(!trigger_has_modifiers(None, &end));
    }

    #[test]
    fn escape_warning_is_logged_once_per_episode_and_clears_on_recovery() {
        let status = HotkeyStatus::default();
        let observer = status.clone();
        assert!(observer.warning().is_none());
        assert!(status.update_escape_availability(false));
        assert_eq!(observer.warning().as_deref(), Some(ESCAPE_CANCEL_WARNING));
        assert!(!status.update_escape_availability(false));
        assert!(!status.update_escape_availability(true));
        assert!(observer.warning().is_none());
        assert!(!status.update_escape_availability(true));
        assert!(status.update_escape_availability(false));
    }

    #[test]
    fn dropping_a_monitor_stops_and_joins_its_worker() {
        let (_, events) = mpsc::sync_channel(1);
        let (_, errors) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_stopped = stopped.clone();
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                thread::yield_now();
            }
            worker_stopped.store(true, Ordering::Release);
        });
        drop(LinuxHotkeyMonitor {
            events,
            errors,
            stop,
            worker: Some(worker),
        });
        assert!(stopped.load(Ordering::Acquire));
    }

    #[test]
    fn commands_alone_drive_a_listener_without_shortcuts() {
        let (sender, events) = mpsc::sync_channel(4);
        let (commands, receiver) = mpsc::sync_channel(4);
        let (ready, readiness) = mpsc::sync_channel(1);
        let status = HotkeyStatus::default();
        let backend = Backend {
            stop: Arc::new(AtomicBool::new(false)),
            ready,
            started: Arc::new(AtomicBool::new(false)),
            status: status.clone(),
        };
        let stop = backend.stop.clone();
        // A starting listener claims no shortcut kind, not even X11's.
        assert_eq!(status.shortcuts(), DesktopShortcuts::Unregistered);
        let worker = thread::spawn(move || {
            backend.run_commands(Dispatcher::new(
                &HotkeyOptions::default(),
                sender,
                receiver.into(),
                ShortcutKeys::default(),
            ))
        });
        readiness.recv().unwrap().unwrap();
        assert_eq!(status.shortcuts(), DesktopShortcuts::Commands);
        commands.send(Command::DictationToggle.into()).unwrap();
        commands.send(Command::DictationToggle.into()).unwrap();
        let wait = || events.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!([wait(), wait()], [START, Finish]);
        stop.store(true, Ordering::Release);
        worker.join().unwrap().unwrap();
    }

    #[test]
    fn a_printing_stop_rides_the_command_queue_with_its_reply() {
        use crate::control::{PrintReply, PrintedTranscript};
        let (sender, events) = mpsc::sync_channel(4);
        let (commands, receiver) = mpsc::sync_channel(4);
        let mut dispatcher = Dispatcher::new(
            &HotkeyOptions::default(),
            sender,
            receiver.into(),
            ShortcutKeys::default(),
        );
        // Nothing records: the reply is answered at once.
        let (reply, idle) = PrintReply::channel();
        commands.send(ListenerCommand::StopAndPrint(reply)).unwrap();
        dispatcher.commands();
        assert_eq!(idle.try_recv().unwrap(), PrintedTranscript::NotRecording);
        // The stop finishes in queue order and a later start isn't blocked.
        let (reply, printed) = PrintReply::channel();
        commands.send(Command::DictationStart.into()).unwrap();
        commands.send(ListenerCommand::StopAndPrint(reply)).unwrap();
        commands.send(Command::DictationStart.into()).unwrap();
        dispatcher.commands();
        assert!(dispatcher.flush().unwrap());
        let received: Vec<_> = events.try_iter().collect();
        assert!(matches!(
            received.as_slice(),
            [
                HotkeyEvent::Start { raw: false },
                HotkeyEvent::FinishPrinting(_),
                HotkeyEvent::Start { raw: false }
            ]
        ));
        // The listener owns the reply now; nothing has answered it yet.
        assert!(printed.try_recv().is_err());
        let Some(HotkeyEvent::FinishPrinting(reply)) = received.into_iter().nth(1) else {
            unreachable!()
        };
        reply.send(PrintedTranscript::Text("Ship it.".into()));
        assert_eq!(
            printed.try_recv().unwrap(),
            PrintedTranscript::Text("Ship it.".into())
        );
    }

    #[test]
    #[ignore = "requires the active X11 desktop"]
    fn grabbed_alt_space_delivers_press_and_release() {
        let harness = X11TestHarness::new();
        let (monitor, _commands) =
            x11_monitor(LinuxHotkey::default(), double_tap(), Default::default());
        harness.tap_alt_space();
        assert_eq!(recv_event(&monitor), HotkeyEvent::Start { raw: false });
        assert_eq!(recv_event(&monitor), HotkeyEvent::Finish);
    }

    #[test]
    #[ignore = "requires the active X11 desktop"]
    fn second_tap_locks_until_the_trigger_is_pressed_again() {
        let harness = X11TestHarness::new();
        let (monitor, _commands) =
            x11_monitor(LinuxHotkey::default(), double_tap(), Default::default());

        harness.tap_alt_space();
        assert_eq!(recv_event(&monitor), HotkeyEvent::Start { raw: false });
        assert_eq!(recv_event(&monitor), HotkeyEvent::Finish);
        harness.tap_alt_space();
        assert_eq!(recv_event(&monitor), HotkeyEvent::Start { raw: false });
        assert!(
            monitor
                .events
                .recv_timeout(Duration::from_millis(100))
                .is_err()
        );
        harness.tap_alt_space();
        assert_eq!(recv_event(&monitor), HotkeyEvent::Finish);
    }

    #[test]
    #[ignore = "requires the active X11 desktop"]
    fn standalone_function_key_delivers_press_and_release() {
        let harness = X11TestHarness::new();
        let binding = LinuxHotkey {
            alt: false,
            key: "f24".into(),
            ..LinuxHotkey::default()
        };
        let (monitor, _commands) =
            x11_monitor(binding, HotkeyOptions::default(), Default::default());
        let trigger = harness.keymap.keycode(keysym("f24").unwrap()).unwrap();
        harness.send_key(KEY_PRESS, trigger);
        harness.send_key(KEY_RELEASE, trigger);

        assert_eq!(recv_event(&monitor), HotkeyEvent::Start { raw: false });
        assert_eq!(recv_event(&monitor), HotkeyEvent::Finish);
    }

    #[test]
    #[ignore = "requires the active X11 desktop"]
    fn commands_and_grabs_share_the_x11_listener() {
        let harness = X11TestHarness::new();
        let (monitor, commands) =
            x11_monitor(LinuxHotkey::default(), double_tap(), Default::default());
        harness.send_key(KEY_PRESS, harness.alt);
        harness.send_key(KEY_PRESS, harness.space);
        assert_eq!(recv_event(&monitor), HotkeyEvent::Start { raw: false });
        commands.send(Command::DictationStop.into()).unwrap();
        assert_eq!(recv_event(&monitor), HotkeyEvent::Finish);
        // Releasing the held shortcut afterwards finishes nothing twice.
        harness.send_key(KEY_RELEASE, harness.space);
        harness.send_key(KEY_RELEASE, harness.alt);
        assert!(
            monitor
                .events
                .recv_timeout(Duration::from_millis(150))
                .is_err()
        );
        commands.send(Command::DictationStart.into()).unwrap();
        assert_eq!(recv_event(&monitor), HotkeyEvent::Start { raw: false });
        // Escape is grabbed while the command's capture records.
        harness.send_key(KEY_PRESS, harness.escape);
        assert_eq!(recv_event(&monitor), HotkeyEvent::Cancel);
        harness.send_key(KEY_RELEASE, harness.escape);
    }

    #[test]
    #[ignore = "requires the active X11 desktop"]
    fn conflicted_escape_still_dictates_without_cancel() {
        let harness = X11TestHarness::new();
        grab_variants(
            &harness.connection,
            harness.root,
            harness.escape,
            &[ModMask::ANY],
        )
        .unwrap();
        harness.connection.flush().unwrap();
        let status = HotkeyStatus::default();
        let (monitor, _commands) = x11_monitor(
            LinuxHotkey::default(),
            HotkeyOptions::default(),
            status.clone(),
        );
        for _ in 0..2 {
            harness.tap_alt_space();
            assert_eq!(recv_event(&monitor), HotkeyEvent::Start { raw: false });
            assert_eq!(status.warning().as_deref(), Some(ESCAPE_CANCEL_WARNING));
            assert_eq!(recv_event(&monitor), HotkeyEvent::Finish);
            assert!(
                monitor
                    .events
                    .recv_timeout(Duration::from_millis(100))
                    .is_err()
            );
            assert!(monitor.errors.try_recv().is_err());
        }
        harness
            .connection
            .ungrab_key(harness.escape, harness.root, ModMask::ANY)
            .unwrap();
        harness.connection.flush().unwrap();
        harness.send_key(KEY_PRESS, harness.alt);
        harness.send_key(KEY_PRESS, harness.space);
        assert_eq!(recv_event(&monitor), HotkeyEvent::Start { raw: false });
        assert!(status.warning().is_none());
        harness.send_key(KEY_PRESS, harness.escape);
        assert_eq!(recv_event(&monitor), HotkeyEvent::Cancel);
        harness.send_key(KEY_RELEASE, harness.escape);
        harness.send_key(KEY_RELEASE, harness.space);
        harness.send_key(KEY_RELEASE, harness.alt);
    }

    #[test]
    #[ignore = "requires an isolated X11 display"]
    fn modified_escape_conflict_keeps_plain_escape_cancel() {
        let harness = X11TestHarness::new();
        grab_variants(
            &harness.connection,
            harness.root,
            harness.escape,
            &[ModMask::M4],
        )
        .unwrap();
        let status = HotkeyStatus::default();
        let (monitor, _commands) =
            x11_monitor(LinuxHotkey::default(), double_tap(), status.clone());
        for expected in [
            HotkeyEvent::Start { raw: false },
            HotkeyEvent::Finish,
            HotkeyEvent::Start { raw: false },
        ] {
            if expected != HotkeyEvent::Finish {
                harness.tap_alt_space();
            }
            assert_eq!(recv_event(&monitor), expected);
        }
        // The second tap locks recording and releases the shortcut modifiers.
        assert!(
            monitor
                .events
                .recv_timeout(Duration::from_millis(100))
                .is_err()
        );
        assert!(status.warning().is_none());
        harness.send_key(KEY_PRESS, harness.escape);
        assert_eq!(recv_event(&monitor), HotkeyEvent::Cancel);
        harness.send_key(KEY_RELEASE, harness.escape);
        // Escape stays grabbed until its release, then returns to applications.
        thread::sleep(Duration::from_millis(50));
        drop(monitor);
        let variants = lock_variants(ModMask::default(), ModMask::M2);
        grab_variants(&harness.connection, harness.root, harness.escape, &variants).unwrap();
    }

    #[test]
    #[ignore = "requires an isolated X11 display"]
    fn conflicted_escape_fallback_releases_partial_grabs() {
        let harness = X11TestHarness::new();
        grab_variants(
            &harness.connection,
            harness.root,
            harness.escape,
            &[ModMask::LOCK],
        )
        .unwrap();
        let (connection, _) = RustConnection::connect(None).unwrap();
        let variants = lock_variants(ModMask::default(), ModMask::M2);
        assert!(
            grab_escape(&connection, harness.root, harness.escape, &variants)
                .unwrap()
                .is_empty()
        );
        // Plain Escape was grabbed before the lock variant failed. It must be freed.
        grab_variants(&harness.connection, harness.root, harness.escape, &variants).unwrap();
        assert!(grab_escape(&connection, 0, harness.escape, &variants).is_err());
    }
}

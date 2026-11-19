use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use color_eyre::eyre::{Result, eyre};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    MAPVK_VK_TO_CHAR, MapVirtualKeyW, SendInput, VK_CONTROL, VK_ESCAPE, VK_LMENU, VK_LWIN, VK_MENU,
    VK_RETURN, VK_RMENU, VK_RWIN, VK_SHIFT, VK_SPACE, VK_TAB, VkKeyScanW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, LLKHF_INJECTED, MSG,
    MSLLHOOKSTRUCT, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, WM_KEYDOWN, WM_KEYUP, WM_QUIT,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::app_settings::{HotkeyBinding, HotkeyKey, HotkeyModifiers, ModifierSide};
use crate::audio::CaptureInstant;
use crate::continuation::InputActivity;
use crate::dictation::DictationFlavour;
use crate::dictation::ShortcutMode;

mod activity;
mod disposition;
mod modifiers;
mod rearm;
mod watchdog;

pub(crate) use self::activity::foreground_window;
use self::disposition::Handled;
use self::modifiers::{LeftControl, ModifierState, SideDownTimes, key_down};

/// The executable of the process that owns `window`.
pub fn window_process_path(
    window: windows_sys::Win32::Foundation::HWND,
) -> Option<std::path::PathBuf> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    if window.is_null() {
        return None;
    }
    unsafe {
        let mut pid = 0_u32;
        GetWindowThreadProcessId(window, &mut pid);
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buffer = [0_u16; 1024];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length);
        CloseHandle(process);
        if ok == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buffer[..length as usize]).into())
    }
}

/// A shortcut as the keyboard hook matches it: the chord's modifier keys,
/// each with its side, and the named key. Settings store it as a portable
/// `HotkeyBinding`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsHotkey {
    pub control: Option<ModifierSide>,
    pub windows: Option<ModifierSide>,
    pub alt: Option<ModifierSide>,
    pub shift: Option<ModifierSide>,
    pub key: Option<String>,
}

impl Default for WindowsHotkey {
    fn default() -> Self {
        Self {
            control: Some(ModifierSide::Either),
            windows: None,
            alt: None,
            shift: None,
            key: Some("space".into()),
        }
    }
}

/// Windows names Command the Windows key and Option Alt; keys go by name.
impl From<&HotkeyBinding> for WindowsHotkey {
    fn from(binding: &HotkeyBinding) -> Self {
        Self {
            control: binding.modifiers.control,
            windows: binding.modifiers.command,
            alt: binding.modifiers.option,
            shift: binding.modifiers.shift,
            key: binding.key.as_ref().map(HotkeyKey::name),
        }
    }
}

impl From<&WindowsHotkey> for HotkeyBinding {
    fn from(hotkey: &WindowsHotkey) -> Self {
        Self {
            modifiers: hotkey.modifiers(),
            key: hotkey.key.as_deref().map(HotkeyKey::named),
        }
    }
}

impl WindowsHotkey {
    fn modifiers(&self) -> HotkeyModifiers {
        HotkeyModifiers {
            control: self.control,
            option: self.alt,
            shift: self.shift,
            command: self.windows,
            function: false,
        }
    }

    pub fn validate(&self) -> Result<()> {
        let modifier_count = self.modifiers().count();
        let Some(key) = self.key.as_deref() else {
            if modifier_count == 0 {
                return Err(eyre!(
                    "Windows dictation shortcuts require a key or a modifier"
                ));
            }
            return Ok(());
        };
        if key.trim().is_empty() {
            return Err(eyre!("Windows dictation shortcut key cannot be empty"));
        }
        crate::windows::input::virtual_key(key)?;
        let function_key = key
            .strip_prefix('f')
            .or_else(|| key.strip_prefix('F'))
            .and_then(|number| number.parse::<u8>().ok())
            .is_some_and(|number| (1..=24).contains(&number));
        // Keys that type nothing may stand alone: function keys, mouse
        // buttons, Menu, Pause, and Scroll Lock, and the navigation keys,
        // which Settings allow alone only with Labs › Special keys alone.
        let types_nothing = function_key
            || HotkeyKey::mouse_number(key).is_some()
            || HotkeyKey::special_name(key)
            || HotkeyKey::navigation_name(key);
        if !types_nothing && modifier_count == 0 {
            return Err(eyre!(
                "Windows dictation shortcuts require a modifier, a function key, Menu, Pause, or Scroll Lock"
            ));
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        HotkeyBinding::from(self).keycaps().join("+")
    }

    /// Releasing Alt or Windows pressed alone opens the menu bar or Start,
    /// so a modifier-only shortcut holding either masks its release.
    fn masks_release(&self) -> bool {
        self.key.is_none() && self.holds_menu_modifier()
    }

    /// The chord holds Alt or Windows, whose release Chrome reads as a menu
    /// tap once Hex swallows the chord's key, so that release is masked.
    fn holds_menu_modifier(&self) -> bool {
        self.alt.is_some() || self.windows.is_some()
    }
}

/// `dwExtraInfo` on every event Hex injects, so its own hook skips them: a
/// paste's Ctrl must never press a Ctrl-alone shortcut.
pub(crate) const HEX_INJECTED: usize = 0x4845_5821;

/// Unassigned, so pressing it between an Alt or Windows press and release
/// keeps the menu bar and Start closed, as AutoHotkey and PowerToys do.
const MASK_KEY: u16 = 0xE8;

/// What the hook does with an event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Disposition {
    Pass,
    Suppress,
    /// Swallow this Alt or Windows release and replay it after the mask key.
    Mask,
}

const EVENT_CAPACITY: usize = 16;
const MAX_EVENT_DELAY_MS: u32 = 60_000;
const DOUBLE_TAP_WINDOW_MS: u32 = crate::dictation::DOUBLE_TAP_WINDOW.as_millis() as u32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeyAction {
    Start,
    Finish,
    Cancel,
    PasteLast,
    /// The Annotate key went down, and came up (`CaptureUp`).
    Capture,
    CaptureUp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HotkeyEvent {
    pub action: HotkeyAction,
    /// The shortcut that started the capture: dictation, Dictate without
    /// processing, Labs › Terminal paste, or Labs › Second dictation.
    pub flavour: DictationFlavour,
    timestamp_ms: u32,
}

impl HotkeyEvent {
    pub fn occurred_at(self, captured_through: CaptureInstant) -> CaptureInstant {
        self.occurred_at_with(unsafe { GetTickCount() }, captured_through)
    }

    fn occurred_at_with(
        self,
        observed_at_ms: u32,
        captured_through: CaptureInstant,
    ) -> CaptureInstant {
        // A delay this long is not a real one: tools that inject keys (a
        // Stream Deck Hotkey action) can stamp events with a time from
        // elsewhere, even slightly in the future, which wraps. Clamping it
        // moved the shortcut a minute back and trimmed the clip to nothing,
        // so such an event counts as happening now.
        let delay = observed_at_ms.wrapping_sub(self.timestamp_ms);
        let delay = if delay > MAX_EVENT_DELAY_MS { 0 } else { delay };
        captured_through
            .checked_sub(Duration::from_millis(u64::from(delay)))
            .unwrap_or(CaptureInstant::ZERO)
    }
}

struct CallbackState {
    events: SyncSender<HotkeyEvent>,
    errors: mpsc::Sender<String>,
    /// The user's own input since the listener started, which ends a paste
    /// continuation.
    activity: InputActivity,
    binding: WindowsHotkey,
    trigger_vk: Option<u32>,
    paste_last: Option<WindowsHotkey>,
    paste_trigger_vk: Option<u32>,
    /// Dictate without processing, while it is bound.
    raw: Option<WindowsHotkey>,
    raw_trigger_vk: Option<u32>,
    raw_trigger_down: bool,
    /// Labs › Terminal paste shortcut, while the lab is on and it is bound.
    terminal_paste: Option<WindowsHotkey>,
    terminal_paste_trigger_vk: Option<u32>,
    terminal_paste_trigger_down: bool,
    /// Labs › Second dictation shortcut, while the lab is on and it is bound.
    second: Option<WindowsHotkey>,
    second_trigger_vk: Option<u32>,
    second_trigger_down: bool,
    annotate: Option<WindowsHotkey>,
    annotate_trigger_vk: Option<u32>,
    annotate_trigger_down: bool,
    /// Escape went down to stop Annotate's drawing; its release is ours too.
    drawing_escape_down: bool,
    /// Toggle mode's cancel shortcut, Hex's only while a recording runs.
    cancel: Option<WindowsHotkey>,
    cancel_vk: Option<u32>,
    /// The cancel key went down while Hex held it; its release is ours too.
    cancel_down: bool,
    modifiers: ModifierState,
    /// The same read with AltGr's synthesized Left Ctrl still counted, which
    /// a binding with a key has always seen (`ModifierState::holds`).
    raw_modifiers: ModifierState,
    side_down: SideDownTimes,
    trigger_down: bool,
    paste_trigger_down: bool,
    double_tap_enabled: bool,
    double_tap_only: bool,
    /// Pass the dictation key on to other apps instead of consuming it.
    share_trigger: bool,
    capture: CaptureState,
    dirty: bool,
    /// A modifier-only shortcut with Alt or Windows was pressed, or a chord
    /// holding either had its key swallowed, with no other key since, so
    /// its release would open the menu bar or Start.
    mask_release: bool,
}

#[derive(Default)]
struct CaptureState {
    active: bool,
    locked: bool,
    second_tap: bool,
    recording: bool,
    last_release_ms: Option<u32>,
    /// When the held press that records began, from the hook's tick count:
    /// a key joining it a second or more later is the app's (Labs › Hold
    /// through other keys).
    pressed_at_ms: Option<u32>,
    /// Every press acts as the second tap of a double-tap: releasing it
    /// locks the capture, and the next press finishes it.
    toggle: bool,
    /// The shortcut, not another, owns this capture (or the last one).
    flavour: DictationFlavour,
}

mod commands;
mod hold_through;

impl CaptureState {
    /// Whether another shortcut owns a capture in progress.
    fn owned_by_other(&self, flavour: DictationFlavour) -> bool {
        self.flavour != flavour && (self.active || self.locked)
    }

    /// Hands an idle capture to one shortcut; a tap of the other one never
    /// counts toward its double tap.
    fn claim(&mut self, flavour: DictationFlavour) {
        if self.flavour != flavour && !self.active && !self.locked {
            self.flavour = flavour;
            self.last_release_ms = None;
        }
    }

    fn press(
        &mut self,
        double_tap_enabled: bool,
        double_tap_only: bool,
        timestamp_ms: u32,
    ) -> Option<HotkeyAction> {
        if self.locked {
            self.locked = false;
            self.recording = false;
            self.last_release_ms = None;
            return Some(HotkeyAction::Finish);
        }
        if self.active {
            return None;
        }
        self.active = true;
        let double_tapped = self
            .last_release_ms
            .take()
            .is_some_and(|released| timestamp_ms.wrapping_sub(released) < DOUBLE_TAP_WINDOW_MS);
        self.second_tap = self.toggle || (double_tap_enabled && double_tapped);
        if double_tap_only && !self.second_tap {
            // Double-tap-only: the first tap arms the window silently;
            // recording waits for the second tap.
            self.recording = false;
            return None;
        }
        self.recording = true;
        self.pressed_at_ms = Some(timestamp_ms);
        Some(HotkeyAction::Start)
    }

    fn release(&mut self, double_tap_enabled: bool, timestamp_ms: u32) -> Option<HotkeyAction> {
        if !self.active {
            return None;
        }
        self.active = false;
        if self.second_tap {
            self.second_tap = false;
            self.locked = true;
            self.last_release_ms = None;
            return None;
        }
        self.last_release_ms = double_tap_enabled.then_some(timestamp_ms);
        if !self.recording {
            // Releasing an armed-but-silent first tap emits nothing.
            return None;
        }
        self.recording = false;
        Some(HotkeyAction::Finish)
    }

    /// Whether Toggle's cancel shortcut is Hex's now: only while a toggled
    /// capture records, so the key stays with every app otherwise.
    fn wants_cancel(&self) -> bool {
        self.toggle && (self.active || self.locked)
    }

    fn cancel(&mut self) -> Option<HotkeyAction> {
        if !self.active && !self.locked {
            return None;
        }
        self.active = false;
        self.locked = false;
        self.second_tap = false;
        self.recording = false;
        self.last_release_ms = None;
        Some(HotkeyAction::Cancel)
    }
}

impl CallbackState {
    /// Whether the chord's modifiers are down: the masked view, and, while
    /// AltGr's synthesized Left Ctrl is the only one down, the Ctrl a
    /// modifier-only AltGr-layout recording made of it
    /// (`ModifierState::holds`).
    fn holds(&self, binding: &WindowsHotkey) -> bool {
        self.modifiers.holds(binding, &self.raw_modifiers)
    }

    /// [`CallbackState::holds`] for "a modifier the chord requires is still
    /// down", which keeps a discarded chord from re-pressing.
    fn holds_any_required(&self, binding: &WindowsHotkey) -> bool {
        self.modifiers
            .holds_any_required(binding, &self.raw_modifiers)
    }

    /// Dictate without processing, Labs › Terminal paste, and Labs › Second
    /// dictation: each key held, locked, and finished like the dictation
    /// key, owning the capture it starts. `None` when the key is not its
    /// trigger.
    fn second_dictation(
        &mut self,
        vk: u32,
        is_down: bool,
        is_up: bool,
        time: u32,
        flavour: DictationFlavour,
    ) -> Option<Disposition> {
        let (trigger_vk, binding) = match flavour {
            DictationFlavour::Raw => (self.raw_trigger_vk, self.raw.as_ref()),
            DictationFlavour::TerminalPaste => {
                (self.terminal_paste_trigger_vk, self.terminal_paste.as_ref())
            }
            DictationFlavour::Second => (self.second_trigger_vk, self.second.as_ref()),
            DictationFlavour::Normal => return None,
        };
        let trigger_down = self.second_trigger_down(flavour);
        let matches = binding.is_some_and(|binding| self.holds(binding));
        let triggered = trigger_vk == Some(vk)
            && (trigger_down
                || (is_down && !self.dirty && !self.capture.owned_by_other(flavour) && matches));
        if !triggered {
            return None;
        }
        let menu = binding.is_some_and(WindowsHotkey::holds_menu_modifier);
        let required_down = binding.is_some_and(|binding| self.holds_any_required(binding));
        // Every key-down of the trigger masks its menu modifier's release,
        // auto-repeats included: `process_button` clears the flag on each
        // non-modifier key-down before this runs, and a held capture over
        // the repeat delay must not leak a bare Alt/Win release. Dictation
        // and Paste Last do the same before their first-press checks.
        self.mask_release |= is_down && menu;
        if is_down && !trigger_down {
            self.set_second_trigger_down(flavour, true);
            self.capture.claim(flavour);
            let was_locked = self.capture.locked;
            if let Some(action) =
                self.capture
                    .press(self.double_tap_enabled, self.double_tap_only, time)
            {
                if was_locked {
                    self.dirty = true;
                }
                send_event(self, action, time);
            }
        } else if is_up && trigger_down {
            self.set_second_trigger_down(flavour, false);
            if let Some(action) = self.capture.release(self.double_tap_enabled, time) {
                send_event(self, action, time);
            }
            if self.dirty && !required_down {
                self.dirty = false;
            }
        }
        Some(Disposition::Suppress)
    }

    fn second_trigger_down(&self, flavour: DictationFlavour) -> bool {
        match flavour {
            DictationFlavour::Raw => self.raw_trigger_down,
            DictationFlavour::TerminalPaste => self.terminal_paste_trigger_down,
            DictationFlavour::Second => self.second_trigger_down,
            DictationFlavour::Normal => false,
        }
    }

    fn set_second_trigger_down(&mut self, flavour: DictationFlavour, down: bool) {
        match flavour {
            DictationFlavour::Raw => self.raw_trigger_down = down,
            DictationFlavour::TerminalPaste => self.terminal_paste_trigger_down = down,
            DictationFlavour::Second => self.second_trigger_down = down,
            DictationFlavour::Normal => {}
        }
    }
}

static CALLBACK_STATE: Mutex<Option<CallbackState>> = Mutex::new(None);

/// While a settings-pane chord capture runs, the hook must neither act on
/// nor suppress anything: acting would fire real dictations/pastes from
/// the chord being recorded, and suppression would hide the current
/// binding's keys from `GetAsyncKeyState`, making it impossible to
/// re-record the same chord. macOS suspends its hotkey handling the same
/// way during capture.
static CAPTURE_INHIBIT: AtomicBool = AtomicBool::new(false);

/// Turns hook processing off (true) or back on (false) for the duration
/// of a chord capture. Entering a capture also cancels any in-flight
/// dictation hold, like the macOS capture flow.
pub(crate) fn set_capture_inhibited(inhibited: bool) {
    CAPTURE_INHIBIT.store(inhibited, Ordering::Relaxed);
    if !inhibited {
        return;
    }
    let mut guard = CALLBACK_STATE
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(state) = guard.as_mut() {
        let now = unsafe { GetTickCount() };
        state.trigger_down = false;
        state.paste_trigger_down = false;
        state.raw_trigger_down = false;
        state.terminal_paste_trigger_down = false;
        state.second_trigger_down = false;
        if let Some(action) = state.capture.cancel() {
            send_event(state, action, now);
        }
    }
}

/// Whether this app owns the foreground window; chord capture only
/// listens while it does, so keystrokes typed into other applications
/// can never rebind a shortcut.
pub(crate) fn app_is_foreground() -> bool {
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_null() {
            return false;
        }
        let mut process_id = 0;
        GetWindowThreadProcessId(foreground, &mut process_id);
        process_id == GetCurrentProcessId()
    }
}

pub struct WindowsHotkeyMonitor {
    pub events: Receiver<HotkeyEvent>,
    pub errors: Receiver<String>,
    /// The user's own input since the listener started; the hooks
    /// invalidate it, and a paste reads its revision.
    pub activity: InputActivity,
    thread_id: u32,
    worker: Option<JoinHandle<()>>,
}

impl WindowsHotkeyMonitor {
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        activity: InputActivity,
        binding: WindowsHotkey,
        paste_last: Option<WindowsHotkey>,
        raw: Option<WindowsHotkey>,
        terminal_paste: Option<WindowsHotkey>,
        second: Option<WindowsHotkey>,
        annotate: Option<WindowsHotkey>,
        cancel: Option<WindowsHotkey>,
        double_tap_enabled: bool,
        double_tap_only: bool,
        shortcut_mode: ShortcutMode,
        share_trigger: bool,
    ) -> Result<Self> {
        binding.validate()?;
        let cancel_vk = cancel
            .as_ref()
            .and_then(|cancel| cancel.key.as_deref())
            .map(virtual_key)
            .transpose()?;
        let trigger_vk = binding.key.as_deref().map(virtual_key).transpose()?;
        let paste_trigger_vk = chord_virtual_key(paste_last.as_ref(), "Paste Last")?;
        let raw_trigger_vk = chord_virtual_key(raw.as_ref(), "Dictate without processing")?;
        let terminal_paste_trigger_vk =
            chord_virtual_key(terminal_paste.as_ref(), "Terminal paste shortcut")?;
        let second_trigger_vk = chord_virtual_key(second.as_ref(), "Second dictation shortcut")?;
        let annotate_trigger_vk = chord_virtual_key(annotate.as_ref(), "Annotate")?;
        let label = binding.label();
        let (event_sender, events) = mpsc::sync_channel(EVENT_CAPACITY);
        let (error_sender, errors) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let state = CallbackState {
            events: event_sender,
            errors: error_sender,
            activity: activity.clone(),
            binding,
            trigger_vk,
            paste_last,
            paste_trigger_vk,
            raw,
            raw_trigger_vk,
            raw_trigger_down: false,
            terminal_paste,
            terminal_paste_trigger_vk,
            terminal_paste_trigger_down: false,
            second,
            second_trigger_vk,
            second_trigger_down: false,
            annotate,
            annotate_trigger_vk,
            annotate_trigger_down: false,
            drawing_escape_down: false,
            cancel,
            cancel_vk,
            cancel_down: false,
            modifiers: ModifierState::default(),
            raw_modifiers: ModifierState::default(),
            side_down: SideDownTimes::default(),
            trigger_down: false,
            paste_trigger_down: false,
            double_tap_enabled,
            double_tap_only,
            share_trigger,
            capture: CaptureState {
                toggle: shortcut_mode == ShortcutMode::Toggle,
                ..CaptureState::default()
            },
            dirty: false,
            mask_release: false,
        };
        let worker = thread::Builder::new()
            .name("windows-hotkey".into())
            .spawn(move || run_hook(state, ready_sender))?;
        let thread_id = ready_receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| eyre!("timed out installing the Windows {label} keyboard hook"))?
            .map_err(|error| eyre!(error))?;
        Ok(Self {
            events,
            errors,
            activity,
            thread_id,
            worker: Some(worker),
        })
    }
}

fn chord_virtual_key(binding: Option<&WindowsHotkey>, name: &str) -> Result<Option<u32>> {
    binding
        .map(|binding| {
            binding.validate()?;
            binding
                .key
                .as_deref()
                .ok_or_else(|| eyre!("{name} requires a non-modifier key"))
                .and_then(virtual_key)
        })
        .transpose()
}

impl Drop for WindowsHotkeyMonitor {
    fn drop(&mut self) {
        let posted = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
        if posted == 0 {
            tracing::warn!(
                error = %std::io::Error::last_os_error(),
                "could not stop Windows hotkey message loop"
            );
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_hook(mut initial: CallbackState, ready: SyncSender<Result<u32, String>>) {
    let thread_id = unsafe { GetCurrentThreadId() };
    let mut message = MSG::default();
    unsafe {
        PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_NOREMOVE);
    }
    let errors = initial.errors.clone();
    initial.modifiers = ModifierState::current();
    initial.raw_modifiers = ModifierState::read(key_down);
    {
        let mut state = CALLBACK_STATE
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.is_some() {
            let _ = ready.send(Err("the Windows keyboard hook is already active".into()));
            return;
        }
        *state = Some(initial);
    }

    let hooks = rearm::KeyboardHooks::install();
    if hooks.keyboard().is_null() {
        let error = std::io::Error::last_os_error();
        hooks.uninstall();
        CALLBACK_STATE
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let _ = ready.send(Err(format!(
            "could not install the Windows keyboard hook: {error}"
        )));
        return;
    }
    if hooks.mouse().is_null() {
        let error = std::io::Error::last_os_error();
        hooks.uninstall();
        CALLBACK_STATE
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let _ = ready.send(Err(format!(
            "could not install the Windows mouse hook: {error}"
        )));
        return;
    }
    if ready.send(Ok(thread_id)).is_err() {
        hooks.uninstall();
        CALLBACK_STATE
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        return;
    }

    loop {
        let status = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
        if status == 0 {
            break;
        }
        if status == -1 {
            let _ = errors.send(format!(
                "Windows hotkey message loop failed: {}",
                std::io::Error::last_os_error()
            ));
            break;
        }
        rearm::KeyboardHooks::on_message(&message);
    }
    hooks.uninstall();
    CALLBACK_STATE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    watchdog::stamp_keyboard();
    if code != HC_ACTION as i32 {
        return unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) };
    }
    let is_down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
    let is_up = matches!(wparam as u32, WM_KEYUP | WM_SYSKEYUP);
    if !is_down && !is_up {
        return unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) };
    }
    let event = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
    if event.dwExtraInfo == HEX_INJECTED {
        return unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) };
    }
    let left_control = LeftControl::of(event.vkCode, event.scanCode);
    // Before anything else: the recorder's hook may be the only one running,
    // and both must know what AltGr sent.
    modifiers::note_left_control(left_control, is_down);
    let vk = track_numpad(event, is_down).unwrap_or(event.vkCode);
    // Injected keys carry whatever time their sender gave them, so they
    // are stamped with the time the hook sees them.
    let time = if event.flags & LLKHF_INJECTED != 0 {
        unsafe { GetTickCount() }
    } else {
        event.time
    };
    let handled = process_button(vk, left_control, is_down, time, key_down);
    if handled.ends_continuation(is_down, ModifierState::is_modifier(vk), event.dwExtraInfo) {
        activity::invalidate();
    }
    match handled.disposition {
        Disposition::Pass => unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) },
        Disposition::Suppress => 1,
        Disposition::Mask if replay_masked_release(event) => 1,
        // Windows refused the injection (an elevated foreground window):
        // let the release through rather than leave the key held.
        Disposition::Mask => unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) },
    }
}

/// Taps the mask key, then replays the swallowed release, so the system
/// sees a chord instead of Alt or Windows alone.
fn replay_masked_release(release: &KBDLLHOOKSTRUCT) -> bool {
    let extended = if release.flags & LLKHF_EXTENDED != 0 {
        KEYEVENTF_EXTENDEDKEY
    } else {
        0
    };
    let input = |vk: u16, scan: u16, flags: u32| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: HEX_INJECTED,
            },
        },
    };
    let inputs = [
        input(MASK_KEY, 0, 0),
        input(MASK_KEY, 0, KEYEVENTF_KEYUP),
        input(
            release.vkCode as u16,
            release.scanCode as u16,
            KEYEVENTF_KEYUP | extended,
        ),
    ];
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    sent as usize == inputs.len()
}

fn mouse_message(message: u32, data: u32) -> Option<(u32, bool)> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONUP,
        WM_XBUTTONDOWN, WM_XBUTTONUP,
    };
    let (button, down) = match message {
        WM_LBUTTONDOWN => (0x01, true),
        WM_LBUTTONUP => (0x01, false),
        WM_RBUTTONDOWN => (0x02, true),
        WM_RBUTTONUP => (0x02, false),
        WM_MBUTTONDOWN => (0x04, true),
        WM_MBUTTONUP => (0x04, false),
        WM_XBUTTONDOWN | WM_XBUTTONUP => (
            match data >> 16 {
                1 => 0x05,
                2 => 0x06,
                _ => return None,
            },
            message == WM_XBUTTONDOWN,
        ),
        _ => return None,
    };
    Some((button, down))
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    watchdog::stamp();
    if code == HC_ACTION as i32 {
        // SAFETY: Windows supplies a live MSLLHOOKSTRUCT for this callback.
        let event = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        if crate::capture_host::wants_input() {
            report_pointer(wparam as u32, event);
        }
        if let Some((vk, down)) = mouse_message(wparam as u32, event.mouseData) {
            // A mouse event settles stale Left Ctrl bits too: a release can
            // land on another desktop and never reach the keyboard hook.
            modifiers::note_left_control(LeftControl::Other, down);
            let handled = process_button(vk, LeftControl::Other, down, event.time, key_down);
            if handled.ends_continuation(down, false, event.dwExtraInfo) {
                activity::invalidate();
            }
            if handled.disposition != Disposition::Pass {
                return 1;
            }
        }
    }
    unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) }
}

/// What a recording Annotate capture takes from the mouse hook: every move,
/// click, and wheel turn, in physical screen pixels as the engine's Windows
/// target frames are. The event still reaches the app beneath.
fn report_pointer(message: u32, event: &MSLLHOOKSTRUCT) {
    use crate::capture_host::{Tapped, report_input};
    use scrawl_capture::hosted_input::HostInput;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        LLMHF_INJECTED, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
        WM_RBUTTONDOWN, WM_RBUTTONUP,
    };
    if event.flags & LLMHF_INJECTED != 0 {
        return;
    }
    let (x, y) = (f64::from(event.pt.x), f64::from(event.pt.y));
    let modifiers = held_modifiers();
    // One notch is 120 in the high word, signed; up and right are positive.
    let notches = f64::from((event.mouseData >> 16) as i16) / 120.0;
    let input = match message {
        WM_MOUSEMOVE => HostInput::Moved { x, y, modifiers },
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP => HostInput::Button {
            down: matches!(message, WM_LBUTTONDOWN | WM_RBUTTONDOWN),
            right: matches!(message, WM_RBUTTONDOWN | WM_RBUTTONUP),
            x,
            y,
            modifiers,
        },
        WM_MOUSEWHEEL => HostInput::Scroll {
            x,
            y,
            dx: 0.0,
            dy: notches,
        },
        WM_MOUSEHWHEEL => HostInput::Scroll {
            x,
            y,
            dx: notches,
            dy: 0.0,
        },
        _ => return,
    };
    report_input(CaptureInstant::now(), Tapped::Pointer(input));
}

/// A modifier changed while an Annotate capture records: Shift alone inks.
/// `held` includes the change, which the async key state does not yet.
fn report_modifiers(held: &ModifierState) {
    use crate::capture_host::{Tapped, report_input};
    use scrawl_capture::hosted_input::HostInput;
    let mut point = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
    // SAFETY: `point` is a valid POINT to fill.
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point) };
    report_input(
        CaptureInstant::now(),
        Tapped::Pointer(HostInput::ModifiersChanged {
            x: f64::from(point.x),
            y: f64::from(point.y),
            modifiers: scrawl_capture::hosted_input::Modifiers {
                command: held.left_windows || held.right_windows,
                control: held.left_control || held.right_control,
                option: held.left_alt || held.right_alt,
                shift: held.left_shift || held.right_shift,
            },
        }),
    );
}

/// The modifiers held now, as the engine names them: the Windows key is its
/// Command, Alt its Option.
fn held_modifiers() -> scrawl_capture::hosted_input::Modifiers {
    scrawl_capture::hosted_input::Modifiers {
        command: key_down(VK_LWIN) || key_down(VK_RWIN),
        control: key_down(VK_CONTROL),
        option: key_down(VK_MENU),
        shift: key_down(VK_SHIFT),
    }
}

// Both low-level hooks feed exactly the same capture and suppression rules.
// `key_held` is the asynchronous key state the event is judged against, which
// a test cannot make physical, so it arrives here as a lookup.
fn process_button(
    vk: u32,
    left_control: LeftControl,
    is_down: bool,
    time: u32,
    key_held: impl Fn(u16) -> bool,
) -> Handled {
    let is_up = !is_down;
    let mut suppressed = false;
    let mut trigger = false;
    let mut mask = false;
    {
        let mut guard = CALLBACK_STATE
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(state) = guard.as_mut() {
            // AltGr's synthesized Left Ctrl is no real Ctrl: it stays out of
            // chord state and matching, and still reaches the app in front,
            // where it makes AltGr type.
            if left_control == LeftControl::AltGr {
                return Handled::PASS;
            }
            // The hook never sees a release that lands on another desktop,
            // such as the Windows key of Win+L or the modifiers of
            // Ctrl+Alt+Delete, so tracked modifiers stayed held after
            // unlocking and Ctrl+Space no longer matched. Re-read them on
            // every event: the asynchronous key state does not include this
            // event yet, so `update` applies it on top.
            let read = ModifierState::read(key_held);
            let altgr_only = modifiers::altgr_only();
            state.raw_modifiers = read;
            state.modifiers = read.without_altgr(altgr_only);
            let modifier_event = state.modifiers.update(vk, is_down);
            state.raw_modifiers.update(vk, is_down);
            state.side_down.update(vk, is_down, time);
            // During a chord capture the hook only tracks modifier state;
            // it neither triggers actions nor suppresses keys, so the
            // capture can observe every key through GetAsyncKeyState.
            if CAPTURE_INHIBIT.load(Ordering::Relaxed) {
                state.mask_release = false;
                return Handled::PASS;
            }
            if !modifier_event && is_down {
                state.mask_release = false;
            }
            // A dictation shortcut's modifiers are down and its key may
            // follow: the HUD's GPU device and pipelines start on a thread
            // now, so the HUD shows at once rather than after them (#1229).
            if modifier_event
                && is_down
                && !state.capture.active
                && [
                    Some(&state.binding),
                    state.raw.as_ref(),
                    state.terminal_paste.as_ref(),
                    state.second.as_ref(),
                ]
                .into_iter()
                .flatten()
                .any(|binding| state.holds(binding))
            {
                rune::warm_gpu_with(
                    crate::rune_hud::Painter::FEATURES,
                    crate::rune_hud::prepare_gpu,
                );
            }
            if modifier_event && crate::capture_host::wants_input() {
                report_modifiers(&state.modifiers);
            }
            let annotate_event = state.annotate_trigger_vk == Some(vk)
                && (state.annotate_trigger_down
                    || (is_down
                        && !state.capture.active
                        && !state.capture.locked
                        && !state.dirty
                        && state
                            .annotate
                            .as_ref()
                            .is_some_and(|binding| state.holds(binding))));
            if annotate_event {
                state.mask_release |= is_down && holds_menu(state.annotate.as_ref());
                if is_down && !state.annotate_trigger_down {
                    state.annotate_trigger_down = true;
                    send_event(state, HotkeyAction::Capture, time);
                } else if is_up && state.annotate_trigger_down {
                    state.annotate_trigger_down = false;
                    send_event(state, HotkeyAction::CaptureUp, time);
                }
                return Handled::own(Disposition::Suppress);
            }
            // Toggle's cancel key discards the recording. Hex takes it only
            // while a toggled capture records, and keeps its repeats and
            // release, so the app in front never acts on it.
            let cancel_event = state.cancel_vk == Some(vk)
                && (state.cancel_down
                    || (is_down
                        && state.capture.wants_cancel()
                        && state
                            .cancel
                            .as_ref()
                            .is_some_and(|binding| state.holds(binding))));
            if cancel_event {
                if is_down && !state.cancel_down {
                    state.cancel_down = true;
                    if let Some(action) = state.capture.cancel() {
                        watchdog::note_cancelled();
                        send_event(state, action, time);
                    }
                } else if is_up {
                    state.cancel_down = false;
                }
                return Handled::own(Disposition::Suppress);
            }
            // Escape stops Annotate's drawing, never its recording, when no
            // dictation takes it.
            if vk == u32::from(VK_ESCAPE)
                && !state.capture.active
                && !state.capture.locked
                && (state.drawing_escape_down || (is_down && crate::capture_host::exit_drawing()))
            {
                state.drawing_escape_down = is_down;
                return Handled::own(Disposition::Suppress);
            }
            // Dictate without processing, Labs › Terminal paste, and Labs ›
            // Second dictation: key chords held, locked, and finished like the
            // dictation key, each owning the capture it starts.
            for flavour in [
                DictationFlavour::Raw,
                DictationFlavour::TerminalPaste,
                DictationFlavour::Second,
            ] {
                if let Some(disposition) = state.second_dictation(vk, is_down, is_up, time, flavour)
                {
                    return Handled::own(disposition);
                }
            }
            let paste_event = state.paste_trigger_vk == Some(vk)
                && (state.paste_trigger_down
                    || (is_down
                        && !state.capture.active
                        && !state.capture.locked
                        && !state.dirty
                        && state
                            .paste_last
                            .as_ref()
                            .is_some_and(|binding| state.holds(binding))));
            if paste_event {
                suppressed = true;
                state.mask_release |= is_down && holds_menu(state.paste_last.as_ref());
                if is_down && !state.paste_trigger_down {
                    state.paste_trigger_down = true;
                    send_event(state, HotkeyAction::PasteLast, time);
                } else if is_up {
                    state.paste_trigger_down = false;
                }
            } else if state.trigger_vk == Some(vk) {
                if is_down {
                    if state.trigger_down {
                        suppressed = !state.share_trigger;
                    } else if !state.dirty
                        && !state.capture.owned_by_other(DictationFlavour::Normal)
                        && state.holds(&state.binding)
                    {
                        state.trigger_down = true;
                        suppressed = !state.share_trigger;
                        state.capture.claim(DictationFlavour::Normal);
                        let was_locked = state.capture.locked;
                        if let Some(action) = state.capture.press(
                            state.double_tap_enabled,
                            state.double_tap_only,
                            time,
                        ) {
                            if was_locked {
                                state.dirty = true;
                            }
                            send_event(state, action, time);
                        }
                    }
                    // The key is Hex's own only once the shortcut fired: Labs
                    // › Share shortcut lets it reach the app, and a bare press
                    // of it without the shortcut's modifiers is the user
                    // typing, which ends a paste continuation.
                    trigger = state.trigger_down;
                    if suppressed && state.binding.holds_menu_modifier() {
                        state.mask_release = true;
                    }
                } else if state.trigger_down {
                    state.trigger_down = false;
                    suppressed = !state.share_trigger;
                    if let Some(action) = state.capture.release(state.double_tap_enabled, time) {
                        send_event(state, action, time);
                    }
                    if state.dirty && !state.holds_any_required(&state.binding) {
                        state.dirty = false;
                    }
                }
            } else if vk == u32::from(VK_ESCAPE) {
                watchdog::escape_reached_app(state, is_down);
            } else if !modifier_event {
                // A key or button joining a held modifier-only shortcut makes
                // a chord meant for another application, such as Alt+Tab,
                // unless Labs › Hold through other keys keeps the long hold
                // and leaves the key to the app in front.
                if is_down
                    && state.trigger_vk.is_none()
                    && state.capture.active
                    && state.capture.flavour.normal()
                    && !state
                        .capture
                        .holds_through_joining_keys(crate::labs::hold_through_keys(), time)
                {
                    state.dirty = true;
                    if let Some(action) = state.capture.interrupt() {
                        send_event(state, action, time);
                    }
                }
            } else {
                if is_up
                    && state.mask_release
                    && matches!(vk as u16, VK_LMENU | VK_RMENU | VK_MENU | VK_LWIN | VK_RWIN)
                {
                    state.mask_release = false;
                    mask = !state.share_trigger;
                }
                if state.trigger_vk.is_none()
                    && state.capture.locked
                    && state.capture.flavour.normal()
                    && !state.dirty
                    && is_down
                    && state.holds(&state.binding)
                    && state.side_down.together(&state.binding)
                {
                    // Only a modifier-only shortcut finishes on a modifier
                    // press. A key chord such as Ctrl+Space finishes on its
                    // key, so resting a finger on Ctrl never ends a locked
                    // or toggled capture.
                    if let Some(action) =
                        state
                            .capture
                            .press(state.double_tap_enabled, state.double_tap_only, time)
                    {
                        state.dirty = true;
                        send_event(state, action, time);
                    }
                } else if state.capture.active
                    && state.capture.flavour.normal()
                    && !state.holds(&state.binding)
                {
                    if let Some(action) = state.capture.release(state.double_tap_enabled, time) {
                        state.dirty = state.holds_any_required(&state.binding);
                        send_event(state, action, time);
                    }
                } else if state.trigger_vk.is_none()
                    && !state.capture.active
                    && !state.capture.locked
                    && !state.dirty
                    && is_down
                    && state.holds(&state.binding)
                    && state.side_down.together(&state.binding)
                {
                    state.capture.claim(DictationFlavour::Normal);
                    if let Some(action) =
                        state
                            .capture
                            .press(state.double_tap_enabled, state.double_tap_only, time)
                    {
                        send_event(state, action, time);
                    }
                }
                if is_down && state.binding.masks_release() && state.holds(&state.binding) {
                    state.mask_release = true;
                }
                if state.dirty && !state.holds_any_required(&state.binding) {
                    state.dirty = false;
                }
            }
        }
    }
    let disposition = if mask {
        Disposition::Mask
    } else if suppressed {
        Disposition::Suppress
    } else {
        Disposition::Pass
    };
    Handled::new(disposition, trigger)
}

/// A swallowed key leaves this chord's Alt or Windows alone (#1254).
fn holds_menu(binding: Option<&WindowsHotkey>) -> bool {
    binding.is_some_and(WindowsHotkey::holds_menu_modifier)
}

fn send_event(state: &mut CallbackState, action: HotkeyAction, timestamp_ms: u32) {
    match state.events.try_send(HotkeyEvent {
        action,
        flavour: state.capture.flavour,
        timestamp_ms,
    }) {
        Ok(()) => crate::windows::dictation::wake(),
        Err(TrySendError::Full(_)) => {
            let _ = state
                .errors
                .send("Windows hotkey event queue overflowed".into());
        }
        Err(TrySendError::Disconnected(_)) => {}
    }
}

mod chord;

pub(crate) use chord::{ChordCapture, ChordPoll, NumpadWatch};

/// Numpad Enter's code in a binding: Enter's virtual key with the extended
/// flag, which no virtual key of its own names.
const NUMPAD_ENTER: u32 = 0x100 | VK_RETURN as u32;

/// The numpad key a keyboard hook last saw go down and not yet up, by its
/// `num` code, for the chord capture: polled key state can't tell Numpad 1
/// with Num Lock off from End. Zero for none. The capture runs a hook of
/// its own (`NumpadWatch`), so it doesn't depend on the listener's.
static NUMPAD_DOWN: AtomicU32 = AtomicU32::new(0);

/// The numpad digit and decimal keys by scan code. With Num Lock off they
/// send the navigation keys' codes (Numpad 1 is End), but keep these scan
/// codes without the extended flag, which the navigation cluster's keys
/// carry.
const NUMPAD_SCAN_CODES: [(u32, u32); 11] = [
    (0x52, 0x60),
    (0x4F, 0x61),
    (0x50, 0x62),
    (0x51, 0x63),
    (0x4B, 0x64),
    (0x4C, 0x65),
    (0x4D, 0x66),
    (0x47, 0x67),
    (0x48, 0x68),
    (0x49, 0x69),
    (0x53, 0x6E),
];

/// Notes a numpad key going down or up for the chord capture
/// (`NUMPAD_DOWN`), and returns its `num` code; `None` for any other key.
/// Only a hook tells a numpad key from the key it stands for.
fn track_numpad(event: &KBDLLHOOKSTRUCT, is_down: bool) -> Option<u32> {
    let vk = numpad_key(event)?;
    if is_down {
        NUMPAD_DOWN.store(vk, Ordering::Relaxed);
    } else {
        let _ = NUMPAD_DOWN.compare_exchange(vk, 0, Ordering::Relaxed, Ordering::Relaxed);
    }
    Some(vk)
}

/// A numpad key's `num` code, whatever Num Lock says: the digits and the
/// decimal by scan code, Numpad Enter by Enter's extended flag, and the
/// operators by their own virtual keys.
fn numpad_key(event: &KBDLLHOOKSTRUCT) -> Option<u32> {
    let extended = event.flags & LLKHF_EXTENDED != 0;
    if event.vkCode == u32::from(VK_RETURN) {
        return extended.then_some(NUMPAD_ENTER);
    }
    if matches!(event.vkCode, 0x6A | 0x6B | 0x6D | 0x6F) {
        return Some(event.vkCode);
    }
    if extended {
        return None;
    }
    NUMPAD_SCAN_CODES
        .into_iter()
        .find(|&(scan, _)| scan == event.scanCode)
        .map(|(_, vk)| vk)
}

/// Non-modifier keys the capture scans, paired with the names
/// [`virtual_key`] accepts (letters, digits, function keys, and the
/// layout's punctuation are scanned separately). Numpad keys are named
/// `num 1`, `num *`, ...: their own keys, not the top row's.
const CAPTURE_NAMED_KEYS: [(u16, &str); 45] = [
    (VK_SPACE, "space"),
    (VK_RETURN, "enter"),
    (VK_TAB, "tab"),
    (0x08, "backspace"),
    (0x2e, "delete"),
    (0x2d, "insert"),
    (0x24, "home"),
    (0x23, "end"),
    (0x21, "pageup"),
    (0x22, "pagedown"),
    (0x25, "left"),
    (0x26, "up"),
    (0x27, "right"),
    (0x28, "down"),
    (0x13, "pause"),
    (0x91, "scroll lock"),
    (0x5D, "menu"),
    // The punctuation keys, by their layout-independent virtual keys; the
    // keycap shows what each types on the active layout (`keycap_label`).
    (0xBA, "oem 1"),
    (0xBF, "oem 2"),
    (0xC0, "oem 3"),
    (0xDB, "oem 4"),
    (0xDC, "oem 5"),
    (0xDD, "oem 6"),
    (0xDE, "oem 7"),
    (0xDF, "oem 8"),
    (0xBB, "oem plus"),
    (0xBC, "oem comma"),
    (0xBD, "oem minus"),
    (0xBE, "oem period"),
    (0xE2, "oem 102"),
    (0x60, "num 0"),
    (0x61, "num 1"),
    (0x62, "num 2"),
    (0x63, "num 3"),
    (0x64, "num 4"),
    (0x65, "num 5"),
    (0x66, "num 6"),
    (0x67, "num 7"),
    (0x68, "num 8"),
    (0x69, "num 9"),
    (0x6A, "num *"),
    (0x6B, "num +"),
    (0x6D, "num -"),
    (0x6E, "num ."),
    (0x6F, "num /"),
];

/// The keycap for a saved key label: a punctuation key shows the character
/// it types on the active layout now (`;`, `ù`), else its name (`Oem 3`).
pub(crate) fn keycap_label(label: &str) -> String {
    let name = label.to_ascii_lowercase();
    CAPTURE_NAMED_KEYS
        .into_iter()
        .find(|&(_, named)| named == name && named.starts_with("oem "))
        .and_then(|(vk, _)| {
            // SAFETY: a pure lookup in this thread's keyboard layout.
            let mapped = unsafe { MapVirtualKeyW(u32::from(vk), MAPVK_VK_TO_CHAR) };
            char::from_u32(mapped & 0xFFFF)
                .filter(|character| !character.is_control() && !character.is_whitespace())
        })
        .map_or_else(
            || label.to_string(),
            |character| character.to_uppercase().collect(),
        )
}

/// The punctuation key that types `character` on the active layout, for a
/// shortcut written with the character (`ctrl+;`).
fn punctuation_key(character: char) -> Option<u32> {
    let mut units = [0u16; 2];
    let [unit] = character.encode_utf16(&mut units) else {
        return None;
    };
    // SAFETY: a pure lookup in this thread's keyboard layout.
    let scanned = unsafe { VkKeyScanW(*unit) };
    let vk = scanned as u16 & 0xFF;
    (scanned != -1
        && CAPTURE_NAMED_KEYS
            .iter()
            .any(|&(named, name)| named == vk && name.starts_with("oem ")))
    .then_some(u32::from(vk))
}

const MOUSE_KEYS: [(u16, &str); 5] = [
    (0x01, "mouse1"),
    (0x02, "mouse2"),
    (0x04, "mouse3"),
    (0x05, "mouse4"),
    (0x06, "mouse5"),
];

pub(crate) fn virtual_key(key: &str) -> Result<u32> {
    let normalized = key.trim().to_ascii_lowercase();
    if let Some(number) = HotkeyKey::mouse_number(&normalized) {
        return MOUSE_KEYS
            .get(usize::from(number - 1))
            .map(|&(vk, _)| u32::from(vk))
            .ok_or_else(|| eyre!("Windows supports Mouse 1 through Mouse 5"));
    }
    if let Some((vk, _)) = CAPTURE_NAMED_KEYS
        .into_iter()
        .find(|&(_, name)| name == normalized)
    {
        return Ok(u32::from(vk));
    }
    let virtual_key = match normalized.as_str() {
        "return" => u32::from(VK_RETURN),
        "escape" => u32::from(VK_ESCAPE),
        "num enter" => NUMPAD_ENTER,
        name if HotkeyKey::special_name(name) && name != "menu" && name != "pause" => 0x91,
        "page_up" => 0x21,
        "page_down" => 0x22,
        _ => {
            if let Some(number) = normalized
                .strip_prefix('f')
                .and_then(|number| number.parse::<u32>().ok())
                .filter(|number| (1..=24).contains(number))
            {
                0x70 + number - 1
            } else if let Some(character) = single_character(&normalized) {
                if character.is_ascii_alphanumeric() {
                    u32::from(character.to_ascii_uppercase() as u8)
                } else if let Some(vk) = punctuation_key(character) {
                    vk
                } else {
                    return Err(eyre!(
                        "{key} isn't a key on the active Windows keyboard layout"
                    ));
                }
            } else {
                return Err(eyre!("unsupported Windows shortcut key: {key}"));
            }
        }
    };
    Ok(virtual_key)
}

fn single_character(name: &str) -> Option<char> {
    let mut characters = name.chars();
    characters.next().filter(|_| characters.next().is_none())
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_LCONTROL;

    use super::*;

    #[test]
    fn a_shared_shortcut_trigger_does_not_end_a_paste_continuation() {
        // Labs › Share shortcut lets the dictation trigger's key reach the
        // app, and its press is Hex's own key, not typing between dictations.
        assert!(Handled::PASS.ends_continuation(true, false, 0));
        assert!(!Handled::own(Disposition::Pass).ends_continuation(true, false, 0));
        // A swallowed trigger never reaches the app, so it ends nothing either.
        assert!(!Handled::own(Disposition::Suppress).ends_continuation(true, false, 0));
        // A modifier-only trigger's press is part of the shortcut's chord.
        let control = u32::from(VK_LCONTROL);
        assert!(!Handled::PASS.ends_continuation(true, ModifierState::is_modifier(control), 0));
        // Hex's own synthetic paste or chord carries its marker.
        assert!(!Handled::PASS.ends_continuation(true, false, HEX_INJECTED));
    }

    /// The hook's state machine as the keyboard hook installs it, for one
    /// Ctrl+Space binding; the caller drives events through
    /// [`process_button`], whose `key_held` lookup stands in for the physical
    /// modifiers a test cannot press.
    fn callback_state(binding: WindowsHotkey, share_trigger: bool) -> Receiver<HotkeyEvent> {
        let (state, receiver) = callback_state_value(binding, share_trigger);
        *CALLBACK_STATE
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(state);
        receiver
    }

    /// [`callback_state`] without the listener's global: a test that drives
    /// the watchdog's heal directly must not compete with its peers.
    fn callback_state_value(
        binding: WindowsHotkey,
        share_trigger: bool,
    ) -> (CallbackState, Receiver<HotkeyEvent>) {
        let (events, receiver) = mpsc::sync_channel(EVENT_CAPACITY);
        let (errors, _unread) = mpsc::channel();
        let state = CallbackState {
            events,
            errors,
            activity: InputActivity::default(),
            binding,
            trigger_vk: Some(u32::from(VK_SPACE)),
            paste_last: None,
            paste_trigger_vk: None,
            raw: None,
            raw_trigger_vk: None,
            raw_trigger_down: false,
            terminal_paste: None,
            terminal_paste_trigger_vk: None,
            terminal_paste_trigger_down: false,
            second: None,
            second_trigger_vk: None,
            second_trigger_down: false,
            annotate: None,
            annotate_trigger_vk: None,
            annotate_trigger_down: false,
            drawing_escape_down: false,
            cancel: None,
            cancel_vk: None,
            cancel_down: false,
            modifiers: ModifierState::default(),
            raw_modifiers: ModifierState::default(),
            side_down: SideDownTimes::default(),
            trigger_down: false,
            paste_trigger_down: false,
            double_tap_enabled: false,
            double_tap_only: false,
            share_trigger,
            capture: CaptureState::default(),
            dirty: false,
            mask_release: false,
        };
        (state, receiver)
    }

    #[test]
    fn a_bare_trigger_key_ends_a_paste_continuation_but_the_shortcut_does_not() {
        let space = u32::from(VK_SPACE);
        let ctrl_held = |key: u16| key == VK_LCONTROL;

        // Labs › Share shortcut: Ctrl+Space reaches the app in front, and
        // since Hex's shortcut fired, its press is Hex's key, not typing.
        let events = callback_state(WindowsHotkey::default(), true);
        let handled = process_button(space, LeftControl::Other, true, 1_000, ctrl_held);
        assert_eq!(handled.disposition, Disposition::Pass);
        assert!(!handled.ends_continuation(true, false, 0));
        assert_eq!(
            events.try_recv().map(|event| event.action),
            Ok(HotkeyAction::Start)
        );

        // A bare Space without the shortcut's Ctrl is the user typing: it
        // reaches the app, starts nothing, and ends the continuation.
        let events = callback_state(WindowsHotkey::default(), true);
        let handled = process_button(space, LeftControl::Other, true, 2_000, |_| false);
        assert_eq!(handled.disposition, Disposition::Pass);
        assert!(handled.ends_continuation(true, false, 0));
        assert!(events.try_recv().is_err());

        // Sharing off, the shortcut's press is swallowed before it can end
        // anything.
        let events = callback_state(WindowsHotkey::default(), false);
        let handled = process_button(space, LeftControl::Other, true, 3_000, ctrl_held);
        assert_eq!(handled.disposition, Disposition::Suppress);
        assert!(!handled.ends_continuation(true, false, 0));
        assert_eq!(
            events.try_recv().map(|event| event.action),
            Ok(HotkeyAction::Start)
        );

        CALLBACK_STATE
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
    }

    #[test]
    fn numpad_pause_and_scroll_lock_round_trip_through_saved_settings() {
        for (vk, name) in CAPTURE_NAMED_KEYS {
            let saved = HotkeyKey::named(name);
            assert_eq!(saved.name(), name);
            assert_eq!(virtual_key(&saved.name()).unwrap(), u32::from(vk), "{name}");
        }
        let enter = HotkeyKey::named("num enter");
        assert_eq!(enter.label, "Num Enter");
        assert_eq!(virtual_key(&enter.name()).unwrap(), NUMPAD_ENTER);
        assert_ne!(virtual_key("num 1").unwrap(), virtual_key("1").unwrap());
        assert_eq!(HotkeyKey::named("num *").label, "Num *");
        assert_eq!(HotkeyKey::named("scroll lock").label, "Scroll Lock");
    }

    #[test]
    fn numpad_keys_keep_their_code_whatever_num_lock_says() {
        let event = |vk: u32, scan: u32, extended: bool| KBDLLHOOKSTRUCT {
            vkCode: vk,
            scanCode: scan,
            flags: if extended { LLKHF_EXTENDED } else { 0 },
            time: 0,
            dwExtraInfo: 0,
        };
        // Numpad 1: VK_NUMPAD1 with Num Lock on, End with it off.
        assert_eq!(numpad_key(&event(0x61, 0x4F, false)), Some(0x61));
        assert_eq!(numpad_key(&event(0x23, 0x4F, false)), Some(0x61));
        // The End key itself is extended.
        assert_eq!(numpad_key(&event(0x23, 0x4F, true)), None);
        // Numpad 5 with Num Lock off sends Clear.
        assert_eq!(numpad_key(&event(0x0C, 0x4C, false)), Some(0x65));
        assert_eq!(
            numpad_key(&event(u32::from(VK_RETURN), 0x1C, true)),
            Some(NUMPAD_ENTER)
        );
        assert_eq!(numpad_key(&event(u32::from(VK_RETURN), 0x1C, false)), None);
        assert_eq!(numpad_key(&event(0x6F, 0x35, true)), Some(0x6F));
    }

    #[test]
    fn pause_and_scroll_lock_stand_alone_but_numpad_keys_need_a_modifier() {
        let alone = |key: &str| WindowsHotkey {
            control: None,
            windows: None,
            alt: None,
            shift: None,
            key: Some(key.into()),
        };
        assert!(alone("pause").validate().is_ok());
        assert!(alone("scroll lock").validate().is_ok());
        assert!(alone("menu").validate().is_ok());
        // Settings gate these behind Labs › Special keys alone.
        assert!(alone("home").validate().is_ok());
        assert!(alone("num 5").validate().is_err());
        let control = WindowsHotkey {
            control: Some(ModifierSide::Either),
            ..alone("num 5")
        };
        assert!(control.validate().is_ok());
    }

    #[test]
    fn mouse_recorder_waits_for_the_opening_click_to_be_released() {
        let mut capture = ChordCapture::default();
        assert!(!capture.arm_mouse(true));
        assert!(!capture.arm_mouse(true));
        assert!(capture.arm_mouse(false));
        assert!(capture.arm_mouse(true), "the next click can be recorded");
    }

    #[test]
    fn mouse_buttons_validate_and_round_trip_without_modifiers() {
        for number in 1..=5 {
            let binding = WindowsHotkey {
                control: None,
                windows: None,
                alt: None,
                shift: None,
                key: Some(format!("mouse{number}")),
            };
            assert!(binding.validate().is_ok());
            assert_eq!(WindowsHotkey::from(&HotkeyBinding::from(&binding)), binding);
            assert_eq!(
                virtual_key(binding.key.as_ref().unwrap()).unwrap(),
                u32::from(MOUSE_KEYS[number - 1].0)
            );
        }
        assert!(virtual_key("mouse6").is_err());
        assert!(virtual_key("mouse0").is_err());
    }

    #[test]
    fn mouse_hook_decodes_both_auxiliary_buttons_and_ignores_wheel_messages() {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        for (down, up, data, vk) in [
            (WM_LBUTTONDOWN, WM_LBUTTONUP, 0, 1),
            (WM_RBUTTONDOWN, WM_RBUTTONUP, 0, 2),
            (WM_MBUTTONDOWN, WM_MBUTTONUP, 0, 4),
            (WM_XBUTTONDOWN, WM_XBUTTONUP, 1 << 16, 5),
            (WM_XBUTTONDOWN, WM_XBUTTONUP, 2 << 16, 6),
        ] {
            assert_eq!(mouse_message(down, data), Some((vk, true)));
            assert_eq!(mouse_message(up, data), Some((vk, false)));
        }
        assert_eq!(mouse_message(WM_MOUSEWHEEL, 120 << 16), None);
        assert_eq!(mouse_message(WM_XBUTTONDOWN, 3 << 16), None);
    }

    #[test]
    fn default_binding_is_ctrl_space() {
        let binding = WindowsHotkey::default();
        assert_eq!(binding.label(), "Ctrl+Space");
        assert!(binding.validate().is_ok());
    }

    #[test]
    fn unmodified_letters_are_rejected() {
        let binding = WindowsHotkey {
            control: None,
            windows: None,
            alt: None,
            shift: None,
            key: Some("d".into()),
        };
        assert!(binding.validate().is_err());
    }

    #[test]
    fn both_sides_of_one_modifier_make_a_chord_of_two_keys() {
        let both_alts = WindowsHotkey {
            control: None,
            windows: None,
            alt: Some(ModifierSide::Both),
            shift: None,
            key: None,
        };
        assert!(both_alts.validate().is_ok());
        assert_eq!(both_alts.label(), "Alt+Alt");
        let right_alt = WindowsHotkey {
            alt: Some(ModifierSide::Right),
            ..both_alts.clone()
        };
        assert!(
            right_alt.validate().is_ok(),
            "one key alone is a shortcut too"
        );
        assert_eq!(
            WindowsHotkey::from(&HotkeyBinding::from(&both_alts)),
            both_alts
        );

        let mut held = ModifierState::default();
        held.update(u32::from(VK_LMENU), true);
        assert!(!held.matches(&both_alts), "one Alt is not the chord");
        held.update(u32::from(VK_RMENU), true);
        assert!(held.matches(&both_alts));
        let either_alt_and_control = WindowsHotkey {
            control: Some(ModifierSide::Either),
            alt: Some(ModifierSide::Either),
            ..both_alts.clone()
        };
        held.update(u32::from(VK_LCONTROL), true);
        assert!(!held.matches(&both_alts), "an extra key is not the chord");
        assert!(
            held.matches(&either_alt_and_control),
            "either side still takes both"
        );
    }

    #[test]
    fn both_sides_must_go_down_together() {
        let both_alts = WindowsHotkey {
            control: None,
            windows: None,
            alt: Some(ModifierSide::Both),
            shift: None,
            key: None,
        };
        let window = crate::dictation::BOTH_SIDES_WINDOW.as_millis() as u32;
        let mut times = SideDownTimes::default();
        times.update(u32::from(VK_RMENU), true, 1_000);
        assert!(!times.together(&both_alts));
        times.update(u32::from(VK_LMENU), true, 1_000 + window);
        assert!(
            times.together(&both_alts),
            "either order, inside the window"
        );
        times.update(u32::from(VK_LMENU), true, 5_000);
        assert!(
            times.together(&both_alts),
            "auto-repeat keeps the first press"
        );
        times.update(u32::from(VK_LMENU), false, 5_100);
        times.update(u32::from(VK_LMENU), true, 5_200);
        assert!(
            !times.together(&both_alts),
            "added later, it presses nothing"
        );
        assert!(
            times.together(&WindowsHotkey::default()),
            "other chords never wait"
        );
        let mut wrapped = SideDownTimes::default();
        wrapped.update(u32::from(VK_LMENU), true, u32::MAX - 10);
        wrapped.update(u32::from(VK_RMENU), true, 10);
        assert!(wrapped.together(&both_alts), "tick wraparound");
    }

    #[test]
    fn one_modifier_alone_is_a_shortcut_and_alt_or_windows_mask_their_release() {
        let none = WindowsHotkey {
            control: None,
            windows: None,
            alt: None,
            shift: None,
            key: None,
        };
        assert!(none.validate().is_err(), "a shortcut needs some key");
        let right_control = WindowsHotkey {
            control: Some(ModifierSide::Right),
            ..none.clone()
        };
        assert!(right_control.validate().is_ok());
        assert!(!right_control.masks_release(), "Ctrl alone opens nothing");
        let right_alt = WindowsHotkey {
            alt: Some(ModifierSide::Right),
            ..none.clone()
        };
        assert!(right_alt.validate().is_ok());
        assert!(right_alt.masks_release(), "Alt alone opens the menu bar");
        let windows = WindowsHotkey {
            windows: Some(ModifierSide::Either),
            ..none.clone()
        };
        assert!(windows.masks_release(), "Windows alone opens Start");
        let alt_space = WindowsHotkey {
            key: Some("space".into()),
            ..right_alt
        };
        assert!(!alt_space.masks_release(), "not a modifier-only shortcut");
        assert!(
            alt_space.holds_menu_modifier(),
            "a swallowed key leaves Alt alone, so its release is masked (#1254)"
        );
        assert!(!WindowsHotkey::default().holds_menu_modifier());

        let mut held = ModifierState::default();
        held.update(u32::from(VK_RMENU), true);
        assert!(held.matches(&WindowsHotkey {
            alt: Some(ModifierSide::Right),
            ..none.clone()
        }));
        held.update(u32::from(VK_LCONTROL), true);
        assert!(
            !held.matches(&WindowsHotkey {
                alt: Some(ModifierSide::Right),
                ..none
            }),
            "a real Left Ctrl is not Right Alt alone"
        );
    }

    #[test]
    fn altgr_left_ctrl_is_no_ctrl_for_a_right_alt_shortcut() {
        let none = WindowsHotkey {
            control: None,
            windows: None,
            alt: None,
            shift: None,
            key: None,
        };
        let right_alt = WindowsHotkey {
            alt: Some(ModifierSide::Right),
            ..none.clone()
        };
        // AltGr on a layout that has it: Windows sends a synthesized Left
        // Ctrl (scan code 0x21D) just before the Right Alt (0x38). A real
        // Left Ctrl is 0x1D.
        assert_eq!(
            LeftControl::of(u32::from(VK_LCONTROL), 0x21D),
            LeftControl::AltGr
        );
        assert_eq!(
            LeftControl::of(u32::from(VK_LCONTROL), 0x1D),
            LeftControl::Real
        );
        assert_eq!(
            LeftControl::of(u32::from(VK_RMENU), 0x38),
            LeftControl::Other,
            "keys that are not a Left Ctrl"
        );

        // The async key state reports the synthesized Ctrl; the masked read,
        // which the listener resyncs from and the recorder polls, drops it.
        let altgr = ModifierState {
            left_control: true,
            right_alt: true,
            ..ModifierState::default()
        };
        let held = altgr.without_altgr(true);
        assert!(!held.left_control && held.right_alt);
        assert!(held.matches(&right_alt));
        assert!(
            !altgr.without_altgr(false).matches(&right_alt),
            "a real Left Ctrl is a Ctrl"
        );
    }

    #[test]
    fn an_altgr_recorded_control_option_still_fires() {
        let none = WindowsHotkey {
            control: None,
            windows: None,
            alt: None,
            shift: None,
            key: None,
        };
        let right_alt = WindowsHotkey {
            alt: Some(ModifierSide::Right),
            ..none.clone()
        };
        // What `recorded_sides` stored on an AltGr layout before AltGr's Ctrl
        // was ignored: both modifiers held, their sides left open.
        let recorded = WindowsHotkey {
            control: Some(ModifierSide::Either),
            alt: Some(ModifierSide::Either),
            windows: None,
            shift: None,
            key: None,
        };
        let altgr = ModifierState {
            left_control: true,
            right_alt: true,
            ..ModifierState::default()
        };
        let masked = altgr.without_altgr(true);

        // Right Alt alone fires on AltGr, and so does the older recording:
        // one view drops the Ctrl, the other counts it, and either matches.
        assert!(masked.holds(&right_alt, &altgr));
        assert!(!masked.matches(&recorded), "the masked view has no Ctrl");
        assert!(masked.holds(&recorded, &altgr));
        assert!(masked.holds_any_required(&recorded, &altgr));
        assert!(masked.holds_any_required(&right_alt, &altgr));

        // A real Left Ctrl + Right Alt is a chord: Right Alt alone is not
        // down, and only the binding that names Ctrl is.
        let real = altgr.without_altgr(false);
        assert!(!real.holds(&right_alt, &altgr));
        assert!(real.holds(&recorded, &altgr));
        assert!(
            !masked.holds(&recorded, &masked),
            "with no AltGr read, the masked view alone has the last word"
        );
    }

    #[test]
    fn a_key_chord_on_altgr_is_not_alt_with_that_key() {
        let chord =
            |control: Option<ModifierSide>, alt: Option<ModifierSide>, key: &str| WindowsHotkey {
                control,
                windows: None,
                alt,
                shift: None,
                key: Some(key.into()),
            };
        let either = Some(ModifierSide::Either);
        let alt_v = chord(None, either, "v");
        let control_alt_v = chord(either, either, "v");
        let control_alt_right_v = chord(either, Some(ModifierSide::Right), "v");
        let control_alt_f9 = chord(either, either, "f9");
        let altgr = ModifierState {
            left_control: true,
            right_alt: true,
            ..ModifierState::default()
        };
        let masked = altgr.without_altgr(true);
        // AltGr+V types `@` on AltGr layouts (#1964): it must press neither
        // an Alt+V nor a Ctrl+Alt+V shortcut, so the key reaches the app.
        assert!(!masked.holds(&alt_v, &altgr));
        assert!(!masked.holds(&control_alt_v, &altgr));
        assert!(!masked.holds(&control_alt_right_v, &altgr));
        assert!(!masked.holds_any_required(&control_alt_v, &altgr));
        // A key that types nothing keeps its chord: AltGr+F9 still presses
        // Ctrl+Alt+F9, as a recording made of it always has.
        assert!(masked.holds(&control_alt_f9, &altgr));
        assert!(masked.holds_any_required(&control_alt_f9, &altgr));

        // A real Left Ctrl with Right Alt is a Ctrl chord, and so is Left
        // Ctrl with Left Alt: the recorded chord presses on either.
        let real_ctrl_right_alt = altgr.without_altgr(false);
        assert!(real_ctrl_right_alt.holds(&control_alt_v, &real_ctrl_right_alt));
        let left_ctrl_left_alt = ModifierState {
            left_control: true,
            left_alt: true,
            ..ModifierState::default()
        };
        assert!(left_ctrl_left_alt.holds(&control_alt_v, &left_ctrl_left_alt));
        // A chord that names Right Alt needs it: Left Alt is not Right Alt.
        assert!(!left_ctrl_left_alt.holds(&control_alt_right_v, &left_ctrl_left_alt));

        // Right Alt alone still fires on AltGr (#1950).
        let right_alt = WindowsHotkey {
            control: None,
            windows: None,
            alt: Some(ModifierSide::Right),
            shift: None,
            key: None,
        };
        assert!(masked.holds(&right_alt, &altgr));
    }

    #[test]
    fn a_key_joining_a_held_modifier_ends_it_quietly_unless_it_records() {
        let mut state = CaptureState::default();
        assert_eq!(state.press(true, false, 100), Some(HotkeyAction::Start));
        assert_eq!(state.interrupt(), Some(HotkeyAction::Cancel));
        assert!(!state.active && !state.locked);
        assert_eq!(state.release(true, 150), None, "its release is spent");
        // The armed first tap of double-tap-only records nothing, so it
        // reports nothing: a bare cancel would cancel the newest job.
        assert_eq!(state.press(true, true, 1_000), None);
        assert_eq!(state.interrupt(), None);
        assert_eq!(state.press(true, true, 1_100), None, "no double tap");
        // A locked capture is not held, so typing never ends it.
        let mut locked = CaptureState {
            toggle: true,
            ..CaptureState::default()
        };
        locked.press(true, false, 100);
        locked.release(true, 140);
        assert_eq!(locked.interrupt(), None);
        assert!(locked.locked);
    }

    #[test]
    fn a_release_missed_while_locked_does_not_block_the_shortcut() {
        let binding = WindowsHotkey::default();
        // Win+L: the hook saw Windows go down, then the lock screen took
        // its release, so tracking events alone leaves it held.
        let mut tracked = ModifierState::default();
        tracked.update(u32::from(VK_LWIN), true);
        tracked.update(u32::from(VK_LCONTROL), true);
        assert!(!tracked.matches(&binding));
        // After unlocking, the system reports every key up before this
        // Ctrl press, which `update` then applies.
        let mut synced = ModifierState::read(|_| false);
        synced.update(u32::from(VK_LCONTROL), true);
        assert!(synced.matches(&binding));
    }

    #[test]
    fn hotkeys_round_trip_through_the_portable_binding() {
        for hotkey in [
            WindowsHotkey::default(),
            WindowsHotkey {
                control: Some(ModifierSide::Either),
                windows: None,
                alt: Some(ModifierSide::Either),
                shift: None,
                key: Some("v".into()),
            },
            WindowsHotkey {
                key: Some("pageup".into()),
                shift: Some(ModifierSide::Either),
                windows: Some(ModifierSide::Either),
                ..WindowsHotkey::default()
            },
        ] {
            assert_eq!(WindowsHotkey::from(&HotkeyBinding::from(&hotkey)), hotkey);
        }
    }

    #[test]
    fn event_delay_is_subtracted_from_the_audio_timeline() {
        let event = HotkeyEvent {
            action: HotkeyAction::Finish,
            flavour: DictationFlavour::Normal,
            timestamp_ms: 1_000,
        };
        let captured = CaptureInstant::from_nanos(10_000_000_000);
        assert_eq!(
            event.occurred_at_with(1_025, captured).as_nanos(),
            9_975_000_000
        );
    }

    #[test]
    fn tick_count_wraparound_keeps_a_short_delay() {
        let event = HotkeyEvent {
            action: HotkeyAction::Start,
            flavour: DictationFlavour::Normal,
            timestamp_ms: u32::MAX - 4,
        };
        let captured = CaptureInstant::from_nanos(1_000_000_000);
        assert_eq!(event.occurred_at_with(5, captured).as_nanos(), 990_000_000);
    }

    #[test]
    fn an_implausible_event_time_counts_as_now() {
        // Injected input stamped slightly in the future wraps to a huge
        // delay; it must not move the shortcut a minute back (#1005).
        let captured = CaptureInstant::from_nanos(90_000_000_000);
        for timestamp_ms in [1_010, 1_000u32.wrapping_sub(MAX_EVENT_DELAY_MS + 1)] {
            let event = HotkeyEvent {
                action: HotkeyAction::Start,
                flavour: DictationFlavour::Normal,
                timestamp_ms,
            };
            assert_eq!(
                event.occurred_at_with(1_000, captured).as_nanos(),
                captured.as_nanos()
            );
        }
    }

    #[test]
    fn second_tap_locks_until_the_shortcut_is_pressed_again() {
        let mut state = CaptureState::default();
        assert_eq!(state.press(true, false, 100), Some(HotkeyAction::Start));
        assert_eq!(state.release(true, 140), Some(HotkeyAction::Finish));
        assert_eq!(state.press(true, false, 250), Some(HotkeyAction::Start));
        assert_eq!(state.release(true, 290), None);
        assert!(state.locked);
        assert_eq!(state.press(true, false, 500), Some(HotkeyAction::Finish));
        assert!(!state.locked);
    }

    #[test]
    fn inhibiting_shortcuts_cancels_a_locked_capture() {
        let mut state = CaptureState::default();
        state.press(true, false, 100);
        state.release(true, 120);
        state.press(true, false, 200);
        state.release(true, 220);
        assert_eq!(state.cancel(), Some(HotkeyAction::Cancel));
        assert!(!state.active);
        assert!(!state.locked);
    }

    #[test]
    fn disabled_double_tap_keeps_both_taps_as_hold_captures() {
        let mut state = CaptureState::default();
        assert_eq!(state.press(false, false, 100), Some(HotkeyAction::Start));
        assert_eq!(state.release(false, 120), Some(HotkeyAction::Finish));
        assert_eq!(state.press(false, false, 200), Some(HotkeyAction::Start));
        assert_eq!(state.release(false, 220), Some(HotkeyAction::Finish));
        assert!(!state.locked);
    }

    #[test]
    fn double_tap_only_arms_silently_and_records_from_the_second_tap() {
        let mut state = CaptureState::default();
        // A lone hold must not record.
        assert_eq!(state.press(true, true, 100), None);
        assert_eq!(state.release(true, 900), None);
        // First tap of a double tap arms; the second starts hands-free.
        assert_eq!(state.press(true, true, 2_000), None);
        assert_eq!(state.release(true, 2_040), None);
        assert_eq!(state.press(true, true, 2_150), Some(HotkeyAction::Start));
        assert_eq!(state.release(true, 2_190), None);
        assert!(state.locked);
        // The next press finishes the locked capture.
        assert_eq!(state.press(true, true, 3_000), Some(HotkeyAction::Finish));
        assert!(!state.locked);
    }

    #[test]
    fn command_capture_finishes_on_the_first_physical_press_in_either_mode() {
        for toggle in [false, true] {
            for flavour in [DictationFlavour::Normal, DictationFlavour::Raw] {
                let mut state = CaptureState {
                    toggle,
                    ..Default::default()
                };
                state.start_command(flavour);
                assert!(state.recording);
                assert_eq!(state.wants_cancel(), toggle);
                assert!(!state.owned_by_other(flavour));
                assert_eq!(state.press(true, false, 100), Some(HotkeyAction::Finish));
                assert_eq!(state.release(true, 150), None);
                assert!(!state.wants_cancel());
                assert!(!state.recording);
            }
        }
    }

    #[test]
    fn command_stop_clears_a_physical_hold_without_a_second_finish() {
        let mut state = CaptureState::default();
        assert_eq!(state.press(true, false, 100), Some(HotkeyAction::Start));
        assert_eq!(state.cancel(), Some(HotkeyAction::Cancel));
        assert_eq!(state.release(true, 150), None);
        assert_eq!(state.press(true, false, 200), Some(HotkeyAction::Start));
        assert_eq!(state.release(true, 250), Some(HotkeyAction::Finish));
    }

    #[test]
    fn command_capture_cancels_without_leaving_the_hook_locked() {
        let mut state = CaptureState {
            toggle: true,
            ..Default::default()
        };
        state.start_command(DictationFlavour::Normal);
        assert!(state.wants_cancel());
        assert_eq!(state.cancel(), Some(HotkeyAction::Cancel));
        assert!(!state.wants_cancel());
        assert_eq!(state.press(false, false, 100), Some(HotkeyAction::Start));
    }

    #[test]
    fn toggle_mode_starts_on_a_press_and_finishes_on_the_next() {
        for (double_tap_enabled, double_tap_only) in [(false, false), (true, false), (true, true)] {
            let mut state = CaptureState {
                toggle: true,
                ..CaptureState::default()
            };
            assert_eq!(
                state.press(double_tap_enabled, double_tap_only, 100),
                Some(HotkeyAction::Start)
            );
            // Releasing keeps recording, however long the press was.
            assert_eq!(state.release(double_tap_enabled, 140), None);
            assert!(state.locked);
            assert_eq!(
                state.press(double_tap_enabled, double_tap_only, 5_000),
                Some(HotkeyAction::Finish)
            );
            assert_eq!(state.release(double_tap_enabled, 5_050), None);
            assert!(!state.active && !state.locked);
            // The next press starts a fresh capture, not a double tap.
            assert_eq!(
                state.press(double_tap_enabled, double_tap_only, 5_100),
                Some(HotkeyAction::Start)
            );
        }
    }

    /// A keyless chord whose release never arrived: Toggle locks exactly as
    /// its release would, Hold finishes, and a chord still holds while a
    /// required modifier is down leaves it alone.
    #[test]
    fn a_keyless_chord_whose_release_never_arrived_resynchronizes() {
        let binding = WindowsHotkey {
            control: None,
            windows: Some(ModifierSide::Either),
            alt: Some(ModifierSide::Either),
            shift: None,
            key: None,
        };

        // Hold: both modifiers still report down, so the chord is held.
        let (mut state, events) = callback_state_value(binding.clone(), false);
        assert_eq!(
            state.capture.press(false, false, 1_000),
            Some(HotkeyAction::Start)
        );
        state.mask_release = true;
        assert!(!watchdog::heal(&mut state, 2_000, |key| key == VK_LMENU || key == VK_LWIN));
        assert!(state.capture.active && !state.dirty && state.mask_release);
        assert!(events.try_recv().is_err());

        // Windows is up and Alt still down: the release arrived, and the
        // capture stays dirty until Alt comes up, as the normal path does.
        assert!(watchdog::heal(&mut state, 2_000, |key| key == VK_LMENU));
        assert!(!state.mask_release);
        assert_eq!(
            events.try_recv().map(|event| event.action),
            Ok(HotkeyAction::Finish)
        );
        assert!(!state.capture.active && state.dirty);

        // Toggle: the lost release locks, and the next press finishes.
        let (mut state, events) = callback_state_value(binding, false);
        state.capture = CaptureState {
            toggle: true,
            ..CaptureState::default()
        };
        assert_eq!(
            state.capture.press(false, false, 1_000),
            Some(HotkeyAction::Start)
        );
        state.mask_release = true;
        assert!(watchdog::heal(&mut state, 2_000, |_| false));
        assert!(!state.capture.active && state.capture.locked && !state.mask_release);
        assert!(events.try_recv().is_err());
        assert_eq!(
            state.capture.press(false, false, 3_000),
            Some(HotkeyAction::Finish)
        );
    }

    /// The release heal leaves alone what it does not own: a chord with a
    /// key, another shortcut's capture, and an idle capture.
    #[test]
    fn the_release_heal_leaves_other_captures_alone() {
        let (mut keyed, events) = callback_state_value(WindowsHotkey::default(), false);
        assert_eq!(
            keyed.capture.press(false, false, 1_000),
            Some(HotkeyAction::Start)
        );
        assert!(!watchdog::heal(&mut keyed, 2_000, |_| false));
        assert!(keyed.capture.active);
        assert!(events.try_recv().is_err());

        let keyless = WindowsHotkey {
            control: None,
            windows: Some(ModifierSide::Either),
            alt: None,
            shift: None,
            key: None,
        };
        let (mut other, events) = callback_state_value(keyless, false);
        other.capture = CaptureState {
            active: true,
            recording: true,
            flavour: DictationFlavour::Raw,
            ..CaptureState::default()
        };
        assert!(!watchdog::heal(&mut other, 2_000, |_| false));
        assert!(other.capture.active);
        assert!(events.try_recv().is_err());

        let (mut idle, events) = callback_state_value(WindowsHotkey::default(), false);
        assert!(!watchdog::heal(&mut idle, 2_000, |_| false));
        assert!(!idle.capture.active);
        assert!(events.try_recv().is_err());
    }

    /// Labs › Stop phrase ends only a hands-free capture, as the shortcut's
    /// finishing press would; a held press records on.
    #[test]
    fn a_stop_phrase_finishes_only_a_hands_free_capture() {
        let mut idle = CaptureState::default();
        assert!(!idle.finish_hands_free());

        // A held press is speech, not a finish.
        let mut held = CaptureState::default();
        assert_eq!(held.press(true, false, 100), Some(HotkeyAction::Start));
        assert!(!held.finish_hands_free());
        assert!(held.active && held.recording);

        // A Toggle press released locks, and the phrase ends that capture.
        let mut locked = CaptureState {
            toggle: true,
            ..CaptureState::default()
        };
        assert_eq!(locked.press(true, false, 100), Some(HotkeyAction::Start));
        assert_eq!(locked.release(true, 140), None);
        assert!(locked.locked);
        assert!(locked.finish_hands_free());
        assert!(!locked.locked && !locked.recording);
        assert!(!locked.finish_hands_free(), "a capture finishes once");
        // The next press starts a fresh capture, as after a finishing press.
        assert_eq!(locked.press(true, false, 5_000), Some(HotkeyAction::Start));
    }

    #[test]
    fn inhibiting_shortcuts_cancels_a_toggled_capture() {
        let mut state = CaptureState {
            toggle: true,
            ..CaptureState::default()
        };
        state.press(true, false, 100);
        state.release(true, 120);
        assert_eq!(state.cancel(), Some(HotkeyAction::Cancel));
        assert!(!state.active && !state.locked);
    }

    /// Toggle's cancel key is Hex's only while a toggled capture records:
    /// held or locked, never idle, and never in Hold mode.
    #[test]
    fn cancel_is_wanted_only_while_a_toggled_capture_records() {
        let mut state = CaptureState {
            toggle: true,
            ..CaptureState::default()
        };
        assert!(!state.wants_cancel());
        assert_eq!(state.press(true, false, 100), Some(HotkeyAction::Start));
        assert!(state.wants_cancel());
        state.release(true, 120);
        assert!(state.locked && state.wants_cancel());
        assert_eq!(state.cancel(), Some(HotkeyAction::Cancel));
        assert!(!state.wants_cancel());
        // The next press starts a fresh recording.
        assert_eq!(state.press(true, false, 1_000), Some(HotkeyAction::Start));

        let mut hold = CaptureState::default();
        hold.press(true, false, 100);
        assert!(!hold.wants_cancel());
        hold.release(true, 120);
        hold.press(true, false, 200);
        hold.release(true, 220);
        assert!(hold.locked && !hold.wants_cancel());
    }

    #[test]
    fn a_second_dictation_shortcut_owns_the_capture_it_starts() {
        let mut state = CaptureState::default();
        state.claim(DictationFlavour::TerminalPaste);
        assert_eq!(state.press(true, false, 100), Some(HotkeyAction::Start));
        assert!(state.owned_by_other(DictationFlavour::Normal));
        assert!(state.owned_by_other(DictationFlavour::Raw));
        // The dictation shortcut can't claim or finish another's capture.
        state.claim(DictationFlavour::Normal);
        assert_eq!(state.flavour, DictationFlavour::TerminalPaste);
        assert_eq!(state.press(true, false, 150), None);
        assert_eq!(state.release(true, 200), Some(HotkeyAction::Finish));
        // A tap of another shortcut right after is not its second tap.
        state.claim(DictationFlavour::Normal);
        assert_eq!(state.flavour, DictationFlavour::Normal);
        assert_eq!(state.press(true, false, 250), Some(HotkeyAction::Start));
        assert!(!state.locked);
    }

    /// Labs › Second dictation shortcut is its own owner too: the dictation,
    /// raw, and terminal-paste shortcuts neither claim its capture nor count
    /// a tap around it toward its double tap.
    #[test]
    fn the_second_dictation_flavour_owns_its_capture() {
        let mut state = CaptureState::default();
        state.claim(DictationFlavour::Second);
        assert_eq!(state.press(true, false, 100), Some(HotkeyAction::Start));
        assert!(state.owned_by_other(DictationFlavour::Normal));
        assert!(state.owned_by_other(DictationFlavour::Raw));
        assert!(state.owned_by_other(DictationFlavour::TerminalPaste));
        for other in [
            DictationFlavour::Normal,
            DictationFlavour::Raw,
            DictationFlavour::TerminalPaste,
        ] {
            state.claim(other);
            assert_eq!(state.flavour, DictationFlavour::Second);
        }
        assert_eq!(state.release(true, 150), Some(HotkeyAction::Finish));
        // Another shortcut's tap right after is not Second's second tap.
        state.claim(DictationFlavour::Normal);
        assert_eq!(state.press(true, false, 200), Some(HotkeyAction::Start));
        assert!(!state.locked);
        assert_eq!(state.release(true, 250), Some(HotkeyAction::Finish));
        // Second starts a fresh capture of its own.
        state.claim(DictationFlavour::Second);
        assert_eq!(state.press(true, false, 300), Some(HotkeyAction::Start));
        assert!(!state.locked);
    }

    #[test]
    fn double_tap_only_taps_outside_the_window_never_record() {
        let mut state = CaptureState::default();
        assert_eq!(state.press(true, true, 100), None);
        assert_eq!(state.release(true, 140), None);
        // Too late for the double-tap window: arms again instead of starting.
        assert_eq!(state.press(true, true, 1_000), None);
        assert_eq!(state.release(true, 1_040), None);
        assert!(!state.locked);
    }
}

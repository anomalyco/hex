use std::ptr;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use color_eyre::eyre::{Result, eyre};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU,
    VK_RCONTROL, VK_RETURN, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT, VK_SPACE, VK_TAB,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE, PeekMessageW,
    PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::audio::CaptureInstant;
use crate::windows_settings::WindowsHotkey;

const EVENT_CAPACITY: usize = 16;
const MAX_EVENT_DELAY_MS: u32 = 60_000;
const DOUBLE_TAP_WINDOW_MS: u32 = 300;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeyAction {
    Start,
    Finish,
    Cancel,
    PasteLast,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HotkeyEvent {
    pub action: HotkeyAction,
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
        let delay = observed_at_ms
            .wrapping_sub(self.timestamp_ms)
            .min(MAX_EVENT_DELAY_MS);
        captured_through
            .checked_sub(Duration::from_millis(u64::from(delay)))
            .unwrap_or(CaptureInstant::ZERO)
    }
}

struct CallbackState {
    events: SyncSender<HotkeyEvent>,
    errors: mpsc::Sender<String>,
    binding: WindowsHotkey,
    trigger_vk: Option<u32>,
    paste_last: Option<WindowsHotkey>,
    paste_trigger_vk: Option<u32>,
    modifiers: ModifierState,
    trigger_down: bool,
    paste_trigger_down: bool,
    escape_down: bool,
    double_tap_enabled: bool,
    capture: CaptureState,
    dirty: bool,
}

struct HookBindings {
    dictation: WindowsHotkey,
    dictation_trigger_vk: Option<u32>,
    paste_last: Option<WindowsHotkey>,
    paste_trigger_vk: Option<u32>,
    double_tap_enabled: bool,
}

#[derive(Default)]
struct CaptureState {
    active: bool,
    locked: bool,
    second_tap: bool,
    last_release_ms: Option<u32>,
}

impl CaptureState {
    fn press(&mut self, double_tap_enabled: bool, timestamp_ms: u32) -> Option<HotkeyAction> {
        if self.locked {
            self.locked = false;
            self.last_release_ms = None;
            return Some(HotkeyAction::Finish);
        }
        if self.active {
            return None;
        }
        self.active = true;
        self.second_tap = double_tap_enabled
            && self
                .last_release_ms
                .take()
                .is_some_and(|released| timestamp_ms.wrapping_sub(released) < DOUBLE_TAP_WINDOW_MS);
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
        Some(HotkeyAction::Finish)
    }

    fn cancel(&mut self) -> Option<HotkeyAction> {
        if !self.active && !self.locked {
            return None;
        }
        self.active = false;
        self.locked = false;
        self.second_tap = false;
        self.last_release_ms = None;
        Some(HotkeyAction::Cancel)
    }
}

#[derive(Default)]
struct ModifierState {
    left_control: bool,
    right_control: bool,
    left_windows: bool,
    right_windows: bool,
    left_alt: bool,
    right_alt: bool,
    left_shift: bool,
    right_shift: bool,
}

static CALLBACK_STATE: Mutex<Option<CallbackState>> = Mutex::new(None);

pub struct WindowsHotkeyMonitor {
    pub events: Receiver<HotkeyEvent>,
    pub errors: Receiver<String>,
    thread_id: u32,
    worker: Option<JoinHandle<()>>,
}

impl WindowsHotkeyMonitor {
    pub fn start(
        binding: WindowsHotkey,
        paste_last: Option<WindowsHotkey>,
        double_tap_enabled: bool,
    ) -> Result<Self> {
        binding.validate()?;
        let trigger_vk = binding.key.as_deref().map(virtual_key).transpose()?;
        let paste_trigger_vk = paste_last
            .as_ref()
            .map(|binding| {
                binding.validate()?;
                binding
                    .key
                    .as_deref()
                    .ok_or_else(|| eyre!("Paste Last requires a non-modifier key"))
                    .and_then(virtual_key)
            })
            .transpose()?;
        let label = binding.label();
        let (event_sender, events) = mpsc::sync_channel(EVENT_CAPACITY);
        let (error_sender, errors) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("windows-hotkey".into())
            .spawn(move || {
                run_hook(
                    HookBindings {
                        dictation: binding,
                        dictation_trigger_vk: trigger_vk,
                        paste_last,
                        paste_trigger_vk,
                        double_tap_enabled,
                    },
                    event_sender,
                    error_sender,
                    ready_sender,
                )
            })?;
        let thread_id = ready_receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| eyre!("timed out installing the Windows {label} keyboard hook"))?
            .map_err(|error| eyre!(error))?;
        Ok(Self {
            events,
            errors,
            thread_id,
            worker: Some(worker),
        })
    }
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

fn run_hook(
    bindings: HookBindings,
    events: SyncSender<HotkeyEvent>,
    errors: mpsc::Sender<String>,
    ready: SyncSender<Result<u32, String>>,
) {
    let thread_id = unsafe { GetCurrentThreadId() };
    let mut message = MSG::default();
    unsafe {
        PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_NOREMOVE);
    }
    {
        let mut state = CALLBACK_STATE
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.is_some() {
            let _ = ready.send(Err("the Windows keyboard hook is already active".into()));
            return;
        }
        *state = Some(CallbackState {
            events,
            errors: errors.clone(),
            binding: bindings.dictation,
            trigger_vk: bindings.dictation_trigger_vk,
            paste_last: bindings.paste_last,
            paste_trigger_vk: bindings.paste_trigger_vk,
            modifiers: ModifierState::current(),
            trigger_down: false,
            paste_trigger_down: false,
            escape_down: false,
            double_tap_enabled: bindings.double_tap_enabled,
            capture: CaptureState::default(),
            dirty: false,
        });
    }

    let hook =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), ptr::null_mut(), 0) };
    if hook.is_null() {
        CALLBACK_STATE
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let _ = ready.send(Err(format!(
            "could not install the Windows keyboard hook: {}",
            std::io::Error::last_os_error()
        )));
        return;
    }
    if ready.send(Ok(thread_id)).is_err() {
        unsafe {
            UnhookWindowsHookEx(hook);
        }
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
    }
    unsafe {
        UnhookWindowsHookEx(hook);
    }
    CALLBACK_STATE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code != HC_ACTION as i32 {
        return unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) };
    }
    let is_down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
    let is_up = matches!(wparam as u32, WM_KEYUP | WM_SYSKEYUP);
    if !is_down && !is_up {
        return unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) };
    }
    let event = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
    let mut suppressed = false;
    {
        let mut guard = CALLBACK_STATE
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(state) = guard.as_mut() {
            let modifier_event = state.modifiers.update(event.vkCode, is_down);
            let paste_event = state.paste_trigger_vk == Some(event.vkCode)
                && (state.paste_trigger_down
                    || (is_down
                        && !state.capture.active
                        && !state.capture.locked
                        && !state.dirty
                        && state
                            .paste_last
                            .as_ref()
                            .is_some_and(|binding| state.modifiers.matches(binding))));
            if paste_event {
                suppressed = true;
                if is_down && !state.paste_trigger_down {
                    state.paste_trigger_down = true;
                    send_event(state, HotkeyAction::PasteLast, event.time);
                } else if is_up {
                    state.paste_trigger_down = false;
                }
            } else if state.trigger_vk == Some(event.vkCode) {
                if is_down {
                    if state.trigger_down {
                        suppressed = true;
                    } else if !state.dirty && state.modifiers.matches(&state.binding) {
                        state.trigger_down = true;
                        suppressed = true;
                        let was_locked = state.capture.locked;
                        if let Some(action) =
                            state.capture.press(state.double_tap_enabled, event.time)
                        {
                            if was_locked {
                                state.dirty = true;
                            }
                            send_event(state, action, event.time);
                        }
                    }
                } else if state.trigger_down {
                    state.trigger_down = false;
                    suppressed = true;
                    if let Some(action) =
                        state.capture.release(state.double_tap_enabled, event.time)
                    {
                        send_event(state, action, event.time);
                    }
                    if state.dirty && !state.modifiers.any_required_down(&state.binding) {
                        state.dirty = false;
                    }
                }
            } else if event.vkCode == u32::from(VK_ESCAPE) {
                if is_down && (state.capture.active || state.capture.locked) {
                    state.dirty = true;
                    state.escape_down = true;
                    suppressed = true;
                    if let Some(action) = state.capture.cancel() {
                        send_event(state, action, event.time);
                    }
                } else if is_up && state.escape_down {
                    state.escape_down = false;
                    suppressed = true;
                }
            } else if modifier_event {
                if state.capture.locked
                    && !state.dirty
                    && is_down
                    && state.modifiers.matches(&state.binding)
                {
                    if let Some(action) = state.capture.press(state.double_tap_enabled, event.time)
                    {
                        state.dirty = true;
                        send_event(state, action, event.time);
                    }
                } else if state.capture.active && !state.modifiers.matches(&state.binding) {
                    if let Some(action) =
                        state.capture.release(state.double_tap_enabled, event.time)
                    {
                        state.dirty = state.modifiers.any_required_down(&state.binding);
                        send_event(state, action, event.time);
                    }
                } else if state.trigger_vk.is_none()
                    && !state.capture.active
                    && !state.capture.locked
                    && !state.dirty
                    && is_down
                    && state.modifiers.matches(&state.binding)
                    && let Some(action) = state.capture.press(state.double_tap_enabled, event.time)
                {
                    send_event(state, action, event.time);
                }
                if state.dirty && !state.modifiers.any_required_down(&state.binding) {
                    state.dirty = false;
                }
            }
        }
    }
    if suppressed {
        1
    } else {
        unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) }
    }
}

fn send_event(state: &mut CallbackState, action: HotkeyAction, timestamp_ms: u32) {
    match state.events.try_send(HotkeyEvent {
        action,
        timestamp_ms,
    }) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            let _ = state
                .errors
                .send("Windows hotkey event queue overflowed".into());
        }
        Err(TrySendError::Disconnected(_)) => {}
    }
}

impl ModifierState {
    fn current() -> Self {
        Self {
            left_control: key_down(VK_LCONTROL),
            right_control: key_down(VK_RCONTROL),
            left_windows: key_down(VK_LWIN),
            right_windows: key_down(VK_RWIN),
            left_alt: key_down(VK_LMENU),
            right_alt: key_down(VK_RMENU),
            left_shift: key_down(VK_LSHIFT),
            right_shift: key_down(VK_RSHIFT),
        }
    }

    fn update(&mut self, key: u32, is_down: bool) -> bool {
        match key as u16 {
            VK_LCONTROL => self.left_control = is_down,
            VK_RCONTROL => self.right_control = is_down,
            VK_CONTROL => {
                self.left_control = is_down;
                self.right_control = false;
            }
            VK_LWIN => self.left_windows = is_down,
            VK_RWIN => self.right_windows = is_down,
            VK_LMENU => self.left_alt = is_down,
            VK_RMENU => self.right_alt = is_down,
            VK_MENU => {
                self.left_alt = is_down;
                self.right_alt = false;
            }
            VK_LSHIFT => self.left_shift = is_down,
            VK_RSHIFT => self.right_shift = is_down,
            VK_SHIFT => {
                self.left_shift = is_down;
                self.right_shift = false;
            }
            _ => return false,
        }
        true
    }

    fn matches(&self, binding: &WindowsHotkey) -> bool {
        binding.control == (self.left_control || self.right_control)
            && binding.windows == (self.left_windows || self.right_windows)
            && binding.alt == (self.left_alt || self.right_alt)
            && binding.shift == (self.left_shift || self.right_shift)
    }

    fn any_required_down(&self, binding: &WindowsHotkey) -> bool {
        (binding.control && (self.left_control || self.right_control))
            || (binding.windows && (self.left_windows || self.right_windows))
            || (binding.alt && (self.left_alt || self.right_alt))
            || (binding.shift && (self.left_shift || self.right_shift))
    }
}

fn key_down(key: u16) -> bool {
    unsafe { GetAsyncKeyState(i32::from(key)) < 0 }
}

pub(crate) fn virtual_key(key: &str) -> Result<u32> {
    let normalized = key.trim().to_ascii_lowercase();
    let virtual_key = match normalized.as_str() {
        "space" => u32::from(VK_SPACE),
        "enter" | "return" => u32::from(VK_RETURN),
        "tab" => u32::from(VK_TAB),
        "backspace" => 0x08,
        "delete" => 0x2e,
        "insert" => 0x2d,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" | "page_up" => 0x21,
        "pagedown" | "page_down" => 0x22,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        _ => {
            if let Some(number) = normalized
                .strip_prefix('f')
                .and_then(|number| number.parse::<u32>().ok())
                .filter(|number| (1..=24).contains(number))
            {
                0x70 + number - 1
            } else if normalized.len() == 1 {
                let character = normalized.as_bytes()[0];
                if character.is_ascii_alphanumeric() {
                    u32::from(character.to_ascii_uppercase())
                } else {
                    return Err(eyre!("unsupported Windows shortcut key: {key}"));
                }
            } else {
                return Err(eyre!("unsupported Windows shortcut key: {key}"));
            }
        }
    };
    Ok(virtual_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_delay_is_subtracted_from_the_audio_timeline() {
        let event = HotkeyEvent {
            action: HotkeyAction::Finish,
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
            timestamp_ms: u32::MAX - 4,
        };
        let captured = CaptureInstant::from_nanos(1_000_000_000);
        assert_eq!(event.occurred_at_with(5, captured).as_nanos(), 990_000_000);
    }

    #[test]
    fn second_tap_locks_until_the_shortcut_is_pressed_again() {
        let mut state = CaptureState::default();
        assert_eq!(state.press(true, 100), Some(HotkeyAction::Start));
        assert_eq!(state.release(true, 140), Some(HotkeyAction::Finish));
        assert_eq!(state.press(true, 250), Some(HotkeyAction::Start));
        assert_eq!(state.release(true, 290), None);
        assert!(state.locked);
        assert_eq!(state.press(true, 500), Some(HotkeyAction::Finish));
        assert!(!state.locked);
    }

    #[test]
    fn escape_cancels_a_locked_capture() {
        let mut state = CaptureState::default();
        state.press(true, 100);
        state.release(true, 120);
        state.press(true, 200);
        state.release(true, 220);
        assert_eq!(state.cancel(), Some(HotkeyAction::Cancel));
        assert!(!state.active);
        assert!(!state.locked);
    }

    #[test]
    fn disabled_double_tap_keeps_both_taps_as_hold_captures() {
        let mut state = CaptureState::default();
        assert_eq!(state.press(false, 100), Some(HotkeyAction::Start));
        assert_eq!(state.release(false, 120), Some(HotkeyAction::Finish));
        assert_eq!(state.press(false, 200), Some(HotkeyAction::Start));
        assert_eq!(state.release(false, 220), Some(HotkeyAction::Finish));
        assert!(!state.locked);
    }
}

use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use color_eyre::eyre::{Result, eyre};
use serde::{Deserialize, Serialize};
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

use crate::app_settings::{HotkeyBinding, HotkeyKey, HotkeyModifiers, ModifierSide};
use crate::audio::CaptureInstant;
use crate::dictation::ShortcutMode;

/// The focused application's executable stem (e.g. "chrome"), lowercased,
/// for per-application dictation modes and history entries.
pub fn foreground_process_stem() -> Option<String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let window = unsafe { GetForegroundWindow() };
    let path = window_process_path(window)?;
    Some(path.file_stem()?.to_string_lossy().to_lowercase())
}

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

/// A shortcut as the keyboard hook matches it: which modifiers are held and
/// the named key. Settings store it as a portable `HotkeyBinding`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct WindowsHotkey {
    pub control: bool,
    pub windows: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Option<String>,
}

impl Default for WindowsHotkey {
    fn default() -> Self {
        Self {
            control: true,
            windows: false,
            alt: false,
            shift: false,
            key: Some("space".into()),
        }
    }
}

/// Windows names Command the Windows key and Option Alt; keys go by name.
impl From<&HotkeyBinding> for WindowsHotkey {
    fn from(binding: &HotkeyBinding) -> Self {
        Self {
            control: binding.modifiers.control.is_some(),
            windows: binding.modifiers.command.is_some(),
            alt: binding.modifiers.option.is_some(),
            shift: binding.modifiers.shift.is_some(),
            key: binding.key.as_ref().map(HotkeyKey::name),
        }
    }
}

impl From<&WindowsHotkey> for HotkeyBinding {
    fn from(hotkey: &WindowsHotkey) -> Self {
        let side = |held: bool| held.then_some(ModifierSide::Either);
        Self {
            modifiers: HotkeyModifiers {
                control: side(hotkey.control),
                option: side(hotkey.alt),
                shift: side(hotkey.shift),
                command: side(hotkey.windows),
                function: false,
            },
            key: hotkey.key.as_deref().map(HotkeyKey::named),
        }
    }
}

impl WindowsHotkey {
    pub fn validate(&self) -> Result<()> {
        let modifier_count = [self.control, self.windows, self.alt, self.shift]
            .into_iter()
            .filter(|enabled| *enabled)
            .count();
        let Some(key) = self.key.as_deref() else {
            if modifier_count < 2 {
                return Err(eyre!(
                    "modifier-only Windows dictation shortcuts require at least two modifiers"
                ));
            }
            return Ok(());
        };
        if key.trim().is_empty() {
            return Err(eyre!("Windows dictation shortcut key cannot be empty"));
        }
        crate::windows_input::virtual_key(key)?;
        let function_key = key
            .strip_prefix('f')
            .or_else(|| key.strip_prefix('F'))
            .and_then(|number| number.parse::<u8>().ok())
            .is_some_and(|number| (1..=24).contains(&number));
        if !function_key && modifier_count == 0 {
            return Err(eyre!(
                "Windows dictation shortcuts require a modifier or a function key"
            ));
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        HotkeyBinding::from(self).keycaps().join("+")
    }
}

const EVENT_CAPACITY: usize = 16;
const MAX_EVENT_DELAY_MS: u32 = 60_000;
const DOUBLE_TAP_WINDOW_MS: u32 = 300;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HotkeyAction {
    Start,
    Finish,
    Cancel,
    PasteLast,
    VoiceStart,
    VoiceFinish,
    VoiceCancel,
}

/// Map a capture-state transition onto the voice-action event family.
fn voice_action_variant(action: HotkeyAction) -> HotkeyAction {
    match action {
        HotkeyAction::Start => HotkeyAction::VoiceStart,
        HotkeyAction::Finish => HotkeyAction::VoiceFinish,
        _ => HotkeyAction::VoiceCancel,
    }
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
    voice_action: Option<WindowsHotkey>,
    voice_trigger_vk: Option<u32>,
    modifiers: ModifierState,
    trigger_down: bool,
    paste_trigger_down: bool,
    voice_trigger_down: bool,
    voice_capture: CaptureState,
    escape_down: bool,
    double_tap_enabled: bool,
    double_tap_only: bool,
    /// Pass the dictation key on to other apps instead of consuming it.
    share_trigger: bool,
    capture: CaptureState,
    dirty: bool,
}

#[derive(Default)]
struct CaptureState {
    active: bool,
    locked: bool,
    second_tap: bool,
    recording: bool,
    last_release_ms: Option<u32>,
    /// Every press acts as the second tap of a double-tap: releasing it
    /// locks the capture, and the next press finishes it.
    toggle: bool,
}

impl CaptureState {
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

/// While a settings-pane chord capture runs, the hook must neither act on
/// nor suppress anything: acting would fire real dictations/pastes from
/// the chord being recorded, and suppression would hide the current
/// binding's keys from `GetAsyncKeyState`, making it impossible to
/// re-record the same chord. macOS suspends its hotkey handling the same
/// way during capture.
static CAPTURE_INHIBIT: AtomicBool = AtomicBool::new(false);

/// Turns hook processing off (true) or back on (false) for the duration
/// of a chord capture. Entering a capture also cancels any in-flight
/// dictation or voice-action hold, like the macOS capture flow.
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
        state.voice_trigger_down = false;
        if let Some(action) = state.capture.cancel() {
            send_event(state, action, now);
        }
        if let Some(action) = state.voice_capture.cancel() {
            send_event(state, voice_action_variant(action), now);
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
    thread_id: u32,
    worker: Option<JoinHandle<()>>,
}

impl WindowsHotkeyMonitor {
    pub fn start(
        binding: WindowsHotkey,
        paste_last: Option<WindowsHotkey>,
        voice_action: Option<WindowsHotkey>,
        double_tap_enabled: bool,
        double_tap_only: bool,
        shortcut_mode: ShortcutMode,
        share_trigger: bool,
    ) -> Result<Self> {
        binding.validate()?;
        let trigger_vk = binding.key.as_deref().map(virtual_key).transpose()?;
        let paste_trigger_vk = chord_virtual_key(paste_last.as_ref(), "Paste Last")?;
        let voice_trigger_vk =
            chord_virtual_key(voice_action.as_ref(), "the voice action shortcut")?;
        let label = binding.label();
        let (event_sender, events) = mpsc::sync_channel(EVENT_CAPACITY);
        let (error_sender, errors) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let state = CallbackState {
            events: event_sender,
            errors: error_sender,
            binding,
            trigger_vk,
            paste_last,
            paste_trigger_vk,
            voice_action,
            voice_trigger_vk,
            modifiers: ModifierState::default(),
            trigger_down: false,
            paste_trigger_down: false,
            voice_trigger_down: false,
            voice_capture: CaptureState::default(),
            escape_down: false,
            double_tap_enabled,
            double_tap_only,
            share_trigger,
            capture: CaptureState {
                toggle: shortcut_mode == ShortcutMode::Toggle,
                ..CaptureState::default()
            },
            dirty: false,
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
            // The hook never sees a release that lands on another desktop,
            // such as the Windows key of Win+L or the modifiers of
            // Ctrl+Alt+Delete, so tracked modifiers stayed held after
            // unlocking and Ctrl+Space no longer matched. Re-read them on
            // every event: the asynchronous key state does not include this
            // event yet, so `update` applies it on top.
            state.modifiers = ModifierState::current();
            let modifier_event = state.modifiers.update(event.vkCode, is_down);
            // During a chord capture the hook only tracks modifier state;
            // it neither triggers actions nor suppresses keys, so the
            // capture can observe every key through GetAsyncKeyState.
            if CAPTURE_INHIBIT.load(Ordering::Relaxed) {
                return unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) };
            }
            let voice_event = state.voice_trigger_vk == Some(event.vkCode)
                && (state.voice_trigger_down
                    || (is_down
                        && !state.capture.active
                        && !state.capture.locked
                        && !state.dirty
                        && state
                            .voice_action
                            .as_ref()
                            .is_some_and(|binding| state.modifiers.matches(binding))));
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
            if voice_event {
                suppressed = true;
                if is_down && !state.voice_trigger_down {
                    state.voice_trigger_down = true;
                    if let Some(action) = state.voice_capture.press(false, false, event.time) {
                        send_event(state, voice_action_variant(action), event.time);
                    }
                } else if is_up && state.voice_trigger_down {
                    state.voice_trigger_down = false;
                    if let Some(action) = state.voice_capture.release(false, event.time) {
                        send_event(state, voice_action_variant(action), event.time);
                    }
                }
            } else if paste_event {
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
                        suppressed = !state.share_trigger;
                    } else if !state.dirty
                        && !state.voice_capture.active
                        && state.modifiers.matches(&state.binding)
                    {
                        state.trigger_down = true;
                        suppressed = !state.share_trigger;
                        let was_locked = state.capture.locked;
                        if let Some(action) = state.capture.press(
                            state.double_tap_enabled,
                            state.double_tap_only,
                            event.time,
                        ) {
                            if was_locked {
                                state.dirty = true;
                            }
                            send_event(state, action, event.time);
                        }
                    }
                } else if state.trigger_down {
                    state.trigger_down = false;
                    suppressed = !state.share_trigger;
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
                if is_down
                    && (state.capture.active || state.capture.locked || state.voice_capture.active)
                {
                    state.dirty = true;
                    state.escape_down = true;
                    suppressed = true;
                    if let Some(action) = state.capture.cancel() {
                        send_event(state, action, event.time);
                    }
                    if let Some(action) = state.voice_capture.cancel() {
                        state.voice_trigger_down = false;
                        send_event(state, voice_action_variant(action), event.time);
                    }
                } else if is_up && state.escape_down {
                    state.escape_down = false;
                    suppressed = true;
                    // Escape is not a modifier, so the modifier path never
                    // sees this release; without clearing here, the next
                    // Ctrl+Space after a cancel was ignored.
                    if !state.modifiers.any_required_down(&state.binding) {
                        state.dirty = false;
                    }
                }
            } else if modifier_event {
                if state.voice_capture.active
                    && !state
                        .voice_action
                        .as_ref()
                        .is_some_and(|binding| state.modifiers.matches(binding))
                {
                    if let Some(action) = state.voice_capture.release(false, event.time) {
                        state.voice_trigger_down = false;
                        send_event(state, voice_action_variant(action), event.time);
                    }
                } else if state.trigger_vk.is_none()
                    && state.capture.locked
                    && !state.dirty
                    && is_down
                    && state.modifiers.matches(&state.binding)
                {
                    // Only a modifier-only shortcut finishes on a modifier
                    // press. A key chord such as Ctrl+Space finishes on its
                    // key, so resting a finger on Ctrl never ends a locked
                    // or toggled capture.
                    if let Some(action) = state.capture.press(
                        state.double_tap_enabled,
                        state.double_tap_only,
                        event.time,
                    ) {
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
                    && !state.voice_capture.active
                    && !state.dirty
                    && is_down
                    && state.modifiers.matches(&state.binding)
                    && let Some(action) = state.capture.press(
                        state.double_tap_enabled,
                        state.double_tap_only,
                        event.time,
                    )
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
        Self::read(key_down)
    }

    fn read(key_down: impl Fn(u16) -> bool) -> Self {
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

/// One step of the settings-pane chord capture. The UI polls this on a
/// timer instead of listening to window keystrokes because dictation
/// bindings can be modifier-only chords (for example Ctrl+Win), which
/// never produce a keystroke event.
pub(crate) enum ChordPoll {
    Pending,
    Cancelled,
    Captured(WindowsHotkey),
}

/// Records the next chord the user presses: modifiers plus a regular key
/// complete immediately; a modifier-only chord completes when every
/// accumulated modifier is released; Escape cancels.
#[derive(Default)]
pub(crate) struct ChordCapture {
    seen_control: bool,
    seen_windows: bool,
    seen_alt: bool,
    seen_shift: bool,
}

/// Non-modifier keys the capture scans, paired with the names
/// [`virtual_key`] accepts (letters, digits, and function keys are
/// scanned separately).
const CAPTURE_NAMED_KEYS: [(u16, &str); 14] = [
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
];

impl ChordCapture {
    pub fn poll(&mut self) -> ChordPoll {
        if key_down(VK_ESCAPE) {
            return ChordPoll::Cancelled;
        }
        let control = key_down(VK_CONTROL);
        let windows = key_down(VK_LWIN) || key_down(VK_RWIN);
        let alt = key_down(VK_MENU);
        let shift = key_down(VK_SHIFT);
        if let Some(key) = pressed_capture_key() {
            return ChordPoll::Captured(WindowsHotkey {
                control,
                windows,
                alt,
                shift,
                key: Some(key),
            });
        }
        if unsupported_key_down() {
            // A key the bindings cannot express (OEM punctuation, media
            // keys, ...) is being pressed: forget the chord in progress so
            // releasing the modifiers cannot record a wrong modifier-only
            // binding.
            *self = Self::default();
            return ChordPoll::Pending;
        }
        self.seen_control |= control;
        self.seen_windows |= windows;
        self.seen_alt |= alt;
        self.seen_shift |= shift;
        let seen_any = self.seen_control || self.seen_windows || self.seen_alt || self.seen_shift;
        if seen_any && !control && !windows && !alt && !shift {
            return ChordPoll::Captured(WindowsHotkey {
                control: self.seen_control,
                windows: self.seen_windows,
                alt: self.seen_alt,
                shift: self.seen_shift,
                key: None,
            });
        }
        ChordPoll::Pending
    }

    /// The modifiers a release would record so far.
    pub fn held(&self) -> WindowsHotkey {
        WindowsHotkey {
            control: self.seen_control,
            windows: self.seen_windows,
            alt: self.seen_alt,
            shift: self.seen_shift,
            key: None,
        }
    }
}

/// Whether any pressed key is one the capture cannot map to a binding
/// name: everything except mouse buttons, modifiers, lock/IME keys, and
/// the mappable set.
fn unsupported_key_down() -> bool {
    for vk in 0x08..=0xFE_u16 {
        match vk {
            // Modifiers (generic + left/right) and the Windows keys.
            0x10..=0x12 | 0xA0..=0xA5 | 0x5B | 0x5C => continue,
            // Escape is the cancel key; lock and IME state keys are inert.
            0x1B | 0x14 | 0x15..=0x1A | 0x90 | 0x91 => continue,
            _ => {}
        }
        if !key_down(vk) {
            continue;
        }
        let mappable = CAPTURE_NAMED_KEYS.iter().any(|(named, _)| *named == vk)
            || (0x41..=0x5A).contains(&vk)
            || (0x30..=0x39).contains(&vk)
            || (0x70..=0x87).contains(&vk);
        if !mappable {
            return true;
        }
    }
    false
}

fn pressed_capture_key() -> Option<String> {
    if let Some((_, name)) = CAPTURE_NAMED_KEYS.into_iter().find(|&(vk, _)| key_down(vk)) {
        return Some(name.to_string());
    }
    for vk in 0x41..=0x5A_u16 {
        // A-Z
        if key_down(vk) {
            return Some(char::from(vk as u8).to_ascii_lowercase().to_string());
        }
    }
    for vk in 0x30..=0x39_u16 {
        // 0-9
        if key_down(vk) {
            return Some(char::from(vk as u8).to_string());
        }
    }
    for vk in 0x70..=0x87_u16 {
        // F1-F24
        if key_down(vk) {
            return Some(format!("f{}", vk - 0x70 + 1));
        }
    }
    None
}

pub(crate) fn virtual_key(key: &str) -> Result<u32> {
    let normalized = key.trim().to_ascii_lowercase();
    if let Some((vk, _)) = CAPTURE_NAMED_KEYS
        .into_iter()
        .find(|&(_, name)| name == normalized)
    {
        return Ok(u32::from(vk));
    }
    let virtual_key = match normalized.as_str() {
        "return" => u32::from(VK_RETURN),
        "page_up" => 0x21,
        "page_down" => 0x22,
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
    fn default_binding_is_ctrl_space() {
        let binding = WindowsHotkey::default();
        assert_eq!(binding.label(), "Ctrl+Space");
        assert!(binding.validate().is_ok());
    }

    #[test]
    fn unmodified_letters_are_rejected() {
        let binding = WindowsHotkey {
            control: false,
            windows: false,
            alt: false,
            shift: false,
            key: Some("d".into()),
        };
        assert!(binding.validate().is_err());
    }

    #[test]
    fn one_modifier_is_not_safe_as_a_modifier_only_binding() {
        let binding = WindowsHotkey {
            control: true,
            windows: false,
            alt: false,
            shift: false,
            key: None,
        };
        assert!(binding.validate().is_err());
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
                control: true,
                windows: false,
                alt: true,
                shift: false,
                key: Some("v".into()),
            },
            WindowsHotkey {
                key: Some("pageup".into()),
                shift: true,
                windows: true,
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
        assert_eq!(state.press(true, false, 100), Some(HotkeyAction::Start));
        assert_eq!(state.release(true, 140), Some(HotkeyAction::Finish));
        assert_eq!(state.press(true, false, 250), Some(HotkeyAction::Start));
        assert_eq!(state.release(true, 290), None);
        assert!(state.locked);
        assert_eq!(state.press(true, false, 500), Some(HotkeyAction::Finish));
        assert!(!state.locked);
    }

    #[test]
    fn escape_cancels_a_locked_capture() {
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

    #[test]
    fn escape_cancels_a_toggled_capture() {
        let mut state = CaptureState {
            toggle: true,
            ..CaptureState::default()
        };
        state.press(true, false, 100);
        state.release(true, 120);
        assert_eq!(state.cancel(), Some(HotkeyAction::Cancel));
        assert!(!state.active && !state.locked);
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

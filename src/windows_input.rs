use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use color_eyre::eyre::{Result, eyre};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, SendInput, VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU,
    VK_RCONTROL, VK_RETURN, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT, VK_SPACE, VK_TAB,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE,
    PeekMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
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
                return Err(eyre!("Windows dictation shortcuts require a key or a modifier"));
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
        if !function_key && HotkeyKey::mouse_number(key).is_none() && modifier_count == 0 {
            return Err(eyre!(
                "Windows dictation shortcuts require a modifier or a function key"
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
        self.key.is_none() && (self.alt.is_some() || self.windows.is_some())
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
    side_down: SideDownTimes,
    trigger_down: bool,
    paste_trigger_down: bool,
    escape_down: bool,
    double_tap_enabled: bool,
    double_tap_only: bool,
    /// Pass the dictation key on to other apps instead of consuming it.
    share_trigger: bool,
    capture: CaptureState,
    dirty: bool,
    /// A modifier-only shortcut with Alt or Windows was pressed with no
    /// other key since, so its release would open the menu bar or Start.
    mask_release: bool,
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

    /// Another key joined a held modifier-only shortcut: it was a chord
    /// meant for another application. Only a recording capture reports
    /// it, since a bare cancel also cancels the newest unfinished job.
    fn interrupt(&mut self) -> Option<HotkeyAction> {
        if !self.active {
            return None;
        }
        let recording = self.recording;
        self.active = false;
        self.second_tap = false;
        self.recording = false;
        self.last_release_ms = None;
        recording.then_some(HotkeyAction::Cancel)
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
    thread_id: u32,
    worker: Option<JoinHandle<()>>,
}

impl WindowsHotkeyMonitor {
    pub fn start(
        binding: WindowsHotkey,
        paste_last: Option<WindowsHotkey>,
        double_tap_enabled: bool,
        double_tap_only: bool,
        shortcut_mode: ShortcutMode,
        share_trigger: bool,
    ) -> Result<Self> {
        binding.validate()?;
        let trigger_vk = binding.key.as_deref().map(virtual_key).transpose()?;
        let paste_trigger_vk = chord_virtual_key(paste_last.as_ref(), "Paste Last")?;
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
            modifiers: ModifierState::default(),
            side_down: SideDownTimes::default(),
            trigger_down: false,
            paste_trigger_down: false,
            escape_down: false,
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
    let mouse = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), ptr::null_mut(), 0) };
    if mouse.is_null() {
        let error = std::io::Error::last_os_error();
        unsafe {
            UnhookWindowsHookEx(hook);
        }
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
        unsafe {
            UnhookWindowsHookEx(hook);
            UnhookWindowsHookEx(mouse);
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
        UnhookWindowsHookEx(mouse);
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
    if event.dwExtraInfo == HEX_INJECTED {
        return unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) };
    }
    match process_button(event.vkCode, is_down, event.time) {
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
    if code == HC_ACTION as i32 {
        // SAFETY: Windows supplies a live MSLLHOOKSTRUCT for this callback.
        let event = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        if let Some((vk, down)) = mouse_message(wparam as u32, event.mouseData)
            && process_button(vk, down, event.time) != Disposition::Pass
        {
            return 1;
        }
    }
    unsafe { CallNextHookEx(ptr::null_mut(), code, wparam, lparam) }
}

// Both low-level hooks feed exactly the same capture and suppression rules.
fn process_button(vk: u32, is_down: bool, time: u32) -> Disposition {
    let is_up = !is_down;
    let mut suppressed = false;
    let mut mask = false;
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
            let modifier_event = state.modifiers.update(vk, is_down);
            state.side_down.update(vk, is_down, time);
            // During a chord capture the hook only tracks modifier state;
            // it neither triggers actions nor suppresses keys, so the
            // capture can observe every key through GetAsyncKeyState.
            if CAPTURE_INHIBIT.load(Ordering::Relaxed) {
                state.mask_release = false;
                return Disposition::Pass;
            }
            if !modifier_event && is_down {
                state.mask_release = false;
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
                            .is_some_and(|binding| state.modifiers.matches(binding))));
            if paste_event {
                suppressed = true;
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
                    } else if !state.dirty && state.modifiers.matches(&state.binding) {
                        state.trigger_down = true;
                        suppressed = !state.share_trigger;
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
                } else if state.trigger_down {
                    state.trigger_down = false;
                    suppressed = !state.share_trigger;
                    if let Some(action) = state.capture.release(state.double_tap_enabled, time) {
                        send_event(state, action, time);
                    }
                    if state.dirty && !state.modifiers.any_required_down(&state.binding) {
                        state.dirty = false;
                    }
                }
            } else if vk == u32::from(VK_ESCAPE) {
                if is_down && (state.capture.active || state.capture.locked) {
                    state.dirty = true;
                    state.escape_down = true;
                    suppressed = true;
                    if let Some(action) = state.capture.cancel() {
                        send_event(state, action, time);
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
            } else if !modifier_event {
                // A key or button joining a held modifier-only shortcut makes
                // a chord meant for another application, such as Alt+Tab.
                if is_down && state.trigger_vk.is_none() && state.capture.active {
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
                    && !state.dirty
                    && is_down
                    && state.modifiers.matches(&state.binding)
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
                } else if state.capture.active && !state.modifiers.matches(&state.binding) {
                    if let Some(action) = state.capture.release(state.double_tap_enabled, time) {
                        state.dirty = state.modifiers.any_required_down(&state.binding);
                        send_event(state, action, time);
                    }
                } else if state.trigger_vk.is_none()
                    && !state.capture.active
                    && !state.capture.locked
                    && !state.dirty
                    && is_down
                    && state.modifiers.matches(&state.binding)
                    && state.side_down.together(&state.binding)
                    && let Some(action) =
                        state
                            .capture
                            .press(state.double_tap_enabled, state.double_tap_only, time)
                {
                    send_event(state, action, time);
                }
                if is_down
                    && state.binding.masks_release()
                    && state.modifiers.matches(&state.binding)
                {
                    state.mask_release = true;
                }
                if state.dirty && !state.modifiers.any_required_down(&state.binding) {
                    state.dirty = false;
                }
            }
        }
    }
    if mask {
        Disposition::Mask
    } else if suppressed {
        Disposition::Suppress
    } else {
        Disposition::Pass
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

    /// Exactly the chord's modifier keys are down: a side alone, both sides
    /// only for `Both`, and either side (or both) for `Either`.
    fn matches(&self, binding: &WindowsHotkey) -> bool {
        side_matches(binding.control, self.left_control, self.right_control)
            && side_matches(binding.windows, self.left_windows, self.right_windows)
            && side_matches(binding.alt, self.left_alt, self.right_alt)
            && side_matches(binding.shift, self.left_shift, self.right_shift)
    }

    fn any_required_down(&self, binding: &WindowsHotkey) -> bool {
        (binding.control.is_some() && (self.left_control || self.right_control))
            || (binding.windows.is_some() && (self.left_windows || self.right_windows))
            || (binding.alt.is_some() && (self.left_alt || self.right_alt))
            || (binding.shift.is_some() && (self.left_shift || self.right_shift))
    }
}

fn side_matches(side: Option<ModifierSide>, left: bool, right: bool) -> bool {
    match side {
        None => !left && !right,
        Some(ModifierSide::Either) => left || right,
        Some(ModifierSide::Left) => left && !right,
        Some(ModifierSide::Right) => right && !left,
        Some(ModifierSide::Both) => left && right,
    }
}

/// The sided modifier keys, left then right for each modifier.
const SIDE_KEYS: [u16; 8] = [
    VK_LCONTROL,
    VK_RCONTROL,
    VK_LWIN,
    VK_RWIN,
    VK_LMENU,
    VK_RMENU,
    VK_LSHIFT,
    VK_RSHIFT,
];

/// When each sided modifier key went down, in hook time (ms), for chords
/// that hold both sides of one modifier.
#[derive(Default)]
struct SideDownTimes([Option<u32>; 8]);

impl SideDownTimes {
    fn update(&mut self, vk: u32, is_down: bool, time: u32) {
        // A side-less modifier code reads as the left key, as `ModifierState`.
        let vk = match vk as u16 {
            VK_CONTROL => VK_LCONTROL,
            VK_MENU => VK_LMENU,
            VK_SHIFT => VK_LSHIFT,
            vk => vk,
        };
        if let Some(index) = SIDE_KEYS.iter().position(|&key| key == vk) {
            if is_down {
                // Auto-repeat keeps the first press.
                self.0[index].get_or_insert(time);
            } else {
                self.0[index] = None;
            }
        }
    }

    /// Both keys of each `Both` modifier went down within
    /// `BOTH_SIDES_WINDOW`, in either order; true for every other chord.
    fn together(&self, binding: &WindowsHotkey) -> bool {
        let window = crate::dictation::BOTH_SIDES_WINDOW.as_millis() as u32;
        [binding.control, binding.windows, binding.alt, binding.shift]
            .into_iter()
            .enumerate()
            .filter(|(_, side)| *side == Some(ModifierSide::Both))
            .all(
                |(index, _)| match (self.0[2 * index], self.0[2 * index + 1]) {
                    (Some(left), Some(right)) => {
                        left.wrapping_sub(right).min(right.wrapping_sub(left)) <= window
                    }
                    _ => false,
                },
            )
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
/// accumulated modifier key is released, keeping the sides
/// `HotkeyModifiers::recorded_sides` keeps (both Alts stay both); Escape
/// cancels.
#[derive(Default)]
pub(crate) struct ChordCapture {
    mouse_armed: bool,
    /// Every sided modifier key seen down since the chord began.
    seen: ModifierState,
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
    fn arm_mouse(&mut self, held: bool) -> bool {
        self.mouse_armed |= !held;
        self.mouse_armed
    }

    pub fn poll(&mut self) -> ChordPoll {
        if key_down(VK_ESCAPE) {
            return ChordPoll::Cancelled;
        }
        if !self.mouse_armed {
            // Do not bind the click that opened the recorder.
            if !self.arm_mouse(MOUSE_KEYS.iter().any(|&(vk, _)| key_down(vk))) {
                return ChordPoll::Pending;
            }
        }
        let held = ModifierState::current();
        if let Some(key) = pressed_capture_key() {
            let any = |down: bool| down.then_some(ModifierSide::Either);
            return ChordPoll::Captured(WindowsHotkey {
                control: any(held.left_control || held.right_control),
                windows: any(held.left_windows || held.right_windows),
                alt: any(held.left_alt || held.right_alt),
                shift: any(held.left_shift || held.right_shift),
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
        self.seen.add(&held);
        if self.seen.any() && !held.any() {
            return ChordPoll::Captured(self.held());
        }
        ChordPoll::Pending
    }

    /// The modifier keys a release would record so far.
    pub fn held(&self) -> WindowsHotkey {
        let seen = &self.seen;
        let side = |left: bool, right: bool| match (left, right) {
            (true, true) => Some(ModifierSide::Both),
            (true, false) => Some(ModifierSide::Left),
            (false, true) => Some(ModifierSide::Right),
            (false, false) => None,
        };
        let modifiers = HotkeyModifiers {
            control: side(seen.left_control, seen.right_control),
            option: side(seen.left_alt, seen.right_alt),
            shift: side(seen.left_shift, seen.right_shift),
            command: side(seen.left_windows, seen.right_windows),
            function: false,
        }
        .recorded_sides();
        WindowsHotkey {
            control: modifiers.control,
            windows: modifiers.command,
            alt: modifiers.option,
            shift: modifiers.shift,
            key: None,
        }
    }
}

impl ModifierState {
    fn add(&mut self, other: &Self) {
        self.left_control |= other.left_control;
        self.right_control |= other.right_control;
        self.left_windows |= other.left_windows;
        self.right_windows |= other.right_windows;
        self.left_alt |= other.left_alt;
        self.right_alt |= other.right_alt;
        self.left_shift |= other.left_shift;
        self.right_shift |= other.right_shift;
    }

    fn any(&self) -> bool {
        self.left_control
            || self.right_control
            || self.left_windows
            || self.right_windows
            || self.left_alt
            || self.right_alt
            || self.left_shift
            || self.right_shift
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

const MOUSE_KEYS: [(u16, &str); 5] = [
    (0x01, "mouse1"),
    (0x02, "mouse2"),
    (0x04, "mouse3"),
    (0x05, "mouse4"),
    (0x06, "mouse5"),
];

fn pressed_capture_key() -> Option<String> {
    if let Some((_, name)) = MOUSE_KEYS.into_iter().find(|&(vk, _)| key_down(vk)) {
        return Some(name.into());
    }
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
        assert!(right_alt.validate().is_ok(), "one key alone is a shortcut too");
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
        assert!(!alt_space.masks_release(), "its key already masks it");

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
            "AltGr's added Left Ctrl is not Right Alt alone"
        );
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

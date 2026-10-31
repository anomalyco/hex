use std::mem::size_of;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr, eyre};
use windows_sys::Win32::System::DataExchange::{CountClipboardFormats, GetClipboardSequenceNumber};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MapVirtualKeyExW, SendInput, VK_CONTROL, VK_LWIN, VK_MENU,
    VK_RWIN, VK_SHIFT, VK_V,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

const CLIPBOARD_RESTORE_DELAY: Duration = Duration::from_millis(500);

enum ClipboardSnapshot {
    Empty,
    Files(Vec<PathBuf>),
    Html { html: String, text: Option<String> },
    Image(arboard::ImageData<'static>),
    Text(String),
}

impl ClipboardSnapshot {
    fn read(clipboard: &mut arboard::Clipboard) -> Result<Self> {
        if let Ok(html) = clipboard.get().html() {
            return Ok(Self::Html {
                html,
                text: clipboard.get_text().ok(),
            });
        }
        if let Ok(text) = clipboard.get_text() {
            return Ok(Self::Text(text));
        }
        if let Ok(files) = clipboard.get().file_list()
            && !files.is_empty()
        {
            return Ok(Self::Files(files));
        }
        if let Ok(image) = clipboard.get().image() {
            return Ok(Self::Image(image));
        }
        if unsafe { CountClipboardFormats() } == 0 {
            return Ok(Self::Empty);
        }
        Err(eyre!(
            "the current clipboard format cannot be preserved safely"
        ))
    }

    fn restore(self, clipboard: &mut arboard::Clipboard) -> Result<()> {
        match self {
            Self::Empty => clipboard.clear()?,
            Self::Files(files) => clipboard.set().file_list(&files)?,
            Self::Html { html, text } => clipboard.set().html(html, text)?,
            Self::Image(image) => clipboard.set_image(image)?,
            Self::Text(text) => clipboard.set_text(text)?,
        }
        Ok(())
    }
}

pub struct WindowsPaster {
    clipboard: arboard::Clipboard,
    clipboard_restore: Arc<Mutex<ClipboardRestore>>,
    /// Terminal paste: send Ctrl+Shift+V instead of Ctrl+V. Shared so a
    /// settings change applies to the next paste without a new paster.
    paste_with_shift: Arc<AtomicBool>,
}

#[derive(Default)]
struct ClipboardRestore {
    generation: u64,
    original: Option<ClipboardSnapshot>,
    last_sequence: Option<u32>,
}

impl ClipboardRestore {
    fn register(
        &mut self,
        previous: ClipboardSnapshot,
        previous_sequence: u32,
        inserted_sequence: u32,
    ) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        if self.last_sequence != Some(previous_sequence) {
            self.original = Some(previous);
        }
        self.last_sequence = Some(inserted_sequence);
        self.generation
    }
}

impl WindowsPaster {
    pub fn new(paste_with_shift: Arc<AtomicBool>) -> Result<Self> {
        Ok(Self {
            clipboard: arboard::Clipboard::new()
                .wrap_err("could not open the Windows clipboard")?,
            clipboard_restore: Arc::new(Mutex::new(ClipboardRestore::default())),
            paste_with_shift,
        })
    }

    pub fn paste(&mut self, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Err(eyre!("refusing to paste an empty transcript"));
        }
        let mut restore = self
            .clipboard_restore
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let original_sequence = clipboard_sequence();
        let snapshot = ClipboardSnapshot::read(&mut self.clipboard)?;
        wait_for_conflicting_modifiers();
        if clipboard_sequence() != original_sequence {
            return Err(eyre!(
                "the clipboard changed while the transcript was being prepared"
            ));
        }
        self.clipboard
            .set_text(text.to_string())
            .wrap_err("could not place the transcript on the Windows clipboard")?;
        let transcript_sequence = clipboard_sequence();
        let generation = restore.register(snapshot, original_sequence, transcript_sequence);
        drop(restore);

        if let Err(error) = send_paste_shortcut(self.paste_with_shift.load(Ordering::Relaxed)) {
            restore_clipboard_generation(&self.clipboard_restore, generation);
            return Err(error);
        }
        let clipboard_restore = self.clipboard_restore.clone();
        thread::spawn(move || {
            thread::sleep(CLIPBOARD_RESTORE_DELAY);
            restore_clipboard_generation(&clipboard_restore, generation);
        });
        Ok(())
    }
}

fn clipboard_sequence() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

fn restore_clipboard_generation(clipboard_restore: &Arc<Mutex<ClipboardRestore>>, generation: u64) {
    let mut restore = clipboard_restore
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if restore.generation != generation {
        return;
    }
    if restore.last_sequence != Some(clipboard_sequence()) {
        restore.original = None;
        restore.last_sequence = None;
        return;
    }
    let Some(snapshot) = restore.original.take() else {
        restore.last_sequence = None;
        return;
    };
    restore.last_sequence = None;
    let result = arboard::Clipboard::new()
        .wrap_err("could not reopen the Windows clipboard")
        .and_then(|mut clipboard| snapshot.restore(&mut clipboard));
    if let Err(error) = result {
        tracing::warn!(%error, "could not restore the previous Windows clipboard");
    }
}

/// Waits, without a deadline, until Shift, Alt, and Win are up: held, they
/// would turn the synthesized Ctrl+V (or Ctrl+Shift+V, whose Shift Hex
/// presses and releases itself) into a different shortcut. A held Ctrl
/// is harmless, so the user may keep it down after finishing with a chord.
fn wait_for_conflicting_modifiers() {
    while [VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
        .into_iter()
        .any(key_down)
    {
        thread::sleep(Duration::from_millis(10));
    }
}

/// Ctrl plus `key` (Ctrl+Shift plus `key` with `shift`), leaving a Ctrl the
/// user physically holds (`control_held`) untouched: synthesizing its
/// release would leave Windows thinking it is up while the user holds it.
/// Shift goes down after Ctrl and comes up before it. Each key carries its
/// scan code (`scan`), as a physical press does.
fn control_chord(
    key: u16,
    shift: bool,
    control_held: bool,
    scan: impl Fn(u16) -> u16,
) -> Vec<INPUT> {
    let input = |key, flags| keyboard_input(key, scan(key), flags);
    let mut inputs = Vec::with_capacity(6);
    if !control_held {
        inputs.push(input(VK_CONTROL, 0));
    }
    if shift {
        inputs.push(input(VK_SHIFT, 0));
    }
    inputs.push(input(key, 0));
    inputs.push(input(key, KEYEVENTF_KEYUP));
    if shift {
        inputs.push(input(VK_SHIFT, KEYEVENTF_KEYUP));
    }
    if !control_held {
        inputs.push(input(VK_CONTROL, KEYEVENTF_KEYUP));
    }
    inputs
}

/// The scan code of `key` in the foreground window's keyboard layout.
/// Chromium apps derive `KeyboardEvent.code` from it, and VS Code matches
/// its keybindings by that code: without one, its terminal never saw Ctrl+V
/// as Paste and sent ^V to the program inside it (#618).
fn foreground_scan_code(key: u16) -> u16 {
    // SAFETY: plain Win32 queries; a null process-ID pointer is allowed.
    unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
        MapVirtualKeyExW(u32::from(key), MAPVK_VK_TO_VSC, GetKeyboardLayout(thread)) as u16
    }
}

fn key_down(key: u16) -> bool {
    unsafe { GetAsyncKeyState(i32::from(key)) < 0 }
}

fn send_paste_shortcut(shift: bool) -> Result<()> {
    let inputs = control_chord(VK_V, shift, key_down(VK_CONTROL), foreground_scan_code);
    let sent = send_inputs(&inputs);
    if sent != inputs.len() as u32 {
        // Release only what this chord pressed; never a key the user holds.
        let cleanup = inputs
            .iter()
            .filter(|input| unsafe { input.Anonymous.ki.dwFlags } & KEYEVENTF_KEYUP != 0)
            .copied()
            .collect::<Vec<_>>();
        send_inputs(&cleanup);
        return Err(eyre!(
            "Windows accepted only {sent} of {} synthetic paste events; an elevated target window may reject input",
            inputs.len()
        ));
    }
    Ok(())
}

fn send_inputs(inputs: &[INPUT]) -> u32 {
    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    }
}

fn keyboard_input(key: u16, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: crate::windows::input::HEX_INJECTED,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rapid_pastes_keep_the_original_clipboard_for_the_latest_restore() {
        let mut restore = ClipboardRestore::default();

        let first = restore.register(ClipboardSnapshot::Text("original".into()), 10, 11);
        let second = restore.register(ClipboardSnapshot::Text("first paste".into()), 11, 12);

        assert_ne!(first, second);
        assert_eq!(restore.last_sequence, Some(12));
        assert!(matches!(
            restore.original.as_ref(),
            Some(ClipboardSnapshot::Text(text)) if text == "original"
        ));
    }

    /// US-layout scan codes for the chord's keys.
    fn scan(key: u16) -> u16 {
        match key {
            VK_CONTROL => 0x1D,
            VK_SHIFT => 0x2A,
            VK_V => 0x2F,
            _ => 0,
        }
    }

    #[test]
    fn paste_chords_carry_each_keys_scan_code() {
        let inputs = control_chord(VK_V, true, false, scan);
        assert!(inputs.iter().all(|input| {
            let ki = unsafe { input.Anonymous.ki };
            ki.wScan == scan(ki.wVk) && ki.wScan != 0
        }));
    }

    fn keys(inputs: &[INPUT]) -> Vec<(u16, bool)> {
        inputs
            .iter()
            .map(|input| {
                let ki = unsafe { input.Anonymous.ki };
                (ki.wVk, ki.dwFlags & KEYEVENTF_KEYUP != 0)
            })
            .collect()
    }

    #[test]
    fn paste_chords_press_shift_inside_control_and_leave_a_held_control_alone() {
        assert_eq!(
            keys(&control_chord(VK_V, false, false, scan)),
            [
                (VK_CONTROL, false),
                (VK_V, false),
                (VK_V, true),
                (VK_CONTROL, true)
            ]
        );
        assert_eq!(
            keys(&control_chord(VK_V, false, true, scan)),
            [(VK_V, false), (VK_V, true)]
        );
        assert_eq!(
            keys(&control_chord(VK_V, true, false, scan)),
            [
                (VK_CONTROL, false),
                (VK_SHIFT, false),
                (VK_V, false),
                (VK_V, true),
                (VK_SHIFT, true),
                (VK_CONTROL, true)
            ]
        );
        assert_eq!(
            keys(&control_chord(VK_V, true, true, scan)),
            [
                (VK_SHIFT, false),
                (VK_V, false),
                (VK_V, true),
                (VK_SHIFT, true)
            ]
        );
    }
}

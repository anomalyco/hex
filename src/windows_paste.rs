use std::mem::size_of;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use windows_sys::Win32::System::DataExchange::{CountClipboardFormats, GetClipboardSequenceNumber};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_V,
};

const MODIFIER_RELEASE_TIMEOUT: Duration = Duration::from_millis(500);
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
    pub fn new() -> Result<Self> {
        Ok(Self {
            clipboard: arboard::Clipboard::new()
                .wrap_err("could not open the Windows clipboard")?,
            clipboard_restore: Arc::new(Mutex::new(ClipboardRestore::default())),
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
        wait_for_modifier_release()?;
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

        if let Err(error) = send_paste_shortcut() {
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

fn wait_for_modifier_release() -> Result<()> {
    let deadline = Instant::now() + MODIFIER_RELEASE_TIMEOUT;
    while Instant::now() < deadline {
        if ![VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
            .into_iter()
            .any(key_down)
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(eyre!(
        "release keyboard modifiers before HEX inserts the transcript"
    ))
}

fn key_down(key: u16) -> bool {
    unsafe { GetAsyncKeyState(i32::from(key)) < 0 }
}

fn send_paste_shortcut() -> Result<()> {
    let inputs = [
        keyboard_input(VK_CONTROL, 0),
        keyboard_input(VK_V, 0),
        keyboard_input(VK_V, KEYEVENTF_KEYUP),
        keyboard_input(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if sent != inputs.len() as u32 {
        let cleanup = [
            keyboard_input(VK_V, KEYEVENTF_KEYUP),
            keyboard_input(VK_CONTROL, KEYEVENTF_KEYUP),
        ];
        unsafe {
            SendInput(
                cleanup.len() as u32,
                cleanup.as_ptr(),
                size_of::<INPUT>() as i32,
            );
        }
        return Err(eyre!(
            "Windows accepted only {sent} of {} synthetic paste events; an elevated target window may reject input",
            inputs.len()
        ));
    }
    Ok(())
}

fn keyboard_input(key: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
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
}

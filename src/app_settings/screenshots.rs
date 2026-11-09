//! Labs › Screenshots' settings (macOS): its shortcut and folder.

use super::*;

/// Where Labs › Screenshots saves, and the shortcut that starts one. The
/// Lab's switch is `labs.screenshots`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ScreenshotSettings {
    /// Starts a screenshot: Command-Shift-4 by default.
    pub hotkey: HotkeyBinding,
    /// Starts a recording of an area or a window; unbound by default.
    pub record_hotkey: Option<HotkeyBinding>,
    /// Where screenshots are saved; `None` is the Desktop.
    pub folder: Option<PathBuf>,
}

impl Default for ScreenshotSettings {
    fn default() -> Self {
        Self {
            hotkey: HotkeyBinding::screenshot_default(),
            record_hotkey: None,
            folder: None,
        }
    }
}

impl ScreenshotSettings {
    /// A left click can't be the record shortcut: it would take every click.
    pub(super) fn clear_left_click_record(&mut self) {
        if self
            .record_hotkey
            .as_ref()
            .is_some_and(HotkeyBinding::is_bare_primary_click)
        {
            tracing::warn!("cleared a left-click screenshot record shortcut");
            self.record_hotkey = None;
        }
    }

    /// The folder screenshots go to.
    pub fn folder(&self) -> PathBuf {
        self.folder
            .clone()
            .unwrap_or_else(crate::app_paths::desktop_dir)
    }
}

impl AppSettings {
    /// The screenshot shortcut while it takes effect: macOS, the Lab on,
    /// and sharing no key with another active shortcut.
    pub fn active_screenshot_hotkey(&self) -> Option<&HotkeyBinding> {
        (cfg!(target_os = "macos") && self.labs.screenshots)
            .then_some(&self.screenshots.hotkey)
            .filter(|binding| !self.hotkey_in_use(HotkeySlot::Screenshot, binding))
    }

    /// The record shortcut while it takes effect: the screenshot shortcut's
    /// conditions, Annotate on (its recorder makes the movie), and bound.
    pub fn active_screenshot_record_hotkey(&self) -> Option<&HotkeyBinding> {
        if !self.screenshot_recording_available() {
            return None;
        }
        self.screenshots
            .record_hotkey
            .as_ref()
            .filter(|binding| !self.hotkey_in_use(HotkeySlot::ScreenshotRecord, binding))
    }

    /// Whether Labs › Screenshots can record: macOS, the Lab on, and
    /// Annotate on, since a recording is an Annotate capture.
    pub fn screenshot_recording_available(&self) -> bool {
        #[cfg(hex_capture)]
        let capture = self.capture_on();
        #[cfg(not(hex_capture))]
        let capture = false;
        cfg!(target_os = "macos") && self.labs.screenshots && capture
    }
}

/// The saved screenshot settings.
pub fn screenshot_settings() -> ScreenshotSettings {
    SCREENSHOT_SETTINGS
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// The screenshot shortcut the session tap takes, while the Lab is on.
pub fn screenshot_hotkey() -> Option<RuntimeHotkey> {
    *SCREENSHOT_HOTKEY
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
}

/// The record shortcut the session tap takes, while it takes effect.
pub fn screenshot_record_hotkey() -> Option<RuntimeHotkey> {
    *SCREENSHOT_RECORD_HOTKEY
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
}

pub(super) fn apply_screenshots(settings: &AppSettings) {
    *SCREENSHOT_RECORD_HOTKEY
        .get_or_init(Default::default)
        .write()
        .unwrap_or_else(|error| error.into_inner()) = settings
        .active_screenshot_record_hotkey()
        .map(HotkeyBinding::runtime);
    *SCREENSHOT_SETTINGS
        .get_or_init(Default::default)
        .write()
        .unwrap_or_else(|error| error.into_inner()) = settings.screenshots.clone();
    *SCREENSHOT_HOTKEY
        .get_or_init(Default::default)
        .write()
        .unwrap_or_else(|error| error.into_inner()) = settings
        .active_screenshot_hotkey()
        .map(HotkeyBinding::runtime);
}

static SCREENSHOT_SETTINGS: OnceLock<RwLock<ScreenshotSettings>> = OnceLock::new();
static SCREENSHOT_HOTKEY: OnceLock<RwLock<Option<RuntimeHotkey>>> = OnceLock::new();
static SCREENSHOT_RECORD_HOTKEY: OnceLock<RwLock<Option<RuntimeHotkey>>> = OnceLock::new();

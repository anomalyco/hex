//! Labs › Screenshots' settings (macOS): its shortcut and folder.

use super::*;

/// Where Labs › Screenshots saves, and the shortcut that starts one. The
/// Lab's switch is `labs.screenshots`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ScreenshotSettings {
    /// Starts a screenshot: Command-Shift-4 by default.
    pub hotkey: HotkeyBinding,
    /// Where screenshots are saved; `None` is the Desktop.
    pub folder: Option<PathBuf>,
}

impl Default for ScreenshotSettings {
    fn default() -> Self {
        Self {
            hotkey: HotkeyBinding::screenshot_default(),
            folder: None,
        }
    }
}

impl ScreenshotSettings {
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

pub(super) fn apply_screenshots(settings: &AppSettings) {
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

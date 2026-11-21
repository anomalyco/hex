//! Labs › Screenshots' settings (macOS): its shortcut and folder, and how
//! a recording is encoded.

use super::*;

/// Where Labs › Screenshots saves, and the shortcut that starts one. The
/// Lab's switch is `labs.screenshots`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ScreenshotSettings {
    /// Starts a screenshot: Command-Shift-4 by default.
    pub hotkey: HotkeyBinding,
    /// Opens the recording toolbar: Command-Shift-5 by default.
    pub record_hotkey: Option<HotkeyBinding>,
    /// Where screenshots are saved; `None` is the Desktop.
    pub folder: Option<PathBuf>,
    /// A recording keeps one pixel a point, so a Retina display records at
    /// its size in points: a quarter of the pixels.
    pub scale_to_1x: bool,
    /// The largest a recording is.
    pub max_resolution: MaxResolution,
    /// A recording's frames a second: 30 or 60.
    pub frame_rate: u32,
    pub quality: RecordingQuality,
    /// The size the editor last exported a recording at.
    pub export_size: ExportSize,
    /// What the recording toolbar records, as last chosen.
    pub record_kind: RecordKind,
    /// The microphone a recording keeps: `none`, `default` (the one
    /// dictation uses), or an input device's name.
    pub record_microphone: String,
    /// A recording keeps what the computer plays.
    pub record_system_audio: bool,
}

impl Default for ScreenshotSettings {
    fn default() -> Self {
        Self {
            hotkey: HotkeyBinding::screenshot_default(),
            record_hotkey: Some(HotkeyBinding::screenshot_record_default()),
            folder: None,
            scale_to_1x: true,
            max_resolution: MaxResolution::Original,
            frame_rate: 30,
            quality: RecordingQuality::High,
            export_size: ExportSize::Original,
            record_kind: RecordKind::Area,
            record_microphone: RECORD_MICROPHONE_DEFAULT.into(),
            record_system_audio: false,
        }
    }
}

/// `record_microphone`'s value for no microphone.
pub const RECORD_MICROPHONE_NONE: &str = "none";
/// `record_microphone`'s value for the microphone dictation uses.
pub const RECORD_MICROPHONE_DEFAULT: &str = "default";

/// What a recording takes: an area dragged out, a window, or a whole
/// display.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    #[default]
    Area,
    Window,
    Screen,
}

impl RecordKind {
    /// Every choice, in the order the toolbar shows them.
    pub const ALL: [Self; 3] = [Self::Area, Self::Window, Self::Screen];

    /// The English label the toolbar shows.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Area => "Area",
            Self::Window => "Window",
            Self::Screen => "Full screen",
        }
    }

    /// The `hex settings` value.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Area => "area",
            Self::Window => "window",
            Self::Screen => "screen",
        }
    }
}

/// The largest a Screenshots recording is, by its long edge.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum MaxResolution {
    #[default]
    #[serde(rename = "original")]
    Original,
    #[serde(rename = "1440p")]
    P1440,
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "720p")]
    P720,
}

impl MaxResolution {
    /// Every choice, in the order the window shows them.
    pub const ALL: [Self; 4] = [Self::Original, Self::P1440, Self::P1080, Self::P720];

    /// The English label the window shows.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::P1440 => "1440p",
            Self::P1080 => "1080p",
            Self::P720 => "720p",
        }
    }

    /// The `hex settings` value.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::P1440 => "1440p",
            Self::P1080 => "1080p",
            Self::P720 => "720p",
        }
    }

    /// The longest side in pixels, of a 16:9 picture that size; `None`
    /// keeps the source's.
    pub const fn long_edge(self) -> Option<u32> {
        match self {
            Self::Original => None,
            Self::P1440 => Some(2560),
            Self::P1080 => Some(1920),
            Self::P720 => Some(1280),
        }
    }
}

/// How much detail a Screenshots recording keeps, as HEVC's constant
/// quality. Measured on a 46 s recording with nearly every pixel moving
/// (1920×1016, 30 fps), a worst case: High kept small changing digits
/// crisp (93 MB, 16 Mbit/s), Medium ghosted them while they changed, as a
/// 6 Mbit/s H.264 export does (35 MB, 6.1 Mbit/s), and Low smeared them
/// (21 MB, 3.7 Mbit/s). A mostly still screen takes far less.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingQuality {
    #[default]
    High,
    Medium,
    Low,
}

impl RecordingQuality {
    /// Every choice, in the order the window shows them.
    pub const ALL: [Self; 3] = [Self::High, Self::Medium, Self::Low];

    /// The English label the window shows.
    pub const fn name(self) -> &'static str {
        match self {
            Self::High => "High",
            Self::Medium => "Medium",
            Self::Low => "Low",
        }
    }

    /// HEVC's constant quality, in hundredths.
    pub const fn hundredths(self) -> u8 {
        match self {
            Self::High => 50,
            Self::Medium => 30,
            Self::Low => 20,
        }
    }

    /// Bits a pixel a second at 30 fps, measured on the recording above:
    /// close to the most a screen recording takes at this quality.
    pub const fn bits_per_pixel_second(self) -> f64 {
        match self {
            Self::High => 8.22,
            Self::Medium => 3.12,
            Self::Low => 1.90,
        }
    }
}

/// The size the editor exports a recording at: as recorded, or shrunk.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ExportSize {
    #[default]
    #[serde(rename = "original")]
    Original,
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "720p")]
    P720,
}

impl ExportSize {
    /// Every choice, in the order the editor shows them.
    pub const ALL: [Self; 3] = [Self::Original, Self::P1080, Self::P720];

    /// The English label the editor shows.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::P1080 => "1080p",
            Self::P720 => "720p",
        }
    }

    /// The `hex settings` value.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::P1080 => "1080p",
            Self::P720 => "720p",
        }
    }

    /// The longest side in pixels; `None` copies the recording as it is.
    pub const fn long_edge(self) -> Option<u32> {
        match self {
            Self::Original => None,
            Self::P1080 => Some(1920),
            Self::P720 => Some(1280),
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

    /// A recording's frames a second: 60 when asked for more than 45,
    /// else 30.
    pub fn fps(&self) -> u32 {
        if self.frame_rate > 45 { 60 } else { 30 }
    }

    /// How a recording's movie is made.
    #[cfg(hex_capture)]
    pub fn movie_size(&self) -> scrawl_capture::capture::MovieSize {
        scrawl_capture::capture::MovieSize {
            one_x: self.scale_to_1x,
            long_edge: self.max_resolution.long_edge(),
            fps: self.fps(),
            quality: self.quality.hundredths(),
            system_audio: self.record_system_audio,
        }
    }
}

impl AppSettings {
    /// A record shortcut that shares Annotate's capture or replay shortcut is
    /// cleared: Annotate's shortcuts aren't filtered at runtime the way the
    /// record one is, so the chord would still start a whole-screen recording,
    /// and a record saved on the replay shortcut turned replay off with the
    /// Lab off. Runs after every repair that can reset `capture.hotkey`, since
    /// a reset puts Annotate back on its default and the record shortcut may
    /// be saved there.
    #[cfg(hex_capture)]
    pub(super) fn clear_record_shortcut_sharing_capture(&mut self) {
        let shared = self
            .screenshots
            .record_hotkey
            .as_ref()
            .is_some_and(|record| {
                record.overlaps(&self.capture.hotkey)
                    || self
                        .capture
                        .replay_hotkey
                        .as_ref()
                        .is_some_and(|replay| record.overlaps(replay))
            });
        if shared {
            tracing::warn!("cleared a record shortcut that shares Annotate's capture shortcut");
            self.screenshots.record_hotkey = None;
        }
    }

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

    /// The shortcuts `slot` counts because of the Screenshots record
    /// shortcut. A saved record shortcut and Annotate's capture shortcuts
    /// never share a key, whether Annotate or the Lab is on, so either one
    /// counts the other; the record shortcut counts for every other shortcut
    /// only while a recording is available (the Lab and Annotate on).
    pub(super) fn screenshots_record_counterparts(&self, slot: HotkeySlot) -> Vec<HotkeyBinding> {
        let mut others = Vec::new();
        if !cfg!(target_os = "macos") {
            return others;
        }
        #[cfg(hex_capture)]
        if slot.is_annotate_capture() {
            if let Some(record) = &self.screenshots.record_hotkey {
                others.push(record.clone());
            }
        } else if slot == HotkeySlot::ScreenshotRecord {
            others.push(self.capture.hotkey.clone());
            if let Some(replay) = &self.capture.replay_hotkey {
                others.push(replay.clone());
            }
        } else if self.screenshot_recording_available()
            && let Some(record) = &self.screenshots.record_hotkey
        {
            others.push(record.clone());
        }
        others
    }
}

impl HotkeySlot {
    /// One of Annotate's own capture shortcuts: its capture, and its 15 s
    /// replay.
    pub(super) fn is_annotate_capture(self) -> bool {
        #[cfg(hex_capture)]
        {
            matches!(self, Self::Capture | Self::CaptureReplay)
        }
        #[cfg(not(hex_capture))]
        {
            let _ = self;
            false
        }
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

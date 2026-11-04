//! Annotate's capture settings and their live values (`cfg(hex_capture)`).

use super::*;

/// Annotate: Scrawl's captures inside Hex, on macOS, Linux, and Windows
/// (`capture_host`). On by default; off means no capture work at all.
#[cfg(hex_capture)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct CaptureSettings {
    pub enabled: bool,
    /// Annotate's one key: tap to start and stop, hold while recording to
    /// draw.
    pub hotkey: HotkeyBinding,
    pub transcriber: CaptureTranscriber,
    /// Where new captures go; `None` is Scrawl's own captures folder.
    pub folder: Option<PathBuf>,
    /// Days a capture keeps its video, audio, and full-size frames; `None`
    /// keeps them.
    pub media_days: Option<u32>,
    /// Days a capture is kept at all; `None` keeps it.
    pub bundle_days: Option<u32>,
    /// Where a written capture is sent among OpenCode sessions: Scrawl's
    /// router settings, imported once from Scrawl.
    pub routing: CaptureRouting,
    /// Lets screen recordings, Hex's own captures included, show Hex's
    /// overlays: the capture capsule, picker, and ink, the dictation HUD,
    /// and the meeting capsules. Off keeps them out (`capture_host`).
    pub show_overlays_in_recordings: bool,
    /// How the capsule offers where to send a finished capture, while Kit
    /// compares the directions.
    pub capsule_flow: CapsuleFlow,
}

/// The capsule's flow after a capture stops, one of five prototypes.
#[cfg(hex_capture)]
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapsuleFlow {
    /// (a) "Copied", and the suggestions in a list under the capsule.
    #[default]
    List,
    /// (b) One quiet line, "Send to SEER ⏎"; ⇥ moves to the next guess.
    Toast,
    /// (c) A short hint of the guesses in the capsule, which then folds
    /// into Hex's menu-bar item.
    MenuBar,
    /// (d) The guesses in a small card at the pointer.
    Cursor,
    /// (e) A pill hanging from the top edge that grows down into the
    /// guesses.
    Notch,
}

#[cfg(hex_capture)]
impl CapsuleFlow {
    pub const ALL: [Self; 5] = [
        Self::List,
        Self::Toast,
        Self::MenuBar,
        Self::Cursor,
        Self::Notch,
    ];
}

#[cfg(hex_capture)]
impl Default for CaptureSettings {
    fn default() -> Self {
        let week = scrawl_capture::retention::Preset::Week.policy();
        Self {
            enabled: true,
            hotkey: HotkeyBinding::capture_default(),
            transcriber: CaptureTranscriber::default(),
            folder: None,
            media_days: week.media_days,
            bundle_days: week.bundle_days,
            routing: CaptureRouting::default(),
            show_overlays_in_recordings: false,
            capsule_flow: CapsuleFlow::default(),
        }
    }
}

/// Scrawl's router settings (`scrawl_capture::RouterConfig`, its
/// camelCase JSON). Never holds a Jev key: Jev is called with
/// `$OPENCODE_API_KEY` or OpenCode's own credential, so `hex settings`
/// never shows one.
#[cfg(hex_capture)]
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(
    from = "scrawl_capture::RouterConfig",
    into = "scrawl_capture::RouterConfig"
)]
pub struct CaptureRouting(scrawl_capture::RouterConfig);

// The config's only non-`Eq` field is a JSON value, which compares totally.
#[cfg(hex_capture)]
impl Eq for CaptureRouting {}

#[cfg(hex_capture)]
impl From<scrawl_capture::RouterConfig> for CaptureRouting {
    fn from(mut config: scrawl_capture::RouterConfig) -> Self {
        config.jev_api_key = None;
        Self(config)
    }
}

#[cfg(hex_capture)]
impl From<CaptureRouting> for scrawl_capture::RouterConfig {
    fn from(routing: CaptureRouting) -> Self {
        routing.0
    }
}

#[cfg(hex_capture)]
impl CaptureRouting {
    pub fn config(&self) -> &scrawl_capture::RouterConfig {
        &self.0
    }
}

#[cfg(hex_capture)]
impl CaptureSettings {
    /// The folder new captures go to.
    pub fn captures_dir(&self) -> PathBuf {
        self.folder
            .clone()
            .unwrap_or_else(scrawl_capture::handoff::builtin_captures_dir)
    }

    pub fn retention(&self) -> scrawl_capture::retention::Policy {
        scrawl_capture::retention::Policy {
            media_days: self.media_days,
            bundle_days: self.bundle_days,
        }
    }

    pub fn set_retention(&mut self, policy: scrawl_capture::retention::Policy) {
        self.media_days = policy.media_days;
        self.bundle_days = policy.bundle_days;
    }
}

/// What turns a capture's narration into words.
#[cfg(hex_capture)]
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureTranscriber {
    /// The dictation model Hex already has loaded, with word times.
    #[default]
    Parakeet,
    /// Apple's speech recognizer, as Scrawl uses by default.
    Apple,
}

#[cfg(hex_capture)]
impl CaptureTranscriber {
    pub const ALL: [Self; 2] = [Self::Parakeet, Self::Apple];
}

/// The saved capture settings (`cfg(hex_capture)` builds).
#[cfg(hex_capture)]
pub fn capture_settings() -> CaptureSettings {
    CAPTURE_SETTINGS
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// The capture shortcut's keycaps (`capture.hotkey`), without cloning the
/// capture settings.
#[cfg(hex_capture)]
pub fn capture_hotkey_keycaps() -> Vec<String> {
    CAPTURE_SETTINGS
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .hotkey
        .keycaps()
}

/// The saved [`CapsuleFlow`], without cloning the capture settings.
#[cfg(hex_capture)]
pub fn capsule_flow() -> CapsuleFlow {
    CapsuleFlow::ALL
        .get(usize::from(CAPSULE_FLOW.load(Ordering::Acquire)))
        .copied()
        .unwrap_or_default()
}

/// Annotate is on (`capture.enabled`).
#[cfg(hex_capture)]
pub fn capture_on() -> bool {
    CAPTURE_ON.load(Ordering::Acquire)
}

/// Scrawl's own app is running, so it owns the capture shortcut and Hex
/// doesn't match it.
#[cfg(hex_capture)]
pub fn scrawl_app_running() -> bool {
    SCRAWL_APP_RUNNING.load(Ordering::Acquire)
}

#[cfg(hex_capture)]
pub fn set_scrawl_app_running(running: bool) {
    SCRAWL_APP_RUNNING.store(running, Ordering::Release);
}

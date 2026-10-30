//! What the current OS's runtime can do, the OS-only state the panes show,
//! and the requests they make of it.
//!
//! The panes never call macOS or Windows APIs. Permissions, the login item,
//! the meetings store, and the Hex Microphone driver reach them as plain data
//! in [`PlatformState`], and every action they take goes out as a
//! [`PlatformRequest`] through [`PlatformHost`]. Linux scenes and tests feed
//! preview data and record the requests, so macOS and Windows surfaces render
//! and are tested on Linux; a native host fills the same data on its own OS.

use super::meetings::Meeting;
use crate::calendar::{CalendarAccess, CalendarInfo, CallProvider};
use crate::hex_microphone::{
    DemoPlaybackTrack, DemoSnapshot, DemoSourceKind, HexMicrophoneStatus, TunedFilterConfig,
};
use crate::login_item::{LoginItemResult, LoginItemStatus};
use crate::onboarding::{Input, PermissionAction, PermissionKind, PermissionState, SetupStatus};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Os {
    Linux,
    Macos,
    Windows,
}

/// What the runtime can do. A missing capability omits its pane or row; the
/// window never shows an imitation of a feature the runtime cannot perform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Capabilities {
    pub(super) os: Os,
    /// Meeting recording and its pane, with the recordings and their
    /// settings (macOS beta).
    pub(super) meetings: bool,
    /// Calendar meeting reminders: the Calendar card under Meetings
    /// (macOS, EventKit).
    pub(super) calendar: bool,
    /// Privacy permissions gate dictation (macOS).
    pub(super) permissions: bool,
    pub(super) launch_at_login: bool,
    pub(super) dock_icon: bool,
    /// The listener can be off: the status card and Enable on launch.
    pub(super) listener_control: bool,
    /// Paste with Ctrl-Shift-V for terminals (Linux and Windows).
    pub(super) terminal_paste: bool,
    /// The dictation shortcut can pass through to other apps (Windows).
    pub(super) share_shortcut: bool,
    /// The experimental Hex Microphone virtual driver (macOS).
    pub(super) hex_microphone: bool,
    /// The window sees dictation events, so the tour can end with a
    /// practice dictation into its own field.
    pub(super) practice_dictation: bool,
    /// The Voice Filter pane: the Hex Microphone's driver, tuning, and
    /// try-it demo (macOS, under Experiments).
    pub(super) fun: bool,
    /// Modifier-only shortcuts can require the left or right key (macOS).
    pub(super) modifier_sides: bool,
    pub(super) duck_audio: bool,
    /// Input-source changes can operate the installed-model picker (macOS).
    pub(super) keyboard_model_switching: bool,
    /// Failures play the sound set's error sound (macOS).
    pub(super) error_sound: bool,
}

impl Capabilities {
    pub(super) const LINUX: Self = Self {
        os: Os::Linux,
        meetings: false,
        calendar: false,
        permissions: false,
        launch_at_login: false,
        dock_icon: false,
        listener_control: true,
        terminal_paste: true,
        share_shortcut: false,
        hex_microphone: false,
        practice_dictation: false,
        fun: false,
        modifier_sides: false,
        duck_audio: false,
        keyboard_model_switching: false,
        error_sound: false,
    };

    pub(super) const MACOS: Self = Self {
        os: Os::Macos,
        meetings: true,
        calendar: true,
        permissions: true,
        launch_at_login: true,
        dock_icon: true,
        listener_control: false,
        terminal_paste: false,
        share_shortcut: false,
        hex_microphone: true,
        practice_dictation: true,
        fun: true,
        modifier_sides: true,
        duck_audio: true,
        keyboard_model_switching: true,
        error_sound: true,
    };

    pub(super) const WINDOWS: Self = Self {
        os: Os::Windows,
        meetings: false,
        calendar: false,
        permissions: false,
        launch_at_login: true,
        dock_icon: false,
        listener_control: true,
        terminal_paste: true,
        share_shortcut: true,
        hex_microphone: false,
        practice_dictation: false,
        fun: false,
        modifier_sides: false,
        duck_audio: false,
        keyboard_model_switching: false,
        error_sound: false,
    };

    /// This OS's capabilities, with the rows its window host decides.
    pub(super) fn current() -> Self {
        let os = if cfg!(target_os = "macos") {
            Self::MACOS
        } else if cfg!(target_os = "windows") {
            Self::WINDOWS
        } else {
            Self::LINUX
        };
        Self {
            terminal_paste: super::host::TERMINAL_PASTE,
            ..os
        }
    }
}

/// What the Calendar card shows; the host reads it off the main thread.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct CalendarView {
    pub(super) access: CalendarAccess,
    pub(super) calendars: Vec<CalendarInfo>,
    /// The next meetings from the included calendars, once connected.
    pub(super) upcoming: Vec<UpcomingMeeting>,
    /// Connect calendar was pressed and macOS is asking.
    pub(super) connecting: bool,
    pub(super) error: Option<String>,
}

impl CalendarView {
    /// A connected calendar with three meetings coming up, for scenes.
    pub(super) fn preview() -> Self {
        let calendar = |id: &str, title: &str, source: &str| CalendarInfo {
            id: id.into(),
            title: title.into(),
            source: source.into(),
        };
        let meeting = |title: &str, when: &str, provider| UpcomingMeeting {
            title: title.into(),
            when: when.into(),
            provider,
        };
        Self {
            access: CalendarAccess::FullAccess,
            calendars: vec![
                calendar("work", "Work", "kit@anoma.ly"),
                calendar("home", "Home", "iCloud"),
                calendar("birthdays", "Birthdays", "Other"),
            ],
            upcoming: vec![
                meeting(
                    "Design sync",
                    "Today · 2:30 PM",
                    Some(CallProvider::GoogleMeet),
                ),
                meeting("Rune review", "Today · 4:00 PM", Some(CallProvider::Zoom)),
                meeting("1:1 with Dax", "Tomorrow · 10:00 AM", None),
            ],
            connecting: false,
            error: None,
        }
    }
}

/// One upcoming meeting as Settings lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct UpcomingMeeting {
    pub(super) title: String,
    /// "Today · 2:30 PM", already localized.
    pub(super) when: String,
    pub(super) provider: Option<CallProvider>,
}

/// What the Calendar card asks the host for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CalendarRequest {
    /// Ask macOS for full calendar access, then mark the calendar connected.
    Connect,
    /// System Settings › Privacy & Security › Calendars.
    OpenPrivacySettings,
}

/// Whether Apple Intelligence can write meeting notes, as
/// `apple_intelligence::Availability` reports it.
// Only the macOS host reports the states fixtures do not show.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum NotesAvailability {
    #[default]
    Available,
    DeviceNotEligible,
    NotEnabled,
    ModelNotReady,
    UnsupportedSystem,
    Unknown,
}

impl NotesAvailability {
    pub(super) fn is_available(self) -> bool {
        self == Self::Available
    }
}

/// What the Scrawl pane shows (the `capture` experiment).
#[cfg(hex_capture)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ScrawlView {
    /// Screen Recording, which a capture needs to record the screen.
    pub(super) screen_recording: bool,
    /// Scrawl's own app runs, so it owns the capture shortcut.
    pub(super) scrawl_running: bool,
    /// Use Hex for annotations was pressed and Scrawl has not quit yet.
    pub(super) quitting_scrawl: bool,
    /// A captures folder that could not be revealed or chosen.
    pub(super) folder_error: Option<String>,
    /// Turning captures on or off failed (the pane sets it).
    pub(super) enable_error: Option<String>,
}

#[cfg(hex_capture)]
impl ScrawlView {
    /// Screen Recording allowed and Scrawl not running.
    pub(super) fn preview() -> Self {
        Self {
            screen_recording: true,
            ..Self::default()
        }
    }
}

/// What the Scrawl pane asks the host for.
#[cfg(hex_capture)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScrawlRequest {
    /// Read Screen Recording and Scrawl's app again now (the pane opened
    /// or the window became key), not at the next once-a-second refresh.
    Refresh,
    /// Use Hex for annotations: ask Scrawl's own app to quit. Nothing else
    /// ever quits it.
    QuitScrawl,
    RevealFolder,
    ChooseFolder,
}

/// The OS-only state the panes show; the host keeps it current.
#[derive(Clone, Debug)]
pub(super) struct PlatformState {
    pub(super) caps: Capabilities,
    pub(super) input_sources: Vec<crate::keyboard_model_switching::InputSource>,
    /// Microphone, Accessibility, and whether the selected
    /// speech model is installed. Every permission is ready where the OS has
    /// none.
    pub(super) permissions: SetupStatus,
    /// Permissions whose Allow was pressed since they were last granted.
    pub(super) permissions_asked: Vec<PermissionKind>,
    /// Screen & System Audio Recording, for meetings.
    pub(super) system_audio: bool,
    /// `None` while the login item worker checks, or when it is unavailable.
    pub(super) login_item: Option<LoginItemStatus>,
    pub(super) login_item_checking: bool,
    pub(super) login_item_error: Option<String>,
    /// The last Launch at login click while macOS registers or unregisters
    /// the login item, so the switch moves at once. Memory only: macOS's
    /// answer replaces it.
    pub(super) login_item_requested: Option<bool>,
    pub(super) hex_microphone: Option<HexMicrophoneStatus>,
    pub(super) notes: NotesAvailability,
    /// A meeting folder that could not be revealed or chosen.
    pub(super) meeting_folder_error: Option<String>,
    /// The Calendar card's access, calendars, and next meetings.
    pub(super) calendar: CalendarView,
    /// The tour's relaunch failed; the person must reopen Hex.
    pub(super) relaunch_failed: bool,
    /// Dev, local, or test build (`crate::experiments::is_dev_build` on a
    /// native host; `true` in fixtures except `"mac-production"`). Production
    /// builds hide Labs and every experimental pane.
    pub(super) dev_build: bool,
    /// Developer builds: keeps Meetings available without opt-in in debug builds
    /// (`crate::DEVELOPER_FEATURES_ENABLED` on a native host).
    pub(super) developer: bool,
    /// The `playground` experiment.
    pub(super) playground: bool,
    /// The `voice-filter` experiment: the Voice Filter pane even before the
    /// Hex Microphone is on (`crate::experiments::enabled` on a native host).
    pub(super) voice_filter: bool,
    /// Live per-experiment switch state for non-pane experiments (`live-text`,
    /// `live-chapters`, …).
    pub(super) experiments: [bool; crate::experiments::Experiment::ALL.len()],
    /// macOS 27 lists Accessibility as "Device Control and Data Access"
    /// (`permission_guide::list_name`); the permission rows use that name.
    pub(super) accessibility_renamed: bool,
    /// The Hex Microphone's live filter tuning
    /// (`hex_microphone::current_tuned_config`).
    pub(super) tuning: TunedFilterConfig,
    /// The Voice Filter pane's try-it demo (`hex_microphone::demo_snapshot`); `None`
    /// until the host has one. Refresh it every frame while it records or
    /// plays (`DemoSnapshot::is_recording`, `playing`).
    pub(super) demo: Option<DemoSnapshot>,
    /// What HUD Lab shows on the dictation indicator. The pane edits it and
    /// sends `PlatformRequest::ConfigureHud`; the host need not write it.
    pub(super) hud: HudLab,
    /// The dictation event log the Activity pane reads (`live.ndjson`);
    /// `None` where the host keeps none.
    pub(super) activity_log: Option<std::path::PathBuf>,
    /// The Scrawl pane, while the `capture` experiment is on; `None` hides
    /// it.
    #[cfg(hex_capture)]
    pub(super) scrawl: Option<ScrawlView>,
}

/// The indicator state HUD Lab demonstrates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum HudDemo {
    #[default]
    Recording,
    Transcribing,
    Processing,
}

/// Everything HUD Lab drives: Dot scope's tuning and the demonstrated
/// state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct HudLab {
    pub(super) tuning: crate::dot_scope::Tuning,
    pub(super) demo: HudDemo,
}

/// The Voice Filter pane's demo controls (`hex_microphone`'s demo functions).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DemoRequest {
    /// `start_demo_recording` (five seconds of the microphone).
    Record,
    StopRecording,
    SelectSource(DemoSourceKind),
    Play(DemoPlaybackTrack),
    StopPlayback,
}

impl PlatformState {
    pub(super) fn new(caps: Capabilities) -> Self {
        Self {
            caps,
            input_sources: Vec::new(),
            permissions: crate::onboarding::status_with_transcription_model(false),
            permissions_asked: Vec::new(),
            system_audio: false,
            login_item: None,
            login_item_checking: false,
            login_item_error: None,
            login_item_requested: None,
            hex_microphone: None,
            notes: NotesAvailability::Available,
            meeting_folder_error: None,
            calendar: CalendarView::default(),
            relaunch_failed: false,
            dev_build: true,
            developer: false,
            playground: false,
            voice_filter: false,
            experiments: [false; crate::experiments::Experiment::ALL.len()],
            accessibility_renamed: false,
            tuning: TunedFilterConfig::default(),
            demo: None,
            hud: HudLab::default(),
            activity_log: None,
            #[cfg(hex_capture)]
            scrawl: None,
        }
    }

    /// The permission state with `transcription_model` from the caller,
    /// which knows the selected model.
    pub(super) fn setup_status(&self, transcription_model: bool) -> SetupStatus {
        if !self.caps.permissions {
            return crate::onboarding::status_with_transcription_model(transcription_model);
        }
        SetupStatus {
            transcription_model,
            ..self.permissions
        }
    }

    /// Adopts what the window host's login item reported: `None` where the
    /// host has no such row (it stays unavailable), an error beside the last
    /// known state.
    pub(super) fn set_login_item(&mut self, result: Option<Result<bool, String>>) {
        match result {
            None => {}
            Some(Ok(enabled)) => {
                self.login_item = Some(if enabled {
                    LoginItemStatus::Enabled
                } else {
                    LoginItemStatus::Disabled
                });
                self.login_item_error = None;
            }
            Some(Err(error)) => {
                self.login_item.get_or_insert(LoginItemStatus::Disabled);
                self.login_item_error = Some(error);
            }
        }
    }

    /// Whether the Launch at login switch shows on: the pending click, else
    /// what macOS last reported.
    pub(super) fn login_item_on(&self) -> bool {
        self.login_item_requested
            .unwrap_or(self.login_item == Some(LoginItemStatus::Enabled))
    }

    /// The switch was pressed: it shows `enabled` while the host asks macOS.
    pub(super) fn request_login_item(&mut self, enabled: bool) {
        self.login_item_requested = Some(enabled);
        self.login_item_error = None;
    }

    /// Adopts the login item worker's answer. `settled` is false while a
    /// later click still waits behind it: the switch keeps that click and
    /// only the reported status is remembered. Once settled, the switch
    /// shows macOS's state, springing back beside the error if the request
    /// failed or macOS kept the other state.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(super) fn login_item_answered(&mut self, answer: LoginItemResult, settled: bool) {
        let current = settled || self.login_item_requested.is_none();
        let error = match answer {
            Ok(status) => {
                self.login_item = Some(status);
                None
            }
            Err(failure) => {
                if failure.status.is_some() {
                    self.login_item = failure.status;
                }
                Some(failure.message)
            }
        };
        if current {
            self.login_item_requested = None;
            self.login_item_error = error;
        }
    }

    pub(super) fn asked(&self, kind: PermissionKind) -> bool {
        self.permissions_asked.contains(&kind)
    }

    pub(super) fn note_asked(&mut self, kind: PermissionKind) {
        if !self.asked(kind) {
            self.permissions_asked.push(kind);
        }
    }

    /// Deterministic macOS state for scenes: every permission granted, the
    /// login item off, and the driver installed.
    pub(super) fn preview(caps: Capabilities) -> Self {
        let mut state = Self::new(caps);
        let ready = PermissionState::Ready;
        state.permissions = SetupStatus {
            microphone: ready,
            accessibility: ready,
            transcription_model: true,
        };
        state.system_audio = true;
        state.login_item = caps.launch_at_login.then_some(LoginItemStatus::Disabled);
        if caps.hex_microphone {
            // Fixed, so the scenes' goldens don't change with the crate version.
            let version = "0.0.0".to_string();
            state.hex_microphone = Some(HexMicrophoneStatus {
                driver_bundle_installed: true,
                driver_loaded: true,
                installed_version: Some(version.clone()),
                bundled_version: version,
                sink_streaming: true,
                mic_in_use: true,
                active_apps: vec!["Slack".into()],
                action_state: crate::hex_microphone::DriverActionState::Idle,
            });
        }
        if caps.fun {
            state.demo = Some(super::fun::preview_demo(false, None));
        }
        state
    }
}

/// A meeting action the pane asks the meetings runtime for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum MeetingRequest {
    Start,
    Stop,
    Rename { id: String, title: String },
    Trash(String),
    Summarize(String),
    Reveal(String),
    RevealFolder,
    ChooseFolder,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum PlatformRequest {
    /// Request the microphone or open the System Settings guide.
    Permission(PermissionAction),
    /// Screen & System Audio Recording, for meetings.
    SystemAudio,
    SetLaunchAtLogin(bool),
    OpenLoginItemSettings,
    InstallHexMicrophone,
    UninstallHexMicrophone,
    Meeting(MeetingRequest),
    Calendar(CalendarRequest),
    /// Quit and reopen Hex so macOS applies new permissions.
    Relaunch,
    /// Apply this tuning to the live Hex Microphone filter
    /// (`hex_microphone::update_tuned_config`). The pane has already saved
    /// the mode, comfort noise, and strength in `settings.json`.
    SetTuning(TunedFilterConfig),
    HexMicrophoneDemo(DemoRequest),
    /// Show this on the dictation indicator: `DictationIndicatorEvent::
    /// ConfigureDotScope` with the tuning, then `Reset` and the
    /// demonstrated state.
    ConfigureHud(HudLab),
    /// Only the tuning, live on whatever the indicator shows (a dictation
    /// or the demo): `ConfigureDotScope`, no `Reset`.
    TuneHud(HudLab),
    /// Leaving HUD Lab: `Reset` hides its demo.
    EndHudDemo,
    /// Save Dot scope's tuning as `dot-scope.json`, which the HUD loads at
    /// launch.
    SaveDotScope(crate::dot_scope::Tuning),
    #[cfg(hex_capture)]
    Scrawl(ScrawlRequest),
}

/// The host that performs [`PlatformRequest`]s and refreshes
/// [`PlatformState`] on its own OS.
// Only macOS has a native host.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(super) trait NativeHost {
    fn send(&mut self, request: PlatformRequest) -> Result<(), String>;
    /// Copies fresh state in; true when anything shown changed.
    fn poll(&mut self, state: &mut PlatformState) -> bool;
    /// The meetings store, newest first, when it changed since the last
    /// call. Asked only while the Meetings pane shows.
    fn meetings(&mut self) -> Option<Result<Vec<Meeting>, String>>;
    /// What the tour hears from the runtime: practice dictation outcomes
    /// while `practicing`, and a shortcut listener that failed to start.
    fn tour_inputs(&mut self, practicing: bool) -> Vec<Input>;
    /// The microphone's latest peak, 0 to 1, while a dictation records;
    /// silence when no audio arrived lately.
    fn practice_level(&self) -> f32 {
        0.0
    }
}

pub(super) enum PlatformHost {
    /// Linux: the service owns everything, so nothing is asked here.
    None,
    /// Scenes and tests: requests are recorded, never performed.
    Fixture(Vec<PlatformRequest>),
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Native(Box<dyn NativeHost>),
}

impl PlatformHost {
    pub(super) fn send(&mut self, request: PlatformRequest) -> Result<(), String> {
        match self {
            #[cfg(target_os = "linux")]
            Self::None => {
                if let PlatformRequest::Scrawl(request) = request {
                    linux_scrawl_request(request);
                }
                Ok(())
            }
            #[cfg(not(target_os = "linux"))]
            Self::None => Ok(()),
            Self::Fixture(sent) => {
                sent.push(request);
                Ok(())
            }
            Self::Native(host) => host.send(request),
        }
    }

    pub(super) fn poll(&mut self, state: &mut PlatformState) -> bool {
        match self {
            Self::Native(host) => host.poll(state),
            #[cfg(target_os = "linux")]
            Self::None => linux_scrawl_poll(state),
            _ => false,
        }
    }

    pub(super) fn meetings(&mut self) -> Option<Result<Vec<Meeting>, String>> {
        match self {
            Self::Native(host) => host.meetings(),
            _ => None,
        }
    }

    pub(super) fn tour_inputs(&mut self, practicing: bool) -> Vec<Input> {
        match self {
            Self::Native(host) => host.tour_inputs(practicing),
            _ => Vec::new(),
        }
    }

    pub(super) fn practice_level(&self) -> f32 {
        match self {
            Self::Native(host) => host.practice_level(),
            _ => 0.0,
        }
    }

    pub(super) fn is_fixture(&self) -> bool {
        matches!(self, Self::Fixture(_))
    }
}

/// The Scrawl pane on Linux: it shows while the `capture` experiment is on.
/// The desktop's portal asks for the screen at each capture, so nothing is
/// granted ahead of time, and Scrawl's own app is macOS's.
#[cfg(target_os = "linux")]
fn linux_scrawl_poll(state: &mut PlatformState) -> bool {
    if !state.dev_build || state.scrawl.is_some() {
        return false;
    }
    state.scrawl = Some(ScrawlView::preview());
    true
}

/// Reveals the captures folder; choosing another is set in `settings.json`
/// (`capture.folder`) for now.
#[cfg(target_os = "linux")]
fn linux_scrawl_request(request: ScrawlRequest) {
    if request == ScrawlRequest::RevealFolder {
        let folder = crate::app_settings::capture_settings().captures_dir();
        if std::fs::create_dir_all(&folder).is_ok()
            && let Err(error) = scrawl_capture::handoff::open_path(&folder)
        {
            tracing::warn!(
                error = format!("{error:#}"),
                "could not reveal the captures folder"
            );
        }
    }
}

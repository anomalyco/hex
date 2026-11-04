//! Meeting settings: recording, retention, call providers, the calendar,
//! the recording indicator, and their live values for the running app.

use super::*;

/// Screen corner where the floating meeting recording indicator snaps.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum MeetingIndicatorCorner {
    #[default]
    TopRight = 0,
    TopLeft = 1,
    BottomRight = 2,
    BottomLeft = 3,
}

impl MeetingIndicatorCorner {
    pub const fn is_right(self) -> bool {
        matches!(self, Self::TopRight | Self::BottomRight)
    }

    pub const fn is_top(self) -> bool {
        matches!(self, Self::TopRight | Self::TopLeft)
    }

    const fn decode(value: u8) -> Self {
        match value {
            1 => Self::TopLeft,
            2 => Self::BottomRight,
            3 => Self::BottomLeft,
            _ => Self::TopRight,
        }
    }
}

/// Opt-in meeting recording. Detection, the Meetings pane, and the meeting
/// paste shortcut stay off in release builds until `enabled` is set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct MeetingSettings {
    pub enabled: bool,
    /// Folder for finished Markdown transcripts; `None` uses the default.
    pub output_folder: Option<PathBuf>,
    /// Title meetings and write a two-sentence summary with Apple
    /// Intelligence when it is available.
    pub summarize_with_apple_intelligence: bool,
    /// Title meetings and write a two-sentence summary with the OpenCode
    /// model, which sends the transcript to it. On a Mac, only when Apple
    /// Intelligence is unavailable or off.
    pub summarize_with_opencode: bool,
    /// Show the floating recording indicator while a meeting is recording.
    pub show_recording_indicator: bool,
    /// Screen corner the floating recording indicator snaps to after dragging.
    pub recording_indicator_corner: MeetingIndicatorCorner,
    /// What to do when a call from each website or app is detected. Older
    /// files only had a `never_offer` list, whose entries carry no `policy`
    /// and so read as Never offer.
    #[serde(alias = "never_offer")]
    pub providers: Vec<MeetingProviderChoice>,
    /// Reminders before calendar meetings (macOS). Off until the user
    /// connects a calendar.
    pub calendar: CalendarSettings,
    /// How long finished meetings are kept before they move to the Trash.
    /// `None` until the user chooses: meetings recorded since the default
    /// began are kept 24 hours, and older ones stay. Left out of the file
    /// while unchosen: a build from before it can't read `null` (#744).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention: Option<MeetingRetention>,
    /// Labs › Choose meeting summary model: the OpenCode `provider/model`
    /// meeting notes use; left out of the file while unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencode_model: Option<String>,
}

/// How long a finished meeting is kept, counted from when it ended.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingRetention {
    Day,
    Week,
    Month,
    Forever,
}

impl MeetingRetention {
    pub const ALL: [Self; 4] = [Self::Day, Self::Week, Self::Month, Self::Forever];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Day => "24 hours",
            Self::Week => "7 days",
            Self::Month => "30 days",
            Self::Forever => "Forever",
        }
    }

    pub const fn max_age_ms(self) -> Option<u64> {
        const DAY_MS: u64 = 24 * 60 * 60 * 1_000;
        match self {
            Self::Day => Some(DAY_MS),
            Self::Week => Some(7 * DAY_MS),
            Self::Month => Some(30 * DAY_MS),
            Self::Forever => None,
        }
    }

    /// The default for meetings recorded since it began.
    pub const DEFAULT: Self = Self::Day;

    pub(super) const fn encode(retention: Option<Self>) -> u8 {
        match retention {
            Some(Self::Day) => 0,
            Some(Self::Week) => 1,
            Some(Self::Month) => 2,
            Some(Self::Forever) => 3,
            None => 4,
        }
    }

    const fn decode(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Day),
            1 => Some(Self::Week),
            2 => Some(Self::Month),
            3 => Some(Self::Forever),
            _ => None,
        }
    }
}

/// The connected calendar and its meeting reminders.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct CalendarSettings {
    /// Set once the user clicks Connect calendar and macOS grants access;
    /// Hex never asks for calendar access before that.
    pub connected: bool,
    /// Calendars left out of reminders, by EventKit identifier. Every
    /// calendar is included by default, so new ones are too.
    pub excluded_calendars: Vec<String>,
    /// Show a reminder capsule before each meeting.
    pub remind: bool,
    /// How many minutes before a meeting starts the reminder shows; zero
    /// shows it at the start.
    pub lead_minutes: u8,
}

/// The reminder lead times Settings offers, in minutes.
pub const CALENDAR_LEAD_MINUTES: [u8; 4] = [0, 1, 2, 5];

impl Default for CalendarSettings {
    fn default() -> Self {
        Self {
            connected: false,
            excluded_calendars: Vec::new(),
            remind: true,
            lead_minutes: 1,
        }
    }
}

impl CalendarSettings {
    /// Whether reminders should run: connected, and wanted.
    pub fn reminders_active(&self) -> bool {
        self.connected && self.remind
    }

    /// The lead time, snapped to an offered choice.
    pub fn lead(&self) -> std::time::Duration {
        let minutes = if CALENDAR_LEAD_MINUTES.contains(&self.lead_minutes) {
            self.lead_minutes
        } else {
            1
        };
        std::time::Duration::from_secs(u64::from(minutes) * 60)
    }

    pub fn includes(&self, calendar: &str) -> bool {
        !self
            .excluded_calendars
            .iter()
            .any(|excluded| excluded == calendar)
    }
}

/// The call services Settings always lists, by provider key (the app's
/// canonical bundle ID, or the website host for browser services) and
/// English name.
pub const KNOWN_MEETING_PROVIDERS: [(&str, &str); 8] = [
    ("com.tinyspeck.slackmacgap", "Slack huddles"),
    ("us.zoom.xos", "Zoom"),
    ("meet.google.com", "Google Meet"),
    ("com.microsoft.teams2", "Microsoft Teams"),
    ("com.apple.FaceTime", "FaceTime"),
    ("net.whatsapp.WhatsApp", "WhatsApp"),
    ("com.gather.GatherV2", "Gather"),
    ("com.hnc.Discord", "Discord"),
];

/// What Hex does when it detects a call from a provider.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingPolicy {
    /// Offer to record.
    #[default]
    Ask,
    /// Start recording without an offer.
    AlwaysRecord,
    /// Neither offer nor record.
    NeverOffer,
}

/// A website host or an app's canonical bundle ID, the name shown in
/// Settings, and the user's choice for its calls. Unlisted providers Ask.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MeetingProviderChoice {
    pub key: String,
    pub label: String,
    /// Always written; missing only in a legacy `never_offer` entry.
    #[serde(default = "legacy_never_offer")]
    pub policy: MeetingPolicy,
}

fn legacy_never_offer() -> MeetingPolicy {
    MeetingPolicy::NeverOffer
}

impl Default for MeetingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            output_folder: None,
            summarize_with_apple_intelligence: true,
            summarize_with_opencode: false,
            show_recording_indicator: true,
            recording_indicator_corner: MeetingIndicatorCorner::TopRight,
            providers: Vec::new(),
            calendar: CalendarSettings::default(),
            retention: None,
            opencode_model: None,
        }
    }
}

impl MeetingSettings {
    /// Whether meetings may record and process: always in developer builds,
    /// otherwise once turned on, and never under a managed lock.
    pub fn available(&self) -> bool {
        crate::labs::Feature::Meetings.managed().is_none()
            && (crate::DEVELOPER_FEATURES_ENABLED || self.enabled)
    }

    pub fn resolved_output_folder(&self) -> PathBuf {
        self.output_folder
            .clone()
            .filter(|folder| !folder.as_os_str().is_empty())
            .unwrap_or_else(default_meeting_output_folder)
    }
}

/// Whether meeting detection and recording may run: always in developer
/// builds, otherwise only after the user opts in.
pub fn meetings_enabled() -> bool {
    MEETINGS_ENABLED.load(Ordering::Acquire)
}

/// Why meetings can't record now.
pub fn meetings_off_reason() -> &'static str {
    match crate::labs::Feature::Meetings.managed() {
        Some(lock) => lock.refusal(),
        None => "Meetings are off; turn them on with `hex settings set meetings.enabled on`",
    }
}

/// Apple Intelligence may write meeting notes (macOS).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn meeting_notes_enabled() -> bool {
    MEETING_NOTES_ENABLED.load(Ordering::Acquire)
}

/// The OpenCode model may write meeting notes.
pub fn meeting_notes_with_opencode() -> bool {
    MEETING_NOTES_OPENCODE.load(Ordering::Acquire)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn meeting_indicator_enabled() -> bool {
    MEETING_INDICATOR_ENABLED.load(Ordering::Acquire)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn meeting_indicator_corner() -> MeetingIndicatorCorner {
    MeetingIndicatorCorner::decode(MEETING_INDICATOR_CORNER.load(Ordering::Acquire))
}

/// The user's choices for detected calls, by website or app.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn meeting_provider_choices() -> Vec<MeetingProviderChoice> {
    MEETING_PROVIDERS
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// The connected calendar's reminder settings, as last applied.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// [`MeetingSettings::retention`] for the running meeting controller.
/// The retention the user chose, `None` until they choose. A process that
/// never loaded settings (`hex mcp`, `hex meetings`) reads it from
/// `settings.json` without migrating or writing anything.
pub fn meeting_retention() -> Option<MeetingRetention> {
    match MEETING_RETENTION.load(Ordering::Acquire) {
        RETENTION_UNSET if cfg!(test) => None,
        RETENTION_UNSET => stored_meeting_retention(),
        value => MeetingRetention::decode(value),
    }
}

fn stored_meeting_retention() -> Option<MeetingRetention> {
    let data = fs::read(path().ok()?).ok()?;
    let settings: serde_json::Value = serde_json::from_slice(&data).ok()?;
    serde_json::from_value(settings.get("meetings")?.get("retention")?.clone()).ok()
}

pub fn calendar_settings() -> CalendarSettings {
    CALENDAR_SETTINGS
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// Sets one provider's policy; returns whether the settings changed.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn set_meeting_provider(meetings: &mut MeetingSettings, choice: MeetingProviderChoice) -> bool {
    match meetings
        .providers
        .iter_mut()
        .find(|known| known.key == choice.key)
    {
        Some(known) if known.policy == choice.policy => false,
        Some(known) => {
            known.policy = choice.policy;
            true
        }
        None => {
            meetings.providers.push(choice);
            true
        }
    }
}

/// Saves a choice made from a meeting capsule while the Settings window is
/// closed.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn save_meeting_provider(choice: MeetingProviderChoice) {
    match AppSettings::load() {
        Ok(mut settings) => {
            if set_meeting_provider(&mut settings.meetings, choice)
                && let Err(error) = settings.save()
            {
                tracing::warn!(%error, "could not save a meeting provider choice");
            }
        }
        Err(error) => tracing::warn!(%error, "could not load settings to save a meeting choice"),
    }
}

/// Saves the corner the recording capsule was dragged to.
pub fn save_meeting_indicator_corner(corner: MeetingIndicatorCorner) {
    match AppSettings::load() {
        Ok(mut settings) => {
            if settings.meetings.recording_indicator_corner == corner {
                return;
            }
            settings.meetings.recording_indicator_corner = corner;
            if let Err(error) = settings.save() {
                tracing::warn!(%error, "could not save the recording capsule's corner");
            }
        }
        Err(error) => tracing::warn!(%error, "could not load settings to save the capsule corner"),
    }
}

pub fn meeting_output_folder() -> PathBuf {
    MEETING_OUTPUT_FOLDER
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
        .unwrap_or_else(default_meeting_output_folder)
}

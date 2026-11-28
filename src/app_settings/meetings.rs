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
    #[cfg(test)]
    pub const ALL: [Self; 4] = [
        Self::TopRight,
        Self::TopLeft,
        Self::BottomRight,
        Self::BottomLeft,
    ];

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
    /// Who titles finished meetings and writes their two-sentence summary.
    pub summaries: MeetingSummaries,
    /// Retired: read once by the summaries migration, never written.
    #[serde(default, skip_serializing)]
    pub summarize_with_apple_intelligence: Option<bool>,
    /// Retired: read once by the summaries migration, never written.
    #[serde(default, skip_serializing)]
    pub summarize_with_opencode: Option<bool>,
    /// Show the floating recording indicator while a meeting is recording.
    pub show_recording_indicator: bool,
    /// Screen corner the floating recording indicator snaps to after dragging.
    pub recording_indicator_corner: MeetingIndicatorCorner,
    /// The display the floating recording indicator was last dragged to,
    /// named the way the OS names it (`\\.\DISPLAY2` on Windows, the
    /// `CGDirectDisplayID` on macOS); `None` is the primary display.
    /// Left out of the file while unset, so an older build reads this file
    /// as before, and read only while Labs › Indicator on any display is
    /// on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording_indicator_display: Option<String>,
    /// What to do when a call from each website or app is detected. Older
    /// files only had a `never_offer` list, whose entries carry no `policy`
    /// and so read as Don't offer.
    #[serde(alias = "never_offer")]
    pub providers: Vec<MeetingProviderChoice>,
    /// What to do for a call from a website or app with no entry in
    /// `providers`. Older files have none and read as Ask, which is what
    /// every such call did before.
    pub default_policy: MeetingPolicy,
    /// Reminders before calendar meetings (macOS). Off until the user
    /// connects a calendar.
    pub calendar: CalendarSettings,
    /// How long finished meetings are kept before they move to the Trash.
    /// `None` until the user chooses: meetings recorded since the default
    /// began are kept 7 days, and older ones stay. Left out of the file
    /// while unchosen: a build from before it can't read `null` (#744).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention: Option<MeetingRetention>,
    /// How long finished meetings keep their WAV tracks, apart from their
    /// text (Meetings › Settings › Keep audio). `None` follows `retention`
    /// (Same as meetings), and is left out of the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_retention: Option<MeetingRetention>,
    /// A recording Hex started on its own stops once its call app has left
    /// the microphone and gone quiet for 30 s (macOS).
    pub stop_when_call_ends: bool,
    /// How finished meetings are written to `output_folder`.
    pub export: MeetingExportSettings,
    /// The OpenCode `provider/model` that writes meeting notes when
    /// `summaries` is OpenCode; left out of the file while unset, when
    /// OpenCode's own default model writes them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencode_model: Option<String>,
}

/// How finished meetings are written to the meetings folder.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct MeetingExportSettings {
    /// Notes or a rename rename the exported Markdown to the meeting's
    /// title, keeping its date and time.
    pub descriptive_filenames: bool,
    /// The transcript exports as `<stem>.transcript.md` and the notes go to
    /// `<stem>.notes.md` beside it, linking back to the transcript.
    pub notes_file: bool,
}

/// Who writes meeting titles and summaries. There is no fallback: Mac never
/// sends the transcript to OpenCode, and OpenCode never runs on this Mac.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum MeetingSummaries {
    Off = 0,
    /// Apple Intelligence on this Mac (macOS only).
    Mac = 1,
    /// The OpenCode model, which the transcript is sent to.
    #[serde(rename = "opencode")]
    OpenCode = 2,
}

impl MeetingSummaries {
    #[cfg(test)]
    pub const ALL: [Self; 3] = [Self::Off, Self::Mac, Self::OpenCode];

    /// On this Mac on macOS, otherwise off.
    pub const fn platform_default() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Off
        }
    }

    pub(super) const fn decode(value: u8) -> Self {
        match value {
            1 => Self::Mac,
            2 => Self::OpenCode,
            _ => Self::Off,
        }
    }

    /// What the two retired switches meant: Apple Intelligence on wins on a
    /// Mac (its OpenCode fallback is gone); elsewhere only OpenCode counted.
    pub(super) fn from_retired(apple_intelligence: bool, opencode: bool) -> Self {
        if cfg!(target_os = "macos") && apple_intelligence {
            Self::Mac
        } else if opencode {
            Self::OpenCode
        } else {
            Self::Off
        }
    }
}

impl Default for MeetingSummaries {
    fn default() -> Self {
        Self::platform_default()
    }
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
    pub const DEFAULT: Self = Self::Week;

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

/// The common call services Hex names, by provider key (the app's
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

/// The other call services Hex names and offers, by the same provider keys.
pub const MORE_MEETING_PROVIDERS: [(&str, &str); 10] = [
    ("org.whispersystems.signal-desktop", "Signal"),
    ("ru.keepcoder.Telegram", "Telegram"),
    ("com.tencent.xinWeChat", "WeChat"),
    ("jp.naver.line.mac", "LINE"),
    ("com.webex.meetingmanager", "Webex"),
    ("im.riot.app", "Element"),
    ("com.apple.mobilephone", "Phone"),
    ("voice.google.com", "Google Voice"),
    ("messenger.com", "Messenger"),
    ("web.groupme.com", "GroupMe"),
];

/// What Hex does when it detects a call from a provider. Shown as Ask each
/// time, Always record, and Don't offer; the stored names predate
/// those and stay so older builds can read the file.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum MeetingPolicy {
    /// Offer to record.
    #[default]
    Ask = 0,
    /// Start recording without an offer.
    AlwaysRecord = 1,
    /// Neither offer nor record.
    NeverOffer = 2,
}

impl MeetingPolicy {
    /// In the order Settings lists them.
    pub const ALL: [Self; 3] = [Self::Ask, Self::AlwaysRecord, Self::NeverOffer];

    /// The English name every surface shows (through `tr`).
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ask => "Ask each time",
            Self::AlwaysRecord => "Always record",
            Self::NeverOffer => "Don't offer",
        }
    }

    pub(super) const fn decode(value: u8) -> Self {
        match value {
            1 => Self::AlwaysRecord,
            2 => Self::NeverOffer,
            _ => Self::Ask,
        }
    }
}

/// A website host or an app's canonical bundle ID, the name shown in
/// Settings, and the user's choice for its calls. Unlisted providers follow
/// [`MeetingSettings::default_policy`].
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

pub(super) const DEFAULT_SHOW_RECORDING_INDICATOR: bool = true;

impl Default for MeetingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            output_folder: None,
            summaries: MeetingSummaries::platform_default(),
            summarize_with_apple_intelligence: None,
            summarize_with_opencode: None,
            show_recording_indicator: DEFAULT_SHOW_RECORDING_INDICATOR,
            recording_indicator_corner: MeetingIndicatorCorner::TopRight,
            recording_indicator_display: None,
            providers: Vec::new(),
            default_policy: MeetingPolicy::Ask,
            calendar: CalendarSettings::default(),
            retention: None,
            audio_retention: None,
            stop_when_call_ends: false,
            export: MeetingExportSettings::default(),
            opencode_model: None,
        }
    }
}

impl MeetingSettings {
    /// Whether meetings may record and process: always in developer builds,
    /// otherwise once turned on, and never under a managed lock.
    pub fn available(&self) -> bool {
        !crate::policy::Policy::current().locks(crate::policy::Lock::Meetings)
            && (crate::DEVELOPER_FEATURES_ENABLED || self.enabled)
    }

    /// Changes the default. Websites and apps whose answer was the old
    /// default, which Settings showed as following it, go on following it.
    pub fn set_default_policy(&mut self, policy: MeetingPolicy) {
        let old = self.default_policy;
        // Kept rather than removed: a website's entry outranks its browser's.
        for choice in &mut self.providers {
            if choice.policy == old {
                choice.policy = policy;
            }
        }
        self.default_policy = policy;
    }

    /// The choice for calls from `key`: its own, else the default.
    pub fn policy_for(&self, key: &str) -> MeetingPolicy {
        self.providers
            .iter()
            .find(|choice| choice.key == key)
            .map_or(self.default_policy, |choice| choice.policy)
    }

    /// Whether `key` answers differently from the default (Settings shows
    /// those with their icon and their value undimmed).
    pub fn has_own_answer(&self, key: &str) -> bool {
        self.policy_for(key) != self.default_policy
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
    if crate::policy::Policy::current().locks(crate::policy::Lock::Meetings) {
        crate::policy::Lock::Meetings.refusal()
    } else {
        "Meetings are off; turn them on with `hex settings set meetings.enabled on`"
    }
}

/// Who writes meeting notes, as last applied.
pub fn meeting_summaries() -> MeetingSummaries {
    MeetingSummaries::decode(MEETING_SUMMARIES.load(Ordering::Acquire))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn meeting_indicator_enabled() -> bool {
    MEETING_INDICATOR_ENABLED.load(Ordering::Acquire)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn meeting_indicator_corner() -> MeetingIndicatorCorner {
    MeetingIndicatorCorner::decode(MEETING_INDICATOR_CORNER.load(Ordering::Acquire))
}

/// The display the recording capsule was last dragged to (Labs › Indicator
/// on any display): the OS's own name for it, or `None` for the primary
/// display. The capsules ignore it while that lab is off.
pub fn saved_meeting_indicator_display() -> Option<String> {
    MEETING_INDICATOR_DISPLAY
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
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

/// The choice for detected calls from websites and apps with none of their
/// own, as last applied.
pub fn meeting_default_policy() -> MeetingPolicy {
    MeetingPolicy::decode(MEETING_DEFAULT_POLICY.load(Ordering::Acquire))
}

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
    stored_retention("retention")
}

/// [`MeetingSettings::audio_retention`] for the running meeting controller;
/// `None` means audio follows `meetings.retention`. A process that never
/// loaded settings reads it from `settings.json` without writing anything.
pub fn meeting_audio_retention() -> Option<MeetingRetention> {
    match MEETING_AUDIO_RETENTION.load(Ordering::Acquire) {
        RETENTION_UNSET if cfg!(test) => None,
        RETENTION_UNSET => stored_audio_retention(),
        value => MeetingRetention::decode(value),
    }
}

fn stored_retention(field: &str) -> Option<MeetingRetention> {
    let data = fs::read(path().ok()?).ok()?;
    let settings: serde_json::Value = serde_json::from_slice(&data).ok()?;
    retention_in(&settings, field)
}

fn stored_audio_retention() -> Option<MeetingRetention> {
    let data = fs::read(path().ok()?).ok()?;
    let settings: serde_json::Value = serde_json::from_slice(&data).ok()?;
    audio_retention_in(&settings)
}

fn retention_in(settings: &serde_json::Value, field: &str) -> Option<MeetingRetention> {
    serde_json::from_value(settings.get("meetings")?.get(field)?.clone()).ok()
}

/// The audio retention a settings file holds. A file the
/// `meeting-labs-settings` migration hasn't seen keeps it only while its
/// retired Labs switch was on, as the migration will.
fn audio_retention_in(settings: &serde_json::Value) -> Option<MeetingRetention> {
    let migrated = settings
        .get("migrations")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|ids| {
            ids.iter()
                .any(|id| id.as_str() == Some(MEETING_LABS_SETTINGS_MIGRATION))
        });
    let lab_on = settings
        .pointer("/labs/expire_meeting_audio")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    (migrated || lab_on)
        .then(|| retention_in(settings, "audio_retention"))
        .flatten()
}

/// Meetings › Settings' Stop when the call ends, as last applied (read on
/// each pass of the meeting controller).
pub fn meeting_stop_when_call_ends() -> bool {
    MEETING_STOP_WHEN_CALL_ENDS.load(Ordering::Acquire)
}

/// Meetings › Settings › Export's Descriptive meeting file names, as last
/// applied (read when notes are applied and when a meeting is renamed).
pub fn meeting_descriptive_filenames() -> bool {
    MEETING_DESCRIPTIVE_FILENAMES.load(Ordering::Acquire)
}

/// Meetings › Settings › Export's Separate notes file, as last applied
/// (read at final export and when notes are written).
pub fn meeting_notes_file() -> bool {
    MEETING_NOTES_FILE.load(Ordering::Acquire)
}

pub fn calendar_settings() -> CalendarSettings {
    CALENDAR_SETTINGS
        .get_or_init(Default::default)
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// Sets one provider's policy; returns whether the settings changed. A
/// Teams page's answer used to be saved under its own host: the Teams app's
/// new answer replaces it rather than being shadowed by it.
pub fn set_meeting_provider(meetings: &mut MeetingSettings, choice: MeetingProviderChoice) -> bool {
    let before = meetings.providers.len();
    meetings.providers.retain(|known| {
        known.key == choice.key
            || crate::meeting::detection::provider_key_for_site(&known.key) != choice.key
    });
    let dropped = meetings.providers.len() != before;
    let changed = match meetings
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
    };
    dropped || changed
}

/// Saves a choice made from a meeting capsule while the Settings window is
/// closed.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn save_meeting_provider(choice: MeetingProviderChoice) {
    match AppSettings::load() {
        Ok(mut settings) => {
            let base = settings.clone();
            if set_meeting_provider(&mut settings.meetings, choice)
                && let Err(error) = base.update(&settings)
            {
                tracing::warn!(%error, "could not save a meeting provider choice");
            }
        }
        Err(error) => tracing::warn!(%error, "could not load settings to save a meeting choice"),
    }
}

/// Saves where the recording capsule was dragged to: its corner, and the
/// display it landed on (Labs › Indicator on any display; `None` is the
/// primary display). Off, the capsules pass the display they already read,
/// so that field never changes.
pub fn save_meeting_indicator_anchor(corner: MeetingIndicatorCorner, display: Option<&str>) {
    match AppSettings::load() {
        Ok(mut settings) => {
            let display = display.map(str::to_owned);
            if settings.meetings.recording_indicator_corner == corner
                && settings.meetings.recording_indicator_display == display
            {
                return;
            }
            let base = settings.clone();
            settings.meetings.recording_indicator_corner = corner;
            settings.meetings.recording_indicator_display = display;
            if let Err(error) = base.update(&settings) {
                tracing::warn!(%error, "could not save where the recording capsule rests");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unmigrated_file_keeps_audio_retention_only_with_its_lab() {
        use MeetingRetention::Day;
        let read = |json: &str| audio_retention_in(&serde_json::from_str(json).unwrap());
        assert_eq!(read(r#"{"meetings":{"audio_retention":"day"}}"#), None);
        assert_eq!(
            read(r#"{"meetings":{"audio_retention":"day"},"labs":{"expire_meeting_audio":true}}"#),
            Some(Day)
        );
        assert_eq!(
            read(
                r#"{"meetings":{"audio_retention":"day"},"migrations":["meeting-labs-settings"]}"#
            ),
            Some(Day)
        );
        assert_eq!(read(r#"{"migrations":["meeting-labs-settings"]}"#), None);
        assert_eq!(MeetingSettings::default().audio_retention, None);
    }

    /// A file from before the default reads as Ask, so every website or app
    /// without its own answer goes on asking, as it did.
    #[test]
    fn an_older_file_keeps_asking_for_unlisted_calls() {
        let meetings: MeetingSettings = serde_json::from_str(
            r#"{"providers":[{"key":"us.zoom.xos","label":"Zoom","policy":"always_record"}]}"#,
        )
        .unwrap();
        assert_eq!(meetings.default_policy, MeetingPolicy::Ask);
        assert_eq!(meetings.policy_for("example.com"), MeetingPolicy::Ask);
        assert_eq!(
            meetings.policy_for("us.zoom.xos"),
            MeetingPolicy::AlwaysRecord
        );
        assert!(meetings.has_own_answer("us.zoom.xos"));
        assert!(!meetings.has_own_answer("example.com"));
    }

    /// Answers that followed the old default follow the new one; others stay.
    #[test]
    fn changing_the_default_moves_the_answers_that_followed_it() {
        let choice = |key: &str, policy| MeetingProviderChoice {
            key: key.into(),
            label: key.into(),
            policy,
        };
        let mut meetings = MeetingSettings {
            providers: vec![
                choice("us.zoom.xos", MeetingPolicy::Ask),
                choice("meet.google.com", MeetingPolicy::NeverOffer),
            ],
            ..Default::default()
        };
        meetings.set_default_policy(MeetingPolicy::AlwaysRecord);
        assert_eq!(
            meetings.policy_for("us.zoom.xos"),
            MeetingPolicy::AlwaysRecord
        );
        assert!(!meetings.has_own_answer("us.zoom.xos"));
        assert_eq!(
            meetings.policy_for("meet.google.com"),
            MeetingPolicy::NeverOffer
        );
        assert_eq!(
            meetings.policy_for("example.com"),
            MeetingPolicy::AlwaysRecord
        );
        assert_eq!(meetings.providers.len(), 2);
    }

    /// An older settings file has no `recording_indicator_display`: the
    /// capsules stay on the primary display, and the field stays out of the
    /// file until a capsule lands on one.
    #[test]
    fn a_saved_file_without_the_indicator_display_stays_on_the_primary() {
        let settings: MeetingSettings = serde_json::from_str(
            r#"{"show_recording_indicator":true,"recording_indicator_corner":"bottom_left"}"#,
        )
        .unwrap();
        assert_eq!(settings.recording_indicator_display, None);
        assert_eq!(
            settings.recording_indicator_corner,
            MeetingIndicatorCorner::BottomLeft
        );
        let written = serde_json::to_value(&settings).unwrap();
        assert!(written.get("recording_indicator_display").is_none());
        // A display a capsule landed on round trips.
        let landed = MeetingSettings {
            recording_indicator_display: Some(r"\\.\DISPLAY2".into()),
            ..Default::default()
        };
        let written = serde_json::to_value(&landed).unwrap();
        assert_eq!(written["recording_indicator_display"], r"\\.\DISPLAY2");
        let read: MeetingSettings = serde_json::from_value(written).unwrap();
        assert_eq!(
            read.recording_indicator_display,
            landed.recording_indicator_display
        );
    }

    /// A Teams page's old answer under its own host gives way to the Teams
    /// app's new one, which would otherwise never apply to that page.
    #[test]
    fn a_teams_answer_replaces_the_one_saved_for_a_teams_page() {
        let mut meetings = MeetingSettings {
            providers: vec![MeetingProviderChoice {
                key: "teams.live.com".into(),
                label: "teams.live.com".into(),
                policy: MeetingPolicy::NeverOffer,
            }],
            ..Default::default()
        };
        assert!(set_meeting_provider(
            &mut meetings,
            MeetingProviderChoice {
                key: "com.microsoft.teams2".into(),
                label: "Microsoft Teams".into(),
                policy: MeetingPolicy::AlwaysRecord,
            },
        ));
        assert_eq!(meetings.providers.len(), 1);
        assert_eq!(
            meetings.policy_for("com.microsoft.teams2"),
            MeetingPolicy::AlwaysRecord
        );
    }
}

//! Labs: features we're unsure about, especially ones people asked for,
//! each behind its own switch, off by default. There is no master switch:
//! Settings › Labs lists every feature as a card with a toggle, under its
//! group's collapsible header (`Feature::group`). A feature
//! with its own pane (Voice Filter, Playground, the Modes editor)
//! joins the sidebar as a normal item while on and is hidden while off; a
//! behaviour-only feature is just its toggle.
//!
//! Each feature has an audience (`Feature::audience`): `Everyone` ships in
//! every build; `Dev` exists only in dev builds (`dev_build::is_dev_build`:
//! debug, local, test, and dev-channel builds). A stable build never lists,
//! accepts, or runs a Dev feature, and keeps its saved setting untouched.
//!
//! Gate a feature where its work starts (the worker, thread, or capture
//! that does it), not only where its UI shows.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, RwLock};

use crate::app_settings::{
    AppSettings, AppearanceTheme, CapsLockLight, DictationStyle, HUD_BOTTOM_GAP_DEFAULT,
    HUD_BOTTOM_GAP_MAX, LabsSettings,
};

mod card;
mod cli;
mod description;
mod feature;
mod gates;
mod group;
#[cfg(test)]
mod web;

pub use self::cli::{LabsCommand, run, run_retired_experiments};
pub use self::feature::Feature;
pub use self::gates::*;
pub use self::group::Group;

/// Where people send feedback on Labs features.
pub const FEEDBACK_URL: &str = "https://github.com/anomalyco/hex/issues/new";

/// Who a Labs feature is for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Audience {
    /// Ships in every build.
    Everyone,
    /// Only dev builds have it.
    Dev,
}

/// An OS Hex runs on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Os {
    Macos,
    Windows,
    Linux,
}

impl Os {
    /// The OS this build runs on.
    pub const CURRENT: Self = if cfg!(target_os = "macos") {
        Self::Macos
    } else if cfg!(target_os = "windows") {
        Self::Windows
    } else {
        Self::Linux
    };
}

impl Audience {
    /// Whether a build of this kind has features for it.
    pub const fn reaches(self, dev_build: bool) -> bool {
        match self {
            Self::Everyone => true,
            Self::Dev => dev_build,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Everyone => "everyone",
            Self::Dev => "dev",
        }
    }
}

impl Feature {
    /// The features this OS and build have.
    pub fn available() -> impl Iterator<Item = Self> {
        Self::available_where(Self::shipped)
    }

    /// The features on this OS that `reaches` admits.
    fn available_where(reaches: impl Fn(Self) -> bool) -> impl Iterator<Item = Self> {
        Self::ALL
            .into_iter()
            .filter(move |feature| feature.on_this_os() && reaches(*feature))
    }

    /// Whether this build has the feature: its audience reaches a dev or
    /// stable build.
    pub fn shipped(self) -> bool {
        self.audience().reaches(crate::dev_build::is_dev_build())
    }

    /// The managed lock that holds this feature off, if any.
    pub fn managed(self) -> Option<crate::policy::Lock> {
        crate::policy::Policy::current().labs_lock(self)
    }

    /// Whether `settings` turn this feature on in this build and OS under
    /// `policy`, without reading the process's runtime switch bank.
    pub(crate) fn enabled_under(
        self,
        settings: &AppSettings,
        policy: crate::policy::Policy,
    ) -> bool {
        self.on(settings) && self.shipped() && self.on_this_os() && policy.labs_lock(self).is_none()
    }
}

/// Number of 64-bit words needed to hold one bit per [`Feature`].
const BANK_WORDS: usize = Feature::ALL.len().div_ceil(64);

/// Encoded runtime switches: one bit per [`Feature`], indexed by
/// `feature as usize`, with build audience, OS availability, and managed
/// policy already applied.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SwitchBank([u64; BANK_WORDS]);

impl SwitchBank {
    const fn bit(feature: Feature) -> (usize, u64) {
        let index = feature as usize;
        (index / 64, 1u64 << (index % 64))
    }

    fn set(&mut self, feature: Feature) {
        let (word, mask) = Self::bit(feature);
        self.0[word] |= mask;
    }

    #[cfg(test)]
    const fn is_on(self, feature: Feature) -> bool {
        let (word, mask) = Self::bit(feature);
        self.0[word] & mask != 0
    }
}

/// This process's runtime switch bank: one bit per [`Feature`], written by
/// [`apply`] or lazy-loaded from disk on first read.
static BANK: [AtomicU64; BANK_WORDS] = [const { AtomicU64::new(0) }; BANK_WORDS];

/// True once [`BANK`] and its companion payloads hold loaded or applied
/// settings.
static LOADED: AtomicBool = AtomicBool::new(false);

/// Serializes [`apply`] and the one-time lazy disk load so payloads and bank
/// words are published in order without racing each other. Hot-path reads
/// never take this lock.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Clamped minutes for Labs › Free memory when idle, paired with
/// `Feature::UnloadIdle` in [`BANK`].
static UNLOAD_IDLE_MINUTES: AtomicU64 = AtomicU64::new(15);

/// The seconds a capture's microphone keeps open after it ends, paired with
/// `Feature::KeepMicrophoneOpen` in [`BANK`]: 0 turns it off.
static MICROPHONE_LINGER_SECONDS: AtomicU64 = AtomicU64::new(0);

/// Labs › HUD height's gap in points above the Dock, paired with
/// `Feature::HudHeight` in [`BANK`].
static HUD_BOTTOM_GAP: AtomicU64 = AtomicU64::new(HUD_BOTTOM_GAP_DEFAULT as u64);

/// Whether Caps Lock dictation's recording light blinks slowly instead of
/// staying solid.
static CAPS_LOCK_SLOW_BLINK: AtomicBool = AtomicBool::new(false);

/// None while YOLO mode is off; an empty app list sends everywhere.
static YOLO_MODE_APPS: RwLock<Option<Vec<String>>> = RwLock::new(None);

/// The words that start a voice prompt. An invalid saved phrase reads as the
/// default.
static VOICE_PROMPT_PHRASE: LazyLock<RwLock<String>> =
    LazyLock::new(|| RwLock::new(crate::dictation::voice_prompt::DEFAULT_PHRASE.to_owned()));

/// The words that finish a hands-free dictation. An invalid saved phrase reads
/// as the default.
static STOP_PHRASE_TEXT: LazyLock<RwLock<String>> =
    LazyLock::new(|| RwLock::new(crate::dictation::stop_phrase::DEFAULT_PHRASE.to_owned()));

/// The signature appended to each normal dictation. Empty appends nothing; an
/// invalid saved text reads as the default.
static SIGNATURE_TEXT: LazyLock<RwLock<String>> =
    LazyLock::new(|| RwLock::new(crate::dictation::signature::DEFAULT_TEXT.to_owned()));

/// The words Labs › Clipboard phrase replaces with the clipboard. An invalid
/// saved phrase reads as the default.
static CLIPBOARD_PHRASE_TEXT: LazyLock<RwLock<String>> =
    LazyLock::new(|| RwLock::new(crate::dictation::clipboard_phrase::DEFAULT_PHRASE.to_owned()));

/// The style each normal dictation is written as, read only while
/// `Feature::DictationStyle` is on.
static DICTATION_STYLE_CHOICE: LazyLock<RwLock<DictationStyle>> =
    LazyLock::new(|| RwLock::new(DictationStyle::Polished));

/// The app or output a meeting's computer audio records, read only while
/// `Feature::MeetingAudioSource` is on.
static MEETING_AUDIO_SOURCE_CHOICE: LazyLock<RwLock<crate::meeting::audio_source::SourceChoice>> =
    LazyLock::new(|| RwLock::new(crate::meeting::audio_source::SourceChoice::All));

pub(super) fn ensure_loaded() {
    if LOADED.load(Ordering::Acquire) {
        return;
    }
    load_from_disk();
}

#[cold]
fn load_from_disk() {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    if LOADED.load(Ordering::Acquire) {
        return;
    }
    let saved = AppSettings::saved_labs();
    store_all(&saved);
    LOADED.store(true, Ordering::Release);
}

pub(super) fn feature_on(feature: Feature) -> bool {
    ensure_loaded();
    let (word, mask) = SwitchBank::bit(feature);
    BANK[word].load(Ordering::Acquire) & mask != 0
}

/// The idle-unload minutes a saved setting names, clamped to 1..=1440.
fn unload_idle_minutes(settings: &LabsSettings) -> u64 {
    u64::from(settings.unload_idle_minutes.clamp(1, 24 * 60))
}

/// The HUD bottom gap a saved setting names, clamped to the offered range.
fn hud_gap_points(settings: &LabsSettings) -> u64 {
    u64::from(settings.hud_bottom_gap.min(HUD_BOTTOM_GAP_MAX))
}

/// The window a saved setting holds, in seconds; 0 when it's off.
fn linger_seconds(settings: &LabsSettings) -> u64 {
    settings
        .keep_microphone_open_for()
        .map_or(0, |window| window.as_secs())
}

/// Records the trigger phrase matching reads; an invalid value never turns
/// matching off.
fn store_voice_prompt_phrase(phrase: &str) {
    *VOICE_PROMPT_PHRASE
        .write()
        .unwrap_or_else(|error| error.into_inner()) =
        crate::dictation::voice_prompt::accepted_phrase(phrase);
}

/// Records the phrase that finishes a hands-free dictation; an invalid value
/// never turns matching off.
fn store_stop_phrase_text(phrase: &str) {
    *STOP_PHRASE_TEXT
        .write()
        .unwrap_or_else(|error| error.into_inner()) =
        crate::dictation::voice_prompt::accepted_phrase_or(
            phrase,
            crate::dictation::stop_phrase::DEFAULT_PHRASE,
        );
}

/// Records the signature each normal dictation gets; invalid text reads as
/// the default, and empty stays empty so it appends nothing.
fn store_signature_text(signature: &str) {
    let signature = signature.trim();
    let accepted = if crate::dictation::signature::text_problem(signature).is_none() {
        signature
    } else {
        crate::dictation::signature::DEFAULT_TEXT
    };
    *SIGNATURE_TEXT
        .write()
        .unwrap_or_else(|error| error.into_inner()) = accepted.to_owned();
}

/// Records the words replaced with the clipboard; an invalid value never
/// turns matching off.
fn store_clipboard_phrase_text(phrase: &str) {
    *CLIPBOARD_PHRASE_TEXT
        .write()
        .unwrap_or_else(|error| error.into_inner()) =
        crate::dictation::voice_prompt::accepted_phrase_or(
            phrase,
            crate::dictation::clipboard_phrase::DEFAULT_PHRASE,
        );
}

/// Records the style each normal dictation is written as. An unknown value
/// never reaches here (serde rejects it on load).
fn store_dictation_style_choice(style: DictationStyle) {
    *DICTATION_STYLE_CHOICE
        .write()
        .unwrap_or_else(|error| error.into_inner()) = style;
}

/// Records the app or output a meeting's computer audio records; a malformed
/// saved value never reaches here (it loads as all computer audio).
fn store_meeting_audio_source_choice(choice: crate::meeting::audio_source::SourceChoice) {
    *MEETING_AUDIO_SOURCE_CHOICE
        .write()
        .unwrap_or_else(|error| error.into_inner()) = choice;
}

fn store_all(settings: &LabsSettings) {
    let policy = crate::policy::Policy::current();
    let reaches = |feature: Feature| {
        feature.shipped() && feature.on_this_os() && policy.labs_lock(feature).is_none()
    };
    let bank = encode_where(settings, reaches);
    // Publish payloads before bank bits so any reader observing a bit enabled
    // sees the matching companion value.
    UNLOAD_IDLE_MINUTES.store(unload_idle_minutes(settings), Ordering::Release);
    MICROPHONE_LINGER_SECONDS.store(linger_seconds(settings), Ordering::Release);
    HUD_BOTTOM_GAP.store(hud_gap_points(settings), Ordering::Release);
    CAPS_LOCK_SLOW_BLINK.store(
        settings.caps_lock_light == CapsLockLight::SlowBlink && reaches(Feature::CapsLockDictation),
        Ordering::Release,
    );
    *YOLO_MODE_APPS
        .write()
        .unwrap_or_else(|error| error.into_inner()) =
        (settings.yolo_mode && reaches(Feature::YoloMode)).then(|| settings.yolo_mode_apps.clone());
    store_voice_prompt_phrase(&settings.voice_prompt_phrase);
    store_stop_phrase_text(&settings.stop_phrase_text);
    store_signature_text(&settings.signature_text);
    store_clipboard_phrase_text(&settings.clipboard_phrase_text);
    store_dictation_style_choice(settings.dictation_style_choice);
    store_meeting_audio_source_choice(settings.meeting_audio_source_choice.clone());
    for (slot, word) in BANK.iter().zip(bank.0) {
        slot.store(word, Ordering::Release);
    }
}

/// Whether the main window draws dark: the OS's appearance, unless Labs ›
/// Appearance is on and names one. Resolved for each view, so System keeps
/// following the OS live. Overlay windows (the HUD, capsules, live text) keep
/// following the OS themselves.
pub fn window_dark(labs: &LabsSettings, system_dark: bool) -> bool {
    if !Feature::Appearance.shipped() || !labs.appearance {
        return system_dark;
    }
    match labs.appearance_theme {
        AppearanceTheme::System => system_dark,
        AppearanceTheme::Light => false,
        AppearanceTheme::Dark => true,
    }
}

/// Records the switches the loaded or saved settings hold.
pub(crate) fn apply(settings: &LabsSettings) {
    {
        let _guard = WRITE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        store_all(settings);
        LOADED.store(true, Ordering::Release);
    }
    // Labs › Pause from menu bar: turning it off resumes at once, without
    // waiting for the menu to refresh; the menu row hides itself there.
    crate::shortcut_pause::reconcile(pause_menu());
    // Labs › Clean up what I say: turning it off frees the loaded model.
    crate::dictation::cleanup::reconcile(cleanup_tagger());
}

#[cfg(test)]
fn encode(settings: &LabsSettings) -> SwitchBank {
    let policy = crate::policy::Policy::current();
    encode_where(settings, |feature| {
        feature.shipped() && feature.on_this_os() && policy.labs_lock(feature).is_none()
    })
}

/// The switches `settings` hold for the features `reaches` admits; the
/// rest read off while their saved setting stays as it is.
fn encode_where(settings: &LabsSettings, reaches: impl Fn(Feature) -> bool) -> SwitchBank {
    let mut bank = SwitchBank::default();
    for feature in Feature::ALL {
        if feature.on_labs(settings) && reaches(feature) {
            bank.set(feature);
        }
    }
    bank
}

/// The names of the features on, for `hex status`.
pub fn active_names(settings: &AppSettings) -> Vec<String> {
    Feature::available()
        .filter(|feature| feature.on(settings))
        .map(|feature| feature.name().into())
        .collect()
}

/// Logs the switches to process.log; call once logging is up in a long-lived Hex.
pub fn log_launch() {
    tracing::info!(
        live_text = live_text(),
        failure_notices = failure_notices(),
        live_typing = live_typing(),
        live_chapters = live_chapters(),
        typed_text = typed_text(),
        playground = playground(),
        screen_vocabulary = screen_vocabulary(),
        learn_from_corrections = learn_from_corrections(),
        cursor_context = cursor_context(),
        copy_dictation_audio = copy_dictation_audio(),
        hold_through_keys = hold_through_keys(),
        confirm_long_cancels = confirm_long_cancels(),
        "labs"
    );
}

#[cfg(test)]
mod tests {
    use super::cli::resolve;
    use super::*;

    #[test]
    fn graduated_features_answer_by_name() {
        let error = resolve("meetings", |_| true).unwrap_err().to_string();
        assert!(error.contains("graduated"), "{error}");
        assert_eq!(Feature::from_name("meetings"), None);
        let error = resolve("custom-models", |_| true).unwrap_err().to_string();
        assert!(error.contains("graduated"), "{error}");
        assert_eq!(Feature::from_name("custom-models"), None);
        let error = resolve("meeting-platform-icons", |_| true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("graduated"), "{error}");
        assert_eq!(Feature::from_name("meeting-platform-icons"), None);
        // Yearly stats is part of Dictation stats now.
        let error = resolve("yearly-stats", |_| true).unwrap_err().to_string();
        assert!(error.contains("history-stats"), "{error}");
        assert_eq!(Feature::from_name("yearly-stats"), None);
    }

    #[test]
    fn every_feature_is_off_by_default_and_round_trips() {
        let mut settings = AppSettings::default();
        for feature in Feature::ALL {
            assert!(
                !feature.on(&settings),
                "{} is on by default",
                feature.name()
            );
            assert_eq!(Feature::from_name(feature.name()), Some(feature));
            feature.set(&mut settings, true);
            assert!(feature.on(&settings));
        }
    }

    #[test]
    fn labs_lock_holds_every_feature_and_app_model_switching_off() {
        let mut settings = AppSettings::default();
        for feature in Feature::ALL {
            feature.set_field(&mut settings, true);
            assert!(feature.on(&settings), "{} has no switch", feature.name());
        }
        settings.labs.app_model_switching = true;
        crate::policy::Policy::default()
            .with(crate::policy::Lock::Labs)
            .enforce(&mut settings);
        for feature in Feature::ALL {
            assert!(
                !feature.on(&settings),
                "{} escaped Lock::Labs",
                feature.name()
            );
        }
        assert!(!settings.labs.app_model_switching);
    }

    #[test]
    fn copy_dictation_audio_requires_both_switches_and_no_policy_lock() {
        let none = crate::policy::Policy::default();
        let locked = none.with(crate::policy::Lock::Labs);
        for (keep, copy) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut settings = AppSettings::default();
            settings.labs.keep_recordings = keep;
            settings.labs.copy_dictation_audio = copy;
            let expected = keep && copy;
            assert_eq!(
                copy_dictation_audio_enabled_under(&settings, none),
                expected,
                "keep={keep}, copy={copy}"
            );
            assert!(
                !copy_dictation_audio_enabled_under(&settings, locked),
                "Lock::Labs must hold copy_dictation_audio off"
            );
            let bank = encode(&settings.labs);
            assert_eq!(
                bank.is_on(Feature::CopyDictationAudio) && bank.is_on(Feature::KeepRecordings),
                expected
            );
        }
    }

    #[test]
    fn history_stats_strip_requires_dictation_stats_and_no_policy_lock() {
        let none = crate::policy::Policy::default();
        let locked = none.with(crate::policy::Lock::Labs);
        for (stats, strip) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut settings = AppSettings::default();
            settings.labs.history_stats = stats;
            settings.labs.history_stats_strip = strip;
            let expected = stats && strip;
            assert_eq!(
                history_stats_strip_enabled_under(&settings, none),
                expected,
                "stats={stats}, strip={strip}"
            );
            assert!(
                !history_stats_strip_enabled_under(&settings, locked),
                "Lock::Labs must hold the strip off"
            );
        }
    }

    #[test]
    fn an_audience_reaches_the_builds_it_names() {
        assert!(Audience::Everyone.reaches(false));
        assert!(Audience::Everyone.reaches(true));
        assert!(Audience::Dev.reaches(true));
        assert!(!Audience::Dev.reaches(false));
    }

    #[test]
    fn the_developer_tools_are_dev_only() {
        for feature in [
            Feature::Playground,
            Feature::VoiceFilter,
            Feature::PasteLastPicker,
        ] {
            assert_eq!(feature.audience(), Audience::Dev, "{}", feature.name());
            assert!(resolve(feature.name(), |f| f.audience().reaches(false)).is_err());
        }
        let labs = LabsSettings {
            playground: true,
            ..LabsSettings::default()
        };
        assert!(!encode_where(&labs, |f| f.audience().reaches(false)).is_on(Feature::Playground));
    }

    /// Stands in for a Dev feature in a stable build.
    fn stable(feature: Feature) -> bool {
        feature != Feature::LiveText
    }

    #[test]
    fn a_dev_feature_in_a_stable_build_is_unlisted_refused_and_off() {
        assert!(Feature::available_where(stable).all(|feature| feature != Feature::LiveText));
        assert!(Feature::available_where(|_| true).any(|feature| feature == Feature::LiveText));
        assert!(resolve("live-text", stable).is_err());
        assert!(resolve("filler-words", stable).is_ok());
        let labs = LabsSettings {
            live_text: true,
            filler_words: true,
            ..LabsSettings::default()
        };
        let bank = encode_where(&labs, stable);
        assert!(!bank.is_on(Feature::LiveText));
        assert!(bank.is_on(Feature::FillerWords));
        assert!(encode_where(&labs, |_| true).is_on(Feature::LiveText));
        assert!(labs.live_text, "the saved setting is kept");
    }

    #[test]
    fn settings_keys_name_their_feature() {
        for feature in Feature::ALL {
            assert_eq!(
                Feature::for_settings_key(feature.settings_key()),
                Some(feature)
            );
            for extra in feature.extra_settings_keys() {
                assert_eq!(Feature::for_settings_key(extra), Some(feature));
            }
        }
        assert_eq!(Feature::for_settings_key("dictation_hotkey"), None);
    }

    #[test]
    fn the_vocabulary_menu_is_a_macos_dictation_lab_off_by_default() {
        let feature = Feature::VocabularyMenu;
        assert_eq!(feature.name(), "vocabulary-menu");
        assert_eq!(feature.group(), Group::Dictation);
        assert_eq!(feature.audience(), Audience::Everyone);
        assert_eq!(feature.oses(), &[Os::Macos]);
        assert_eq!(feature.added(), 20261123);
        assert!(!feature.on(&AppSettings::default()));
        assert_eq!(feature.on_this_os(), cfg!(target_os = "macos"));
        assert_eq!(Feature::from_name("vocabulary-menu"), Some(feature));
    }

    #[test]
    fn the_cleanup_tagger_is_a_macos_output_lab_off_by_default() {
        let feature = Feature::CleanupTagger;
        assert_eq!(feature.name(), "clean-up-what-i-say");
        assert_eq!(feature.settings_key(), "labs.cleanup_tagger");
        assert_eq!(feature.group(), Group::Output);
        assert_eq!(feature.audience(), Audience::Everyone);
        assert_eq!(feature.oses(), &[Os::Macos]);
        assert_eq!(feature.added(), 20261126);
        assert!(!feature.on(&AppSettings::default()));
        assert_eq!(feature.on_this_os(), cfg!(target_os = "macos"));
        assert_eq!(Feature::from_name("clean-up-what-i-say"), Some(feature));
    }

    #[test]
    fn the_middle_mouse_dictation_lab_is_a_macos_shortcuts_lab_off_by_default() {
        let feature = Feature::MiddleMouseDictation;
        assert_eq!(feature.name(), "middle-mouse-dictation");
        assert_eq!(feature.group(), Group::Shortcuts);
        assert_eq!(feature.audience(), Audience::Everyone);
        assert_eq!(feature.oses(), &[Os::Macos]);
        assert_eq!(feature.added(), 20261125);
        assert!(!feature.on(&AppSettings::default()));
        assert_eq!(feature.on_this_os(), cfg!(target_os = "macos"));
        assert_eq!(Feature::from_name("middle-mouse-dictation"), Some(feature));
    }

    #[test]
    fn the_speaker_names_lab_is_a_macos_meetings_lab_off_by_default() {
        let feature = Feature::MeetingSpeakerNames;
        assert_eq!(feature.name(), "speaker-names");
        assert_eq!(feature.settings_key(), "labs.meeting_speaker_names");
        assert_eq!(feature.title(), "Speaker names");
        assert_eq!(feature.group(), Group::Meetings);
        assert_eq!(feature.audience(), Audience::Everyone);
        assert_eq!(feature.oses(), &[Os::Macos]);
        assert_eq!(feature.added(), 20261127);
        assert!(!feature.on(&AppSettings::default()));
        assert_eq!(feature.on_this_os(), cfg!(target_os = "macos"));
        assert_eq!(Feature::from_name("speaker-names"), Some(feature));
    }

    #[test]
    fn the_meeting_microphone_mute_lab_is_a_macos_meetings_lab_off_by_default() {
        let feature = Feature::MeetingMicrophoneMute;
        assert_eq!(feature.name(), "meeting-microphone-mute");
        assert_eq!(feature.group(), Group::Meetings);
        assert_eq!(feature.audience(), Audience::Everyone);
        assert_eq!(feature.oses(), &[Os::Macos]);
        assert_eq!(feature.added(), 20261124);
        assert!(!feature.on(&AppSettings::default()));
        assert_eq!(feature.on_this_os(), cfg!(target_os = "macos"));
        assert_eq!(Feature::from_name("meeting-microphone-mute"), Some(feature));
    }

    #[test]
    fn the_meeting_indicator_display_lab_is_a_desktop_meetings_lab_off_by_default() {
        let feature = Feature::MeetingIndicatorDisplay;
        assert_eq!(feature.name(), "meeting-indicator-display");
        assert_eq!(feature.group(), Group::Meetings);
        assert_eq!(feature.audience(), Audience::Everyone);
        assert_eq!(feature.oses(), &[Os::Macos, Os::Windows]);
        assert_eq!(feature.added(), 20261124);
        assert!(!feature.on(&AppSettings::default()));
        assert_eq!(
            feature.on_this_os(),
            cfg!(any(target_os = "macos", target_os = "windows"))
        );
        assert_eq!(
            Feature::from_name("meeting-indicator-display"),
            Some(feature)
        );
    }

    #[test]
    fn the_choose_meeting_audio_lab_ships_on_every_os_and_is_off_by_default() {
        let feature = Feature::MeetingAudioSource;
        assert_eq!(feature.name(), "meeting-audio-source");
        assert_eq!(feature.group(), Group::Meetings);
        assert_eq!(feature.audience(), Audience::Everyone);
        assert_eq!(feature.oses(), &[Os::Macos, Os::Windows, Os::Linux]);
        assert_eq!(feature.added(), 20261124);
        assert!(!feature.on(&AppSettings::default()));
        assert!(feature.on_this_os());
        assert_eq!(
            Feature::from_name("meeting-audio-source"),
            Some(feature),
            "hex labs on meeting-audio-source takes its CLI name"
        );
        assert_eq!(
            feature.extra_settings_keys(),
            ["labs.meeting_audio_source_choice"]
        );
    }

    #[test]
    fn the_stop_any_call_recording_lab_ships_on_every_os_and_is_off_by_default() {
        let feature = Feature::StopAnyCallRecording;
        assert_eq!(feature.name(), "stop-any-call-recording");
        assert_eq!(feature.group(), Group::Meetings);
        assert_eq!(feature.audience(), Audience::Everyone);
        assert_eq!(feature.oses(), &[Os::Macos, Os::Windows, Os::Linux]);
        assert!(!feature.on(&AppSettings::default()));
        assert!(feature.on_this_os());
        assert_eq!(Feature::from_name("stop-any-call-recording"), Some(feature));
    }

    #[test]
    fn every_labs_feature_encodes_independently_in_the_switch_bank() {
        let default_bank = encode_where(&LabsSettings::default(), |_| true);
        for feature in Feature::ALL {
            assert!(
                !default_bank.is_on(feature),
                "{} is set in the default bank",
                feature.name()
            );
        }

        for feature in Feature::ALL {
            let mut settings = AppSettings::default();
            feature.set(&mut settings, true);
            let stored_in_labs = feature.on_labs(&settings.labs);
            let unrestricted = encode_where(&settings.labs, |_| true);
            for other in Feature::ALL {
                let expected = stored_in_labs && other == feature;
                assert_eq!(
                    unrestricted.is_on(other),
                    expected,
                    "toggling {} unexpectedly changed {} in the bank",
                    feature.name(),
                    other.name()
                );
            }
            let encoded = encode(&settings.labs);
            let expected_live = stored_in_labs
                && feature.shipped()
                && feature.on_this_os()
                && feature.managed().is_none();
            assert_eq!(
                encoded.is_on(feature),
                expected_live,
                "encode() disagreed with shipped/on_this_os/managed for {}",
                feature.name()
            );
        }
    }

    #[test]
    fn encode_blocks_features_from_other_oses() {
        let labs = LabsSettings {
            notch_indicator: true,
            opencode_in_wsl: true,
            typed_text: true,
            live_text: true,
            ..LabsSettings::default()
        };
        let bank = encode(&labs);
        assert!(bank.is_on(Feature::LiveText));
        assert_eq!(
            bank.is_on(Feature::NotchIndicator),
            cfg!(target_os = "macos")
        );
        assert_eq!(
            bank.is_on(Feature::OpenCodeInWsl),
            cfg!(target_os = "windows")
        );
        assert_eq!(bank.is_on(Feature::TypedText), cfg!(target_os = "linux"));
    }

    #[test]
    fn live_chapters_ships_on_every_os() {
        assert_eq!(
            Feature::LiveChapters.oses(),
            &[Os::Macos, Os::Windows, Os::Linux]
        );
        assert!(Feature::LiveChapters.on_this_os());
    }

    #[test]
    fn unload_idle_clamps_its_minutes() {
        let mut labs = LabsSettings::default();
        assert_eq!(labs.unload_idle_minutes, 15);
        assert_eq!(unload_idle_minutes(&labs), 15);
        labs.unload_idle_minutes = 0;
        assert_eq!(unload_idle_minutes(&labs), 1);
        labs.unload_idle_minutes = 30;
        assert_eq!(unload_idle_minutes(&labs), 30);
        labs.unload_idle_minutes = 10_000;
        assert_eq!(unload_idle_minutes(&labs), 24 * 60);
    }

    #[test]
    fn keep_microphone_open_carries_its_window_and_rejects_unknown_choices() {
        let mut labs = LabsSettings::default();
        assert_eq!(
            labs.keep_microphone_open_seconds,
            LabsSettings::DEFAULT_KEEP_MICROPHONE_OPEN_SECONDS
        );
        assert_eq!(labs.keep_microphone_open_for(), None);
        assert_eq!(linger_seconds(&labs), 0);

        labs.keep_microphone_open = true;
        labs.keep_microphone_open_seconds = 180;
        assert_eq!(
            labs.keep_microphone_open_for(),
            Some(std::time::Duration::from_secs(180))
        );
        assert_eq!(linger_seconds(&labs), 180);

        labs.keep_microphone_open_seconds = 45;
        assert_eq!(labs.keep_microphone_open_for(), None);
        assert_eq!(linger_seconds(&labs), 0);
    }

    #[test]
    fn hud_gap_clamps_to_offered_maximum() {
        let mut labs = LabsSettings::default();
        assert_eq!(labs.hud_bottom_gap, HUD_BOTTOM_GAP_DEFAULT);
        assert_eq!(hud_gap_points(&labs), u64::from(HUD_BOTTOM_GAP_DEFAULT));
        labs.hud_bottom_gap = HUD_BOTTOM_GAP_MAX + 120;
        assert_eq!(hud_gap_points(&labs), u64::from(HUD_BOTTOM_GAP_MAX));
    }

    #[test]
    fn the_window_follows_the_os_unless_appearance_forces_it() {
        let labs = LabsSettings::default();
        assert!(window_dark(&labs, true));
        assert!(!window_dark(&labs, false));
        let appearance = |theme| LabsSettings {
            appearance: true,
            appearance_theme: theme,
            ..LabsSettings::default()
        };
        assert!(window_dark(&appearance(AppearanceTheme::System), true));
        assert!(!window_dark(&appearance(AppearanceTheme::System), false));
        assert!(!window_dark(&appearance(AppearanceTheme::Light), true));
        assert!(window_dark(&appearance(AppearanceTheme::Dark), false));
    }

    #[test]
    fn companion_defaults_match_their_domains() {
        let labs = LabsSettings::default();
        assert_eq!(labs.caps_lock_light, CapsLockLight::Steady);
        assert_eq!(labs.dictation_style_choice, DictationStyle::Polished);
        assert_eq!(
            labs.voice_prompt_phrase,
            crate::dictation::voice_prompt::DEFAULT_PHRASE
        );
        assert_eq!(
            labs.stop_phrase_text,
            crate::dictation::stop_phrase::DEFAULT_PHRASE
        );
        assert_eq!(
            labs.signature_text,
            crate::dictation::signature::DEFAULT_TEXT
        );
        assert_eq!(
            labs.clipboard_phrase_text,
            crate::dictation::clipboard_phrase::DEFAULT_PHRASE
        );
    }
}

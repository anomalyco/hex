use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::context::ContextSnapshot;
use crate::microphone_activity::ActiveMicrophoneApplication;

const STABLE_OBSERVATIONS: u8 = 4;
/// A browser using the microphone on a page that is not a known call site
/// must keep it for about 20 s: voice notes and dictation fields open the
/// microphone for a few seconds, calls hold it for minutes.
const BROWSER_STABLE_OBSERVATIONS: u8 = 40;
/// An offered call stays snoozed until its app has been off the microphone
/// for about a minute of 500 ms observations, so one call is offered once.
const ABSENT_OBSERVATIONS_TO_RESET: u8 = 120;
/// About three seconds without the call app on the microphone.
const CALL_ENDED_OBSERVATIONS: u8 = 6;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MeetingCandidate {
    pub key: String,
    pub title: String,
    pub source: MeetingSource,
    /// Canonical bundle identifier of the application using the microphone.
    #[serde(default)]
    pub application: Option<String>,
    /// Whether `title` names this meeting rather than its app.
    #[serde(default)]
    pub named: bool,
    /// Website host of the browser tab, when the browser adapter knows it.
    #[serde(default)]
    pub site: Option<String>,
    /// Display name of the application using the microphone.
    #[serde(default)]
    pub application_name: Option<String>,
}

impl MeetingCandidate {
    pub fn manual() -> Self {
        Self {
            key: "manual".into(),
            title: MeetingSource::Manual.label().into(),
            source: MeetingSource::Manual,
            application: None,
            named: false,
            site: None,
            application_name: None,
        }
    }

    pub fn recording_start(&self) -> crate::meeting::MeetingStart {
        crate::meeting::MeetingStart {
            title: self.named.then(|| self.title.clone()),
            app: self.app_label(),
        }
    }

    /// What the call is in: the service, else the website, else the browser.
    fn app_label(&self) -> Option<String> {
        match self.source {
            MeetingSource::Manual => None,
            MeetingSource::BrowserCall => {
                self.site.clone().or_else(|| self.application_name.clone())
            }
            source => Some(source.label().into()),
        }
    }

    /// The offer capsule's headline.
    pub fn offer_title(&self) -> String {
        if self.named {
            return self.title.clone();
        }
        match self.source {
            MeetingSource::BrowserCall => match (&self.site, &self.application_name) {
                (Some(site), _) => format!("Call on {site}"),
                (None, Some(application)) => format!("Call in {application}"),
                (None, None) => "Call detected".into(),
            },
            MeetingSource::SlackHuddle => "Slack huddle detected".into(),
            source => format!("{} call detected", source.label()),
        }
    }

    /// What "Never offer" silences: the website when known, else the app.
    pub fn never_offer_target(&self) -> Option<crate::app_settings::NeverOfferMeeting> {
        if let Some(site) = &self.site {
            return Some(crate::app_settings::NeverOfferMeeting {
                key: site.clone(),
                label: site.clone(),
            });
        }
        let application = self.application.clone()?;
        Some(crate::app_settings::NeverOfferMeeting {
            label: self
                .application_name
                .clone()
                .unwrap_or_else(|| application.clone()),
            key: application,
        })
    }

    /// Whether the user asked never to be offered this site or app.
    pub fn is_silenced(&self, keys: &[String]) -> bool {
        keys.iter().any(|key| {
            self.site.as_deref() == Some(key.as_str())
                || self.application.as_deref() == Some(key.as_str())
        })
    }

    fn required_observations(&self) -> u8 {
        match self.source {
            MeetingSource::BrowserCall => BROWSER_STABLE_OBSERVATIONS,
            _ => STABLE_OBSERVATIONS,
        }
    }
}

/// Notices when the call application behind a recording stops using the
/// microphone, so the user can choose to stop. It never stops anything.
#[derive(Default)]
pub struct CallPresence {
    absent: u8,
    prompted: bool,
}

impl CallPresence {
    /// Returns true once per absence that lasts long enough.
    pub fn observe(&mut self, present: bool) -> bool {
        if present {
            self.absent = 0;
            self.prompted = false;
            return false;
        }
        self.absent = self.absent.saturating_add(1);
        if self.absent >= CALL_ENDED_OBSERVATIONS && !self.prompted {
            self.prompted = true;
            return true;
        }
        false
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingSource {
    Manual,
    GoogleMeet,
    Zoom,
    Teams,
    SlackHuddle,
    FaceTime,
    BrowserCall,
}

impl MeetingSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Meeting",
            Self::GoogleMeet => "Google Meet",
            Self::Zoom => "Zoom",
            Self::Teams => "Microsoft Teams",
            Self::SlackHuddle => "Slack Huddle",
            Self::FaceTime => "FaceTime",
            Self::BrowserCall => "Browser call",
        }
    }
}

#[derive(Default)]
pub struct MeetingDetector {
    pending: Option<(String, u8)>,
    offered: Option<String>,
    offered_application: Option<String>,
    absent_observations: u8,
}

impl MeetingDetector {
    pub fn is_application_snoozed(&self, canonical_bundle_id: &str) -> bool {
        self.offered_application.as_deref() == Some(canonical_bundle_id)
    }

    pub fn observe(&mut self, candidate: Option<MeetingCandidate>) -> Option<MeetingCandidate> {
        let Some(candidate) = candidate else {
            self.pending = None;
            self.absent_observations = self.absent_observations.saturating_add(1);
            if self.absent_observations >= ABSENT_OBSERVATIONS_TO_RESET {
                self.offered = None;
                self.offered_application = None;
            }
            return None;
        };
        self.absent_observations = 0;
        if self.offered.as_deref() == Some(&candidate.key)
            || (candidate.application.is_some()
                && self.offered_application == candidate.application)
        {
            self.pending = None;
            return None;
        }
        let observations = match &mut self.pending {
            Some((key, count)) if key == &candidate.key => {
                *count = count.saturating_add(1);
                *count
            }
            _ => {
                self.pending = Some((candidate.key.clone(), 1));
                1
            }
        };
        if observations < candidate.required_observations() {
            return None;
        }
        self.pending = None;
        self.offered = Some(candidate.key.clone());
        self.offered_application = candidate.application.clone();
        Some(candidate)
    }
}

pub fn candidate_from_context(context: &ContextSnapshot) -> Option<MeetingCandidate> {
    let application = context.application.as_deref()?;
    let application_lower = application.to_ascii_lowercase();
    let title = context.window_title.as_deref().unwrap_or("").trim();
    let title_lower = title.to_ascii_lowercase();

    if is_browser_application(&application_lower)
        && let Some(url) = &context.browser_url
        && is_google_meet_call(url)
    {
        return Some(named(candidate(
            MeetingSource::GoogleMeet,
            url.as_str(),
            &clean_google_meet_title(title),
        )));
    }
    if application_lower.contains("zoom")
        && (title_lower.contains("meeting") || title_lower.contains("webinar"))
    {
        return Some(candidate(MeetingSource::Zoom, title, title));
    }
    if application_lower.contains("teams")
        && (title_lower.contains("meeting") || title_lower.contains("call"))
    {
        return Some(candidate(MeetingSource::Teams, title, title));
    }
    if application_lower == "slack" && title_lower.contains("huddle") {
        return Some(candidate(MeetingSource::SlackHuddle, title, title));
    }
    if application_lower == "facetime" && !title.is_empty() && title_lower != "facetime" {
        return Some(named(candidate(MeetingSource::FaceTime, title, title)));
    }
    None
}

fn named(mut candidate: MeetingCandidate) -> MeetingCandidate {
    candidate.named = candidate.title != candidate.source.label();
    candidate
}

pub fn candidate_from_microphone(
    application: &ActiveMicrophoneApplication,
    context: &ContextSnapshot,
) -> Option<MeetingCandidate> {
    let canonical_bundle = application.canonical_bundle_id()?;
    let application_name = crate::microphone_activity::canonical_application_name(canonical_bundle)
        .map(str::to_string)
        .unwrap_or_else(|| application.name.clone());
    if application.is_browser() {
        let matches = context_matches_browser(canonical_bundle, &application.name, context);
        let site = matches
            .then(|| context.browser_host())
            .flatten()
            .map(|host| {
                host.strip_prefix("www.")
                    .unwrap_or(host)
                    .to_ascii_lowercase()
            });
        if matches && let Some(mut candidate) = candidate_from_context(context) {
            candidate.application = Some(canonical_bundle.into());
            candidate.application_name = Some(application_name);
            candidate.site = site;
            return Some(candidate);
        }
        let mut candidate = candidate(MeetingSource::BrowserCall, canonical_bundle, "");
        candidate.application = Some(canonical_bundle.into());
        candidate.application_name = Some(application_name);
        candidate.site = site;
        return Some(candidate);
    }
    let (source, title) = match canonical_bundle {
        "us.zoom.xos" => (MeetingSource::Zoom, "Zoom meeting"),
        "com.microsoft.teams2" => (MeetingSource::Teams, "Microsoft Teams meeting"),
        "com.tinyspeck.slackmacgap" => (MeetingSource::SlackHuddle, "Slack huddle"),
        "com.apple.FaceTime" => (MeetingSource::FaceTime, "FaceTime call"),
        _ => return None,
    };
    let mut candidate = candidate(source, canonical_bundle, title);
    candidate.application = Some(canonical_bundle.into());
    candidate.application_name = Some(application_name);
    Some(candidate)
}

fn is_browser_application(application_lower: &str) -> bool {
    ["brave", "chrome", "safari", "firefox", "edge"]
        .iter()
        .any(|browser| application_lower.contains(browser))
}

fn context_matches_browser(
    canonical_bundle: &str,
    application_name: &str,
    context: &ContextSnapshot,
) -> bool {
    let Some(foreground) = context.application.as_deref() else {
        return false;
    };
    if foreground.contains(application_name) || application_name.contains(foreground) {
        return true;
    }
    let lower = foreground.to_ascii_lowercase();
    match canonical_bundle {
        "com.brave.Browser" => lower.contains("brave"),
        "com.google.Chrome" => lower.contains("chrome"),
        "com.apple.Safari" => lower.contains("safari"),
        "org.mozilla.firefox" => lower.contains("firefox"),
        "com.microsoft.edgemac" => lower.contains("edge"),
        _ => false,
    }
}

fn candidate(source: MeetingSource, identity: &str, title: &str) -> MeetingCandidate {
    MeetingCandidate {
        key: opaque_key(source, identity),
        title: if title.is_empty() {
            source.label().into()
        } else {
            title.into()
        },
        source,
        application: None,
        named: false,
        site: None,
        application_name: None,
    }
}

fn opaque_key(source: MeetingSource, identity: &str) -> String {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    identity.hash(&mut hasher);
    format!("meeting-{:016x}", hasher.finish())
}

fn is_google_meet_call(url: &Url) -> bool {
    if url.host_str() != Some("meet.google.com") {
        return false;
    }
    let path = url.path().trim_matches('/');
    !path.is_empty() && path != "landing" && path != "new"
}

fn clean_google_meet_title(title: &str) -> String {
    let mut cleaned = clean_title(
        title,
        &[
            " - Audio playing - Brave",
            " - Brave",
            " - Brave Browser",
            " - Google Chrome",
            " — Mozilla Firefox",
            " - Microsoft Edge",
            " - Safari",
        ],
    );
    cleaned = clean_title(cleaned, &[" - Google Meet", " | Google Meet"]);
    for prefix in ["Meet - ", "Meet – ", "Meet — "] {
        if let Some(rest) = cleaned.strip_prefix(prefix) {
            cleaned = rest.trim();
            break;
        }
    }
    if cleaned.eq_ignore_ascii_case("Google Meet")
        || cleaned.eq_ignore_ascii_case("Meet")
        || is_google_meet_code(cleaned)
    {
        return String::new();
    }
    cleaned.to_string()
}

fn is_google_meet_code(value: &str) -> bool {
    let mut parts = value.split('-');
    let (Some(first), Some(second), Some(third), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    first.len() == 3
        && second.len() == 4
        && third.len() == 3
        && [first, second, third]
            .into_iter()
            .all(|part| part.bytes().all(|byte| byte.is_ascii_lowercase()))
}

fn clean_title<'a>(title: &'a str, suffixes: &[&str]) -> &'a str {
    suffixes
        .iter()
        .find_map(|suffix| title.strip_suffix(suffix))
        .unwrap_or(title)
        .trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(application: &str, url: Option<&str>, title: &str) -> ContextSnapshot {
        ContextSnapshot {
            application: Some(application.into()),
            browser_url: url.map(|url| Url::parse(url).unwrap()),
            window_title: Some(title.into()),
            selected_text: None,
            input_revision: None,
        }
    }

    #[test]
    fn recognizes_calls_without_treating_landing_pages_as_meetings() {
        assert!(
            candidate_from_context(&context(
                "Brave Browser",
                Some("https://meet.google.com/abc-defg-hij"),
                "Design sync - Google Meet"
            ))
            .is_some()
        );
        assert!(
            candidate_from_context(&context(
                "Brave Browser",
                Some("https://meet.google.com/landing"),
                "Google Meet"
            ))
            .is_none()
        );
        assert!(candidate_from_context(&context("zoom.us", None, "Zoom Workplace")).is_none());
        assert!(candidate_from_context(&context("zoom.us", None, "Zoom Meeting")).is_some());
    }

    #[test]
    fn requires_stability_and_offers_once_per_session() {
        let candidate = candidate_from_context(&context(
            "Brave Browser",
            Some("https://meet.google.com/abc-defg-hij"),
            "Design sync - Google Meet",
        ));
        let mut detector = MeetingDetector::default();
        assert!(detector.observe(candidate.clone()).is_none());
        assert!(detector.observe(candidate.clone()).is_none());
        assert!(detector.observe(candidate.clone()).is_none());
        assert!(detector.observe(candidate.clone()).is_some());
        assert!(detector.observe(candidate.clone()).is_none());
        for _ in 0..ABSENT_OBSERVATIONS_TO_RESET {
            assert!(detector.observe(None).is_none());
        }
        assert!(detector.observe(candidate.clone()).is_none());
        assert!(detector.observe(candidate.clone()).is_none());
        assert!(detector.observe(candidate.clone()).is_none());
        assert!(detector.observe(candidate).is_some());
    }

    #[test]
    fn a_brief_microphone_release_does_not_re_offer_the_same_call() {
        let candidate = candidate(MeetingSource::Zoom, "us.zoom.xos", "Zoom meeting");
        let mut detector = MeetingDetector::default();
        for _ in 0..STABLE_OBSERVATIONS - 1 {
            assert!(detector.observe(Some(candidate.clone())).is_none());
        }
        assert!(detector.observe(Some(candidate.clone())).is_some());
        // Dismissed or ignored: twenty seconds off the microphone keeps it snoozed.
        for _ in 0..40 {
            assert!(detector.observe(None).is_none());
        }
        for _ in 0..10 {
            assert!(detector.observe(Some(candidate.clone())).is_none());
        }
    }

    #[test]
    fn named_meetings_keep_their_title_and_app_calls_use_the_app() {
        let meet = candidate_from_context(&context(
            "Brave Browser",
            Some("https://meet.google.com/abc-defg-hij"),
            "Design sync - Google Meet",
        ))
        .unwrap();
        let start = meet.recording_start();
        assert_eq!(start.title.as_deref(), Some("Design sync"));
        assert_eq!(start.app.as_deref(), Some("Google Meet"));

        let zoom = candidate(MeetingSource::Zoom, "us.zoom.xos", "Zoom meeting").recording_start();
        assert_eq!(zoom.title, None);
        assert_eq!(zoom.app.as_deref(), Some("Zoom"));

        let manual = MeetingCandidate::manual().recording_start();
        assert_eq!((manual.title, manual.app), (None, None));
    }

    #[test]
    fn call_end_is_reported_once_per_sustained_absence() {
        let mut presence = CallPresence::default();
        assert!(!presence.observe(true));
        for _ in 0..CALL_ENDED_OBSERVATIONS - 1 {
            assert!(!presence.observe(false));
        }
        assert!(presence.observe(false));
        assert!(!presence.observe(false));
        // Rejoining and leaving again asks again.
        assert!(!presence.observe(true));
        for _ in 0..CALL_ENDED_OBSERVATIONS - 1 {
            assert!(!presence.observe(false));
        }
        assert!(presence.observe(false));
    }

    #[test]
    fn classifies_helper_and_host_microphone_applications() {
        let brave_helper = ActiveMicrophoneApplication {
            pid: 2352,
            bundle_id: "com.brave.Browser.helper".into(),
            name: "Brave Browser Helper".into(),
        };
        let meet_context = context(
            "Brave Browser",
            Some("https://meet.google.com/abc-defg-hij"),
            "Meet - Design sync - Brave",
        );
        let meet = candidate_from_microphone(&brave_helper, &meet_context).unwrap();
        assert_eq!(meet.source, MeetingSource::GoogleMeet);
        assert_eq!(meet.title, "Design sync");
        assert!(meet.named);
        assert_eq!(meet.application.as_deref(), Some("com.brave.Browser"));

        let slack_helper = ActiveMicrophoneApplication {
            pid: 31807,
            bundle_id: "com.tinyspeck.slackmacgap.helper".into(),
            name: "Slack Helper".into(),
        };
        let huddle = candidate_from_microphone(&slack_helper, &ContextSnapshot::default()).unwrap();
        assert_eq!(huddle.source, MeetingSource::SlackHuddle);
        assert_eq!(
            huddle.application.as_deref(),
            Some("com.tinyspeck.slackmacgap")
        );

        let zoom_helper = ActiveMicrophoneApplication {
            pid: 9010,
            bundle_id: "us.zoom.CptHost".into(),
            name: "CptHost".into(),
        };
        let zoom = candidate_from_microphone(&zoom_helper, &ContextSnapshot::default()).unwrap();
        assert_eq!(zoom.source, MeetingSource::Zoom);
        assert_eq!(zoom.application.as_deref(), Some("us.zoom.xos"));

        let safari_gpu = ActiveMicrophoneApplication {
            pid: 7100,
            bundle_id: "com.apple.WebKit.GPU".into(),
            name: "Safari Graphics and Media".into(),
        };
        let browser = candidate_from_microphone(&safari_gpu, &ContextSnapshot::default()).unwrap();
        assert_eq!(browser.source, MeetingSource::BrowserCall);
        assert_eq!(browser.application.as_deref(), Some("com.apple.Safari"));
    }

    #[test]
    fn cleans_google_meet_titles_and_ignores_raw_meet_codes() {
        let code_only = candidate_from_context(&context(
            "Brave Browser",
            Some("https://meet.google.com/abc-defg-hij"),
            "Meet - abc-defg-hij - Brave",
        ))
        .unwrap();
        assert_eq!(code_only.source, MeetingSource::GoogleMeet);
        assert_eq!(code_only.title, "Google Meet");
        assert!(!code_only.named);
        assert_eq!(code_only.recording_start().title, None);

        let named_prefix = candidate_from_context(&context(
            "Google Chrome",
            Some("https://meet.google.com/abc-defg-hij"),
            "Meet – Weekly Architecture - Google Chrome",
        ))
        .unwrap();
        assert_eq!(named_prefix.title, "Weekly Architecture");
        assert!(named_prefix.named);
    }

    #[test]
    fn switching_tabs_during_a_snoozed_browser_call_does_not_re_offer() {
        let brave = ActiveMicrophoneApplication {
            pid: 1592,
            bundle_id: "com.brave.Browser".into(),
            name: "Brave Browser".into(),
        };
        let meet_context = context(
            "Brave Browser",
            Some("https://meet.google.com/abc-defg-hij"),
            "Design sync - Google Meet",
        );
        let meet = candidate_from_microphone(&brave, &meet_context).unwrap();
        let fallback = candidate_from_microphone(&brave, &ContextSnapshot::default()).unwrap();

        let mut detector = MeetingDetector::default();
        for _ in 0..STABLE_OBSERVATIONS - 1 {
            assert!(detector.observe(Some(meet.clone())).is_none());
        }
        assert!(detector.observe(Some(meet)).is_some());
        assert!(detector.is_application_snoozed("com.brave.Browser"));
        for _ in 0..STABLE_OBSERVATIONS + 2 {
            assert!(detector.observe(Some(fallback.clone())).is_none());
        }
    }

    fn brave() -> ActiveMicrophoneApplication {
        ActiveMicrophoneApplication {
            pid: 49686,
            bundle_id: "com.brave.Browser".into(),
            name: "Brave Browser".into(),
        }
    }

    #[test]
    fn offer_titles_name_the_service_site_or_browser_without_repeating_call() {
        let problems = context(
            "Brave Browser",
            Some("https://www.problems.kitlangton.com/board"),
            "Problems",
        );
        let site = candidate_from_microphone(&brave(), &problems).unwrap();
        assert_eq!(site.source, MeetingSource::BrowserCall);
        assert_eq!(site.offer_title(), "Call on problems.kitlangton.com");
        assert_eq!(
            site.recording_start().app.as_deref(),
            Some("problems.kitlangton.com")
        );

        let unknown = candidate_from_microphone(&brave(), &ContextSnapshot::default()).unwrap();
        assert_eq!(unknown.offer_title(), "Call in Brave Browser");

        let meet = candidate_from_microphone(
            &brave(),
            &context(
                "Brave Browser",
                Some("https://meet.google.com/abc-defg-hij"),
                "Meet - abc-defg-hij - Brave",
            ),
        )
        .unwrap();
        assert_eq!(meet.offer_title(), "Google Meet call detected");
        assert_eq!(
            candidate(MeetingSource::SlackHuddle, "slack", "Slack huddle").offer_title(),
            "Slack huddle detected"
        );
    }

    #[test]
    fn never_offer_silences_the_site_when_known_and_the_app_otherwise() {
        let problems = context(
            "Brave Browser",
            Some("https://problems.kitlangton.com/"),
            "Problems",
        );
        let site = candidate_from_microphone(&brave(), &problems).unwrap();
        let target = site.never_offer_target().unwrap();
        assert_eq!(target.key, "problems.kitlangton.com");
        assert!(site.is_silenced(&[target.key]));

        let unknown = candidate_from_microphone(&brave(), &ContextSnapshot::default()).unwrap();
        let app = unknown.never_offer_target().unwrap();
        assert_eq!(
            (app.key.as_str(), app.label.as_str()),
            ("com.brave.Browser", "Brave Browser")
        );
        assert!(!unknown.is_silenced(&["problems.kitlangton.com".into()]));
        // An app-level entry silences every site in that browser.
        assert!(site.is_silenced(&["com.brave.Browser".into()]));
    }

    #[test]
    fn a_browser_voice_note_is_not_offered_but_a_sustained_browser_call_is() {
        let problems = context(
            "Brave Browser",
            Some("https://problems.kitlangton.com/"),
            "Problems",
        );
        let candidate = candidate_from_microphone(&brave(), &problems).unwrap();
        let mut detector = MeetingDetector::default();
        // A six-second voice note: 12 observations, then the mic closes.
        for _ in 0..12 {
            assert!(detector.observe(Some(candidate.clone())).is_none());
        }
        assert!(detector.observe(None).is_none());
        for _ in 0..BROWSER_STABLE_OBSERVATIONS - 1 {
            assert!(detector.observe(Some(candidate.clone())).is_none());
        }
        assert!(detector.observe(Some(candidate)).is_some());
    }
}

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::app_settings::{
    KNOWN_MEETING_PROVIDERS, MORE_MEETING_PROVIDERS, MeetingPolicy, MeetingProviderChoice,
};
use crate::dictation::context::ContextSnapshot;
use crate::microphone_activity::ActiveMicrophoneApplication;

/// A known call service must keep the microphone for about 2 s.
const STABLE_FOR: Duration = Duration::from_secs(2);
/// A browser using the microphone on a page that is not a known call site
/// must keep it for about 20 s: voice notes and dictation fields open the
/// microphone for a few seconds, calls hold it for minutes.
const BROWSER_STABLE_FOR: Duration = Duration::from_secs(20);
/// An offered call stays snoozed until its app has been off the microphone
/// for about a minute, so one call is offered once.
const ABSENT_FOR_RESET: Duration = Duration::from_secs(60);
/// About three seconds without the call app on the microphone.
const CALL_ENDED_AFTER: Duration = Duration::from_secs(3);
/// Observations further apart than this (a sleeping Mac, a stalled loop) do
/// not make one run: the clock restarts rather than counting the gap.
const MAX_OBSERVATION_GAP: Duration = Duration::from_secs(10);
/// A call that drops off one microphone check (or two, on a slow loop) and
/// is back right after keeps its time toward the offer (#835).
const DROPOUT_GRACE: Duration = Duration::from_secs(2);
/// A page the browser showed while it held the microphone still names the
/// call this long after it was last read.
const BROWSER_PAGE_MEMORY: Duration = Duration::from_secs(60);

/// Websites whose pages are calls whenever a browser holds the microphone on
/// them. Google Meet is separate: only its meeting paths are calls.
const CALL_SITES: [(&str, MeetingSource); 4] = [
    ("banshee.app", MeetingSource::Banshee),
    ("web.whatsapp.com", MeetingSource::WhatsApp),
    ("app.v2.gather.town", MeetingSource::Gather),
    ("app.gather.town", MeetingSource::Gather),
];

/// The websites Labs › More call apps names. Off, they are unknown pages and
/// a call there stays a plain browser call.
const MORE_CALL_SITES: [(&str, MeetingSource); 5] = [
    ("voice.google.com", MeetingSource::GoogleVoice),
    ("messenger.com", MeetingSource::Messenger),
    ("teams.live.com", MeetingSource::Teams),
    ("facetime.apple.com", MeetingSource::FaceTime),
    ("web.groupme.com", MeetingSource::GroupMe),
];

/// How long a condition has held across observations, by the monotonic
/// clock, so a slow loop iteration cannot stretch a threshold.
#[derive(Clone, Copy, Debug)]
struct Run {
    since: Instant,
    last: Instant,
}

impl Run {
    fn start(now: Instant) -> Self {
        Self {
            since: now,
            last: now,
        }
    }

    /// Records an observation and returns how long the run has lasted.
    fn extend(&mut self, now: Instant) -> Duration {
        if now.saturating_duration_since(self.last) > MAX_OBSERVATION_GAP {
            self.since = now;
        }
        self.last = now;
        now.saturating_duration_since(self.since)
    }
}

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
    /// The call's own meeting code when its page names one (a Google Meet
    /// code), so two calls in one app are told apart (#1113).
    #[serde(default)]
    pub call: Option<String>,
}

fn known_provider_label(key: &str) -> Option<&'static str> {
    KNOWN_MEETING_PROVIDERS
        .iter()
        .find(|(known, _)| *known == key)
        .map(|(_, label)| *label)
        .or_else(|| {
            CALL_SITES
                .iter()
                .find(|(host, _)| *host == key)
                .map(|(_, source)| source.label())
        })
        .or_else(|| {
            // The lab's names are unknown while it is off, exactly as before.
            if !crate::labs::more_call_apps() {
                return None;
            }
            MORE_MEETING_PROVIDERS
                .iter()
                .find(|(known, _)| *known == key)
                .map(|(_, label)| *label)
                .or_else(|| {
                    MORE_CALL_SITES
                        .iter()
                        .find(|(host, _)| *host == key)
                        .map(|(_, source)| source.label())
                })
                .or_else(|| is_webex_host(key).then_some(MeetingSource::Webex.label()))
        })
}

/// A website or app whose calls share one Ask / Always record / Never offer
/// choice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MeetingProvider {
    pub key: String,
    pub label: String,
}

impl MeetingProvider {
    pub fn choose(self, policy: MeetingPolicy) -> MeetingProviderChoice {
        MeetingProviderChoice {
            key: self.key,
            label: self.label,
            policy,
        }
    }
}

/// What a stable detected call leads to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Detection {
    Offer(MeetingCandidate),
    /// The provider is set to Always record.
    Record(MeetingCandidate),
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
            call: None,
        }
    }

    /// A manual recording, named `title` when one is given.
    pub fn manual_titled(title: Option<String>) -> Self {
        match title.map(|title| title.trim().to_owned()) {
            Some(title) if !title.is_empty() => Self {
                title,
                named: true,
                ..Self::manual()
            },
            _ => Self::manual(),
        }
    }

    pub fn recording_start(&self) -> crate::meeting::MeetingStart {
        crate::meeting::MeetingStart {
            title: self.named.then(|| self.title.clone()),
            app: self.app_label(),
            application: self.application.clone(),
            calendar: None,
            title_source: if self.source == MeetingSource::Manual {
                crate::meeting::TitleSource::User
            } else {
                crate::meeting::TitleSource::Call
            },
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

    /// Whose call this is, for the user's Ask / Always record / Never offer
    /// choice: the website when known, else the app. Known services use the
    /// name Settings lists them under.
    pub fn provider(&self) -> Option<MeetingProvider> {
        let key = self.site.clone().or_else(|| self.application.clone())?;
        let label = known_provider_label(&key)
            .map(str::to_string)
            .or_else(|| self.site.clone())
            .or_else(|| self.application_name.clone())
            .unwrap_or_else(|| key.clone());
        Some(MeetingProvider { key, label })
    }

    /// Whether one click may set this provider to Always record: not for a
    /// browser whose page is unknown, which would record every page.
    pub fn can_always_record(&self) -> bool {
        self.provider().is_some()
            && !(self.source == MeetingSource::BrowserCall && self.site.is_none())
    }

    /// The user's choice for this call: the website's when it has one, else
    /// the app's (a browser's choice covers its other sites), else Ask.
    pub fn policy(&self, choices: &[MeetingProviderChoice]) -> MeetingPolicy {
        let chosen = |key: Option<&str>| {
            let key = key?;
            choices
                .iter()
                .find(|choice| choice.key == key)
                .map(|choice| choice.policy)
        };
        chosen(self.site.as_deref())
            .or_else(|| chosen(self.application.as_deref()))
            .unwrap_or_default()
    }

    fn required_stability(&self) -> Duration {
        match self.source {
            // WhatsApp also takes the microphone for voice notes (#835), and
            // Gather can hold it while you sit in a space (#855). So can the
            // messaging apps Labs › More call apps adds.
            MeetingSource::BrowserCall
            | MeetingSource::WhatsApp
            | MeetingSource::Gather
            | MeetingSource::Signal
            | MeetingSource::Telegram
            | MeetingSource::WeChat
            | MeetingSource::Line
            | MeetingSource::Element => BROWSER_STABLE_FOR,
            _ => STABLE_FOR,
        }
    }
}

/// Notices when the call application behind a recording stops using the
/// microphone, so the user can choose to stop. It never stops anything.
#[derive(Default)]
pub struct CallPresence {
    absent: Option<Run>,
    prompted: bool,
}

impl CallPresence {
    /// Returns true once per absence that lasts long enough.
    pub fn observe(&mut self, present: bool) -> bool {
        self.observe_at(present, Instant::now())
    }

    fn observe_at(&mut self, present: bool, now: Instant) -> bool {
        if present {
            self.absent = None;
            self.prompted = false;
            return false;
        }
        let absent = self.absent.get_or_insert_with(|| Run::start(now));
        if absent.extend(now) >= CALL_ENDED_AFTER && !self.prompted {
            self.prompted = true;
            return true;
        }
        false
    }
}

/// How long an automatic recording keeps going after its call ended, under
/// Labs › Stop when the call ends (#835).
pub const AUTO_STOP_GRACE: Duration = Duration::from_secs(30);

/// Labs › Stop when the call ends: the deadline at which a recording Hex
/// started on its own stops, armed when [`CallPresence`] reports the call
/// ended and dropped when the call app comes back or the user keeps
/// recording.
#[derive(Default)]
pub struct AutoStop {
    deadline: Option<Instant>,
}

impl AutoStop {
    /// The call ended: stop after the grace.
    pub fn arm(&mut self) {
        self.arm_at(Instant::now());
    }

    fn arm_at(&mut self, now: Instant) {
        self.deadline = Some(now + AUTO_STOP_GRACE);
    }

    /// The call app came back, the user kept recording, or the toggle is
    /// off: nothing stops. Returns whether a stop was pending.
    pub fn disarm(&mut self) -> bool {
        self.deadline.take().is_some()
    }

    /// Whether the grace has run out; true once, then disarmed.
    pub fn due(&mut self) -> bool {
        self.due_at(Instant::now())
    }

    fn due_at(&mut self, now: Instant) -> bool {
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            self.deadline = None;
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
    Banshee,
    Zoom,
    Teams,
    SlackHuddle,
    FaceTime,
    WhatsApp,
    Gather,
    Discord,
    BrowserCall,
    // Labs › More call apps; after `BrowserCall` so its generated key stays
    // the same as before the lab existed.
    Signal,
    Telegram,
    WeChat,
    Line,
    Webex,
    Element,
    Phone,
    GoogleVoice,
    Messenger,
    GroupMe,
}

impl MeetingSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Meeting",
            Self::GoogleMeet => "Google Meet",
            Self::Banshee => "Banshee",
            Self::Zoom => "Zoom",
            Self::Teams => "Microsoft Teams",
            Self::SlackHuddle => "Slack Huddle",
            Self::FaceTime => "FaceTime",
            Self::WhatsApp => "WhatsApp",
            Self::Gather => "Gather",
            Self::Discord => "Discord",
            Self::BrowserCall => "Browser call",
            Self::Signal => "Signal",
            Self::Telegram => "Telegram",
            Self::WeChat => "WeChat",
            Self::Line => "LINE",
            Self::Webex => "Webex",
            Self::Element => "Element",
            Self::Phone => "Phone",
            Self::GoogleVoice => "Google Voice",
            Self::Messenger => "Messenger",
            Self::GroupMe => "GroupMe",
        }
    }
}

#[derive(Default)]
pub struct MeetingDetector {
    pending: Option<Pending>,
    offered: Option<String>,
    offered_application: Option<String>,
    /// The offered call's meeting code, when its page named one: another
    /// code in the same app is a different call, not this one returning.
    offered_call: Option<String>,
    /// The offered call's recording was stopped by hand: the snooze ends
    /// once its app has been off the microphone for [`CALL_ENDED_AFTER`],
    /// not a minute, so the next call is offered (#1113).
    stopped: bool,
    /// The offer's capsule closed on its timeout without an answer: the
    /// snooze ends as soon as the call app leaves the microphone, so a call
    /// joined after its pre-join screen is offered again (#1267).
    timed_out: bool,
    /// When a candidate was last on the microphone (or known to be, while
    /// recording); absence is counted from here, gaps included.
    last_present: Option<Instant>,
    calendar: Option<CalendarSnooze>,
}

/// A candidate that has been on the microphone but is not yet stable.
struct Pending {
    key: String,
    application: Option<String>,
    /// A browser call whose page is not known (yet).
    unresolved: bool,
    run: Run,
}

/// A call the user already answered a calendar reminder for: recorded it,
/// or dismissed it.
#[derive(Clone, Debug, Eq, PartialEq)]
struct CalendarSnooze {
    /// App bundle IDs and website hosts of the meeting's call service;
    /// empty when the invitation had no call link, which covers the next
    /// call alone.
    keys: Vec<String>,
    /// When the snooze lapses, in wall-clock milliseconds: the meeting's
    /// end, or shortly after its start when it has no call link.
    until_ms: i64,
}

impl CalendarSnooze {
    fn covers(&self, candidate: &MeetingCandidate) -> bool {
        self.keys.is_empty()
            || [candidate.site.as_deref(), candidate.application.as_deref()]
                .into_iter()
                .flatten()
                .any(|key| {
                    self.keys
                        .iter()
                        .any(|known| crate::calendar::key_matches(key, known))
                })
    }
}

impl MeetingDetector {
    /// A calendar reminder was answered: its call, when detected before
    /// `until_ms`, is treated as already offered. `keys` names the call
    /// service; empty covers the next call detected.
    pub fn snooze_calendar_call(&mut self, keys: Vec<String>, until_ms: i64) {
        self.calendar = Some(CalendarSnooze { keys, until_ms });
    }

    /// The call that was offered is still on the microphone, as a running
    /// recording's call app is: its snooze must last from now, not from the
    /// last detection pass.
    pub fn keep_snooze_alive(&mut self) {
        self.keep_snooze_alive_at(Instant::now());
    }

    fn keep_snooze_alive_at(&mut self, now: Instant) {
        self.last_present = Some(now);
    }

    /// The snoozed call's recording was stopped by hand.
    pub fn recording_stopped(&mut self) {
        if self.offered.is_some() {
            self.stopped = true;
        }
    }

    /// The offer's capsule closed on its timeout, unanswered.
    pub fn offer_timed_out(&mut self) {
        if self.offered.is_some() {
            self.timed_out = true;
        }
    }

    /// Whether the call app is on the microphone, seen while a recording is
    /// transcribing and detection doesn't run: an absence then still ends
    /// the snooze.
    pub fn observe_call_app(&mut self, present: bool) {
        self.observe_call_app_at(present, Instant::now());
    }

    fn observe_call_app_at(&mut self, present: bool, now: Instant) {
        if present {
            self.last_present = Some(now);
        } else {
            self.absent_at(now);
        }
    }

    /// Whether the app is snoozed whatever page it shows, so its tabs need
    /// not be read. A snoozed call with a meeting code isn't: the page can
    /// show a different call.
    pub fn is_application_snoozed(&self, canonical_bundle_id: &str) -> bool {
        self.offered_application.as_deref() == Some(canonical_bundle_id)
            && self.offered_call.is_none()
    }

    /// Whether `candidate` is the offered call: the same key, or the same
    /// app unless both name different meeting codes.
    fn snoozes(&self, candidate: &MeetingCandidate) -> bool {
        if self.offered.as_deref() == Some(&candidate.key) {
            return true;
        }
        candidate.application.is_some()
            && self.offered_application == candidate.application
            && !(self.offered_call.is_some()
                && candidate.call.is_some()
                && self.offered_call != candidate.call)
    }

    /// No candidate is on the microphone: the snooze ends once that has
    /// lasted long enough.
    fn absent_at(&mut self, now: Instant) {
        let since = *self.last_present.get_or_insert(now);
        if self.timed_out {
            self.offered = None;
            self.offered_application = None;
            self.offered_call = None;
            self.stopped = false;
            self.timed_out = false;
            return;
        }
        let reset_after = if self.stopped {
            CALL_ENDED_AFTER
        } else {
            ABSENT_FOR_RESET
        };
        if now.saturating_duration_since(since) >= reset_after {
            self.offered = None;
            self.offered_application = None;
            self.offered_call = None;
            self.stopped = false;
            self.timed_out = false;
        }
    }

    /// One observation under the user's choices. A Never offer call shows
    /// nothing but still counts as present; Always record ones pass the same
    /// stability time and snooze as an offer, so a stopped or dismissed call
    /// never starts again until its app leaves the microphone.
    pub fn detect(
        &mut self,
        candidate: Option<MeetingCandidate>,
        choices: &[MeetingProviderChoice],
    ) -> Option<Detection> {
        self.detect_at(
            candidate,
            choices,
            Instant::now(),
            crate::calendar::wall_clock_ms(),
        )
    }

    fn detect_at(
        &mut self,
        candidate: Option<MeetingCandidate>,
        choices: &[MeetingProviderChoice],
        now: Instant,
        now_ms: i64,
    ) -> Option<Detection> {
        if candidate
            .as_ref()
            .is_some_and(|candidate| candidate.policy(choices) == MeetingPolicy::NeverOffer)
        {
            // Still on the microphone: an earlier offer stays snoozed.
            self.pending = None;
            self.last_present = Some(now);
            return None;
        }
        let candidate = self.observe_at(candidate, now, now_ms)?;
        Some(match candidate.policy(choices) {
            MeetingPolicy::AlwaysRecord => Detection::Record(candidate),
            _ => Detection::Offer(candidate),
        })
    }

    fn observe_at(
        &mut self,
        candidate: Option<MeetingCandidate>,
        now: Instant,
        now_ms: i64,
    ) -> Option<MeetingCandidate> {
        let Some(candidate) = candidate else {
            if self.pending.as_ref().is_some_and(|pending| {
                now.saturating_duration_since(pending.run.last) > DROPOUT_GRACE
            }) {
                self.pending = None;
            }
            self.absent_at(now);
            return None;
        };
        self.last_present = Some(now);
        if self.snoozes(&candidate) {
            self.pending = None;
            return None;
        }
        let unresolved = candidate.source == MeetingSource::BrowserCall && candidate.site.is_none();
        let held = match &mut self.pending {
            Some(pending) if pending.key == candidate.key => pending.run.extend(now),
            // The browser's page was found: the same browser has been on the
            // microphone all along, so the run carries over.
            Some(pending)
                if pending.unresolved
                    && candidate.site.is_some()
                    && pending.application.is_some()
                    && pending.application == candidate.application =>
            {
                pending.key = candidate.key.clone();
                pending.unresolved = unresolved;
                pending.run.extend(now)
            }
            _ => {
                self.pending = Some(Pending {
                    key: candidate.key.clone(),
                    application: candidate.application.clone(),
                    unresolved,
                    run: Run::start(now),
                });
                Duration::ZERO
            }
        };
        if held < candidate.required_stability() {
            return None;
        }
        self.pending = None;
        self.offered = Some(candidate.key.clone());
        self.offered_application = candidate.application.clone();
        self.offered_call = candidate.call.clone();
        self.stopped = false;
        self.timed_out = false;
        if self
            .calendar
            .as_ref()
            .is_some_and(|snooze| now_ms >= snooze.until_ms)
        {
            self.calendar = None;
        }
        if let Some(snooze) = &self.calendar
            && snooze.covers(&candidate)
        {
            tracing::info!(
                source = candidate.source.label(),
                "detected call already answered from its calendar reminder"
            );
            if snooze.keys.is_empty() {
                self.calendar = None;
            }
            return None;
        }
        Some(candidate)
    }
}

/// The call `context` describes. `more_call_apps` is Labs › More call apps:
/// off, the extra websites are unknown pages.
pub fn candidate_from_context(
    context: &ContextSnapshot,
    more_call_apps: bool,
) -> Option<MeetingCandidate> {
    let application = context.application.as_deref()?;
    let application_lower = application.to_ascii_lowercase();
    let title = context.window_title.as_deref().unwrap_or("").trim();
    let title_lower = title.to_ascii_lowercase();

    if is_browser_application(&application_lower)
        && let Some(url) = &context.browser_url
        && is_google_meet_call(url)
    {
        let mut meet = named(candidate(
            MeetingSource::GoogleMeet,
            url.as_str(),
            &clean_google_meet_title(title),
        ));
        meet.call = google_meet_url_code(url);
        return Some(meet);
    }
    // No page address (Automation off, no web area behind another app): a
    // Meet call's tab title still names it (#827).
    if is_browser_application(&application_lower)
        && context.browser_url.is_none()
        && is_google_meet_call_title(title)
    {
        let mut meet = named(candidate(
            MeetingSource::GoogleMeet,
            &format!("{GOOGLE_MEET_SITE} {title}"),
            &clean_google_meet_title(title),
        ));
        meet.site = Some(GOOGLE_MEET_SITE.into());
        meet.call = google_meet_title_code(title);
        return Some(meet);
    }
    if is_browser_application(&application_lower)
        && let Some(host) = context.browser_host()
        && let Some(source) = call_site_source(host, more_call_apps)
    {
        return Some(candidate(source, &call_site_host(host, more_call_apps), ""));
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
    more_call_apps: bool,
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
            .map(|host| call_site_host(host, more_call_apps));
        if matches && let Some(mut candidate) = candidate_from_context(context, more_call_apps) {
            candidate.application = Some(canonical_bundle.into());
            candidate.application_name = Some(application_name);
            candidate.site = site.or(candidate.site.take());
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
        "net.whatsapp.WhatsApp" => (MeetingSource::WhatsApp, "WhatsApp call"),
        "com.gather.GatherV2" => (MeetingSource::Gather, "Gather call"),
        "com.hnc.Discord" => (MeetingSource::Discord, "Discord call"),
        _ if more_call_apps => more_call_app(canonical_bundle)?,
        _ => return None,
    };
    let mut candidate = candidate(source, canonical_bundle, title);
    candidate.application = Some(canonical_bundle.into());
    candidate.application_name = Some(application_name);
    Some(candidate)
}

/// The apps Labs › More call apps adds, and the title each call gets.
fn more_call_app(canonical_bundle: &str) -> Option<(MeetingSource, &'static str)> {
    Some(match canonical_bundle {
        "org.whispersystems.signal-desktop" => (MeetingSource::Signal, "Signal call"),
        "ru.keepcoder.Telegram" => (MeetingSource::Telegram, "Telegram call"),
        "com.tencent.xinWeChat" => (MeetingSource::WeChat, "WeChat call"),
        "jp.naver.line.mac" => (MeetingSource::Line, "LINE call"),
        "com.webex.meetingmanager" => (MeetingSource::Webex, "Webex meeting"),
        "im.riot.app" => (MeetingSource::Element, "Element call"),
        "com.apple.mobilephone" => (MeetingSource::Phone, "Phone call"),
        _ => return None,
    })
}

/// A website host as a site key: lowercase, without `www.`, and a Webex
/// subdomain as `webex.com` while Labs › More call apps is on, so one choice
/// covers every Webex page.
fn call_site_host(host: &str, more_call_apps: bool) -> String {
    let host = site_host(host);
    if more_call_apps && is_webex_host(&host) {
        "webex.com".into()
    } else {
        host
    }
}

/// Whether a website host is Webex's: `webex.com` or any of its subdomains.
fn is_webex_host(host: &str) -> bool {
    host == "webex.com" || host.ends_with(".webex.com")
}

/// A website host as a site key: lowercase, without `www.`.
fn site_host(host: &str) -> String {
    host.strip_prefix("www.")
        .unwrap_or(host)
        .to_ascii_lowercase()
}

/// The call service a browser page on `host` belongs to, if it is a known
/// call site and, for the extra sites, Labs › More call apps is on.
fn call_site_source(host: &str, more_call_apps: bool) -> Option<MeetingSource> {
    let host = site_host(host);
    if let Some((_, source)) = CALL_SITES.iter().find(|(known, _)| *known == host) {
        return Some(*source);
    }
    if !more_call_apps {
        return None;
    }
    if is_webex_host(&host) {
        return Some(MeetingSource::Webex);
    }
    MORE_CALL_SITES
        .iter()
        .find(|(known, _)| *known == host)
        .map(|(_, source)| *source)
}

/// The last page a browser showed while it held the microphone. The call's
/// site is read from the browser's tabs, which fails now and then (a timed
/// out AppleScript, a browser without an adapter); the page seen a moment
/// ago still names the call, but not one seen minutes ago.
#[derive(Default)]
pub struct BrowserPageMemory {
    last: Option<(&'static str, ContextSnapshot, Instant)>,
}

impl BrowserPageMemory {
    /// The page to judge `application`'s call by: `context` when it shows the
    /// browser's page, else the page remembered from the last 60 s.
    pub fn recall(
        &mut self,
        application: &ActiveMicrophoneApplication,
        context: ContextSnapshot,
        now: Instant,
    ) -> ContextSnapshot {
        let Some(bundle) = application
            .canonical_bundle_id()
            .filter(|_| application.is_browser())
        else {
            self.last = None;
            return context;
        };
        if context_matches_browser(bundle, &application.name, &context)
            && context.browser_url.is_some()
        {
            self.last = Some((bundle, context.clone(), now));
            return context;
        }
        match &self.last {
            Some((remembered, page, at))
                if *remembered == bundle
                    && now.saturating_duration_since(*at) <= BROWSER_PAGE_MEMORY =>
            {
                page.clone()
            }
            _ => context,
        }
    }

    /// No browser holds the microphone, so the next call starts fresh.
    pub fn forget(&mut self) {
        self.last = None;
    }
}

fn is_browser_application(application_lower: &str) -> bool {
    // Arc's name is too short to match as a substring.
    application_lower == "arc"
        || ["brave", "chrome", "safari", "firefox", "edge", "vivaldi"]
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
        "company.thebrowser.Browser" => lower == "arc",
        "com.vivaldi.Vivaldi" => lower.contains("vivaldi"),
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
        call: None,
    }
}

fn opaque_key(source: MeetingSource, identity: &str) -> String {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    identity.hash(&mut hasher);
    format!("meeting-{:016x}", hasher.finish())
}

/// Whether a page address is a call this detection knows: a Google Meet
/// call, a known call site, or with `more_call_apps`, one of the extra sites.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn names_call(url: &Url, more_call_apps: bool) -> bool {
    is_google_meet_call(url)
        || url
            .host_str()
            .and_then(|host| call_site_source(host, more_call_apps))
            .is_some()
}

/// Whether a browser tab's title names a call (a Google Meet call's).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn title_names_call(title: &str) -> bool {
    is_google_meet_call_title(title)
}

fn is_google_meet_call(url: &Url) -> bool {
    if url.host_str() != Some("meet.google.com") {
        return false;
    }
    let path = url.path().trim_matches('/');
    !path.is_empty() && path != "landing" && path != "new"
}

const GOOGLE_MEET_SITE: &str = "meet.google.com";

const BROWSER_TITLE_SUFFIXES: &[&str] = &[
    " - Audio playing - Brave",
    " - Brave",
    " - Brave Browser",
    " - Google Chrome",
    " — Mozilla Firefox",
    " - Microsoft Edge",
    " - Safari",
    " - Arc",
    " - Vivaldi",
];

const GOOGLE_MEET_TITLE_PREFIXES: [&str; 3] = ["Meet - ", "Meet – ", "Meet — "];

/// Whether a browser tab title is a Google Meet call's ("Meet - abc-defg-hij",
/// "Meet – Design sync"); the landing page is just "Google Meet".
fn is_google_meet_call_title(title: &str) -> bool {
    let cleaned = clean_title(title, BROWSER_TITLE_SUFFIXES);
    GOOGLE_MEET_TITLE_PREFIXES.into_iter().any(|prefix| {
        cleaned
            .strip_prefix(prefix)
            .is_some_and(|rest| !rest.trim().is_empty())
    })
}

fn clean_google_meet_title(title: &str) -> String {
    let mut cleaned = clean_title(title, BROWSER_TITLE_SUFFIXES);
    cleaned = clean_title(cleaned, &[" - Google Meet", " | Google Meet"]);
    for prefix in GOOGLE_MEET_TITLE_PREFIXES {
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

/// A Meet call's code from its address: the first path segment, so query
/// strings (`?authuser=0`) never make one call look like two.
fn google_meet_url_code(url: &Url) -> Option<String> {
    let code = url
        .path()
        .trim_matches('/')
        .split('/')
        .next()?
        .to_ascii_lowercase();
    (!code.is_empty()).then_some(code)
}

/// A Meet call's code from a tab title that shows only the code
/// ("Meet - abc-defg-hij"); a named title carries none.
fn google_meet_title_code(title: &str) -> Option<String> {
    let cleaned = clean_title(title, BROWSER_TITLE_SUFFIXES);
    GOOGLE_MEET_TITLE_PREFIXES.into_iter().find_map(|prefix| {
        let rest = cleaned.strip_prefix(prefix)?.trim();
        is_google_meet_code(rest).then(|| rest.to_string())
    })
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

    /// These tests judge calls with Labs › More call apps off; the lab's own
    /// tests call `super::candidate_from_*` with the switch on.
    fn candidate_from_context(context: &ContextSnapshot) -> Option<MeetingCandidate> {
        super::candidate_from_context(context, false)
    }

    fn candidate_from_microphone(
        application: &ActiveMicrophoneApplication,
        context: &ContextSnapshot,
    ) -> Option<MeetingCandidate> {
        super::candidate_from_microphone(application, context, false)
    }

    fn context(application: &str, url: Option<&str>, title: &str) -> ContextSnapshot {
        ContextSnapshot {
            application: Some(application.into()),
            browser_url: url.map(|url| Url::parse(url).unwrap()),
            window_title: Some(title.into()),
            input_revision: None,
        }
    }

    const STEP_MS: u64 = 500;

    /// A detector fed on a fake monotonic clock, like the watcher loop does.
    struct Watch {
        detector: MeetingDetector,
        base: Instant,
        elapsed_ms: u64,
        wall_ms: i64,
    }

    impl Watch {
        fn new() -> Self {
            Self {
                detector: MeetingDetector::default(),
                base: Instant::now(),
                elapsed_ms: 0,
                wall_ms: 0,
            }
        }

        /// Observes `candidate` every `step_ms` for `ms`, the first at the
        /// current time, and returns each detection with the time it came.
        fn run_every(
            &mut self,
            candidate: Option<&MeetingCandidate>,
            choices: &[MeetingProviderChoice],
            ms: u64,
            step_ms: u64,
        ) -> Vec<(u64, Detection)> {
            let mut found = Vec::new();
            let end = self.elapsed_ms + ms;
            while self.elapsed_ms < end {
                let now = self.base + Duration::from_millis(self.elapsed_ms);
                if let Some(detection) =
                    self.detector
                        .detect_at(candidate.cloned(), choices, now, self.wall_ms)
                {
                    found.push((self.elapsed_ms, detection));
                }
                self.elapsed_ms += step_ms;
            }
            found
        }

        fn run(
            &mut self,
            candidate: Option<&MeetingCandidate>,
            choices: &[MeetingProviderChoice],
            ms: u64,
        ) -> Vec<(u64, Detection)> {
            self.run_every(candidate, choices, ms, STEP_MS)
        }

        /// Time passes with no observation, as when the Mac sleeps.
        fn skip(&mut self, ms: u64) {
            self.elapsed_ms += ms;
        }

        /// Observes until the candidate has been stable for just under its
        /// threshold, then once more; returns what that last observation
        /// detected.
        fn stable(
            &mut self,
            candidate: &MeetingCandidate,
            choices: &[MeetingProviderChoice],
        ) -> Option<Detection> {
            let required = candidate.required_stability().as_millis() as u64;
            assert!(self.run(Some(candidate), choices, required).is_empty());
            self.run(Some(candidate), choices, STEP_MS)
                .pop()
                .map(|(_, detection)| detection)
        }

        /// Off the microphone for longer than the snooze.
        fn leave(&mut self) {
            let ms = ABSENT_FOR_RESET.as_millis() as u64 + STEP_MS;
            assert!(self.run(None, &[], ms).is_empty());
        }
    }

    /// When each detection came, asserting every one is an offer.
    fn offered(found: &[(u64, Detection)]) -> Vec<u64> {
        found
            .iter()
            .map(|(at, detection)| {
                assert!(matches!(detection, Detection::Offer(_)), "{detection:?}");
                *at
            })
            .collect()
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
        let banshee = candidate_from_context(&context(
            "Brave Browser",
            Some("https://www.banshee.app/room-1"),
            "Room 1",
        ))
        .unwrap();
        assert_eq!(banshee.source, MeetingSource::Banshee);
        assert!(
            candidate_from_context(&context(
                "Brave Browser",
                Some("https://notbanshee.app/"),
                "Not Banshee",
            ))
            .is_none()
        );
    }

    #[test]
    fn gather_calls_are_detected_in_its_app_and_browser() {
        for bundle_id in [
            "com.gather.GatherV2",
            "com.gather.GatherV2.helper.Renderer",
            "com.gather.Gather",
            "Gather.exe",
        ] {
            let app = ActiveMicrophoneApplication {
                pid: 7,
                bundle_id: bundle_id.into(),
                name: "Gather".into(),
            };
            let call = candidate_from_microphone(&app, &ContextSnapshot::default())
                .unwrap_or_else(|| panic!("{bundle_id} is Gather"));
            assert_eq!(call.source, MeetingSource::Gather);
            assert_eq!(call.application.as_deref(), Some("com.gather.GatherV2"));
            // Sitting in a space can hold the microphone, so about 20 s.
            assert_eq!(call.required_stability(), BROWSER_STABLE_FOR);
        }
        for url in [
            "https://app.v2.gather.town/space/abc",
            "https://app.gather.town/app/x",
        ] {
            let web =
                candidate_from_microphone(&brave(), &context("Brave Browser", Some(url), "Gather"))
                    .unwrap();
            assert_eq!(web.source, MeetingSource::Gather, "{url}");
        }
        assert_eq!(known_provider_label("com.gather.GatherV2"), Some("Gather"));
    }

    #[test]
    fn discord_calls_are_detected_on_every_desktop() {
        for bundle_id in [
            "com.hnc.Discord",
            "com.hnc.Discord.helper",
            "com.hnc.Discord.helper.Renderer",
            "com.hnc.DiscordCanary",
            "com.hnc.DiscordPTB",
            "Discord.exe",
            "DiscordCanary.exe",
            "DiscordPTB.exe",
            "Discord",
            "discord",
            "discord-canary",
            "com.discordapp.Discord",
        ] {
            let app = ActiveMicrophoneApplication {
                pid: 7,
                bundle_id: bundle_id.into(),
                name: "Discord".into(),
            };
            let call = candidate_from_microphone(&app, &ContextSnapshot::default())
                .unwrap_or_else(|| panic!("{bundle_id} is Discord"));
            assert_eq!(call.source, MeetingSource::Discord);
            assert_eq!(call.application.as_deref(), Some("com.hnc.Discord"));
            assert_eq!(call.required_stability(), STABLE_FOR);
        }
        assert_eq!(known_provider_label("com.hnc.Discord"), Some("Discord"));
    }

    #[test]
    fn whatsapp_calls_are_detected_after_a_voice_note_would_have_ended() {
        let mut offers = Vec::new();
        for (bundle_id, name) in [
            ("net.whatsapp.WhatsApp", "WhatsApp"),
            ("desktop.WhatsApp", "WhatsApp"),
            ("WhatsApp.Root.exe", "WhatsApp"),
            ("whatsapp.exe", "WhatsApp"),
        ] {
            let app = ActiveMicrophoneApplication {
                pid: 7,
                bundle_id: bundle_id.into(),
                name: name.into(),
            };
            let call = candidate_from_microphone(&app, &ContextSnapshot::default())
                .unwrap_or_else(|| panic!("{bundle_id} is WhatsApp"));
            assert_eq!(call.source, MeetingSource::WhatsApp);
            assert_eq!(call.application.as_deref(), Some("net.whatsapp.WhatsApp"));
            offers.push(call);
        }
        // Voice notes hold the microphone too, so it takes about 20 s.
        let mut watch = Watch::new();
        assert!(watch.run(Some(&offers[0]), &[], 19_500).is_empty());
        assert_eq!(watch.run(Some(&offers[0]), &[], 1_000).len(), 1);

        let web = candidate_from_microphone(
            &brave(),
            &context(
                "Brave Browser",
                Some("https://web.whatsapp.com/"),
                "WhatsApp",
            ),
        )
        .unwrap();
        assert_eq!(web.source, MeetingSource::WhatsApp);
        assert_eq!(web.required_stability(), BROWSER_STABLE_FOR);
        assert_eq!(
            known_provider_label("net.whatsapp.WhatsApp"),
            Some("WhatsApp")
        );
    }

    #[test]
    fn more_call_apps_are_unknown_until_their_lab_is_on() {
        for bundle_id in [
            "org.whispersystems.signal-desktop",
            "org.whispersystems.signal-desktop.helper",
            "ru.keepcoder.Telegram",
            "com.tdesktop.Telegram",
            "org.telegram.desktop",
            "com.tencent.xinWeChat",
            "jp.naver.line.mac",
            "im.riot.app",
            "com.webex.meetingmanager",
            "com.apple.mobilephone",
        ] {
            let app = ActiveMicrophoneApplication {
                pid: 7,
                bundle_id: bundle_id.into(),
                name: "Signal".into(),
            };
            assert_eq!(
                candidate_from_microphone(&app, &ContextSnapshot::default()),
                None,
                "{bundle_id} is only a call app with its lab on"
            );
        }
        // Off, the page is an unknown browser page: a plain "Browser call".
        let page = context(
            "Brave Browser",
            Some("https://voice.google.com/u/0/calls"),
            "Google Voice",
        );
        let call = candidate_from_microphone(&brave(), &page).unwrap();
        assert_eq!(call.source, MeetingSource::BrowserCall);
        assert_eq!(call.site.as_deref(), Some("voice.google.com"));
        assert_eq!(call.offer_title(), "Call on voice.google.com");
    }

    #[test]
    fn more_call_apps_names_its_apps_and_websites_with_their_lab_on() {
        for (bundle_id, source, canonical, title, stability) in [
            (
                "org.whispersystems.signal-desktop",
                MeetingSource::Signal,
                "org.whispersystems.signal-desktop",
                "Signal call",
                BROWSER_STABLE_FOR,
            ),
            (
                "org.whispersystems.signal-desktop.helper.Renderer",
                MeetingSource::Signal,
                "org.whispersystems.signal-desktop",
                "Signal call",
                BROWSER_STABLE_FOR,
            ),
            (
                "ru.keepcoder.Telegram",
                MeetingSource::Telegram,
                "ru.keepcoder.Telegram",
                "Telegram call",
                BROWSER_STABLE_FOR,
            ),
            (
                "com.tdesktop.Telegram",
                MeetingSource::Telegram,
                "ru.keepcoder.Telegram",
                "Telegram call",
                BROWSER_STABLE_FOR,
            ),
            (
                "org.telegram.desktop",
                MeetingSource::Telegram,
                "ru.keepcoder.Telegram",
                "Telegram call",
                BROWSER_STABLE_FOR,
            ),
            (
                "com.tencent.xinWeChat",
                MeetingSource::WeChat,
                "com.tencent.xinWeChat",
                "WeChat call",
                BROWSER_STABLE_FOR,
            ),
            (
                "jp.naver.line.mac",
                MeetingSource::Line,
                "jp.naver.line.mac",
                "LINE call",
                BROWSER_STABLE_FOR,
            ),
            (
                "com.webex.meetingmanager",
                MeetingSource::Webex,
                "com.webex.meetingmanager",
                "Webex meeting",
                STABLE_FOR,
            ),
            (
                "com.cisco.webexmeetingsapp",
                MeetingSource::Webex,
                "com.webex.meetingmanager",
                "Webex meeting",
                STABLE_FOR,
            ),
            (
                "im.riot.app",
                MeetingSource::Element,
                "im.riot.app",
                "Element call",
                BROWSER_STABLE_FOR,
            ),
            (
                "com.apple.mobilephone",
                MeetingSource::Phone,
                "com.apple.mobilephone",
                "Phone call",
                STABLE_FOR,
            ),
        ] {
            let app = ActiveMicrophoneApplication {
                pid: 7,
                bundle_id: bundle_id.into(),
                name: "Call".into(),
            };
            let call = super::candidate_from_microphone(&app, &ContextSnapshot::default(), true)
                .unwrap_or_else(|| panic!("{bundle_id} is a call app with the lab on"));
            assert_eq!(call.source, source, "{bundle_id}");
            assert_eq!(call.application.as_deref(), Some(canonical), "{bundle_id}");
            assert_eq!(call.title, title, "{bundle_id}");
            assert_eq!(call.required_stability(), stability, "{bundle_id}");
        }
        for (url, source, site) in [
            (
                "https://voice.google.com/u/0/calls",
                MeetingSource::GoogleVoice,
                "voice.google.com",
            ),
            (
                "https://www.messenger.com/t/ana",
                MeetingSource::Messenger,
                "messenger.com",
            ),
            (
                "https://teams.live.com/meet/123",
                MeetingSource::Teams,
                "teams.live.com",
            ),
            (
                "https://facetime.apple.com/join/abc",
                MeetingSource::FaceTime,
                "facetime.apple.com",
            ),
            (
                "https://web.groupme.com/chats/1",
                MeetingSource::GroupMe,
                "web.groupme.com",
            ),
            (
                "https://meetings.webex.com/mw",
                MeetingSource::Webex,
                "webex.com",
            ),
            (
                "https://www.webex.com/meet",
                MeetingSource::Webex,
                "webex.com",
            ),
        ] {
            let page = context("Brave Browser", Some(url), "A call");
            let call = super::candidate_from_microphone(&brave(), &page, true).unwrap();
            assert_eq!(call.source, source, "{url}");
            assert_eq!(call.site.as_deref(), Some(site), "{url}");
        }
    }

    #[test]
    fn a_telegram_voice_note_is_not_offered_but_a_sustained_call_is() {
        let app = ActiveMicrophoneApplication {
            pid: 7,
            bundle_id: "ru.keepcoder.Telegram".into(),
            name: "Telegram".into(),
        };
        let call = super::candidate_from_microphone(&app, &ContextSnapshot::default(), true)
            .expect("Telegram is a call app with the lab on");
        let mut watch = Watch::new();
        assert!(watch.run(Some(&call), &[], 19_500).is_empty());
        assert_eq!(watch.run(Some(&call), &[], 1_000).len(), 1);
    }

    #[test]
    fn requires_stability_and_offers_once_per_session() {
        let candidate = candidate_from_context(&context(
            "Brave Browser",
            Some("https://meet.google.com/abc-defg-hij"),
            "Design sync - Google Meet",
        ))
        .unwrap();
        let mut watch = Watch::new();
        assert!(watch.run(Some(&candidate), &[], 2_000).is_empty());
        assert_eq!(offered(&watch.run(Some(&candidate), &[], 500)), [2_000]);
        assert!(watch.run(Some(&candidate), &[], 30_000).is_empty());
        // A minute off the microphone ends the call.
        assert!(watch.run(None, &[], 59_000).is_empty());
        assert!(watch.run(Some(&candidate), &[], 2_500).is_empty());
        watch.run(None, &[], 61_000);
        assert!(watch.run(Some(&candidate), &[], 2_000).is_empty());
        assert_eq!(watch.run(Some(&candidate), &[], 500).len(), 1);
    }

    #[test]
    fn a_brief_microphone_release_does_not_re_offer_the_same_call() {
        let candidate = candidate(MeetingSource::Zoom, "us.zoom.xos", "Zoom meeting");
        let mut watch = Watch::new();
        assert!(watch.run(Some(&candidate), &[], 2_000).is_empty());
        assert_eq!(watch.run(Some(&candidate), &[], 500).len(), 1);
        // Dismissed or ignored: twenty seconds off the microphone keeps it snoozed.
        assert!(watch.run(None, &[], 20_000).is_empty());
        assert!(watch.run(Some(&candidate), &[], 5_000).is_empty());
    }

    #[test]
    fn a_timed_out_offer_is_offered_again_when_the_call_retakes_the_microphone() {
        // Google Meet's pre-join screen holds the microphone; nobody answers
        // the offer before it times out (#1267).
        let meet = candidate_from_context(&context(
            "Arc",
            Some("https://meet.google.com/abc-defg-hij"),
            "Meet - Design sync",
        ))
        .unwrap();
        let mut watch = Watch::new();
        assert!(watch.stable(&meet, &[]).is_some());
        assert!(watch.run(Some(&meet), &[], 30_000).is_empty());
        watch.detector.offer_timed_out();
        assert!(watch.run(Some(&meet), &[], 10_000).is_empty());
        // Joining releases and retakes the microphone: offered again.
        assert!(watch.run(None, &[], 500).is_empty());
        assert_eq!(
            watch.stable(&meet, &[]),
            Some(Detection::Offer(meet.clone()))
        );

        // An offer that hasn't timed out stays snoozed through the same release.
        assert!(watch.run(None, &[], 500).is_empty());
        assert!(watch.run(Some(&meet), &[], 10_000).is_empty());
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
        let base = Instant::now();
        let at = |ms: u64| base + Duration::from_millis(ms);
        let mut presence = CallPresence::default();
        assert!(!presence.observe_at(true, at(0)));
        // A slow loop (900 ms an iteration) still ends the call after 3 s.
        assert!(!presence.observe_at(false, at(900)));
        assert!(!presence.observe_at(false, at(1_800)));
        assert!(!presence.observe_at(false, at(2_700)));
        assert!(presence.observe_at(false, at(3_900)));
        assert!(!presence.observe_at(false, at(4_800)));
        // Rejoining and leaving again asks again.
        assert!(!presence.observe_at(true, at(5_000)));
        assert!(!presence.observe_at(false, at(5_500)));
        assert!(!presence.observe_at(false, at(8_000)));
        assert!(presence.observe_at(false, at(8_500)));
    }

    #[test]
    fn an_ended_call_stops_after_the_grace_unless_it_comes_back_or_is_kept() {
        let base = Instant::now();
        let at = |ms: u64| base + Duration::from_millis(ms);
        let grace = AUTO_STOP_GRACE.as_millis() as u64;
        let mut stop = AutoStop::default();
        assert!(!stop.due_at(at(0)), "nothing is armed");

        // The call ended: it stops once the grace runs out, once.
        let mut presence = CallPresence::default();
        assert!(!presence.observe_at(false, at(0)));
        assert!(presence.observe_at(false, at(3_000)));
        stop.arm_at(at(3_000));
        assert!(!stop.due_at(at(3_000 + grace - 500)));
        assert!(stop.due_at(at(3_000 + grace)));
        assert!(!stop.due_at(at(3_000 + grace + 500)));

        // The call app is back during the grace: nothing stops.
        stop.arm_at(at(0));
        assert!(!presence.observe_at(true, at(10_000)));
        assert!(stop.disarm());
        assert!(!stop.due_at(at(grace + 1_000)));

        // Keep recording: this absence never stops it; leaving again re-arms.
        stop.arm_at(at(0));
        assert!(stop.disarm());
        assert!(!stop.due_at(at(grace * 2)));
        assert!(!presence.observe_at(false, at(20_000)));
        assert!(!presence.observe_at(false, at(21_000)));
        assert!(presence.observe_at(false, at(23_000)));
        stop.arm_at(at(23_000));
        assert!(stop.due_at(at(23_000 + grace)));
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

        let mut watch = Watch::new();
        assert!(watch.run(Some(&meet), &[], 2_000).is_empty());
        assert_eq!(watch.run(Some(&meet), &[], 500).len(), 1);
        assert!(watch.run(Some(&fallback), &[], 5_000).is_empty());
        // A page that isn't a call, while the Meet call goes on, is still it.
        let other = candidate_from_microphone(
            &brave,
            &context("Brave Browser", Some("https://mail.google.com/"), "Inbox"),
        )
        .unwrap();
        assert!(watch.run(Some(&other), &[], 25_000).is_empty());
    }

    fn meet_in_brave(url: &str, title: &str) -> MeetingCandidate {
        candidate_from_microphone(&brave(), &context("Brave Browser", Some(url), title)).unwrap()
    }

    #[test]
    fn a_different_meet_call_in_the_same_browser_is_not_snoozed_by_the_first() {
        let first = meet_in_brave("https://meet.google.com/abc-defg-hij", "Meet - Design sync");
        let again = meet_in_brave(
            "https://meet.google.com/abc-defg-hij?authuser=0",
            "Meet - Design sync",
        );
        let second = meet_in_brave("https://meet.google.com/xyz-wxyz-xyz", "Meet - Standup");
        assert_eq!(first.call.as_deref(), Some("abc-defg-hij"));
        assert_eq!(second.call.as_deref(), Some("xyz-wxyz-xyz"));

        let mut watch = Watch::new();
        assert_eq!(
            watch.stable(&first, &[]),
            Some(Detection::Offer(first.clone()))
        );
        // The browser's tabs are still read, since its page can show
        // another call.
        assert!(!watch.detector.is_application_snoozed("com.brave.Browser"));
        // The same call under another address is still snoozed.
        assert!(watch.run(Some(&again), &[], 5_000).is_empty());
        // A different call a few seconds later is offered (#1113).
        assert!(watch.run(None, &[], 2_000).is_empty());
        assert_eq!(watch.stable(&second, &[]), Some(Detection::Offer(second)));

        // A code-only tab title names the call too; a named one doesn't.
        assert_eq!(
            google_meet_title_code("Meet - abc-defg-hij - Arc").as_deref(),
            Some("abc-defg-hij")
        );
        assert_eq!(google_meet_title_code("Meet - Design sync"), None);
    }

    #[test]
    fn a_stopped_call_s_app_is_snoozed_only_until_it_leaves_for_a_few_seconds() {
        let slack = slack_huddle();
        let mut watch = Watch::new();
        assert!(watch.stable(&slack, &[]).is_some());
        watch.detector.recording_stopped();
        // A moment off the microphone: still the same call.
        assert!(watch.run(None, &[], 2_000).is_empty());
        assert!(watch.run(Some(&slack), &[], 5_000).is_empty());
        // Off for 3 s: the next call is offered.
        assert!(watch.run(None, &[], 3_500).is_empty());
        assert_eq!(
            watch.stable(&slack, &[]),
            Some(Detection::Offer(slack.clone()))
        );

        // Not stopped by hand: the minute still applies.
        assert!(watch.run(None, &[], 20_000).is_empty());
        assert!(watch.run(Some(&slack), &[], 5_000).is_empty());

        // The absence is seen while the stopped recording transcribes.
        let mut watch = Watch::new();
        assert!(watch.stable(&slack, &[]).is_some());
        watch.detector.recording_stopped();
        for _ in 0..8 {
            let now = watch.base + Duration::from_millis(watch.elapsed_ms);
            watch.detector.observe_call_app_at(false, now);
            watch.elapsed_ms += STEP_MS;
        }
        assert_eq!(watch.stable(&slack, &[]), Some(Detection::Offer(slack)));
    }

    #[test]
    fn vivaldi_on_the_microphone_is_a_browser_call() {
        // A Teams web app in Vivaldi runs in Vivaldi's own process (#1105).
        let vivaldi = ActiveMicrophoneApplication {
            pid: 4242,
            bundle_id: "vivaldi-bin".into(),
            name: "vivaldi-bin".into(),
        };
        let call = candidate_from_microphone(&vivaldi, &ContextSnapshot::default()).unwrap();
        assert_eq!(call.source, MeetingSource::BrowserCall);
        assert_eq!(call.application.as_deref(), Some("com.vivaldi.Vivaldi"));
        assert_eq!(call.application_name.as_deref(), Some("Vivaldi"));
        assert_eq!(call.offer_title(), "Call in Vivaldi");
        assert_eq!(call.required_stability(), BROWSER_STABLE_FOR);
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
        let banshee = candidate_from_microphone(
            &brave(),
            &context("Brave Browser", Some("https://banshee.app/room-1"), "Room"),
        )
        .unwrap();
        assert_eq!(banshee.offer_title(), "Banshee call detected");
        assert_eq!(banshee.recording_start().app.as_deref(), Some("Banshee"));
        assert_eq!(
            candidate(MeetingSource::SlackHuddle, "slack", "Slack huddle").offer_title(),
            "Slack huddle detected"
        );
    }

    fn choice(key: &str, policy: MeetingPolicy) -> MeetingProviderChoice {
        MeetingProviderChoice {
            key: key.into(),
            label: key.into(),
            policy,
        }
    }

    #[test]
    fn choices_apply_to_the_site_when_known_and_the_app_otherwise() {
        let problems = context(
            "Brave Browser",
            Some("https://problems.kitlangton.com/"),
            "Problems",
        );
        let site = candidate_from_microphone(&brave(), &problems).unwrap();
        let provider = site.provider().unwrap();
        assert_eq!(provider.key, "problems.kitlangton.com");
        assert_eq!(
            site.policy(&[provider.choose(MeetingPolicy::NeverOffer)]),
            MeetingPolicy::NeverOffer
        );
        assert!(site.can_always_record());

        let unknown = candidate_from_microphone(&brave(), &ContextSnapshot::default()).unwrap();
        let app = unknown.provider().unwrap();
        assert_eq!(
            (app.key.as_str(), app.label.as_str()),
            ("com.brave.Browser", "Brave Browser")
        );
        assert!(!unknown.can_always_record());
        let never_site = [choice("problems.kitlangton.com", MeetingPolicy::NeverOffer)];
        assert_eq!(unknown.policy(&never_site), MeetingPolicy::Ask);
        // A browser's choice covers its sites unless the site has its own.
        let never_brave = choice("com.brave.Browser", MeetingPolicy::NeverOffer);
        assert_eq!(
            site.policy(std::slice::from_ref(&never_brave)),
            MeetingPolicy::NeverOffer
        );
        assert_eq!(
            site.policy(&[
                never_brave.clone(),
                choice("problems.kitlangton.com", MeetingPolicy::AlwaysRecord)
            ]),
            MeetingPolicy::AlwaysRecord
        );
        // A site set back to Ask asks, whatever its browser's choice.
        assert_eq!(
            site.policy(&[
                never_brave,
                choice("problems.kitlangton.com", MeetingPolicy::Ask)
            ]),
            MeetingPolicy::Ask
        );

        // Known services carry the name Settings lists them under.
        let meet = candidate_from_microphone(
            &brave(),
            &context(
                "Brave Browser",
                Some("https://meet.google.com/abc-defg-hij"),
                "Design sync - Google Meet",
            ),
        )
        .unwrap();
        let provider = meet.provider().unwrap();
        assert_eq!(
            (provider.key.as_str(), provider.label.as_str()),
            ("meet.google.com", "Google Meet")
        );
        let slack = slack_huddle();
        assert_eq!(slack.provider().unwrap().label, "Slack huddles");
        let banshee = candidate_from_microphone(
            &brave(),
            &context("Brave Browser", Some("https://banshee.app/room-1"), "Room"),
        )
        .unwrap();
        let provider = banshee.provider().unwrap();
        assert_eq!(
            (provider.key.as_str(), provider.label.as_str()),
            ("banshee.app", "Banshee")
        );
        assert!(banshee.can_always_record());
        assert!(slack.can_always_record());
    }

    fn slack_huddle() -> MeetingCandidate {
        let helper = ActiveMicrophoneApplication {
            pid: 31807,
            bundle_id: "com.tinyspeck.slackmacgap.helper".into(),
            name: "Slack Helper".into(),
        };
        candidate_from_microphone(&helper, &ContextSnapshot::default()).unwrap()
    }

    #[test]
    fn always_record_starts_after_the_same_stability_time_as_an_offer() {
        let slack = slack_huddle();
        let always = [choice(
            "com.tinyspeck.slackmacgap",
            MeetingPolicy::AlwaysRecord,
        )];
        let mut watch = Watch::new();
        assert!(watch.run(Some(&slack), &always, 2_000).is_empty());
        assert_eq!(
            watch.run(Some(&slack), &always, 500),
            [(2_000, Detection::Record(slack.clone()))]
        );
        // Stopped mid-call: the call stays snoozed while it uses the microphone.
        assert!(watch.run(Some(&slack), &always, 10_000).is_empty());

        // Browser pages still need about 20 s on the microphone.
        let problems = candidate_from_microphone(
            &brave(),
            &context(
                "Brave Browser",
                Some("https://problems.kitlangton.com/"),
                "Problems",
            ),
        )
        .unwrap();
        let always = [choice(
            "problems.kitlangton.com",
            MeetingPolicy::AlwaysRecord,
        )];
        let mut watch = Watch::new();
        assert!(watch.run(Some(&problems), &always, 6_000).is_empty());
        assert!(watch.run(None, &always, 5_000).is_empty());
        assert!(watch.run(Some(&problems), &always, 20_000).is_empty());
        assert_eq!(
            watch.run(Some(&problems), &always, 500),
            [(31_000, Detection::Record(problems))]
        );
    }

    #[test]
    fn never_offer_and_snoozed_calls_never_start() {
        let slack = slack_huddle();
        let never = [choice(
            "com.tinyspeck.slackmacgap",
            MeetingPolicy::NeverOffer,
        )];
        let mut watch = Watch::new();
        assert!(watch.run(Some(&slack), &never, 8_000).is_empty());

        // Offered and dismissed, then switched to Always record mid-call.
        let mut watch = Watch::new();
        assert!(watch.run(Some(&slack), &[], 2_000).is_empty());
        assert_eq!(
            watch.run(Some(&slack), &[], 500),
            [(2_000, Detection::Offer(slack.clone()))]
        );
        let always = [choice(
            "com.tinyspeck.slackmacgap",
            MeetingPolicy::AlwaysRecord,
        )];
        assert!(watch.run(None, &always, 20_000).is_empty());
        assert!(watch.run(Some(&slack), &always, 8_000).is_empty());
        // Never offer, then Always record, while the call stays on the
        // microphone well past the snooze reset: still nothing.
        let never = [choice(
            "com.tinyspeck.slackmacgap",
            MeetingPolicy::NeverOffer,
        )];
        assert!(watch.run(Some(&slack), &never, 120_000).is_empty());
        assert!(watch.run(Some(&slack), &always, 8_000).is_empty());
        // The next call, after a minute off the microphone, records.
        watch.run(None, &always, 61_000);
        assert!(watch.run(Some(&slack), &always, 2_000).is_empty());
        assert_eq!(
            watch.run(Some(&slack), &always, 500).pop().unwrap().1,
            Detection::Record(slack)
        );
    }

    #[test]
    fn a_browser_voice_note_is_not_offered_but_a_sustained_browser_call_is() {
        let problems = context(
            "Brave Browser",
            Some("https://problems.kitlangton.com/"),
            "Problems",
        );
        let candidate = candidate_from_microphone(&brave(), &problems).unwrap();
        let mut watch = Watch::new();
        // A six-second voice note, then the mic closes for a few seconds.
        assert!(watch.run(Some(&candidate), &[], 6_000).is_empty());
        assert!(watch.run(None, &[], 5_000).is_empty());
        assert!(watch.run(Some(&candidate), &[], 20_000).is_empty());
        assert_eq!(watch.run(Some(&candidate), &[], 500).len(), 1);
    }

    #[test]
    fn a_missed_microphone_check_does_not_restart_the_wait_for_an_offer() {
        let app = ActiveMicrophoneApplication {
            pid: 7,
            bundle_id: "net.whatsapp.WhatsApp".into(),
            name: "WhatsApp".into(),
        };
        let call = candidate_from_microphone(&app, &ContextSnapshot::default()).unwrap();
        // One check misses the call 10 s in: still offered at 20 s.
        let mut watch = Watch::new();
        assert!(watch.run(Some(&call), &[], 10_000).is_empty());
        assert!(watch.run(None, &[], 500).is_empty());
        assert!(watch.run(Some(&call), &[], 9_500).is_empty());
        assert_eq!(offered(&watch.run(Some(&call), &[], 500)), [20_000]);

        // On a slow loop, two missed checks in a row are still a dropout.
        let mut watch = Watch::new();
        assert!(watch.run_every(Some(&call), &[], 9_900, 900).is_empty());
        assert!(watch.run_every(None, &[], 1_800, 900).is_empty());
        assert!(watch.run_every(Some(&call), &[], 9_000, 900).is_empty());
        assert_eq!(
            offered(&watch.run_every(Some(&call), &[], 900, 900)),
            [20_700]
        );

        // A longer release is the call (or voice note) ending: start over.
        let mut watch = Watch::new();
        assert!(watch.run(Some(&call), &[], 10_000).is_empty());
        assert!(watch.run(None, &[], 3_000).is_empty());
        assert!(watch.run(Some(&call), &[], 20_000).is_empty());
        assert_eq!(offered(&watch.run(Some(&call), &[], 500)), [33_000]);
    }

    #[test]
    fn a_slow_loop_still_offers_an_unknown_browser_call_after_about_20_seconds() {
        let candidate = candidate_from_microphone(&brave(), &ContextSnapshot::default()).unwrap();
        assert_eq!(candidate.source, MeetingSource::BrowserCall);
        // The loop sleeps 500 ms but its browser inspection takes 400 ms more.
        let mut watch = Watch::new();
        assert!(
            watch
                .run_every(Some(&candidate), &[], 20_700, 900)
                .is_empty()
        );
        assert_eq!(
            offered(&watch.run_every(Some(&candidate), &[], 900, 900)),
            [20_700]
        );

        // The same call seen at the nominal pace offers at the same time.
        let mut watch = Watch::new();
        assert!(watch.run(Some(&candidate), &[], 20_000).is_empty());
        assert_eq!(offered(&watch.run(Some(&candidate), &[], 500)), [20_000]);
    }

    #[test]
    fn a_slow_loop_still_ends_a_snooze_after_a_minute() {
        let slack = slack_huddle();
        let mut watch = Watch::new();
        assert!(watch.run_every(Some(&slack), &[], 2_700, 900).is_empty());
        assert_eq!(watch.run_every(Some(&slack), &[], 900, 900).len(), 1);
        assert!(watch.run_every(None, &[], 54_000, 900).is_empty());
        assert!(watch.run_every(Some(&slack), &[], 2_700, 900).is_empty());
        watch.run_every(None, &[], 63_000, 900);
        assert!(watch.run_every(Some(&slack), &[], 2_700, 900).is_empty());
        assert_eq!(watch.run_every(Some(&slack), &[], 900, 900).len(), 1);
    }

    #[test]
    fn a_stalled_loop_does_not_count_the_gap_as_time_on_the_microphone() {
        let candidate = candidate_from_microphone(&brave(), &ContextSnapshot::default()).unwrap();
        let mut watch = Watch::new();
        assert!(watch.run(Some(&candidate), &[], 1_000).is_empty());
        // The Mac slept; the call is on the microphone again afterwards.
        watch.skip(60_000);
        assert!(watch.run(Some(&candidate), &[], 20_000).is_empty());
        assert_eq!(watch.run(Some(&candidate), &[], 500).len(), 1);
    }

    #[test]
    fn banshee_is_offered_after_about_two_seconds() {
        let banshee = candidate_from_microphone(&brave(), &banshee_page()).unwrap();
        assert_eq!(banshee.source, MeetingSource::Banshee);
        assert_eq!(banshee.site.as_deref(), Some("banshee.app"));
        let mut watch = Watch::new();
        assert!(watch.run(Some(&banshee), &[], 2_000).is_empty());
        assert_eq!(offered(&watch.run(Some(&banshee), &[], 500)), [2_000]);

        // A slow loop does not stretch it.
        let mut watch = Watch::new();
        assert!(watch.run_every(Some(&banshee), &[], 2_700, 900).is_empty());
        assert_eq!(
            offered(&watch.run_every(Some(&banshee), &[], 900, 900)),
            [2_700]
        );
    }

    #[test]
    fn finding_the_page_keeps_the_run_toward_the_known_threshold() {
        let unknown = candidate_from_microphone(&brave(), &ContextSnapshot::default()).unwrap();
        let banshee = candidate_from_microphone(&brave(), &banshee_page()).unwrap();
        // The page is found 1 s in: offered 2 s after the browser took the
        // microphone, not 2 s after the page was found.
        let mut watch = Watch::new();
        assert!(watch.run(Some(&unknown), &[], 1_000).is_empty());
        assert!(watch.run(Some(&banshee), &[], 1_000).is_empty());
        assert_eq!(offered(&watch.run(Some(&banshee), &[], 500)), [2_000]);

        // The time held while unresolved is never more than the browser held
        // the microphone, and a different browser starts over.
        let chrome = ActiveMicrophoneApplication {
            pid: 400,
            bundle_id: "com.google.Chrome.helper".into(),
            name: "Google Chrome Helper".into(),
        };
        let chrome_unknown =
            candidate_from_microphone(&chrome, &ContextSnapshot::default()).unwrap();
        let mut watch = Watch::new();
        assert!(watch.run(Some(&chrome_unknown), &[], 1_500).is_empty());
        assert!(watch.run(Some(&banshee), &[], 2_000).is_empty());
        assert_eq!(offered(&watch.run(Some(&banshee), &[], 500)), [3_500]);

        // An unresolved page that is never found still needs 20 s.
        let mut watch = Watch::new();
        assert!(watch.run(Some(&unknown), &[], 20_000).is_empty());
        assert_eq!(offered(&watch.run(Some(&unknown), &[], 500)), [20_000]);
    }

    #[test]
    fn a_voice_note_on_another_site_is_not_a_banshee_call() {
        // The browser's active tab is the page using the microphone; a
        // Banshee tab open in the background is never read, so it can neither
        // shorten the threshold nor trigger Always record.
        let problems = candidate_from_microphone(
            &brave(),
            &context(
                "Brave Browser",
                Some("https://problems.kitlangton.com/"),
                "Problems",
            ),
        )
        .unwrap();
        let choices = [
            choice("banshee.app", MeetingPolicy::AlwaysRecord),
            choice("problems.kitlangton.com", MeetingPolicy::NeverOffer),
        ];
        let mut watch = Watch::new();
        assert!(watch.run(Some(&problems), &choices, 25_000).is_empty());

        // Without a choice for that site it is the unknown-page 20 s, an offer.
        let choices = [choice("banshee.app", MeetingPolicy::AlwaysRecord)];
        let mut watch = Watch::new();
        assert!(watch.run(Some(&problems), &choices, 2_500).is_empty());
        assert_eq!(watch.run(Some(&problems), &choices, 20_000).len(), 1);
    }

    #[test]
    fn a_gap_with_the_app_absent_counts_as_absence() {
        let slack = slack_huddle();
        let mut watch = Watch::new();
        assert!(watch.run(Some(&slack), &[], 2_000).is_empty());
        assert_eq!(watch.run(Some(&slack), &[], 500).len(), 1);
        // The Mac slept, or Meetings was off, for a while; the call is over.
        watch.skip(120_000);
        assert!(watch.run(None, &[], 500).is_empty());
        assert!(watch.run(Some(&slack), &[], 2_000).is_empty());
        assert_eq!(watch.run(Some(&slack), &[], 500).len(), 1);

        // Still on the microphone after the gap: still the same call.
        watch.skip(120_000);
        assert!(watch.run(Some(&slack), &[], 10_000).is_empty());
    }

    #[test]
    fn a_recorded_call_that_left_the_microphone_is_not_snoozed_for_another_minute() {
        let slack = slack_huddle();
        let mut watch = Watch::new();
        assert!(watch.run(Some(&slack), &[], 2_000).is_empty());
        assert_eq!(watch.run(Some(&slack), &[], 500).len(), 1);
        // Recorded for an hour; the app left the microphone after 5 minutes,
        // so the first pass after Stop already ends the snooze.
        watch.skip(3_600_000);
        assert!(watch.run(None, &[], 500).is_empty());
        assert!(watch.run(Some(&slack), &[], 2_000).is_empty());
        assert_eq!(watch.run(Some(&slack), &[], 500).len(), 1);

        // While the recording sees the app on the microphone, the snooze
        // counts from the last such moment.
        let mut watch = Watch::new();
        watch.run(Some(&slack), &[], 2_500);
        watch.skip(97_500);
        watch
            .detector
            .keep_snooze_alive_at(watch.base + Duration::from_millis(watch.elapsed_ms));
        assert!(watch.run(None, &[], 59_500).is_empty());
        assert!(watch.run(Some(&slack), &[], 5_000).is_empty());
        assert!(watch.run(None, &[], 61_000).is_empty());
        assert!(watch.run(Some(&slack), &[], 2_000).is_empty());
        assert_eq!(watch.run(Some(&slack), &[], 500).len(), 1);
    }

    fn banshee_page() -> ContextSnapshot {
        context(
            "Brave Browser",
            Some("https://banshee.app/standup"),
            "Standup",
        )
    }

    #[test]
    fn the_site_is_resolved_while_hex_is_in_front() {
        let t0 = Instant::now();
        let mut memory = BrowserPageMemory::default();
        let hex = context("Hex", None, "Hex");

        // Nothing seen yet: the site is unknown.
        let page = memory.recall(&brave(), hex.clone(), t0);
        assert_eq!(
            candidate_from_microphone(&brave(), &page).unwrap().source,
            MeetingSource::BrowserCall
        );

        // Brave was in front on Banshee, then Hex came forward.
        memory.recall(&brave(), banshee_page(), t0);
        let page = memory.recall(&brave(), hex, t0 + Duration::from_secs(30));
        let candidate = candidate_from_microphone(&brave(), &page).unwrap();
        assert_eq!(candidate.source, MeetingSource::Banshee);
        assert_eq!(candidate.site.as_deref(), Some("banshee.app"));
    }

    #[test]
    fn a_page_seen_minutes_ago_no_longer_names_the_call() {
        let t0 = Instant::now();
        let mut memory = BrowserPageMemory::default();
        let hex = context("Hex", None, "Hex");
        memory.recall(&brave(), banshee_page(), t0);
        let page = memory.recall(&brave(), hex.clone(), t0 + Duration::from_secs(61));
        assert_eq!(page, hex);
        assert!(
            candidate_from_microphone(&brave(), &page)
                .unwrap()
                .site
                .is_none()
        );
    }

    #[test]
    fn a_remembered_page_belongs_to_its_browser_and_its_call() {
        let t0 = Instant::now();
        let mut memory = BrowserPageMemory::default();
        let hex = context("Hex", None, "Hex");
        memory.recall(&brave(), banshee_page(), t0);

        let chrome = ActiveMicrophoneApplication {
            pid: 400,
            bundle_id: "com.google.Chrome.helper".into(),
            name: "Google Chrome Helper".into(),
        };
        assert_eq!(memory.recall(&chrome, hex.clone(), t0), hex);

        // The microphone is released: the next call starts fresh.
        memory.recall(&brave(), banshee_page(), t0);
        memory.forget();
        assert_eq!(memory.recall(&brave(), hex.clone(), t0), hex);

        // A fresh page replaces the remembered one.
        memory.recall(&brave(), banshee_page(), t0);
        let other = context("Brave Browser", Some("https://example.com/"), "Example");
        assert_eq!(memory.recall(&brave(), other.clone(), t0), other);
        assert_eq!(memory.recall(&brave(), hex, t0), other);
    }

    #[test]
    fn a_call_answered_from_its_calendar_reminder_is_not_offered_again() {
        let zoom = candidate(MeetingSource::Zoom, "us.zoom.xos", "Zoom meeting");
        let zoom = MeetingCandidate {
            application: Some("us.zoom.xos".into()),
            ..zoom
        };
        let zoom_keys = crate::calendar::CallProvider::Zoom
            .detection_keys()
            .iter()
            .map(|key| key.to_string())
            .collect::<Vec<_>>();
        let mut watch = Watch::new();
        watch
            .detector
            .snooze_calendar_call(zoom_keys.clone(), 1_000);
        watch.wall_ms = 10;
        assert_eq!(watch.stable(&zoom, &[]), None);
        // Rejoining the same meeting later is still covered.
        watch.wall_ms = 20;
        watch.leave();
        watch.wall_ms = 30;
        assert_eq!(watch.stable(&zoom, &[]), None);
        // Always record doesn't restart a call the user already stopped.
        watch.wall_ms = 40;
        watch.leave();
        let always = [choice("us.zoom.xos", MeetingPolicy::AlwaysRecord)];
        watch.wall_ms = 50;
        assert_eq!(watch.stable(&zoom, &always), None);
        // Another service's call is offered as usual.
        let slack = slack_huddle();
        watch.wall_ms = 60;
        watch.leave();
        watch.wall_ms = 70;
        assert_eq!(
            watch.stable(&slack, &[]),
            Some(Detection::Offer(slack.clone()))
        );
        // Once the meeting ends, its service's calls are offered again.
        watch.wall_ms = 2_000;
        watch.leave();
        assert_eq!(
            watch.stable(&zoom, &[]),
            Some(Detection::Offer(zoom.clone()))
        );

        // A Meet link covers the site in any browser.
        let meet = candidate_from_microphone(
            &brave(),
            &context(
                "Brave Browser",
                Some("https://meet.google.com/abc-defg-hij"),
                "Design sync - Google Meet",
            ),
        )
        .unwrap();
        let mut watch = Watch::new();
        watch
            .detector
            .snooze_calendar_call(vec!["meet.google.com".into()], 1_000);
        watch.wall_ms = 10;
        assert_eq!(watch.stable(&meet, &[]), None);
    }

    #[test]
    fn a_google_meet_call_in_arc_records_after_about_two_seconds() {
        let arc_helper = ActiveMicrophoneApplication {
            pid: 5120,
            bundle_id: "company.thebrowser.browser.helper".into(),
            name: "Browser Helper".into(),
        };
        assert!(arc_helper.is_browser());
        let meet = candidate_from_microphone(
            &arc_helper,
            &context(
                "Arc",
                Some("https://meet.google.com/abc-defg-hij"),
                "Meet - Design sync",
            ),
        )
        .unwrap();
        assert_eq!(meet.source, MeetingSource::GoogleMeet);
        assert_eq!(
            meet.application.as_deref(),
            Some("company.thebrowser.Browser")
        );
        assert_eq!(meet.application_name.as_deref(), Some("Arc"));
        assert_eq!(meet.site.as_deref(), Some("meet.google.com"));
        let always = [choice("meet.google.com", MeetingPolicy::AlwaysRecord)];
        let mut watch = Watch::new();
        assert!(watch.run(Some(&meet), &always, 2_000).is_empty());
        assert_eq!(
            watch.run(Some(&meet), &always, 500),
            [(2_000, Detection::Record(meet))]
        );
    }

    #[test]
    fn a_google_meet_call_in_arc_without_its_address_is_known_by_its_title() {
        let arc_helper = ActiveMicrophoneApplication {
            pid: 5120,
            bundle_id: "company.thebrowser.browser.helper".into(),
            name: "Browser Helper".into(),
        };
        // Automation off and Arc behind another app: no page address, only
        // the window's title.
        let page = context("Arc", None, "Meet - Design sync");
        let meet = candidate_from_microphone(&arc_helper, &page).unwrap();
        assert_eq!(meet.source, MeetingSource::GoogleMeet);
        assert_eq!(meet.title, "Design sync");
        assert_eq!(meet.site.as_deref(), Some("meet.google.com"));
        let always = [choice("meet.google.com", MeetingPolicy::AlwaysRecord)];
        let mut watch = Watch::new();
        assert!(watch.run(Some(&meet), &always, 2_000).is_empty());
        assert_eq!(
            watch.run(Some(&meet), &always, 500),
            [(2_000, Detection::Record(meet))]
        );

        let page = context("Arc", None, "Meet – abc-defg-hij");
        let code = candidate_from_microphone(&arc_helper, &page).unwrap();
        assert_eq!(code.source, MeetingSource::GoogleMeet);
        assert!(!code.named);

        // The landing page and other pages stay unnamed browser calls.
        for title in ["Google Meet", "Meet", "Problems"] {
            let page = context("Arc", None, title);
            let other = candidate_from_microphone(&arc_helper, &page).unwrap();
            assert_eq!(other.source, MeetingSource::BrowserCall, "{title}");
            assert!(other.site.is_none());
        }
    }

    #[test]
    fn a_reminder_without_a_call_link_covers_only_the_next_call() {
        let slack = slack_huddle();
        let mut watch = Watch::new();
        watch.detector.snooze_calendar_call(Vec::new(), 1_000);
        watch.wall_ms = 10;
        assert_eq!(watch.stable(&slack, &[]), None);
        watch.wall_ms = 20;
        watch.leave();
        watch.wall_ms = 30;
        assert_eq!(watch.stable(&slack, &[]), Some(Detection::Offer(slack)));
    }
}

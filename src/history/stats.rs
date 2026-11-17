//! Labs › Dictation stats (issue 790): a summary of the retained
//! dictations, computed when asked. It adds nothing to History; the dialog
//! also reads the separate daily rollup ([`super::usage`]) that outlives
//! retention.

use std::collections::HashMap;

use serde::Serialize;

use super::{History, HistoryEntry, HistoryStore};

/// How many applications the summary names.
pub const TOP_APPS: usize = 5;
/// How many recent local days the daily series covers, today last.
pub const DAILY_DAYS: i64 = 30;
/// How many rewrite profiles the latency summary names.
pub const TOP_PROFILES: usize = 4;

/// One application and how many retained dictations went to it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AppCount {
    pub application: String,
    pub dictations: u64,
}

/// One local day and how many retained dictations it holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct DayCount {
    /// Days since 1970-01-01, local time (see [`local_day`]).
    pub day: i64,
    pub dictations: u64,
}

/// Words over recorded minutes, or `None` without recorded audio.
pub fn words_per_minute(words: u64, audio_ms: u64) -> Option<u32> {
    (audio_ms > 0).then(|| ((words as f64 * 60_000.0) / audio_ms as f64).round() as u32)
}

/// What typing `words` at `typing_wpm` takes, less `audio_ms`, floored at
/// zero.
pub fn time_saved_ms(words: u64, audio_ms: u64, typing_wpm: u32) -> u64 {
    let typing_ms =
        words.saturating_mul(60_000) / u64::from(crate::app_settings::clamp_typing_wpm(typing_wpm));
    typing_ms.saturating_sub(audio_ms)
}

/// One mode's Output processing time, in milliseconds: its rewrite plus any
/// transformations that ran after it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProfileOutputTime {
    pub profile: String,
    pub runs: u64,
    /// The middle run: with an even count, the mean of the two middle runs,
    /// rounded down.
    pub median_ms: u64,
    pub slowest_ms: u64,
}

/// What the retained entries add up to.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HistoryStats {
    pub dictations: u64,
    /// Words in the text that was inserted, split on whitespace.
    pub words: u64,
    /// Recorded audio across the entries, in milliseconds.
    pub audio_ms: u64,
    /// The typing speed [`Self::time_saved_ms`] assumes, in words a minute.
    pub typing_wpm: u32,
    /// What typing [`Self::words`] would have taken, less [`Self::audio_ms`],
    /// floored at zero.
    pub time_saved_ms: u64,
    /// Words over audio minutes, counting only entries that recorded audio;
    /// `None` without any.
    pub words_per_minute: Option<u32>,
    /// Entries whose inserted text differs from the transcript (Output's
    /// replacements, modes, rewriting, or transformations changed it).
    pub changed_by_output: u64,
    /// Consecutive local days with a dictation, ending today, or yesterday
    /// when there is none yet today.
    pub streak_days: u32,
    /// Local days with at least one dictation.
    pub active_days: u32,
    /// Dictations by local hour, index 0 = midnight.
    pub hours: [u64; 24],
    /// The local hour with the most dictations, earliest on a tie.
    pub busiest_hour: Option<u8>,
    /// The last [`DAILY_DAYS`] local days ending today, oldest first, with
    /// zero on the days that had no dictation.
    pub daily: Vec<DayCount>,
    /// The most dictated-into applications, most first, then by name.
    pub top_apps: Vec<AppCount>,
    /// The most run modes' Output time, most runs first, then by name.
    pub output_time: Vec<ProfileOutputTime>,
}

impl Default for HistoryStats {
    fn default() -> Self {
        Self {
            dictations: 0,
            words: 0,
            audio_ms: 0,
            typing_wpm: crate::app_settings::DEFAULT_TYPING_WPM,
            time_saved_ms: 0,
            words_per_minute: None,
            changed_by_output: 0,
            streak_days: 0,
            active_days: 0,
            hours: [0; 24],
            busiest_hour: None,
            daily: Vec::new(),
            top_apps: Vec::new(),
            output_time: Vec::new(),
        }
    }
}

impl HistoryStats {
    /// The share of dictations Output changed, rounded, or `None` without
    /// dictations.
    pub fn changed_percent(&self) -> Option<u32> {
        (self.dictations > 0).then(|| {
            ((self.changed_by_output as f64 * 100.0) / self.dictations as f64).round() as u32
        })
    }
}

/// Adds up `entries`. `day_of` turns a timestamp into a day number (local
/// days in the app), `hour_of` its local hour, `typing_wpm` the speed time
/// saved is measured against, and `today` is the current day.
pub fn summarize<'a>(
    entries: impl IntoIterator<Item = &'a HistoryEntry>,
    today: i64,
    day_of: impl Fn(u64) -> i64,
    hour_of: impl Fn(u64) -> usize,
    typing_wpm: u32,
) -> HistoryStats {
    let mut stats = HistoryStats {
        typing_wpm: crate::app_settings::clamp_typing_wpm(typing_wpm),
        ..HistoryStats::default()
    };
    let mut timed_words = 0_u64;
    let mut timed_ms = 0_u64;
    let mut apps: HashMap<&str, u64> = HashMap::new();
    let mut days = std::collections::BTreeSet::new();
    let mut per_day: HashMap<i64, u64> = HashMap::new();
    let mut profiles: HashMap<&str, Vec<u64>> = HashMap::new();
    for entry in entries {
        if entry.cancelled {
            continue;
        }
        stats.dictations += 1;
        let words = entry.final_text.split_whitespace().count() as u64;
        stats.words += words;
        stats.audio_ms = stats.audio_ms.saturating_add(entry.audio_ms);
        let hour = hour_of(entry.timestamp_ms).min(23);
        stats.hours[hour] = stats.hours[hour].saturating_add(1);
        if entry.audio_ms > 0 {
            timed_words += words;
            timed_ms = timed_ms.saturating_add(entry.audio_ms);
        }
        if entry.raw_text.trim() != entry.final_text.trim() {
            stats.changed_by_output += 1;
        }
        if let Some(application) = entry.application.as_deref().map(str::trim)
            && !application.is_empty()
        {
            *apps.entry(application).or_default() += 1;
        }
        let day = day_of(entry.timestamp_ms);
        days.insert(day);
        *per_day.entry(day).or_default() += 1;
        if let Some(processing) = &entry.processing
            && processing.fallback.is_none()
        {
            // A fallback run is a failure or a deadline, not the time Output
            // takes, and a transformation-only run is not a mode.
            let profile = processing.profile.trim();
            if !profile.is_empty() && profile != crate::dictation::processor::CUSTOM_TRANSFORMATIONS
            {
                profiles
                    .entry(profile)
                    .or_default()
                    .push(processing.latency_ms);
            }
        }
    }
    stats.words_per_minute = words_per_minute(timed_words, timed_ms);
    stats.time_saved_ms = time_saved_ms(stats.words, stats.audio_ms, stats.typing_wpm);
    stats.active_days = days.len() as u32;
    stats.busiest_hour = busiest_hour(&stats.hours);
    let mut day = if days.contains(&today) {
        today
    } else {
        today - 1
    };
    while days.contains(&day) {
        stats.streak_days += 1;
        day -= 1;
    }
    if stats.dictations > 0 {
        stats.daily = (today - DAILY_DAYS + 1..=today)
            .map(|day| DayCount {
                day,
                dictations: per_day.get(&day).copied().unwrap_or(0),
            })
            .collect();
    }
    let mut apps: Vec<_> = apps.into_iter().collect();
    apps.sort_by(|(a, a_count), (b, b_count)| b_count.cmp(a_count).then_with(|| a.cmp(b)));
    stats.top_apps = apps
        .into_iter()
        .take(TOP_APPS)
        .map(|(application, dictations)| AppCount {
            application: application.to_owned(),
            dictations,
        })
        .collect();
    stats.output_time = output_time(profiles);
    stats
}

/// The most run modes, most runs first, each with its median and slowest
/// Output time. An even count's median is the mean of its two middle runs,
/// rounded down.
fn output_time(profiles: HashMap<&str, Vec<u64>>) -> Vec<ProfileOutputTime> {
    let mut profiles: Vec<_> = profiles.into_iter().collect();
    profiles
        .sort_by(|(a, a_runs), (b, b_runs)| b_runs.len().cmp(&a_runs.len()).then_with(|| a.cmp(b)));
    profiles
        .into_iter()
        .take(TOP_PROFILES)
        .map(|(profile, mut latencies)| {
            latencies.sort_unstable();
            let middle = latencies.len() / 2;
            let median_ms = if latencies.len() % 2 == 1 {
                latencies[middle]
            } else {
                let (low, high) = (latencies[middle - 1], latencies[middle]);
                low + (high - low) / 2
            };
            ProfileOutputTime {
                profile: profile.to_owned(),
                runs: latencies.len() as u64,
                median_ms,
                slowest_ms: latencies.last().copied().unwrap_or_default(),
            }
        })
        .collect()
}

/// The local hour with the most entries, earliest on a tie.
pub fn busiest_hour(hours: &[u64; 24]) -> Option<u8> {
    let mut busiest: Option<(u64, u8)> = None;
    for (hour, count) in hours.iter().copied().enumerate() {
        if count > 0 && busiest.is_none_or(|(best, _)| count > best) {
            busiest = Some((count, hour as u8));
        }
    }
    busiest.map(|(_, hour)| hour)
}

/// The local calendar day of a timestamp, as days since 1970-01-01.
pub fn local_day(timestamp_ms: u64) -> i64 {
    let iso = crate::meeting::query::iso_local(timestamp_ms);
    let field = |range: std::ops::Range<usize>| iso.get(range)?.parse::<i64>().ok();
    match (field(0..4), field(5..7), field(8..10)) {
        (Some(year), Some(month), Some(day)) => days_from_civil(year, month, day),
        _ => (timestamp_ms / 86_400_000) as i64,
    }
}

/// The local hour of a timestamp, 0 through 23.
pub fn local_hour(timestamp_ms: u64) -> usize {
    let iso = crate::meeting::query::iso_local(timestamp_ms);
    iso.get(11..13)
        .and_then(|hour| hour.parse::<usize>().ok())
        .filter(|hour| *hour < 24)
        .unwrap_or(((timestamp_ms / 3_600_000) % 24) as usize)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

impl HistoryStore {
    /// Every entry within retention, not just a search's first results.
    pub fn stats(&self, now_ms: u64, typing_wpm: u32) -> HistoryStats {
        let cutoff = self.retention.cutoff(now_ms);
        summarize(
            self.entries
                .iter()
                .filter(|entry| cutoff.is_none_or(|cutoff| entry.timestamp_ms >= cutoff)),
            local_day(now_ms),
            local_day,
            local_hour,
            typing_wpm,
        )
    }
}

impl History {
    /// The summary of every retained entry (Labs › Dictation stats).
    pub fn stats(&self, typing_wpm: u32) -> HistoryStats {
        self.synced().stats(super::now_ms(), typing_wpm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HistoryKind;

    const DAY: u64 = 86_400_000;

    fn entry(
        day: u64,
        raw: &str,
        final_text: &str,
        app: Option<&str>,
        audio_ms: u64,
    ) -> HistoryEntry {
        HistoryEntry {
            cancelled: false,
            id: day,
            timestamp_ms: day * DAY + 1_000,
            kind: HistoryKind::Dictation,
            raw_text: raw.into(),
            final_text: final_text.into(),
            application: app.map(str::to_owned),
            processing: None,
            audio_ms,
            inference_ms: 0,
            total_ms: 0,
            recording_bytes: 0,
            model: None,
            language: None,
            text_ms: None,
            versions: Vec::new(),
        }
    }

    fn utc_day(timestamp_ms: u64) -> i64 {
        (timestamp_ms / DAY) as i64
    }

    fn utc_hour(timestamp_ms: u64) -> usize {
        ((timestamp_ms / 3_600_000) % 24) as usize
    }

    /// The default typing speed, for the tests that don't change it.
    fn stats_of<'a>(
        entries: impl IntoIterator<Item = &'a HistoryEntry>,
        today: i64,
    ) -> HistoryStats {
        summarize(
            entries,
            today,
            utc_day,
            utc_hour,
            crate::app_settings::DEFAULT_TYPING_WPM,
        )
    }

    /// `entry` with a rewrite profile and its latency.
    fn rewritten(day: u64, profile: &str, latency_ms: u64) -> HistoryEntry {
        let mut entry = entry(day, "a", "a", None, 0);
        entry.processing = Some(crate::desktop::events::DictationProcessing {
            profile: profile.into(),
            latency_ms,
            model: None,
            fallback: None,
        });
        entry
    }

    #[test]
    fn words_speed_and_changes_add_up() {
        let entries = [
            entry(10, "one two three", "One two three.", Some("Zed"), 3_000),
            entry(10, "four five", "four five", Some("Slack"), 3_000),
            // No audio: its words count, but not toward the speed.
            entry(10, "six", "six", None, 0),
        ];
        let stats = stats_of(&entries, 10);
        assert_eq!(stats.dictations, 3);
        assert_eq!(stats.words, 6);
        assert_eq!(stats.audio_ms, 6_000);
        // Five words in six seconds.
        assert_eq!(stats.words_per_minute, Some(50));
        assert_eq!(stats.changed_by_output, 1);
        assert_eq!(stats.changed_percent(), Some(33));
        // Six words at 40 wpm take nine seconds; six were recorded.
        assert_eq!(stats.typing_wpm, crate::app_settings::DEFAULT_TYPING_WPM);
        assert_eq!(stats.time_saved_ms, 3_000);
        // Every entry's timestamp is just past midnight.
        assert_eq!(stats.hours[0], 3);
        assert_eq!(stats.busiest_hour, Some(0));
    }

    #[test]
    fn time_saved_floors_at_zero_and_a_saved_speed_is_clamped() {
        // One word typed at 200 wpm takes 300 ms, under an hour of audio.
        let slow = [entry(1, "one", "one", None, 3_600_000)];
        let stats = summarize(&slow, 1, utc_day, utc_hour, 500);
        assert_eq!(stats.typing_wpm, crate::app_settings::MAX_TYPING_WPM);
        assert_eq!(stats.time_saved_ms, 0);
        // Two hundred words at 10 wpm take twenty minutes: the same words
        // dictated in ten seconds saved most of them.
        let words = "a ".repeat(200);
        let fast = [entry(1, &words, words.trim(), None, 10_000)];
        let stats = summarize(&fast, 1, utc_day, utc_hour, 0);
        assert_eq!(stats.typing_wpm, crate::app_settings::MIN_TYPING_WPM);
        assert_eq!(stats.time_saved_ms, 1_190_000);
    }

    #[test]
    fn hours_are_counted_per_local_hour_with_the_earliest_busiest() {
        let mut entries = [
            entry(2, "a", "a", None, 0),
            entry(2, "a", "a", None, 0),
            entry(2, "a", "a", None, 0),
        ];
        entries[0].timestamp_ms = 2 * DAY + 9 * 3_600_000;
        entries[1].timestamp_ms = 2 * DAY + 9 * 3_600_000 + 60_000;
        entries[2].timestamp_ms = 2 * DAY + 14 * 3_600_000;
        let stats = stats_of(&entries, 2);
        assert_eq!(stats.hours[9], 2);
        assert_eq!(stats.hours[14], 1);
        assert_eq!(stats.hours.iter().sum::<u64>(), 3);
        assert_eq!(stats.busiest_hour, Some(9));
        // A tie goes to the earlier hour.
        entries[0].timestamp_ms = 2 * DAY + 16 * 3_600_000;
        entries[1].timestamp_ms = 2 * DAY + 9 * 3_600_000;
        entries[2].timestamp_ms = 2 * DAY + 20 * 3_600_000;
        let tied = stats_of(&entries, 2);
        assert_eq!((tied.hours[9], tied.hours[16], tied.hours[20]), (1, 1, 1));
        assert_eq!(tied.busiest_hour, Some(9));
    }

    #[test]
    fn nothing_retained_is_all_zero() {
        let stats = stats_of(std::iter::empty(), 10);
        assert_eq!(stats, HistoryStats::default());
        assert_eq!(stats.changed_percent(), None);
        assert_eq!(stats.words_per_minute, None);
    }

    #[test]
    fn top_apps_are_most_first_then_by_name() {
        let entries = [
            entry(1, "a", "a", Some("Zed"), 0),
            entry(1, "a", "a", Some("Slack"), 0),
            entry(1, "a", "a", Some("Zed"), 0),
            entry(1, "a", "a", Some("Mail"), 0),
            entry(1, "a", "a", Some("Arc"), 0),
            entry(1, "a", "a", Some("Notes"), 0),
            entry(1, "a", "a", Some("Xcode"), 0),
            entry(1, "a", "a", Some("  "), 0),
        ];
        let stats = stats_of(&entries, 1);
        let names: Vec<_> = stats
            .top_apps
            .iter()
            .map(|app| (app.application.as_str(), app.dictations))
            .collect();
        assert_eq!(
            names,
            [
                ("Zed", 2),
                ("Arc", 1),
                ("Mail", 1),
                ("Notes", 1),
                ("Slack", 1)
            ]
        );
    }

    #[test]
    fn the_daily_series_is_the_last_thirty_days_ending_today_with_empty_days() {
        let entries = [
            entry(10, "a", "a", None, 0),
            entry(10, "a", "a", None, 0),
            entry(8, "a", "a", None, 0),
        ];
        let stats = stats_of(&entries, 10);
        assert_eq!(stats.daily.len(), DAILY_DAYS as usize);
        // Oldest first, today last, with the days in between at zero.
        assert_eq!(
            stats.daily.first(),
            Some(&DayCount {
                day: -19,
                dictations: 0
            })
        );
        assert_eq!(
            stats.daily[27],
            DayCount {
                day: 8,
                dictations: 1
            }
        );
        assert_eq!(
            stats.daily[28],
            DayCount {
                day: 9,
                dictations: 0
            }
        );
        assert_eq!(
            stats.daily[29],
            DayCount {
                day: 10,
                dictations: 2
            }
        );
        assert!(stats.daily.last().is_some_and(|day| day.day == 10));
        // Nothing retained is no series at all, so the dialog has no chart.
        assert!(stats_of(std::iter::empty(), 10).daily.is_empty());
        // Days outside the window do not add bars.
        let old = stats_of(&[entry(1, "a", "a", None, 0)], 60);
        assert_eq!(old.daily.len(), DAILY_DAYS as usize);
        assert!(old.daily.iter().all(|day| day.dictations == 0));
        assert_eq!(old.daily.last().map(|day| day.day), Some(60));
    }

    #[test]
    fn output_time_groups_by_profile_with_median_and_slowest() {
        let fallback = |profile: &str, latency_ms: u64| {
            let mut entry = rewritten(1, profile, latency_ms);
            entry.processing.as_mut().expect("processing").fallback =
                Some("deadline exceeded".into());
            entry
        };
        let entries = [
            rewritten(1, "Chat", 900),
            rewritten(1, "Chat", 300),
            rewritten(1, "Chat", 3_200),
            // A failed or timed-out run is not Output time, even when its
            // profile runs otherwise.
            fallback("Chat", 20),
            // An even count: the median is the mean of 400 and 900.
            rewritten(1, "Email", 900),
            rewritten(1, "Email", 400),
            rewritten(1, "Notes", 120),
            // A transformation-only run is not a mode.
            rewritten(1, crate::dictation::processor::CUSTOM_TRANSFORMATIONS, 400),
            rewritten(1, "  ", 500),
            entry(1, "a", "a", None, 0),
        ];
        let stats = stats_of(&entries, 1);
        let times: Vec<_> = stats
            .output_time
            .iter()
            .map(|profile| {
                (
                    profile.profile.as_str(),
                    profile.runs,
                    profile.median_ms,
                    profile.slowest_ms,
                )
            })
            .collect();
        assert_eq!(
            times,
            [
                ("Chat", 3, 900, 3_200),
                ("Email", 2, 650, 900),
                ("Notes", 1, 120, 120)
            ]
        );
        // An entry without processing contributes no profile.
        let plain = stats_of(&[entry(1, "a", "a", None, 0)], 1);
        assert!(plain.output_time.is_empty());
    }

    #[test]
    fn output_time_names_the_four_most_run_profiles_by_name_on_a_tie() {
        let profiles = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
        let mut entries = Vec::new();
        for profile in profiles {
            for _ in 0..2 {
                entries.push(rewritten(1, profile, 100));
            }
        }
        // Alpha runs one more time than every other profile.
        entries.push(rewritten(1, "Alpha", 1_000));
        let stats = stats_of(&entries, 1);
        let names: Vec<_> = stats
            .output_time
            .iter()
            .map(|profile| profile.profile.as_str())
            .collect();
        assert_eq!(stats.output_time.len(), TOP_PROFILES);
        assert_eq!(names, ["Alpha", "Bravo", "Charlie", "Delta"]);
        assert_eq!(stats.output_time[0].runs, 3);
        assert_eq!(stats.output_time[0].median_ms, 100);
        assert_eq!(stats.output_time[0].slowest_ms, 1_000);
    }

    #[test]
    fn the_json_names_every_daily_and_output_time_field() {
        let entries = [rewritten(1, "Chat", 900), entry(1, "a", "a", None, 0)];
        let stats = stats_of(&entries, 1);
        let json = serde_json::to_value(&stats).unwrap();
        let keys = |value: &serde_json::Value| {
            let mut keys: Vec<String> = value
                .as_object()
                .expect("an object")
                .keys()
                .cloned()
                .collect();
            keys.sort_unstable();
            keys
        };
        let day = &json["daily"][0];
        assert_eq!(keys(day), ["day", "dictations"]);
        assert_eq!(day["day"], -28);
        assert_eq!(day["dictations"], 0);
        assert_eq!(json["typing_wpm"], crate::app_settings::DEFAULT_TYPING_WPM);
        assert_eq!(json["time_saved_ms"], 3_000);
        assert_eq!(json["hours"].as_array().map(Vec::len), Some(24));
        assert_eq!(json["busiest_hour"], 0);
        let time = &json["output_time"][0];
        assert_eq!(keys(time), ["median_ms", "profile", "runs", "slowest_ms"]);
        assert_eq!(time["profile"], "Chat");
        assert_eq!(time["runs"], 1);
    }

    #[test]
    fn a_streak_counts_back_from_today_or_yesterday_and_stops_at_a_gap() {
        let entries = [
            entry(3, "a", "a", None, 0),
            entry(5, "a", "a", None, 0),
            entry(6, "a", "a", None, 0),
            entry(7, "a", "a", None, 0),
        ];
        let streak = |today| stats_of(&entries, today).streak_days;
        assert_eq!(streak(7), 3);
        // Nothing yet today: yesterday's run still counts.
        assert_eq!(streak(8), 3);
        // A day missed ends it.
        assert_eq!(streak(9), 0);
        assert_eq!(stats_of(&entries, 7).active_days, 4);
    }

    #[test]
    fn the_store_counts_every_retained_entry_and_skips_expired_ones() {
        use crate::history::{HistoryDraft, HistoryRetention};
        let directory =
            std::env::temp_dir().join(format!("hex-history-stats-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let mut store =
            HistoryStore::open(directory.join("history.json"), HistoryRetention::Day, 0);
        let draft = |text: &str| HistoryDraft {
            kind: HistoryKind::Dictation,
            raw_text: text.into(),
            final_text: text.into(),
            application: None,
            processing: None,
            audio_ms: 0,
            inference_ms: 0,
            total_ms: 0,
            recording: None,
            model: None,
        };
        store.record(draft("old words"), 1_000).unwrap();
        for _ in 0..(crate::history::MAX_SEARCH_RESULTS + 5) {
            store.record(draft("fresh"), 2_000).unwrap();
        }
        let stats = store.stats(DAY + 1_500, crate::app_settings::DEFAULT_TYPING_WPM);
        assert_eq!(
            stats.dictations,
            (crate::history::MAX_SEARCH_RESULTS + 5) as u64
        );
        assert_eq!(stats.words, stats.dictations);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn civil_days_count_from_the_epoch() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    }
}

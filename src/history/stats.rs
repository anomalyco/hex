//! Labs › Dictation stats (issue 790): a summary of the retained
//! dictations only, computed when asked. Nothing is stored beyond History
//! itself, so the stats end with the entries retention removes.

use std::collections::HashMap;

use serde::Serialize;

use super::{History, HistoryEntry, HistoryStore};

/// How many applications the summary names.
pub const TOP_APPS: usize = 5;

/// One application and how many retained dictations went to it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AppCount {
    pub application: String,
    pub dictations: u64,
}

/// What the retained entries add up to.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct HistoryStats {
    pub dictations: u64,
    /// Words in the text that was inserted, split on whitespace.
    pub words: u64,
    /// Recorded audio across the entries, in milliseconds.
    pub audio_ms: u64,
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
    /// The most dictated-into applications, most first, then by name.
    pub top_apps: Vec<AppCount>,
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
/// days in the app), and `today` is the current one.
pub fn summarize<'a>(
    entries: impl IntoIterator<Item = &'a HistoryEntry>,
    today: i64,
    day_of: impl Fn(u64) -> i64,
) -> HistoryStats {
    let mut stats = HistoryStats::default();
    let mut timed_words = 0_u64;
    let mut timed_ms = 0_u64;
    let mut apps: HashMap<&str, u64> = HashMap::new();
    let mut days = std::collections::BTreeSet::new();
    for entry in entries {
        stats.dictations += 1;
        let words = entry.final_text.split_whitespace().count() as u64;
        stats.words += words;
        stats.audio_ms = stats.audio_ms.saturating_add(entry.audio_ms);
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
        days.insert(day_of(entry.timestamp_ms));
    }
    stats.words_per_minute =
        (timed_ms > 0).then(|| ((timed_words as f64 * 60_000.0) / timed_ms as f64).round() as u32);
    stats.active_days = days.len() as u32;
    let mut day = if days.contains(&today) {
        today
    } else {
        today - 1
    };
    while days.contains(&day) {
        stats.streak_days += 1;
        day -= 1;
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
    stats
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
    pub fn stats(&self, now_ms: u64) -> HistoryStats {
        let cutoff = self
            .retention
            .max_age_ms()
            .map(|max_age| now_ms.saturating_sub(max_age));
        summarize(
            self.entries
                .iter()
                .filter(|entry| cutoff.is_none_or(|cutoff| entry.timestamp_ms >= cutoff)),
            local_day(now_ms),
            local_day,
        )
    }
}

impl History {
    /// The summary of every retained entry (Labs › Dictation stats).
    pub fn stats(&self) -> HistoryStats {
        self.synced().stats(super::now_ms())
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
        }
    }

    fn utc_day(timestamp_ms: u64) -> i64 {
        (timestamp_ms / DAY) as i64
    }

    #[test]
    fn words_speed_and_changes_add_up() {
        let entries = [
            entry(10, "one two three", "One two three.", Some("Zed"), 3_000),
            entry(10, "four five", "four five", Some("Slack"), 3_000),
            // No audio: its words count, but not toward the speed.
            entry(10, "six", "six", None, 0),
        ];
        let stats = summarize(&entries, 10, utc_day);
        assert_eq!(stats.dictations, 3);
        assert_eq!(stats.words, 6);
        assert_eq!(stats.audio_ms, 6_000);
        // Five words in six seconds.
        assert_eq!(stats.words_per_minute, Some(50));
        assert_eq!(stats.changed_by_output, 1);
        assert_eq!(stats.changed_percent(), Some(33));
    }

    #[test]
    fn nothing_retained_is_all_zero() {
        let stats = summarize(std::iter::empty(), 10, utc_day);
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
        let stats = summarize(&entries, 1, utc_day);
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
    fn a_streak_counts_back_from_today_or_yesterday_and_stops_at_a_gap() {
        let entries = [
            entry(3, "a", "a", None, 0),
            entry(5, "a", "a", None, 0),
            entry(6, "a", "a", None, 0),
            entry(7, "a", "a", None, 0),
        ];
        let streak = |today| summarize(&entries, today, utc_day).streak_days;
        assert_eq!(streak(7), 3);
        // Nothing yet today: yesterday's run still counts.
        assert_eq!(streak(8), 3);
        // A day missed ends it.
        assert_eq!(streak(9), 0);
        assert_eq!(summarize(&entries, 7, utc_day).active_days, 4);
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
        };
        store.record(draft("old words"), 1_000).unwrap();
        for _ in 0..(crate::history::MAX_SEARCH_RESULTS + 5) {
            store.record(draft("fresh"), 2_000).unwrap();
        }
        let stats = store.stats(DAY + 1_500);
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

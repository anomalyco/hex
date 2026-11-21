//! Labs › Dictation stats: an owner-only daily rollup of dictations that
//! outlives History's retention window.
//!
//! While the lab is on, every entry History records also adds its day, word
//! count, recorded time, and local hour here. The file holds no text and no
//! application names: per local day it keeps only a count, its words and
//! audio, and 24 hourly counts. Days older than [`MAX_DAYS`] are dropped, so
//! the file stays small however long the lab stays on. History retention Off
//! records nothing, here or in History, and Clear all history deletes it.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::stats::{local_day, local_hour, words_per_minute};

/// The rollup file's format version.
const VERSION: u32 = 1;
/// How many recent local days the rollup keeps.
pub const MAX_DAYS: i64 = 400;
/// The file stays well under this: 400 days of bounded hourly counts.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const FILE_NAME: &str = "usage.json";

/// One local day's dictations, without their text or applications.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct UsageDay {
    /// Days since 1970-01-01, local time (see [`local_day`]).
    pub day: i64,
    pub dictations: u64,
    pub words: u64,
    /// Recorded audio across the day's entries, in milliseconds.
    pub audio_ms: u64,
    /// Dictations by local hour, index 0 = midnight.
    #[serde(default)]
    pub hours: [u64; 24],
}

/// The rollup file beside the history store.
pub fn path(history: &Path) -> PathBuf {
    history.with_file_name(FILE_NAME)
}

#[derive(Serialize)]
struct SavedUsage<'a> {
    version: u32,
    days: &'a [UsageDay],
}

#[derive(Deserialize)]
struct LoadedUsage {
    #[serde(default = "version")]
    version: u32,
    #[serde(default)]
    days: Vec<UsageDay>,
}

const fn version() -> u32 {
    VERSION
}

/// The daily rollup, oldest day first.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Usage {
    days: Vec<UsageDay>,
}

impl Usage {
    /// A rollup of `days`, merged by day, oldest first.
    pub fn from_days(days: impl IntoIterator<Item = UsageDay>) -> Self {
        let mut usage = Self::default();
        for day in days {
            usage.merge(day);
        }
        usage
    }

    /// One day's counts, if the day has any.
    pub fn day(&self, day: i64) -> Option<&UsageDay> {
        self.days.iter().find(|usage| usage.day == day)
    }

    /// Every day, oldest first.
    pub fn days(&self) -> &[UsageDay] {
        &self.days
    }

    pub fn dictations(&self) -> u64 {
        self.days
            .iter()
            .fold(0, |total, day| total.saturating_add(day.dictations))
    }

    pub fn words(&self) -> u64 {
        self.days
            .iter()
            .fold(0, |total, day| total.saturating_add(day.words))
    }

    pub fn audio_ms(&self) -> u64 {
        self.days
            .iter()
            .fold(0, |total, day| total.saturating_add(day.audio_ms))
    }

    /// Dictations by local hour across every kept day.
    pub fn hours(&self) -> [u64; 24] {
        let mut hours = [0_u64; 24];
        for day in &self.days {
            for (total, count) in hours.iter_mut().zip(day.hours) {
                *total = total.saturating_add(count);
            }
        }
        hours
    }

    /// The local hour with the most dictations, earliest on a tie.
    pub fn busiest_hour(&self) -> Option<u8> {
        let mut busiest: Option<(u64, u8)> = None;
        for (hour, count) in self.hours().into_iter().enumerate() {
            if count > 0 && busiest.is_none_or(|(best, _)| count > best) {
                busiest = Some((count, hour as u8));
            }
        }
        busiest.map(|(_, hour)| hour)
    }

    pub fn words_per_minute(&self) -> Option<u32> {
        words_per_minute(self.words(), self.audio_ms())
    }

    /// Consecutive local days with a dictation, ending today, or yesterday
    /// when there is none yet today.
    pub fn streak_days(&self, today: i64) -> u32 {
        let mut day = if self.dictations_on(today) > 0 {
            today
        } else {
            today - 1
        };
        let mut streak = 0;
        while self.dictations_on(day) > 0 {
            streak += 1;
            day -= 1;
        }
        streak
    }

    /// How many dictations `day` holds.
    pub fn dictations_on(&self, day: i64) -> u64 {
        self.day(day).map_or(0, |usage| usage.dictations)
    }

    /// Adds one recorded dictation to `day`'s counts.
    pub fn add(&mut self, day: i64, hour: usize, words: u64, audio_ms: u64) {
        let position = self.days.partition_point(|usage| usage.day < day);
        if let Some(existing) = self.days.get_mut(position).filter(|usage| usage.day == day) {
            existing.dictations = existing.dictations.saturating_add(1);
            existing.words = existing.words.saturating_add(words);
            existing.audio_ms = existing.audio_ms.saturating_add(audio_ms);
            let hour = existing.hours.get_mut(hour.min(23)).expect("24 hours");
            *hour = hour.saturating_add(1);
            return;
        }
        let mut hours = [0_u64; 24];
        hours[hour.min(23)] = 1;
        self.days.insert(
            position,
            UsageDay {
                day,
                dictations: 1,
                words,
                audio_ms,
                hours,
            },
        );
    }

    /// Merges a whole day's counts into this rollup.
    fn merge(&mut self, day: UsageDay) {
        let position = self.days.partition_point(|usage| usage.day < day.day);
        if let Some(existing) = self
            .days
            .get_mut(position)
            .filter(|usage| usage.day == day.day)
        {
            existing.dictations = existing.dictations.saturating_add(day.dictations);
            existing.words = existing.words.saturating_add(day.words);
            existing.audio_ms = existing.audio_ms.saturating_add(day.audio_ms);
            for (total, count) in existing.hours.iter_mut().zip(day.hours) {
                *total = total.saturating_add(count);
            }
            return;
        }
        self.days.insert(position, day);
    }

    /// Drops days older than [`MAX_DAYS`] and keeps at most that many.
    pub fn bound(&mut self, today: i64) {
        self.days.retain(|day| day.day > today - MAX_DAYS);
        if self.days.len() > MAX_DAYS as usize {
            self.days.drain(..self.days.len() - MAX_DAYS as usize);
        }
    }

    /// Reads the rollup at `path`, bounded to [`MAX_DAYS`] ending at
    /// `today`. A missing file is an empty rollup; an unreadable or invalid
    /// one is logged and reads empty, and the next write keeps it aside as
    /// `usage.json.corrupt` ([`count`]).
    pub fn load(path: &Path, today: i64) -> Self {
        let mut usage = match read(path) {
            Ok(usage) => usage,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "could not read the usage rollup");
                Self::default()
            }
        };
        usage.bound(today);
        usage
    }

    /// Writes the rollup atomically and owner-only, bounded to
    /// [`MAX_DAYS`] ending at `today`.
    pub fn save(&mut self, path: &Path, today: i64) -> io::Result<()> {
        self.bound(today);
        if let Some(parent) = path.parent() {
            crate::app_paths::create_private_dir(parent)?;
        }
        let saved = SavedUsage {
            version: VERSION,
            days: &self.days,
        };
        let json = serde_json::to_vec(&saved).map_err(io::Error::other)?;
        crate::durable::write_private(path, &json)
    }
}

fn read(path: &Path) -> io::Result<Usage> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(io::Error::other("the usage rollup exceeds the read limit"));
    }
    let loaded: LoadedUsage = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if loaded.version != VERSION {
        return Err(io::Error::other("unsupported usage rollup version"));
    }
    let mut days = loaded.days;
    days.sort_by_key(|day| day.day);
    let mut usage = Usage::default();
    for day in days {
        usage.merge(day);
    }
    Ok(usage)
}

/// Records one dictation in the rollup at `path`. A malformed or unreadable
/// file is moved aside as `usage.json.corrupt`, as History does, so a
/// damaged year is never silently overwritten; only a missing file starts
/// empty. A write failure is the caller's to log: capture and paste never
/// wait on it.
pub fn count(path: &Path, timestamp_ms: u64, words: u64, audio_ms: u64) -> io::Result<()> {
    let today = local_day(timestamp_ms);
    let mut usage = read_for_count(path)?;
    usage.add(today, local_hour(timestamp_ms), words, audio_ms);
    usage.save(path, today)
}

/// [`read`] for a write: only NotFound is an empty rollup. Any other failure
/// preserves the file beside it before starting fresh, and a file that
/// cannot be moved is an error rather than an overwrite.
fn read_for_count(path: &Path) -> io::Result<Usage> {
    match read(path) {
        Ok(usage) => Ok(usage),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Usage::default()),
        Err(error) => {
            let backup = path.with_extension("json.corrupt");
            fs::rename(path, &backup)?;
            tracing::warn!(
                %error,
                path = %path.display(),
                backup = %backup.display(),
                "the usage rollup was damaged; kept it aside and started fresh"
            );
            Ok(Usage::default())
        }
    }
}

/// Deletes the rollup with Clear all history. A missing file is already
/// clear; a failure is logged and left for the next Clear.
pub fn clear(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "could not clear the usage rollup");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::stats::local_day;

    fn temp_dir(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("hex-usage-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn a_day_accumulates_its_hours_words_and_audio() {
        let mut usage = Usage::default();
        let today = local_day(1_700_000_000_000);
        usage.add(today, 9, 4, 1_000);
        usage.add(today, 9, 6, 2_000);
        usage.add(today, 14, 2, 500);

        assert_eq!(usage.dictations(), 3);
        assert_eq!(usage.words(), 12);
        assert_eq!(usage.audio_ms(), 3_500);
        assert_eq!(usage.hours()[9], 2);
        assert_eq!(usage.hours()[14], 1);
        assert_eq!(usage.busiest_hour(), Some(9));
        assert_eq!(usage.dictations_on(today), 3);
    }

    #[test]
    fn several_days_stay_oldest_first_and_total_up() {
        let mut usage = Usage::default();
        usage.add(10, 8, 3, 1_000);
        usage.add(12, 8, 5, 1_000);
        usage.add(11, 20, 2, 1_000);

        assert_eq!(
            usage.days().iter().map(|day| day.day).collect::<Vec<_>>(),
            [10, 11, 12]
        );
        assert_eq!(usage.dictations(), 3);
        assert_eq!(usage.words(), 10);
        assert_eq!(usage.busiest_hour(), Some(8));
    }

    #[test]
    fn the_rollup_keeps_only_the_last_four_hundred_days() {
        let mut usage = Usage::default();
        for day in 0..(MAX_DAYS + 25) {
            usage.add(day, 12, 1, 100);
        }
        usage.bound(MAX_DAYS + 24);
        assert_eq!(usage.days().len(), MAX_DAYS as usize);
        assert_eq!(usage.days().first().map(|day| day.day), Some(25));
        assert_eq!(usage.days().last().map(|day| day.day), Some(MAX_DAYS + 24));

        // A day exactly MAX_DAYS old is dropped; the one inside it stays.
        let mut edge = Usage::default();
        edge.add(100 - MAX_DAYS, 12, 1, 100);
        edge.add(100 - MAX_DAYS + 1, 12, 1, 100);
        edge.bound(100);
        assert_eq!(
            edge.days().iter().map(|day| day.day).collect::<Vec<_>>(),
            [100 - MAX_DAYS + 1]
        );
    }

    #[test]
    fn a_streak_counts_back_from_today_or_yesterday() {
        let mut usage = Usage::default();
        for day in [5, 6, 7] {
            usage.add(day, 9, 1, 100);
        }
        assert_eq!(usage.streak_days(7), 3);
        assert_eq!(usage.streak_days(8), 3);
        assert_eq!(usage.streak_days(10), 0);
    }

    #[test]
    fn a_count_round_trips_through_the_owner_only_file() {
        let directory = temp_dir("round-trip");
        let path = directory.join("usage.json");
        let timestamp = 1_700_000_000_000;
        count(&path, timestamp, 12, 4_000).unwrap();
        count(&path, timestamp, 3, 1_000).unwrap();

        let today = local_day(timestamp);
        let usage = Usage::load(&path, today);
        assert_eq!(usage.dictations(), 2);
        assert_eq!(usage.words(), 15);
        assert_eq!(usage.audio_ms(), 5_000);
        assert_eq!(usage.hours()[local_hour(timestamp)], 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        clear(&path);
        assert!(!path.exists());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_damaged_file_reads_empty_and_is_left_for_the_next_write() {
        let directory = temp_dir("damaged");
        let path = directory.join("usage.json");
        fs::write(
            &path,
            br#"{"version":1,"days":[{"day":1,"dictations":"many"}]}"#,
        )
        .unwrap();
        let usage = Usage::load(&path, 10);
        assert_eq!(usage, Usage::default());
        assert!(path.exists());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_count_keeps_a_damaged_rollup_aside_instead_of_overwriting_it() {
        let directory = temp_dir("damaged-count");
        let path = directory.join("usage.json");
        let damaged = br#"{"version":1,"days":[{"day":1,"dictations":"many"}]}"#;
        fs::write(&path, damaged).unwrap();
        let timestamp = 1_700_000_000_000;
        count(&path, timestamp, 7, 2_000).unwrap();

        let backup = path.with_extension("json.corrupt");
        assert_eq!(fs::read(&backup).unwrap(), damaged);
        let usage = Usage::load(&path, local_day(timestamp));
        assert_eq!(usage.dictations(), 1);
        assert_eq!(usage.words(), 7);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn days_names_every_kept_day() {
        let usage = Usage::from_days([
            UsageDay {
                day: 3,
                dictations: 1,
                ..UsageDay::default()
            },
            UsageDay {
                day: 1,
                dictations: 2,
                ..UsageDay::default()
            },
        ]);
        assert_eq!(
            usage.days().iter().map(|day| day.day).collect::<Vec<_>>(),
            [1, 3]
        );
        assert_eq!(usage.dictations(), 3);
    }
}

//! The one clock: the Mach host clock that `ScreenCaptureKit` frame display times, `CGEvent`
//! timestamps and Core Audio host times all use, plus an exact rational media time.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

/// A point on the Mach host clock, in native ticks (`mach_absolute_time`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostTime(u64);

impl HostTime {
    /// Wraps raw Mach ticks.
    #[must_use]
    pub const fn from_ticks(ticks: u64) -> Self {
        Self(ticks)
    }

    /// Raw Mach ticks.
    #[must_use]
    pub const fn ticks(self) -> u64 {
        self.0
    }

    /// The host clock now.
    #[must_use]
    pub fn now() -> Self {
        // SAFETY: `mach_absolute_time` has no preconditions.
        Self(unsafe { mach2::mach_time::mach_absolute_time() })
    }

    /// From nanoseconds on the host clock, such as a Core Audio stream instant.
    #[must_use]
    pub fn from_nanos(nanos: u64) -> Self {
        let timebase = Timebase::current();
        Self(scale(nanos, timebase.denom, timebase.numer))
    }

    /// Nanoseconds on the host clock.
    #[must_use]
    pub fn as_nanos(self) -> u64 {
        let timebase = Timebase::current();
        scale(self.0, timebase.numer, timebase.denom)
    }
}

/// The Mach timebase: nanoseconds = ticks × `numer` / `denom`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timebase {
    pub numer: u32,
    pub denom: u32,
}

impl Timebase {
    /// Reads the kernel's timebase. It never changes while the machine runs, and reading it is a
    /// shared-page load, so nothing caches it.
    #[must_use]
    pub fn current() -> Self {
        let mut info = mach2::mach_time::mach_timebase_info_data_t { numer: 0, denom: 0 };
        // SAFETY: `info` is a valid, writable `mach_timebase_info_data_t`.
        unsafe { mach2::mach_time::mach_timebase_info(&raw mut info) };
        if info.numer == 0 || info.denom == 0 {
            return Self { numer: 1, denom: 1 };
        }
        Self {
            numer: info.numer,
            denom: info.denom,
        }
    }
}

fn scale(value: u64, numer: u32, denom: u32) -> u64 {
    let scaled = u128::from(value) * u128::from(numer) / u128::from(denom.max(1));
    u64::try_from(scaled).unwrap_or(u64::MAX)
}

/// Whole milliseconds from `start` to `end`, if `end` is not earlier.
#[must_use]
pub fn host_delta_millis(start: HostTime, end: HostTime) -> Option<u64> {
    let ticks = end.0.checked_sub(start.0)?;
    Some(HostTime(ticks).as_nanos() / 1_000_000)
}

/// A canonical, non-negative rational timestamp, compared exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MediaTime {
    value: i64,
    timescale: i32,
}

impl MediaTime {
    pub const ZERO: Self = Self {
        value: 0,
        timescale: 1,
    };

    /// A normalized `value / timescale`, or `None` when negative, non-positive in scale, or too
    /// large to represent.
    #[must_use]
    pub fn new(value: i64, timescale: i32) -> Option<Self> {
        if value < 0 || timescale <= 0 {
            return None;
        }
        Self::from_ratio(i128::from(value), i128::from(timescale))
    }

    #[must_use]
    pub fn from_millis(millis: u64) -> Option<Self> {
        Self::from_ratio(i128::from(millis), 1_000)
    }

    #[must_use]
    pub const fn value(self) -> i64 {
        self.value
    }

    #[must_use]
    pub const fn timescale(self) -> i32 {
        self.timescale
    }

    #[must_use]
    pub fn checked_add(self, other: Self) -> Option<Self> {
        let denominator = i128::from(self.timescale) * i128::from(other.timescale);
        let numerator = i128::from(self.value) * i128::from(other.timescale)
            + i128::from(other.value) * i128::from(self.timescale);
        Self::from_ratio(numerator, denominator)
    }

    /// Rounds up to whole milliseconds.
    #[must_use]
    pub fn ceil_to_millis(self) -> Option<u64> {
        let numerator = i128::from(self.value) * 1_000;
        let denominator = i128::from(self.timescale);
        u64::try_from(numerator.div_euclid(denominator) + i128::from(numerator % denominator != 0))
            .ok()
    }

    fn from_ratio(numerator: i128, denominator: i128) -> Option<Self> {
        if numerator < 0 || denominator <= 0 {
            return None;
        }
        if numerator == 0 {
            return Some(Self::ZERO);
        }
        let divisor = gcd(numerator, denominator);
        Some(Self {
            value: i64::try_from(numerator / divisor).ok()?,
            timescale: i32::try_from(denominator / divisor).ok()?,
        })
    }
}

impl PartialOrd for MediaTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MediaTime {
    fn cmp(&self, other: &Self) -> Ordering {
        (i128::from(self.value) * i128::from(other.timescale))
            .cmp(&(i128::from(other.value) * i128::from(self.timescale)))
    }
}

fn gcd(mut a: i128, mut b: i128) -> i128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.abs().max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_time_normalizes_and_compares_exactly() {
        assert_eq!(MediaTime::new(2_000, 30_000), MediaTime::new(1, 15));
        assert!(MediaTime::new(1, 15) < MediaTime::new(1, 14));
        assert_eq!(MediaTime::new(-1, 1), None);
        assert_eq!(MediaTime::new(1, 0), None);
        assert_eq!(
            MediaTime::new(1, 3).and_then(MediaTime::ceil_to_millis),
            Some(334)
        );
        assert_eq!(
            MediaTime::from_millis(1_500).and_then(|t| t.checked_add(MediaTime::new(1, 2)?)),
            MediaTime::new(2, 1)
        );
    }

    #[test]
    fn host_time_round_trips_through_nanos() {
        let now = HostTime::now();
        let back = HostTime::from_nanos(now.as_nanos());
        assert!(now.ticks().abs_diff(back.ticks()) <= 1);
        let later = HostTime::from_ticks(now.ticks() + HostTime::from_nanos(1_500_000_000).ticks());
        assert_eq!(host_delta_millis(now, later), Some(1_500));
        assert_eq!(host_delta_millis(later, now), None);
    }
}

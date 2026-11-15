//! Calendar days, independent of how long a local day lasts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct LocalDate {
    pub year: i64,
    pub month: i64,
    pub day: i64,
}

impl LocalDate {
    /// A lab or release's YYYYMMDD date is already a day, not a UTC instant.
    pub fn from_ymd(ymd: u32) -> Self {
        Self {
            year: (ymd / 10000).into(),
            month: (ymd / 100 % 100).into(),
            day: (ymd % 100).into(),
        }
    }

    /// The `YYYY-MM-DD` at the start of an ISO local time.
    pub fn parse(iso: &str) -> Option<Self> {
        let date = iso.get(0..10)?;
        let mut parts = date.split('-');
        let year = parts.next()?.parse().ok()?;
        let month = parts.next()?.parse().ok()?;
        let day = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self { year, month, day })
    }

    pub fn days(self) -> i64 {
        crate::meeting::query::days_from_civil(self.year, self.month, self.day)
    }

    /// The weekday, `0` for Sunday; 1970-01-01 was a Thursday.
    pub fn weekday(self) -> usize {
        usize::try_from((self.days() + 4).rem_euclid(7)).unwrap_or_default()
    }
}

pub(crate) fn local_date(ms: i64) -> Option<LocalDate> {
    let iso = crate::meeting::query::iso_local(u64::try_from(ms).unwrap_or_default());
    LocalDate::parse(&iso)
}

use std::ops::Add;
#[cfg(target_os = "macos")]
use std::sync::OnceLock;
use std::time::Duration;
#[cfg(target_os = "windows")]
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CaptureInstant(u64);

impl CaptureInstant {
    pub const ZERO: Self = Self(0);

    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    #[cfg(target_os = "macos")]
    pub fn now() -> Self {
        unsafe extern "C" {
            fn mach_absolute_time() -> u64;
        }
        Self::from_mach_ticks(unsafe { mach_absolute_time() })
    }

    /// The monotonic clock. Linux's microphone streams count from when they
    /// opened, so the capture timeline anchors them to this.
    #[cfg(target_os = "linux")]
    pub fn now() -> Self {
        let mut now = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `now` is a valid timespec to fill.
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) };
        Self(now.tv_sec as u64 * 1_000_000_000 + now.tv_nsec as u64)
    }

    /// The monotonic clock, counted from its first read (one second in, so
    /// it is never [`Self::ZERO`]). Windows' microphone streams keep their
    /// own clock, so the capture timeline anchors them to this.
    #[cfg(target_os = "windows")]
    pub fn now() -> Self {
        static ANCHOR: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);
        let since = u64::try_from(ANCHOR.elapsed().as_nanos()).unwrap_or(u64::MAX);
        Self(since.saturating_add(1_000_000_000))
    }

    #[cfg(target_os = "macos")]
    pub fn from_mach_ticks(ticks: u64) -> Self {
        let Some((numerator, denominator)) = mach_timebase() else {
            return Self::ZERO;
        };
        Self(scale_mach_ticks(ticks, numerator, denominator))
    }

    pub fn from_stream(instant: cpal::StreamInstant) -> Self {
        Self(u64::try_from(instant.as_nanos()).unwrap_or(u64::MAX))
    }

    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        let nanos = u64::try_from(duration.as_nanos()).ok()?;
        self.0.checked_add(nanos).map(Self)
    }

    pub fn checked_sub(self, duration: Duration) -> Option<Self> {
        let nanos = u64::try_from(duration.as_nanos()).ok()?;
        self.0.checked_sub(nanos).map(Self)
    }

    pub fn checked_duration_since(self, earlier: Self) -> Option<Duration> {
        self.0.checked_sub(earlier.0).map(Duration::from_nanos)
    }

    pub fn duration_since(self, earlier: Self) -> Duration {
        Duration::from_nanos(self.0.saturating_sub(earlier.0))
    }

    pub fn sub_frames(self, frames: usize, sample_rate: u32) -> Self {
        self.checked_sub(duration_for_frames(frames, sample_rate))
            .unwrap_or(self)
    }
}

pub fn duration_for_frames(frames: usize, sample_rate: u32) -> Duration {
    let nanos = frames as u128 * 1_000_000_000 / u128::from(sample_rate);
    Duration::from_nanos(nanos as u64)
}

#[cfg(target_os = "macos")]
pub(super) fn scale_mach_ticks(ticks: u64, numerator: u32, denominator: u32) -> u64 {
    let nanos = u128::from(ticks) * u128::from(numerator) / u128::from(denominator);
    u64::try_from(nanos).unwrap_or(u64::MAX)
}

#[cfg(target_os = "macos")]
fn mach_timebase() -> Option<(u32, u32)> {
    #[repr(C)]
    struct MachTimebaseInfo {
        numer: u32,
        denom: u32,
    }

    unsafe extern "C" {
        fn mach_timebase_info(info: *mut MachTimebaseInfo) -> i32;
    }

    static TIMEBASE: OnceLock<Option<(u32, u32)>> = OnceLock::new();
    *TIMEBASE.get_or_init(|| {
        let mut info = MachTimebaseInfo { numer: 0, denom: 0 };
        let status = unsafe { mach_timebase_info(&mut info) };
        (status == 0 && info.denom != 0).then_some((info.numer, info.denom))
    })
}

impl Add<Duration> for CaptureInstant {
    type Output = Self;

    fn add(self, duration: Duration) -> Self::Output {
        self.checked_add(duration)
            .expect("capture instant overflowed")
    }
}

//! Meeting clock times: English's 12-hour clock and every other language's
//! 24-hour one.

use super::{ENGLISH, current};

/// A meeting clock time in the active language: `1:05 PM` in English and
/// `13:05` in every other.
pub(crate) fn clock_time(hour: u32, minute: u32) -> String {
    if current() == ENGLISH {
        twelve_hour_clock(hour, minute)
    } else {
        format!("{hour:02}:{minute:02}")
    }
}

/// `1:05 PM`: English's 12-hour clock in any language, for reading titles
/// and dates written before meeting times followed the language.
pub(crate) fn twelve_hour_clock(hour: u32, minute: u32) -> String {
    let period = if hour < 12 { "AM" } else { "PM" };
    let hour = match hour % 12 {
        0 => 12,
        hour => hour,
    };
    format!("{hour}:{minute:02} {period}")
}

//! The hourly strip and the 53-week heatmap in the Dictation stats dialog:
//! a year of dictations per local day from the rollup that outlives
//! History's retention, and 24 bars for the hours dictations land in. Both
//! borrow the existing chart's look and give screen readers one labelled
//! image.

use std::collections::HashMap;

use rune::{El, col, el, row, text};

use super::super::controls::Paint;
use super::super::lists::section_label;
use crate::desktop::i18n::numbers;
use crate::desktop::i18n::{clock_time, tr, tr_named};
use crate::history::stats::{busiest_hour, local_day};
use crate::history::usage::Usage;

/// The heatmap's cell size and gap: 53 weeks and 52 gaps fill the dialog's
/// 424 px content width exactly.
const CELL: f32 = 6.0;
const CELL_GAP: f32 = 2.0;
/// How many weeks the heatmap covers, the current one last.
const WEEKS: i64 = 53;
const WEEKDAYS: i64 = 7;
/// The accent's opacity at each of the four non-empty levels.
const LEVELS: [f32; 4] = [0.25, 0.45, 0.7, 1.0];
/// The hourly strip's height and gap between hours.
const HOUR_HEIGHT: f32 = 30.0;
const HOUR_GAP: f32 = 3.0;

/// The Monday that starts the heatmap's first week, `52` weeks before the
/// current one.
fn grid_start(today: i64) -> i64 {
    // 1970-01-01 was a Thursday; 3 makes Monday 0.
    let weekday = (today + 3).rem_euclid(WEEKDAYS);
    today - weekday - (WEEKS - 1) * WEEKDAYS
}

/// Which of the four accent levels a day's count earns against the year's
/// busiest day; 0 is a day with no dictation.
fn level(count: u64, peak: u64) -> u8 {
    if count == 0 || peak == 0 {
        return 0;
    }
    let share = count as f64 / peak as f64;
    if share <= 0.25 {
        1
    } else if share <= 0.5 {
        2
    } else if share <= 0.75 {
        3
    } else {
        4
    }
}

/// A 53-week × 7-day heatmap of dictations per local day from the rollup.
pub(super) fn yearly(p: Paint, usage: &Usage) -> Option<El> {
    let today = local_day(crate::history::now_ms());
    let start = grid_start(today);
    // One lookup per cell, and the peak only over the weeks shown (the
    // rollup can hold up to 400 days).
    let counts: HashMap<i64, u64> = usage
        .days()
        .iter()
        .filter(|day| day.day >= start && day.day <= today)
        .map(|day| (day.day, day.dictations))
        .collect();
    let peak = counts.values().copied().max().filter(|peak| *peak > 0)?;
    let total = numbers::grouped(usage.dictations());
    let label = tr_named(
        "Dictations per day over the past year: {total} in total",
        &[("total", &total)],
    );
    let mut weeks = row()
        .key("history-stats-year")
        .role("img")
        .label(label)
        .w_full()
        .gap(CELL_GAP);
    for week in 0..WEEKS {
        let mut column = col().key(format!("week-{week}")).gap(CELL_GAP);
        for weekday in 0..WEEKDAYS {
            let day = start + week * WEEKDAYS + weekday;
            let count = counts.get(&day).copied().unwrap_or(0);
            column = column.child(cell(p, day, count, peak, day > today));
        }
        weeks = weeks.child(column);
    }
    Some(
        col()
            .key("history-stats-yearly")
            .gap(6.0)
            .child(section_label(p, "label", tr("Past year")))
            .child(weeks),
    )
}

/// One day's square: empty days faint, busy days a deeper accent.
fn cell(p: Paint, day: i64, count: u64, peak: u64, future: bool) -> El {
    let square = el()
        .key(format!("day-{day}"))
        .w(CELL)
        .h(CELL)
        .radius(p.k.radius.xs)
        .a11y_hidden();
    if future {
        return square;
    }
    let fill = match level(count, peak) {
        0 => p.k.alpha(p.faint(), 0.18),
        level => p.k.alpha(p.accent(), LEVELS[usize::from(level) - 1]),
    };
    square.fill(fill)
}

/// A compact strip of the day's 24 hours, the busiest in the accent colour.
/// `hours` and `busiest` come from the same source as the headline tile:
/// the rollup while Labs › Dictation stats keeps one, else retained entries.
pub(super) fn hours(p: Paint, hours: &[u64; 24], busiest: Option<u8>) -> Option<El> {
    let peak = hours.iter().copied().max().filter(|peak| *peak > 0)?;
    let busiest = busiest.or_else(|| busiest_hour(hours))?;
    let hour = clock_time(u32::from(busiest), 0);
    let label = tr_named("Dictations by hour: busiest at {hour}", &[("hour", &hour)]);
    let mut bars = row()
        .key("history-stats-hours")
        .role("img")
        .label(label)
        .w_full()
        .h(HOUR_HEIGHT)
        .gap(HOUR_GAP)
        .items_end();
    for (index, count) in hours.iter().copied().enumerate() {
        bars = bars.child(hour_bar(
            p,
            index,
            count,
            peak,
            index == usize::from(busiest),
        ));
    }
    Some(
        col()
            .key("history-stats-hourly")
            .gap(6.0)
            .child(
                row()
                    .key("head")
                    .gap(8.0)
                    .center_y()
                    .child(section_label(p, "label", tr("Busiest hour")).grow(1.0))
                    .child(
                        text(hour)
                            .key("hour")
                            .font(p.k.ty.small_medium)
                            .color(p.text()),
                    ),
            )
            .child(bars)
            .child(
                el().key("axis")
                    .w_full()
                    .h(p.hair)
                    .fill(p.k.c.border_muted)
                    .a11y_hidden(),
            ),
    )
}

/// One hour's bar, measured against the busiest hour.
fn hour_bar(p: Paint, hour: usize, count: u64, peak: u64, busiest: bool) -> El {
    let height = if count == 0 {
        0.0
    } else {
        // Any dictation is at least a visible sliver.
        ((count as f32 / peak as f32) * HOUR_HEIGHT).max(2.0)
    };
    el().key(format!("hour-{hour}"))
        .grow(1.0)
        .shrink()
        .h(height)
        .radius(p.k.radius.xs)
        .fill(if busiest { p.accent() } else { p.faint() })
        .a11y_hidden()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::usage::UsageDay;

    #[test]
    fn the_grid_starts_on_a_monday_and_ends_in_the_current_week() {
        for today in [19_000_i64, 20_700, 20_744] {
            let start = grid_start(today);
            assert_eq!((start + 3).rem_euclid(7), 0, "a Monday");
            assert!(start <= today);
            assert_eq!(
                start + (WEEKS - 1) * WEEKDAYS + 6,
                today - (today + 3).rem_euclid(WEEKDAYS) + 6,
                "the last column holds today's week"
            );
        }
    }

    #[test]
    fn busy_days_reach_deeper_accent_levels() {
        assert_eq!(level(0, 10), 0);
        assert_eq!(level(1, 10), 1);
        assert_eq!(level(3, 10), 2);
        assert_eq!(level(6, 10), 3);
        assert_eq!(level(10, 10), 4);
        assert_eq!(level(100, 0), 0);
    }

    #[test]
    fn a_rollup_day_reads_back_for_its_square() {
        let usage = Usage::from_days([UsageDay {
            day: 20_744,
            dictations: 4,
            words: 40,
            audio_ms: 2_000,
            ..UsageDay::default()
        }]);
        assert_eq!(usage.dictations_on(20_744), 4);
        assert_eq!(usage.dictations_on(20_743), 0);
    }
}

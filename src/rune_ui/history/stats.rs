//! Labs › Dictation stats: the dialog behind History's Stats button. It
//! headlines the retained history's totals, then shows the recent daily
//! activity, the apps dictations went to, how often Output changed the text,
//! and each mode's Output time. Sections with nothing to show are left out,
//! and the body scrolls while the window is short.

use rune::{Align, Cx, El, col, el, row, text};
use rune_kit::Progress;

use super::super::controls::{
    ButtonKind, METRICS, MODAL, ModalWidth, Paint, button, modal_panel, modal_sheet,
};
use super::super::list_detail::HANG;
use super::super::lists::{inline_slider, section_label};
use super::super::meetings::length;
use super::super::{Modal, Ui};
use crate::desktop::i18n::numbers;
use crate::desktop::i18n::{clock_time, tr, tr_fill, tr_named};
use crate::history::stats::{DayCount, HistoryStats, ProfileOutputTime};
use crate::history::usage::Usage;

/// A window's first frame has no measured title or footer yet; these stand
/// in until they are.
const TITLE_HEIGHT: f32 = 20.0;
const FOOTER_HEIGHT: f32 = 30.0;
/// A scrolling body never shrinks below this, however short the window.
const MIN_BODY_HEIGHT: f32 = 120.0;
/// The daily chart's height and the gap between its day bars.
const CHART_HEIGHT: f32 = 44.0;
const BAR_GAP: f32 = 3.0;
/// A top app's count on the trailing edge.
const COUNT_WIDTH: f32 = 34.0;
/// Room beside the body's scroll bar.
const SCROLL_BAR_INSET: f32 = 12.0;
/// The Labs › Stats on History strip's inner padding, gaps, and room above
/// the list; it hangs on History's own gutter ([`HANG`]).
const STRIP_PAD: f32 = 16.0;
const STRIP_PAD_Y: f32 = 8.0;
const STRIP_GAP: f32 = 24.0;
const STRIP_BOTTOM: f32 = 12.0;

/// The stats dialog's scrim and panel, while its button and stats show. Its
/// body scrolls while the window is too short for the sections, and takes
/// the sections' own height when they fit, so the title and Close stay put
/// without a scroll bar or a gap.
pub(in crate::rune_ui) fn modal(ui: &Ui, p: Paint, cx: &mut Cx<Ui>) -> Option<[El; 2]> {
    if !ui.settings.labs.history_stats {
        return None;
    }
    let stats = ui.history.stats.as_ref()?;
    if !ui.history.has_stats() {
        return None;
    }
    let numbers = headline_inputs(ui, stats);
    // The window is the layer the panel centers in; the title and footer are
    // measured so their real heights set the body's room.
    let window_height = cx
        .layout_size("history-stats-layer")
        .map_or(super::super::MIN_WINDOW_SIZE.1, |size| size[1]);
    let title_height = cx
        .layout_size("history-stats-dialog/title")
        .map_or(TITLE_HEIGHT, |size| size[1]);
    let footer_height = cx
        .layout_size("history-stats-dialog/footer")
        .map_or(FOOTER_HEIGHT, |size| size[1]);
    let content_height = cx.layout_size("history-stats-sections").map(|size| size[1]);
    let chrome = MODAL.top + title_height + 2.0 * MODAL.gap + footer_height + MODAL.bottom;
    let room = (window_height - 2.0 * MODAL.margin - chrome).max(MIN_BODY_HEIGHT);
    let (fade_top, fade_bottom) = super::super::SCROLL_FADE;
    // The measured sections set the mode; the first frame, with no measure
    // yet, draws them whole and settles into scrolling if they don't fit.
    let scrolling = content_height.is_some_and(|height| height + fade_top + fade_bottom > room);
    let mut sections = col().key("history-stats-sections").w_full().gap(16.0);
    if scrolling {
        // Keep the bar off the counts and timings while preserving the text rail.
        sections = sections.pr(SCROLL_BAR_INSET);
    }
    let mut sections = sections.child(tiles(p, &numbers));
    sections = sections.child(typing_speed(p, numbers.typing_wpm));
    if let Some(daily) = daily(p, stats) {
        sections = sections.child(daily);
    }
    if let Some(hours) = super::heatmap::hours(p, &numbers.hours, numbers.busiest_hour) {
        sections = sections.child(hours);
    }
    if let Some(yearly) = numbers
        .usage
        .and_then(|usage| super::heatmap::yearly(p, usage))
    {
        sections = sections.child(yearly);
    }
    if let Some(apps) = top_apps(p, stats) {
        sections = sections.child(apps);
    }
    sections = sections.child(changed(p, stats));
    if let Some(times) = output_time(p, stats) {
        sections = sections.child(times);
    }
    let body = if scrolling {
        col()
            .key("history-stats-body-frame")
            .pl(METRICS.row_px)
            .pr(METRICS.row_px - SCROLL_BAR_INSET)
            .h(room)
            .child(super::super::faded_body(
                p.k,
                "history-stats-body",
                sections,
                super::super::SHEET_FADE,
            ))
    } else {
        // No viewport at all: no scroll bar, no fade padding, and the
        // counts end on the Close button's edge.
        col()
            .key("history-stats-body")
            .px(METRICS.row_px)
            .child(sections)
    };
    let panel = modal_panel(
        p,
        "history-stats-dialog",
        tr("Dictation stats"),
        ModalWidth::Narrow,
        vec![body],
        vec![
            button(
                p,
                "history-stats-close",
                tr("Close"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| close(ui),
            )
            .default_button(),
        ],
    );
    Some(modal_sheet(p, "history-stats", panel, close))
}

/// Closes the stats dialog, from its button, Escape, or the scrim.
fn close(ui: &mut Ui) {
    ui.close_modal_if(Modal::HistoryStats);
}

/// The six headline numbers' inputs, read from the window's History: the
/// rollup while it has a dictation, the typing speed in effect, and the hours
/// both the heatmap and the busiest-hour tile use. The tiles and History's
/// strip share it, so the two can't drift.
struct Headline<'a> {
    stats: &'a HistoryStats,
    usage: Option<&'a Usage>,
    typing_wpm: u32,
    hours: [u64; 24],
    busiest_hour: Option<u8>,
}

fn headline_inputs<'a>(ui: &'a Ui, stats: &'a HistoryStats) -> Headline<'a> {
    let usage = ui
        .history
        .usage
        .as_ref()
        .filter(|usage| usage.dictations() > 0);
    let typing_wpm = ui
        .history
        .typing_speed_drag
        .unwrap_or_else(|| crate::app_settings::clamp_typing_wpm(ui.settings.typing_wpm));
    // The headline tile and the hourly strip read the same hours: the
    // rollup while Labs › Dictation stats has one, else retained entries.
    let (hours, busiest_hour) = match usage {
        Some(usage) => (usage.hours(), usage.busiest_hour()),
        None => (stats.hours, stats.busiest_hour),
    };
    Headline {
        stats,
        usage,
        typing_wpm,
        hours,
        busiest_hour,
    }
}

/// The six headline numbers, each with its key, value, and translated label.
/// The dialog's tiles and History's strip draw these, so they can't drift.
fn headline(input: &Headline) -> [(&'static str, String, &'static str); 6] {
    let all = input.usage;
    let stats = input.stats;
    let dictations = all.map_or(stats.dictations, Usage::dictations);
    let words = all.map_or(stats.words, Usage::words);
    let audio_ms = all.map_or(stats.audio_ms, Usage::audio_ms);
    let time_saved = crate::history::stats::time_saved_ms(words, audio_ms, input.typing_wpm);
    let words_per_minute = all.map_or(stats.words_per_minute, Usage::words_per_minute);
    let streak = all.map_or(stats.streak_days, |usage| {
        usage.streak_days(crate::history::stats::local_day(crate::history::now_ms()))
    });
    let wpm =
        words_per_minute.map_or_else(|| "–".to_owned(), |wpm| numbers::grouped(u64::from(wpm)));
    let busiest = input
        .busiest_hour
        .map_or_else(|| "–".to_owned(), |hour| clock_time(u32::from(hour), 0));
    [
        ("dictations", numbers::grouped(dictations), tr("Dictations")),
        ("words", numbers::grouped(words), tr("Words")),
        ("time-saved", length(time_saved), tr("Time saved")),
        ("wpm", wpm, tr("Words per minute")),
        (
            "streak",
            numbers::grouped(u64::from(streak)),
            tr("Daily streak"),
        ),
        ("busiest-hour", busiest, tr("Busiest hour")),
    ]
}

/// The six headline numbers, two rows of three equal-width tiles.
fn tiles(p: Paint, input: &Headline) -> El {
    let items = headline(input);
    col()
        .key("tiles")
        .w_full()
        .gap(8.0)
        .children(items.chunks(3).enumerate().map(|(index, chunk)| {
            chunk.iter().fold(
                row().key(format!("tiles-row-{index}")).w_full().gap(8.0),
                |line, (key, value, label)| line.child(tile(p, key, value, label)),
            )
        }))
}

/// Labs › Stats on History: the dialog's six headline numbers as one
/// compact row above History's list, while both labs and the stats are on.
/// Off, History is exactly as before.
pub(super) fn strip(ui: &Ui, p: Paint) -> Option<El> {
    if !crate::labs::history_stats_strip_enabled(&ui.settings) || !ui.history.has_stats() {
        return None;
    }
    let stats = ui.history.stats.as_ref()?;
    let numbers = headline_inputs(ui, stats);
    let cells = headline(&numbers).into_iter().fold(
        row()
            .key("bar")
            .w_full()
            .px(STRIP_PAD)
            .py(STRIP_PAD_Y)
            .gap(STRIP_GAP)
            .radius(p.k.radius.md)
            .fill(p.k.c.layer_01),
        |cells, (key, value, label)| cells.child(strip_cell(p, key, &value, label)),
    );
    Some(
        row()
            .key("history-stats-strip")
            .w_full()
            .px(HANG)
            .mb(STRIP_BOTTOM)
            .child(cells),
    )
}

/// One of the strip's numbers: a compact value over its muted label.
fn strip_cell(p: Paint, key: &str, value: &str, label: &str) -> El {
    col()
        .key(format!("strip-{key}"))
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(1.0)
        .child(
            text(value.to_owned())
                .key("value")
                .font(p.k.ty.small_medium)
                .color(p.text())
                .truncate(),
        )
        .child(
            text(label.to_owned())
                .key("label")
                .font(p.k.ty.small)
                .color(p.muted())
                .truncate(),
        )
}

/// The typing speed time saved is measured against, adjustable in place.
fn typing_speed(p: Paint, typing_wpm: u32) -> El {
    use crate::app_settings::{MAX_TYPING_WPM, MIN_TYPING_WPM};
    let range = (MAX_TYPING_WPM - MIN_TYPING_WPM) as f32;
    let value = (typing_wpm - MIN_TYPING_WPM) as f32 / range;
    let display = tr_fill("{} wpm", &typing_wpm.to_string());
    row()
        .key("history-stats-typing")
        .w_full()
        .gap(12.0)
        .center_y()
        .child(
            text(tr("Typing speed"))
                .key("label")
                .font(p.k.ty.small)
                .color(p.muted())
                .grow(1.0),
        )
        .child(inline_slider(
            p,
            "history-stats-typing-speed",
            tr("Typing speed"),
            value,
            display,
            160.0,
            move |ui: &mut Ui, position: f32, done: bool| {
                let wpm = crate::app_settings::clamp_typing_wpm(
                    (MIN_TYPING_WPM as f32 + position.clamp(0.0, 1.0) * range).round() as u32,
                );
                ui.history.typing_speed_drag = (!done).then_some(wpm);
                if done && ui.update(|settings| settings.typing_wpm = wpm) {
                    ui.history.typing_wpm = wpm;
                    ui.history.reload_stats(ui.settings.labs.history_stats);
                }
            },
        ))
}

/// One tile: the value over its label, on a raised surface.
fn tile(p: Paint, key: &str, value: &str, label: &str) -> El {
    col()
        .key(format!("tile-{key}"))
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(2.0)
        .px(12.0)
        .py(8.0)
        .radius(p.k.radius.md)
        .fill(p.k.c.layer_01)
        .child(
            text(value.to_owned())
                .key("value")
                .font(p.tabular(p.display()))
                .color(p.text())
                .truncate(),
        )
        .child(
            text(label.to_owned())
                .key("label")
                .font(p.k.ty.small)
                .color(p.muted())
                .wrap(),
        )
}

/// Dictations per local day over the recent window, today's bar in the
/// accent colour. No dictation in the window leaves the chart out.
fn daily(p: Paint, stats: &HistoryStats) -> Option<El> {
    let peak = stats
        .daily
        .iter()
        .map(|day| day.dictations)
        .max()
        .filter(|peak| *peak > 0)?;
    let today = stats.daily.last().map(|day| day.day);
    let today_count = stats.daily.last().map_or(0, |day| day.dictations);
    let today_count = numbers::grouped(today_count);
    let peak_count = numbers::grouped(peak);
    let label = tr_named(
        "Dictations per day: {today} today, {busiest} on the busiest day",
        &[("today", &today_count), ("busiest", &peak_count)],
    );
    let mut bars = row()
        .key("bars")
        .role("img")
        .label(label)
        .w_full()
        .h(CHART_HEIGHT)
        .gap(BAR_GAP)
        .items_end();
    for day in &stats.daily {
        bars = bars.child(bar(p, day, peak, today == Some(day.day)));
    }
    Some(
        col()
            .key("history-stats-daily")
            .gap(6.0)
            .child(
                row()
                    .key("head")
                    .gap(8.0)
                    .center_y()
                    .child(section_label(p, "label", tr("Dictations per day")).grow(1.0))
                    .child(
                        row()
                            .key("legend")
                            .gap(4.0)
                            .center_y()
                            .child(
                                el().key("swatch")
                                    .w(6.0)
                                    .h(6.0)
                                    .radius(p.k.radius.xs)
                                    .fill(p.accent()),
                            )
                            .child(
                                text(tr("Today"))
                                    .key("today")
                                    .font(p.k.ty.small)
                                    .color(p.muted()),
                            ),
                    ),
            )
            .child(bars)
            // An axis under the bars, so a day with nothing still reads as
            // an empty day.
            .child(
                el().key("axis")
                    .w_full()
                    .h(p.hair)
                    .fill(p.k.c.border_muted)
                    .a11y_hidden(),
            ),
    )
}

/// One day's bar: its height against the window's busiest day.
fn bar(p: Paint, day: &DayCount, peak: u64, today: bool) -> El {
    let height = if day.dictations == 0 {
        0.0
    } else {
        // Any dictation is at least a visible sliver.
        ((day.dictations as f32 / peak as f32) * CHART_HEIGHT).max(2.0)
    };
    el().key(format!("bar{}", day.day))
        .grow(1.0)
        .shrink()
        .h(height)
        .radius(p.k.radius.xs)
        .fill(if today { p.accent() } else { p.faint() })
        .a11y_hidden()
}

/// The apps dictations went to, each bar against the busiest one.
fn top_apps(p: Paint, stats: &HistoryStats) -> Option<El> {
    let top = stats.top_apps.first()?.dictations;
    if top == 0 {
        return None;
    }
    let rows = stats
        .top_apps
        .iter()
        .fold(col().key("apps").w_full().gap(8.0), |rows, app| {
            rows.child(
                row()
                    .key(format!("app-{}", app.application))
                    .w_full()
                    .gap(8.0)
                    .center_y()
                    .child(
                        text(app.application.clone())
                            .key("name")
                            .font(p.k.ty.small)
                            .color(p.text())
                            .w(METRICS.fact_label_w)
                            .truncate(),
                    )
                    .child(
                        Progress::new(Some(app.dictations as f32 / top as f32))
                            .build(p.k)
                            .grow(1.0)
                            .a11y_hidden(),
                    )
                    .child(
                        text(numbers::grouped(app.dictations))
                            .key("count")
                            .font(p.k.ty.small)
                            .color(p.muted())
                            .w(COUNT_WIDTH)
                            .align(Align::End),
                    ),
            )
        });
    Some(
        col()
            .key("history-stats-apps")
            .gap(8.0)
            .child(section_label(p, "label", tr("Top apps")))
            .child(rows),
    )
}

/// How often Output changed the text, as a meter.
fn changed(p: Paint, stats: &HistoryStats) -> El {
    let percent = stats.changed_percent().unwrap_or_default();
    col()
        .key("history-stats-changed")
        .gap(6.0)
        .child(
            row()
                .key("head")
                .gap(8.0)
                .center_y()
                .child(section_label(p, "label", tr("Changed by Output")).grow(1.0))
                .child(
                    text(format!("{percent}%"))
                        .key("percent")
                        .font(p.k.ty.small_medium)
                        .color(p.text()),
                ),
        )
        .child(
            Progress::new(Some(percent as f32 / 100.0))
                .build(p.k)
                .a11y_hidden(),
        )
}

/// Each mode's median and slowest Output time, most run first.
fn output_time(p: Paint, stats: &HistoryStats) -> Option<El> {
    if stats.output_time.is_empty() {
        return None;
    }
    let rows =
        stats
            .output_time
            .iter()
            .fold(col().key("profiles").w_full().gap(6.0), |rows, profile| {
                rows.child(
                    row()
                        .key(format!("profile-{}", profile.profile))
                        .w_full()
                        .gap(12.0)
                        .center_y()
                        .child(
                            text(profile.profile.clone())
                                .key("name")
                                .font(p.k.ty.small)
                                .color(p.text())
                                .grow(1.0)
                                .truncate(),
                        )
                        .child(
                            text(timing(profile))
                                .key("time")
                                .font(p.k.ty.small)
                                .color(p.muted()),
                        ),
                )
            });
    Some(
        col()
            .key("history-stats-output-time")
            .gap(8.0)
            .child(section_label(p, "label", tr("Output time")))
            .child(rows),
    )
}

/// `median 0.9 s · slowest 3.2 s`.
fn timing(profile: &ProfileOutputTime) -> String {
    let median = seconds(profile.median_ms);
    let slowest = seconds(profile.slowest_ms);
    tr_named(
        "median {median} s · slowest {slowest} s",
        &[("median", &median), ("slowest", &slowest)],
    )
}

/// `0.9`, or `0,9` in the languages that write a decimal comma.
fn seconds(milliseconds: u64) -> String {
    let seconds = format!("{:.1}", milliseconds as f64 / 1000.0);
    if crate::desktop::i18n::numbers::decimal_comma() {
        seconds.replace('.', ",")
    } else {
        seconds
    }
}

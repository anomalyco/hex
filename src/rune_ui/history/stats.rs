//! Labs › Dictation stats: the dialog behind History's Stats button. It
//! headlines the retained history's totals, then shows the recent daily
//! activity, the apps dictations went to, how often Output changed the text,
//! and each mode's Output time. Sections with nothing to show are left out,
//! and the body scrolls while the window is short.

use rune::{Align, Cx, El, col, el, row, text};
use rune_kit::Progress;

use super::super::Ui;
use super::super::controls::{
    ButtonKind, Paint, STATS_NUMBER, button, modal_scrim_transition, modal_transition,
};
use super::super::lists::{inline_slider, section_label};
use super::super::meetings::length;
use crate::desktop::i18n::numbers;
use crate::desktop::i18n::{clock_time, tr, tr_fill, tr_named};
use crate::history::stats::{DayCount, HistoryStats, ProfileOutputTime};
use crate::history::usage::Usage;

/// The stats dialog's width and padding.
const STATS_WIDTH: f32 = 460.0;
const PADDING: f32 = 18.0;
/// The least clear space the dialog leaves above and below itself.
const DIALOG_MARGIN: f32 = 24.0;
/// A window's first frame has no measured title or footer yet; these stand
/// in until they are.
const TITLE_HEIGHT: f32 = 20.0;
const FOOTER_HEIGHT: f32 = 30.0;
/// A scrolling body never shrinks below this, however short the window.
const MIN_BODY_HEIGHT: f32 = 120.0;
/// The daily chart's height and the gap between its day bars.
const CHART_HEIGHT: f32 = 44.0;
const BAR_GAP: f32 = 3.0;
/// A top app's name, and its count on the trailing edge.
const APP_NAME_WIDTH: f32 = 120.0;
const COUNT_WIDTH: f32 = 34.0;
/// A headline tile: three per row inside the dialog's width.
const TILE_WIDTH: f32 = 136.0;
/// Room beside the body's scroll bar.
const SCROLL_BAR_INSET: f32 = 12.0;

/// The stats dialog's scrim and panel, while its button and stats show. Its
/// body scrolls while the window is too short for the sections, and takes
/// the sections' own height when they fit, so the title and Close stay put
/// without a scroll bar or a gap.
pub(super) fn modal(ui: &Ui, p: Paint, cx: &mut Cx<Ui>) -> Option<[El; 2]> {
    if !ui.history.stats_open || !ui.settings.labs.history_stats {
        return None;
    }
    let stats = ui.history.stats.as_ref()?;
    if !ui.history.has_stats() {
        return None;
    }
    let c = &p.k.c;
    let typing_wpm = ui
        .history
        .typing_speed_drag
        .unwrap_or_else(|| crate::app_settings::clamp_typing_wpm(ui.settings.typing_wpm));
    let usage = ui
        .history
        .usage
        .as_ref()
        .filter(|usage| usage.dictations() > 0);
    // The headline tile and the hourly strip read the same hours: the
    // rollup while Labs › Dictation stats has one, else retained entries.
    let (hours, busiest_hour) = match usage {
        Some(usage) => (usage.hours(), usage.busiest_hour()),
        None => (stats.hours, stats.busiest_hour),
    };
    // The window is the layer the panel centers in; the title and footer are
    // measured so their real heights set the body's room.
    let window_height = cx
        .layout_size("history-stats-layer")
        .map_or(super::super::MIN_WINDOW_SIZE.1, |size| size[1]);
    let title_height = cx
        .layout_size("history-stats-title")
        .map_or(TITLE_HEIGHT, |size| size[1]);
    let footer_height = cx
        .layout_size("history-stats-footer")
        .map_or(FOOTER_HEIGHT, |size| size[1]);
    let content_height = cx.layout_size("history-stats-sections").map(|size| size[1]);
    let chrome = PADDING + title_height + 16.0 + 16.0 + footer_height + 14.0;
    let room = (window_height - 2.0 * DIALOG_MARGIN - chrome).max(MIN_BODY_HEIGHT);
    let (fade_top, fade_bottom) = super::super::SCROLL_FADE;
    // The measured sections set the mode; the first frame, with no measure
    // yet, draws them whole and settles into scrolling if they don't fit.
    let scrolling = content_height.is_some_and(|height| height + fade_top + fade_bottom > room);
    let mut sections = col().key("history-stats-sections").w_full().gap(16.0);
    if scrolling {
        // Keep the bar off the counts and timings.
        sections = sections.pr(SCROLL_BAR_INSET);
    }
    let mut sections = sections.child(tiles(p, stats, usage, typing_wpm, busiest_hour));
    sections = sections.child(typing_speed(p, typing_wpm));
    if let Some(daily) = daily(p, stats) {
        sections = sections.child(daily);
    }
    if let Some(hours) = super::heatmap::hours(p, &hours, busiest_hour) {
        sections = sections.child(hours);
    }
    if let Some(yearly) = usage.and_then(|usage| super::heatmap::yearly(p, usage)) {
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
            .h(room)
            .child(super::super::scroll_body(
                p.k,
                "history-stats-body",
                sections,
            ))
    } else {
        // No viewport at all: no scroll bar, no fade padding, and the
        // counts end on the Close button's edge.
        col().key("history-stats-body").child(sections)
    };
    let mut panel = col()
        .key("history-stats-dialog")
        .role("dialog")
        .label(tr("Dictation stats"))
        .w(STATS_WIDTH)
        .pt(PADDING)
        .px(PADDING)
        .pb(14.0)
        .gap(16.0)
        .radius(p.k.radius.xl)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak)
        // Presses inside the panel stay inside.
        .on_press(|_: &mut Ui| {})
        .child(
            text(tr("Dictation stats"))
                .key("history-stats-title")
                .font(p.k.ty.body_medium)
                .color(p.text()),
        )
        .child(body)
        .child(
            row().key("history-stats-footer").justify_end().child(
                button(
                    p,
                    "history-stats-close",
                    tr("Close"),
                    ButtonKind::Secondary,
                    |ui: &mut Ui, _| close_stats(ui),
                )
                .default_button(),
            ),
        );
    for shadow in &p.k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    let layer = |key: &str| {
        el().key(key)
            .role("none")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
    };
    Some([
        layer("history-stats-scrim")
            .fill(c.scrim)
            .on_press(close_stats)
            .transition(modal_scrim_transition(p)),
        layer("history-stats-layer")
            .center()
            .transition(modal_transition(p))
            // Keys stay inside, Escape or a press outside closes, and focus
            // returns to the Stats button that opened it.
            .child(panel.modal(true, close_stats)),
    ])
}

/// Closes the stats dialog, from its button, Escape, or the scrim.
pub(crate) fn close_stats(ui: &mut Ui) {
    ui.history.stats_open = false;
}

/// The six headline numbers, each a large value over a muted label. With
/// Labs › Dictation stats on, dictations, words, time saved, words per
/// minute, the streak, and the busiest hour cover the rollup's whole span
/// instead of only what History retains.
fn tiles(
    p: Paint,
    stats: &HistoryStats,
    usage: Option<&Usage>,
    typing_wpm: u32,
    busiest_hour: Option<u8>,
) -> El {
    let all = usage.filter(|usage| usage.dictations() > 0);
    let dictations = all.map_or(stats.dictations, Usage::dictations);
    let words = all.map_or(stats.words, Usage::words);
    let audio_ms = all.map_or(stats.audio_ms, Usage::audio_ms);
    let time_saved = crate::history::stats::time_saved_ms(words, audio_ms, typing_wpm);
    let words_per_minute = all.map_or(stats.words_per_minute, Usage::words_per_minute);
    let streak = all.map_or(stats.streak_days, |usage| {
        usage.streak_days(crate::history::stats::local_day(crate::history::now_ms()))
    });
    let wpm =
        words_per_minute.map_or_else(|| "–".to_owned(), |wpm| numbers::grouped(u64::from(wpm)));
    let busiest =
        busiest_hour.map_or_else(|| "–".to_owned(), |hour| clock_time(u32::from(hour), 0));
    let values = [
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
    ];
    values.into_iter().fold(
        row().key("tiles").w_full().gap(8.0).flex_wrap(),
        |tiles, value| tiles.child(tile(p, value.0, &value.1, value.2)),
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
        .w(TILE_WIDTH)
        .min_w(TILE_WIDTH)
        .gap(2.0)
        .px(10.0)
        .py(8.0)
        .radius(p.k.radius.md)
        .fill(p.k.c.layer_01)
        .child(
            text(value.to_owned())
                .key("value")
                .font(STATS_NUMBER)
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
                            .gap(5.0)
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
                    .gap(10.0)
                    .center_y()
                    .child(
                        text(app.application.clone())
                            .key("name")
                            .font(p.k.ty.small)
                            .color(p.text())
                            .w(APP_NAME_WIDTH)
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

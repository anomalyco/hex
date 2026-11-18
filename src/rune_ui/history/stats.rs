//! Labs › Dictation stats: the dialog behind History's Stats button. It
//! headlines the retained history's totals, then shows the recent daily
//! activity, the apps dictations went to, how often Output changed the text,
//! and each mode's Output time. Sections with nothing to show are left out,
//! and the body scrolls while the window is short.

use rune::{Align, El, col, el, row, text};
use rune_kit::Progress;

use super::super::Ui;
use super::super::controls::{
    ButtonKind, Paint, STATS_NUMBER, button, modal_scrim_transition, modal_transition,
};
use super::super::lists::section_label;
use crate::desktop::i18n::{tr, tr_named};
use crate::history::stats::{DayCount, HistoryStats, ProfileOutputTime};

/// The stats dialog's width and padding.
const STATS_WIDTH: f32 = 460.0;
const PADDING: f32 = 18.0;
/// The least clear space the dialog leaves above and below itself.
const DIALOG_MARGIN: f32 = 24.0;
/// Everything in the panel but its body: both paddings, the title's line, the
/// gaps on either side of the body, and the footer's button.
const PANEL_CHROME_HEIGHT: f32 = PADDING + 20.0 + 16.0 + 16.0 + 30.0 + 14.0;
/// A scrolling body never shrinks below this, however short the window.
const MIN_BODY_HEIGHT: f32 = 120.0;
/// The daily chart's height and the gap between its day bars.
const CHART_HEIGHT: f32 = 44.0;
const BAR_GAP: f32 = 3.0;
/// A top app's name, and its count on the trailing edge.
const APP_NAME_WIDTH: f32 = 120.0;
const COUNT_WIDTH: f32 = 34.0;
/// Room beside the body's scroll bar.
const SCROLL_BAR_INSET: f32 = 12.0;

/// The stats dialog's scrim and panel, while its button and stats show.
/// `window_height` is the window the panel centers in, which bounds its body
/// so the title and Close stay put, and `content_height` is the body's own
/// height from the last layout, so a short body doesn't leave blank space.
pub(super) fn modal(
    ui: &Ui,
    p: Paint,
    window_height: f32,
    content_height: Option<f32>,
) -> Option<[El; 2]> {
    if !ui.history.stats_open || !ui.settings.labs.history_stats {
        return None;
    }
    let stats = ui
        .history
        .stats
        .as_ref()
        .filter(|stats| stats.dictations > 0)?;
    let c = &p.k.c;
    let mut body = col()
        .key("history-stats-sections")
        .w_full()
        .gap(16.0)
        .pr(SCROLL_BAR_INSET)
        .child(tiles(p, stats));
    if let Some(daily) = daily(p, stats) {
        body = body.child(daily);
    }
    if let Some(apps) = top_apps(p, stats) {
        body = body.child(apps);
    }
    body = body.child(changed(p, stats));
    if let Some(times) = output_time(p, stats) {
        body = body.child(times);
    }
    let body = col()
        .key("history-stats-body-frame")
        .h(body_height(
            window_height,
            content_height,
            super::super::SCROLL_FADE,
        ))
        .child(super::super::scroll_body(p.k, "history-stats-body", body));
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
                .key("title")
                .font(p.k.ty.body_medium)
                .color(p.text()),
        )
        .child(body)
        .child(
            row().key("footer").justify_end().child(
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

/// How tall the scrolling body is in a window `window_height` tall: its
/// measured content (plus the scroll body's edge fades), never past the
/// window's own room, and never less than [`MIN_BODY_HEIGHT`].
fn body_height(window_height: f32, content_height: Option<f32>, fades: (f32, f32)) -> f32 {
    let room = (window_height - 2.0 * DIALOG_MARGIN - PANEL_CHROME_HEIGHT).max(MIN_BODY_HEIGHT);
    content_height.map_or(room, |height| {
        (height + fades.0 + fades.1).min(room).max(MIN_BODY_HEIGHT)
    })
}

/// Closes the stats dialog, from its button, Escape, or the scrim.
pub(crate) fn close_stats(ui: &mut Ui) {
    ui.history.stats_open = false;
}

/// The four headline numbers, each a large value over a muted label.
fn tiles(p: Paint, stats: &HistoryStats) -> El {
    let wpm = stats
        .words_per_minute
        .map_or_else(|| "–".to_owned(), |wpm| grouped(u64::from(wpm)));
    let values = [
        ("dictations", grouped(stats.dictations), tr("Dictations")),
        ("words", grouped(stats.words), tr("Words")),
        ("wpm", wpm, tr("Words per minute")),
        (
            "streak",
            grouped(u64::from(stats.streak_days)),
            tr("Daily streak"),
        ),
    ];
    values
        .into_iter()
        .fold(row().key("tiles").w_full().gap(8.0), |tiles, value| {
            tiles.child(tile(p, value.0, &value.1, value.2))
        })
}

/// A count with thousands grouped by commas: `128402` is `128,402`.
fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.char_indices() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// One tile: the value over its label, on a raised surface.
fn tile(p: Paint, key: &str, value: &str, label: &str) -> El {
    col()
        .key(format!("tile-{key}"))
        .grow(1.0)
        .shrink()
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
    let today_count = grouped(today_count);
    let peak_count = grouped(peak);
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
                        text(grouped(app.dictations))
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
    if crate::desktop::i18n::decimal_comma() {
        seconds.replace('.', ",")
    } else {
        seconds
    }
}

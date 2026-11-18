//! Labs › Dictation stats: the dialog behind History's Stats button. It
//! headlines the retained history's totals, then shows the recent daily
//! activity, the apps dictations went to, how often Output changed the text,
//! and each rewrite profile's latency. Sections with nothing to show are
//! left out.

use rune::{Align, El, col, el, row, text};
use rune_kit::Progress;

use super::super::Ui;
use super::super::controls::{
    ButtonKind, Paint, STATS_NUMBER, button, modal_scrim_transition, modal_transition,
};
use super::super::lists::section_label;
use crate::desktop::i18n::{tr, tr_named};
use crate::history::stats::{DayCount, HistoryStats, ProfileLatency};

/// The stats dialog's width and padding.
const STATS_WIDTH: f32 = 460.0;
const PADDING: f32 = 18.0;
/// The daily chart's height and the gap between its day bars.
const CHART_HEIGHT: f32 = 44.0;
const BAR_GAP: f32 = 3.0;
/// A top app's name, and its count on the trailing edge.
const APP_NAME_WIDTH: f32 = 120.0;
const COUNT_WIDTH: f32 = 34.0;

/// The stats dialog's scrim and panel, while its button and stats show.
pub(super) fn modal(ui: &Ui, p: Paint) -> Option<[El; 2]> {
    if !ui.history.stats_open || !ui.settings.labs.history_stats {
        return None;
    }
    let stats = ui
        .history
        .stats
        .as_ref()
        .filter(|stats| stats.dictations > 0)?;
    let c = &p.k.c;
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
        .child(tiles(p, stats));
    if let Some(daily) = daily(p, stats) {
        panel = panel.child(daily);
    }
    if let Some(apps) = top_apps(p, stats) {
        panel = panel.child(apps);
    }
    panel = panel.child(changed(p, stats));
    if let Some(latency) = rewrite_latency(p, stats) {
        panel = panel.child(latency);
    }
    let mut panel = panel.child(
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

/// Closes the stats dialog, from its button, Escape, or the scrim.
pub(crate) fn close_stats(ui: &mut Ui) {
    ui.history.stats_open = false;
}

/// The four headline numbers, each a large value over a muted label.
fn tiles(p: Paint, stats: &HistoryStats) -> El {
    let wpm = stats
        .words_per_minute
        .map_or_else(|| "–".to_owned(), |wpm| wpm.to_string());
    let values = [
        ("dictations", stats.dictations.to_string(), tr("Dictations")),
        ("words", stats.words.to_string(), tr("Words")),
        ("wpm", wpm, tr("Words per minute")),
        ("streak", stats.streak_days.to_string(), tr("Daily streak")),
    ];
    values
        .into_iter()
        .fold(row().key("tiles").w_full().gap(8.0), |tiles, value| {
            tiles.child(tile(p, value.0, &value.1, value.2))
        })
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
                .color(p.text()),
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
    let mut bars = row()
        .key("bars")
        .role("img")
        .label(tr("Dictations per day"))
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
                        text(app.dictations.to_string())
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

/// Each rewrite profile's median and slowest latency, most run first.
fn rewrite_latency(p: Paint, stats: &HistoryStats) -> Option<El> {
    if stats.rewrite_latency.is_empty() {
        return None;
    }
    let rows = stats.rewrite_latency.iter().fold(
        col().key("profiles").w_full().gap(6.0),
        |rows, profile| {
            rows.child(
                row()
                    .key(format!("profile-{}", profile.profile))
                    .w_full()
                    .gap(12.0)
                    .center_y()
                    .child(
                        text(profile.profile.clone())
                            .key("name")
                            .font(p.k.ty.ui)
                            .color(p.text())
                            .grow(1.0)
                            .truncate(),
                    )
                    .child(
                        text(latency(profile))
                            .key("latency")
                            .font(p.k.ty.small)
                            .color(p.muted()),
                    ),
            )
        },
    );
    Some(
        col()
            .key("history-stats-latency")
            .gap(8.0)
            .child(section_label(p, "label", tr("Rewrite latency")))
            .child(rows),
    )
}

/// `median 0.9 s · slowest 3.2 s`.
fn latency(profile: &ProfileLatency) -> String {
    let median = seconds(profile.median_ms);
    let slowest = seconds(profile.slowest_ms);
    tr_named(
        "median {median} s · slowest {slowest} s",
        &[("median", &median), ("slowest", &slowest)],
    )
}

fn seconds(milliseconds: u64) -> String {
    format!("{:.1}", milliseconds as f64 / 1000.0)
}

//! Shipped highlights. The feed follows the Labs registry: the newest eight
//! labs this build offers, each with its own date, copy, and card icon, then
//! the hand-written Sounds note. Highlighting a lab is independent of whether
//! the person has switched it on.
use super::*;
use crate::calendar::date::{LocalDate, local_date};
use crate::desktop::i18n::tr_named;
use crate::labs::Feature;
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
enum Highlight {
    Lab(Feature),
    Sounds,
}

impl Highlight {
    fn open(self, ui: &mut Ui, tx: &mut Tx) {
        match self {
            Self::Lab(feature) => labs::reveal(
                ui,
                &labs::LabsLink {
                    lab: Some(feature.name().into()),
                    ..Default::default()
                },
                tx,
            ),
            Self::Sounds => {
                ui.select_pane(Pane::General, tx);
                sound_sets::open(ui);
            }
        }
    }

    fn row(self, p: Paint, symbol: &'static rune::Icon) -> (u32, El) {
        let (day, id, title, summary, button, label) = match self {
            Self::Lab(f) => (
                f.added(),
                f.name(),
                tr(f.title()),
                tr(f.summary()),
                format!("open-lab-{}", f.name()),
                tr("Labs"),
            ),
            Self::Sounds => (
                20261029,
                "sounds",
                tr("Sounds"),
                tr("Preview the tones Hex plays when you dictate."),
                "open-sounds".to_owned(),
                tr("Sounds"),
            ),
        };
        let link = controls::button(
            p,
            &button,
            label,
            controls::ButtonKind::Secondary,
            move |ui, tx| self.open(ui, tx),
        );
        (
            day,
            rune::row()
                .key(id)
                .center_y()
                .pl(controls::METRICS.row_px)
                .gap(12.0)
                .child(icons::tile(p, symbol, 36.0))
                .child(
                    controls::setting_row(
                        p,
                        &format!("highlight-{id}"),
                        title,
                        Some(summary),
                        link,
                    )
                    .pl(0.0)
                    .grow(1.0)
                    .shrink(),
                ),
        )
    }
}

/// What's new lists no more than this many labs.
const NEWEST_LABS: usize = 8;
/// The day a fixture's feed dates from when this build offers no lab at all.
const FIXTURE_DAY: u32 = 20261115;

pub(super) fn feed(ui: &Ui, p: Paint) -> El {
    // Newest first, then the registry's own order, as Labs lists its newest.
    let mut labs = labs::offered_labs(ui);
    labs.sort_by_key(|(feature, _)| {
        std::cmp::Reverse((
            feature.added(),
            Feature::ALL
                .iter()
                .position(|known| known == feature)
                .unwrap_or(0),
        ))
    });
    labs.truncate(NEWEST_LABS);
    let today = if ui.is_fixture() {
        Some(LocalDate::from_ymd(
            labs.first()
                .map_or(FIXTURE_DAY, |(feature, _)| feature.added()),
        ))
    } else {
        local_date(crate::history::now_ms() as i64)
    };
    let highlights = labs
        .into_iter()
        .map(|(feature, icon)| (Highlight::Lab(feature), icon))
        .chain([(Highlight::Sounds, &icons::VOLUME)])
        .map(|(entry, symbol)| entry.row(p, symbol));
    let feed = controls::page("whats-new-feed").children(sections(p, today, highlights));
    super::scroll_body(p.k, "whats-new-scroll", feed)
}

fn sections(
    p: Paint,
    today: Option<LocalDate>,
    rows: impl IntoIterator<Item = (u32, El)>,
) -> impl Iterator<Item = El> {
    let mut days = BTreeMap::<_, Vec<_>>::new();
    for (day, row) in rows {
        days.entry(day).or_default().push(row);
    }
    days.into_iter().rev().map(move |(day, rows)| {
        controls::section(
            p,
            &format!("day-{day}"),
            &heading(LocalDate::from_ymd(day), today),
            rows,
        )
    })
}

fn heading(date: LocalDate, today: Option<LocalDate>) -> String {
    match today.map(|today| today.days() - date.days()) {
        Some(0) => return tr("Today").into(),
        Some(1) => return tr("Yesterday").into(),
        Some(days @ 2..=6) => return tr_fill("{} days ago", &days.to_string()),
        _ => (),
    }
    let months = tr("Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec");
    let month = months
        .split_whitespace()
        .nth((date.month - 1) as usize)
        .unwrap_or("");
    let fields = [
        ("month", month),
        ("day", &date.day.to_string()),
        ("year", &date.year.to_string()),
    ];
    if today.is_some_and(|today| today.year == date.year) {
        tr_named("{month} {day}", &fields)
    } else {
        tr_named("{month} {day}, {year}", &fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_count_local_days_not_hours() {
        // Just after a 23-hour day and late in a 25-hour day in US Pacific
        // time. Interpret the wall times in the test process's own zone.
        for (now, entry, label) in [
            ("2026-03-09T00:01", 20260308, "Yesterday"),
            ("2026-11-01T23:59", 20261031, "Yesterday"),
            ("2027-01-01", 20270101, "Today"),
            ("2027-01-01", 20261231, "Yesterday"),
            ("2024-03-01", 20240228, "2 days ago"),
            ("2026-11-04", 20261029, "6 days ago"),
            ("2026-11-05", 20261029, "Oct 29"),
            ("2027-01-01", 20261029, "Oct 29, 2026"),
        ] {
            let instant = crate::meeting::query::parse_time(now, 0, false).unwrap();
            assert_eq!(
                heading(LocalDate::from_ymd(entry), local_date(instant as i64)),
                label
            );
        }
    }

    #[test]
    fn entries_on_one_day_share_a_section() {
        let rows = [
            (20261101, rune::el()),
            (20261029, rune::el()),
            (20261101, rune::el()),
        ];
        assert_eq!(sections(Paint::new(true, 1.0), None, rows).count(), 2);
    }
}

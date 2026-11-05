//! Labs › Collapsible meeting sections (#1348): the Meetings list's day
//! headings and the detail's Summary and Chapters labels fold what follows
//! them. Folds last only while Hex runs; everything starts open. While the
//! search field has text, every match shows whatever its day's fold. With
//! the lab off, the labels are the plain ones and nothing folds.

use std::collections::HashSet;

use rune::El;

use super::super::Ui;
use super::super::controls::Paint;
use super::super::list_detail as ld;
use super::super::lists::section_label;
use crate::desktop::i18n::tr;
use crate::labs::Feature;

/// What is folded, by day label and section.
#[derive(Debug, Default)]
pub(in crate::rune_ui) struct Folds {
    days: HashSet<String>,
    summary: bool,
    chapters: bool,
}

/// Whether the lab is on in this build.
fn on(ui: &Ui) -> bool {
    ui.settings.labs.collapsible_meeting_sections && Feature::CollapsibleMeetingSections.shipped()
}

/// Whether `day`'s rows are hidden: folded, the lab on, and no search.
pub(super) fn day_folded(ui: &Ui, day: &str) -> bool {
    on(ui) && ui.meetings.search.text.trim().is_empty() && ui.meetings.folds.days.contains(day)
}

/// A day's heading: one that folds while the lab is on, else the plain one.
pub(super) fn day_heading(ui: &Ui, p: Paint, key: &str, label: &str, day: &str, first: bool) -> El {
    if !on(ui) {
        return ld::group(p, key, label, first);
    }
    let day = day.to_string();
    let expanded = !day_folded(ui, &day);
    ld::foldable_group(p, key, label, first, expanded, move |ui: &mut Ui| {
        let days = &mut ui.meetings.folds.days;
        if !days.remove(&day) {
            days.insert(day.clone());
        }
    })
}

/// Whether the summary's text shows under its label.
pub(super) fn summary_shown(ui: &Ui) -> bool {
    !(on(ui) && ui.meetings.folds.summary)
}

/// Whether the chapters show under their label.
pub(super) fn chapters_shown(ui: &Ui) -> bool {
    !(on(ui) && ui.meetings.folds.chapters)
}

/// The Summary label, which folds the summary while the lab is on.
pub(super) fn summary_label(ui: &Ui, p: Paint) -> El {
    if !on(ui) {
        return section_label(p, "summary-label", tr("Summary"));
    }
    hugging(
        "summary-fold",
        ld::fold_label(
            p,
            "summary-label",
            tr("Summary"),
            p.muted(),
            summary_shown(ui),
            |ui: &mut Ui| ui.meetings.folds.summary = !ui.meetings.folds.summary,
        ),
    )
}

/// `label` as wide as its words, rather than its column, with its hover
/// fill hanging just past them.
fn hugging(key: &str, label: El) -> El {
    rune::row()
        .key(key)
        .role("none")
        .ml(-4.0)
        .child(label.px(4.0).py(1.0))
}

/// The Chapters label, which folds the chapters while the lab is on.
pub(super) fn chapters_label(ui: &Ui, p: Paint) -> El {
    if !on(ui) {
        return section_label(p, "chapters-label", tr("Chapters"));
    }
    hugging(
        "chapters-fold",
        ld::fold_label(
            p,
            "chapters-label",
            tr("Chapters"),
            p.muted(),
            chapters_shown(ui),
            |ui: &mut Ui| ui.meetings.folds.chapters = !ui.meetings.folds.chapters,
        ),
    )
}

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::super::MeetingsPane;
    use super::*;
    use crate::rune_ui::Service;

    fn ui(lab: bool) -> Ui {
        let mut settings = crate::app_settings::AppSettings::default();
        settings.labs.collapsible_meeting_sections = lab;
        let mut ui = Ui::with(settings, Service::Fixture(Vec::new()));
        ui.meetings = MeetingsPane::chapters_fixture();
        ui
    }

    /// Off, a folded day or section still shows: nothing changes (#1348).
    #[test]
    fn nothing_folds_with_the_lab_off() {
        let mut ui = ui(false);
        ui.meetings.folds.days.insert("Today".into());
        ui.meetings.folds.summary = true;
        ui.meetings.folds.chapters = true;
        assert!(!day_folded(&ui, "Today"));
        assert!(summary_shown(&ui));
        assert!(chapters_shown(&ui));
    }

    /// On, a folded day hides its rows until searched, and the summary and
    /// chapters fold on their own.
    #[test]
    fn days_and_sections_fold_with_the_lab_on_and_search_ignores_folds() {
        let mut ui = ui(true);
        assert!(!day_folded(&ui, "Today"));
        assert!(summary_shown(&ui) && chapters_shown(&ui));
        ui.meetings.folds.days.insert("Today".into());
        assert!(day_folded(&ui, "Today"));
        assert!(!day_folded(&ui, "Yesterday"));
        ui.meetings.search = rune::TextEdit::new("arcade".to_string());
        assert!(!day_folded(&ui, "Today"));
        ui.meetings.folds.summary = true;
        assert!(!summary_shown(&ui));
        assert!(chapters_shown(&ui));
        ui.meetings.folds.chapters = true;
        assert!(!chapters_shown(&ui));
    }
}

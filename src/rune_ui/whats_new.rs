//! Shipped highlights, newest first. Opening an entry only reveals its control.
use super::*;
use crate::labs::Feature;

pub(super) fn feed(ui: &Ui, p: Paint) -> El {
    let mut feed = controls::page("whats-new-feed");
    let lab = Feature::YoloMode;
    if labs::offered(ui, lab) {
        let day = lab.added();
        let date = format!("{:04}-{:02}-{:02}", day / 10000, day / 100 % 100, day % 100);
        feed = feed.child(controls::section(
            p,
            lab.name(),
            &date,
            vec![controls::setting_row(
                p,
                "highlight",
                tr(lab.title()),
                Some(tr(lab.summary())),
                controls::button(
                    p,
                    "open-lab",
                    tr("Labs"),
                    ButtonKind::Secondary,
                    move |ui, tx| {
                        labs::reveal(
                            ui,
                            &labs::LabsLink {
                                lab: Some(lab.name().into()),
                                ..Default::default()
                            },
                            tx,
                        );
                    },
                ),
            )],
        ));
    }
    feed.child(controls::section(
        p,
        "sounds",
        "2026-10-29",
        vec![controls::setting_row(
            p,
            "highlight",
            tr("Sounds"),
            Some(tr("Preview the tones Hex plays when you dictate.")),
            controls::button(
                p,
                "open-sounds",
                tr("Sounds"),
                ButtonKind::Secondary,
                |ui, tx| {
                    ui.select_pane(Pane::General, tx);
                    sound_sets::open(ui);
                },
            ),
        )],
    ))
    .children(sidebar::release_notes_link(ui, p))
}

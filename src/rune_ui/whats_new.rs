//! Shipped highlights. A lab supplies its own date and copy; highlighting it
//! here is independent of whether the person has switched it on.
use super::*;
use crate::labs::Feature;

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

    fn row(self, ui: &Ui, p: Paint) -> Option<(u32, El)> {
        let (day, id, title, summary, button, label) = match self {
            Self::Lab(f) if labs::offered(ui, f) => (
                f.added(),
                f.name(),
                tr(f.title()),
                tr(f.summary()),
                "open-lab",
                tr("Labs"),
            ),
            Self::Lab(_) => return None,
            Self::Sounds => (
                20261029,
                "sounds",
                tr("Sounds"),
                tr("Preview the tones Hex plays when you dictate."),
                "open-sounds",
                tr("Sounds"),
            ),
        };
        let date = format!("{:04}-{:02}-{:02}", day / 10000, day / 100 % 100, day % 100);
        let link = controls::button(
            p,
            button,
            label,
            controls::ButtonKind::Secondary,
            move |ui, tx| self.open(ui, tx),
        );
        Some((
            day,
            controls::section(
                p,
                id,
                &date,
                vec![controls::setting_row(
                    p,
                    "highlight",
                    title,
                    Some(summary),
                    link,
                )],
            ),
        ))
    }
}

pub(super) fn feed(ui: &Ui, p: Paint) -> El {
    let mut highlights: Vec<_> = [Highlight::Lab(Feature::YoloMode), Highlight::Sounds]
        .into_iter()
        .filter_map(|entry| entry.row(ui, p))
        .collect();
    highlights.sort_by_key(|(day, _)| std::cmp::Reverse(*day));
    controls::page("whats-new-feed").children(highlights.into_iter().map(|(_, row)| row))
}

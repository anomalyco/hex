//! The picker's first view: the saved language's first three models side
//! by side, measured row by row, with the best value in each row read
//! first, and a way to every other model.

use rune::{El, col, el, row, text};

use super::facts::{
    self as facts, Entry, State, compact_progress, errors, errors_heading, errors_label, tnum,
};
use super::{self as picker, View};
use crate::desktop::i18n::{tr, tr_fill};
use crate::rune_ui::Ui;
use crate::rune_ui::controls::Paint;
use crate::transcription::models::Recommendation;

pub(super) const WIDTH: f32 = 800.0;
const LABEL: f32 = 156.0;
const PAD: f32 = 24.0;
const HEAD: f32 = 124.0;
const ROW: f32 = 40.0;
const ACTION: f32 = 64.0;
const COLUMNS: usize = 3;

/// The columns: the first three models that speak the language, with the
/// one being prepared, else the one in use, in the last place when the
/// three leave it out.
fn columns(entries: &[Entry]) -> Vec<&Entry> {
    let mut shown: Vec<&Entry> = entries
        .iter()
        .filter(|entry| entry.supports)
        .take(COLUMNS)
        .collect();
    let left_out = |entry: &&Entry| shown.iter().all(|column| column.def.id != entry.def.id);
    let extra = entries
        .iter()
        .find(|entry| entry.preparing() && left_out(entry))
        .or_else(|| {
            entries
                .iter()
                .find(|entry| entry.in_use() && left_out(entry))
        });
    if let Some(extra) = extra {
        if shown.len() == COLUMNS {
            shown.pop();
        }
        shown.push(extra);
    }
    shown
}

struct Measure {
    key: &'static str,
    label: &'static str,
    values: Vec<String>,
    /// Higher is better; `None` has nothing to compare.
    scores: Vec<Option<f64>>,
}

impl Measure {
    /// The column whose value reads first: the one best score.
    fn best(&self) -> Option<usize> {
        let top = self
            .scores
            .iter()
            .flatten()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let mut best = self
            .scores
            .iter()
            .enumerate()
            .filter(|(_, score)| **score == Some(top));
        match (best.next(), best.next()) {
            (Some((index, _)), None) => Some(index),
            _ => None,
        }
    }
}

fn measures(ui: &Ui, shown: &[&Entry], language: &str) -> Vec<Measure> {
    let mut measures = vec![Measure {
        key: "wait",
        label: tr("Wait when you stop"),
        values: shown
            .iter()
            .map(|entry| facts::wait_label(entry.def))
            .collect(),
        scores: shown
            .iter()
            .map(|entry| facts::wait_score(entry.def))
            .collect(),
    }];
    // Wrong words only where at least one column was measured in the
    // language, in the first measured column's terms.
    let measured: Vec<_> = shown
        .iter()
        .map(|entry| errors(entry.def, language))
        .collect();
    if let Some((metric, _)) = measured.iter().flatten().next().copied() {
        let same = |found: &Option<(_, f64)>| found.filter(|(other, _)| *other == metric);
        measures.push(Measure {
            key: "wrong",
            label: errors_heading(metric),
            values: measured
                .iter()
                .map(|found| {
                    same(found)
                        .map_or_else(|| tr("Not measured").into(), |(_, per)| errors_label(per))
                })
                .collect(),
            scores: measured
                .iter()
                .map(|found| same(found).map(|(_, per)| per))
                .collect(),
        });
    }
    measures.push(Measure {
        key: "languages",
        label: tr("Languages"),
        values: shown
            .iter()
            .map(|entry| facts::languages_label(entry.def))
            .collect(),
        scores: shown
            .iter()
            .map(|entry| Some(facts::language_count(entry.def) as f64))
            .collect(),
    });
    measures.push(Measure {
        key: "vocabulary",
        label: tr("Reads your vocabulary"),
        values: shown
            .iter()
            .map(|entry| {
                if facts::reads_vocabulary(ui, entry.def) {
                    tr("Yes")
                } else {
                    "—"
                }
                .into()
            })
            .collect(),
        scores: shown
            .iter()
            .map(|entry| facts::reads_vocabulary(ui, entry.def).then_some(1.0))
            .collect(),
    });
    measures.push(Measure {
        key: "download",
        label: tr("Download"),
        values: shown
            .iter()
            .map(|entry| facts::download_label(entry))
            .collect(),
        scores: shown
            .iter()
            .map(|entry| Some(-(facts::to_download(entry) as f64)))
            .collect(),
    });
    measures
}

fn rule(p: Paint) -> El {
    el().key("rule").h(p.hair).w_full().fill(p.k.c.border_weak)
}

/// One row's cell: a rule over the content.
fn cell(p: Paint, key: &str, height: f32, content: El) -> El {
    col()
        .key(key.to_owned())
        .h(height)
        .child(rule(p))
        .child(row().key("cell").grow(1.0).center_y().child(content))
}

pub(super) fn body(ui: &Ui, p: Paint, entries: &[Entry], language: &str) -> El {
    let c = &p.k.c;
    let shown = columns(entries);
    let measures = measures(ui, &shown, language);
    let width = (WIDTH - 2.0 * p.hair - 2.0 * PAD - LABEL) / shown.len().max(COLUMNS) as f32;

    let mut labels = col().key("labels").w(LABEL).child(el().key("head").h(HEAD));
    for measure in &measures {
        labels = labels.child(cell(
            p,
            measure.key,
            ROW,
            text(measure.label)
                .key("label")
                .font(p.k.ty.ui)
                .color(p.muted()),
        ));
    }
    labels = labels.child(cell(p, "action", ACTION, el()));

    // The one main action: the recommended model, unless it is in use.
    let primary = shown.iter().position(|entry| {
        entry.recommendation == Some(Recommendation::Recommended) && entry.state.choosable()
    });
    let mut table = row().key("compare").px(PAD).pt(8.0).child(labels);
    for (index, entry) in shown.iter().enumerate() {
        let recommended = entry.recommendation == Some(Recommendation::Recommended);
        let eyebrow = facts::role(ui, entry, &shown).unwrap_or("");
        let head = col()
            .key("head")
            .h(HEAD)
            .px(16.0)
            .pt(16.0)
            .gap(4.0)
            .child(
                text(eyebrow)
                    .key("eyebrow")
                    .font(p.k.ty.small_medium)
                    .color(if recommended { p.accent() } else { p.faint() }),
            )
            .child(
                text(entry.def.name)
                    .key("name")
                    .font(p.k.ty.body_medium)
                    .color(p.text()),
            )
            .child(
                text(facts::blurb(entry.def))
                    .key("blurb")
                    .font(p.k.ty.caption())
                    .color(p.muted())
                    .wrap(),
            );
        let mut column = col()
            .key(format!("model{index}"))
            .w(width)
            .radius(p.k.radius.lg)
            .child(head);
        if entry.in_use() {
            column = column.fill(c.layer_01);
        }
        for measure in &measures {
            let best = measure.best() == Some(index);
            let mut value = row().key("value").px(16.0).gap(6.0).center_y();
            if measure.key == "vocabulary" && measure.scores[index].is_some() {
                value = value.child(rune::icon(p.k.glyph.check_small, 16.0).color(c.icon_base));
            }
            value = value.child(
                text(measure.values[index].clone())
                    .key("text")
                    .font(tnum(if best { p.k.ty.ui_medium } else { p.k.ty.ui }))
                    .color(if best { p.text() } else { p.muted() }),
            );
            column = column.child(cell(p, measure.key, ROW, value));
        }
        let action = match &entry.state {
            State::InUse => facts::in_use(p),
            State::Preparing { .. } => compact_progress(p, entry),
            _ if entry.def.available() => facts::choose_button(p, entry, primary == Some(index)),
            _ => el(),
        };
        column = column.child(cell(
            p,
            "action",
            ACTION,
            row().key("slot").w_full().px(16.0).child(action),
        ));
        table = table.child(column);
    }

    let mut body = col().key("compare-view").child(table);
    let rest = entries.len().saturating_sub(shown.len());
    if rest > 0 {
        body = body.child(
            row().key("more").px(PAD - 12.0).pt(4.0).pb(12.0).child(
                row()
                    .key("more-models")
                    .role("button")
                    .label(more_label(rest))
                    .h(40.0)
                    .px(12.0)
                    .gap(8.0)
                    .grow(1.0)
                    .center_y()
                    .radius(p.k.radius.md)
                    .hover(c.hover)
                    .press(c.pressed)
                    .on_press(|ui: &mut Ui| picker::show(ui, View::All))
                    .child(
                        text(more_label(rest))
                            .key("label")
                            .font(p.k.ty.ui)
                            .color(p.muted())
                            .grow(1.0),
                    )
                    .child(
                        rune::icon(&rune_theme_opencode::icons::CHEVRON_RIGHT, 16.0)
                            .color(c.icon_muted),
                    ),
            ),
        );
    } else {
        body = body.child(el().key("end").h(16.0));
    }
    body
}

fn more_label(count: usize) -> String {
    if count == 1 {
        tr("1 more model").into()
    } else {
        tr_fill("{} more models", &count.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure(scores: &[Option<f64>]) -> Measure {
        Measure {
            key: "test",
            label: "",
            values: Vec::new(),
            scores: scores.to_vec(),
        }
    }

    #[test]
    fn only_a_single_best_value_reads_first() {
        assert_eq!(measure(&[Some(1.0), Some(3.0), Some(2.0)]).best(), Some(1));
        assert_eq!(measure(&[Some(3.0), Some(3.0), Some(2.0)]).best(), None);
        assert_eq!(measure(&[None, Some(-2.0), None]).best(), Some(1));
        assert_eq!(measure(&[None, None]).best(), None);
    }
}

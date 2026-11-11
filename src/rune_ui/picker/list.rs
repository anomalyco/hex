//! The picker's second view: every model as one compact row, the ones
//! that speak the saved language first, your own model files after, and
//! those that would change the language folded away at the end.

use rune::{El, col, el, row, text};
use rune_kit::{Badge, ButtonVariant, ControlSize, IconButton, IconButtonVariant};

use super::facts::{self as facts, Entry, State, tnum};
use super::{self as picker, delete};
use crate::desktop::i18n::{tr, tr_fill, tr_named};
use crate::rune_ui::Ui;
use crate::rune_ui::controls::{ButtonKind, Paint, PushSize, button};
use crate::rune_ui::lists::group_label;
use crate::transcription::models::{
    self as transcription_models, AUTO_LANGUAGE, ModelRuntime, TranscriptionModelId, language_name,
};

const PROGRESS_WIDTH: f32 = 196.0;

pub(super) fn body(ui: &Ui, p: Paint, entries: &[Entry], language: &str) -> El {
    let mut body = col().key("all-view").px(16.0).pb(16.0);
    // Files other apps downloaded that aren't catalog models; a catalog
    // one shows on its own row. Labs › Downloaded models only leaves them out.
    if !picker::downloaded_filter(ui)
        && let Some(found) = crate::rune_ui::found::section(ui, p)
    {
        body = body.child(found.pb(16.0));
    }
    let Groups { leading, others } = groups(entries, picker::list_first(ui));

    let mut first = true;
    for (key, models) in &leading {
        if models.is_empty() {
            continue;
        }
        let heading = match *key {
            IMPORTED => tr("Your model files").to_owned(),
            _ if language == AUTO_LANGUAGE => tr("Detect the language").to_owned(),
            _ => language_name(language).to_owned(),
        };
        body = body.child(group(ui, p, key, &heading, first, models, language));
        first = false;
    }
    if !others.is_empty() {
        // A model on its way stays in sight.
        let open = ui.picker.others_open || others.iter().any(|entry| entry.preparing());
        body = body.child(fold(p, others.len(), language, open, first));
        if open {
            body = body.child(group(ui, p, "others", "", true, &others, language));
        }
    }
    body
}

const SPEAKING: &str = "speaking";
const IMPORTED: &str = "imported";

/// The list's groups: the models that speak the language and your own
/// model files, in the order they show, then the rest.
struct Groups<'a> {
    leading: [(&'static str, Vec<&'a Entry>); 2],
    others: Vec<&'a Entry>,
}

/// Sorts `entries` into their groups. `first` (Labs › Model in use first)
/// leads its group, and an imported one puts your model files first.
fn groups(entries: &[Entry], first: Option<TranscriptionModelId>) -> Groups<'_> {
    let speaking = lead(
        entries
            .iter()
            .filter(|entry| entry.supports && !entry.def.id.is_custom())
            .collect(),
        first,
    );
    let imported = lead(
        entries
            .iter()
            .filter(|entry| entry.def.id.is_custom())
            .collect(),
        first,
    );
    let others = lead(
        entries
            .iter()
            .filter(|entry| !entry.supports && !entry.def.id.is_custom())
            .collect(),
        first,
    );
    let imported_first = first.is_some_and(|id| imported.iter().any(|entry| entry.def.id == id));
    let leading = if imported_first {
        [(IMPORTED, imported), (SPEAKING, speaking)]
    } else {
        [(SPEAKING, speaking), (IMPORTED, imported)]
    };
    Groups { leading, others }
}

/// `models` with `first` moved to the top, when it is among them.
fn lead(mut models: Vec<&Entry>, first: Option<TranscriptionModelId>) -> Vec<&Entry> {
    if let Some(index) = first.and_then(|id| models.iter().position(|entry| entry.def.id == id)) {
        let pinned = models.remove(index);
        models.insert(0, pinned);
    }
    models
}

fn group(
    ui: &Ui,
    p: Paint,
    key: &str,
    heading: &str,
    first: bool,
    entries: &[&Entry],
    language: &str,
) -> El {
    let mut group = col().key(key.to_owned()).role("list");
    if !heading.is_empty() {
        group = group
            .label(heading)
            .child(group_label(p, "heading", heading, first).px(0.0));
    }
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            group = group.child(
                el().key(format!("rule{index}"))
                    .h(p.hair)
                    .w_full()
                    .fill(p.k.c.border_weak)
                    .a11y_hidden(),
            );
        }
        group = group
            .child(model_row(ui, p, entry, language).position_in_set(index + 1, entries.len()));
    }
    group
}

/// The row that opens the models that don't speak the saved language.
fn fold(p: Paint, count: usize, language: &str, open: bool, first: bool) -> El {
    let c = &p.k.c;
    let label = if language == AUTO_LANGUAGE {
        tr_fill("{} more that can't detect the language", &count.to_string())
    } else {
        tr_named(
            "{count} more that don't speak {language}",
            &[
                ("count", &count.to_string()),
                ("language", language_name(language)),
            ],
        )
    };
    row()
        .key("other-languages")
        .role("button")
        .label(label.clone())
        .expanded(open)
        .mt(if first { 0.0 } else { 12.0 })
        .mx(-8.0)
        .h(36.0)
        .px(8.0)
        .gap(6.0)
        .center_y()
        .radius(p.k.radius.md)
        .hover(c.hover)
        .press(c.pressed)
        .on_press(|ui: &mut Ui| ui.picker.others_open = !ui.picker.others_open)
        .child(
            text(label)
                .key("label")
                .font(p.k.ty.small_medium)
                .color(p.faint())
                .grow(1.0),
        )
        .child(
            rune::icon(
                if open {
                    p.k.glyph.chevron_down
                } else {
                    &rune_theme_opencode::icons::CHEVRON_RIGHT
                },
                16.0,
            )
            .color(c.icon_muted),
        )
}

/// What a row's second line says: the wait, the wrong words where they
/// were measured in the language, the languages, and the size.
fn facts_line(entry: &Entry, language: &str) -> String {
    let def = entry.def;
    if def.id.is_custom() {
        return crate::rune_ui::model_import::metadata(def);
    }
    let mut parts = Vec::new();
    if let Some(wait) = facts::wait(def) {
        parts.push(if wait == 0.0 {
            tr("No wait").to_owned()
        } else if wait < 1.0 {
            tr("Under 1 s wait").to_owned()
        } else {
            tr_fill("{} s wait", &format!("{:.0}", wait.round()))
        });
    }
    if let Some((metric, per)) = facts::errors(def, language) {
        let per = format!("{per:.0}");
        parts.push(match metric {
            crate::transcription::model_benchmarks::ErrorMetric::Wer => {
                tr_fill("1 in {} words wrong", &per)
            }
            crate::transcription::model_benchmarks::ErrorMetric::Cer => {
                tr_fill("1 in {} characters wrong", &per)
            }
        });
    }
    parts.push(match facts::language_count(def) {
        1 => facts::languages_label(def),
        count => tr_fill("{} languages", &count.to_string()),
    });
    parts.push(facts::size_text(def));
    parts.join(" · ")
}

fn model_row(ui: &Ui, p: Paint, entry: &Entry, language: &str) -> El {
    let def = entry.def;
    // The row is named by the model; its other lines stay readable inside.
    let mut title = row().key("title").gap(8.0).center_y().child(
        text(def.name)
            .key("name")
            .font(p.k.ty.ui_medium)
            .color(p.text())
            .a11y_hidden(),
    );
    if let Some((role, first)) = facts::list_role(entry) {
        title = title.child(
            text(role)
                .key("role")
                .font(p.k.ty.small_medium)
                .color(if first { p.accent() } else { p.faint() }),
        );
    }
    // A catalog model behind its own Labs toggle says so.
    if def.id == TranscriptionModelId::ParakeetUltra {
        title = title.child(Badge::new(tr("Labs")).build(p.k));
    }
    let line = |key: &str, copy: String, color| {
        text(copy)
            .key(key.to_owned())
            .font(tnum(p.k.ty.small))
            .color(color)
            .wrap()
    };
    let mut copy = col().key("copy").grow(1.0).shrink().gap(2.0).child(title);
    let blurb = facts::blurb(def);
    if !blurb.is_empty() {
        copy = copy.child(line("blurb", blurb.to_owned(), p.muted()));
    }
    copy = copy.child(line("facts", facts_line(entry, language), p.faint()));
    if !entry.supports && !def.id.is_custom() {
        copy = copy.child(line(
            "language-switch",
            tr_named(
                "Choosing it switches the language to {language}",
                &[("language", language_name(def.default_language()))],
            ),
            p.warning(),
        ));
    }
    if matches!(
        def.id,
        TranscriptionModelId::ParakeetV3
            | TranscriptionModelId::Orukeet
            | TranscriptionModelId::ParakeetUltra
    ) && language != AUTO_LANGUAGE
    {
        copy = copy.child(line(
            "note",
            tr("It hears the language from the audio, not from your setting").to_owned(),
            p.faint(),
        ));
    }
    // Labs › Benchmark details: raw error rates with their provenance.
    if picker::benchmark_details_on(ui)
        && let Some(record) = transcription_models::benchmark(def)
    {
        copy = copy.child(line(
            "benchmark",
            format!(
                "{} · {}",
                picker::benchmark_rate(&record),
                picker::benchmark_detail(&record)
            ),
            p.muted(),
        ));
    }
    if let Some(note) = crate::rune_ui::model_import::note(p, def) {
        copy = copy.child(note);
    }
    if let Some(details) = crate::rune_ui::model_details::section(ui, p, def) {
        copy = copy.child(details);
    }

    let mut actions = row().key("actions").gap(8.0).center_y();
    if let Some(toggle) = crate::rune_ui::model_details::toggle(ui, p, def) {
        actions = actions.child(toggle);
    }
    let preparing = ui
        .view
        .as_ref()
        .and_then(|view| view.transcription.preparing);
    let free = preparing.is_none() && ui.settings.transcription.model != def.id;
    if def.id.is_custom() && !entry.in_use() && !entry.preparing() {
        for action in crate::rune_ui::model_import::actions(ui, p, def, free) {
            actions = actions.child(action);
        }
    } else if let Some(delete) = delete_button(ui, p, entry, free) {
        actions = actions.child(delete);
    }
    actions = actions.child(match &entry.state {
        State::InUse => facts::in_use(p),
        State::Preparing { .. } => row()
            .key("progress-slot")
            .w(PROGRESS_WIDTH)
            .child(facts::compact_progress(p, entry)),
        _ if def.available() => facts::row_choose_button(p, entry),
        _ => el(),
    });

    row()
        .key(format!("model-{}", def.id.as_str()))
        .role("listitem")
        .label(def.name)
        .py(12.0)
        .gap(16.0)
        .center_y()
        .child(copy)
        .child(actions)
}

/// Delete for a downloaded catalog model, Forget for another app's file
/// used in place; the first press arms it.
fn delete_button(ui: &Ui, p: Paint, entry: &Entry, free: bool) -> Option<El> {
    let def = entry.def;
    let deletable =
        entry.state == State::Downloaded && free && matches!(def.runtime, ModelRuntime::Gguf(_));
    if !deletable {
        return None;
    }
    let in_place = transcription_models::is_in_place(def);
    if ui.picker.deletion == Some(def.id) {
        let label = if in_place {
            tr("Forget it?").to_owned()
        } else {
            tr_fill("Delete {}?", &def.size_label())
        };
        return Some(
            button(
                p,
                "delete-model",
                &label,
                ButtonKind::Danger,
                move |ui: &mut Ui, _| delete(ui, def),
            )
            .default_button(),
        );
    }
    if in_place {
        return Some(
            rune_kit::Button::new(tr("Forget"))
                .variant(ButtonVariant::Ghost)
                .on_press_tx(move |ui: &mut Ui, _| delete(ui, def))
                .build_small_push(p.k)
                .key("delete-model"),
        );
    }
    Some(
        IconButton::new(&rune_theme_opencode::icons::TRASH)
            .variant(IconButtonVariant::Ghost)
            .control_size(ControlSize::Regular)
            .label(tr("Delete"))
            .on_press(move |ui: &mut Ui| delete(ui, def))
            .build(p.k)
            .key("delete-model"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(language: &str) -> Vec<Entry> {
        transcription_models::picker_models(language)
            .into_iter()
            .map(|(def, recommendation)| Entry {
                def,
                recommendation,
                supports: def.supports_language(language),
                state: State::NotDownloaded,
            })
            .collect()
    }

    fn ids(models: &[&Entry]) -> Vec<TranscriptionModelId> {
        models.iter().map(|entry| entry.def.id).collect()
    }

    #[test]
    fn the_model_in_use_leads_its_group_only_when_pinned() {
        let entries = entries("en");
        let unpinned = groups(&entries, None);
        assert_eq!(unpinned.leading[0].0, SPEAKING);
        let speaking = ids(&unpinned.leading[0].1);
        assert!(speaking.len() > 1);
        let last = *speaking.last().unwrap();
        let pinned = groups(&entries, Some(last));
        assert_eq!(pinned.leading[0].0, SPEAKING);
        let mut expected = vec![last];
        expected.extend_from_slice(&speaking[..speaking.len() - 1]);
        assert_eq!(ids(&pinned.leading[0].1), expected);
        assert_eq!(ids(&pinned.others), ids(&unpinned.others));
        assert_eq!(ids(&pinned.leading[1].1), ids(&unpinned.leading[1].1));
    }
}

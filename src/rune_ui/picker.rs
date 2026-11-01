//! The speech model: the local transcription picker, a modal listing every
//! model with the saved language's recommendations first, and the Language
//! select below the model in Settings. Choosing a model asks the service to
//! download, verify, and load it (`ChooseModel`) with the saved language, or
//! the model's default language when it does not support that one; changing
//! only the language prepares the same model again. Progress comes back in
//! the service snapshot, and the service saves the selection when it is
//! ready. Downloaded models other than the active one can be deleted after a
//! confirmation.

use rune::{El, col, el, row, text};
use rune_kit::{Badge, ControlSize, IconButton, IconButtonVariant, Progress};

use super::controls::{
    ButtonKind, Paint, button, modal_scrim_transition, modal_transition, pressable_row, row_switch,
    segmented,
};
use super::lists::error_panel;
use super::{Request, Ui};
use crate::app_settings::AppSettings;
use crate::desktop::i18n::{tr, tr_fill, tr_named};
use crate::transcription::models::{
    self as transcription_models, AUTO_LANGUAGE, BenchmarkRecord, ModelDefinition,
    ModelPreparationStage, ModelRuntime, Recommendation, TranscriptionModelId,
    TranscriptionSelection, language_name,
};

const MODAL_WIDTH: f32 = 620.0;
const MODAL_HEIGHT: f32 = 520.0;

/// A model is the active selection only once it is installed.
fn transcription_selection_is_active(
    selection: &TranscriptionSelection,
    model: &ModelDefinition,
    installed: bool,
) -> bool {
    installed && selection.model == model.id
}

/// The language a model choice replaced because the model does not support
/// it; Settings names the change until the next choice.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LanguageFallback {
    pub(super) model: TranscriptionModelId,
    pub(super) from: String,
}

/// How Labs › Sort and filter models orders the cards. Browsing only: it is
/// never saved and never changes the language or the active model.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Sort {
    /// The catalog's order for the saved language (the picker without the Lab).
    #[default]
    Recommended,
    /// Smallest model file first; models of unknown size last.
    Size,
    Name,
}

impl Sort {
    const ALL: [Self; 3] = [Self::Recommended, Self::Size, Self::Name];

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|sort| *sort == self)
            .unwrap_or_default()
    }
}

#[derive(Default)]
pub(super) struct Picker {
    pub(super) open: bool,
    /// The card order while Labs › Sort and filter models is on.
    sort: Sort,
    /// Only models that support the saved language (or detect it, for Auto),
    /// while Labs › Sort and filter models is on.
    supported_only: bool,
    /// The model whose Delete is armed.
    deletion: Option<TranscriptionModelId>,
    deletion_error: Option<String>,
    /// Installed models for fixtures; live windows ask the filesystem.
    pub(super) installed: Option<Vec<TranscriptionModelId>>,
    pub(super) fallback: Option<LanguageFallback>,
}

impl Picker {
    pub(super) fn installed(&self, model: &ModelDefinition, language: &str) -> bool {
        match &self.installed {
            Some(installed) => installed.contains(&model.id),
            None => transcription_models::is_installed(model, language),
        }
    }

    pub(super) fn selected_installed(&self, settings: &AppSettings) -> bool {
        transcription_models::validate(&settings.transcription)
            .is_ok_and(|model| self.installed(model, &settings.transcription.language))
    }

    fn clear_deletion(&mut self) {
        self.deletion = None;
        self.deletion_error = None;
    }
}

pub(super) fn open(ui: &mut Ui) {
    super::explainer::close(ui);
    super::sound_sets::close(ui);
    super::vocabulary::close(ui);
    ui.open = None;
    ui.picker.open = true;
    ui.picker.clear_deletion();
    ui.picker.sort = Sort::default();
    ui.picker.supported_only = false;
}

/// Whether Labs › Sort and filter models is on.
fn sort_filter_on(ui: &Ui) -> bool {
    let feature = crate::labs::Feature::PickerSortFilter;
    feature.shipped() && feature.on(&ui.settings)
}

/// Whether Labs › Benchmark details is on.
fn benchmark_details_on(ui: &Ui) -> bool {
    let feature = crate::labs::Feature::BenchmarkDetails;
    feature.shipped() && feature.on(&ui.settings)
}

/// The badge a card shows. With Benchmark details on, "Most accurate" stays
/// only when its figure is comparable with the other recommendations'.
fn shown_recommendation(
    recommendation: Option<Recommendation>,
    model: &ModelDefinition,
    language: &str,
    details: bool,
) -> Option<Recommendation> {
    recommendation.filter(|recommendation| {
        !details
            || *recommendation != Recommendation::MostAccurate
            || transcription_models::most_accurate_is_comparable(model, language)
    })
}

/// The raw error rate with its direction: `7.57% CER · lower is better`.
fn benchmark_rate(record: &BenchmarkRecord) -> String {
    tr_named(
        "{rate} · lower is better",
        &[("rate", record.rate_label().as_str())],
    )
}

/// One line of where a figure comes from: language, dataset and split,
/// source, conditions, and the speed figure's qualification. Whatever the
/// source did not record reads as unknown.
fn benchmark_detail(record: &BenchmarkRecord) -> String {
    let mut parts = vec![
        language_name(record.language).to_owned(),
        record
            .dataset
            .map_or_else(|| tr("Dataset unknown").to_owned(), str::to_owned),
        match (record.source, record.source_revision) {
            (Some(source), Some(revision)) => tr_named(
                "Source: {source} at {revision}",
                &[("source", source), ("revision", revision)],
            ),
            (Some(source), None) => tr_named("Source: {source}", &[("source", source)]),
            (None, _) => tr("Source unrecorded").to_owned(),
        },
    ];
    if record.utterances.is_none()
        && record.batch_size.is_none()
        && record.timestamps.is_none()
        && record.hardware.is_none()
    {
        parts.push(tr("Conditions unknown").to_owned());
    } else {
        if let Some(utterances) = record.utterances {
            parts.push(tr_fill("{} utterances", &utterances.to_string()));
        }
        if let Some(batch) = record.batch_size {
            parts.push(tr_fill("Batch size {}", &batch.to_string()));
        }
        if let Some(timestamps) = record.timestamps {
            parts.push(
                if timestamps {
                    tr("With timestamps")
                } else {
                    tr("Without timestamps")
                }
                .to_owned(),
            );
        }
        parts.push(
            record
                .hardware
                .map_or_else(|| tr("Hardware unknown").to_owned(), str::to_owned),
        );
    }
    parts.push(tr("Speed: upstream figure; hardware and method unknown").to_owned());
    parts.join(" · ")
}

type PickerModel = (
    &'static ModelDefinition,
    Option<transcription_models::Recommendation>,
);

/// The picker's models for `language` in `sort`'s order, only those that
/// support it when `supported_only`. Recommended keeps the catalog's order.
fn arranged(language: &str, sort: Sort, supported_only: bool) -> Vec<PickerModel> {
    let mut models = transcription_models::picker_models(language);
    if supported_only {
        models.retain(|(model, _)| model.supports_language(language));
    }
    match sort {
        Sort::Recommended => {}
        Sort::Size => models.sort_by_key(|(model, _)| {
            let bytes = model.download_bytes();
            (bytes.is_none(), bytes.unwrap_or_default())
        }),
        Sort::Name => models.sort_by_cached_key(|(model, _)| model.name.to_lowercase()),
    }
    models
}

/// The cards the picker shows: the catalog's order, or with the Lab on its
/// sort and language toggle.
fn shown_models(ui: &Ui, language: &str) -> Vec<PickerModel> {
    if sort_filter_on(ui) {
        arranged(language, ui.picker.sort, ui.picker.supported_only)
    } else {
        transcription_models::picker_models(language)
    }
}

/// The model the language toggle hides that still matters: the one being
/// prepared (true), else the active one (false).
fn hidden_model(
    shown: &[PickerModel],
    preparing: Option<TranscriptionModelId>,
    active: Option<TranscriptionModelId>,
) -> Option<(TranscriptionModelId, bool)> {
    let hidden = |id: &TranscriptionModelId| shown.iter().all(|(model, _)| model.id != *id);
    preparing
        .filter(|id| hidden(id))
        .map(|id| (id, true))
        .or_else(|| active.filter(|id| hidden(id)).map(|id| (id, false)))
}

pub(super) fn close(ui: &mut Ui) {
    ui.picker.open = false;
    ui.picker.clear_deletion();
}

fn cancel(ui: &mut Ui) {
    if ui.preparing.is_some() {
        ui.send(Request::CancelModel);
    }
}

/// Asks for `model` with the saved language when it supports it, else with
/// its default language, which Settings then names.
fn choose(ui: &mut Ui, model: &'static ModelDefinition) {
    ui.picker.clear_deletion();
    let current = &ui.settings.transcription;
    let installed = ui.picker.installed(model, &current.language);
    if transcription_selection_is_active(current, model, installed) {
        close(ui);
        return;
    }
    let language = model.language_for(&current.language).to_owned();
    ui.picker.fallback = (language != current.language).then(|| LanguageFallback {
        model: model.id,
        from: current.language.clone(),
    });
    ui.send(Request::ChooseModel {
        model: model.id,
        language,
    });
}

/// Prepares the saved model again for `language`, which installs or loads
/// whatever that language needs (Apple Speech's locales).
pub(super) fn choose_language(ui: &mut Ui, language: &str) {
    ui.open = None;
    let current = &ui.settings.transcription;
    if current.language == language {
        return;
    }
    ui.picker.fallback = None;
    ui.send(Request::ChooseModel {
        model: current.model,
        language: language.to_owned(),
    });
}

/// The fallback's note, once the service saved the model it was chosen for.
pub(super) fn fallback_note(ui: &Ui) -> Option<String> {
    let fallback = ui.picker.fallback.as_ref()?;
    let selection = &ui.settings.transcription;
    (selection.model == fallback.model && selection.language != fallback.from).then(|| {
        tr_named(
            "{model} doesn't support {previous}, so the language changed to {language}",
            &[
                (
                    "model",
                    transcription_models::definition(fallback.model).name,
                ),
                ("previous", language_name(&fallback.from)),
                ("language", language_name(&selection.language)),
            ],
        )
    })
}

fn delete(ui: &mut Ui, model: &'static ModelDefinition) {
    ui.picker.deletion_error = None;
    if ui.picker.deletion != Some(model.id) {
        ui.picker.deletion = Some(model.id);
        return;
    }
    ui.picker.deletion = None;
    if !ui.live() {
        return;
    }
    match transcription_models::remove_installed(model) {
        Ok(()) => tracing::info!(model = ?model.id, "deleted transcription model"),
        Err(error) => ui.picker.deletion_error = Some(error.to_string()),
    }
}

/// The picker's scrim and panel over the window, when open: the scrim fades
/// and the panel scales in as one surface ([`modal_transition`]).
pub(super) fn modal(ui: &Ui, p: Paint) -> Option<[El; 2]> {
    if !ui.picker.open {
        return None;
    }
    let c = &p.k.c;
    let language = ui.settings.transcription.language.clone();
    let mut models = col().key("models").gap(8.0);
    let transcription = ui.view.as_ref().map(|view| &view.transcription);
    if let Some(error) = transcription.and_then(|t| t.error.clone()) {
        models = models.child(error_panel(
            p,
            "install-error",
            tr("Model could not be installed."),
            &error,
        ));
    }
    if let Some(error) = &ui.picker.deletion_error {
        models = models.child(error_panel(
            p,
            "delete-error",
            tr("Model could not be deleted."),
            error,
        ));
    }
    let shown = shown_models(ui, &language);
    for (index, (model, recommendation)) in shown.iter().copied().enumerate() {
        models = models.child(model_card(ui, p, index, model, recommendation, &language));
    }
    let header = row()
        .key("picker-header")
        .h(52.0)
        .pl(20.0)
        .pr(12.0)
        .center_y()
        .child(
            text(tr("Local transcription"))
                .key("title")
                .font(p.k.ty.body_medium)
                .color(p.text())
                .grow(1.0),
        )
        .child(
            IconButton::new(p.k.glyph.close)
                .variant(IconButtonVariant::Ghost)
                .control_size(ControlSize::Regular)
                .label(tr("Close"))
                .on_press(close)
                .build(p.k)
                .key("close-picker"),
        );
    let mut panel = col()
        .key("transcription-picker")
        .role("dialog")
        .w(MODAL_WIDTH)
        .h(MODAL_HEIGHT)
        .radius(p.k.radius.xl)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak)
        // Children sit inside the border, so their fills never paint over it.
        .padding(p.hair)
        .clip_soft(0.0)
        // Presses inside the panel stay inside.
        .on_press(|_: &mut Ui| {})
        .child(header)
        .child(el().key("rule").h(p.hair).w_full().fill(c.border_weak));
    if sort_filter_on(ui) {
        panel = panel.child(sort_filter_row(ui, p, &language));
        if let Some(note) = hidden_note(ui, p, &shown, &language) {
            panel = panel.child(note);
        }
    }
    panel = panel.child(
        col()
            .key("model-column")
            .grow(1.0)
            .shrink()
            .w_full()
            .pt(12.0)
            .child(super::scroll_body("model-scroll", models.px(16.0))),
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
        layer("picker-scrim")
            .fill(c.scrim)
            .on_press(close)
            .transition(modal_scrim_transition(p)),
        layer("picker-layer")
            .center()
            .transition(modal_transition(p))
            // Keys stay inside, Escape or a press outside closes, and focus
            // returns to the control that opened it.
            .child(panel.modal(true, close)),
    ])
}

/// `model`'s preparation label and progress while the service prepares it.
fn preparation(ui: &Ui, model: &ModelDefinition) -> Option<(String, Option<f32>)> {
    let transcription = ui.view.as_ref().map(|view| &view.transcription)?;
    if transcription.preparing != Some(model.id) {
        return None;
    }
    Some(match transcription.preparation_stage {
        Some(ModelPreparationStage::Verifying) => (tr("Verifying model").into(), None),
        Some(ModelPreparationStage::Loading) => {
            let elapsed = ui
                .preparing
                .map_or(0, |(_, since)| since.elapsed().as_secs());
            (tr_fill("Loading model · {}s", &elapsed.to_string()), None)
        }
        _ => {
            let fraction = model.download_bytes().map_or(0.0, |bytes| {
                (transcription.downloaded_bytes as f32 / bytes as f32).clamp(0.0, 1.0)
            });
            (
                tr_fill("Downloading {}%", &format!("{:.0}", fraction * 100.0)),
                matches!(model.runtime, ModelRuntime::Gguf(_)).then_some(fraction),
            )
        }
    })
}

/// Labs › Sort and filter models' row under the header: the sort and the
/// saved-language toggle. Neither saves anything.
fn sort_filter_row(ui: &Ui, p: Paint, language: &str) -> El {
    let labels = [tr("By recommendation"), tr("By size"), tr("By name")];
    let sort = segmented(
        p,
        "picker-sort",
        &labels,
        ui.picker.sort.index(),
        |ui: &mut Ui, index| ui.picker.sort = Sort::ALL[index],
    );
    let label = if language == AUTO_LANGUAGE {
        tr("Only models that detect the language automatically").to_owned()
    } else {
        tr_named(
            "Only models that support {language}",
            &[("language", language_name(language))],
        )
    };
    let toggle = pressable_row(
        row()
            .key("picker-supported")
            .label(label.clone())
            .gap(8.0)
            .center_y()
            .shrink()
            .child(
                text(label)
                    .key("label")
                    .font(p.k.ty.small)
                    .color(p.text())
                    .shrink()
                    .wrap(),
            )
            .child(row_switch(p, ui.picker.supported_only)),
    )
    .on_press(|ui: &mut Ui| ui.picker.supported_only = !ui.picker.supported_only);
    row()
        .key("picker-controls")
        .px(16.0)
        .pt(12.0)
        .gaps(8.0, 12.0)
        .center_y()
        .justify_between()
        .flex_wrap()
        .child(sort)
        .child(toggle)
}

/// One line outside the results naming the active model or the one being
/// prepared while the language toggle hides it, with the preparation's
/// Cancel.
fn hidden_note(ui: &Ui, p: Paint, shown: &[PickerModel], language: &str) -> Option<El> {
    let transcription = ui.view.as_ref().map(|view| &view.transcription);
    let preparing = transcription.and_then(|t| t.preparing);
    let selected = transcription_models::definition(ui.settings.transcription.model);
    let active = transcription_selection_is_active(
        &ui.settings.transcription,
        selected,
        ui.picker
            .installed(selected, selected.language_for(language)),
    )
    .then_some(selected.id);
    let (id, is_preparing) = hidden_model(shown, preparing, active)?;
    let model = transcription_models::definition(id);
    let state = if is_preparing {
        preparation(ui, model)?.0
    } else {
        tr("Active").to_owned()
    };
    let mut note = row()
        .key("picker-hidden")
        .px(16.0)
        .pt(8.0)
        .gap(8.0)
        .center_y()
        .child(
            text(tr_named(
                "{model} is hidden by the language filter: {state}",
                &[("model", model.name), ("state", state.as_str())],
            ))
            .key("label")
            .font(p.k.ty.small)
            .color(p.muted())
            .grow(1.0)
            .shrink()
            .wrap(),
        );
    if is_preparing {
        note = note.child(button(
            p,
            "cancel-hidden-model",
            tr("Cancel"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| cancel(ui),
        ));
    }
    Some(note)
}

enum Status {
    Active,
    Available {
        installed: bool,
    },
    Preparing {
        label: String,
        progress: Option<f32>,
    },
}

fn model_card(
    ui: &Ui,
    p: Paint,
    index: usize,
    model: &'static ModelDefinition,
    recommendation: Option<transcription_models::Recommendation>,
    language: &str,
) -> El {
    let c = &p.k.c;
    let installed = ui.picker.installed(model, model.language_for(language));
    let preparation = preparation(ui, model);
    let preparing = preparation.is_some();
    let active = transcription_selection_is_active(&ui.settings.transcription, model, installed);
    let status = if let Some((label, progress)) = preparation {
        Status::Preparing { label, progress }
    } else if active {
        Status::Active
    } else {
        Status::Available { installed }
    };
    let deletable = installed
        && !preparing
        && matches!(model.runtime, ModelRuntime::Gguf(_))
        && ui.settings.transcription.model != model.id;
    let confirming = ui.picker.deletion == Some(model.id);
    let metadata = model
        .streams
        .then(|| tr("Transcribes while you speak on a GPU"))
        .into_iter()
        .chain([
            tr(model.coverage),
            tr(model.quality_context),
            tr(model.timestamps),
        ])
        .chain((installed && !active).then(|| tr("Installed")))
        .collect::<Vec<_>>()
        .join(" · ");
    let mut actions = row().key("actions").gap(8.0).center_y();
    match &status {
        Status::Active => {
            actions = actions.child(
                row()
                    .key("active")
                    .gap(6.0)
                    .center_y()
                    .child(super::controls::status_dot(p.accent()))
                    .child(
                        text(tr("Active"))
                            .key("label")
                            .font(p.k.ty.ui_medium)
                            .color(p.text()),
                    ),
            );
        }
        Status::Available { installed } => {
            if deletable {
                let label = if confirming {
                    tr_fill("Delete {}?", &model.size_label())
                } else {
                    tr("Delete").into()
                };
                let delete_button = button(
                    p,
                    "delete-model",
                    &label,
                    if confirming {
                        ButtonKind::Danger
                    } else {
                        ButtonKind::Secondary
                    },
                    move |ui: &mut Ui, _| delete(ui, model),
                );
                actions = actions.child(if confirming {
                    delete_button.default_button()
                } else {
                    delete_button
                });
            }
            actions = actions.child(button(
                p,
                "choose-model",
                if *installed {
                    tr("Use")
                } else {
                    tr("Download")
                },
                ButtonKind::Secondary,
                move |ui: &mut Ui, _| choose(ui, model),
            ));
        }
        Status::Preparing { .. } => {
            actions = actions.child(button(
                p,
                "cancel-model",
                tr("Cancel"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| cancel(ui),
            ));
        }
    }
    let mut title = row()
        .key("title")
        .gap(8.0)
        .center_y()
        .grow(1.0)
        .shrink()
        .child(
            text(model.name)
                .key("name")
                .font(p.k.ty.body_medium)
                .color(p.text()),
        );
    // Labs › Benchmark details: raw error rates with their provenance.
    let details = benchmark_details_on(ui);
    let record = details
        .then(|| transcription_models::benchmark(model))
        .flatten();
    if let Some(recommendation) = shown_recommendation(recommendation, model, language, details) {
        title = title.child(Badge::new(tr(recommendation.label())).build(p.k));
    }
    if model.streams {
        title = title.child(Badge::new(tr("Streams")).build(p.k));
    }
    let metric = |key: &str, label: &str, value: String| {
        row()
            .key(key)
            .gap(6.0)
            .child(text(label).key("label").font(p.k.ty.small).color(p.muted()))
            .child(
                text(value)
                    .key("value")
                    .font(p.k.ty.small_medium)
                    .color(p.text()),
            )
    };
    let mut card = col()
        .key(format!("model{index}"))
        .px(16.0)
        .py(12.0)
        .gap(8.0)
        .radius(p.k.radius.lg)
        .fill(if active { c.layer_02 } else { c.bg_base })
        .border(
            p.hair,
            if active {
                c.border_focus
            } else {
                c.border_weak
            },
        )
        .child(
            row()
                .key("top")
                .gap(12.0)
                .center_y()
                .child(title)
                .child(actions),
        )
        .child(
            text(metadata)
                .key("metadata")
                .font(p.k.ty.small)
                .color(p.muted())
                .wrap(),
        )
        .child(
            row()
                .key("metrics")
                .gap(20.0)
                .child(metric("speed", tr("Speed"), tr(model.realtime).into()))
                .child(metric("size", tr("Size"), model.size_label()))
                .child(match &record {
                    Some(record) => metric("error-rate", tr("Error rate"), benchmark_rate(record)),
                    None => metric(
                        "accuracy",
                        tr("Accuracy"),
                        model.accuracy_label().unwrap_or_else(|| "System".into()),
                    ),
                }),
        );
    if let Some(record) = &record {
        card = card.child(
            text(benchmark_detail(record))
                .key("benchmark")
                .font(p.k.ty.small)
                .color(p.muted())
                .wrap(),
        );
    }
    if !model.supports_language(language) {
        card = card.child(
            text(tr_named(
                "Doesn't support {language}; switches the language to {fallback}",
                &[
                    ("language", language_name(language)),
                    ("fallback", language_name(model.default_language())),
                ],
            ))
            .key("language-switch")
            .font(p.k.ty.small)
            .color(p.warning())
            .wrap(),
        );
    }
    if model.id == TranscriptionModelId::ParakeetV3 {
        card = card.child(
            text(
                "Detects language from audio, not the selected language. \
                 For language-guided transcription, try Whisper.",
            )
            .key("note")
            .font(p.k.ty.small)
            .color(p.muted())
            .wrap(),
        );
    }
    // One bar from download through verifying and loading: it fills, then
    // sweeps while the stage has no measure.
    if let Status::Preparing { label, progress } = status {
        let bar = Progress::new(progress).label(label);
        let bar = if progress.is_some() {
            bar.show_value()
        } else {
            bar
        };
        card = card.child(bar.build(p.k).key("progress"));
    }
    if !active && !preparing {
        card = card
            .role("button")
            .hover(c.hover)
            .press(c.pressed)
            .on_press(move |ui: &mut Ui| choose(ui, model));
    }
    card
}

/// The picker open with English and Parakeet saved; `downloading` shows a
/// model mid-download.
pub(super) fn fixture(ui: &mut Ui, downloading: Option<TranscriptionModelId>) {
    ui.picker.open = true;
    if let (Some(model), Some(view)) = (downloading, ui.view.as_mut()) {
        let bytes = transcription_models::definition(model)
            .download_bytes()
            .unwrap_or(0);
        view.transcription.preparing = Some(model);
        view.transcription.downloaded_bytes = bytes * 37 / 100;
        view.transcription.preparation_stage = Some(ModelPreparationStage::Downloading);
        ui.preparing = Some((model, std::time::Instant::now()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_transcription_model_is_not_active_until_installed() {
        let selection = TranscriptionSelection::default();
        let model = transcription_models::definition(selection.model);

        assert!(!transcription_selection_is_active(&selection, model, false));
        assert!(transcription_selection_is_active(&selection, model, true));
    }

    fn ids(models: &[PickerModel]) -> Vec<TranscriptionModelId> {
        models.iter().map(|(model, _)| model.id).collect()
    }

    #[test]
    fn recommended_keeps_the_catalogs_order_and_the_others_reorder_it() {
        for language in ["en", "ja", AUTO_LANGUAGE] {
            assert_eq!(
                ids(&arranged(language, Sort::Recommended, false)),
                ids(&transcription_models::picker_models(language))
            );
            let by_size = arranged(language, Sort::Size, false);
            let sizes: Vec<_> = by_size
                .iter()
                .map(|(model, _)| model.download_bytes())
                .collect();
            let known = sizes.iter().take_while(|bytes| bytes.is_some()).count();
            assert!(
                sizes[known..].iter().all(Option::is_none),
                "unknown sizes last"
            );
            assert!(sizes[..known].windows(2).all(|pair| pair[0] <= pair[1]));
            let by_name = arranged(language, Sort::Name, false);
            assert!(
                by_name
                    .windows(2)
                    .all(|pair| { pair[0].0.name.to_lowercase() <= pair[1].0.name.to_lowercase() })
            );
            assert_eq!(by_name.len(), by_size.len());
        }
    }

    #[test]
    fn the_language_toggle_keeps_only_models_for_the_saved_language() {
        for language in ["en", "ja", "mt", AUTO_LANGUAGE] {
            let all = transcription_models::picker_models(language);
            let supported = arranged(language, Sort::Recommended, true);
            assert!(!supported.is_empty());
            assert_eq!(
                supported.len(),
                all.iter()
                    .filter(|(model, _)| model.language_for(language) == language)
                    .count()
            );
        }
    }

    #[test]
    fn a_hidden_preparing_or_active_model_is_named() {
        let shown = arranged("ja", Sort::Recommended, true);
        let hidden = TranscriptionModelId::ParakeetUnifiedEnglish;
        let visible = shown[0].0.id;
        assert!(!ids(&shown).contains(&hidden));
        assert_eq!(
            hidden_model(&shown, Some(hidden), Some(visible)),
            Some((hidden, true))
        );
        assert_eq!(
            hidden_model(&shown, None, Some(hidden)),
            Some((hidden, false))
        );
        assert_eq!(hidden_model(&shown, Some(visible), Some(visible)), None);
        assert_eq!(hidden_model(&shown, None, None), None);
    }

    #[test]
    fn benchmark_details_show_raw_rates_and_mark_unknowns() {
        let qwen = transcription_models::definition(TranscriptionModelId::Qwen3Asr06B);
        let record = transcription_models::benchmark(qwen).unwrap();
        assert_eq!(benchmark_rate(&record), "7.57% CER · lower is better");
        let detail = benchmark_detail(&record);
        for part in [
            "Mandarin Chinese",
            "FLEURS test",
            "at 7045d1c",
            "945 utterances",
            "Batch size 8",
            "Without timestamps",
            "Hardware unknown",
            "Speed: upstream figure",
        ] {
            assert!(detail.contains(part), "{detail:?} lacks {part:?}");
        }
        let parakeet = transcription_models::definition(TranscriptionModelId::ParakeetV2);
        let detail = benchmark_detail(&transcription_models::benchmark(parakeet).unwrap());
        for part in [
            "English",
            "Dataset unknown",
            "Source unrecorded",
            "Conditions unknown",
        ] {
            assert!(detail.contains(part), "{detail:?} lacks {part:?}");
        }
        let apple = transcription_models::definition(TranscriptionModelId::AppleSpeech);
        assert!(transcription_models::benchmark(apple).is_none());
    }

    #[test]
    fn most_accurate_needs_comparable_figures_only_with_benchmark_details() {
        let cohere = transcription_models::definition(TranscriptionModelId::CohereTranscribe);
        let badge = Some(Recommendation::MostAccurate);
        assert_eq!(shown_recommendation(badge, cohere, "en", false), badge);
        assert_eq!(shown_recommendation(badge, cohere, "en", true), None);
        let recommended = Some(Recommendation::Recommended);
        assert_eq!(
            shown_recommendation(recommended, cohere, "el", true),
            recommended
        );
    }

    #[test]
    fn benchmark_details_are_off_by_default() {
        let ui = Ui::with(
            AppSettings::default(),
            super::super::Service::Fixture(Vec::new()),
        );
        assert!(!benchmark_details_on(&ui));
        let mut settings = AppSettings::default();
        settings.labs.benchmark_details = true;
        let ui = Ui::with(settings, super::super::Service::Fixture(Vec::new()));
        assert!(benchmark_details_on(&ui));
    }

    #[test]
    fn browsing_never_saves_or_asks_for_a_model() {
        let mut settings = AppSettings::default();
        settings.labs.picker_sort_filter = true;
        let mut ui = Ui::with(settings, super::super::Service::Fixture(Vec::new()));
        let before = ui.settings.clone();
        open(&mut ui);
        assert!(sort_filter_on(&ui));
        ui.picker.sort = Sort::Size;
        ui.picker.supported_only = true;
        let _ = shown_models(&ui, &before.transcription.language);
        assert_eq!(ui.settings, before);
        assert_eq!(ui.saved, 0);
        assert!(matches!(&ui.service, super::super::Service::Fixture(sent) if sent.is_empty()));
        close(&mut ui);
        open(&mut ui);
        assert_eq!(ui.picker.sort, Sort::Recommended);
        assert!(!ui.picker.supported_only);
    }
}

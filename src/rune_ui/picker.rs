//! The speech model: the local transcription picker, a modal that compares
//! the saved language's first three models side by side
//! ([`compare`]) and lists every model a step away
//! ([`list`]), and the Language select below the model in
//! Settings. Choosing a model asks the service to
//! download, verify, and load it (`ChooseModel`) with the saved language, or
//! the model's default language when it does not support that one; changing
//! only the language prepares the same model again. Progress comes back in
//! the service snapshot, and the service saves the selection when it is
//! ready. Downloaded models other than the active one can be deleted after a
//! confirmation.

mod compare;
mod facts;
mod list;

use rune::{El, col, el, row, text};
use rune_kit::{ControlSize, IconButton, IconButtonVariant};

use super::controls::{
    ButtonKind, Paint, button, modal_scrim_transition, modal_transition, pressable_row, row_switch,
    segmented,
};
use super::lists::{Tone, error_panel};
use super::{Request, Ui};
use crate::app_settings::AppSettings;
use crate::desktop::i18n::{tr, tr_fill, tr_named};
use crate::transcription::models::{
    self as transcription_models, AUTO_LANGUAGE, BenchmarkRecord, ModelDefinition,
    ModelPreparationStage, ModelRuntime, Recommendation, TranscriptionModelId,
    TranscriptionSelection, language_name,
};

const PANEL_WIDTH: f32 = compare::WIDTH;
/// The full list's height: the window's less a margin above and below.
const PANEL_HEIGHT: f32 = 680.0;

/// A model is the active selection only once it is installed.
pub(super) fn transcription_selection_is_active(
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

/// What the picker shows: the saved language's first three models side
/// by side, or every model.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum View {
    #[default]
    Compare,
    All,
}

#[derive(Default)]
pub(super) struct Picker {
    pub(super) open: bool,
    pub(super) view: View,
    /// The models that don't speak the saved language, in the full list.
    pub(super) others_open: bool,
    /// The card order while Labs › Sort and filter models is on.
    sort: Sort,
    /// Only models that support the saved language (or detect it, for Auto),
    /// while Labs › Sort and filter models is on.
    supported_only: bool,
    /// Only downloaded models, while Labs › Downloaded models only is on.
    downloaded_only: bool,
    /// The model whose Delete (an import's Remove) is armed.
    pub(super) deletion: Option<TranscriptionModelId>,
    deletion_error: Option<String>,
    /// Installed models for fixtures; live windows ask the filesystem.
    pub(super) installed: Option<Vec<TranscriptionModelId>>,
    pub(super) fallback: Option<LanguageFallback>,
    /// Your own speech models: the file dialog, the import
    /// worker, and the name being edited.
    pub(super) import: super::model_import::ImportState,
    /// The card whose details are open (Parakeet Ultra's credit and
    /// published benchmarks).
    pub(super) details: super::model_details::DetailsState,
    /// The model in use when the picker opened, which Labs › Model in use
    /// first lists first; fixed until the picker closes.
    in_use_first: Option<TranscriptionModelId>,
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
    // Models already on this computer: headers only, on a worker.
    super::found::scan(ui);
    ui.picker.sort = Sort::default();
    ui.picker.supported_only = false;
    ui.picker.downloaded_only = false;
    ui.picker.view = View::Compare;
    ui.picker.others_open = false;
    ui.picker.in_use_first = (active_first_on(ui) && ui.picker.selected_installed(&ui.settings))
        .then_some(ui.settings.transcription.model);
}

/// Switches between the comparison and the full list.
pub(super) fn show(ui: &mut Ui, view: View) {
    ui.picker.view = view;
    ui.picker.clear_deletion();
}

/// Whether Labs › Sort and filter models is on.
fn sort_filter_on(ui: &Ui) -> bool {
    let feature = crate::labs::Feature::PickerSortFilter;
    feature.shipped() && feature.on(&ui.settings)
}

/// Whether Labs › Downloaded models only is on.
fn downloaded_on(ui: &Ui) -> bool {
    let feature = crate::labs::Feature::PickerDownloaded;
    feature.shipped() && feature.on(&ui.settings)
}

/// Whether the full list shows only downloaded models now.
pub(super) fn downloaded_filter(ui: &Ui) -> bool {
    downloaded_on(ui) && ui.picker.downloaded_only
}

/// Whether `model` is downloaded for the picker, as its row's state reads it.
fn is_downloaded(ui: &Ui, model: &ModelDefinition, language: &str) -> bool {
    ui.picker.installed(model, model.language_for(language))
}

/// The bytes the downloaded `models` take in Hex's own models folder: files
/// another app downloaded, or added where they are, aren't Hex's.
fn downloaded_total(
    models: &[PickerModel],
    downloaded: impl Fn(&ModelDefinition) -> bool,
    owned: impl Fn(&ModelDefinition) -> bool,
) -> u64 {
    models
        .iter()
        .filter(|(model, _)| downloaded(model) && owned(model))
        .filter_map(|(model, _)| model.download_bytes())
        .sum()
}

/// Whether Hex keeps `model`'s file in its own models folder.
fn owned_by_hex(model: &ModelDefinition) -> bool {
    if model.id.is_custom() {
        return !crate::transcription::custom_models::imported()
            .iter()
            .any(|import| import.definition.id == model.id && import.in_place);
    }
    !transcription_models::is_in_place(model)
}

/// A size as the picker writes one: `2.4 GB`, `640 MB`.
fn bytes_label(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else {
        format!("{} MB", bytes / 1_000_000)
    }
}

/// Whether Labs › Model in use first is on.
fn active_first_on(ui: &Ui) -> bool {
    let feature = crate::labs::Feature::PickerActiveFirst;
    feature.shipped() && feature.on(&ui.settings)
}

/// The model the comparison shows first: the one in use when the picker
/// opened, while Labs › Model in use first is on.
fn pinned(ui: &Ui) -> Option<TranscriptionModelId> {
    ui.picker.in_use_first.filter(|_| active_first_on(ui))
}

/// The model the full list leads with: [`pinned`], unless Labs › Sort and
/// filter models sorts by size or name, which keep their exact order.
fn list_first(ui: &Ui) -> Option<TranscriptionModelId> {
    pinned(ui).filter(|_| !sort_filter_on(ui) || ui.picker.sort == Sort::Recommended)
}

/// Whether Labs › Benchmark details is on.
pub(super) fn benchmark_details_on(ui: &Ui) -> bool {
    let feature = crate::labs::Feature::BenchmarkDetails;
    feature.shipped() && feature.on(&ui.settings)
}

/// The badge a card shows. With Benchmark details on, "Most accurate" stays
/// only when its figure is comparable with the other recommendations'.
pub(super) fn shown_recommendation(
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
pub(super) fn benchmark_rate(record: &BenchmarkRecord) -> String {
    tr_named(
        "{rate} · lower is better",
        &[("rate", record.rate_label().as_str())],
    )
}

/// One line of where a figure comes from: language, dataset and split,
/// source, conditions, and the speed figure's qualification. Whatever the
/// source did not record reads as unknown.
pub(super) fn benchmark_detail(record: &BenchmarkRecord) -> String {
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

pub(super) type PickerModel = (
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
    let mut models = if sort_filter_on(ui) {
        arranged(language, ui.picker.sort, ui.picker.supported_only)
    } else {
        transcription_models::picker_models(language)
    };
    // Labs › Downloaded models only.
    if downloaded_filter(ui) {
        models.retain(|(model, _)| is_downloaded(ui, model, language));
    }
    models
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
    ui.picker.in_use_first = None;
    ui.picker.clear_deletion();
}

pub(super) fn cancel(ui: &mut Ui) {
    if ui.preparing.is_some() {
        ui.send(Request::CancelModel);
    }
}

/// Asks for `model` with the saved language when it supports it, else with
/// its default language, which Settings then names.
pub(super) fn choose(ui: &mut Ui, model: &'static ModelDefinition) {
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

pub(super) fn delete(ui: &mut Ui, model: &'static ModelDefinition) {
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
    let view = ui.picker.view;
    let inset = match view {
        View::Compare => 24.0,
        View::All => 16.0,
    };
    let transcription = ui.view.as_ref().map(|view| &view.transcription);
    let errors = [
        (
            "install-error",
            tr("Model could not be installed."),
            transcription.and_then(|t| t.error.as_deref()),
        ),
        (
            "import-error",
            tr("Model could not be added."),
            ui.picker.import.error.as_deref(),
        ),
        (
            "delete-error",
            tr("Model could not be deleted."),
            ui.picker.deletion_error.as_deref(),
        ),
        (
            "found-error",
            tr("Model could not be used."),
            ui.found.error.as_deref(),
        ),
    ];
    let close_button = IconButton::new(p.k.glyph.close)
        .variant(IconButtonVariant::Ghost)
        .control_size(ControlSize::Regular)
        .label(tr("Close"))
        .on_press(close)
        .build(p.k)
        .key("close-picker");
    let mut header = row()
        .key("picker-header")
        .h(56.0)
        .pr(12.0)
        .gap(8.0)
        .center_y();
    let title = match view {
        View::Compare => facts::title(&language),
        View::All => tr("All speech models").to_owned(),
    };
    let heading = |title: &str| {
        text(title.to_owned())
            .key("title")
            .role("heading")
            .level(2)
            .font(p.k.ty.body_medium)
            .color(p.text())
            .grow(1.0)
    };
    let body = match view {
        View::Compare => {
            let entries = facts::entries(
                ui,
                &language,
                &transcription_models::picker_models(&language),
            );
            header = header.pl(24.0).child(heading(&title));
            compare::body(ui, p, &entries, &language)
        }
        View::All => {
            let shown = shown_models(ui, &language);
            let entries = facts::entries(ui, &language, &shown);
            header = header
                .pl(12.0)
                .child(
                    IconButton::new(&rune_theme_opencode::icons::CHEVRON_RIGHT)
                        .variant(IconButtonVariant::Ghost)
                        .control_size(ControlSize::Regular)
                        .label(tr("Back"))
                        .on_press(|ui: &mut Ui| show(ui, View::Compare))
                        .build(p.k)
                        .key("picker-back")
                        .rotate(std::f32::consts::PI),
                )
                .child(heading(&title));
            // Labs › Use model files where they are.
            if let Some(in_place) = super::model_import::in_place_button(ui, p) {
                header = header.child(in_place);
            }
            header = header.child(super::model_import::add_button(ui, p));
            let mut all = col().key("all");
            if sort_filter_on(ui) {
                all = all.child(sort_filter_row(ui, p, &language));
            }
            if downloaded_on(ui) {
                all = all.child(downloaded_row(ui, p, &language));
            }
            if (sort_filter_on(ui) || downloaded_on(ui))
                && let Some(note) = hidden_note(ui, p, &shown, &language)
            {
                all = all.child(note);
            }
            all.child(list::body(ui, p, &entries, &language))
        }
    };
    let header = header.child(close_button);
    let mut content = col().key("picker-content");
    if errors.iter().any(|(_, _, detail)| detail.is_some()) {
        let mut panels = col().key("errors").gap(8.0).px(inset).pb(8.0);
        for (key, title, detail) in errors {
            if let Some(detail) = detail {
                panels = panels.child(error_panel(p, key, title, detail, Tone::Danger));
            }
        }
        content = content.child(panels);
    }
    let content = content.child(body);
    let mut panel = col()
        .key("transcription-picker")
        .role("dialog")
        .label(title)
        .w(PANEL_WIDTH)
        .radius(p.k.radius.xl)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak)
        // Children sit inside the border, so their fills never paint over it.
        .padding(p.hair)
        .clip_soft(0.0)
        // Presses inside the panel stay inside.
        .on_press(|_: &mut Ui| {})
        .child(header);
    // The comparison fits; the list scrolls inside the window's height.
    panel = match view {
        View::Compare => panel.child(content),
        View::All => panel
            .h(PANEL_HEIGHT)
            .child(super::scroll_body(p.k, "model-scroll", content)),
    };
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
pub(super) fn preparation(ui: &Ui, model: &ModelDefinition) -> Option<(String, Option<f32>)> {
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
                    .wrap()
                    .a11y_hidden(),
            )
            .child(row_switch(p, ui.picker.supported_only)),
        ui.picker.supported_only,
    )
    .on_press(|ui: &mut Ui| ui.picker.supported_only = !ui.picker.supported_only);
    row()
        .key("picker-controls")
        .px(16.0)
        .pb(16.0)
        .gaps(8.0, 12.0)
        .center_y()
        .justify_between()
        .flex_wrap()
        .child(sort)
        .child(toggle)
}

/// Labs › Downloaded models only's row under the header: what the
/// downloaded models take on disk and the toggle. It saves nothing.
fn downloaded_row(ui: &Ui, p: Paint, language: &str) -> El {
    let total = downloaded_total(
        &transcription_models::picker_models(language),
        |model| is_downloaded(ui, model, language),
        owned_by_hex,
    );
    let label = tr("Only downloaded models");
    let toggle = pressable_row(
        row()
            .key("picker-downloaded")
            .label(label)
            .gap(8.0)
            .center_y()
            .shrink()
            .child(
                text(label)
                    .key("label")
                    .font(p.k.ty.small)
                    .color(p.text())
                    .shrink()
                    .wrap()
                    .a11y_hidden(),
            )
            .child(row_switch(p, ui.picker.downloaded_only)),
        ui.picker.downloaded_only,
    )
    .on_press(|ui: &mut Ui| ui.picker.downloaded_only = !ui.picker.downloaded_only);
    row()
        .key("picker-downloaded-controls")
        .px(16.0)
        .pb(16.0)
        .gaps(8.0, 12.0)
        .center_y()
        .justify_between()
        .flex_wrap()
        .child(
            text(tr_fill("Downloaded models take {}", &bytes_label(total)))
                .key("downloaded-total")
                .font(facts::tnum(p.k.ty.small))
                .color(p.muted()),
        )
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
    let names = [("model", model.name), ("state", state.as_str())];
    let copy = if downloaded_filter(ui) && !is_downloaded(ui, model, language) {
        tr_named(
            "{model} is hidden by the downloaded filter: {state}",
            &names,
        )
    } else {
        tr_named("{model} is hidden by the language filter: {state}", &names)
    };
    let mut note = row()
        .key("picker-hidden")
        .px(16.0)
        .pt(8.0)
        .gap(8.0)
        .center_y()
        .child(
            text(copy)
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

/// Parakeet Ultra's card first with its details and benchmark table open
/// (Maltese saved, sorted by name, only models for Maltese). It lists Ultra
/// only where Labs › Parakeet Ultra is on in the saved settings.
pub(super) fn parakeet_ultra_fixture(ui: &mut Ui) {
    ui.settings.labs.parakeet_ultra = true;
    ui.settings.labs.picker_sort_filter = true;
    ui.settings.transcription = TranscriptionSelection {
        model: TranscriptionModelId::ParakeetV3,
        language: "mt".into(),
    };
    ui.picker.open = true;
    ui.picker.view = View::All;
    ui.picker.sort = Sort::Name;
    ui.picker.supported_only = true;
    ui.picker.details =
        super::model_details::DetailsState::open_with_table(TranscriptionModelId::ParakeetUltra);
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

/// The full list with English and Parakeet saved.
pub(super) fn all_fixture(ui: &mut Ui) {
    fixture(ui, None);
    ui.picker.view = View::All;
}

/// The picker with Japanese saved and Whisper in use; `downloading` shows
/// a model mid-download.
pub(super) fn japanese_fixture(ui: &mut Ui, downloading: Option<TranscriptionModelId>) {
    ui.settings.transcription = TranscriptionSelection {
        model: TranscriptionModelId::WhisperLargeV3Turbo,
        language: "ja".into(),
    };
    ui.picker.installed = Some(vec![
        TranscriptionModelId::WhisperLargeV3Turbo,
        TranscriptionModelId::ParakeetUnifiedEnglish,
    ]);
    fixture(ui, downloading);
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

    fn ui_with_active_first(on: bool) -> Ui {
        let mut settings = AppSettings::default();
        settings.labs.picker_active_first = on;
        let mut ui = Ui::with(settings, super::super::Service::Fixture(Vec::new()));
        ui.picker.installed = Some(vec![ui.settings.transcription.model]);
        ui
    }

    #[test]
    fn the_model_in_use_is_fixed_while_the_picker_is_open() {
        let mut ui = ui_with_active_first(true);
        let saved = ui.settings.transcription.model;
        open(&mut ui);
        assert_eq!(pinned(&ui), Some(saved));
        assert_eq!(list_first(&ui), Some(saved));
        // Another model becomes the saved one while it is open: no change.
        ui.settings.transcription.model = TranscriptionModelId::WhisperLargeV3Turbo;
        assert_eq!(pinned(&ui), Some(saved));
        // By size and by name keep their exact order.
        ui.settings.labs.picker_sort_filter = true;
        ui.picker.sort = Sort::Size;
        assert_eq!(list_first(&ui), None);
        assert_eq!(pinned(&ui), Some(saved));
        ui.picker.sort = Sort::Name;
        assert_eq!(list_first(&ui), None);
        ui.picker.sort = Sort::Recommended;
        assert_eq!(list_first(&ui), Some(saved));
        close(&mut ui);
        assert_eq!(pinned(&ui), None);
    }

    #[test]
    fn without_model_in_use_first_nothing_is_pinned() {
        let mut ui = ui_with_active_first(false);
        open(&mut ui);
        assert_eq!(pinned(&ui), None);
        assert_eq!(list_first(&ui), None);
        // A model that isn't installed is never pinned.
        let mut ui = ui_with_active_first(true);
        ui.picker.installed = Some(Vec::new());
        open(&mut ui);
        assert_eq!(pinned(&ui), None);
    }

    fn ui_with_downloaded(on: bool) -> Ui {
        let mut settings = AppSettings::default();
        settings.labs.picker_downloaded = on;
        let mut ui = Ui::with(settings, super::super::Service::Fixture(Vec::new()));
        ui.picker.installed = Some(vec![
            TranscriptionModelId::ParakeetUnifiedEnglish,
            TranscriptionModelId::WhisperLargeV3Turbo,
        ]);
        ui
    }

    #[test]
    fn only_downloaded_models_keeps_the_installed_ones() {
        let mut ui = ui_with_downloaded(true);
        open(&mut ui);
        assert!(!ui.picker.downloaded_only, "the switch starts off");
        let all = shown_models(&ui, "en");
        ui.picker.downloaded_only = true;
        let shown = shown_models(&ui, "en");
        assert_eq!(
            ids(&shown),
            ids(&all)
                .into_iter()
                .filter(|id| matches!(
                    id,
                    TranscriptionModelId::ParakeetUnifiedEnglish
                        | TranscriptionModelId::WhisperLargeV3Turbo
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(shown.len(), 2);
        // A model being downloaded is named while the switch hides it.
        let cohere = TranscriptionModelId::CohereTranscribe;
        assert_eq!(
            hidden_model(&shown, Some(cohere), None),
            Some((cohere, true))
        );
        close(&mut ui);
        open(&mut ui);
        assert!(!ui.picker.downloaded_only);
    }

    #[test]
    fn the_downloaded_total_adds_up_hexs_own_files() {
        let models = transcription_models::picker_models("en");
        let installed = [
            TranscriptionModelId::ParakeetUnifiedEnglish,
            TranscriptionModelId::WhisperLargeV3Turbo,
        ];
        let expected: u64 = installed
            .iter()
            .filter_map(|id| transcription_models::definition(*id).download_bytes())
            .sum();
        assert!(expected > 0);
        let downloaded = |model: &ModelDefinition| installed.contains(&model.id);
        assert_eq!(downloaded_total(&models, downloaded, |_| true), expected);
        // A file another app keeps isn't counted.
        let whisper = transcription_models::definition(TranscriptionModelId::WhisperLargeV3Turbo)
            .download_bytes()
            .unwrap();
        assert_eq!(
            downloaded_total(&models, downloaded, |model| {
                model.id != TranscriptionModelId::WhisperLargeV3Turbo
            }),
            expected - whisper
        );
        assert_eq!(bytes_label(2_400_000_000), "2.4 GB");
        assert_eq!(bytes_label(640_000_000), "640 MB");
    }

    #[test]
    fn without_downloaded_models_only_the_picker_is_unchanged() {
        let mut ui = ui_with_downloaded(false);
        open(&mut ui);
        let all = shown_models(&ui, "en");
        // Even a stale switch does nothing while the Lab is off.
        ui.picker.downloaded_only = true;
        assert!(!downloaded_filter(&ui));
        assert_eq!(ids(&shown_models(&ui, "en")), ids(&all));
        assert_eq!(ids(&all), ids(&transcription_models::picker_models("en")));
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

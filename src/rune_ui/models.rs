//! The OpenCode model catalog and the model and thinking pickers that Voice
//! Action and each mode's rewriting share: the catalog loads off the UI
//! thread, a missing `opencode2` is retried at a coarse interval, failures
//! wait for an explicit retry, and choosing a model also picks its
//! recommended thinking variant.

use std::sync::mpsc::Receiver;
use std::time::Instant;

use rune::{El, TextEdit, Tx, col, row, text};
use rune_kit::{Badge, Placement, Popover, Select, SelectAppearance, TextInput};

use super::controls::{
    ButtonKind, DESCRIPTION, Highlight, InstantHover, Paint, button, setting_row,
};
use super::modes::Selection;
use super::{Menu, Ui};
use crate::app_settings::AppSettings;
use crate::desktop_i18n::{tr, tr_fill};
use crate::dictation_processor::{ModelCatalog, ModelChoice};
use crate::opencode_catalog::{
    ModelCatalogState, OPENCODE_INSTALL_RETRY_INTERVAL, OpenCodeFeature, filtered_model_keys,
    model_presentation, model_variants, open_opencode_beta_docs, opencode_unavailable_copy,
    start_model_catalog_load,
};

const MODEL_FIELD_WIDTH: f32 = 240.0;

/// Which setting a picker edits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Target {
    VoiceAction,
    Mode(Selection),
}

pub(super) struct Models {
    state: ModelCatalogState,
    receiver: Option<Receiver<ModelCatalogState>>,
    retry_at: Instant,
    /// The open model popover's search field.
    search: TextEdit,
    highlight: Highlight,
}

impl Default for Models {
    fn default() -> Self {
        Self {
            state: ModelCatalogState::Loading,
            receiver: None,
            retry_at: Instant::now(),
            search: TextEdit::default(),
            highlight: Highlight::default(),
        }
    }
}

impl Models {
    pub(super) fn load() -> Self {
        let mut models = Self::default();
        models.start_load();
        models
    }

    fn start_load(&mut self) {
        self.retry_at = Instant::now() + OPENCODE_INSTALL_RETRY_INTERVAL;
        self.receiver = Some(start_model_catalog_load());
    }

    fn query(&self) -> String {
        self.search.text.trim().to_lowercase()
    }

    /// Adopts a finished load; retries a missing `opencode2` now and then.
    pub(super) fn poll(&mut self, live: bool) -> bool {
        if let Some(receiver) = &self.receiver {
            let Ok(state) = receiver.try_recv() else {
                return false;
            };
            self.state = state;
            self.receiver = None;
            return true;
        }
        if live
            && matches!(self.state, ModelCatalogState::Missing)
            && Instant::now() >= self.retry_at
        {
            self.start_load();
        }
        false
    }

    pub(super) fn reload(&mut self) {
        if self.receiver.is_some() {
            return;
        }
        self.state = ModelCatalogState::Loading;
        self.start_load();
    }

    pub(super) fn available(&self) -> bool {
        matches!(self.state, ModelCatalogState::Loaded(_))
    }

    fn catalog(&self) -> Option<&ModelCatalog> {
        match &self.state {
            ModelCatalogState::Loaded(catalog) => Some(catalog),
            _ => None,
        }
    }

    pub(super) fn fixture() -> Self {
        let choice = |key: &str, name: &str, provider: &str, variants: &[&str]| ModelChoice {
            key: key.into(),
            name: name.into(),
            provider: provider.into(),
            variants: variants.iter().map(|v| v.to_string()).collect(),
        };
        Self {
            state: ModelCatalogState::Loaded(ModelCatalog {
                models: vec![
                    choice(
                        "anthropic/claude-sonnet-4-5",
                        "Claude Sonnet 4.5",
                        "anthropic",
                        &["high", "max"],
                    ),
                    choice(
                        "anthropic/claude-haiku-4-5",
                        "Claude Haiku 4.5",
                        "anthropic",
                        &[],
                    ),
                    choice(
                        "openai/gpt-5",
                        "GPT-5",
                        "openai",
                        &["low", "medium", "high"],
                    ),
                    choice("opencode/grok-code", "Grok Code Fast", "opencode", &[]),
                ],
                default_key: Some("anthropic/claude-haiku-4-5".into()),
                default_name: Some("Claude Haiku 4.5".into()),
                default_variant: None,
            }),
            ..Self::default()
        }
    }

    pub(super) fn fixture_missing() -> Self {
        Self {
            state: ModelCatalogState::Missing,
            ..Self::default()
        }
    }
}

fn selected(settings: &AppSettings, target: Target) -> (Option<&str>, Option<&str>) {
    match target {
        Target::VoiceAction => (
            settings.voice_action.model.as_deref(),
            settings.voice_action.variant.as_deref(),
        ),
        Target::Mode(selection) => {
            let processing = &super::modes::mode(settings, selection).post_processing;
            (processing.model.as_deref(), processing.variant.as_deref())
        }
    }
}

fn set_model(
    settings: &mut AppSettings,
    target: Target,
    model: Option<String>,
    variant: Option<String>,
) {
    match target {
        Target::VoiceAction => {
            settings.voice_action.model = model;
            settings.voice_action.variant = variant;
        }
        Target::Mode(selection) => {
            let processing = &mut super::modes::mode_mut(settings, selection).post_processing;
            processing.model = model;
            processing.variant = variant;
        }
    }
}

fn choose_model(ui: &mut Ui, target: Target, model: Option<String>) {
    ui.open = None;
    let variant = model.as_deref().and_then(|key| {
        ui.models
            .catalog()
            .and_then(|catalog| catalog.recommended_variant_for(key))
            .map(str::to_owned)
    });
    ui.update(|settings| set_model(settings, target, model, variant));
}

fn choose_variant(ui: &mut Ui, target: Target, variant: Option<String>) {
    ui.open = None;
    let default_key = ui
        .models
        .catalog()
        .and_then(|catalog| catalog.default_key.clone());
    ui.update(|settings| {
        let (model, _) = selected(settings, target);
        // Variants belong to an explicit catalog model, not the floating default.
        let model = match model {
            Some(model) => Some(model.to_owned()),
            None if variant.is_some() => default_key,
            None => None,
        };
        set_model(settings, target, model, variant);
    });
}

fn open_model_picker(ui: &mut Ui) {
    if ui.open == Some(Menu::Model) {
        ui.open = None;
        return;
    }
    ui.models.search = TextEdit::default();
    ui.models.search.focused = true;
    ui.models.highlight = Highlight::default();
    ui.open = Some(Menu::Model);
}

/// Up and Down in the search field move the model popover's highlight.
fn move_highlight(ui: &mut Ui, event: &rune::KeyEvent) -> bool {
    let query = ui.models.query();
    let count = ui
        .models
        .catalog()
        .map_or(0, |catalog| filtered_model_keys(catalog, &query).len());
    ui.models.highlight.key(&event.key, &query, count)
}

fn close(ui: &mut Ui) {
    ui.open = None;
}

/// The catalog's status line under the model row's title.
fn status(models: &Models) -> String {
    match &models.state {
        ModelCatalogState::Loading => tr("Loading OpenCode…").into(),
        ModelCatalogState::Missing => tr("OpenCode beta required").into(),
        ModelCatalogState::Failed(error) => tr_fill("OpenCode models unavailable: {}", error),
        ModelCatalogState::Loaded(catalog) => {
            tr_fill("{} available models", &catalog.models.len().to_string())
        }
    }
}

/// The model row (and the thinking row when the model has variants).
pub(super) fn rows(ui: &Ui, p: Paint, target: Target, layer: &mut Vec<El>) -> Vec<El> {
    let (model, variant) = selected(&ui.settings, target);
    let presentation = model_presentation(&ui.models.state, model);
    let c = &p.k.c;
    let mut field = row()
        .key("model-value")
        .w(MODEL_FIELD_WIDTH)
        .h(32.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .radius(p.k.radius.md)
        .fill(c.bg_base)
        .hover(c.hover)
        .instant_hover()
        .press(c.pressed)
        .on_press(|ui: &mut Ui| open_model_picker(ui));
    for shadow in &p.k.elevation.button_neutral {
        field = field.box_shadow(*shadow);
    }
    field = match &presentation {
        Some(presentation) => field
            .child(
                text(presentation.name.clone())
                    .key("name")
                    .font(p.k.ty.ui_medium)
                    .color(p.text())
                    .grow(1.0)
                    .truncate(),
            )
            .child(
                text(presentation.provider.clone())
                    .key("provider")
                    .font(p.k.ty.small)
                    .color(p.muted()),
            )
            .children(
                presentation
                    .is_default
                    .then(|| Badge::new(tr("Default")).build(p.k)),
            ),
        None => field.child(
            text(tr("Search available models"))
                .key("placeholder")
                .font(p.k.ty.ui)
                .color(p.faint())
                .grow(1.0),
        ),
    };
    field = field.child(
        rune::icon(p.k.glyph.chevron_down, 16.0)
            .key("chevron")
            .color(c.icon_muted),
    );
    let picker = Popover::new("model-picker")
        .trigger(field)
        .placement(Placement::BottomEnd)
        .open(ui.open == Some(Menu::Model))
        .on_close(close)
        .child(model_list(ui, p, target))
        .build(p.k, layer);
    let refresh = ui.models.available().then(|| {
        button(
            p,
            "refresh-models",
            tr("Refresh"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| ui.models.reload(),
        )
    });
    let control = row()
        .key("model-control")
        .gap(8.0)
        .center_y()
        .child(picker)
        .children(refresh);
    let description = status(&ui.models);
    let mut rows = vec![setting_row(
        p,
        "opencode-model",
        tr("OpenCode model"),
        Some(&description),
        control,
    )];
    let catalog = ui.models.catalog();
    let variants = catalog
        .map(|catalog| model_variants(catalog, model))
        .unwrap_or_default();
    if !variants.is_empty() {
        let effective = variant.or_else(|| {
            if model.is_none() {
                catalog.and_then(|catalog| catalog.default_variant.as_deref())
            } else {
                None
            }
        });
        let mut options = vec![tr("Default").to_string()];
        options.extend(variants);
        let current = effective.map_or_else(|| tr("Default").to_string(), str::to_owned);
        let select = Select::new("model-variant", &options)
            .appearance(SelectAppearance::Field)
            .width(160.0)
            .current(Some(&current))
            .open(ui.open == Some(Menu::Variant))
            .on_toggle(|ui: &mut Ui| {
                ui.open = (ui.open != Some(Menu::Variant)).then_some(Menu::Variant);
            })
            .on_close(close)
            .on_choose(move |ui: &mut Ui, choice: &str, _: &mut Tx| {
                let variant = (choice != tr("Default")).then(|| choice.to_owned());
                choose_variant(ui, target, variant);
            })
            .build(p.k, layer);
        rows.push(setting_row(
            p,
            "thinking",
            tr("Thinking"),
            Some(tr("Choose how much reasoning the model should use")),
            select,
        ));
    }
    rows
}

/// The open popover: a search field over the matching models.
fn model_list(ui: &Ui, p: Paint, target: Target) -> El {
    let c = &p.k.c;
    let search = TextInput::new(&ui.models.search)
        .placeholder(tr("Search available models"))
        .width(Some(280.0))
        .leading_icon(p.k.glyph.search)
        .on_edit(|ui: &mut Ui| &mut ui.models.search)
        .on_key(|ui: &mut Ui, event: &rune::KeyEvent, _: &mut Tx| move_highlight(ui, event))
        .on_submit(move |ui: &mut Ui, _: &mut Tx| {
            let query = ui.models.query();
            let picked = ui.models.catalog().and_then(|catalog| {
                let keys = filtered_model_keys(catalog, &query);
                let index = ui.models.highlight.index(&query, keys.len())?;
                keys.into_iter().nth(index)
            });
            if let Some(model) = picked {
                choose_model(ui, target, model);
            }
        })
        .build(p.k)
        .instant_hover()
        .key("model-search");
    let mut list = col().key("model-results").w(280.0).mt(8.0);
    let Some(catalog) = ui.models.catalog() else {
        return col().key("model-picker-body").child(search);
    };
    let (current, _) = selected(&ui.settings, target);
    let query = ui.models.query();
    let keys = filtered_model_keys(catalog, &query);
    let highlighted = ui.models.highlight.index(&query, keys.len());
    if keys.is_empty() {
        list = list.child(
            text(tr("No models match"))
                .key("empty")
                .font(DESCRIPTION)
                .color(p.muted())
                .px(8.0)
                .py(6.0),
        );
    }
    for (index, key) in keys.into_iter().enumerate() {
        let (title, detail) = match &key {
            None => (
                tr("Use OpenCode default").to_string(),
                catalog.default_name.clone().unwrap_or_default(),
            ),
            Some(key) => (
                catalog
                    .models
                    .iter()
                    .find(|choice| &choice.key == key)
                    .map_or_else(|| key.clone(), |choice| choice.name.clone()),
                key.clone(),
            ),
        };
        let chosen = key.as_deref() == current;
        let pick = key.clone();
        let mut entry = col()
            .key(format!("model{index}"))
            .role("menuitem")
            .label(title.clone())
            .px(8.0)
            .py(5.0)
            .gap(1.0)
            .radius(p.k.radius.sm)
            .hover(c.hover)
            .instant_hover()
            .press(c.pressed)
            .on_press(move |ui: &mut Ui| choose_model(ui, target, pick.clone()))
            .child(
                text(title)
                    .key("name")
                    .font(p.k.ty.ui_medium)
                    .color(p.text())
                    .truncate(),
            )
            .child(
                text(detail)
                    .key("key")
                    .font(p.k.ty.small)
                    .color(p.muted())
                    .truncate(),
            );
        if chosen {
            entry = entry.fill(c.layer_02);
        } else if highlighted == Some(index) {
            entry = entry.fill(c.hover);
        }
        list = list.child(entry);
    }
    col().key("model-picker-body").child(search).child(list)
}

/// Why OpenCode features are unavailable, with retry and setup actions;
/// `None` once the catalog loads.
pub(super) fn unavailable(ui: &Ui, p: Paint, feature: OpenCodeFeature) -> Option<El> {
    let copy = opencode_unavailable_copy(&ui.models.state, feature)?;
    let mut actions = row().key("actions").gap(8.0);
    if copy.can_retry {
        actions = actions.child(button(
            p,
            "retry-opencode",
            copy.retry_label,
            ButtonKind::Secondary,
            |ui: &mut Ui, _| ui.models.reload(),
        ));
    }
    if copy.can_open_setup {
        actions = actions.child(button(
            p,
            "open-opencode-setup",
            tr("Open OpenCode setup"),
            ButtonKind::Secondary,
            |_: &mut Ui, _| open_opencode_beta_docs(),
        ));
    }
    let description = copy
        .error
        .clone()
        .unwrap_or_else(|| copy.description.to_string());
    let color = if copy.error.is_some() {
        p.danger()
    } else {
        p.muted()
    };
    Some(
        super::controls::described_row(
            p,
            "opencode-unavailable",
            copy.title,
            Some((description, color)),
            actions,
        )
        .min_h(64.0),
    )
}

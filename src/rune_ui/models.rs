//! The OpenCode model catalog and the model and thinking pickers that each
//! mode's rewriting uses: the catalog loads off the UI thread only while
//! the Modes pane shows, a stopped OpenCode is looked for again at a coarse
//! interval, failures wait for an explicit retry, and choosing a model also
//! picks its recommended thinking variant.

use std::sync::mpsc::Receiver;
use std::time::Instant;

use rune::{El, row, text};
use rune_kit::{
    Badge, Combobox, ComboboxOption, ComboboxState, Placement, Select, SelectAppearance,
};

use super::controls::{ButtonKind, Paint, button, setting_row};
use super::modes::Selection;
use super::{Menu, Ui};
use crate::app_settings::AppSettings;
use crate::desktop_i18n::{tr, tr_fill};
use crate::dictation::processor::{ModelCatalog, ModelChoice};
use crate::opencode_catalog::{
    ModelCatalogState, OPENCODE_RETRY_INTERVAL, OpenCodeFeature, model_presentation,
    model_variants, open_opencode_docs, opencode_unavailable_copy, provider_label,
    start_model_catalog_load,
};

const MODEL_FIELD_WIDTH: f32 = 240.0;

/// Which setting a picker edits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Target {
    Mode(Selection),
}

pub(super) struct Models {
    state: ModelCatalogState,
    receiver: Option<Receiver<ModelCatalogState>>,
    /// When to look for OpenCode next; `None` until a pane first wants it.
    retry_at: Option<Instant>,
    /// The model popover: open, its search, and its highlight.
    pub(super) picker: ComboboxState,
}

impl Default for Models {
    fn default() -> Self {
        Self {
            state: ModelCatalogState::Loading,
            receiver: None,
            retry_at: None,
            picker: ComboboxState::default(),
        }
    }
}

impl Models {
    fn start_load(&mut self) {
        self.retry_at = Some(Instant::now() + OPENCODE_RETRY_INTERVAL);
        self.receiver = Some(start_model_catalog_load());
    }

    /// Opens the model popover with its search focused, as pressing the
    /// field does.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn open_picker(&mut self) {
        self.picker.open = true;
        self.picker.search.focused = true;
    }

    /// Adopts a finished load. While `wanted` (a pane that uses the catalog
    /// shows), loads it the first time and looks for a stopped OpenCode
    /// now and then; otherwise touches nothing.
    pub(super) fn poll(&mut self, wanted: bool) -> bool {
        if let Some(receiver) = &self.receiver {
            let Ok(state) = receiver.try_recv() else {
                return false;
            };
            self.state = state;
            self.receiver = None;
            return true;
        }
        let due = match self.retry_at {
            None => true,
            Some(at) => matches!(self.state, ModelCatalogState::NotRunning) && Instant::now() >= at,
        };
        if wanted && due {
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

    /// [`Models::fixture`] with as many models as OpenCode lists with
    /// several providers signed in, more than fit in the window.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn fixture_long() -> Self {
        let mut models = Self::fixture();
        if let ModelCatalogState::Loaded(catalog) = &mut models.state {
            for (provider, family) in [
                ("anthropic", "Claude"),
                ("openai", "GPT"),
                ("google", "Gemini"),
            ] {
                for version in 1..=16 {
                    catalog.models.push(ModelChoice {
                        key: format!("{provider}/{}-{version}", family.to_lowercase()),
                        name: format!("{family} {version}"),
                        provider: provider.into(),
                        variants: Vec::new(),
                    });
                }
            }
        }
        models
    }

    pub(super) fn fixture_not_running() -> Self {
        Self {
            state: ModelCatalogState::NotRunning,
            retry_at: Some(Instant::now() + OPENCODE_RETRY_INTERVAL),
            ..Self::default()
        }
    }
}

fn selected(settings: &AppSettings, target: Target) -> (Option<&str>, Option<&str>) {
    let Target::Mode(selection) = target;
    let processing = &super::modes::mode(settings, selection).post_processing;
    (processing.model.as_deref(), processing.variant.as_deref())
}

fn set_model(
    settings: &mut AppSettings,
    target: Target,
    model: Option<String>,
    variant: Option<String>,
) {
    let Target::Mode(selection) = target;
    let processing = &mut super::modes::mode_mut(settings, selection).post_processing;
    processing.model = model;
    processing.variant = variant;
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

/// Closes the model popover, as leaving the pane or window does.
pub(super) fn close(ui: &mut Ui) {
    ui.models.picker.close();
}

fn close_menu(ui: &mut Ui) {
    ui.open = None;
}

/// The catalog's status line under the model row's title.
fn status(models: &Models) -> String {
    match &models.state {
        ModelCatalogState::Loading => tr("Loading OpenCode…").into(),
        ModelCatalogState::NotRunning => tr("OpenCode isn't running").into(),
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
        .press(c.pressed);
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
    let picker = model_picker(ui, p, target, field, layer);
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
            .on_close(close_menu)
            .on_choose(move |ui: &mut Ui, choice: &str, _: &mut rune::Tx| {
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

/// The model field's popover: a search over every model, with the OpenCode
/// default first. Options are keyed by their place in the catalog, since
/// model keys contain slashes.
fn model_picker(ui: &Ui, p: Paint, target: Target, field: El, layer: &mut Vec<El>) -> El {
    let catalog = ui.models.catalog();
    let mut keys = Vec::new();
    let mut options = Vec::new();
    if let Some(catalog) = catalog {
        if let Some(name) = &catalog.default_name {
            keys.push(None);
            let mut option = ComboboxOption::new("0", tr("Use OpenCode default"));
            option.detail = name.clone();
            options.push(option);
        }
        for choice in &catalog.models {
            let mut option = ComboboxOption::new(keys.len().to_string(), choice.name.clone());
            option.detail = provider_label(&choice.provider);
            options.push(option);
            keys.push(Some(choice.key.clone()));
        }
    }
    let (current, _) = selected(&ui.settings, target);
    let current = keys
        .iter()
        .position(|key| key.as_deref() == current)
        .map(|index| index.to_string());
    Combobox::new(
        "model-picker",
        &ui.models.picker,
        &options,
        |ui: &mut Ui| &mut ui.models.picker,
    )
    .trigger(field)
    .placement(Placement::BottomEnd)
    .width(320.0)
    .max_height(400.0)
    .current(current.as_deref())
    .search_placeholder(tr("Search available models"))
    .empty_message(tr("No models match"))
    .on_choose(move |ui: &mut Ui, id: &str, _| {
        if let Some(key) = id.parse::<usize>().ok().and_then(|index| keys.get(index)) {
            choose_model(ui, target, key.clone());
        }
    })
    .build(p.k, layer)
}

/// Why `feature` is unavailable in a few words, for beside its switch;
/// `None` once the catalog loads.
pub(super) fn unavailable_reason(ui: &Ui, feature: OpenCodeFeature) -> Option<&'static str> {
    opencode_unavailable_copy(&ui.models.state, feature).map(|copy| copy.reason)
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
            |_: &mut Ui, _| open_opencode_docs(),
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
        .min_h(super::controls::METRICS.row_h_described),
    )
}

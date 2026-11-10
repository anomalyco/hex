//! The speech model and language a person can switch to: an input source's
//! mapping in General, and a dictation mode's choice in Modes. Both show the
//! picker's installed GGUF models plus Don't switch, then a language the
//! model supports.

use rune::{El, row};

use super::controls::{Choice, Paint, fixed_select, select, setting_row};
use super::settings::toggle_menu;
use super::{Menu, Ui};
use crate::app_settings::KeyboardModelChoice;
use crate::desktop::i18n::tr;
use crate::transcription::models::{self as transcription_models, ModelDefinition, ModelRuntime};

/// Whether the Modes pane offers a speech choice per mode: Labs › Switch
/// model with app is on where the runtime can follow the frontmost app.
pub(super) fn enabled(ui: &Ui) -> bool {
    ui.platform.caps.app_model_switching && ui.settings.labs.app_model_switching
}

/// One row's keys and copy.
pub(super) struct SpeechRow<'a> {
    /// The row's key.
    pub(super) key: &'a str,
    /// The model select's key.
    pub(super) model_key: &'a str,
    /// The language select's key.
    pub(super) language_key: &'a str,
    pub(super) title: &'a str,
    pub(super) description: Option<&'a str>,
    /// The saved choice; None shows Don't switch.
    pub(super) choice: Option<KeyboardModelChoice>,
    pub(super) model_menu: Menu,
    pub(super) language_menu: Menu,
}

/// A setting row choosing Don't switch or an installed GGUF model, then
/// the model's language. `language_for` picks the language a new choice starts
/// on, given the model and its last-used language; `save` writes the choice
/// or clears it with None.
pub(super) fn setting(
    ui: &Ui,
    p: Paint,
    keys: SpeechRow<'_>,
    language_for: impl Fn(&ModelDefinition, &str) -> String,
    save: impl Fn(&mut Ui, Option<KeyboardModelChoice>) + Clone + 'static,
    layer: &mut Vec<El>,
) -> El {
    let SpeechRow {
        key,
        model_key,
        language_key,
        title,
        description,
        choice,
        model_menu,
        language_menu,
    } = keys;
    let mut choices = vec![Choice {
        label: tr("Don't switch").into(),
        detail: None,
        selected: choice.is_none(),
        choose: {
            let save = save.clone();
            Box::new(move |ui| {
                ui.open = None;
                save(ui, None);
            })
        },
    }];
    for model in transcription_models::all_models() {
        let fallback = ui.settings.transcription_for_model(model.id).language;
        if !model.available()
            || !matches!(model.runtime, ModelRuntime::Gguf(_))
            || !ui.picker.installed(model, &fallback)
        {
            continue;
        }
        let language = match &choice {
            Some(choice) if model.supports_language(&choice.language) => choice.language.clone(),
            _ => language_for(model, &fallback),
        };
        let id = model.id;
        let save = save.clone();
        choices.push(Choice {
            label: model.name.into(),
            detail: None,
            selected: choice.as_ref().is_some_and(|choice| choice.model == id),
            choose: Box::new(move |ui| {
                ui.open = None;
                save(
                    ui,
                    Some(KeyboardModelChoice {
                        model: id,
                        language: language.clone(),
                    }),
                );
            }),
        });
    }
    let label = choice.as_ref().map_or_else(
        || tr("Don't switch").to_string(),
        |choice| {
            let model = transcription_models::definition(choice.model);
            if ui.picker.installed(model, &choice.language) {
                model.name.into()
            } else {
                format!("{} · {}", model.name, tr("Not installed"))
            }
        },
    );
    let mut controls = row().gap(8.0).center_y();
    if let Some(choice) = &choice {
        controls = controls.child(language(
            ui,
            p,
            language_key,
            choice,
            language_menu,
            save.clone(),
            layer,
        ));
    }
    setting_row(
        p,
        key,
        title,
        description,
        controls.child(select(
            p,
            model_key,
            tr("Speech model"),
            &label,
            ui.open == Some(model_menu),
            choices,
            move |ui| toggle_menu(ui, model_menu),
            layer,
        )),
    )
}

/// A chosen model's language: a select, or fixed for one-language models.
fn language(
    ui: &Ui,
    p: Paint,
    key: &str,
    choice: &KeyboardModelChoice,
    menu: Menu,
    save: impl Fn(&mut Ui, Option<KeyboardModelChoice>) + Clone + 'static,
    layer: &mut Vec<El>,
) -> El {
    let current = transcription_models::language_name(&choice.language);
    let codes = transcription_models::definition(choice.model).language_choices();
    if codes.len() <= 1 {
        return fixed_select(p, key, tr("Language"), current);
    }
    let choices = codes
        .into_iter()
        .map(|code| {
            let save = save.clone();
            let model = choice.model;
            Choice {
                label: transcription_models::language_name(code).to_owned(),
                detail: None,
                selected: code == choice.language,
                choose: Box::new(move |ui| {
                    ui.open = None;
                    save(
                        ui,
                        Some(KeyboardModelChoice {
                            model,
                            language: code.to_owned(),
                        }),
                    );
                }),
            }
        })
        .collect();
    select(
        p,
        key,
        tr("Language"),
        current,
        ui.open == Some(menu),
        choices,
        move |ui| toggle_menu(ui, menu),
        layer,
    )
}

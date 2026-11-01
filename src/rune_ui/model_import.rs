//! Labs › Use your own speech model in the Local transcription picker: "Add
//! model file…" opens the native dialog for a GGUF file, which is checked
//! and copied on a worker (`custom_models::import`) while the picker stays
//! responsive. Imported cards say what the file declared, can be renamed,
//! and can be removed; one that is missing, changed, or behind its toggle
//! says so instead of offering Use.

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use rune::{El, TextEdit, Waker, edit_access, row, text};
use rune_kit::TextInput;

use super::Ui;
use super::controls::{ButtonKind, Paint, button};
use crate::desktop::file_dialog::{Dialog, Filter, Pending, Target};
use crate::desktop::i18n::{tr, tr_named};
use crate::transcription::custom_models::{self, Status};
use crate::transcription::models::{CustomModelId, ModelDefinition, TranscriptionModelId};

#[derive(Default)]
pub(super) struct ImportState {
    chooser: Option<Pending>,
    worker: Option<Receiver<Result<CustomModelId, String>>>,
    pub(super) error: Option<String>,
    /// The import being renamed, with its name field.
    renaming: Option<(CustomModelId, TextEdit)>,
}

impl ImportState {
    fn busy(&self) -> bool {
        self.chooser.is_some() || self.worker.is_some()
    }
}

/// Whether Labs › Use your own speech model is on.
pub(super) fn on(ui: &Ui) -> bool {
    let feature = crate::labs::Feature::CustomModels;
    feature.shipped() && feature.on(&ui.settings)
}

fn wake(waker: Option<Waker>) -> impl Fn() + Send + 'static {
    move || {
        if let Some(waker) = &waker {
            waker();
        }
    }
}

/// The header's "Add model file…" button while the Lab is on.
pub(super) fn add_button(ui: &Ui, p: Paint) -> Option<El> {
    on(ui).then(|| {
        button(
            p,
            "add-model-file",
            if ui.picker.import.busy() {
                tr("Adding model…")
            } else {
                tr("Add model file…")
            },
            ButtonKind::Secondary,
            |ui: &mut Ui, _| choose(ui),
        )
    })
}

fn choose(ui: &mut Ui) {
    if !ui.live() || ui.picker.import.busy() {
        return;
    }
    ui.picker.import.error = None;
    ui.picker.import.chooser = Some(crate::desktop::file_dialog::open(
        Dialog {
            title: tr("Add model file").into(),
            target: Target::File(vec![
                Filter {
                    label: tr("GGUF speech models").into(),
                    extensions: &["gguf"],
                },
                Filter {
                    label: tr("All files").into(),
                    extensions: &["*"],
                },
            ]),
            directory: dirs::download_dir(),
        },
        wake(ui.modes.waker.clone()),
    ));
}

/// Takes the dialog's answer and the worker's result. True when either
/// changed what the picker shows.
pub(super) fn poll(ui: &mut Ui) -> bool {
    let mut changed = false;
    if let Some(choice) = ui.picker.import.chooser.as_ref().and_then(Pending::poll) {
        ui.picker.import.chooser = None;
        changed = true;
        match choice {
            Ok(Some(path)) => {
                let (sender, receiver) = channel();
                let wake = wake(ui.modes.waker.clone());
                let started = std::thread::Builder::new()
                    .name("hex-model-import".into())
                    .spawn(move || {
                        let result = custom_models::import(&path, None, &AtomicBool::new(false))
                            .map_err(|error| format!("{error:#}"));
                        let _ = sender.send(result);
                        wake();
                    });
                match started {
                    Ok(_) => ui.picker.import.worker = Some(receiver),
                    Err(error) => ui.picker.import.error = Some(error.to_string()),
                }
            }
            Ok(None) => {}
            Err(error) => ui.picker.import.error = Some(error),
        }
    }
    let result = match ui.picker.import.worker.as_ref().map(Receiver::try_recv) {
        Some(Ok(result)) => Some(result),
        Some(Err(TryRecvError::Disconnected)) => Some(Err(tr("The import stopped.").into())),
        Some(Err(TryRecvError::Empty)) | None => None,
    };
    if let Some(result) = result {
        ui.picker.import.worker = None;
        changed = true;
        match result {
            Ok(id) => tracing::info!(id = id.as_str(), "imported a speech model from the picker"),
            Err(error) => ui.picker.import.error = Some(error),
        }
    }
    changed
}

/// Why an import can't be used now, for its card and General's model row.
pub(super) fn status_note(model: &ModelDefinition) -> Option<String> {
    let TranscriptionModelId::Custom(id) = model.id else {
        return None;
    };
    match custom_models::status(id) {
        Status::Ready => None,
        Status::Missing => Some(tr("Its file is missing. Import it again, or remove it.").into()),
        Status::Changed => Some(
            tr("Its file changed since it was imported. Import it again, or remove it.").into(),
        ),
        Status::Off => Some(tr("Turn on Labs › Use your own speech model to use it.").into()),
    }
}

/// General's notice when the saved model is an import that can't run: Hex
/// keeps the choice and runs the default model until it is fixed.
pub(super) fn selection_notice(ui: &Ui) -> Option<String> {
    let model = crate::transcription::models::definition(ui.settings.transcription.model);
    let TranscriptionModelId::Custom(id) = model.id else {
        return None;
    };
    let fallback = crate::transcription::models::definition(custom_models::fallback().model).name;
    let name = if custom_models::is_registered(id) {
        model.name
    } else {
        tr("The imported model")
    };
    let notice = match custom_models::status(id) {
        Status::Ready => return None,
        Status::Missing => {
            "{model} is missing, so Hex uses {fallback} until you import it again or choose another model"
        }
        Status::Changed => {
            "{model} changed since it was imported, so Hex uses {fallback} until you import it again or choose another model"
        }
        Status::Off => {
            "{model} needs Labs › Use your own speech model, so Hex uses {fallback} until you turn it on or choose another model"
        }
    };
    Some(tr_named(notice, &[("model", name), ("fallback", fallback)]))
}

/// What an imported card's metadata line says.
pub(super) fn metadata(model: &ModelDefinition) -> String {
    let import = custom_models::imported()
        .into_iter()
        .find(|import| import.definition.id == model.id);
    let languages = match model.languages {
        [only] => crate::transcription::models::language_name(only).to_owned(),
        languages => crate::desktop::i18n::tr_fill("{} languages", &languages.len().to_string()),
    };
    let mut parts = vec![
        tr("Imported").to_owned(),
        custom_models::family(import.as_ref().map_or("", |import| import.architecture)).to_owned(),
        languages,
        tr("Not benchmarked").to_owned(),
    ];
    if let Some(import) = import {
        if let Some(license) = import.license {
            parts.push(license);
        }
        parts.push(format!("SHA-256 {}…", &import.sha256[..12]));
    }
    parts.join(" · ")
}

/// Rename and Remove for an imported card, or its name field while renaming.
pub(super) fn actions(ui: &Ui, p: Paint, model: &'static ModelDefinition, removable: bool) -> Vec<El> {
    let TranscriptionModelId::Custom(id) = model.id else {
        return Vec::new();
    };
    if let Some((renaming, field)) = &ui.picker.import.renaming
        && *renaming == id
    {
        return vec![
            TextInput::new(field)
                .placeholder(tr("Name"))
                .width(Some(200.0))
                .on_edit(edit_access(move |ui: &mut Ui| {
                    &mut ui
                        .picker
                        .import
                        .renaming
                        .get_or_insert_with(|| (id, TextEdit::default()))
                        .1
                }))
                .on_submit(move |ui: &mut Ui, _| save_name(ui))
                .build(p.k)
                .key("rename-field"),
            button(p, "save-name", tr("Save"), ButtonKind::Secondary, |ui: &mut Ui, _| save_name(ui)),
        ];
    }
    let mut actions = vec![button(
        p,
        "rename-model",
        tr("Rename"),
        ButtonKind::Secondary,
        move |ui: &mut Ui, _| {
            ui.picker.import.renaming = Some((id, TextEdit::new(model.name)));
        },
    )];
    if removable {
        actions.push(button(
            p,
            "remove-model",
            tr("Remove"),
            ButtonKind::Secondary,
            move |ui: &mut Ui, _| remove(ui, id),
        ));
    }
    actions
}

fn save_name(ui: &mut Ui) {
    let Some((id, field)) = ui.picker.import.renaming.take() else {
        return;
    };
    if !ui.live() {
        return;
    }
    if let Err(error) = custom_models::rename(id, &field.text) {
        ui.picker.import.error = Some(format!("{error:#}"));
    }
}

fn remove(ui: &mut Ui, id: CustomModelId) {
    if !ui.live() {
        return;
    }
    if let Err(error) = custom_models::remove(id) {
        ui.picker.import.error = Some(format!("{error:#}"));
    }
}

/// The note under an imported card that can't be used, in the warning colour.
pub(super) fn note(p: Paint, model: &ModelDefinition) -> Option<El> {
    status_note(model).map(|note| {
        row().key("import-status").child(
            text(note)
                .key("label")
                .font(p.k.ty.small)
                .color(p.warning())
                .wrap(),
        )
    })
}

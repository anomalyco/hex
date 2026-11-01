//! Models found on this computer, in the window: the hub-cache scan the
//! picker and onboarding's model step start when they open (never in the
//! background), and choosing a found file that isn't a catalog model, which
//! hashes it once on a worker and registers it in place before preparing it.

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use rune::Waker;

use super::{Request, Ui};
use crate::desktop::onboarding::{Input, SetupItem};
use crate::transcription::custom_models;
use crate::transcription::found_models::{self, Found};
use crate::transcription::models::{CustomModelId, TranscriptionModelId, TranscriptionSelection};

#[derive(Default)]
pub(super) struct FoundState {
    scan: Option<Receiver<Vec<Found>>>,
    /// Hashing and registering a chosen file; true when onboarding chose it.
    adopting: Option<(Receiver<Result<CustomModelId, String>>, bool)>,
    pub(super) error: Option<String>,
    /// Whether this window has scanned since it opened the picker or step.
    scanned: bool,
}

impl FoundState {
    pub(super) fn adopting(&self) -> bool {
        self.adopting.is_some()
    }

    pub(super) fn scanning(&self) -> bool {
        self.scan.is_some()
    }
}

fn wake(waker: Option<Waker>) -> impl Fn() + Send + 'static {
    move || {
        if let Some(waker) = &waker {
            waker();
        }
    }
}

/// Starts a scan when the picker or onboarding's model step opens. Headers
/// only, and an unchanged file is answered from the cache.
pub(super) fn scan(ui: &mut Ui) {
    if !ui.live() || ui.found.scan.is_some() {
        return;
    }
    ui.found.scanned = true;
    let (sender, receiver) = channel();
    let wake = wake(ui.modes.waker.clone());
    let started = std::thread::Builder::new()
        .name("hex-found-models".into())
        .spawn(move || {
            let _ = sender.send(found_models::scan());
            wake();
        });
    if started.is_ok() {
        ui.found.scan = Some(receiver);
    }
}

/// Starts a scan the first time onboarding shows.
pub(super) fn scan_for_setup(ui: &mut Ui) {
    if !ui.found.scanned {
        scan(ui);
    }
}

/// The scan's and the chosen file's results; true when they changed what shows.
pub(super) fn poll(ui: &mut Ui) -> bool {
    let mut changed = false;
    match ui.found.scan.as_ref().map(Receiver::try_recv) {
        Some(Ok(found)) => {
            ui.found.scan = None;
            ui.found_models = found;
            changed = true;
        }
        Some(Err(TryRecvError::Disconnected)) => ui.found.scan = None,
        _ => {}
    }
    let adopted = match ui
        .found
        .adopting
        .as_ref()
        .map(|(receiver, _)| receiver.try_recv())
    {
        Some(Ok(result)) => Some(result),
        Some(Err(TryRecvError::Disconnected)) => Some(Err(crate::desktop::i18n::tr(
            "Checking the model stopped.",
        )
        .into())),
        _ => None,
    };
    if let Some(result) = adopted {
        let onboarding = ui
            .found
            .adopting
            .take()
            .is_some_and(|(_, onboarding)| onboarding);
        changed = true;
        match result {
            Ok(id) => {
                let model = TranscriptionModelId::Custom(id);
                let language = crate::transcription::models::definition(model)
                    .language_for(&ui.settings.transcription.language)
                    .to_owned();
                if onboarding {
                    ui.onboarding_choice = Some(TranscriptionSelection { model, language });
                    super::onboarding::input(ui, Input::Grant(SetupItem::Model));
                } else {
                    ui.send(Request::ChooseModel { model, language });
                }
            }
            Err(error) => ui.found.error = Some(error),
        }
    }
    changed
}

/// Uses `found`, a file that isn't a catalog model, where it is: its full
/// SHA-256 once on a worker (it must match the hub's blob name), then the
/// model is prepared, and its load is the final test.
pub(super) fn adopt(ui: &mut Ui, found: Found, onboarding: bool) {
    if !ui.live() || ui.found.adopting.is_some() {
        return;
    }
    ui.found.error = None;
    let (sender, receiver) = channel();
    let wake = wake(ui.modes.waker.clone());
    let started = std::thread::Builder::new()
        .name("hex-found-adopt".into())
        .spawn(move || {
            let result = custom_models::adopt_in_place(
                &found.path,
                found.blob_sha256.as_deref(),
                &found.source(),
                &AtomicBool::new(false),
            )
            .map_err(|error| format!("{error:#}"));
            let _ = sender.send(result);
            wake();
        });
    match started {
        Ok(_) => ui.found.adopting = Some((receiver, onboarding)),
        Err(error) => ui.found.error = Some(error.to_string()),
    }
}

/// Two found files for scenes: Parakeet v3's pinned file (an exact catalog
/// match) and a Whisper Small Q8_0 Hex hasn't tested.
pub(super) fn fixture() -> Vec<Found> {
    let v3 = crate::transcription::models::definition(TranscriptionModelId::ParakeetV3);
    let crate::transcription::models::ModelRuntime::Gguf(artifact) = v3.runtime else {
        unreachable!("Parakeet v3 is a GGUF model");
    };
    let hub = std::path::Path::new("/home/you/.cache/huggingface/hub");
    vec![
        Found {
            path: hub.join(
                "models--handy-computer--parakeet-tdt-0.6b-v3-gguf/snapshots/85ac09ea12fc4b1112fa76810059364bc6adc9de/parakeet-tdt-0.6b-v3-Q8_0.gguf",
            ),
            repository: artifact.repository.into(),
            revision: artifact.revision.into(),
            file_name: artifact.filename.into(),
            bytes: artifact.bytes,
            blob_sha256: Some(artifact.sha256.into()),
            catalog: Some(v3.id),
            name: "Parakeet TDT 0.6B v3".into(),
            architecture: "parakeet".into(),
            variant: artifact.variant.into(),
            languages: v3.languages.iter().map(|code| (*code).to_owned()).collect(),
            compression: "Q8_0",
            tensor_bytes: artifact.bytes - 2_000_000,
            too_big: false,
        },
        Found {
            path: hub.join(
                "models--handy-computer--whisper-small-gguf/snapshots/3b8e0c4f9d2a7e61c5f0a9b8d7c6e5f4a3b2c1d0/whisper-small-Q8_0.gguf",
            ),
            repository: "handy-computer/whisper-small-gguf".into(),
            revision: "3b8e0c4f9d2a7e61c5f0a9b8d7c6e5f4a3b2c1d0".into(),
            file_name: "whisper-small-Q8_0.gguf".into(),
            bytes: 264_000_000,
            blob_sha256: Some("9f".repeat(32)),
            catalog: None,
            name: "Whisper Small".into(),
            architecture: "whisper".into(),
            variant: "whisper-small".into(),
            languages: vec!["en".into(), "de".into(), "fr".into()],
            compression: "Q8_0",
            tensor_bytes: 262_000_000,
            too_big: false,
        },
    ]
}

/// The picker's "Found on this computer" section: compatible files that
/// aren't catalog models and aren't in use yet, each labelled from its
/// header and path, marked untested, with Use.
pub(super) fn section(ui: &Ui, p: super::controls::Paint) -> Option<rune::El> {
    use rune::{col, row, text};
    use rune_kit::Badge;
    let found: Vec<(usize, &Found)> = ui
        .found_models
        .iter()
        .enumerate()
        .filter(|(_, found)| found.catalog.is_none() && !custom_models::uses_path(&found.path))
        .collect();
    if found.is_empty() {
        return None;
    }
    let c = &p.k.c;
    let mut section = col().key("found-models").gap(8.0).child(
        text(crate::desktop::i18n::tr("Found on this computer"))
            .key("found-heading")
            .font(p.k.ty.body_medium)
            .color(p.text()),
    );
    for (index, model) in found {
        let detail = format!(
            "{} · {} · {}",
            model.compression,
            model.source(),
            crate::desktop::i18n::tr_fill("{} languages", &model.languages.len().to_string())
        );
        let use_button = super::controls::button(
            p,
            "use-found",
            if ui.found.adopting() {
                crate::desktop::i18n::tr("Checking…")
            } else {
                crate::desktop::i18n::tr("Use")
            },
            super::controls::ButtonKind::Secondary,
            move |ui: &mut Ui, _| {
                if let Some(found) = ui.found_models.get(index).cloned() {
                    adopt(ui, found, false);
                }
            },
        );
        let mut card = col()
            .key(format!("found{index}"))
            .px(16.0)
            .py(12.0)
            .gap(6.0)
            .radius(p.k.radius.lg)
            .fill(c.bg_base)
            .border(p.hair, c.border_weak)
            .child(
                row()
                    .key("top")
                    .gap(12.0)
                    .center_y()
                    .child(
                        row()
                            .key("title")
                            .gap(8.0)
                            .center_y()
                            .grow(1.0)
                            .shrink()
                            .child(
                                text(model.name.clone())
                                    .key("name")
                                    .font(p.k.ty.body_medium)
                                    .color(p.text()),
                            )
                            .child(
                                Badge::new(crate::desktop::i18n::tr("Not tested by Hex"))
                                    .build(p.k),
                            ),
                    )
                    .child(use_button),
            )
            .child(
                text(detail)
                    .key("metadata")
                    .font(p.k.ty.small)
                    .color(p.muted())
                    .wrap(),
            );
        if model.too_big {
            card = card.child(
                text(crate::desktop::i18n::tr(
                    "It may be too big for this computer's GPU memory.",
                ))
                .key("too-big")
                .font(p.k.ty.small)
                .color(p.warning())
                .wrap(),
            );
        }
        section = section.child(card);
    }
    Some(
        section.child(
            text(crate::desktop::i18n::tr(
                "Compatible doesn't mean accurate: a model that loads can still transcribe poorly.",
            ))
            .key("found-note")
            .font(p.k.ty.small)
            .color(p.muted())
            .wrap(),
        ),
    )
}

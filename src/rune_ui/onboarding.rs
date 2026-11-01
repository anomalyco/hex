//! The first run's state machine in the window: [`Tour`] feeds
//! `onboarding::Flow` the permissions and the model download, starts the
//! download, asks for permissions, relaunches when macOS needs it, watches
//! the practice dictation, and records completion. The first run the person
//! sees is the onboarding story over it ([`onboarding_kit::live`]), the one
//! onboarding on every OS.
//!
//! [`onboarding_kit::live`]: super::onboarding_kit::live

use std::time::{Duration, Instant};

use super::Request;
use super::Ui;
use super::platform::{PlatformRequest, PlatformState};
use crate::app_settings::AppSettings;
use crate::desktop::onboarding::{
    Effect, Flow, Input, ItemStatus, ModelProgress, PermissionState, SetupItem, Step,
};

/// How long the ready check shows before setup moves on.
const ADVANCE_BEAT: Duration = Duration::from_millis(700);

pub(super) struct Tour {
    pub(super) flow: Flow,
    advance_at: Option<Instant>,
    completed: bool,
}

impl Tour {
    fn new(flow: Flow, completed: bool) -> Self {
        Self {
            flow,
            advance_at: None,
            completed,
        }
    }

    /// The tour from Welcome, as on a new install: Run setup again.
    pub(super) fn replay(settings: &AppSettings, platform: &PlatformState) -> Tour {
        let installed = super::picker::Picker::default().selected_installed(settings);
        Tour::new(
            Flow::new(
                Step::Welcome,
                platform.setup_status(installed),
                platform.caps.permissions,
                platform.caps.practice_dictation,
            ),
            false,
        )
    }

    /// The tour when this run still needs it.
    pub(super) fn start(settings: &AppSettings, platform: &PlatformState) -> Option<Tour> {
        let installed = super::picker::Picker::default().selected_installed(settings);
        let status = platform.setup_status(installed);
        let completed = crate::desktop::onboarding::completion_recorded();
        let resume = crate::desktop::onboarding::take_resume();
        let saved = (!completed)
            .then(crate::desktop::onboarding::saved_step)
            .flatten();
        (resume.is_some() || (!completed && (!status.ready() || saved.is_some()))).then(|| {
            Tour::new(
                Flow::new(
                    resume.or(saved).unwrap_or(Step::Welcome),
                    status,
                    platform.caps.permissions,
                    platform.caps.practice_dictation,
                ),
                completed,
            )
        })
    }
}

pub(super) fn progress(ui: &Ui) -> (bool, ModelProgress) {
    let installed = ui.picker.selected_installed(&ui.settings);
    let selected = crate::transcription::models::validate(&ui.settings.transcription).ok();
    let transcription = ui.view.as_ref().map(|view| &view.transcription);
    let downloading = selected.and_then(|model| {
        (transcription.and_then(|t| t.preparing) == Some(model.id)).then(|| {
            model.download_bytes().map_or(0.0, |bytes| {
                (transcription.map_or(0, |t| t.downloaded_bytes) as f32 / bytes as f32)
                    .clamp(0.0, 1.0)
            })
        })
    });
    (
        installed,
        ModelProgress {
            installed,
            downloading,
            failed: downloading.is_none() && transcription.is_some_and(|t| t.error.is_some()),
        },
    )
}

/// Feeds the flow fresh permission and model state; true when the tour
/// changed.
pub(super) fn poll(ui: &mut Ui) -> bool {
    // Onboarding looks for models already on this computer once, and holds
    // its own download until the scan answers; with one found, the model
    // list (`found_choice`) starts the row the person picks.
    if ui.tour.is_some() {
        super::found::scan_for_setup(ui);
        // Only until setup: past it, the recommended model downloads.
        let held = (ui.found.scanning() || !ui.found.models.is_empty())
            && ui
                .tour
                .as_ref()
                .is_some_and(|tour| matches!(tour.flow.step(), Step::Welcome | Step::Setup));
        let choosing = super::found_choice::offered(
            ui,
            ui.tour
                .as_ref()
                .map_or(ItemStatus::Todo, |tour| tour.flow.view().model),
        );
        if choosing && ui.found.recommended.is_none() {
            ui.found.recommended = Some(ui.settings.transcription.clone());
        }
        if let Some(tour) = &mut ui.tour {
            tour.flow.hold_download(held);
        }
    }
    let (installed, model) = progress(ui);
    let status = ui.platform.setup_status(installed);
    let accessibility_stale = ui
        .platform
        .stale(crate::desktop::onboarding::PermissionKind::Accessibility);
    let live = ui.live();
    let Some(tour) = &mut ui.tour else {
        return false;
    };
    let before = tour.flow.view();
    let step = before.step;
    let mut effect = tour.flow.on(Input::Status(status, model));
    tour.flow.set_accessibility_stale(accessibility_stale);
    if tour
        .advance_at
        .is_some_and(|at| at.elapsed() >= ADVANCE_BEAT)
    {
        tour.advance_at = None;
        if tour.flow.step() == Step::Setup {
            effect = tour.flow.on(Input::Continue);
        }
    }
    if status.ready() && !tour.completed {
        tour.completed = true;
        if live && let Err(error) = crate::desktop::onboarding::record_completion() {
            tracing::warn!(%error, "could not record onboarding completion");
        }
    }
    let practicing = tour.flow.step() == Step::TryIt;
    apply(ui, effect);
    let heard = ui.host.tour_inputs(practicing);
    let changed = !heard.is_empty();
    for heard in heard {
        input(ui, heard);
    }
    remember_step(ui, step);
    let after = ui.tour.as_ref().map(|tour| tour.flow.view());
    changed || after.as_ref() != Some(&before)
}

pub(super) fn input(ui: &mut Ui, input: Input) {
    let Some(tour) = &mut ui.tour else {
        return;
    };
    let step = tour.flow.step();
    let effect = tour.flow.on(input);
    apply(ui, effect);
    remember_step(ui, step);
}

/// Saves a new step so a run quit part-way resumes there. The story sounds
/// the success chime itself, on Hex is ready.
fn remember_step(ui: &Ui, before: Step) {
    if let Some(tour) = &ui.tour
        && tour.flow.step() != before
        && ui.live()
    {
        crate::desktop::onboarding::record_step(tour.flow.step());
    }
}

fn apply(ui: &mut Ui, effect: Effect) {
    match effect {
        Effect::DownloadModel => {
            // The model onboarding's choice picked, else the recommended one.
            let selection = ui
                .found
                .choice
                .take()
                .unwrap_or_else(|| ui.settings.transcription.clone());
            if crate::transcription::models::validate(&selection).is_ok() {
                ui.send(Request::ChooseModel {
                    model: selection.model,
                    language: selection.language,
                });
            }
        }
        Effect::AdvanceSoon => {
            if let Some(tour) = &mut ui.tour {
                tour.advance_at = Some(Instant::now());
            }
        }
        Effect::RequestMicrophone => ui.ask(PlatformRequest::Permission(
            crate::desktop::onboarding::PermissionAction::RequestMicrophone,
        )),
        Effect::ShowGuide(kind) => ui.ask(PlatformRequest::Permission(
            crate::desktop::onboarding::PermissionAction::ShowGuide(kind),
        )),
        Effect::ResetPermission(kind) => ui.ask(PlatformRequest::Permission(
            crate::desktop::onboarding::PermissionAction::ResetAndPrompt(kind),
        )),
        Effect::Relaunch => ui.ask(PlatformRequest::Relaunch),
        Effect::Finish => {
            ui.tour = None;
            ui.first_run = None;
        }
        Effect::None => {}
    }
}

/// A first run at `step`, with the selected model not yet installed.
pub(super) fn fixture(ui: &mut Ui, step: Step, downloading: bool) {
    ui.picker.installed = Some(Vec::new());
    let status = ui.platform.setup_status(false);
    let mut flow = Flow::new(
        Step::Welcome,
        status,
        ui.platform.caps.permissions,
        ui.platform.caps.practice_dictation,
    );
    flow.hold_download(!ui.found.models.is_empty());
    if step != Step::Welcome {
        flow.on(Input::Continue);
    }
    ui.tour = Some(Tour::new(flow, false));
    if downloading {
        input(ui, Input::Grant(SetupItem::Model));
        set_downloaded(ui, 62);
        poll(ui);
    }
}

pub(super) fn set_downloaded(ui: &mut Ui, percent: u64) {
    if let Some(view) = &mut ui.view {
        let model = ui.settings.transcription.model;
        let bytes = crate::transcription::models::definition(model)
            .download_bytes()
            .unwrap_or(0);
        view.transcription.preparing = Some(model);
        view.transcription.downloaded_bytes = bytes * percent / 100;
    }
}

/// The macOS tour's states, walked as a person would.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MacStep {
    Setup,
    SetupStale,
    SetupDenied,
    /// Every permission on, Accessibility waiting for a restart.
    SetupRestart,
    TryIt,
    /// The shortcut listener failed after setup.
    TryItReopen,
    Done,
}

/// Walks a fresh macOS first run to `state`. `ui.platform` must carry
/// macOS capabilities.
pub(super) fn mac_fixture(ui: &mut Ui, state: MacStep) {
    use PermissionState::{NeedsRequest, NeedsRestart, NeedsSettings, Ready};
    let fresh = crate::desktop::onboarding::SetupStatus {
        microphone: NeedsRequest,
        accessibility: NeedsSettings,
        transcription_model: false,
    };
    let installed = matches!(state, MacStep::TryIt | MacStep::TryItReopen | MacStep::Done);
    let permissions = match state {
        MacStep::Setup => fresh,
        MacStep::SetupStale => crate::desktop::onboarding::SetupStatus {
            microphone: Ready,
            ..fresh
        },
        MacStep::SetupDenied => crate::desktop::onboarding::SetupStatus {
            microphone: NeedsSettings,
            ..fresh
        },
        MacStep::SetupRestart => crate::desktop::onboarding::SetupStatus {
            microphone: Ready,
            accessibility: NeedsRestart,
            ..fresh
        },
        _ => crate::desktop::onboarding::SetupStatus {
            microphone: Ready,
            accessibility: Ready,
            transcription_model: true,
        },
    };
    ui.platform.permissions = fresh;
    ui.picker.installed = Some(Vec::new());
    ui.tour = Some(Tour::new(
        Flow::new(Step::Welcome, fresh, true, true),
        false,
    ));
    // The tour starts the download as soon as it sees no model.
    poll(ui);
    input(ui, Input::Continue);
    if state == MacStep::SetupStale {
        for item in [SetupItem::Microphone, SetupItem::Accessibility] {
            input(ui, Input::Grant(item));
        }
        ui.platform
            .note_stale(crate::desktop::onboarding::PermissionKind::Accessibility);
    }
    ui.platform.permissions = permissions;
    if installed {
        ui.picker.installed = Some(vec![ui.settings.transcription.model]);
        if let Some(view) = &mut ui.view {
            view.transcription.preparing = None;
        }
    } else {
        set_downloaded(ui, 31);
    }
    poll(ui);
    if installed {
        input(ui, Input::Continue);
    }
    if let Some(tour) = &mut ui.tour {
        tour.advance_at = None;
    }
    if state == MacStep::TryItReopen {
        input(
            ui,
            Input::ListenerFailed {
                relaunch_allowed: false,
            },
        );
    }
    if state == MacStep::Done {
        input(ui, Input::Skip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_stale_fixture_marks_accessibility_stale_and_dispatches_reset() {
        let mut ui = crate::rune_ui::mac_fixture();
        mac_fixture(&mut ui, MacStep::SetupStale);
        let rows = ui.tour.as_ref().unwrap().flow.view().rows;
        assert!(
            rows.iter()
                .any(|row| row.item == SetupItem::Accessibility && row.status == ItemStatus::Stale)
        );
        input(&mut ui, Input::Grant(SetupItem::Accessibility));
        assert!(
            !ui.platform
                .stale(crate::desktop::onboarding::PermissionKind::Accessibility)
        );
        match &ui.host {
            crate::rune_ui::platform::PlatformHost::Fixture(sent) => {
                assert_eq!(
                    sent.last(),
                    Some(&PlatformRequest::Permission(
                        crate::desktop::onboarding::PermissionAction::ResetAndPrompt(
                            crate::desktop::onboarding::PermissionKind::Accessibility
                        )
                    ))
                );
            }
            _ => panic!("expected fixture host"),
        }
    }
}

//! Hex's first run on the onboarding story, replayed by Labs' Run setup
//! again and `hex onboarding reset`; a new install still gets the tour.
//! The window's tour ([`onboarding`](super::super::onboarding)) stays the state machine underneath: it starts the model download, asks
//! for the permissions, relaunches when macOS needs it, watches the practice
//! dictation, and records completion. This module shows the story's pages
//! over the window and feeds them that real state each frame.

use std::sync::OnceLock;
use std::time::Instant;

use rune::{El, TextEdit, Transition, Tx, text};

use super::super::controls::Paint;
use super::super::{Request, Ui, onboarding};
use super::fixture::{Language, Speech};
use super::pill::set_other_label;
use super::stories::{FRAME, Flow, Onboard, advance_to};
use crate::desktop::onboarding::{
    Input, ItemStatus, Practice, SetupItem, SetupStatus, Step as TourStep,
};
use crate::transcription::models::{
    self as transcription_models, TranscriptionModelId, TranscriptionSelection,
};

/// How long both permission checks show before the page moves on.
const GRANTED_BEAT: f64 = 0.7;
/// How often the first run looks at the model on disk and the permissions.
const CHECK_EVERY: f64 = 0.25;

/// What the real first run reports to the story's pages.
pub(crate) struct Live {
    /// Hex's own dictation records.
    pub(crate) listening: bool,
    /// The model download stopped with an error.
    pub(crate) failed: bool,
    pub(crate) microphone: ItemStatus,
    pub(crate) accessibility: ItemStatus,
    /// Every permission is on and one applies only after Hex restarts.
    pub(crate) restart: bool,
    pub(crate) practice: Practice,
    /// The model and language this run downloads.
    choice: TranscriptionSelection,
    granted_at: Option<f64>,
    checked: Option<Checked>,
    /// Dictation streams send their live words to the practice field.
    _live_words: crate::dictation::live_text::Demand,
}

/// The last look at the disk and macOS.
#[derive(Clone, Copy)]
struct Checked {
    at: f64,
    installed: bool,
    status: SetupStatus,
}

impl Live {
    pub(crate) fn granted_for(&self, now: f64) -> bool {
        self.granted_at.is_some_and(|at| now - at >= GRANTED_BEAT)
    }
}

/// Seconds on the first run's own clock.
fn clock() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64() + 1.0
}

fn english(selection: &TranscriptionSelection) -> bool {
    selection.model == TranscriptionModelId::ParakeetUnifiedEnglish
}

/// The selection the language capsule switches to.
fn other_selection(current: &TranscriptionSelection) -> TranscriptionSelection {
    if english(current) {
        let v3 = transcription_models::definition(TranscriptionModelId::ParakeetV3);
        let preferred = crate::desktop::i18n::preferred_language();
        let language = if preferred != "en" && v3.supports_language(&preferred) {
            preferred
        } else {
            "fr".to_owned()
        };
        TranscriptionSelection {
            model: TranscriptionModelId::ParakeetV3,
            language,
        }
    } else {
        TranscriptionSelection {
            model: TranscriptionModelId::ParakeetUnifiedEnglish,
            language: "en".into(),
        }
    }
}

fn installed(ui: &Ui, selection: &TranscriptionSelection) -> bool {
    transcription_models::validate(selection)
        .is_ok_and(|model| ui.picker.installed(model, &selection.language))
}

/// Builds the first run over the tour Run setup again just started.
fn start(ui: &mut Ui) {
    let Some(tour_step) = ui.tour.as_ref().map(|tour| tour.flow.step()) else {
        return;
    };
    let choice = ui.settings.transcription.clone();
    let wants = if english(&choice) {
        Language::English
    } else {
        Language::Other
    };
    let other = if english(&choice) {
        other_selection(&choice)
    } else {
        choice.clone()
    };
    set_other_label(transcription_models::language_name(&other.language));
    let had_model = installed(ui, &choice);
    let status = ui.platform.setup_status(had_model);
    let granted = status.microphone == crate::desktop::onboarding::PermissionState::Ready
        && status.accessibility == crate::desktop::onboarding::PermissionState::Ready;

    let mut s = Onboard::new(Flow::FirstRun);
    s.speech = Speech::AfterDownload;
    s.wants = wants;
    s.caps = ui.settings.dictation_hotkey.keycaps();
    s.had_model = had_model;
    s.granted = granted;
    s.live = Some(Live {
        listening: false,
        failed: false,
        microphone: ItemStatus::Todo,
        accessibility: ItemStatus::Todo,
        restart: false,
        practice: Practice::Waiting,
        choice,
        granted_at: None,
        checked: None,
        _live_words: crate::dictation::live_text::demand(),
    });
    let now = clock();
    s.now = now;
    match tour_step {
        TourStep::Welcome => s.begin_live(now),
        TourStep::Setup => s.jump_to_permissions(),
        TourStep::TryIt if had_model => s.jump_to_try(),
        TourStep::TryIt => s.jump_to_waiting(),
        TourStep::Done => s.jump_to_ready(),
    }
    ui.first_run = Some(s);
    // The tour waits at setup, where it asks for permissions, while the
    // story plays; with everything granted it goes on to the practice.
    if tour_step == TourStep::Welcome {
        onboarding::input(ui, Input::Continue);
    }
}

/// "Run setup again" and `hex onboarding reset`: forgets that onboarding
/// finished and opens it now. Settings, history, models, and permissions
/// stay as they are, so the run finds them ready and passes their steps.
pub(crate) fn restart(ui: &mut Ui, tx: &mut Tx) -> Result<(), String> {
    if ui.live() {
        crate::desktop::onboarding::reset().map_err(|error| format!("{error:#}"))?;
    }
    ui.select_pane(super::super::Pane::General, tx);
    ui.first_run = None;
    ui.tour = Some(onboarding::Tour::replay(&ui.settings, &ui.platform));
    start(ui);
    tx.settle();
    if ui.first_run.is_none() {
        return Err("Hex could not open onboarding".into());
    }
    Ok(())
}

/// Feeds the first run fresh state and moves it on; true while it shows.
pub(in crate::rune_ui) fn poll(ui: &mut Ui, tx: &mut Tx) -> bool {
    if ui.first_run.is_none() {
        return false;
    }
    let now = clock();
    let view = ui.tour.as_ref().map(|tour| tour.flow.view());
    let caps = ui.settings.dictation_hotkey.keycaps();
    let Some(choice) = ui
        .first_run
        .as_ref()
        .and_then(|s| s.live.as_ref())
        .map(|live| live.choice.clone())
    else {
        return false;
    };
    let model = transcription_models::validate(&choice).ok();
    let (preparing, downloaded_bytes, errored) =
        ui.view.as_ref().map_or((false, 0, false), |view| {
            let t = &view.transcription;
            (
                model.is_some_and(|m| t.preparing == Some(m.id)),
                t.downloaded_bytes,
                t.error.is_some(),
            )
        });
    // The disk and macOS checks run a few times a second, not every frame.
    let checked = ui
        .first_run
        .as_ref()
        .and_then(|s| s.live.as_ref())
        .and_then(|live| live.checked)
        .filter(|checked| now - checked.at < CHECK_EVERY);
    let checked = match checked {
        Some(checked) => checked,
        None => {
            let installed = installed(ui, &choice);
            Checked {
                at: now,
                installed,
                status: ui.platform.setup_status(installed),
            }
        }
    };
    let (choice_installed, status) = (checked.installed, checked.status);
    // Setup that is already ready never moves the tour past it by itself,
    // and the tour watches the practice dictation only once it has.
    if status.ready()
        && ui
            .tour
            .as_ref()
            .is_some_and(|tour| tour.flow.step() == TourStep::Setup)
    {
        onboarding::input(ui, Input::Continue);
    }
    let asks_permissions = ui.platform.caps.permissions;
    let Some(s) = ui.first_run.as_mut() else {
        return false;
    };
    let Some(live) = s.live.as_mut() else {
        return false;
    };
    live.checked = Some(checked);
    // Linux and Windows ask for no permissions: both rows are ready.
    if !asks_permissions {
        live.microphone = ItemStatus::Ready;
        live.accessibility = ItemStatus::Ready;
    }

    // Permissions, as the tour's setup rows show them.
    if let Some(view) = &view {
        for row in &view.rows {
            match row.item {
                SetupItem::Microphone => live.microphone = row.status,
                SetupItem::Accessibility => live.accessibility = row.status,
                _ => {}
            }
        }
        live.restart = view.restart;
        live.practice = view.practice;
    }
    let granted = status.microphone == crate::desktop::onboarding::PermissionState::Ready
        && status.accessibility == crate::desktop::onboarding::PermissionState::Ready;
    if granted {
        live.granted_at.get_or_insert(now);
    } else {
        live.granted_at = None;
    }
    live.listening = live.practice == Practice::Listening;

    // The model download.
    live.failed = !choice_installed && !preparing && errored;
    s.granted = granted;
    s.caps = caps;
    if let Some(download) = s.download.as_mut() {
        if let Some(total) = model.and_then(|m| m.download_bytes()) {
            download.total = total;
        }
        if choice_installed {
            download.downloaded = download.total;
        } else if preparing {
            download.downloaded = downloaded_bytes.min(download.total.saturating_sub(1));
        }
    }

    // Hex's state arrives between frames, so the first run keeps polling.
    advance_to(s, now, tx);
    tx.wake_after(FRAME);
    true
}

/// The first run over the whole window, while it shows.
pub(in crate::rune_ui) fn overlay(ui: &Ui, p: Paint, reduced: bool) -> Option<El> {
    let s = ui.first_run.as_ref()?;
    s.reduced.set(reduced);
    Some(
        super::flows::view(p, s)
            .key("first-run")
            .role("dialog")
            .absolute(0.0, 0.0)
            .on_press(|_: &mut Ui| {}),
    )
}

/// A story handler run on the window's first run.
pub(super) fn on_ui(f: fn(&mut Onboard)) -> impl Fn(&mut Ui) + 'static {
    move |ui: &mut Ui| {
        if let Some(s) = ui.first_run.as_mut() {
            f(s);
        }
    }
}

pub(super) fn press_continue(ui: &mut Ui) {
    if let Some(s) = ui.first_run.as_mut() {
        s.press_continue();
    }
}

/// "Start using Hex": closes the first run and the tour under it.
pub(super) fn finish(ui: &mut Ui) {
    let installed = ui.picker.selected_installed(&ui.settings);
    if ui.live()
        && ui.platform.setup_status(installed).ready()
        && !crate::desktop::onboarding::completion_recorded()
        && let Err(error) = crate::desktop::onboarding::record_completion()
    {
        tracing::warn!(%error, "could not record onboarding completion");
    }
    ui.first_run = None;
    ui.tour = None;
}

fn choose(ui: &mut Ui, selection: &TranscriptionSelection) {
    if transcription_models::validate(selection).is_ok() {
        ui.send(Request::ChooseModel {
            model: selection.model,
            language: selection.language.clone(),
        });
    }
}

pub(super) fn cycle_language(ui: &mut Ui) {
    let Some(s) = ui.first_run.as_mut() else {
        return;
    };
    s.cycle_language();
    let Some(live) = s.live.as_mut() else {
        return;
    };
    live.choice = other_selection(&live.choice);
    live.failed = false;
    live.checked = None;
    let choice = live.choice.clone();
    choose(ui, &choice);
}

pub(super) fn retry_download(ui: &mut Ui) {
    let Some(live) = ui.first_run.as_mut().and_then(|s| s.live.as_mut()) else {
        return;
    };
    live.failed = false;
    let choice = live.choice.clone();
    choose(ui, &choice);
}

/// Allow on a permission row.
pub(super) fn allow(item: SetupItem) -> impl Fn(&mut Ui) + 'static {
    move |ui: &mut Ui| onboarding::input(ui, Input::Grant(item))
}

/// "Restart Hex" once macOS applies a grant only after a restart.
pub(super) fn restart_hex(ui: &mut Ui) {
    onboarding::input(ui, Input::Restart);
}

pub(super) fn practice(ui: &mut Ui) -> &mut TextEdit {
    match ui.first_run.as_mut() {
        Some(s) => &mut s.practice,
        // The field shows only with the first run.
        None => Box::leak(Box::default()),
    }
}

pub(super) fn practice_changed(ui: &mut Ui, tx: &mut Tx) {
    if let Some(s) = ui.first_run.as_mut() {
        s.on_practice_change(tx);
    }
}

/// What the practice dictation's last try needs said.
pub(super) fn hint_copy(live: &Live) -> Option<&'static str> {
    Some(match live.practice {
        Practice::TooShort => "Hold the shortcut a little longer while you speak.",
        Practice::NothingHeard => "Hex didn't hear any words. Try again a little louder.",
        Practice::Failed => "That didn't work. Try again.",
        Practice::Transcribing => "Typing what you said…",
        _ => return None,
    })
}

/// [`hint_copy`] under the practice field.
pub(super) fn hint(p: Paint, live: &Live) -> Option<El> {
    let copy = hint_copy(live)?;
    Some(
        text(copy)
            .key("practice-hint")
            .font(p.k.ty.small)
            .color(p.muted())
            .transition(Transition::opacity()),
    )
}
